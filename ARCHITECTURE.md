# Architecture

This file is a map of the code: what lives where and which rules hold everywhere. The full design, with diagrams and rationale, is in [docs/03-arquitectura.md](docs/03-arquitectura.md) and the [ADRs](docs/adr/) (in Spanish).

## Bird's-eye view

apifast reads a collection of YAML files from disk, resolves inheritance and variables, sends requests and shows the responses. The GUI and the CLI are two thin drivers over the same core.

The code follows a hexagonal architecture ([ADR 0001](docs/adr/0001-arquitectura-hexagonal.md)):

```
drivers      apifast-gui, apifast-cli          wire everything together
   │
application  apifast-app                       use cases + ports (traits)
   │
domain       apifast-domain, apifast-template  pure logic, no I/O
   ▲
adapters     apifast-storage-fs, apifast-http, ...   implement the ports
```

Adapters depend on the application layer, never the other way round. Only the drivers know the concrete adapter types.

A request travels like this:

1. `apifast-storage-fs` loads the collection, its folders and the request into domain types.
2. `apifast-domain::resolve` applies collection → folder → request inheritance and returns a `ResolvedRequest`, recording the origin of every value.
3. `apifast-template` renders `{{variables}}` using the scopes merged by `apifast-domain::resolve_variables`.
4. `apifast-http` sends it and streams the body to `apifast-history`.

## Code map

Crates live in `crates/`. Crates marked *planned* do not exist yet; the [roadmap](docs/12-roadmap.md) says when they arrive.

| Crate | Layer | Responsibility |
|---|---|---|
| `apifast-domain` | Domain | Entities (`Collection`, `Folder`, `HttpRequest`, `Environment`, `Variable`, `Auth`, `Body`), `Setting<T>`, inheritance resolution with `Origin` |
| `apifast-template` | Domain | Template parser and renderer (`{{name}}`, dynamic variables) |
| `apifast-app` | Application | Use cases (`SendRequest`, `SaveRequest`, ...) and ports (`CollectionRepository`, `HttpTransport`, ...) |
| `apifast-storage-fs` | Adapter | OpenCollection YAML read/write with `oneyaml`, atomic writes, `.apifast/` folder |
| `apifast-http` | Adapter | HTTP transport with reqwest and rustls |
| `apifast-history` | Adapter | Response bodies and history on disk |
| `apifast-vault` | Adapter | Encrypted secrets and keyring (*planned*, F3) |
| `apifast-watch` | Adapter | File system watcher (*planned*, F2) |
| `apifast-import` | Adapter | Importers and exporters (*planned*, F6) |
| `apifast-ws`, `apifast-grpc` | Adapter | WebSocket and gRPC (*planned*, F7–F8) |
| `apifast-script-js` | Adapter | JavaScript engine (*planned*, F9) |
| `apifast-gui` | Driver | egui interface and composition root (*planned*, F2) |
| `apifast-cli` | Driver | The `apifast` binary and composition root |
| `apifast-arch-tests` | Tooling | Tests that enforce the dependency rules below |

Other top-level folders:

- `docs/`: design documents, ADRs and spike results (Spanish).
- `spikes/`: throwaway F0 prototypes, kept so their measurements can be reproduced. Not part of the workspace.

## Invariants

These rules are checked by CI or by the type system. Breaking one needs an ADR.

- **The domain has no I/O.** `apifast-domain` and `apifast-template` depend on no other workspace crate (except template → domain) and on none of tokio, serde, reqwest, egui or oneyaml. Serialization DTOs live in the adapters.
- **Dependencies point inwards.** The application depends only on the domain; an adapter depends only on the domain and the application, never on another adapter. `apifast-arch-tests` enforces this from `cargo metadata`, and every new crate must be given a layer there.
- **Only resolved requests are sent.** `ResolvedRequest` has private fields and is built only by `resolve`, so sending a request without applying inheritance does not compile.
- **Files are the source of truth** ([ADR 0003](docs/adr/0003-archivos-como-fuente-de-verdad.md)). Collections are plain OpenCollection YAML that Bruno can open; unknown keys are preserved on save. apifast's own data goes in `.apifast/`, never inside OpenCollection files ([ADR 0002](docs/adr/0002-formato-yaml-opencollection.md)).
- **Secrets never leak.** No error message, log line or `Debug` output contains a secret value; auth fields hold templates such as `{{token}}`, not the secret itself.
- **No `unsafe`, no `unwrap`.** `unsafe_code` is forbidden workspace-wide; `unwrap` and `expect` are denied outside tests.

## Where to start

| To change... | Look at |
|---|---|
| How headers, auth or settings are inherited | `crates/apifast-domain/src/resolve.rs` |
| Variable precedence | `crates/apifast-domain/src/variable.rs`, `Chain::variable_layers` |
| The on-disk format | `crates/apifast-storage-fs` and [docs/04-formato-de-archivos.md](docs/04-formato-de-archivos.md) |
| Layer rules | `crates/apifast-arch-tests/tests/architecture.rs` |
| Lints and CI checks | `[workspace.lints]` in `Cargo.toml`, `deny.toml`, `.github/workflows/ci.yml` |
