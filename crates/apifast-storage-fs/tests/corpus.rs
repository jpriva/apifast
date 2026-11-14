//! Round trip of every OpenCollection file from Bruno's test fixtures
//! (`tests/corpus/bruno`, see `tests/corpus/README.md`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers: a failure fails the test"
)]

use std::fs;
use std::path::{Path, PathBuf};

use apifast_app::{CollectionRepository, EnvironmentRepository, RequestPath};
use apifast_domain::{AuthScheme, BearerAuth, Body, Header, HttpRequest, Setting};
use apifast_storage_fs::FsCollection;

const MINIMAL_COLLECTION: &str = "opencollection: 1.0.0\ninfo:\n  name: Corpus\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Collection,
    Folder,
    Environment,
    Request,
}

struct Sample {
    source: PathBuf,
    text: String,
    kind: Kind,
}

fn corpus() -> Vec<Sample> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/bruno");
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|source| {
            let text = fs::read_to_string(&source).unwrap();
            let kind = classify(&source, &text);
            Sample { source, text, kind }
        })
        .collect()
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "yml" || e == "yaml") {
            out.push(path);
        }
    }
}

fn classify(path: &Path, text: &str) -> Kind {
    let has_line = |prefix: &str| text.lines().any(|l| l.starts_with(prefix));
    if has_line("opencollection:") || path.file_name().is_some_and(|n| n == "opencollection.yml") {
        Kind::Collection
    } else if path.file_name().is_some_and(|n| n == "folder.yml") || text.contains("type: folder") {
        Kind::Folder
    } else if !has_line("info:") && (has_line("variables:") || has_line("name:")) {
        Kind::Environment
    } else {
        Kind::Request
    }
}

/// Puts `sample` in a fresh collection where the repository will find it,
/// and returns the collection and the file's path inside it.
fn place(sample: &Sample) -> (tempfile::TempDir, FsCollection, String) {
    let dir = tempfile::tempdir().unwrap();
    let ext = sample
        .source
        .extension()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let relative = match sample.kind {
        Kind::Collection => "opencollection.yml".to_owned(),
        Kind::Folder => "folder/folder.yml".to_owned(),
        Kind::Environment => format!("environments/env.{ext}"),
        Kind::Request => format!("request.{ext}"),
    };
    if sample.kind != Kind::Collection {
        fs::write(dir.path().join("opencollection.yml"), MINIMAL_COLLECTION).unwrap();
    }
    let full = dir.path().join(&relative);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(&full, &sample.text).unwrap();
    let repo = FsCollection::open(dir.path()).unwrap();
    (dir, repo, relative)
}

/// Whether the file is valid YAML at all. A few fixtures are broken on purpose.
fn parses(text: &str) -> bool {
    oneyaml::from_str::<oneyaml::Value>(text).is_ok()
}

fn name(sample: &Sample) -> String {
    sample
        .source
        .to_string_lossy()
        .rsplit("corpus/")
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_corpus_is_there() {
    let samples = corpus();
    assert!(
        samples.len() >= 400,
        "only {} files in the corpus",
        samples.len()
    );
    let count = |k| samples.iter().filter(|s| s.kind == k).count();
    eprintln!(
        "corpus: {} files ({} collections, {} folders, {} environments, {} requests)",
        samples.len(),
        count(Kind::Collection),
        count(Kind::Folder),
        count(Kind::Environment),
        count(Kind::Request)
    );
    // Every kind is represented, so each code path is exercised.
    for kind in [
        Kind::Collection,
        Kind::Folder,
        Kind::Environment,
        Kind::Request,
    ] {
        assert!(count(kind) > 0, "no {kind:?} in the corpus");
    }
}

#[test]
fn every_file_saves_back_byte_for_byte() {
    let mut failures = Vec::new();
    let mut broken = Vec::new();
    for sample in corpus() {
        if !parses(&sample.text) {
            broken.push(name(&sample));
            continue;
        }
        let (dir, repo, relative) = place(&sample);
        let result = match sample.kind {
            Kind::Collection => repo.collection().and_then(|c| repo.save_collection(&c)),
            Kind::Folder => {
                // A folder is loaded through the chain of a request inside it.
                let probe = dir.path().join("folder/probe.yml");
                fs::write(&probe, "info:\n  name: P\n  type: http\n").unwrap();
                let loaded = repo.request(&RequestPath::new("folder/probe.yml"));
                fs::remove_file(&probe).unwrap();
                loaded.and_then(|l| repo.save_folder("folder", &l.folders[0]))
            }
            Kind::Environment => repo
                .environments()
                .and_then(|envs| envs.iter().try_for_each(|e| repo.save_environment(e))),
            Kind::Request => match repo.request(&RequestPath::new(relative.as_str())) {
                Ok(loaded) => {
                    repo.save_request(&RequestPath::new(relative.as_str()), &loaded.request)
                }
                // Non-HTTP requests cannot be loaded yet; they are only listed.
                Err(apifast_app::RepositoryError::Unsupported { .. }) => Ok(()),
                Err(e) => Err(e),
            },
        };
        let after = fs::read_to_string(dir.path().join(&relative)).unwrap();
        match result {
            Ok(()) if after == sample.text => {}
            Ok(()) => failures.push(format!("{}: changed on save", name(&sample))),
            Err(e) => failures.push(format!("{}: {e}", name(&sample))),
        }
    }
    assert!(
        failures.is_empty(),
        "{} files failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    eprintln!("corpus: broken on purpose: {broken:?}");
    // Bruno's error fixtures; they must fail to parse, not panic.
    assert!(broken.len() <= 3, "unexpected broken files: {broken:?}");
}

/// A labelled edit of a request.
type Edit = (&'static str, fn(&mut HttpRequest));

fn edits() -> Vec<Edit> {
    vec![
        ("url", |r| r.url.push_str("/edited")),
        ("name", |r| r.name = "Edited name".into()),
        ("add header", |r| {
            r.headers.push(Header::new("x-edited", "1"));
        }),
        ("toggle first header", |r| {
            if let Some(h) = r.headers.first_mut() {
                h.enabled = !h.enabled;
            }
        }),
        ("bearer auth", |r| {
            r.auth = Setting::Set(AuthScheme::Bearer(BearerAuth {
                token: "{{edited}}".into(),
            }));
        }),
        ("json body", |r| {
            r.body = Body::Json("{\"edited\": true}".into());
        }),
    ]
}

#[test]
fn every_http_request_survives_edits() {
    let mut failures = Vec::new();
    let mut edited = 0;
    for sample in corpus()
        .into_iter()
        .filter(|s| s.kind == Kind::Request && parses(&s.text))
    {
        for (label, edit) in edits() {
            let (dir, repo, relative) = place(&sample);
            let path = RequestPath::new(relative.as_str());
            let Ok(loaded) = repo.request(&path) else {
                continue;
            };
            let mut request = loaded.request;
            let before = request.clone();
            edit(&mut request);
            if request == before {
                continue;
            }
            edited += 1;
            if let Err(e) = repo.save_request(&path, &request) {
                failures.push(format!("{} [{label}]: {e}", name(&sample)));
                continue;
            }
            let reloaded = repo.request(&path).map(|l| l.request);
            if reloaded.as_ref() != Ok(&request) {
                failures.push(format!(
                    "{} [{label}]: reads back differently",
                    name(&sample)
                ));
                continue;
            }
            if label == "url" {
                let after = fs::read_to_string(dir.path().join(&relative)).unwrap();
                let changed = sample
                    .text
                    .lines()
                    .zip(after.lines())
                    .filter(|(a, b)| a != b)
                    .count()
                    + sample.text.lines().count().abs_diff(after.lines().count());
                if changed > 2 {
                    failures.push(format!("{} [url]: {changed} lines changed", name(&sample)));
                }
            }
        }
    }
    eprintln!("corpus: {edited} edits checked");
    assert!(edited > 500, "only {edited} edits ran");
    assert!(
        failures.is_empty(),
        "{} edits failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_collection_lists_without_errors() {
    let roots = corpus().into_iter().filter(|s| {
        s.kind == Kind::Collection
            && parses(&s.text)
            // Other collection fixtures are loose files, not collection folders.
            && s.source.file_name().is_some_and(|n| n == "opencollection.yml")
    });
    let mut listed = 0;
    for sample in roots {
        listed += 1;
        // Open the real fixture folder read-only: listing must not fail.
        let repo = FsCollection::open(sample.source.parent().unwrap()).unwrap();
        let tree = repo.tree();
        assert!(tree.is_ok(), "{}: {:?}", name(&sample), tree.err());
    }
    assert!(listed > 0, "no collection folders in the corpus");
}
