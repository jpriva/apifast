# OpenAPI fixtures

OpenAPI 3 specs copied from Bruno's repository (MIT license), from its
importer's tests: `tests/import/openapi/`, `tests/import/test-data/` and
`tests/openapi-sync/`. Each file's name is its path there, with `/` as `__`.
Two are broken on purpose (`openapi-broken.json`, `openapi-malformed.yaml`).
They are read by `tests/openapi_corpus.rs`.
