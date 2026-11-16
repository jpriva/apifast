//! Command line of apifast and composition root.

// The CLI is the only crate that writes straight to the terminal.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod locate;
mod system;
mod vault;

use std::fmt::Write as _;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use apifast_app::{
    CancelToken, CollectionRepository, NoSecrets, ResponseStore, SecretStore, SendOptions,
    SendOutcome, SendPorts, send_request,
};
use apifast_domain::Variable;
use apifast_history::FsResponseStore;
use apifast_http::ReqwestTransport;
use apifast_storage_fs::FsCollection;
use apifast_vault::{VaultSecrets, parse_shareable_key};
use clap::{Parser, Subcommand};

/// Exit code when `--fail` is set and the status is 400 or more.
const EXIT_HTTP_ERROR: u8 = 4;
/// Exit code when an assertion of the request failed.
const EXIT_ASSERTION_FAILED: u8 = 1;
/// Window used to copy the stored body to stdout.
const COPY_WINDOW: usize = 64 * 1024;

#[derive(Parser)]
#[command(name = "apifast", version, about = "A fast, file-based API client")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Send one request from a collection and print the response body.
    ///
    /// The body goes to stdout and a summary to stderr, so the output can be
    /// piped: `apifast send users/get.yml | jq`.
    Send(SendArgs),
    /// Secrets and the collection's key.
    Vault(vault::VaultArgs),
}

#[derive(clap::Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag of the command line is a bool"
)]
struct SendArgs {
    /// The request file, inside a collection.
    file: PathBuf,
    /// Active environment, by name.
    #[arg(short, long)]
    env: Option<String>,
    /// The collection folder; by default, the nearest folder above the file
    /// with an opencollection.yml.
    #[arg(long)]
    collection: Option<PathBuf>,
    /// A value that overrides every scope, as `NAME=VALUE`. Repeatable.
    #[arg(long = "var", value_name = "NAME=VALUE", value_parser = parse_var)]
    vars: Vec<(String, String)>,
    /// Send even if variables are missing; they are left empty.
    #[arg(long)]
    allow_unresolved: bool,
    /// Also print the status line and headers to stdout, before the body.
    #[arg(short, long)]
    include: bool,
    /// Exit with code 4 when the status is 400 or more.
    #[arg(long)]
    fail: bool,
    /// Print no summary to stderr.
    #[arg(short, long)]
    quiet: bool,
}

fn parse_var(raw: &str) -> Result<(String, String), String> {
    match raw.split_once('=') {
        Some((name, value)) if !name.trim().is_empty() => {
            Ok((name.trim().to_owned(), value.to_owned()))
        }
        _ => Err(format!("expected NAME=VALUE, got `{raw}`")),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Send(args) => run_send(&args),
        Command::Vault(args) => vault::run(&args).map(|()| ExitCode::SUCCESS),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            // `{:#}` prints the whole chain of causes on one line.
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_send(args: &SendArgs) -> Result<ExitCode> {
    let located = locate::locate(&args.file, args.collection.as_deref())?;
    let repo = Arc::new(FsCollection::open(&located.root)?);
    let collection = repo.collection()?;
    let responses = FsResponseStore::new(apifast_history::dirs::responses_dir(
        &located.root,
        collection
            .id
            .as_ref()
            .map(apifast_domain::CollectionId::as_str),
    ));
    let transport = ReqwestTransport::new(&located.root)?;
    // Secrets need the collection's id: without one, none was ever stored.
    let secrets: Box<dyn SecretStore> = match &collection.id {
        Some(id) => {
            let vault = VaultSecrets::for_collection(
                &apifast_history::dirs::data_dir(),
                id.as_str(),
                repo.clone(),
                vault::keyring(),
            )?;
            // CI passes the collection's key in the environment: used for
            // this run only, never stored.
            if let Ok(text) = std::env::var("APIFAST_COLLECTION_KEY") {
                let key = parse_shareable_key(&text).context("APIFAST_COLLECTION_KEY")?;
                vault
                    .use_key_in_memory(key)
                    .context("APIFAST_COLLECTION_KEY")?;
            }
            Box::new(vault)
        }
        None => Box::new(NoSecrets),
    };
    let (clock, random) = (system::SystemClock, system::SystemRandom::new());
    let options = SendOptions {
        environment: args.env.clone(),
        runtime: args
            .vars
            .iter()
            .map(|(n, v)| Variable::text(n, v))
            .collect(),
        allow_unresolved: args.allow_unresolved,
        draft: None,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?;
    let cancel = CancelToken::new();
    let outcome = runtime.block_on(async {
        let on_ctrl_c = cancel.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                on_ctrl_c.cancel();
            }
        });
        let ports = SendPorts {
            collections: &*repo,
            environments: &*repo,
            transport: &transport,
            responses: &responses,
            clock: &clock,
            random: &random,
            prompter: &system::TerminalPrompter,
            secrets: secrets.as_ref(),
            // Shown before sending, so they are seen even if the send fails.
            report: &|problem| eprintln!("warning: {problem}"),
            history: None,
        };
        send_request(ports, &located.request, &options, &cancel).await
    })?;

    if !args.quiet {
        eprintln!("{}", summary(&outcome));
    }
    report_checks(&outcome, args.quiet);
    print_response(&outcome, &responses, args.include)?;

    Ok(if outcome.assertions.iter().any(|a| !a.verdict.passed) {
        ExitCode::from(EXIT_ASSERTION_FAILED)
    } else if args.fail && outcome.head.status >= 400 {
        ExitCode::from(EXIT_HTTP_ERROR)
    } else {
        ExitCode::SUCCESS
    })
}

/// Writes the optional head and the body to stdout. A closed pipe (as in
/// `| head`) is not an error. On a terminal, a final newline is added if the
/// body lacks one, so the prompt does not stick to it; pipes and files get
/// the body's exact bytes.
fn print_response(outcome: &SendOutcome, responses: &FsResponseStore, include: bool) -> Result<()> {
    let stdout = std::io::stdout();
    let on_terminal = stdout.is_terminal();
    let mut out = stdout.lock();
    let result = (|| -> std::io::Result<()> {
        if include {
            writeln!(
                out,
                "{} {}",
                outcome.head.version,
                status_text(outcome.head.status)
            )?;
            for (name, value) in &outcome.head.headers {
                writeln!(out, "{name}: {value}")?;
            }
            writeln!(out)?;
        }
        let mut offset = 0u64;
        let mut last_byte = None;
        loop {
            let window = responses
                .read(&outcome.body.id, offset, COPY_WINDOW)
                .map_err(std::io::Error::other)?;
            if window.is_empty() {
                break;
            }
            out.write_all(&window)?;
            offset += window.len() as u64;
            last_byte = window.last().copied();
        }
        if needs_final_newline(on_terminal, last_byte) {
            writeln!(out)?;
        }
        out.flush()
    })();
    match result {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other.context("cannot write the response"),
    }
}

/// Whether to end the output with a newline the body does not have.
fn needs_final_newline(on_terminal: bool, last_byte: Option<u8>) -> bool {
    on_terminal && last_byte.is_some_and(|b| b != b'\n')
}

/// Captures that found nothing and assertions, on stderr; with `quiet`,
/// only the failures. Their values come already masked.
fn report_checks(outcome: &SendOutcome, quiet: bool) {
    for capture in &outcome.captures {
        if let Err(reason) = &capture.value {
            eprintln!("warning: capture of `{}`: {reason}", capture.variable);
        }
    }
    for result in &outcome.assertions {
        let assertion = &result.assertion;
        let text = format!(
            "{} {}{}",
            assertion.expression.trim(),
            assertion.operator.as_str(),
            result
                .expected
                .as_deref()
                .map(|e| format!(" {e}"))
                .unwrap_or_default()
        );
        match &result.verdict.message {
            None if !quiet => eprintln!("pass  {text}"),
            None => {}
            Some(message) => eprintln!("FAIL  {text}: {message}"),
        }
    }
}

fn summary(outcome: &SendOutcome) -> String {
    let head = &outcome.head;
    let mut text = format!(
        "{} · {} ms · {}",
        status_text(head.status),
        head.elapsed_ms,
        apifast_domain::human_size(outcome.body.size)
    );
    if head.redirects > 0 {
        let plural = if head.redirects == 1 { "" } else { "s" };
        let _ = write!(text, " · {} redirect{plural}", head.redirects);
    }
    text
}

fn status_text(status: u16) -> String {
    match apifast_domain::reason_phrase(status) {
        Some(reason) => format!("{status} {reason}"),
        None => status.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vars_need_a_name_and_an_equals_sign() {
        assert_eq!(parse_var("a=1"), Ok(("a".into(), "1".into())));
        assert_eq!(parse_var("token=a=b"), Ok(("token".into(), "a=b".into())));
        assert_eq!(parse_var("empty="), Ok(("empty".into(), String::new())));
        assert!(parse_var("noequals").is_err());
        assert!(parse_var("=x").is_err());
    }

    #[test]
    fn a_final_newline_is_added_only_on_a_terminal() {
        assert!(needs_final_newline(true, Some(b'}')));
        assert!(!needs_final_newline(true, Some(b'\n')));
        // An empty body prints nothing, so it needs no newline.
        assert!(!needs_final_newline(true, None));
        // Pipes and files get the exact bytes.
        assert!(!needs_final_newline(false, Some(b'}')));
    }

    #[test]
    fn statuses_read_well() {
        assert_eq!(status_text(404), "404 Not Found");
        assert_eq!(status_text(599), "599");
    }

    #[test]
    fn the_command_line_parses() {
        let cli = Cli::try_parse_from([
            "apifast", "send", "r.yml", "-e", "prod", "--var", "a=1", "--var", "b=2", "-i",
            "--fail",
        ])
        .unwrap();
        let Command::Send(args) = cli.command else {
            panic!("not send");
        };
        assert_eq!(args.env.as_deref(), Some("prod"));
        assert_eq!(
            args.vars,
            [("a".into(), "1".into()), ("b".into(), "2".into())]
        );
        assert!(args.include && args.fail && !args.quiet && !args.allow_unresolved);
        assert!(Cli::try_parse_from(["apifast", "send"]).is_err());
        // A secret's value is never an argument.
        assert!(
            Cli::try_parse_from(["apifast", "vault", "set", "prod", "token", "s3cret"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["apifast", "vault", "set", "prod", "token", "--shared"]).is_ok()
        );
    }
}
