# apifast-query

**Layer:** Domain

JSONPath (RFC 9535) and XPath 1.0 queries over response bodies: what `response(path=…)`, assertions and the response filter use. Pure and bounded: a query that visits too much stops with an error.

**Allowed workspace dependencies:** `apifast-domain`. The only crate that uses `roxmltree`. See the [architecture](../../docs/03-arquitectura.md) (in Spanish).
