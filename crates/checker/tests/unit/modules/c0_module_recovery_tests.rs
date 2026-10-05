use tsc_binder::bind_source_file;
use tsc_syntax::{
    parse_source_file, LanguageVariant, NodeData, ParseOptions, SourceFile, SyntaxKind,
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
                .get_target_of_alias_declaration(import)
                .expect("valid target lookup")
                .is_some());
            let live_type = state
                .get_type_of_alias(live_symbol)
                .expect("valid alias type");
            assert!(!state.tables.is_error_type(live_type));

            assert_eq!(
                state
                    .get_target_of_alias_declaration(root)
                    .expect("unexpected declaration has no alias target"),
                None
            );

            let number = state.tables.intrinsics.number;
            let target = state.binder.create_symbol(
                SymbolFlags::PROPERTY,
                tsc_types::EscapedName::from_escaped_value(("target".to_owned()).into()),
            );
            state
                .links
                .set_fresh_symbol_type(target, LinkSlot::Resolved(number));
            let recovered = state.binder.create_symbol(
                SymbolFlags::ALIAS,
                tsc_types::EscapedName::from_escaped_value(("Recovered".to_owned()).into()),
            );
            state.binder.symbol_mut(recovered).declarations = vec![root].into();
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
    let recovered = state.binder.create_symbol(
        SymbolFlags::ALIAS,
        tsc_types::EscapedName::from_escaped_value(("Recovered".to_owned()).into()),
    );

    assert_eq!(
        state
            .resolve_alias(recovered)
            .expect("declarationless alias resolves to the miss sentinel"),
        state.unknown_symbol
    );
    assert_eq!(
        *state.links.symbol_cold().alias_target.get(recovered),
        LinkSlot::Resolved(state.unknown_symbol)
    );
    assert_eq!(
        state
            .get_immediate_aliased_symbol(recovered)
            .expect("declarationless immediate target is absent"),
        None
    );
    assert_eq!(
        *state.links.symbol_cold().immediate_target.get(recovered),
        Some(None)
    );

    state
        .mark_alias_symbol_as_referenced(recovered)
        .expect("declarationless alias can still be marked referenced");
    assert!(*state.links.symbol_cold().alias_referenced.get(recovered));
}
