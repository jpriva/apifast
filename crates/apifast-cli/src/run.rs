//! `apifast run`: runs a collection or a folder and reports the results
//! (docs/08).

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use apifast_app::{
    DataFormat, ReportFormat, ReportMeta, RunEvent, RunOptions, RunReport, SendOptions, SendPorts,
    StepResult, assertion_text, duration, parse_data, run_collection,
};
use apifast_domain::Variable;

use crate::{Wired, locate, parse_var, runtime, system, wire};

/// Some request could not be sent, or the run could not start.
pub const EXIT_RUN_ERROR: u8 = 2;

#[derive(clap::Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag of the command line is a bool"
)]
pub struct RunArgs {
    /// The collection or folder to run; the current folder by default.
    path: Option<PathBuf>,
    /// Active environment, by name.
    #[arg(short, long)]
    env: Option<String>,
    /// A value that overrides every scope, as `NAME=VALUE`. Repeatable.
    #[arg(long = "var", visible_alias = "env-var", value_name = "NAME=VALUE", value_parser = parse_var)]
    vars: Vec<(String, String)>,
    /// Also run the requests in subfolders.
    #[arg(short, long)]
    recursive: bool,
    /// Run only requests with one of these tags.
    #[arg(long, value_delimiter = ',', value_name = "TAG,…")]
    tags: Vec<String>,
    /// Do not run requests with any of these tags.
    #[arg(long, value_delimiter = ',', value_name = "TAG,…")]
    exclude_tags: Vec<String>,
    /// Stop at the first request that fails.
    #[arg(long)]
    bail: bool,
    /// Pause between requests, in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = 0)]
    delay: u64,
    /// Run the selection this many times.
    #[arg(long, value_name = "N", default_value_t = 1,
          value_parser = clap::value_parser!(u32).range(1..), conflicts_with = "data")]
    iterations: u32,
    /// A CSV or JSON file: one iteration per row, its columns as variables.
    #[arg(long, value_name = "FILE")]
    data: Option<PathBuf>,
    /// Write a report, as `json=FILE`, `junit=FILE` or `html=FILE`.
    /// Repeatable.
    #[arg(long = "reporter", value_name = "TYPE=FILE", value_parser = parse_reporter)]
    reporters: Vec<(ReportFormat, PathBuf)>,
    /// Send even if variables are missing; they are left empty.
    #[arg(long)]
    allow_unresolved: bool,
    /// No colors, even on a terminal.
    #[arg(long)]
    no_color: bool,
    /// Show only failures and the summary.
    #[arg(short, long, conflicts_with = "verbose")]
    quiet: bool,
    /// Also show passing assertions and captured values.
    #[arg(short, long)]
    verbose: bool,
}

fn parse_reporter(raw: &str) -> Result<(ReportFormat, PathBuf), String> {
    let (kind, file) = raw
        .split_once('=')
        .filter(|(_, f)| !f.is_empty())
        .ok_or_else(|| format!("expected TYPE=FILE, got `{raw}`"))?;
    let format = ReportFormat::parse(kind)
        .ok_or_else(|| format!("unknown report type `{kind}`; use json, junit or html"))?;
    Ok((format, PathBuf::from(file)))
}

/// Where to run: the collection root and the folder inside it.
fn target(path: &Path) -> Result<(PathBuf, Option<String>)> {
    if path.is_file() {
        bail!(
            "`{}` is a file; run a folder, or send one request with `apifast send`",
            path.display()
        );
    }
    let root = locate::collection_root(path)?;
    let folder = path
        .canonicalize()
        .with_context(|| format!("cannot open `{}`", path.display()))?;
    let relative = folder.strip_prefix(&root).unwrap_or(Path::new(""));
    let folder: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok((root, (!folder.is_empty()).then(|| folder.join("/"))))
}

fn read_data(file: &Path) -> Result<Vec<Vec<Variable>>> {
    let name = file.to_string_lossy();
    let format = DataFormat::of(&name)
        .with_context(|| format!("`{name}`: the data file must end in .csv or .json"))?;
    let text = std::fs::read_to_string(file).with_context(|| format!("cannot read `{name}`"))?;
    parse_data(&text, format).with_context(|| format!("`{name}`"))
}

/// Runs and reports; the exit code says how it went.
pub fn run(args: &RunArgs) -> Result<ExitCode> {
    let (root, folder) = target(args.path.as_deref().unwrap_or(Path::new(".")))?;
    let data = args.data.as_deref().map(read_data).transpose()?;
    let Wired {
        repo,
        collection,
        responses,
        transport,
        secrets,
    } = wire(&root)?;
    let (clock, random) = (system::SystemClock, system::SystemRandom::new());
    let options = RunOptions {
        folder,
        recursive: args.recursive,
        tags: args.tags.clone(),
        exclude_tags: args.exclude_tags.clone(),
        delay_ms: args.delay,
        bail: args.bail,
        iterations: args.iterations,
        data: data.unwrap_or_default(),
        send: SendOptions {
            environment: args.env.clone(),
            runtime: args
                .vars
                .iter()
                .map(|(n, v)| Variable::text(n, v))
                .collect(),
            allow_unresolved: args.allow_unresolved,
            draft: None,
        },
    };
    let console = Console {
        color: !args.no_color
            && std::env::var_os("NO_COLOR").is_none()
            && std::io::stdout().is_terminal(),
        quiet: args.quiet,
        verbose: args.verbose,
        iterations: if options.data.is_empty() {
            options.iterations
        } else {
            u32::try_from(options.data.len()).unwrap_or(u32::MAX)
        },
    };
    let meta = ReportMeta {
        collection: collection.name.clone(),
        environment: args.env.clone(),
        started_ms: apifast_app::Clock::unix_millis(&clock),
    };
    if !args.quiet {
        Console::header(&meta, options.folder.as_deref());
    }

    // A token obtained once serves the whole run.
    let tokens = apifast_app::memory::MemoryTokens::default();
    let authorizer = crate::terminal_authorizer();
    let (runtime, cancel) = runtime()?;
    let report = runtime.block_on(async {
        let ports = SendPorts {
            collections: &*repo,
            environments: &*repo,
            transport: &transport,
            responses: &responses,
            clock: &clock,
            random: &random,
            prompter: &system::TerminalPrompter,
            secrets: secrets.as_ref(),
            report: &|problem| {
                if !args.quiet {
                    println!(
                        "  {}",
                        console.paint(YELLOW, &format!("warning: {problem}"))
                    );
                }
            },
            history: None,
            tokens: Some(&tokens),
            authorizer: authorizer
                .as_ref()
                .map(|a| a as &dyn apifast_app::Authorizer),
        };
        let on_event = |event: RunEvent<'_>| console.event(&event);
        run_collection(ports, &system::TokioPause, &options, &cancel, &on_event).await
    })?;
    console.footer(&report);

    for (format, file) in &args.reporters {
        if let Some(parent) = file.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create `{}`", parent.display()))?;
        }
        std::fs::write(file, format.render(&report, &meta))
            .with_context(|| format!("cannot write the report `{}`", file.display()))?;
        if !args.quiet {
            println!("Report written to {}", file.display());
        }
    }

    let summary = report.summary();
    Ok(if summary.errors > 0 || report.cancelled {
        ExitCode::from(EXIT_RUN_ERROR)
    } else if summary.failed > 0 {
        ExitCode::from(crate::EXIT_ASSERTION_FAILED)
    } else {
        ExitCode::SUCCESS
    })
}

const GREEN: &str = "32";
const RED: &str = "31";
const YELLOW: &str = "33";
const DIM: &str = "2";

/// The console reporter: a line per request as it finishes, then totals.
struct Console {
    color: bool,
    quiet: bool,
    verbose: bool,
    iterations: u32,
}

impl Console {
    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    fn header(meta: &ReportMeta, folder: Option<&str>) {
        let folder = folder.map(|f| format!(" / {f}")).unwrap_or_default();
        let environment = (meta.environment.as_deref())
            .map(|e| format!(" with environment {e}"))
            .unwrap_or_default();
        println!("Running {}{folder}{environment}", meta.collection);
    }

    fn event(&self, event: &RunEvent<'_>) {
        match event {
            RunEvent::Started {
                iteration, index, ..
            } if *index == 0 && self.iterations > 1 && !self.quiet => {
                println!(
                    "{}",
                    self.paint(
                        DIM,
                        &format!("Iteration {iteration} of {}", self.iterations)
                    )
                );
            }
            RunEvent::Started { .. } => {}
            RunEvent::Finished(step) => self.step(step),
        }
    }

    fn step(&self, step: &StepResult) {
        let passed = step.passed();
        if self.quiet && passed {
            return;
        }
        let place = self.paint(
            DIM,
            &format!("{} · {}", step.path, duration(step.duration_ms)),
        );
        match &step.outcome {
            Err(error) => {
                println!("{} {} {place}", self.paint(RED, "ERROR"), step.name);
                println!("      {}", self.paint(RED, error));
            }
            Ok(outcome) => {
                let mark = if passed {
                    self.paint(GREEN, "PASS ")
                } else {
                    self.paint(RED, "FAIL ")
                };
                println!("{mark} {} {place} · {}", step.name, outcome.head.status);
                for assertion in &outcome.assertions {
                    match &assertion.verdict.message {
                        Some(message) => println!(
                            "      {} {}: {message}",
                            self.paint(RED, "✗"),
                            assertion_text(assertion)
                        ),
                        None if self.verbose => println!(
                            "      {} {}",
                            self.paint(GREEN, "✓"),
                            assertion_text(assertion)
                        ),
                        None => {}
                    }
                }
                for capture in &outcome.captures {
                    match &capture.value {
                        Err(reason) => println!(
                            "      {}",
                            self.paint(
                                YELLOW,
                                &format!("capture of `{}`: {reason}", capture.variable)
                            )
                        ),
                        Ok(value) if self.verbose => {
                            let value = if capture.secret {
                                apifast_app::HIDDEN
                            } else {
                                value.data.as_str()
                            };
                            println!("      {} = {value}", capture.variable);
                        }
                        Ok(_) => {}
                    }
                }
            }
        }
    }

    fn footer(&self, report: &RunReport) {
        if !self.quiet {
            for skipped in &report.skipped {
                println!(
                    "{} {} {}",
                    self.paint(YELLOW, "SKIP "),
                    skipped.path,
                    self.paint(DIM, &format!("· {}", skipped.reason))
                );
            }
        }
        let s = report.summary();
        let mut parts = vec![format!("{} passed", s.passed)];
        if s.failed > 0 {
            parts.push(format!("{} failed", s.failed));
        }
        if s.errors > 0 {
            parts.push(format!("{} with errors", s.errors));
        }
        if s.skipped > 0 {
            parts.push(format!("{} skipped", s.skipped));
        }
        let plural = if s.requests == 1 { "" } else { "s" };
        let mut line = format!(
            "{} request{plural}: {} · {}/{} assertions · {}",
            s.requests,
            parts.join(", "),
            s.assertions_passed,
            s.assertions_passed + s.assertions_failed,
            duration(report.duration_ms)
        );
        if report.bailed {
            line.push_str(" · stopped at the first failure");
        }
        if report.cancelled {
            line.push_str(" · cancelled");
        }
        let code = if report.succeeded() { GREEN } else { RED };
        println!("{}", self.paint(code, &line));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reporters_need_a_known_type_and_a_file() {
        assert_eq!(
            parse_reporter("junit=out/report.xml"),
            Ok((ReportFormat::Junit, PathBuf::from("out/report.xml")))
        );
        assert!(parse_reporter("junit=").is_err());
        assert!(
            parse_reporter("pdf=x.pdf")
                .unwrap_err()
                .contains("unknown report type")
        );
        assert!(parse_reporter("report.xml").is_err());
    }
}
