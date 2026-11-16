//! Reports of a run (docs/08): JSON to process, JUnit XML for CI and a
//! self-contained HTML page. Pure text from a [`RunReport`]; the driver
//! writes it where asked.
//!
//! Secrets stay masked: assertion values come masked from the send, and a
//! captured secret is written as [`HIDDEN`](crate::HIDDEN). Request URLs are
//! left out, since their query may carry secrets.

use std::fmt::Write as _;

use apifast_domain::Json;
use apifast_template::iso_timestamp;

use crate::{
    AssertionOutcome, CaptureOutcome, HIDDEN, RunReport, RunSummary, SendOutcome, StepResult,
};

/// What a report says about the run besides its results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportMeta {
    /// The collection's name.
    pub collection: String,
    /// The active environment.
    pub environment: Option<String>,
    /// When the run started, in milliseconds since the Unix epoch.
    pub started_ms: i64,
}

/// The kinds of report files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    /// The whole result as JSON.
    Json,
    /// JUnit XML, as CI systems read it.
    Junit,
    /// A self-contained HTML page.
    Html,
}

impl ReportFormat {
    /// The format named `name`: `json`, `junit` or `html`.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "junit" | "xml" => Some(Self::Junit),
            "html" => Some(Self::Html),
            _ => None,
        }
    }

    /// The report of `report` in this format.
    pub fn render(self, report: &RunReport, meta: &ReportMeta) -> String {
        match self {
            Self::Json => json_report(report, meta),
            Self::Junit => junit_report(report, meta),
            Self::Html => html_report(report, meta),
        }
    }
}

fn text(value: &str) -> Json {
    Json::String(value.to_owned())
}

fn number(value: impl std::fmt::Display) -> Json {
    Json::Number(value.to_string())
}

fn object(pairs: Vec<(&str, Json)>) -> Json {
    Json::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn optional(value: Option<&str>) -> Json {
    value.map_or(Json::Null, text)
}

/// A captured value as a report shows it: masked when secret.
fn captured(capture: &CaptureOutcome) -> Result<String, String> {
    match &capture.value {
        Ok(_) if capture.secret => Ok(HIDDEN.to_owned()),
        Ok(value) => Ok(value.data.clone()),
        Err(reason) => Err(reason.clone()),
    }
}

/// One line for an assertion: `res.status eq 200`.
pub fn assertion_text(outcome: &AssertionOutcome) -> String {
    let assertion = &outcome.assertion;
    let mut line = format!(
        "{} {}",
        assertion.expression.trim(),
        assertion.operator.as_str()
    );
    if let Some(expected) = &outcome.expected {
        line.push(' ');
        line.push_str(expected);
    }
    line
}

fn summary_json(summary: &RunSummary) -> Json {
    object(vec![
        ("requests", number(summary.requests)),
        ("passed", number(summary.passed)),
        ("failed", number(summary.failed)),
        ("errors", number(summary.errors)),
        ("skipped", number(summary.skipped)),
        (
            "assertions",
            object(vec![
                ("passed", number(summary.assertions_passed)),
                ("failed", number(summary.assertions_failed)),
            ]),
        ),
    ])
}

fn step_json(step: &StepResult) -> Json {
    let mut pairs = vec![
        ("iteration", number(step.iteration)),
        ("path", text(step.path.as_str())),
        ("name", text(&step.name)),
        ("passed", Json::Bool(step.passed())),
        ("durationMs", number(step.duration_ms)),
    ];
    match &step.outcome {
        Err(error) => pairs.push(("error", text(error))),
        Ok(outcome) => {
            pairs.extend(outcome_json(outcome));
        }
    }
    object(pairs)
}

fn outcome_json(outcome: &SendOutcome) -> Vec<(&'static str, Json)> {
    let assertions = outcome
        .assertions
        .iter()
        .map(|a| {
            object(vec![
                ("expression", text(a.assertion.expression.trim())),
                ("operator", text(a.assertion.operator.as_str())),
                ("expected", optional(a.expected.as_deref())),
                ("passed", Json::Bool(a.verdict.passed)),
                ("actual", text(&a.verdict.actual)),
                ("message", optional(a.verdict.message.as_deref())),
            ])
        })
        .collect();
    let captures = outcome
        .captures
        .iter()
        .map(|c| {
            let (value, error) = match captured(c) {
                Ok(value) => (text(&value), Json::Null),
                Err(reason) => (Json::Null, text(&reason)),
            };
            object(vec![
                ("variable", text(&c.variable)),
                ("scope", text(c.scope.as_str())),
                ("value", value),
                ("error", error),
            ])
        })
        .collect();
    vec![
        ("status", number(outcome.head.status)),
        ("responseTimeMs", number(outcome.head.elapsed_ms)),
        ("size", number(outcome.body.size)),
        ("assertions", Json::Array(assertions)),
        ("captures", Json::Array(captures)),
    ]
}

/// The whole result as JSON.
pub fn json_report(report: &RunReport, meta: &ReportMeta) -> String {
    let skipped = report
        .skipped
        .iter()
        .map(|s| {
            object(vec![
                ("path", text(s.path.as_str())),
                ("name", text(&s.name)),
                ("reason", text(&s.reason)),
            ])
        })
        .collect();
    let json = object(vec![
        ("collection", text(&meta.collection)),
        ("environment", optional(meta.environment.as_deref())),
        ("startedAt", text(&iso_timestamp(meta.started_ms))),
        ("durationMs", number(report.duration_ms)),
        ("succeeded", Json::Bool(report.succeeded())),
        ("bailed", Json::Bool(report.bailed)),
        ("cancelled", Json::Bool(report.cancelled)),
        ("summary", summary_json(&report.summary())),
        (
            "results",
            Json::Array(report.steps.iter().map(step_json).collect()),
        ),
        ("skipped", Json::Array(skipped)),
    ]);
    let mut out = json.to_string();
    out.push('\n');
    out
}

/// Escapes text for XML and HTML, also in attributes. Characters XML 1.0
/// does not allow are dropped.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c < ' ' || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

fn seconds(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

/// The name of a step: its name, and its iteration when there are several.
fn step_name(step: &StepResult, iterations: bool) -> String {
    if iterations {
        format!("{} [{}]", step.path, step.iteration)
    } else {
        step.path.to_string()
    }
}

/// JUnit XML: a test suite per request sent, with a test case per
/// assertion (or one for the request when it has none), and one suite for
/// the skipped requests.
pub fn junit_report(report: &RunReport, meta: &ReportMeta) -> String {
    let summary = report.summary();
    let iterations = report.steps.iter().any(|s| s.iteration > 1);
    let mut suites = String::new();
    let mut tests = 0;
    for step in &report.steps {
        let name = step_name(step, iterations);
        let time = seconds(step.duration_ms);
        let mut cases = String::new();
        let (mut count, mut failures, mut errors) = (0, 0, 0);
        match &step.outcome {
            Err(error) => {
                count += 1;
                errors += 1;
                let _ = write!(
                    cases,
                    "      <testcase name=\"request\" classname=\"{}\" time=\"{time}\">\n        <error message=\"{}\"/>\n      </testcase>\n",
                    escape(&name),
                    escape(error)
                );
            }
            Ok(outcome) if outcome.assertions.is_empty() => {
                count += 1;
                let _ = writeln!(
                    cases,
                    "      <testcase name=\"request\" classname=\"{}\" time=\"{time}\"/>",
                    escape(&name)
                );
            }
            Ok(outcome) => {
                for assertion in &outcome.assertions {
                    count += 1;
                    let case = escape(&assertion_text(assertion));
                    match &assertion.verdict.message {
                        None => {
                            let _ = writeln!(
                                cases,
                                "      <testcase name=\"{case}\" classname=\"{}\" time=\"0\"/>",
                                escape(&name)
                            );
                        }
                        Some(message) => {
                            failures += 1;
                            let _ = write!(
                                cases,
                                "      <testcase name=\"{case}\" classname=\"{}\" time=\"0\">\n        <failure message=\"{}\" type=\"assertion\"/>\n      </testcase>\n",
                                escape(&name),
                                escape(message)
                            );
                        }
                    }
                }
            }
        }
        tests += count;
        let _ = write!(
            suites,
            "    <testsuite name=\"{}\" tests=\"{count}\" failures=\"{failures}\" errors=\"{errors}\" skipped=\"0\" time=\"{time}\">\n{cases}    </testsuite>\n",
            escape(&name)
        );
    }
    if !report.skipped.is_empty() {
        let mut cases = String::new();
        for skipped in &report.skipped {
            let _ = write!(
                cases,
                "      <testcase name=\"{}\" classname=\"skipped\" time=\"0\">\n        <skipped message=\"{}\"/>\n      </testcase>\n",
                escape(skipped.path.as_str()),
                escape(&skipped.reason)
            );
        }
        tests += report.skipped.len();
        let _ = write!(
            suites,
            "    <testsuite name=\"skipped\" tests=\"{0}\" failures=\"0\" errors=\"0\" skipped=\"{0}\" time=\"0\">\n{cases}    </testsuite>\n",
            report.skipped.len()
        );
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"{}\" tests=\"{tests}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{}\" timestamp=\"{}\">\n{suites}</testsuites>\n",
        escape(&meta.collection),
        summary.assertions_failed,
        summary.errors,
        summary.skipped,
        seconds(report.duration_ms),
        iso_timestamp(meta.started_ms),
    )
}

const STYLE: &str = "
:root { --bg: #fff; --fg: #1d2025; --muted: #646b76; --line: #dfe2e7; --pass: #1a7f37; --fail: #c62828; --skip: #8a6d00; --card: #f6f7f9; }
@media (prefers-color-scheme: dark) { :root { --bg: #16181c; --fg: #e6e8eb; --muted: #9aa1ab; --line: #2c3036; --pass: #4ac26b; --fail: #ff6b6b; --skip: #e3b341; --card: #1e2126; } }
* { box-sizing: border-box; }
body { margin: 0; padding: 24px 16px; background: var(--bg); color: var(--fg); font: 14px/1.5 system-ui, sans-serif; }
main { max-width: 960px; margin: 0 auto; }
h1 { font-size: 20px; margin: 0 0 4px; }
.meta, .muted { color: var(--muted); }
.totals { display: flex; flex-wrap: wrap; gap: 8px; margin: 16px 0; }
.totals div { background: var(--card); border: 1px solid var(--line); border-radius: 8px; padding: 8px 12px; min-width: 96px; }
.totals b { display: block; font-size: 18px; }
details { border: 1px solid var(--line); border-radius: 8px; margin: 8px 0; background: var(--card); }
summary { cursor: pointer; padding: 8px 12px; display: flex; gap: 12px; align-items: baseline; flex-wrap: wrap; }
summary .name { font-weight: 600; flex: 1; min-width: 12em; overflow-wrap: anywhere; }
.body { padding: 0 12px 12px; overflow-x: auto; }
table { border-collapse: collapse; width: 100%; }
td, th { text-align: left; padding: 4px 8px; border-top: 1px solid var(--line); vertical-align: top; overflow-wrap: anywhere; }
.pass { color: var(--pass); } .fail { color: var(--fail); } .skip { color: var(--skip); }
code { font: 13px ui-monospace, monospace; }
";

/// A self-contained HTML page: totals, then each request with its
/// assertions and captures.
pub fn html_report(report: &RunReport, meta: &ReportMeta) -> String {
    let summary = report.summary();
    let iterations = report.steps.iter().any(|s| s.iteration > 1);
    let (verdict, class) = if report.succeeded() {
        ("Passed", "pass")
    } else {
        ("Failed", "fail")
    };
    let mut body = String::new();
    let _ = write!(
        body,
        "<h1>{}</h1>\n<p class=\"meta\">{}{} · {} · <span class=\"{class}\">{verdict}</span>{}</p>\n",
        escape(&meta.collection),
        iso_timestamp(meta.started_ms),
        meta.environment
            .as_deref()
            .map(|e| format!(" · environment {}", escape(e)))
            .unwrap_or_default(),
        duration(report.duration_ms),
        if report.bailed {
            " · stopped at the first failure"
        } else if report.cancelled {
            " · cancelled"
        } else {
            ""
        }
    );
    let _ = writeln!(
        body,
        "<div class=\"totals\"><div><b>{}</b>requests</div><div><b class=\"pass\">{}</b>passed</div><div><b class=\"fail\">{}</b>failed</div><div><b class=\"fail\">{}</b>errors</div><div><b class=\"skip\">{}</b>skipped</div><div><b>{}/{}</b>assertions</div></div>",
        summary.requests,
        summary.passed,
        summary.failed,
        summary.errors,
        summary.skipped,
        summary.assertions_passed,
        summary.assertions_passed + summary.assertions_failed,
    );
    for step in &report.steps {
        html_step(&mut body, step, iterations);
    }
    if !report.skipped.is_empty() {
        body.push_str("<h2>Skipped</h2>\n<table>\n");
        for skipped in &report.skipped {
            let _ = writeln!(
                body,
                "<tr><td><code>{}</code></td><td class=\"skip\">{}</td></tr>",
                escape(skipped.path.as_str()),
                escape(&skipped.reason)
            );
        }
        body.push_str("</table>\n");
    }
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{} run</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<main>\n{body}</main>\n</body>\n</html>\n",
        escape(&meta.collection)
    )
}

fn html_step(body: &mut String, step: &StepResult, iterations: bool) {
    let (mark, class) = match &step.outcome {
        Err(_) => ("ERROR", "fail"),
        Ok(_) if step.passed() => ("PASS", "pass"),
        Ok(_) => ("FAIL", "fail"),
    };
    let open = if step.passed() { "" } else { " open" };
    let detail = match &step.outcome {
        Err(_) => String::new(),
        Ok(outcome) => format!("{} · {} ms", outcome.head.status, outcome.head.elapsed_ms),
    };
    let _ = write!(
        body,
        "<details{open}>\n<summary><span class=\"{class}\">{mark}</span><span class=\"name\">{}</span><code class=\"muted\">{}</code><span class=\"muted\">{detail}</span></summary>\n<div class=\"body\">\n",
        escape(&step.name),
        escape(&step_name(step, iterations)),
    );
    match &step.outcome {
        Err(error) => {
            let _ = writeln!(body, "<p class=\"fail\">{}</p>", escape(error));
        }
        Ok(outcome) => {
            if outcome.assertions.is_empty() && outcome.captures.is_empty() {
                body.push_str("<p class=\"muted\">No assertions.</p>\n");
            }
            if !outcome.assertions.is_empty() {
                body.push_str("<table>\n<tr><th></th><th>Assertion</th><th>Actual</th></tr>\n");
                for assertion in &outcome.assertions {
                    let (mark, class) = if assertion.verdict.passed {
                        ("✓", "pass")
                    } else {
                        ("✗", "fail")
                    };
                    let message = assertion
                        .verdict
                        .message
                        .as_deref()
                        .map(|m| format!("<br><span class=\"fail\">{}</span>", escape(m)))
                        .unwrap_or_default();
                    let _ = writeln!(
                        body,
                        "<tr><td class=\"{class}\">{mark}</td><td><code>{}</code>{message}</td><td><code>{}</code></td></tr>",
                        escape(&assertion_text(assertion)),
                        escape(&assertion.verdict.actual)
                    );
                }
                body.push_str("</table>\n");
            }
            if !outcome.captures.is_empty() {
                body.push_str("<table>\n<tr><th>Captured</th><th>Value</th></tr>\n");
                for capture in &outcome.captures {
                    let value = match captured(capture) {
                        Ok(value) => format!("<code>{}</code>", escape(&value)),
                        Err(reason) => format!("<span class=\"skip\">{}</span>", escape(&reason)),
                    };
                    let _ = writeln!(
                        body,
                        "<tr><td><code>{}</code></td><td>{value}</td></tr>",
                        escape(&capture.variable)
                    );
                }
                body.push_str("</table>\n");
            }
        }
    }
    body.push_str("</div>\n</details>\n");
}

/// A duration for people: `850 ms`, `12.3 s`.
pub fn duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{}.{} s", ms / 1000, ms % 1000 / 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_covers_markup_and_drops_control_characters() {
        assert_eq!(
            escape("<a href=\"x\">'&'</a>\u{1}\n"),
            "&lt;a href=&quot;x&quot;&gt;&apos;&amp;&apos;&lt;/a&gt;\n"
        );
    }

    #[test]
    fn times_read_well() {
        assert_eq!(seconds(1234), "1.234");
        assert_eq!(seconds(5), "0.005");
        assert_eq!(duration(850), "850 ms");
        assert_eq!(duration(12_345), "12.3 s");
    }

    #[test]
    fn formats_are_named() {
        assert_eq!(ReportFormat::parse("JUnit"), Some(ReportFormat::Junit));
        assert_eq!(ReportFormat::parse("json"), Some(ReportFormat::Json));
        assert_eq!(ReportFormat::parse("pdf"), None);
    }
}
