# apifast-vault

**Layer:** Adapter

Encrypted secrets of apifast: the `enc:v1:` envelope (XChaCha20-Poly1305 with associated data), keys that are wiped when dropped, and the vaults built on them.

**Allowed workspace dependencies:** `apifast-domain`, `apifast-app`. See the [architecture](../../docs/03-arquitectura.md) and the [vault](../../docs/06-boveda-de-secretos.md) (in Spanish).
