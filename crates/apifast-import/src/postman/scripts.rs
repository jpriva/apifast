//! Postman scripts as scripts with Bruno's API, as Bruno's importer
//! translates them (`bruno-converters/src/postman/postman-translations.js`):
//! each `pm.*` call it knows becomes its `bru`, `req`, `res`, `test` or
//! `expect` counterpart. A line that still uses `pm` or `postman` is kept
//! commented out, so the script still runs and the line is not lost.

use std::sync::LazyLock;

use regex::Regex;

/// What each Postman call becomes, in order: longer ones before those
/// they start with.
const TRANSLATIONS: &[(&str, &str)] = &[
    (r"pm\.environment\.get\(", "bru.getEnvVar("),
    (r"pm\.environment\.set\(", "bru.setEnvVar("),
    (r"pm\.environment\.unset\(", "bru.deleteEnvVar("),
    (r"pm\.environment\.has\(", "bru.hasEnvVar("),
    (r"pm\.environment\.name", "bru.getEnvName()"),
    (r"pm\.variables\.get\(", "bru.getVar("),
    (r"pm\.variables\.set\(", "bru.setVar("),
    (r"pm\.variables\.has\(", "bru.hasVar("),
    (r"pm\.variables\.unset\(", "bru.deleteVar("),
    (r"pm\.variables\.replaceIn\(", "bru.interpolate("),
    (r"pm\.collectionVariables\.get\(", "bru.getVar("),
    (r"pm\.collectionVariables\.set\(", "bru.setVar("),
    (r"pm\.collectionVariables\.has\(", "bru.hasVar("),
    (r"pm\.collectionVariables\.unset\(", "bru.deleteVar("),
    (r"pm\.globals\.get\(", "bru.getVar("),
    (r"pm\.globals\.set\(", "bru.setVar("),
    (
        r"pm\.execution\.setNextRequest\(",
        "bru.runner.setNextRequest(",
    ),
    (
        r"pm\.execution\.skipRequest\(\)",
        "bru.runner.skipRequest()",
    ),
    (r"pm\.setNextRequest\(", "bru.setNextRequest("),
    (r"postman\.setNextRequest\(", "bru.setNextRequest("),
    (r"postman\.setEnvironmentVariable\(", "bru.setEnvVar("),
    (r"postman\.getEnvironmentVariable\(", "bru.getEnvVar("),
    (r"postman\.clearEnvironmentVariable\(", "bru.deleteEnvVar("),
    (r"postman\.setGlobalVariable\(", "bru.setVar("),
    (r"postman\.getGlobalVariable\(", "bru.getVar("),
    (r"pm\.test\(", "test("),
    (r"pm\.expect\.fail\(", "expect.fail("),
    (r"pm\.expect\(", "expect("),
    (
        r"pm\.response\.to\.have\.status\(",
        "expect(res.getStatus()).to.equal(",
    ),
    (
        r"pm\.response\.to\.have\.header\(",
        "expect(res.getHeaders()).to.have.property(",
    ),
    (
        r"pm\.response\.to\.have\.body\(",
        "expect(res.getBody()).to.equal(",
    ),
    (r"pm\.response\.json\(\)", "res.getBody()"),
    (r"pm\.response\.text\(\)", "JSON.stringify(res.getBody())"),
    (r"pm\.response\.code", "res.getStatus()"),
    (r"pm\.response\.status", "res.getStatusText()"),
    (r"pm\.response\.responseTime", "res.getResponseTime()"),
    (r"pm\.response\.headers\.get\(", "res.getHeader("),
    (r"pm\.response\.headers", "res.getHeaders()"),
    (r"pm\.request\.url\.toString\(\)", "req.getUrl()"),
    (r"pm\.request\.url", "req.getUrl()"),
    (r"pm\.request\.method", "req.getMethod()"),
    (r"pm\.request\.headers\.remove\(", "req.deleteHeader("),
    (r"pm\.request\.headers\.get\(", "req.getHeader("),
    (r"pm\.request\.headers", "req.getHeaders()"),
    (r"pm\.request\.body", "req.getBody()"),
    (r"pm\.info\.requestName", "req.getName()"),
    (r"JSON\.parse\(responseBody\)", "res.getBody()"),
    (r"responseCode\.code", "res.getStatus()"),
];

/// The translations, compiled once.
/// A property (no call) is translated only when nothing more of it
/// follows: `pm.request.headers` alone, not `pm.request.headers.add(…)`,
/// which has no counterpart.
static COMPILED: LazyLock<Vec<(Regex, String)>> = LazyLock::new(|| {
    TRANSLATIONS
        .iter()
        .filter_map(|(pattern, to)| {
            let call = pattern.ends_with(r"\(") || pattern.ends_with(r"\)");
            let (pattern, to) = if call {
                ((*pattern).to_owned(), (*to).to_owned())
            } else {
                (format!(r"{pattern}([^.\w]|$)"), format!("{to}${{1}}"))
            };
            Some((Regex::new(&pattern).ok()?, to))
        })
        .collect()
});

/// The old `tests["name"] = condition;`, as a test.
static OLD_TEST: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"tests\[\s*(['"])(.+?)['"]\s*\]\s*=\s*([^;\n]+);?"#).ok());

/// What is left of Postman after translating.
static POSTMAN: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b(pm|postman)\.").ok());

/// `code` with Bruno's API, and how many lines were left commented out
/// because they use Postman's API apifast does not have.
pub(super) fn translate(code: &str) -> (String, usize) {
    let mut text = code.to_owned();
    for (pattern, to) in COMPILED.iter() {
        text = pattern.replace_all(&text, to.as_str()).into_owned();
    }
    if let Some(old) = OLD_TEST.as_ref() {
        text = old
            .replace_all(
                &text,
                "test($1$2$1, function () { expect(Boolean($3)).to.be.true; });",
            )
            .into_owned();
    }
    let mut commented = 0;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            let left = POSTMAN.as_ref().is_some_and(|p| p.is_match(line));
            if left && !line.trim_start().starts_with("//") {
                commented += 1;
                format!("// {line}")
            } else {
                line.to_owned()
            }
        })
        .collect();
    (lines.join("\n"), commented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postman_calls_become_brunos() {
        let code = r#"pm.test("Status is 200", function () {
    pm.response.to.have.status(200);
});
const data = pm.response.json();
pm.environment.set("token", data.token);
pm.collectionVariables.set("id", data.id);
pm.expect(pm.response.responseTime).to.be.below(500);
const host = pm.environment.get("host");
if (pm.response.code === 404) pm.execution.skipRequest();
pm.request.headers.add({key: "X", value: "1"});
tests["has id"] = data.id !== undefined;"#;
        let (translated, commented) = translate(code);
        assert_eq!(
            translated,
            r#"test("Status is 200", function () {
    expect(res.getStatus()).to.equal(200);
});
const data = res.getBody();
bru.setEnvVar("token", data.token);
bru.setVar("id", data.id);
expect(res.getResponseTime()).to.be.below(500);
const host = bru.getEnvVar("host");
if (res.getStatus() === 404) bru.runner.skipRequest();
// pm.request.headers.add({key: "X", value: "1"});
test("has id", function () { expect(Boolean(data.id !== undefined)).to.be.true; });"#
        );
        // Postman's header objects have no counterpart.
        assert_eq!(commented, 1);
    }

    #[test]
    fn what_has_no_translation_is_commented_out() {
        let (translated, commented) =
            translate("pm.sendRequest('http://x', () => {});\nconsole.log(1);\n// pm.old()");
        assert_eq!(
            translated,
            "// pm.sendRequest('http://x', () => {});\nconsole.log(1);\n// pm.old()"
        );
        assert_eq!(commented, 1);
    }
}
