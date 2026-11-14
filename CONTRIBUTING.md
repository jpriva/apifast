# Contributing to apifast

Thanks for your interest. apifast is at an early stage (see the [roadmap](docs/12-roadmap.md)), so open an issue to discuss larger changes before writing code.

Read [ARCHITECTURE.md](ARCHITECTURE.md) first: it explains where things live and which rules every change must keep.

## Setup

You need [rustup](https://rustup.rs). The Rust version is pinned in `rust-toolchain.toml`, and rustup installs it, with rustfmt and clippy, the first time you run `cargo` in the repository.

```
git clone <repository URL>
cd apifast
cargo test --workspace
```

`cargo deny` is a separate tool: `cargo install --locked cargo-deny`.

## Before sending a change

Run the same checks as CI:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo deny check
```

A change is done when:

1. The behavior has tests and they pass.
2. All the checks above pass.
3. It does not break any [performance budget](docs/10-rendimiento-y-memoria.md).
4. The affected documentation is updated.
5. It adds no `unwrap`, `unsafe` or new dependency without a reason.

## Conventions

The full guide is [docs/11-calidad-y-clean-code.md](docs/11-calidad-y-clean-code.md) (in Spanish). The essentials:

### Language

- **English:** code, identifiers, comments, rustdoc, test names, error and log messages, commit messages, READMEs and config comments.
- **Spanish:** design documents in `docs/`, ADRs and spike results.
- **UI text:** always through the translation system, never hard-coded.

### Code

- Every public item has a doc comment (`missing_docs` is on).
- Comments explain why, not what.
- `unwrap`, `expect`, `dbg!`, `todo!` and printing are denied outside tests; the CLI is the only crate that prints.
- Each crate defines its error enum with `thiserror`; only the drivers use `anyhow`.
- No error message or log line ever contains a secret.

### Tests

- The domain and templates are developed test-first.
- Every bug fix comes with a test that reproduces it.
- Tests never use the external network or the real keyring.
- Test names describe the behavior: `headers_accumulate_and_override_case_insensitively`.

### Commits

- [Conventional Commits](https://www.conventionalcommits.org): `feat:`, `fix:`, `docs:`, `refactor:`, `build:`, `test:`, with an optional scope such as `feat(domain):`.
- Small, focused changes, each with its tests.
- The body explains why the change is needed.

## Changes that need extra care

- **Architecture.** A decision that changes the architecture is recorded as an ADR in `docs/adr/`. A new crate must be assigned a layer in `crates/apifast-arch-tests/tests/architecture.rs`.
- **File format.** Changes to how collections are read or written update [docs/04-formato-de-archivos.md](docs/04-formato-de-archivos.md) and add cases to the round-trip corpus. Collections must stay openable by Bruno.
- **Vault and cryptography.** Every change to `apifast-vault` gets a dedicated security review. Cryptographic dependencies are updated deliberately, checking the fixed test vectors.
- **Dependencies.** Only permissive licenses are allowed (`deny.toml`). Explain in the pull request why a new dependency is needed.
- **Rust version.** It is bumped on purpose every 2 or 3 releases, in a `build: bump Rust to 1.xx` commit that also fixes new lints.

## License

apifast is dual-licensed under MIT OR Apache-2.0. Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
