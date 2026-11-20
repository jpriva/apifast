//! What a descriptor pool describes, as the domain says it: the services
//! and their methods, and the fields of the messages the methods take.

use std::collections::{BTreeMap, VecDeque};

use apifast_domain::{
    GrpcCatalog, GrpcField, GrpcFieldKind, GrpcMethod, GrpcMethodType, GrpcService,
};
use prost_reflect::{DescriptorPool, FieldDescriptor, Kind, MessageDescriptor};

/// The services in `pool`, sorted by name, but those of reflection itself;
/// and the messages their methods take, with those inside them.
pub(crate) fn catalog(pool: &DescriptorPool) -> GrpcCatalog {
    let mut services: Vec<GrpcService> = pool
        .services()
        .filter(|service| !service.full_name().starts_with("grpc.reflection."))
        .map(|service| GrpcService {
            name: service.full_name().to_owned(),
            methods: service
                .methods()
                .map(|method| GrpcMethod {
                    name: method.name().to_owned(),
                    path: format!("/{}/{}", service.full_name(), method.name()),
                    method_type: match (method.is_client_streaming(), method.is_server_streaming())
                    {
                        (false, false) => GrpcMethodType::Unary,
                        (true, false) => GrpcMethodType::ClientStreaming,
                        (false, true) => GrpcMethodType::ServerStreaming,
                        (true, true) => GrpcMethodType::BidiStreaming,
                    },
                    input: method.input().full_name().to_owned(),
                    output: method.output().full_name().to_owned(),
                })
                .collect(),
        })
        .collect();
    services.sort_by(|a, b| a.name.cmp(&b.name));
    let inputs = (pool.services()).flat_map(|service| {
        service
            .methods()
            .map(|method| method.input())
            .collect::<Vec<_>>()
    });
    GrpcCatalog {
        services,
        messages: messages(inputs),
    }
}

/// The fields of `roots` and of every message inside them.
fn messages(
    roots: impl IntoIterator<Item = MessageDescriptor>,
) -> BTreeMap<String, Vec<GrpcField>> {
    let mut messages = BTreeMap::new();
    let mut queue: VecDeque<MessageDescriptor> = roots.into_iter().collect();
    while let Some(message) = queue.pop_front() {
        if messages.contains_key(message.full_name()) || well_known(&message) {
            continue;
        }
        let fields: Vec<GrpcField> = message.fields().map(|field| self::field(&field)).collect();
        for field in message.fields() {
            if let Kind::Message(inner) = value_kind(&field) {
                queue.push_back(inner);
            }
        }
        messages.insert(message.full_name().to_owned(), fields);
    }
    messages
}

/// What a field holds; for a map, what its values hold.
fn value_kind(field: &FieldDescriptor) -> Kind {
    match field.kind() {
        Kind::Message(entry) if field.is_map() => entry.map_entry_value_field().kind(),
        kind => kind,
    }
}

/// A Google message whose JSON is not an object of its fields (a time, a
/// duration, a wrapper…): it is written as a scalar.
fn well_known(message: &MessageDescriptor) -> bool {
    message.full_name().starts_with("google.protobuf.")
}

fn field(field: &FieldDescriptor) -> GrpcField {
    let kind = match value_kind(field) {
        Kind::Message(message) if well_known(&message) => {
            GrpcFieldKind::Scalar(message.full_name().to_owned())
        }
        Kind::Message(message) => GrpcFieldKind::Message(message.full_name().to_owned()),
        Kind::Enum(values) => GrpcFieldKind::Enum(
            values.full_name().to_owned(),
            values.values().map(|v| v.name().to_owned()).collect(),
        ),
        scalar => GrpcFieldKind::Scalar(format!("{scalar:?}").to_ascii_lowercase()),
    };
    GrpcField {
        name: field.json_name().to_owned(),
        kind,
        repeated: field.is_list(),
        map: field.is_map(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_described_with_their_nested_ones_enums_and_maps() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(
            dir.path().join("s.proto"),
            "syntax = \"proto3\";\npackage s;\nimport \"google/protobuf/timestamp.proto\";\nenum Color { RED = 0; BLUE = 1; }\nmessage Tag { string label = 1; }\nmessage Ask {\n  string user_id = 1;\n  repeated Tag tags = 2;\n  map<string, Tag> by_name = 3;\n  Color color = 4;\n  google.protobuf.Timestamp at = 5;\n}\nmessage Done {}\nservice S { rpc Do(Ask) returns (Done); }\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let pool = crate::protos::compile(dir.path(), "s.proto").unwrap_or_else(|e| panic!("{e}"));
        let catalog = catalog(&pool);
        assert_eq!(catalog.services[0].methods[0].input, "s.Ask");
        let ask = &catalog.messages["s.Ask"];
        let names: Vec<&str> = ask.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["userId", "tags", "byName", "color", "at"]);
        assert_eq!(ask[0].kind, GrpcFieldKind::Scalar("string".into()));
        assert!(ask[1].repeated && !ask[1].map);
        assert_eq!(ask[2].kind, GrpcFieldKind::Message("s.Tag".into()));
        assert!(ask[2].map);
        assert_eq!(
            ask[3].kind,
            GrpcFieldKind::Enum("s.Color".into(), vec!["RED".into(), "BLUE".into()])
        );
        assert_eq!(
            ask[4].kind,
            GrpcFieldKind::Scalar("google.protobuf.Timestamp".into())
        );
        assert!(catalog.messages.contains_key("s.Tag"));
        assert!(!catalog.messages.contains_key("google.protobuf.Timestamp"));
    }
}
