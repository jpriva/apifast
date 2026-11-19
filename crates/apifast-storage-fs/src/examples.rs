//! The `examples` of a request file, as Bruno reads and writes them
//! (`bruno-filestore/src/formats/yml/items/stringifyHttpRequest.ts`):
//!
//! ```yaml
//! examples:
//!   - name: Found
//!     description: A user that exists
//!     request: {url: ..., method: GET, headers: [...], params: [...], body: {...}}
//!     response:
//!       status: 200
//!       statusText: OK
//!       headers: [{name: content-type, value: application/json}]
//!       body: {type: json, data: "{...}"}
//! ```

use apifast_domain::{Example, ExampleBody, ExampleRequest, ExampleResponse, Method};
use oneyaml::Value;

use crate::read;
use crate::write;
use crate::yaml::{get, items, mapping, number, set, string, text, unsigned};

/// The examples of a request document.
pub(crate) fn read_examples(doc: &Value) -> Vec<Example> {
    items(get(doc, "examples")).iter().map(example).collect()
}

fn example(item: &Value) -> Example {
    Example {
        name: text(get(item, "name")).unwrap_or_else(|| "Untitled Example".into()),
        description: text(get(item, "description")).filter(|d| !d.trim().is_empty()),
        request: get(item, "request")
            .filter(|r| r.as_mapping().is_some())
            .map(|r| ExampleRequest {
                method: Method::parse(&text(get(r, "method")).unwrap_or_else(|| "GET".into())),
                url: text(get(r, "url")).unwrap_or_default(),
                headers: read::headers(get(r, "headers")),
                params: items(get(r, "params")).iter().map(read::param).collect(),
                body: read::body(get(r, "body")),
            }),
        response: get(item, "response")
            .filter(|r| r.as_mapping().is_some())
            .map(|r| ExampleResponse {
                status: unsigned(get(r, "status")),
                status_text: text(get(r, "statusText")).filter(|t| !t.is_empty()),
                headers: read::headers(get(r, "headers")),
                body: get(r, "body")
                    .filter(|b| b.as_mapping().is_some())
                    .map(|b| ExampleBody {
                        kind: text(get(b, "type")).unwrap_or_else(|| "text".into()),
                        data: text(get(b, "data")).unwrap_or_default(),
                    }),
            }),
    }
}

/// `example` as a file writes it, in Bruno's order.
pub(crate) fn example_value(example: &Example) -> Value {
    let request = example.request.as_ref().map(|r| {
        mapping([
            ("url", Some(string(r.url.as_str()))),
            ("method", Some(string(r.method.as_str()))),
            (
                "headers",
                write::list_value(&r.headers, write::header_value),
            ),
            ("params", write::list_value(&r.params, write::param_value)),
            ("body", write::body_value(&r.body)),
        ])
    });
    let response = example.response.as_ref().map(|r| {
        mapping([
            ("status", r.status.map(number)),
            ("statusText", r.status_text.as_deref().map(string)),
            (
                "headers",
                write::list_value(&r.headers, write::header_value),
            ),
            (
                "body",
                r.body.as_ref().map(|b| {
                    mapping([
                        ("type", Some(string(b.kind.as_str()))),
                        ("data", Some(string(b.data.as_str()))),
                    ])
                }),
            ),
        ])
    });
    mapping([
        ("name", Some(string(example.name.as_str()))),
        ("description", example.description.as_deref().map(string)),
        ("request", request),
        ("response", response),
    ])
}

/// Writes `new` in place of `old`: an example only renamed keeps the rest
/// of its item as it was; the others that did not change are not touched.
pub(crate) fn update_examples(doc: &mut Value, old: &[Example], new: &[Example]) {
    write::update_list(
        doc,
        "examples",
        old,
        new,
        example_value,
        |item, old, new| {
            let renamed_only = Example {
                name: new.name.clone(),
                ..old.clone()
            } == *new;
            if renamed_only {
                set(item, "name", string(new.name.as_str()));
            } else {
                *item = example_value(new);
            }
        },
    );
}
