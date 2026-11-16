//! XPath 1.0.

use apifast_domain::Json;
use apifast_query::{QueryError, XPath};

const FEED: &str = r#"<?xml version="1.0"?>
<!DOCTYPE catalog>
<catalog xmlns:x="urn:extra">
  <!-- a comment -->
  <book id="b1" lang="en"><title>Dune</title><price>9.5</price></book>
  <book id="b2"><title>Emma</title><price>4</price><x:note>old</x:note></book>
  <book id="b3" lang="es"><title>Niebla</title><price>12</price></book>
  <?render fast?>
</catalog>"#;

fn texts(path: &str, xml: &str) -> Vec<Json> {
    XPath::parse(path)
        .unwrap_or_else(|e| panic!("{e}"))
        .evaluate(xml)
        .unwrap_or_else(|e| panic!("{e}"))
}

fn feed(path: &str) -> Vec<String> {
    texts(path, FEED).into_iter().map(|j| j.to_text()).collect()
}

#[test]
fn location_paths() {
    assert_eq!(feed("/catalog/book/title"), ["Dune", "Emma", "Niebla"]);
    assert_eq!(feed("//title"), ["Dune", "Emma", "Niebla"]);
    assert_eq!(feed("catalog/book[2]/title"), ["Emma"]);
    assert_eq!(feed("//book[last()]/@id"), ["b3"]);
    assert_eq!(feed("//book/@lang"), ["en", "es"]);
    assert_eq!(feed("//book[@lang='es']/title/text()"), ["Niebla"]);
    assert_eq!(feed("//book[price > 5][2]/title"), ["Niebla"]);
    assert_eq!(feed("//title[. = 'Emma']/../@id"), ["b2"]);
    assert_eq!(
        feed("//book[title='Dune']/following-sibling::book/@id"),
        ["b2", "b3"]
    );
    assert_eq!(feed("//book[3]/preceding-sibling::book[1]/@id"), ["b2"]);
    assert_eq!(feed("//price/ancestor::book[1]/@id"), ["b1", "b2", "b3"]);
    assert_eq!(feed("//book[@id='b2']/following::title"), ["Niebla"]);
    assert_eq!(feed("//book[@id='b2']/preceding::title"), ["Dune"]);
    assert_eq!(feed("//book/@*[1]"), ["b1", "b2", "b3"]);
    assert_eq!(feed("(//title)[1] | (//title)[3]"), ["Dune", "Niebla"]);
    assert_eq!(feed("//comment()"), [" a comment "]);
    assert_eq!(feed("//processing-instruction('render')"), ["fast"]);
    assert_eq!(feed("/catalog/book[1]"), ["Dune9.5"]);
    assert_eq!(feed("/nope"), Vec::<String>::new());
}

#[test]
fn namespaces_match_by_local_name_unless_prefixed() {
    assert_eq!(feed("//note"), ["old"]);
    assert_eq!(feed("//x:note"), ["old"]);
    assert_eq!(feed("//y:note"), Vec::<String>::new());
    assert_eq!(feed("name(//note)"), ["x:note"]);
    assert_eq!(feed("local-name(//x:*)"), ["note"]);
    assert_eq!(feed("namespace-uri(//note)"), ["urn:extra"]);
    let soap = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
        <s:Body><Token xmlns="urn:auth">abc</Token></s:Body></s:Envelope>"#;
    assert_eq!(texts("//Body/Token", soap), [Json::String("abc".into())]);
}

#[test]
fn values_that_are_not_node_sets() {
    assert_eq!(texts("count(//book)", FEED), [Json::Number("3".into())]);
    assert_eq!(texts("sum(//price)", FEED), [Json::Number("25.5".into())]);
    assert_eq!(
        texts("sum(//price) div 2", FEED),
        [Json::Number("12.75".into())]
    );
    assert_eq!(texts("7 mod 3 - -1", FEED), [Json::Number("2".into())]);
    assert_eq!(texts("1 div 0", FEED), [Json::String("Infinity".into())]);
    assert_eq!(texts("//book/@id = 'b2'", FEED), [Json::Bool(true)]);
    assert_eq!(texts("//price > 10", FEED), [Json::Bool(true)]);
    assert_eq!(texts("not(//price > 100)", FEED), [Json::Bool(true)]);
    assert_eq!(texts("//nope = false()", FEED), [Json::Bool(true)]);
    assert_eq!(
        texts(
            "concat(substring-before('a-b', '-'), translate('xyz', 'y', 'Y'), normalize-space('  p  q '))",
            FEED
        ),
        [Json::String("axYzp q".into())]
    );
    assert_eq!(
        texts("string-length(//title[1])", FEED),
        [Json::Number("4".into())]
    );
    assert_eq!(
        texts(
            "starts-with(//title, 'Du') and contains(//title, 'un')",
            FEED
        ),
        [Json::Bool(true)]
    );
    assert_eq!(
        texts("round(2.5) + floor(-1.5) + ceiling(0.2)", FEED),
        [Json::Number("2".into())]
    );
    assert_eq!(
        texts("//book[position() mod 2 = 1]/@id", FEED),
        [Json::String("b1".into()), Json::String("b3".into())]
    );
}

#[test]
fn errors() {
    let error = |path: &str| XPath::parse(path).expect_err(path).to_string();
    assert!(error("//book[").contains("expected a step"));
    assert!(error("nope(1)").contains("unknown function `nope`"));
    assert!(error("count()").contains("wrong number of arguments"));
    assert!(error("$x").contains("variables are not supported"));
    assert!(error("bogus::a").contains("unknown axis"));
    assert!(error("//a ]").contains("unexpected text"));
    let deep = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert!(error(&deep).contains("nested too deep"));
    let path = XPath::parse("//a").expect("parses");
    assert!(matches!(
        path.evaluate("{\"a\": 1}"),
        Err(QueryError::NotXml(_))
    ));
}

#[test]
fn expensive_queries_stop() {
    let xml = format!("<r>{}</r>", "<a><b/></a>".repeat(5_000));
    let path = XPath::parse("//*//*//*//*").expect("parses");
    assert!(path.evaluate(&xml).is_ok_and(|r| r.is_empty()));
    let path = XPath::parse("//*/following::*/following::*").expect("parses");
    assert_eq!(path.evaluate(&xml), Err(QueryError::TooExpensive));
}
