//! The `.bru` format of Bruno (`bruno-lang/v2/src/bruToJson.js`): a file
//! is a list of blocks, each a dictionary (`headers { name: value }`), a
//! text (`body:json { … }`, indented two spaces) or a list
//! (`vars:secret [ a, b ]`). A block ends at a `}` (or `]`) at the start
//! of a line.

/// A pair of a dictionary block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pair {
    pub key: String,
    pub value: String,
    /// `false` when the key starts with `~`.
    pub enabled: bool,
    /// The type of its value from an annotation (`@number`, `@boolean`,
    /// `@object`), if any.
    pub kind: Option<String>,
}

/// What a block holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Block {
    Dict(Vec<Pair>),
    Text(String),
    List(Vec<String>),
}

/// A `.bru` file: its blocks by name, in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct BruFile {
    pub blocks: Vec<(String, Block)>,
}

/// Blocks whose content is text, not pairs.
fn is_text(name: &str) -> bool {
    matches!(name, "body" | "tests" | "docs" | "example")
        || name.starts_with("script:")
        || (name.starts_with("body:")
            && !matches!(
                name,
                "body:form-urlencoded"
                    | "body:multipart-form"
                    | "body:file"
                    | "body:grpc"
                    | "body:ws"
            ))
}

impl BruFile {
    /// The first block named `name`.
    pub fn block(&self, name: &str) -> Option<&Block> {
        self.blocks.iter().find(|(n, _)| n == name).map(|(_, b)| b)
    }

    /// The pairs of the dictionary block `name`; none if it is not one.
    pub fn pairs(&self, name: &str) -> &[Pair] {
        match self.block(name) {
            Some(Block::Dict(pairs)) => pairs,
            _ => &[],
        }
    }

    /// The value of `key` in the dictionary block `name`.
    pub fn get(&self, name: &str, key: &str) -> Option<&str> {
        (self.pairs(name).iter())
            .find(|p| p.key == key)
            .map(|p| p.value.as_str())
    }

    /// The text of the text block `name`.
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.block(name) {
            Some(Block::Text(text)) => Some(text),
            _ => None,
        }
    }
}

/// Why a file is not `.bru`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParseError {
    pub line: usize,
    pub message: String,
}

/// The blocks of `text`.
pub(crate) fn parse(text: &str) -> Result<BruFile, ParseError> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut file = BruFile::default();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }
        let error = |message: &str| ParseError {
            line: i + 1,
            message: message.to_owned(),
        };
        let (name, open) = if let Some(name) = line.strip_suffix('{') {
            (name.trim().to_owned(), '{')
        } else if let Some(name) = line.strip_suffix('[') {
            (name.trim().to_owned(), '[')
        } else {
            return Err(error("a block was expected, as `name {`"));
        };
        let close = if open == '{' { "}" } else { "]" };
        let start = i + 1;
        let end = (start..lines.len())
            .find(|&j| lines[j].starts_with(close))
            .ok_or_else(|| error(&format!("the block `{name}` is not closed")))?;
        let body = &lines[start..end];
        let block = if open == '[' {
            Block::List(list_items(body))
        } else if is_text(&name) {
            Block::Text(outdent(body, 2))
        } else {
            Block::Dict(pairs(body).map_err(|(offset, message)| ParseError {
                line: start + offset + 1,
                message,
            })?)
        };
        file.blocks.push((name, block));
        i = end + 1;
    }
    Ok(file)
}

/// The items of a list: one per line, without a trailing comma.
fn list_items(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.trim().trim_end_matches(',').trim().to_owned())
        .filter(|l| !l.is_empty())
        .collect()
}

/// `lines` with `spaces` leading spaces taken from each, joined.
fn outdent(lines: &[&str], spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    lines
        .iter()
        .map(|l| l.strip_prefix(prefix.as_str()).unwrap_or(l.trim_start()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The pairs of a dictionary's lines, or the line (from 0) that is wrong.
fn pairs(lines: &[&str]) -> Result<Vec<Pair>, (usize, String)> {
    let mut out = Vec::new();
    let mut kind = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }
        if let Some(annotation) = line.strip_prefix('@') {
            // `@description('''…''')` may go on for several lines.
            if annotation.contains("('''") && !annotation.ends_with("''')") {
                while i + 1 < lines.len() && !lines[i].trim_end().ends_with("''')") {
                    i += 1;
                }
            }
            let name = annotation.split('(').next().unwrap_or_default();
            if matches!(name, "number" | "boolean" | "object" | "string") {
                kind = Some(name.to_owned());
            }
            i += 1;
            continue;
        }
        let (key, rest) = split_key(line).ok_or((i, format!("`{line}` has no `:`")))?;
        let (key, enabled) = match key.strip_prefix('~') {
            Some(key) => (key.to_owned(), false),
            None => (key, true),
        };
        let mut value = rest.trim().to_owned();
        if value.starts_with("'''") && !(value.len() > 3 && value.ends_with("'''")) {
            // A value of several lines, `'''` to `'''`, indented four.
            let start = i + 1;
            let end = (start..lines.len())
                .find(|&j| lines[j].trim().starts_with("'''"))
                .ok_or((i, format!("the value of `{key}` is not closed with '''")))?;
            value = outdent(&lines[start..end], 4);
            i = end;
        } else if let Some(inner) = value
            .strip_prefix("'''")
            .and_then(|v| v.strip_suffix("'''"))
        {
            value = inner.to_string();
        } else if value == "[" {
            let start = i + 1;
            let end = (start..lines.len())
                .find(|&j| lines[j].trim() == "]")
                .ok_or((i, format!("the list of `{key}` is not closed")))?;
            value = list_items(&lines[start..end]).join(",");
            i = end;
        }
        out.push(Pair {
            key,
            value,
            enabled,
            kind: kind.take(),
        });
        i += 1;
    }
    Ok(out)
}

/// A pair's key and the rest of its line: up to the first `:`, or a
/// quoted key (`"a: b": value`).
fn split_key(line: &str) -> Option<(String, &str)> {
    let (disabled, quoted) = match line.strip_prefix('~') {
        Some(rest) => ("~", rest),
        None => ("", line),
    };
    if let Some(rest) = quoted.strip_prefix('"') {
        let mut key = String::new();
        let mut chars = rest.char_indices();
        while let Some((at, c)) = chars.next() {
            match c {
                '\\' => {
                    if let Some((_, next)) = chars.next() {
                        key.push(next);
                    }
                }
                '"' => {
                    let after = rest[at + 1..].trim_start().strip_prefix(':')?;
                    return Some((format!("{disabled}{key}"), after));
                }
                other => key.push(other),
            }
        }
        return None;
    }
    let (key, rest) = line.split_once(':')?;
    Some((key.trim().to_owned(), rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST: &str = "meta {\n  name: Create user\n  type: http\n  seq: 2\n}\n\npost {\n  url: {{host}}/users?a=1\n  body: json\n  auth: bearer\n}\n\nheaders {\n  @description('''\n    several\n    lines\n  ''')\n  content-type: application/json\n  ~x-off: 1\n  \"weird: key\": v\n}\n\nbody:json {\n  {\n    \"name\": \"ana\"\n  }\n}\n\nvars:pre-request {\n  @number\n  n: 3\n  obj: '''\n    {\"a\": 1}\n  '''\n  one: '''inline'''\n}\n\nvars:secret [\n  token,\n  other\n]\n";

    #[test]
    fn blocks_are_read_by_kind() {
        let file = parse(REQUEST).unwrap();
        assert_eq!(file.get("meta", "name"), Some("Create user"));
        assert_eq!(file.get("post", "url"), Some("{{host}}/users?a=1"));
        assert_eq!(file.text("body:json"), Some("{\n  \"name\": \"ana\"\n}"));
        let headers = file.pairs("headers");
        assert_eq!(headers.len(), 3);
        assert_eq!(
            (headers[0].key.as_str(), headers[0].value.as_str()),
            ("content-type", "application/json")
        );
        assert_eq!(
            (headers[1].key.as_str(), headers[1].enabled),
            ("x-off", false)
        );
        assert_eq!(headers[2].key, "weird: key");
        let vars = file.pairs("vars:pre-request");
        assert_eq!(vars[0].kind.as_deref(), Some("number"));
        assert_eq!(vars[1].value, "{\"a\": 1}");
        assert_eq!(vars[1].kind, None);
        assert_eq!(vars[2].value, "inline");
        assert_eq!(
            file.block("vars:secret"),
            Some(&Block::List(vec!["token".into(), "other".into()]))
        );
    }

    #[test]
    fn broken_files_say_where() {
        assert_eq!(parse("meta {\n  name: x\n").unwrap_err().line, 1);
        assert_eq!(parse("not a block").unwrap_err().line, 1);
        assert_eq!(
            parse("headers {\n  no colon here\n}\n").unwrap_err().line,
            2
        );
        assert_eq!(parse("").unwrap(), BruFile::default());
    }
}
