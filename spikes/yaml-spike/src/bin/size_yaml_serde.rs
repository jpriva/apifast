include!("size_common.inc");
fn main() { let d: Result<Doc, _> = yaml_serde::from_str(&input()); if let Ok(d) = d { println!("{}", yaml_serde::to_string(&d).unwrap_or_default()); } }
