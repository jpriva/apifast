# apifast-ws

**Layer:** Adapter

WebSocket transport of apifast. The handshake goes through `apifast-http`, so it shares its proxy, TLS, client certificates, DNS overrides and cookies; tokio-tungstenite reads and writes the frames.

**Allowed workspace dependencies:** `apifast-domain`, `apifast-app`, `apifast-http`. See the [architecture](../../docs/03-arquitectura.md) (in Spanish).
