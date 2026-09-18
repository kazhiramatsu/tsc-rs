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
    assert!(jsdoc.parse_recovery().actions().is_empty());
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

#[test]
fn missing_node_provenance_keeps_full_start_before_utf16_trivia() {
    let text = "async function f(a = await /*😀*/ ) {}";
    let parsed = source(text);
    assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
    let events = parsed.parse_recovery().events();
    assert_eq!(events.len(), 1);
    let event = events[0];
    let position = text[..text.find(" /*😀*/").unwrap()].encode_utf16().count() as u32;
    let token_start = text[..text.find(')').unwrap()].encode_utf16().count() as u32;
    assert_eq!(
        event.missing_node,
        Some(MissingNodeRecovery {
            kind: SyntaxKind::Identifier,
            position,
        })
    );
    assert_eq!(event.start, token_start);
    assert_ne!(position, event.start);
    assert!(parsed.parse_recovery().actions().is_empty());
    assert!(!parsed.has_only_literal_recovery());
}

#[test]
fn skip_provenance_survives_same_start_diagnostic_deduplication() {
    let text = "async function foo(a = await => await): Promise<void> {}";
    let parsed = source(text);
    assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
    let events = parsed.parse_recovery().events();
    assert_eq!(events.len(), 3);
    assert!(events[0].missing_node.is_some());
    for event in &events[1..] {
        assert_eq!(event.diagnostic_index, None);
        assert_eq!(event.missing_node, None);
        assert_eq!(event.start, events[0].start);
    }
    assert_eq!(
        parsed.parse_recovery().actions(),
        &[ParseRecoveryAction::TokenSkipped {
            token: SyntaxKind::EqualsGreaterThanToken,
            start: text.find("=>").unwrap() as u32,
            length: 2,
            statement_start: 0,
            site: ParseTokenSkipSite::ListAbort,
        },]
    );
}

#[test]
fn speculative_skips_and_missing_provenance_roll_back_together() {
    let mut parser = Parser::new(
        "main.ts".into(),
        "a /*😀*/ )",
        LanguageVariant::Standard,
        false,
    );
    parser.next_token();
    let before = parser.parse_recovery.clone();
    assert!(!parser.try_parse(|parser| {
        parser.next_token();
        parser.create_missing_node(
            SyntaxKind::Identifier,
            false,
            Some(&gen::Identifier_expected),
            &[],
        );
        parser.record_token_skip(ParseTokenSkipSite::ListAbort);
        parser.next_token();
        assert_eq!(parser.parse_recovery.actions().len(), 1);
        assert!(parser.parse_recovery.events()[0].missing_node.is_some());
        false
    }));
    assert_eq!(parser.parse_recovery, before);
    assert!(parser.parse_diagnostics.is_empty());
    assert_eq!(parser.token(), SyntaxKind::Identifier);
}

#[test]
fn top_level_reparse_ranges_belong_to_reparsed_statements() {
    // A call-shaped await is ambiguous until the external module is known.
    // In contrast, `await f()` parses as an await expression on the first pass.
    let text = "export {}; const before = ; await(f()); const after = ;";
    let parsed = source(text);
    assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
    assert_eq!(parsed.parse_diagnostics.len(), 2);
    assert_eq!(parsed.parse_recovery().events().len(), 2);
    assert_eq!(
        parsed
            .parse_recovery()
            .events()
            .iter()
            .map(|event| event.start)
            .collect::<Vec<_>>(),
        vec![
            text.find("= ;").unwrap() as u32 + 2,
            text.rfind("= ;").unwrap() as u32 + 2
        ],
    );
    assert_eq!(
        parsed.parse_recovery().actions(),
        &[ParseRecoveryAction::Reparsed {
            start: text.find(" await").unwrap() as u32,
            end: text.find(" const after").unwrap() as u32,
        },]
    );
    let direct = source("export {}; await f();");
    assert!(direct.has_only_literal_recovery());
    assert!(direct.parse_recovery().actions().is_empty());
    let clean = source("export {}; await(f());");
    assert!(clean.has_only_literal_recovery());
    assert!(matches!(
        clean.parse_recovery().actions(),
        [ParseRecoveryAction::Reparsed { .. }]
    ));
}
