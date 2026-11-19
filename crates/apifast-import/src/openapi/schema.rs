//! The parts of an OpenAPI document that refer to others: local `$ref`s
//! followed, and an example made from a schema when the spec gives none.

use serde_json::{Map, Value, json};

/// How deep a generated example goes: past it, an object is empty.
const MAX_DEPTH: usize = 8;
/// How many `$ref`s one lookup follows, against cycles.
const MAX_REFS: usize = 32;

/// The document, to follow `$ref`s in it.
pub(crate) struct Spec<'a> {
    pub root: &'a Value,
}

impl<'a> Spec<'a> {
    /// `value`, or what its local `$ref` points to (followed again if it
    /// is a `$ref` too). An external or broken `$ref` gives `None`.
    pub fn resolve(&self, value: &'a Value) -> Option<&'a Value> {
        let mut current = value;
        for _ in 0..MAX_REFS {
            let Some(reference) = current.get("$ref").and_then(Value::as_str) else {
                return Some(current);
            };
            let pointer = reference.strip_prefix('#')?;
            current = self.root.pointer(pointer)?;
        }
        None
    }

    /// An example of `schema`: its own `example` or `default`, else one
    /// built from its type and properties.
    pub fn example(&self, schema: &'a Value) -> Value {
        self.example_at(schema, 0)
    }

    fn example_at(&self, schema: &'a Value, depth: usize) -> Value {
        let Some(schema) = self.resolve(schema) else {
            return Value::Null;
        };
        for key in ["example", "default"] {
            if let Some(value) = schema.get(key) {
                return value.clone();
            }
        }
        if let Some(Value::Array(examples)) = schema.get("examples")
            && let Some(first) = examples.first()
        {
            return first.clone();
        }
        if let Some(Value::Array(values)) = schema.get("enum")
            && let Some(first) = values.first()
        {
            return first.clone();
        }
        if depth > MAX_DEPTH {
            return Value::Null;
        }
        if let Some(Value::Array(parts)) = schema.get("allOf") {
            let mut merged = Map::new();
            for part in parts {
                if let Value::Object(object) = self.example_at(part, depth + 1) {
                    merged.extend(object);
                }
            }
            return Value::Object(merged);
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(Value::Array(options)) = schema.get(key)
                && let Some(first) = options.first()
            {
                return self.example_at(first, depth + 1);
            }
        }
        match kind(schema) {
            "object" => {
                let mut object = Map::new();
                if let Some(Value::Object(properties)) = schema.get("properties") {
                    for (name, property) in properties {
                        object.insert(name.clone(), self.example_at(property, depth + 1));
                    }
                }
                Value::Object(object)
            }
            "array" => match schema.get("items") {
                Some(items) => Value::Array(vec![self.example_at(items, depth + 1)]),
                None => json!([]),
            },
            "integer" => json!(0),
            "number" => json!(0.0),
            "boolean" => json!(false),
            "string" => json!(string_example(schema)),
            _ => Value::Null,
        }
    }
}

/// The schema's type; `object` when it has properties, as OpenAPI allows
/// it unsaid.
fn kind(schema: &Value) -> &str {
    match schema.get("type") {
        Some(Value::String(kind)) => kind,
        // OpenAPI 3.1 allows a list, such as `[string, "null"]`.
        Some(Value::Array(kinds)) => kinds
            .iter()
            .filter_map(Value::as_str)
            .find(|k| *k != "null")
            .unwrap_or("null"),
        _ if schema.get("properties").is_some() => "object",
        _ => "",
    }
}

/// A string that fits the schema's format.
fn string_example(schema: &Value) -> &'static str {
    match schema.get("format").and_then(Value::as_str) {
        Some("date-time") => "2024-01-01T00:00:00Z",
        Some("date") => "2024-01-01",
        Some("email") => "user@example.com",
        Some("uuid") => "00000000-0000-0000-0000-000000000000",
        Some("uri" | "url") => "https://example.com",
        _ => "string",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_are_followed_and_examples_built() {
        let root = json!({
            "components": {"schemas": {
                "Pet": {"type": "object", "required": ["name"], "properties": {
                    "id": {"type": "integer", "format": "int64"},
                    "name": {"type": "string", "example": "Rex"},
                    "tags": {"type": "array", "items": {"$ref": "#/components/schemas/Tag"}},
                    "status": {"type": "string", "enum": ["available", "sold"]},
                    "born": {"type": "string", "format": "date"}
                }},
                "Tag": {"properties": {"label": {"type": "string"}}},
                "Loop": {"type": "object", "properties": {"next": {"$ref": "#/components/schemas/Loop"}}},
                "Both": {"allOf": [{"$ref": "#/components/schemas/Tag"}, {"properties": {"x": {"type": "boolean"}}}]},
                "Either": {"oneOf": [{"type": "number"}, {"type": "string"}]},
                "Self": {"$ref": "#/components/schemas/Self"}
            }}
        });
        let spec = Spec { root: &root };
        let pet = json!({"$ref": "#/components/schemas/Pet"});
        assert_eq!(
            spec.example(&pet),
            json!({"id": 0, "name": "Rex", "tags": [{"label": "string"}], "status": "available", "born": "2024-01-01"})
        );
        assert_eq!(
            spec.example(&json!({"$ref": "#/components/schemas/Both"})),
            json!({"label": "string", "x": false})
        );
        assert_eq!(
            spec.example(&json!({"$ref": "#/components/schemas/Either"})),
            json!(0.0)
        );
        assert_eq!(
            spec.example(&json!({"type": ["string", "null"]})),
            json!("string")
        );
        // A cycle ends, and a `$ref` to itself or outside resolves to nothing.
        let looped = spec.example(&json!({"$ref": "#/components/schemas/Loop"}));
        assert!(looped.pointer("/next/next/next").is_some());
        assert_eq!(
            spec.resolve(&json!({"$ref": "#/components/schemas/Self"})),
            None
        );
        assert_eq!(spec.resolve(&json!({"$ref": "other.yaml#/Pet"})), None);
        assert_eq!(spec.resolve(&json!({"$ref": "#/nope"})), None);
    }
}
