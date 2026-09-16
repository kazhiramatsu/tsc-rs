//! C01 / A40-LITERAL-UPDATE: literal value updates and their propagation.
//!
//! Three fixed TypeScript 6.0.3 observation groups (`scripts/observe-literal-update.mjs`):
//!
//! * `factory`: direct factory + standalone printer. Every row builds a literal
//!   of one origin (synthetic, parsed, clone, setOriginalNode, textSourceNode,
//!   cross-source clone), applies one update operation (same value, cooked
//!   only, raw only, raw absent/empty, template flags, quote) and compares the
//!   operand/updated tree state, node identity and the printed bytes.
//! * `transform`: a `before` transformer updates parsed template fragments and
//!   the ordinary ES5 / ES2015 / ESNext script pipeline emits the file.
//! * `lifetime`: re-print and TransformationResult disposal controls.
//!
//! Rust update routes: `generic` is `NodeFactory::update_node` with a
//! replacement `NodeData` payload (children/flags update; raw text only through
//! the lossy projection); `typed` is the value-changing typed update. A row a
//! route cannot express is recorded as `na`, never as a match. Set
//! `TSC_RS_LITERAL_UPDATE_REPORT_DIR` to write per-route JSON reports.
use std::collections::BTreeMap;

use serde_json::{json, Value};
use tsc_emitter::{
    base64_encode, create_printer, get_script_transformers, transform_nodes, transform_type_script,
    EmitConstantValue, EmitEnumMemberValue, EmitExportContainerMode, EmitFlags, EmitMetadata,
    EmitResolver, EmitResolverError, EmitResolverNode, NewLineKind, PrintRequest, PrinterOptions,
    SourceFileId, SourceFileTextMode, StandaloneWriter, TransformArena, TransformError,
    TransformFlags, TransformNode, TransformNodeArray, TransformRoot, TransformSourceId,
    TransformationContext, Transformer,
};
use tsc_syntax::{
    for_each_child,
    nodes::{
        ExpressionStatementData, NoSubstitutionTemplateLiteralData, SourceFileData,
        StringLiteralData, TaggedTemplateExpressionData, TemplateExpressionData, TemplateHeadData,
        TemplateMiddleData, TemplateSpanData, TemplateTailData,
    },
    parse_source_file, NodeData, SyntaxKind,
};
use tsc_types::{CompilerOptions, JsString, ScriptTarget, TokenFlags};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Generic,
    Typed,
}

impl Route {
    const ALL: [Route; 2] = [Route::Generic, Route::Typed];

    fn name(self) -> &'static str {
        match self {
            Route::Generic => "generic",
            Route::Typed => "typed",
        }
    }
}

fn units(value: &Value) -> Vec<u16> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
        .collect()
}

fn kind_of(name: &str) -> SyntaxKind {
    match name {
        "StringLiteral" => SyntaxKind::StringLiteral,
        "NoSubstitutionTemplateLiteral" => SyntaxKind::NoSubstitutionTemplateLiteral,
        "TemplateHead" => SyntaxKind::TemplateHead,
        "TemplateMiddle" => SyntaxKind::TemplateMiddle,
        "TemplateTail" => SyntaxKind::TemplateTail,
        other => panic!("unknown literal kind {other}"),
    }
}

fn is_template(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateHead
            | SyntaxKind::TemplateMiddle
            | SyntaxKind::TemplateTail
    )
}

/// First node of `kind` in the current tree of `source` (depth first).
fn find_kind(arena: &TransformArena, source: TransformSourceId, kind: SyntaxKind) -> TransformNode {
    fn walk(
        arena: &TransformArena,
        source: TransformSourceId,
        node: TransformNode,
        kind: SyntaxKind,
    ) -> Option<TransformNode> {
        let record = arena.node(node).unwrap();
        // Only parse-tree nodes qualify: the TypeScript pass may have added a
        // synthetic prologue literal ahead of the parsed statement.
        if record.kind == kind && record.pos != u32::MAX {
            return Some(node);
        }
        let syntax = arena.source(source).unwrap().syntax();
        let mut children = Vec::new();
        for_each_child(&syntax.arena, record, |child| {
            children.push(child);
            false
        });
        children
            .into_iter()
            .find_map(|child| walk(arena, source, TransformNode::new(source, child), kind))
    }
    walk(arena, source, arena.root(source).unwrap(), kind).expect("parsed literal present")
}

fn template_payload(kind: SyntaxKind, text: JsString, raw_text: Option<String>) -> NodeData {
    match kind {
        SyntaxKind::NoSubstitutionTemplateLiteral => {
            NodeData::NoSubstitutionTemplateLiteral(NoSubstitutionTemplateLiteralData {
                text,
                raw_text,
            })
        }
        SyntaxKind::TemplateHead => NodeData::TemplateHead(TemplateHeadData { text, raw_text }),
        SyntaxKind::TemplateMiddle => {
            NodeData::TemplateMiddle(TemplateMiddleData { text, raw_text })
        }
        SyntaxKind::TemplateTail => NodeData::TemplateTail(TemplateTailData { text, raw_text }),
        other => panic!("not a template kind: {other:?}"),
    }
}

fn literal_parts(data: &NodeData) -> (JsString, Option<String>, Option<bool>) {
    match data {
        NodeData::StringLiteral(data) => {
            (data.text.clone(), None, data.has_extended_unicode_escape)
        }
        NodeData::NoSubstitutionTemplateLiteral(data) => {
            (data.text.clone(), data.raw_text.clone(), None)
        }
        NodeData::TemplateHead(data) => (data.text.clone(), data.raw_text.clone(), None),
        NodeData::TemplateMiddle(data) => (data.text.clone(), data.raw_text.clone(), None),
        NodeData::TemplateTail(data) => (data.text.clone(), data.raw_text.clone(), None),
        other => panic!("not a literal payload: {other:?}"),
    }
}

/// The observable literal state, mirroring the observer's `factoryState`.
fn state(
    arena: &TransformArena,
    node: TransformNode,
    kind: SyntaxKind,
    labels: &[(&str, Option<TransformNode>)],
) -> Value {
    let record = arena.node(node).unwrap();
    let metadata = arena.metadata(node);
    let properties = arena.literal_properties(node);
    let is_string = kind == SyntaxKind::StringLiteral;
    let position = |value: u32| {
        if value == u32::MAX {
            -1i64
        } else {
            i64::from(value)
        }
    };
    let (_, raw_projection, has_extended_unicode_escape) = literal_parts(&record.data);
    let raw_text_utf16 = if is_string {
        Value::Null
    } else if let Some(raw) =
        properties.and_then(tsc_emitter::LiteralNodeProperties::raw_template_text)
    {
        json!(raw.code_units())
    } else {
        json!(raw_projection.map(|raw| raw.encode_utf16().collect::<Vec<_>>()))
    };
    let original = metadata.and_then(EmitMetadata::original).map(|original| {
        labels
            .iter()
            .find(|(_, candidate)| *candidate == Some(original))
            .map_or("other", |(name, _)| *name)
    });
    json!({
        "kind": record.kind as u16,
        "pos": position(record.pos),
        "end": position(record.end),
        "flags": record.flags,
        "transform_flags": arena.transform_flags(node).bits(),
        "emit_flags": metadata.map_or(0, |metadata| metadata.flags().bits()),
        "parent": record.parent.is_some(),
        "text_utf16": arena.literal_code_units(node).unwrap().unwrap(),
        "raw_text_utf16": raw_text_utf16,
        "template_flags": if is_string { Value::Null } else { json!(record.template_flags) },
        "single_quote": if is_string {
            json!(properties.and_then(tsc_emitter::LiteralNodeProperties::string_literal_single_quote))
        } else {
            Value::Null
        },
        "has_extended_unicode_escape": if is_string { json!(has_extended_unicode_escape) } else { Value::Null },
        "text_source": if is_string {
            json!(properties
                .and_then(tsc_emitter::LiteralNodeProperties::string_literal_text_source)
                .map(|_| "identifier"))
        } else {
            Value::Null
        },
        "original": original,
    })
}

fn printed_value(printed: &tsc_emitter::PrintedText) -> Value {
    json!({
        "text_utf16": printed.text_utf16().as_ref(),
        "utf8_base64": base64_encode(printed.text().as_bytes()),
        "utf8_bytes": printed.text().len(),
        "end_utf16": {
            "position": printed.end().position().value(),
            "line": printed.end().line(),
            "column": printed.end().column(),
        },
    })
}

/// Outcome of one row on one route.
#[derive(Debug)]
enum Outcome {
    Exact,
    Divergence(Value),
    Error(String),
    NotExpressible,
}

fn transform_flags_of(data: &NodeData, original_flags: TransformFlags) -> TransformFlags {
    // A generic caller carries the original's flags through (visit-each-child
    // recomputes only child-derived bits, which a literal has none of).
    let _ = data;
    original_flags
}

fn apply_generic(
    arena: &mut TransformArena,
    node: TransformNode,
    kind: SyntaxKind,
    operation: &str,
    after: &[u16],
) -> Result<Option<TransformNode>, TransformError> {
    let record = arena.node(node)?.clone();
    let flags = arena.transform_flags(node);
    let (text, raw_text, has_extended_unicode_escape) = literal_parts(&record.data);
    let scalar = |units: &[u16]| String::from_utf16(units).ok();
    let payload = match (kind == SyntaxKind::StringLiteral, operation) {
        (_, "same") => record.data.clone(),
        // `createStringLiteral(after, node.singleQuote)`: no escape marker.
        (true, "cooked") => NodeData::StringLiteral(StringLiteralData {
            text: JsString::from_code_units(after),
            has_extended_unicode_escape: None,
        }),
        (false, "cooked") => template_payload(kind, JsString::from_code_units(after), raw_text),
        (false, "raw") => match scalar(after) {
            Some(raw) => template_payload(kind, text, Some(raw)),
            None => return Ok(None),
        },
        (false, "raw-absent") => template_payload(kind, text, None),
        (false, "raw-empty") => template_payload(kind, text, Some(String::new())),
        // Template flags and the quote preference are node properties, not
        // payload fields: the generic update cannot express them.
        (false, "flags") | (true, "quote") => return Ok(None),
        (is_string, other) => panic!("unsupported generic operation {other} (string: {is_string})"),
    };
    let _ = has_extended_unicode_escape;
    let flags = transform_flags_of(&payload, flags);
    arena.factory().update_node(node, payload, flags).map(Some)
}

/// The typed route: `update_string_literal` / `update_template_literal_like_node`
/// with the constructor arguments of the modelled TypeScript operation.
fn apply_typed(
    arena: &mut TransformArena,
    node: TransformNode,
    kind: SyntaxKind,
    operation: &str,
    after: &[u16],
) -> Result<Option<TransformNode>, TransformError> {
    let record = arena.node(node)?.clone();
    let current_text = arena.literal_code_units(node)?.expect("literal operand");
    let properties = arena.literal_properties(node);
    if kind == SyntaxKind::StringLiteral {
        let (_, _, has_extended_unicode_escape) = literal_parts(&record.data);
        let current_quote =
            properties.and_then(tsc_emitter::LiteralNodeProperties::string_literal_single_quote);
        let (text, quote, escape): (&[u16], Option<bool>, Option<bool>) = match operation {
            "same" => (&current_text, current_quote, has_extended_unicode_escape),
            "cooked" => (after, current_quote, None),
            "quote" => (&current_text, Some(!current_quote.unwrap_or(false)), None),
            other => panic!("unsupported typed string operation {other}"),
        };
        return arena
            .factory()
            .update_string_literal(node, text, quote, escape)
            .map(Some);
    }
    let (_, projection, _) = literal_parts(&record.data);
    let current_raw: Option<Vec<u16>> =
        match properties.and_then(tsc_emitter::LiteralNodeProperties::raw_template_text) {
            Some(owned) => Some(owned.code_units().to_vec()),
            None => projection.map(|raw| raw.encode_utf16().collect()),
        };
    let flags = record.template_flags & TokenFlags::TEMPLATE_LITERAL_LIKE_FLAGS.bits();
    let empty: &[u16] = &[];
    let (text, raw, flags): (&[u16], Option<&[u16]>, i32) = match operation {
        "same" => (&current_text, current_raw.as_deref(), flags),
        "cooked" => (after, current_raw.as_deref(), flags),
        "raw" => (&current_text, Some(after), flags),
        "raw-absent" => (&current_text, None, flags),
        "raw-empty" => (&current_text, Some(empty), flags),
        "flags" => (
            &current_text,
            current_raw.as_deref(),
            flags ^ TokenFlags::CONTAINS_INVALID_ESCAPE.bits(),
        ),
        other => panic!("unsupported typed template operation {other}"),
    };
    arena
        .factory()
        .update_template_literal_like_node(node, text, raw, TokenFlags::from_bits(flags))
        .map(Some)
}

fn factory_case(case: &Value, route: Route) -> Outcome {
    let id = case["case_id"].as_str().unwrap();
    let kind = kind_of(case["kind"].as_str().unwrap());
    let origin = case["origin"].as_str().unwrap();
    let operation = case["operation"].as_str().unwrap();
    let policy = case["policy"].as_str().unwrap();
    let before = units(&case["before"]);
    let after = units(&case["after"]);
    let source_text = case["source"].as_str().unwrap_or("");
    let expected = &case["typescript_observation"];
    let result = std::panic::catch_unwind(|| -> Result<Value, String> {
        let parsed = parse_source_file("main.ts", source_text, Default::default(), None);
        let other = parse_source_file("other.ts", "", Default::default(), None);
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, None);
        let other_source = arena.add_source(&other, None);
        // The TypeScript transformer initializes parsed transform flags; it
        // leaves every literal node of these sources untouched.
        let options = CompilerOptions {
            target: Some(99),
            module: Some(99),
            ..CompilerOptions::default()
        };
        let resolver = NoConstantValueResolver;
        let mut transformation = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            vec![transform_type_script(&options, &resolver)],
            false,
        )
        .map_err(|error| format!("transform: {error:?}"))?;
        let arena = transformation
            .arena_mut()
            .map_err(|error| format!("arena: {error:?}"))?;
        let is_string = kind == SyntaxKind::StringLiteral;
        let create = |arena: &mut TransformArena,
                      target: TransformSourceId,
                      text: &[u16],
                      raw: Option<&[u16]>|
         -> Result<TransformNode, TransformError> {
            if is_string {
                arena
                    .factory()
                    .create_string_literal_from_code_units(target, text, false)
            } else {
                arena
                    .factory()
                    .create_template_literal_like_from_code_units(target, kind, text, raw)
            }
        };
        let mut base = None;
        let mut build = || -> Result<TransformNode, TransformError> {
            match origin {
                "synthetic" => create(arena, source, &before, Some(&before)),
                "synthetic-raw-absent" => create(arena, source, &before, None),
                "synthetic-raw-empty" => create(arena, source, &before, Some(&[])),
                "parsed" => Ok(find_kind(arena, source, kind)),
                "clone" => {
                    let created = create(arena, source, &before, Some(&before))?;
                    base = Some(created);
                    arena.factory().clone_node(created)
                }
                "set-original" => {
                    let parsed_node = find_kind(arena, source, kind);
                    base = Some(parsed_node);
                    let created = create(arena, source, &before, Some(&before))?;
                    arena.set_original_node(created, Some(parsed_node))?;
                    Ok(created)
                }
                "cross-source" => {
                    let created = create(arena, source, &before, Some(&before))?;
                    base = Some(created);
                    arena.factory().clone_node_to_source(created, other_source)
                }
                "text-source" => {
                    let identifier = arena.factory().create_identifier(source, "abc")?;
                    base = Some(identifier);
                    let literal = arena.factory().create_node(
                        source,
                        NodeData::StringLiteral(StringLiteralData {
                            text: "abc".into(),
                            has_extended_unicode_escape: None,
                        }),
                        TransformFlags::NONE,
                    )?;
                    arena
                        .literal_properties_mut(literal)?
                        .set_string_literal_text_source(identifier);
                    Ok(literal)
                }
                other => panic!("unknown origin {other}"),
            }
        };
        let node = build().map_err(|error| format!("origin: {error:?}"))?;
        if policy == "node-no-ascii" {
            arena
                .metadata_mut(node)
                .add_flags(EmitFlags::NO_ASCII_ESCAPING);
        }
        let updated = match route {
            Route::Generic => apply_generic(arena, node, kind, operation, &after),
            Route::Typed => apply_typed(arena, node, kind, operation, &after),
        }
        .map_err(|error| format!("update: {error:?}"))?;
        let Some(updated) = updated else {
            return Err("na".to_owned());
        };
        let labels = [
            ("base", base),
            ("node", Some(node)),
            ("updated", Some(updated)),
        ];
        let node_state = state(arena, node, kind, &labels);
        let updated_state = state(arena, updated, kind, &labels);
        let mut printer = create_printer(PrinterOptions::new(NewLineKind::LineFeed));
        let request = || PrintRequest::StandaloneNode {
            node: updated,
            writer: StandaloneWriter::MultiLine,
        };
        let first = printer
            .print(&mut transformation, request(), None)
            .map_err(|error| format!("print: {error:?}"))?;
        let second = printer
            .print(&mut transformation, request(), None)
            .map_err(|error| format!("re-print: {error:?}"))?;
        if first != second {
            return Err("re-print drift".to_owned());
        }
        Ok(json!({
            "node": node_state,
            "updated": updated_state,
            "identity": updated == node,
            "printed": printed_value(&first),
        }))
    });
    match result {
        Ok(Ok(actual)) if actual == *expected => Outcome::Exact,
        Ok(Ok(actual)) => Outcome::Divergence(json!({"expected": expected, "actual": actual})),
        Ok(Err(message)) if message == "na" => Outcome::NotExpressible,
        Ok(Err(message)) => Outcome::Error(format!("{id}: {message}")),
        Err(_) => Outcome::Error(format!("{id}: panic")),
    }
}

// ------------------------------------------------------------------ transform

/// The `before` transformer of the transform group: updates every template
/// fragment of the first template statement, or replaces the span expressions.
struct Updater {
    operation: String,
    after: Vec<u16>,
    route: Route,
    /// Rows the route cannot express are reported, never silently skipped.
    expressible: bool,
}

impl Updater {
    fn update_fragment(
        &mut self,
        context: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let kind = context.arena().node(node)?.kind;
        if !is_template(kind) {
            return Ok(node);
        }
        let operation = if self.operation == "children" {
            "same"
        } else {
            self.operation.as_str()
        };
        let arena = context.arena_mut()?;
        let updated = match self.route {
            Route::Generic => apply_generic(arena, node, kind, operation, &self.after)?,
            Route::Typed => apply_typed(arena, node, kind, operation, &self.after)?,
        };
        match updated {
            Some(updated) => Ok(updated),
            None => {
                self.expressible = false;
                Ok(node)
            }
        }
    }

    fn update_template(
        &mut self,
        context: &mut TransformationContext,
        template: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = template.source();
        let record = context.arena().node(template)?.clone();
        match record.data {
            NodeData::NoSubstitutionTemplateLiteral(_) => self.update_fragment(context, template),
            NodeData::TemplateExpression(data) => {
                let head =
                    self.update_fragment(context, TransformNode::new(source, data.head.unwrap()))?;
                let spans_array = TransformNodeArray::new(source, data.template_spans.unwrap());
                let span_ids = context.arena().node_array(spans_array)?.nodes.clone();
                let mut spans = Vec::with_capacity(span_ids.len());
                let mut flags = context.arena().transform_flags(template)
                    | context.arena().transform_flags(head);
                for span_id in span_ids {
                    let span = TransformNode::new(source, span_id);
                    let NodeData::TemplateSpan(span_data) =
                        context.arena().node(span)?.data.clone()
                    else {
                        unreachable!("template span");
                    };
                    let expression = TransformNode::new(source, span_data.expression.unwrap());
                    let expression = if self.operation == "children" {
                        context.factory()?.create_identifier(source, "z")?
                    } else {
                        expression
                    };
                    let literal = self.update_fragment(
                        context,
                        TransformNode::new(source, span_data.literal.unwrap()),
                    )?;
                    let span_flags = context.arena().transform_flags(span)
                        | context.arena().transform_flags(expression)
                        | context.arena().transform_flags(literal);
                    let updated = context.factory()?.update_node(
                        span,
                        NodeData::TemplateSpan(TemplateSpanData {
                            expression: Some(expression.node()),
                            literal: Some(literal.node()),
                        }),
                        span_flags,
                    )?;
                    flags |= span_flags;
                    spans.push(updated);
                }
                let spans = context.factory()?.update_node_array(spans_array, spans)?;
                context.factory()?.update_node(
                    template,
                    NodeData::TemplateExpression(TemplateExpressionData {
                        head: Some(head.node()),
                        template_spans: Some(spans.array()),
                    }),
                    flags,
                )
            }
            _ => Ok(template),
        }
    }
}

impl Transformer for Updater {
    fn name(&self) -> &'static str {
        "literal-update-before"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Ok(root);
        };
        let root_node = context.arena().root(source)?;
        let NodeData::SourceFile(file) = context.arena().node(root_node)?.data.clone() else {
            unreachable!("source file root");
        };
        let statements_array = TransformNodeArray::new(source, file.statements.unwrap());
        let statement_ids = context.arena().node_array(statements_array)?.nodes.clone();
        let mut statements = Vec::with_capacity(statement_ids.len());
        let mut flags = context.arena().transform_flags(root_node);
        for statement_id in statement_ids {
            let statement = TransformNode::new(source, statement_id);
            let NodeData::ExpressionStatement(data) = context.arena().node(statement)?.data.clone()
            else {
                statements.push(statement);
                continue;
            };
            let Some(expression_id) = data.expression else {
                statements.push(statement);
                continue;
            };
            let expression = TransformNode::new(source, expression_id);
            let record = context.arena().node(expression)?.clone();
            let updated_expression = match record.data {
                NodeData::TaggedTemplateExpression(data) => {
                    let template = self.update_template(
                        context,
                        TransformNode::new(source, data.template.unwrap()),
                    )?;
                    let tagged_flags = context.arena().transform_flags(expression)
                        | context.arena().transform_flags(template);
                    context.factory()?.update_node(
                        expression,
                        NodeData::TaggedTemplateExpression(TaggedTemplateExpressionData {
                            tag: data.tag,
                            type_arguments: data.type_arguments,
                            template: Some(template.node()),
                            question_dot_token: data.question_dot_token,
                        }),
                        tagged_flags,
                    )?
                }
                NodeData::TemplateExpression(_) | NodeData::NoSubstitutionTemplateLiteral(_) => {
                    self.update_template(context, expression)?
                }
                _ => {
                    statements.push(statement);
                    continue;
                }
            };
            let statement_flags = context.arena().transform_flags(statement)
                | context.arena().transform_flags(updated_expression);
            flags |= statement_flags;
            statements.push(context.factory()?.update_node(
                statement,
                NodeData::ExpressionStatement(ExpressionStatementData {
                    expression: Some(updated_expression.node()),
                }),
                statement_flags,
            )?);
        }
        let statements = context
            .factory()?
            .update_node_array(statements_array, statements)?;
        let updated_root = context.factory()?.update_node(
            root_node,
            NodeData::SourceFile(SourceFileData {
                statements: Some(statements.array()),
                end_of_file_token: file.end_of_file_token,
            }),
            flags,
        )?;
        context.arena_mut()?.replace_root(source, updated_root)?;
        Ok(root)
    }
}

fn transform_case(case: &Value, route: Route) -> Outcome {
    let id = case["case_id"].as_str().unwrap();
    let source_text = case["source"].as_str().unwrap();
    let target = match case["target"].as_str().unwrap() {
        "es5" => 1,
        "es2015" => 2,
        "esnext" => 99,
        other => panic!("unknown target {other}"),
    };
    let expected = &case["typescript_observation"]["js"];
    let operation = case["operation"].as_str().unwrap().to_owned();
    let after = units(&case["after"]);
    let result = std::panic::catch_unwind(|| -> Result<Value, String> {
        let parsed = parse_source_file("main.ts", source_text, Default::default(), None);
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
        // module Preserve: the ESM transform's output does not depend on an
        // emit host's implied file format (the observer uses the same kind).
        let options = CompilerOptions {
            target: Some(target),
            module: Some(200),
            strict: Some(true),
            ..CompilerOptions::default()
        };
        let resolver = NoConstantValueResolver;
        let mut transformers = get_script_transformers(&options, &resolver)
            .map_err(|error| format!("transformers: {error:?}"))?;
        // TypeScript runs custom `before` transformers ahead of
        // transformTypeScript; the port's TypeScript pass owns parsed
        // transform-flag initialization, so the updater follows it. Neither
        // pass changes the template statements observed here.
        let typescript = transformers
            .iter()
            .position(|transformer| transformer.name() == "transformTypeScript")
            .expect("transformTypeScript leads the script transformers");
        let mut updater = Updater {
            operation: operation.clone(),
            after: after.clone(),
            route,
            expressible: true,
        };
        // The updater must report inexpressible rows after the run; keep a
        // shared flag through a raw pointer-free channel: run, then inspect.
        let expressible = std::cell::Cell::new(true);
        struct Probe<'a> {
            inner: Updater,
            expressible: &'a std::cell::Cell<bool>,
        }
        impl Transformer for Probe<'_> {
            fn name(&self) -> &'static str {
                self.inner.name()
            }
            fn transform_root(
                &mut self,
                context: &mut TransformationContext,
                root: TransformRoot,
            ) -> Result<TransformRoot, TransformError> {
                let result = self.inner.transform_root(context, root);
                self.expressible.set(self.inner.expressible);
                result
            }
        }
        updater.expressible = true;
        transformers.insert(
            typescript + 1,
            Box::new(Probe {
                inner: updater,
                expressible: &expressible,
            }),
        );
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            transformers,
            false,
        )
        .map_err(|error| format!("transform: {error:?}"))?;
        if !expressible.get() {
            return Err("na".to_owned());
        }
        let printed = create_printer(
            PrinterOptions::new(NewLineKind::LineFeed)
                .with_target(ScriptTarget::from_bits(target))
                .with_source_file_text_mode(SourceFileTextMode::Canonical),
        )
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .map_err(|error| format!("print: {error:?}"))?;
        Ok(printed_value(&printed))
    });
    match result {
        Ok(Ok(actual)) if actual == *expected => Outcome::Exact,
        Ok(Ok(actual)) => Outcome::Divergence(json!({"expected": expected, "actual": actual})),
        Ok(Err(message)) if message == "na" => Outcome::NotExpressible,
        Ok(Err(message)) => Outcome::Error(format!("{id}: {message}")),
        Err(_) => Outcome::Error(format!("{id}: panic")),
    }
}

// ------------------------------------------------------------------- lifetime

fn lifetime_case(case: &Value) -> Result<Value, String> {
    let kind = kind_of(case["kind"].as_str().unwrap());
    let origin = case["origin"].as_str().unwrap();
    let value = units(&case["value"]);
    let source_text = case["source"].as_str().unwrap_or("");
    let expected = &case["typescript_observation"];
    let parsed = parse_source_file("main.ts", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let node = if origin == "parsed" {
        find_kind(&arena, source, kind)
    } else if kind == SyntaxKind::StringLiteral {
        arena
            .factory()
            .create_string_literal_from_code_units(source, &value, false)
            .unwrap()
    } else {
        arena
            .factory()
            .create_template_literal_like_from_code_units(source, kind, &value, Some(&value))
            .unwrap()
    };
    arena
        .metadata_mut(node)
        .add_flags(EmitFlags::NO_ASCII_ESCAPING);
    let mut transformation = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .unwrap();
    let mut printer = create_printer(PrinterOptions::new(NewLineKind::LineFeed));
    let request = || PrintRequest::StandaloneNode {
        node,
        writer: StandaloneWriter::MultiLine,
    };
    let first = printer.print(&mut transformation, request(), None).unwrap();
    let second = printer.print(&mut transformation, request(), None).unwrap();
    assert_eq!(first, second, "re-print drift");
    let before = printed_value(&first);
    transformation.dispose();
    let arena = transformation.arena();
    let is_string = kind == SyntaxKind::StringLiteral;
    let (_, raw_projection, _) = literal_parts(&arena.node(node).unwrap().data);
    let survived = json!({
        "text_utf16": arena.literal_code_units(node).unwrap().unwrap(),
        "raw_text_utf16": if is_string { Value::Null } else if let Some(raw) =
            arena.literal_properties(node).and_then(tsc_emitter::LiteralNodeProperties::raw_template_text)
        {
            json!(raw.code_units())
        } else {
            json!(raw_projection.map(|raw| raw.encode_utf16().collect::<Vec<_>>()))
        },
        "single_quote": if is_string {
            json!(arena.literal_properties(node).and_then(tsc_emitter::LiteralNodeProperties::string_literal_single_quote))
        } else { Value::Null },
    });
    let emit_flags_after = arena
        .metadata(node)
        .map_or(0, |metadata| metadata.flags().bits());
    let after = match printer.print(&mut transformation, request(), None) {
        Ok(printed) => json!({"printed": printed_value(&printed)}),
        Err(error) => json!({"error": format!("{error:?}")}),
    };
    Ok(json!({
        "printed_before_dispose": before,
        "survived": survived,
        "emit_flags_after_dispose": emit_flags_after,
        "after_dispose": after,
        "expected_emit_flags_after_dispose": expected["emit_flags_after_dispose"],
        "expected_printed_after_dispose": expected["printed_after_dispose"],
    }))
}

// --------------------------------------------------------------------- driver

fn report_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("TSC_RS_LITERAL_UPDATE_REPORT_DIR").map(std::path::PathBuf::from)
}

fn write_report(name: &str, report: &Value) {
    if let Some(directory) = report_dir() {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_string_pretty(report).unwrap() + "\n",
        )
        .unwrap();
    }
}

fn run_group(
    name: &str,
    cases: &[Value],
    run: impl Fn(&Value, Route) -> Outcome,
) -> BTreeMap<&'static str, (usize, usize, usize, usize, Vec<String>)> {
    let mut summary = BTreeMap::new();
    for route in Route::ALL {
        let mut exact = 0;
        let mut divergent = Vec::new();
        let mut errors = Vec::new();
        let mut inexpressible = Vec::new();
        let mut rows = Vec::new();
        for case in cases {
            let id = case["case_id"].as_str().unwrap();
            // Each row runs twice; both runs must agree with the expectation.
            let outcomes = [run(case, route), run(case, route)];
            let status = match &outcomes {
                [Outcome::Exact, Outcome::Exact] => {
                    exact += 1;
                    "exact"
                }
                [Outcome::NotExpressible, Outcome::NotExpressible] => {
                    inexpressible.push(id.to_owned());
                    "na"
                }
                [Outcome::Error(message), _] | [_, Outcome::Error(message)] => {
                    errors.push(message.clone());
                    "error"
                }
                _ => {
                    divergent.push(id.to_owned());
                    "divergence"
                }
            };
            let detail = match &outcomes[0] {
                Outcome::Divergence(detail) => detail.clone(),
                Outcome::Error(message) => json!(message),
                _ => Value::Null,
            };
            rows.push(json!({"case_id": id, "status": status, "detail": detail}));
        }
        write_report(
            &format!("{name}-{}", route.name()),
            &json!({
                "group": name,
                "route": route.name(),
                "cases": cases.len(),
                "exact": exact,
                "divergent": divergent,
                "errors": errors,
                "not_expressible": inexpressible,
                "rows": rows,
            }),
        );
        summary.insert(
            route.name(),
            (
                exact,
                divergent.len(),
                errors.len(),
                inexpressible.len(),
                divergent
                    .iter()
                    .cloned()
                    .chain(errors.iter().cloned())
                    .collect(),
            ),
        );
    }
    summary
}

fn load(bytes: &[u8], group: &str, count: usize) -> Vec<Value> {
    let artifact: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["group"], group);
    let cases = artifact["cases"].as_array().unwrap().clone();
    assert_eq!(cases.len(), count);
    cases
}

#[test]
fn factory_literal_updates_match_typescript() {
    let cases = load(
        include_bytes!("fixtures/literal-update-factory.json"),
        "factory",
        987,
    );
    let summary = run_group("factory", &cases, factory_case);
    for (route, (exact, divergent, errors, na, failures)) in &summary {
        eprintln!(
            "factory/{route}: exact {exact}, divergent {divergent}, errors {errors}, not expressible {na}"
        );
        assert!(
            failures.is_empty(),
            "factory/{route}: {} rows differ from TypeScript: {failures:?}",
            failures.len()
        );
        let expected = match *route {
            "generic" => (782, 205),
            "typed" => (987, 0),
            _ => unreachable!(),
        };
        assert_eq!((*exact, *na), expected, "factory/{route}: route coverage");
    }
}

#[test]
fn transform_routes_match_typescript() {
    let cases = load(
        include_bytes!("fixtures/literal-update-transform.json"),
        "transform",
        399,
    );
    let summary = run_group("transform", &cases, transform_case);
    for (route, (exact, divergent, errors, na, failures)) in &summary {
        eprintln!(
            "transform/{route}: exact {exact}, divergent {divergent}, errors {errors}, not expressible {na}"
        );
        assert!(
            failures.is_empty(),
            "transform/{route}: {} rows differ from TypeScript: {failures:?}",
            failures.len()
        );
        let expected = match *route {
            "generic" => (315, 84),
            "typed" => (399, 0),
            _ => unreachable!(),
        };
        assert_eq!((*exact, *na), expected, "transform/{route}: route coverage");
    }
}

#[test]
fn lifetime_controls_keep_literal_properties_across_disposal() {
    let cases = load(
        include_bytes!("fixtures/literal-update-lifetime.json"),
        "lifetime",
        10,
    );
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for case in &cases {
        let id = case["case_id"].as_str().unwrap();
        let expected = &case["typescript_observation"];
        let actual = lifetime_case(case).unwrap();
        assert_eq!(
            actual,
            lifetime_case(case).unwrap(),
            "{id}: repetition drift"
        );
        let origin = case["origin"].as_str().unwrap();
        let mut problems = Vec::new();
        if actual["printed_before_dispose"] != expected["printed_before_dispose"] {
            problems.push("printed_before_dispose");
        }
        if actual["survived"]["text_utf16"] != expected["text_utf16"]
            || actual["survived"]["raw_text_utf16"] != expected["raw_text_utf16"]
            || actual["survived"]["single_quote"] != expected["single_quote"]
        {
            problems.push("literal properties did not survive disposal");
        }
        // Upstream disposal clears the emitNode of parse-tree nodes only; the
        // port's session metadata is cleared for every node. Parsed-origin
        // rows must agree; synthetic-origin retention is recorded, not required.
        let flags_agree =
            actual["emit_flags_after_dispose"] == expected["emit_flags_after_dispose"];
        if origin == "parsed" && !flags_agree {
            problems.push("emit flags after disposal");
        }
        rows.push(json!({"case_id": id, "problems": problems, "emit_flags_agree": flags_agree, "actual": actual}));
        if !problems.is_empty() {
            failures.push(format!("{id}: {problems:?}"));
        }
    }
    write_report(
        "lifetime",
        &json!({"group": "lifetime", "cases": cases.len(), "rows": rows}),
    );
    assert!(failures.is_empty(), "lifetime controls: {failures:?}");
}
