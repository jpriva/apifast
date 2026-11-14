# apifast-arch-tests

Test que lee `cargo metadata` y falla si se rompe alguna regla de dependencia de la [arquitectura](../../docs/03-arquitectura.md). Todo crate nuevo del workspace debe clasificarse en una capa en `tests/dependencias.rs`.

**Dependencias permitidas del workspace:** ninguna.
