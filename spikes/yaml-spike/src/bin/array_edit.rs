//! How much of a list does an edit of one element rewrite?
const SRC: &str = "info:\n  name: Login\n  type: http\nhttp:\n  method: GET\n  url: x\n  headers:\n    # auth header, value comes from the vault\n    - name: authorization\n      value: \"Bearer {{token}}\"\n    - name: accept # json only\n      value: application/json\n";
fn main() {
    // a) merge patch: arrays are replaced wholesale (RFC 7396)
    let mut doc = oneyaml::Document::parse(SRC).unwrap();
    let mp = oneyaml::MergePatch::from_json(r#"{"http":{"headers":[{"name":"authorization","value":"Bearer {{token}}"},{"name":"accept","value":"text/plain"}]}}"#).unwrap();
    doc.apply_merge_patch(&mp).unwrap();
    println!("--- merge patch (whole list) ---\n{doc}");
    // b) element-wise path edit
    let mut doc = oneyaml::Document::parse(SRC).unwrap();
    doc.set_path("http.headers[1].value", "text/plain").unwrap();
    println!("--- set_path on element ---\n{doc}");
    // c) JSON Patch replace on element
    let mut doc = oneyaml::Document::parse(SRC).unwrap();
    doc.apply_patch(&oneyaml::Patch::from_json(r#"[{"op":"replace","path":"/http/headers/1/value","value":"text/plain"},{"op":"add","path":"/http/headers/-","value":{"name":"x-new","value":"1"}}]"#).unwrap()).unwrap();
    println!("--- JSON patch: replace + append ---\n{doc}");
}
