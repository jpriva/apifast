//! Messages without generated code: JSON in and out, encoded to and from
//! protobuf by the method's descriptors (prost-reflect).

use apifast_domain::WsData;
use prost::Message as _;
use prost_reflect::{DynamicMessage, MessageDescriptor, SerializeOptions};
use tonic::Status;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};

/// The codec of one method: what it takes and what it answers with.
#[derive(Debug, Clone)]
pub(crate) struct DynamicCodec {
    output: MessageDescriptor,
}

impl DynamicCodec {
    /// The codec of a method answering with `output`.
    pub(crate) fn new(output: MessageDescriptor) -> Self {
        Self { output }
    }
}

impl Codec for DynamicCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = DynamicEncoder;
    type Decoder = DynamicDecoder;

    fn encoder(&mut self) -> DynamicEncoder {
        DynamicEncoder
    }

    fn decoder(&mut self) -> DynamicDecoder {
        DynamicDecoder(self.output.clone())
    }
}

/// Encodes messages.
#[derive(Debug)]
pub(crate) struct DynamicEncoder;

impl Encoder for DynamicEncoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn encode(&mut self, item: DynamicMessage, dst: &mut EncodeBuf<'_>) -> Result<(), Status> {
        item.encode(dst)
            .map_err(|e| Status::internal(format!("cannot encode the message: {e}")))
    }
}

/// Decodes messages of one type.
#[derive(Debug)]
pub(crate) struct DynamicDecoder(MessageDescriptor);

impl Decoder for DynamicDecoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<DynamicMessage>, Status> {
        DynamicMessage::decode(self.0.clone(), src)
            .map(Some)
            .map_err(|e| Status::internal(format!("cannot decode the answer: {e}")))
    }
}

/// `data`, JSON, as a message of type `input`.
///
/// # Errors
///
/// If it is not JSON, or does not fit the type; the message says why.
pub(crate) fn from_json(
    data: &WsData,
    input: &MessageDescriptor,
) -> Result<DynamicMessage, String> {
    let WsData::Text(text) = data else {
        return Err("a gRPC message is JSON, not binary".into());
    };
    let mut json = serde_json::Deserializer::from_str(text);
    let message = DynamicMessage::deserialize(input.clone(), &mut json)
        .map_err(|e| format!("not a `{}`: {e}", input.full_name()))?;
    json.end()
        .map_err(|e| format!("not a `{}`: {e}", input.full_name()))?;
    Ok(message)
}

/// `message` as JSON on one line, with every field, defaults too.
pub(crate) fn to_json(message: &DynamicMessage) -> String {
    let options = SerializeOptions::new().skip_default_fields(false);
    let mut out = Vec::new();
    let mut json = serde_json::Serializer::new(&mut out);
    match message.serialize_with_options(&mut json, &options) {
        Ok(()) => String::from_utf8_lossy(&out).into_owned(),
        Err(e) => format!("{{\"error\": \"cannot show the message: {e}\"}}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> MessageDescriptor {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(
            dir.path().join("m.proto"),
            "syntax = \"proto3\";\npackage t;\nmessage M { string name = 1; int32 count = 2; repeated string tags = 3; }\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let pool = crate::protos::compile(dir.path(), "m.proto").unwrap_or_else(|e| panic!("{e}"));
        pool.get_message_by_name("t.M")
            .unwrap_or_else(|| panic!("no t.M"))
    }

    #[test]
    fn json_goes_in_and_comes_out_with_defaults() {
        let m = descriptor();
        let message = from_json(&WsData::Text(r#"{"name": "Ada"}"#.into()), &m)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(to_json(&message), r#"{"name":"Ada","count":0,"tags":[]}"#);
        let error = from_json(&WsData::Text(r#"{"nope": 1}"#.into()), &m).unwrap_err();
        assert!(error.contains("not a `t.M`"), "{error}");
        assert!(from_json(&WsData::Text("{} {}".into()), &m).is_err());
        assert!(from_json(&WsData::Binary(vec![1]), &m).is_err());
    }
}
