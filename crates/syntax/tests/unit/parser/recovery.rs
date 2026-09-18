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

#[test]
fn missing_await_admission_requires_only_committed_reachable_operands() {
    for text in [
        "async function f(a = await /*😀*/ ) {}",
        "const f = async (a = await) => {};",
        "async function f() { await; return await; }",
        "async function f() { const x = await; return x; }",
        "async function* f() { yield await; }",
        "async function f() { return await await; }",
        "async function f(a = await) {} const x = '\\8';",
    ] {
        let parsed = source(text);
        assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
        assert!(!parsed.has_only_literal_recovery(), "{text}");
        assert!(
            parsed.has_only_literal_or_missing_await_recovery(),
            "{text}: {:?}",
            parsed.parse_recovery()
        );
    }
    for text in [
        "async function f(a = await => await) {}",
        "const f = async (a = await => await) => {};",
        "async function f() { await; const x = ; }",
        "async function f() { await; x.; }",
        "async function f(a = await) {} /* unterminated",
        "export {}; await(f()); async function f(a = await) {}",
        "export {}; async function f(a = await) {} await(f());",
    ] {
        let parsed = source(text);
        assert!(
            !parsed.has_only_literal_or_missing_await_recovery(),
            "{text}: {:?}",
            parsed.parse_recovery()
        );
    }
    // Clean reparse and literal recovery preserve the previous admission.
    for text in [
        "export {}; await(f());",
        "export {}; await(f()); const x = '\\8';",
        "const a = [,]; function f(a?: number) {}",
    ] {
        let parsed = source(text);
        assert!(parsed.has_only_literal_recovery(), "{text}");
        assert!(
            parsed.has_only_literal_or_missing_await_recovery(),
            "{text}"
        );
    }
}

#[test]
fn missing_await_admission_rejects_lost_duplicate_and_unowned_facts() {
    let parsed = source("async function f(a = await /*😀*/ ) {}");
    assert!(parsed.has_only_literal_or_missing_await_recovery());
    let mut missing = parsed.clone();
    missing.parse_recovery.events.clear();
    assert!(!missing.has_only_literal_or_missing_await_recovery());
    let mut duplicate = parsed.clone();
    duplicate
        .parse_recovery
        .events
        .push(parsed.parse_recovery.events[0]);
    assert!(!duplicate.has_only_literal_or_missing_await_recovery());
    let mut suppressed = parsed.clone();
    suppressed.parse_recovery.events[0].diagnostic_index = None;
    assert!(!suppressed.has_only_literal_or_missing_await_recovery());
    let mut misplaced = parsed.clone();
    misplaced.parse_recovery.events[0]
        .missing_node
        .as_mut()
        .unwrap()
        .position += 1;
    assert!(!misplaced.has_only_literal_or_missing_await_recovery());
    let mut skipped = parsed.clone();
    skipped
        .parse_recovery
        .actions
        .push(ParseRecoveryAction::TokenSkipped {
            token: SyntaxKind::EqualsGreaterThanToken,
            start: 0,
            length: 2,
            statement_start: 0,
            site: ParseTokenSkipSite::ListAbort,
        });
    assert!(!skipped.has_only_literal_or_missing_await_recovery());
}

#[test]
fn statement_missing_declarations_require_unique_retained_creation_events() {
    for text in [
        "@g<number> class C {}",
        "@g<number>",
        "{ @g<number> class C {} }",
        "namespace N { @g()<number> class C {} }",
        "switch (1) { case 1: @g<number> class C {} }",
        "switch (1) { default: @g<number> class C {} }",
        "/*😀*/ { @g<number> class C {} }",
    ] {
        let source = source(text);
        assert_coverage(&source.parse_diagnostics, source.parse_recovery());
        assert!(
            !source.has_only_literal_or_missing_await_recovery(),
            "{text}"
        );
        assert!(
            source.has_supported_emit_recovery(),
            "{text}: {:?}",
            source.parse_recovery()
        );
    }
    for text in [
        "x = @g<number> foo;",
        "{ @g<number> class C {}",
        "{ @g<number> class C {} } const x = ;",
    ] {
        let source = source(text);
        assert!(!source.has_supported_emit_recovery(), "{text}");
    }
    let mut missing = source("{ @g<number> class C {} }");
    let saved = missing.parse_recovery.clone();
    missing.parse_recovery.events[0]
        .missing_node
        .as_mut()
        .unwrap()
        .position += 1;
    assert!(!missing.has_supported_emit_recovery());
    missing.parse_recovery = saved.clone();
    missing.parse_recovery.events.push(saved.events[0]);
    assert!(!missing.has_supported_emit_recovery());
    missing.parse_recovery = saved;
    missing.parse_recovery.events[0].diagnostic_index = None;
    assert!(!missing.has_supported_emit_recovery());
}

#[test]
fn skipped_parameter_gaps_require_a_retained_missing_await_operand() {
    for text in [
        "async function f(a = await => await): Promise<void> {}",
        "async function f(a = await => b) {}",
        "async function f(a = await =>) {}",
        "class C { async m(a = await => await) {} }",
        "async function* g(a = await => await) {}",
        "export async function f(a = await => await) {}",
        "async function f(a = await => await) { await x; }",
        "async function f(a = await /*c*/ => /*d*/ await) {}",
        "/*😀*/ async function f(a = await /*😀*/ => await) {}",
        "async function f(a = await\n=> await) {}",
    ] {
        let source = source(text);
        assert_coverage(&source.parse_diagnostics, source.parse_recovery());
        assert!(!source.has_only_missing_node_emit_recovery(), "{text}");
        assert!(
            source.has_only_parameter_gap_emit_recovery(),
            "{text}: {:?}",
            source.parse_recovery()
        );
    }
    for text in [
        "async function f(a = await => await => await) {}",
        "async function f(a = await, => b) {}",
        "async function f() { const g = async (a = await => await) => {}; }",
        "async function f(a = await => await",
        "function f(a, => b) {}",
    ] {
        let source = source(text);
        assert!(
            !source.has_only_parameter_gap_emit_recovery(),
            "{text}: {:?}",
            source.parse_recovery()
        );
    }
    let clean = source("function f(a = await => await) {}");
    assert!(clean.parse_diagnostics.is_empty());
    assert!(clean.has_only_parameter_gap_emit_recovery());
}

#[test]
fn parameter_gap_admission_rejects_forged_or_unowned_recovery() {
    let source = source("async function f(a = await => await): Promise<void> {}");
    assert!(source.has_only_parameter_gap_emit_recovery());
    let check = |mutate: &dyn Fn(&mut ParseRecovery)| {
        let mut changed = source.clone();
        mutate(&mut changed.parse_recovery);
        assert!(
            !changed.has_only_parameter_gap_emit_recovery(),
            "{:?}",
            changed.parse_recovery()
        );
    };
    check(&|r| r.actions.push(r.actions[0]));
    check(&|r| {
        let ParseRecoveryAction::TokenSkipped { start, .. } = &mut r.actions[0] else {
            unreachable!()
        };
        *start += 1;
    });
    check(&|r| {
        let ParseRecoveryAction::TokenSkipped { length, .. } = &mut r.actions[0] else {
            unreachable!()
        };
        *length = 0;
    });
    check(&|r| {
        let ParseRecoveryAction::TokenSkipped { site, .. } = &mut r.actions[0] else {
            unreachable!()
        };
        *site = ParseTokenSkipSite::DelimitedNoProgress;
    });
    check(&|r| {
        r.actions
            .push(ParseRecoveryAction::Reparsed { start: 0, end: 10 })
    });
    check(&|r| {
        let missing = r
            .events
            .iter()
            .find_map(|event| event.missing_node)
            .unwrap();
        r.events
            .iter_mut()
            .find(|event| event.diagnostic_index.is_none())
            .unwrap()
            .missing_node = Some(missing);
    });
    check(&|r| {
        r.events
            .iter_mut()
            .find(|event| event.diagnostic_index.is_none())
            .unwrap()
            .start += 1
    });
    check(&|r| {
        r.events
            .iter_mut()
            .find(|event| event.missing_node.is_some())
            .unwrap()
            .diagnostic_index = None
    });
    check(&|r| r.events.retain(|event| event.missing_node.is_none()));
    check(&|r| {
        r.events
            .iter_mut()
            .find_map(|event| event.missing_node.as_mut())
            .unwrap()
            .position += 1
    });
}

#[test]
fn report_only_recovery_keeps_creation_full_start_through_deduplication() {
    let text = "/*😀*/ x /*gap*/ )";
    let mut parser = Parser::new("main.ts".into(), text, LanguageVariant::Standard, false);
    parser.next_token();
    parser.next_token();
    assert_eq!(parser.token(), SyntaxKind::CloseParenToken);
    assert!(!parser.parse_expected(SyntaxKind::CloseBracketToken, None));
    assert!(!parser.parse_expected(SyntaxKind::CloseBraceToken, None));
    let full_start = text[..text.find(" /*gap*/").unwrap()]
        .encode_utf16()
        .count() as u32;
    let token_start = text[..text.find(')').unwrap()].encode_utf16().count() as u32;
    assert_ne!(full_start, token_start);
    assert_eq!(parser.parse_diagnostics.len(), 1);
    assert_eq!(parser.parse_recovery.events.len(), 2);
    for (index, event) in parser.parse_recovery.events.iter().enumerate() {
        assert_eq!(event.full_start, full_start);
        assert_eq!(event.start, token_start);
        assert_eq!(event.missing_node, None);
        assert_eq!(event.diagnostic_index, (index == 0).then_some(0));
    }
    assert_coverage(&parser.parse_diagnostics, &parser.parse_recovery);
}

#[test]
fn missing_closer_report_full_start_identifies_the_retained_parenthesis_end() {
    let text = "export const value = (object?.x //😀\n as number);";
    let parsed = source(text);
    let full_start = text[..text.find(" //😀").unwrap()].encode_utf16().count() as u32;
    let token_start = text[..text.find("as number").unwrap()]
        .encode_utf16()
        .count() as u32;
    let events: Vec<_> = parsed
        .parse_recovery
        .events()
        .iter()
        .filter(|event| event.start == token_start && event.diagnostic_index.is_some())
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].kind,
        ParseRecoveryKind::Diagnostic(ParseDiagnosticOrigin::Parser)
    );
    assert_eq!(events[0].missing_node, None);
    assert_eq!(events[0].full_start, full_start);
    assert!(parsed.arena.nodes().iter().any(|node| {
        node.kind == SyntaxKind::ParenthesizedExpression
            && parsed.positions().byte_to_utf16(node.end) == Some(full_start)
    }));
    // The later missing-semicolon report targets the whole `number` node
    // after that node has been consumed. Its full start is the following `)`.
    let number_start = text[..text.find("number").unwrap()].encode_utf16().count() as u32;
    let number_end = number_start + 6;
    let range_event = parsed
        .parse_recovery
        .events()
        .iter()
        .find(|event| event.start == number_start && event.diagnostic_index.is_some())
        .unwrap();
    assert_eq!(range_event.length, 6);
    assert_eq!(range_event.full_start, number_end);
    assert!(range_event.start < range_event.full_start);
    assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
    assert!(!parsed.has_only_parameter_gap_emit_recovery());
    assert!(parsed.has_supported_emit_recovery());
}

#[test]
fn explicit_range_recovery_full_start_is_not_the_reported_node_start() {
    let text = "const v = a?.#b;";
    let parsed = source(text);
    let event = parsed
        .parse_recovery
        .events()
        .iter()
        .find(|event| event.diagnostic_index.is_some())
        .unwrap();
    assert_eq!((event.start, event.length, event.full_start), (13, 2, 15));
    assert_eq!(event.missing_node, None);
    assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
    assert!(!parsed.has_supported_emit_recovery());
}

#[test]
fn statement_gap_recovery_accounts_for_reports_and_their_reachable_owners() {
    for text in [
        "var foo = async (a = await => await): Promise<void> => {}",
        "let x = <void> =>;",
        "export const value = (object?.x //😀\n as number);",
        "export const value = (object?.x\n /* value */ as number);",
        "const value = (object.x\n as number);",
        "x = (a\n as T);",
        "{ const value = (a\n as number); }",
        "namespace N { export const value = (1\n as number); }",
        "switch (x) { case 1: const value = (a\n as number); break; default: x = (b\n as T); }",
        "/*😀*/ var foo = async (a = await => await): Promise<void> => {}",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
        assert!(!parsed.has_only_parameter_gap_emit_recovery(), "{text}");
        assert!(
            parsed.has_supported_emit_recovery(),
            "{text}: {:?}",
            parsed.parse_recovery()
        );
    }
    for text in [
        "const value = (a /*c*/",
        "foo(a, b",
        "const x = [a, b",
        "const x = {a: 1",
        "let x = 1 let y = 2;",
        "let x = 1, (y);",
        "let x = 1 \"s\";",
        "const value = a?.#b;",
        "export {}; await (1,);",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(
            !parsed.has_supported_emit_recovery(),
            "{text}: {:?}",
            parsed.parse_recovery()
        );
    }
    for text in [
        "const value = (a as T);",
        "const value = (a /*c*/ as T);",
        "let x = 1 <T>y;",
    ] {
        let parsed = source(text);
        assert!(parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(parsed.has_supported_emit_recovery(), "{text}");
    }
}

#[test]
fn statement_gap_recovery_rejects_unowned_or_forged_events() {
    let parsed = source("export const value = (object?.x //😀\n as number);");
    assert!(parsed.has_supported_emit_recovery());
    let check = |mutate: &dyn Fn(&mut ParseRecovery)| {
        let mut changed = parsed.clone();
        mutate(&mut changed.parse_recovery);
        assert!(
            !changed.has_supported_emit_recovery(),
            "{:?}",
            changed.parse_recovery()
        );
    };
    check(&|r| {
        r.events.remove(0);
    });
    check(&|r| r.events.push(r.events[0]));
    check(&|r| r.actions.push(r.actions[0]));
    check(&|r| {
        r.actions
            .push(ParseRecoveryAction::Reparsed { start: 0, end: 1 })
    });
    check(&|r| {
        r.events
            .iter_mut()
            .find(|e| e.diagnostic_index.is_none())
            .unwrap()
            .start += 1
    });
    check(&|r| {
        let start = match r.actions[0] {
            ParseRecoveryAction::TokenSkipped { start, .. } => start,
            _ => unreachable!(),
        };
        r.events
            .iter_mut()
            .find(|e| e.start == start && e.diagnostic_index.is_some())
            .unwrap()
            .full_start -= 1;
    });
    check(&|r| {
        r.events[0].length = 0;
    });
    check(&|r| {
        let event = r
            .events
            .iter_mut()
            .find(|e| e.diagnostic_index.is_none())
            .unwrap();
        event.missing_node = Some(MissingNodeRecovery {
            kind: SyntaxKind::Identifier,
            position: event.full_start,
        });
    });
}
