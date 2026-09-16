//! Failure order and continuation of the printer pipeline.
//!
//! Replays `fixtures/printer-failure-hooks.json`, recorded from the fixed
//! TypeScript printer by `scripts/observe-printer-failures.mjs`: every case is a
//! sequence of print operations against ONE shared printer (plus fresh
//! controls) with a transformer whose substitute / before / after hooks fail at
//! a selected occurrence. No adapter adds cleanup: an `after` hook that never
//! runs is part of the observation, and so is the next print's output.
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, EmitFlags, EmitHint, GeneratedIdentifierFlags,
    NewLineKind, PrintRequest, PrintedText, Printer, PrinterError, PrinterOptions,
    SourceFileTextMode, SourceRange, StandaloneWriter, TransformArena, TransformBundle,
    TransformError, TransformFlags, TransformNode, TransformNodeArray, TransformRoot,
    TransformSourceId, TransformationContext, Transformer, UnsupportedEmitFeature,
};
use tsc_syntax::{nodes::ArrayLiteralExpressionData, parse_source_file, NodeData, SyntaxKind};
use tsc_types::NodeFlags;

/// Divergences that remain after the candidate, keyed by `case_id#opN` or
/// `case_id#events`. Each entry names its design boundary; the test is green
/// only when the observed mismatch set equals this list exactly.
const KNOWN_DIVERGENCES: &[(&str, &str)] = &[(
    "printer-failure/printNode/unique-name/after/statement-1/recover-new-unique#op2",
    "tsc generates names lazily inside the print and keeps generatedNames after a \
         failure (x_2); Rust finalizes generated binding names eagerly per print on the \
         transformation, so a later print restarts at x_1",
)];

#[derive(Clone, Debug, Default)]
struct OpConfig {
    index: usize,
    fault: Option<(String, u16, u32)>,
    substitution: Option<(u16, u32, String)>,
    substitution_hint: Option<String>,
}

#[derive(Default)]
struct HookState {
    config: OpConfig,
    counts: BTreeMap<(String, u16), u32>,
    events: Vec<serde_json::Value>,
    /// The substitute identifier. TypeScript creates it inside the hook;
    /// the Rust transformation context refuses factory use after
    /// completion (`InvalidLifecycle`), so the replay pre-creates it.
    substitution_node: Option<TransformNode>,
}

struct FailingHooks {
    tracked: Vec<SyntaxKind>,
    state: Rc<RefCell<HookState>>,
    helper: Option<tsc_emitter::EmitHelper>,
}

fn kind_from_number(kind: u64) -> SyntaxKind {
    [
        SyntaxKind::Identifier,
        SyntaxKind::ExpressionStatement,
        SyntaxKind::VariableStatement,
        SyntaxKind::VariableDeclarationList,
        SyntaxKind::VariableDeclaration,
        SyntaxKind::CallExpression,
        SyntaxKind::Block,
        SyntaxKind::SourceFile,
    ]
    .into_iter()
    .find(|candidate| *candidate as u64 == kind)
    .unwrap_or_else(|| panic!("untracked syntax kind {kind}"))
}

impl FailingHooks {
    /// Count, record, then fail: the occurrence is counted per op, phase and
    /// kind exactly as the upstream observer counts its handler calls.
    fn observe(
        &self,
        context: &TransformationContext,
        phase: &str,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<u32, TransformError> {
        let arena = context.arena();
        let record = arena.node(node)?;
        let kind = record.kind as u16;
        let positions = arena.source(node.source())?.syntax().positions();
        let position = |value: u32| {
            if value == u32::MAX {
                -1
            } else {
                i64::from(positions.byte_to_utf16(value).unwrap())
            }
        };
        let (pos, end) = (position(record.pos), position(record.end));
        let mut state = self.state.borrow_mut();
        let occurrence = {
            let count = state.counts.entry((phase.to_owned(), kind)).or_insert(0);
            *count += 1;
            *count
        };
        let op = state.config.index;
        state.events.push(serde_json::json!({
            "op": op, "phase": phase, "hint": format!("{hint:?}"), "kind": kind, "pos": pos, "end": end
        }));
        if let Some((fault_phase, fault_kind, fault_occurrence)) = &state.config.fault {
            if fault_phase == phase && *fault_kind == kind && *fault_occurrence == occurrence {
                return Err(TransformError::Unsupported(
                    UnsupportedEmitFeature::CustomTransformers,
                ));
            }
        }
        Ok(occurrence)
    }
}

impl Transformer for FailingHooks {
    fn name(&self) -> &'static str {
        "printer-failure-hooks"
    }
    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        for kind in self.tracked.iter().copied() {
            context.enable_substitution(kind)?;
            context.enable_emit_notification(kind)?;
        }
        Ok(())
    }
    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        if let Some(helper) = &self.helper {
            context.request_emit_helper(helper.clone())?;
        }
        Ok(root)
    }
    fn substitute_node(
        &mut self,
        context: &mut TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let occurrence = self.observe(context, "substitute", hint, node)?;
        let kind = context.arena().node(node)?.kind as u16;
        let replacement = self.state.borrow().config.substitution.clone();
        if let Some((substitute_kind, substitute_occurrence, _)) = replacement {
            if substitute_kind == kind
                && substitute_occurrence == occurrence
                && self
                    .state
                    .borrow()
                    .config
                    .substitution_hint
                    .as_ref()
                    .is_none_or(|expected| expected == &format!("{hint:?}"))
            {
                return Ok(self
                    .state
                    .borrow()
                    .substitution_node
                    .expect("pre-created substitute identifier"));
            }
        }
        Ok(node)
    }
    fn before_emit_node(
        &mut self,
        context: &TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        self.observe(context, "before", hint, node).map(|_| ())
    }
    fn after_emit_node(
        &mut self,
        context: &TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        self.observe(context, "after", hint, node).map(|_| ())
    }
}

#[derive(Clone)]
enum Target {
    Node(TransformNode),
    Source(TransformSourceId),
    Bundle,
}

fn statement(arena: &TransformArena, source: TransformSourceId, index: usize) -> TransformNode {
    let root = arena.root(source).unwrap();
    let NodeData::SourceFile(data) = &arena.node(root).unwrap().data else {
        panic!("source file")
    };
    let id = arena
        .node_array(TransformNodeArray::new(source, data.statements.unwrap()))
        .unwrap()
        .nodes[index];
    arena.node_ref(source, id).unwrap()
}

fn expression_of(arena: &TransformArena, statement: TransformNode) -> TransformNode {
    let NodeData::ExpressionStatement(data) = &arena.node(statement).unwrap().data else {
        panic!("expression statement")
    };
    arena
        .node_ref(statement.source(), data.expression.unwrap())
        .unwrap()
}

fn unique_statement(arena: &mut TransformArena, source: TransformSourceId) -> TransformNode {
    let mut factory = arena.factory();
    let name = factory
        .create_unique_name(source, "x", GeneratedIdentifierFlags::NONE)
        .unwrap();
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

fn build_targets(
    case: &serde_json::Value,
    arena: &mut TransformArena,
    sources: &[TransformSourceId],
    parsed: &[tsc_syntax::SourceFile],
) -> BTreeMap<String, Target> {
    let id = case["case_id"].as_str().unwrap();
    let mut targets = BTreeMap::new();
    match case["entry"].as_str().unwrap() {
        "printFile" => {
            targets.insert("file".to_owned(), Target::Source(sources[0]));
            if let Some(other) = sources.get(1) {
                targets.insert("other".to_owned(), Target::Source(*other));
            }
        }
        "printBundle" => {
            targets.insert("bundle".to_owned(), Target::Bundle);
        }
        _ if id.contains("/unique-name/") => {
            targets.insert(
                "u1".to_owned(),
                Target::Node(unique_statement(arena, sources[0])),
            );
            targets.insert(
                "u2".to_owned(),
                Target::Node(unique_statement(arena, sources[0])),
            );
        }
        _ if id.contains("/cursor-array") => {
            let s0 = statement(arena, sources[0], 0);
            let call = expression_of(arena, s0);
            let NodeData::CallExpression(data) = arena.node(call).unwrap().data.clone() else {
                panic!("call")
            };
            let arguments = arena
                .node_array(TransformNodeArray::new(sources[0], data.arguments.unwrap()))
                .unwrap()
                .nodes
                .clone();
            let items = arguments[1..]
                .iter()
                .map(|id| arena.node_ref(sources[0], *id).unwrap())
                .collect::<Vec<_>>();
            let record = arena.node(call).unwrap();
            let range =
                SourceRange::from_raw(record.pos, record.end, parsed[0].positions()).unwrap();
            let elements = arena
                .factory()
                .create_node_array(sources[0], items)
                .unwrap();
            let array = arena
                .factory()
                .create_node(
                    sources[0],
                    NodeData::ArrayLiteralExpression(ArrayLiteralExpressionData {
                        elements: Some(elements.array()),
                    }),
                    TransformFlags::NONE,
                )
                .unwrap();
            arena.factory().set_multi_line(array, false).unwrap();
            arena
                .factory()
                .set_text_range_from_source_range(array, sources[0], range)
                .unwrap();
            arena.set_original_node(array, Some(call)).unwrap();
            targets.insert("s0".to_owned(), Target::Node(s0));
            targets.insert("array_bc".to_owned(), Target::Node(array));
        }
        _ => {
            let s0 = statement(arena, sources[0], 0);
            targets.insert("s0".to_owned(), Target::Node(s0));
            targets.insert(
                "s1".to_owned(),
                Target::Node(statement(arena, sources[0], 1)),
            );
            if let Some(other) = sources.get(1) {
                targets.insert("o0".to_owned(), Target::Node(statement(arena, *other, 0)));
            }
            for entry in case["emit_flags"].as_array().into_iter().flatten() {
                assert_eq!(entry["target"], "s0");
                assert_eq!(entry["path"], "expression");
                let node = expression_of(arena, s0);
                let flags = u32::try_from(entry["flags"].as_u64().unwrap()).unwrap();
                arena
                    .metadata_mut(node)
                    .set_flags(EmitFlags::from_bits(flags));
            }
        }
    }
    if case["case_id"].as_str().unwrap().contains("/review/") {
        targets.insert("file".to_owned(), Target::Source(sources[0]));
        if let Some(other) = sources.get(1) {
            targets.insert("other".to_owned(), Target::Source(*other));
        }
        targets.insert("bundle".to_owned(), Target::Bundle);
        if let Some(identifiers) = case["synthetic_identifiers"].as_object() {
            for (target, units) in identifiers {
                let units = units
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
                    .collect::<Vec<_>>();
                let value = tsc_diagnostics::JsString::from_code_units(&units);
                let id = arena
                    .factory()
                    .create_unchecked_identifier(sources[0], value.as_js())
                    .unwrap();
                let statement = arena
                    .factory()
                    .create_expression_statement(sources[0], id)
                    .unwrap();
                targets.insert(target.to_owned(), Target::Node(statement));
            }
        }
    }
    if let Some(specs) = case["target_specs"].as_object() {
        for (name, spec) in specs {
            targets.insert(
                name.to_owned(),
                Target::Node(statement(
                    arena,
                    sources[spec["source"].as_u64().unwrap() as usize],
                    spec["index"].as_u64().unwrap() as usize,
                )),
            );
        }
    }
    targets
}

fn measure(printed: &PrintedText, op: usize) -> serde_json::Value {
    let text = printed.text();
    serde_json::json!({
        "op": op, "status": "returned", "text": text,
        "utf8_base64": base64_encode(text.as_bytes()), "utf8_bytes": text.len(),
        "end_utf16": {"position": printed.end().position().value(), "line": printed.end().line(), "column": printed.end().column()}
    })
}

fn replay(case: &serde_json::Value) -> serde_json::Value {
    let parsed = case["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| {
            parse_source_file(
                source["name"].as_str().unwrap(),
                source["text"].as_str().unwrap(),
                Default::default(),
                None,
            )
        })
        .collect::<Vec<_>>();
    let mut arena = TransformArena::new();
    let sources = parsed
        .iter()
        .map(|file| arena.add_source(file, None))
        .collect::<Vec<_>>();
    let targets = build_targets(case, &mut arena, &sources, &parsed);
    let substitution_node = case["ops"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|op| op["substitution"]["replacement"].as_str())
        .map(|replacement| {
            arena
                .factory()
                .create_identifier(sources[0], replacement)
                .unwrap()
        });
    let entry = case["entry"].as_str().unwrap();
    let roots = if entry == "printBundle"
        || case["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op["entry"] == "printBundle")
    {
        let mut roots = vec![TransformRoot::Bundle(TransformBundle::new(sources.clone()))];
        if case["case_id"].as_str().unwrap().contains("/review/") {
            roots.extend(
                sources
                    .iter()
                    .map(|source| TransformRoot::SourceFile(*source)),
            );
        }
        roots
    } else {
        sources
            .iter()
            .map(|source| TransformRoot::SourceFile(*source))
            .collect()
    };
    let state = Rc::new(RefCell::new(HookState {
        substitution_node,
        ..HookState::default()
    }));
    let hooks = FailingHooks {
        tracked: case["tracked"]
            .as_array()
            .unwrap()
            .iter()
            .map(|kind| kind_from_number(kind.as_u64().unwrap()))
            .collect(),
        state: Rc::clone(&state),
        helper: case["helper"].as_object().map(|helper| {
            tsc_emitter::EmitHelper::with_text(
                helper["name"].as_str().unwrap(),
                false,
                helper["text"].as_str().unwrap(),
                None,
                vec![],
            )
        }),
    };
    let mut transformation = transform_nodes(arena, roots, vec![Box::new(hooks)], false).unwrap();
    let bundle = match transformation.roots()[0].clone() {
        TransformRoot::Bundle(bundle) => Some(bundle),
        TransformRoot::SourceFile(_) => None,
    };
    let mut options = PrinterOptions::new(if case["newLine"] == "lf" {
        NewLineKind::LineFeed
    } else {
        NewLineKind::CarriageReturnLineFeed
    })
    .with_remove_comments(case["removeComments"].as_bool() == Some(true))
    .with_source_file_text_mode(SourceFileTextMode::Canonical);
    if let Some(kind) = case["moduleKind"].as_i64() {
        options = options.with_module_kind(i32::try_from(kind).unwrap());
    }
    let mut shared = create_printer(options);
    let mut results = Vec::new();
    for (index, op) in case["ops"].as_array().unwrap().iter().enumerate() {
        {
            let mut state = state.borrow_mut();
            state.config = OpConfig {
                index,
                fault: op["fault"].as_object().map(|fault| {
                    (
                        fault["phase"].as_str().unwrap().to_owned(),
                        u16::try_from(fault["kind"].as_u64().unwrap()).unwrap(),
                        u32::try_from(fault["occurrence"].as_u64().unwrap()).unwrap(),
                    )
                }),
                substitution: op["substitution"].as_object().map(|substitution| {
                    (
                        u16::try_from(substitution["kind"].as_u64().unwrap()).unwrap(),
                        u32::try_from(substitution["occurrence"].as_u64().unwrap()).unwrap(),
                        substitution["replacement"].as_str().unwrap().to_owned(),
                    )
                }),
                substitution_hint: op["substitution"]["hint"].as_str().map(str::to_owned),
            };
            state.counts.clear();
        }
        let mut fresh;
        let printer: &mut Printer = if op["printer"] == "fresh" {
            fresh = create_printer(options);
            &mut fresh
        } else {
            &mut shared
        };
        let request = match targets[op["target"].as_str().unwrap()].clone() {
            Target::Node(node) => PrintRequest::StandaloneNode {
                node,
                writer: StandaloneWriter::MultiLine,
            },
            Target::Source(source) => PrintRequest::SourceFile(source),
            Target::Bundle => PrintRequest::Bundle(bundle.clone().expect("bundle root")),
        };
        results.push(match printer.print(&mut transformation, request, None) {
            Ok(printed) => {
                let mut value = measure(&printed, index);
                if case["case_id"].as_str().unwrap().contains("/review/") {
                    value["text_utf16"] = serde_json::json!(printed.text_utf16().as_ref());
                }
                value
            }
            Err(error) => {
                assert!(
                    matches!(
                        error,
                        PrinterError::Transform(TransformError::Unsupported(
                            UnsupportedEmitFeature::CustomTransformers
                        ))
                    ),
                    "unexpected error at op {index}: {error:?}"
                );
                assert!(op["fault"].is_object(), "unarmed fault at op {index}");
                serde_json::json!({"op": index, "status": "threw", "error": format!("{error:?}")})
            }
        });
    }
    let events = std::mem::take(&mut state.borrow_mut().events);
    serde_json::json!({"events": events, "results": results})
}

fn without_error(value: &serde_json::Value) -> serde_json::Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("error");
    }
    value
}

/// Compare one replay against the upstream observation; every mismatch is
/// reported by `op` (results) or as the first differing event.
fn compare(
    id: &str,
    expected: &serde_json::Value,
    actual: &serde_json::Value,
) -> Vec<(String, String)> {
    let mut mismatches = Vec::new();
    let expected_events = expected["events"].as_array().unwrap();
    let actual_events = actual["events"].as_array().unwrap();
    if expected_events != actual_events {
        let first = expected_events
            .iter()
            .zip(actual_events)
            .position(|(left, right)| left != right)
            .unwrap_or(expected_events.len().min(actual_events.len()));
        mismatches.push((
            format!("{id}#events"),
            format!(
                "expected {} events, actual {}; first difference at {first}: expected {} actual {}",
                expected_events.len(),
                actual_events.len(),
                expected_events
                    .get(first)
                    .map_or("<none>".to_owned(), ToString::to_string),
                actual_events
                    .get(first)
                    .map_or("<none>".to_owned(), ToString::to_string)
            ),
        ));
    }
    let expected_results = expected["results"].as_array().unwrap();
    let actual_results = actual["results"].as_array().unwrap();
    assert_eq!(
        expected_results.len(),
        actual_results.len(),
        "{id}: op count"
    );
    for (index, (left, right)) in expected_results.iter().zip(actual_results).enumerate() {
        if without_error(left) != without_error(right) {
            mismatches.push((
                format!("{id}#op{index}"),
                format!("expected {left} actual {right}"),
            ));
        }
    }
    mismatches
}

#[test]
fn printer_failure_hooks_match_typescript() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/printer-failure-hooks.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-printer-hook-faults");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 25);
    let mut mismatches = Vec::new();
    let mut actual_observations = Vec::new();
    let mut exact = 0usize;
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let first = replay(case);
        let second = replay(case);
        assert_eq!(first, second, "{id}: repetitions differ");
        let found = compare(id, &case["typescript_observation"], &first);
        if found.is_empty() {
            exact += 1;
        }
        mismatches.extend(found);
        actual_observations.push((id.to_owned(), first));
    }
    let known = KNOWN_DIVERGENCES
        .iter()
        .map(|(key, _)| (*key).to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let observed = mismatches
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<std::collections::BTreeSet<_>>();
    println!(
        "printer failure hooks: {exact}/{} cases exact; {} mismatching keys; {} known",
        cases.len(),
        observed.len(),
        known.len()
    );
    for (key, detail) in &mismatches {
        println!("MISMATCH {key}: {detail}");
    }
    // Complete Rust-side captures for the record: every op of every case,
    // written next to the upstream fixture when the caller asks for them.
    if let Ok(path) = std::env::var("TSC_RS_PRINTER_FAILURE_ACTUAL") {
        let captured = serde_json::json!({
            "route": "rust-replay",
            "cases": actual_observations.iter().map(|(id, actual)| {
                serde_json::json!({"case_id": id, "rust_observation": actual})
            }).collect::<Vec<_>>(),
        });
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&captured).unwrap() + "\n",
        )
        .unwrap_or_else(|error| panic!("write {path}: {error}"));
    }
    for (id, actual) in &actual_observations {
        for result in actual["results"].as_array().unwrap() {
            if result["status"] == "threw" {
                println!("THREW {id}#op{}: {}", result["op"], result["error"]);
            }
        }
    }
    assert_known_native_results(&actual_observations);
    assert_eq!(
        observed, known,
        "printer failure hook mismatches differ from the recorded divergence list"
    );
}

fn assert_known_native_results(actual_observations: &[(String, serde_json::Value)]) {
    let fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/printer-failure-known-native.json"))
            .unwrap();
    let rows = fixture["cases"].as_array().unwrap();
    assert_eq!(rows.len(), KNOWN_DIVERGENCES.len());
    let keys = rows
        .iter()
        .map(|row| row["key"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        keys,
        KNOWN_DIVERGENCES.iter().map(|(key, _)| *key).collect()
    );
    for row in rows {
        assert_eq!(
            row["disposition"],
            "deferred-compatibility-gap-no-exact-credit"
        );
        let (id, op) = row["key"].as_str().unwrap().rsplit_once("#op").unwrap();
        let (_, actual) = actual_observations
            .iter()
            .find(|(case, _)| case == id)
            .unwrap();
        assert_eq!(
            actual["results"][op.parse::<usize>().unwrap()],
            row["native_result"],
            "known divergence changed: {}",
            row["key"]
        );
    }
}

#[test]
fn known_native_controls_reject_widened_gaps() {
    let fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/printer-failure-hooks.json")).unwrap();
    let mut observations = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            KNOWN_DIVERGENCES
                .iter()
                .any(|(key, _)| key.split('#').next() == case["case_id"].as_str())
        })
        .map(|case| (case["case_id"].as_str().unwrap().to_owned(), replay(case)))
        .collect::<Vec<_>>();
    assert_known_native_results(&observations);
    // The mismatch remains at the SAME registered op, but its native tuple
    // has changed. A set-of-mismatch-keys check alone used to accept this.
    observations[0].1["results"][2]["text"] = "widened gap".into();
    assert!(std::panic::catch_unwind(|| assert_known_native_results(&observations)).is_err());
}

#[test]
fn cloning_a_failed_printer_keeps_independent_continuations() {
    let text = "/*a*/ f(x); //t\r\ng(y);\r\n";
    let parsed = parse_source_file("main.ts", text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let node = statement(&arena, source, 0);
    let state = Rc::new(RefCell::new(HookState::default()));
    let hooks = FailingHooks {
        tracked: vec![SyntaxKind::ExpressionStatement, SyntaxKind::Identifier],
        state: Rc::clone(&state),
        helper: None,
    };
    let mut transformation = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(hooks)],
        false,
    )
    .unwrap();
    let mut printer = create_printer(PrinterOptions::new(NewLineKind::CarriageReturnLineFeed));
    state.borrow_mut().config.fault = Some(("before".into(), SyntaxKind::Identifier as u16, 2));
    let request = || PrintRequest::StandaloneNode {
        node,
        writer: StandaloneWriter::MultiLine,
    };
    assert!(matches!(
        printer.print(&mut transformation, request(), None),
        Err(PrinterError::Transform(TransformError::Unsupported(
            UnsupportedEmitFeature::CustomTransformers
        )))
    ));
    let mut fork = printer.clone();
    assert_eq!(printer, fork);
    state.borrow_mut().config.fault = None;
    let first = printer.print(&mut transformation, request(), None).unwrap();
    let fork_first = fork.print(&mut transformation, request(), None).unwrap();
    // TypeScript has no clone API: compare the Rust fork's value and isolation
    // against the independently recorded continuation of the same prefix.
    let fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/printer-failure-hooks.json")).unwrap();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| {
            case["case_id"] == "printer-failure/printNode/before/identifier-2/recover-same"
        })
        .unwrap();
    assert_eq!(
        measure(&first, 2),
        case["typescript_observation"]["results"][2]
    );
    assert_eq!(measure(&fork_first, 2), measure(&first, 2));
    assert_eq!(
        measure(
            &printer.print(&mut transformation, request(), None).unwrap(),
            3
        ),
        case["typescript_observation"]["results"][3]
    );
    assert_eq!(
        measure(
            &fork.print(&mut transformation, request(), None).unwrap(),
            3
        ),
        case["typescript_observation"]["results"][3]
    );
}

#[test]
fn printer_failure_probes_are_recorded_upstream_only_evidence() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/printer-failure-probes.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "upstream-only-printer-probes");
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 14);
    for case in cases {
        assert_eq!(case["rust_counterpart"], "absent", "{}", case["case_id"]);
        assert!(case["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty()));
    }
}

/// Rust-only typed errors are a separate battery: they have no TypeScript
/// observation and claim no compatibility. This pins the current behavior so
/// a later change to it is deliberate.
#[test]
fn rust_only_typed_errors_keep_the_printer_usable() {
    let text = "/*a*/ f(x); //t\r\n";
    let parsed = parse_source_file("main.ts", text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let s0 = statement(&arena, source, 0);
    let root = arena.root(source).unwrap();
    let NodeData::SourceFile(data) = &arena.node(root).unwrap().data else {
        panic!("source file")
    };
    let statements = TransformNodeArray::new(source, data.statements.unwrap());
    let state = Rc::new(RefCell::new(HookState::default()));
    let hooks = FailingHooks {
        tracked: vec![SyntaxKind::SourceFile, SyntaxKind::ExpressionStatement],
        state: Rc::clone(&state),
        helper: None,
    };
    let replacement = arena.factory().create_identifier(source, "y").unwrap();
    state.borrow_mut().substitution_node = Some(replacement);
    let mut transformation = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(hooks)],
        false,
    )
    .unwrap();
    // Text mode PreserveUnchanged: the H1 identity arm, unreachable under
    // compiler emit and never a TypeScript printer path.
    let mut printer = create_printer(PrinterOptions::new(NewLineKind::CarriageReturnLineFeed));

    // 1. A typed unsupported request touches no writer; the next print is
    // fresh.
    let error = printer
        .print(
            &mut transformation,
            PrintRequest::NodeList(statements),
            None,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        PrinterError::Unsupported(UnsupportedEmitFeature::NodeListPrinting)
    ));
    let printed = printer
        .print(
            &mut transformation,
            PrintRequest::StandaloneNode {
                node: s0,
                writer: StandaloneWriter::MultiLine,
            },
            None,
        )
        .unwrap();
    assert_eq!(printed.text(), text);
    state.borrow_mut().events.clear();
    state.borrow_mut().counts.clear();

    // 2. The identity arm refuses a substituted statement with a Rust-only
    // error AND runs the after-notifications for the statement and the
    // source file first: a restoration guard for a failure TypeScript cannot
    // produce, deliberately different from the no-cleanup rule of hook
    // faults (`printer_failure_hooks_match_typescript`).
    state.borrow_mut().config = OpConfig {
        index: 1,
        fault: None,
        substitution: Some((SyntaxKind::ExpressionStatement as u16, 1, "y".to_owned())),
        substitution_hint: None,
    };
    let error = printer
        .print(&mut transformation, PrintRequest::SourceFile(source), None)
        .unwrap_err();
    assert!(matches!(
        error,
        PrinterError::TransformedNodeWorkerUnavailable(node) if node == replacement
    ));
    let phases = state
        .borrow()
        .events
        .iter()
        .map(|event| format!("{}:{}", event["phase"].as_str().unwrap(), event["kind"]))
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        [
            "substitute:308",
            "before:308",
            "substitute:245",
            "before:245",
            "after:245",
            "after:308"
        ]
    );

    // 3. Nothing was written before the refusal, no comment scope was
    // recorded, so the next identity print is complete and unchanged.
    state.borrow_mut().config = OpConfig::default();
    state.borrow_mut().counts.clear();
    let printed = printer
        .print(&mut transformation, PrintRequest::SourceFile(source), None)
        .unwrap();
    assert_eq!(printed.text(), text);
}

#[test]
fn review_entry_carry_controls() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/printer-failure-review.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 21);
    let mut failures = Vec::new();
    let mut captures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let first = replay(case);
        assert_eq!(first, replay(case), "{id}: nondeterministic");
        let found = compare(id, &case["typescript_observation"], &first);
        if found.is_empty() {
            println!("REVIEW EXACT x2 {id}");
        } else {
            for (key, detail) in &found {
                println!("REVIEW MISMATCH {key}: {detail}");
            }
            failures.push(id);
        }
        captures.push(serde_json::json!({"case_id":id,"rust_observation":first}));
    }
    if let Ok(path) = std::env::var("TSC_RS_PRINTER_FAILURE_REVIEW_ACTUAL") {
        std::fs::write(
            path,
            serde_json::to_string_pretty(&captures).unwrap() + "\n",
        )
        .unwrap();
    }
    println!(
        "REVIEW exact {}/{}",
        cases.len() - failures.len(),
        cases.len()
    );
    assert!(failures.is_empty(), "review failures: {failures:?}");
}

#[test]
fn comment_containers_match_across_sources() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/printer-comment-carry.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 24);
    let mut captures = Vec::new();
    let mut exact = 0;
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let actual = replay(case);
        assert_eq!(actual, replay(case), "{id}: repetitions");
        exact += usize::from(assert_comment_carry(case, &actual));
        captures.push(serde_json::json!({"case_id":id,"rust_observation":actual}));
    }
    if let Ok(path) = std::env::var("TSC_RS_COMMENT_CARRY_ACTUAL") {
        std::fs::write(
            path,
            serde_json::to_string_pretty(&captures).unwrap() + "\n",
        )
        .unwrap();
    }
    println!(
        "COMMENT CARRY exact {exact}/{}; complete operation results and hook events",
        cases.len()
    );
    assert_eq!(exact, 24);
}

/// API1.2-HINT closes the four declaration-name gaps against the unchanged
/// TypeScript fixture. Both events and operation results must match exactly.
fn assert_comment_carry(case: &serde_json::Value, actual: &serde_json::Value) -> bool {
    let id = case["case_id"].as_str().unwrap();
    let found = compare(id, &case["typescript_observation"], actual);
    assert!(found.is_empty(), "{id}: {found:?}");
    println!("COMMENT CARRY EXACT x2 {id}");
    true
}

#[test]
fn comment_containers_survive_replaced_transformations() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/printer-comment-carry.json")).unwrap();
    for case in artifact["cases"].as_array().unwrap() {
        let first = replay_with_replaced_transformations(case);
        assert_eq!(first, replay_with_replaced_transformations(case));
        assert_comment_carry(case, &first);
    }
}

/// Recreate and drop the transformation for EACH print. Every source now gets
/// local source ID zero, and the prior arena is unavailable to the next print.
/// The same upstream sequence applies: allocation layout is Rust-only, so
/// these controls do not add another 24 cases to compatibility counts.
fn replay_with_replaced_transformations(case: &serde_json::Value) -> serde_json::Value {
    let options = PrinterOptions::new(if case["newLine"] == "lf" {
        NewLineKind::LineFeed
    } else {
        NewLineKind::CarriageReturnLineFeed
    })
    .with_source_file_text_mode(SourceFileTextMode::Canonical);
    let mut shared = create_printer(options);
    let state = Rc::new(RefCell::new(HookState::default()));
    let mut results = Vec::new();
    for (index, op) in case["ops"].as_array().unwrap().iter().enumerate() {
        let spec = &case["target_specs"][op["target"].as_str().unwrap()];
        let input = &case["sources"][spec["source"].as_u64().unwrap() as usize];
        let parsed = parse_source_file(
            input["name"].as_str().unwrap(),
            input["text"].as_str().unwrap(),
            Default::default(),
            None,
        );
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, None);
        let node = statement(&arena, source, spec["index"].as_u64().unwrap() as usize);
        {
            let mut state = state.borrow_mut();
            state.counts.clear();
            state.config = OpConfig {
                index,
                fault: op["fault"].as_object().map(|fault| {
                    (
                        fault["phase"].as_str().unwrap().to_owned(),
                        u16::try_from(fault["kind"].as_u64().unwrap()).unwrap(),
                        u32::try_from(fault["occurrence"].as_u64().unwrap()).unwrap(),
                    )
                }),
                substitution: None,
                substitution_hint: None,
            };
        }
        let hooks = FailingHooks {
            tracked: case["tracked"]
                .as_array()
                .unwrap()
                .iter()
                .map(|kind| kind_from_number(kind.as_u64().unwrap()))
                .collect(),
            state: Rc::clone(&state),
            helper: None,
        };
        let mut transformation = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            vec![Box::new(hooks)],
            false,
        )
        .unwrap();
        let mut fresh = create_printer(options);
        let printer = if op["printer"] == "fresh" {
            &mut fresh
        } else {
            &mut shared
        };
        let result = printer.print(
            &mut transformation,
            PrintRequest::StandaloneNode {
                node,
                writer: StandaloneWriter::MultiLine,
            },
            None,
        );
        results.push(match result {
            Ok(printed) => measure(&printed, index),
            Err(error) => {
                assert!(op["fault"].is_object());
                assert!(matches!(
                    error,
                    PrinterError::Transform(TransformError::Unsupported(
                        UnsupportedEmitFeature::CustomTransformers
                    ))
                ));
                serde_json::json!({"op":index,"status":"threw","error":format!("{error:?}")})
            }
        });
    }
    let events = std::mem::take(&mut state.borrow_mut().events);
    serde_json::json!({"events":events,"results":results})
}

#[test]
fn declaration_hook_hints_match_typescript_twice() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/printer-hook-hints.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 72);
    let mut captures = Vec::new();
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let actual = replay(case);
        assert_eq!(actual, replay(case), "{id}: repetitions");
        let found = compare(id, &case["typescript_observation"], &actual);
        if found.is_empty() {
            println!("HOOK HINT EXACT x2 {id}");
        } else {
            println!("HOOK HINT MISMATCH {id}: {found:?}");
            failures.push(id);
        }
        captures.push(serde_json::json!({"case_id":id,"rust_observation":actual}));
    }
    if let Ok(path) = std::env::var("TSC_RS_HOOK_HINT_ACTUAL") {
        std::fs::write(
            path,
            serde_json::to_string_pretty(&captures).unwrap() + "\n",
        )
        .unwrap();
    }
    println!(
        "HOOK HINT exact {}/{}",
        cases.len() - failures.len(),
        cases.len()
    );
    assert!(failures.is_empty(), "hook hint failures: {failures:?}");
}

#[test]
fn comment_carry_controls_reject_event_and_output_changes() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/printer-comment-carry.json")).unwrap();
    let case = artifact["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| {
            case["case_id"]
                .as_str()
                .unwrap()
                .contains("declaration-list-end/lf/before")
        })
        .unwrap();
    let mut actual = replay(case);
    assert!(assert_comment_carry(case, &actual));
    actual["events"][0]["hint"] = "widened gap".into();
    assert!(std::panic::catch_unwind(|| assert_comment_carry(case, &actual)).is_err());
    let mut actual = replay(case);
    actual["results"][2]["text"] = "widened output gap".into();
    assert!(std::panic::catch_unwind(|| assert_comment_carry(case, &actual)).is_err());
}
