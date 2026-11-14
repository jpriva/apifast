include!("size_common.inc");
fn main() { let d: Result<Doc, _> = serde_json::from_str(&input()); if let Ok(d) = d { println!("{}", serde_json::to_string(&d).unwrap_or_default()); } }
