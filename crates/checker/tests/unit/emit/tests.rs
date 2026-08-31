use super::*;

use std::{path::Path, sync::Arc};

use serde_json::Value;
use tsc_binder::{node_util, BinderWorker};
use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_emitter::{
    create_printer, get_script_transformers, get_script_transformers_for_source, transform_nodes,
    EmitHost, EmitResolverError, EmitResolverMethod, EmitResolverNode, EmitResolverSymbol,
    EmitSource, EmitSymbolAccessibility, EmitSymbolMeaning, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileId, TransformArena, TransformRoot,
};
use tsc_syntax::{LanguageVariant, NodeData, NodeId, ParseOptions, SyntaxKind};
use tsc_types::{CompilerOptions, IdentityDomain, ModuleKind, ScriptTarget, SymbolFlags};

use crate::state::test_support::with_program_state;
use crate::{BoundDocument, ParsedDocument, ProgramSnapshot};

const ACTIVE_TRANSFORM_ORACLE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-active-transform.v1.json"
));

fn active_transform_oracle() -> Value {
    serde_json::from_slice(ACTIVE_TRANSFORM_ORACLE)
        .expect("H1.3 active-transform oracle is valid JSON")
}

struct CheckerEmitHost<'a> {
    options: &'a CompilerOptions,
    syntax: &'a tsc_syntax::SourceFile,
    source_ids: [SourceFileId; 1],
}

impl EmitHost for CheckerEmitHost<'_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.options
    }

    fn current_directory(&self) -> &Path {
        Path::new("/project")
    }

    fn common_source_directory(&self) -> &Path {
        Path::new("/project")
    }

    fn config_file_path(&self) -> Option<&Path> {
        None
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.source_ids
    }

    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        (id == self.source_ids[0]).then(|| {
            let path = Path::new(&self.syntax.file_name);
            EmitSource::new(id, path, path, true, None, Some(self.syntax))
        })
    }
}

fn checked_alias_session() -> (ProgramSnapshot, CompilerOptions, Vec<NodeId>, Vec<NodeId>) {
    let oracle = active_transform_oracle();
    let inputs = oracle["inputs"].as_array().expect("oracle inputs");
    let dependency_path = inputs[0]["path"].as_str().expect("dependency path");
    let dependency_text = inputs[0]["text"].as_str().expect("dependency text");
    let main_path = inputs[1]["path"].as_str().expect("main path");
    let main_text = inputs[1]["text"].as_str().expect("main text");
    let domain = IdentityDomain::reclaiming();
    let dependency = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        dependency_path.to_owned(),
        TextSnapshot::new(dependency_text.to_owned(), DocumentVersion::new("1")),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("dependency source identity allocation");
    let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        main_path.to_owned(),
        TextSnapshot::new(main_text.to_owned(), DocumentVersion::new("1")),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("source identity allocation");
    let dependency = Arc::new(dependency);
    let source = Arc::new(source);
    let options = CompilerOptions {
        target: Some(
            i32::try_from(
                oracle["compiler_options"]["target"]
                    .as_i64()
                    .expect("target"),
            )
            .expect("target fits i32"),
        ),
        module: Some(
            i32::try_from(
                oracle["compiler_options"]["module"]
                    .as_i64()
                    .expect("module"),
            )
            .expect("module fits i32"),
        ),
        use_define_for_class_fields: oracle["compiler_options"]["useDefineForClassFields"]
            .as_bool(),
        ..CompilerOptions::default()
    };
    assert_eq!(options.target, Some(ScriptTarget::ES_NEXT.bits()));
    let dependency_worker = BinderWorker::bind_in_identity_domain(&dependency, &options, &domain)
        .expect("dependency bind identity allocation");
    let worker = BinderWorker::bind_in_identity_domain(&source, &options, &domain)
        .expect("bind identity allocation");
    let dependency_document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&dependency))),
        dependency_worker.into_bind_data(),
    ));
    let document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&source))),
        worker.into_bind_data(),
    ));
    let program =
        ProgramSnapshot::new(vec![dependency_document, document], 0).expect("Program snapshot");
    let import_aliases = source
        .arena
        .node_ids()
        .filter(|id| {
            matches!(
                source.arena.node(*id).kind,
                SyntaxKind::ImportClause | SyntaxKind::ImportSpecifier
            )
        })
        .collect();
    let export_aliases = source
        .arena
        .node_ids()
        .filter(|id| source.arena.node(*id).kind == SyntaxKind::ExportSpecifier)
        .collect();
    (program, options, import_aliases, export_aliases)
}

fn named_declaration(
    state: &CheckerState<'_>,
    file: usize,
    kind: SyntaxKind,
    name: &str,
) -> NodeId {
    declarations_named(state, file, kind, name)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("missing {kind:?} declaration {name}"))
}

fn declarations_named(
    state: &CheckerState<'_>,
    file: usize,
    kind: SyntaxKind,
    name: &str,
) -> Vec<NodeId> {
    state
        .binder
        .source(file)
        .arena
        .node_ids()
        .filter(|&node| {
            state.kind_of(node) == kind
                && node_util::get_name_of_declaration(state.binder.source_of_node(node), node)
                    .and_then(|name_node| state.identifier_text_of(name_node))
                    == Some(name)
        })
        .collect()
}

fn parameter_named(state: &CheckerState<'_>, function: NodeId, name: &str) -> NodeId {
    state
        .parameters_of_function(function)
        .into_iter()
        .find(|&parameter| {
            matches!(
                state.data_of(parameter),
                NodeData::Parameter(data)
                    if data
                        .name
                        .and_then(|name_node| state.identifier_text_of(name_node))
                        == Some(name)
            )
        })
        .unwrap_or_else(|| panic!("missing parameter {name}"))
}

fn node_with_text(state: &CheckerState<'_>, file: usize, kind: SyntaxKind, text: &str) -> NodeId {
    state
        .binder
        .source(file)
        .arena
        .node_ids()
        .find(|&node| {
            state.kind_of(node) == kind && state.text_of_node(node).ok().as_deref() == Some(text)
        })
        .unwrap_or_else(|| panic!("missing {kind:?} with text {text:?}"))
}

#[test]
fn scoped_emit_resolver_reads_live_alias_and_constant_links_and_fails_closed_elsewhere() {
    let (snapshot, options, import_aliases, export_aliases) = checked_alias_session();
    assert_eq!(import_aliases.len(), 3);
    assert_eq!(export_aliases.len(), 2);

    let mut state = CheckerState::from_snapshot(&snapshot, &options);
    state.check_source_file(0);
    state.check_source_file(1);
    let symbol_index = state
        .binder
        .node_symbol(export_aliases[0])
        .expect("export alias symbol")
        .0;
    let session = CheckerSession::from_checked_state(state);

    let printed = session.with_emit_resolver(|resolver| {
        let node = |id| EmitResolverNode::from_raw_source(1, id);
        assert!(resolver
            .is_referenced_alias_declaration(node(import_aliases[2]))
            .expect("used default import reference"));
        assert!(!resolver
            .is_referenced_alias_declaration(node(import_aliases[0]))
            .expect("unused named import reference"));
        assert!(!resolver
            .is_referenced_alias_declaration(node(import_aliases[1]))
            .expect("type-only named import reference"));

        assert!(resolver
            .is_value_alias_declaration(node(export_aliases[0]))
            .expect("runtime export value"));
        assert!(!resolver
            .is_value_alias_declaration(node(export_aliases[1]))
            .expect("type export value"));

        assert_eq!(
            resolver
                .get_constant_value(node(export_aliases[0]))
                .expect("non-enum value query"),
            None
        );
        assert!(matches!(
            resolver.is_value_alias_declaration(EmitResolverNode::from_raw_source(
                99,
                export_aliases[0]
            )),
            Err(EmitResolverError::UnknownSource {
                method: EmitResolverMethod::IsValueAliasDeclaration,
                ..
            })
        ));
        assert!(matches!(
            resolver.is_value_alias_declaration(EmitResolverNode::from_raw_source(
                0,
                export_aliases[0]
            )),
            Err(EmitResolverError::SourceNodeMismatch {
                method: EmitResolverMethod::IsValueAliasDeclaration,
                actual_program_index: 1,
                ..
            })
        ));
        assert!(matches!(
            resolver
                .is_value_alias_declaration(EmitResolverNode::from_raw_source(1, NodeId(u32::MAX))),
            Err(EmitResolverError::UnknownNode {
                method: EmitResolverMethod::IsValueAliasDeclaration,
                ..
            })
        ));

        assert!(!resolver
            .is_definitely_reference_to_global_symbol_object(node(export_aliases[0]))
            .expect("global Symbol predicate on an export alias"));
        let accessibility = resolver
            .is_symbol_accessible(
                EmitResolverSymbol {
                    session_token: session.session_token,
                    symbol_index,
                },
                node(export_aliases[0]),
                EmitSymbolMeaning::VALUE_EXPORT_VALUE,
                true,
            )
            .expect("visibility-cluster symbol query");
        assert_ne!(
            accessibility.accessibility,
            EmitSymbolAccessibility::NotResolved
        );
        resolver
            .is_entity_name_visible(node(export_aliases[0]), node(export_aliases[0]))
            .expect("visibility-cluster entity query");
        assert!(!resolver
            .is_declaration_visible(node(export_aliases[0]))
            .expect("visibility-cluster declaration query"));
        assert!(!resolver
            .is_optional_parameter(node(export_aliases[0]))
            .expect("optional-parameter predicate on an export alias"));
        assert!(!resolver
            .is_implementation_of_overload(node(export_aliases[0]))
            .expect("overload predicate on an export alias"));
        assert!(!resolver
            .requires_adding_implicit_undefined(node(export_aliases[0]), None)
            .expect("implicit-undefined predicate on an export alias"));
        assert!(!resolver
            .is_expando_function_declaration(node(export_aliases[0]))
            .expect("expando predicate on an export alias"));
        assert!(resolver
            .get_properties_of_container_function(node(export_aliases[0]))
            .expect("container properties on an export alias")
            .is_empty());
        assert!(!resolver
            .is_literal_const_declaration(node(export_aliases[0]))
            .expect("literal-const predicate on an export alias"));
        assert!(!resolver
            .is_late_bound(node(export_aliases[0]))
            .expect("late-bound predicate on an export alias"));
        assert!(!resolver
            .is_import_required_by_augmentation(node(export_aliases[0]))
            .expect("augmentation predicate on an export alias"));

        let mut arena = TransformArena::new();
        let source = arena.add_source(
            snapshot.document(1).source(),
            Some(SourceFileId::from_raw(1)),
        );
        let mut transformed = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            get_script_transformers(&options, resolver).expect("bootstrap transformer list"),
            false,
        )
        .expect("transform with live checker resolver");
        create_printer(PrinterOptions::new(NewLineKind::LineFeed))
            .print(&mut transformed, PrintRequest::SourceFile(source), None)
            .expect("print while checker resolver remains live")
            .text()
            .to_owned()
    });
    let oracle = active_transform_oracle();
    let expected = oracle["observation"]["writes"]
        .as_array()
        .expect("oracle writes")
        .iter()
        .find(|write| write["path"] == "/project/main.js")
        .and_then(|write| write["text"].as_str())
        .expect("main JavaScript oracle write");
    assert_eq!(printed, expected);

    let state = session.into_state();
    assert_eq!(state.binder.file_count(), 2);
}

#[test]
fn dm_global_optional_and_overload_predicates_cover_adjacent_negative_shapes() {
    let source = concat!(
        "export {};\n",
        "const Symbol = {};\n",
        "globalThis.Symbol.value;\n",
        "Symbol.value;\n",
        "other.value;\n",
        "function optional(a?: string, b = 1) {}\n",
        "function required(c: number) {}\n",
        "(function (first, second) {})(1);\n",
        "function overloaded(value: string): string;\n",
        "function overloaded(value: number): number;\n",
        "function overloaded(value: string | number): string | number { return value; }\n",
        "function plain(value: string) { return value; }\n",
    );
    with_program_state(
        &[("predicates.ts", source)],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);

            let global_this_access = node_with_text(
                state,
                0,
                SyntaxKind::PropertyAccessExpression,
                "globalThis.Symbol.value",
            );
            let shadowed_symbol_access = node_with_text(
                state,
                0,
                SyntaxKind::PropertyAccessExpression,
                "Symbol.value",
            );
            let unrelated_access = node_with_text(
                state,
                0,
                SyntaxKind::PropertyAccessExpression,
                "other.value",
            );
            assert!(state
                .emit_is_definitely_reference_to_global_symbol_object(global_this_access)
                .expect("globalThis.Symbol property predicate"));
            assert!(!state
                .emit_is_definitely_reference_to_global_symbol_object(shadowed_symbol_access)
                .expect("shadowed Symbol property predicate"));
            assert!(!state
                .emit_is_definitely_reference_to_global_symbol_object(unrelated_access)
                .expect("unrelated property predicate"));

            let optional = named_declaration(state, 0, SyntaxKind::FunctionDeclaration, "optional");
            assert!(state
                .emit_is_optional_parameter(parameter_named(state, optional, "a"))
                .expect("question-mark parameter"));
            assert!(state
                .emit_is_optional_parameter(parameter_named(state, optional, "b"))
                .expect("initializer parameter"));
            let required = named_declaration(state, 0, SyntaxKind::FunctionDeclaration, "required");
            assert!(!state
                .emit_is_optional_parameter(parameter_named(state, required, "c"))
                .expect("required parameter"));

            let iife = state
                .binder
                .source(0)
                .arena
                .node_ids()
                .find(|&node| {
                    state.kind_of(node) == SyntaxKind::FunctionExpression
                        && state.parameters_of_function(node).len() == 2
                })
                .expect("IIFE function expression");
            assert!(!state
                .emit_is_optional_parameter(parameter_named(state, iife, "first"))
                .expect("provided IIFE parameter"));
            assert!(state
                .emit_is_optional_parameter(parameter_named(state, iife, "second"))
                .expect("omitted IIFE parameter"));

            let overloads =
                declarations_named(state, 0, SyntaxKind::FunctionDeclaration, "overloaded");
            let implementation = overloads
                .iter()
                .copied()
                .find(|&node| {
                    node_util::body_of(state.binder.source_of_node(node), node).is_some_and(
                        |body| {
                            !node_util::node_is_missing(
                                state.binder.source_of_node(node),
                                Some(body),
                            )
                        },
                    )
                })
                .expect("overload implementation");
            let overload_signature = overloads
                .iter()
                .copied()
                .find(|&node| node != implementation)
                .unwrap_or(overloads[0]);
            assert!(state
                .emit_is_implementation_of_overload(implementation)
                .expect("overload implementation predicate"));
            assert!(!state
                .emit_is_implementation_of_overload(overload_signature)
                .expect("overload signature predicate"));
            let plain = named_declaration(state, 0, SyntaxKind::FunctionDeclaration, "plain");
            assert!(!state
                .emit_is_implementation_of_overload(plain)
                .expect("single implementation predicate"));
        },
    );
}

#[test]
fn dm_implicit_undefined_and_expando_predicates_cover_type_and_modifier_gates() {
    let source = concat!(
        "function requiredInitialized(initialized = 1, required: number) {}\n",
        "function typedInitialized(initialized: number | undefined = 1, required: number) {}\n",
        "function errorInitialized(initialized: Missing = 1, required: number) {}\n",
        "class InitializedParameters { constructor(public initialized = 1, required: number) {} }\n",
        "class Parameters { constructor(public optional?: number) {} }\n",
        "function functionExpando() {}\n",
        "functionExpando.extra = 1;\n",
        "const variableExpando = function () {};\n",
        "variableExpando.extra = 1;\n",
        "const typedExpando: any = function () {};\n",
        "typedExpando.extra = 1;\n",
        "let mutableExpando = function () {};\n",
        "mutableExpando.extra = 1;\n",
    );
    let mut options = CompilerOptions::default();
    options.strict_null_checks = Some(true);
    with_program_state(&[("expandos.ts", source)], &options, |state| {
        state.check_source_file(0);

        let required = named_declaration(
            state,
            0,
            SyntaxKind::FunctionDeclaration,
            "requiredInitialized",
        );
        let required_parameter = parameter_named(state, required, "initialized");
        assert!(state
            .emit_requires_adding_implicit_undefined(required_parameter, Some(required))
            .expect("required initialized parameter"));
        assert!(state
            .emit_requires_adding_implicit_undefined(required_parameter, None)
            .expect("ordinary parameter does not require an enclosing declaration"));

        let typed = named_declaration(
            state,
            0,
            SyntaxKind::FunctionDeclaration,
            "typedInitialized",
        );
        assert!(!state
            .emit_requires_adding_implicit_undefined(
                parameter_named(state, typed, "initialized"),
                Some(typed),
            )
            .expect("undefined annotation suppresses implicit undefined"));

        let error = named_declaration(
            state,
            0,
            SyntaxKind::FunctionDeclaration,
            "errorInitialized",
        );
        assert!(!state
            .emit_requires_adding_implicit_undefined(
                parameter_named(state, error, "initialized"),
                Some(error),
            )
            .expect("error type suppresses implicit undefined"));

        let initialized_parameters = named_declaration(
            state,
            0,
            SyntaxKind::ClassDeclaration,
            "InitializedParameters",
        );
        let initialized_constructor = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&node| {
                state.kind_of(node) == SyntaxKind::Constructor
                    && state.parent_of(node) == Some(initialized_parameters)
            })
            .expect("initialized parameter-property constructor");
        let initialized_parameter = parameter_named(state, initialized_constructor, "initialized");
        assert!(state
            .emit_requires_adding_implicit_undefined(
                initialized_parameter,
                Some(initialized_constructor),
            )
            .expect("initialized parameter property"));
        assert!(!state
            .emit_requires_adding_implicit_undefined(initialized_parameter, None)
            .expect("initialized parameter property requires its enclosing declaration"));

        let parameters = named_declaration(state, 0, SyntaxKind::ClassDeclaration, "Parameters");
        let constructor = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&node| {
                state.kind_of(node) == SyntaxKind::Constructor
                    && state.parent_of(node) == Some(parameters)
            })
            .expect("parameter-property constructor");
        let optional_parameter = parameter_named(state, constructor, "optional");
        assert!(state
            .emit_requires_adding_implicit_undefined(optional_parameter, Some(constructor),)
            .expect("optional parameter property"));

        let function_expando =
            named_declaration(state, 0, SyntaxKind::FunctionDeclaration, "functionExpando");
        assert!(state
            .emit_is_expando_function_declaration(function_expando)
            .expect("function declaration expando"));
        let variable_expando =
            named_declaration(state, 0, SyntaxKind::VariableDeclaration, "variableExpando");
        assert!(state
            .emit_is_expando_function_declaration(variable_expando)
            .expect("const function expression expando"));
        let typed_expando =
            named_declaration(state, 0, SyntaxKind::VariableDeclaration, "typedExpando");
        assert!(!state
            .emit_is_expando_function_declaration(typed_expando)
            .expect("typed variable is not an expando declaration"));
        let mutable_expando =
            named_declaration(state, 0, SyntaxKind::VariableDeclaration, "mutableExpando");
        assert!(!state
            .emit_is_expando_function_declaration(mutable_expando)
            .expect("mutable variable is not an expando declaration"));
    });
}

#[test]
fn dm_container_properties_literal_const_and_late_bound_predicates_preserve_shape() {
    let source = concat!(
        "function container() {}\n",
        "container.first = 1;\n",
        "container.second = 'two';\n",
        "function noProperties() {}\n",
        "const literal = 'literal';\n",
        "let mutable = 'mutable';\n",
        "const number = 42;\n",
        "const key = 'computed' as const;\n",
        "class Late { [key] = 1; regular = 2; }\n",
    );
    with_program_state(
        &[("shapes.ts", source)],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);

            let container =
                named_declaration(state, 0, SyntaxKind::FunctionDeclaration, "container");
            let properties = state
                .emit_get_properties_of_container_function(container, 17)
                .expect("container function properties");
            assert_eq!(
                properties
                    .iter()
                    .map(|property| property.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["first", "second"]
            );
            let parent = properties[0].parent;
            assert_eq!(parent.session_token, 17);
            assert!(properties.iter().all(|property| {
                property.parent == parent
                    && property.symbol.session_token == 17
                    && property.value_declaration.is_some()
            }));
            assert!(state
                .emit_get_properties_of_container_function(
                    named_declaration(state, 0, SyntaxKind::FunctionDeclaration, "noProperties"),
                    17,
                )
                .expect("non-expando function properties")
                .is_empty());

            assert!(state
                .emit_is_literal_const_declaration(named_declaration(
                    state,
                    0,
                    SyntaxKind::VariableDeclaration,
                    "literal",
                ))
                .expect("const string literal"));
            assert!(state
                .emit_is_literal_const_declaration(named_declaration(
                    state,
                    0,
                    SyntaxKind::VariableDeclaration,
                    "number",
                ))
                .expect("const numeric literal"));
            assert!(!state
                .emit_is_literal_const_declaration(named_declaration(
                    state,
                    0,
                    SyntaxKind::VariableDeclaration,
                    "mutable",
                ))
                .expect("mutable literal"));

            let late = named_declaration(state, 0, SyntaxKind::ClassDeclaration, "Late");
            let mut computed = None;
            let mut regular = None;
            for node in state.binder.source(0).arena.node_ids() {
                if state.kind_of(node) != SyntaxKind::PropertyDeclaration
                    || state.parent_of(node) != Some(late)
                {
                    continue;
                }
                let Some(name) = state.name_of_node(node) else {
                    continue;
                };
                if state.kind_of(name) == SyntaxKind::ComputedPropertyName {
                    computed = Some(node);
                } else if state.identifier_text_of(name) == Some("regular") {
                    regular = Some(node);
                }
            }
            let computed = computed.expect("computed late-bound property");
            let regular = regular.expect("ordinary property");
            assert!(state
                .emit_is_late_bound(computed)
                .expect("late-bound computed property"));
            assert!(!state
                .emit_is_late_bound(regular)
                .expect("ordinary property is not late-bound"));
        },
    );
}

#[test]
fn dm_import_required_by_augmentation_uses_the_merged_source_key() {
    let files = [
        (
            "child1.ts",
            concat!(
                "import { ParentThing } from './parent';\n",
                "declare module './parent' {\n",
                "  interface ParentThing { add: (a: number, b: number) => number; }\n",
                "}\n",
                "export function child1(prototype: ParentThing) {\n",
                "  prototype.add = (a: number, b: number) => a + b;\n",
                "}\n",
            ),
        ),
        (
            "parent.ts",
            concat!(
                "import { child1 } from './child1';\n",
                "export class ParentThing implements ParentThing {}\n",
                "child1(ParentThing.prototype);\n",
            ),
        ),
        ("plain.ts", "import { ParentThing } from './parent';\n"),
    ];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        for file in 0..files.len() {
            state.check_source_file(file);
        }
        let augmentation_import = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == SyntaxKind::ImportDeclaration)
            .expect("augmentation import");
        let parent_import = state
            .binder
            .source(1)
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == SyntaxKind::ImportDeclaration)
            .expect("parent import");
        let plain_import = state
            .binder
            .source(2)
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == SyntaxKind::ImportDeclaration)
            .expect("ordinary import");
        assert!(state
            .emit_is_import_required_by_augmentation(parent_import)
            .expect("parent import required by augmentation"));
        assert!(!state
            .emit_is_import_required_by_augmentation(augmentation_import)
            .expect("import in augmentation source predicate"));
        assert!(!state
            .emit_is_import_required_by_augmentation(plain_import)
            .expect("ordinary import predicate"));

        let module_augmentation = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&node| {
                matches!(
                    state.data_of(node),
                    NodeData::ModuleDeclaration(data)
                        if data.name.is_some_and(|name| state.kind_of(name) == SyntaxKind::StringLiteral)
                )
            })
            .expect("module augmentation");
        assert!(!state
            .emit_is_import_required_by_augmentation(module_augmentation)
            .expect("module augmentation itself is not an import"));
    });
}

#[test]
fn emit_resolver_validates_symbol_session_before_symbol_bounds() {
    let (snapshot, options, _, export_aliases) = checked_alias_session();
    let mut state = CheckerState::from_snapshot(&snapshot, &options);
    state.check_source_file(0);
    state.check_source_file(1);
    let symbol_index = state
        .binder
        .node_symbol(export_aliases[0])
        .expect("export alias symbol")
        .0;
    let session = CheckerSession::from_checked_state(state);
    let node = EmitResolverNode::from_raw_source(1, export_aliases[0]);

    session.with_emit_resolver(|resolver| {
        assert!(matches!(
            resolver.is_symbol_accessible(
                EmitResolverSymbol {
                    session_token: session.session_token.wrapping_add(1),
                    symbol_index,
                },
                node,
                EmitSymbolMeaning::TYPE,
                false,
            ),
            Err(EmitResolverError::ForeignSymbol {
                method: EmitResolverMethod::IsSymbolAccessible,
                symbol: EmitResolverSymbol {
                    symbol_index: actual_index,
                    ..
                },
            }) if actual_index == symbol_index
        ));
        assert!(matches!(
            resolver.is_symbol_accessible(
                EmitResolverSymbol {
                    session_token: session.session_token,
                    symbol_index: u32::MAX,
                },
                node,
                EmitSymbolMeaning::TYPE,
                false,
            ),
            Err(EmitResolverError::UnknownSymbol {
                method: EmitResolverMethod::IsSymbolAccessible,
                symbol: EmitResolverSymbol {
                    symbol_index: u32::MAX,
                    ..
                },
            })
        ));
    });
}

#[test]
fn dm_visibility_walk_collects_and_monotonically_paints_linked_aliases() {
    let source = concat!(
        "namespace N { export class Value {} }\n",
        "import Alias = N.Value;\n",
        "export = Alias;\n",
    );
    with_program_state(&[("a.ts", source)], &CompilerOptions::default(), |state| {
        let nodes = state.binder.source(0).arena.node_ids().collect::<Vec<_>>();
        let import_equals = nodes
            .iter()
            .copied()
            .find(|&node| state.kind_of(node) == SyntaxKind::ImportEqualsDeclaration)
            .expect("internal import-equals declaration");
        let namespace = nodes
            .iter()
            .copied()
            .find(|&node| {
                matches!(
                    state.data_of(node),
                    NodeData::ModuleDeclaration(data)
                        if data.name.is_some_and(|name| {
                            state.identifier_text_of(name) == Some("N")
                        })
                )
            })
            .expect("linked namespace declaration");
        let export_name = nodes
            .iter()
            .copied()
            .find(|&node| {
                state.kind_of(node) == SyntaxKind::Identifier
                    && state.identifier_text_of(node) == Some("Alias")
                    && state
                        .parent_of(node)
                        .is_some_and(|parent| state.kind_of(parent) == SyntaxKind::ExportAssignment)
            })
            .expect("export-assignment identifier");

        assert_eq!(state.links.node(import_equals).is_visible, None);
        assert!(!state
            .emit_is_declaration_visible(import_equals)
            .expect("initial visibility memo"));
        assert_eq!(state.links.node(import_equals).is_visible, Some(false));

        let collected = state
            .collect_linked_aliases(export_name, /*set_visibility*/ false)
            .expect("collect branch")
            .expect("linked alias nodes");
        assert_eq!(collected, vec![import_equals, namespace]);
        assert_eq!(state.links.node(import_equals).is_visible, Some(false));
        assert_eq!(state.links.node(namespace).is_visible, None);

        assert_eq!(
            state
                .collect_linked_aliases(export_name, /*set_visibility*/ true)
                .expect("paint branch"),
            None
        );
        assert_eq!(state.links.node(import_equals).is_visible, Some(true));
        assert_eq!(state.links.node(namespace).is_visible, Some(true));
        assert!(state
            .emit_is_declaration_visible(import_equals)
            .expect("painted visibility query"));
        assert!(state
            .emit_is_declaration_visible(import_equals)
            .expect("repeated painted visibility query"));
    });
}

#[test]
fn dm_check_phase_paints_both_export_alias_arms_only_for_declaration_outputs() {
    let files = [
        (
            "assignment.ts",
            concat!(
                "namespace A { export class Value {} }\n",
                "import AssignmentAlias = A.Value;\n",
                "export = AssignmentAlias;\n",
            ),
        ),
        (
            "specifier.ts",
            concat!(
                "namespace S { export class Value {} }\n",
                "import SpecifierAlias = S.Value;\n",
                "export { SpecifierAlias };\n",
            ),
        ),
    ];

    let visibility_after_check = |options: &CompilerOptions| {
        with_program_state(&files, options, |state| {
            state.check_source_file(0);
            state.check_source_file(1);
            (0..2)
                .map(|file| {
                    let declaration = state
                        .binder
                        .source(file)
                        .arena
                        .node_ids()
                        .find(|&node| state.kind_of(node) == SyntaxKind::ImportEqualsDeclaration)
                        .expect("import-equals declaration");
                    state.links.node(declaration).is_visible
                })
                .collect::<Vec<_>>()
        })
    };

    assert_eq!(
        visibility_after_check(&CompilerOptions::default()),
        vec![None, None]
    );
    let declaration_options = CompilerOptions {
        declaration: Some(true),
        ..CompilerOptions::default()
    };
    assert_eq!(
        visibility_after_check(&declaration_options),
        vec![Some(true), Some(true)]
    );
}

#[test]
fn dm_symbol_and_entity_visibility_preserve_result_codes_aliases_and_error_nodes() {
    let files = [
        ("dep.ts", "export class Public {}\nclass Hidden {}\n"),
        (
            "main.ts",
            concat!(
                "import { Public } from \"./dep\";\n",
                "export const callable = function privateName() { return 1; };\n",
                "export interface Use { remote: Public; missing: Missing; }\n",
                "export interface Box<T> { value: T; }\n",
                "export class ThisBox { current!: typeof this; }\n",
            ),
        ),
        ("consumer.js", "export {};\n"),
    ];
    with_program_state(&files, &CompilerOptions::default(), |state| {
        let named_declaration =
            |state: &CheckerState<'_>, file: usize, kind: SyntaxKind, expected: &str| {
                state
                    .binder
                    .source(file)
                    .arena
                    .node_ids()
                    .find(|&node| {
                        if state.kind_of(node) != kind {
                            return false;
                        }
                        node_util::get_name_of_declaration(state.binder.source_of_node(node), node)
                            .is_some_and(|name| state.identifier_text_of(name) == Some(expected))
                    })
                    .unwrap_or_else(|| panic!("missing {kind:?} {expected}"))
            };
        let public = named_declaration(state, 0, SyntaxKind::ClassDeclaration, "Public");
        let hidden = named_declaration(state, 0, SyntaxKind::ClassDeclaration, "Hidden");
        let private_function =
            named_declaration(state, 1, SyntaxKind::FunctionExpression, "privateName");
        let use_declaration = named_declaration(state, 1, SyntaxKind::InterfaceDeclaration, "Use");
        let box_declaration = named_declaration(state, 1, SyntaxKind::InterfaceDeclaration, "Box");
        let this_box = named_declaration(state, 1, SyntaxKind::ClassDeclaration, "ThisBox");
        let import_specifier = state
            .binder
            .source(1)
            .arena
            .node_ids()
            .find(|&node| state.kind_of(node) == SyntaxKind::ImportSpecifier)
            .expect("Public import specifier");
        let type_reference = |expected: &str| {
            state
                .binder
                .source(1)
                .arena
                .node_ids()
                .find(|&node| {
                    state.kind_of(node) == SyntaxKind::Identifier
                        && state.identifier_text_of(node) == Some(expected)
                        && state.parent_of(node).is_some_and(|parent| {
                            state.kind_of(parent) == SyntaxKind::TypeReference
                        })
                })
                .unwrap_or_else(|| panic!("missing type reference {expected}"))
        };
        let public_reference = type_reference("Public");
        let missing_reference = type_reference("Missing");
        let type_parameter_reference = type_reference("T");
        let this_reference = state
            .binder
            .source(1)
            .arena
            .node_ids()
            .find(|&node| {
                state
                    .parent_of(node)
                    .is_some_and(|parent| state.kind_of(parent) == SyntaxKind::TypeQuery)
                    && state.text_of_node(node).ok().as_deref() == Some("this")
            })
            .expect("this type-query reference");
        let symbols = [public, hidden, private_function].map(|declaration| {
            state
                .node_symbol(declaration)
                .expect("named declaration symbol")
        });

        state.check_source_file(0);
        state.check_source_file(1);
        state.check_source_file(2);
        let node = |id| EmitResolverNode::from_raw_source(1, id);
        assert!(state
            .emit_is_declaration_visible(use_declaration)
            .expect("exported interface visibility"));
        assert!(!state
            .emit_is_declaration_visible(private_function)
            .expect("private function-expression visibility"));

        let absent_symbol = state
            .is_symbol_accessible_worker(
                None,
                Some(use_declaration),
                SymbolFlags::TYPE,
                /*should_compute_aliases_to_make_visible*/ true,
                /*allow_modules*/ true,
            )
            .expect("absent-symbol worker arm");
        assert_eq!(
            absent_symbol.accessibility,
            EmitSymbolAccessibility::Accessible
        );
        let public_access = state
            .emit_is_symbol_accessible(
                symbols[0],
                use_declaration,
                EmitSymbolMeaning::TYPE,
                /*should_compute_aliases_to_make_visible*/ false,
            )
            .expect("exported external symbol accessibility");
        assert_eq!(
            public_access.accessibility,
            EmitSymbolAccessibility::Accessible
        );

        let hidden_access = state
            .emit_is_symbol_accessible(symbols[1], use_declaration, EmitSymbolMeaning::TYPE, true)
            .expect("private external symbol accessibility");
        assert_eq!(
            hidden_access.accessibility,
            EmitSymbolAccessibility::CannotBeNamed
        );
        assert_eq!(hidden_access.error_symbol_name.as_deref(), Some("Hidden"));
        assert!(hidden_access.error_module_name.is_some());
        assert_eq!(hidden_access.error_node, None);

        let consumer_root = state.binder.source(2).root;
        let hidden_from_js = state
            .emit_is_symbol_accessible(symbols[1], consumer_root, EmitSymbolMeaning::TYPE, true)
            .expect("private external symbol accessibility from JavaScript");
        assert_eq!(
            hidden_from_js.accessibility,
            EmitSymbolAccessibility::CannotBeNamed
        );
        assert_eq!(
            hidden_from_js.error_node,
            Some(EmitResolverNode::from_raw_source(2, consumer_root))
        );

        let private_access = state
            .emit_is_symbol_accessible(
                symbols[2],
                private_function,
                EmitSymbolMeaning::VALUE_EXPORT_VALUE,
                true,
            )
            .expect("private same-module symbol accessibility");
        assert_eq!(
            private_access.accessibility,
            EmitSymbolAccessibility::NotAccessible
        );
        assert_eq!(
            private_access.error_symbol_name.as_deref(),
            Some("privateName")
        );

        let public_entity = state
            .emit_is_entity_name_visible(
                public_reference,
                use_declaration,
                /*should_compute_aliases_to_make_visible*/ true,
            )
            .expect("imported entity visibility");
        assert_eq!(
            public_entity.accessibility,
            EmitSymbolAccessibility::Accessible
        );
        assert_eq!(
            public_entity.aliases_to_make_visible,
            Some(vec![EmitResolverNode::from_raw_source(
                1,
                state
                    .parent_of(import_specifier)
                    .and_then(|parent| state.parent_of(parent))
                    .and_then(|parent| state.parent_of(parent))
                    .expect("owning import declaration"),
            )])
        );
        assert!(state
            .emit_is_declaration_visible(import_specifier)
            .expect("alias-painted import specifier"));
        assert!(state
            .emit_is_declaration_visible(import_specifier)
            .expect("repeated alias-painted import specifier"));

        let missing = state
            .emit_is_entity_name_visible(
                missing_reference,
                use_declaration,
                /*should_compute_aliases_to_make_visible*/ true,
            )
            .expect("unresolved entity visibility");
        assert_eq!(missing.accessibility, EmitSymbolAccessibility::NotResolved);
        assert_eq!(missing.error_symbol_name.as_deref(), Some("Missing"));
        assert_eq!(missing.error_node, Some(node(missing_reference)));

        let type_parameter = state
            .emit_is_entity_name_visible(
                type_parameter_reference,
                box_declaration,
                /*should_compute_aliases_to_make_visible*/ true,
            )
            .expect("type-parameter entity visibility");
        assert_eq!(
            type_parameter.accessibility,
            EmitSymbolAccessibility::Accessible
        );

        let this_entity = state
            .emit_is_entity_name_visible(
                this_reference,
                this_box,
                /*should_compute_aliases_to_make_visible*/ true,
            )
            .expect("this-container entity visibility");
        assert_eq!(
            this_entity.accessibility,
            EmitSymbolAccessibility::Accessible
        );
    });
}

#[test]
fn dm_meaning_classification_table_and_emit_declaration_gate_are_exact() {
    let source = concat!(
        "declare const value: unknown;\n",
        "declare const key: unique symbol;\n",
        "type Query = typeof value;\n",
        "type Computed = { [key]: string };\n",
        "class Base<T> {}\n",
        "class Derived extends Base<string> {}\n",
        "namespace Ns { export type T = string; }\n",
        "type Qualified = Ns.T;\n",
        "import Alias = Ns;\n",
        "function predicate(x: unknown): x is string { return true; }\n",
        "type Plain = Base<string>;\n",
    );
    with_program_state(
        &[("meaning.ts", source)],
        &CompilerOptions::default(),
        |state| {
            let nodes = state.binder.source(0).arena.node_ids().collect::<Vec<_>>();
            let identifier_with_parent = |text: &str, parent_kind: SyntaxKind| {
                nodes
                    .iter()
                    .copied()
                    .find(|&node| {
                        state.kind_of(node) == SyntaxKind::Identifier
                            && state.identifier_text_of(node) == Some(text)
                            && state
                                .parent_of(node)
                                .is_some_and(|parent| state.kind_of(parent) == parent_kind)
                    })
                    .unwrap_or_else(|| panic!("missing {text} under {parent_kind:?}"))
            };
            let type_query = identifier_with_parent("value", SyntaxKind::TypeQuery);
            let computed = identifier_with_parent("key", SyntaxKind::ComputedPropertyName);
            let heritage = identifier_with_parent("Base", SyntaxKind::ExpressionWithTypeArguments);
            let qualified_left = identifier_with_parent("Ns", SyntaxKind::QualifiedName);
            let import_equals = identifier_with_parent("Ns", SyntaxKind::ImportEqualsDeclaration);
            let plain_type = nodes
                .iter()
                .copied()
                .rev()
                .find(|&node| {
                    state.kind_of(node) == SyntaxKind::Identifier
                        && state.identifier_text_of(node) == Some("Base")
                        && state.parent_of(node).is_some_and(|parent| {
                            state.kind_of(parent) == SyntaxKind::TypeReference
                        })
                })
                .expect("plain type reference");
            let predicate_parameter = nodes
                .iter()
                .copied()
                .find(|&node| {
                    state.parent_of(node).is_some_and(|parent| {
                        matches!(
                            state.data_of(parent),
                            NodeData::TypePredicate(data)
                                if data.parameter_name == Some(node)
                        )
                    })
                })
                .expect("type-predicate parameter name");
            let qualified = state.parent_of(qualified_left).expect("qualified name");

            let rows = [
                (type_query, EmitSymbolMeaning::VALUE_EXPORT_VALUE),
                (computed, EmitSymbolMeaning::VALUE_EXPORT_VALUE),
                (heritage, EmitSymbolMeaning::VALUE_EXPORT_VALUE),
                (predicate_parameter, EmitSymbolMeaning::VALUE_EXPORT_VALUE),
                (qualified_left, EmitSymbolMeaning::NAMESPACE),
                (qualified, EmitSymbolMeaning::NAMESPACE),
                (import_equals, EmitSymbolMeaning::NAMESPACE),
                (plain_type, EmitSymbolMeaning::TYPE),
            ];
            for (node, expected) in rows {
                assert_eq!(
                    state.get_meaning_of_entity_name_reference(node),
                    expected,
                    "meaning for {:?}",
                    state.kind_of(node),
                );
            }
        },
    );

    assert!(!crate::declaration_emit::emit_declarations(
        &CompilerOptions::default()
    ));
    assert!(crate::declaration_emit::emit_declarations(
        &CompilerOptions {
            declaration: Some(true),
            ..CompilerOptions::default()
        }
    ));
    assert!(crate::declaration_emit::emit_declarations(
        &CompilerOptions {
            composite: Some(true),
            ..CompilerOptions::default()
        }
    ));
}

#[test]
fn classic_jsx_fragment_retains_only_the_fragment_factory_import() {
    let domain = IdentityDomain::reclaiming();
    let dependency = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/project/jsx.ts".to_owned(),
        TextSnapshot::new(
            "export function element() {}\nexport function fragment() {}\n".to_owned(),
            DocumentVersion::new("1"),
        ),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("JSX factory dependency identity allocation");
    let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/project/index.tsx".to_owned(),
        TextSnapshot::new(
            concat!(
                "import { element, fragment } from \"./jsx\";\n",
                "export const a = <>fragment text</>;\n",
            )
            .to_owned(),
            DocumentVersion::new("1"),
        ),
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
        &domain,
    )
    .expect("classic JSX source identity allocation");
    let dependency = Arc::new(dependency);
    let source = Arc::new(source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        jsx: Some(2),
        jsx_factory: Some("element".to_owned()),
        jsx_fragment_factory: Some("fragment".to_owned()),
        no_unused_locals: Some(true),
        ..CompilerOptions::default()
    };
    let dependency_worker = BinderWorker::bind_in_identity_domain(&dependency, &options, &domain)
        .expect("JSX dependency bind identity allocation");
    let worker = BinderWorker::bind_in_identity_domain(&source, &options, &domain)
        .expect("classic JSX bind identity allocation");
    let dependency_document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&dependency))),
        dependency_worker.into_bind_data(),
    ));
    let document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&source))),
        worker.into_bind_data(),
    ));
    let snapshot =
        ProgramSnapshot::new(vec![dependency_document, document], 0).expect("Program snapshot");
    let import_specifiers = source
        .arena
        .node_ids()
        .filter(|id| source.arena.node(*id).kind == SyntaxKind::ImportSpecifier)
        .collect::<Vec<_>>();
    assert_eq!(import_specifiers.len(), 2);

    let mut state = CheckerState::from_snapshot(&snapshot, &options);
    state.check_source_file(0);
    state.check_source_file(1);
    let session = CheckerSession::from_checked_state(state);
    let printed = session.with_emit_resolver(|resolver| {
        assert!(
            !resolver
                .is_referenced_alias_declaration(EmitResolverNode::from_raw_source(
                    1,
                    import_specifiers[0],
                ))
                .expect("ordinary classic JSX factory reachability"),
            "the fragment's semantic factory lookup is not import reachability",
        );
        assert!(
            resolver
                .is_referenced_alias_declaration(EmitResolverNode::from_raw_source(
                    1,
                    import_specifiers[1],
                ))
                .expect("classic JSX fragment factory reachability"),
            "the fragment factory value must survive import elision",
        );

        let program_source = SourceFileId::from_raw(1);
        let host = CheckerEmitHost {
            options: &options,
            syntax: &source,
            source_ids: [program_source],
        };
        let mut arena = TransformArena::new();
        let source_id = arena.add_source(&source, Some(program_source));
        let mut transformed = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source_id)],
            get_script_transformers_for_source(&options, resolver, &host, program_source)
                .expect("classic JSX transformers"),
            false,
        )
        .expect("classic JSX transform");
        create_printer(PrinterOptions::new(NewLineKind::LineFeed))
            .print(&mut transformed, PrintRequest::SourceFile(source_id), None)
            .expect("classic JSX CommonJS print")
            .text()
            .to_owned()
    });

    assert!(printed.contains("require(\"./jsx\")"), "{printed}",);
    assert!(
        printed.contains("jsx_1.element") && printed.contains("jsx_1.fragment"),
        "{printed}",
    );
}

#[test]
fn null_jsx_fragment_factory_does_not_retain_the_ordinary_factory_import_or_its_pragmas() {
    let domain = IdentityDomain::reclaiming();
    let dependency = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/project/renderer.ts".to_owned(),
        TextSnapshot::new(
            "export function jsx() {}\n".to_owned(),
            DocumentVersion::new("1"),
        ),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("JSX factory dependency identity allocation");
    let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/project/index.tsx".to_owned(),
        TextSnapshot::new(
            concat!(
                "/* @jsx jsx */\n",
                "/* @jsxfrag null */\n",
                "import { jsx } from \"./renderer\";\n",
                "<></>;\n",
            )
            .to_owned(),
            DocumentVersion::new("1"),
        ),
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
        &domain,
    )
    .expect("null JSX fragment source identity allocation");
    let dependency = Arc::new(dependency);
    let source = Arc::new(source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        jsx: Some(2),
        no_unused_locals: Some(true),
        ..CompilerOptions::default()
    };
    let dependency_worker = BinderWorker::bind_in_identity_domain(&dependency, &options, &domain)
        .expect("JSX dependency bind identity allocation");
    let worker = BinderWorker::bind_in_identity_domain(&source, &options, &domain)
        .expect("null JSX fragment bind identity allocation");
    let dependency_document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&dependency))),
        dependency_worker.into_bind_data(),
    ));
    let document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&source))),
        worker.into_bind_data(),
    ));
    let snapshot =
        ProgramSnapshot::new(vec![dependency_document, document], 0).expect("Program snapshot");
    let import_specifier = source
        .arena
        .node_ids()
        .find(|id| source.arena.node(*id).kind == SyntaxKind::ImportSpecifier)
        .expect("ordinary JSX factory import specifier");

    let mut state = CheckerState::from_snapshot(&snapshot, &options);
    state.check_source_file(0);
    state.check_source_file(1);
    let session = CheckerSession::from_checked_state(state);
    let printed = session.with_emit_resolver(|resolver| {
        assert!(
            !resolver
                .is_referenced_alias_declaration(EmitResolverNode::from_raw_source(
                    1,
                    import_specifier,
                ))
                .expect("null-fragment ordinary JSX factory reachability"),
            "the semantic factory lookup must not become runtime import reachability",
        );

        let program_source = SourceFileId::from_raw(1);
        let host = CheckerEmitHost {
            options: &options,
            syntax: &source,
            source_ids: [program_source],
        };
        let mut arena = TransformArena::new();
        let source_id = arena.add_source(&source, Some(program_source));
        let mut transformed = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source_id)],
            get_script_transformers_for_source(&options, resolver, &host, program_source)
                .expect("classic JSX transformers"),
            false,
        )
        .expect("null JSX fragment transform");
        create_printer(PrinterOptions::new(NewLineKind::LineFeed))
            .print(&mut transformed, PrintRequest::SourceFile(source_id), None)
            .expect("print null JSX fragment")
            .text()
            .to_owned()
    });

    assert!(
        printed.contains("(0, renderer_1.jsx)(null, null);"),
        "{printed}",
    );
    assert!(!printed.contains("require(\"./renderer\")"), "{printed}");
    assert!(!printed.contains("@jsx"), "{printed}");
}

#[test]
fn emit_import_resolution_filters_only_value_bearing_type_only_origins() {
    let cases = [
        (
            "named type provenance merged with a value",
            vec![
                ("/project/a.ts", "interface A {}\nexport type { A };\n"),
                (
                    "/project/b.ts",
                    "import { A } from \"./a\";\nconst A = 0;\nexport { A };\n",
                ),
                ("/project/c.ts", "import { A } from \"./b\";\nA;\n"),
            ],
            true,
        ),
        (
            "type-star provenance merged with a value",
            vec![
                ("/project/a.ts", "export type A = number;\n"),
                ("/project/b.ts", "export type * from \"./a\";\n"),
                (
                    "/project/c.ts",
                    "import { A } from \"./b\";\nconst A = 1;\nexport { A };\n",
                ),
                ("/project/d.ts", "import { A } from \"./c\";\nA;\n"),
            ],
            true,
        ),
        (
            "a runtime value explicitly exported as type-only",
            vec![
                ("/project/a.ts", "export class A {}\n"),
                ("/project/b.ts", "export type { A } from \"./a\";\n"),
                ("/project/c.ts", "import { A } from \"./b\";\nA;\n"),
            ],
            false,
        ),
    ];

    for (label, inputs, retains_runtime_import) in cases {
        let domain = IdentityDomain::reclaiming();
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES2015.bits()),
            module: Some(ModuleKind::COMMON_JS.bits()),
            ..CompilerOptions::default()
        };
        let sources = inputs
            .iter()
            .map(|(path, text)| {
                Arc::new(
                    tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                        (*path).to_owned(),
                        TextSnapshot::new((*text).to_owned(), DocumentVersion::new("1")),
                        ParseOptions::default(),
                        None,
                        &domain,
                    )
                    .expect("emit alias source identity allocation"),
                )
            })
            .collect::<Vec<_>>();
        let documents = sources
            .iter()
            .map(|source| {
                let worker = BinderWorker::bind_in_identity_domain(source, &options, &domain)
                    .expect("emit alias bind identity allocation");
                Arc::new(BoundDocument::new(
                    Arc::new(ParsedDocument::new(Arc::clone(source))),
                    worker.into_bind_data(),
                ))
            })
            .collect();
        let snapshot = ProgramSnapshot::new(documents, 0).expect("Program snapshot");
        let final_index = sources.len() - 1;
        let final_source = &sources[final_index];
        let import_specifier = final_source
            .arena
            .node_ids()
            .find(|id| final_source.arena.node(*id).kind == SyntaxKind::ImportSpecifier)
            .expect("downstream import specifier");
        let reference = final_source
            .arena
            .node_ids()
            .find(|id| {
                let node = final_source.arena.node(*id);
                node.kind == SyntaxKind::Identifier
                    && node.parent.is_some_and(|parent| {
                        final_source.arena.node(parent).kind == SyntaxKind::ExpressionStatement
                    })
            })
            .expect("downstream value reference");

        let mut state = CheckerState::from_snapshot(&snapshot, &options);
        for index in 0..sources.len() {
            state.check_source_file(index);
        }
        let session = CheckerSession::from_checked_state(state);
        let source_id = u32::try_from(final_index).expect("test source index fits SourceFileId");
        let reference = EmitResolverNode::from_raw_source(source_id, reference);
        let expected = retains_runtime_import
            .then(|| EmitResolverNode::from_raw_source(source_id, import_specifier));
        session.with_emit_resolver(|resolver| {
            assert_eq!(
                resolver
                    .get_referenced_import_declaration(reference)
                    .expect("parsed import reference resolution"),
                expected,
                "{label}",
            );
            assert_eq!(
                resolver
                    .get_jsx_factory_import_declaration(reference, "A")
                    .expect("classic JSX factory import resolution"),
                expected,
                "{label}",
            );
        });
    }
}

#[test]
fn emit_resolver_reanchors_decorator_metadata_imports_to_the_class_scope() {
    let domain = IdentityDomain::reclaiming();
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        experimental_decorators: true,
        emit_decorator_metadata: Some(true),
        ..CompilerOptions::default()
    };
    let inputs = [
        ("/service.ts", "export default class Service {}\n"),
        (
            "/component.ts",
            concat!(
                "import Service from './service';\n",
                "declare const decorator: any;\n",
                "@decorator class Component {\n",
                "    constructor(Service: Service) {}\n",
                "}\n",
            ),
        ),
    ];
    let sources = inputs
        .into_iter()
        .map(|(path, text)| {
            Arc::new(
                tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                    path.to_owned(),
                    TextSnapshot::new(text.to_owned(), DocumentVersion::new("1")),
                    ParseOptions::default(),
                    None,
                    &domain,
                )
                .expect("decorator metadata source identity allocation"),
            )
        })
        .collect::<Vec<_>>();
    let documents = sources
        .iter()
        .map(|source| {
            let worker = BinderWorker::bind_in_identity_domain(source, &options, &domain)
                .expect("decorator metadata bind identity allocation");
            Arc::new(BoundDocument::new(
                Arc::new(ParsedDocument::new(Arc::clone(source))),
                worker.into_bind_data(),
            ))
        })
        .collect();
    let snapshot = ProgramSnapshot::new(documents, 0).expect("Program snapshot");
    let component = &sources[1];
    let import_clause = component
        .arena
        .node_ids()
        .find(|node| component.arena.node(*node).kind == SyntaxKind::ImportClause)
        .expect("default import clause");
    let class_scope = component
        .arena
        .node_ids()
        .find(|node| component.arena.node(*node).kind == SyntaxKind::ClassDeclaration)
        .expect("decorated class scope");
    let type_name = component
        .arena
        .node_ids()
        .find(|node| {
            let record = component.arena.node(*node);
            record.kind == SyntaxKind::Identifier
                && record.parent.is_some_and(|parent| {
                    component.arena.node(parent).kind == SyntaxKind::TypeReference
                })
        })
        .expect("constructor parameter type name");

    let mut state = CheckerState::from_snapshot(&snapshot, &options);
    state.check_source_file(0);
    state.check_source_file(1);
    let session = CheckerSession::from_checked_state(state);
    let type_name = EmitResolverNode::from_raw_source(1, type_name);
    let class_scope = EmitResolverNode::from_raw_source(1, class_scope);
    session.with_emit_resolver(|resolver| {
        assert_eq!(
            resolver
                .get_referenced_import_declaration_at_location(type_name, class_scope)
                .expect("class-scoped metadata import resolution"),
            Some(EmitResolverNode::from_raw_source(1, import_clause)),
        );
    });
}

// ====================================================================
// H2.5h-b B-1: foundation direct-control replay for the six-query
// resolver surface. The frozen H2.5h-a foundation artifact records the
// vendored TypeScript oracle's resolver answers over three
// checker+resolver control programs; this contract replays every
// recorded query against the production CheckerSession bridge and
// demands equality, so the colliding-name/capture trio lands against
// oracle-observed expectations rather than authored ones.
// ====================================================================

const FOUNDATION_ARTIFACT_RELATIVE: &str = "../../ratchets/h2-5h-a-foundation.v1.json";

fn foundation_artifact() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(FOUNDATION_ARTIFACT_RELATIVE);
    serde_json::from_slice(&std::fs::read(&path).expect("foundation artifact readable"))
        .expect("foundation artifact is valid JSON")
}

fn replay_syntax_kind(name: &str) -> SyntaxKind {
    match name {
        "Identifier" => SyntaxKind::Identifier,
        "VariableDeclaration" => SyntaxKind::VariableDeclaration,
        "VariableDeclarationList" => SyntaxKind::VariableDeclarationList,
        "ForStatement" => SyntaxKind::ForStatement,
        "BinaryExpression" => SyntaxKind::BinaryExpression,
        "PostfixUnaryExpression" => SyntaxKind::PostfixUnaryExpression,
        "Block" => SyntaxKind::Block,
        other => panic!("unmapped control subject kind {other}"),
    }
}

fn replay_check_flag_bits(name: &str) -> u32 {
    match name {
        "LoopWithCapturedBlockScopedBinding" => 4096,
        "ContainsCapturedBlockScopeBinding" => 8192,
        "CapturedBlockScopedBinding" => 16384,
        "BlockScopedBindingInLoop" => 32768,
        "NeedsLoopOutParameter" => 65536,
        other => panic!("unmapped control check flag {other}"),
    }
}

fn locate_control_node(source: &tsc_syntax::SourceFile, subject: &Value) -> NodeId {
    let kind = replay_syntax_kind(subject["kind"].as_str().expect("subject kind"));
    let start = u32::try_from(subject["start"].as_u64().expect("subject start")).expect("start");
    let end = u32::try_from(subject["end"].as_u64().expect("subject end")).expect("end");
    let matches = source
        .arena
        .node_ids()
        .filter(|id| {
            let node = source.arena.node(*id);
            node.kind == kind
                && node.end == end
                && u32::try_from(tsc_syntax::skip_trivia(
                    source.text(),
                    usize::try_from(node.pos).expect("pos fits usize"),
                ))
                .expect("trivia start fits u32")
                    == start
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "control subject {kind:?} {start}-{end} must match exactly one node"
    );
    matches[0]
}

fn control_compiler_options(options: &Value) -> CompilerOptions {
    CompilerOptions {
        target: options["target"].as_i64().map(|value| value as i32),
        module: options["module"].as_i64().map(|value| value as i32),
        always_strict: options["alwaysStrict"].as_bool(),
        downlevel_iteration: options["downlevelIteration"].as_bool(),
        ignore_deprecations: options["ignoreDeprecations"].as_str().map(str::to_owned),
        import_helpers: options["importHelpers"].as_bool(),
        new_line: options["newLine"].as_i64().map(|value| value as i32),
        no_emit_helpers: options["noEmitHelpers"].as_bool(),
        use_define_for_class_fields: options["useDefineForClassFields"].as_bool(),
        use_unknown_in_catch_variables: options["useUnknownInCatchVariables"].as_bool(),
        ..CompilerOptions::default()
    }
}

#[test]
fn resolver_queries_replay_the_foundation_direct_controls() {
    let artifact = foundation_artifact();
    let controls = artifact["direct_controls"]
        .as_array()
        .expect("direct controls");
    let replayed_controls = [
        "checker-colliding-block-scope",
        "checker-captured-loop-bindings",
        "checker-arguments-and-catch-reference",
    ];
    let mut replayed_queries = 0usize;
    for control in controls {
        let control_id = control["control_id"].as_str().expect("control id");
        if !replayed_controls.contains(&control_id) {
            continue;
        }
        let files = control["input"]["files"].as_array().expect("control files");
        assert_eq!(files.len(), 1, "{control_id} is a single-file control");
        let file = &files[0];
        let file_path = file["path"].as_str().expect("file path");
        let text = String::from_utf8(base64_decode(
            file["utf8_base64"].as_str().expect("file bytes"),
        ))
        .expect("control source is UTF-8");
        let options = control_compiler_options(&control["input"]["compiler_options"]);

        let domain = IdentityDomain::reclaiming();
        let source = Arc::new(
            tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                file_path.to_owned(),
                TextSnapshot::new(text, DocumentVersion::new("1")),
                ParseOptions::default(),
                None,
                &domain,
            )
            .expect("control source parses"),
        );
        let worker = BinderWorker::bind_in_identity_domain(&source, &options, &domain)
            .expect("control source binds");
        let document = Arc::new(BoundDocument::new(
            Arc::new(ParsedDocument::new(Arc::clone(&source))),
            worker.into_bind_data(),
        ));
        let snapshot = ProgramSnapshot::new(vec![document], 0).expect("control snapshot");
        let mut state = CheckerState::from_snapshot(&snapshot, &options);
        state.check_source_file(0);
        let session = CheckerSession::from_checked_state(state);

        session.with_emit_resolver(|resolver| {
            for query in control["observation"]["resolver_queries"]
                .as_array()
                .expect("resolver queries")
            {
                let method = query["method"].as_str().expect("query method");
                let subject = &query["subject"];
                assert_eq!(subject["file"].as_str(), Some(file_path));
                let subject_node =
                    EmitResolverNode::from_raw_source(0, locate_control_node(&source, subject));
                let result = &query["result"];
                let describe = || {
                    format!(
                        "{control_id} {method} {}-{}",
                        subject["start"], subject["end"]
                    )
                };
                match method {
                    "isDeclarationWithCollidingName" => {
                        let expected = result["boolean"].as_bool().expect("boolean result");
                        let actual = resolver
                            .is_declaration_with_colliding_name(subject_node)
                            .unwrap_or_else(|error| panic!("{}: {error}", describe()));
                        assert_eq!(actual, expected, "{}", describe());
                    }
                    "isArgumentsLocalBinding" => {
                        let expected = result["boolean"].as_bool().expect("boolean result");
                        let actual = resolver
                            .is_arguments_local_binding(subject_node)
                            .unwrap_or_else(|error| panic!("{}: {error}", describe()));
                        assert_eq!(actual, expected, "{}", describe());
                    }
                    "hasNodeCheckFlag" => {
                        let flag =
                            replay_check_flag_bits(query["argument"].as_str().expect("flag name"));
                        let expected = result["boolean"].as_bool().expect("boolean result");
                        let actual = resolver
                            .has_node_check_flag(subject_node, flag)
                            .unwrap_or_else(|error| panic!("{}: {error}", describe()));
                        assert_eq!(actual, expected, "{}", describe());
                    }
                    "isBindingCapturedByNode" => {
                        let declaration = EmitResolverNode::from_raw_source(
                            0,
                            locate_control_node(&source, &query["secondary_subject"]),
                        );
                        let expected = result["boolean"].as_bool().expect("boolean result");
                        let actual = resolver
                            .is_binding_captured_by_node(subject_node, declaration)
                            .unwrap_or_else(|error| panic!("{}: {error}", describe()));
                        assert_eq!(actual, expected, "{}", describe());
                    }
                    "getReferencedDeclarationWithCollidingName"
                    | "getReferencedValueDeclaration" => {
                        let expected = if result["kind"].as_str() == Some("declaration") {
                            Some(locate_control_node(&source, &result["declaration"]))
                        } else {
                            None
                        };
                        let actual = if method == "getReferencedValueDeclaration" {
                            resolver.get_referenced_value_declaration(subject_node)
                        } else {
                            resolver.get_referenced_declaration_with_colliding_name(subject_node)
                        }
                        .unwrap_or_else(|error| panic!("{}: {error}", describe()));
                        assert_eq!(
                            actual.map(|declaration| declaration.node()),
                            expected,
                            "{}",
                            describe()
                        );
                    }
                    other => panic!("unmapped control resolver method {other}"),
                }
                replayed_queries += 1;
            }
        });
    }
    assert_eq!(
        replayed_queries, 43,
        "the three controls carry 43 recorded resolver queries"
    );
}

// Minimal base64 decoder for the control payloads (standard alphabet,
// '=' padding) so the dev-dependency surface stays unchanged.
fn base64_decode(encoded: &str) -> Vec<u8> {
    fn value(byte: u8) -> u32 {
        match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("unexpected base64 byte {other}"),
        }
    }
    let bytes = encoded.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut accumulator = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            accumulator |= value(*byte) << (18 - 6 * index);
        }
        out.push((accumulator >> 16) as u8);
        if chunk.len() > 2 {
            out.push((accumulator >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(accumulator as u8);
        }
    }
    out
}
