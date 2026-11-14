//! Entidades, value objects y reglas de herencia de apifast, sin entrada/salida.
//!
//! Los textos de URL, headers, bodies y auth se guardan **sin renderizar**: pueden
//! contener plantillas como `{{host}}`. El renderizado ocurre después de resolver
//! la herencia, en `apifast-template`.
//!
//! El flujo es:
//!
//! 1. Se construye una [`Chain`] con la colección, sus carpetas y la request.
//! 2. [`resolve`] aplica la herencia y devuelve un [`ResolvedRequest`], con la
//!    procedencia ([`Origin`]) de cada header, de la auth y de cada ajuste.
//! 3. [`resolve_variables`] combina las variables de todos los ámbitos según su
//!    precedencia, también con procedencia.

mod auth;
mod body;
mod collection;
mod environment;
mod origin;
mod request;
mod resolve;
mod setting;
mod settings;
mod variable;

pub use auth::{ApiKeyAuth, ApiKeyPlacement, Auth, AuthScheme, BasicAuth, BearerAuth};
pub use body::{Body, FormField, MultipartPart, MultipartValue};
pub use collection::{Collection, CollectionId, Folder, RequestDefaults};
pub use environment::{Environment, EnvironmentError, environment_chain};
pub use origin::Origin;
pub use request::{Header, HttpRequest, Method, Param, ParamKind, Request};
pub use resolve::{Chain, ResolvedRequest, resolve};
pub use setting::{Resolved, Setting};
pub use settings::{ResolvedSettings, Settings};
pub use variable::{
    ResolvedVariable, ResolvedVariables, ValueKind, Variable, VariableValue, resolve_variables,
};
