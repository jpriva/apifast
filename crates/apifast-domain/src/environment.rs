use crate::Variable;

/// Entorno de una colección (`environments/<nombre>.yml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// Nombre.
    pub name: String,
    /// Color en la interfaz, por ejemplo `#d9534f`.
    pub color: Option<String>,
    /// Nombre del entorno del que hereda variables.
    pub extends: Option<String>,
    /// Variables propias.
    pub variables: Vec<Variable>,
}

impl Environment {
    /// Entorno sin `extends` ni color.
    pub fn new(name: impl Into<String>, variables: Vec<Variable>) -> Self {
        Self {
            name: name.into(),
            color: None,
            extends: None,
            variables,
        }
    }
}

/// Error al resolver la herencia entre entornos.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvironmentError {
    /// No existe un entorno con ese nombre.
    #[error("no existe el entorno «{0}»")]
    NotFound(String),
    /// La cadena de `extends` vuelve a un entorno ya visitado.
    #[error("ciclo en extends: {}", .0.join(" → "))]
    Cycle(Vec<String>),
}

/// Cadena de herencia del entorno `name`, **del más general al más específico**.
///
/// El resultado se pasa a [`Chain::variable_layers`](crate::Chain::variable_layers):
/// las variables del padre se aplican primero y las del hijo las sobrescriben.
///
/// # Errors
///
/// [`EnvironmentError::NotFound`] si falta `name` o algún entorno de su `extends`,
/// y [`EnvironmentError::Cycle`] si la cadena forma un ciclo.
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
    fn la_cadena_va_del_padre_al_hijo() {
        let all = [
            env("prod", Some("base")),
            env("base", None),
            env("prod-eu", Some("prod")),
        ];
        let chain = environment_chain("prod-eu", &all).unwrap();
        assert_eq!(names(&chain), ["base", "prod", "prod-eu"]);
    }

    #[test]
    fn detecta_ciclos() {
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
    fn falla_si_falta_el_padre() {
        let all = [env("prod", Some("base"))];
        assert_eq!(
            environment_chain("prod", &all),
            Err(EnvironmentError::NotFound("base".into()))
        );
    }
}
