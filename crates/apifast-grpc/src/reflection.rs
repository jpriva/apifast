//! Server reflection (docs/07, F8): the services a server offers, and the
//! files that describe them, asked of the server itself. Version v1 is
//! asked first; a server without it is asked v1alpha.

use std::collections::{BTreeMap, VecDeque};

use apifast_app::TransportError;
use prost::Message as _;
use prost_reflect::DescriptorPool;
use prost_types::FileDescriptorProto;
use protox::file::{FileResolver as _, GoogleFileResolver};
use tonic::metadata::MetadataMap;
use tonic::{Code, Status};

use crate::channel::Prefixed;

/// What one version of reflection asks: the services, then the file of
/// each, then the files those import.
enum Ask {
    Services,
    Symbol(String),
    File(String),
}

/// What it answers.
enum Answer {
    Services(Vec<String>),
    Files(Vec<Vec<u8>>),
}

/// Generates the exchange for one version of the reflection protocol;
/// both have the same messages, in different packages.
macro_rules! version {
    ($name:ident, $pb:path) => {
        async fn $name(
            channel: Prefixed,
            metadata: &MetadataMap,
            ask: Ask,
        ) -> Result<Answer, Status> {
            use pb::server_reflection_request::MessageRequest;
            use pb::server_reflection_response::MessageResponse;
            use $pb as pb;
            let request = pb::ServerReflectionRequest {
                host: String::new(),
                message_request: Some(match ask {
                    Ask::Services => MessageRequest::ListServices(String::new()),
                    Ask::Symbol(name) => MessageRequest::FileContainingSymbol(name),
                    Ask::File(name) => MessageRequest::FileByFilename(name),
                }),
            };
            let mut request = tonic::Request::new(futures_util::stream::iter([request]));
            *request.metadata_mut() = metadata.clone();
            let mut client = pb::server_reflection_client::ServerReflectionClient::new(channel);
            let mut answers = client.server_reflection_info(request).await?.into_inner();
            let answer = answers
                .message()
                .await?
                .ok_or_else(|| Status::unknown("the server gave no reflection answer"))?;
            match answer.message_response {
                Some(MessageResponse::ListServicesResponse(list)) => Ok(Answer::Services(
                    list.service.into_iter().map(|s| s.name).collect(),
                )),
                Some(MessageResponse::FileDescriptorResponse(files)) => {
                    Ok(Answer::Files(files.file_descriptor_proto))
                }
                Some(MessageResponse::ErrorResponse(error)) => Err(Status::new(
                    Code::from(error.error_code),
                    error.error_message,
                )),
                _ => Err(Status::unknown(
                    "the server gave an unexpected reflection answer",
                )),
            }
        }
    };
}

version!(ask_v1, tonic_reflection::pb::v1);
version!(ask_v1alpha, tonic_reflection::pb::v1alpha);

/// The descriptors of every service `channel`'s server offers, with the
/// files they import.
///
/// # Errors
///
/// If the server does not offer reflection, or its answers are not valid.
pub(crate) async fn reflect(
    channel: &Prefixed,
    metadata: &MetadataMap,
) -> Result<DescriptorPool, TransportError> {
    let mut alpha = false;
    let services = match ask_v1(channel.clone(), metadata, Ask::Services).await {
        Err(status) if status.code() == Code::Unimplemented => {
            alpha = true;
            ask_v1alpha(channel.clone(), metadata, Ask::Services).await
        }
        other => other,
    };
    let services = match services {
        Ok(Answer::Services(names)) => names,
        Ok(Answer::Files(_)) => return Err(unexpected()),
        Err(status) if status.code() == Code::Unimplemented => {
            return Err(TransportError::Protocol(
                "the server does not offer reflection: choose a .proto file".into(),
            ));
        }
        Err(status) => return Err(crate::status_error(&status)),
    };
    let ask = |ask: Ask| async move {
        if alpha {
            ask_v1alpha(channel.clone(), metadata, ask).await
        } else {
            ask_v1(channel.clone(), metadata, ask).await
        }
    };
    let mut files: BTreeMap<String, FileDescriptorProto> = BTreeMap::new();
    let mut queue: VecDeque<Ask> = (services.into_iter())
        .filter(|name| !name.starts_with("grpc.reflection."))
        .map(Ask::Symbol)
        .collect();
    while let Some(next) = queue.pop_front() {
        let wanted = match &next {
            Ask::File(name) => Some(name.clone()),
            _ => None,
        };
        let answer = match (ask(next).await, wanted) {
            (Ok(Answer::Files(found)), _) => found,
            (Ok(Answer::Services(_)), _) => return Err(unexpected()),
            // A well-known type the server does not serve is built in.
            (Err(status), Some(name)) if name.starts_with("google/protobuf/") => {
                match GoogleFileResolver::new().open_file(&name) {
                    Ok(file) => {
                        files.insert(name, file.file_descriptor_proto().clone());
                        continue;
                    }
                    Err(_) => return Err(crate::status_error(&status)),
                }
            }
            (Err(status), _) => return Err(crate::status_error(&status)),
        };
        for bytes in found_files(&answer)? {
            for dependency in &bytes.dependency {
                let known = files.contains_key(dependency)
                    || (queue.iter()).any(|q| matches!(q, Ask::File(f) if f == dependency));
                if !known {
                    queue.push_back(Ask::File(dependency.clone()));
                }
            }
            files.entry(bytes.name().to_owned()).or_insert(bytes);
        }
    }
    let mut pool = DescriptorPool::new();
    pool.add_file_descriptor_protos(files.into_values())
        .map_err(|e| TransportError::Protocol(format!("the server's descriptors: {e}")))?;
    Ok(pool)
}

/// The files of an answer, decoded.
fn found_files(answer: &[Vec<u8>]) -> Result<Vec<FileDescriptorProto>, TransportError> {
    (answer.iter())
        .map(|bytes| {
            FileDescriptorProto::decode(bytes.as_slice())
                .map_err(|e| TransportError::Protocol(format!("a descriptor of the server: {e}")))
        })
        .collect()
}

fn unexpected() -> TransportError {
    TransportError::Protocol("the server gave an unexpected reflection answer".into())
}
