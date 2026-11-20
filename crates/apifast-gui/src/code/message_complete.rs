//! What can be typed at the cursor of a gRPC message, written in JSON,
//! from its type: the fields of the message whose object is open (those
//! not written yet), an enum's values, and `true` or `false`.
//!
//! A light reading, not a parser: it walks the text up to the cursor,
//! keeping a stack of the objects and lists it is in. `{{templates}}` are
//! skipped. What it cannot tell gives no suggestions rather than wrong
//! ones.

use apifast_domain::{GrpcCatalog, GrpcField, GrpcFieldKind};

use super::{Completion, CompletionItem};

/// What a value is, as far as completing it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// An object of the message's fields.
    Message(String),
    /// An object of any keys, each value a `.0`.
    Map(Box<Target>),
    /// A list, each value a `.0`.
    List(Box<Target>),
    /// One of these names.
    Enum(Vec<String>),
    Bool,
    /// Anything else: a number, a string, a well-known type.
    Other,
}

impl Target {
    fn of(field: &GrpcField) -> Self {
        let one = match &field.kind {
            GrpcFieldKind::Message(name) => Self::Message(name.clone()),
            GrpcFieldKind::Enum(_, values) => Self::Enum(values.clone()),
            GrpcFieldKind::Scalar(name) if name == "bool" => Self::Bool,
            GrpcFieldKind::Scalar(_) => Self::Other,
        };
        if field.map {
            Self::Map(Box::new(one))
        } else if field.repeated {
            Self::List(Box::new(one))
        } else {
            one
        }
    }
}

/// An object or a list the cursor is in.
#[derive(Debug)]
struct Frame {
    /// What it is: an object (`Message` or `Map`) or a list (`List`).
    target: Target,
    /// The keys written in it so far.
    keys: Vec<String>,
    /// The key whose value is being written.
    key: Option<String>,
    /// Whether a `:` came after the key.
    colon: bool,
}

impl Frame {
    fn new(target: Target) -> Self {
        Self {
            target,
            keys: Vec::new(),
            key: None,
            colon: false,
        }
    }
}

/// Where the cursor is.
#[derive(Debug)]
enum Place {
    /// Where an object's key goes.
    Key,
    /// Where a value of the target goes.
    Value(Target),
    /// Nowhere it can tell.
    Unknown,
}

/// The completion of `text` at byte `cursor`, a message of type `input`
/// as `catalog` describes it.
pub fn complete_message(
    text: &str,
    cursor: usize,
    catalog: &GrpcCatalog,
    input: &str,
) -> Completion {
    let mut cursor = cursor.min(text.len());
    while !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let before = &text[..cursor];
    let root = Target::Message(input.to_owned());
    let mut stack: Vec<Frame> = Vec::new();
    let mut at = 0;
    while at < before.len() {
        let rest = &before[at..];
        if rest.starts_with("{{") {
            match rest.find("}}") {
                Some(end) => at += end + 2,
                // Inside a template: nothing to suggest.
                None => return Completion::default(),
            }
            continue;
        }
        let Some(c) = rest.chars().next() else { break };
        match c {
            '"' => {
                let Some(len) = closed_string(rest) else {
                    let start = at + 1;
                    let place = place(&stack, &root, catalog);
                    return in_string(place, &before[start..], start, &stack, catalog);
                };
                if let Some(frame) = stack.last_mut()
                    && is_object(frame)
                    && !frame.colon
                {
                    frame.key = Some(rest[1..len - 1].to_owned());
                }
                at += len;
                continue;
            }
            '{' | '[' => {
                let target = match value_target(&stack, &root, catalog) {
                    Some(Target::List(inner)) if c == '[' => Target::List(inner),
                    Some(target @ (Target::Message(_) | Target::Map(_))) if c == '{' => target,
                    _ => Target::Other,
                };
                stack.push(Frame::new(target));
            }
            '}' | ']' => {
                stack.pop();
            }
            ':' => {
                if let Some(frame) = stack.last_mut() {
                    frame.colon = true;
                }
            }
            ',' => {
                if let Some(frame) = stack.last_mut() {
                    if let Some(key) = frame.key.take() {
                        frame.keys.push(key);
                    }
                    frame.colon = false;
                }
            }
            _ => {}
        }
        at += c.len_utf8();
    }
    let start = before
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    let word = &before[start..];
    // Right after what opens a key or a value, and nothing else.
    let opened = before[..start].trim_end();
    let fresh = opened.is_empty() || opened.ends_with(['{', '[', ',', ':']);
    if !fresh {
        return Completion::default();
    }
    let items = match place(&stack, &root, catalog) {
        Place::Key => keys(&stack, catalog)
            .into_iter()
            .map(|(name, detail)| CompletionItem {
                label: format!("\"{name}\": "),
                detail,
            })
            .collect(),
        Place::Value(Target::Enum(values)) => (values.iter())
            .map(|value| CompletionItem {
                label: format!("\"{value}\""),
                detail: "enum".into(),
            })
            .collect(),
        Place::Value(Target::Bool) => ["true", "false"]
            .map(|value| CompletionItem {
                label: value.into(),
                detail: "bool".into(),
            })
            .to_vec(),
        _ => Vec::new(),
    };
    filtered(items, word, start, true)
}

/// Inside an unclosed string at `start`: a key's name, or an enum value.
fn in_string(
    place: Place,
    word: &str,
    start: usize,
    stack: &[Frame],
    catalog: &GrpcCatalog,
) -> Completion {
    let items = match place {
        Place::Key => keys(stack, catalog)
            .into_iter()
            .map(|(label, detail)| CompletionItem { label, detail })
            .collect(),
        Place::Value(Target::Enum(values)) => (values.into_iter())
            .map(|label| CompletionItem {
                label,
                detail: "enum".into(),
            })
            .collect(),
        _ => Vec::new(),
    };
    filtered(items, word, start, false)
}

/// The items that start with `word`, replacing it from `start`; outside a
/// string, matched without their quote.
fn filtered(items: Vec<CompletionItem>, word: &str, start: usize, quoted: bool) -> Completion {
    let items = items
        .into_iter()
        .filter(|item| {
            let label = if quoted {
                item.label.trim_start_matches('"')
            } else {
                &item.label
            };
            label.starts_with(word) && label != word
        })
        .collect();
    Completion { start, items }
}

/// The fields not yet written in the message whose object the cursor is
/// in, with what each holds.
fn keys(stack: &[Frame], catalog: &GrpcCatalog) -> Vec<(String, String)> {
    let Some(Frame {
        target: Target::Message(name),
        keys,
        key,
        ..
    }) = stack.last()
    else {
        return Vec::new();
    };
    let fields = catalog.messages.get(name).map_or(&[][..], Vec::as_slice);
    (fields.iter())
        .filter(|f| !keys.contains(&f.name) && key.as_ref() != Some(&f.name))
        .map(|f| (f.name.clone(), detail(f)))
        .collect()
}

/// What a field holds, shown next to it: `string`, `repeated Tag`,
/// `map<Tag>`.
fn detail(field: &GrpcField) -> String {
    let one = match &field.kind {
        GrpcFieldKind::Scalar(name)
        | GrpcFieldKind::Message(name)
        | GrpcFieldKind::Enum(name, _) => name.rsplit('.').next().unwrap_or(name).to_owned(),
    };
    if field.map {
        format!("map<{one}>")
    } else if field.repeated {
        format!("repeated {one}")
    } else {
        one
    }
}

fn is_object(frame: &Frame) -> bool {
    matches!(frame.target, Target::Message(_) | Target::Map(_))
}

/// Where the cursor is, by the open object or list.
fn place(stack: &[Frame], root: &Target, catalog: &GrpcCatalog) -> Place {
    match stack.last() {
        Some(frame) if is_object(frame) && !frame.colon => Place::Key,
        _ => value_target(stack, root, catalog).map_or(Place::Unknown, Place::Value),
    }
}

/// What the value at the cursor is.
fn value_target(stack: &[Frame], root: &Target, catalog: &GrpcCatalog) -> Option<Target> {
    let Some(frame) = stack.last() else {
        return Some(root.clone());
    };
    match &frame.target {
        Target::List(inner) => Some((**inner).clone()),
        Target::Map(inner) if frame.colon => Some((**inner).clone()),
        Target::Message(name) if frame.colon => {
            let key = frame.key.as_ref()?;
            let fields = catalog.messages.get(name)?;
            fields.iter().find(|f| &f.name == key).map(Target::of)
        }
        _ => None,
    }
}

/// The length of the string `text` starts with, quotes included, if it
/// closes.
fn closed_string(text: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in text.char_indices().skip(1) {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return Some(i + 1),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn field(name: &str, kind: GrpcFieldKind, repeated: bool, map: bool) -> GrpcField {
        GrpcField {
            name: name.into(),
            kind,
            repeated,
            map,
        }
    }

    fn catalog() -> GrpcCatalog {
        let scalar = |n: &str| GrpcFieldKind::Scalar(n.into());
        let mut messages = BTreeMap::new();
        messages.insert(
            "s.Ask".to_owned(),
            vec![
                field("userId", scalar("string"), false, false),
                field("tags", GrpcFieldKind::Message("s.Tag".into()), true, false),
                field(
                    "byName",
                    GrpcFieldKind::Message("s.Tag".into()),
                    false,
                    true,
                ),
                field(
                    "color",
                    GrpcFieldKind::Enum("s.Color".into(), vec!["RED".into(), "BLUE".into()]),
                    false,
                    false,
                ),
                field("urgent", scalar("bool"), false, false),
            ],
        );
        messages.insert(
            "s.Tag".to_owned(),
            vec![field("label", scalar("string"), false, false)],
        );
        GrpcCatalog {
            services: Vec::new(),
            messages,
        }
    }

    /// The labels at the end of `text`, and where they start.
    fn at_end(text: &str) -> (usize, Vec<String>) {
        let completion = complete_message(text, text.len(), &catalog(), "s.Ask");
        (
            completion.start,
            completion.items.into_iter().map(|i| i.label).collect(),
        )
    }

    #[test]
    fn keys_are_the_fields_not_written_yet() {
        let (start, labels) = at_end("{\n  ");
        assert_eq!(start, 4);
        assert_eq!(labels[0], "\"userId\": ");
        assert_eq!(labels.len(), 5);
        let (_, labels) = at_end("{\"userId\": \"x\", \"urgent\": true, ");
        assert_eq!(labels, ["\"tags\": ", "\"byName\": ", "\"color\": "]);
        // In an open key, the name only, by what it starts with.
        let (start, labels) = at_end("{\"col");
        assert_eq!((start, labels), (2, vec!["color".to_owned()]));
        // Nested, by the field's type.
        let (_, labels) = at_end("{\"tags\": [{\"label\": \"a\"}, {");
        assert_eq!(labels, ["\"label\": "]);
        let (_, labels) = at_end("{\"byName\": {\"x\": {");
        assert_eq!(labels, ["\"label\": "]);
    }

    #[test]
    fn values_are_an_enums_names_or_a_bool() {
        let (_, labels) = at_end("{\"color\": ");
        assert_eq!(labels, ["\"RED\"", "\"BLUE\""]);
        let (start, labels) = at_end("{\"color\": \"B");
        assert_eq!((start, labels), (11, vec!["BLUE".to_owned()]));
        let (_, labels) = at_end("{\"urgent\": f");
        assert_eq!(labels, ["false"]);
    }

    #[test]
    fn nothing_where_it_cannot_tell() {
        let none: Vec<String> = Vec::new();
        for text in [
            "{\"userId\": ",
            "{\"userId\": \"{{na",
            "{\"userId\": \"x\" ",
            "{\"byName\": {",
        ] {
            assert_eq!(at_end(text).1, none, "{text}");
        }
        let unknown = complete_message("{", 1, &catalog(), "s.Nope");
        assert_eq!(unknown.items, []);
    }
}
