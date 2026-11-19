//! The services of a descriptor pool, as the domain describes them.

use apifast_domain::{GrpcMethod, GrpcMethodType, GrpcService};
use prost_reflect::DescriptorPool;

/// The services in `pool`, sorted by name, but those of reflection itself.
pub(crate) fn services(pool: &DescriptorPool) -> Vec<GrpcService> {
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
    services
}
