use tsc_binder::bind_source_file;
use tsc_syntax::nodes::ElementAccessExpressionData;
use tsc_syntax::{
    parse_source_file, LanguageVariant, NodeData, ParseOptions, SourceFile, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, SymbolFlags};

use crate::state::CheckerState;

fn parse(text: &str) -> SourceFile {
    let source = parse_source_file(
        "annotation-recovery.ts".to_owned(),
        text.to_owned(),
        ParseOptions {
            language_variant: LanguageVariant::Standard,
            javascript_file: false,
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
fn declarationless_type_alias_caches_error_while_valid_alias_keeps_its_type() {
    let source = parse("type Live = string;\n");
    let options = CompilerOptions::default();
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);

    let live = state
        .resolve_file_scope_name("Live", SymbolFlags::TYPE_ALIAS)
        .expect("valid alias sibling");
    assert_eq!(
        state
            .get_declared_type_of_type_alias(live)
            .expect("valid alias resolves"),
        state.tables.intrinsics.string
    );

    let recovered = state
        .binder
        .create_symbol(SymbolFlags::TYPE_ALIAS, "Recovered".to_owned());
    let error = state
        .get_declared_type_of_type_alias(recovered)
        .expect("declarationless recovery alias resolves to errorType");
    assert!(state.tables.is_error_type(error));
    assert_eq!(
        state
            .get_declared_type_of_type_alias(recovered)
            .expect("recovery alias is cached"),
        error
    );
}

#[test]
fn unexpected_signature_uses_cached_unknown_signature_beside_a_valid_signature() {
    let source = parse("type Live = (value: string) => number;\n");
    let options = CompilerOptions::default();
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);
    let function_type = source
        .arena
        .node_ids()
        .find(|&node| source.arena.node(node).kind == SyntaxKind::FunctionType)
        .expect("valid signature sibling");
    let live = state
        .get_signature_from_declaration(function_type)
        .expect("valid signature");
    assert_ne!(live, state.unknown_signature);

    let root = source.root;
    let recovered = state
        .get_signature_from_declaration(root)
        .expect("unexpected declaration recovers");
    assert_eq!(recovered, state.unknown_signature);
    assert_eq!(
        state
            .get_signature_from_declaration(root)
            .expect("unknown signature is cached"),
        recovered
    );
}

#[test]
fn missing_late_bindable_argument_is_error_type_beside_a_valid_element_name() {
    let mut source = parse("obj[\"live\"];\n");
    let valid = source
        .arena
        .node_ids()
        .find(|&node| source.arena.node(node).kind == SyntaxKind::ElementAccessExpression)
        .expect("valid element access sibling");
    let missing = source.arena.alloc_node(
        NodeData::ElementAccessExpression(ElementAccessExpressionData {
            expression: None,
            question_dot_token: None,
            argument_expression: None,
        }),
        0,
        0,
        NodeFlags::NONE,
    );
    let options = CompilerOptions::default();
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);

    let live = state
        .late_bindable_name_type(valid)
        .expect("valid element name");
    assert!(!state.tables.is_error_type(live));
    let recovered = state
        .late_bindable_name_type(missing)
        .expect("missing element argument recovers");
    assert!(state.tables.is_error_type(recovered));
}

#[test]
fn malformed_annotation_inputs_resolve_to_error_type_without_unwinding() {
    let source = parse("const live = 1;\n");
    let options = CompilerOptions::default();
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);

    let declarationless = state
        .binder
        .create_symbol(SymbolFlags::VARIABLE, "Recovered".to_owned());
    let recovered_symbol_type = state
        .get_type_of_variable_or_parameter_or_property(declarationless)
        .expect("declarationless variable/property symbol recovers");
    assert!(state.tables.is_error_type(recovered_symbol_type));
    assert_eq!(
        state
            .get_type_of_variable_or_parameter_or_property(declarationless)
            .expect("recovery result is cached"),
        recovered_symbol_type
    );

    let recovered_binding_type = state
        .get_type_from_binding_element(source.root, false, true)
        .expect("non-binding element recovers");
    assert!(state.tables.is_error_type(recovered_binding_type));
}
