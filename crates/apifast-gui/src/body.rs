//! Response bodies read by windows: a handle to where a body is kept, and an
//! index of its rows so a viewer can draw only the visible ones without
//! holding the body in memory.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

/// The longest row, in bytes. Longer lines (minified JSON is one line) are
/// cut into rows of about this size, so a row is always cheap to draw.
pub const MAX_ROW: u64 = 2048;

/// Somewhere a body can be read from by windows.
pub trait BodySource: Send + Sync {
    /// Size of the body in bytes.
    fn len(&self) -> u64;

    /// Whether the body is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Up to `len` bytes starting at `offset`; fewer at the end.
    ///
    /// # Errors
    ///
    /// A message when the body cannot be read, ready to show.
    fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>, String>;
}

impl BodySource for Vec<u8> {
    fn len(&self) -> u64 {
        self.len() as u64
    }

    fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.len());
        let end = start.saturating_add(len).min(self.len());
        Ok(self[start..end].to_vec())
    }
}

/// A shared handle to a body. Two handles are equal when their keys are:
/// the key names one body, such as its response id.
#[derive(Clone)]
pub struct BodyHandle {
    key: Arc<str>,
    source: Arc<dyn BodySource>,
}

impl BodyHandle {
    /// A handle to `source`, named `key`.
    pub fn new(key: impl Into<Arc<str>>, source: Arc<dyn BodySource>) -> Self {
        Self {
            key: key.into(),
            source,
        }
    }

    /// A body held in memory, for tests and samples.
    pub fn memory(key: impl Into<Arc<str>>, bytes: impl Into<Vec<u8>>) -> Self {
        Self::new(key, Arc::new(bytes.into()))
    }

    /// The name of the body.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Size of the body in bytes.
    pub fn len(&self) -> u64 {
        self.source.len()
    }

    /// Whether the body is empty.
    pub fn is_empty(&self) -> bool {
        self.source.is_empty()
    }

    /// Up to `len` bytes starting at `offset`.
    ///
    /// # Errors
    ///
    /// A message when the body cannot be read.
    pub fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        self.source.read(offset, len)
    }
}

impl PartialEq for BodyHandle {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for BodyHandle {}

impl fmt::Debug for BodyHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BodyHandle")
            .field("key", &self.key)
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

/// Rows between two checkpoints of a [`RowIndex`].
pub const STRIDE: usize = 64;

/// Finds where rows start in bytes fed in order: after each newline, and
/// every [`MAX_ROW`] bytes of a longer line, never inside a character.
#[derive(Debug, Clone, Copy, Default)]
struct Scanner {
    /// Where the current row starts.
    row_start: u64,
    /// Bytes fed so far, as an offset into the body.
    at: u64,
}

impl Scanner {
    fn starting_at(at: u64) -> Self {
        Self { row_start: at, at }
    }

    /// Feeds the next bytes. `row` gets the start of each row that begins
    /// in them, and whether it continues a cut line.
    fn feed(&mut self, chunk: &[u8], mut row: impl FnMut(u64, bool)) {
        let base = self.at;
        let mut i = 0;
        while i < chunk.len() {
            let at = base + i as u64;
            // Bytes left before the row is long enough to cut.
            let room = usize::try_from((self.row_start + MAX_ROW).saturating_sub(at))
                .unwrap_or(usize::MAX)
                .min(chunk.len() - i);
            if let Some(n) = chunk[i..i + room].iter().position(|&b| b == b'\n') {
                self.row_start = at + n as u64 + 1;
                row(self.row_start, false);
                i += n + 1;
                continue;
            }
            i += room;
            let Some(&byte) = chunk.get(i) else {
                break; // The row goes on in the next chunk.
            };
            let at = base + i as u64;
            if byte == b'\n' {
                self.row_start = at + 1;
                row(self.row_start, false);
                i += 1;
            } else if is_char_start(byte) {
                self.row_start = at;
                row(at, true);
            } else {
                i += 1;
            }
        }
        self.at = base + chunk.len() as u64;
    }
}

/// Where a row starts, kept for one row in every [`STRIDE`].
#[derive(Debug, Clone, Copy)]
struct Checkpoint {
    start: u64,
    /// The 1-based line the row belongs to.
    line: u64,
    /// Whether it continues a cut line.
    continued: bool,
}

/// A row: its bytes, with its newline, and the line it starts, or `None`
/// if it continues a line cut because it was too long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Bytes of the body it covers.
    pub range: Range<u64>,
    /// The 1-based line it starts.
    pub line: Option<u64>,
}

/// An index of the rows of a body, built from its bytes in order, chunk by
/// chunk, so a large body is indexed over several frames.
///
/// It keeps where one row in every [`STRIDE`] starts: 24 bytes per 64 rows,
/// about 3 MB for 100 MB of short lines. The rows in between are found
/// again from the bytes of the window being drawn ([`Self::split`]).
#[derive(Debug, Clone)]
pub struct RowIndex {
    checkpoints: Vec<Checkpoint>,
    rows: usize,
    line: u64,
    scanner: Scanner,
}

impl Default for RowIndex {
    fn default() -> Self {
        Self {
            checkpoints: vec![Checkpoint {
                start: 0,
                line: 1,
                continued: false,
            }],
            rows: 1,
            line: 1,
            scanner: Scanner::default(),
        }
    }
}

impl RowIndex {
    /// Bytes fed so far.
    pub fn scanned(&self) -> u64 {
        self.scanner.at
    }

    /// Indexes the next bytes of the body.
    pub fn feed(&mut self, chunk: &[u8]) {
        let Self {
            checkpoints,
            rows,
            line,
            scanner,
        } = self;
        scanner.feed(chunk, |start, continued| {
            if !continued {
                *line += 1;
            }
            if *rows % STRIDE == 0 {
                checkpoints.push(Checkpoint {
                    start,
                    line: *line,
                    continued,
                });
            }
            *rows += 1;
        });
    }

    /// Number of rows found so far. An empty body has one empty row, and so
    /// does the end of a body that ends with a newline.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The bytes to read to [`split`](Self::split) `rows`: from the
    /// checkpoint before them to past their end, within what was indexed.
    pub fn span(&self, rows: &Range<usize>) -> Range<u64> {
        let (first, checkpoint) = self.checkpoint(rows.start);
        let count = rows.end.saturating_sub(first) as u64;
        // A row is at most `MAX_ROW` bytes, one character more and its
        // newline.
        let end = checkpoint
            .start
            .saturating_add(count.saturating_mul(MAX_ROW + 5))
            .min(self.scanned());
        checkpoint.start..end
    }

    /// The rows in `rows`, found in `bytes`, which were read at the start
    /// of [`span`](Self::span). Rows past what the bytes hold are left out.
    pub fn split(&self, rows: &Range<usize>, bytes: &[u8]) -> Vec<Row> {
        let (first, checkpoint) = self.checkpoint(rows.start);
        let mut starts = vec![(checkpoint.start, checkpoint.continued)];
        Scanner::starting_at(checkpoint.start)
            .feed(bytes, |start, continued| starts.push((start, continued)));
        let end_of_bytes = checkpoint.start + bytes.len() as u64;
        let mut line = checkpoint.line;
        let mut out = Vec::with_capacity(rows.len());
        for (k, &(start, continued)) in starts.iter().enumerate() {
            if k > 0 && !continued {
                line += 1;
            }
            let row = first + k;
            if row >= rows.end.min(self.rows) {
                break;
            }
            if row >= rows.start {
                let end = starts.get(k + 1).map_or(end_of_bytes, |next| next.0);
                out.push(Row {
                    range: start..end,
                    line: (!continued).then_some(line),
                });
            }
        }
        out
    }

    /// The checkpoint at or before `row`, and the row it is.
    fn checkpoint(&self, row: usize) -> (usize, Checkpoint) {
        let index = (row / STRIDE).min(self.checkpoints.len() - 1);
        (index * STRIDE, self.checkpoints[index])
    }
}

/// Whether `byte` starts a UTF-8 character (it is not a continuation byte).
fn is_char_start(byte: u8) -> bool {
    byte & 0b1100_0000 != 0b1000_0000
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(chunks: &[&[u8]]) -> RowIndex {
        let mut index = RowIndex::default();
        for chunk in chunks {
            index.feed(chunk);
        }
        index
    }

    /// Every row, found as a viewer finds them: through a span.
    fn all(index: &RowIndex, text: &[u8]) -> Vec<Row> {
        let rows = 0..index.rows();
        let span = index.span(&rows);
        let bytes = &text[usize::try_from(span.start).unwrap()..usize::try_from(span.end).unwrap()];
        index.split(&rows, bytes)
    }

    fn rows(index: &RowIndex, text: &[u8]) -> Vec<Range<u64>> {
        all(index, text).into_iter().map(|row| row.range).collect()
    }

    fn lines(index: &RowIndex, text: &[u8]) -> Vec<Option<u64>> {
        all(index, text).into_iter().map(|row| row.line).collect()
    }

    #[test]
    fn rows_end_after_each_newline() {
        let text = b"ab\ncd\n\nef";
        let index = index(&[text]);
        assert_eq!(rows(&index, text), [0..3, 3..6, 6..7, 7..9]);
        assert_eq!(lines(&index, text), [Some(1), Some(2), Some(3), Some(4)]);
    }

    #[test]
    fn an_empty_body_has_one_empty_row() {
        let index = index(&[]);
        assert_eq!(rows(&index, b""), vec![0..0]);
    }

    #[test]
    fn chunks_split_anywhere_give_the_same_rows() {
        let text = format!("a\n{}\né\n", "x".repeat(5000)).into_bytes();
        let whole = index(&[&text]);
        for size in [1, 7, 2048, 4999] {
            let chunks: Vec<&[u8]> = text.chunks(size).collect();
            let split = index(&chunks);
            assert_eq!(all(&split, &text), all(&whole, &text), "chunks of {size}");
        }
    }

    #[test]
    fn long_lines_are_cut_into_rows_that_keep_their_line_number() {
        let text = format!("{}\nend", "x".repeat(5000));
        let index = index(&[text.as_bytes()]);
        let text = text.as_bytes();
        assert_eq!(
            rows(&index, text),
            [0..2048, 2048..4096, 4096..5001, 5001..5004]
        );
        assert_eq!(lines(&index, text), [Some(1), None, None, Some(2)]);
    }

    #[test]
    fn a_cut_never_splits_a_character() {
        // `é` is two bytes: the cut at 2048 would fall inside the one at
        // 2047, so it moves after it.
        let text = format!("{}é{}", "x".repeat(2047), "y".repeat(10));
        let index = index(&[text.as_bytes()]);
        assert_eq!(rows(&index, text.as_bytes()), [0..2049, 2049..2059]);
    }

    #[test]
    fn any_window_is_split_like_the_whole_body() {
        // Short lines, long ones and cut ones, across many checkpoints.
        let mut text = String::new();
        for n in 0..1000 {
            let len = [3, 0, 40, 5000][n % 4];
            text.push_str(&"x".repeat(len));
            text.push('\n');
        }
        let text = text.as_bytes();
        let index = index(&[text]);
        let whole = all(&index, text);
        // A thousand lines; the 250 long ones cut into three rows each.
        assert_eq!(index.rows(), 1501);
        assert_eq!(whole.len(), index.rows());
        for rows in [0..1, 63..65, 64..128, 500..537, 1400..index.rows()] {
            let span = index.span(&rows);
            let bytes =
                &text[usize::try_from(span.start).unwrap()..usize::try_from(span.end).unwrap()];
            assert_eq!(
                index.split(&rows, bytes),
                whole[rows.clone()],
                "rows {rows:?}"
            );
        }
    }

    #[test]
    fn a_memory_body_reads_by_windows() {
        let body = BodyHandle::memory("b", "hello");
        assert_eq!(body.read(1, 3).unwrap(), b"ell");
        assert_eq!(body.read(4, 10).unwrap(), b"o");
        assert_eq!(body.read(9, 10).unwrap(), b"");
        assert_eq!(body, BodyHandle::memory("b", "other"));
    }
}
