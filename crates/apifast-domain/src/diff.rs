//! What changed in a request, a folder, a collection or an environment
//! between two versions of its file (docs/09, F10): field by field, not line
//! by line. Lists whose items have a name (headers, parameters, variables,
//! assertions, captures, scripts) are compared item by item, so a header
//! that changed is one change, whatever moved around it.

use std::fmt::Write as _;

use crate::{
    Assertion, Auth, Body, Capture, Collection, Environment, Folder, Header, HttpRequest, Param,
    RequestDefaults, ScriptBlock, Setting, Variable,
};

/// A field that differs between two versions: what it was and what it is,
/// as text; `None` where it is not there (added, or removed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    /// The field, as a person names it: `method`, `header X-Team`,
    /// `assertion res.status`.
    pub field: String,
    /// Before.
    pub before: Option<String>,
    /// After.
    pub after: Option<String>,
}

impl FieldChange {
    /// Whether it was not there before.
    pub fn is_added(&self) -> bool {
        self.before.is_none()
    }

    /// Whether it is not there anymore.
    pub fn is_removed(&self) -> bool {
        self.after.is_none()
    }
}

/// The changes, in the order of the file's fields.
#[derive(Default)]
struct Changes(Vec<FieldChange>);

impl Changes {
    /// `field`, when `before` and `after` differ.
    fn value(&mut self, field: &str, before: Option<String>, after: Option<String>) {
        if before != after {
            self.0.push(FieldChange {
                field: field.to_owned(),
                before,
                after,
            });
        }
    }

    /// A field that is always there.
    fn text(&mut self, field: &str, before: impl Into<String>, after: impl Into<String>) {
        self.value(field, Some(before.into()), Some(after.into()));
    }

    /// The items of a list, by their key: each one added, removed or changed
    /// is a change named `{label} {key}`. Of several items with one key,
    /// each is matched with the one in the same place among them.
    fn list<T>(
        &mut self,
        label: &str,
        (before, after): (&[T], &[T]),
        key: impl Fn(&T) -> String,
        shown: impl Fn(&T) -> String,
    ) {
        let mut keys: Vec<(String, usize)> = Vec::new();
        let mut nth = |items: &[T]| -> Vec<((String, usize), String)> {
            let mut seen: Vec<String> = Vec::new();
            (items.iter())
                .map(|item| {
                    let k = key(item);
                    let n = seen.iter().filter(|s| **s == k).count();
                    seen.push(k.clone());
                    if !keys.contains(&(k.clone(), n)) {
                        keys.push((k.clone(), n));
                    }
                    ((k, n), shown(item))
                })
                .collect()
        };
        let (old, new) = (nth(before), nth(after));
        for slot in &keys {
            let find = |items: &[((String, usize), String)]| {
                items
                    .iter()
                    .find(|(k, _)| k == slot)
                    .map(|(_, v)| v.clone())
            };
            let field = if slot.0.is_empty() {
                label.to_owned()
            } else {
                format!("{label} {}", slot.0)
            };
            self.value(&field, find(&old), find(&new));
        }
    }
}

fn off(enabled: bool) -> &'static str {
    if enabled { "" } else { " (off)" }
}

fn header_list(changes: &mut Changes, before: &[Header], after: &[Header]) {
    changes.list(
        "header",
        (before, after),
        |h| h.name.to_ascii_lowercase(),
        |h| format!("{}: {}{}", h.name, h.value, off(h.enabled)),
    );
}

fn param_list(changes: &mut Changes, before: &[Param], after: &[Param]) {
    changes.list(
        "param",
        (before, after),
        |p| {
            let kind = match p.kind {
                crate::ParamKind::Query => "query",
                crate::ParamKind::Path => "path",
            };
            format!("{} ({kind})", p.name)
        },
        |p| format!("{}={}{}", p.name, p.value, off(p.enabled)),
    );
}

fn variable_list(changes: &mut Changes, before: &[Variable], after: &[Variable]) {
    changes.list(
        "variable",
        (before, after),
        |v| v.name.clone(),
        variable_text,
    );
}

fn variable_text(variable: &Variable) -> String {
    let value = match (&variable.value, variable.secret) {
        (_, true) => "(secret)".to_owned(),
        (Some(value), false) => value.data.clone(),
        (None, false) => String::new(),
    };
    format!("{}{}", value, off(variable.enabled))
}

fn assertion_list(changes: &mut Changes, before: &[Assertion], after: &[Assertion]) {
    changes.list(
        "assertion",
        (before, after),
        |a| a.expression.clone(),
        |a| {
            let value = a
                .value
                .as_deref()
                .map(|v| format!(" {v}"))
                .unwrap_or_default();
            format!("{}{value}{}", a.operator.as_str(), off(a.enabled))
        },
    );
}

fn capture_list(changes: &mut Changes, before: &[Capture], after: &[Capture]) {
    changes.list(
        "capture",
        (before, after),
        |c| c.variable.clone(),
        |c| format!("{} → {} ({})", c.expression, c.variable, c.scope.as_str()),
    );
}

fn script_list(changes: &mut Changes, before: &[ScriptBlock], after: &[ScriptBlock]) {
    changes.list(
        "script",
        (before, after),
        |s| s.kind.as_str().to_owned(),
        |s| s.code.clone(),
    );
}

fn tag_list(changes: &mut Changes, before: &[String], after: &[String]) {
    let join = |tags: &[String]| (!tags.is_empty()).then(|| tags.join(", "));
    changes.value("tags", join(before), join(after));
}

/// An auth as text: `inherit`, `none`, or its scheme and fields.
fn auth_text(auth: &Auth) -> String {
    match auth {
        Setting::Inherit => "inherit".to_owned(),
        Setting::Set(scheme) => format!("{scheme:?}"),
    }
}

/// A body as text: its kind, and what it holds.
fn body_text(body: &Body) -> Option<String> {
    let text = match body {
        Body::None => return None,
        Body::Json(text) => format!("json\n{text}"),
        Body::Text(text) => format!("text\n{text}"),
        Body::Xml(text) => format!("xml\n{text}"),
        Body::Sparql(text) => format!("sparql\n{text}"),
        Body::FormUrlEncoded(fields) => fields.iter().fold("form".to_owned(), |mut out, f| {
            let _ = write!(out, "\n{}={}{}", f.name, f.value, off(f.enabled));
            out
        }),
        other => format!("{other:?}"),
    };
    Some(text)
}

fn defaults(changes: &mut Changes, before: &RequestDefaults, after: &RequestDefaults) {
    header_list(changes, &before.headers, &after.headers);
    changes.text("auth", auth_text(&before.auth), auth_text(&after.auth));
    variable_list(changes, &before.variables, &after.variables);
    changes.text(
        "settings",
        format!("{:?}", before.settings),
        format!("{:?}", after.settings),
    );
    script_list(changes, &before.scripts, &after.scripts);
}

/// What changed from `before` to `after`, a request.
pub fn request_changes(before: &HttpRequest, after: &HttpRequest) -> Vec<FieldChange> {
    let mut changes = Changes::default();
    changes.text("name", &before.name, &after.name);
    changes.value(
        "position",
        before.seq.map(|s| s.to_string()),
        after.seq.map(|s| s.to_string()),
    );
    tag_list(&mut changes, &before.tags, &after.tags);
    changes.text("method", before.method.as_str(), after.method.as_str());
    changes.text("url", &before.url, &after.url);
    param_list(&mut changes, &before.params, &after.params);
    header_list(&mut changes, &before.headers, &after.headers);
    changes.value("body", body_text(&before.body), body_text(&after.body));
    changes.text("auth", auth_text(&before.auth), auth_text(&after.auth));
    variable_list(&mut changes, &before.variables, &after.variables);
    assertion_list(&mut changes, &before.assertions, &after.assertions);
    capture_list(&mut changes, &before.captures, &after.captures);
    script_list(&mut changes, &before.scripts, &after.scripts);
    changes.text(
        "settings",
        format!("{:?}", before.settings),
        format!("{:?}", after.settings),
    );
    changes.value("docs", before.docs.clone(), after.docs.clone());
    changes.list(
        "example",
        (&before.examples, &after.examples),
        |e| e.name.clone(),
        |e| format!("{e:?}"),
    );
    changes.0
}

/// What changed from `before` to `after`, a folder.
pub fn folder_changes(before: &Folder, after: &Folder) -> Vec<FieldChange> {
    let mut changes = Changes::default();
    changes.text("name", &before.name, &after.name);
    changes.value(
        "position",
        before.seq.map(|s| s.to_string()),
        after.seq.map(|s| s.to_string()),
    );
    tag_list(&mut changes, &before.tags, &after.tags);
    defaults(&mut changes, &before.defaults, &after.defaults);
    changes.0
}

/// What changed from `before` to `after`, the collection.
pub fn collection_changes(before: &Collection, after: &Collection) -> Vec<FieldChange> {
    let mut changes = Changes::default();
    changes.text("name", &before.name, &after.name);
    defaults(&mut changes, &before.defaults, &after.defaults);
    changes.text(
        "proxy",
        format!("{:?}", before.proxy),
        format!("{:?}", after.proxy),
    );
    changes.text(
        "client certificates",
        format!("{:?}", before.client_certificates),
        format!("{:?}", after.client_certificates),
    );
    changes.0
}

/// What changed from `before` to `after`, an environment.
pub fn environment_changes(before: &Environment, after: &Environment) -> Vec<FieldChange> {
    let mut changes = Changes::default();
    changes.text("name", &before.name, &after.name);
    changes.value("color", before.color.clone(), after.color.clone());
    changes.value("extends", before.extends.clone(), after.extends.clone());
    variable_list(&mut changes, &before.variables, &after.variables);
    changes.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Method, Operator, ParamKind};

    fn change(field: &str, before: Option<&str>, after: Option<&str>) -> FieldChange {
        FieldChange {
            field: field.into(),
            before: before.map(str::to_owned),
            after: after.map(str::to_owned),
        }
    }

    #[test]
    fn a_request_changes_field_by_field_and_its_lists_item_by_item() {
        let mut before = HttpRequest::new("List users", Method::Get, "{{host}}/users");
        before.headers = vec![
            Header::new("Accept", "application/json"),
            Header::new("X-Team", "core"),
        ];
        before.assertions = vec![Assertion::new(
            "res.status",
            Operator::parse("eq"),
            Some("200"),
        )];
        let mut after = before.clone();
        after.method = Method::Post;
        after.headers = vec![
            Header::new("x-team", "platform"),
            Header::new("Accept", "application/json"),
            Header::new("X-New", "1"),
        ];
        after.params = vec![Param {
            name: "page".into(),
            value: "2".into(),
            kind: ParamKind::Query,
            enabled: false,
        }];
        after.body = Body::Json("{}".into());
        after.assertions.clear();
        after.tags = vec!["smoke".into()];

        assert_eq!(
            request_changes(&before, &after),
            [
                change("tags", None, Some("smoke")),
                change("method", Some("GET"), Some("POST")),
                change("param page (query)", None, Some("page=2 (off)")),
                change(
                    "header x-team",
                    Some("X-Team: core"),
                    Some("x-team: platform")
                ),
                change("header x-new", None, Some("X-New: 1")),
                change("body", None, Some("json\n{}")),
                change("assertion res.status", Some("eq 200"), None),
            ]
        );
        assert_eq!(request_changes(&before, &before), []);
    }

    #[test]
    fn repeated_names_are_matched_in_order_and_secrets_never_show() {
        let mut before = Environment::new("dev", vec![Variable::secret("token")]);
        before.variables.push(Variable::text("host", "a"));
        let mut after = before.clone();
        after.variables = vec![
            Variable::secret("token"),
            Variable::text("host", "b"),
            Variable::text("host", "c"),
        ];
        assert_eq!(
            environment_changes(&before, &after),
            [
                change("variable host", Some("a"), Some("b")),
                change("variable host", None, Some("c")),
            ]
        );
    }

    #[test]
    fn a_folder_says_what_its_requests_inherit_that_changed() {
        let before = Folder::new("Users");
        let mut after = before.clone();
        after.defaults.headers = vec![Header::new("X-Team", "core")];
        after.tags = vec!["users".into()];
        let changes = folder_changes(&before, &after);
        assert_eq!(
            changes,
            [
                change("tags", None, Some("users")),
                change("header x-team", None, Some("X-Team: core")),
            ]
        );
        assert!(changes[0].is_added() && !changes[0].is_removed());
    }
}
