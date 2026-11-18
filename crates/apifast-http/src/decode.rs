//! Response bodies decompressed by hand (docs/07, "Respuesta"), to know
//! both sizes: what came over the wire and what it decodes to. Each chunk
//! goes through a decoder that writes into the body store as it arrives,
//! so a body is never whole in memory.

use std::io::{self, Write};

use apifast_app::{BodyWriter, StoreError};

/// What `Accept-Encoding` offers when the request says nothing, as reqwest
/// did.
pub(crate) const ACCEPTED: &str = "zstd,gzip,deflate,br";

/// An encoding apifast decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Gzip,
    /// HTTP's `deflate`: zlib.
    Deflate,
    Brotli,
    Zstd,
}

impl Encoding {
    /// The encoding of a `Content-Encoding` value, if it is one apifast
    /// decodes. `identity`, several encodings or an unknown one give `None`:
    /// the body is kept as it came.
    pub(crate) fn of(content_encoding: &str) -> Option<Self> {
        match content_encoding.trim().to_ascii_lowercase().as_str() {
            "gzip" | "x-gzip" => Some(Self::Gzip),
            "deflate" => Some(Self::Deflate),
            "br" => Some(Self::Brotli),
            "zstd" => Some(Self::Zstd),
            _ => None,
        }
    }

    /// As `Content-Encoding` writes it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::Deflate => "deflate",
            Self::Brotli => "br",
            Self::Zstd => "zstd",
        }
    }
}

/// Where decoded bytes go: the body store, counting them. A store error is
/// kept to report it as such, not as a decoding error.
struct Sink<'w, W: BodyWriter> {
    writer: &'w mut W,
    bytes: u64,
    error: Option<StoreError>,
}

impl<W: BodyWriter> Write for Sink<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Err(e) = self.writer.write(buf) {
            self.error = Some(e);
            return Err(io::Error::other("the body store failed"));
        }
        self.bytes += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A decoder writing into the sink.
enum Inner<'w, W: BodyWriter> {
    Gzip(flate2::write::GzDecoder<Sink<'w, W>>),
    Deflate(flate2::write::ZlibDecoder<Sink<'w, W>>),
    Brotli(Box<brotli_decompressor::DecompressorWriter<Sink<'w, W>>>),
    Zstd(zstd::stream::write::Decoder<'static, Sink<'w, W>>),
}

/// Why a body could not be stored.
#[derive(Debug)]
pub(crate) enum DecodeError {
    /// The body is not valid in its encoding.
    Corrupt(Encoding, String),
    /// The store failed.
    Store(StoreError),
}

/// Decodes a body into a [`BodyWriter`], chunk by chunk.
pub(crate) struct Decoder<'w, W: BodyWriter> {
    encoding: Encoding,
    inner: Inner<'w, W>,
    wire: u64,
}

impl<'w, W: BodyWriter> Decoder<'w, W> {
    /// A decoder of `encoding` into `writer`.
    ///
    /// # Errors
    ///
    /// If zstd cannot make its context (only without memory).
    pub(crate) fn new(encoding: Encoding, writer: &'w mut W) -> Result<Self, DecodeError> {
        let sink = Sink {
            writer,
            bytes: 0,
            error: None,
        };
        let inner = match encoding {
            Encoding::Gzip => Inner::Gzip(flate2::write::GzDecoder::new(sink)),
            Encoding::Deflate => Inner::Deflate(flate2::write::ZlibDecoder::new(sink)),
            Encoding::Brotli => Inner::Brotli(Box::new(
                brotli_decompressor::DecompressorWriter::new(sink, 8 * 1024),
            )),
            Encoding::Zstd => Inner::Zstd(
                zstd::stream::write::Decoder::new(sink)
                    .map_err(|e| DecodeError::Corrupt(encoding, e.to_string()))?,
            ),
        };
        Ok(Self {
            encoding,
            inner,
            wire: 0,
        })
    }

    /// Decodes `chunk`, as it came over the wire.
    ///
    /// # Errors
    ///
    /// If the chunk is not valid, or the store fails.
    pub(crate) fn write(&mut self, chunk: &[u8]) -> Result<(), DecodeError> {
        self.wire += chunk.len() as u64;
        let written = match &mut self.inner {
            Inner::Gzip(d) => d.write_all(chunk),
            Inner::Deflate(d) => d.write_all(chunk),
            Inner::Brotli(d) => d.write_all(chunk),
            Inner::Zstd(d) => d.write_all(chunk),
        };
        written.map_err(|e| self.error(&e))
    }

    /// Ends the body: returns the bytes that came over the wire and those
    /// it decoded to.
    ///
    /// # Errors
    ///
    /// If the body ended before its encoding says, or the store fails.
    pub(crate) fn finish(mut self) -> Result<(u64, u64), DecodeError> {
        // A body with no bytes (HEAD, 204, 304) has nothing to decode.
        if self.wire == 0 {
            return Ok((0, 0));
        }
        // Write out what is pending, still able to tell a store error.
        let flushed = match &mut self.inner {
            Inner::Gzip(d) => d.try_finish(),
            Inner::Deflate(d) => d.try_finish(),
            Inner::Brotli(d) => d.flush(),
            Inner::Zstd(d) => d.flush(),
        };
        flushed.map_err(|e| self.error(&e))?;
        let (encoding, wire) = (self.encoding, self.wire);
        let bytes = match self.inner {
            Inner::Gzip(mut d) => d.get_mut().bytes,
            Inner::Deflate(mut d) => d.get_mut().bytes,
            // A stream that did not reach its end gives the writer back as
            // an error.
            Inner::Brotli(d) => {
                (*d).into_inner()
                    .map_err(|_| DecodeError::Corrupt(encoding, "the body ends too soon".into()))?
                    .bytes
            }
            Inner::Zstd(mut d) => d.get_mut().bytes,
        };
        Ok((wire, bytes))
    }

    fn error(&mut self, error: &io::Error) -> DecodeError {
        let sink = match &mut self.inner {
            Inner::Gzip(d) => d.get_mut(),
            Inner::Deflate(d) => d.get_mut(),
            Inner::Brotli(d) => d.get_mut(),
            Inner::Zstd(d) => d.get_mut(),
        };
        match sink.error.take() {
            Some(store) => DecodeError::Store(store),
            None => DecodeError::Corrupt(self.encoding, error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apifast_app::{ResponseId, StoredBody};

    #[derive(Default)]
    struct Collect(Vec<u8>);

    impl BodyWriter for Collect {
        fn write(&mut self, chunk: &[u8]) -> Result<(), StoreError> {
            self.0.extend_from_slice(chunk);
            Ok(())
        }
        fn finish(self) -> Result<StoredBody, StoreError> {
            Ok(StoredBody {
                id: ResponseId(String::new()),
                size: 0,
            })
        }
    }

    fn text() -> Vec<u8> {
        "hello, compressed world. ".repeat(200).into_bytes()
    }

    fn encoded(encoding: Encoding) -> Vec<u8> {
        let plain = text();
        match encoding {
            Encoding::Gzip => {
                let mut e =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(&plain).unwrap();
                e.finish().unwrap()
            }
            Encoding::Deflate => {
                let mut e =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(&plain).unwrap();
                e.finish().unwrap()
            }
            Encoding::Zstd => zstd::stream::encode_all(plain.as_slice(), 3).unwrap(),
            Encoding::Brotli => {
                let mut out = Vec::new();
                let mut e = brotli::CompressorWriter::new(&mut out, 4096, 5, 22);
                e.write_all(&plain).unwrap();
                drop(e);
                out
            }
        }
    }

    fn decode(
        encoding: Encoding,
        bytes: &[u8],
        chunk: usize,
    ) -> Result<(Vec<u8>, u64, u64), DecodeError> {
        let mut out = Collect::default();
        let mut decoder = Decoder::new(encoding, &mut out)?;
        for piece in bytes.chunks(chunk) {
            decoder.write(piece)?;
        }
        let (wire, body) = decoder.finish()?;
        Ok((out.0, wire, body))
    }

    #[test]
    fn each_encoding_decodes_in_small_chunks_and_counts_both_sizes() {
        for encoding in [
            Encoding::Gzip,
            Encoding::Deflate,
            Encoding::Brotli,
            Encoding::Zstd,
        ] {
            let bytes = encoded(encoding);
            let (plain, wire, body) = decode(encoding, &bytes, 7).unwrap();
            assert_eq!(plain, text(), "{encoding:?}");
            assert_eq!(wire, bytes.len() as u64);
            assert_eq!(body, text().len() as u64);
            assert!(wire < body, "{encoding:?}");
        }
    }

    #[test]
    fn a_corrupt_or_cut_body_is_an_error_and_an_empty_one_is_not() {
        let bytes = encoded(Encoding::Gzip);
        let cut = decode(Encoding::Gzip, &bytes[..bytes.len() / 2], 64);
        assert!(matches!(cut, Err(DecodeError::Corrupt(Encoding::Gzip, _))));
        let brotli = encoded(Encoding::Brotli);
        let cut = decode(Encoding::Brotli, &brotli[..brotli.len() / 2], 64);
        assert!(matches!(
            cut,
            Err(DecodeError::Corrupt(Encoding::Brotli, _))
        ));
        let garbage = decode(Encoding::Deflate, b"not deflate at all", 64);
        assert!(matches!(
            garbage,
            Err(DecodeError::Corrupt(Encoding::Deflate, _))
        ));
        assert_eq!(decode(Encoding::Zstd, b"", 1).unwrap(), (Vec::new(), 0, 0));
    }

    #[test]
    fn content_encodings_are_read_without_case() {
        assert_eq!(Encoding::of(" GZIP "), Some(Encoding::Gzip));
        assert_eq!(Encoding::of("x-gzip"), Some(Encoding::Gzip));
        assert_eq!(Encoding::of("br"), Some(Encoding::Brotli));
        assert_eq!(Encoding::of("identity"), None);
        assert_eq!(Encoding::of("gzip, br"), None);
    }
}
