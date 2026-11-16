//! Rendering through the public API, with a fake [`Host`].

use std::borrow::Cow;
use std::collections::HashMap;

use apifast_domain::{Origin, ResolvedVariables, Variable, resolve_variables};
use apifast_template::{FunctionArg, Host, MAX_DEPTH, Mode, Problem, RenderError, render};

#[derive(Default)]
struct FakeHost {
    millis: i64,
    random: u64,
    env: HashMap<String, String>,
    prompts: HashMap<String, String>,
    calls: Vec<(String, Vec<(String, FunctionArg)>)>,
}

impl Host for FakeHost {
    fn unix_millis(&mut self) -> i64 {
        self.millis
    }

    fn random_u64(&mut self) -> u64 {
        self.random
    }

    fn process_env(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }

    fn prompt(&mut self, label: &str) -> Option<String> {
        self.prompts.get(label).cloned()
    }

    fn call(&mut self, name: &str, args: &[(String, FunctionArg)]) -> Result<String, String> {
        self.calls.push((name.to_owned(), args.to_vec()));
        match name {
            "upper" => match args {
                [(_, FunctionArg::Text(text))] => Ok(text.to_uppercase()),
                _ => Err("expected one text argument".into()),
            },
            _ => Err(format!("unknown function `{name}`")),
        }
    }
}

fn vars(pairs: &[(&str, &str)]) -> ResolvedVariables {
    let list: Vec<Variable> = pairs.iter().map(|(n, v)| Variable::text(*n, *v)).collect();
    resolve_variables([(Origin::Request, list.as_slice())])
}

fn strict(input: &str, variables: &ResolvedVariables) -> Result<String, RenderError> {
    render(input, variables, &mut FakeHost::default(), Mode::Strict).map(|r| r.text.into_owned())
}

#[test]
fn renders_the_documentation_example() {
    let collection = [Variable::text("api_version", "v1")];
    let env = [Variable::text("host", "https://api.ejemplo.com")];
    let folder = [Variable::text("resource", "users")];
    let runtime = [Variable::text("user_id", "981")];
    let variables = resolve_variables([
        (Origin::Collection, &collection[..]),
        (Origin::Environment("produccion".into()), &env[..]),
        (Origin::Folder(0), &folder[..]),
        (Origin::Runtime, &runtime[..]),
    ]);

    assert_eq!(
        strict(
            "{{host}}/{{api_version}}/{{resource}}/{{user_id}}",
            &variables
        )
        .unwrap(),
        "https://api.ejemplo.com/v1/users/981"
    );
}

#[test]
fn text_without_tags_is_borrowed() {
    let rendered = render(
        "plain text",
        &vars(&[]),
        &mut FakeHost::default(),
        Mode::Strict,
    )
    .unwrap();
    assert!(matches!(rendered.text, Cow::Borrowed("plain text")));
}

#[test]
fn variable_values_are_rendered_recursively() {
    let variables = vars(&[
        ("base", "{{scheme}}://{{host}}"),
        ("scheme", "https"),
        ("host", "example.com"),
    ]);
    assert_eq!(
        strict("{{base}}/x", &variables).unwrap(),
        "https://example.com/x"
    );
}

#[test]
fn reports_cycles_between_variables() {
    let variables = vars(&[("a", "{{b}}"), ("b", "x{{a}}")]);
    assert_eq!(
        strict("{{a}}", &variables),
        Err(RenderError(Problem::Cycle(vec![
            "a".into(),
            "b".into(),
            "a".into()
        ])))
    );
}

#[test]
fn stops_at_the_maximum_depth() {
    let names: Vec<String> = (0..=MAX_DEPTH + 1).map(|i| format!("v{i}")).collect();
    let list: Vec<Variable> = names
        .windows(2)
        .map(|w| Variable::text(&w[0], format!("{{{{{}}}}}", w[1])))
        .collect();
    let variables = resolve_variables([(Origin::Request, list.as_slice())]);

    assert_eq!(
        strict("{{v0}}", &variables),
        Err(RenderError(Problem::TooDeep(format!("v{MAX_DEPTH}"))))
    );
}

#[test]
fn strict_mode_fails_on_a_missing_variable() {
    assert_eq!(
        strict("{{host}}/users", &vars(&[])),
        Err(RenderError(Problem::MissingVariable("host".into())))
    );
}

#[test]
fn lenient_mode_leaves_missing_values_empty_and_lists_them() {
    let rendered = render(
        "{{host}}/{{$nope}}/{{process.env.X}}/{{?Code}}/{{ upper(a=1, b=2) }}/ok",
        &vars(&[]),
        &mut FakeHost::default(),
        Mode::Lenient,
    )
    .unwrap();

    assert_eq!(rendered.text, "/////ok");
    assert_eq!(
        rendered.problems,
        [
            Problem::MissingVariable("host".into()),
            Problem::UnknownDynamic("nope".into()),
            Problem::MissingProcessEnv("X".into()),
            Problem::MissingPrompt("Code".into()),
            Problem::MissingVariable("1".into()),
            Problem::MissingVariable("2".into()),
            Problem::Function {
                name: "upper".into(),
                message: "expected one text argument".into()
            },
        ]
    );
}

#[test]
fn secrets_mark_the_result_and_are_never_in_problems() {
    let env = [
        Variable {
            value: Some(apifast_domain::VariableValue::text("s3cr3t")),
            ..Variable::secret("token")
        },
        Variable::secret("locked"),
    ];
    let variables = resolve_variables([(Origin::Environment("prod".into()), &env[..])]);

    let rendered = render(
        "Bearer {{token}}",
        &variables,
        &mut FakeHost::default(),
        Mode::Strict,
    )
    .unwrap();
    assert_eq!(rendered.text, "Bearer s3cr3t");
    assert!(rendered.contains_secret);
    assert_eq!(rendered.secrets, [apifast_domain::Secret::new("s3cr3t")]);

    let plain = render(
        "{{x}}",
        &vars(&[("x", "1")]),
        &mut FakeHost::default(),
        Mode::Strict,
    )
    .unwrap();
    assert!(!plain.contains_secret);
    assert_eq!(plain.secrets, []);

    assert_eq!(
        strict("{{locked}}", &variables),
        Err(RenderError(Problem::SecretUnavailable("locked".into())))
    );
}

#[test]
fn escapes_and_malformed_tags_are_literal() {
    assert_eq!(
        strict("\\{{host}} {{ }} {{a b}} {{open", &vars(&[])).unwrap(),
        "{{host}} {{ }} {{a b}} {{open"
    );
}

#[test]
fn literals_render_as_text() {
    assert_eq!(
        strict("{{'a b'}}|{{true}}|{{false}}|{{null}}", &vars(&[])).unwrap(),
        "a b|true|false|"
    );
}

#[test]
fn dynamic_variables_use_the_host_clock_and_randomness() {
    let mut host = FakeHost {
        millis: 1_700_000_000_123,
        random: 1234,
        ..FakeHost::default()
    };
    let rendered = render(
        "{{$timestamp}} {{$isoTimestamp}} {{$randomInt}} {{$randomUUID}}",
        &vars(&[]),
        &mut host,
        Mode::Strict,
    )
    .unwrap();
    assert_eq!(
        rendered.text,
        "1700000000 2023-11-14T22:13:20.123Z 233 00000000-0000-44d2-8000-0000000004d2"
    );
}

#[test]
fn process_env_and_prompts_come_from_the_host() {
    let mut host = FakeHost::default();
    host.env.insert("API_KEY".into(), "k".into());
    host.prompts.insert("OTP code".into(), "123456".into());
    let rendered = render(
        "{{process.env.API_KEY}}:{{?OTP code}}",
        &vars(&[]),
        &mut host,
        Mode::Strict,
    )
    .unwrap();
    assert_eq!(rendered.text, "k:123456");
}

#[test]
fn functions_receive_rendered_named_arguments() {
    let mut host = FakeHost::default();
    let rendered = render(
        "{{ upper(value=name) }}",
        &vars(&[("name", "{{first}} doe"), ("first", "jane")]),
        &mut host,
        Mode::Strict,
    )
    .unwrap();
    assert_eq!(rendered.text, "JANE DOE");

    render(
        "{{ other(a='x', b=true, c=null) }}",
        &vars(&[]),
        &mut host,
        Mode::Lenient,
    )
    .unwrap();
    assert_eq!(
        host.calls[1],
        (
            "other".into(),
            vec![
                ("a".into(), FunctionArg::Text("x".into())),
                ("b".into(), FunctionArg::Bool(true)),
                ("c".into(), FunctionArg::Null),
            ]
        )
    );
}

#[test]
fn object_variables_are_inserted_as_json() {
    let list = [Variable {
        value: Some(apifast_domain::VariableValue {
            kind: apifast_domain::ValueKind::Object,
            data: r#"{"beta": true}"#.into(),
        }),
        ..Variable::text("flags", "")
    }];
    let variables = resolve_variables([(Origin::Request, &list[..])]);
    assert_eq!(
        strict("{{flags}}", &variables).unwrap(),
        r#"{"beta": true}"#
    );
}

/// A secret whose value is a template keeps its rendered value, so that is
/// what gets masked.
#[test]
fn a_secret_made_of_other_variables_is_kept_as_rendered() {
    let env = [
        Variable::text("user", "ana"),
        Variable {
            value: Some(apifast_domain::VariableValue::text("{{user}}:pa55")),
            ..Variable::secret("credentials")
        },
    ];
    let variables = resolve_variables([(Origin::Environment("prod".into()), &env[..])]);
    let rendered = render(
        "x={{credentials}}&u={{user}}",
        &variables,
        &mut FakeHost::default(),
        Mode::Strict,
    )
    .unwrap();
    assert_eq!(rendered.text, "x=ana:pa55&u=ana");
    assert_eq!(rendered.secrets, [apifast_domain::Secret::new("ana:pa55")]);
}

#[test]
fn paths_reach_inside_object_variables() {
    let user = r#"{"id":5,"address":{"city":"Lima"},"tags":["a","b"],"ref":"{{other}}"}"#;
    let variables = vars(&[
        ("user", user),
        ("other", "x"),
        ("api", r#"{"v":{"x":"short"}}"#),
        ("api.v", r#"{"x":"long"}"#),
        ("host.name", "h"),
        ("text", "not json"),
    ]);
    assert_eq!(strict("{{user.id}}", &variables).unwrap(), "5");
    assert_eq!(
        strict("{{ user.address.city }}", &variables).unwrap(),
        "Lima"
    );
    assert_eq!(
        strict("{{user.address}}", &variables).unwrap(),
        r#"{"city":"Lima"}"#
    );
    assert_eq!(strict("{{user.tags.1}}", &variables).unwrap(), "b");
    // The value is rendered before it is read.
    assert_eq!(strict("{{user.ref}}", &variables).unwrap(), "x");
    // A variable with dots in its name comes first, the longest one next.
    assert_eq!(strict("{{host.name}}", &variables).unwrap(), "h");
    assert_eq!(strict("{{api.v.x}}", &variables).unwrap(), "long");
    for missing in ["user.nope", "user.address.city.x", "text.a", "nobody.id"] {
        assert_eq!(
            strict(&format!("{{{{{missing}}}}}"), &variables),
            Err(RenderError(Problem::MissingVariable(missing.into())))
        );
    }
}

#[test]
fn a_path_inside_a_secret_is_a_secret() {
    let env = [Variable {
        value: Some(apifast_domain::VariableValue::text(r#"{"key":"k3y"}"#)),
        ..Variable::secret("creds")
    }];
    let variables = resolve_variables([(Origin::Environment("prod".into()), &env[..])]);
    let rendered = render(
        "{{creds.key}}",
        &variables,
        &mut FakeHost::default(),
        Mode::Strict,
    )
    .unwrap();
    assert_eq!(rendered.text, "k3y");
    assert!(rendered.contains_secret);
    assert_eq!(rendered.secrets, [apifast_domain::Secret::new("k3y")]);
    let locked = resolve_variables([(
        Origin::Environment("prod".into()),
        &[Variable::secret("creds")][..],
    )]);
    assert_eq!(
        strict("{{creds.key}}", &locked),
        Err(RenderError(Problem::SecretUnavailable("creds".into())))
    );
}

#[test]
fn test_data_variables_have_the_expected_shapes() {
    let mut host = FakeHost {
        random: 1234,
        ..FakeHost::default()
    };
    let mut one = |name: &str| {
        render(
            &format!("{{{{${name}}}}}"),
            &vars(&[]),
            &mut host,
            Mode::Strict,
        )
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .text
        .to_string()
    };
    let email = one("randomEmail");
    assert!(
        email.ends_with("@example.com") && email.contains('.'),
        "{email}"
    );
    let full = one("randomFullName");
    assert_eq!(full.split(' ').count(), 2, "{full}");
    assert_ne!(one("randomFirstName"), "");
    assert_ne!(one("randomLastName"), "");
    assert!(one("randomUserName").contains('.'));
    assert!(one("randomWord").chars().all(|c| c.is_ascii_lowercase()));
    assert_eq!(one("randomWords").split(' ').count(), 3);
    assert!(["true", "false"].contains(&one("randomBoolean").as_str()));
    assert_eq!(one("randomAlphaNumeric").len(), 1);
    let phone = one("randomPhoneNumber");
    assert_eq!(phone.len(), 12, "{phone}");
    assert_eq!(phone.matches('-').count(), 2, "{phone}");
    assert_ne!(one("randomCity"), "");
    assert_ne!(one("randomCountry"), "");
    // Still an error: a name that is not one of them.
    assert!(render("{{$randomUnicorn}}", &vars(&[]), &mut host, Mode::Strict).is_err());
}
