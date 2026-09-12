use tsc_emitter::{
    create_printer, transform_nodes, NewLineKind, PrintRequest, PrinterOptions, SourceFileId,
    StandaloneWriter, TransformArena, TransformNode, TransformSourceId,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;

use super::*;

#[test]
fn implements_reuse_restores_synthetic_scope_after_success_and_factory_error() {
    let options = CompilerOptions {
        allow_js: true,
        declaration: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[(
            "/main.js",
            "class A {}\n/** @implements {A} */\nclass Foo {}\n",
        )],
        &options,
        |checker| {
            let root = checker.binder.source(0).root;
            let locals = checker.binder.locals_of(root).unwrap();
            let foo = locals["Foo"];
            let declaration = checker.binder.symbol(foo).value_declaration.unwrap();
            let clauses = checker
                .get_effective_implements_type_nodes(declaration)
                .unwrap();
            assert_eq!(clauses.len(), 1);
            let mut arena = TransformArena::new();
            let target =
                arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
            let mut other_arena = TransformArena::new();
            other_arena.add_source(checker.binder.source(0), None);
            let absent_target = other_arena.add_source(checker.binder.source(0), None);
            assert!(arena.source(absent_target).is_err());
            with_context(
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
                    for kind in [SyntaxKind::ModuleDeclaration, SyntaxKind::Block] {
                        let conflicting_locals = HashMap::from([("A".to_owned(), foo)]);
                        context.enclosing_declaration = Some(root);
                        context.enclosing_declaration_is_synthetic = true;
                        context.synthetic_scope_kind = Some(kind);
                        context.synthetic_scope_locals = Some(conflicting_locals.clone());
                        for destination in [target, absent_target] {
                            let result =
                                StatementSerializer::new(checker, arena, destination, context)
                                    .sanitize_jsdoc_implements(&clauses);
                            if destination == target {
                                assert_eq!(result.unwrap().unwrap().len(), 1);
                            } else {
                                assert!(matches!(result, Err(EmitResolverError::Factory { .. })));
                            }
                            assert_eq!(context.enclosing_declaration, Some(root));
                            assert!(context.enclosing_declaration_is_synthetic);
                            assert_eq!(context.synthetic_scope_kind, Some(kind));
                            assert_eq!(
                                context.synthetic_scope_locals.as_ref(),
                                Some(&conflicting_locals)
                            );
                        }
                    }
                    Ok(())
                },
                None,
            )
            .expect("the caller observes the typed factory error after scope restoration");
        },
    );
}

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

fn assert_property_require_alias_shape(main_text: &str, expected_generated_name: &str) {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        declaration: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[("/m.js", "exports.y = 1;\n"), ("/main.js", main_text)],
        &options,
        |checker| {
            let root = checker.binder.source(1).root;
            let table = checker
                .binder
                .locals_of(root)
                .cloned()
                .expect("main.js locals");
            let mut arena_owner = TransformArena::new();
            let targets = (0..checker.binder.file_count())
                .map(|index| {
                    arena_owner.add_source(
                        checker.binder.source(index),
                        Some(SourceFileId::from_raw(index as u32)),
                    )
                })
                .collect::<Vec<_>>();
            let target = targets[1];
            let mut statements = None;
            with_context(
                checker,
                &mut arena_owner,
                target,
                Some(root),
                Some(EmitNodeBuilderFlags::NONE),
                Some(EmitInternalNodeBuilderFlags::NONE),
                None,
                None,
                None,
                |checker, arena, target, context| {
                    statements = Some(symbol_table_to_declaration_statements(
                        checker, arena, target, &table, context,
                    )?);
                    Ok(())
                },
                None,
            )
            .expect("property require alias serialization succeeds");
            let statements = statements.expect("serializer callback ran");
            let arena = &mut arena_owner;
            let imports = statements
                .iter()
                .copied()
                .filter(|&statement| {
                    node(arena, statement).kind == SyntaxKind::ImportEqualsDeclaration
                })
                .collect::<Vec<_>>();
            assert_eq!(
                imports.len(),
                2,
                "property require alias emits two imports; produced kinds: {:?}",
                statements
                    .iter()
                    .map(|&statement| node(arena, statement).kind)
                    .collect::<Vec<_>>()
            );

            let NodeData::ImportEqualsDeclaration(first) = &node(arena, imports[0]).data else {
                unreachable!()
            };
            let first_name = child(arena, imports[0], first.name);
            assert_eq!(node(arena, first_name).kind, SyntaxKind::Identifier);
            assert_eq!(arena.generated_binding_base(first_name), Some("y"));
            let first_reference = child(arena, imports[0], first.module_reference);
            let NodeData::ExternalModuleReference(reference) = &node(arena, first_reference).data
            else {
                panic!("first import must use an external module reference")
            };
            let specifier = child(arena, first_reference, reference.expression);
            let NodeData::StringLiteral(specifier) = &node(arena, specifier).data else {
                panic!("external module reference must hold a string literal")
            };
            assert!(!specifier.text.is_empty());

            let NodeData::ImportEqualsDeclaration(second) = &node(arena, imports[1]).data else {
                unreachable!()
            };
            assert_eq!(name_text(arena, imports[1], second.name), "y");
            let second_name = child(arena, imports[1], second.name);
            assert_eq!(arena.generated_binding_base(second_name), None);
            let qualified = child(arena, imports[1], second.module_reference);
            let NodeData::QualifiedName(qualified_data) = &node(arena, qualified).data else {
                panic!("second import must use a qualified name")
            };
            let qualified_left = child(arena, qualified, qualified_data.left);
            let qualified_right = child(arena, qualified, qualified_data.right);
            assert_eq!(qualified_left, first_name, "generated identifier identity");
            assert_eq!(
                name_text(arena, qualified, Some(qualified_right.node())),
                "y"
            );
            // w4 T7: upstream's `canHaveExportModifier` excludes
            // ImportEqualsDeclaration, so the synthesized property alias carries
            // no inline `export` (the export goes through an export specifier).
            assert!(!array_nodes(arena, imports[1], second.modifiers)
                .iter()
                .any(|&modifier| node(arena, modifier).kind == SyntaxKind::ExportKeyword));

            let mut display = transform_nodes(std::mem::take(arena), Vec::new(), Vec::new(), true)
                .expect("checker-built alias arena becomes a print result");
            create_printer(
                PrinterOptions::new(NewLineKind::LineFeed).with_declaration_syntax(true),
            )
            .print(
                &mut display,
                PrintRequest::StandaloneNode {
                    node: imports[1],
                    writer: StandaloneWriter::MultiLine,
                },
                None,
            )
            .expect("print second import-equals declaration");
            let NodeData::Identifier(generated) = &display
                .arena()
                .node(first_name)
                .expect("generated identifier after print")
                .data
            else {
                unreachable!()
            };
            assert_eq!(generated.text, expected_generated_name);
        },
    );
}

#[test]
fn property_require_alias_emits_unique_pair_and_finalizes_against_source_names() {
    assert_property_require_alias_shape("const y = require(\"./m\").y;\n", "y_1");
    assert_property_require_alias_shape("const y_1 = 0;\nconst y = require(\"./m\").y;\n", "y_2");
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
            // w4 T7: no inline `export` on a synthesized import-equals alias
            // (upstream `canHaveExportModifier` excludes ImportEqualsDeclaration).
            assert!(!array_nodes(arena, imports[1], second.modifiers)
                .iter()
                .any(|&modifier| node(arena, modifier).kind == SyntaxKind::ExportKeyword));
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

#[test]
fn setter_name_serialization_queries_the_write_type_only_when_emitting_it() {
    let options = CompilerOptions {
        allow_js: true,
        declaration: Some(true),
        ..CompilerOptions::default()
    };
    for private in [false, true] {
        let source = format!(
            "class Foo {{\n/**\n{} * @param {{number}} supplied\n */\nset x(supplied) {{}}\n}}\n",
            if private { " * @private\n" } else { "" },
        );
        with_program_state(&[("/main.js", &source)], &options, |checker| {
            let root = checker.binder.source(0).root;
            let foo = checker.binder.locals_of(root).unwrap()["Foo"];
            let property = checker.binder.symbol(foo).members["x"];
            let setter = checker.binder.symbol(property).declarations[0];
            assert_eq!(checker.kind_of(setter), SyntaxKind::SetAccessor);
            assert!(checker
                .links
                .symbol(property)
                .write_type
                .resolved()
                .is_none());
            // Isolate the write-type cache from the signature's own lazy work.
            checker.get_signature_from_declaration(setter).unwrap();
            assert!(checker
                .links
                .symbol(property)
                .write_type
                .resolved()
                .is_none());
            let mut arena = TransformArena::new();
            let target =
                arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
            with_context(
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
                    let members = StatementSerializer::new(checker, arena, target, context)
                        .make_serialize_property_symbol(property, false, None, true, true)?;
                    assert_eq!(members.len(), 1);
                    let NodeData::SetAccessor(data) = &node(arena, members[0]).data else {
                        panic!("expected a setter");
                    };
                    let parameters = array_nodes(arena, members[0], data.parameters);
                    assert_eq!(parameters.len(), 1);
                    let NodeData::Parameter(data) = &node(arena, parameters[0]).data else {
                        panic!("expected a parameter");
                    };
                    assert_eq!(name_text(arena, parameters[0], data.name), "supplied");
                    assert_eq!(data.r#type.is_none(), private);
                    assert_eq!(
                        checker
                            .links
                            .symbol(property)
                            .write_type
                            .resolved()
                            .is_none(),
                        private
                    );
                    Ok(())
                },
                None,
            )
            .unwrap();
        });
    }
}

#[test]
fn setter_name_serialization_rejects_a_set_accessor_without_a_declaration() {
    let options = CompilerOptions {
        allow_js: true,
        declaration: Some(true),
        ..CompilerOptions::default()
    };
    with_program_state(
        &[("/main.js", "class Foo { set x(supplied) {} }")],
        &options,
        |checker| {
            let root = checker.binder.source(0).root;
            let foo = checker.binder.locals_of(root).unwrap()["Foo"];
            let property = checker.binder.symbol(foo).members["x"];
            assert!(checker
                .binder
                .symbol(property)
                .flags
                .intersects(SymbolFlags::SET_ACCESSOR));
            // Deliberately corrupt the internal flag/declaration invariant.
            // Source inputs, clones and merged symbols preserve these declarations.
            let property = checker.clone_symbol(property);
            checker.binder.symbol_mut(property).declarations.clear();
            let mut arena = TransformArena::new();
            let target =
                arena.add_source(checker.binder.source(0), Some(SourceFileId::from_raw(0)));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                with_context(
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
                        StatementSerializer::new(checker, arena, target, context)
                            .make_serialize_property_symbol(property, false, None, true, true)?;
                        Ok(())
                    },
                    None,
                )
            }));
            let panic = result.expect_err("an absent setter must not become a value parameter");
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied());
            assert_eq!(
                message,
                Some("SetAccessor symbol requires a setter declaration")
            );
        },
    );
}

// These fixtures are the ordinary/instrumented TS commands frozen by the
// declaration-comment-range observation notebook before native edits.
fn declaration_comment_range_traces() -> serde_json::Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../compiler/tests/fixtures/declaration-comment-range-traces.json"
    )))
    .unwrap()
}

fn declaration_comment_range_with_context(
    checker: &mut CheckerState<'_>,
    run: impl FnOnce(
        &mut CheckerState<'_>,
        &mut TransformArena,
        TransformSourceId,
        &mut NodeBuilderContext<'_>,
    ) -> BuildResult<()>,
) {
    let root = checker.binder.source(0).root;
    let mut arena = TransformArena::new();
    let targets = (0..checker.binder.file_count())
        .map(|index| {
            arena.add_source(
                checker.binder.source(index),
                Some(SourceFileId::from_raw(index as u32)),
            )
        })
        .collect::<Vec<_>>();
    assert!(with_context(
        checker,
        &mut arena,
        targets[0],
        Some(root),
        None,
        None,
        None,
        None,
        None,
        run,
        None,
    )
    .unwrap()
    .is_some());
}

fn declaration_comment_range_description(
    arena: &TransformArena,
    value: TransformNode,
) -> serde_json::Value {
    let value_node = node(arena, value);
    serde_json::json!({
        "kind": format!("{:?}", value_node.kind),
        "pos": value_node.pos as i32,
        "end": value_node.end as i32,
        "file": arena.source(value.source()).unwrap().syntax().file_name,
    })
}

#[test]
fn declaration_comment_range_classifier_matches_js_and_ts_observations() {
    let traces = declaration_comment_range_traces();
    let cases = traces["predicates"].as_array().unwrap();
    assert_eq!(cases.len(), 24);
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let filename = format!("/project/main.{}", id.rsplit('/').next().unwrap());
        for _ in 0..2 {
            with_program_state(
                &[(filename.as_str(), case["source"].as_str().unwrap())],
                &CompilerOptions {
                    allow_js: true,
                    ..CompilerOptions::default()
                },
                |checker| {
                    let source = checker.binder.source(0);
                    let expression = source
                        .arena
                        .node_ids()
                        .find(|&id| source.arena.node(id).kind == SyntaxKind::BinaryExpression)
                        .unwrap();
                    assert_eq!(
                        tsc_binder::assignment::get_assignment_declaration_kind(source, expression)
                            as u32,
                        case["assignment_kind"].as_u64().unwrap() as u32,
                        "{id}",
                    );
                },
            );
        }
    }
}

#[test]
fn declaration_comment_range_g4a_internal_sentinels() {
    // The upstream sentinel controls construct incomplete topology. Remove
    // the same edges before binding; ordinary Program topology is untouched.
    let traces = declaration_comment_range_traces();
    assert_eq!(traces["sentinels"].as_array().unwrap().len(), 4);
    for case in traces["sentinels"].as_array().unwrap() {
        let id = case["case_id"].as_str().unwrap();
        for _ in 0..2 {
            let mut source = tsc_syntax::parse_source_file(
                "/project/main.ts",
                "const fn = function() {};",
                tsc_syntax::ParseOptions::default(),
                None,
            );
            let declaration = source
                .arena
                .node_ids()
                .find(|&id| source.arena.node(id).kind == SyntaxKind::FunctionExpression)
                .unwrap();
            let variable = source.arena.node(declaration).parent.unwrap();
            let list = source.arena.node(variable).parent.unwrap();
            match id {
                "missing-declaration" | "parentless-declaration" => {
                    source.arena.node_mut(declaration).parent = None;
                }
                "variable-without-list" => source.arena.node_mut(variable).parent = None,
                "variable-with-list" => {}
                _ => panic!("unknown sentinel {id}"),
            }
            let options = CompilerOptions::default();
            let mut binder = tsc_binder::Binder::new(&source, &options);
            binder.bind_source_file();
            let mut checker = CheckerState::new(&source, &binder, &options);
            let signature = checker.get_signature_from_declaration(declaration).unwrap();
            if id == "missing-declaration" {
                checker.signature_mut(signature).declaration = None;
            }
            declaration_comment_range_with_context(
                &mut checker,
                |checker, arena, target, context| {
                    let selected = StatementSerializer::new(checker, arena, target, context)
                        .get_signature_text_range_location(signature);
                    let description = match selected {
                        None => "absent",
                        Some(node) if node == declaration => "declaration",
                        Some(node) if node == list => "variable-list",
                        _ => "unexpected",
                    };
                    assert_eq!(description, case["selected"], "{id}");
                    Ok(())
                },
            );
        }
    }
}

#[test]
fn declaration_comment_range_preserves_range_and_original_identity() {
    let traces = declaration_comment_range_traces();
    assert_eq!(traces["ranges"].as_array().unwrap().len(), 4);
    for case in traces["ranges"].as_array().unwrap() {
        let id = case["case_id"].as_str().unwrap();
        for _ in 0..2 {
            with_program_state(
                &[
                    ("/project/a.ts", "type Local = number;"),
                    ("/project/b.ts", "type Foreign = string;"),
                ],
                &CompilerOptions::default(),
                |checker| {
                    declaration_comment_range_with_context(
                        checker,
                        |checker, arena, target, context| {
                            let mut names = Vec::new();
                            for index in 0..2 {
                                let source = checker.binder.source(index);
                                let name = source
                                    .arena
                                    .nodes()
                                    .iter()
                                    .find_map(|node| {
                                        if let NodeData::TypeAliasDeclaration(data) = &node.data {
                                            data.name
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap();
                                names.push(project_parse_node(checker, arena, name)?.unwrap());
                            }
                            let input = if id == "parsed-input" {
                                names[0]
                            } else {
                                let input = create_identifier(arena, target, "generated")?;
                                arena
                                    .set_original_node(input, Some(names[0]))
                                    .map_err(factory_error)?;
                                input
                            };
                            let location = match id {
                                "foreign-source" => Some(names[1]),
                                "no-location" => None,
                                _ => Some(names[0]),
                            };
                            let result = set_text_range2(checker, arena, context, input, location)?;
                            let original = arena.get_original_node(result);
                            let observed = serde_json::json!({
                                "same_identity": result == input,
                                "pos": node(arena, result).pos as i32,
                                "end": node(arena, result).end as i32,
                                "original": declaration_comment_range_description(arena, original),
                            });
                            assert_eq!(observed, case["observation"], "{id}");
                            let expected_original = if id == "foreign-source" {
                                names[1]
                            } else {
                                names[0]
                            };
                            assert_eq!(original, expected_original, "{id}: actual node identity");
                            assert_eq!(
                                arena
                                    .parse_tree_resolver_node(result)
                                    .map_err(factory_error)?,
                                arena
                                    .parse_tree_resolver_node(expected_original)
                                    .map_err(factory_error)?,
                                "{id}: mounted source identity",
                            );
                            Ok(())
                        },
                    )
                },
            );
        }
    }
}

fn declaration_comment_range_collect_signatures(
    arena: &TransformArena,
    current: TransformNode,
    signatures: &mut Vec<TransformNode>,
) {
    if matches!(
        node(arena, current).kind,
        SyntaxKind::FunctionDeclaration | SyntaxKind::MethodDeclaration
    ) {
        signatures.push(current);
    }
    let source = arena.source(current.source()).unwrap().syntax();
    tsc_syntax::for_each_child(&source.arena, node(arena, current), |id| {
        declaration_comment_range_collect_signatures(
            arena,
            arena.node_ref(current.source(), id).unwrap(),
            signatures,
        );
        false
    });
}

#[test]
fn declaration_comment_range_serialized_locations_match_upstream_traces() {
    let artifact: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../compiler/tests/fixtures/declaration-comment-ranges.json"
    )))
    .unwrap();
    let traces = declaration_comment_range_traces();
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 17);
    let mut failures = Vec::new();
    for (case, trace) in cases.iter().zip(traces["cases"].as_array().unwrap()) {
        let id = case["case_id"].as_str().unwrap();
        assert_eq!(case["case_id"], trace["case_id"]);
        // Blocking belongs to the ordinary command test. Direct serialization
        // would bypass that boundary and is not an observation of this case.
        if id.ends_with("/blocked-semantic") {
            continue;
        }
        for repetition in 0..2 {
            let result = std::panic::catch_unwind(|| {
                let files = case["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|file| {
                        (
                            file["path"].as_str().unwrap(),
                            file["text"].as_str().unwrap(),
                        )
                    })
                    .collect::<Vec<_>>();
                let options = CompilerOptions {
                    allow_js: true,
                    check_js: Some(true),
                    declaration: Some(true),
                    target: Some(case["options"]["target"].as_i64().unwrap() as i32),
                    module: Some(case["options"]["module"].as_i64().unwrap() as i32),
                    ..CompilerOptions::default()
                };
                with_declaration_statements(
                    &files,
                    0,
                    &options,
                    None,
                    |checker, arena, _, statements| {
                        let mut signatures = Vec::new();
                        for statement in statements {
                            declaration_comment_range_collect_signatures(
                                arena,
                                statement,
                                &mut signatures,
                            );
                        }
                        let expected = trace["trace"].as_array().unwrap();
                        assert_eq!(signatures.len(), expected.len(), "{id}: signature count");
                        for (signature, expected) in signatures.iter().copied().zip(expected) {
                            let original = arena.get_original_node(signature);
                            assert_eq!(
                                declaration_comment_range_description(arena, original),
                                expected["location"],
                                "{id}"
                            );
                            assert_eq!(
                                (node(arena, signature).pos, node(arena, signature).end),
                                (node(arena, original).pos, node(arena, original).end),
                                "{id}: raw range"
                            );
                            let source = checker.binder.source(0);
                            let declaration = source.arena.node_ids().find(|&id| {
                            let node = source.arena.node(id);
                            serde_json::json!({"kind":format!("{:?}",node.kind),"pos":node.pos as i32,"end":node.end as i32,"file":source.file_name}) == expected["declaration"]
                        }).unwrap();
                            let (parameters, return_type) = match &node(arena, signature).data {
                                NodeData::FunctionDeclaration(data) => {
                                    (data.parameters, data.r#type)
                                }
                                NodeData::MethodDeclaration(data) => (data.parameters, data.r#type),
                                _ => unreachable!(),
                            };
                            assert!(
                                return_type.is_some(),
                                "{id}: return type constructed before location"
                            );
                            let actual_names = array_nodes(arena, signature, parameters)
                                .iter()
                                .map(|&parameter| {
                                    let NodeData::Parameter(data) = &node(arena, parameter).data
                                    else {
                                        unreachable!()
                                    };
                                    assert!(data.r#type.is_some(), "{id}: parameter type retained");
                                    name_text(arena, parameter, data.name)
                                })
                                .collect::<Vec<_>>();
                            let expected_names = checker
                                .parameters_of_function(declaration)
                                .iter()
                                .map(|&parameter| {
                                    let NodeData::Parameter(data) =
                                        &source.arena.node(parameter).data
                                    else {
                                        unreachable!()
                                    };
                                    let NodeData::Identifier(data) =
                                        &source.arena.node(data.name.unwrap()).data
                                    else {
                                        unreachable!()
                                    };
                                    data.text.clone()
                                })
                                .collect::<Vec<_>>();
                            assert_eq!(actual_names, expected_names, "{id}: parameter order");
                            let mounted = project_parse_node(checker, arena, original.node())
                                .unwrap()
                                .unwrap();
                            assert_eq!(mounted, original, "{id}: selected source identity");
                        }
                    },
                );
            });
            if result.is_err() {
                failures.push(format!("{id} repetition {repetition}"));
            }
        }
    }
    assert!(failures.is_empty(), "native trace failures: {failures:?}");
}

#[test]
fn declaration_comment_range_g4b_absent_signature_has_no_property_fallback() {
    // An internal missing-declaration control, distinct from parsed Programs.
    // Keep the property's real assignment declaration to expose the removed
    // first_property_like fallback without changing its flags or topology.
    for _ in 0..2 {
        with_program_state(
            &[(
                "/project/main.js",
                "function C() {} C.prototype.m = function(value) { return value; };",
            )],
            &CompilerOptions {
                allow_js: true,
                declaration: Some(true),
                ..CompilerOptions::default()
            },
            |checker| {
                let root = checker.binder.source(0).root;
                let class = checker.binder.locals_of(root).unwrap()["C"];
                let property = checker.binder.symbol(class).members["m"];
                let method_type = checker.get_type_of_symbol(property).unwrap();
                let signatures = checker
                    .get_signatures_of_type(method_type, SignatureKind::Call)
                    .unwrap();
                assert_eq!(signatures.len(), 1);
                let signature = signatures[0];
                checker.get_return_type_of_signature(signature).unwrap();
                checker.signature_mut(signature).declaration = None;
                declaration_comment_range_with_context(
                    checker,
                    |checker, arena, target, context| {
                        let methods = StatementSerializer::new(checker, arena, target, context)
                            .make_serialize_property_symbol(property, false, None, true, true)?;
                        assert_eq!(methods.len(), 1);
                        let method = methods[0];
                        assert_eq!(node(arena, method).kind, SyntaxKind::MethodDeclaration);
                        assert_eq!(arena.get_original_node(method), method);
                        assert_eq!(node(arena, method).pos as i32, -1);
                        assert_eq!(node(arena, method).end as i32, -1);
                        Ok(())
                    },
                );
            },
        );
    }
}
