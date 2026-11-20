//! Use cases and ports (traits) of apifast.
//!
//! The use cases ([`send_request`], [`save_request`], ...) only talk to the
//! outside world through the ports in this crate. Adapters implement them;
//! [`memory`] has in-memory versions for tests.
//!
//! Sending goes through three types, each built only from the previous one:
//! a request file → [`apifast_domain::ResolvedRequest`] (inheritance applied)
//! → [`RenderedRequest`] (templates and auth applied) → the [`HttpTransport`].

mod assertions;
mod checks;
mod cookies;
mod copy;
mod data;
mod environments;
mod error;
mod examples;
mod export;
mod functions;
mod graphql;
mod grpc;
mod history;
mod host;
mod import;
pub mod memory;
mod oauth2;
mod organize;
mod ports;
mod reimport;
mod render;
mod report;
mod runner;
mod scripts;
mod url_encoding;
mod use_cases;
mod websocket;

pub use assertions::{ExpressionError, ResponseFacts, Verdict, check, reason_phrase};
pub use checks::{AssertionOutcome, CHECK_LIMIT, CaptureOutcome, apply_captures, variable_value};
pub use cookies::{CookieStore, Cookies};
pub use copy::{CopiedRequest, render_for_copy};
pub use data::{DataError, DataFormat, parse_data};
pub use environments::{
    create_environment, delete_environment, rename_environment, save_environment,
};
pub use error::{AppError, RepositoryError, StoreError, TransportError};
pub use examples::{EXAMPLE_LIMIT, ExampleError, example_from, example_name};
pub use export::collection_contents;
pub use graphql::{SchemaCache, cached_schema, fetch_schema, schema_key};
pub use grpc::{GrpcTransport, NO_GRPC, NoGrpc, call_grpc, describe_grpc};
pub use history::{
    HIDDEN, HistoryEntry, HistoryLimits, HistoryOutcome, HistoryStore, hide_query, within_limits,
};
pub use host::AppHost;
pub use import::{
    ImportPlan, ImportReport, ImportWarning, ImportedCollection, ImportedItem, PlannedEntry,
    apply_import, plan_import,
};
pub use oauth2::{OAuth2Token, TokenInfo, TokenStore, forget_token, get_new_token, stored_token};
pub use organize::{
    Placement, create_folder, create_request, delete_item, move_item, rename_item, slug,
};
pub use ports::{
    Authorization, Authorizer, BodyWriter, CancelToken, Clock, CollectionRepository,
    CollectionTree, EnvironmentRepository, HttpTransport, ItemPath, LoadedRequest, NoPrompter,
    NoSecrets, Pause, Prompter, Random, RequestPath, ResponseHead, ResponseId, ResponseStore,
    SecretStore, Session, SessionStore, SessionTab, SharedSecretFiles, StoredBody, TimelineEvent,
    TimelineKind, TreeItem, Workspace, WorkspaceEntry, WorkspaceRepository, sort_tree,
};
pub use reimport::{
    ImportRecord, ImportRecords, RecordedRequest, ReimportAction, ReimportEntry, ReimportPlan,
    apply_reimport, plan_reimport, request_hash,
};
pub use render::{
    AwsV4Credentials, RenderOutcome, RenderedBody, RenderedPart, RenderedRequest, TransportAuth,
    render_request,
};
pub use report::{
    ReportFormat, ReportMeta, assertion_text, duration, html_report, json_report, junit_report,
};
pub use runner::{
    RunEvent, RunOptions, RunReport, RunSummary, Selection, Skipped, StepResult, run_collection,
    run_selection,
};
pub use scripts::{
    ConsoleLevel, ConsoleLine, Script, ScriptEngine, ScriptError, ScriptLimits, ScriptOutcome,
};
pub use use_cases::{
    SendOptions, SendOutcome, SendPorts, load_request, open_collection, resolve_request_variables,
    save_request, send_request,
};
pub use websocket::{WsHandler, WsOutbox, WsTransport, connect_request};
