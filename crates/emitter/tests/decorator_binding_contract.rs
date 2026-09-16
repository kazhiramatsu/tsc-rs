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
    TransformArena, TransformBundle, TransformError, TransformNode, TransformNodeArray,
    TransformRoot, TransformSourceId, TransformationContext, Transformer, UnsupportedEmitFeature,
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

/// Replaces the root statements of each bundle source: the observer's
/// per-file `factory.updateSourceFile(file, statements)` over `ts.transform`
/// of two source files.
struct BundleStatementInjector {
    per_source: Vec<(TransformSourceId, Vec<TransformNode>)>,
}

impl Transformer for BundleStatementInjector {
    fn name(&self) -> &'static str {
        "decorator-binding-bundle-statement-injector"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        // A bundle root reaches a transformer once per member source
        // (`TransformRoot::SourceFile`); the bundle arm is kept for a
        // transformer chain that hands the bundle itself.
        let sources = match &root {
            TransformRoot::SourceFile(source) => vec![*source],
            TransformRoot::Bundle(bundle) => bundle.sources().to_vec(),
        };
        for source in sources {
            let Some((_, statements)) = self.per_source.iter().find(|(id, _)| *id == source) else {
                continue;
            };
            let file = context.arena().root(source)?;
            let NodeData::SourceFile(data) = context.arena().node(file)?.data.clone() else {
                panic!("source file root");
            };
            let mut flags = context.arena().transform_flags(file);
            for statement in statements {
                flags |= context.arena().transform_flags(*statement);
            }
            let array = context
                .factory()?
                .create_node_array(source, statements.clone())?;
            let updated = context.factory()?.update_node(
                file,
                NodeData::SourceFile(SourceFileData {
                    statements: Some(array.array()),
                    end_of_file_token: data.end_of_file_token,
                }),
                flags,
            )?;
            context.arena_mut()?.replace_root(source, updated)?;
        }
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
        // The observer enables VariableStatement notifications (and
        // FunctionDeclaration for the scope controls); a notification without
        // an armed fault is transparent.
        context.enable_emit_notification(SyntaxKind::VariableStatement)?;
        context.enable_emit_notification(SyntaxKind::FunctionDeclaration)
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

// ---------------------------------------------------------------- identity trace

/// Projects the native generated-binding identities of a transformed tree
/// into the schema the handoff asks for: one row per binding with every
/// occurrence (node id, printed spelling, declaration or reference role).
/// The identity numbers are opaque per transformation and never compared
/// with upstream node ids; the trace proves that every occurrence of one
/// binding carries the same spelling and that distinct spellings never
/// share one binding, independently of the printed text comparison.
fn identity_trace(arena: &TransformArena, source: TransformSourceId) -> Value {
    let root = arena.root(source).expect("root");
    let syntax = arena.source(source).expect("source").syntax();
    let mut rows: BTreeMap<u64, Vec<Value>> = BTreeMap::new();
    let mut stack: Vec<(NodeId, bool)> = vec![(root.node(), false)];
    while let Some((id, declares)) = stack.pop() {
        let node = arena.node_ref(source, id).expect("node");
        let record = arena.node(node).expect("record");
        if let NodeData::Identifier(identifier) = &record.data {
            if let Some(binding) = arena.generated_binding_identity(node) {
                rows.entry(binding).or_default().push(json!({
                    "node": id.0,
                    "text": identifier.text,
                    "role": if declares { "declaration" } else { "reference" },
                }));
            }
        }
        let declared_name = match &record.data {
            NodeData::VariableDeclaration(data) => data.name,
            NodeData::Parameter(data) => data.name,
            NodeData::BindingElement(data) => data.name,
            NodeData::ClassDeclaration(data) => data.name,
            NodeData::ClassExpression(data) => data.name,
            NodeData::FunctionDeclaration(data) => data.name,
            NodeData::FunctionExpression(data) => data.name,
            _ => None,
        };
        let mut children = Vec::new();
        for_each_child(&syntax.arena, record, |child| {
            children.push((child, declared_name == Some(child)));
            false
        });
        // Preserve tree order in the rows (the stack pops in reverse).
        for child in children.into_iter().rev() {
            stack.push(child);
        }
    }
    let bindings = rows
        .iter()
        .map(|(binding, occurrences)| {
            let spellings: BTreeSet<&str> = occurrences
                .iter()
                .map(|row| row["text"].as_str().unwrap())
                .collect();
            assert_eq!(
                spellings.len(),
                1,
                "binding {binding} carries more than one spelling: {occurrences:?}"
            );
            json!({"binding": binding, "spelling": spellings.iter().next().unwrap(),
                "declarations": occurrences.iter().filter(|row| row["role"] == "declaration").count(),
                "references": occurrences.iter().filter(|row| row["role"] == "reference").count(),
                "occurrences": occurrences})
        })
        .collect::<Vec<_>>();
    json!({"bindings": bindings})
}

// ---------------------------------------------------------------- synthetic

fn emit_synthetic(case: &Value, oracle_route: bool) -> Value {
    let options_in = &case["options"];
    let (mut arena, source) = parse(case["text"].as_str().expect("text"));
    let target = i32::try_from(options_in["target"].as_u64().expect("target")).expect("target");
    let options = CompilerOptions {
        target: Some(target),
        module: Some(
            i32::try_from(options_in["module"].as_u64().expect("module")).expect("module"),
        ),
        use_define_for_class_fields: options_in["useDefineForClassFields"].as_bool(),
        strict: options_in["strict"].as_bool(),
        ..CompilerOptions::default()
    };
    let resolver = NoConstantValueResolver;
    let mut transformers =
        get_script_transformers(&options, &resolver).expect("script transformers");
    if let Some(name) = case["inject"].as_str() {
        let statement = plain_let(&mut arena, source, name);
        let typescript = transformers
            .iter()
            .position(|transformer| transformer.name() == "transformTypeScript")
            .expect("transformTypeScript leads the script transformers");
        transformers.insert(
            typescript + 1,
            Box::new(StatementInjector {
                statements: vec![statement],
                prepend: true,
            }),
        );
    }
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        transformers,
        false,
    )
    .expect("transform");
    let mut printer = create_printer(synthetic_printer_options(target));
    let mut value = if oracle_route {
        let oracle = Oracle {
            names: BTreeSet::new(),
            error: false,
            queries: RefCell::new(Vec::new()),
        };
        printed(printer.print_javascript_with_global_names(
            &mut result,
            PrintRequest::SourceFile(source),
            None,
            &oracle,
        ))
    } else {
        printed(printer.print(&mut result, PrintRequest::SourceFile(source), None))
    };
    // The trace is taken after the print: the finalizer has written the
    // printed spelling into every identifier of each binding.
    value["identity_trace"] = identity_trace(result.arena(), source);
    value
}

// ---------------------------------------------------------------- global

fn observe_global(case: &Value) -> Value {
    let (mut arena, source) = parse(case["source"].as_str().expect("source"));
    let name = generated_name(
        &mut arena,
        source,
        case["kind"].as_str().expect("kind"),
        case["base"].as_str(),
        0,
    );
    let statement = const_declaration(&mut arena, source, name);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(StatementInjector {
            statements: vec![statement],
            prepend: false,
        })],
        false,
    )
    .expect("transform");
    let chain = case["chain"]
        .as_array()
        .expect("chain")
        .iter()
        .map(|name| name.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let oracle = Oracle {
        names: match case["oracle"].as_str().expect("oracle") {
            "hit" => chain.iter().take(1).cloned().collect(),
            "hit-chain" => chain.iter().cloned().collect(),
            _ => BTreeSet::new(),
        },
        error: case["oracle"] == "error",
        queries: RefCell::new(Vec::new()),
    };
    let mut printer = create_printer(printer_options());
    let mut value = printed(printer.print_javascript_with_global_names(
        &mut result,
        PrintRequest::SourceFile(source),
        None,
        &oracle,
    ));
    value["queries"] = json!(oracle.queries.borrow().clone());
    value
}

// ---------------------------------------------------------------- lifecycle

const LIFECYCLE_SOURCE: &str = "let x = 1;\nx;\nconst o = { [x]: 1 };\n";

/// Failure-carry identity controls (integration review F1 / F2): the same
/// binding printed again on the printer whose print threw keeps its
/// spelling (`autoGeneratedIdToGeneratedName`); a binding of another
/// transformation (another arena) never resolves through the first one's
/// node cache; a repeated failure of one binding consumes no second
/// ordinal.
fn observe_failure_carry(case: &Value) -> Value {
    let kind = case["kind"].as_str().expect("kind");
    let base = case["base"].as_str();
    let op = case["op"].as_str().expect("op");
    let other_text = case["second"].as_str() == Some("other-text");
    let request = |node| PrintRequest::StandaloneNode {
        node,
        writer: StandaloneWriter::MultiLine,
    };
    let (mut arena, source) = parse(LIFECYCLE_SOURCE);
    let u1_name = generated_name(&mut arena, source, kind, base, 0);
    let u1 = const_declaration(&mut arena, source, u1_name);
    let u2_same_arena = (op == "double-failure").then(|| {
        let name = generated_name(&mut arena, source, kind, base, 1);
        const_declaration(&mut arena, source, name)
    });
    let fault = Rc::new(RefCell::new(None));
    let mut first = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(FailingHooks {
            target: u1,
            fault: Rc::clone(&fault),
        })],
        false,
    )
    .expect("transform");
    // cross-arena: the second binding lives in a second transformation whose
    // node numbers coincide with the first one's.
    let mut second = (op == "cross-arena").then(|| {
        let text = if other_text {
            LIFECYCLE_SOURCE.replace('x', "y")
        } else {
            LIFECYCLE_SOURCE.to_owned()
        };
        let (mut arena, source) = parse(&text);
        let base = if other_text && kind == "numbered" {
            Some("y")
        } else {
            base
        };
        let name = generated_name(&mut arena, source, kind, base, 0);
        let u2 = const_declaration(&mut arena, source, name);
        let result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            vec![],
            false,
        )
        .expect("transform");
        (result, u2)
    });
    let mut shared = create_printer(printer_options());
    let mut results = Vec::new();
    let mut record = |index: usize, value: Value| {
        let mut value = value;
        value["op"] = json!(index);
        results.push(value);
    };
    let phase = case["phase"].as_str().unwrap_or("after").to_owned();
    let failures = if op == "double-failure" { 2 } else { 1 };
    for index in 0..failures {
        *fault.borrow_mut() = Some(phase.clone());
        let faulted = shared.print(&mut first, request(u1), None);
        assert!(
            fault.borrow().is_none(),
            "{}: the armed fault fired",
            case["case_id"]
        );
        assert!(
            matches!(
                faulted,
                Err(PrinterError::Transform(TransformError::Unsupported(
                    UnsupportedEmitFeature::CustomTransformers
                )))
            ),
            "{}: the injected fault is the typed hook error, got {faulted:?}",
            case["case_id"]
        );
        record(index, printed(faulted));
    }
    let next = failures;
    match (&mut second, u2_same_arena) {
        (Some((result, u2)), None) => {
            record(next, printed(shared.print(result, request(*u2), None)));
            record(
                next + 1,
                printed(create_printer(printer_options()).print(result, request(*u2), None)),
            );
        }
        (None, Some(u2)) => {
            record(next, printed(shared.print(&mut first, request(u2), None)));
            record(
                next + 1,
                printed(create_printer(printer_options()).print(&mut first, request(u2), None)),
            );
        }
        (None, None) => {
            assert_eq!(op, "reprint-after-failure");
            record(next, printed(shared.print(&mut first, request(u1), None)));
            record(
                next + 1,
                printed(create_printer(printer_options()).print(&mut first, request(u1), None)),
            );
        }
        (Some(_), Some(_)) => unreachable!("one second binding"),
    }
    json!({"results": results})
}

/// Scope controls of the carried tables (DESIGN.md §6.4). scope-fault: a
/// source-file print holding two root bindings of the kind, a function whose
/// body holds a third and a fourth root binding after it; the hook throws
/// inside the function (after its inner statement), after the function, or
/// after the trailing root statement, and the next standalone print on the
/// same printer shows which scope's tables survived (tsc pops a closed
/// scope's tempFlags / reservedNames, keeps the innermost open scope's
/// tempFlags current, and consults the whole stale reservedNames stack).
/// file-after-failure: a standalone print fails, then a source-file print
/// follows on the same printer (emitSourceFileWorker pushes a fresh scope).
fn observe_scope_control(case: &Value) -> Value {
    let kind = case["kind"].as_str().expect("kind");
    let base = case["base"].as_str();
    let op = case["op"].as_str().expect("op");
    let (mut arena, source) = parse(LIFECYCLE_SOURCE);
    let binding_in = |arena: &mut TransformArena, source: TransformSourceId| -> TransformNode {
        let name = if kind == "temp" {
            // Distinct temps need distinct non-member nodes: a synthesized
            // empty statement each (the observer's createEmptyStatement).
            let anchor = arena.factory().create_empty_statement(source).unwrap();
            arena
                .factory()
                .get_generated_name_for_non_member_node(anchor)
                .unwrap()
        } else {
            generated_name(arena, source, kind, base, 0)
        };
        const_declaration(arena, source, name)
    };
    let binding = |arena: &mut TransformArena| -> TransformNode { binding_in(arena, source) };
    let request = |node| PrintRequest::StandaloneNode {
        node,
        writer: StandaloneWriter::MultiLine,
    };
    let fault = Rc::new(RefCell::new(None));
    let mut results = Vec::new();
    let mut record = |index: usize, value: Value| {
        let mut value = value;
        value["op"] = json!(index);
        results.push(value);
    };
    let assert_fault =
        |case_id: &Value, faulted: &Result<tsc_emitter::PrintedText, PrinterError>| {
            assert!(
                matches!(
                    faulted,
                    Err(PrinterError::Transform(TransformError::Unsupported(
                        UnsupportedEmitFeature::CustomTransformers
                    )))
                ),
                "{case_id}: the injected fault is the typed hook error, got {faulted:?}"
            );
            assert!(fault.borrow().is_none(), "{case_id}: the armed fault fired");
        };
    if op == "scope-fault" {
        let extra_nested = case["extra_nested"].as_bool().unwrap_or(false);
        let d = (0..if extra_nested { 5 } else { 4 })
            .map(|_| binding(&mut arena))
            .collect::<Vec<_>>();
        let next = binding(&mut arena);
        let f = {
            let mut factory = arena.factory();
            let name = factory.create_identifier(source, "f").unwrap();
            let inner = if extra_nested {
                vec![d[2], d[4]]
            } else {
                vec![d[2]]
            };
            let statements = factory.create_node_array(source, inner).unwrap();
            let body = factory.create_block(source, statements, true).unwrap();
            let parameters = factory.create_node_array(source, vec![]).unwrap();
            factory
                .create_function_declaration(
                    source,
                    None,
                    None,
                    Some(name),
                    None,
                    parameters,
                    None,
                    Some(body),
                )
                .unwrap()
        };
        let target = match case["fault"].as_str().expect("fault") {
            "inside-nested" => d[2],
            "after-nested" => f,
            "after-tail" => d[3],
            other => panic!("unknown scope fault {other}"),
        };
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            vec![
                Box::new(StatementInjector {
                    statements: vec![d[0], d[1], f, d[3]],
                    prepend: false,
                }),
                Box::new(FailingHooks {
                    target,
                    fault: Rc::clone(&fault),
                }),
            ],
            false,
        )
        .expect("transform");
        let mut shared = create_printer(printer_options());
        *fault.borrow_mut() = Some("after".to_owned());
        let faulted = shared.print(&mut result, PrintRequest::SourceFile(source), None);
        assert_fault(&case["case_id"], &faulted);
        record(0, printed(faulted));
        record(1, printed(shared.print(&mut result, request(next), None)));
        record(
            2,
            printed(create_printer(printer_options()).print(&mut result, request(next), None)),
        );
        return json!({"results": results});
    }
    if op == "bundle-fault" {
        // Two transformed sources in one bundle: the hook throws after the
        // first source's statement, before the second source's worker names
        // anything.
        let parsed = parse_source_file(
            "second.ts",
            LIFECYCLE_SOURCE.replace('x', "y"),
            Default::default(),
            None,
        );
        let second = arena.add_source(&parsed, Some(SourceFileId::from_raw(1)));
        let b1 = binding(&mut arena);
        let b2 = binding_in(&mut arena, second);
        let next = binding(&mut arena);
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::Bundle(TransformBundle::new(vec![
                source, second,
            ]))],
            vec![
                Box::new(BundleStatementInjector {
                    per_source: vec![(source, vec![b1]), (second, vec![b2])],
                }),
                Box::new(FailingHooks {
                    target: b1,
                    fault: Rc::clone(&fault),
                }),
            ],
            false,
        )
        .expect("transform");
        let mut shared = create_printer(printer_options());
        *fault.borrow_mut() = Some("after".to_owned());
        let faulted = shared.print(
            &mut result,
            PrintRequest::Bundle(TransformBundle::new(vec![source, second])),
            None,
        );
        assert_fault(&case["case_id"], &faulted);
        record(0, printed(faulted));
        record(1, printed(shared.print(&mut result, request(next), None)));
        record(
            2,
            printed(create_printer(printer_options()).print(&mut result, request(next), None)),
        );
        return json!({"results": results});
    }
    assert_eq!(op, "file-after-failure");
    let u1 = binding(&mut arena);
    let u2 = binding(&mut arena);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![
            Box::new(StatementInjector {
                statements: vec![u2],
                prepend: false,
            }),
            Box::new(FailingHooks {
                target: u1,
                fault: Rc::clone(&fault),
            }),
        ],
        false,
    )
    .expect("transform");
    let mut shared = create_printer(printer_options());
    *fault.borrow_mut() = Some("after".to_owned());
    let faulted = shared.print(&mut result, request(u1), None);
    assert_fault(&case["case_id"], &faulted);
    record(0, printed(faulted));
    record(
        1,
        printed(shared.print(&mut result, PrintRequest::SourceFile(source), None)),
    );
    record(
        2,
        printed(create_printer(printer_options()).print(
            &mut result,
            PrintRequest::SourceFile(source),
            None,
        )),
    );
    json!({"results": results})
}

fn observe_lifecycle(case: &Value) -> Value {
    let kind = case["kind"].as_str().expect("kind");
    let base = case["base"].as_str();
    let op = case["op"].as_str().expect("op");
    if matches!(op, "scope-fault" | "file-after-failure" | "bundle-fault") {
        return observe_scope_control(case);
    }
    if matches!(
        op,
        "reprint-after-failure" | "cross-arena" | "double-failure"
    ) {
        return observe_failure_carry(case);
    }
    let (mut arena, source) = parse(case["source"].as_str().unwrap_or(LIFECYCLE_SOURCE));
    if op != "failure" {
        let name = generated_name(&mut arena, source, kind, base, 0);
        let mut statements = vec![const_declaration(&mut arena, source, name)];
        if op == "clone" {
            let clone = arena.factory().clone_node(name).unwrap();
            let expression = arena
                .factory()
                .create_expression_statement(source, clone)
                .unwrap();
            statements.push(expression);
        }
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            vec![Box::new(StatementInjector {
                statements,
                prepend: false,
            })],
            false,
        )
        .expect("transform");
        let mut printer = create_printer(printer_options());
        let first = printed(printer.print(&mut result, PrintRequest::SourceFile(source), None));
        if op == "dispose" {
            result.dispose();
            let after = create_printer(printer_options()).print(
                &mut result,
                PrintRequest::SourceFile(source),
                None,
            );
            // The recorded divergence is exactly the typed lifecycle
            // refusal, never an arbitrary error under the same key.
            assert!(
                matches!(
                    after,
                    Err(PrinterError::Transform(TransformError::InvalidLifecycle { .. }))
                ),
                "{}: printing a disposed transformation is the typed InvalidLifecycle refusal, got {after:?}",
                case["case_id"]
            );
            let after = printed(after);
            return json!({"first": first, "after_dispose": after});
        }
        let second = printed(printer.print(&mut result, PrintRequest::SourceFile(source), None));
        return json!({"first": first, "second": second});
    }
    let u1_name = generated_name(&mut arena, source, kind, base, 0);
    let u1 = const_declaration(&mut arena, source, u1_name);
    let u2_name = generated_name(&mut arena, source, kind, base, 1);
    let u2 = const_declaration(&mut arena, source, u2_name);
    let fault = Rc::new(RefCell::new(None));
    let hooks = FailingHooks {
        target: u1,
        fault: Rc::clone(&fault),
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(hooks)],
        false,
    )
    .expect("transform");
    let request = |node| PrintRequest::StandaloneNode {
        node,
        writer: StandaloneWriter::MultiLine,
    };
    let mut shared = create_printer(printer_options());
    let mut results = Vec::new();
    let mut record = |index: usize, value: Value| {
        let mut value = value;
        value["op"] = json!(index);
        results.push(value);
    };
    record(
        0,
        printed(create_printer(printer_options()).print(&mut result, request(u1), None)),
    );
    *fault.borrow_mut() = Some(case["phase"].as_str().expect("phase").to_owned());
    let faulted = shared.print(&mut result, request(u1), None);
    assert!(
        fault.borrow().is_none(),
        "{}: the armed fault fired",
        case["case_id"]
    );
    assert!(
        matches!(
            faulted,
            Err(PrinterError::Transform(TransformError::Unsupported(
                UnsupportedEmitFeature::CustomTransformers
            )))
        ),
        "{}: the injected fault is the typed hook error, got {faulted:?}",
        case["case_id"]
    );
    record(1, printed(faulted));
    record(2, printed(shared.print(&mut result, request(u2), None)));
    record(
        3,
        printed(create_printer(printer_options()).print(&mut result, request(u2), None)),
    );
    json!({"results": results})
}

// ---------------------------------------------------------------- comparison

fn text_of(value: &Value) -> Value {
    json!({"status": value["status"], "text": value["text"]})
}

/// Compare one native observation against the upstream one: status and text
/// of every printed result; the queried global names and error messages are
/// recorded only.
fn mismatches(case: &Value, route: &str, actual: &Value) -> Vec<(String, String)> {
    let id = case["case_id"].as_str().unwrap();
    let expected = &case["typescript_observation"];
    let mut out = Vec::new();
    let mut check = |key: &str, left: &Value, right: &Value| {
        if text_of(left) != text_of(right) {
            out.push((
                format!("{id}#{route}{key}"),
                format!("expected {} actual {}", text_of(left), text_of(right)),
            ));
        }
    };
    match case["group"].as_str().unwrap() {
        "synthetic" => check(
            "",
            &json!({"status": "returned", "text": expected["js_text"]}),
            actual,
        ),
        "global" => check("", expected, actual),
        _ if expected.get("results").is_some() => {
            let expected_results = expected["results"].as_array().unwrap();
            let actual_results = actual["results"].as_array().unwrap();
            assert_eq!(
                expected_results.len(),
                actual_results.len(),
                "{id}: op count"
            );
            for (index, (left, right)) in expected_results.iter().zip(actual_results).enumerate() {
                check(&format!("#op{index}"), left, right);
            }
        }
        _ => {
            for key in ["first", "second", "after_dispose"] {
                if expected.get(key).is_some() {
                    check(
                        &format!("#{key}"),
                        &json!({"status": "returned", "text": expected[key]["text"]}),
                        &actual[key],
                    );
                }
            }
        }
    }
    out
}

fn observe(case: &Value, route: &str) -> Value {
    match case["group"].as_str().unwrap() {
        "synthetic" => emit_synthetic(case, route == "oracle"),
        "global" => observe_global(case),
        "lifecycle" => observe_lifecycle(case),
        other => panic!("unknown group {other}"),
    }
}

#[test]
fn decorator_binding_direct_controls_match_typescript() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("fixtures/decorator-binding-direct.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-generated-name-controls");
    assert_eq!(artifact["repetitions"], 2);
    assert!(
        artifact["selection"].is_null(),
        "frozen fixture is complete"
    );
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 146);
    assert_eq!(
        artifact["groups"],
        json!({"synthetic": 48, "global": 22, "lifecycle": 76})
    );
    let carry_edges: Value =
        serde_json::from_slice(include_bytes!("fixtures/decorator-binding-carry-edge.json"))
            .unwrap();
    assert_eq!(carry_edges["typescript"], "6.0.3");
    assert_eq!(carry_edges["repetitions"], 2);
    let edge_cases = carry_edges["cases"].as_array().unwrap();
    assert_eq!(edge_cases.len(), 10);
    let mut all_mismatches = Vec::new();
    let mut observations = Vec::new();
    let mut exact = 0usize;
    let mut compared = 0usize;
    for case in cases.iter().chain(edge_cases) {
        let id = case["case_id"].as_str().unwrap();
        let routes: &[&str] = if case["group"] == "synthetic" {
            &["plain", "oracle"]
        } else {
            &["direct"]
        };
        for route in routes {
            let first = observe(case, route);
            let second = observe(case, route);
            assert_eq!(first, second, "{id}#{route}: repeat drift");
            let found = mismatches(case, route, &first);
            compared += 1;
            if found.is_empty() {
                exact += 1;
                eprintln!("decorator binding direct EXACT x2 {id}#{route}");
            } else {
                for (key, detail) in &found {
                    eprintln!("MISMATCH {key}: {detail}");
                }
                all_mismatches.extend(found);
            }
            observations.push(json!({"case_id": id, "route": route, "actual": first}));
        }
    }
    if let Some(directory) = std::env::var_os("TSC_RS_DECORATOR_BINDING_REPORT_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("decorator-binding-direct-native.json"),
            serde_json::to_vec_pretty(&json!({"exact": exact, "compared": compared,
                "mismatches": all_mismatches.iter().map(|(key, detail)| json!({"key": key, "detail": detail})).collect::<Vec<_>>(),
                "observations": observations}))
            .unwrap(),
        )
        .unwrap();
    }
    eprintln!(
        "decorator binding direct SUMMARY exact={exact} compared={compared} mismatching={}",
        all_mismatches.len()
    );
    let observed: BTreeSet<&str> = all_mismatches.iter().map(|(key, _)| key.as_str()).collect();
    let known: BTreeSet<&str> = KNOWN_DIVERGENCES.iter().map(|(key, _)| *key).collect();
    assert_eq!(
        observed, known,
        "mismatch set differs from the recorded divergences"
    );
}
