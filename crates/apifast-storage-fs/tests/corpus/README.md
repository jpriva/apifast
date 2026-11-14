# Round-trip corpus

`bruno/` holds every OpenCollection YAML file from the test fixtures of
[Bruno](https://github.com/usebruno/bruno), copied unchanged from commit
`09625f67c9957b8eb8b04fb73756c6b3c7aa7679` (2026-11-12). They are MIT-licensed;
see `bruno/LICENSE.md`.

`bruno/app-4.3.0-saved/` is different: a collection that the Bruno 4.3.0 app
itself opened, edited and saved (see `docs/spikes/03-bruno-ignora-apifast.md`),
so it holds Bruno's real output, including the defaults it adds on save.

The tests in `tests/corpus.rs` read every file and check that saving it
unchanged leaves it byte for byte identical. A few files are broken on purpose
(error fixtures); those must fail to read, never panic.

To refresh, copy the `.yml` files under `bruno/tests` and `bruno/packages`
whose lines start with `info:`, `opencollection:` or `variables:`, skipping
`node_modules`.
