# apifast-git-contract

**Layer:** Test support (only as a dev dependency)

What every adapter of the `GitRepository` port must do, as tests any of them can run. An adapter's tests pass a function that opens the repository of a collection, and the macro writes one test per scenario:

```rust
apifast_git_contract::git_repository_contract!(|root: &std::path::Path| {
    apifast_git::GixRepository::discover(root)
        .unwrap()
        .map(|repo| Box::new(repo) as Box<dyn apifast_app::GitRepository>)
});
```

The repositories are made with the `git` binary, which the tests need; the adapters do not.

**Allowed workspace dependencies:** `apifast-domain`, `apifast-app`. See the [architecture](../../docs/03-arquitectura.md) (in Spanish).
