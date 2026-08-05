use super::*;

#[test]
fn parses_minimal_pin_with_defaults() {
    let pins = parse_pins("[[pair]]\nsource = \"1\"\ntarget = \"number\"\n").expect("parses");
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].source, "1");
    assert_eq!(pins[0].target, "number");
    assert_eq!(pins[0].relation, Relation::Assignable);
    assert_eq!(pins[0].expect, None);
    assert!(pins[0].options.is_empty());
}

#[test]
fn parses_full_pin() {
    let text = concat!(
        "# section comment\n",
        "[[pair]]\n",
        "source = '\"a\"'      # literal string keeps quotes\n",
        "target = \"string\"\n",
        "relation = \"comparable\"\n",
        "options = { strictNullChecks = false, target = \"es5\" }\n",
        "setup = \"interface A { next: B }\\ninterface B { next: A }\"\n",
        "expr = \"{ a: 1 }\"\n",
        "expect = \"yes\"\n",
    );
    let pins = parse_pins(text).expect("parses");
    assert_eq!(pins[0].source, "\"a\"");
    assert_eq!(pins[0].relation, Relation::Comparable);
    assert_eq!(
        pins[0].options,
        vec![
            ("strictNullChecks".to_owned(), TomlValue::Bool(false)),
            ("target".to_owned(), TomlValue::Str("es5".to_owned())),
        ]
    );
    assert_eq!(
        pins[0].setup.as_deref(),
        Some("interface A { next: B }\ninterface B { next: A }")
    );
    assert_eq!(pins[0].expr.as_deref(), Some("{ a: 1 }"));
    assert_eq!(pins[0].expect, Some(Expect::Yes));
    assert_eq!(pins[0].expect_line, Some(8));
}

#[test]
fn rejects_malformed_pins() {
    for (text, needle) in [
        ("source = \"1\"\n", "key outside"),
        ("[[pair]]\ntarget = \"number\"\n", "missing source"),
        ("[[pair]]\nsource = \"1\"\n", "missing target"),
        (
            "[[pair]]\nsource = \"1\"\ntarget = \"n\"\nbogus = \"x\"\n",
            "unknown pin key",
        ),
        (
            "[[pair]]\nsource = \"1\"\ntarget = \"n\"\nrelation = \"identity\"\n",
            "relation must be",
        ),
        (
            "[[pair]]\nsource = \"1\"\ntarget = \"n\"\nexpect = \"maybe\"\n",
            "expect must be",
        ),
        (
            "[[pair]]\nsource = \"1\"\ntarget = \"n\"\noptions = { target = \"es5, es2015\" }\n",
            "fixture matrix",
        ),
        (
            "[[pair]]\nsource = \"a\\nb\"\ntarget = \"n\"\n",
            "single-line",
        ),
        (
            "[[pair]]\nsource = \"1\"\nsource = \"2\"\ntarget = \"n\"\n",
            "duplicate key",
        ),
        ("[table]\n", "only [[pair]]"),
    ] {
        let err = parse_pins(text).expect_err(text);
        assert!(
            err.to_string().contains(needle),
            "{text:?}: {err} should contain {needle:?}"
        );
    }
}

fn pin(relation: Relation, expr: Option<&str>) -> Pin {
    Pin {
        index: 7,
        source: "{ a: number }".to_owned(),
        target: "{ a?: number }".to_owned(),
        relation,
        options: vec![("strictNullChecks".to_owned(), TomlValue::Bool(false))],
        setup: None,
        expr: expr.map(str::to_owned),
        expect: None,
        last_key_line: 0,
        expect_line: None,
    }
}

#[test]
fn fixture_text_assignable_snapshot() {
    assert_eq!(
        fixture_text(&pin(Relation::Assignable, None)),
        "// @noLib: true\n\
         // @strictNullChecks: false\n\
         \n\
         // relpin p007: assignable source=\"{ a: number }\" target=\"{ a?: number }\"\n\
         declare var s: { a: number };\n\
         var t: { a?: number } = s;\n"
    );
}

#[test]
fn fixture_text_expr_and_comparable_snapshots() {
    assert_eq!(
        fixture_text(&pin(Relation::Assignable, Some("{ a: 1 }")))
            .lines()
            .last(),
        Some("var t: { a?: number } = { a: 1 };")
    );
    assert_eq!(
        fixture_text(&pin(Relation::Comparable, None))
            .lines()
            .last(),
        Some("var t = s as { a?: number };")
    );
    assert_eq!(
        fixture_text(&pin(Relation::Comparable, Some("{ a: 1 }")))
            .lines()
            .last(),
        Some("var t = ({ a: 1 }) as { a?: number };")
    );
}

#[test]
fn fixture_text_includes_setup_and_skips_nolib_when_overridden() {
    let mut with_setup = pin(Relation::Assignable, None);
    with_setup.setup = Some("interface A { next: B }\ninterface B { next: A }".to_owned());
    let text = fixture_text(&with_setup);
    assert!(text.contains("interface A { next: B }\ninterface B { next: A }\ndeclare var s:"));

    let mut no_lib_false = pin(Relation::Assignable, None);
    no_lib_false.options = vec![("noLib".to_owned(), TomlValue::Bool(false))];
    let text = fixture_text(&no_lib_false);
    assert_eq!(text.matches("noLib").count(), 1);
    assert!(text.starts_with("// @noLib: false\n"));
}

#[test]
fn rewrite_inserts_and_replaces_expects() {
    let text = concat!(
        "# header comment\n",
        "[[pair]]\n",
        "source = \"1\"\n",
        "target = \"number\"\n",
        "\n",
        "# next section\n",
        "[[pair]]\n",
        "source = \"2\"\n",
        "target = \"string\"\n",
        "expect = \"yes\"\n",
    );
    let pins = parse_pins(text).expect("parses");
    let rewritten = rewrite_expects(text, &pins, &[Expect::Yes, Expect::No]);
    assert_eq!(
        rewritten,
        concat!(
            "# header comment\n",
            "[[pair]]\n",
            "source = \"1\"\n",
            "target = \"number\"\n",
            "expect = \"yes\"\n",
            "\n",
            "# next section\n",
            "[[pair]]\n",
            "source = \"2\"\n",
            "target = \"string\"\n",
            "expect = \"no\"\n",
        )
    );
    // Regenerating is idempotent: parse the rewritten file and
    // rewrite with the same verdicts.
    let pins = parse_pins(&rewritten).expect("reparses");
    assert_eq!(
        rewrite_expects(&rewritten, &pins, &[Expect::Yes, Expect::No]),
        rewritten
    );
}
