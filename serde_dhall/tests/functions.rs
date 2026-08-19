use serde::Deserialize;
use sessiond_serde_dhall::{from_str, Function, SimpleType, SimpleValue};
use std::collections::HashMap;

#[test]
fn apply_a_simple_function() {
    let f: Function = from_str("\\(x : Natural) -> x + 1").parse().unwrap();
    assert_eq!(f.apply::<_, u64>(1u64).unwrap(), 2);
    assert_eq!(f.apply::<_, u64>(41u64).unwrap(), 42);
}

#[test]
fn a_function_can_be_applied_repeatedly() {
    let f: Function = from_str("\\(x : Natural) -> x * 2").parse().unwrap();
    let mut n = 1u64;
    for _ in 0..5 {
        n = f.apply(n).unwrap();
    }
    assert_eq!(n, 32);
}

#[test]
fn apply_a_function_exposed_by_a_config() {
    #[derive(Deserialize)]
    struct Config {
        port: u64,
        greeting: Function,
    }

    let config: Config = from_str(
        r#"{ port = 8080, greeting = \(name : Text) -> "Hello, ${name}!" }"#,
    )
    .parse()
    .unwrap();

    assert_eq!(config.port, 8080);
    assert_eq!(
        config.greeting.apply::<_, String>("world").unwrap(),
        "Hello, world!".to_string()
    );
}

#[test]
fn functions_nested_in_lists_and_optionals() {
    let fs: Vec<Function> =
        from_str("[\\(x : Natural) -> x + 1, \\(x : Natural) -> x + 2]")
            .parse()
            .unwrap();
    assert_eq!(fs.len(), 2);
    assert_eq!(fs[0].apply::<_, u64>(0u64).unwrap(), 1);
    assert_eq!(fs[1].apply::<_, u64>(0u64).unwrap(), 2);

    let f: Option<Function> =
        from_str("Some (\\(x : Natural) -> x + 1)").parse().unwrap();
    assert_eq!(f.unwrap().apply::<_, u64>(0u64).unwrap(), 1);

    let f: Option<Function> =
        from_str("None (Natural -> Natural)").parse().unwrap();
    assert!(f.is_none());
}

#[test]
fn functions_of_several_arguments_are_curried() {
    let f: Function = from_str("\\(x : Natural) -> \\(y : Natural) -> x * y")
        .parse()
        .unwrap();
    let times_six: Function = f.apply(6u64).unwrap();
    assert_eq!(times_six.apply::<_, u64>(7u64).unwrap(), 42);
}

#[test]
fn a_function_captures_its_environment() {
    let f: Function = from_str("let n = 3 in \\(x : Natural) -> x * n")
        .parse()
        .unwrap();
    assert_eq!(f.apply::<_, u64>(14u64).unwrap(), 42);
}

#[test]
fn apply_a_partially_applied_builtin() {
    let f: Function = from_str("Natural/subtract 1").parse().unwrap();
    assert_eq!(f.apply::<_, u64>(43u64).unwrap(), 42);
}

#[test]
fn arguments_and_results_can_be_structured() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    struct Point {
        x: u64,
        y: u64,
    }
    #[derive(serde::Serialize)]
    struct Delta {
        dx: u64,
        dy: u64,
    }

    let f: Function = from_str(
        "\\(d : { dx : Natural, dy : Natural }) -> { x = d.dx + 1, y = d.dy + 2 }",
    )
    .parse()
    .unwrap();

    assert_eq!(
        f.apply::<_, Point>(Delta { dx: 1, dy: 2 }).unwrap(),
        Point { x: 2, y: 4 }
    );
}

#[test]
fn an_empty_list_argument_uses_the_declared_input_type() {
    // The empty list needs a type annotation to be converted to Dhall; it is taken from the
    // function's own type.
    let f: Function =
        from_str("\\(xs : List Natural) -> List/length Natural xs")
            .parse()
            .unwrap();
    let empty: Vec<u64> = vec![];
    assert_eq!(f.apply::<_, u64>(empty).unwrap(), 0);
    assert_eq!(f.apply::<_, u64>(vec![1u64, 2, 3]).unwrap(), 3);
}

#[test]
fn applying_an_argument_of_the_wrong_type_fails() {
    let f: Function = from_str("\\(x : Natural) -> x + 1").parse().unwrap();
    assert!(f.apply::<_, u64>("not a number").is_err());
    assert!(f.apply::<_, u64>(true).is_err());
}

#[test]
fn deserializing_the_result_into_the_wrong_type_fails() {
    let f: Function = from_str("\\(x : Natural) -> x + 1").parse().unwrap();
    assert!(f.apply::<_, String>(1u64).is_err());
}

#[test]
fn a_function_reports_its_type() {
    let f: Function = from_str("\\(x : Natural) -> [x]").parse().unwrap();
    assert_eq!(f.input_type(), Some(SimpleType::Natural));
    assert_eq!(
        f.output_type(),
        Some(SimpleType::List(Box::new(SimpleType::Natural)))
    );
}

#[test]
fn a_polymorphic_function_has_no_simple_type() {
    let f: Function =
        from_str("\\(a : Type) -> \\(x : a) -> x").parse().unwrap();
    // `Type` is not a `SimpleType`, and neither is the dependent result `∀(x : a) → a`.
    assert_eq!(f.input_type(), None);
    assert_eq!(f.output_type(), None);

    // It can still be applied, one argument at a time.
    let identity: Function = f.apply(SimpleType::Natural).unwrap();
    assert_eq!(identity.apply::<_, u64>(42u64).unwrap(), 42);
}

#[test]
fn function_types_are_simple_types() {
    let ty: SimpleType = from_str("Natural -> Text").parse().unwrap();
    assert_eq!(
        ty,
        SimpleType::Function(
            Box::new(SimpleType::Natural),
            Box::new(SimpleType::Text)
        )
    );

    // And they can be used as a type annotation.
    let f = from_str("\\(x : Natural) -> Natural/show x")
        .type_annotation(&ty)
        .parse::<Function>()
        .unwrap();
    assert_eq!(f.apply::<_, String>(42u64).unwrap(), "42".to_string());

    assert!(from_str("\\(x : Natural) -> x")
        .type_annotation(&ty)
        .parse::<Function>()
        .is_err());
}

#[test]
fn a_record_type_can_mention_functions() {
    let ty: SimpleType = from_str("{ f : Natural -> Natural, x : Natural }")
        .parse()
        .unwrap();
    match &ty {
        SimpleType::Record(kts) => {
            assert_eq!(
                kts.get("f"),
                Some(&SimpleType::Function(
                    Box::new(SimpleType::Natural),
                    Box::new(SimpleType::Natural)
                ))
            );
        }
        _ => panic!("expected a record type"),
    }

    #[derive(Deserialize)]
    struct Foo {
        f: Function,
        x: u64,
    }
    let foo = from_str("{ f = \\(x : Natural) -> x + 1, x = 1 }")
        .type_annotation(&ty)
        .parse::<Foo>()
        .unwrap();
    assert_eq!(foo.x, 1);
    assert_eq!(foo.f.apply::<_, u64>(1u64).unwrap(), 2);
}

#[test]
fn a_dependent_function_type_is_not_a_simple_type() {
    assert!(from_str("forall (a : Type) -> List a")
        .parse::<SimpleType>()
        .is_err());
}

#[test]
fn a_function_survives_a_simple_value_round_trip() {
    let val: SimpleValue =
        from_str("\\(x : Natural) -> x + 1").parse().unwrap();
    let f = match val {
        SimpleValue::Function(f) => f,
        v => panic!("expected a function, got {:?}", v),
    };
    assert_eq!(f.apply::<_, u64>(1u64).unwrap(), 2);

    // A function nested inside a record survives too.
    let val: SimpleValue = from_str("{ f = \\(x : Natural) -> x + 1, y = 1 }")
        .parse()
        .unwrap();
    let f = match &val {
        SimpleValue::Record(kvs) => match kvs.get("f") {
            Some(SimpleValue::Function(f)) => f.clone(),
            v => panic!("expected a function, got {:?}", v),
        },
        v => panic!("expected a record, got {:?}", v),
    };
    assert_eq!(f.apply::<_, u64>(1u64).unwrap(), 2);

    // And it can be handed back to serde afterwards.
    #[derive(Deserialize)]
    struct Foo {
        f: Function,
        y: u64,
    }
    let foo: Foo = sessiond_serde_dhall::from_simple_value(val).unwrap();
    assert_eq!(foo.y, 1);
    assert_eq!(foo.f.apply::<_, u64>(1u64).unwrap(), 2);
}

#[test]
fn deserializing_a_function_into_another_type_fails() {
    assert!(from_str("\\(x : Natural) -> x").parse::<u64>().is_err());
    assert!(from_str("\\(x : Natural) -> x").parse::<String>().is_err());
    assert!(from_str("{ f = \\(x : Natural) -> x }")
        .parse::<HashMap<String, u64>>()
        .is_err());
}

#[test]
fn a_function_serializes_back_to_dhall() {
    let f: Function = from_str("\\(x : Natural) -> x + 1").parse().unwrap();
    assert_eq!(
        sessiond_serde_dhall::serialize(&f).to_string().unwrap(),
        "λ(x : Natural) → x + 1".to_string()
    );

    // What is stored is the normal form, so the output is equivalent to the input rather than
    // identical to it.
    let f: Function = from_str("let n = 1 in \\(x : Natural) -> x + n")
        .parse()
        .unwrap();
    assert_eq!(
        sessiond_serde_dhall::serialize(&f).to_string().unwrap(),
        "λ(x : Natural) → x + 1".to_string()
    );
}

#[test]
fn a_config_containing_functions_round_trips() {
    #[derive(serde::Serialize, Deserialize)]
    struct Config {
        port: u64,
        greeting: Function,
    }

    let src =
        r#"{ greeting = \(name : Text) -> "Hello, ${name}!", port = 8080 }"#;
    let config: Config = from_str(src).parse().unwrap();
    let out = sessiond_serde_dhall::serialize(&config)
        .to_string()
        .unwrap();

    // The re-serialized config parses back to something that behaves the same.
    let config: Config = from_str(&out).parse().unwrap();
    assert_eq!(config.port, 8080);
    assert_eq!(
        config.greeting.apply::<_, String>("world").unwrap(),
        "Hello, world!".to_string()
    );
}

#[test]
fn a_function_can_be_serialized_through_simple_value() {
    let f: Function = from_str("\\(x : Natural) -> x").parse().unwrap();
    let val = SimpleValue::Function(f);
    assert_eq!(
        sessiond_serde_dhall::serialize(&val).to_string().unwrap(),
        "λ(x : Natural) → x".to_string()
    );
}

#[test]
fn a_function_can_be_passed_to_another_function() {
    let map: Function =
        from_str("\\(f : Natural -> Natural) -> \\(x : Natural) -> f (f x)")
            .parse()
            .unwrap();
    let increment: Function =
        from_str("\\(x : Natural) -> x + 1").parse().unwrap();

    let increment_twice: Function = map.apply(increment).unwrap();
    assert_eq!(increment_twice.apply::<_, u64>(40u64).unwrap(), 42);
}

#[test]
fn a_function_serialized_with_a_type_annotation_is_checked() {
    let f: Function = from_str("\\(x : Natural) -> x").parse().unwrap();
    let fn_ty: SimpleType = from_str("Natural -> Natural").parse().unwrap();
    assert!(sessiond_serde_dhall::serialize(&f)
        .type_annotation(&fn_ty)
        .to_string()
        .is_ok());

    // A non-function annotation is rejected.
    assert!(sessiond_serde_dhall::serialize(&f)
        .type_annotation(&SimpleType::Natural)
        .to_string()
        .is_err());
}

#[test]
fn functions_still_reject_non_functions() {
    assert!(from_str("1").parse::<Function>().is_err());
    assert!(from_str("{ x = 1 }").parse::<Function>().is_err());
    assert!(from_str("Natural").parse::<Function>().is_err());
}

#[test]
fn function_types_and_functions_print_as_dhall() {
    let ty: SimpleType = from_str("Natural -> Text").parse().unwrap();
    assert_eq!(ty.to_string(), "Natural → Text".to_string());

    let f: Function = from_str("\\(x : Natural) -> x + 1").parse().unwrap();
    assert_eq!(f.to_string(), "λ(x : Natural) → x + 1".to_string());
}

#[test]
fn functions_serialize_from_every_nesting_position() {
    let f: Function = from_str("\\(x : Natural) -> x").parse().unwrap();
    let expected = "λ(x : Natural) → x".to_string();

    // Bare.
    assert_eq!(
        sessiond_serde_dhall::serialize(&f).to_string().unwrap(),
        expected
    );

    // Through `serialize_some`.
    assert_eq!(
        sessiond_serde_dhall::serialize(&Some(f.clone()))
            .to_string()
            .unwrap(),
        format!("Some ({})", expected)
    );

    // Through `serialize_seq`.
    assert_eq!(
        sessiond_serde_dhall::serialize(&vec![f.clone()])
            .to_string()
            .unwrap(),
        format!("[{}]", expected)
    );

    // Through `serialize_struct`.
    #[derive(serde::Serialize)]
    struct Wrapper {
        f: Function,
    }
    assert_eq!(
        sessiond_serde_dhall::serialize(&Wrapper { f: f.clone() })
            .to_string()
            .unwrap(),
        format!("{{ f = {} }}", expected)
    );

    // Through `serialize_newtype_variant`.
    #[derive(serde::Serialize)]
    enum E {
        F(Function),
    }
    let ty: SimpleType =
        from_str("< F : Natural -> Natural >").parse().unwrap();
    assert_eq!(
        sessiond_serde_dhall::serialize(&E::F(f))
            .type_annotation(&ty)
            .to_string()
            .unwrap(),
        format!("< F: Natural → Natural >.F ({})", expected)
    );
}
