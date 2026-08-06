use tsc_binder::bind_source_file;
use tsc_syntax::{
    parse_source_file, LanguageVariant, NodeData, NodeId, ParseOptions, SourceFile, SyntaxKind,
};
use tsc_types::{CompilerOptions, SymbolFlags};

use crate::links::LinkSlot;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn parse_js(text: &str) -> SourceFile {
    let source = parse_source_file(
        "commonjs-recovery.js".to_owned(),
        text.to_owned(),
        ParseOptions {
            language_variant: LanguageVariant::Standard,
            javascript_file: true,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
    source
}

fn property_access_with_text(source: &SourceFile, expected: &str) -> NodeId {
    source
        .arena
        .node_ids()
        .find(|&node| {
            let raw = source.arena.node(node);
            let start = tsc_syntax::skip_trivia(source.text(), raw.pos as usize);
            raw.kind == SyntaxKind::PropertyAccessExpression
                && &source.text()[start..raw.end as usize] == expected
        })
        .unwrap_or_else(|| panic!("property access {expected:?}"))
}

#[test]
fn common_js_flow_recovery_values_leave_the_ordinary_flow_query_live() {
    let source = parse_js("obj.x = 1;\nexports.x = 1;\nexports.x = 2;\n");
    let obj_access = property_access_with_text(&source, "obj.x");
    let exports_accesses = source
        .arena
        .node_ids()
        .filter(|&node| {
            let raw = source.arena.node(node);
            let start = tsc_syntax::skip_trivia(source.text(), raw.pos as usize);
            raw.kind == SyntaxKind::PropertyAccessExpression
                && &source.text()[start..raw.end as usize] == "exports.x"
        })
        .collect::<Vec<_>>();
    assert_eq!(exports_accesses.len(), 2);
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);

    let declarationless = state
        .binder
        .create_symbol(SymbolFlags::PROPERTY, "none".to_owned());
    let missing_source = state
        .get_flow_type_from_common_js_export(declarationless)
        .expect("declarationless export recovers");
    assert!(state.tables.is_error_type(missing_source));

    let non_exports = state
        .binder
        .create_symbol(SymbolFlags::PROPERTY, "x".to_owned());
    state.binder.symbol_mut(non_exports).declarations = vec![obj_access];
    assert_eq!(
        state
            .get_flow_type_from_common_js_export(non_exports)
            .expect("synthetic exports reference cannot match"),
        state.tables.intrinsics.undefined
    );

    let ordinary = state
        .binder
        .create_symbol(SymbolFlags::PROPERTY, "x".to_owned());
    state.binder.symbol_mut(ordinary).declarations = exports_accesses;
    let invocations = state.flow_invocation_count;
    let result = state
        .get_flow_type_from_common_js_export(ordinary)
        .expect("ordinary exports flow remains live");
    assert!(!state.tables.is_error_type(result));
    assert!(
        state.flow_invocation_count > invocations,
        "the ordinary sibling must enter the flow walker"
    );
}

#[test]
fn missing_common_js_end_flow_returns_auto_without_starting_a_flow_walk() {
    let source = parse_js("exports.x = 1;\n");
    let access = property_access_with_text(&source, "exports.x");
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    let mut binder = bind_source_file(&source, &options);
    assert!(
        binder.node_end_flow.remove(&source.root).is_some(),
        "valid sibling normally has a source-file end flow"
    );
    let mut state = CheckerState::new(&source, &binder, &options);
    let symbol = state
        .binder
        .create_symbol(SymbolFlags::PROPERTY, "x".to_owned());
    state.binder.symbol_mut(symbol).declarations = vec![access];
    let invocations = state.flow_invocation_count;
    assert_eq!(
        state
            .get_flow_type_from_common_js_export(symbol)
            .expect("missing end flow uses getFlowTypeOfReference fallback"),
        state.tables.intrinsics.auto
    );
    assert_eq!(state.flow_invocation_count, invocations);
}

#[test]
fn malformed_alias_declarations_have_no_target_and_keep_resolved_value_fallback() {
    let text = "namespace N { export const value = 1; }\nimport Live = N;\nexport {};\n";
    with_program_state(
        &[("alias-recovery.ts", text)],
        &CompilerOptions::default(),
        |state| {
            let root = state.binder.source(0).root;
            let import = match state.data_of(root) {
                NodeData::SourceFile(data) => state
                    .nodes_of(data.statements)
                    .into_iter()
                    .find(|&node| state.kind_of(node) == SyntaxKind::ImportEqualsDeclaration)
                    .expect("valid alias declaration sibling"),
                _ => panic!("root is SourceFile"),
            };
            let live_symbol = state
                .get_symbol_of_declaration(import)
                .expect("valid alias symbol");
            assert!(state
                .get_target_of_alias_declaration(import, false)
                .expect("valid target lookup")
                .is_some());
            let live_type = state
                .get_type_of_alias(live_symbol)
                .expect("valid alias type");
            assert!(!state.tables.is_error_type(live_type));

            assert_eq!(
                state
                    .get_target_of_alias_declaration(root, false)
                    .expect("unexpected declaration has no alias target"),
                None
            );

            let number = state.tables.intrinsics.number;
            let target = state
                .binder
                .create_symbol(SymbolFlags::PROPERTY, "target".to_owned());
            state
                .links
                .set_fresh_symbol_type(target, LinkSlot::Resolved(number));
            let recovered = state
                .binder
                .create_symbol(SymbolFlags::ALIAS, "Recovered".to_owned());
            state.binder.symbol_mut(recovered).declarations = vec![root];
            state
                .links
                .set_fresh_symbol_alias_target(recovered, LinkSlot::Resolved(target));
            assert_eq!(
                state
                    .get_type_of_alias(recovered)
                    .expect("malformed declaration retains resolved target fallback"),
                number
            );
        },
    );
}

#[test]
fn declarationless_recovery_alias_uses_stable_miss_sentinels() {
    let source = parse_js("export {};\n");
    let options = CompilerOptions::default();
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);
    let recovered = state
        .binder
        .create_symbol(SymbolFlags::ALIAS, "Recovered".to_owned());

    assert_eq!(
        state
            .resolve_alias(recovered)
            .expect("declarationless alias resolves to the miss sentinel"),
        state.unknown_symbol
    );
    assert_eq!(
        state.links.symbol(recovered).alias_target,
        LinkSlot::Resolved(state.unknown_symbol)
    );
    assert_eq!(
        state
            .get_immediate_aliased_symbol(recovered)
            .expect("declarationless immediate target is absent"),
        None
    );
    assert_eq!(state.links.symbol(recovered).immediate_target, Some(None));

    state
        .mark_alias_symbol_as_referenced(recovered)
        .expect("declarationless alias can still be marked referenced");
    assert!(state.links.symbol(recovered).alias_referenced);
}
