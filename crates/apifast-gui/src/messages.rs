//! The errors and notices of the layers under the interface, in its
//! language. Those layers say them in English (`Display`, which the command
//! line and the reports use); here each one is said again from what it
//! carries, with the templates of [`Errors`]. What the system or a library
//! says inside one (an I/O error, a parser's reason, a server's answer)
//! stays as they say it. An error added after this version falls back to
//! its English.

use std::fmt::Display;

use apifast_app::{
    AppError, AwsField, CallbackProblem, ExampleError, GitError, ImportNote, Invalid, OAuth2Field,
    RepositoryError, StoreError, TokenFailure, TransportError, WrongKind,
};
use apifast_domain::{EnvironmentError, SchemaError};
use apifast_import::{BrunoError, CurlError, InsomniaError, OpenApiError, PostmanError};
use apifast_query::QueryError;
use apifast_template::{Problem, RenderError};
use apifast_vault::{KeyStoreError, SealedError, ShareableKeyError, VaultError};

use crate::strings::{Errors, Strings, fill};

/// Something the interface can say in the language of `strings`.
pub trait Say {
    /// What it says, in that language.
    fn say(&self, strings: &Strings) -> String;
}

/// Shorthand for [`fill`].
fn put(template: &str, values: &[(&str, &dyn Display)]) -> String {
    fill(template, values)
}

impl Say for AppError {
    fn say(&self, s: &Strings) -> String {
        match self {
            Self::Repository(e) => e.say(s),
            Self::Environment(e) => e.say(s),
            Self::Render(e) => e.say(s),
            Self::Transport(e) => e.say(s),
            Self::Store(e) => e.say(s),
            Self::Git(e) => e.say(s),
            Self::Validation(e) => e.say(s),
            // A script's own words, and a skip's reason.
            Self::Script(text) | Self::Skipped(text) => text.clone(),
            _ => self.to_string(),
        }
    }
}

impl Say for RepositoryError {
    fn say(&self, s: &Strings) -> String {
        let e = &s.errors;
        match self {
            Self::NotFound(path) => put(e.not_found, &[("path", path)]),
            Self::Invalid { path, message } => {
                put(e.invalid_file, &[("path", path), ("message", message)])
            }
            Self::Unsupported { path, what } => {
                put(e.unsupported_file, &[("path", path), ("what", what)])
            }
            Self::Io { path, message } => {
                put(e.cannot_access, &[("path", path), ("message", message)])
            }
            _ => self.to_string(),
        }
    }
}

impl Say for TransportError {
    fn say(&self, s: &Strings) -> String {
        let e = &s.errors;
        match self {
            Self::InvalidUrl(why) => put(e.invalid_url, &[("why", why)]),
            Self::InvalidRequest(why) => put(e.invalid_request, &[("why", why)]),
            Self::Tls(why) => put(e.tls_failed, &[("why", why)]),
            Self::Protocol(why) => put(e.protocol_failed, &[("why", why)]),
            Self::TooManyRedirects(n) => s.count(
                &e.too_many_redirects,
                usize::try_from(*n).unwrap_or(usize::MAX),
                &[],
            ),
            Self::Timeout(ms) => put(e.timed_out, &[("ms", ms)]),
            Self::Cancelled => e.send_cancelled.to_owned(),
            // What the system said about the name or the connection.
            Self::Dns(why) | Self::Connect(why) | Self::Other(why) => why.clone(),
            _ => self.to_string(),
        }
    }
}

impl Say for StoreError {
    fn say(&self, s: &Strings) -> String {
        put(s.errors.response_store, &[("why", &self.0)])
    }
}

impl Say for EnvironmentError {
    fn say(&self, s: &Strings) -> String {
        let e = &s.errors;
        match self {
            Self::NotFound(name) => put(e.environment_not_found, &[("name", name)]),
            Self::Cycle(chain) => put(e.environment_cycle, &[("chain", &chain.join(" -> "))]),
        }
    }
}

impl Say for RenderError {
    fn say(&self, s: &Strings) -> String {
        self.0.say(s)
    }
}

impl Say for Problem {
    fn say(&self, s: &Strings) -> String {
        let e = &s.errors;
        match self {
            Self::MissingVariable(name) => put(e.variable_undefined, &[("name", name)]),
            Self::SecretUnavailable(name) => put(e.secret_unavailable, &[("name", name)]),
            Self::Cycle(chain) => put(e.variables_cycle, &[("chain", &chain.join(" -> "))]),
            Self::TooDeep(name) => put(
                e.variable_too_deep,
                &[("name", name), ("max", &apifast_template::MAX_DEPTH)],
            ),
            Self::UnknownDynamic(name) => put(e.dynamic_unknown, &[("name", name)]),
            Self::MissingProcessEnv(name) => put(e.process_env_missing, &[("name", name)]),
            Self::MissingPrompt(name) => put(e.prompt_missing, &[("name", name)]),
            Self::Function { name, message } => {
                put(e.function_failed, &[("name", name), ("message", message)])
            }
            _ => self.to_string(),
        }
    }
}

impl Say for GitError {
    fn say(&self, s: &Strings) -> String {
        let e = &s.errors;
        match self {
            Self::NoIdentity => e.git_no_identity.to_owned(),
            Self::NoBranch(branch) => put(e.git_no_branch, &[("branch", branch)]),
            Self::Uncommitted(files) => put(e.git_uncommitted, &[("files", &files.join(", "))]),
            Self::Untracked(files) => put(e.git_untracked, &[("files", &files.join(", "))]),
            Self::Failed(why) => put(e.git_failed, &[("why", why)]),
            _ => self.to_string(),
        }
    }
}

impl Say for Invalid {
    fn say(&self, s: &Strings) -> String {
        let e = &s.errors;
        match self {
            Self::EnvironmentNameEmpty => e.environment_name_empty.to_owned(),
            Self::EnvironmentExists(name) => put(e.environment_exists, &[("name", name)]),
            Self::EnvironmentExtended { name, children } => put(
                e.environment_extended,
                &[("name", name), ("children", &children.join(", "))],
            ),
            Self::FolderIntoItself => e.folder_into_itself.to_owned(),
            Self::ExistsInFolder(name) => put(e.exists_in_folder, &[("name", name)]),
            Self::NameEmpty => e.name_empty.to_owned(),
            Self::RequestNameEmpty => e.request_name_empty.to_owned(),
            Self::NotAFolder(path) => put(e.not_a_folder, &[("path", path)]),
            Self::IntrospectionStatus(status) => put(e.introspection_status, &[("status", status)]),
            Self::SchemaTooLarge(mb) => put(e.schema_too_large, &[("mb", mb)]),
            Self::SchemaNotJson(why) => put(e.schema_not_json, &[("why", why)]),
            Self::Schema(error) => error.say(s),
            Self::CommitMessageEmpty => e.commit_message_empty.to_owned(),
            Self::CommitNothingChosen => e.commit_nothing_chosen.to_owned(),
            Self::AwsMissing(field) => put(e.aws_missing, &[("field", &aws_field(*field, e))]),
            Self::OAuth2RenderedAlone => e.oauth2_rendered_alone.to_owned(),
            Self::AuthUnsupported(kind) => put(e.auth_scheme_unsupported, &[("kind", kind)]),
            Self::AuthNotYet => e.auth_scheme_not_yet.to_owned(),
            Self::BodyNotYet => e.body_kind_not_yet.to_owned(),
            Self::GraphQlVariablesNotJson(why) => {
                put(e.graphql_variables_not_json, &[("why", why)])
            }
            Self::GraphQlVariablesNotObject => e.graphql_variables_not_object.to_owned(),
            Self::OAuth2NoTokenUrl => e.oauth2_no_token_url.to_owned(),
            Self::OAuth2Missing(field) => {
                put(e.oauth2_missing, &[("field", &oauth2_field(*field, e))])
            }
            Self::TokenRequestFailed { status, reason } => put(
                e.token_request_failed,
                &[("status", status), ("reason", &token_failure(reason, e))],
            ),
            Self::NoAccessToken => e.no_access_token.to_owned(),
            Self::NoIdToken => e.no_id_token.to_owned(),
            Self::ImplicitGrant => e.implicit_grant.to_owned(),
            Self::NoBrowser => e.no_browser.to_owned(),
            Self::AuthorizationRefused { error, description } => put(
                e.authorization_refused,
                &[("error", &with_description(error, description.as_deref()))],
            ),
            Self::AuthorizationState => e.authorization_state.to_owned(),
            Self::AuthorizationNoCode => e.authorization_no_code.to_owned(),
            Self::AuthorizationTimedOut => e.authorization_timed_out.to_owned(),
            Self::NoRefreshToken => e.no_refresh_token.to_owned(),
            Self::TokenExpired => e.token_expired.to_owned(),
            Self::NoToken => e.no_token.to_owned(),
            Self::NotOAuth2 => e.not_oauth2.to_owned(),
            Self::Callback { url, problem } => put(
                e.callback_url,
                &[("url", url), ("problem", &callback_problem(problem, e))],
            ),
            Self::BrowserFailed { error, url } => {
                put(e.browser_failed, &[("why", error), ("url", url)])
            }
            Self::CallbackFailed(why) => put(e.callback_failed, &[("why", why)]),
            Self::DependencyCycle(chain) => {
                put(e.dependency_cycle, &[("chain", &chain.join(" → "))])
            }
            Self::TooManyChained => e.too_many_chained.to_owned(),
            Self::Dependency { dependency, error } => put(
                e.dependency_failed,
                &[("dependency", dependency), ("error", &error.say(s))],
            ),
            Self::WrongKind { path, kind } => put(
                e.wrong_kind,
                &[("path", path), ("kind", &wrong_kind(*kind, e))],
            ),
            _ => self.to_string(),
        }
    }
}

fn aws_field(field: AwsField, e: &Errors) -> &'static str {
    match field {
        AwsField::AccessKeyId => e.aws_access_key_id,
        AwsField::SecretAccessKey => e.aws_secret_access_key,
        AwsField::Service => e.aws_service,
        AwsField::Region => e.aws_region,
    }
}

fn oauth2_field(field: OAuth2Field, e: &Errors) -> &'static str {
    match field {
        OAuth2Field::AuthorizationUrl => e.oauth2_field_authorization_url,
        OAuth2Field::CallbackUrl => e.oauth2_field_callback_url,
    }
}

/// A server's error and its description, as it gave them.
fn with_description(error: &str, description: Option<&str>) -> String {
    description.map_or_else(|| error.to_owned(), |d| format!("{error}: {d}"))
}

fn token_failure(reason: &TokenFailure, e: &Errors) -> String {
    match reason {
        TokenFailure::Error { error, description } => {
            with_description(error, description.as_deref())
        }
        TokenFailure::NotJson => e.token_answer_not_json.to_owned(),
        TokenFailure::Status(status) => put(e.token_status, &[("status", status)]),
    }
}

fn callback_problem(problem: &CallbackProblem, e: &Errors) -> String {
    match problem {
        CallbackProblem::NotUrl => e.callback_not_url.to_owned(),
        CallbackProblem::NotLocal => e.callback_not_local.to_owned(),
        CallbackProblem::CannotListen(why) => put(e.callback_cannot_listen, &[("why", why)]),
    }
}

fn wrong_kind(kind: WrongKind, e: &Errors) -> &'static str {
    match kind {
        WrongKind::IsWebSocket => e.is_websocket,
        WrongKind::NotWebSocket => e.not_websocket,
        WrongKind::IsGrpc => e.is_grpc,
        WrongKind::NotGrpc => e.not_grpc,
    }
}

impl Say for SchemaError {
    fn say(&self, s: &Strings) -> String {
        match self {
            Self::Refused(why) => put(s.errors.introspection_refused, &[("why", why)]),
            Self::NotIntrospection => s.errors.not_introspection.to_owned(),
        }
    }
}

impl Say for KeyStoreError {
    fn say(&self, s: &Strings) -> String {
        let e = &s.errors;
        match self {
            Self::Keyring(why) => put(e.keyring_failed, &[("why", why)]),
            Self::File { path, message } => put(
                e.cannot_use,
                &[("path", &path.display()), ("message", message)],
            ),
            Self::Damaged => e.master_key_damaged.to_owned(),
            Self::Collection(why) => why.clone(),
            Self::NoCollectionKey => e.collection_shares_already.to_owned(),
            Self::SharedWithoutKey(name) => put(e.shared_without_key, &[("name", name)]),
            Self::WrongCollectionKey => e.wrong_collection_key.to_owned(),
            Self::Shareable(error) => error.say(s),
            Self::Vault(error) => error.say(s),
        }
    }
}

impl Say for ShareableKeyError {
    fn say(&self, s: &Strings) -> String {
        match self {
            Self::Format => s.errors.key_format.to_owned(),
            Self::Check => s.errors.key_check.to_owned(),
        }
    }
}

impl Say for VaultError {
    fn say(&self, s: &Strings) -> String {
        let e = &s.errors;
        match self {
            Self::NotAnEnvelope => e.not_an_envelope.to_owned(),
            Self::UnknownVersion(version) => put(e.newer_envelope, &[("version", version)]),
            Self::Rejected => e.decryption_rejected.to_owned(),
            Self::Randomness(why) => put(e.no_randomness, &[("why", why)]),
        }
    }
}

impl Say for SealedError {
    fn say(&self, s: &Strings) -> String {
        put(
            s.errors.cannot_use,
            &[("path", &self.path.display()), ("message", &self.message)],
        )
    }
}

impl Say for BrunoError {
    fn say(&self, s: &Strings) -> String {
        match self {
            Self::NotCollection(path) => put(s.errors.bruno_not_collection, &[("path", path)]),
            Self::Unreadable(path, why) => {
                put(s.errors.cannot_read, &[("path", path), ("why", why)])
            }
            _ => self.to_string(),
        }
    }
}

impl Say for InsomniaError {
    fn say(&self, s: &Strings) -> String {
        match self {
            Self::NotInsomnia => s.errors.not_insomnia.to_owned(),
            _ => self.to_string(),
        }
    }
}

impl Say for OpenApiError {
    fn say(&self, s: &Strings) -> String {
        match self {
            Self::NotOpenApi => s.errors.not_openapi.to_owned(),
            Self::Swagger2 => s.errors.swagger2.to_owned(),
            _ => self.to_string(),
        }
    }
}

impl Say for PostmanError {
    fn say(&self, s: &Strings) -> String {
        match self {
            Self::NotJson(why) => put(s.errors.not_json, &[("why", why)]),
            Self::NotCollection => s.errors.not_postman_collection.to_owned(),
            Self::NotEnvironment => s.errors.not_postman_environment.to_owned(),
            _ => self.to_string(),
        }
    }
}

impl Say for apifast_import::ExportError {
    fn say(&self, s: &Strings) -> String {
        match self {
            Self::NoEnvironment { name, known } => put(
                s.errors.export_no_environment,
                &[("name", name), ("known", known)],
            ),
            Self::Write { path, source } => put(
                s.errors.cannot_write,
                &[("path", &path.display()), ("why", source)],
            ),
            _ => self.to_string(),
        }
    }
}

impl Say for CurlError {
    fn say(&self, s: &Strings) -> String {
        let e = &s.errors;
        match self {
            Self::NotCurl => e.not_curl.to_owned(),
            Self::Unclosed(quote) => put(e.curl_unclosed, &[("quote", quote)]),
            Self::NoUrl => e.curl_no_url.to_owned(),
            Self::MissingValue(option) => put(e.curl_missing_value, &[("option", option)]),
            _ => self.to_string(),
        }
    }
}

impl Say for QueryError {
    fn say(&self, s: &Strings) -> String {
        let e = &s.errors;
        match self {
            Self::Syntax {
                language,
                expression,
                reason,
                at,
            } => put(
                e.query_syntax,
                &[
                    ("language", language),
                    ("expression", expression),
                    ("reason", reason),
                    ("at", at),
                ],
            ),
            Self::NotJson => e.body_not_json.to_owned(),
            Self::NotXml(why) => put(e.body_not_xml, &[("why", why)]),
            Self::TooExpensive => put(
                e.query_too_expensive,
                &[("max", &apifast_query::MAX_VISITS)],
            ),
            Self::Regex(expression) => put(e.bad_regex, &[("expression", expression)]),
        }
    }
}

impl Say for ExampleError {
    fn say(&self, s: &Strings) -> String {
        match self {
            Self::TooLarge => put(
                s.errors.example_too_large,
                &[("kb", &(apifast_app::EXAMPLE_LIMIT / 1024))],
            ),
            Self::Binary => s.errors.example_binary.to_owned(),
        }
    }
}

impl Say for ImportNote {
    fn say(&self, s: &Strings) -> String {
        notes::say(self, s)
    }
}

/// What an import or an export did not keep, apart for its length.
mod notes {
    use super::{ImportNote, Say, Strings, put};

    pub(super) fn say(note: &ImportNote, s: &Strings) -> String {
        type N = ImportNote;
        let e = &s.errors;
        let kind = |template: &str, kind: &String| put(template, &[("kind", kind)]);
        match note {
            N::SecretNotImported => e.note_secret_not_imported.to_owned(),
            N::SecretsApart(n) => s.count(&e.note_secrets_apart, *n, &[]),
            N::BodyUnsupported(mode) => put(e.note_body_unsupported, &[("mode", mode)]),
            N::SettingNotKept(key) => put(e.note_setting_not_kept, &[("key", key)]),
            N::ScriptNotImported(event) => put(e.note_script_not_imported, &[("event", event)]),
            N::ScriptLinesCommented { script, lines } => s.count(
                &e.note_script_lines_commented,
                *lines,
                &[("script", script)],
            ),
            N::ScriptsNotImported => e.note_scripts_not_imported.to_owned(),
            N::UnknownDynamic(name) => put(e.note_unknown_dynamic, &[("name", name)]),
            N::Unparsable { line, message } => {
                put(e.note_unparsable, &[("line", line), ("message", message)])
            }
            N::RequestKindUnsupported(k) => kind(e.note_request_kind_unsupported, k),
            N::ResourceUnsupported(k) => kind(e.note_resource_unsupported, k),
            N::AuthUnsupported(k) => kind(e.note_auth_unsupported, k),
            N::GrantUnsupported(grant) => put(e.note_grant_unsupported, &[("grant", grant)]),
            N::FlowUnsupported(flow) => put(e.note_flow_unsupported, &[("flow", flow)]),
            N::HttpSecurityUnsupported(scheme) => {
                put(e.note_http_security_unsupported, &[("scheme", scheme)])
            }
            N::ApiKeyInUnsupported(place) => {
                put(e.note_api_key_in_unsupported, &[("place", place)])
            }
            N::SecurityUnsupported(k) => kind(e.note_security_unsupported, k),
            N::SecuritySchemeUndefined(name) => {
                put(e.note_security_scheme_undefined, &[("name", name)])
            }
            N::OAuth2WithoutFlows => e.note_oauth2_without_flows.to_owned(),
            N::NoServers => e.note_no_servers.to_owned(),
            N::RefNotFollowed => e.note_ref_not_followed.to_owned(),
            N::ParameterRefNotFollowed => e.note_parameter_ref_not_followed.to_owned(),
            N::ParameterNotImported { location, name } => put(
                e.note_parameter_not_imported,
                &[("location", location), ("name", name)],
            ),
            N::XmlBodyEmpty => e.note_xml_body_empty.to_owned(),
            N::TemplateTags => e.note_template_tags.to_owned(),
            N::GraphQlAsJson => e.note_graphql_as_json.to_owned(),
            N::EnvironmentsIgnored => e.note_environments_ignored.to_owned(),
            N::FolderVariableClash(name) => put(e.note_folder_variable_clash, &[("name", name)]),
            N::EnvironmentAlone(why) => put(e.note_environment_alone, &[("why", &why.say(s))]),
            N::EnvironmentMerged => e.note_environment_merged.to_owned(),
            N::SecretWithoutValue => e.note_secret_without_value.to_owned(),
            N::FolderVariablesMoved => e.note_folder_variables_moved.to_owned(),
            N::HeadersCopied => e.note_headers_copied.to_owned(),
            N::ScriptsNotExported => e.note_scripts_not_exported.to_owned(),
            N::AssertionsNotExported => e.note_assertions_not_exported.to_owned(),
            N::CapturesNotExported => e.note_captures_not_exported.to_owned(),
            N::VariablesNotExported => e.note_variables_not_exported.to_owned(),
            N::TagsNotExported => e.note_tags_not_exported.to_owned(),
            N::SparqlAsText => e.note_sparql_as_text.to_owned(),
            N::BodyNotExported => e.note_body_not_exported.to_owned(),
            N::AwsProfileNotExported => e.note_aws_profile_not_exported.to_owned(),
            N::AuthNotExportable(k) => kind(e.note_auth_not_exportable, k),
            N::AuthNotExported => e.note_auth_not_exported.to_owned(),
            N::TimeoutNotExported => e.note_timeout_not_exported.to_owned(),
            N::RequestNotExported(k) => kind(e.note_request_not_exported, k),
            N::Unreadable(why) => put(e.note_unreadable, &[("why", why)]),
            _ => note.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use apifast_app::{AppError, ImportNote, Invalid};

    use super::*;
    use crate::strings::{EN, language};

    /// In English, the interface says what `Display` says.
    fn same(error: &(impl Say + Display)) {
        assert_eq!(error.say(&EN), error.to_string());
    }

    fn text(s: &str) -> String {
        s.to_owned()
    }

    #[test]
    fn in_english_every_error_says_what_its_display_says() {
        for error in [
            AppError::from(RepositoryError::NotFound(text("a.yml"))),
            RepositoryError::Invalid {
                path: text("a.yml"),
                message: text("bad"),
            }
            .into(),
            RepositoryError::Unsupported {
                path: text("a.yml"),
                what: text("`x` requests"),
            }
            .into(),
            RepositoryError::Io {
                path: text("a.yml"),
                message: text("denied"),
            }
            .into(),
            TransportError::InvalidUrl(text("x")).into(),
            TransportError::InvalidRequest(text("x")).into(),
            TransportError::Tls(text("x")).into(),
            TransportError::Protocol(text("x")).into(),
            TransportError::TooManyRedirects(5).into(),
            TransportError::Timeout(30).into(),
            TransportError::Cancelled.into(),
            TransportError::Dns(text("no such host")).into(),
            StoreError(text("full")).into(),
            EnvironmentError::NotFound(text("dev")).into(),
            EnvironmentError::Cycle(vec![text("a"), text("b")]).into(),
            AppError::Render(Problem::MissingVariable(text("host")).into()),
            AppError::Render(Problem::SecretUnavailable(text("k")).into()),
            AppError::Render(Problem::Cycle(vec![text("a"), text("b")]).into()),
            AppError::Render(Problem::TooDeep(text("a")).into()),
            AppError::Render(Problem::UnknownDynamic(text("x")).into()),
            AppError::Render(Problem::MissingProcessEnv(text("HOME")).into()),
            AppError::Render(Problem::MissingPrompt(text("p")).into()),
            AppError::Render(
                Problem::Function {
                    name: text("f"),
                    message: text("m"),
                }
                .into(),
            ),
            GitError::NoIdentity.into(),
            GitError::NoBranch(text("main")).into(),
            GitError::Uncommitted(vec![text("a"), text("b")]).into(),
            GitError::Untracked(vec![text("a")]).into(),
            GitError::Failed(text("x")).into(),
            AppError::Script(text("the script failed")),
        ] {
            same(&error);
        }
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "one line per variant")]
    fn in_english_every_invalid_says_what_its_display_says() {
        use apifast_app::{AwsField, CallbackProblem, OAuth2Field, TokenFailure, WrongKind};
        let invalid = [
            Invalid::EnvironmentNameEmpty,
            Invalid::EnvironmentExists(text("dev")),
            Invalid::EnvironmentExtended {
                name: text("base"),
                children: vec![text("a"), text("b")],
            },
            Invalid::FolderIntoItself,
            Invalid::ExistsInFolder(text("a.yml")),
            Invalid::NameEmpty,
            Invalid::RequestNameEmpty,
            Invalid::NotAFolder(text("x")),
            Invalid::IntrospectionStatus(500),
            Invalid::SchemaTooLarge(16),
            Invalid::SchemaNotJson(text("x")),
            Invalid::Schema(SchemaError::Refused(text("off"))),
            Invalid::Schema(SchemaError::NotIntrospection),
            Invalid::CommitMessageEmpty,
            Invalid::CommitNothingChosen,
            Invalid::AwsMissing(AwsField::AccessKeyId),
            Invalid::AwsMissing(AwsField::SecretAccessKey),
            Invalid::AwsMissing(AwsField::Service),
            Invalid::AwsMissing(AwsField::Region),
            Invalid::OAuth2RenderedAlone,
            Invalid::AuthUnsupported(text("hawk")),
            Invalid::AuthNotYet,
            Invalid::BodyNotYet,
            Invalid::GraphQlVariablesNotJson(text("x")),
            Invalid::GraphQlVariablesNotObject,
            Invalid::OAuth2NoTokenUrl,
            Invalid::OAuth2Missing(OAuth2Field::AuthorizationUrl),
            Invalid::OAuth2Missing(OAuth2Field::CallbackUrl),
            Invalid::TokenRequestFailed {
                status: 400,
                reason: TokenFailure::Error {
                    error: text("invalid_client"),
                    description: Some(text("bad")),
                },
            },
            Invalid::TokenRequestFailed {
                status: 500,
                reason: TokenFailure::NotJson,
            },
            Invalid::TokenRequestFailed {
                status: 500,
                reason: TokenFailure::Status(500),
            },
            Invalid::NoAccessToken,
            Invalid::NoIdToken,
            Invalid::ImplicitGrant,
            Invalid::NoBrowser,
            Invalid::AuthorizationRefused {
                error: text("denied"),
                description: None,
            },
            Invalid::AuthorizationRefused {
                error: text("denied"),
                description: Some(text("no")),
            },
            Invalid::AuthorizationState,
            Invalid::AuthorizationNoCode,
            Invalid::AuthorizationTimedOut,
            Invalid::NoRefreshToken,
            Invalid::TokenExpired,
            Invalid::NoToken,
            Invalid::NotOAuth2,
            Invalid::Callback {
                url: text("x"),
                problem: CallbackProblem::NotUrl,
            },
            Invalid::Callback {
                url: text("x"),
                problem: CallbackProblem::NotLocal,
            },
            Invalid::Callback {
                url: text("x"),
                problem: CallbackProblem::CannotListen(text("busy")),
            },
            Invalid::BrowserFailed {
                error: text("none"),
                url: text("https://a"),
            },
            Invalid::CallbackFailed(text("x")),
            Invalid::DependencyCycle(vec![text("a.yml"), text("b.yml")]),
            Invalid::TooManyChained,
            Invalid::Dependency {
                dependency: text("a.yml"),
                error: Box::new(TransportError::Timeout(5).into()),
            },
            Invalid::WrongKind {
                path: text("a.yml"),
                kind: WrongKind::IsWebSocket,
            },
            Invalid::WrongKind {
                path: text("a.yml"),
                kind: WrongKind::NotWebSocket,
            },
            Invalid::WrongKind {
                path: text("a.yml"),
                kind: WrongKind::IsGrpc,
            },
            Invalid::WrongKind {
                path: text("a.yml"),
                kind: WrongKind::NotGrpc,
            },
        ];
        for error in invalid {
            same(&error);
        }
    }

    #[test]
    fn in_english_the_vault_the_importers_and_the_filters_say_what_they_display() {
        for error in [
            KeyStoreError::Keyring(text("x")),
            KeyStoreError::File {
                path: "/k".into(),
                message: text("x"),
            },
            KeyStoreError::Damaged,
            KeyStoreError::NoCollectionKey,
            KeyStoreError::SharedWithoutKey(text("token")),
            KeyStoreError::WrongCollectionKey,
            ShareableKeyError::Format.into(),
            ShareableKeyError::Check.into(),
            VaultError::NotAnEnvelope.into(),
            VaultError::UnknownVersion(text("v9")).into(),
            VaultError::Rejected.into(),
            VaultError::Randomness(text("x")).into(),
        ] {
            same(&error);
        }
        same(&BrunoError::NotCollection(text("/c")));
        same(&BrunoError::Unreadable(text("/c"), text("x")));
        same(&InsomniaError::NotInsomnia);
        same(&OpenApiError::NotOpenApi);
        same(&OpenApiError::Swagger2);
        same(&PostmanError::NotJson(text("x")));
        same(&PostmanError::NotCollection);
        same(&PostmanError::NotEnvironment);
        same(&CurlError::NotCurl);
        same(&CurlError::Unclosed('\''));
        same(&CurlError::NoUrl);
        same(&CurlError::MissingValue(text("-H")));
        same(&QueryError::NotJson);
        same(&QueryError::NotXml(text("x")));
        same(&QueryError::TooExpensive);
        same(&QueryError::Regex(text("(")));
        same(&ExampleError::TooLarge);
        same(&ExampleError::Binary);
    }

    #[test]
    fn in_english_every_import_note_says_what_its_display_says() {
        type N = ImportNote;
        for note in [
            N::SecretNotImported,
            N::SecretsApart(3),
            N::BodyUnsupported(text("x")),
            N::SettingNotKept(text("x")),
            N::ScriptNotImported(text("test")),
            N::ScriptLinesCommented {
                script: text("test"),
                lines: 1,
            },
            N::ScriptLinesCommented {
                script: text("test"),
                lines: 2,
            },
            N::ScriptsNotImported,
            N::UnknownDynamic(text("guid")),
            N::Unparsable {
                line: 3,
                message: text("x"),
            },
            N::RequestKindUnsupported(text("x")),
            N::ResourceUnsupported(text("x")),
            N::AuthUnsupported(text("x")),
            N::GrantUnsupported(text("x")),
            N::FlowUnsupported(text("x")),
            N::HttpSecurityUnsupported(text("x")),
            N::ApiKeyInUnsupported(text("x")),
            N::SecurityUnsupported(text("x")),
            N::SecuritySchemeUndefined(text("x")),
            N::OAuth2WithoutFlows,
            N::NoServers,
            N::RefNotFollowed,
            N::ParameterRefNotFollowed,
            N::ParameterNotImported {
                location: text("cookie"),
                name: text("id"),
            },
            N::XmlBodyEmpty,
            N::TemplateTags,
            N::GraphQlAsJson,
            N::EnvironmentsIgnored,
            N::FolderVariableClash(text("x")),
            N::EnvironmentAlone(EnvironmentError::NotFound(text("base"))),
            N::EnvironmentMerged,
            N::SecretWithoutValue,
            N::FolderVariablesMoved,
            N::HeadersCopied,
            N::ScriptsNotExported,
            N::AssertionsNotExported,
            N::CapturesNotExported,
            N::VariablesNotExported,
            N::TagsNotExported,
            N::SparqlAsText,
            N::BodyNotExported,
            N::AwsProfileNotExported,
            N::AuthNotExportable(text("x")),
            N::AuthNotExported,
            N::TimeoutNotExported,
            N::RequestNotExported(text("grpc")),
            N::Unreadable(text("x")),
        ] {
            same(&note);
        }
    }

    #[test]
    fn spanish_says_them_in_spanish_with_their_data() {
        let es = language("es").map_or(&EN, |l| l.strings);
        let error = AppError::from(Invalid::EnvironmentExists(text("dev")));
        assert_eq!(error.say(es), "ya existe un entorno llamado `dev`");
        let error = AppError::from(TransportError::TooManyRedirects(1));
        assert_eq!(error.say(es), "se detuvo tras 1 redirección");
        let nested = Invalid::Dependency {
            dependency: text("a.yml"),
            error: Box::new(GitError::NoBranch(text("x")).into()),
        };
        assert_eq!(
            nested.say(es),
            "`a.yml`, usada por response(): git: no existe la rama `x`"
        );
        // What the system says stays as it says it.
        assert_eq!(
            AppError::from(TransportError::Dns(text("no such host"))).say(es),
            "no such host"
        );
    }
}
