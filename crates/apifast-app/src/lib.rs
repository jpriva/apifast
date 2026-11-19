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
mod functions;
mod history;
mod host;
pub mod memory;
mod oauth2;
mod organize;
mod ports;
mod render;
mod report;
mod runner;
mod url_encoding;
mod use_cases;

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
pub use history::{
    HIDDEN, HistoryEntry, HistoryLimits, HistoryOutcome, HistoryStore, hide_query, within_limits,
};
pub use host::AppHost;
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
pub use use_cases::{
    SendOptions, SendOutcome, SendPorts, load_request, open_collection, resolve_request_variables,
    save_request, send_request,
};
