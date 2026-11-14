# apifast-arch-tests

A test that reads `cargo metadata` and fails when a dependency rule of the [architecture](../../docs/03-arquitectura.md) (in Spanish) is broken. Every new workspace crate must be assigned a layer in `tests/architecture.rs`.

**Allowed workspace dependencies:** none.
