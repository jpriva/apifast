//! Small helpers over [`oneyaml::Value`], shaped after how Bruno reads its
//! YAML: missing or odd values fall back to defaults instead of failing.

use oneyaml::{Mapping, Number, Value};

/// `value[key]`, if `value` is a mapping that has it.
pub(crate) fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_mapping()?.get_str(key)
}

/// Follows a path of keys.
pub(crate) fn get_in<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |v, key| get(v, key))
}

/// Text of a scalar, like Bruno's `ensureString`: numbers and booleans become
/// their text; null, sequences and mappings give `None`.
pub(crate) fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// [`text`], or an empty string.
pub(crate) fn text_or_empty(value: Option<&Value>) -> String {
    text(value).unwrap_or_default()
}

/// The items of a sequence, or none.
pub(crate) fn items(value: Option<&Value>) -> &[Value] {
    value
        .and_then(Value::as_sequence)
        .map_or(&[], oneyaml::Sequence::as_slice)
}

/// A boolean, accepting the strings `"true"` and `"false"` as Bruno does.
pub(crate) fn boolean(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(b) => Some(*b),
        Value::String(s) if s == "true" => Some(true),
        Value::String(s) if s == "false" => Some(false),
        _ => None,
    }
}

/// A non-negative integer that fits in `T`.
pub(crate) fn unsigned<T: TryFrom<u64>>(value: Option<&Value>) -> Option<T> {
    value?.as_u64().and_then(|n| T::try_from(n).ok())
}

/// `true` when `disabled: true` is set.
pub(crate) fn is_disabled(item: &Value) -> bool {
    boolean(get(item, "disabled")) == Some(true)
}

/// A string value.
pub(crate) fn string(s: impl Into<String>) -> Value {
    Value::String(s.into())
}

/// A number value.
pub(crate) fn number(n: impl Into<Number>) -> Value {
    Value::Number(n.into())
}

/// Builds a mapping from `(key, value)` pairs, skipping `None` values.
pub(crate) fn mapping<'a>(pairs: impl IntoIterator<Item = (&'a str, Option<Value>)>) -> Value {
    let mut map = Mapping::new();
    for (key, value) in pairs {
        if let Some(value) = value {
            map.insert(string(key), value);
        }
    }
    Value::Mapping(map)
}

/// Sets `value[key]`, turning `value` into a mapping if it is not one.
pub(crate) fn set(value: &mut Value, key: &str, new: Value) {
    if value.as_mapping().is_none() {
        *value = Value::Mapping(Mapping::new());
    }
    if let Some(map) = value.as_mapping_mut() {
        if let Some(slot) = map.get_mut(&string(key)) {
            *slot = new;
        } else {
            map.insert(string(key), new);
        }
    }
}

/// Removes `value[key]`, if present.
pub(crate) fn remove(value: &mut Value, key: &str) {
    if let Some(map) = value.as_mapping_mut() {
        map.remove(&string(key));
    }
}

/// Sets `value[key]` to `new`, or removes the key when `new` is `None`.
pub(crate) fn set_or_remove(value: &mut Value, key: &str, new: Option<Value>) {
    match new {
        Some(new) => set(value, key, new),
        None => remove(value, key),
    }
}

/// The mapping at `value[key]`, created if missing (or not a mapping).
pub(crate) fn child_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    let needs_map = get(value, key).is_none_or(|v| v.as_mapping().is_none());
    if needs_map {
        set(value, key, Value::Mapping(Mapping::new()));
    }
    let Some(map) = value.as_mapping_mut() else {
        unreachable!("`set` turned the value into a mapping")
    };
    let Some(child) = map.get_mut(&string(key)) else {
        unreachable!("`set` inserted the key")
    };
    child
}

/// Removes `value[key]` when it is an empty mapping.
pub(crate) fn prune_empty(value: &mut Value, key: &str) {
    if get(value, key)
        .and_then(Value::as_mapping)
        .is_some_and(Mapping::is_empty)
    {
        remove(value, key);
    }
}
