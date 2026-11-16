//! Entities, value objects and inheritance rules of apifast, with no I/O.
//!
//! URL, header, body and auth texts are stored **unrendered**: they may contain
//! templates such as `{{host}}`. Rendering happens after inheritance is
//! resolved, in `apifast-template`.
//!
//! The flow is:
//!
//! 1. Build a [`Chain`] with the collection, its folders and the request.
//! 2. [`resolve`] applies inheritance and returns a [`ResolvedRequest`], with the
//!    [`Origin`] of every header, of the auth and of every setting.
//! 3. [`resolve_variables`] merges the variables of every scope by precedence,
//!    also recording where each one comes from.

mod assertion;
mod auth;
mod body;
mod collection;
mod environment;
mod json;
mod origin;
mod request;
mod resolve;
mod secret;
mod setting;
mod settings;
mod status;
mod url_params;
mod variable;

pub use assertion::{Assertion, Capture, CaptureScope, Operator};
pub use auth::{ApiKeyAuth, ApiKeyPlacement, Auth, AuthScheme, AwsV4Auth, BasicAuth, BearerAuth};
pub use body::{Body, FormField, MultipartPart, MultipartValue};
pub use collection::{Collection, CollectionId, Folder, RequestDefaults};
pub use environment::{Environment, EnvironmentError, environment_chain};
pub use json::{Json, JsonError, MAX_DEPTH as MAX_JSON_DEPTH, PathError};
pub use origin::Origin;
pub use request::{Header, HttpRequest, Method, Param, ParamKind, Request};
pub use resolve::{Chain, ResolvedRequest, resolve};
pub use secret::{MASK, Secret, mask};
pub use setting::{Resolved, Setting};
pub use settings::{ResolvedSettings, Settings};
pub use status::{human_size, reason_phrase};
pub use url_params::{path_param_names, query_params, sync_params_from_url, sync_url_from_params};
pub use variable::{
    ResolvedVariable, ResolvedVariables, ValueKind, Variable, VariableValue, resolve_variables,
};
