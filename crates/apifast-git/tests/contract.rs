//! gix passes the contract of the git port.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

apifast_git_contract::git_repository_contract!(|root: &std::path::Path| {
    apifast_git::GixRepository::discover(root)
        .unwrap()
        .map(|repo| Box::new(repo) as Box<dyn apifast_app::GitRepository>)
});
