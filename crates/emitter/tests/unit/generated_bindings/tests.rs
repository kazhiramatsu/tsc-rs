use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use tsc_syntax::{nodes as syntax_nodes, parse_source_file, NodeData};
use tsc_types::NodeFlags;

use super::super::target_bindings::TargetBinding;
use super::{AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes};
use crate::{
    create_printer, transform_nodes, NewLineKind, PrintRequest, PrinterOptions, StandaloneWriter,
    TransformArena, TransformError, TransformFlags, TransformNode, TransformRoot,
    TransformSourceId, TransformationContext, TransformationResult, Transformer,
};

#[derive(Clone, Copy)]
enum PrintGeneratedNameFixture {
    Identifier,
    SyntheticConstSibling,
}

struct UnfinalizedGeneratedNameTransformer {
    fixture: PrintGeneratedNameFixture,
    printed_node: Rc<Cell<Option<TransformNode>>>,
}

impl UnfinalizedGeneratedNameTransformer {
    fn create_identifier(
        context: &mut TransformationContext,
        source: TransformSourceId,
        text: &str,
    ) -> Result<TransformNode, TransformError> {
        context.factory()?.create_node(
            source,
            NodeData::Identifier(syntax_nodes::IdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(text),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_const_declaration(
        context: &mut TransformationContext,
        source: TransformSourceId,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let zero = context.factory()?.create_node(
            source,
            NodeData::NumericLiteral(syntax_nodes::NumericLiteralData {
                text: "0".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        context.factory()?.create_node(
            source,
            NodeData::VariableDeclaration(syntax_nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: Some(zero.node()),
            }),
            TransformFlags::NONE,
        )
    }
}

impl Transformer for UnfinalizedGeneratedNameTransformer {
    fn name(&self) -> &'static str {
        "unfinalizedGeneratedName"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            unreachable!("generated-name fixtures use a source-file root");
        };
        let binding = TargetBinding::allocate_numbered(context, "y".into(), "y".into())?;
        let generated = Self::create_identifier(context, source, "y")?;
        binding.write_generated_metadata(context.arena_mut()?, generated);

        let printed_node = match self.fixture {
            PrintGeneratedNameFixture::Identifier => generated,
            PrintGeneratedNameFixture::SyntheticConstSibling => {
                let ordinary = Self::create_identifier(context, source, "y_1")?;
                let ordinary = Self::create_const_declaration(context, source, ordinary)?;
                let generated = Self::create_const_declaration(context, source, generated)?;
                let declarations = context
                    .factory()?
                    .create_node_array(source, vec![ordinary, generated])?;
                let declaration_list = context.factory()?.create_node(
                    source,
                    NodeData::VariableDeclarationList(syntax_nodes::VariableDeclarationListData {
                        declarations: Some(declarations.array()),
                    }),
                    TransformFlags::NONE,
                )?;
                context
                    .factory()?
                    .set_node_flags(declaration_list, NodeFlags::CONST)?;
                context.factory()?.create_node(
                    source,
                    NodeData::VariableStatement(syntax_nodes::VariableStatementData {
                        modifiers: None,
                        declaration_list: Some(declaration_list.node()),
                    }),
                    TransformFlags::NONE,
                )?
            }
        };
        self.printed_node.set(Some(printed_node));
        Ok(root)
    }
}

fn generated_name_print_fixture(
    source_text: &str,
    fixture: PrintGeneratedNameFixture,
) -> (TransformationResult<'static>, TransformNode) {
    let parsed = parse_source_file(
        "generated-name-print.ts",
        source_text,
        Default::default(),
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let printed_node = Rc::new(Cell::new(None));
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(UnfinalizedGeneratedNameTransformer {
            fixture,
            printed_node: Rc::clone(&printed_node),
        })],
        true,
    )
    .expect("build an unfinalized generated-name fixture");
    let printed_node = printed_node
        .get()
        .expect("fixture published its print root");
    (result, printed_node)
}

fn print_standalone_generated_name(
    result: &mut TransformationResult<'_>,
    node: TransformNode,
) -> String {
    create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            result,
            PrintRequest::StandaloneNode {
                node,
                writer: StandaloneWriter::MultiLine,
            },
            None,
        )
        .expect("print the generated-name fixture")
        .text()
        .to_owned()
}

#[test]
fn print_generated_name_reserves_identifiers_outside_the_printed_node() {
    let (mut result, node) =
        generated_name_print_fixture("const y_1 = 0;", PrintGeneratedNameFixture::Identifier);

    assert_eq!(print_standalone_generated_name(&mut result, node), "y_2");
}

#[test]
fn print_generated_name_starts_with_first_suffix_without_a_source_collision() {
    let (mut result, node) =
        generated_name_print_fixture("", PrintGeneratedNameFixture::Identifier);

    assert_eq!(print_standalone_generated_name(&mut result, node), "y_1");
}

#[test]
fn print_generated_name_ignores_an_ordinary_synthetic_sibling() {
    let (mut result, node) =
        generated_name_print_fixture("", PrintGeneratedNameFixture::SyntheticConstSibling);

    assert_eq!(
        print_standalone_generated_name(&mut result, node),
        "const y_1 = 0, y_1 = 0;",
    );
}

#[test]
fn generated_name_state_starts_fresh_for_each_print() {
    let (mut result, node) =
        generated_name_print_fixture("const y_1 = 0;", PrintGeneratedNameFixture::Identifier);

    let first = print_standalone_generated_name(&mut result, node);
    let second = print_standalone_generated_name(&mut result, node);
    assert_eq!(first, "y_2");
    assert_eq!(second, first);
}

#[test]
fn planned_temp_is_retained_when_available() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);

    assert_eq!(
        scopes.allocate_planned_temp_with_policy("_c".into(), false),
        "_c",
    );
}

#[test]
fn duplicate_planned_temp_in_same_scope_falls_back_to_temp_sequence() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);

    assert_eq!(
        scopes.allocate_planned_temp_with_policy("_a".into(), false),
        "_a",
    );
    assert_eq!(
        scopes.allocate_planned_temp_with_policy("_a".into(), false),
        "_b",
    );
}

#[test]
fn planned_temp_can_be_reused_in_sibling_scopes() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    let (source, first) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_temp_with_policy("_a".into(), false),
        "_a",
    );
    let _ = scopes.exit(source, first);

    let (source, sibling) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_temp_with_policy("_a".into(), false),
        "_a",
    );
    let _ = scopes.exit(source, sibling);
}

#[test]
fn descendant_reserved_preferred_bindings_still_reuse_in_siblings() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    let (source, first) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_super", "_super".into(), true),
        "_super",
    );
    let (first_scope, nested) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_super", "_super".into(), true),
        "_super_1",
    );
    let _ = scopes.exit(first_scope, nested);
    let _ = scopes.exit(source, first);

    let (source, sibling) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_super", "_super".into(), true),
        "_super",
    );
    let _ = scopes.exit(source, sibling);
}

#[test]
fn preferred_reconciliation_advances_from_the_planned_suffix() {
    let mut scopes = GeneratedBindingScopes::new(
        BTreeSet::from(["_super".to_owned(), "_super_1".to_owned()]),
        AncestorBindingPolicy::AllowShadow,
    );
    let (source, function) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_super", "_super_1".into(), true),
        "_super_2",
    );
    let _ = scopes.exit(source, function);
}

#[test]
fn file_level_optimistic_peers_share_text_but_reserve_descendants() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    assert_eq!(
        scopes.reserve_planned_file_level_optimistic_with_policy("_default".into(), true),
        "_default",
    );
    assert_eq!(
        scopes.reserve_planned_file_level_optimistic_with_policy("_default".into(), true),
        "_default",
    );

    let (source, first) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_default", "_default".into(), true),
        "_default_1",
    );
    let _ = scopes.exit(source, first);

    let (source, sibling) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_planned_preferred_with_policy("_default", "_default".into(), true),
        "_default_1",
    );
    let _ = scopes.exit(source, sibling);
}

#[test]
fn eager_local_preferred_reservations_are_not_hoisted_bindings() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    let (source, outer) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_local_preferred_with_policy("_super".into(), true),
        "_super",
    );
    let (outer_scope, inner) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_local_preferred_with_policy("_super".into(), true),
        "_super_1",
    );
    assert!(scopes.exit(outer_scope, inner).names().is_empty());
    assert!(scopes.exit(source, outer).names().is_empty());
}

#[test]
fn formatted_private_temps_have_a_role_local_sequence() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    assert_eq!(scopes.allocate_local_temp(), "_a");
    assert_eq!(
        scopes.allocate_private_temp_with_role_suffix("_accessor_storage", &BTreeSet::new(),),
        "_a_accessor_storage",
    );
    assert_eq!(
        scopes.allocate_private_temp_with_role_suffix("_accessor_storage", &BTreeSet::new(),),
        "_b_accessor_storage",
    );
}

#[test]
fn generated_private_names_reserve_ancestors_but_reuse_in_siblings() {
    let mut scopes =
        GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::AllowShadow);
    assert_eq!(
        scopes.allocate_private_preferred_with_role_suffix(
            "a",
            "_accessor_storage",
            &BTreeSet::new(),
        ),
        "a_accessor_storage",
    );
    let (source, outer) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_private_preferred_with_role_suffix(
            "a",
            "_accessor_storage",
            &BTreeSet::new(),
        ),
        "a_1_accessor_storage",
    );
    assert_eq!(
        scopes.allocate_private_preferred_with_role_suffix(
            "b",
            "_accessor_storage",
            &BTreeSet::new(),
        ),
        "b_accessor_storage",
    );
    let _ = scopes.exit(source, outer);

    let (source, sibling) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(
        scopes.allocate_private_preferred_with_role_suffix(
            "b",
            "_accessor_storage",
            &BTreeSet::new(),
        ),
        "b_accessor_storage",
    );
    let _ = scopes.exit(source, sibling);
}

// ================================================================
// H2.5h-b B-1: the E-NAMES-H policy-arm contracts (packet §12.3(b))
// and the loop-variable / node-keyed completion surface.
// ================================================================

#[test]
fn loop_variable_prefers_the_dedicated_slot_once_per_scope() {
    let mut scopes = GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::Reserve);
    let (source, body) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(scopes.allocate_loop_variable(false), "_i");
    assert_eq!(scopes.allocate_loop_variable(false), "_a");
    assert_eq!(scopes.allocate_temp(), "_b");
    let _ = scopes.exit(source, body);
}

#[test]
fn occupied_loop_slot_falls_through_to_the_temp_sequence() {
    let mut scopes = GeneratedBindingScopes::new(
        ["_i".to_owned()].into_iter().collect(),
        AncestorBindingPolicy::Reserve,
    );
    assert_eq!(scopes.allocate_loop_variable(false), "_a");
}

#[test]
fn sibling_scopes_reuse_the_loop_variable_spelling() {
    // §12.3(b) sibling-reuse arm: tsc resets tempFlags per function, so
    // sibling function scopes may both own `_i`.
    let mut scopes = GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::Reserve);
    let (source, first) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(scopes.allocate_loop_variable(false), "_i");
    let _ = scopes.exit(source, first);
    let (source, sibling) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(scopes.allocate_loop_variable(false), "_i");
    let _ = scopes.exit(source, sibling);
}

#[test]
fn active_ancestor_bindings_stay_reserved_in_descendants() {
    // §12.3(b) ancestor-reservation arm: an active ancestor's generated
    // bindings remain reserved while a descendant scope allocates.
    let mut scopes = GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::Reserve);
    let (source, outer) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(scopes.allocate_temp(), "_a");
    assert_eq!(scopes.allocate_loop_variable(false), "_i");
    let (outer_id, inner) = scopes.enter(GeneratedBindingOwner::FunctionBody);
    assert_eq!(scopes.allocate_loop_variable(false), "_b");
    assert_eq!(scopes.allocate_temp(), "_c");
    let _ = scopes.exit(outer_id, inner);
    let _ = scopes.exit(source, outer);
}

#[test]
fn node_keyed_allocation_is_stable_per_node_and_advances_per_source_name() {
    let mut scopes = GeneratedBindingScopes::new(BTreeSet::new(), AncestorBindingPolicy::Reserve);
    assert_eq!(
        scopes.allocate_source_numbered_for_node((0, 1), "loop_init"),
        "loop_init_1",
    );
    assert_eq!(
        scopes.allocate_source_numbered_for_node((0, 1), "loop_init"),
        "loop_init_1",
    );
    assert_eq!(
        scopes.allocate_source_numbered_for_node((0, 2), "loop_init"),
        "loop_init_2",
    );
}

#[test]
fn source_occupied_allocator_pushes_past_the_reserved_names() {
    // §12.3(a) universe-equality direction: parsed identifiers occupy the
    // allocator exactly as tsc's file-level unique-name predicate does.
    let mut scopes = GeneratedBindingScopes::new(
        ["_a", "_b", "_i", "_super"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        AncestorBindingPolicy::Reserve,
    );
    assert_eq!(scopes.allocate_temp(), "_c");
    assert_eq!(scopes.allocate_loop_variable(false), "_d");
}
