//! CS-6 witness-driven full-pipeline fixture gate.
//!
//! Every case of the frozen comment-scope witness artifact drives the
//! port end to end: the stored input bytes parse, the case's transform
//! runs as a BEFORE-transformer (the oracle emitted with
//! `{ before: [transform] }`), the ES2016 script pipeline follows, and
//! the printed output must byte-equal the stored oracle output
//! (`observation.writes[0].callback_utf8_base64`). No expected value is
//! authored here: the artifact is the entire expectation, and a red case
//! is fixed in production under the frozen bytes' authority
//! (h2-5h-a-cs-6.md §6), never by amending the witness.

use serde_json::Value;

use std::path::Path;

use tsc_emitter::{
    create_printer, get_script_transformers_for_source, transform_nodes, DisabledSourceMapRecorder,
    EmitConstantValue, EmitEnumMemberValue, EmitFlags, EmitHost, EmitResolver, EmitResolverError,
    EmitResolverNode, EmitSource, NewLineKind, PrintRequest, PrinterOptions, SourceFileId,
    SourceFileTextMode, SourceRange, SyntheticComment, SyntheticCommentKind, TransformArena,
    TransformError, TransformFlags, TransformNode, TransformRoot, TransformSourceId,
    TransformationContext, Transformer,
};
use tsc_syntax::{
    escape_leading_underscores,
    nodes::{
        ArrowFunctionData, BlockData, CallExpressionData, ExpressionStatementData, IdentifierData,
        SourceFileData,
    },
    parse_source_file, NodeData, SyntaxKind,
};
use tsc_types::{CompilerOptions, ModuleKind, ScriptTarget};

const WITNESSES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-5h-a-comment-scope-witnesses.v1.json"
));

/// RFC 4648 standard-alphabet decoder; the artifact never needs URL-safe
/// or unpadded forms, and a new workspace dependency is not worth twenty
/// lines.
fn decode_base64(text: &str) -> Vec<u8> {
    fn value(byte: u8) -> u32 {
        match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 byte {byte:#x}"),
        }
    }
    let bytes = text.as_bytes();
    assert!(bytes.len().is_multiple_of(4), "unpadded base64 payload");
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let padding = chunk.iter().filter(|byte| **byte == b'=').count();
        let mut accumulator = 0_u32;
        for (index, byte) in chunk.iter().enumerate() {
            let part = if *byte == b'=' {
                assert!(index >= 2, "malformed base64 padding");
                0
            } else {
                value(*byte)
            };
            accumulator = (accumulator << 6) | part;
        }
        out.push((accumulator >> 16) as u8);
        if padding < 2 {
            out.push((accumulator >> 8) as u8);
        }
        if padding < 1 {
            out.push(accumulator as u8);
        }
    }
    out
}

/// The oracle emitted through a program host (its virtual /project); this
/// is the measured test-host shape the active-transform contracts use for
/// host-dependent module formats.
struct WitnessHost<'a> {
    options: &'a CompilerOptions,
    syntax: &'a tsc_syntax::SourceFile,
    source_ids: [SourceFileId; 1],
}

impl EmitHost for WitnessHost<'_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.options
    }

    fn current_directory(&self) -> &Path {
        Path::new("/")
    }

    fn common_source_directory(&self) -> &Path {
        Path::new("/")
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
}

/// The six frozen witness transforms, mirrored from the generator's
/// TRANSFORMS table. Case selectors are structural (call statements of a
/// named identifier, the lone `x;` statement, the lone block), exactly as
/// the oracle located them.
struct WitnessCaseTransformer {
    transform: String,
    /// Prebuilt zero-width `{ pos, pos }` range for the shrinkMe
    /// replacement, computed by the drive from the parsed tree exactly
    /// like the oracle's closure over `statement.pos`.
    shrink_range: Option<SourceRange>,
}

fn statements_of(
    context: &TransformationContext,
    source: TransformSourceId,
) -> (
    TransformNode,
    tsc_emitter::TransformNodeArray,
    Vec<TransformNode>,
) {
    let root = context.arena().root(source).expect("source root");
    let NodeData::SourceFile(data) = &context.arena().node(root).expect("root record").data else {
        panic!("root is not a source file");
    };
    let array_id = data.statements.expect("source statements");
    let array = context
        .arena()
        .node_array_ref(source, array_id)
        .expect("statements array");
    let nodes = context
        .arena()
        .node_array(array)
        .expect("statement nodes")
        .nodes
        .iter()
        .map(|id| context.arena().node_ref(source, *id).expect("statement"))
        .collect();
    (root, array, nodes)
}

fn call_statement_name(
    context: &TransformationContext,
    statement: TransformNode,
) -> Option<String> {
    let NodeData::ExpressionStatement(data) = &context.arena().node(statement).ok()?.data.clone()
    else {
        return None;
    };
    let expression = context
        .arena()
        .node_ref(statement.source(), data.expression?)?;
    let NodeData::CallExpression(call) = &context.arena().node(expression).ok()?.data.clone()
    else {
        return None;
    };
    let callee = context
        .arena()
        .node_ref(statement.source(), call.expression?)?;
    let NodeData::Identifier(identifier) = &context.arena().node(callee).ok()?.data else {
        return None;
    };
    Some(identifier.text.clone())
}

fn is_x_statement(context: &TransformationContext, statement: TransformNode) -> bool {
    let Ok(record) = context.arena().node(statement) else {
        return false;
    };
    let NodeData::ExpressionStatement(data) = &record.data.clone() else {
        return false;
    };
    let Some(expression) = data
        .expression
        .and_then(|id| context.arena().node_ref(statement.source(), id))
    else {
        return false;
    };
    matches!(
        &context.arena().node(expression).expect("expression").data,
        NodeData::Identifier(identifier) if identifier.text == "x"
    )
}

fn create_identifier(
    context: &mut TransformationContext,
    source: TransformSourceId,
    text: &str,
) -> TransformNode {
    context
        .factory()
        .expect("factory")
        .create_node(
            source,
            NodeData::Identifier(IdentifierData {
                escaped_text: escape_leading_underscores(text),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
        .expect("create identifier")
}

fn create_call_statement(
    context: &mut TransformationContext,
    source: TransformSourceId,
    callee: &str,
) -> TransformNode {
    let callee = create_identifier(context, source, callee);
    let factory = &mut context.factory().expect("factory");
    let arguments = factory
        .create_node_array(source, Vec::new())
        .expect("call arguments");
    let call = factory
        .create_node(
            source,
            NodeData::CallExpression(CallExpressionData {
                expression: Some(callee.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            TransformFlags::NONE,
        )
        .expect("create call");
    factory
        .create_node(
            source,
            NodeData::ExpressionStatement(ExpressionStatementData {
                expression: Some(call.node()),
            }),
            TransformFlags::NONE,
        )
        .expect("create call statement")
}

fn add_witness_synthetic_comments(context: &mut TransformationContext, node: TransformNode) {
    let metadata = context.arena_mut().expect("arena").metadata_mut(node);
    metadata.add_leading_comment(SyntheticComment::new(
        SyntheticCommentKind::MultiLine,
        " SYN-LEAD ",
        false,
        false,
    ));
    metadata.add_trailing_comment(SyntheticComment::new(
        SyntheticCommentKind::MultiLine,
        " SYN-TRAIL ",
        false,
        false,
    ));
}

fn replace_root_statements(
    context: &mut TransformationContext,
    source: TransformSourceId,
    root: TransformNode,
    statements: Vec<TransformNode>,
) -> Result<(), TransformError> {
    let end_of_file_token = match &context.arena().node(root)?.data {
        NodeData::SourceFile(data) => data.end_of_file_token,
        _ => unreachable!("root is a source file"),
    };
    let array = context.factory()?.create_node_array(source, statements)?;
    let flags = context.arena().transform_flags(root);
    let updated = context.factory()?.update_node(
        root,
        NodeData::SourceFile(SourceFileData {
            statements: Some(array.array()),
            end_of_file_token,
        }),
        flags,
    )?;
    context.arena_mut()?.replace_root(source, updated)?;
    Ok(())
}

impl Transformer for WitnessCaseTransformer {
    fn name(&self) -> &'static str {
        "comment-scope-witness-case"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Ok(root);
        };
        match self.transform.as_str() {
            "identity" => {}
            "wrap-expression-statements-in-synthetic-arrow" => {
                let (root_node, _, statements) = statements_of(context, source);
                let mut wrapped = 0;
                let mut replaced = Vec::with_capacity(statements.len());
                for statement in statements {
                    if !is_x_statement(context, statement) {
                        replaced.push(statement);
                        continue;
                    }
                    wrapped += 1;
                    let block_statements = context
                        .factory()?
                        .create_node_array(source, vec![statement])?;
                    let block = context.factory()?.create_node(
                        source,
                        NodeData::Block(BlockData {
                            statements: Some(block_statements.array()),
                        }),
                        TransformFlags::NONE,
                    )?;
                    let token = context.factory()?.create_token(
                        source,
                        SyntaxKind::EqualsGreaterThanToken,
                        TransformFlags::NONE,
                    )?;
                    let parameters = context.factory()?.create_node_array(source, Vec::new())?;
                    let arrow = context.factory()?.create_node(
                        source,
                        NodeData::ArrowFunction(ArrowFunctionData {
                            type_parameters: None,
                            parameters: Some(parameters.array()),
                            r#type: None,
                            body: Some(block.node()),
                            modifiers: None,
                            equals_greater_than_token: Some(token.node()),
                        }),
                        TransformFlags::NONE,
                    )?;
                    let wrapper = context.factory()?.create_node(
                        source,
                        NodeData::ExpressionStatement(ExpressionStatementData {
                            expression: Some(arrow.node()),
                        }),
                        TransformFlags::NONE,
                    )?;
                    replaced.push(wrapper);
                }
                assert_eq!(wrapped, 1, "synthetic wrapper target disappeared");
                replace_root_statements(context, source, root_node, replaced)?;
            }
            "apply-comment-emit-flags" => {
                let (_, _, statements) = statements_of(context, source);
                let mut flagged = 0;
                for statement in statements {
                    let flags = match call_statement_name(context, statement).as_deref() {
                        Some("suppressLead") => Some(EmitFlags::NO_LEADING_COMMENTS),
                        Some("suppressTrail") => Some(EmitFlags::NO_TRAILING_COMMENTS),
                        _ => {
                            let record = context.arena().node(statement)?;
                            if record.kind == SyntaxKind::Block {
                                Some(EmitFlags::NO_NESTED_COMMENTS)
                            } else {
                                None
                            }
                        }
                    };
                    if let Some(flags) = flags {
                        context
                            .arena_mut()?
                            .metadata_mut(statement)
                            .add_flags(flags);
                        flagged += 1;
                    }
                }
                assert_eq!(flagged, 3, "emit-flag targets disappeared");
            }
            "add-synthetic-comments-to-marked" => {
                let (_, _, statements) = statements_of(context, source);
                let mut annotated = 0;
                for statement in statements {
                    if call_statement_name(context, statement).as_deref() != Some("markMe") {
                        continue;
                    }
                    add_witness_synthetic_comments(context, statement);
                    annotated += 1;
                }
                assert_eq!(annotated, 1, "synthetic-comment target disappeared");
            }
            "append-synthetic-statement-with-synthetic-comments" => {
                let (root_node, _, statements) = statements_of(context, source);
                let synthetic = create_call_statement(context, source, "syntheticMarker");
                add_witness_synthetic_comments(context, synthetic);
                let mut replaced = statements;
                replaced.push(synthetic);
                replace_root_statements(context, source, root_node, replaced)?;
            }
            "replace-not-emitted-and-zero-width" => {
                let (root_node, _, statements) = statements_of(context, source);
                let mut dropped = 0;
                let mut shrunk = 0;
                let mut replaced = Vec::with_capacity(statements.len());
                for statement in statements {
                    match call_statement_name(context, statement).as_deref() {
                        Some("dropMe") => {
                            dropped += 1;
                            let not_emitted =
                                context.factory()?.create_not_emitted_statement(statement)?;
                            replaced.push(not_emitted);
                        }
                        Some("shrinkMe") => {
                            shrunk += 1;
                            let replacement =
                                create_call_statement(context, source, "shrunkMarker");
                            let range = self
                                .shrink_range
                                .expect("prebuilt zero-width range for shrinkMe");
                            context.factory()?.set_text_range_from_source_range(
                                replacement,
                                source,
                                range,
                            )?;
                            replaced.push(replacement);
                        }
                        _ => replaced.push(statement),
                    }
                }
                assert_eq!(
                    (dropped, shrunk),
                    (1, 1),
                    "not-emitted/zero-width targets disappeared"
                );
                replace_root_statements(context, source, root_node, replaced)?;
            }
            other => panic!("unknown witness transform {other}"),
        }
        Ok(TransformRoot::SourceFile(source))
    }
}

/// The stored serialized options are exactly this four-key set for every
/// case; anything else fails closed (h2-5h-a-cs-6.md §4: bootstrap
/// defaults such as `always_strict` would silently break prologue byte
/// parity).
fn case_compiler_options(serialized: &Value) -> (CompilerOptions, bool) {
    let map = serialized.as_object().expect("serialized options object");
    let mut remove_comments = false;
    for (key, value) in map {
        match key.as_str() {
            "module" => assert_eq!(value.as_u64(), Some(99), "module must be ESNext"),
            "newLine" => assert_eq!(value.as_u64(), Some(1), "newLine must be LF"),
            "target" => assert_eq!(value.as_u64(), Some(3), "target must be ES2016"),
            "removeComments" => {
                remove_comments = value.as_bool().expect("removeComments bool");
            }
            other => panic!("unexpected stored compiler option {other}"),
        }
    }
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2016.bits()),
        module: Some(ModuleKind::ES_NEXT.bits()),
        ..CompilerOptions::default()
    };
    (options, remove_comments)
}

fn shrink_range_for(parsed: &tsc_syntax::SourceFile) -> Option<SourceRange> {
    let NodeData::SourceFile(source_file) = &parsed.arena.node(parsed.root).data else {
        panic!("parsed root is a source file");
    };
    let statements = parsed
        .arena
        .node_array(source_file.statements.expect("parsed statements"));
    for statement in statements.nodes.iter().copied() {
        let record = parsed.arena.node(statement);
        let NodeData::ExpressionStatement(data) = &record.data else {
            continue;
        };
        let Some(expression) = data.expression else {
            continue;
        };
        let NodeData::CallExpression(call) = &parsed.arena.node(expression).data else {
            continue;
        };
        let Some(callee) = call.expression else {
            continue;
        };
        let NodeData::Identifier(identifier) = &parsed.arena.node(callee).data else {
            continue;
        };
        if identifier.text == "shrinkMe" {
            let pos = record.pos;
            let range = tsc_emitter::SourceByteRange::new(pos, pos, parsed.positions())
                .expect("zero-width shrink range");
            return Some(SourceRange::Original(range));
        }
    }
    None
}

fn drive_case(
    case_id: &str,
    transform: &str,
    input_text: &str,
    serialized_options: &Value,
) -> String {
    let parsed = parse_source_file("input.ts", input_text, Default::default(), None);
    let shrink_range = shrink_range_for(&parsed);
    let mut arena = TransformArena::new();
    let source_id = SourceFileId::from_raw(0);
    let source = arena.add_source(&parsed, Some(source_id));
    let (options, remove_comments) = case_compiler_options(serialized_options);
    let host = WitnessHost {
        options: &options,
        syntax: &parsed,
        source_ids: [source_id],
    };
    let mut transformers: Vec<Box<dyn Transformer>> = vec![Box::new(WitnessCaseTransformer {
        transform: transform.to_owned(),
        shrink_range,
    })];
    transformers.extend(
        get_script_transformers_for_source(&options, &NoConstantValueResolver, &host, source_id)
            .expect("script transformers"),
    );
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        transformers,
        false,
    )
    .unwrap_or_else(|error| panic!("{case_id}: transform failed: {error:?}"));
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::ES2016)
            .with_remove_comments(remove_comments)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .unwrap_or_else(|error| panic!("{case_id}: print failed: {error:?}"))
    .text()
    .to_owned()
}

fn first_divergence(expected: &str, actual: &str) -> String {
    let byte = expected
        .bytes()
        .zip(actual.bytes())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.len().min(actual.len()));
    let start = byte.saturating_sub(40);
    format!(
        "first divergence at byte {byte}\n  expected …{:?}\n  actual   …{:?}",
        &expected[start..(byte + 40).min(expected.len())],
        &actual[start..(byte + 40).min(actual.len())],
    )
}

/// The four production gaps the fixture gate surfaced on its first run
/// (h2-5h-a-cs-6.md §6 step 1): each is a frozen oracle byte sequence the
/// port does not yet reproduce, owned by the CS-6 train as production
/// fixes cited by these case ids. The list may only SHRINK: a case that
/// starts passing must be removed here, and any NEW divergence fails the
/// suite immediately.
const KNOWN_DIVERGENCES: [&str; 0] = [];

#[test]
fn every_frozen_witness_case_reproduces_the_oracle_bytes() {
    let artifact: Value = serde_json::from_slice(WITNESSES).expect("witness artifact JSON");
    let families = artifact["families"].as_array().expect("families");
    assert_eq!(families.len(), 10, "family census changed");
    let mut cases_run = 0;
    let mut failures: Vec<String> = Vec::new();
    let mut known_diverging: Vec<String> = Vec::new();
    for family in families {
        for case in family["cases"].as_array().expect("cases") {
            let case_id = case["case_id"].as_str().expect("case id");
            let observation = &case["observation"];

            // Structural guards measured across all 30 cases at design
            // time (h2-5h-a-cs-6.md §4).
            let files = case["input"]["files"].as_array().expect("input files");
            assert_eq!(files.len(), 1, "{case_id}: single input file");
            assert_eq!(
                case["input"]["roots"].as_array().expect("roots").len(),
                1,
                "{case_id}: single root"
            );
            let writes = observation["writes"].as_array().expect("writes");
            assert_eq!(writes.len(), 1, "{case_id}: single write");
            assert_eq!(
                observation["emit_skipped"].as_bool(),
                Some(false),
                "{case_id}: emit not skipped"
            );
            assert!(
                observation["reported_diagnostics"]
                    .as_array()
                    .expect("reported diagnostics")
                    .is_empty()
                    && observation["emit_diagnostics"]
                        .as_array()
                        .expect("emit diagnostics")
                        .is_empty(),
                "{case_id}: diagnostics-free"
            );

            let input_bytes =
                decode_base64(files[0]["utf8_base64"].as_str().expect("input base64"));
            let input_text = String::from_utf8(input_bytes).expect("input utf8");
            let expected_bytes = decode_base64(
                writes[0]["callback_utf8_base64"]
                    .as_str()
                    .expect("output base64"),
            );
            let expected = String::from_utf8(expected_bytes).expect("output utf8");

            let transform = case["transform"].as_str().expect("transform id");
            let actual = drive_case(
                case_id,
                transform,
                &input_text,
                &case["input"]["compiler_options"],
            );
            if actual != expected {
                if KNOWN_DIVERGENCES.contains(&case_id) {
                    known_diverging.push(case_id.to_owned());
                    cases_run += 1;
                    continue;
                }
                failures.push(format!(
                    "{case_id} ({transform})\n{}\n--- expected ---\n{expected}--- actual ---\n{actual}",
                    first_divergence(&expected, &actual),
                ));
            }
            cases_run += 1;
        }
    }
    assert_eq!(cases_run, 30, "case census changed");
    assert!(
        failures.is_empty(),
        "{} NEW frozen-case divergence(s) beyond the known set:\n\n{}",
        failures.len(),
        failures.join("\n=====\n"),
    );
    // Shrink-only: a known divergence that starts passing must leave the
    // list in the same change that fixes it.
    assert_eq!(
        known_diverging.len(),
        KNOWN_DIVERGENCES.len(),
        "known-divergence list is stale; now passing: {:?}",
        KNOWN_DIVERGENCES
            .iter()
            .filter(|id| !known_diverging.contains(&(**id).to_owned()))
            .collect::<Vec<_>>(),
    );
}
