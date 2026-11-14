#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Doc { info: Info, #[serde(flatten)] extra: indexmap::IndexMap<String, serde_json::Value> }
#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Info { name: String, seq: Option<i64> }
fn input() -> String { std::fs::read_to_string(std::env::args().nth(1).unwrap_or_default()).unwrap_or_default() }
