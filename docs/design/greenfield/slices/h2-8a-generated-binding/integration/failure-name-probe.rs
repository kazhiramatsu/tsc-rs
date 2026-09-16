#![allow(dead_code, unused_imports)]
//! A41-BINDING (C02) direct controls for the generated-name domains
//! (`scripts/observe-decorator-bindings.mjs direct` →
//! `fixtures/decorator-binding-direct.json`, two matching upstream runs per
//! case):
//!
//! * `synthetic`: a transformer inserted right after `transformTypeScript`
//!   prepends `let <name> = 0;` with a plain (non-generated) identifier spelled
//!   like a decorator-generated name to the parsed tree of a complete emit, the
//!   way the observer's `customTransformers.before` transformer does. tsc's
//!   `isFileLevelUniqueName` / `isUniqueName` consult only the parsed
//!   identifier table, the reserved-name stack and the printer's generated
//!   names, so the synthetic spelling never shifts a generated name. Each row
//!   is printed through both JavaScript entries: the oracle-free `print` and
//!   `print_javascript_with_global_names` with an all-miss oracle (the
//!   compiler route).
//! * `global`: one generated name of every domain (FileLevel, scoped
//!   optimistic, file-wide optimistic, numbered, temp) declared in a
//!   transformed source file and printed with an explicit global-name oracle
//!   answering miss / hit / hit-chain / error (the printer `hasGlobalName`
//!   handler). The queried names are recorded, never compared.
//! * `lifecycle`: re-print on one printer, a clone of a generated identifier,
//!   a hook failure before/after the first statement followed by a second
//!   generated name of the same domain on the same printer (the C03
//!   `x_2`/`x_1` shape generalized to every domain), and disposal.
//!
//! Printed text only: supplementary to the complete-command witnesses of
//! `crates/compiler/tests/decorator_binding_pipeline_contract.rs`. Set
//! `TSC_RS_DECORATOR_BINDING_REPORT_DIR` to write the native observations.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde_json::{json, Value};
use tsc_emitter::{
    create_printer, get_script_transformers, transform_nodes, EmitConstantValue,
    EmitEnumMemberValue, EmitExportContainerMode, EmitHint, EmitResolver, EmitResolverError,
    EmitResolverMethod, EmitResolverNode, GeneratedIdentifierFlags, GlobalNameOracle, NewLineKind,
    PrintRequest, PrinterError, PrinterOptions, SourceFileTextMode, StandaloneWriter,
    TransformArena, TransformError, TransformNode, TransformNodeArray, TransformRoot,
    TransformSourceId, TransformationContext, Transformer, UnsupportedEmitFeature,
};
use tsc_program::SourceFileId;
use tsc_syntax::{
    for_each_child, nodes::SourceFileData, parse_source_file, NodeData, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeFlags, ScriptTarget};

/// Divergences that remain after the candidate, keyed by `case_id#route[#op]`.
/// The test is green only when the observed mismatch set equals this list.
///
/// * failure carry: tsc's printer keeps its name tables after a hook throws
///   (no `finally`; `reset()` runs only after a completed print), so the next
///   print on the same printer continues `generatedNames` (`x_2`, `_o_1`),
///   the root reserved names (`_s_1`) and `tempFlags` (`_b`). Rust finalizes
///   generated binding names per print on the transformation and carries only
///   the writer and comment state (design memo §6.4 records the seeding plan;
///   it needs a printer/transform.rs change outside this candidate).
/// * dispose: tsc keeps the `emitNode` of synthetic nodes after
///   `TransformationResult.dispose()`, so a fresh printer prints them again;
///   Rust refuses printing a disposed transformation with a typed
///   `InvalidLifecycle` (session model, as C01's lifetime rows record).
const KNOWN_DIVERGENCES: &[(&str, &str)] = &[
    (
        "decorator-binding-direct/lifecycle/dispose/numbered#direct#after_dispose",
        "printing a disposed transformation is a typed InvalidLifecycle refusal",
    ),
    (
        "decorator-binding-direct/lifecycle/dispose/file-level#direct#after_dispose",
        "printing a disposed transformation is a typed InvalidLifecycle refusal",
    ),
];

struct NoConstantValueResolver;

impl EmitResolver for NoConstantValueResolver {
    fn get_constant_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_enum_member_value(
        &self,
        _node: EmitResolverNode,
    ) -> Result<Option<EmitEnumMemberValue>, EmitResolverError> {
        Ok(None)
    }

    fn get_referenced_export_container(
        &self,
        _node: EmitResolverNode,
        _mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }

    fn has_node_check_flag(
        &self,
        _node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}

/// The observer's `hasGlobalName` handler: a fixed hit set, or a failing
/// query. Queried names are recorded for the report only.
struct Oracle {
    names: BTreeSet<String>,
    error: bool,
    queries: RefCell<Vec<String>>,
}

impl GlobalNameOracle for Oracle {
    fn has_global_name(&self, name: &str) -> Result<bool, EmitResolverError> {
        self.queries.borrow_mut().push(name.to_owned());
        if self.error {
            return Err(EmitResolverError::UnavailableForName {
                method: EmitResolverMethod::HasGlobalName,
                name: name.into(),
            });
        }
        Ok(self.names.contains(name))
    }
}

fn root_statements(arena: &TransformArena, source: TransformSourceId) -> Vec<NodeId> {
    let root = arena.root(source).expect("root");
    let NodeData::SourceFile(data) = &arena.node(root).expect("root record").data else {
        panic!("source file root");
    };
    arena
        .node_array(TransformNodeArray::new(
            source,
            data.statements.expect("statements"),
        ))
        .expect("statements")
        .nodes
        .clone()
}

fn declaration_name(
    arena: &TransformArena,
    source: TransformSourceId,
    statement: NodeId,
) -> NodeId {
    let node = arena.node_ref(source, statement).expect("statement");
    let NodeData::VariableStatement(data) = &arena.node(node).expect("record").data else {
        panic!("variable statement");
    };
    let list = arena
        .node_ref(source, data.declaration_list.expect("list"))
        .expect("list");
    let NodeData::VariableDeclarationList(list_data) = &arena.node(list).expect("record").data
    else {
        panic!("declaration list");
    };
    let declaration = arena
        .node_array(TransformNodeArray::new(
            source,
            list_data.declarations.expect("declarations"),
        ))
        .expect("declarations")
        .nodes[0];
    let declaration = arena.node_ref(source, declaration).expect("declaration");
    let NodeData::VariableDeclaration(declaration_data) =
        &arena.node(declaration).expect("record").data
    else {
        panic!("variable declaration");
    };
    declaration_data.name.expect("name")
}

fn expression_identifier(
    arena: &TransformArena,
    source: TransformSourceId,
    statement: NodeId,
) -> NodeId {
    let node = arena.node_ref(source, statement).expect("statement");
    let NodeData::ExpressionStatement(data) = &arena.node(node).expect("record").data else {
        panic!("expression statement");
    };
    data.expression.expect("expression")
}

/// The generated identifier of one domain, created with the public factory
/// exactly as the observer creates it (`generatedName` / `make`).
fn generated_name(
    arena: &mut TransformArena,
    source: TransformSourceId,
    kind: &str,
    base: Option<&str>,
    statement_index: usize,
) -> TransformNode {
    let statements = root_statements(arena, source);
    let statement_node = arena
        .node_ref(source, statements[statement_index])
        .expect("statement");
    let derived_identifier = if kind == "node-derived-same-node" || statement_index == 0 {
        declaration_name(arena, source, statements[0])
    } else {
        expression_identifier(arena, source, statements[1])
    };
    let derived_identifier = arena
        .node_ref(source, derived_identifier)
        .expect("identifier");
    let mut factory = arena.factory();
    match kind {
        "file-level" => factory
            .create_unique_name(
                source,
                base.expect("base"),
                GeneratedIdentifierFlags::OPTIMISTIC | GeneratedIdentifierFlags::FILE_LEVEL,
            )
            .unwrap(),
        "scoped" => factory
            .create_unique_name(
                source,
                base.expect("base"),
                GeneratedIdentifierFlags::OPTIMISTIC
                    | GeneratedIdentifierFlags::RESERVED_IN_NESTED_SCOPES,
            )
            .unwrap(),
        "file-wide" => factory
            .create_unique_name(
                source,
                base.expect("base"),
                GeneratedIdentifierFlags::OPTIMISTIC,
            )
            .unwrap(),
        "numbered" => factory
            .create_unique_name(source, base.expect("base"), GeneratedIdentifierFlags::NONE)
            .unwrap(),
        // getGeneratedNameForNode of a non-member node: the printer's default
        // arm (makeTempVariableName(Auto), not reserved in nested scopes).
        "temp" => factory
            .get_generated_name_for_non_member_node(statement_node)
            .unwrap(),
        "node-derived-same-node" | "node-derived-other-node" => factory
            .get_generated_name_for_node(
                derived_identifier,
                GeneratedIdentifierFlags::NONE,
                None,
                None,
            )
            .unwrap(),
        other => panic!("unknown generated-name kind {other}"),
    }
}

fn const_declaration(
    arena: &mut TransformArena,
    source: TransformSourceId,
    name: TransformNode,
) -> TransformNode {
    let mut factory = arena.factory();
    let initializer = factory.create_numeric_literal(source, "1").unwrap();
    let declaration = factory
        .create_variable_declaration(source, name, None, None, Some(initializer))
        .unwrap();
    let declarations = factory
        .create_node_array(source, vec![declaration])
        .unwrap();
    let list = factory
        .create_variable_declaration_list(source, declarations, NodeFlags::CONST)
        .unwrap();
    factory
        .create_variable_statement(source, None, list)
        .unwrap()
}

fn plain_let(arena: &mut TransformArena, source: TransformSourceId, name: &str) -> TransformNode {
    let mut factory = arena.factory();
    let identifier = factory.create_identifier(source, name).unwrap();
    let initializer = factory.create_numeric_literal(source, "0").unwrap();
    let declaration = factory
        .create_variable_declaration(source, identifier, None, None, Some(initializer))
        .unwrap();
    let declarations = factory
        .create_node_array(source, vec![declaration])
        .unwrap();
    let list = factory
        .create_variable_declaration_list(source, declarations, NodeFlags::LET)
        .unwrap();
    factory
        .create_variable_statement(source, None, list)
        .unwrap()
}

/// Replaces (or prepends to) the root statements: the observer's
/// `factory.updateSourceFile(file, statements)`.
struct StatementInjector {
    statements: Vec<TransformNode>,
    prepend: bool,
}

impl Transformer for StatementInjector {
    fn name(&self) -> &'static str {
        "decorator-binding-statement-injector"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Ok(root);
        };
        let file = context.arena().root(source)?;
        let NodeData::SourceFile(data) = context.arena().node(file)?.data.clone() else {
            panic!("source file root");
        };
        let mut statements = self.statements.clone();
        if self.prepend {
            for id in root_statements(context.arena(), source) {
                statements.push(context.arena().node_ref(source, id).expect("statement"));
            }
        }
        let mut flags = context.arena().transform_flags(file);
        for statement in &statements {
            flags |= context.arena().transform_flags(*statement);
        }
        let array = context.factory()?.create_node_array(source, statements)?;
        let updated = context.factory()?.update_node(
            file,
            NodeData::SourceFile(SourceFileData {
                statements: Some(array.array()),
                end_of_file_token: data.end_of_file_token,
            }),
            flags,
        )?;
        context.arena_mut()?.replace_root(source, updated)?;
        Ok(root)
    }
}

/// The observer's printer `onEmitNode` fault: throws before or after the
/// callback of the first standalone statement, once.
struct FailingHooks {
    target: TransformNode,
    fault: Rc<RefCell<Option<String>>>,
}

impl FailingHooks {
    fn fail_if_armed(&self, phase: &str, node: TransformNode) -> Result<(), TransformError> {
        let armed = self.fault.borrow().as_deref() == Some(phase) && node == self.target;
        if armed {
            *self.fault.borrow_mut() = None;
            return Err(TransformError::Unsupported(
                UnsupportedEmitFeature::CustomTransformers,
            ));
        }
        Ok(())
    }
}

impl Transformer for FailingHooks {
    fn name(&self) -> &'static str {
        "decorator-binding-failing-hooks"
    }

    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        context.enable_emit_notification(SyntaxKind::VariableStatement)
    }

    fn before_emit_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        self.fail_if_armed("before", node)
    }

    fn after_emit_node(
        &mut self,
        _context: &TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        self.fail_if_armed("after", node)
    }
}

fn printer_options() -> PrinterOptions {
    PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
        .with_source_file_text_mode(SourceFileTextMode::Canonical)
}

/// The synthetic group is a complete emit with `newLine: LineFeed` and
/// `module: Preserve` (the decorator-super-direct shape).
fn synthetic_printer_options(target: i32) -> PrinterOptions {
    PrinterOptions::new(NewLineKind::LineFeed)
        .with_target(ScriptTarget::from_bits(target))
        .with_source_file_text_mode(SourceFileTextMode::Canonical)
}

fn parse(text: &str) -> (TransformArena, TransformSourceId) {
    let parsed = parse_source_file("main.ts", text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    (arena, source)
}

fn printed(result: Result<tsc_emitter::PrintedText, PrinterError>) -> Value {
    match result {
        Ok(printed) => json!({"status": "returned", "text": printed.text()}),
        Err(error) => json!({"status": "threw", "error": format!("{error:?}")}),
    }
}

fn cross_arena() {
    for second_text in ["let x = 1;\nx;\n", "let y = 1;\ny;\n"] {
        let (mut a1, s1) = parse("let x = 1;\nx;\n");
        let n1 = generated_name(&mut a1, s1, "node-derived-same-node", None, 0);
        let u1 = const_declaration(&mut a1, s1, n1);
        let fault = Rc::new(RefCell::new(Some("after".to_owned())));
        let mut r1 = transform_nodes(
            a1,
            vec![TransformRoot::SourceFile(s1)],
            vec![Box::new(FailingHooks {
                target: u1,
                fault: fault.clone(),
            })],
            false,
        )
        .unwrap();
        let (mut a2, s2) = parse(second_text);
        let n2 = generated_name(&mut a2, s2, "node-derived-same-node", None, 0);
        let u2 = const_declaration(&mut a2, s2, n2);
        let mut r2 =
            transform_nodes(a2, vec![TransformRoot::SourceFile(s2)], vec![], false).unwrap();
        let request = |node| PrintRequest::StandaloneNode {
            node,
            writer: StandaloneWriter::MultiLine,
        };
        let mut p = create_printer(printer_options());
        let first = printed(p.print(&mut r1, request(u1), None));
        assert_eq!(first["status"], "threw");
        assert!(fault.borrow().is_none());
        let second = printed(p.print(&mut r2, request(u2), None));
        let fresh = printed(create_printer(printer_options()).print(&mut r2, request(u2), None));
        println!(
            "{}",
            json!({"second_source": second_text, "first": first, "second": second, "fresh": fresh})
        );
    }
}

fn same_binding() {
    for (kind, base) in [
        ("numbered", Some("x")),
        ("scoped", Some("_s")),
        ("file-wide", Some("_o")),
        ("file-level", Some("_m")),
        ("temp", None),
    ] {
        let (mut arena, source) = parse("let x = 1;\nx;\n");
        let name = generated_name(&mut arena, source, kind, base, 0);
        let node = const_declaration(&mut arena, source, name);
        let fault = Rc::new(RefCell::new(Some("after".to_owned())));
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            vec![Box::new(FailingHooks {
                target: node,
                fault: fault.clone(),
            })],
            false,
        )
        .unwrap();
        let request = PrintRequest::StandaloneNode {
            node,
            writer: StandaloneWriter::MultiLine,
        };
        let mut p = create_printer(printer_options());
        let first = printed(p.print(&mut result, request.clone(), None));
        assert_eq!(first["status"], "threw");
        assert!(fault.borrow().is_none());
        let second = printed(p.print(&mut result, request.clone(), None));
        let fresh = printed(create_printer(printer_options()).print(&mut result, request, None));
        println!(
            "{}",
            json!({"same_binding": kind, "first": first, "second": second, "fresh": fresh})
        );
    }
}
fn main() {
    cross_arena();
    same_binding();
}
