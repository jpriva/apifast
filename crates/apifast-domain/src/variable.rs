use std::collections::BTreeMap;

use crate::Origin;

/// Variable of a collection, folder, environment or request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    /// Name, as used in `{{name}}`.
    pub name: String,
    /// Value. A secret is declared without one: its value lives in the vault.
    pub value: Option<VariableValue>,
    /// When `false`, the variable is ignored.
    pub enabled: bool,
    /// Whether it is a secret.
    pub secret: bool,
}

impl Variable {
    /// Enabled text variable.
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Some(VariableValue::text(value)),
            enabled: true,
            secret: false,
        }
    }

    /// Enabled secret, with no value.
    pub fn secret(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: None,
            enabled: true,
            secret: true,
        }
    }
}

/// Typed value of a variable.
///
/// The text is kept exactly as written in the file (`data`), so it can be
/// written back unchanged. It is converted to its real type when used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableValue {
    /// Declared type.
    pub kind: ValueKind,
    /// Text of the value.
    pub data: String,
}

impl VariableValue {
    /// Text value.
    pub fn text(data: impl Into<String>) -> Self {
        Self {
            kind: ValueKind::String,
            data: data.into(),
        }
    }
}

/// Type of a variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueKind {
    /// Text.
    #[default]
    String,
    /// Number.
    Number,
    /// Boolean.
    Bool,
    /// JSON object.
    Object,
}

/// A resolved variable, with its origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedVariable {
    /// Effective value. `None` for a secret that has not been decrypted yet.
    pub value: Option<VariableValue>,
    /// Whether it is a secret.
    pub secret: bool,
    /// Scope that defines the effective value.
    pub origin: Origin,
    /// Overridden scopes, from lowest to highest precedence.
    pub overridden: Vec<Origin>,
}

/// Resolved variables from every scope, by name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedVariables {
    by_name: BTreeMap<String, ResolvedVariable>,
}

impl ResolvedVariables {
    /// Looks a variable up by name.
    pub fn get(&self, name: &str) -> Option<&ResolvedVariable> {
        self.by_name.get(name)
    }

    /// Iterates over the variables, sorted by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ResolvedVariable)> {
        self.by_name.iter().map(|(name, var)| (name.as_str(), var))
    }

    /// Number of variables.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Whether there are no variables.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Secrets of environments that have no value yet: their name and the
    /// environment that declares them. Their values live in the vault.
    pub fn secrets_without_value(&self) -> Vec<(String, String)> {
        self.by_name
            .iter()
            .filter(|(_, var)| var.secret && var.value.is_none())
            .filter_map(|(name, var)| match &var.origin {
                Origin::Environment(environment) => Some((name.clone(), environment.clone())),
                _ => None,
            })
            .collect()
    }

    /// Gives secret `name` the value read from the vault. Does nothing for a
    /// variable that is not a secret.
    pub fn fill_secret(&mut self, name: &str, value: VariableValue) {
        if let Some(var) = self.by_name.get_mut(name).filter(|var| var.secret) {
            var.value = Some(value);
        }
    }

    /// Adds secret `name`, above every scope: a value obtained for this send,
    /// such as an OAuth 2.0 token, that must be masked wherever it shows.
    pub fn add_secret(&mut self, name: &str, value: VariableValue) {
        self.by_name.insert(
            name.to_owned(),
            ResolvedVariable {
                value: Some(value),
                secret: true,
                origin: Origin::Runtime,
                overridden: Vec::new(),
            },
        );
    }

    /// Sets `name` to a value captured from a response, above every scope.
    /// A secret stays a secret, so its new value is masked as well.
    pub fn capture(&mut self, name: &str, value: VariableValue) {
        let previous = self.by_name.remove(name);
        let secret = previous.as_ref().is_some_and(|p| p.secret);
        let overridden = previous
            .map(|p| {
                let mut chain = p.overridden;
                chain.push(p.origin);
                chain
            })
            .unwrap_or_default();
        self.by_name.insert(
            name.to_owned(),
            ResolvedVariable {
                value: Some(value),
                secret,
                origin: Origin::Runtime,
                overridden,
            },
        );
    }
}

/// Merges the variables of several scopes.
///
/// `layers` goes **from lowest to highest precedence**: when a name repeats, the
/// last one wins. Disabled variables are ignored. For the scope order of a
/// request, see [`Chain::variable_layers`](crate::Chain::variable_layers).
pub fn resolve_variables<'a>(
    layers: impl IntoIterator<Item = (Origin, &'a [Variable])>,
) -> ResolvedVariables {
    let mut by_name: BTreeMap<String, ResolvedVariable> = BTreeMap::new();
    for (origin, variables) in layers {
        for var in variables.iter().filter(|v| v.enabled) {
            let overridden = by_name
                .remove(&var.name)
                .map(|previous| {
                    let mut chain = previous.overridden;
                    chain.push(previous.origin);
                    chain
                })
                .unwrap_or_default();
            by_name.insert(
                var.name.clone(),
                ResolvedVariable {
                    value: var.value.clone(),
                    secret: var.secret,
                    origin: origin.clone(),
                    overridden,
                },
            );
        }
    }
    ResolvedVariables { by_name }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_secrets_of_environments_are_filled_from_the_vault() {
        let env = [Variable::secret("token"), Variable::text("host", "h")];
        let request = [Variable::secret("local_only")];
        let mut vars = resolve_variables([
            (Origin::Environment("prod".into()), &env[..]),
            (Origin::Request, &request[..]),
        ]);
        assert_eq!(
            vars.secrets_without_value(),
            [("token".to_owned(), "prod".to_owned())]
        );
        vars.fill_secret("token", VariableValue::text("t0k"));
        vars.fill_secret("host", VariableValue::text("ignored"));
        assert_eq!(
            vars.get("token").unwrap().value,
            Some(VariableValue::text("t0k"))
        );
        assert_eq!(
            vars.get("host").unwrap().value,
            Some(VariableValue::text("h"))
        );
        assert_eq!(vars.secrets_without_value(), Vec::new());
    }

    #[test]
    fn highest_precedence_scope_wins_and_records_overridden_ones() {
        let collection = [Variable::text("host", "a")];
        let env = [Variable::text("host", "b")];
        let request = [Variable::text("host", "c")];
        let vars = resolve_variables([
            (Origin::Collection, &collection[..]),
            (Origin::Environment("local".into()), &env[..]),
            (Origin::Request, &request[..]),
        ]);

        let host = vars.get("host").unwrap();
        assert_eq!(host.value, Some(VariableValue::text("c")));
        assert_eq!(host.origin, Origin::Request);
        assert_eq!(
            host.overridden,
            [Origin::Collection, Origin::Environment("local".into())]
        );
    }

    #[test]
    fn disabled_variables_do_not_override() {
        let collection = [Variable::text("host", "a")];
        let mut disabled = Variable::text("host", "b");
        disabled.enabled = false;
        let request = [disabled];
        let vars = resolve_variables([
            (Origin::Collection, &collection[..]),
            (Origin::Request, &request[..]),
        ]);

        let host = vars.get("host").unwrap();
        assert_eq!(host.origin, Origin::Collection);
        assert_eq!(host.overridden, Vec::<Origin>::new());
    }

    #[test]
    fn a_secret_keeps_its_flag_and_has_no_value() {
        let env = [Variable::secret("api_key")];
        let vars = resolve_variables([(Origin::Environment("prod".into()), &env[..])]);

        let key = vars.get("api_key").unwrap();
        assert!(key.secret);
        assert_eq!(key.value, None);
    }
}
