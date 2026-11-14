use super::*;
use crate::state::test_support::with_program_state;
use tsc_emitter::{
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, EmitResolverNode, EmitSymbolTracker,
    SourceFileId, TransformArena,
};
use tsc_types::CompilerOptions;

struct NoopTracker;
impl EmitSymbolTracker for NoopTracker {}

fn with_member_state(
    source: &str,
    run: impl FnOnce(&mut CheckerState<'_>, &mut TransformArena, tsc_emitter::TransformSourceId),
) {
    with_program_state(
        &[("/main.ts", source)],
        &CompilerOptions::default(),
        |checker| {
            let mut arena = TransformArena::new();
            let target =
                arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
            run(checker, &mut arena, target);
        },
    );
}

fn node_kind(
    arena: &TransformArena,
    target: tsc_emitter::TransformSourceId,
    node: tsc_emitter::TransformNode,
) -> SyntaxKind {
    let _ = target;
    arena
        .source(node.source())
        .expect("transform source")
        .syntax()
        .arena
        .node(node.node())
        .kind
}

fn statement_at(checker: &CheckerState<'_>, index: usize) -> NodeId {
    let source = checker.binder.source(0);
    let NodeData::SourceFile(data) = source.arena.node(source.root).data() else {
        panic!("source file expected")
    };
    let statements = source
        .arena
        .node_array(data.statements.expect("statements"));
    statements.nodes[index]
}

fn first_variable_declaration(checker: &CheckerState<'_>, statement: usize) -> NodeId {
    let statement = statement_at(checker, statement);
    let source = checker.binder.source(0);
    let NodeData::VariableStatement(data) = source.arena.node(statement).data() else {
        panic!("variable statement expected")
    };
    let NodeData::VariableDeclarationList(list) = &source
        .arena
        .node(data.declaration_list.expect("declaration list"))
        .data()
    else {
        panic!("declaration list expected")
    };
    source
        .arena
        .node_array(list.declarations.expect("declarations"))
        .nodes[0]
}

#[test]
fn dm_serialize_type_of_declaration() {
    with_member_state(
        "export const named: string = \"a\";",
        |checker, arena, target| {
            let declaration = first_variable_declaration(checker, 0);
            let root = checker.binder.source(0).root;
            let mut tracker = NoopTracker;
            let node = checker
                .emit_create_type_of_declaration(
                    arena,
                    target,
                    declaration,
                    root,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    None,
                    &mut tracker,
                )
                .expect("serialization succeeds")
                .expect("annotated declaration serializes");
            assert_eq!(node_kind(arena, target, node), SyntaxKind::StringKeyword);
        },
    );
    // Non-inferred-type kinds take the AnyKeyword token fallback.
    with_member_state("export function f(): void {}", |checker, arena, target| {
        let function = statement_at(checker, 0);
        let root = checker.binder.source(0).root;
        let mut tracker = NoopTracker;
        let node = checker
            .emit_create_type_of_declaration(
                arena,
                target,
                function,
                root,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                None,
                &mut tracker,
            )
            .expect("fallback succeeds")
            .expect("fallback token");
        assert_eq!(node_kind(arena, target, node), SyntaxKind::AnyKeyword);
    });
}

#[test]
fn dm_serialize_type_of_expando_property_in_synthetic_scope() {
    with_member_state("function f() {}\nf.x = 1;", |checker, arena, target| {
        let function = statement_at(checker, 0);
        let properties = checker
            .emit_get_properties_of_container_function(function, 0)
            .expect("container properties resolve");
        let declaration = properties
            .iter()
            .find(|property| property.name == "x")
            .and_then(|property| property.value_declaration)
            .expect("expando property declaration")
            .node();
        let root = checker.binder.source(0).root;
        let mut tracker = NoopTracker;
        let node = checker
            .emit_create_type_of_expando_member(
                arena,
                target,
                declaration,
                "x",
                root,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut tracker,
            )
            .expect("expando serialization succeeds")
            .expect("expando property type serializes");
        assert_eq!(node_kind(arena, target, node), SyntaxKind::NumberKeyword);
    });
}

#[test]
fn dm_symbol_declaration_order_facts_follow_binder_declarations() {
    let options = CompilerOptions::default();
    let mut source = tsc_syntax::parse_published_source_file(
        "/main.ts".to_owned(),
        "function f(x: string): string;\n\
         function f(x: number): number;\n\
         function f(x: unknown) { return x; }"
            .to_owned(),
        Default::default(),
        None,
    );
    let NodeData::SourceFile(data) = source.arena.node(source.root).data() else {
        panic!("source file expected")
    };
    let declarations = source
        .arena
        .node_array(data.statements.expect("statements"))
        .nodes
        .to_vec();
    let binder = tsc_binder::bind_source_file(&mut source);
    let mut state = CheckerState::from_program(vec![&binder], &options);
    state.merge_module_augmentations();
    let session = crate::emit::CheckerSession::from_checked_state(state);

    session.with_emit_resolver(|resolver| {
        let node = |node| EmitResolverNode::from_raw_source(0, node);
        // tsgo writes the properties of any declaration of a function that
        // has an implementation (util.go:155-162).
        assert!(resolver
            .should_emit_function_properties(node(declarations[0]))
            .expect("first overload fact"));
        assert!(resolver
            .should_emit_function_properties(node(declarations[1]))
            .expect("last overload fact"));
        assert!(resolver
            .should_emit_function_properties(node(declarations[2]))
            .expect("implementation fact"));

        assert!(resolver
            .is_first_declaration_of_symbol(node(declarations[0]))
            .expect("first declaration fact"));
        assert!(!resolver
            .is_first_declaration_of_symbol(node(declarations[1]))
            .expect("second declaration fact"));
        assert!(!resolver
            .is_first_declaration_of_symbol(node(declarations[2]))
            .expect("implementation declaration fact"));
    });
}

#[test]
fn dm_function_properties_need_an_implementation() {
    // tsgo shouldEmitFunctionProperties (util.go:155-162): a function whose
    // declarations are all bodiless writes no expando namespace.
    let options = CompilerOptions::default();
    let mut source = tsc_syntax::parse_published_source_file(
        "/main.ts".to_owned(),
        "declare function g(): void;\n\
         declare function g(x: number): void;"
            .to_owned(),
        Default::default(),
        None,
    );
    let NodeData::SourceFile(data) = source.arena.node(source.root).data() else {
        panic!("source file expected")
    };
    let declarations = source
        .arena
        .node_array(data.statements.expect("statements"))
        .nodes
        .to_vec();
    let binder = tsc_binder::bind_source_file(&mut source);
    let mut state = CheckerState::from_program(vec![&binder], &options);
    state.merge_module_augmentations();
    let session = crate::emit::CheckerSession::from_checked_state(state);

    session.with_emit_resolver(|resolver| {
        let node = |node| EmitResolverNode::from_raw_source(0, node);
        for declaration in declarations {
            assert!(!resolver
                .should_emit_function_properties(node(declaration))
                .expect("bodiless declaration fact"));
        }
    });
}

#[test]
fn dm_serialize_return_type() {
    with_member_state(
        "export function f(): number { return 1; }",
        |checker, arena, target| {
            let function = statement_at(checker, 0);
            let root = checker.binder.source(0).root;
            let mut tracker = NoopTracker;
            let node = checker
                .emit_create_return_type_of_signature_declaration(
                    arena,
                    target,
                    function,
                    root,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    &mut tracker,
                )
                .expect("serialization succeeds")
                .expect("annotated return serializes");
            assert_eq!(node_kind(arena, target, node), SyntaxKind::NumberKeyword);
        },
    );
}

#[test]
fn dm_literal_const_value() {
    let source = "export const s = \"hi\";\n\
                  export const n = 3;\n\
                  export const m = -4;\n\
                  export const b = true;\n\
                  export const g = 5n;";
    with_member_state(source, |checker, arena, target| {
        let mut tracker = NoopTracker;
        let expectations = [
            (0usize, SyntaxKind::StringLiteral),
            (1, SyntaxKind::NumericLiteral),
            (2, SyntaxKind::PrefixUnaryExpression),
            (3, SyntaxKind::TrueKeyword),
            (4, SyntaxKind::BigIntLiteral),
        ];
        for (statement, expected) in expectations {
            let declaration = first_variable_declaration(checker, statement);
            let node = checker
                .emit_create_literal_const_value(arena, target, declaration, &mut tracker)
                .expect("literal serializes")
                .expect("a literal type has a literal value");
            assert_eq!(
                node_kind(arena, target, node),
                expected,
                "statement {statement}"
            );
        }
    });
}

#[test]
fn dm_late_bound_index_signatures() {
    // A source-declared index signature is skipped (info.declaration
    // set) after the present-empty result materializes.
    with_member_state(
        "export class Boxes { [key: string]: number; }",
        |checker, arena, target| {
            let class = statement_at(checker, 0);
            let root = checker.binder.source(0).root;
            let mut tracker = NoopTracker;
            let result = checker
                .emit_create_late_bound_index_signatures(
                    arena,
                    target,
                    class,
                    root,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    &mut tracker,
                )
                .expect("member succeeds");
            // Upstream returns a PRESENT-EMPTY array here: the instance
            // info list is non-empty (result ||= [] fires) and its one
            // declared info is then skipped (:88632-88633).
            assert_eq!(result, Some(Vec::new()));
        },
    );
}

#[test]
fn declaration_alias_views_preserve_stored_keys_and_order_after_preparation() {
    use std::sync::Arc;
    use tsc_binder::SymbolTable;
    use tsc_types::{EscapedName, SymbolFlags};

    with_program_state(
        &[("/main.ts", "export {};")],
        &CompilerOptions::default(),
        |checker| {
            let first_key = EscapedName::escape("storedFirst".into());
            let second_key = EscapedName::escape("default".into());
            let first = checker
                .binder
                .create_symbol(SymbolFlags::ALIAS, EscapedName::escape("symbolName".into()));
            let second = checker
                .binder
                .create_symbol(SymbolFlags::ALIAS, EscapedName::escape("otherName".into()));
            let ordinary = checker.binder.create_symbol(
                SymbolFlags::FUNCTION,
                EscapedName::escape("ordinary".into()),
            );
            let table = Arc::new(SymbolTable::from_iter([
                (first_key, first),
                (EscapedName::escape("ordinary".into()), ordinary),
                (second_key, second),
            ]));
            assert!(checker.declaration_emit_alias_table(Some(&table)).is_none());
            assert!(checker.declaration_emit_inputs.is_none());
            checker
                .emit_precalculate_declaration_emit_visibility(checker.binder.source(0).root)
                .expect("declaration preparation");
            let cold = checker
                .declaration_emit_alias_table(Some(&table))
                .expect("prepared alias view");
            let warm = checker
                .declaration_emit_alias_table(Some(&table))
                .expect("warm alias view");
            assert!(Arc::ptr_eq(&cold, &warm));
            assert_eq!(
                cold.iter()
                    .map(|(name, &symbol)| (*name, symbol))
                    .collect::<Vec<_>>(),
                vec![(first_key, first), (second_key, second)]
            );
            assert_eq!(table.len(), 3);
            assert!(checker
                .declaration_emit_alias_table(Some(&Arc::new(SymbolTable::default())))
                .is_none());
        },
    );
}

#[test]
fn declaration_alias_views_follow_copy_on_write_export_snapshots() {
    use std::sync::Arc;
    use tsc_binder::SymbolTable;
    use tsc_types::{EscapedName, SymbolFlags};

    with_program_state(
        &[("/main.ts", "export {};")],
        &CompilerOptions::default(),
        |checker| {
            let a = EscapedName::escape("a".into());
            let b = EscapedName::escape("b".into());
            let first = checker.binder.create_symbol(SymbolFlags::ALIAS, a);
            let second = checker.binder.create_symbol(SymbolFlags::ALIAS, b);
            let mut exports = Arc::new(SymbolTable::from_iter([(a, first)]));
            checker
                .emit_precalculate_declaration_emit_visibility(checker.binder.source(0).root)
                .expect("declaration preparation");
            let old_address = Arc::as_ptr(&exports);
            let old = checker
                .declaration_emit_alias_table(Some(&exports))
                .expect("original export view");
            Arc::make_mut(&mut exports).insert(b, second);
            assert_ne!(Arc::as_ptr(&exports), old_address);
            let updated = checker
                .declaration_emit_alias_table(Some(&exports))
                .expect("replacement export view");
            assert_eq!(old.len(), 1);
            assert_eq!(old.get(b), None);
            assert_eq!(updated.get(b), Some(&second));
            assert_eq!(updated.len(), 2);
        },
    );
}

#[test]
fn display_type_nodes_keep_the_symbols_of_their_emitted_identifiers() {
    with_member_state(
        "export class Named {} export const value = new Named();",
        |checker, arena, target| {
            let declaration = first_variable_declaration(checker, 1);
            let variable = checker
                .get_symbol_of_declaration(declaration)
                .expect("variable");
            let ty = checker.get_type_of_symbol(variable).expect("instance type");
            let symbol = checker.tables.type_of(ty).symbol.expect("class symbol");
            let (node, symbols) = crate::node_builder::type_to_type_node_with_identifier_symbols(
                checker,
                arena,
                target,
                ty,
                Some(declaration),
                EmitNodeBuilderFlags::NO_TRUNCATION,
                None,
            )
            .expect("display type")
            .expect("type reference");
            let NodeData::TypeReference(reference) = arena.node(node).expect("type node").data()
            else {
                panic!("named instance must use a type reference");
            };
            let name =
                tsc_emitter::TransformNode::new(node.source(), reference.type_name.expect("name"));
            assert!(symbols
                .iter()
                .any(|&(identifier, named)| identifier == name && named == symbol));
        },
    );
}

#[test]
fn declaration_and_semantic_literals_keep_all_javascript_code_units() {
    for units in [
        vec![0xd800],
        vec![0xdc00],
        vec![0xd83d, 0xde00],
        vec![0xfffd],
        vec![0, 0xfffd, 0xd800, 0xd83d, 0xde00, 0xdc00, 0x78],
    ] {
        let escaped = units
            .iter()
            .map(|unit| format!("\\u{unit:04x}"))
            .collect::<String>();
        let source = format!("export const value = \"{escaped}\";");
        with_member_state(&source, |checker, arena, target| {
            let declaration = first_variable_declaration(checker, 0);
            let symbol = checker
                .get_symbol_of_declaration(declaration)
                .expect("variable");
            let ty = checker.get_type_of_symbol(symbol).expect("literal type");
            let constant = checker
                .emit_create_literal_const_value(arena, target, declaration, &mut NoopTracker)
                .expect("declaration literal")
                .expect("constant");
            assert_eq!(
                arena
                    .literal_value(constant)
                    .expect("constant value")
                    .expect("string")
                    .to_utf16(),
                units
            );
            let node = crate::node_builder::with_context(
                checker,
                arena,
                target,
                Some(declaration),
                Some(EmitNodeBuilderFlags::NO_TRUNCATION),
                None,
                None,
                None,
                None,
                |checker, arena, target, context| {
                    crate::node_builder::type_to_type_node_helper(
                        checker, arena, target, ty, context,
                    )
                },
                None,
            )
            .expect("semantic type")
            .flatten()
            .expect("literal type node");
            let NodeData::LiteralType(literal) = arena.node(node).expect("type node").data() else {
                panic!("string type must use a literal type node");
            };
            let literal =
                tsc_emitter::TransformNode::new(node.source(), literal.literal.expect("literal"));
            assert_eq!(
                arena
                    .literal_value(literal)
                    .expect("type value")
                    .expect("string")
                    .to_utf16(),
                units
            );
        });
    }
}

fn chain_declaration(
    checker: &CheckerState<'_>,
    file: usize,
    kind: SyntaxKind,
    name: &str,
) -> NodeId {
    let source = checker.binder.source(file);
    source
        .arena
        .node_ids()
        .find(|&node| {
            source.arena.node(node).kind == kind
                && node_util::get_name_of_declaration(source, node)
                    .and_then(|name| checker.identifier_text_of(name))
                    .is_some_and(|text| text == name)
        })
        .expect("named declaration in chain fixture")
}

fn prepared_chain_cache<'state>(
    checker: &'state CheckerState<'_>,
) -> &'state DeclarationChainCache {
    checker
        .declaration_emit_inputs
        .as_ref()
        .expect("declaration input owner")
        .chains
        .as_ref()
        .expect("initialized chain cache")
}

#[test]
fn declaration_chains_share_canonical_scopes_and_keep_shadowing_meanings() {
    let source = "interface Shared {}\nconst Shared = 0;\n\
        function outer<T>() { let first: Shared; let second: Shared;\n\
        function nested<Shared>() { let inside: Shared; } }";
    with_program_state(
        &[("/main.ts", source)],
        &CompilerOptions::default(),
        |checker| {
            let root = checker.binder.source(0).root;
            let symbol = checker
                .binder
                .node_symbol(chain_declaration(
                    checker,
                    0,
                    SyntaxKind::InterfaceDeclaration,
                    "Shared",
                ))
                .expect("interface symbol");
            let first = chain_declaration(checker, 0, SyntaxKind::VariableDeclaration, "first");
            let second = chain_declaration(checker, 0, SyntaxKind::VariableDeclaration, "second");
            let inside = chain_declaration(checker, 0, SyntaxKind::VariableDeclaration, "inside");
            let baseline_type = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(inside), false)
                .expect("baseline type")
                .map(AccessibleSymbolChain::into_vec);
            let baseline_value = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::VALUE, Some(inside), false)
                .expect("baseline value")
                .map(AccessibleSymbolChain::into_vec);
            assert_ne!(
                baseline_type, baseline_value,
                "a type parameter shadows only the type meaning"
            );
            assert!(checker.declaration_emit_inputs.is_none());
            checker
                .emit_precalculate_declaration_emit_visibility(root)
                .expect("prepare");
            assert!(matches!(
                checker
                    .declaration_emit_inputs
                    .as_ref()
                    .expect("owner")
                    .chains,
                DeclarationChainCacheState::Cold
            ));
            let first_chain = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(first), false)
                .expect("first chain")
                .expect("visible global");
            assert!(matches!(&first_chain, AccessibleSymbolChain::Owned(_)));
            let second_chain = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(second), false)
                .expect("second chain")
                .expect("same visible global");
            assert_eq!(&*first_chain, &*second_chain);
            let AccessibleSymbolChain::Retained(second_owner) = second_chain else {
                panic!("equivalent scopes should retain a hit")
            };
            let first_again = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(first), false)
                .expect("repeat")
                .expect("visible global");
            let AccessibleSymbolChain::Retained(first_owner) = first_again else {
                panic!("repeat should retain a hit")
            };
            assert!(Arc::ptr_eq(&first_owner, &second_owner));
            let cached_type = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(inside), false)
                .expect("shadowed type")
                .map(AccessibleSymbolChain::into_vec);
            let cached_value = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::VALUE, Some(inside), false)
                .expect("unshadowed value")
                .map(AccessibleSymbolChain::into_vec);
            assert_eq!(cached_type, baseline_type);
            assert_eq!(cached_value, baseline_value);
            let cache = prepared_chain_cache(checker);
            assert_eq!(cache.scopes[&Some(first)], cache.scopes[&Some(second)]);
            assert_ne!(cache.scopes[&Some(first)], cache.scopes[&Some(inside)]);
            assert!(cache
                .answers
                .keys()
                .any(|key| key.symbol == symbol && key.meaning == SymbolFlags::TYPE.bits()));
            assert!(cache
                .answers
                .keys()
                .any(|key| key.symbol == symbol && key.meaning == SymbolFlags::VALUE.bits()));
        },
    );
}

#[test]
fn declaration_chains_retain_negative_answers_between_external_files() {
    let files = [
        ("/hidden.ts", "interface Hidden {} export {};"),
        ("/main.ts", "export {};"),
    ];
    with_program_state(&files, &CompilerOptions::default(), |checker| {
        let symbol = checker
            .binder
            .node_symbol(chain_declaration(
                checker,
                0,
                SyntaxKind::InterfaceDeclaration,
                "Hidden",
            ))
            .expect("private interface");
        let root = checker.binder.source(1).root;
        assert!(checker
            .get_accessible_symbol_chain(symbol, Some(root), SymbolFlags::TYPE, false)
            .expect("baseline miss")
            .is_none());
        checker
            .emit_precalculate_declaration_emit_visibility(root)
            .expect("prepare");
        for _ in 0..2 {
            assert!(checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(root), false)
                .expect("cached miss")
                .is_none());
        }
        let cache = prepared_chain_cache(checker);
        assert!(cache
            .answers
            .iter()
            .any(|(key, value)| key.symbol == symbol && value.is_none()));
        assert!(cache.scopes[&Some(root)].external_module);
    });
}

#[test]
fn declaration_chains_keep_internal_and_external_alias_policies_separate() {
    let source = "namespace Root { export interface Item {} } import Short = Root;";
    with_program_state(
        &[("/main.ts", source)],
        &CompilerOptions::default(),
        |checker| {
            let root = checker.binder.source(0).root;
            let symbol = checker
                .binder
                .node_symbol(chain_declaration(
                    checker,
                    0,
                    SyntaxKind::InterfaceDeclaration,
                    "Item",
                ))
                .expect("namespace member");
            let ordinary = checker
                .get_accessible_symbol_chain(symbol, Some(root), SymbolFlags::TYPE, false)
                .expect("ordinary baseline");
            let external = checker
                .get_accessible_symbol_chain(symbol, Some(root), SymbolFlags::TYPE, true)
                .expect("external baseline");
            assert!(ordinary.is_some());
            assert_ne!(ordinary, external);
            checker
                .emit_precalculate_declaration_emit_visibility(root)
                .expect("prepare");
            for _ in 0..2 {
                assert_eq!(
                    checker
                        .get_accessible_symbol_chain(symbol, Some(root), SymbolFlags::TYPE, false)
                        .expect("ordinary cached"),
                    ordinary
                );
                assert_eq!(
                    checker
                        .get_accessible_symbol_chain(symbol, Some(root), SymbolFlags::TYPE, true)
                        .expect("external cached"),
                    external
                );
            }
            let cache = prepared_chain_cache(checker);
            assert!(cache
                .answers
                .keys()
                .any(|key| key.symbol == symbol && key.external_only));
            assert!(cache
                .answers
                .keys()
                .any(|key| key.symbol == symbol && !key.external_only));
        },
    );
}

#[test]
fn declaration_chain_storage_stays_cold_for_excluded_or_unsettled_queries() {
    with_program_state(
        &[("/main.ts", "class Box { run() {} }")],
        &CompilerOptions::default(),
        |checker| {
            let root = checker.binder.source(0).root;
            let class = checker
                .binder
                .node_symbol(chain_declaration(
                    checker,
                    0,
                    SyntaxKind::ClassDeclaration,
                    "Box",
                ))
                .expect("class");
            let method = checker
                .binder
                .node_symbol(chain_declaration(
                    checker,
                    0,
                    SyntaxKind::MethodDeclaration,
                    "run",
                ))
                .expect("method");
            checker
                .emit_precalculate_declaration_emit_visibility(root)
                .expect("prepare");
            assert!(checker
                .accessible_symbol_chain_at(method, SymbolFlags::VALUE, Some(root), false)
                .expect("method exclusion")
                .is_none());
            checker.speculation_depth += 1;
            let speculative = checker
                .accessible_symbol_chain_at(class, SymbolFlags::TYPE, Some(root), false)
                .expect("speculative")
                .expect("class");
            assert!(matches!(speculative, AccessibleSymbolChain::Owned(_)));
            checker.speculation_depth -= 1;
            let pending = checker.binder.create_symbol(
                SymbolFlags::TYPE_ALIAS,
                tsc_types::EscapedName::escape("Pending".into()),
            );
            assert!(checker.push_type_resolution(
                crate::state::ResolutionTarget::Symbol(pending),
                tsc_types::TypeSystemPropertyName::TYPE
            ));
            let unsettled = checker
                .accessible_symbol_chain_at(class, SymbolFlags::TYPE, Some(root), false)
                .expect("active resolution")
                .expect("class");
            assert!(matches!(unsettled, AccessibleSymbolChain::Owned(_)));
            assert!(checker.pop_type_resolution());
            assert!(matches!(
                checker
                    .declaration_emit_inputs
                    .as_ref()
                    .expect("owner")
                    .chains,
                DeclarationChainCacheState::Cold
            ));
            let cold = checker
                .accessible_symbol_chain_at(class, SymbolFlags::TYPE, Some(root), false)
                .expect("settled")
                .expect("class");
            assert!(matches!(cold, AccessibleSymbolChain::Owned(_)));
            let warm = checker
                .accessible_symbol_chain_at(class, SymbolFlags::TYPE, Some(root), false)
                .expect("repeat")
                .expect("class");
            assert!(matches!(warm, AccessibleSymbolChain::Retained(_)));
        },
    );
}

#[test]
fn declaration_chain_invalidation_and_static_suspension_keep_other_inputs() {
    let source = "const key = 'field'; class Box { static [key]: number; static field: number; }";
    with_program_state(
        &[("/main.ts", source)],
        &CompilerOptions::default(),
        |checker| {
            let root = checker.binder.source(0).root;
            let symbol = checker
                .binder
                .node_symbol(chain_declaration(
                    checker,
                    0,
                    SyntaxKind::ClassDeclaration,
                    "Box",
                ))
                .expect("class");
            checker
                .emit_precalculate_declaration_emit_visibility(root)
                .expect("prepare");
            let module_names = Arc::new(DeclarationEmitModuleNames {
                imports: Vec::new(),
                augmentations: Vec::new(),
            });
            checker
                .declaration_emit_inputs
                .as_ref()
                .expect("owner")
                .module_names
                .borrow_mut()
                .insert(0, Arc::clone(&module_names));
            checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(root), false)
                .expect("cold chain")
                .expect("class");
            let before_clone = prepared_chain_cache(checker).generation;
            checker.clone_symbol(symbol);
            assert!(prepared_chain_cache(checker).generation > before_clone);
            assert!(prepared_chain_cache(checker).answers.is_empty());
            checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(root), false)
                .expect("rebuild")
                .expect("class");
            let generation = prepared_chain_cache(checker).generation;
            let outer = checker.suspend_declaration_chain_cache();
            assert!(outer.is_some());
            assert!(checker.suspend_declaration_chain_cache().is_none());
            let during = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(root), false)
                .expect("suspended")
                .expect("class");
            assert!(matches!(during, AccessibleSymbolChain::Owned(_)));
            checker.restore_declaration_chain_cache(None);
            checker.restore_declaration_chain_cache(outer);
            assert!(prepared_chain_cache(checker).generation > generation);
            checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(root), false)
                .expect("new generation")
                .expect("class");
            let before_static = prepared_chain_cache(checker).generation;
            let exports = checker
                .get_exports_of_symbol(symbol)
                .expect("late static exports");
            assert!(!exports.is_empty());
            assert!(prepared_chain_cache(checker).generation > before_static);
            assert!(prepared_chain_cache(checker).answers.is_empty());
            assert!(Arc::ptr_eq(
                checker
                    .declaration_emit_inputs
                    .as_ref()
                    .expect("owner")
                    .module_names
                    .borrow()
                    .get(&0)
                    .expect("retained module inputs"),
                &module_names
            ));
            let chain = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(root), false)
                .expect("after static binding")
                .expect("class");
            assert!(matches!(chain, AccessibleSymbolChain::Owned(_)));
        },
    );
}

#[test]
fn declaration_chains_follow_replaced_global_snapshots() {
    with_program_state(
        &[("/main.ts", "interface Shared {}")],
        &CompilerOptions::default(),
        |checker| {
            let root = checker.binder.source(0).root;
            let symbol = checker
                .binder
                .node_symbol(chain_declaration(
                    checker,
                    0,
                    SyntaxKind::InterfaceDeclaration,
                    "Shared",
                ))
                .expect("interface");
            let original_globals = Arc::clone(&checker.globals);
            checker
                .emit_precalculate_declaration_emit_visibility(root)
                .expect("prepare");
            checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(root), false)
                .expect("original globals")
                .expect("visible interface");
            let generation = prepared_chain_cache(checker).generation;
            checker.globals = Arc::new(SymbolTable::default());
            assert!(checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(root), false)
                .expect("replaced globals")
                .is_none());
            assert!(prepared_chain_cache(checker).generation > generation);
            checker.globals = original_globals;
            let restored = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(root), false)
                .expect("restored globals")
                .expect("visible interface");
            assert!(matches!(restored, AccessibleSymbolChain::Owned(_)));
            let repeat = checker
                .accessible_symbol_chain_at(symbol, SymbolFlags::TYPE, Some(root), false)
                .expect("repeat")
                .expect("visible interface");
            assert!(matches!(repeat, AccessibleSymbolChain::Retained(_)));
        },
    );
}
