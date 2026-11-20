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
mod certificates;
mod collection;
mod cookie;
mod dns;
mod environment;
mod example;
mod graphql;
mod grpc;
mod json;
mod origin;
mod proxy;
mod request;
mod resolve;
mod secret;
mod setting;
mod settings;
mod sse;
mod status;
mod url_params;
mod variable;
mod ws_log;

pub use assertion::{Assertion, Capture, CaptureScope, Operator};
pub use auth::{
    ApiKeyAuth, ApiKeyPlacement, Auth, AuthScheme, AwsV4Auth, BasicAuth, BearerAuth,
    CredentialsPlacement, DigestAuth, NtlmAuth, OAuth2Auth, OAuth2Flow, OAuth2Parameter,
    OAuth2Request, ParameterPlacement, TokenPlacement,
};
pub use body::{
    Body, FormField, GrpcCall, GrpcMessage, GrpcMethodType, MultipartPart, MultipartValue,
    WsMessage, WsMessageKind,
};
pub use certificates::{ClientCertificate, ClientCertificateKind, TlsTrust};
pub use collection::{Collection, CollectionId, Folder, RequestDefaults};
pub use cookie::{
    Cookie, CookieJar, CookieTarget, SameSite, format_expiry, merge_cookie_header, parse_expiry,
    parse_set_cookie,
};
pub use dns::{DnsOverride, overridden_addresses};
pub use environment::{Environment, EnvironmentError, environment_chain};
pub use example::{Example, ExampleBody, ExampleRequest, ExampleResponse};
pub use graphql::{
    EnumValue, GraphQlSchema, INTROSPECTION_QUERY, InputValue, SchemaError, SchemaField,
    SchemaType, TypeKind, TypeRef,
};
pub use grpc::{
    GrpcCatalog, GrpcField, GrpcFieldKind, GrpcMethod, GrpcService, find_grpc_method,
    grpc_status_name, is_grpc_response,
};
pub use json::{Json, JsonError, MAX_DEPTH as MAX_JSON_DEPTH, PathError};
pub use origin::Origin;
pub use proxy::{CollectionProxy, ProxyChoice, ProxyConfig, ProxyMode, ProxyProtocol};
pub use request::{Header, HttpRequest, Method, Param, ParamKind, Request, RequestKind};
pub use resolve::{Chain, ResolvedRequest, resolve};
pub use secret::{MASK, Secret, mask};
pub use setting::{Resolved, Setting};
pub use settings::{ResolvedSettings, Settings};
pub use sse::{DEFAULT_EVENT, SseEvent, SseParser, is_event_stream, parse_events};
pub use status::{human_size, reason_phrase};
pub use url_params::{path_param_names, query_params, sync_params_from_url, sync_url_from_params};
pub use variable::{
    ResolvedVariable, ResolvedVariables, ValueKind, Variable, VariableValue, resolve_variables,
};
pub use ws_log::{
    WEBSOCKET_UPGRADE, WsData, WsEvent, WsLogEntry, WsLogParser, WsSide, is_websocket_handshake,
    parse_ws_log,
};
