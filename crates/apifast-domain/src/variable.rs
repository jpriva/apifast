use std::collections::BTreeMap;

use crate::Origin;

/// Variable de una colección, carpeta, entorno o request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    /// Nombre, tal como se usa en `{{nombre}}`.
    pub name: String,
    /// Valor. Un secreto se declara sin valor: el valor está en la bóveda.
    pub value: Option<VariableValue>,
    /// Si es `false`, la variable se ignora.
    pub enabled: bool,
    /// Si es un secreto.
    pub secret: bool,
}

impl Variable {
    /// Variable de texto habilitada.
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Some(VariableValue::text(value)),
            enabled: true,
            secret: false,
        }
    }

    /// Secreto habilitado, sin valor.
    pub fn secret(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: None,
            enabled: true,
            secret: true,
        }
    }
}

/// Valor tipado de una variable.
///
/// Se guarda el texto tal como está en el archivo (`data`), para reescribirlo
/// sin cambios. La conversión al tipo real se hace al usarlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableValue {
    /// Tipo declarado.
    pub kind: ValueKind,
    /// Texto del valor.
    pub data: String,
}

impl VariableValue {
    /// Valor de texto.
    pub fn text(data: impl Into<String>) -> Self {
        Self {
            kind: ValueKind::String,
            data: data.into(),
        }
    }
}

/// Tipo de una variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueKind {
    /// Texto.
    #[default]
    String,
    /// Número.
    Number,
    /// Booleano.
    Bool,
    /// Objeto JSON.
    Object,
}

/// Variable resuelta, con su procedencia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedVariable {
    /// Valor efectivo. `None` en un secreto cuyo valor aún no se descifró.
    pub value: Option<VariableValue>,
    /// Si es un secreto.
    pub secret: bool,
    /// Ámbito que define el valor efectivo.
    pub origin: Origin,
    /// Ámbitos sobrescritos, del de menor al de mayor prioridad.
    pub overridden: Vec<Origin>,
}

/// Variables resueltas de todos los ámbitos, por nombre.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedVariables {
    by_name: BTreeMap<String, ResolvedVariable>,
}

impl ResolvedVariables {
    /// Busca una variable por nombre.
    pub fn get(&self, name: &str) -> Option<&ResolvedVariable> {
        self.by_name.get(name)
    }

    /// Recorre las variables ordenadas por nombre.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ResolvedVariable)> {
        self.by_name.iter().map(|(name, var)| (name.as_str(), var))
    }

    /// Número de variables.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// Si no hay ninguna variable.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

/// Combina las variables de varios ámbitos.
///
/// `layers` va **de menor a mayor prioridad**: si un nombre se repite, gana el
/// último. Las variables deshabilitadas se ignoran. Para el orden de los ámbitos
/// de una request, ver [`Chain::variable_layers`](crate::Chain::variable_layers).
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
    fn gana_el_ambito_de_mayor_prioridad_y_registra_los_sobrescritos() {
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
    fn las_variables_deshabilitadas_no_sobrescriben() {
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
    fn un_secreto_conserva_su_marca_y_no_tiene_valor() {
        let env = [Variable::secret("api_key")];
        let vars = resolve_variables([(Origin::Environment("prod".into()), &env[..])]);

        let key = vars.get("api_key").unwrap();
        assert!(key.secret);
        assert_eq!(key.value, None);
    }
}
