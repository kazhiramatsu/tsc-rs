// These admission tests accompany complete compiler commands; they do not
// establish output compatibility by themselves.
#[test]
fn leading_invalid_binding_recovery_requires_the_composite_statement() {
    let parse = |text: &str, target| {
        parse_source_file(
            "main.ts".into(),
            text.into(),
            ParseOptions {
                script_target: target,
                ..ParseOptions::default()
            },
            None,
        )
    };
    for text in [
        "export const \\u{10400} = 1;",
        "export let \\u{10401} = 2;",
        "var \\u{10400} = 1;",
        "export const \\u{10400} = 1; export const \\u{10401} = 2;",
        "/* 😀 */ export const \\u{10400} = 1;",
    ] {
        let parsed = parse(text, tsc_types::ScriptTarget::ES5);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
        assert!(!parsed.has_only_statement_gap_emit_recovery(), "{text}");
        assert!(parsed.has_supported_emit_recovery(), "{text}");
        let modern = parse(text, tsc_types::ScriptTarget::ES2015);
        assert!(modern.parse_diagnostics.is_empty(), "{text}");
        assert!(modern.has_supported_emit_recovery(), "{text}");
    }
    let declaration = parse_source_file(
        "main.d.ts".into(),
        "export const \\u{10400} = 1;".into(),
        ParseOptions {
            script_target: tsc_types::ScriptTarget::ES5,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(!declaration.has_supported_emit_recovery());
    for text in [
        "let \\u{10401} = 2;",
        "const ¬u = 1;",
        "const u {a} = 1;",
        "const u, {10400} = 1;",
        "const a, \\u{10400} = 1;",
        "for (var \\u{10400} = 0;;) {}",
        "declare const \\u{10400} = 1;",
        "declare namespace N { const \\u{10400} = 1; }",
        "export const \\u{10400} = f();",
        "export const \\u{10400} = 'text';",
        "const \\u{\"a\"} = 1;",
        "const \\u{10400} = 1, z = 2;",
        "const \\u{10400,10401} = 1;",
        "using \\u{10400} = 1;",
        "export const \\u{1F600}x = 1;",
    ] {
        let parsed = parse(text, tsc_types::ScriptTarget::ES5);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(!parsed.has_supported_emit_recovery(), "{text}");
    }
}

#[test]
fn leading_invalid_binding_recovery_refuses_unpaired_or_shifted_facts() {
    let parsed = parse_source_file(
        "main.ts".into(),
        "export const \\u{10400} = 1;".into(),
        ParseOptions {
            script_target: tsc_types::ScriptTarget::ES5,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(parsed.has_supported_emit_recovery());
    for index in 0..parsed.parse_recovery.events.len() {
        let mut missing = parsed.clone();
        missing.parse_recovery.events.remove(index);
        assert!(
            !missing.has_supported_emit_recovery(),
            "removed event {index}"
        );
        let mut duplicated = parsed.clone();
        duplicated
            .parse_recovery
            .events
            .push(parsed.parse_recovery.events[index]);
        assert!(
            !duplicated.has_supported_emit_recovery(),
            "duplicated event {index}"
        );
        let mut shifted = parsed.clone();
        shifted.parse_recovery.events[index].full_start += 1;
        assert!(
            !shifted.has_supported_emit_recovery(),
            "shifted event {index}"
        );
    }
    let mut no_skip = parsed.clone();
    no_skip.parse_recovery.actions.clear();
    assert!(!no_skip.has_supported_emit_recovery());
    let mut duplicate_skip = parsed.clone();
    duplicate_skip
        .parse_recovery
        .actions
        .push(parsed.parse_recovery.actions[0]);
    assert!(!duplicate_skip.has_supported_emit_recovery());
    let mut wrong_owner = parsed;
    let ParseRecoveryAction::TokenSkipped {
        statement_start, ..
    } = &mut wrong_owner.parse_recovery.actions[0]
    else {
        unreachable!()
    };
    *statement_start += 1;
    assert!(!wrong_owner.has_supported_emit_recovery());
}
