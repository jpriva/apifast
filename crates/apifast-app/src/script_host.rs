//! What the scripts of a send ask of the application while they run
//! (docs/08, F9): `bru.sendRequest`, a request as the script wrote it, and
//! `bru.runRequest`, a request of the collection with its scripts.

use apifast_domain::Method;

use crate::ports::{BodyWriter, CollectionRepository, TreeItem};
use crate::use_cases::{ChainContext, send_in_chain};
use crate::{
    CancelToken, EnvironmentRepository, HostFuture, HttpTransport, RanRequest, RenderedBody,
    RenderedRequest, RequestPath, ResponseHead, ResponseStore, ScriptHost, ScriptHttpRequest,
    ScriptHttpResponse, SendOptions, SendPorts, StoreError, StoredBody, reason_phrase,
};

/// The most of a body a script reads back: as for checks.
const BODY_LIMIT: usize = 10 << 20;

/// The host of the scripts of one send: its ports, its options, and the
/// requests it is part of, so a request that runs itself is an error.
pub(crate) struct SendHost<'s, 'p, C, E, T, S> {
    ports: &'s SendPorts<'p, C, E, T, S>,
    options: SendOptions,
    chain: Vec<RequestPath>,
    cancel: &'s CancelToken,
}

impl<'s, 'p, C, E, T, S> SendHost<'s, 'p, C, E, T, S> {
    /// The host of a send with `options`, within `chain`.
    pub(crate) fn new(
        ports: &'s SendPorts<'p, C, E, T, S>,
        options: &SendOptions,
        chain: Vec<RequestPath>,
        cancel: &'s CancelToken,
    ) -> Self {
        Self {
            ports,
            // A request it runs is the one saved, not the draft being sent.
            options: SendOptions {
                draft: None,
                ..options.clone()
            },
            chain,
            cancel,
        }
    }
}

impl<C, E, T, S> ScriptHost for SendHost<'_, '_, C, E, T, S>
where
    C: CollectionRepository + Sync,
    E: EnvironmentRepository + Sync,
    T: HttpTransport + Sync,
    S: ResponseStore + Sync,
    S::Writer: Send,
{
    fn send_request(&self, request: ScriptHttpRequest) -> HostFuture<'_, ScriptHttpResponse> {
        Box::pin(async move {
            let body = request.body.map_or(RenderedBody::None, RenderedBody::Text);
            let mut rendered = RenderedRequest::own(
                Method::parse(&request.method),
                request.url,
                request.headers,
                body,
                Vec::new(),
            );
            if let Some(ms) = request.timeout_ms {
                rendered = rendered.with_timeout_ms(ms);
            }
            let mut body = Collect::default();
            let head = (self.ports.transport)
                .send(&rendered, &mut body, self.cancel)
                .await
                .map_err(|e| e.to_string())?;
            Ok(response(&head, &body.0))
        })
    }

    fn run_request<'a>(&'a self, path: &'a str) -> HostFuture<'a, RanRequest> {
        Box::pin(async move {
            let path = find(self.ports.collections, path)?;
            let mut context = ChainContext::following(&self.chain);
            let outcome =
                send_in_chain(self.ports, &path, &self.options, self.cancel, &mut context)
                    .await
                    .map_err(|e| e.to_string())?;
            let size = usize::try_from(outcome.body.size).unwrap_or(usize::MAX);
            let bytes = (self.ports.responses)
                .read(&outcome.body.id, 0, size.min(BODY_LIMIT))
                .map_err(|e| e.to_string())?;
            Ok(RanRequest {
                response: response(&outcome.head, &bytes),
                variables: outcome.scripts.variables,
            })
        })
    }
}

fn response(head: &ResponseHead, body: &[u8]) -> ScriptHttpResponse {
    ScriptHttpResponse {
        status: head.status,
        status_text: reason_phrase(head.status).to_owned(),
        headers: head.headers.clone(),
        body: String::from_utf8_lossy(body).into_owned(),
        time_ms: head.elapsed_ms,
        url: head.url.clone(),
    }
}

/// The request at `path`, as Bruno names it: relative to the root, with
/// or without its extension. A name an importer changed (`runRequest-2`
/// saved as `runrequest-2.yml`) is found too, ignoring case and taking
/// spaces and `_` for `-`.
fn find(collections: &impl CollectionRepository, path: &str) -> Result<RequestPath, String> {
    let trimmed = path.trim_matches('/');
    for candidate in [trimmed.to_owned(), format!("{trimmed}.yml")] {
        let candidate = RequestPath::new(candidate);
        if collections.request(&candidate).is_ok() {
            return Ok(candidate);
        }
    }
    let tree = collections.tree().map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    collect(&tree.items, &mut paths);
    let bare = |p: &str| -> String {
        let p = p.trim_matches('/');
        let p = (p.strip_suffix(".yml").or_else(|| p.strip_suffix(".yaml"))).unwrap_or(p);
        p.to_owned()
    };
    let loose = |p: &str| -> String {
        (bare(p).chars())
            .map(|c| {
                if c == ' ' || c == '_' {
                    '-'
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect()
    };
    let wanted = bare(path);
    (paths.iter().find(|p| bare(p.as_str()) == wanted))
        .or_else(|| paths.iter().find(|p| loose(p.as_str()) == loose(path)))
        .cloned()
        .ok_or_else(|| format!("there is no request `{path}` in the collection"))
}

fn collect(items: &[TreeItem], paths: &mut Vec<RequestPath>) {
    for item in items {
        match item {
            TreeItem::Folder { children, .. } => collect(children, paths),
            TreeItem::Request { path, .. } => paths.push(path.clone()),
            TreeItem::Other { .. } | TreeItem::Invalid { .. } => {}
        }
    }
}

/// A body collected in memory, up to [`BODY_LIMIT`]: what a script reads.
#[derive(Default)]
struct Collect(Vec<u8>);

impl BodyWriter for Collect {
    fn write(&mut self, chunk: &[u8]) -> Result<(), StoreError> {
        if self.0.len() + chunk.len() > BODY_LIMIT {
            return Err(StoreError("the response is too large for a script".into()));
        }
        self.0.extend_from_slice(chunk);
        Ok(())
    }

    fn finish(self) -> Result<StoredBody, StoreError> {
        Err(StoreError("the responses of scripts are not stored".into()))
    }
}

#[cfg(test)]
mod tests {
    use apifast_domain::{Collection, Folder, HttpRequest};

    use super::*;
    use crate::memory::MemoryCollections;

    #[test]
    fn a_request_is_found_as_bruno_names_it() {
        let get = || HttpRequest::new("r", Method::Get, "http://x");
        let collections = MemoryCollections::new(Collection::new("c"))
            .with_folder("scripting", Folder::new("Scripting"))
            .with_request("ping.yml", get())
            .with_request("scripting/runrequest-2.yml", get())
            .with_request("scripting/my-request.yml", get());
        let found = |path| find(&collections, path).map(|p| p.to_string());
        assert_eq!(found("ping").as_deref(), Ok("ping.yml"));
        assert_eq!(found("/ping.yml").as_deref(), Ok("ping.yml"));
        assert_eq!(
            found("scripting/runRequest-2").as_deref(),
            Ok("scripting/runrequest-2.yml")
        );
        assert_eq!(
            found("scripting/My Request").as_deref(),
            Ok("scripting/my-request.yml")
        );
        assert_eq!(
            found("nope"),
            Err("there is no request `nope` in the collection".to_owned())
        );
    }
}
