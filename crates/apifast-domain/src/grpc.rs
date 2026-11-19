//! What a gRPC server offers (docs/07, F8): its services and their
//! methods, as reflection or a `.proto` file describe them.

use crate::GrpcMethodType;

/// A service and its methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrpcService {
    /// Its full name, as `package.Service`.
    pub name: String,
    /// Its methods, in the order they are declared.
    pub methods: Vec<GrpcMethod>,
}

/// A method of a service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrpcMethod {
    /// Its name, as `Method`.
    pub name: String,
    /// What a request calls: `/package.Service/Method`.
    pub path: String,
    /// How it streams.
    pub method_type: GrpcMethodType,
    /// The full name of the message it takes.
    pub input: String,
    /// The full name of the message it answers with.
    pub output: String,
}

/// The names of gRPC's status codes, by code.
const STATUS_NAMES: [&str; 17] = [
    "OK",
    "CANCELLED",
    "UNKNOWN",
    "INVALID_ARGUMENT",
    "DEADLINE_EXCEEDED",
    "NOT_FOUND",
    "ALREADY_EXISTS",
    "PERMISSION_DENIED",
    "RESOURCE_EXHAUSTED",
    "FAILED_PRECONDITION",
    "ABORTED",
    "OUT_OF_RANGE",
    "UNIMPLEMENTED",
    "INTERNAL",
    "UNAVAILABLE",
    "DATA_LOSS",
    "UNAUTHENTICATED",
];

/// The name of the gRPC status `code` (`OK`, `NOT_FOUND`…), if it is one.
pub fn grpc_status_name(code: u16) -> Option<&'static str> {
    STATUS_NAMES.get(usize::from(code)).copied()
}

/// Whether a response's headers are those of a gRPC call: its stored body
/// is then the call's log, as a WebSocket connection's
/// ([`crate::WsLogEntry`]).
pub fn is_grpc_response(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-type")
            && value
                .trim()
                .to_ascii_lowercase()
                .starts_with("application/grpc")
    })
}

/// The method at `path` (`/package.Service/Method`) among `services`.
pub fn find_grpc_method<'a>(services: &'a [GrpcService], path: &str) -> Option<&'a GrpcMethod> {
    let path = path.trim();
    (services.iter())
        .flat_map(|service| &service.methods)
        .find(|method| method.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_have_names_and_calls_are_told_by_their_content_type() {
        assert_eq!(grpc_status_name(0), Some("OK"));
        assert_eq!(grpc_status_name(16), Some("UNAUTHENTICATED"));
        assert_eq!(grpc_status_name(17), None);
        let headers = |value: &str| vec![("Content-Type".to_owned(), value.to_owned())];
        assert!(is_grpc_response(&headers("application/grpc+proto")));
        assert!(!is_grpc_response(&headers("application/json")));
    }

    #[test]
    fn a_method_is_found_by_its_path() {
        let services = [GrpcService {
            name: "hello.Greeter".into(),
            methods: vec![GrpcMethod {
                name: "SayHello".into(),
                path: "/hello.Greeter/SayHello".into(),
                method_type: GrpcMethodType::Unary,
                input: "hello.HelloRequest".into(),
                output: "hello.HelloReply".into(),
            }],
        }];
        assert_eq!(
            find_grpc_method(&services, " /hello.Greeter/SayHello").map(|m| m.name.as_str()),
            Some("SayHello")
        );
        assert!(find_grpc_method(&services, "/hello.Greeter/Other").is_none());
    }
}
