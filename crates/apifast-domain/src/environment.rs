use crate::Variable;

/// Environment of a collection (`environments/<name>.yml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// Name.
    pub name: String,
    /// Color in the UI, such as `#d9534f`.
    pub color: Option<String>,
    /// Name of the environment it inherits variables from.
    pub extends: Option<String>,
    /// Own variables.
    pub variables: Vec<Variable>,
}

impl Environment {
    /// Environment with no `extends` and no color.
    pub fn new(name: impl Into<String>, variables: Vec<Variable>) -> Self {
        Self {
            name: name.into(),
            color: None,
            extends: None,
            variables,
        }
    }
}

/// Error while resolving inheritance between environments.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvironmentError {
    /// There is no environment with that name.
    #[error("environment `{0}` does not exist")]
    NotFound(String),
    /// The `extends` chain comes back to an environment already visited.
    #[error("cycle in extends: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
}

/// Inheritance chain of environment `name`, **from most general to most specific**.
///
/// Pass the result to [`Chain::variable_layers`](crate::Chain::variable_layers):
/// the parent's variables apply first and the child's override them.
///
/// # Errors
///
/// [`EnvironmentError::NotFound`] if `name` or any environment in its `extends`
/// chain is missing, and [`EnvironmentError::Cycle`] if the chain forms a cycle.
pub fn environment_chain<'a>(
    name: &str,
    environments: &'a [Environment],
) -> Result<Vec<&'a Environment>, EnvironmentError> {
    let find = |n: &str| {
        environments
            .iter()
            .find(|e| e.name == n)
            .ok_or_else(|| EnvironmentError::NotFound(n.to_owned()))
    };

    let mut chain = vec![find(name)?];
    while let Some(parent) = chain.last().and_then(|e| e.extends.as_deref()) {
        if chain.iter().any(|e| e.name == parent) {
            let mut names: Vec<String> = chain.iter().map(|e| e.name.clone()).collect();
            names.push(parent.to_owned());
            return Err(EnvironmentError::Cycle(names));
        }
        chain.push(find(parent)?);
    }
    chain.reverse();
    Ok(chain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str, extends: Option<&str>) -> Environment {
        Environment {
            extends: extends.map(str::to_owned),
            ..Environment::new(name, vec![])
        }
    }

    fn names(chain: &[&Environment]) -> Vec<String> {
        chain.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn chain_goes_from_parent_to_child() {
        let all = [
            env("prod", Some("base")),
            env("base", None),
            env("prod-eu", Some("prod")),
        ];
        let chain = environment_chain("prod-eu", &all).unwrap();
        assert_eq!(names(&chain), ["base", "prod", "prod-eu"]);
    }

    #[test]
    fn detects_cycles() {
        let all = [env("a", Some("b")), env("b", Some("a"))];
        assert_eq!(
            environment_chain("a", &all),
            Err(EnvironmentError::Cycle(vec![
                "a".into(),
                "b".into(),
                "a".into()
            ]))
        );
    }

    #[test]
    fn fails_when_the_parent_is_missing() {
        let all = [env("prod", Some("base"))];
        assert_eq!(
            environment_chain("prod", &all),
            Err(EnvironmentError::NotFound("base".into()))
        );
    }
}
