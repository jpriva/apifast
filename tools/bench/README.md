# Benchmarks

Scripts that measure the budgets of [docs/10-rendimiento-y-memoria.md](../../docs/10-rendimiento-y-memoria.md) (in Spanish) on a real window. Linux only: they read `/proc`.

| Script | What it does |
|---|---|
| `make_collection.py DIR N` | Writes a synthetic collection of `N` requests |
| `measure.py BINARY COLLECTION` | Startup times, PSS after 10 s idle, idle CPU |
| `breakdown.py BINARY [COLLECTION]` | PSS by mapping, to see where memory goes; apifast's own memory (what the budgets measure) apart from the system's graphics stack |

```sh
cargo build --release -p apifast-gui
for n in 100 1000 5000; do python3 tools/bench/make_collection.py /tmp/c$n $n; done
for n in 100 1000 5000; do python3 tools/bench/measure.py target/release/apifast-gui /tmp/c$n; done
```

Three more need the real window or the core, and run as an example of `apifast-gui`:

```sh
cargo run --release -p apifast-gui --example bench -- response   # memory to receive and show 100 MB
cargo run --release -p apifast-gui --example bench -- typing     # CPU per keystroke in a 200 KB body
cargo run --release -p apifast-gui --example bench -- send       # time of a send to a local server
```

The startup times come from `APIFAST_TIMINGS=1`, which makes `apifast-gui` write when it showed its first frame and the collection's tree.
