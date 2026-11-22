//! A GraphQL request's schema (docs/07, F7): asked of the server with the
//! introspection query, sent as the request itself would be (its
//! environment, inherited headers, auth and settings), and kept in a local
//! cache so it is there the next time without asking again.

use crate::Invalid;
use apifast_domain::{Body, GraphQlSchema, HttpRequest, INTROSPECTION_QUERY, Json, Method};

use crate::{
    AppError, CancelToken, CollectionRepository, EnvironmentRepository, HttpTransport, RequestPath,
    ResponseStore, SendOptions, SendPorts, StoreError, send_request,
};

/// The largest introspection answer read, in bytes: a big public schema
/// (GitHub's) is a few megabytes.
const MAX_SCHEMA_BYTES: u64 = 64 * 1024 * 1024;

/// Where schemas are kept between sessions, by [`schema_key`].
pub trait SchemaCache {
    /// The introspection answer kept for `key`, if any.
    ///
    /// # Errors
    ///
    /// If it cannot be read.
    fn cached_schema(&self, key: &str) -> Result<Option<String>, StoreError>;

    /// Keeps `answer` for `key`, in place of the one kept.
    ///
    /// # Errors
    ///
    /// If it cannot be written.
    fn save_schema(&self, key: &str, answer: &str) -> Result<(), StoreError>;
}

/// What a schema is kept by: the request's URL as written and the
/// environment, as each environment may point to another server.
pub fn schema_key(url: &str, environment: Option<&str>) -> String {
    format!("{}\n{url}", environment.unwrap_or_default())
}

/// The schema kept for `key`, if any. One that no longer reads as a
/// schema is as good as none.
///
/// # Errors
///
/// If the cache cannot be read.
pub fn cached_schema(
    cache: &dyn SchemaCache,
    key: &str,
) -> Result<Option<GraphQlSchema>, AppError> {
    let Some(answer) = cache.cached_schema(key)? else {
        return Ok(None);
    };
    Ok(Json::parse(&answer)
        .ok()
        .and_then(|json| GraphQlSchema::from_introspection(&json).ok()))
}

/// Asks the server of the request at `path` (or of `options.draft`) for its
/// schema, and keeps the answer in `cache` by `key`.
///
/// # Errors
///
/// If the request cannot be sent, the server does not answer 2xx, or the
/// answer is not an introspection result.
pub async fn fetch_schema<C, E, T, S>(
    ports: SendPorts<'_, C, E, T, S>,
    (path, options): (&RequestPath, &SendOptions),
    (cache, key): (&(dyn SchemaCache + Sync), &str),
    cancel: &CancelToken,
) -> Result<GraphQlSchema, AppError>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
{
    let request = match &options.draft {
        Some(draft) => draft.clone(),
        None => ports.collections.request(path)?.request,
    };
    let options = SendOptions {
        draft: Some(introspection(request)),
        ..options.clone()
    };
    let outcome = send_request(ports, path, &options, cancel).await?;
    if !(200..300).contains(&outcome.head.status) {
        return Err(Invalid::IntrospectionStatus(outcome.head.status).into());
    }
    if outcome.body.size > MAX_SCHEMA_BYTES {
        return Err(Invalid::SchemaTooLarge(MAX_SCHEMA_BYTES / 1024 / 1024).into());
    }
    let size = usize::try_from(outcome.body.size).unwrap_or(usize::MAX);
    let bytes = ports.responses.read(&outcome.body.id, 0, size)?;
    let answer = String::from_utf8_lossy(&bytes);
    let json =
        Json::parse(&answer).map_err(|e| AppError::from(Invalid::SchemaNotJson(e.to_string())))?;
    let schema =
        GraphQlSchema::from_introspection(&json).map_err(|e| AppError::from(Invalid::Schema(e)))?;
    cache.save_schema(key, &answer)?;
    Ok(schema)
}

/// `request` asking for the schema: the introspection query as its body,
/// with nothing to check on the answer.
fn introspection(request: HttpRequest) -> HttpRequest {
    HttpRequest {
        method: Method::Post,
        body: Body::GraphQl {
            query: INTROSPECTION_QUERY.to_owned(),
            variables: String::new(),
        },
        assertions: Vec::new(),
        captures: Vec::new(),
        ..request
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct Cache(Mutex<BTreeMap<String, String>>);

    impl SchemaCache for Cache {
        fn cached_schema(&self, key: &str) -> Result<Option<String>, StoreError> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }

        fn save_schema(&self, key: &str, answer: &str) -> Result<(), StoreError> {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_owned(), answer.to_owned());
            Ok(())
        }
    }

    #[test]
    fn the_cache_gives_back_a_schema_and_ignores_a_broken_one() {
        let cache = Cache::default();
        let key = schema_key("{{host}}/graphql", Some("local"));
        assert_ne!(key, schema_key("{{host}}/graphql", None));
        assert_eq!(cached_schema(&cache, &key).unwrap(), None);
        let answer = r#"{"data": {"__schema": {"queryType": {"name": "Q"}, "types": []}}}"#;
        cache.save_schema(&key, answer).unwrap();
        let schema = cached_schema(&cache, &key).unwrap().unwrap();
        assert_eq!(schema.query.as_deref(), Some("Q"));
        cache.save_schema(&key, "not json").unwrap();
        assert_eq!(cached_schema(&cache, &key).unwrap(), None);
    }

    #[test]
    fn the_introspection_keeps_the_request_but_its_body_and_checks() {
        let mut request = HttpRequest::new("q", Method::Get, "{{host}}/graphql");
        request
            .headers
            .push(apifast_domain::Header::new("X-Key", "k"));
        request.assertions.push(apifast_domain::Assertion::new(
            "res.status",
            apifast_domain::Operator::Eq,
            Some("201"),
        ));
        let asked = introspection(request);
        assert_eq!(asked.method, Method::Post);
        assert_eq!(asked.headers.len(), 1);
        assert_eq!(asked.assertions, Vec::new());
        let Body::GraphQl { query, .. } = &asked.body else {
            panic!("{:?}", asked.body);
        };
        assert!(query.starts_with("query IntrospectionQuery"));
    }
}
