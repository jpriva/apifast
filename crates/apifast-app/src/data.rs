//! Data files for data-driven runs (docs/08): a CSV with a header row or a
//! JSON array of objects. Each row is one iteration, its columns variables.

use apifast_domain::{Json, Variable};

use crate::variable_value;

/// The format of a data file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    /// RFC 4180: a header row, commas, fields in double quotes when needed.
    Csv,
    /// An array of objects.
    Json,
}

impl DataFormat {
    /// The format a file name says, by its extension.
    pub fn of(file_name: &str) -> Option<Self> {
        let extension = file_name.rsplit_once('.')?.1;
        if extension.eq_ignore_ascii_case("csv") {
            Some(Self::Csv)
        } else if extension.eq_ignore_ascii_case("json") {
            Some(Self::Json)
        } else {
            None
        }
    }
}

/// Why a data file cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DataError(pub String);

/// The rows of a data file, each one the variables of one iteration.
///
/// # Errors
///
/// If the text is not valid in its format, or has no rows.
pub fn parse_data(text: &str, format: DataFormat) -> Result<Vec<Vec<Variable>>, DataError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rows = match format {
        DataFormat::Csv => csv_rows(text)?,
        DataFormat::Json => json_rows(text)?,
    };
    if rows.is_empty() {
        return Err(DataError("the data file has no rows".into()));
    }
    Ok(rows)
}

fn json_rows(text: &str) -> Result<Vec<Vec<Variable>>, DataError> {
    let json = Json::parse(text).map_err(|e| DataError(e.to_string()))?;
    let Json::Array(items) = json else {
        return Err(DataError("expected an array of objects".into()));
    };
    items
        .iter()
        .enumerate()
        .map(|(i, item)| match item {
            Json::Object(pairs) => Ok(pairs
                .iter()
                .map(|(name, value)| Variable {
                    name: name.clone(),
                    value: Some(variable_value(value)),
                    enabled: true,
                    secret: false,
                })
                .collect()),
            other => Err(DataError(format!(
                "item {} is {}, not an object",
                i + 1,
                other.type_name()
            ))),
        })
        .collect()
}

fn csv_rows(text: &str) -> Result<Vec<Vec<Variable>>, DataError> {
    let mut records = csv_records(text)?.into_iter();
    let Some((_, header)) = records.next() else {
        return Ok(Vec::new());
    };
    let header: Vec<String> = header.into_iter().map(|h| h.trim().to_owned()).collect();
    if let Some(i) = header.iter().position(String::is_empty) {
        return Err(DataError(format!("column {} has no name", i + 1)));
    }
    for (i, name) in header.iter().enumerate() {
        if header[..i].contains(name) {
            return Err(DataError(format!("the column `{name}` appears twice")));
        }
    }
    records
        .map(|(line, fields)| {
            if fields.len() != header.len() {
                return Err(DataError(format!(
                    "line {line} has {} fields; the header has {}",
                    fields.len(),
                    header.len()
                )));
            }
            Ok(header
                .iter()
                .zip(fields)
                .map(|(name, value)| Variable::text(name.clone(), value))
                .collect())
        })
        .collect()
}

/// The records of a CSV text, with the line each starts on. Blank lines
/// are skipped; quoted fields may hold commas, quotes (`""`) and newlines.
fn csv_records(text: &str) -> Result<Vec<(usize, Vec<String>)>, DataError> {
    let mut records = Vec::new();
    let mut chars = text.chars().peekable();
    let mut line = 1;
    while chars.peek().is_some() {
        let start = line;
        let mut fields = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        loop {
            match chars.next() {
                None => break,
                Some('"') if quoted => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        quoted = false;
                    }
                }
                Some('"') if field.is_empty() => quoted = true,
                Some(c) if quoted => {
                    if c == '\n' {
                        line += 1;
                    }
                    field.push(c);
                }
                Some(',') => fields.push(std::mem::take(&mut field)),
                Some('\r') if chars.peek() == Some(&'\n') => {}
                Some('\n') => {
                    line += 1;
                    break;
                }
                Some(c) => field.push(c),
            }
        }
        if quoted {
            return Err(DataError(format!("line {start}: a quote is not closed")));
        }
        fields.push(field);
        if !(fields.len() == 1 && fields[0].is_empty()) {
            records.push((start, fields));
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use apifast_domain::ValueKind;

    fn texts(rows: &[Vec<Variable>]) -> Vec<Vec<(String, String)>> {
        rows.iter()
            .map(|row| {
                row.iter()
                    .map(|v| {
                        let value = v.value.as_ref().map(|v| v.data.clone()).unwrap_or_default();
                        (v.name.clone(), value)
                    })
                    .collect()
            })
            .collect()
    }

    fn pairs(row: &[(&str, &str)]) -> Vec<(String, String)> {
        row.iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    }

    #[test]
    fn formats_follow_the_extension() {
        assert_eq!(DataFormat::of("users.CSV"), Some(DataFormat::Csv));
        assert_eq!(DataFormat::of("dir.v2/rows.json"), Some(DataFormat::Json));
        assert_eq!(DataFormat::of("rows.txt"), None);
        assert_eq!(DataFormat::of("rows"), None);
    }

    #[test]
    fn csv_with_quotes_newlines_and_crlf() {
        let text = "\u{feff}name, email\r\n\"Ana, M.\",ana@x.io\r\n\r\n\"say \"\"hi\"\"\",\"two\nlines\"\nBo,\n";
        let rows = parse_data(text, DataFormat::Csv).unwrap();
        assert_eq!(
            texts(&rows),
            [
                pairs(&[("name", "Ana, M."), ("email", "ana@x.io")]),
                pairs(&[("name", "say \"hi\""), ("email", "two\nlines")]),
                pairs(&[("name", "Bo"), ("email", "")]),
            ]
        );
    }

    #[test]
    fn csv_errors_name_the_line() {
        let error = |text: &str| parse_data(text, DataFormat::Csv).unwrap_err().0;
        assert_eq!(error("a,b\n1\n"), "line 2 has 1 fields; the header has 2");
        assert_eq!(error("a,b\n1,2\n\"x\n3,4"), "line 3: a quote is not closed");
        assert_eq!(error("a,,c\n1,2,3"), "column 2 has no name");
        assert_eq!(error("a,a\n1,2"), "the column `a` appears twice");
        assert_eq!(error("a,b\n"), "the data file has no rows");
        assert_eq!(error(""), "the data file has no rows");
    }

    #[test]
    fn json_objects_become_rows_with_typed_values() {
        let rows = parse_data(
            r#"[{"id": 7, "user": {"name": "Ana"}}, {"id": "x"}]"#,
            DataFormat::Json,
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0][0].value.as_ref().map(|v| (v.kind, v.data.as_str())),
            Some((ValueKind::Number, "7"))
        );
        let user = rows[0][1].value.as_ref().unwrap();
        assert_eq!(user.data, r#"{"name":"Ana"}"#);
        let error = |text: &str| parse_data(text, DataFormat::Json).unwrap_err().0;
        assert_eq!(error(r#"{"a": 1}"#), "expected an array of objects");
        assert_eq!(error("[{}, 3]"), "item 2 is number, not an object");
        assert_eq!(error("[]"), "the data file has no rows");
        assert!(error("[").starts_with("not JSON"));
    }
}
