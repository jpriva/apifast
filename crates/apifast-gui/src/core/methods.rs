//! The methods of a gRPC request (docs/09, F8): asked of its server by
//! reflection, or read from its `.proto`, in the background, and kept by
//! its URL, `.proto` and environment while the collection is open.

use std::sync::Arc;

use apifast_app::{
    CancelToken, NoPrompter, NoSecrets, SecretStore, SendOptions, SendPorts, describe_grpc,
};
use apifast_domain::{Body, GrpcCatalog, HttpRequest};

use super::collections::Open;
use super::{Message, State, TabKey};
use crate::system::{SystemClock, SystemRandom};

/// What the interface knows of a gRPC request's methods.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MethodsView {
    /// What the server or the `.proto` described last.
    pub catalog: Option<Arc<GrpcCatalog>>,
    /// Whether they are being loaded.
    pub loading: bool,
    /// Why the last load failed.
    pub error: Option<String>,
}

/// The key the methods of `request` are kept by in `open`, if it is gRPC.
pub(super) fn key_of(open: &Open, request: &HttpRequest) -> Option<String> {
    let Body::Grpc(call) = &request.body else {
        return None;
    };
    Some(format!(
        "{}\n{}\n{}",
        request.url.trim(),
        call.proto_path.as_deref().unwrap_or_default(),
        open.environment.as_deref().unwrap_or_default()
    ))
}

/// The gRPC transport of this build, over `http`'s connections: tonic's,
/// or, without the `grpc` feature, one that says there is none.
#[cfg(feature = "grpc")]
pub(super) fn grpc_transport(http: apifast_http::ReqwestTransport) -> apifast_grpc::TonicTransport {
    apifast_grpc::TonicTransport::new(http)
}

/// The gRPC transport of this build: without the `grpc` feature, one that
/// says there is none.
#[cfg(not(feature = "grpc"))]
pub(super) fn grpc_transport(_: apifast_http::ReqwestTransport) -> apifast_app::NoGrpc {
    apifast_app::NoGrpc
}

impl State {
    /// Loads the methods of the tab's gRPC request, as it is on screen, in
    /// the background.
    pub(super) fn load_methods(&mut self, key: TabKey) {
        let Some(request) = self.tab(&key).map(|tab| tab.request().clone()) else {
            return;
        };
        let Some(open) = self.open.get_mut(&key.root) else {
            return;
        };
        let Some(methods_key) = key_of(open, &request) else {
            return;
        };
        let view = open.methods.entry(methods_key.clone()).or_default();
        if view.loading {
            return;
        }
        view.loading = true;
        let (repo, transport, responses, tokens) = (
            open.repo.clone(),
            open.transport.clone(),
            open.responses.clone(),
            open.tokens.clone(),
        );
        let secrets: Arc<dyn SecretStore + Send> = match &open.vault {
            Some(vault) => vault.clone(),
            None => Arc::new(NoSecrets),
        };
        let options = SendOptions {
            environment: open.environment.clone(),
            runtime: open.runtime.clone(),
            script_changes: open.script_changes.clone(),
            draft: Some(request),
            ..SendOptions::default()
        };
        let messages = self.messages.clone();
        let root = key.root.clone();
        tokio::spawn(async move {
            let (clock, random) = (SystemClock, SystemRandom::default());
            let ports = SendPorts {
                collections: &*repo,
                environments: &*repo,
                transport: &*transport,
                responses: &*responses,
                clock: &clock,
                random: &random,
                prompter: &NoPrompter,
                secrets: &*secrets,
                report: &|_| {},
                history: None,
                tokens: Some(tokens.as_ref()),
                authorizer: None,
                scripts: None,
            };
            let grpc = grpc_transport((*transport).clone());
            let result = describe_grpc(ports, &grpc, &key.path, &options, &CancelToken::new())
                .await
                .map_err(|e| e.to_string());
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::MethodsLoaded(root, methods_key, result));
            }
        });
        self.refresh_variables(None);
    }

    /// What the server or the `.proto` gave for the methods kept by
    /// `methods_key`.
    pub(super) fn methods_loaded(
        &mut self,
        root: &std::path::Path,
        methods_key: String,
        result: Result<GrpcCatalog, String>,
    ) {
        let Some(open) = self.open.get_mut(root) else {
            return;
        };
        let view = open.methods.entry(methods_key).or_default();
        view.loading = false;
        match result {
            Ok(catalog) => {
                view.catalog = Some(Arc::new(catalog));
                view.error = None;
            }
            Err(why) => view.error = Some(why),
        }
        self.refresh_collection(root);
    }
}
