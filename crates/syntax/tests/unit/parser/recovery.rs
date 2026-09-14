use super::*;

fn assert_coverage(diagnostics: &DiagnosticList, recovery: &ParseRecovery) {
    assert_eq!(diagnostics.len(), recovery.diagnostic_origins().len());
    let mut covered = vec![false; diagnostics.len()];
    for event in recovery.events() {
        if let Some(index) = event.diagnostic_index {
            let diagnostic = &diagnostics[index];
            assert!(
                !covered[index],
                "retained diagnostic has one producing event"
            );
            covered[index] = true;
            assert_eq!(event.start, diagnostic.start.unwrap());
            assert_eq!(event.length, diagnostic.length.unwrap());
            assert_eq!(
                event.kind,
                ParseRecoveryKind::Diagnostic(recovery.diagnostic_origins()[index]),
            );
        }
    }
    assert!(covered.into_iter().all(|covered| covered));
}

fn source(text: &str) -> SourceFile {
    parse_source_file(
        "main.ts".into(),
        text.to_owned(),
        ParseOptions::default(),
        None,
    )
}

#[test]
fn literal_recovery_uses_token_kinds_and_keeps_every_retained_origin() {
    // These are grammar/token controls, independent of diagnostic codes.
    let admitted = [
        r#"const x = "\8";"#,
        r#"const x = "\xG";"#,
        "const x = 'unterminated\n;",
        r#"const x = `\8${a}\9${b}\xG`;"#,
        r#"const x = tag`\unicode`;"#,
        "function f(a?: number) {};; const a = [,];",
        "export {}; await f();",
        r#"export {}; const a = "\8"; await f(); const b = "\9";"#,
    ];
    for text in admitted {
        let source = source(text);
        assert_coverage(&source.parse_diagnostics, source.parse_recovery());
        assert!(
            source.has_only_literal_recovery(),
            "{text}: {:?}",
            source.parse_recovery()
        );
    }
    let fragments = source(r#"const x = `\8${a}\9${b}\xG`;"#);
    assert_eq!(
        fragments.parse_recovery().diagnostic_origins(),
        &[
            ParseDiagnosticOrigin::ScannerToken(SyntaxKind::TemplateHead),
            ParseDiagnosticOrigin::ScannerToken(SyntaxKind::TemplateMiddle),
            ParseDiagnosticOrigin::ScannerToken(SyntaxKind::TemplateTail),
        ]
    );
    for text in [
        r#"const x = 1__0; const y = "\8";"#,
        r#"const \u00G0 = 1;"#,
        "const x = /unterminated",
        "/* unterminated",
        r#"const "\8" = 1;"#,
        "export {}; const x = ; await f();",
        "export {}; await f(); const x = ;",
        "export {}; await f(;",
    ] {
        let source = source(text);
        assert_coverage(&source.parse_diagnostics, source.parse_recovery());
        assert!(!source.has_only_literal_recovery(), "{text}");
    }
    let comment = source("/* unterminated");
    assert_eq!(
        comment.parse_recovery().diagnostic_origins(),
        &[ParseDiagnosticOrigin::ScannerTrivia(
            SyntaxKind::MultiLineCommentTrivia
        ),]
    );
}

#[test]
fn same_start_deduplication_cannot_hide_structural_recovery() {
    let mut parser = Parser::new(
        "main.ts".into(),
        r#""\8""#,
        LanguageVariant::Standard,
        false,
    );
    parser.next_token();
    assert_eq!(parser.parse_diagnostics.len(), 1);
    assert!(parser.parse_recovery.is_literal_only(1));
    let start = parser.parse_diagnostics[0].start.unwrap();
    let byte_start = parser.positions.utf16_to_byte(start).unwrap() as usize;
    parser.parse_error_at_position(byte_start, 0, &gen::Identifier_expected, &[]);
    assert_eq!(parser.parse_diagnostics.len(), 1);
    assert_eq!(parser.parse_recovery.events().len(), 2);
    assert_eq!(parser.parse_recovery.events()[1].diagnostic_index, None);
    assert!(!parser.parse_recovery.is_literal_only(1));
    assert_coverage(&parser.parse_diagnostics, &parser.parse_recovery);

    let mut parser = Parser::new(
        "main.ts".into(),
        r#""\8""#,
        LanguageVariant::Standard,
        false,
    );
    parser.parse_error_at_position(1, 2, &gen::Identifier_expected, &[]);
    parser.next_token();
    assert_eq!(parser.parse_diagnostics.len(), 1);
    assert_eq!(parser.parse_recovery.events().len(), 2);
    assert_eq!(parser.parse_recovery.events()[1].diagnostic_index, None);
    assert!(!parser.parse_recovery.is_literal_only(1));
    assert_coverage(&parser.parse_diagnostics, &parser.parse_recovery);
}

#[test]
fn silent_missing_nodes_are_events_and_wrappers_do_not_double_count() {
    let mut parser = Parser::new("main.ts".into(), ";", LanguageVariant::Standard, false);
    parser.next_token();
    parser.create_missing_node(SyntaxKind::Identifier, false, None, &[]);
    assert!(parser.parse_diagnostics.is_empty());
    assert_eq!(parser.parse_recovery.events().len(), 1);
    assert!(!parser.parse_recovery.is_literal_only(0));
    parser.create_missing_node(
        SyntaxKind::Identifier,
        false,
        Some(&gen::Identifier_expected),
        &[],
    );
    assert_eq!(parser.parse_recovery.events().len(), 2);
    assert_coverage(&parser.parse_diagnostics, &parser.parse_recovery);
}

#[test]
fn parser_transactions_restore_origins_and_silent_events_but_commit_success() {
    let mut parser = Parser::new(
        "main.ts".into(),
        r#"a "\8""#,
        LanguageVariant::Standard,
        false,
    );
    parser.next_token();
    let baseline = parser.parse_recovery.clone();
    parser.look_ahead(|parser| {
        parser.next_token();
        parser.create_missing_node(SyntaxKind::Identifier, false, None, &[]);
        assert_eq!(parser.parse_recovery.events().len(), 2);
        true
    });
    assert_eq!(parser.parse_recovery, baseline);
    assert!(parser.parse_diagnostics.is_empty());
    assert!(!parser.try_parse(|parser| {
        parser.next_token();
        parser.create_missing_node(SyntaxKind::Identifier, false, None, &[]);
        false
    }));
    assert_eq!(parser.parse_recovery, baseline);
    assert!(parser.parse_diagnostics.is_empty());
    assert!(parser.try_parse(|parser| {
        parser.next_token();
        true
    }));
    assert!(parser.parse_recovery.is_literal_only(1));
    let literal_only = parser.parse_recovery.clone();
    assert!(!parser.try_parse(|parser| {
        parser.create_missing_node(SyntaxKind::Identifier, false, None, &[]);
        false
    }));
    assert_eq!(parser.parse_recovery, literal_only);
    assert!(parser.try_parse(|parser| {
        parser.create_missing_node(SyntaxKind::Identifier, false, None, &[]);
        true
    }));
    assert!(!parser.parse_recovery.is_literal_only(1));
    assert_coverage(&parser.parse_diagnostics, &parser.parse_recovery);
}

#[test]
fn reference_directives_are_structural_and_jsdoc_has_a_separate_destination() {
    let reference = source("/// <reference path=oops />\nconst x = 1;");
    assert!(!reference.has_only_literal_recovery());
    assert!(reference
        .parse_recovery()
        .diagnostic_origins()
        .contains(&ParseDiagnosticOrigin::ReferenceDirective,));
    assert_coverage(&reference.parse_diagnostics, reference.parse_recovery());

    let jsdoc = parse_source_file(
        "main.js".into(),
        "/** @type {Array<} */ const x = 1;".to_owned(),
        ParseOptions {
            javascript_file: true,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(!jsdoc.js_doc_diagnostics.is_empty());
    assert!(jsdoc.parse_diagnostics.is_empty());
    assert!(jsdoc.parse_recovery().events().is_empty());
    assert!(jsdoc.has_only_literal_recovery());
}

#[test]
fn original_adjacent_inputs_have_complete_recovery_coverage() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../fixtures/utf16-owned-literal-values.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let text = case["source"].as_str().unwrap();
        let parsed = source(text);
        assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
        // The additional invalid binding control is structural; the original
        // 23 inputs and the other value-ownership controls fit the boundary.
        let admitted = case["case_id"] != "ownership/invalid-binding-name";
        assert_eq!(
            parsed.has_only_literal_recovery(),
            admitted,
            "{}: {:?}",
            case["case_id"],
            parsed.parse_recovery()
        );
    }
}
