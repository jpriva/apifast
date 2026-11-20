//! Command line of apifast and composition root.

// The CLI is the only crate that writes straight to the terminal.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod describe;
mod export;
mod import;
mod locate;
mod run;
mod socket;
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
    SendOutcome, SendPorts, call_grpc, connect_request, send_request,
};
use apifast_domain::{Variable, WsMessageKind};
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
    /// Run the requests of a collection or a folder, with their assertions.
    ///
    /// Exits with 0 when everything passed, 1 when an assertion failed, 2
    /// when a request could not be sent and 3 on a usage error.
    Run(run::RunArgs),
    /// Secrets and the collection's key.
    Vault(vault::VaultArgs),
    /// Import a collection from another tool into a new folder.
    ///
    /// Prints what is created and what could not be kept; `--dry-run`
    /// stops there.
    Import(import::ImportArgs),
    /// List what a gRPC request can call: the methods of its .proto file or,
    /// without one, of its server, by reflection.
    ///
    /// One method per line: its path, how it streams, and the messages it
    /// takes and answers with.
    Describe(describe::DescribeArgs),
    /// Export a collection, and its environments, to another tool.
    ///
    /// Prints what could not be kept.
    Export(export::ExportArgs),
}

#[derive(clap::Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag of the command line is a bool"
)]
struct SendArgs {
    /// The request file, inside a collection.
    file: PathBuf,
    #[command(flatten)]
    tls: TlsArgs,
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
    /// Exit with code 4 when the status is 400 or more, or a gRPC call ends
    /// with a status other than OK.
    #[arg(long)]
    fail: bool,
    /// Print no summary to stderr.
    #[arg(short, long)]
    quiet: bool,
}

/// How servers are trusted, for `send` and `run`.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct TlsArgs {
    /// A PEM file of certificate authorities to trust besides the system's.
    /// Repeatable.
    #[arg(long = "cacert", value_name = "FILE")]
    cacert: Vec<PathBuf>,
    /// Do not check the server's certificate. Dangerous: anyone on the way
    /// can read and change the traffic.
    #[arg(long)]
    insecure: bool,
}

impl TlsArgs {
    fn trust(&self) -> apifast_domain::TlsTrust {
        apifast_domain::TlsTrust {
            extra_ca: self.cacert.clone(),
            insecure: self.insecure,
        }
    }
}

fn parse_var(raw: &str) -> Result<(String, String), String> {
    match raw.split_once('=') {
        Some((name, value)) if !name.trim().is_empty() => {
            Ok((name.trim().to_owned(), value.to_owned()))
        }
        _ => Err(format!("expected NAME=VALUE, got `{raw}`")),
    }
}

/// Exit code of a usage error: unknown options, missing arguments.
const EXIT_USAGE: u8 = 3;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            // `--help` and `--version` also come here, and are no error.
            return if error.use_stderr() {
                ExitCode::from(EXIT_USAGE)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    let (result, failure) = match cli.command {
        Command::Send(args) => (run_send(&args), ExitCode::FAILURE),
        Command::Run(args) => (run::run(&args), ExitCode::from(run::EXIT_RUN_ERROR)),
        Command::Vault(args) => (
            vault::run(&args).map(|()| ExitCode::SUCCESS),
            ExitCode::FAILURE,
        ),
        Command::Describe(args) => (describe::run(&args), ExitCode::FAILURE),
        Command::Import(args) => (
            import::run(&args).map(|()| ExitCode::SUCCESS),
            ExitCode::FAILURE,
        ),
        Command::Export(args) => (
            export::run(&args).map(|()| ExitCode::SUCCESS),
            ExitCode::FAILURE,
        ),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            // `{:#}` prints the whole chain of causes on one line.
            eprintln!("error: {error:#}");
            failure
        }
    }
}

/// The adapters of one collection, as both `send` and `run` need them.
struct Wired {
    repo: Arc<FsCollection>,
    collection: apifast_domain::Collection,
    responses: FsResponseStore,
    transport: ReqwestTransport,
    secrets: Box<dyn SecretStore>,
}

fn wire(root: &std::path::Path, tls: &TlsArgs) -> Result<Wired> {
    let repo = Arc::new(FsCollection::open(root)?);
    let collection = repo.collection()?;
    let responses = FsResponseStore::new(apifast_history::dirs::responses_dir(
        root,
        collection
            .id
            .as_ref()
            .map(apifast_domain::CollectionId::as_str),
    ));
    // The collection's proxy, or the system's when it inherits: the CLI has
    // no proxy of its own, and reads the HTTP_PROXY family of variables.
    let proxy = collection
        .proxy
        .choose(&apifast_domain::ProxyChoice::System);
    if tls.insecure {
        eprintln!("warning: --insecure: server certificates are not checked");
    }
    // One jar for the whole command, in memory, as in Bruno's CLI: what a
    // request sets goes with the next ones of the run.
    let transport = ReqwestTransport::configured(
        root,
        proxy,
        &tls.trust(),
        &collection.client_certificates,
        &collection.dns_overrides,
    )?
    .with_cookies(Arc::new(apifast_app::Cookies::new()));
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
    Ok(Wired {
        repo,
        collection,
        responses,
        transport,
        secrets,
    })
}

/// The browser for the OAuth 2.0 authorization code grant, only for a
/// person at a terminal: in CI that grant fails at once instead of waiting.
fn terminal_authorizer() -> Option<apifast_http::LoopbackAuthorizer> {
    std::io::stdin()
        .is_terminal()
        .then(apifast_http::LoopbackAuthorizer::system)
}

/// A runtime for one command, and a token cancelled by Ctrl+C.
fn runtime() -> Result<(tokio::runtime::Runtime, CancelToken)> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?;
    let cancel = CancelToken::new();
    let on_ctrl_c = cancel.clone();
    runtime.spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_ctrl_c.cancel();
        }
    });
    Ok((runtime, cancel))
}

fn run_send(args: &SendArgs) -> Result<ExitCode> {
    let located = locate::locate(&args.file, args.collection.as_deref())?;
    let Wired {
        repo,
        responses,
        transport,
        secrets,
        ..
    } = wire(&located.root, &args.tls)?;
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

    let (messages, grpc) = exchanged(repo.request(&located.request)?.request.body);
    let tokens = apifast_app::memory::MemoryTokens::default();
    let authorizer = terminal_authorizer();
    let (runtime, cancel) = runtime()?;
    // The same ports for both, but for where the body goes.
    macro_rules! ports {
        ($responses:expr) => {
            SendPorts {
                collections: &*repo,
                environments: &*repo,
                transport: &transport,
                responses: $responses,
                clock: &clock,
                random: &random,
                prompter: &system::TerminalPrompter,
                secrets: secrets.as_ref(),
                // Shown before sending, so they are seen even if the send
                // fails.
                report: &|problem| eprintln!("warning: {problem}"),
                history: None,
                tokens: Some(&tokens),
                authorizer: authorizer
                    .as_ref()
                    .map(|a| a as &dyn apifast_app::Authorizer),
            }
        };
    }
    if let Some(messages) = messages {
        let ended = socket::Ended::default();
        let printing = socket::Printing {
            store: &responses,
            quiet: args.quiet,
            grpc,
            ended: ended.clone(),
        };
        let mut outbox = socket::Messages(messages.into(), grpc);
        if grpc {
            let calls = grpc_transport(transport.clone());
            let outcome = runtime.block_on(call_grpc(
                ports!(&printing),
                (&calls, &mut outbox),
                &located.request,
                &options,
                &cancel,
            ))?;
            let ended = ended.lock().map(|e| e.clone()).unwrap_or_default();
            return Ok(call_ended(&outcome, ended, args));
        }
        let sockets = apifast_ws::TungsteniteTransport::new(transport.clone());
        let outcome = runtime.block_on(connect_request(
            ports!(&printing),
            (&sockets, &mut outbox),
            &located.request,
            &options,
            &cancel,
        ))?;
        if !args.quiet {
            eprintln!("{}", summary(&outcome));
        }
        return Ok(ExitCode::SUCCESS);
    }
    let outcome = runtime.block_on(send_request(
        ports!(&responses),
        &located.request,
        &options,
        &cancel,
    ))?;

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

/// The messages of a request that exchanges them, and whether it is a gRPC
/// call: a WebSocket request connects and sends them instead of being
/// sent, and a gRPC one is called with them.
fn exchanged(body: apifast_domain::Body) -> (Option<Vec<apifast_domain::WsMessage>>, bool) {
    match body {
        apifast_domain::Body::WebSocket(messages) => (Some(messages), false),
        apifast_domain::Body::Grpc(call) => {
            let messages = (call.messages.into_iter())
                .map(|m| apifast_domain::WsMessage::new(m.name, WsMessageKind::Json, m.data))
                .collect();
            (Some(messages), true)
        }
        _ => (None, false),
    }
}

/// The gRPC transport of this build, over `http`'s connections: tonic's,
/// or, without the `grpc` feature, one that says there is none.
#[cfg(feature = "grpc")]
fn grpc_transport(http: ReqwestTransport) -> apifast_grpc::TonicTransport {
    apifast_grpc::TonicTransport::new(http)
}

/// The gRPC transport of this build: without the `grpc` feature, one that
/// says there is none.
#[cfg(not(feature = "grpc"))]
fn grpc_transport(_: ReqwestTransport) -> apifast_app::NoGrpc {
    apifast_app::NoGrpc
}

/// Tells how a gRPC call `ended` (its status, then its time) and gives its
/// exit code: with `--fail`, 4 for a status other than `OK`.
fn call_ended(
    outcome: &SendOutcome,
    ended: Option<(Option<u16>, String)>,
    args: &SendArgs,
) -> ExitCode {
    let (code, status) = ended.unwrap_or((None, "ended without a status".into()));
    if !args.quiet || code != Some(0) {
        eprintln!("{status} · {} ms", outcome.head.elapsed_ms);
    }
    if args.fail && code != Some(0) {
        ExitCode::from(EXIT_HTTP_ERROR)
    } else {
        ExitCode::SUCCESS
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
