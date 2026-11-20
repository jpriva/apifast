//! GraphQL schemas in the interface: the one kept for a tab's request,
//! read once from the collection's cache, and asking the server for it
//! again in the background.

use std::sync::Arc;

use apifast_app::{
    CancelToken, NoPrompter, NoSecrets, SecretStore, SendOptions, SendPorts, cached_schema,
    fetch_schema, schema_key,
};
use apifast_domain::{Body, GraphQlSchema, HttpRequest};

use super::collections::Open;
use super::{Message, State, TabKey};
use crate::system::{SystemClock, SystemRandom};

/// What the interface knows of a GraphQL request's schema.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaView {
    /// The last schema the server gave, if any.
    pub schema: Option<Arc<GraphQlSchema>>,
    /// Whether it is being asked for.
    pub fetching: bool,
    /// Why the last time it was asked for failed.
    pub error: Option<String>,
}

/// The key the schema of `request` is kept by in `open`, if it is GraphQL.
pub(super) fn key_of(open: &Open, request: &HttpRequest) -> Option<String> {
    matches!(request.body, Body::GraphQl { .. })
        .then(|| schema_key(&request.url, open.environment.as_deref()))
}

impl State {
    /// Reads the cached schema of the tab `key` into its collection, the
    /// first time it is needed.
    pub(super) fn load_schema(&mut self, key: &TabKey) {
        let Some(request) = self.tab(key).map(|tab| tab.request().clone()) else {
            return;
        };
        let Some(open) = self.open.get_mut(&key.root) else {
            return;
        };
        let Some(schema_key) = key_of(open, &request) else {
            return;
        };
        if open.schemas.contains_key(&schema_key) {
            return;
        }
        let view = match cached_schema(&*open.schema_cache, &schema_key) {
            Ok(schema) => SchemaView {
                schema: schema.map(Arc::new),
                ..SchemaView::default()
            },
            Err(e) => SchemaView {
                error: Some(e.to_string()),
                ..SchemaView::default()
            },
        };
        open.schemas.insert(schema_key, view);
    }

    /// Asks the server of the tab's request for its schema, in the
    /// background.
    pub(super) fn fetch_schema(&mut self, key: TabKey) {
        let Some(request) = self.tab(&key).map(|tab| tab.request().clone()) else {
            return;
        };
        let Some(open) = self.open.get_mut(&key.root) else {
            return;
        };
        let Some(schema_key) = key_of(open, &request) else {
            return;
        };
        let view = open.schemas.entry(schema_key.clone()).or_default();
        if view.fetching {
            return;
        }
        view.fetching = true;
        let (repo, transport, responses, tokens, cache) = (
            open.repo.clone(),
            open.transport.clone(),
            open.responses.clone(),
            open.tokens.clone(),
            open.schema_cache.clone(),
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
            let result = fetch_schema(
                ports,
                (&key.path, &options),
                (&*cache, &schema_key),
                &CancelToken::new(),
            )
            .await
            .map_err(|e| e.to_string());
            if let Some(messages) = messages.upgrade() {
                let _ = messages.send(Message::SchemaFetched(root, schema_key, result));
            }
        });
        self.refresh_variables(None);
    }

    /// What the server answered for the schema kept by `schema_key`.
    pub(super) fn schema_fetched(
        &mut self,
        root: &std::path::Path,
        schema_key: String,
        result: Result<GraphQlSchema, String>,
    ) {
        let Some(open) = self.open.get_mut(root) else {
            return;
        };
        let view = open.schemas.entry(schema_key).or_default();
        view.fetching = false;
        match result {
            Ok(schema) => {
                view.schema = Some(Arc::new(schema));
                view.error = None;
            }
            // The last schema stays: a server down for a moment does not
            // take it away.
            Err(why) => view.error = Some(why),
        }
        self.refresh_collection(root);
    }
}
