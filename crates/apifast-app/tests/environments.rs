//! Environments are created, renamed, saved and deleted without leaving an
//! `extends` that points at nothing or goes round in a circle.

#![allow(clippy::unwrap_used, reason = "test: a failure fails the test")]

use apifast_app::memory::MemoryEnvironments;
use apifast_app::{
    AppError, EnvironmentRepository, create_environment, delete_environment, rename_environment,
    save_environment,
};
use apifast_domain::{Environment, Variable};

fn repo() -> MemoryEnvironments {
    let mut staging = Environment::new("staging", vec![Variable::text("host", "s")]);
    staging.extends = Some("base".into());
    MemoryEnvironments::new(vec![
        Environment::new("base", vec![Variable::text("host", "b")]),
        staging,
    ])
}

fn names(repo: &MemoryEnvironments) -> Vec<String> {
    repo.environments()
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect()
}

#[test]
fn a_new_environment_needs_a_free_name() {
    let repo = repo();
    assert_eq!(create_environment(&repo, "  prod ").unwrap(), "prod");
    assert_eq!(names(&repo), ["base", "staging", "prod"]);
    assert!(matches!(
        create_environment(&repo, "prod"),
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        create_environment(&repo, " "),
        Err(AppError::Validation(_))
    ));
}

#[test]
fn renaming_follows_the_environments_that_extend_it() {
    let repo = repo();
    assert_eq!(
        rename_environment(&repo, "base", "common").unwrap(),
        "common"
    );
    let all = repo.environments().unwrap();
    assert_eq!(all[0].name, "common");
    assert_eq!(all[1].extends.as_deref(), Some("common"));
    assert!(matches!(
        rename_environment(&repo, "common", "staging"),
        Err(AppError::Validation(_))
    ));
    // The same name is not a change.
    assert_eq!(
        rename_environment(&repo, "common", "common").unwrap(),
        "common"
    );
}

#[test]
fn an_environment_others_extend_cannot_be_deleted() {
    let repo = repo();
    let error = delete_environment(&repo, "base").unwrap_err();
    assert_eq!(
        error.to_string(),
        "`base` cannot be deleted: `staging` extend it"
    );
    delete_environment(&repo, "staging").unwrap();
    delete_environment(&repo, "base").unwrap();
    assert_eq!(names(&repo), Vec::<String>::new());
}

#[test]
fn saving_refuses_an_extends_that_is_missing_or_circular() {
    let repo = repo();
    let mut base = repo.environments().unwrap()[0].clone();
    base.extends = Some("staging".into());
    assert!(matches!(
        save_environment(&repo, &base),
        Err(AppError::Environment(_))
    ));
    base.extends = Some("nowhere".into());
    assert!(matches!(
        save_environment(&repo, &base),
        Err(AppError::Environment(_))
    ));
    base.extends = None;
    base.variables.push(Variable::text("token", "t"));
    save_environment(&repo, &base).unwrap();
    assert_eq!(repo.environments().unwrap()[0].variables.len(), 2);
}
