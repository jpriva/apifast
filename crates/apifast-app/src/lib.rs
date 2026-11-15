//! Use cases and ports (traits) of apifast.
//!
//! The use cases ([`send_request`], [`save_request`], ...) only talk to the
//! outside world through the ports in this crate. Adapters implement them;
//! [`memory`] has in-memory versions for tests.
//!
//! Sending goes through three types, each built only from the previous one:
//! a request file → [`apifast_domain::ResolvedRequest`] (inheritance applied)
//! → [`RenderedRequest`] (templates and auth applied) → the [`HttpTransport`].

mod error;
mod host;
pub mod memory;
mod organize;
mod ports;
mod render;
mod use_cases;

pub use error::{AppError, RepositoryError, StoreError, TransportError};
pub use host::AppHost;
pub use organize::{
    Placement, create_folder, create_request, delete_item, move_item, rename_item, slug,
};
pub use ports::{
    BodyWriter, CancelToken, Clock, CollectionRepository, CollectionTree, EnvironmentRepository,
    HttpTransport, ItemPath, LoadedRequest, NoPrompter, Prompter, Random, RequestPath,
    ResponseHead, ResponseId, ResponseStore, Session, SessionStore, SessionTab, StoredBody,
    TimelineEvent, TimelineKind, TreeItem, sort_tree,
};
pub use render::{RenderOutcome, RenderedBody, RenderedPart, RenderedRequest, render_request};
pub use use_cases::{
    SendOptions, SendOutcome, SendPorts, load_request, open_collection, resolve_request_variables,
    save_request, send_request,
};
