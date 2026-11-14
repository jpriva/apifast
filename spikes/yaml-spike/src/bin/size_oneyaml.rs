include!("size_common.inc");
fn main() {
    let s = input();
    let d: Result<Doc, _> = oneyaml::from_str(&s); if let Ok(d) = d { println!("{}", oneyaml::to_string(&d).unwrap_or_default()); }
    if let Ok(mut doc) = oneyaml::Document::parse(s.as_str()) { let _ = doc.set_path("info.name", "x"); println!("{doc}"); }
}
