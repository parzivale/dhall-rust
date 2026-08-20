mod serde {
    use serde::{Deserialize, Serialize};
    use sessiond_serde_dhall::{
        FromDhall, StaticType, ToDhall, Value, from_str, serialize,
    };
    use std::collections;

    fn assert_de<T>(s: &str, x: T)
    where
        T: FromDhall + StaticType + PartialEq + std::fmt::Debug,
    {
        assert_eq!(
            from_str(s)
                .static_type_annotation()
                .parse::<T>()
                .map_err(|e| e.to_string()),
            Ok(x)
        );
    }
    fn assert_ser<T>(s: &str, x: T)
    where
        T: ToDhall + StaticType + PartialEq + std::fmt::Debug,
    {
        assert_eq!(
            serialize(&x)
                .static_type_annotation()
                .to_string()
                .map_err(|e| e.to_string()),
            Ok(s.to_string())
        );
    }
    fn assert_serde<T>(s: &str, x: T)
    where
        T: ToDhall
            + FromDhall
            + StaticType
            + PartialEq
            + std::fmt::Debug
            + Clone,
    {
        assert_de(s, x.clone());
        assert_ser(s, x);
    }

    #[test]
    fn numbers() {
        assert_serde("True", true);

        assert_serde("1", 1u64);
        assert_serde("1", 1u32);
        assert_serde("1", 1usize);

        assert_serde("+1", 1i64);
        assert_serde("+1", 1i32);
        assert_serde("+1", 1isize);

        assert_serde("1.0", 1.0f64);
        assert_serde("1.0", 1.0f32);
    }

    #[test]
    fn text() {
        assert_serde(r#""foo""#, "foo".to_owned());
        assert_ser(r#""foo""#, "foo");
    }

    #[test]
    fn list() {
        assert_serde("[] : List Natural", <Vec<u64>>::new());
        assert_serde("[] : List Text", <Vec<String>>::new());
        assert_serde(
            r#"["foo", "bar"]"#,
            vec!["foo".to_owned(), "bar".to_owned()],
        );
        assert_ser(r#"["foo", "bar"]"#, vec!["foo", "bar"]);
        assert_serde::<Vec<u64>>("[1, 2]", vec![1, 2]);
    }

    #[test]
    fn optional() {
        assert_serde("None Natural", None::<u64>);
        assert_serde("None Text", None::<String>);
        assert_serde("Some 1", Some(1u64));
        assert_eq!(
            serialize(&None::<u64>).to_string().map_err(|e| e.to_string()),
            Err("cannot serialize value without a type annotation: Optional(None)".to_string())
        );
    }

    #[test]
    fn tuple() {
        assert_serde::<()>(r#"{=}"#, ());
        assert_serde::<(u64, String)>(
            r#"{ _1 = 1, _2 = "foo" }"#,
            (1, "foo".to_owned()),
        );
        assert_serde::<(u64, u64, u64, u64)>(
            r#"{ _1 = 1, _2 = 2, _3 = 3, _4 = 4 }"#,
            (1, 2, 3, 4),
        );
    }

    #[test]
    fn structs() {
        // #[derive(
        //     Debug, Clone, PartialEq, Eq, Deserialize, Serialize, StaticType,
        // )]
        // struct Foo;
        // assert_serde::<Foo>("{=}", Foo);

        // #[derive(
        //     Debug, Clone, PartialEq, Eq, Deserialize, Serialize, StaticType,
        // )]
        // struct Bar(u64);
        // assert_serde::<Bar>("{ _1 = 1 }", Bar (1));

        #[derive(
            Debug, Clone, PartialEq, Eq, Deserialize, Serialize, StaticType,
        )]
        struct Baz {
            x: u64,
            y: i64,
        }
        assert_serde::<Baz>("{ x = 1, y = -2 }", Baz { x: 1, y: -2 });
    }

    #[test]
    fn enums() {
        #[derive(
            Debug, Clone, PartialEq, Eq, Deserialize, Serialize, StaticType,
        )]
        enum Foo {
            X(u64),
            Y(i64),
        }
        assert_serde::<Foo>("< X: Natural | Y: Integer >.X 1", Foo::X(1));

        #[derive(
            Debug, Clone, PartialEq, Eq, Deserialize, Serialize, StaticType,
        )]
        enum Bar {
            X,
            Y(i64),
        }
        assert_serde::<Bar>("< X | Y: Integer >.X", Bar::X);

        assert!(
            from_str("< X | Y: Integer >.Y")
                .static_type_annotation()
                .parse::<Bar>()
                .is_err()
        );
    }

    #[test]
    fn with_builtin_type() {
        #[derive(Debug, Deserialize, StaticType, Eq, PartialEq)]
        enum Foo {
            X(u64),
            Y(i64),
        }

        assert_eq!(
            from_str("Foo.X 1")
                .with_builtin_type("Foo".to_string(), Foo::static_type())
                .static_type_annotation()
                .parse::<Foo>()
                .unwrap(),
            Foo::X(1)
        );

        let mut substs = collections::HashMap::new();
        substs.insert("Foo".to_string(), Foo::static_type());

        assert_eq!(
            from_str("Foo.X 1")
                .with_builtin_types(substs)
                .static_type_annotation()
                .parse::<Foo>()
                .unwrap(),
            Foo::X(1)
        );

        #[derive(Debug, Deserialize, StaticType, Eq, PartialEq)]
        enum Bar {
            A,
            B,
        }
        #[derive(Debug, Deserialize, StaticType, Eq, PartialEq)]
        enum Baz {
            X(Bar),
            Y(i64),
        }

        assert_eq!(
            from_str("Baz.X Bar.A")
                .with_builtin_type("Bar".to_string(), Bar::static_type())
                .with_builtin_type("Baz".to_string(), Baz::static_type())
                .static_type_annotation()
                .parse::<Baz>()
                .unwrap(),
            Baz::X(Bar::A)
        );

        let mut substs = collections::HashMap::new();
        substs.insert("Baz".to_string(), Baz::static_type());

        assert_eq!(
            from_str("Baz.X Bar.A")
                .with_builtin_types(substs.clone())
                .with_builtin_type("Bar".to_string(), Bar::static_type())
                .static_type_annotation()
                .parse::<Baz>()
                .unwrap(),
            Baz::X(Bar::A)
        );

        // Check that when chaining builtins, later builtins override earlier ones
        substs.insert("Bar".to_string(), u64::static_type());

        assert_eq!(
            from_str("Baz.X Bar.A")
                .with_builtin_types(substs)
                .with_builtin_type("Bar".to_string(), Bar::static_type())
                .static_type_annotation()
                .parse::<Baz>()
                .unwrap(),
            Baz::X(Bar::A)
        );
    }

    #[test]
    fn test_de_untyped() {
        use std::collections::BTreeMap;
        use std::collections::HashMap;

        fn parse<T: FromDhall>(s: &str) -> T {
            from_str(s).parse().unwrap()
        }

        // Test tuples on record of wrong type. Fields are just taken in alphabetic order.
        assert_eq!(
            parse::<(u64, String, i64)>(r#"{ y = "foo", x = 1, z = +42 }"#),
            (1, "foo".to_owned(), 42)
        );

        let mut expected_map = HashMap::new();
        expected_map.insert("x".to_string(), 1);
        expected_map.insert("y".to_string(), 2);
        assert_eq!(
            parse::<HashMap<String, u64>>("{ x = 1, y = 2 }"),
            expected_map
        );
        // A `Prelude.Map.Type` is a list of records and stays one, so it does
        // not deserialize into a map. Doing so would drop repeated keys and the
        // ordering, both of which a Dhall map can carry.
        assert!(
            from_str("toMap { x = 1, y = 2 }")
                .parse::<HashMap<String, u64>>()
                .is_err()
        );
        assert_eq!(
            parse::<Vec<(String, u64)>>(
                "[ { _1 = \"x\", _2 = 1 }, { _1 = \"y\", _2 = 2 } ]"
            ),
            vec![("x".to_string(), 1), ("y".to_string(), 2)]
        );

        let mut expected_map = HashMap::new();
        expected_map.insert("if".to_string(), 1);
        expected_map.insert("FOO_BAR".to_string(), 2);
        expected_map.insert("baz-kux".to_string(), 3);
        assert_eq!(
            parse::<HashMap<String, u64>>(
                "{ `if` = 1, FOO_BAR = 2, baz-kux = 3 }"
            ),
            expected_map
        );

        let mut expected_map = BTreeMap::new();
        expected_map.insert("x".to_string(), 1);
        expected_map.insert("y".to_string(), 2);
        assert_eq!(
            parse::<BTreeMap<String, u64>>("{ x = 1, y = 2 }"),
            expected_map
        );

        #[derive(Debug, PartialEq, Eq, Deserialize)]
        struct Foo {
            x: u64,
            y: Option<u64>,
        }
        // Omit optional field
        assert_eq!(parse::<Foo>("{ x = 1 }"), Foo { x: 1, y: None });

        // https://github.com/Nadrieril/dhall-rust/issues/155
        assert!(from_str("List/length [True, 42]").parse::<bool>().is_err());
    }

    /// Locate a fixture in the dhall-lang suite. Uses the pin when one is set
    /// (the nix devshell does), else a checkout beside the workspace.
    fn dhall_lang_file(rel_path: &str) -> String {
        match std::env::var("DHALL_LANG_DIR") {
            Ok(dir) => format!("{}/{}", dir, rel_path),
            Err(_) => format!("../dhall-lang/{}", rel_path),
        }
    }

    #[test]
    fn test_file() {
        assert_eq!(
            sessiond_serde_dhall::from_file(dhall_lang_file(
                "tests/parser/success/unit/BoolLitTrueA.dhall"
            ))
            .static_type_annotation()
            .parse::<bool>()
            .map_err(|e| e.to_string()),
            Ok(true)
        );
        assert_eq!(
            sessiond_serde_dhall::from_binary_file(dhall_lang_file(
                "tests/parser/success/unit/BoolLitTrueB.dhallb"
            ))
            .static_type_annotation()
            .parse::<bool>()
            .map_err(|e| e.to_string()),
            Ok(true)
        );
    }

    #[test]
    fn test_import() {
        assert_de(
            &dhall_lang_file("tests/parser/success/unit/BoolLitTrueA.dhall"),
            true,
        );
        assert_eq!(
            sessiond_serde_dhall::from_str(
                "../dhall-lang/tests/parser/success/unit/BoolLitTrueA.dhall"
            )
            .imports(false)
            .static_type_annotation()
            .parse::<bool>()
            .map_err(|e| e.to_string()),
            Err("importing `../dhall-lang/tests/parser/success/unit/BoolLitTrueA.dhall` is not allowed here".to_string())
        );
    }

    #[test]
    fn test_remote_imports_can_be_refused_while_local_still_work() {
        // No network involved: a refused remote import is rejected before any
        // request goes out.
        let err =
            sessiond_serde_dhall::from_str("https://example.com/config.dhall")
                .remote_imports(false)
                .parse::<u64>()
                .unwrap_err()
                .to_string();
        assert!(
            err.contains("remote imports are disabled")
                && err.contains("https://example.com/config.dhall"),
            "expected the error to name the refused import, got: {}",
            err
        );

        // A local import is still resolved.
        assert_eq!(
            sessiond_serde_dhall::from_str(&dhall_lang_file(
                "tests/parser/success/unit/BoolLitTrueA.dhall"
            ))
            .remote_imports(false)
            .parse::<bool>()
            .map_err(|e| e.to_string()),
            Ok(true)
        );

        // `imports(false)` still refuses everything, remote included.
        assert!(
            sessiond_serde_dhall::from_str("https://example.com/config.dhall")
                .imports(false)
                .parse::<u64>()
                .is_err()
        );
    }

    #[test]
    #[ignore] // Way too slow
    fn test_prelude() {
        assert_eq!(
            sessiond_serde_dhall::from_str(
                "https://prelude.dhall-lang.org/package.dhall"
            )
            .parse::<Value>()
            .map(|_| ())
            .map_err(|e| e.to_string()),
            Ok(())
        );
    }

    // TODO: test various builder configurations
    // In particular test cloning and reusing builder
}
