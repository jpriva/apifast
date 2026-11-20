# apifast-cli

**Layer:** Driver

The `apifast` binary: command line and composition root. Wires the use cases of `apifast-app` to the concrete adapters.

**Features:** `grpc` (on by default) brings `apifast-grpc`. Without it (`--no-default-features`), gRPC requests still open and save, and calling them says the build has no gRPC support.

**Allowed workspace dependencies:** any. See the [architecture](../../docs/03-arquitectura.md) (in Spanish).
