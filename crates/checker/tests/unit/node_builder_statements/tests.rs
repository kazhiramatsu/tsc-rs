use tsc_emitter::{SourceFileId, TransformArena, TransformNode, TransformSourceId};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;

use super::*;

fn with_declaration_statements(
    files: &[(&str, &str)],
    target_index: usize,
    options: &CompilerOptions,
    verbosity: Option<i32>,
    run: impl FnOnce(&mut CheckerState<'_>, &mut TransformArena, TransformSourceId, Vec<TransformNode>),
) {
    with_program_state(files, options, |checker| {
        let root = checker.binder.source(target_index).root;
        let table = checker
            .binder
            .node_symbol(root)
            .map(|symbol| checker.binder.symbol(symbol).exports.clone())
            .or_else(|| checker.binder.locals_of(root).cloned())
            .expect("source-file symbol table");
        let mut arena = TransformArena::new();
        let targets = (0..checker.binder.file_count())
            .map(|index| {
                arena.add_source(
                    checker.binder.source(index),
                    Some(SourceFileId::from_raw(index as u32)),
                )
            })
            .collect::<Vec<_>>();
        let target = targets[target_index];
        let mut statements = None;
        let result = with_context(
            checker,
            &mut arena,
            target,
            Some(root),
            Some(EmitNodeBuilderFlags::NONE),
            Some(EmitInternalNodeBuilderFlags::NONE),
            None,
            None,
            verbosity,
            |checker, arena, target, context| {
                statements = Some(symbol_table_to_declaration_statements(
                    checker, arena, target, &table, context,
                )?);
                Ok(())
            },
            None,
        )
        .expect("statement serialization succeeds");
        assert!(result.is_some(), "node-builder context remains valid");
        run(
            checker,
            &mut arena,
            target,
            statements.expect("serializer callback ran"),
        );
    });
}

fn node(arena: &TransformArena, node: TransformNode) -> &tsc_syntax::Node {
    arena.node(node).expect("transform node")
}

fn child(arena: &TransformArena, parent: TransformNode, child: Option<NodeId>) -> TransformNode {
    arena
        .node_ref(parent.source(), child.expect("child node"))
        .expect("child belongs to statement source")
}

fn array_nodes(
    arena: &TransformArena,
    parent: TransformNode,
    array: Option<NodeArrayId>,
) -> Vec<TransformNode> {
    let Some(array) = array.and_then(|array| arena.node_array_ref(parent.source(), array)) else {
        return Vec::new();
    };
    arena
        .node_array(array)
        .expect("node array")
        .nodes
        .iter()
        .filter_map(|&node| arena.node_ref(parent.source(), node))
        .collect()
}

fn name_text(arena: &TransformArena, parent: TransformNode, name: Option<NodeId>) -> String {
    let name = child(arena, parent, name);
    match &node(arena, name).data {
        NodeData::Identifier(data) => data.text.clone(),
        NodeData::PrivateIdentifier(data) => data.text.clone(),
        NodeData::StringLiteral(data) => data.text.clone(),
        NodeData::NumericLiteral(data) => data.text.clone(),
        data => panic!("unexpected declaration name: {data:?}"),
    }
}

fn find_statement(
    statements: &[TransformNode],
    arena: &TransformArena,
    kind: SyntaxKind,
) -> TransformNode {
    statements
        .iter()
        .copied()
        .find(|&statement| node(arena, statement).kind == kind)
        .unwrap_or_else(|| {
            panic!(
                "missing {kind:?} in {:#?}",
                statements
                    .iter()
                    .map(|&statement| node(arena, statement).kind)
                    .collect::<Vec<_>>()
            )
        })
}

fn module_statements(arena: &TransformArena, module: TransformNode) -> Vec<TransformNode> {
    let NodeData::ModuleDeclaration(data) = &node(arena, module).data else {
        panic!("module declaration expected")
    };
    let body = child(arena, module, data.body);
    let NodeData::ModuleBlock(data) = &node(arena, body).data else {
        panic!("module block expected")
    };
    array_nodes(arena, body, data.statements)
}

#[test]
fn js_exported_function_preserves_expando_namespace_shape() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..CompilerOptions::default()
    };
    with_declaration_statements(
        &[(
            "/main.js",
            "/** @param {number} value @returns {string} */\nfunction api(value) { return String(value); }\napi.version = 1;\nmodule.exports = api;\n",
        )],
        0,
        &options,
        None,
        |_checker, arena, _target, statements| {
            assert!(statements.iter().any(|&statement| {
                node(arena, statement).kind == SyntaxKind::FunctionDeclaration
            }));
            let namespace = find_statement(&statements, arena, SyntaxKind::ModuleDeclaration);
            let NodeData::ModuleDeclaration(namespace_data) = &node(arena, namespace).data
            else {
                unreachable!()
            };
            assert_eq!(name_text(arena, namespace, namespace_data.name), "api");
            let members = module_statements(arena, namespace);
            assert!(members.iter().any(|&member| {
                node(arena, member).kind == SyntaxKind::VariableStatement
            }));
            assert!(statements.iter().any(|&statement| {
                node(arena, statement).kind == SyntaxKind::ExportAssignment
            }));
        },
    );
}

#[test]
fn class_synthesis_emits_heritage_members_and_cloned_hash_private_name() {
    with_declaration_statements(
        &[(
            "/main.ts",
            "class Base { base = 0; }\nexport class Derived extends Base { #secret = 1; value = ''; method(x: number): string { return String(x); } get size() { return 1; } set size(value: number) {} }\n",
        )],
        0,
        &CompilerOptions::default(),
        Some(1),
        |_checker, arena, _target, statements| {
            let derived = statements
                .iter()
                .copied()
                .find(|&statement| match &node(arena, statement).data {
                    NodeData::ClassDeclaration(data) => {
                        name_text(arena, statement, data.name) == "Derived"
                    }
                    _ => false,
                })
                .expect("Derived declaration");
            let NodeData::ClassDeclaration(data) = &node(arena, derived).data else {
                unreachable!()
            };
            assert_ne!(arena.get_original_node(derived), derived);
            assert_eq!(array_nodes(arena, derived, data.heritage_clauses).len(), 1);
            let members = array_nodes(arena, derived, data.members);
            let member_kinds = members
                .iter()
                .map(|&member| node(arena, member).kind)
                .collect::<Vec<_>>();
            assert!(member_kinds.contains(&SyntaxKind::MethodDeclaration));
            assert!(member_kinds.contains(&SyntaxKind::GetAccessor));
            assert!(member_kinds.contains(&SyntaxKind::SetAccessor));
            let private_member = members
                .iter()
                .copied()
                .find(|&member| match &node(arena, member).data {
                    NodeData::PropertyDeclaration(data) => {
                        name_text(arena, member, data.name) == "#secret"
                    }
                    _ => false,
                })
                .expect("cloned hash-private member");
            let NodeData::PropertyDeclaration(private_data) =
                &node(arena, private_member).data
            else {
                unreachable!()
            };
            let private_name = child(arena, private_member, private_data.name);
            assert_ne!(arena.get_original_node(private_name), private_name);
        },
    );
}

#[test]
fn nested_namespace_and_const_regular_enums_keep_statement_shapes() {
    with_declaration_statements(
        &[(
            "/main.ts",
            "export namespace Outer { export namespace Inner { export const value = 1; } }\nexport const enum ConstKind { A = 1, B = 3 }\nexport enum RegularKind { X = 'x', Y = 'y' }\n",
        )],
        0,
        &CompilerOptions::default(),
        None,
        |_checker, arena, _target, statements| {
            let outer = find_statement(&statements, arena, SyntaxKind::ModuleDeclaration);
            let outer_members = module_statements(arena, outer);
            let inner = find_statement(&outer_members, arena, SyntaxKind::ModuleDeclaration);
            assert!(module_statements(arena, inner).iter().any(|&member| {
                node(arena, member).kind == SyntaxKind::VariableStatement
            }));

            let enums = statements
                .iter()
                .copied()
                .filter(|&statement| {
                    node(arena, statement).kind == SyntaxKind::EnumDeclaration
                })
                .collect::<Vec<_>>();
            assert_eq!(enums.len(), 2);
            for declaration in enums {
                let NodeData::EnumDeclaration(data) = &node(arena, declaration).data else {
                    unreachable!()
                };
                let members = array_nodes(arena, declaration, data.members);
                assert_eq!(members.len(), 2);
                assert!(members.iter().all(|&member| {
                    matches!(
                        &node(arena, member).data,
                        NodeData::EnumMember(data) if data.initializer.is_some()
                    )
                }));
                let flags = transform_modifier_flags(
                    arena,
                    declaration.source(),
                    data.modifiers,
                )
                .expect("enum modifiers");
                if name_text(arena, declaration, data.name) == "ConstKind" {
                    assert!(flags.intersects(ModifierFlags::CONST));
                } else {
                    assert!(!flags.intersects(ModifierFlags::CONST));
                }
            }
        },
    );
}

#[test]
fn import_equals_and_export_equals_are_composed() {
    with_declaration_statements(
        &[
            ("/dep.ts", "export class Item {}\n"),
            (
                "/main.ts",
                "import Dependency = require('./dep');\nexport = Dependency;\n",
            ),
        ],
        1,
        &CompilerOptions::default(),
        None,
        |_checker, arena, _target, statements| {
            assert!(
                statements.iter().any(|&statement| {
                    node(arena, statement).kind == SyntaxKind::ImportEqualsDeclaration
                }),
                "statement kinds: {:?}",
                statements
                    .iter()
                    .map(|&statement| node(arena, statement).kind)
                    .collect::<Vec<_>>()
            );
            let assignment = find_statement(&statements, arena, SyntaxKind::ExportAssignment);
            let NodeData::ExportAssignment(data) = &node(arena, assignment).data else {
                unreachable!()
            };
            assert_eq!(data.is_export_equals, Some(true));
        },
    );
}

#[test]
fn alias_reexport_keeps_module_specifier_and_named_export() {
    with_declaration_statements(
        &[
            ("/dep.ts", "export class Item {}\n"),
            ("/main.ts", "export { Item as Renamed } from './dep';\n"),
        ],
        1,
        &CompilerOptions::default(),
        None,
        |_checker, arena, _target, statements| {
            let declaration = find_statement(&statements, arena, SyntaxKind::ExportDeclaration);
            let NodeData::ExportDeclaration(data) = &node(arena, declaration).data else {
                unreachable!()
            };
            let specifier = child(arena, declaration, data.module_specifier);
            let NodeData::StringLiteral(specifier) = &node(arena, specifier).data else {
                panic!("string module specifier expected")
            };
            assert_eq!(specifier.text, "./dep");
            assert!(data.export_clause.is_some());
        },
    );
}

#[test]
fn redundant_alias_reexports_are_merged_without_extra_specifiers() {
    with_declaration_statements(
        &[
            (
                "/dep.ts",
                "export class A {}\nexport interface B { value: number; }\n",
            ),
            ("/main.ts", "export { A as X, B as Y } from './dep';\n"),
        ],
        1,
        &CompilerOptions::default(),
        None,
        |_checker, arena, _target, statements| {
            let exports = statements
                .iter()
                .copied()
                .filter(|&statement| node(arena, statement).kind == SyntaxKind::ExportDeclaration)
                .collect::<Vec<_>>();
            assert_eq!(exports.len(), 1);
            let declaration = exports[0];
            let NodeData::ExportDeclaration(data) = &node(arena, declaration).data else {
                unreachable!()
            };
            let clause = child(arena, declaration, data.export_clause);
            let NodeData::NamedExports(data) = &node(arena, clause).data else {
                panic!("named exports expected")
            };
            assert_eq!(array_nodes(arena, clause, data.elements).len(), 2);
        },
    );
}

#[test]
fn interface_and_type_alias_are_synthesized_with_members_and_parameters() {
    with_declaration_statements(
        &[(
            "/main.ts",
            "export interface Box<T> { value: T; get(): T; }\nexport type Maybe<T> = T | undefined;\n",
        )],
        0,
        &CompilerOptions::default(),
        None,
        |_checker, arena, _target, statements| {
            let interface =
                find_statement(&statements, arena, SyntaxKind::InterfaceDeclaration);
            let NodeData::InterfaceDeclaration(data) = &node(arena, interface).data else {
                unreachable!()
            };
            assert_eq!(
                array_nodes(arena, interface, data.type_parameters).len(),
                1
            );
            assert_eq!(array_nodes(arena, interface, data.members).len(), 2);
            let alias = find_statement(&statements, arena, SyntaxKind::TypeAliasDeclaration);
            let NodeData::TypeAliasDeclaration(data) = &node(arena, alias).data else {
                unreachable!()
            };
            assert_eq!(array_nodes(arena, alias, data.type_parameters).len(), 1);
            assert!(data.r#type.is_some());
        },
    );
}

#[test]
fn unused_name_mangling_is_stable_for_colliding_authoring_names() {
    with_program_state(
        &[(
            "/main.ts",
            "class Taken {}\nclass Other {}\nexport { Taken, Other };\n",
        )],
        &CompilerOptions::default(),
        |checker| {
            let root = checker.binder.source(0).root;
            let mut arena = TransformArena::new();
            let target =
                arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
            let result = with_context(
                checker,
                &mut arena,
                target,
                Some(root),
                None,
                None,
                None,
                None,
                None,
                |checker, arena, target, context| {
                    context
                        .used_symbol_names
                        .get_or_insert_with(HashSet::new)
                        .insert("Taken".to_owned());
                    let mut serializer = StatementSerializer::new(checker, arena, target, context);
                    assert_eq!(serializer.get_unused_name("Taken", None), "Taken_1");
                    assert_eq!(serializer.get_unused_name("Taken", None), "Taken_2");
                    Ok(())
                },
                None,
            )
            .expect("name generation succeeds");
            assert!(result.is_some());
        },
    );
}

#[test]
fn symbol_to_declarations_simplifies_class_interface_enum_and_module_modifiers() {
    with_program_state(
        &[(
            "/main.ts",
            "export abstract class C { abstract value: number; }\nexport interface I { value: number; }\nexport const enum E { A }\nexport namespace N { export const value = 1; }\n",
        )],
        &CompilerOptions::default(),
        |checker| {
            let root = checker.binder.source(0).root;
            let exports = checker
                .binder
                .node_symbol(root)
                .map(|symbol| checker.binder.symbol(symbol).exports.clone())
                .expect("module exports");
            let mut arena = TransformArena::new();
            let target = arena.add_source(
                checker.binder.source(0),
                Some(SourceFileId::from_raw(0)),
            );
            for (name, meaning, expected_kind, retained) in [
                (
                    "C",
                    EmitSymbolMeaning::TYPE,
                    SyntaxKind::ClassDeclaration,
                    ModifierFlags::ABSTRACT,
                ),
                (
                    "I",
                    EmitSymbolMeaning::TYPE,
                    SyntaxKind::InterfaceDeclaration,
                    ModifierFlags::NONE,
                ),
                (
                    "E",
                    EmitSymbolMeaning::TYPE,
                    SyntaxKind::EnumDeclaration,
                    ModifierFlags::CONST,
                ),
                (
                    "N",
                    EmitSymbolMeaning::NAMESPACE,
                    SyntaxKind::ModuleDeclaration,
                    ModifierFlags::NONE,
                ),
            ] {
                let symbol = exports
                    .get(name)
                    .copied()
                    .unwrap_or_else(|| panic!("missing {name}"));
                let declarations = symbol_to_declarations(
                    checker,
                    &mut arena,
                    target,
                    symbol,
                    meaning,
                    EmitNodeBuilderFlags::NONE,
                    None,
                    None,
                    None,
                )
                .expect("symbol declaration synthesis");
                let declaration = declarations
                    .iter()
                    .copied()
                    .find(|&declaration| node(&arena, declaration).kind == expected_kind)
                    .unwrap_or_else(|| panic!("missing simplified {expected_kind:?}"));
                let flags = transform_modifier_flags(
                    &arena,
                    declaration.source(),
                    modifiers_of(&node(&arena, declaration).data),
                )
                .expect("simplified modifiers");
                assert!(!flags.intersects(ModifierFlags::EXPORT | ModifierFlags::AMBIENT));
                assert_eq!(flags.intersects(retained), !retained.is_empty());
            }
        },
    );
}

#[test]
fn javascript_require_property_alias_emits_generated_import_then_qualified_alias() {
    let options = CompilerOptions {
        allow_js: true,
        declaration: Some(true),
        ..CompilerOptions::default()
    };
    with_declaration_statements(
        &[
            ("/m.js", "exports.y = 1;\n"),
            (
                "/main.js",
                "const y = require(\"./m\").y;\nexports.y = y;\n",
            ),
        ],
        1,
        &options,
        None,
        |_checker, arena, _target, statements| {
            let imports = statements
                .iter()
                .copied()
                .filter(|&statement| {
                    node(arena, statement).kind == SyntaxKind::ImportEqualsDeclaration
                })
                .collect::<Vec<_>>();
            assert_eq!(imports.len(), 2, "expected require binding plus alias");

            let NodeData::ImportEqualsDeclaration(first) = &node(arena, imports[0]).data else {
                unreachable!()
            };
            assert!(first.modifiers.is_none());
            let generated = child(arena, imports[0], first.name);
            assert_eq!(name_text(arena, imports[0], first.name), "y");
            assert!(
                arena.metadata(generated).is_some(),
                "first import name carries generated-binding metadata",
            );
            assert_eq!(arena.generated_binding_base(generated), Some("y"));
            let external = child(arena, imports[0], first.module_reference);
            assert_eq!(
                node(arena, external).kind,
                SyntaxKind::ExternalModuleReference,
            );

            let NodeData::ImportEqualsDeclaration(second) = &node(arena, imports[1]).data else {
                unreachable!()
            };
            assert!(second.modifiers.is_some());
            assert_eq!(name_text(arena, imports[1], second.name), "y");
            let qualified = child(arena, imports[1], second.module_reference);
            let NodeData::QualifiedName(qualified_data) = &node(arena, qualified).data else {
                panic!("second import must reference a qualified name")
            };
            assert_eq!(qualified_data.left, Some(generated.node()));
            assert_eq!(name_text(arena, qualified, qualified_data.right), "y");
        },
    );
}
