use super::*;

const NOW: i64 = 1_700_000_000_000;

fn at<'a>(url_host: &str, path: &'a str) -> CookieTarget<'a> {
    CookieTarget::new("https", url_host, path)
}

fn parse(header: &str, target: &CookieTarget<'_>) -> Option<Cookie> {
    parse_set_cookie(header, target, NOW)
}

fn jar_with(headers: &[&str], target: &CookieTarget<'_>) -> CookieJar {
    let mut jar = CookieJar::new();
    for header in headers {
        jar.receive(header, target, NOW);
    }
    jar
}

#[test]
fn a_plain_cookie_is_host_only_with_the_default_path() {
    let cookie = parse(" sid = abc=1 ", &at("API.test", "/v1/users/1")).unwrap();
    assert_eq!(cookie.name, "sid");
    assert_eq!(cookie.value, "abc=1");
    assert_eq!(cookie.domain, "api.test");
    assert!(cookie.host_only);
    assert_eq!(cookie.path, "/v1/users");
    assert_eq!(cookie.expires_ms, None);
    assert_eq!(cookie.created_ms, NOW);
    assert_eq!(parse("a=1", &at("h.test", "/x")).unwrap().path, "/");
    assert_eq!(parse("a=1", &at("h.test", "")).unwrap().path, "/");
}

#[test]
fn attributes_are_read_in_any_case_and_the_last_wins() {
    let cookie = parse(
        "a=1; Path=/one; path=/two; SECURE; HttpOnly; SameSite=lax; Domain=.Example.test",
        &at("api.example.test", "/"),
    )
    .unwrap();
    assert_eq!(cookie.path, "/two");
    assert!(cookie.secure && cookie.http_only);
    assert_eq!(cookie.same_site, Some(SameSite::Lax));
    assert_eq!(cookie.domain, "example.test");
    assert!(!cookie.host_only);
    let relative = parse("a=1; Path=rel", &at("h.test", "/a/b")).unwrap();
    assert_eq!(relative.path, "/a");
}

#[test]
fn max_age_wins_over_expires() {
    let target = at("h.test", "/");
    let both = parse(
        "a=1; Max-Age=60; Expires=Wed, 21 Oct 2015 07:28:00 GMT",
        &target,
    );
    assert_eq!(both.unwrap().expires_ms, Some(NOW + 60_000));
    let expires = parse("a=1; Expires=Wed, 21 Oct 2015 07:28:00 GMT", &target);
    assert_eq!(expires.unwrap().expires_ms, Some(1_445_412_480_000));
    let bad = parse("a=1; Max-Age=soon; Expires=never", &target);
    assert_eq!(bad.unwrap().expires_ms, None);
    let gone = parse("a=1; Max-Age=0", &target).unwrap();
    assert!(gone.is_expired(NOW));
    let huge = parse("a=1; Max-Age=99999999999999999999", &target).unwrap();
    assert_eq!(huge.expires_ms, Some(i64::MAX));
}

#[test]
fn values_without_a_name_or_an_equals_sign_are_ignored() {
    let target = at("h.test", "/");
    assert_eq!(parse("novalue", &target), None);
    assert_eq!(parse("=x", &target), None);
    assert_eq!(parse("", &target), None);
    assert_eq!(parse("a=", &target).unwrap().value, "");
}

#[test]
fn a_domain_must_cover_the_host_and_have_a_dot() {
    let target = at("api.example.test", "/");
    assert!(parse("a=1; Domain=example.test", &target).is_some());
    assert!(parse("a=1; Domain=api.example.test", &target).is_some());
    assert_eq!(parse("a=1; Domain=other.test", &target), None);
    assert_eq!(parse("a=1; Domain=xample.test", &target), None);
    assert_eq!(parse("a=1; Domain=test", &target), None);
    let local = parse("a=1; Domain=localhost", &at("localhost", "/")).unwrap();
    assert_eq!(local.domain, "localhost");
    // An empty Domain is as none.
    assert!(parse("a=1; Domain=", &target).unwrap().host_only);
}

#[test]
fn prefixed_names_follow_their_rules() {
    let target = at("h.test", "/a/b");
    assert!(parse("__Secure-a=1; Secure", &target).is_some());
    assert_eq!(parse("__Secure-a=1", &target), None);
    assert!(parse("__Host-a=1; Secure; Path=/", &target).is_some());
    assert_eq!(parse("__Host-a=1; Secure", &target), None, "path /a");
    assert_eq!(
        parse("__Host-a=1; Secure; Path=/; Domain=h.test", &target),
        None
    );
}

#[test]
fn cookies_go_to_their_domain_path_and_scheme() {
    let target = at("api.example.test", "/");
    let jar = jar_with(
        &[
            "host=1",
            "wide=2; Domain=example.test",
            "deep=3; Path=/v1/users",
            "safe=4; Secure",
        ],
        &target,
    );
    let header = |scheme: &str, host: &str, path: &str| {
        jar.header_for(&CookieTarget::new(scheme, host, path), NOW)
    };
    assert_eq!(
        header("https", "api.example.test", "/v1/users/1").as_deref(),
        // Same path and age: in the jar's order (domain, path, name).
        Some("deep=3; host=1; safe=4; wide=2")
    );
    assert_eq!(
        header("http", "API.example.test", "/").as_deref(),
        Some("host=1; wide=2")
    );
    assert_eq!(
        header("https", "www.example.test", "/v1/users").as_deref(),
        Some("wide=2")
    );
    assert_eq!(
        header("https", "example.test", "/").as_deref(),
        Some("wide=2")
    );
    assert_eq!(header("https", "other.test", "/"), None);
    assert_eq!(
        header("https", "api.example.test", "/v1/usersX").as_deref(),
        Some("host=1; safe=4; wide=2"),
        "a path matches at a / only"
    );
}

#[test]
fn secure_cookies_go_to_the_machine_itself_over_http() {
    for host in ["localhost", "app.localhost", "127.0.0.1", "[::1]"] {
        let target = CookieTarget::new("http", host, "/");
        let jar = jar_with(&["a=1; Secure"], &target);
        assert_eq!(
            jar.header_for(&target, NOW).as_deref(),
            Some("a=1"),
            "{host}"
        );
    }
}

#[test]
fn an_ip_host_gets_no_subdomain_matches() {
    assert!(!domain_matches("1.2.3.4", "2.3.4"));
    assert!(domain_matches("1.2.3.4", "1.2.3.4"));
}

#[test]
fn a_cookie_replaces_its_slot_and_keeps_its_creation_time() {
    let target = at("h.test", "/");
    let mut jar = jar_with(&["a=1", "b=2"], &target);
    assert!(jar.receive("a=3", &target, NOW + 5));
    assert!(!jar.receive("a=3", &target, NOW + 9), "nothing changed");
    assert_eq!(jar.cookies().len(), 2);
    let a = jar.cookies().iter().find(|c| c.name == "a").unwrap();
    assert_eq!((a.value.as_str(), a.created_ms), ("3", NOW));
    // The same name at another path is another cookie.
    jar.receive("a=4; Path=/x", &target, NOW);
    assert_eq!(jar.cookies().len(), 3);
    assert_eq!(
        jar.header_for(&at("h.test", "/x"), NOW).as_deref(),
        Some("a=4; a=3; b=2")
    );
}

#[test]
fn an_expired_cookie_removes_its_slot() {
    let target = at("h.test", "/");
    let mut jar = jar_with(&["a=1", "b=2; Max-Age=10"], &target);
    assert!(jar.receive("a=gone; Max-Age=0", &target, NOW));
    assert!(!jar.receive("c=never; Max-Age=-1", &target, NOW));
    assert_eq!(jar.header_for(&target, NOW).as_deref(), Some("b=2"));
    assert_eq!(jar.header_for(&target, NOW + 10_000), None, "b expired");
    assert!(jar.remove_expired(NOW + 10_000));
    assert!(jar.is_empty());
}

#[test]
fn the_jar_is_kept_in_order_and_edited() {
    let mut jar = CookieJar::with_cookies(
        [
            parse("z=1", &at("b.test", "/")).unwrap(),
            parse("y=1; Max-Age=0", &at("a.test", "/")).unwrap(),
            parse("x=1; Path=/p", &at("a.test", "/")).unwrap(),
            parse("w=1", &at("a.test", "/")).unwrap(),
        ],
        NOW,
    );
    let names: Vec<&str> = jar.cookies().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["w", "x", "z"]);
    assert!(jar.remove("a.test", "/p", "x"));
    assert!(!jar.remove("a.test", "/", "x"));
    assert!(jar.remove_domain("a.test"));
    assert_eq!(jar.cookies().len(), 1);
    jar.clear();
    assert!(jar.is_empty());
}

#[test]
fn the_request_header_is_merged_with_the_jar_as_in_bruno() {
    assert_eq!(
        merge_cookie_header("a=mine; b=2;", "a=jar; c=3"),
        "a=jar; b=2; c=3"
    );
    assert_eq!(merge_cookie_header("", "c=3"), "c=3");
    assert_eq!(merge_cookie_header(" flag ;x=1", "y=2"), "flag=; x=1; y=2");
}

#[test]
fn a_target_is_read_from_a_url_and_a_script_cookie_takes_what_it_leaves_out() {
    let target = CookieTarget::from_url("https://u:p@API.test:8443/v1/users?x=1#f").unwrap();
    assert_eq!(
        (target.host(), target.path(), target.secure),
        ("api.test", "/v1/users", true)
    );
    let bare = CookieTarget::from_url("http://[::1]:80").unwrap();
    assert_eq!((bare.host(), bare.path()), ("::1", ""));
    assert!(CookieTarget::from_url("api.test/x").is_none());

    let spec = |domain: Option<&str>, path: Option<&str>| CookieSpec {
        name: "sid".into(),
        value: "1".into(),
        domain: domain.map(str::to_owned),
        path: path.map(str::to_owned),
        ..CookieSpec::default()
    };
    let own = cookie_from_spec(spec(None, None), &target, 5).unwrap();
    assert_eq!(
        (own.domain.as_str(), own.host_only, own.path.as_str()),
        ("api.test", true, "/v1")
    );
    let parent = cookie_from_spec(spec(Some(".Test"), Some("/")), &target, 5).unwrap();
    assert_eq!(
        (
            parent.domain.as_str(),
            parent.host_only,
            parent.path.as_str()
        ),
        ("test", false, "/")
    );
    assert!(cookie_from_spec(spec(Some("other.test"), None), &target, 5).is_none());

    let mut jar = CookieJar::new();
    jar.set(own, 5);
    jar.set(parent, 6);
    let paths: Vec<_> = (jar.going_to(&target, 7).iter())
        .map(|c| c.path.as_str())
        .collect();
    assert_eq!(paths, ["/v1", "/"]);
}
