use tsc_binder::bind_source_file;
use tsc_syntax::nodes::{
    EmptyStatementData, JsxAttributeData, JsxAttributesData, JsxSelfClosingElementData,
    JsxSpreadAttributeData,
};
use tsc_syntax::{parse_source_file, LanguageVariant, NodeData, ParseOptions, SyntaxKind};
use tsc_types::{CheckMode, CompilerOptions, NodeFlags};

use super::JsxReferenceKind;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

#[test]
fn synthetic_attribute_recovery_preserves_a_bound_sibling() {
    let mut source = parse_source_file(
        "jsx-recovery.tsx".to_owned(),
        "declare namespace JSX { interface Element {} interface IntrinsicElements { x: {} } }\n\
         declare namespace Ns { const Tag: any; }\n\
         (<x live />);\n\
         (<Ns.Tag />);\n"
            .to_owned(),
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            javascript_file: false,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(source.parse_diagnostics.is_empty());

    let (tag_name, valid_attribute, value_tag_opening) = {
        let mut intrinsic = None;
        let mut value = None;
        for node in source.arena.node_ids() {
            let NodeData::JsxSelfClosingElement(data) = &source.arena.node(node).data else {
                continue;
            };
            let Some(tag_name) = data.tag_name else {
                continue;
            };
            if source.arena.node(tag_name).kind == SyntaxKind::Identifier {
                let valid_attribute = data
                    .attributes
                    .and_then(|attributes| match &source.arena.node(attributes).data {
                        NodeData::JsxAttributes(data) => data.properties,
                        _ => None,
                    })
                    .and_then(|properties| {
                        source.arena.node_array(properties).nodes.first().copied()
                    })
                    .expect("bound valid attribute");
                intrinsic = Some((tag_name, valid_attribute));
            } else if source.arena.node(tag_name).kind == SyntaxKind::PropertyAccessExpression {
                value = Some(node);
            }
        }
        let (tag_name, valid_attribute) = intrinsic.expect("intrinsic opening");
        (
            tag_name,
            valid_attribute,
            value.expect("value-tag opening sibling"),
        )
    };

    // These nodes are deliberately detached from the parsed root:
    // the binder therefore leaves the named attribute unbound,
    // while the parsed `live` sibling retains its real symbol.
    let unbound_attribute = source.arena.alloc_node(
        NodeData::JsxAttribute(JsxAttributeData {
            name: None,
            initializer: None,
        }),
        0,
        0,
        NodeFlags::NONE,
    );
    let unknown_attribute = source.arena.alloc_node(
        NodeData::EmptyStatement(EmptyStatementData {}),
        0,
        0,
        NodeFlags::NONE,
    );
    let missing_spread = source.arena.alloc_node(
        NodeData::JsxSpreadAttribute(JsxSpreadAttributeData { expression: None }),
        0,
        0,
        NodeFlags::NONE,
    );
    let synthetic_properties = source.arena.alloc_synthetic_array(vec![
        valid_attribute,
        unbound_attribute,
        unknown_attribute,
        missing_spread,
    ]);
    let synthetic_attributes = source.arena.alloc_node(
        NodeData::JsxAttributes(JsxAttributesData {
            properties: Some(synthetic_properties),
        }),
        0,
        0,
        NodeFlags::NONE,
    );
    let synthetic_opening = source.arena.alloc_node(
        NodeData::JsxSelfClosingElement(JsxSelfClosingElementData {
            tag_name: Some(tag_name),
            type_arguments: None,
            attributes: Some(synthetic_attributes),
        }),
        0,
        0,
        NodeFlags::NONE,
    );
    let attributes_missing_opening = source.arena.alloc_node(
        NodeData::JsxSelfClosingElement(JsxSelfClosingElementData {
            tag_name: Some(tag_name),
            type_arguments: None,
            attributes: None,
        }),
        0,
        0,
        NodeFlags::NONE,
    );
    let tag_missing_opening = source.arena.alloc_node(
        NodeData::JsxSelfClosingElement(JsxSelfClosingElementData {
            tag_name: None,
            type_arguments: None,
            attributes: Some(synthetic_attributes),
        }),
        0,
        0,
        NodeFlags::NONE,
    );
    // These openings are not reachable from SourceFile.children,
    // so the binder still leaves their synthetic descendants
    // unbound. Give the opening-like nodes a lexical parent,
    // however: resolveName's scope walk is defined only for nodes
    // whose parent chain terminates at the SourceFile.
    let source_root = source.root;
    for opening in [
        synthetic_opening,
        attributes_missing_opening,
        tag_missing_opening,
    ] {
        source.arena.node_mut(opening).parent = Some(source_root);
    }

    let options = CompilerOptions {
        jsx: Some(1),
        ..CompilerOptions::default()
    };
    let binder = bind_source_file(&source, &options);
    let mut state = CheckerState::new(&source, &binder, &options);

    assert!(state.node_symbol(valid_attribute).is_some());
    assert!(state.node_symbol(unbound_attribute).is_none());
    let recovered = state
        .create_jsx_attributes_type_from_attributes_property(synthetic_opening, CheckMode::NORMAL)
        .expect("synthetic attributes recover without containment");
    assert!(
        state
            .get_property_of_type_full(recovered, "live")
            .expect("valid property lookup")
            .is_some(),
        "malformed synthetic attributes must not discard the bound sibling"
    );

    let missing_attributes = state
        .create_jsx_attributes_type_from_attributes_property(
            attributes_missing_opening,
            CheckMode::NORMAL,
        )
        .expect("missing attributes recover to error type");
    assert!(state.tables.is_error_type(missing_attributes));
    let orphan_attributes = state
        .check_jsx_attributes(synthetic_attributes, CheckMode::NORMAL)
        .expect("orphan attributes recover to error type");
    assert!(state.tables.is_error_type(orphan_attributes));

    let diagnostics_before = state.diagnostics.len();
    assert_eq!(
        state
            .get_intrinsic_tag_symbol(tag_missing_opening)
            .expect("missing intrinsic tag recovers"),
        state.unknown_symbol
    );
    assert_eq!(
        state
            .get_jsx_reference_kind(tag_missing_opening)
            .expect("missing reference tag recovers"),
        JsxReferenceKind::Mixed
    );
    let missing_static = state
        .get_static_type_of_referenced_jsx_constructor(tag_missing_opening)
        .expect("missing static tag recovers");
    assert!(state.tables.is_error_type(missing_static));
    let missing_intrinsic = state
        .get_intrinsic_attributes_type_from_jsx_opening_like_element(tag_missing_opening)
        .expect("missing intrinsic attributes tag recovers");
    assert!(state.tables.is_error_type(missing_intrinsic));
    state
        .check_jsx_opening_like_element_or_opening_fragment(tag_missing_opening)
        .expect("missing opening tag uses local no-signature recovery");
    assert_eq!(state.diagnostics.len(), diagnostics_before);

    let value_tag_name = match state.data_of(value_tag_opening) {
        NodeData::JsxSelfClosingElement(data) => data.tag_name.expect("value tag"),
        _ => unreachable!("selected a self-closing element"),
    };
    assert!(!state.is_jsx_intrinsic_tag_name(value_tag_name));
    let diagnostics_before = state.diagnostics.len();
    assert_eq!(
        state
            .get_intrinsic_tag_symbol(value_tag_opening)
            .expect("intrinsic-only helper misuse recovers"),
        state.unknown_symbol
    );
    assert_eq!(state.diagnostics.len(), diagnostics_before);
}

#[test]
fn non_generic_interface_element_type_uses_its_declared_type() {
    let options = CompilerOptions {
        jsx: Some(1),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[(
            "a.tsx",
            "declare namespace JSX { interface Element {} interface ElementType {} interface IntrinsicElements { x: {} } }\n(<x />);\n",
        )],
        &options,
        |state| {
            let (opening, element_type_declaration) = {
                let source = state.binder.source(0);
                let opening = source
                    .arena
                    .node_ids()
                    .find(|&node| {
                        source.arena.node(node).kind == SyntaxKind::JsxSelfClosingElement
                    })
                    .expect("JSX opening");
                let declaration = source
                    .arena
                    .node_ids()
                    .find(|&node| match &source.arena.node(node).data {
                        NodeData::InterfaceDeclaration(data) => {
                            data.name.is_some_and(|name| {
                                matches!(
                                    &source.arena.node(name).data,
                                    NodeData::Identifier(data)
                                        if data.escaped_text == "ElementType"
                                )
                            })
                        }
                        _ => false,
                    })
                    .expect("ElementType interface");
                (opening, declaration)
            };
            let symbol = state
                .get_symbol_of_declaration(element_type_declaration)
                .expect("ElementType symbol");
            let declared = state
                .get_declared_type_of_symbol_slice(symbol)
                .expect("declared ElementType");
            let recovered = state
                .instantiate_alias_or_interface_with_defaults(symbol, false, &[])
                .expect("non-generic interface recovery")
                .expect("zero-argument instantiation");
            assert_eq!(recovered, declared);
            let constraint = state
                .get_jsx_element_type_type_at(opening)
                .expect("non-generic interface recovery")
                .expect("ElementType is present");
            assert_eq!(constraint, declared);
        },
    );
}
