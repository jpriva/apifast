//! The collection runner against the in-memory ports.

use std::future::Future;
use std::pin::pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use apifast_app::memory::{
    CountingRandom, FixedClock, MemoryCollections, MemoryEnvironments, MemoryResponses,
    MemorySecrets, MemoryTransport, RecordedPauses, ScriptedPrompter,
};
use apifast_app::{
    AppError, CancelToken, CollectionRepository, RequestPath, RunEvent, RunOptions, RunReport,
    SendPorts, TreeItem, run_collection, run_selection,
};
use apifast_domain::{Assertion, Capture, Collection, Folder, HttpRequest, Method, Operator};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("in-memory futures complete on the first poll"),
    }
}

fn request(name: &str, seq: u32, url: &str) -> HttpRequest {
    let mut request = HttpRequest::new(name, Method::Get, url);
    request.seq = Some(seq);
    request
}

fn expecting(mut request: HttpRequest, status: &str) -> HttpRequest {
    request.assertions.push(Assertion::new(
        "res.status",
        Operator::parse("eq"),
        Some(status),
    ));
    request
}

/// `login` captures `ok` from the body; `users/list` uses it; `users/admin/
/// purge` fails its assertion; `health` is tagged `smoke`.
fn collection() -> MemoryCollections {
    let mut login = request("Login", 1, "http://api/login");
    login.captures.push(Capture::new("res.body.ok", "flag"));
    let mut health = expecting(request("Health", 3, "http://api/health"), "200");
    health.tags = vec!["smoke".into()];
    MemoryCollections::new(Collection::new("Shop"))
        .with_request("login.yml", login)
        .with_folder("users", Folder::new("Users"))
        .with_request(
            "users/list.yml",
            request("List", 1, "http://api/users?flag={{flag}}"),
        )
        .with_folder("users/admin", Folder::new("Admin"))
        .with_request(
            "users/admin/purge.yml",
            expecting(request("Purge", 1, "http://api/purge"), "204"),
        )
        .with_request("health.yml", health)
}

struct World {
    collections: MemoryCollections,
    environments: MemoryEnvironments,
    transport: MemoryTransport,
    responses: MemoryResponses,
    clock: FixedClock,
    random: CountingRandom,
    prompter: ScriptedPrompter,
    secrets: MemorySecrets,
    pauses: RecordedPauses,
}

impl World {
    fn new() -> Self {
        Self {
            collections: collection(),
            environments: MemoryEnvironments::default(),
            transport: MemoryTransport::new(200, r#"{"ok":true}"#),
            responses: MemoryResponses::default(),
            clock: FixedClock(1_700_000_000_000),
            random: CountingRandom::new(7),
            prompter: ScriptedPrompter::default(),
            secrets: MemorySecrets::default(),
            pauses: RecordedPauses::default(),
        }
    }

    fn run_with(
        &self,
        options: &RunOptions,
        cancel: &CancelToken,
        events: &Mutex<Vec<String>>,
    ) -> Result<RunReport, AppError> {
        let ports = SendPorts {
            collections: &self.collections,
            environments: &self.environments,
            transport: &self.transport,
            responses: &self.responses,
            clock: &self.clock,
            random: &self.random,
            prompter: &self.prompter,
            secrets: &self.secrets,
            report: &|_| {},
            history: None,
        };
        let on_event = |event: RunEvent<'_>| {
            let line = match event {
                RunEvent::Started {
                    iteration,
                    index,
                    total,
                    path,
                } => format!("start {iteration}:{index}/{total} {path}"),
                RunEvent::Finished(step) => format!("done {} {}", step.path, step.passed()),
            };
            events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(line);
        };
        block_on(run_collection(
            ports,
            &self.pauses,
            options,
            cancel,
            &on_event,
        ))
    }

    fn run(&self, options: &RunOptions) -> RunReport {
        self.run_with(options, &CancelToken::new(), &Mutex::default())
            .unwrap_or_else(|e| panic!("{e}"))
    }

    fn urls(&self) -> Vec<String> {
        self.transport
            .sent()
            .iter()
            .map(|r| r.url().to_owned())
            .collect()
    }
}

fn paths(report: &RunReport) -> Vec<&str> {
    report.steps.iter().map(|s| s.path.as_str()).collect()
}

#[test]
fn a_collection_runs_in_tree_order_carrying_captures() {
    let world = World::new();
    let report = world.run(&RunOptions::default());
    assert_eq!(
        paths(&report),
        [
            "login.yml",
            "health.yml",
            "users/list.yml",
            "users/admin/purge.yml"
        ]
    );
    // What login captured reaches the next requests, and the report.
    assert_eq!(world.urls()[2], "http://api/users?flag=true");
    assert!(report.runtime.iter().any(|v| v.name == "flag"));
    let summary = report.summary();
    assert_eq!(
        (
            summary.requests,
            summary.passed,
            summary.failed,
            summary.errors
        ),
        (4, 3, 1, 0)
    );
    assert_eq!(
        (summary.assertions_passed, summary.assertions_failed),
        (1, 1)
    );
    assert!(!report.succeeded());
    assert!(!report.bailed);
}

#[test]
fn a_folder_runs_alone_and_recursion_is_optional() {
    let world = World::new();
    let folder = |recursive| RunOptions {
        folder: Some("users".into()),
        recursive,
        ..RunOptions::default()
    };
    assert_eq!(
        paths(&world.run(&folder(true))),
        ["users/list.yml", "users/admin/purge.yml"]
    );
    assert_eq!(paths(&world.run(&folder(false))), ["users/list.yml"]);
    let nested = RunOptions {
        folder: Some("/users/admin/".into()),
        ..RunOptions::default()
    };
    assert_eq!(paths(&world.run(&nested)), ["users/admin/purge.yml"]);
    let missing = RunOptions {
        folder: Some("nope".into()),
        ..RunOptions::default()
    };
    assert_eq!(
        world
            .run_with(&missing, &CancelToken::new(), &Mutex::default())
            .unwrap_err()
            .to_string(),
        "`nope` is not a folder of the collection"
    );
}

#[test]
fn bail_stops_at_the_first_failure() {
    let world = World::new();
    let report = world.run(&RunOptions {
        bail: true,
        iterations: 3,
        ..RunOptions::default()
    });
    assert!(report.bailed);
    assert_eq!(paths(&report).last(), Some(&"users/admin/purge.yml"));
    assert_eq!(report.steps.len(), 4);
}

#[test]
fn iterations_repeat_with_a_pause_between_requests() {
    let world = World::new();
    let report = world.run(&RunOptions {
        folder: Some("users".into()),
        iterations: 2,
        delay_ms: 250,
        ..RunOptions::default()
    });
    let iterations: Vec<u32> = report.steps.iter().map(|s| s.iteration).collect();
    assert_eq!(iterations, [1, 1, 2, 2]);
    // Between requests, not before the first.
    assert_eq!(world.pauses.asked(), [250, 250, 250]);
}

#[test]
fn tags_select_and_the_rest_is_skipped_with_a_reason() {
    let world = World::new();
    let report = world.run(&RunOptions {
        tags: vec!["smoke".into()],
        ..RunOptions::default()
    });
    assert_eq!(paths(&report), ["health.yml"]);
    assert!(report.succeeded());
    assert_eq!(report.skipped.len(), 3);
    assert_eq!(report.skipped[0].reason, "has none of the tags smoke");

    let report = world.run(&RunOptions {
        exclude_tags: vec!["SMOKE".into()],
        ..RunOptions::default()
    });
    assert_eq!(report.steps.len(), 3);
    assert_eq!(report.skipped[0].reason, "excluded by the tag `SMOKE`");
}

#[test]
fn events_follow_each_request_and_cancel_stops_the_run() {
    let world = World::new();
    let events = Mutex::default();
    let options = RunOptions {
        folder: Some("users".into()),
        ..RunOptions::default()
    };
    let report = world
        .run_with(&options, &CancelToken::new(), &events)
        .unwrap();
    // Without login first, `flag` is missing: an error, and the run goes on.
    let error = report.steps[0].outcome.as_ref().unwrap_err();
    assert!(error.contains("flag"), "{error}");
    assert_eq!(report.summary().errors, 1);
    assert_eq!(
        *events.lock().unwrap(),
        [
            "start 1:0/2 users/list.yml",
            "done users/list.yml false",
            "start 1:1/2 users/admin/purge.yml",
            "done users/admin/purge.yml false",
        ]
    );

    let cancel = CancelToken::new();
    cancel.cancel();
    let report = world
        .run_with(&options, &cancel, &Mutex::default())
        .unwrap();
    assert!(report.cancelled);
    assert_eq!(report.steps, []);
    assert!(!report.succeeded());
}

#[test]
fn requests_that_cannot_run_are_skipped() {
    let items = vec![
        TreeItem::Other {
            path: RequestPath::new("gql.yml"),
            name: "Query".into(),
            kind: "graphql".into(),
            seq: None,
        },
        TreeItem::Invalid {
            path: RequestPath::new("dir/broken.yml"),
            message: "bad YAML".into(),
        },
    ];
    let selection = run_selection(&items, None, true).unwrap();
    assert_eq!(selection.runnable, []);
    let reasons: Vec<(&str, &str)> = selection
        .skipped
        .iter()
        .map(|s| (s.name.as_str(), s.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        [
            ("Query", "graphql requests cannot run yet"),
            ("broken.yml", "bad YAML")
        ]
    );
}

#[test]
fn data_rows_drive_the_iterations() {
    let world = World::new();
    let rows = apifast_app::parse_data("id\n1\n2\n3\n", apifast_app::DataFormat::Csv)
        .unwrap_or_else(|e| panic!("{e}"));
    world
        .collections
        .save_request(
            &RequestPath::new("users/list.yml"),
            &request("List", 1, "http://api/users/{{id}}"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let report = world.run(&RunOptions {
        folder: Some("users".into()),
        recursive: false,
        iterations: 10,
        data: rows,
        ..RunOptions::default()
    });
    assert_eq!(
        world.urls(),
        [
            "http://api/users/1",
            "http://api/users/2",
            "http://api/users/3"
        ]
    );
    let iterations: Vec<u32> = report.steps.iter().map(|s| s.iteration).collect();
    assert_eq!(iterations, [1, 2, 3]);
    assert!(report.runtime.iter().any(|v| v.name == "id"));
}
