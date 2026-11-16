# apifast

A desktop client for testing APIs, written in Rust with an egui interface. It takes the best ideas from [Bruno](https://github.com/usebruno/bruno) and [Yaak](https://github.com/mountain-loop/yaak) and combines them into a native, lightweight, file-based application.

> **Status: phases F1 (headless core), F2 (desktop GUI) and F3 (secret vault) done.** `apifast-gui` edits and sends requests from Bruno-compatible YAML collections; `apifast send` does it from the command line.

## What it is

- **Collections as files.** One YAML file per request, folders as directories. Works with git without surprises.
- **Native and lightweight.** A single Rust binary: no Electron, no webview and no helper processes.
- **Encrypted secrets.** An XChaCha20-Poly1305 vault: secrets can be shared inside the repository or kept on the local machine only.
- **One engine.** The GUI and the command line run exactly the same core.
- **Hexagonal architecture.** A pure domain, explicit ports and swappable adapters.

## Key decisions

| Topic | Decision | Details |
|---|---|---|
| Language and UI | Rust + egui (eframe) with wgpu | [ADR 0004](docs/adr/0004-egui-como-ui.md) |
| Architecture | Hexagonal, workspace of crates | [ADR 0001](docs/adr/0001-arquitectura-hexagonal.md) |
| On-disk format | Plain OpenCollection YAML; apifast's own data in `.apifast/` | [ADR 0002](docs/adr/0002-formato-yaml-opencollection.md) |
| Persistence | Files are the source of truth | [ADR 0003](docs/adr/0003-archivos-como-fuente-de-verdad.md) |
| Secrets | Hybrid vault: local-only by default, shared and encrypted by choice | [ADR 0005](docs/adr/0005-boveda-hibrida.md) |
| Tests | Declarative assertions first, JavaScript later | [ADR 0006](docs/adr/0006-scripting-diferido.md) |
| Network | reqwest/hyper + rustls, a single TLS stack | [ADR 0007](docs/adr/0007-pila-http-tls.md) |
| Local state | Plain files, no database | [ADR 0008](docs/adr/0008-estado-local.md) |
| Workspaces | Bruno-compatible `workspace.yml`; several collections open at once, loaded on demand | [ADR 0009](docs/adr/0009-workspaces-y-varias-colecciones.md) |
| Platforms | Linux first; macOS and Windows built in CI | [Vision](docs/01-vision-y-alcance.md) |
| Language | Code in English; UI in English and Spanish | [Quality](docs/11-calidad-y-clean-code.md) |

## Documentation

The design documents and ADRs are written in Spanish.

1. [Vision and scope](docs/01-vision-y-alcance.md)
2. [Analysis of Bruno and Yaak](docs/02-analisis-bruno-y-yaak.md)
3. [Architecture](docs/03-arquitectura.md)
4. [File format](docs/04-formato-de-archivos.md)
5. [Variables and templates](docs/05-variables-y-plantillas.md)
6. [Secret vault](docs/06-boveda-de-secretos.md)
7. [Network engine](docs/07-motor-de-red.md)
8. [Tests and runner](docs/08-tests-y-runner.md)
9. [UI and UX](docs/09-ui-ux.md)
10. [Performance and memory](docs/10-rendimiento-y-memoria.md)
11. [Quality and clean code](docs/11-calidad-y-clean-code.md)
12. [Roadmap](docs/12-roadmap.md)

Architecture decisions live in [docs/adr/](docs/adr/).

## Usage

The desktop app starts in its main view, in the last workspace used: a Bruno-compatible folder with a `workspace.yml` that groups collections, all shown at once in the sidebar. Collections and workspaces are created, opened and switched from inside it (sidebar, command palette or `Ctrl+O`). The first time, "My workspace" is made in the data directory. A folder can also be given:

```
apifast-gui                          # the last workspace
apifast-gui path/to/workspace        # a folder with workspace.yml
apifast-gui path/to/collection       # added to the active workspace
```

From the command line:

```
apifast send users/get.yml --env production
apifast send users/get.yml -e production -i          # status line and headers too
apifast send users/create.yml --var name=Ana --fail  # exit code 4 on HTTP 4xx/5xx
apifast send users/get.yml -e production | jq .      # the body goes to stdout
```

The collection is the nearest folder above the file with an `opencollection.yml` (or `--collection`). A summary and any warnings go to stderr. Responses are kept under the user's data directory (`APIFAST_DATA_DIR` overrides it); `send` never writes into the collection.

Exit codes: 0 when a response arrived (and its assertions passed), 1 on errors or a failed assertion, 3 on usage errors, 4 with `--fail` when the status is 400 or more.

### Running a collection

`apifast run` sends the requests of a collection or a folder in order, checks their assertions and carries captured values from one request to the next:

```
apifast run -r -e staging                                # the whole collection
apifast run users -e staging --tags smoke --bail         # one folder, tagged requests only
apifast run -r -e staging --data users.csv               # one iteration per row
apifast run -r -e ci --reporter junit=out/junit.xml --reporter html=out/report.html
```

It prints a line per request and a summary. Exit codes: 0 when everything passed, 1 when an assertion failed, 2 when a request could not be sent, 3 on usage errors.

### Secrets

A variable marked `secret: true` keeps only its name in the collection; its value lives in the vault, sealed with a master key kept in the system keyring. Values are read from standard input, never from arguments:

```
printf %s "$TOKEN" | apifast vault set production token           # only on this machine
printf %s "$TOKEN" | apifast vault set production token --shared  # for the team, via git
apifast vault status                                              # names and where each value is
apifast vault key show | import | rotate                          # the collection's key
```

In CI, `APIFAST_COLLECTION_KEY=AFK-...` opens shared secrets for one run without storing the key, and `APIFAST_NO_KEYRING=1` keeps the master key in a file instead of a keyring.

## Development

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
```

The Rust version is pinned in `rust-toolchain.toml`; rustup installs it automatically.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the conventions and [ARCHITECTURE.md](ARCHITECTURE.md) for a map of the code.

## Roadmap at a glance

| Phase | Outcome |
|---|---|
| F0 | Documentation, ADRs and technical spikes |
| F1 | Headless core: send a request from a file via the CLI |
| F2 | Minimal GUI, usable day to day |
| F3 | Secret vault |
| F4 | Assertions, captures, runner and reporters |
| F5 | Advanced auth, proxy, certificates, cookies, timeline |
| F6 | Import and export |
| F7 | GraphQL, SSE and WebSocket |
| F8 | gRPC |
| F9 | JavaScript scripting |
| F10 | Version 1.0 |

Each phase is detailed in [docs/12-roadmap.md](docs/12-roadmap.md).

## License

Dual-licensed under **MIT OR Apache-2.0**, at your option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

## References

The analysis was done on local copies of both projects, placed next to this repository (`../bruno` and `../yaak`). Both are MIT-licensed; any code adapted from them must keep its copyright notice.
