//! Any expression, against any body, gives a result or an error.

use apifast_domain::Json;
use apifast_query::select;
use proptest::prelude::*;

const BODY_JSON: &str = r#"{"a": [1, {"b": "x", "c": [true, null]}], "d": {"e": 2.5}}"#;
const BODY_XML: &str = r#"<a x="1"><b>t</b><c><b>u</b></c></a>"#;

proptest! {
    #[test]
    fn any_expression(expression in r#"[$@/.\[\]\(\)*?:,'"=!<>&|@a-z0-9 \-]{0,40}"#) {
        let json = Json::parse(BODY_JSON).ok();
        let _ = select(&expression, BODY_JSON, json.as_ref());
        let _ = select(&expression, BODY_XML, None);
    }
}
