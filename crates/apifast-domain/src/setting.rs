use crate::Origin;

/// Valor heredable: lo define este nivel o lo toma del padre más cercano.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Setting<T> {
    /// Se usa el valor del padre más cercano que lo defina.
    #[default]
    Inherit,
    /// Valor propio de este nivel.
    Set(T),
}

impl<T> Setting<T> {
    /// El valor propio, si lo hay.
    pub fn as_set(&self) -> Option<&T> {
        match self {
            Self::Inherit => None,
            Self::Set(value) => Some(value),
        }
    }
}

/// Valor ya resuelto junto con el nivel que lo definió.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved<T> {
    /// El valor efectivo.
    pub value: T,
    /// De dónde sale.
    pub origin: Origin,
}

impl<T> Resolved<T> {
    /// Crea un valor resuelto.
    pub fn new(value: T, origin: Origin) -> Self {
        Self { value, origin }
    }
}
