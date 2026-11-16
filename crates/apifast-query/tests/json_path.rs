//! JSONPath, with the examples of RFC 9535.

use apifast_domain::Json;
use apifast_query::{JsonPath, QueryError};

const STORE: &str = r#"{ "store": {
    "book": [
      { "category": "reference", "author": "Nigel Rees",
        "title": "Sayings of the Century", "price": 8.95 },
      { "category": "fiction", "author": "Evelyn Waugh",
        "title": "Sword of Honour", "price": 12.99 },
      { "category": "fiction", "author": "Herman Melville",
        "title": "Moby Dick", "isbn": "0-553-21311-3", "price": 8.99 },
      { "category": "fiction", "author": "J. R. R. Tolkien",
        "title": "The Lord of the Rings", "isbn": "0-395-19395-8",
        "price": 22.99 }
    ],
    "bicycle": { "color": "red", "price": 399 }
  } }"#;

fn select(path: &str, body: &str) -> Vec<String> {
    let json = Json::parse(body).unwrap_or_else(|e| panic!("{e}"));
    JsonPath::parse(path)
        .unwrap_or_else(|e| panic!("{e}"))
        .select(&json)
        .unwrap_or_else(|e| panic!("{e}"))
        .into_iter()
        .map(Json::to_text)
        .collect()
}

fn store(path: &str) -> Vec<String> {
    select(path, STORE)
}

#[test]
fn the_rfc_bookstore_examples() {
    let authors = [
        "Nigel Rees",
        "Evelyn Waugh",
        "Herman Melville",
        "J. R. R. Tolkien",
    ];
    assert_eq!(store("$.store.book[*].author"), authors);
    assert_eq!(store("$..author"), authors);
    assert_eq!(store("$.store.*").len(), 2);
    assert_eq!(
        store("$.store..price"),
        ["8.95", "12.99", "8.99", "22.99", "399"]
    );
    assert_eq!(store("$..book[2].author"), ["Herman Melville"]);
    assert_eq!(store("$..book[-1].title"), ["The Lord of the Rings"]);
    assert_eq!(
        store("$..book[0,1].title"),
        ["Sayings of the Century", "Sword of Honour"]
    );
    assert_eq!(
        store("$..book[:2].title"),
        ["Sayings of the Century", "Sword of Honour"]
    );
    assert_eq!(
        store("$..book[?@.isbn].title"),
        ["Moby Dick", "The Lord of the Rings"]
    );
    assert_eq!(
        store("$..book[?@.price<10].title"),
        ["Sayings of the Century", "Moby Dick"]
    );
    assert_eq!(store("$..*").len(), 27);
}

#[test]
fn bracket_names_quotes_and_escapes() {
    let body = r#"{"a.b": 1, "it's": 2, "é": 3, "x": {"y z": 4}}"#;
    assert_eq!(select("$['a.b']", body), ["1"]);
    assert_eq!(select(r#"$["it's"]"#, body), ["2"]);
    assert_eq!(select(r"$['it\'s']", body), ["2"]);
    assert_eq!(select(r"$['é']", body), ["3"]);
    assert_eq!(select("$.é", body), ["3"]);
    assert_eq!(select("$.x['y z']", body), ["4"]);
    assert_eq!(select("$ .x [ 'y z' ]", body), ["4"]);
    assert!(JsonPath::parse("$. x").is_err());
    assert_eq!(select("$.missing", body), Vec::<String>::new());
}

#[test]
fn slices_and_negative_steps() {
    let body = "[0,1,2,3,4,5,6]";
    assert_eq!(select("$[1:3]", body), ["1", "2"]);
    assert_eq!(select("$[5:]", body), ["5", "6"]);
    assert_eq!(select("$[1:5:2]", body), ["1", "3"]);
    assert_eq!(select("$[5:1:-2]", body), ["5", "3"]);
    assert_eq!(select("$[::-1]", body), ["6", "5", "4", "3", "2", "1", "0"]);
    assert_eq!(select("$[-2:]", body), ["5", "6"]);
}

#[test]
fn filters_with_logic_and_functions() {
    let body = r#"{"items": [
        {"id": 1, "name": "apple", "tags": ["a", "b"], "price": 3},
        {"id": 2, "name": "banana", "tags": [], "price": 1.0},
        {"id": 3, "name": "cherry", "tags": ["c"], "price": 1, "sold": true}
    ]}"#;
    let ids = |path: &str| select(path, body);
    assert_eq!(ids("$.items[?@.price == 1].id"), ["2", "3"]);
    assert_eq!(ids("$.items[?(@.price == 1 && !@.sold)].id"), ["2"]);
    assert_eq!(ids("$.items[?@.id == 1 || @.sold == true].id"), ["1", "3"]);
    assert_eq!(ids("$.items[?length(@.tags) > 0].id"), ["1", "3"]);
    assert_eq!(ids("$.items[?count(@.tags[*]) == 1].id"), ["3"]);
    assert_eq!(ids("$.items[?match(@.name, 'b.*')].id"), ["2"]);
    assert_eq!(ids("$.items[?search(@.name, 'err')].id"), ["3"]);
    assert_eq!(
        ids("$.items[?match(@.name, 'an')].id"),
        Vec::<String>::new()
    );
    assert_eq!(ids("$.items[?value(@.tags[0]) == 'c'].id"), ["3"]);
    assert_eq!(ids("$.items[?@.name > 'b'].id"), ["2", "3"]);
    assert_eq!(ids("$.items[?@.price >= $.items[0].price].id"), ["1"]);
    // Missing compares equal to missing, and never less than anything.
    assert_eq!(ids("$.items[?@.nope == @.none].id"), ["1", "2", "3"]);
    assert_eq!(ids("$.items[?@.nope < 5].id"), Vec::<String>::new());
    // Objects are filtered by their values.
    assert_eq!(select("$[?@ > 1]", r#"{"a": 1, "b": 2}"#), ["2"]);
}

#[test]
fn errors_say_where() {
    let error = |path: &str| JsonPath::parse(path).expect_err(path).to_string();
    assert_eq!(
        error("a.b"),
        "invalid JSONPath `a.b`: expected `$` at character 1"
    );
    assert_eq!(
        error("$.a["),
        "invalid JSONPath `$.a[`: expected a selector at character 5"
    );
    assert_eq!(
        error("$[?@.a == ]"),
        "invalid JSONPath `$[?@.a == ]`: expected a value, `@` or `$` at character 11"
    );
    assert!(error("$[?nope(@)]").contains("unknown function `nope`"));
    assert!(error("$[?length(@, 1)]").contains("takes 1 argument"));
    assert!(error("$[?1]").contains("a literal alone is not a test"));
    assert!(error("$.a b").contains("unexpected text"));
    let deep = format!("$[?{}@{}]", "(".repeat(100), ")".repeat(100));
    assert!(error(&deep).contains("nested too deep"));
}

#[test]
fn a_bad_regex_is_reported() {
    let json = Json::parse(r#"[{"a": "x"}]"#).expect("test JSON");
    let path = JsonPath::parse("$[?match(@.a, '(')]").expect("parses");
    assert_eq!(path.select(&json), Err(QueryError::Regex("(".into())));
}

#[test]
fn deep_descendants_stop_at_the_budget() {
    // 1000 levels of nesting would pass the Json depth limit, so a wide
    // array visited many times stands in for an expensive query.
    let wide = format!("[{}]", vec!["[[[[[[[[[[0]]]]]]]]]]"; 20_000].join(","));
    let json = Json::parse(&wide).expect("test JSON");
    let path = JsonPath::parse("$..*..*..*..*").expect("parses");
    assert_eq!(path.select(&json), Err(QueryError::TooExpensive));
}
