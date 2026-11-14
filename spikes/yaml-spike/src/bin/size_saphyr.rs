include!("size_common.inc");
fn main() { let d: Result<Doc, _> = serde_saphyr::from_str(&input()); if let Ok(d) = d { println!("{}", serde_saphyr::to_string(&d).unwrap_or_default()); } }
