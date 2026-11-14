use crate::Origin;

/// Inheritable value: either set at this level or taken from the closest parent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Setting<T> {
    /// Use the value of the closest parent that sets it.
    #[default]
    Inherit,
    /// This level's own value.
    Set(T),
}

impl<T> Setting<T> {
    /// The own value, if any.
    pub fn as_set(&self) -> Option<&T> {
        match self {
            Self::Inherit => None,
            Self::Set(value) => Some(value),
        }
    }
}

/// A resolved value together with the level that defined it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved<T> {
    /// The effective value.
    pub value: T,
    /// Where it comes from.
    pub origin: Origin,
}

impl<T> Resolved<T> {
    /// Creates a resolved value.
    pub fn new(value: T, origin: Origin) -> Self {
        Self { value, origin }
    }
}
