//! Formats JSON and XML: one item per line, indented by two spaces.
//!
//! The formatters only move whitespace, so they also format text that is not
//! quite valid, such as JSON with `{{id}}` where a value goes. They refuse
//! only when the structure is broken: brackets or tags that do not match, or
//! a string or tag that never ends.

use std::fmt;

use super::{Language, tag_len};

const INDENT: &str = "  ";

/// Why a text could not be formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatError {
    /// Byte where the problem was found.
    pub offset: usize,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the structure is broken at byte {}", self.offset)
    }
}

impl std::error::Error for FormatError {}

/// `text` formatted as `language`. Languages that are not formatted (see
/// [`Language::formats`]) come back unchanged.
///
/// # Errors
///
/// When brackets or tags do not match, or a string, tag or comment is not
/// closed: the error tells where.
pub fn format(text: &str, language: Language) -> Result<String, FormatError> {
    match language {
        Language::Json => json(text),
        Language::Xml => xml(text),
        Language::Sparql | Language::Text => Ok(text.to_owned()),
    }
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str(INDENT);
    }
}

/// Splits JSON into strings, brackets, `,`, `:`, template tags and runs of
/// anything else; whitespace between them is dropped.
fn json_items(text: &str) -> Result<Vec<(usize, &str)>, FormatError> {
    let mut items = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let c = rest.as_bytes()[0];
        let len = if c.is_ascii_whitespace() {
            at += 1;
            continue;
        } else if let Some(len) = tag_len(rest) {
            if !rest[..len].ends_with("}}") {
                return Err(FormatError { offset: at });
            }
            len
        } else if c == b'"' {
            closed_string_len(rest).ok_or(FormatError { offset: at })?
        } else if matches!(c, b'{' | b'}' | b'[' | b']' | b',' | b':') {
            1
        } else {
            rest.find(|c: char| {
                c.is_ascii_whitespace() || matches!(c, '{' | '}' | '[' | ']' | ',' | ':' | '"')
            })
            .unwrap_or(rest.len())
        };
        items.push((at, &rest[..len]));
        at += len;
    }
    Ok(items)
}

/// Length of the JSON string at the start of `rest`, if it is closed.
fn closed_string_len(rest: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, b) in rest.bytes().enumerate().skip(1) {
        match b {
            _ if escaped => escaped = false,
            b'\\' => escaped = true,
            b'"' => return Some(i + 1),
            _ => {}
        }
    }
    None
}

fn json(text: &str) -> Result<String, FormatError> {
    let items = json_items(text)?;
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut open: Vec<&str> = Vec::new();
    let mut iter = items.iter().peekable();
    while let Some(&(offset, item)) = iter.next() {
        match item {
            "{" | "[" => {
                let close = if item == "{" { "}" } else { "]" };
                // Empty: `{}` on one line.
                if iter.peek().is_some_and(|(_, next)| *next == close) {
                    iter.next();
                    out.push_str(item);
                    out.push_str(close);
                } else {
                    out.push_str(item);
                    open.push(close);
                    newline(&mut out, open.len());
                }
            }
            "}" | "]" => {
                if open.pop() != Some(item) {
                    return Err(FormatError { offset });
                }
                newline(&mut out, open.len());
                out.push_str(item);
            }
            "," => {
                out.push(',');
                newline(&mut out, open.len());
            }
            ":" => out.push_str(": "),
            _ => out.push_str(item),
        }
    }
    if open.is_empty() {
        Ok(out)
    } else {
        Err(FormatError { offset: text.len() })
    }
}

/// A part of an XML document.
#[derive(Debug, PartialEq, Eq)]
enum XmlItem<'a> {
    /// `<name ...>`, with its name.
    Open(&'a str, &'a str),
    /// `</name>`, with its name.
    Close(&'a str, &'a str),
    /// A tag that stands alone: `<a/>`, `<?xml ...?>`, `<!-- -->`,
    /// `<!DOCTYPE>` or CDATA.
    Single(&'a str),
    /// Text between tags, trimmed.
    Text(&'a str),
}

/// End of the XML tag that starts at `rest`, skipping quoted values.
fn xml_tag_len(rest: &str) -> Option<usize> {
    let mut quote = None;
    for (i, b) in rest.bytes().enumerate().skip(1) {
        match (quote, b) {
            (Some(q), _) if b == q => quote = None,
            (None, b'"' | b'\'') => quote = Some(b),
            (None, b'>') => return Some(i + 1),
            _ => {}
        }
    }
    None
}

fn xml_items(text: &str) -> Result<Vec<(usize, XmlItem<'_>)>, FormatError> {
    let mut items = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let broken = FormatError { offset: at };
        let (len, item) = if !rest.starts_with('<') {
            let len = rest.find('<').unwrap_or(rest.len());
            let trimmed = rest[..len].trim();
            at += len;
            if !trimmed.is_empty() {
                items.push((at - len, XmlItem::Text(trimmed)));
            }
            continue;
        } else if let Some(end) = single_end(rest) {
            let len = rest.find(end).ok_or(broken)? + end.len();
            (len, XmlItem::Single(&rest[..len]))
        } else {
            let len = xml_tag_len(rest).ok_or(broken)?;
            let tag = &rest[..len];
            let name_of = |from: usize| {
                let name = &tag[from..];
                &name[..name
                    .find(|c: char| c.is_whitespace() || matches!(c, '>' | '/'))
                    .unwrap_or(name.len())]
            };
            let item = if tag.starts_with("</") {
                XmlItem::Close(tag, name_of(2))
            } else if tag.starts_with("<?") || tag.starts_with("<!") || tag.ends_with("/>") {
                XmlItem::Single(tag)
            } else {
                XmlItem::Open(tag, name_of(1))
            };
            (len, item)
        };
        items.push((at, item));
        at += len;
    }
    Ok(items)
}

fn xml(text: &str) -> Result<String, FormatError> {
    let items = xml_items(text)?;
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut open: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let (offset, item) = &items[i];
        if !out.is_empty() {
            let closing = matches!(item, XmlItem::Close(..));
            newline(&mut out, open.len().saturating_sub(usize::from(closing)));
        }
        match item {
            XmlItem::Open(tag, name) => {
                out.push_str(tag);
                // `<a>text</a>` and `<a></a>` stay on one line.
                let closes = |at: usize| matches!(items.get(at), Some((_, XmlItem::Close(_, close))) if close == name);
                let inline = match items.get(i + 1) {
                    Some((_, XmlItem::Text(text))) if closes(i + 2) => Some((*text, i + 2)),
                    _ if closes(i + 1) => Some(("", i + 1)),
                    _ => None,
                };
                if let Some((text, close)) = inline {
                    out.push_str(text);
                    if let XmlItem::Close(tag, _) = items[close].1 {
                        out.push_str(tag);
                    }
                    i = close + 1;
                    continue;
                }
                open.push(name);
            }
            XmlItem::Close(tag, name) => {
                if open.pop() != Some(*name) {
                    return Err(FormatError { offset: *offset });
                }
                out.push_str(tag);
            }
            XmlItem::Single(tag) | XmlItem::Text(tag) => out.push_str(tag),
        }
        i += 1;
    }
    if open.is_empty() {
        Ok(out)
    } else {
        Err(FormatError { offset: text.len() })
    }
}

/// The end of a comment or CDATA starting at `rest`, if one does.
fn single_end(rest: &str) -> Option<&'static str> {
    if rest.starts_with("<!--") {
        Some("-->")
    } else if rest.starts_with("<![CDATA[") {
        Some("]]>")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_indented_by_two_spaces() {
        let formatted = format(
            r#"{"a":1,"b":[true,{"c":"x, y: {z}"}],"d":{},"e":[ ]}"#,
            Language::Json,
        )
        .unwrap();
        assert_eq!(
            formatted,
            r#"{
  "a": 1,
  "b": [
    true,
    {
      "c": "x, y: {z}"
    }
  ],
  "d": {},
  "e": []
}"#
        );
    }

    #[test]
    fn json_with_template_tags_is_formatted() {
        let formatted = format(r#"{"id":{{id}},"at":{{$timestamp}}}"#, Language::Json).unwrap();
        assert_eq!(
            formatted,
            "{\n  \"id\": {{id}},\n  \"at\": {{$timestamp}}\n}"
        );
    }

    #[test]
    fn formatting_twice_changes_nothing() {
        let once = format(r#"[{"a" : [1,2]}, "b"]"#, Language::Json).unwrap();
        assert_eq!(format(&once, Language::Json).unwrap(), once);
        let once = format("<a><b>x</b><c/></a>", Language::Xml).unwrap();
        assert_eq!(format(&once, Language::Xml).unwrap(), once);
    }

    #[test]
    fn broken_json_says_where() {
        assert_eq!(
            format(r#"{"a": [1}"#, Language::Json),
            Err(FormatError { offset: 8 })
        );
        assert_eq!(
            format(r#"{"a": "x"#, Language::Json),
            Err(FormatError { offset: 6 })
        );
        assert_eq!(format("[1", Language::Json), Err(FormatError { offset: 2 }));
        assert_eq!(
            format("{{id", Language::Json),
            Err(FormatError { offset: 0 })
        );
    }

    #[test]
    fn xml_is_indented_and_short_elements_stay_on_one_line() {
        let formatted = format(
            "<?xml version=\"1.0\"?><a x=\"1 > 0\"><b>hi {{name}}</b><c></c><!-- n --><d/>  </a>",
            Language::Xml,
        )
        .unwrap();
        assert_eq!(
            formatted,
            "<?xml version=\"1.0\"?>\n<a x=\"1 > 0\">\n  <b>hi {{name}}</b>\n  <c></c>\n  <!-- n -->\n  <d/>\n</a>"
        );
    }

    #[test]
    fn broken_xml_says_where() {
        assert_eq!(
            format("<a><b></a>", Language::Xml),
            Err(FormatError { offset: 6 })
        );
        assert_eq!(format("<a", Language::Xml), Err(FormatError { offset: 0 }));
        assert_eq!(format("<a>", Language::Xml), Err(FormatError { offset: 3 }));
    }

    #[test]
    fn text_is_not_formatted() {
        assert_eq!(format(" x ", Language::Text).unwrap(), " x ");
    }
}
