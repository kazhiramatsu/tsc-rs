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
fn missing_variable_type_annotations_keep_other_type_and_expression_slots_closed() {
    for text in [
        "export let x: = 1;",
        "export let x: ;",
        "let x: = 1; export {};",
        "export const x: = 1;",
        "export var x: ;",
        "export let a: , b: = 2;",
        "export let x: /*c*/ = 1;",
        "export let x: //c\n = 1;",
        "namespace N { export let x: = 1; }",
        "export {}; { using x: = f(); }",
        "export async function h() { await using x: = f(); }",
        "declare let d: ; export declare let e: ;",
        "export let x!: = 1;",
        "export function* g() { let x: = 1; yield x; }",
        "export async function h() { let x: = 1; await 0; return x; }",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
        assert!(!parsed.has_only_statement_gap_emit_recovery(), "{text}");
        assert!(parsed.has_supported_emit_recovery(), "{text}");
    }
    for text in [
        "function f(x: ) {}",
        "class C { x: ; }",
        "let x: A<,> = 1;",
        "let x: A. = 1;",
        "let x: () = 1;",
        "let x: | = 1;",
        "let x = ;",
        "for (let x: ; ;) {}",
        "let {x}: = source;",
        "let x: = 1; const y = ;",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(!parsed.has_supported_emit_recovery(), "{text}");
    }
}

#[test]
fn missing_variable_type_events_keep_full_start_distinct_from_diagnostic_start() {
    let parsed = source("export let x: /*c*/ = 1;");
    assert!(parsed.has_supported_emit_recovery());
    assert_eq!(parsed.parse_recovery.events.len(), 1);
    assert_eq!(parsed.parse_recovery.events[0].start, 20);
    assert_eq!(
        parsed.parse_recovery.events[0].missing_node,
        Some(MissingNodeRecovery {
            kind: SyntaxKind::Identifier,
            position: 13,
        })
    );
    let mut shifted = parsed.clone();
    shifted.parse_recovery.events[0]
        .missing_node
        .as_mut()
        .unwrap()
        .position = 20;
    assert!(!shifted.has_supported_emit_recovery());
    let mut duplicated = parsed.clone();
    duplicated
        .parse_recovery
        .events
        .push(parsed.parse_recovery.events[0]);
    assert!(!duplicated.has_supported_emit_recovery());
    let mut unpaired = parsed;
    unpaired.parse_recovery.events[0].diagnostic_index = None;
    assert!(!unpaired.has_supported_emit_recovery());
}

#[test]
fn missing_variable_delimiters_require_retained_named_statement_declarations() {
    for text in [
        "export const a number = \"missing colon\";",
        "export const a string = \"missing colon\";",
        "let a b = 1;",
        "let a = 1 b = 2;",
        "let a b c = 1;",
        "let a, b c;",
        "let a /*c*/ b = 1;",
        "var a /*a*/ /*b*/ b = 1;",
        "namespace N { export let a b = 1; }",
        "declare let a b;",
        "let a: = 1 b = 2;",
        "let a: number b = 2;",
        "let é 漢字 = 1;",
        "using a b = f();",
        "async function f() { await using a b = g(); }",
        "using a = f() b = f();",
        "async function f() { await using a = g() b = g(); }",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
        assert!(!parsed.has_only_statement_gap_emit_recovery(), "{text}");
        assert!(parsed.has_supported_emit_recovery(), "{text}");
    }
    for text in [
        "for (let a b = 0;;) {}",
        "for (let a b of xs) {}",
        "for (let a b in o) {}",
        "let a {b} = o;",
        "let {a} b = o;",
        "let a [b] = o;",
        "for (using a b of xs) {}",
        "let a = f(x y);",
        "let a = {x: 1 y: 2};",
        "let a = [1 2];",
        "function f(a b) {}",
        "let a 1;",
        "enum E { a b }",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(!parsed.has_supported_emit_recovery(), "{text}");
    }
    let asi = source("let a\nb = 1;");
    assert!(asi.parse_diagnostics.is_empty());
    assert!(asi.has_supported_emit_recovery());
}

#[test]
fn missing_variable_delimiter_reports_keep_token_spans_and_diagnostic_ownership() {
    let parsed = source("let a /*c*/ b = 1;");
    assert!(parsed.has_supported_emit_recovery());
    assert_eq!(parsed.parse_recovery.events.len(), 1);
    let mut shifted = parsed.clone();
    shifted.parse_recovery.events[0].full_start += 1;
    assert!(!shifted.has_supported_emit_recovery());
    let mut duplicated = parsed.clone();
    duplicated
        .parse_recovery
        .events
        .push(parsed.parse_recovery.events[0]);
    assert!(!duplicated.has_supported_emit_recovery());
    let mut unpaired = parsed.clone();
    unpaired.parse_recovery.events[0].diagnostic_index = None;
    assert!(!unpaired.has_supported_emit_recovery());
    let mut wrong_message = parsed;
    wrong_message.parse_diagnostics[0].message.text = "';' expected.".into();
    assert!(!wrong_message.has_supported_emit_recovery());
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
    assert!(
        clean.parse_diagnostics.is_empty(),
        "{:?}",
        clean.parse_diagnostics
    );
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
        "let x = <void>;",
        "foo bar;",
        "a b c;",
        "foo /*😀*/ bar;",
        "functon f();",
        "(a\nb);",
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
            parsed.has_only_statement_gap_emit_recovery(),
            "{text}: {:?}",
            parsed.parse_recovery()
        );
    }
    for text in [
        "functon f() {}",
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
            !parsed.has_only_statement_gap_emit_recovery(),
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
        assert!(parsed.has_only_statement_gap_emit_recovery(), "{text}");
    }
}

#[test]
fn statement_gap_recovery_rejects_unowned_or_forged_events() {
    let parsed = source("export const value = (object?.x //😀\n as number);");
    assert!(parsed.has_only_statement_gap_emit_recovery());
    let check = |mutate: &dyn Fn(&mut ParseRecovery)| {
        let mut changed = parsed.clone();
        mutate(&mut changed.parse_recovery);
        assert!(
            !changed.has_only_statement_gap_emit_recovery(),
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

#[test]
fn nested_parenthesis_recovery_requires_a_direct_chain_and_owned_closing_tokens() {
    for text in [
        "export const z = ((x //c\n as number));",
        "export const z = (((x //c\n as number)));",
        // Both missing parens end before the first as; the later closers
        // belong to two independently owned statement gaps.
        "export const z = ((x\n as number)\n as string);",
        "foo()\n))",
        "(x //c\n as number)) /*m*/ );",
        "export const z = ((x) //c\n as number);",
        "export const z = ((x /*c*/\n as number));",
        "export const z = x; )))\n",
        "{ const z = ((x //😀\n as number) /*😀*/ ); }",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
        assert!(parsed.has_only_statement_gap_emit_recovery(), "{text}");
        assert!(parsed.has_supported_emit_recovery(), "{text}");
    }
    for text in [
        "export const z = (<any>(x //c\n as number));",
        "(a, (x //c\n as number))",
        "export const z = x; )]\n",
        "));",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(!parsed.has_only_statement_gap_emit_recovery(), "{text}");
        assert!(!parsed.has_supported_emit_recovery(), "{text}");
    }
}

#[test]
fn nested_parenthesis_recovery_rejects_forged_chains_and_skip_runs() {
    let parsed = source("export const z = ((x //c\n as number));");
    assert!(parsed.has_only_statement_gap_emit_recovery());
    let check = |mutate: &dyn Fn(&mut ParseRecovery)| {
        let mut changed = parsed.clone();
        mutate(&mut changed.parse_recovery);
        assert!(!changed.has_only_statement_gap_emit_recovery());
        assert!(!changed.has_supported_emit_recovery());
    };
    check(&|r| {
        let index = r
            .events
            .iter()
            .position(|event| {
                event.diagnostic_index.is_none() && event.full_start == r.events[0].full_start
            })
            .unwrap();
        r.events.remove(index);
    });
    check(&|r| {
        let event = *r
            .events
            .iter()
            .find(|event| {
                event.diagnostic_index.is_none() && event.full_start == r.events[0].full_start
            })
            .unwrap();
        r.events.push(event);
    });
    check(&|r| {
        let full_start = r.events[0].full_start;
        r.events
            .iter_mut()
            .find(|event| event.diagnostic_index.is_none() && event.full_start == full_start)
            .unwrap()
            .full_start += 1;
    });
    check(&|r| r.actions.reverse());
    check(&|r| r.actions.push(r.actions[0]));
    check(&|r| {
        if let ParseRecoveryAction::TokenSkipped { start, .. } = &mut r.actions[1] {
            *start -= 1;
        }
    });
    check(&|r| {
        if let ParseRecoveryAction::TokenSkipped { token, .. } = &mut r.actions[1] {
            *token = SyntaxKind::CloseBracketToken;
        }
    });
    let mut mixed = source("export const z = x; )]\n");
    for action in &mut mixed.parse_recovery.actions {
        if let ParseRecoveryAction::TokenSkipped { token, .. } = action {
            *token = SyntaxKind::CloseParenToken;
        }
    }
    assert!(!mixed.has_only_statement_gap_emit_recovery());
    assert!(!mixed.has_supported_emit_recovery());
}

#[test]
fn decorator_await_consumption_has_a_typed_token_skip_and_missing_head() {
    for text in [
        "export {}; @await class C {}",
        "export {}; @ /*😀*/await(1) class C {}",
        "export {}; class C { @await method() {} }",
        "async function f() { @ /*😀*/await.foo(1) class C {} }",
    ] {
        let parsed = source(text);
        assert_coverage(&parsed.parse_diagnostics, parsed.parse_recovery());
        let skips: Vec<_> = parsed
            .parse_recovery()
            .actions()
            .iter()
            .filter_map(|action| match action {
                ParseRecoveryAction::TokenSkipped {
                    token,
                    start,
                    length,
                    site: ParseTokenSkipSite::DecoratorAwait,
                    ..
                } => Some((*token, *start, *length)),
                _ => None,
            })
            .collect();
        let start = text[..text.find("await").unwrap()].encode_utf16().count() as u32;
        assert_eq!(skips, [(SyntaxKind::AwaitKeyword, start, 5)], "{text}");
        let events: Vec<_> = parsed
            .parse_recovery()
            .events()
            .iter()
            .filter(|event| event.start == start && event.missing_node.is_some())
            .collect();
        assert_eq!(events.len(), 1, "{text}");
        let event = events[0];
        assert!(event.diagnostic_index.is_some());
        let missing = event.missing_node.unwrap();
        assert_eq!(missing.kind, SyntaxKind::Identifier);
        assert_eq!(missing.position, event.full_start);
        let full_start = parsed.positions().utf16_to_byte(event.full_start).unwrap();
        let token_start = crate::skip_trivia(text, full_start as usize);
        assert_eq!(
            parsed.positions().byte_to_utf16(token_start as u32),
            Some(start)
        );
        if text.contains("/*😀*/") {
            assert!(event.full_start < start);
        }
        assert!(!parsed.has_only_parameter_gap_emit_recovery());
    }
}

#[test]
fn clean_decorator_await_identifiers_do_not_acquire_skip_provenance() {
    for text in [
        "@await class C {}",
        "@await(1) class C {}",
        "export {}; @dec class C {}",
    ] {
        let parsed = source(text);
        assert!(parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(parsed
            .parse_recovery()
            .actions()
            .iter()
            .all(|action| !matches!(
                action,
                ParseRecoveryAction::TokenSkipped {
                    site: ParseTokenSkipSite::DecoratorAwait,
                    ..
                }
            )));
    }
}

#[test]
fn context_recovery_requires_owned_runs_expression_slots_and_heritage_tiles() {
    let original = r#"export {};

// reparse call as invalid await should error
await (1,);
await <number, string>(1);

// reparse tagged template as invalid await should error
await <number, string> ``;

// reparse class extends clause should fail
class C extends await<string> {
}

// await in class decorators should fail
@(await)
class C1 {}

@await(x)
class C2 {}

@await
class C3 {}

// await in member decorators should fail
class C4 {
    @await
    ["foo"]() {}
}
class C5 {
    @await(1)
    ["foo"]() {}
}
class C6 {
    @(await)
    ["foo"]() {}
}

// await in parameter decorators should fail
class C7 {
    method1(@await [x]) {}
    method2(@await(1) [x]) {}
    method3(@(await) [x]) {}
}
"#;
    for text in [
        original,
        "export {}; await (1,);",
        "(1,);",
        "export {}; await <number, string>(1);",
        "export {}; await <number, string> ``;",
        "export {}; class C extends await<string> {}",
        "export {}; @await class C {}",
        "export {}; @ /*😀*/await(1) class C {}",
        "export {}; @(await) class C {}",
        "export {}; class C { @await ['a']() {} @await(1) ['b']() {} }",
        "export {}; class C { m(@await x: any) {} n(@await(1) x: any) {} }",
        "export {}; await (1,); const stable = 1; @await class C {} const after = 2;",
        "async function f() { @ /*😀*/await.foo(1) class C {} }",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(!parsed.has_only_statement_gap_emit_recovery(), "{text}");
        assert!(
            parsed.has_supported_emit_recovery(),
            "{text}: {:?}",
            parsed.parse_recovery()
        );
    }
    for text in [
        "export {}; class C extends await {}",
        "export {}; await (1 +);",
        "export {}; @) class D {}",
        "export {}; await <number, string>(1",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(
            !parsed.has_supported_emit_recovery(),
            "{text}: {:?}",
            parsed.parse_recovery()
        );
    }
}

#[test]
fn context_recovery_refuses_forged_decorator_and_reparse_witnesses() {
    let parsed = source("export {}; @ /*😀*/await(1) class C {}");
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
        r.actions.retain(|a| {
            !matches!(
                a,
                ParseRecoveryAction::TokenSkipped {
                    site: ParseTokenSkipSite::DecoratorAwait,
                    ..
                }
            )
        })
    });
    check(&|r| {
        let skip = *r
            .actions
            .iter()
            .find(|a| {
                matches!(
                    a,
                    ParseRecoveryAction::TokenSkipped {
                        site: ParseTokenSkipSite::DecoratorAwait,
                        ..
                    }
                )
            })
            .unwrap();
        r.actions.push(skip);
    });
    check(&|r| {
        for a in &mut r.actions {
            if let ParseRecoveryAction::TokenSkipped { token, .. } = a {
                *token = SyntaxKind::YieldKeyword;
            }
        }
    });
    check(&|r| {
        for a in &mut r.actions {
            if let ParseRecoveryAction::Reparsed { end, .. } = a {
                *end -= 1;
            }
        }
    });
    check(&|r| {
        r.actions
            .retain(|action| !matches!(action, ParseRecoveryAction::Reparsed { .. }));
    });
    check(&|r| {
        r.events[0].missing_node.as_mut().unwrap().position = r.events[0].start;
    });
    check(&|r| {
        r.events[0].full_start += 1;
    });
    check(&|r| {
        r.events[0].diagnostic_index = None;
    });
    let mut assertion = source("export {}; await <number, string>(1);");
    assert!(assertion.has_supported_emit_recovery());
    assertion
        .parse_recovery
        .events
        .iter_mut()
        .find(|e| e.missing_node.is_some())
        .unwrap()
        .full_start += 1;
    assert!(!assertion.has_supported_emit_recovery());
}

#[test]
fn context_recovery_covers_decorator_wrappers_and_trivia_but_not_overlapping_tails() {
    for text in [
        "export {}; @await! class C {}",
        "export {}; @await`x` class C {}",
        "export {}; @await<T>() class C {}",
        "export {}; @await<T>`x` class C {}",
        "export {}; @await?.x class C {}",
        "export {}; @await?.[x] class C {}",
        "export {}; class C implements await<string> {}",
        "export {}; class C extends /*😀*/await<string /*😀*/> {}",
        "(1, /*😀*/);",
        "export {}; await <number /*😀*/, string>(1);",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(
            parsed.has_supported_emit_recovery(),
            "{text}: {:?}",
            parsed.parse_recovery()
        );
    }
    let clean = source("export {}; interface I extends await<string> {}");
    assert!(clean.parse_diagnostics.is_empty());
    assert!(clean.has_supported_emit_recovery());
    for text in [
        "export {}; class C extends await<A>, B {}",
        "export {}; class C implements A, await<B> {}",
        "export {}; 1, ;",
        "export {}; await (1,",
        "export {}; let a = await /1; b; c; x/;",
        "export {}; let a = await /1; b; c; x/; await (1,);",
    ] {
        let parsed = source(text);
        assert!(!parsed.parse_diagnostics.is_empty(), "{text}");
        assert!(
            !parsed.has_supported_emit_recovery(),
            "{text}: {:?}",
            parsed.parse_recovery()
        );
    }
}

#[test]
fn context_recovery_refuses_truncated_runs_and_unowned_heritage_reports() {
    let mut parsed = source("export {}; await (1,); @await class C {}");
    assert!(parsed.has_supported_emit_recovery());
    let NodeData::SourceFile(data) = &parsed.arena.node(parsed.root).data else {
        unreachable!()
    };
    let statements = &parsed.arena.node_array(data.statements.unwrap()).nodes;
    let end = parsed
        .positions()
        .byte_to_utf16(parsed.arena.node(statements[1]).end)
        .unwrap();
    let action = parsed
        .parse_recovery
        .actions
        .iter_mut()
        .find(|action| matches!(action, ParseRecoveryAction::Reparsed { .. }))
        .unwrap();
    let ParseRecoveryAction::Reparsed { end: run_end, .. } = action else {
        unreachable!()
    };
    *run_end = end;
    assert!(!parsed.has_supported_emit_recovery());

    let mut parsed = source("export {}; class C extends await<string> {}");
    assert!(parsed.has_supported_emit_recovery());
    let start = parsed
        .parse_recovery
        .actions
        .iter()
        .find_map(|action| match action {
            ParseRecoveryAction::TokenSkipped {
                start,
                site: ParseTokenSkipSite::ListAbort,
                ..
            } => Some(*start),
            _ => None,
        })
        .unwrap();
    parsed
        .parse_recovery
        .events
        .iter_mut()
        .find(|event| event.start == start && event.diagnostic_index.is_some())
        .unwrap()
        .diagnostic_index = None;
    assert!(!parsed.has_supported_emit_recovery());
}
