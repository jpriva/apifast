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
