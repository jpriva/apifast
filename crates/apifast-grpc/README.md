# apifast-grpc

**Layer:** Adapter

gRPC transport of apifast. The connection (TCP, TLS trust, client certificates, DNS overrides) is opened by `apifast-http`; tonic speaks HTTP/2 and gRPC over it. Services are discovered by reflection (v1, falling back to v1alpha) or from `.proto` files, compiled by protox, with no external `protoc`.

**Allowed workspace dependencies:** `apifast-domain`, `apifast-app`, `apifast-http`. See the [architecture](../../docs/03-arquitectura.md) (in Spanish).
