//! Throwaway spike (phase F0): compare YAML libraries for apifast's file format.
//!
//! Usage: yaml-spike <dir-with-opencollection-yaml-files>
//!
//! Measures, per library, on real Bruno OpenCollection files:
//! 1. parse success
//! 2. generic round-trip: semantic equality and byte identity
//! 3. typed round-trip (DTO with `#[serde(flatten)]` extras): unknown keys kept, byte identity
//! 4. "edit one field and save": lines changed in the diff (git noise)
//! 5. interpretation agreement between libraries
//! 6. tricky scalars, comments, block scalars
//! 7. speed

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use similar::{ChangeTag, TextDiff};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Instant;

type R<T> = Result<T, Box<dyn Error>>;

// ---------------------------------------------------------------------------
// DTO: minimal typed view of an OpenCollection HTTP request, keeping unknown keys.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RequestFile {
    info: Info,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    http: Option<Http>,
    #[serde(flatten)]
    extra: IndexMap<String, Json>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Info {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seq: Option<i64>,
    #[serde(flatten)]
    extra: IndexMap<String, Json>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Http {
    method: String,
    url: String,
    #[serde(flatten)]
    extra: IndexMap<String, Json>,
}

// ---------------------------------------------------------------------------
// Library adapters
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Lib {
    name: &'static str,
    to_json: fn(&str) -> R<Json>,
    from_json: fn(&Json) -> R<String>,
    de_req: fn(&str) -> R<RequestFile>,
    ser_req: fn(&RequestFile) -> R<String>,
}

fn saphyr_opts() -> serde_saphyr::SerializerOptions {
    // Tuned to look like Bruno's output: indented lists, no folding of long strings.
    serde_saphyr::ser_options! {
        compact_list_indent: false,
        folded_wrap_chars: usize::MAX,
    }
}

fn libs() -> Vec<Lib> {
    vec![
        Lib {
            name: "yaml_serde",
            to_json: |s| Ok(yaml_serde::from_str(s)?),
            from_json: |v| Ok(yaml_serde::to_string(v)?),
            de_req: |s| Ok(yaml_serde::from_str(s)?),
            ser_req: |r| Ok(yaml_serde::to_string(r)?),
        },
        Lib {
            name: "serde-saphyr (default)",
            to_json: |s| Ok(serde_saphyr::from_str(s)?),
            from_json: |v| Ok(serde_saphyr::to_string(v)?),
            de_req: |s| Ok(serde_saphyr::from_str(s)?),
            ser_req: |r| Ok(serde_saphyr::to_string(r)?),
        },
        Lib {
            name: "serde-saphyr (tuned)",
            to_json: |s| Ok(serde_saphyr::from_str(s)?),
            from_json: |v| Ok(serde_saphyr::to_string_with_options(v, saphyr_opts())?),
            de_req: |s| Ok(serde_saphyr::from_str(s)?),
            ser_req: |r| Ok(serde_saphyr::to_string_with_options(r, saphyr_opts())?),
        },
        Lib {
            name: "oneyaml (serde)",
            to_json: |s| Ok(oneyaml::from_str(s)?),
            from_json: |v| Ok(oneyaml::to_string(v)?),
            de_req: |s| Ok(oneyaml::from_str(s)?),
            ser_req: |r| Ok(oneyaml::to_string(r)?),
        },
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn changed_lines(a: &str, b: &str) -> usize {
    TextDiff::from_lines(a, b)
        .iter_all_changes()
        .filter(|c| c.tag() != ChangeTag::Equal)
        .count()
}

fn collect_yaml(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "node_modules" || n == ".git") {
                continue;
            }
            collect_yaml(&p, out);
        } else if p.extension().is_some_and(|x| x == "yml" || x == "yaml") {
            out.push(p);
        }
    }
}

fn is_opencollection(src: &str) -> bool {
    src.lines().any(|l| {
        l.starts_with("info:") || l.starts_with("opencollection:") || l.starts_with("variables:")
    })
}

/// RFC 7396 merge patch that turns `old` into `new`.
fn merge_patch(old: &Json, new: &Json) -> Option<Json> {
    match (old, new) {
        (Json::Object(o), Json::Object(n)) => {
            let mut p = serde_json::Map::new();
            for (k, nv) in n {
                match o.get(k) {
                    Some(ov) => {
                        if let Some(d) = merge_patch(ov, nv) {
                            p.insert(k.clone(), d);
                        }
                    }
                    None => {
                        p.insert(k.clone(), nv.clone());
                    }
                }
            }
            for k in o.keys() {
                if !n.contains_key(k) {
                    p.insert(k.clone(), Json::Null);
                }
            }
            (!p.is_empty()).then_some(Json::Object(p))
        }
        _ if old == new => None,
        _ => Some(new.clone()),
    }
}

fn pct(a: usize, b: usize) -> String {
    if b == 0 {
        "-".into()
    } else {
        format!("{a}/{b} ({:.0}%)", 100.0 * a as f64 / b as f64)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

fn corpus_tests(files: &[(PathBuf, String)]) {
    println!("\n## 1-4. Corpus: {} OpenCollection files from Bruno\n", files.len());
    println!(
        "| Library | Parse OK | Generic round-trip: same data | Generic: byte-identical | Typed (http) round-trip: same data | Typed: byte-identical | Edit url: avg lines changed | Edit url: max |"
    );
    println!("|---|---|---|---|---|---|---|---|");

    let http_files: Vec<&(PathBuf, String)> = files
        .iter()
        .filter(|(_, s)| s.contains("\n  type: http") || s.starts_with("info:\n") && s.contains("type: http"))
        .collect();

    for lib in libs() {
        let (mut ok, mut same, mut ident) = (0, 0, 0);
        let mut first_err: Option<String> = None;
        for (p, src) in files {
            match (lib.to_json)(src) {
                Ok(v) => {
                    ok += 1;
                    if let Ok(out) = (lib.from_json)(&v) {
                        if out == *src {
                            ident += 1;
                        }
                        if (lib.to_json)(&out).ok().as_ref() == Some(&v) {
                            same += 1;
                        }
                    }
                }
                Err(e) => {
                    first_err.get_or_insert_with(|| format!("{}: {e}", p.display()));
                }
            }
        }

        let (mut t_total, mut t_same, mut t_ident, mut edit_sum, mut edit_max) = (0, 0, 0, 0, 0);
        for (_, src) in &http_files {
            let Ok(req) = (lib.de_req)(src) else { continue };
            if req.http.is_none() {
                continue;
            }
            t_total += 1;
            let Ok(out) = (lib.ser_req)(&req) else { continue };
            if out == **src {
                t_ident += 1;
            }
            if (lib.de_req)(&out).ok().as_ref() == Some(&req) {
                t_same += 1;
            }
            let mut edited = req.clone();
            if let Some(h) = edited.http.as_mut() {
                h.url.push_str("/edited");
            }
            if let Ok(out2) = (lib.ser_req)(&edited) {
                let n = changed_lines(src, &out2);
                edit_sum += n;
                edit_max = edit_max.max(n);
            }
        }
        println!(
            "| {} | {} | {} | {} | {} | {} | {:.1} | {} |",
            lib.name,
            pct(ok, files.len()),
            pct(same, ok),
            pct(ident, ok),
            pct(t_same, t_total),
            pct(t_ident, t_total),
            edit_sum as f64 / t_total.max(1) as f64,
            edit_max
        );
        if let Some(e) = first_err {
            println!("|  ↳ first parse error | {} |||||||", e.replace('\n', " ").replace('|', "/"));
        }
    }

    // oneyaml lossless Document
    let (mut ok, mut ident) = (0, 0);
    let (mut set_total, mut set_sum, mut set_max) = (0, 0, 0);
    let (mut mp_total, mut mp_sum, mut mp_max, mut mp_fail) = (0, 0, 0, 0);
    for (_, src) in files {
        if let Ok(doc) = oneyaml::Document::parse(src.as_str()) {
            ok += 1;
            if doc.to_string() == *src {
                ident += 1;
            }
        }
    }
    for (_, src) in &http_files {
        let Ok(req) = oneyaml::from_str::<RequestFile>(src) else { continue };
        let Some(http) = &req.http else { continue };
        let new_url = format!("{}/edited", http.url);

        // a) direct path edit
        if let Ok(mut doc) = oneyaml::Document::parse(src.as_str()) {
            if doc.set_path("http.url", new_url.as_str()).is_ok() {
                set_total += 1;
                let n = changed_lines(src, &doc.to_string());
                set_sum += n;
                set_max = set_max.max(n);
            }
        }
        // b) model-driven save: diff old/new model as JSON, apply as merge patch
        let mut edited = req.clone();
        edited.http.as_mut().map(|h| h.url = new_url.clone());
        let (Ok(old), Ok(new)) = (serde_json::to_value(&req), serde_json::to_value(&edited)) else { continue };
        let Some(patch) = merge_patch(&old, &new) else { continue };
        mp_total += 1;
        let res = oneyaml::Document::parse(src.as_str()).ok().and_then(|mut doc| {
            let mp = oneyaml::MergePatch::from_json(&patch.to_string()).ok()?;
            doc.apply_merge_patch(&mp).ok()?;
            Some(doc.to_string())
        });
        match res {
            Some(out) => {
                let n = changed_lines(src, &out);
                mp_sum += n;
                mp_max = mp_max.max(n);
            }
            None => mp_fail += 1,
        }
    }
    println!(
        "| oneyaml (Document, lossless) | {} | n/a | {} | n/a | n/a | set_path: {:.1} / merge-patch: {:.1} | {} / {} |",
        pct(ok, files.len()),
        pct(ident, ok),
        set_sum as f64 / set_total.max(1) as f64,
        mp_sum as f64 / (mp_total - mp_fail).max(1) as f64,
        set_max,
        mp_max
    );
    println!(
        "\nHTTP request files used for typed/edit tests: {}. Merge-patch failures: {mp_fail}. \
         An ideal edit of one field changes 2 lines (1 removed + 1 added).",
        http_files.len()
    );
}

fn agreement(files: &[(PathBuf, String)]) {
    println!("\n## 5. Do the libraries read the same data from each file?\n");
    let ls = libs();
    let reference = ls[3]; // oneyaml (YAML 1.2 core, same as Bruno's JS `yaml` default)
    for lib in [ls[0], ls[1]] {
        let mut diff = 0;
        let mut example = None;
        for (p, src) in files {
            let (a, b) = ((lib.to_json)(src), (reference.to_json)(src));
            if let (Ok(a), Ok(b)) = (a, b) {
                if a != b {
                    diff += 1;
                    example.get_or_insert_with(|| p.display().to_string());
                }
            }
        }
        println!(
            "- {} vs oneyaml: {diff} files read differently{}",
            lib.name,
            example.map(|e| format!(" (e.g. {e})")).unwrap_or_default()
        );
    }
}

fn tricky_scalars() {
    println!("\n## 6a. Tricky strings: written by each library, read back by every library\n");
    let values = [
        "200", "true", "yes", "on", "no", "null", "~", "", "0777", "1e3", "0x1F", ".nan", "2026-11-14",
        "a: b", "#hash", "- dash", "{{host}}/v1", "  leading space", "trailing ", "multi\nline\n",
        "tab\there", "quote's", "\"dq\"", "ñandú €",
    ];
    let mut map = IndexMap::new();
    for (i, v) in values.iter().enumerate() {
        map.insert(format!("k{i}"), Json::String((*v).to_string()));
    }
    let original = Json::Object(map.into_iter().collect());
    let ls = libs();
    println!("| Written by \\ read by | {} |", ls.iter().map(|l| l.name).collect::<Vec<_>>().join(" | "));
    println!("|---|{}", "---|".repeat(ls.len()));
    for w in &ls {
        let out = (w.from_json)(&original).unwrap_or_default();
        let cells: Vec<String> = ls
            .iter()
            .map(|r| match (r.to_json)(&out) {
                Ok(v) if v == original => "ok".into(),
                Ok(v) => {
                    let bad: Vec<String> = original
                        .as_object()
                        .into_iter()
                        .flatten()
                        .filter(|(k, ov)| v.get(k.as_str()) != Some(ov))
                        .map(|(k, ov)| format!("{ov} → {}", v.get(k.as_str()).map(|x| x.to_string()).unwrap_or("∅".into())))
                        .collect();
                    format!("FAIL {}", bad.join(", "))
                }
                Err(e) => format!("ERR {}", e.to_string().lines().next().unwrap_or("")),
            })
            .collect();
        println!("| {} | {} |", w.name, cells.join(" | "));
    }

    println!("\n## 6b. How each library reads unquoted `yes`, `on`, `0777` (YAML 1.1 vs 1.2)\n");
    let src = "a: yes\nb: on\nc: 0777\nd: 2026-11-14\n";
    for l in &ls {
        println!("- {}: {}", l.name, (l.to_json)(src).map(|v| v.to_string()).unwrap_or_else(|e| e.to_string()));
    }
}

const COMMENTED: &str = r#"# Login request, used by every other request in the collection
info:
  name: Login
  type: http
  seq: 1

http:
  method: POST
  url: "{{host}}/auth/login" # base url comes from the environment
  headers:
    - name: content-type
      value: application/json
  body:
    type: json
    data: |-
      {
        "user": "{{user}}",
        "password": "{{password}}"
      }

runtime:
  assertions:
    - expression: res.status
      operator: eq
      value: "200"

docs: |-
  Long single-line documentation text that is longer than eighty characters so we can see whether it gets folded.
"#;

fn comments_and_blocks() {
    println!("\n## 6c. Hand-written file with comments, blank lines and block scalars: edit url, save\n");
    println!("| Library | Lines changed | Comments kept | Blank lines kept | `data: |-` kept | Long docs line kept |");
    println!("|---|---|---|---|---|---|");
    let check = |name: &str, out: &str| {
        println!(
            "| {name} | {} | {} | {} | {} | {} |",
            changed_lines(COMMENTED, out),
            out.contains("# Login request") && out.contains("# base url"),
            out.contains("\n\nhttp:"),
            out.contains("data: |-"),
            out.contains("  Long single-line documentation text that is longer than eighty characters so we can see whether it gets folded.")
        );
    };
    for lib in libs() {
        match (lib.de_req)(COMMENTED) {
            Ok(mut r) => {
                r.http.as_mut().map(|h| h.url = "{{host}}/auth/v2/login".into());
                check(lib.name, &(lib.ser_req)(&r).unwrap_or_default());
            }
            Err(e) => println!("| {} | ERR {e} |||||", lib.name),
        }
    }
    let mut doc = oneyaml::Document::parse(COMMENTED).expect("parse");
    doc.set_path("http.url", "{{host}}/auth/v2/login").expect("set");
    let out = doc.to_string();
    check("oneyaml (Document)", &out);
    println!("\noneyaml Document output after the edit:\n\n```yaml\n{out}```");
    let saphyr = libs()[1];
    let r = (saphyr.de_req)(COMMENTED).expect("de");
    println!("\nserde-saphyr (default) output of the same file, no edit:\n\n```yaml\n{}```", (saphyr.ser_req)(&r).unwrap_or_default());
}

fn speed(files: &[(PathBuf, String)]) {
    println!("\n## 7. Speed (release build, whole corpus × 20, parse + serialize)\n");
    let rounds = 20;
    let bytes: usize = files.iter().map(|(_, s)| s.len()).sum::<usize>() * rounds;
    println!("| Library | Parse into Json | Serialize Json | Typed parse+serialize (http) | 1 MB JSON body: parse + write |");
    println!("|---|---|---|---|---|");
    let big_body = {
        let mut s = String::from("info:\n  name: Big\n  type: http\nhttp:\n  method: POST\n  url: x\n  body:\n    type: json\n    data: |-\n");
        while s.len() < 1_000_000 {
            s.push_str("      {\"id\": 12345, \"name\": \"some value here\", \"tags\": [\"a\", \"b\"]},\n");
        }
        s
    };
    for lib in libs() {
        let parsed: Vec<Json> = files.iter().filter_map(|(_, s)| (lib.to_json)(s).ok()).collect();
        let t = Instant::now();
        for _ in 0..rounds {
            for (_, s) in files {
                let _ = std::hint::black_box((lib.to_json)(s));
            }
        }
        let parse = t.elapsed();
        let t = Instant::now();
        for _ in 0..rounds {
            for v in &parsed {
                let _ = std::hint::black_box((lib.from_json)(v));
            }
        }
        let ser = t.elapsed();
        let t = Instant::now();
        for _ in 0..rounds {
            for (_, s) in files {
                if let Ok(r) = (lib.de_req)(s) {
                    let _ = std::hint::black_box((lib.ser_req)(&r));
                }
            }
        }
        let typed = t.elapsed();
        let t = Instant::now();
        let ok = (lib.de_req)(&big_body).and_then(|r| (lib.ser_req)(&r)).is_ok();
        let big = t.elapsed();
        println!(
            "| {} | {:.1} ms ({:.0} MB/s) | {:.1} ms | {:.1} ms | {:.1} ms{} |",
            lib.name,
            parse.as_secs_f64() * 1e3,
            bytes as f64 / parse.as_secs_f64() / 1e6,
            ser.as_secs_f64() * 1e3,
            typed.as_secs_f64() * 1e3,
            big.as_secs_f64() * 1e3,
            if ok { "" } else { " (failed)" }
        );
    }
    let t = Instant::now();
    for _ in 0..rounds {
        for (_, s) in files {
            let _ = std::hint::black_box(oneyaml::Document::parse(s.as_str()).map(|d| d.to_string()));
        }
    }
    let doc = t.elapsed();
    let t = Instant::now();
    let _ = oneyaml::Document::parse(big_body.as_str()).map(|d| d.to_string());
    println!(
        "| oneyaml (Document parse + write) | {:.1} ms (parse+write) | - | - | {:.1} ms |",
        doc.as_secs_f64() * 1e3,
        t.elapsed().as_secs_f64() * 1e3
    );
    println!("\nCorpus size: {:.1} KB per round.", bytes as f64 / rounds as f64 / 1e3);
}

fn main() -> R<()> {
    let dir = std::env::args().nth(1).ok_or("usage: yaml-spike <dir>")?;
    let mut paths = Vec::new();
    collect_yaml(Path::new(&dir), &mut paths);
    paths.sort();
    let files: Vec<(PathBuf, String)> = paths
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|s| (p, s)))
        .filter(|(_, s)| is_opencollection(s))
        .collect();

    println!("# YAML library spike results");
    corpus_tests(&files);
    agreement(&files);
    tricky_scalars();
    comments_and_blocks();
    speed(&files);
    Ok(())
}
