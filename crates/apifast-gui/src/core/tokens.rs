//! OAuth 2.0 tokens in the interface: what is stored for a tab's request,
//! getting a new one in the background, and forgetting it.

use std::sync::Arc;

use apifast_app::{
    CancelToken, NoPrompter, NoSecrets, SecretStore, SendOptions, SendPorts, forget_token,
    get_new_token, stored_token,
};
use apifast_domain::HttpRequest;

use super::collections::Open;
use super::{Message, State, TabKey, TokenState};
use crate::system::{SystemClock, SystemRandom};

/// The token stored for `request` of the tab `key`.
pub(super) fn token_state(open: &Open, key: &TabKey, request: &HttpRequest) -> TokenState {
    let result = with_ports(open, |ports| {
        stored_token(ports, &key.path, &options(open, request))
    });
    match result {
        Ok(Some(info)) => TokenState::Stored(info),
        Ok(None) => TokenState::Missing,
        Err(e) => TokenState::Unreadable(e.to_string()),
    }
}

/// How the tab's request is sent: its environment, the runtime values and
/// the request as edited.
fn options(open: &Open, request: &HttpRequest) -> SendOptions {
    SendOptions {
        environment: open.environment.clone(),
        runtime: open.runtime.clone(),
        script_changes: open.script_changes.clone(),
        draft: Some(request.clone()),
        ..SendOptions::default()
    }
}

/// Runs `f` with the ports of the collection `open`.
fn with_ports<R>(
    open: &Open,
    f: impl FnOnce(
        &SendPorts<
            '_,
            apifast_storage_fs::FsCollection,
            apifast_storage_fs::FsCollection,
            apifast_http::ReqwestTransport,
            apifast_history::FsResponseStore,
        >,
    ) -> R,
) -> R {
    let secrets: Arc<dyn SecretStore + Send> = match &open.vault {
        Some(vault) => vault.clone(),
        None => Arc::new(NoSecrets),
    };
    let (clock, random) = (SystemClock, SystemRandom::default());
    f(&SendPorts {
        collections: &*open.repo,
        environments: &*open.repo,
        transport: &*open.transport,
        responses: &*open.responses,
        clock: &clock,
        random: &random,
        prompter: &NoPrompter,
        secrets: &*secrets,
        report: &|_| {},
        history: None,
        tokens: Some(open.tokens.as_ref()),
        authorizer: None,
        scripts: None,
    })
}

impl State {
    /// Gets a new token for the tab's request, in the background.
    pub(super) fn get_token(&mut self, key: TabKey) {
        if self.snapshot.getting_token.is_some() {
            return;
        }
        let Some(request) = self.tab(&key).map(super::Tab::request).cloned() else {
            return;
        };
        let Some(open) = self.open.get(&key.root) else {
            return;
        };
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
        let options = options(open, &request);
        self.snapshot.getting_token = Some(key.clone());
        let messages = self.messages.clone();
        tokio::spawn(async move {
            let authorizer = apifast_http::LoopbackAuthorizer::system();
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
                authorizer: Some(&authorizer),
                scripts: None,
            };
            let result = get_new_token(ports, &key.path, &options, &CancelToken::new())
                .await
                .map(|_| ())
                .map_err(|e| e.to_string());
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::TokenObtained(key, result));
            }
        });
    }

    pub(super) fn token_obtained(&mut self, key: &TabKey, result: Result<(), String>) {
        self.snapshot.getting_token = None;
        match result {
            Ok(()) => self.snapshot.error = None,
            Err(message) => self.snapshot.error = Some(message),
        }
        self.refresh_variables(Some(key));
    }

    /// Forgets the token stored for the tab's request.
    pub(super) fn forget_token(&mut self, key: &TabKey) {
        let Some(request) = self.tab(key).map(super::Tab::request).cloned() else {
            return;
        };
        let Some(open) = self.open.get(&key.root) else {
            return;
        };
        let result = with_ports(open, |ports| {
            forget_token(ports, &key.path, &options(open, &request))
        });
        if let Err(e) = result {
            self.snapshot.error = Some(e.to_string());
        }
        self.refresh_variables(Some(key));
    }
}
