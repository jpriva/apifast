//! `apifast describe` (docs/08, F8): what a gRPC request can call, as its
//! `.proto` file describes it or, without one, as its server says by
//! reflection. One method per line: its path, how it streams, and the
//! messages it takes and answers with.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use apifast_app::{SendOptions, SendPorts, describe_grpc};
use apifast_domain::{GrpcService, Variable};

use crate::{TlsArgs, Wired, locate, parse_var, runtime, system, terminal_authorizer, wire};

#[derive(clap::Args)]
pub struct DescribeArgs {
    /// The gRPC request file, inside a collection.
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
    /// Go on even if variables are missing; they are left empty.
    #[arg(long)]
    allow_unresolved: bool,
}

pub fn run(args: &DescribeArgs) -> Result<ExitCode> {
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
        runtime: (args.vars.iter())
            .map(|(n, v)| Variable::text(n, v))
            .collect(),
        allow_unresolved: args.allow_unresolved,
        draft: None,
    };
    let tokens = apifast_app::memory::MemoryTokens::default();
    let authorizer = terminal_authorizer();
    let (runtime, cancel) = runtime()?;
    let grpc = crate::grpc_transport(transport.clone());
    let ports = SendPorts {
        collections: &*repo,
        environments: &*repo,
        transport: &transport,
        responses: &responses,
        clock: &clock,
        random: &random,
        prompter: &system::TerminalPrompter,
        secrets: secrets.as_ref(),
        report: &|problem| eprintln!("warning: {problem}"),
        history: None,
        tokens: Some(&tokens),
        authorizer: (authorizer.as_ref()).map(|a| a as &dyn apifast_app::Authorizer),
    };
    let catalog = runtime.block_on(describe_grpc(
        ports,
        &grpc,
        &located.request,
        &options,
        &cancel,
    ))?;
    print!("{}", listing(&catalog.services));
    Ok(ExitCode::SUCCESS)
}

/// The methods of `services`, one per line, in aligned columns.
fn listing(services: &[GrpcService]) -> String {
    let methods: Vec<_> = services.iter().flat_map(|s| &s.methods).collect();
    let width = |f: &dyn Fn(&&apifast_domain::GrpcMethod) -> usize| {
        methods.iter().map(f).max().unwrap_or(0)
    };
    let (path, kind) = (
        width(&|m| m.path.len()),
        width(&|m| m.method_type.as_str().len()),
    );
    methods.iter().fold(String::new(), |mut out, m| {
        let _ = writeln!(
            out,
            "{:<path$}  {:<kind$}  {} -> {}",
            m.path,
            m.method_type.as_str(),
            m.input,
            m.output
        );
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use apifast_domain::{GrpcMethod, GrpcMethodType};

    #[test]
    fn methods_are_listed_in_columns() {
        let method = |name: &str, kind| GrpcMethod {
            name: name.into(),
            path: format!("/h.G/{name}"),
            method_type: kind,
            input: "h.In".into(),
            output: "h.Out".into(),
        };
        let services = [GrpcService {
            name: "h.G".into(),
            methods: vec![
                method("Hi", GrpcMethodType::Unary),
                method("Chat", GrpcMethodType::BidiStreaming),
            ],
        }];
        assert_eq!(
            listing(&services),
            "/h.G/Hi    unary           h.In -> h.Out\n/h.G/Chat  bidi-streaming  h.In -> h.Out\n"
        );
    }
}
