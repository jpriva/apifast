//! YAML read as JSON, for the importers (`apifast-import`), which work on
//! JSON values: OpenAPI specs come in either. `oneyaml` stays in this crate.

/// The YAML document `text` as a JSON value. YAML is a superset of JSON,
/// so a JSON document reads too.
///
/// # Errors
///
/// If `text` is not YAML, or holds what JSON has not (such as a mapping
/// with non-text keys).
pub fn yaml_to_json(text: &str) -> Result<serde_json::Value, String> {
    let value: oneyaml::Value = oneyaml::from_str(text).map_err(|e| e.to_string())?;
    serde_json::to_value(&value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_and_json_read_alike() {
        let yaml = yaml_to_json("openapi: 3.0.0\ninfo:\n  title: Pets\npaths:\n  /pets: {get: {responses: {'200': {description: ok}}}}\ntags: [a, b]\nn: 3\nf: 1.5\nb: true\nz: ~\n").unwrap();
        assert_eq!(yaml["info"]["title"], "Pets");
        assert_eq!(
            yaml["paths"]["/pets"]["get"]["responses"]["200"]["description"],
            "ok"
        );
        assert_eq!(yaml["tags"][1], "b");
        assert_eq!(
            (yaml["n"].as_i64(), yaml["f"].as_f64()),
            (Some(3), Some(1.5))
        );
        assert_eq!(
            (yaml["b"].as_bool(), yaml["z"].is_null()),
            (Some(true), true)
        );
        let json = yaml_to_json(r#"{"info": {"title": "Pets"}}"#).unwrap();
        assert_eq!(json["info"]["title"], "Pets");
        assert!(yaml_to_json("a: [unclosed").is_err());
    }
}
