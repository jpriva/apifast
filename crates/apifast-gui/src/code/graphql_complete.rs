//! What can be typed at the cursor of a GraphQL operation, from its schema:
//! fields of the type whose selection is open, arguments of the field whose
//! parentheses are open, the operation's `$variables`, an enum's values,
//! type names after `on` and in variable definitions, or the keywords that
//! start a definition.
//!
//! A light reading, not a parser: it walks the text up to the cursor,
//! keeping a stack of the selections and parentheses it is in. Strings,
//! comments and `{{templates}}` are skipped. What it cannot tell gives no
//! suggestions rather than wrong ones.

use apifast_domain::{GraphQlSchema, SchemaField, TypeKind, TypeRef};

/// A suggestion: what is put in the text, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// The text it puts in place of the word at the cursor.
    pub label: String,
    /// Its type, or what it is, shown next to it.
    pub detail: String,
}

/// The suggestions at a cursor, and the word they replace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Completion {
    /// Where the word being typed starts, in bytes.
    pub start: usize,
    /// Its suggestions, those that start with the word, in the schema's order.
    pub items: Vec<CompletionItem>,
}

/// Keywords that start a definition at the top of a document.
const DEFINITIONS: &[&str] = &["query", "mutation", "subscription", "fragment"];

/// One open level: a selection of a type, the parentheses of a field's
/// arguments, or the variable definitions of an operation.
#[derive(Debug, Clone)]
enum Level {
    /// `{` of a selection; the type, if known.
    Selection(Option<String>),
    /// `(` of a field's arguments: the field, and its name as written.
    Arguments(Option<SchemaField>, String),
    /// `(` after an operation's name: its variable definitions.
    Variables,
    /// `[` or `{` of a value inside arguments.
    Value,
}

/// The completion of `text` at byte `cursor`, from `schema`.
pub fn complete(text: &str, cursor: usize, schema: &GraphQlSchema) -> Completion {
    let cursor = floor_char_boundary(text, cursor.min(text.len()));
    let before = &text[..cursor];
    let start = word_start(before);
    let word = &before[start..];
    if ends_inside(&before[..start]) {
        return Completion {
            start,
            items: Vec::new(),
        };
    }
    let reading = read(&before[..start], schema);
    let mut items = suggestions(&reading, schema);
    let lower = word.to_lowercase();
    items.retain(|item| {
        let label = item.label.to_lowercase();
        label.starts_with(&lower) && label != lower
    });
    Completion { start, items }
}

/// What the text before the word says about where it is.
#[derive(Debug, Default)]
struct Reading {
    levels: Vec<Level>,
    /// The `$variables` the operation declares, with their types.
    variables: Vec<(String, String)>,
    /// The last word before the cursor, and the one before it.
    last: Option<String>,
    previous: Option<String>,
    /// Whether the last thing in the open arguments is `name:`, and the
    /// argument it names.
    argument: Option<String>,
    /// The argument names already given in the open arguments.
    given: Vec<String>,
    /// The keyword of the definition being read, if any.
    definition: Option<String>,
}

fn suggestions(reading: &Reading, schema: &GraphQlSchema) -> Vec<CompletionItem> {
    // `$` starts a variable wherever it is.
    if reading.last.as_deref() == Some("$") {
        return (reading.variables.iter())
            .map(|(name, kind)| item(&format!("${name}"), kind))
            .collect();
    }
    if reading.last.as_deref() == Some("on") {
        return type_names(schema, |kind| {
            matches!(
                kind,
                TypeKind::Object | TypeKind::Interface | TypeKind::Union
            )
        });
    }
    match reading.levels.last() {
        None => {
            if reading.last.is_some() {
                return Vec::new();
            }
            DEFINITIONS.iter().map(|k| item(k, "keyword")).collect()
        }
        Some(Level::Selection(Some(name))) => {
            let Some(shown) = schema.type_named(name) else {
                return Vec::new();
            };
            // Right after a field's name, the next word is another field.
            let mut items: Vec<CompletionItem> = (shown.fields.iter())
                .filter(|f| !f.deprecated)
                .map(|f| item(&f.name, &f.type_ref.to_string()))
                .collect();
            items.push(item("__typename", "String!"));
            items
        }
        Some(Level::Arguments(Some(field), _)) => match &reading.argument {
            // After `name:`, its value: a variable, or a value of its type.
            Some(argument) => {
                let kind = (field.args.iter())
                    .find(|a| a.name == *argument)
                    .map(|a| a.type_ref.clone());
                values(kind.as_ref(), reading, schema)
            }
            None => (field.args.iter())
                .filter(|a| !reading.given.contains(&a.name))
                .map(|a| item(&a.name, &a.type_ref.to_string()))
                .collect(),
        },
        Some(Level::Variables) if reading.argument.is_some() => type_names(schema, |kind| {
            matches!(
                kind,
                TypeKind::Scalar | TypeKind::Enum | TypeKind::InputObject
            )
        }),
        _ => Vec::new(),
    }
}

/// What can be an argument's value of type `kind`.
fn values(
    kind: Option<&TypeRef>,
    reading: &Reading,
    schema: &GraphQlSchema,
) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = (reading.variables.iter())
        .map(|(name, kind)| item(&format!("${name}"), kind))
        .collect();
    let Some(kind) = kind else {
        return items;
    };
    match schema.type_named(kind.name()) {
        Some(shown) if shown.kind == TypeKind::Enum => {
            let name = &shown.name;
            items.extend(
                (shown.enum_values.iter())
                    .filter(|v| !v.deprecated)
                    .map(|v| item(&v.name, name)),
            );
        }
        _ if kind.name() == "Boolean" => {
            items.extend(["true", "false"].map(|b| item(b, "Boolean")));
        }
        _ => {}
    }
    items
}

fn type_names(schema: &GraphQlSchema, keep: impl Fn(TypeKind) -> bool) -> Vec<CompletionItem> {
    (schema.user_types())
        .filter(|t| keep(t.kind))
        .map(|t| item(&t.name, t.kind.keyword()))
        .collect()
}

fn item(label: &str, detail: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        detail: detail.to_owned(),
    }
}

/// Walks `text` (all before the word at the cursor), keeping the levels it
/// is in.
fn read(text: &str, schema: &GraphQlSchema) -> Reading {
    let mut reading = Reading::default();
    for token in tokens(text) {
        match token {
            Token::Punct('{') => reading.open_brace(schema),
            Token::Punct('(') => reading.open_paren(schema),
            Token::Punct('[')
                if matches!(
                    reading.levels.last(),
                    Some(Level::Arguments(..) | Level::Value)
                ) =>
            {
                reading.levels.push(Level::Value);
            }
            Token::Punct(close @ ('}' | ')' | ']')) => reading.close(close),
            Token::Punct(':') => reading.colon(),
            Token::Punct('$') => reading.push_word("$".into()),
            Token::Punct(_) => {}
            Token::Word(word) => reading.word(word),
            Token::Value => {
                if matches!(reading.levels.last(), Some(Level::Arguments(..))) {
                    reading.argument = None;
                }
                reading.push_word(String::new());
            }
        }
    }
    // After `name:` and a space, the value is the word at the cursor.
    if matches!(reading.levels.last(), Some(Level::Arguments(..)))
        && reading.argument.is_some()
        && reading.last.as_deref() != reading.argument.as_deref()
    {
        reading.argument = None;
    }
    reading
}

impl Reading {
    fn push_word(&mut self, word: String) {
        self.previous = self.last.take();
        self.last = Some(word);
    }

    /// `{`: a selection, of the type the words before it say, or a value.
    fn open_brace(&mut self, schema: &GraphQlSchema) {
        let level = match self.levels.last() {
            None => Level::Selection(self.root_type(schema)),
            Some(Level::Selection(Some(parent))) => {
                // `... on Type {` or `field {` / `alias: field(…) {`.
                let named = if self.previous.as_deref() == Some("on") {
                    self.last.clone()
                } else {
                    (self.last.as_ref()).and_then(|field| {
                        field_of(schema, parent, field).map(|f| f.type_ref.name().to_owned())
                    })
                };
                Level::Selection(named)
            }
            Some(Level::Arguments(..) | Level::Value | Level::Variables) => Level::Value,
            Some(Level::Selection(None)) => Level::Selection(None),
        };
        self.levels.push(level);
        self.last = None;
        self.previous = None;
    }

    /// `(`: an operation's variable definitions, a field's arguments, or a
    /// value.
    fn open_paren(&mut self, schema: &GraphQlSchema) {
        let name = self.last.clone().unwrap_or_default();
        let level = match self.levels.last() {
            None => Level::Variables,
            Some(Level::Selection(Some(parent))) => {
                Level::Arguments(field_of(schema, parent, &name).cloned(), name)
            }
            Some(Level::Selection(None)) => Level::Arguments(None, name),
            Some(_) => Level::Value,
        };
        self.levels.push(level);
        self.argument = None;
        self.given.clear();
    }

    fn close(&mut self, close: char) {
        let closed = self.levels.pop();
        if close == '}' && self.levels.is_empty() {
            // A definition ends: what comes next starts another.
            self.definition = None;
            self.last = None;
            self.previous = None;
        }
        // After a field's arguments, the field is the last word, for the `{`
        // of its selection.
        if let Some(Level::Arguments(_, field)) = closed {
            self.argument = None;
            self.previous = None;
            self.last = Some(field);
        }
    }

    fn colon(&mut self) {
        match self.levels.last() {
            Some(Level::Arguments(..)) => {
                self.argument.clone_from(&self.last);
                if let Some(name) = &self.last {
                    self.given.push(name.clone());
                }
            }
            Some(Level::Variables) => self.argument = Some(String::new()),
            _ => {}
        }
    }

    fn word(&mut self, word: &str) {
        let in_level = self.levels.last();
        if self.last.as_deref() == Some("$") {
            // A variable: declared if in the definitions, used otherwise.
            if matches!(in_level, Some(Level::Variables)) && self.argument.is_none() {
                self.variables.push((word.to_owned(), String::new()));
            } else if matches!(in_level, Some(Level::Arguments(..))) {
                self.argument = None;
            }
            self.previous = None;
            self.last = Some(format!("${word}"));
            return;
        }
        match in_level {
            None if DEFINITIONS.contains(&word) => self.definition = Some(word.to_owned()),
            Some(Level::Variables) if self.argument.is_some() => {
                // The type of the last declared variable.
                if let Some((_, kind)) = self.variables.last_mut()
                    && kind.is_empty()
                {
                    word.clone_into(kind);
                }
                self.argument = None;
            }
            // A value ends the argument; the next word is a name.
            Some(Level::Arguments(..)) => self.argument = None,
            _ => {}
        }
        self.push_word(word.to_owned());
    }

    /// The type a definition's selection is of: `query`/`mutation`/
    /// `subscription`, or the type of `fragment F on Type`.
    fn root_type(&self, schema: &GraphQlSchema) -> Option<String> {
        match self.definition.as_deref() {
            Some("mutation") => schema.mutation.clone(),
            Some("subscription") => schema.subscription.clone(),
            Some("fragment") => self.last.clone(),
            // `query`, or a bare `{ … }`.
            _ => schema.query.clone(),
        }
    }
}

fn field_of<'a>(schema: &'a GraphQlSchema, parent: &str, field: &str) -> Option<&'a SchemaField> {
    schema
        .type_named(parent)?
        .fields
        .iter()
        .find(|f| f.name == field)
}

#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Word(&'a str),
    Punct(char),
    /// A string, a number or a template: a value.
    Value,
}

/// The tokens of `text`, without spaces, commas or comments.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let c = rest.chars().next().unwrap_or(' ');
        let len = if rest.starts_with("{{") {
            out.push(Token::Value);
            rest.find("}}").map_or(rest.len(), |end| end + 2)
        } else if let Some(block) = rest.strip_prefix("\"\"\"") {
            out.push(Token::Value);
            block.find("\"\"\"").map_or(rest.len(), |end| end + 6)
        } else if c == '"' {
            out.push(Token::Value);
            string_len(rest)
        } else if c == '#' {
            rest.find('\n').unwrap_or(rest.len())
        } else if c.is_ascii_alphabetic() || c == '_' {
            let len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            out.push(Token::Word(&rest[..len]));
            len
        } else if c.is_ascii_digit() || c == '-' {
            out.push(Token::Value);
            rest.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')))
                .unwrap_or(rest.len())
        } else if "{}()[]:$@=!|&".contains(c) {
            out.push(Token::Punct(c));
            1
        } else {
            c.len_utf8()
        };
        at += len.max(1);
    }
    out
}

/// Whether `text` ends inside a string, a comment or a template, where
/// nothing is suggested.
fn ends_inside(text: &str) -> bool {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let (len, closed) = if rest.starts_with("{{") {
            rest.find("}}")
                .map_or((rest.len(), false), |end| (end + 2, true))
        } else if let Some(block) = rest.strip_prefix("\"\"\"") {
            block
                .find("\"\"\"")
                .map_or((rest.len(), false), |end| (end + 6, true))
        } else if rest.starts_with('"') {
            let len = string_len(rest);
            (len, len > 1 && rest[..len].ends_with(['"', '\n']))
        } else if rest.starts_with('#') {
            rest.find('\n')
                .map_or((rest.len(), false), |end| (end, true))
        } else {
            (rest.chars().next().map_or(1, char::len_utf8), true)
        };
        if !closed && at + len >= text.len() {
            return true;
        }
        at += len.max(1);
    }
    false
}

fn string_len(rest: &str) -> usize {
    let mut escaped = false;
    for (i, b) in rest.bytes().enumerate().skip(1) {
        match b {
            _ if escaped => escaped = false,
            b'\\' => escaped = true,
            b'"' | b'\n' => return i + 1,
            _ => {}
        }
    }
    rest.len()
}

/// Where the word ending at the end of `before` starts: names, and a `$`
/// before them.
fn word_start(before: &str) -> usize {
    let start = before
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    if before[..start].ends_with('$') {
        start - 1
    } else {
        start
    }
}

fn floor_char_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use apifast_domain::Json;

    use super::*;

    fn schema() -> GraphQlSchema {
        let answer = r#"{"data": {"__schema": {"queryType": {"name": "Query"}, "mutationType": {"name": "Mutation"}, "types": [
            {"kind": "OBJECT", "name": "Query", "fields": [
                {"name": "user", "args": [{"name": "id", "type": {"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID"}}},
                                          {"name": "role", "type": {"kind": "ENUM", "name": "Role"}}],
                 "type": {"kind": "OBJECT", "name": "User"}},
                {"name": "users", "args": [], "type": {"kind": "LIST", "ofType": {"kind": "OBJECT", "name": "User"}}},
                {"name": "old", "args": [], "isDeprecated": true, "type": {"kind": "SCALAR", "name": "String"}}]},
            {"kind": "OBJECT", "name": "Mutation", "fields": [
                {"name": "createUser", "args": [{"name": "admin", "type": {"kind": "SCALAR", "name": "Boolean"}}], "type": {"kind": "OBJECT", "name": "User"}}]},
            {"kind": "OBJECT", "name": "User", "fields": [
                {"name": "id", "args": [], "type": {"kind": "SCALAR", "name": "ID"}},
                {"name": "name", "args": [], "type": {"kind": "SCALAR", "name": "String"}},
                {"name": "friends", "args": [], "type": {"kind": "LIST", "ofType": {"kind": "OBJECT", "name": "User"}}}]},
            {"kind": "ENUM", "name": "Role", "enumValues": [{"name": "ADMIN"}, {"name": "GUEST"}]},
            {"kind": "SCALAR", "name": "ID"}, {"kind": "SCALAR", "name": "String"}, {"kind": "SCALAR", "name": "Boolean"}]}}}"#;
        GraphQlSchema::from_introspection(&Json::parse(answer).unwrap()).unwrap()
    }

    /// The labels suggested where `|` is in `text`.
    fn labels(text: &str) -> Vec<String> {
        let cursor = text.find('|').unwrap();
        let text = text.replace('|', "");
        complete(&text, cursor, &schema())
            .items
            .into_iter()
            .map(|i| i.label)
            .collect()
    }

    #[test]
    fn fields_follow_the_type_of_the_selection() {
        assert_eq!(
            labels("{ |"),
            ["user", "users", "__typename"],
            "deprecated ones are left out"
        );
        assert_eq!(labels("query { us|"), ["user", "users"]);
        assert_eq!(
            labels("query Q { user(id: 1) { |"),
            ["id", "name", "friends", "__typename"]
        );
        assert_eq!(labels("{ users { friends { na|"), ["name"]);
        assert_eq!(labels("{ me: user { n|"), ["name"], "through an alias");
        assert_eq!(labels("mutation { |"), ["createUser", "__typename"]);
        assert_eq!(
            labels("{ user { id } us|"),
            ["user", "users"],
            "back in the root"
        );
        assert_eq!(labels("fragment F on User { fr|"), ["friends"]);
        assert_eq!(labels("{ users { ... on User { i|"), ["id"]);
    }

    #[test]
    fn arguments_values_and_variables() {
        assert_eq!(labels("{ user(|"), ["id", "role"]);
        assert_eq!(
            labels("{ user(id: 1, |"),
            ["role"],
            "given ones are left out"
        );
        assert_eq!(labels("{ user(role: |"), ["ADMIN", "GUEST"]);
        assert_eq!(labels("{ user(role: G|"), ["GUEST"]);
        assert_eq!(labels("mutation { createUser(admin: |"), ["true", "false"]);
        assert_eq!(
            labels("query Q($id: ID!, $r: Role) { user(id: $|"),
            ["$id", "$r"]
        );
        assert_eq!(labels("query Q($id: ID!) { user(id: |"), ["$id"]);
        assert_eq!(
            labels("query Q($id: |"),
            ["Role", "ID", "String", "Boolean"]
        );
        assert_eq!(labels("{ users { ... on |"), ["Query", "Mutation", "User"]);
    }

    #[test]
    fn keywords_at_the_top_and_nothing_where_it_cannot_tell() {
        assert_eq!(
            labels("|"),
            ["query", "mutation", "subscription", "fragment"]
        );
        assert_eq!(labels("{ user { id } }\nmu|"), ["mutation"]);
        assert_eq!(
            labels("{ nope { |"),
            Vec::<String>::new(),
            "an unknown field"
        );
        assert_eq!(
            labels("{ user(id: \"{ x |"),
            Vec::<String>::new(),
            "inside a string"
        );
        assert_eq!(
            labels("# { x\n{ |"),
            ["user", "users", "__typename"],
            "a comment is skipped"
        );
        assert_eq!(labels("{ # use|"), Vec::<String>::new(), "inside a comment");
        let at = complete("{ use", 5, &schema());
        assert_eq!(at.start, 2, "the word replaced");
    }
}
