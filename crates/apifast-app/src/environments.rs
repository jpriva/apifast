//! Creating, renaming, saving and deleting environments, keeping the
//! `extends` links between them valid.

use apifast_domain::{Environment, environment_chain};

use crate::{AppError, EnvironmentRepository};

fn valid_name(name: &str, environments: &[Environment]) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("an environment needs a name".into()));
    }
    if environments.iter().any(|e| e.name == name) {
        return Err(AppError::Validation(format!(
            "an environment named `{name}` already exists"
        )));
    }
    Ok(name.to_owned())
}

/// Creates an empty environment named `name` (trimmed). Returns its name.
///
/// # Errors
///
/// [`AppError::Validation`] if the name is blank or taken; the repository
/// error if it cannot be written.
pub fn create_environment(
    repo: &impl EnvironmentRepository,
    name: &str,
) -> Result<String, AppError> {
    let name = valid_name(name, &repo.environments()?)?;
    repo.save_environment(&Environment::new(name.clone(), Vec::new()))?;
    Ok(name)
}

/// Renames environment `from` to `to` (trimmed), and the `extends` of the
/// environments that inherit from it. Returns the new name.
///
/// # Errors
///
/// [`AppError::Validation`] if the name is blank or taken; the repository
/// error if a file cannot be written.
pub fn rename_environment(
    repo: &impl EnvironmentRepository,
    from: &str,
    to: &str,
) -> Result<String, AppError> {
    if from == to.trim() {
        return Ok(from.to_owned());
    }
    let environments = repo.environments()?;
    let to = valid_name(to, &environments)?;
    repo.rename_environment(from, &to)?;
    for child in environments
        .iter()
        .filter(|e| e.extends.as_deref() == Some(from))
    {
        repo.save_environment(&Environment {
            extends: Some(to.clone()),
            ..child.clone()
        })?;
    }
    Ok(to)
}

/// Saves `environment`, found by its name.
///
/// # Errors
///
/// [`AppError::Environment`] if its `extends` names an environment that
/// does not exist or comes back to it; the repository error if it cannot be
/// written.
pub fn save_environment(
    repo: &impl EnvironmentRepository,
    environment: &Environment,
) -> Result<(), AppError> {
    let mut environments = repo.environments()?;
    match environments.iter_mut().find(|e| e.name == environment.name) {
        Some(existing) => environment.clone_into(existing),
        None => environments.push(environment.clone()),
    }
    environment_chain(&environment.name, &environments)?;
    repo.save_environment(environment)?;
    Ok(())
}

/// Deletes environment `name`.
///
/// # Errors
///
/// [`AppError::Validation`] if other environments extend it, so none is
/// left pointing at nothing; the repository error if it cannot be removed.
pub fn delete_environment(repo: &impl EnvironmentRepository, name: &str) -> Result<(), AppError> {
    let children: Vec<String> = repo
        .environments()?
        .into_iter()
        .filter(|e| e.extends.as_deref() == Some(name))
        .map(|e| format!("`{}`", e.name))
        .collect();
    if !children.is_empty() {
        return Err(AppError::Validation(format!(
            "`{name}` cannot be deleted: {} extend it",
            children.join(", ")
        )));
    }
    repo.delete_environment(name)?;
    Ok(())
}
