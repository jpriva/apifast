# apifast-gui

**Layer:** Driver

The desktop interface, built with egui on wgpu, and its composition root. Components follow atomic design (`theme/`, `atoms/`, `molecules/`, `organisms/`, `layout/`, `screens/`); see [UI and UX](../../docs/09-ui-ux.md#componentes-atomic-design) (in Spanish).

Run the component gallery with `cargo run -p apifast-gui --example gallery`.

**Features:** `grpc` (on by default) brings `apifast-grpc`. Without it (`--no-default-features`), gRPC requests still open and save, and calling them says the build has no gRPC support.

**Allowed workspace dependencies:** any. See the [architecture](../../docs/03-arquitectura.md) (in Spanish).
