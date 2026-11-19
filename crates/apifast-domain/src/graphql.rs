//! A GraphQL schema, as a server describes it to the introspection query
//! (docs/07, F7): its types, their fields and arguments. It feeds the
//! schema explorer and autocompletion.

use std::fmt;

use crate::Json;

/// The standard introspection query, as GraphiQL sends it: every type,
/// with its fields, arguments, input fields, enum values, interfaces and
/// possible types, and type references seven levels deep.
pub const INTROSPECTION_QUERY: &str = "query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types { ...FullType }
  }
}

fragment FullType on __Type {
  kind
  name
  description
  fields(includeDeprecated: true) {
    name
    description
    args { ...InputValue }
    type { ...TypeRef }
    isDeprecated
  }
  inputFields { ...InputValue }
  interfaces { ...TypeRef }
  enumValues(includeDeprecated: true) { name description isDeprecated }
  possibleTypes { ...TypeRef }
}

fragment InputValue on __InputValue {
  name
  description
  type { ...TypeRef }
  defaultValue
}

fragment TypeRef on __Type {
  kind
  name
  ofType { kind name ofType { kind name ofType { kind name ofType { kind name
    ofType { kind name ofType { kind name ofType { kind name } } } } } } }
}
";

/// A GraphQL schema.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphQlSchema {
    /// The type of queries, such as `Query`.
    pub query: Option<String>,
    /// The type of mutations, if any.
    pub mutation: Option<String>,
    /// The type of subscriptions, if any.
    pub subscription: Option<String>,
    /// Every type, built-in ones (`__Type`…) included, in the server's order.
    pub types: Vec<SchemaType>,
}

/// A type of a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaType {
    /// Its name.
    pub name: String,
    /// What it is.
    pub kind: TypeKind,
    /// What it is for, if said.
    pub description: Option<String>,
    /// Fields of an object or an interface.
    pub fields: Vec<SchemaField>,
    /// Fields of an input object.
    pub input_fields: Vec<InputValue>,
    /// Values of an enum.
    pub enum_values: Vec<EnumValue>,
    /// Interfaces an object implements.
    pub interfaces: Vec<String>,
    /// Types of a union, or the implementations of an interface.
    pub possible_types: Vec<String>,
}

/// The kind of a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    /// `Int`, `String`, a custom scalar…
    Scalar,
    /// An object with fields.
    Object,
    /// An interface.
    Interface,
    /// A union.
    Union,
    /// An enum.
    Enum,
    /// An input object, for arguments.
    InputObject,
}

impl TypeKind {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "SCALAR" => Self::Scalar,
            "OBJECT" => Self::Object,
            "INTERFACE" => Self::Interface,
            "UNION" => Self::Union,
            "ENUM" => Self::Enum,
            "INPUT_OBJECT" => Self::InputObject,
            _ => return None,
        })
    }

    /// Its keyword in the schema language: `type`, `input`…
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Object => "type",
            Self::Interface => "interface",
            Self::Union => "union",
            Self::Enum => "enum",
            Self::InputObject => "input",
        }
    }
}

/// A field of an object or an interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaField {
    /// Its name.
    pub name: String,
    /// What it is for, if said.
    pub description: Option<String>,
    /// Its arguments.
    pub args: Vec<InputValue>,
    /// What it returns.
    pub type_ref: TypeRef,
    /// Whether it is deprecated.
    pub deprecated: bool,
}

/// An argument, or a field of an input object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputValue {
    /// Its name.
    pub name: String,
    /// What it is for, if said.
    pub description: Option<String>,
    /// Its type.
    pub type_ref: TypeRef,
    /// Its default, as GraphQL text, if any.
    pub default: Option<String>,
}

/// A value of an enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumValue {
    /// Its name.
    pub name: String,
    /// What it means, if said.
    pub description: Option<String>,
    /// Whether it is deprecated.
    pub deprecated: bool,
}

/// A reference to a type: a named one, a list of one, or one that cannot be
/// null.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRef {
    /// A named type.
    Named(String),
    /// `[T]`.
    List(Box<TypeRef>),
    /// `T!`.
    NonNull(Box<TypeRef>),
}

impl TypeRef {
    /// The named type inside it: `User` for `[User!]!`.
    pub fn name(&self) -> &str {
        match self {
            Self::Named(name) => name,
            Self::List(inner) | Self::NonNull(inner) => inner.name(),
        }
    }
}

/// As GraphQL writes it: `[User!]!`.
impl fmt::Display for TypeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => f.write_str(name),
            Self::List(inner) => write!(f, "[{inner}]"),
            Self::NonNull(inner) => write!(f, "{inner}!"),
        }
    }
}

/// Why an introspection result is not a schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaError {
    /// The server answered with errors, such as introspection being off.
    #[error("the server refused the introspection: {0}")]
    Refused(String),
    /// The answer has no `__schema`.
    #[error("the answer is not an introspection result")]
    NotIntrospection,
}

impl GraphQlSchema {
    /// The schema in an introspection result: the server's answer
    /// (`{"data": {"__schema": …}}`) or its `data`.
    ///
    /// # Errors
    ///
    /// If the server answered with errors, or there is no `__schema`.
    pub fn from_introspection(answer: &Json) -> Result<Self, SchemaError> {
        let data = answer.get("data").unwrap_or(answer);
        let Some(schema) = data.get("__schema") else {
            let message = (answer.get("errors"))
                .and_then(|errors| match errors {
                    Json::Array(list) => list.first(),
                    _ => None,
                })
                .and_then(|error| error.get("message"))
                .map(Json::to_text);
            return Err(message.map_or(SchemaError::NotIntrospection, SchemaError::Refused));
        };
        let root = |key: &str| {
            (schema.get(key))
                .and_then(|t| t.get("name"))
                .map(Json::to_text)
        };
        Ok(Self {
            query: root("queryType"),
            mutation: root("mutationType"),
            subscription: root("subscriptionType"),
            types: list(schema.get("types"))
                .iter()
                .filter_map(schema_type)
                .collect(),
        })
    }

    /// The type called `name`.
    pub fn type_named(&self, name: &str) -> Option<&SchemaType> {
        self.types.iter().find(|t| t.name == name)
    }

    /// The types a user looks at: not the built-in `__Type`, `__Field`…
    pub fn user_types(&self) -> impl Iterator<Item = &SchemaType> {
        self.types.iter().filter(|t| !t.name.starts_with("__"))
    }
}

fn list(value: Option<&Json>) -> &[Json] {
    match value {
        Some(Json::Array(items)) => items,
        _ => &[],
    }
}

fn text(value: &Json, key: &str) -> Option<String> {
    match value.get(key) {
        Some(Json::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn description(value: &Json) -> Option<String> {
    text(value, "description").filter(|d| !d.trim().is_empty())
}

fn deprecated(value: &Json) -> bool {
    matches!(value.get("isDeprecated"), Some(Json::Bool(true)))
}

fn schema_type(value: &Json) -> Option<SchemaType> {
    Some(SchemaType {
        name: text(value, "name")?,
        kind: TypeKind::parse(&text(value, "kind")?)?,
        description: description(value),
        fields: list(value.get("fields"))
            .iter()
            .filter_map(|field| {
                Some(SchemaField {
                    name: text(field, "name")?,
                    description: description(field),
                    args: list(field.get("args"))
                        .iter()
                        .filter_map(input_value)
                        .collect(),
                    type_ref: type_ref(field.get("type")?)?,
                    deprecated: deprecated(field),
                })
            })
            .collect(),
        input_fields: (list(value.get("inputFields")).iter())
            .filter_map(input_value)
            .collect(),
        enum_values: (list(value.get("enumValues")).iter())
            .filter_map(|v| {
                Some(EnumValue {
                    name: text(v, "name")?,
                    description: description(v),
                    deprecated: deprecated(v),
                })
            })
            .collect(),
        interfaces: names(value.get("interfaces")),
        possible_types: names(value.get("possibleTypes")),
    })
}

fn names(value: Option<&Json>) -> Vec<String> {
    list(value).iter().filter_map(|t| text(t, "name")).collect()
}

fn input_value(value: &Json) -> Option<InputValue> {
    Some(InputValue {
        name: text(value, "name")?,
        description: description(value),
        type_ref: type_ref(value.get("type")?)?,
        default: text(value, "defaultValue"),
    })
}

fn type_ref(value: &Json) -> Option<TypeRef> {
    match text(value, "kind")?.as_str() {
        "NON_NULL" => Some(TypeRef::NonNull(Box::new(type_ref(value.get("ofType")?)?))),
        "LIST" => Some(TypeRef::List(Box::new(type_ref(value.get("ofType")?)?))),
        _ => text(value, "name").map(TypeRef::Named),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"{"data": {"__schema": {
        "queryType": {"name": "Query"}, "mutationType": null, "subscriptionType": null,
        "types": [
            {"kind": "OBJECT", "name": "Query", "description": "", "fields": [
                {"name": "users", "description": "All users.", "isDeprecated": false,
                 "args": [{"name": "first", "description": null, "defaultValue": "10",
                           "type": {"kind": "SCALAR", "name": "Int", "ofType": null}}],
                 "type": {"kind": "NON_NULL", "name": null, "ofType": {"kind": "LIST", "name": null,
                          "ofType": {"kind": "NON_NULL", "name": null, "ofType": {"kind": "OBJECT", "name": "User"}}}}}
            ], "inputFields": null, "interfaces": [], "enumValues": null, "possibleTypes": null},
            {"kind": "OBJECT", "name": "User", "fields": [
                {"name": "id", "args": [], "isDeprecated": false, "type": {"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID"}}},
                {"name": "login", "args": [], "isDeprecated": true, "type": {"kind": "SCALAR", "name": "String"}}
            ], "interfaces": [{"kind": "INTERFACE", "name": "Node"}]},
            {"kind": "ENUM", "name": "Role", "enumValues": [{"name": "ADMIN", "isDeprecated": false}]},
            {"kind": "SCALAR", "name": "Int"},
            {"kind": "OBJECT", "name": "__Type", "fields": []}
        ]}}}"#;

    #[test]
    fn an_introspection_answer_gives_the_schema() {
        let schema = GraphQlSchema::from_introspection(&Json::parse(ANSWER).unwrap()).unwrap();
        assert_eq!(schema.query.as_deref(), Some("Query"));
        assert_eq!(schema.mutation, None);
        let users = &schema.type_named("Query").unwrap().fields[0];
        assert_eq!(users.type_ref.to_string(), "[User!]!");
        assert_eq!(users.type_ref.name(), "User");
        assert_eq!(users.description.as_deref(), Some("All users."));
        assert_eq!(users.args[0].default.as_deref(), Some("10"));
        assert_eq!(
            schema.type_named("Query").unwrap().description,
            None,
            "a blank one is none"
        );
        let user = schema.type_named("User").unwrap();
        assert!(user.fields[1].deprecated);
        assert_eq!(user.interfaces, ["Node"]);
        assert_eq!(
            schema.type_named("Role").unwrap().enum_values[0].name,
            "ADMIN"
        );
        let names: Vec<&str> = schema.user_types().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Query", "User", "Role", "Int"]);
    }

    #[test]
    fn errors_and_other_answers_say_why() {
        let refused = r#"{"errors": [{"message": "introspection is disabled"}]}"#;
        assert_eq!(
            GraphQlSchema::from_introspection(&Json::parse(refused).unwrap()),
            Err(SchemaError::Refused("introspection is disabled".into()))
        );
        assert_eq!(
            GraphQlSchema::from_introspection(&Json::parse("{\"ok\": 1}").unwrap()),
            Err(SchemaError::NotIntrospection)
        );
        assert!(INTROSPECTION_QUERY.contains("__schema"));
    }
}
