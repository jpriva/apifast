# apifast-log

**Layer:** Adapter

Where apifast's log goes and at which level: the terminal, and a file in the data folder that rotates when it grows. The other crates only emit events with `tracing`; this one is the only user of `tracing-subscriber`. The level comes from `APIFAST_LOG` (the `tracing` filter syntax) or from a setting, and changes while running.

**Allowed workspace dependencies:** `apifast-domain`, `apifast-app`. See the [architecture](../../docs/03-arquitectura.md) (in Spanish).
