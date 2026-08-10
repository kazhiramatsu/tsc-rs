use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use tsc_syntax::{for_each_child, scan_tokens, skip_trivia, NodeData, NodeId, SyntaxKind};
use tsc_types::{NodeFlags, ScriptTarget, TokenFlags};

use crate::{
    create_text_writer, EmitFlags, EmitHelper, EmitHint, GeneratedUtf16Location, NewLineKind,
    SourceBytePosition, SourceByteRange, SourceMapRange, SourcePositionError, SourceRange,
    SourceUtf16Location, SyntheticComment, SyntheticCommentKind, TextWriter, TransformBundle,
    TransformError, TransformNode, TransformNodeArray, TransformSourceId, TransformationResult,
    UnsupportedEmitFeature,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModifierListItemKind {
    Decorator,
    Modifier,
}

impl ModifierListItemKind {
    const fn from_syntax_kind(kind: SyntaxKind) -> Self {
        if matches!(kind, SyntaxKind::Decorator) {
            Self::Decorator
        } else {
            Self::Modifier
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ModifierListItem {
    node: NodeId,
    kind: ModifierListItemKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParenthesizedNoAsiExpression {
    inner: NodeId,
    comment_anchor: TransformNode,
}

/// Source-leading comments separated from the first statement by a blank
/// line are owned by the source-file boundary, not by whichever transformed
/// statement eventually retains that source range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DetachedSourcePrefix {
    anchor: TransformNode,
    byte_len: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ExpressionEmissionContext {
    #[default]
    Normal,
    NoAsi,
}

/// Selects whether an unchanged source-file root is a text-preserving printer
/// request or a compiler JavaScript emit. The standalone printer keeps H1's
/// exact identity contract by default; compiler emit always walks the AST,
/// matching tsc's `emitSourceFileWorker` even when no transformer cloned the
/// root.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SourceFileTextMode {
    #[default]
    PreserveUnchanged,
    Canonical,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrinterOptions {
    new_line: NewLineKind,
    remove_comments: bool,
    no_implicit_use_strict: bool,
    no_emit_helpers: bool,
    target: Option<ScriptTarget>,
    source_file_text_mode: SourceFileTextMode,
}

impl PrinterOptions {
    pub const fn new(new_line: NewLineKind) -> Self {
        Self {
            new_line,
            remove_comments: false,
            no_implicit_use_strict: false,
            no_emit_helpers: false,
            target: None,
            source_file_text_mode: SourceFileTextMode::PreserveUnchanged,
        }
    }

    pub const fn with_remove_comments(mut self, value: bool) -> Self {
        self.remove_comments = value;
        self
    }

    pub const fn with_no_implicit_use_strict(mut self, value: bool) -> Self {
        self.no_implicit_use_strict = value;
        self
    }

    pub const fn with_no_emit_helpers(mut self, value: bool) -> Self {
        self.no_emit_helpers = value;
        self
    }

    pub const fn with_target(mut self, target: ScriptTarget) -> Self {
        self.target = Some(target);
        self
    }

    pub const fn with_source_file_text_mode(mut self, mode: SourceFileTextMode) -> Self {
        self.source_file_text_mode = mode;
        self
    }

    pub const fn new_line(self) -> NewLineKind {
        self.new_line
    }

    pub const fn remove_comments(self) -> bool {
        self.remove_comments
    }

    pub const fn no_implicit_use_strict(self) -> bool {
        self.no_implicit_use_strict
    }

    pub const fn no_emit_helpers(self) -> bool {
        self.no_emit_helpers
    }

    pub const fn target(self) -> Option<ScriptTarget> {
        self.target
    }

    pub const fn source_file_text_mode(self) -> SourceFileTextMode {
        self.source_file_text_mode
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrintRequest {
    SourceFile(TransformSourceId),
    StandaloneNode(TransformNode),
    NodeList(TransformNodeArray),
    Bundle(TransformBundle),
    JavaScriptMap(TransformSourceId),
    Declaration(TransformSourceId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrintedText {
    text: String,
    end: GeneratedUtf16Location,
}

impl PrintedText {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub const fn end(&self) -> GeneratedUtf16Location {
        self.end
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SourceMapHookPhase {
    BeforeNode,
    AfterNode,
    BeforeToken,
    AfterToken,
}

/// Test-observable source-map hook input. It records typed source-byte and
/// generated-UTF-16 domains but performs no VLQ encoding or map serialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceMapHookEvent {
    phase: SourceMapHookPhase,
    node: TransformNode,
    token: Option<SyntaxKind>,
    source: TransformSourceId,
    source_position: SourceBytePosition,
    generated: GeneratedUtf16Location,
}

impl SourceMapHookEvent {
    pub const fn phase(self) -> SourceMapHookPhase {
        self.phase
    }

    pub const fn node(self) -> TransformNode {
        self.node
    }

    pub const fn token(self) -> Option<SyntaxKind> {
        self.token
    }

    pub const fn source(self) -> TransformSourceId {
        self.source
    }

    pub const fn source_position(self) -> SourceBytePosition {
        self.source_position
    }

    pub const fn generated(self) -> GeneratedUtf16Location {
        self.generated
    }
}

/// Dormant map-recorder seam. Production H1 supplies the disabled
/// implementation; focused tests may capture the exact hook phases.
pub trait SourceMapRecorder {
    fn enabled(&self) -> bool;
    fn record(&mut self, event: SourceMapHookEvent);
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisabledSourceMapRecorder;

impl SourceMapRecorder for DisabledSourceMapRecorder {
    fn enabled(&self) -> bool {
        false
    }

    fn record(&mut self, _event: SourceMapHookEvent) {}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Printer {
    options: PrinterOptions,
    literal_emission_plan: LiteralEmissionPlan,
}

/// Nodes on a path to a literal whose original token text cannot be reused.
///
/// This is built once per print request. Keeping the decision in the printer
/// mirrors tsc's `canUseOriginalText` boundary without forcing target-specific
/// spelling changes into an ECMAScript tree transformer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LiteralEmissionPlan {
    structured_nodes: BTreeSet<TransformNode>,
}

/// tsc-port: createPrinter @6.0.3
/// tsc-hash: b227b66a85178f81faf58d6de65ed31fe2a87de1448ec6ec61e535fd36194697
/// tsc-span: _tsc.js:116912-121378
///
/// H1.2 implements the pipeline foundation and whole-source identity arm.
pub fn create_printer(options: PrinterOptions) -> Printer {
    Printer {
        options,
        literal_emission_plan: LiteralEmissionPlan::default(),
    }
}

impl Printer {
    pub const fn options(&self) -> PrinterOptions {
        self.options
    }

    fn prepare_literal_emission_plan(
        &mut self,
        transformation: &TransformationResult<'_>,
        root: TransformNode,
    ) -> Result<(), PrinterError> {
        let mut structured_nodes = BTreeSet::new();
        let mut memo = BTreeMap::new();
        Self::collect_literal_emission_path(
            transformation,
            root,
            self.options.target,
            &mut memo,
            &mut structured_nodes,
        )?;
        self.literal_emission_plan = LiteralEmissionPlan { structured_nodes };
        Ok(())
    }

    fn collect_literal_emission_path(
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        target: Option<ScriptTarget>,
        memo: &mut BTreeMap<TransformNode, bool>,
        structured_nodes: &mut BTreeSet<TransformNode>,
    ) -> Result<bool, PrinterError> {
        if let Some(requires_structured_emit) = memo.get(&node) {
            return Ok(*requires_structured_emit);
        }

        let record = transformation.arena().node(node)?;
        let requires_literal_rewrite = match &record.data {
            NodeData::NumericLiteral(_) => {
                let flags = TokenFlags::from_bits(record.numeric_literal_flags);
                flags.intersects(TokenFlags::IS_INVALID)
                    || flags.contains(TokenFlags::CONTAINS_SEPARATOR)
                        && target.is_none_or(|target| target < ScriptTarget::ES2021)
            }
            // tsc's canUseOriginalText always rejects BigIntLiteral nodes.
            NodeData::BigIntLiteral(_) => true,
            _ => false,
        };
        let mut children = Vec::new();
        let source = transformation.arena().source(node.source())?.syntax();
        for_each_child(&source.arena, record, |child| {
            children.push(child);
            false
        });

        let mut requires_structured_emit = requires_literal_rewrite;
        for child in children {
            let child = transformation
                .arena()
                .node_ref(node.source(), child)
                .ok_or(PrinterError::UnknownStatement(child.0))?;
            requires_structured_emit |= Self::collect_literal_emission_path(
                transformation,
                child,
                target,
                memo,
                structured_nodes,
            )?;
        }
        if requires_structured_emit {
            structured_nodes.insert(node);
        }
        memo.insert(node, requires_structured_emit);
        Ok(requires_structured_emit)
    }

    /// The generic H1 printer surface. H1.2 established the exact whole-source
    /// identity arm; H1.3 adds the bounded changed-node JavaScript workers
    /// while the remaining request/product axes stay typed controls.
    pub fn print(
        &mut self,
        transformation: &mut TransformationResult<'_>,
        request: PrintRequest,
        recorder: &mut dyn SourceMapRecorder,
    ) -> Result<PrintedText, PrinterError> {
        match request {
            PrintRequest::SourceFile(source) => {
                if self.options.remove_comments {
                    return Err(PrinterError::OptionUnavailable("removeComments"));
                }
                self.print_source_file(transformation, source, recorder)
            }
            PrintRequest::StandaloneNode(_) => Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::StandaloneNodePrinting,
            )),
            PrintRequest::NodeList(_) => Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::NodeListPrinting,
            )),
            PrintRequest::Bundle(_) => Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::BundleRoot,
            )),
            PrintRequest::JavaScriptMap(_) => Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::JavaScriptMap,
            )),
            PrintRequest::Declaration(_) => Err(PrinterError::Unsupported(
                UnsupportedEmitFeature::Declaration,
            )),
        }
    }

    fn print_source_file(
        &mut self,
        transformation: &mut TransformationResult<'_>,
        source_id: TransformSourceId,
        recorder: &mut dyn SourceMapRecorder,
    ) -> Result<PrintedText, PrinterError> {
        if !transformation.roots().iter().any(
            |root| matches!(root, crate::TransformRoot::SourceFile(source) if *source == source_id),
        ) {
            return Err(PrinterError::SourceIsNotATransformedRoot(source_id));
        }

        let root = transformation.arena().root(source_id)?;
        self.prepare_literal_emission_plan(transformation, root)?;
        if transformation
            .arena()
            .source(source_id)?
            .syntax()
            .file_name
            .to_ascii_lowercase()
            .ends_with(".json")
        {
            return self.print_json_source_file(transformation, source_id, root, recorder);
        }
        let (text, language_variant, statement_array, statements, token_spans) = {
            let source = transformation.arena().source(source_id)?.syntax();
            let root_record = source.arena.node(root.node());
            let statement_array = match &root_record.data {
                NodeData::SourceFile(data) => data.statements,
                _ => return Err(PrinterError::RootIsNotSourceFile(root)),
            };
            let statements = statement_array
                .map(|array| source.arena.node_array(array).nodes.clone())
                .unwrap_or_default();
            let token_spans = scan_tokens(source.text(), source.language_variant)
                .into_iter()
                .map(|token| {
                    let start = source.positions().utf16_to_byte(token.start).ok_or(
                        PrinterError::TokenPositionNotScalarBoundary {
                            position: token.start,
                        },
                    )?;
                    let end = source.positions().utf16_to_byte(token.end).ok_or(
                        PrinterError::TokenPositionNotScalarBoundary {
                            position: token.end,
                        },
                    )?;
                    Ok(TokenSpan {
                        kind: token.kind,
                        start,
                        end,
                    })
                })
                .collect::<Result<Vec<_>, PrinterError>>()?;
            (
                source.text().to_owned(),
                source.language_variant,
                statement_array.map(|array| TransformNodeArray::new(source_id, array)),
                statements,
                token_spans,
            )
        };
        let _ = language_variant;

        if transformation
            .arena()
            .metadata(root)
            .and_then(crate::EmitMetadata::original)
            .is_some()
            || self.literal_emission_plan.structured_nodes.contains(&root)
            || self.options.source_file_text_mode == SourceFileTextMode::Canonical
        {
            return self.print_transformed_source_file(
                transformation,
                source_id,
                root,
                statement_array,
                statements,
                recorder,
            );
        }

        transformation.before_emit_node(EmitHint::SourceFile, root)?;
        let substituted_root = transformation.substitute_node(EmitHint::SourceFile, root)?;
        if substituted_root != root {
            transformation.after_emit_node(EmitHint::SourceFile, root)?;
            return Err(PrinterError::TransformedNodeWorkerUnavailable(
                substituted_root,
            ));
        }

        let mut writer = create_text_writer(self.options.new_line);
        let mut cursor = 0u32;
        for raw_statement in statements {
            let statement = transformation
                .arena()
                .node_ref(source_id, raw_statement)
                .ok_or(PrinterError::UnknownStatement(raw_statement.0))?;
            transformation.before_emit_node(EmitHint::Unspecified, statement)?;
            let emitted = transformation.substitute_node(EmitHint::Unspecified, statement)?;
            if emitted != statement {
                transformation.after_emit_node(EmitHint::Unspecified, statement)?;
                transformation.after_emit_node(EmitHint::SourceFile, root)?;
                return Err(PrinterError::TransformedNodeWorkerUnavailable(emitted));
            }

            let range = self.node_range(transformation, statement)?;
            let start = range.start().value();
            let end = range.end().value();
            if start < cursor {
                return Err(PrinterError::OverlappingSourceRange {
                    previous_end: cursor,
                    start,
                });
            }
            raw_write_range(&mut writer, &text, cursor, start)?;
            self.record_node_hook(
                transformation,
                recorder,
                SourceMapHookPhase::BeforeNode,
                statement,
                &writer,
            )?;
            self.write_original_node(
                transformation,
                statement,
                OriginalNodeText {
                    range,
                    text: &text,
                    tokens: &token_spans,
                },
                &mut writer,
                recorder,
            )?;
            self.record_node_hook(
                transformation,
                recorder,
                SourceMapHookPhase::AfterNode,
                statement,
                &writer,
            )?;
            transformation.after_emit_node(EmitHint::Unspecified, statement)?;
            cursor = end;
        }
        raw_write_range(
            &mut writer,
            &text,
            cursor,
            u32::try_from(text.len()).expect("source text exceeds u32"),
        )?;
        transformation.after_emit_node(EmitHint::SourceFile, root)?;
        Ok(PrintedText {
            text: writer.text().to_owned(),
            end: writer.location(),
        })
    }

    /// tsc-port: emitExpressionStatement @6.0.3
    /// tsc-hash: 2735e6c85cd4ac9311765eef71de22d5fd8e247cfc4f67ed4e04cc688fe3f2a2
    /// tsc-span: _tsc.js:118623-118628
    ///
    /// JSON SourceFiles deliberately bypass the whole-source identity arm:
    /// TypeScript prints their single value as an expression, omits the
    /// statement semicolon, normalizes whitespace/newlines, and lets the
    /// write callback own BOM materialization.
    fn print_json_source_file(
        &self,
        transformation: &mut TransformationResult<'_>,
        source_id: TransformSourceId,
        root: TransformNode,
        _recorder: &mut dyn SourceMapRecorder,
    ) -> Result<PrintedText, PrinterError> {
        transformation.before_emit_node(EmitHint::SourceFile, root)?;
        let emitted_root = transformation.substitute_node(EmitHint::SourceFile, root)?;
        if emitted_root != root {
            transformation.after_emit_node(EmitHint::SourceFile, root)?;
            return Err(PrinterError::TransformedNodeWorkerUnavailable(emitted_root));
        }

        let statements = match &transformation.arena().node(root)?.data {
            NodeData::SourceFile(data) => data
                .statements
                .and_then(|array| transformation.arena().node_array_ref(source_id, array))
                .map(|array| transformation.arena().node_array(array))
                .transpose()?
                .map(|array| array.nodes.clone())
                .unwrap_or_default(),
            _ => return Err(PrinterError::RootIsNotSourceFile(root)),
        };
        let mut writer = create_text_writer(self.options.new_line);
        if let Some(statement_id) = statements.first().copied() {
            if statements.len() != 1 {
                transformation.after_emit_node(EmitHint::SourceFile, root)?;
                return Err(PrinterError::UnsupportedTransformedSyntax {
                    node: root,
                    kind: SyntaxKind::SourceFile,
                });
            }
            let statement = transformation
                .arena()
                .node_ref(source_id, statement_id)
                .ok_or(PrinterError::UnknownStatement(statement_id.0))?;
            let expression = match &transformation.arena().node(statement)?.data {
                NodeData::ExpressionStatement(data) => {
                    data.expression
                        .ok_or(PrinterError::MissingTransformedChild {
                            parent: SyntaxKind::ExpressionStatement,
                            field: "expression",
                        })?
                }
                _ => {
                    let kind = transformation.arena().node(statement)?.kind;
                    transformation.after_emit_node(EmitHint::SourceFile, root)?;
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node: statement,
                        kind,
                    });
                }
            };
            transformation.before_emit_node(EmitHint::Unspecified, statement)?;
            let emitted_statement =
                transformation.substitute_node(EmitHint::Unspecified, statement)?;
            if emitted_statement != statement {
                transformation.after_emit_node(EmitHint::Unspecified, statement)?;
                transformation.after_emit_node(EmitHint::SourceFile, root)?;
                return Err(PrinterError::TransformedNodeWorkerUnavailable(
                    emitted_statement,
                ));
            }
            self.emit_leading_comments_for_node(transformation, statement, &mut writer)?;
            self.emit_node_id(transformation, source_id, expression, &mut writer)?;
            self.emit_trailing_comments_for_node(transformation, statement, &mut writer)?;
            transformation.after_emit_node(EmitHint::Unspecified, statement)?;
            writer.write_line(false);
        }
        transformation.after_emit_node(EmitHint::SourceFile, root)?;
        Ok(PrintedText {
            text: writer.text().to_owned(),
            end: writer.location(),
        })
    }

    fn print_transformed_source_file(
        &self,
        transformation: &mut TransformationResult<'_>,
        source_id: TransformSourceId,
        root: TransformNode,
        statement_array: Option<TransformNodeArray>,
        statements: Vec<tsc_syntax::NodeId>,
        recorder: &mut dyn SourceMapRecorder,
    ) -> Result<PrintedText, PrinterError> {
        transformation.before_emit_node(EmitHint::SourceFile, root)?;
        let emitted_root = transformation.substitute_node(EmitHint::SourceFile, root)?;
        if emitted_root != root {
            transformation.after_emit_node(EmitHint::SourceFile, root)?;
            return Err(PrinterError::TransformedNodeWorkerUnavailable(emitted_root));
        }

        let (original_source_was_statementless, original_first_statement) = {
            let original_root = transformation.arena().get_original_node(root);
            match &transformation.arena().node(original_root)?.data {
                NodeData::SourceFile(data) => {
                    let statements = data.statements.and_then(|array| {
                        transformation
                            .arena()
                            .node_array_ref(original_root.source(), array)
                    });
                    let statements = statements
                        .map(|array| transformation.arena().node_array(array))
                        .transpose()?;
                    let first = statements
                        .and_then(|array| array.nodes.first().copied())
                        .and_then(|id| transformation.arena().node_ref(original_root.source(), id));
                    (statements.is_none_or(|array| array.nodes.is_empty()), first)
                }
                _ => (false, None),
            }
        };
        let mut writer = create_text_writer(self.options.new_line);
        let source_text = transformation.arena().source(source_id)?.syntax().text();
        if let Some(shebang) = source_shebang(source_text) {
            writer.write_comment(shebang);
            writer.write_line(false);
        }
        let helpers = if self.options.no_emit_helpers {
            Vec::new()
        } else {
            let mut helpers = transformation.emit_helpers().to_vec();
            helpers.sort_by_key(|helper| helper.priority());
            helpers
        };
        let system_scoped_helpers = !helpers.is_empty()
            && statements.first().is_some_and(|statement| {
                transformation
                    .arena()
                    .node_ref(source_id, *statement)
                    .is_some_and(|statement| {
                        self.is_system_register_statement(transformation, statement)
                    })
            });
        let source_helpers = if system_scoped_helpers {
            &[][..]
        } else {
            helpers.as_slice()
        };
        let helper_offset = statements
            .iter()
            .take_while(|statement| {
                transformation
                    .arena()
                    .node_ref(source_id, **statement)
                    .is_some_and(|statement| self.is_prologue_statement(transformation, statement))
            })
            .count();
        let detached_source_prefix = original_first_statement
            .map(|first| self.detached_source_prefix(transformation, first))
            .transpose()?
            .flatten();
        let source_owned_detached_prefix = self
            .source_file_owns_detached_prefix(
                transformation,
                statement_array,
                statements.first().copied(),
            )?
            .then_some(detached_source_prefix)
            .flatten();
        let mut emitted_detached_source_prefix = false;
        let mut accounted_for_original_prefix = source_owned_detached_prefix.is_some();
        let mut last_original_statement = None;
        if statements.is_empty() {
            self.emit_detached_source_prefix(
                transformation,
                source_owned_detached_prefix,
                &mut writer,
            )?;
            emitted_detached_source_prefix = source_owned_detached_prefix.is_some();
            self.emit_helpers(source_helpers, &mut writer)?;
        }
        for (statement_index, raw_statement) in statements.into_iter().enumerate() {
            if statement_index == helper_offset {
                self.emit_detached_source_prefix(
                    transformation,
                    source_owned_detached_prefix,
                    &mut writer,
                )?;
                emitted_detached_source_prefix = source_owned_detached_prefix.is_some();
                self.emit_helpers(source_helpers, &mut writer)?;
            }
            let statement = transformation
                .arena()
                .node_ref(source_id, raw_statement)
                .ok_or(PrinterError::UnknownStatement(raw_statement.0))?;
            transformation.before_emit_node(EmitHint::Unspecified, statement)?;
            let emitted = transformation.substitute_node(EmitHint::Unspecified, statement)?;
            let original = transformation.arena().get_original_node(emitted);
            let original_source = transformation.arena().source(original.source())?.syntax();
            let original_record = transformation.arena().node(original)?;
            let had_previous_original_statement = last_original_statement.is_some();
            let emitted_has_original_range = matches!(
                SourceRange::from_raw(
                    original_record.pos,
                    original_record.end,
                    original_source.positions(),
                )?,
                SourceRange::Original(_)
            );
            if emitted_has_original_range {
                last_original_statement = Some(original);
            }
            if !accounted_for_original_prefix && emitted_has_original_range {
                if original_first_statement.is_some_and(|first| first != original) {
                    self.emit_detached_source_prefix(
                        transformation,
                        detached_source_prefix,
                        &mut writer,
                    )?;
                }
                accounted_for_original_prefix = true;
            }
            let detached_prefix_len = source_owned_detached_prefix
                .filter(|prefix| emitted_detached_source_prefix && prefix.anchor == original)
                .map_or(0, |prefix| prefix.byte_len);
            if detached_prefix_len != 0 {
                self.emit_leading_comments_for_node_worker(
                    transformation,
                    emitted,
                    had_previous_original_statement && emitted_has_original_range,
                    detached_prefix_len,
                    &mut writer,
                )?;
            } else if had_previous_original_statement && emitted_has_original_range {
                self.emit_leading_comments_for_node_after_sibling(
                    transformation,
                    emitted,
                    &mut writer,
                )?;
            } else {
                self.emit_leading_comments_for_node(transformation, emitted, &mut writer)?;
            }
            self.record_node_hook(
                transformation,
                recorder,
                SourceMapHookPhase::BeforeNode,
                emitted,
                &writer,
            )?;
            self.emit_transformed_node(
                transformation,
                emitted,
                ExpressionEmissionContext::Normal,
                &mut writer,
            )?;
            self.emit_trailing_comments_for_node(transformation, emitted, &mut writer)?;
            self.record_node_hook(
                transformation,
                recorder,
                SourceMapHookPhase::AfterNode,
                emitted,
                &writer,
            )?;
            transformation.after_emit_node(EmitHint::Unspecified, statement)?;
            writer.write_line(false);
        }
        if original_source_was_statementless && !self.options.remove_comments {
            let source = transformation.arena().source(source_id)?.syntax();
            emit_leading_comments(source.text(), &mut writer);
        } else if let Some(last_original_statement) = last_original_statement {
            self.emit_source_file_trailing_comments(
                transformation,
                last_original_statement,
                &mut writer,
            )?;
        }
        transformation.after_emit_node(EmitHint::SourceFile, root)?;
        let text = if system_scoped_helpers {
            self.insert_system_scoped_helpers(writer.text(), &helpers)?
        } else {
            writer.text().to_owned()
        };
        let end = if system_scoped_helpers {
            let mut measured = create_text_writer(self.options.new_line);
            measured.raw_write(&text);
            measured.location()
        } else {
            writer.location()
        };
        Ok(PrintedText { text, end })
    }

    fn is_system_register_statement(
        &self,
        transformation: &TransformationResult<'_>,
        statement: TransformNode,
    ) -> bool {
        let Ok(statement_record) = transformation.arena().node(statement) else {
            return false;
        };
        let Some(call) = (match &statement_record.data {
            NodeData::ExpressionStatement(data) => data.expression,
            _ => None,
        })
        .and_then(|expression| {
            transformation
                .arena()
                .node_ref(statement.source(), expression)
        }) else {
            return false;
        };
        let Ok(call_record) = transformation.arena().node(call) else {
            return false;
        };
        let Some(access) = (match &call_record.data {
            NodeData::CallExpression(data) => data.expression,
            _ => None,
        })
        .and_then(|expression| {
            transformation
                .arena()
                .node_ref(statement.source(), expression)
        }) else {
            return false;
        };
        let Ok(access_record) = transformation.arena().node(access) else {
            return false;
        };
        let NodeData::PropertyAccessExpression(data) = &access_record.data else {
            return false;
        };
        let expression = data.expression.and_then(|expression| {
            transformation
                .arena()
                .node_ref(statement.source(), expression)
        });
        let name = data
            .name
            .and_then(|name| transformation.arena().node_ref(statement.source(), name));
        expression
            .and_then(|node| transformation.arena().node(node).ok())
            .is_some_and(
                |node| matches!(&node.data, NodeData::Identifier(data) if data.text == "System"),
            )
            && name
                .and_then(|node| transformation.arena().node(node).ok())
                .is_some_and(
                    |node| matches!(&node.data, NodeData::Identifier(data) if data.text == "register"),
                )
    }

    fn insert_system_scoped_helpers(
        &self,
        text: &str,
        helpers: &[EmitHelper],
    ) -> Result<String, PrinterError> {
        let new_line = self.options.new_line.text();
        let first_line_end = text
            .find(new_line)
            .map(|offset| offset + new_line.len())
            .unwrap_or(0);
        let strict = format!("    \"use strict\";{new_line}");
        let insertion_offset = if text[first_line_end..].starts_with(&strict) {
            first_line_end + strict.len()
        } else {
            first_line_end
        };
        let mut helper_writer = create_text_writer(self.options.new_line);
        helper_writer.increase_indent();
        self.emit_helpers(helpers, &mut helper_writer)?;
        let mut output = String::with_capacity(text.len() + helper_writer.text().len());
        output.push_str(&text[..insertion_offset]);
        output.push_str(helper_writer.text());
        output.push_str(&text[insertion_offset..]);
        Ok(output)
    }

    fn emit_transformed_node(
        &self,
        transformation: &mut TransformationResult<'_>,
        node: TransformNode,
        expression_context: ExpressionEmissionContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let record = transformation.arena().node(node)?.clone();
        let changed = transformation
            .arena()
            .metadata(node)
            .and_then(crate::EmitMetadata::original)
            .is_some()
            || NodeFlags::from_bits(record.flags).contains(NodeFlags::SYNTHESIZED)
            || self.literal_emission_plan.structured_nodes.contains(&node);
        let multi_line = record.multi_line == Some(true);
        let json_source = transformation
            .arena()
            .source(node.source())?
            .syntax()
            .file_name
            .to_ascii_lowercase()
            .ends_with(".json");

        if expression_context == ExpressionEmissionContext::NoAsi {
            if let Some(parenthesized) =
                self.parenthesized_no_asi_expression(transformation, node)?
            {
                writer.write_punctuation("(");
                self.emit_leading_comments_for_node(
                    transformation,
                    parenthesized.comment_anchor,
                    writer,
                )?;
                self.emit_node_id(transformation, node.source(), parenthesized.inner, writer)?;
                writer.write_punctuation(")");
                return Ok(());
            }
        }

        match record.data {
            NodeData::Token if record.kind == SyntaxKind::JsxOpeningFragment => {
                writer.write_punctuation("<>");
                Ok(())
            }
            NodeData::Token if record.kind == SyntaxKind::JsxClosingFragment => {
                writer.write_punctuation("</>");
                Ok(())
            }
            NodeData::Token if changed => {
                let text = tsc_syntax::tokens::token_to_string(record.kind).ok_or(
                    PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    },
                )?;
                writer.write(text);
                Ok(())
            }
            NodeData::Identifier(data) if changed => {
                writer.write_symbol(&data.text);
                Ok(())
            }
            NodeData::PrivateIdentifier(data) if changed => {
                writer.write_symbol(&data.text);
                Ok(())
            }
            NodeData::NumericLiteral(data) if changed => {
                writer.write_literal(&data.text);
                Ok(())
            }
            NodeData::BigIntLiteral(data) => {
                writer.write_literal(&data.text);
                Ok(())
            }
            NodeData::Decorator(data) => {
                writer.write_punctuation("@");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::Decorator,
                    "expression",
                    writer,
                )
            }
            NodeData::ExpressionStatement(data) => {
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::ExpressionStatement,
                    "expression",
                    writer,
                )?;
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::ReturnStatement(data) => {
                writer.write_keyword("return");
                if let Some(expression) = data.expression {
                    writer.write_space(" ");
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        expression,
                        ExpressionEmissionContext::NoAsi,
                        writer,
                    )?;
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::StringLiteral(data) => {
                if !changed {
                    self.write_original_without_leading_trivia(transformation, node, writer)
                } else {
                    let metadata = transformation.arena().metadata(node);
                    let single_quote = metadata
                        .and_then(crate::EmitMetadata::string_literal_single_quote)
                        .unwrap_or(false);
                    let no_ascii_escaping = metadata.is_some_and(|metadata| {
                        metadata.flags().contains(EmitFlags::NO_ASCII_ESCAPING)
                    });
                    let quoted = metadata
                        .and_then(crate::EmitMetadata::javascript_string_value)
                        .map(|value| {
                            quote_javascript_string(
                                value.code_units(),
                                single_quote,
                                no_ascii_escaping,
                            )
                        })
                        .unwrap_or_else(|| {
                            quote_string_literal(&data.text, single_quote, no_ascii_escaping)
                        });
                    writer.write_string_literal(&quoted);
                    Ok(())
                }
            }
            NodeData::JsxElement(data) => {
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.opening_element,
                    SyntaxKind::JsxElement,
                    "opening_element",
                    writer,
                )?;
                self.emit_node_array(transformation, node.source(), data.children, "", writer)?;
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.closing_element,
                    SyntaxKind::JsxElement,
                    "closing_element",
                    writer,
                )
            }
            NodeData::JsxSelfClosingElement(data) => {
                writer.write_punctuation("<");
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.tag_name,
                    SyntaxKind::JsxSelfClosingElement,
                    "tag_name",
                    writer,
                )?;
                self.emit_node_array(
                    transformation,
                    node.source(),
                    data.type_arguments,
                    ", ",
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.attributes,
                    SyntaxKind::JsxSelfClosingElement,
                    "attributes",
                    writer,
                )?;
                writer.write_punctuation("/>");
                Ok(())
            }
            NodeData::JsxFragment(data) => {
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.opening_fragment,
                    SyntaxKind::JsxFragment,
                    "opening_fragment",
                    writer,
                )?;
                self.emit_node_array(transformation, node.source(), data.children, "", writer)?;
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.closing_fragment,
                    SyntaxKind::JsxFragment,
                    "closing_fragment",
                    writer,
                )
            }
            NodeData::JsxOpeningElement(data) => {
                writer.write_punctuation("<");
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.tag_name,
                    SyntaxKind::JsxOpeningElement,
                    "tag_name",
                    writer,
                )?;
                self.emit_node_array(
                    transformation,
                    node.source(),
                    data.type_arguments,
                    ", ",
                    writer,
                )?;
                let has_attributes = data
                    .attributes
                    .and_then(|id| transformation.arena().node_ref(node.source(), id))
                    .and_then(|attributes| transformation.arena().node(attributes).ok())
                    .and_then(|attributes| attributes.data.as_jsx_attributes())
                    .and_then(|attributes| attributes.properties)
                    .and_then(|id| transformation.arena().node_array_ref(node.source(), id))
                    .is_some_and(|array| {
                        transformation
                            .arena()
                            .node_array(array)
                            .is_ok_and(|array| !array.nodes.is_empty())
                    });
                if has_attributes {
                    writer.write_space(" ");
                }
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.attributes,
                    SyntaxKind::JsxOpeningElement,
                    "attributes",
                    writer,
                )?;
                writer.write_punctuation(">");
                Ok(())
            }
            NodeData::JsxClosingElement(data) => {
                writer.write_punctuation("</");
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.tag_name,
                    SyntaxKind::JsxClosingElement,
                    "tag_name",
                    writer,
                )?;
                writer.write_punctuation(">");
                Ok(())
            }
            NodeData::JsxAttributes(data) => {
                self.emit_node_array(transformation, node.source(), data.properties, " ", writer)
            }
            NodeData::JsxAttribute(data) => {
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::JsxAttribute,
                    "name",
                    writer,
                )?;
                if let Some(initializer) = data.initializer {
                    writer.write_punctuation("=");
                    self.emit_node_id(transformation, node.source(), initializer, writer)?;
                }
                Ok(())
            }
            NodeData::JsxSpreadAttribute(data) => {
                writer.write_punctuation("{...");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::JsxSpreadAttribute,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation("}");
                Ok(())
            }
            NodeData::JsxExpression(data) => {
                let Some(expression) = data.expression else {
                    let source = transformation.arena().source(node.source())?.syntax();
                    let record = transformation.arena().node(node)?;
                    let has_comment = if record.pos <= record.end
                        && record.end != u32::MAX
                        && (record.end as usize) <= source.text().len()
                    {
                        source
                            .text()
                            .get(record.pos as usize..record.end as usize)
                            .is_some_and(|text| text.contains("/*") || text.contains("//"))
                    } else {
                        false
                    };
                    return if has_comment {
                        self.write_original_without_leading_trivia(transformation, node, writer)
                    } else {
                        Ok(())
                    };
                };
                writer.write_punctuation("{");
                if let Some(dot_dot_dot) = data.dot_dot_dot_token {
                    self.emit_node_id(transformation, node.source(), dot_dot_dot, writer)?;
                }
                self.emit_node_id(transformation, node.source(), expression, writer)?;
                writer.write_punctuation("}");
                Ok(())
            }
            NodeData::JsxNamespacedName(data) => {
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.namespace,
                    SyntaxKind::JsxNamespacedName,
                    "namespace",
                    writer,
                )?;
                writer.write_punctuation(":");
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::JsxNamespacedName,
                    "name",
                    writer,
                )
            }
            NodeData::JsxText(data) => {
                writer.write_literal(&data.text);
                Ok(())
            }
            NodeData::NoSubstitutionTemplateLiteral(_) | NodeData::TemplateExpression(_)
                if !changed =>
            {
                self.write_original_without_leading_trivia_verbatim(transformation, node, writer)
            }
            NodeData::TemplateExpression(data) => {
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.head,
                    SyntaxKind::TemplateExpression,
                    "head",
                    writer,
                )?;
                self.emit_node_array(
                    transformation,
                    node.source(),
                    data.template_spans,
                    "",
                    writer,
                )
            }
            NodeData::TemplateHead(data) => {
                writer.write_punctuation("`");
                writer.write_literal(data.raw_text.as_deref().unwrap_or(&data.text));
                writer.write_punctuation("${");
                Ok(())
            }
            NodeData::TemplateMiddle(data) => {
                writer.write_punctuation("}");
                writer.write_literal(data.raw_text.as_deref().unwrap_or(&data.text));
                writer.write_punctuation("${");
                Ok(())
            }
            NodeData::TemplateTail(data) => {
                writer.write_punctuation("}");
                writer.write_literal(data.raw_text.as_deref().unwrap_or(&data.text));
                writer.write_punctuation("`");
                Ok(())
            }
            NodeData::TemplateSpan(data) => {
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::TemplateSpan,
                    "expression",
                    writer,
                )?;
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.literal,
                    SyntaxKind::TemplateSpan,
                    "literal",
                    writer,
                )
            }
            NodeData::ImportDeclaration(data) => {
                if !changed
                    && !self.options.remove_comments
                    && self.original_node_has_internal_comments(transformation, node)?
                {
                    return self.write_original_module_statement(transformation, node, writer);
                }
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                writer.write_keyword("import");
                if let Some(clause) = data.import_clause {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), clause, writer)?;
                    writer.write_space(" ");
                    writer.write_keyword("from");
                }
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.module_specifier,
                    SyntaxKind::ImportDeclaration,
                    "module_specifier",
                    writer,
                )?;
                if let Some(attributes) = data.attributes {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), attributes, writer)?;
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::ImportClause(data) => {
                if data.is_type_only || data.phase_modifier == Some(SyntaxKind::TypeKeyword) {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                if let Some(phase_modifier) = data.phase_modifier {
                    if phase_modifier != SyntaxKind::DeferKeyword {
                        return Err(PrinterError::UnsupportedTransformedSyntax {
                            node,
                            kind: record.kind,
                        });
                    }
                    writer.write_keyword("defer");
                    writer.write_space(" ");
                }
                if let Some(name) = data.name {
                    self.emit_identifier_name(transformation, node.source(), name, writer)?;
                    if data.named_bindings.is_some() {
                        writer.write_punctuation(",");
                        writer.write_space(" ");
                    }
                }
                if let Some(bindings) = data.named_bindings {
                    self.emit_node_id(transformation, node.source(), bindings, writer)?;
                }
                Ok(())
            }
            NodeData::NamespaceImport(data) => {
                writer.write_operator("*");
                writer.write_space(" ");
                writer.write_keyword("as");
                writer.write_space(" ");
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::NamespaceImport,
                    "name",
                    writer,
                )
            }
            NodeData::NamedImports(data) => self.emit_named_import_or_export_list(
                transformation,
                node.source(),
                data.elements,
                writer,
            ),
            NodeData::ImportSpecifier(data) => {
                if data.is_type_only {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                self.emit_renamed_specifier(
                    transformation,
                    node.source(),
                    data.property_name,
                    data.name,
                    SyntaxKind::ImportSpecifier,
                    writer,
                )
            }
            NodeData::ExportDeclaration(data) => {
                if !changed
                    && !self.options.remove_comments
                    && self.original_node_has_internal_comments(transformation, node)?
                {
                    return self.write_original_module_statement(transformation, node, writer);
                }
                if data.is_type_only {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                writer.write_keyword("export");
                writer.write_space(" ");
                if let Some(clause) = data.export_clause {
                    self.emit_node_id(transformation, node.source(), clause, writer)?;
                } else {
                    writer.write_operator("*");
                }
                if let Some(module_specifier) = data.module_specifier {
                    writer.write_space(" ");
                    writer.write_keyword("from");
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), module_specifier, writer)?;
                }
                if let Some(attributes) = data.attributes {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), attributes, writer)?;
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::ImportAttributes(data) => {
                let keyword = tsc_syntax::tokens::token_to_string(data.token).ok_or(
                    PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    },
                )?;
                writer.write_keyword(keyword);
                writer.write_space(" ");
                writer.write_punctuation("{");
                let elements = data
                    .elements
                    .and_then(|id| transformation.arena().node_array_ref(node.source(), id))
                    .map(|array| transformation.arena().node_array(array))
                    .transpose()?;
                let non_empty = elements.is_some_and(|array| !array.nodes.is_empty());
                if non_empty {
                    if data.multi_line == Some(true) || multi_line {
                        let (ids, trailing_comma) = elements
                            .map(|array| (array.nodes.clone(), array.has_trailing_comma))
                            .unwrap_or_default();
                        writer.write_line(false);
                        writer.increase_indent();
                        for (index, id) in ids.iter().enumerate() {
                            self.emit_node_id(transformation, node.source(), *id, writer)?;
                            if index + 1 != ids.len() || trailing_comma {
                                writer.write_punctuation(",");
                            }
                            writer.write_line(false);
                        }
                        writer.decrease_indent();
                    } else {
                        writer.write_space(" ");
                        self.emit_node_array(
                            transformation,
                            node.source(),
                            data.elements,
                            ", ",
                            writer,
                        )?;
                        writer.write_space(" ");
                    }
                }
                writer.write_punctuation("}");
                Ok(())
            }
            NodeData::ImportAttribute(data) => {
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::ImportAttribute,
                    "name",
                    writer,
                )?;
                writer.write_punctuation(":");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.value,
                    SyntaxKind::ImportAttribute,
                    "value",
                    writer,
                )
            }
            NodeData::NamedExports(data) => self.emit_named_import_or_export_list(
                transformation,
                node.source(),
                data.elements,
                writer,
            ),
            NodeData::NamespaceExport(data) => {
                writer.write_operator("*");
                writer.write_space(" ");
                writer.write_keyword("as");
                writer.write_space(" ");
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::NamespaceExport,
                    "name",
                    writer,
                )
            }
            NodeData::ExportSpecifier(data) => {
                if data.is_type_only {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                self.emit_renamed_specifier(
                    transformation,
                    node.source(),
                    data.property_name,
                    data.name,
                    SyntaxKind::ExportSpecifier,
                    writer,
                )
            }
            NodeData::ExportAssignment(data) => {
                if !changed
                    && !self.options.remove_comments
                    && self.original_node_has_internal_comments(transformation, node)?
                {
                    return self.write_original_module_statement(transformation, node, writer);
                }
                if data.is_export_equals == Some(true) {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                writer.write_keyword("export");
                writer.write_space(" ");
                writer.write_keyword("default");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::ExportAssignment,
                    "expression",
                    writer,
                )?;
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::VariableStatement(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.declaration_list,
                    SyntaxKind::VariableStatement,
                    "declaration_list",
                    writer,
                )?;
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::VariableDeclarationList(data) => {
                let flags = NodeFlags::from_bits(record.flags);
                if flags.contains(NodeFlags::AWAIT_USING) {
                    writer.write_keyword("await");
                    writer.write_space(" ");
                    writer.write_keyword("using");
                } else if flags.contains(NodeFlags::USING) {
                    writer.write_keyword("using");
                } else if flags.contains(NodeFlags::LET) {
                    writer.write_keyword("let");
                } else if flags.contains(NodeFlags::CONST) {
                    writer.write_keyword("const");
                } else {
                    writer.write_keyword("var");
                }
                writer.write_space(" ");
                self.emit_node_array(
                    transformation,
                    node.source(),
                    data.declarations,
                    ", ",
                    writer,
                )
            }
            NodeData::VariableDeclaration(data) => {
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::VariableDeclaration,
                    "name",
                    writer,
                )?;
                if let Some(initializer) = data.initializer {
                    writer.write_space(" ");
                    writer.write_operator("=");
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), initializer, writer)?;
                }
                Ok(())
            }
            NodeData::ArrayLiteralExpression(data) if json_source => self
                .emit_json_delimited_expression_list(
                    transformation,
                    node,
                    data.elements,
                    "[",
                    "]",
                    multi_line,
                    true,
                    writer,
                ),
            NodeData::ArrayLiteralExpression(data) => self.emit_delimited_expression_list(
                transformation,
                node.source(),
                data.elements,
                "[",
                "]",
                multi_line,
                writer,
            ),
            NodeData::ArrayBindingPattern(data) => self.emit_delimited_expression_list(
                transformation,
                node.source(),
                data.elements,
                "[",
                "]",
                multi_line,
                writer,
            ),
            NodeData::ObjectBindingPattern(data) => self.emit_delimited_expression_list(
                transformation,
                node.source(),
                data.elements,
                "{",
                "}",
                multi_line,
                writer,
            ),
            NodeData::BindingElement(data) => {
                if let Some(dot_dot_dot) = data.dot_dot_dot_token {
                    self.emit_node_id(transformation, node.source(), dot_dot_dot, writer)?;
                }
                if let Some(property_name) = data.property_name {
                    self.emit_identifier_name(
                        transformation,
                        node.source(),
                        property_name,
                        writer,
                    )?;
                    writer.write_punctuation(":");
                    writer.write_space(" ");
                }
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::BindingElement,
                    "name",
                    writer,
                )?;
                if let Some(initializer) = data.initializer {
                    writer.write_space(" ");
                    writer.write_operator("=");
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), initializer, writer)?;
                }
                Ok(())
            }
            NodeData::ComputedPropertyName(data) => {
                writer.write_punctuation("[");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::ComputedPropertyName,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation("]");
                Ok(())
            }
            NodeData::AwaitExpression(data) => {
                writer.write_keyword("await");
                writer.write_space(" ");
                let expression = data
                    .expression
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                if let Some(expression) = expression {
                    self.emit_leading_comments_for_node(transformation, expression, writer)?;
                }
                if expression.is_some_and(|expression| {
                    transformation
                        .arena()
                        .node(expression)
                        .is_ok_and(|record| record.kind == SyntaxKind::ConditionalExpression)
                }) {
                    writer.write_punctuation("(");
                    self.emit_required_node(
                        transformation,
                        node.source(),
                        data.expression,
                        SyntaxKind::AwaitExpression,
                        "expression",
                        writer,
                    )?;
                    writer.write_punctuation(")");
                    Ok(())
                } else {
                    self.emit_required_node(
                        transformation,
                        node.source(),
                        data.expression,
                        SyntaxKind::AwaitExpression,
                        "expression",
                        writer,
                    )
                }
            }
            NodeData::VoidExpression(data) => {
                writer.write_keyword("void");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::VoidExpression,
                    "expression",
                    writer,
                )
            }
            NodeData::DeleteExpression(data) => {
                writer.write_keyword("delete");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::DeleteExpression,
                    "expression",
                    writer,
                )
            }
            NodeData::TypeOfExpression(data) => {
                writer.write_keyword("typeof");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::TypeOfExpression,
                    "expression",
                    writer,
                )
            }
            NodeData::ConditionalExpression(data) => {
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.condition,
                    SyntaxKind::ConditionalExpression,
                    "condition",
                    expression_context,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.question_token,
                    SyntaxKind::ConditionalExpression,
                    "question_token",
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.when_true,
                    SyntaxKind::ConditionalExpression,
                    "when_true",
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.colon_token,
                    SyntaxKind::ConditionalExpression,
                    "colon_token",
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.when_false,
                    SyntaxKind::ConditionalExpression,
                    "when_false",
                    writer,
                )
            }
            NodeData::YieldExpression(data) => {
                writer.write_keyword("yield");
                if let Some(asterisk) = data.asterisk_token {
                    self.emit_node_id(transformation, node.source(), asterisk, writer)?;
                }
                if let Some(expression) = data.expression {
                    writer.write_space(" ");
                    self.emit_node_id_with_context(
                        transformation,
                        node.source(),
                        expression,
                        ExpressionEmissionContext::NoAsi,
                        writer,
                    )?;
                }
                Ok(())
            }
            NodeData::ObjectLiteralExpression(data) if json_source => self
                .emit_json_delimited_expression_list(
                    transformation,
                    node,
                    data.properties,
                    "{",
                    "}",
                    multi_line,
                    false,
                    writer,
                ),
            NodeData::ObjectLiteralExpression(data) => self.emit_delimited_expression_list(
                transformation,
                node.source(),
                data.properties,
                "{",
                "}",
                multi_line,
                writer,
            ),
            NodeData::PropertyAssignment(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::PropertyAssignment,
                    "name",
                    writer,
                )?;
                writer.write_punctuation(":");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.initializer,
                    SyntaxKind::PropertyAssignment,
                    "initializer",
                    writer,
                )
            }
            NodeData::HeritageClause(data) => {
                let keyword = match data.token {
                    SyntaxKind::ExtendsKeyword => "extends",
                    SyntaxKind::ImplementsKeyword => "implements",
                    _ => {
                        return Err(PrinterError::UnsupportedTransformedSyntax {
                            node,
                            kind: record.kind,
                        });
                    }
                };
                writer.write_keyword(keyword);
                writer.write_space(" ");
                self.emit_node_array(transformation, node.source(), data.types, ", ", writer)
            }
            NodeData::ExpressionWithTypeArguments(data) => self.emit_required_node(
                transformation,
                node.source(),
                data.expression,
                SyntaxKind::ExpressionWithTypeArguments,
                "expression",
                writer,
            ),
            NodeData::SpreadAssignment(data) => {
                writer.write_punctuation("...");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::SpreadAssignment,
                    "expression",
                    writer,
                )
            }
            NodeData::SpreadElement(data) => {
                writer.write_punctuation("...");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::SpreadElement,
                    "expression",
                    writer,
                )
            }
            NodeData::FunctionDeclaration(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                writer.write_keyword("function");
                if let Some(asterisk) = data.asterisk_token {
                    self.emit_node_id(transformation, node.source(), asterisk, writer)?;
                }
                if let Some(name) = data.name {
                    writer.write_space(" ");
                    self.emit_identifier_name(transformation, node.source(), name, writer)?;
                } else {
                    writer.write_space(" ");
                }
                self.emit_parameter_list(transformation, node.source(), data.parameters, writer)?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), body, writer)?;
                }
                Ok(())
            }
            NodeData::FunctionExpression(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                writer.write_keyword("function");
                if let Some(asterisk) = data.asterisk_token {
                    self.emit_node_id(transformation, node.source(), asterisk, writer)?;
                }
                if let Some(name) = data.name {
                    writer.write_space(" ");
                    self.emit_identifier_name(transformation, node.source(), name, writer)?;
                } else {
                    writer.write_space(" ");
                }
                self.emit_parameter_list(transformation, node.source(), data.parameters, writer)?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), body, writer)?;
                }
                Ok(())
            }
            NodeData::ArrowFunction(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                if let Some(parameter) = self.simple_arrow_parameter(transformation, node, &data)? {
                    self.emit_node_id(transformation, node.source(), parameter, writer)?;
                } else {
                    self.emit_parameter_list(
                        transformation,
                        node.source(),
                        data.parameters,
                        writer,
                    )?;
                }
                writer.write_space(" ");
                writer.write_operator("=>");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.body,
                    SyntaxKind::ArrowFunction,
                    "body",
                    writer,
                )
            }
            NodeData::Parameter(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                if let Some(rest) = data.dot_dot_dot_token {
                    self.emit_node_id(transformation, node.source(), rest, writer)?;
                }
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::Parameter,
                    "name",
                    writer,
                )?;
                if let Some(initializer) = data.initializer {
                    writer.write_space(" ");
                    writer.write_operator("=");
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), initializer, writer)?;
                }
                Ok(())
            }
            NodeData::ClassDeclaration(data) => self.emit_class(
                transformation,
                node,
                node.source(),
                data.modifiers,
                data.name,
                data.heritage_clauses,
                data.members,
                false,
                writer,
            ),
            NodeData::ClassExpression(data) => self.emit_class(
                transformation,
                node,
                node.source(),
                data.modifiers,
                data.name,
                data.heritage_clauses,
                data.members,
                true,
                writer,
            ),
            NodeData::ClassStaticBlockDeclaration(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                writer.write_keyword("static");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.body,
                    SyntaxKind::ClassStaticBlockDeclaration,
                    "body",
                    writer,
                )
            }
            NodeData::PropertyDeclaration(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::PropertyDeclaration,
                    "name",
                    writer,
                )?;
                if let Some(initializer) = data.initializer {
                    writer.write_space(" ");
                    writer.write_operator("=");
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), initializer, writer)?;
                    if let Some(initializer) =
                        transformation.arena().node_ref(node.source(), initializer)
                    {
                        self.emit_trailing_block_comments_before_semicolon(
                            transformation,
                            initializer,
                            writer,
                        )?;
                    }
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::Constructor(data) => {
                writer.write_keyword("constructor");
                self.emit_parameter_list(transformation, node.source(), data.parameters, writer)?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), body, writer)?;
                }
                Ok(())
            }
            NodeData::MethodDeclaration(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                if let Some(asterisk) = data.asterisk_token {
                    self.emit_node_id(transformation, node.source(), asterisk, writer)?;
                }
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::MethodDeclaration,
                    "name",
                    writer,
                )?;
                self.emit_parameter_list(transformation, node.source(), data.parameters, writer)?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), body, writer)?;
                }
                Ok(())
            }
            NodeData::GetAccessor(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                writer.write_keyword("get");
                writer.write_space(" ");
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::GetAccessor,
                    "name",
                    writer,
                )?;
                self.emit_parameter_list(transformation, node.source(), data.parameters, writer)?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), body, writer)?;
                }
                Ok(())
            }
            NodeData::SetAccessor(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                writer.write_keyword("set");
                writer.write_space(" ");
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.name,
                    SyntaxKind::SetAccessor,
                    "name",
                    writer,
                )?;
                self.emit_parameter_list(transformation, node.source(), data.parameters, writer)?;
                if let Some(body) = data.body {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), body, writer)?;
                }
                Ok(())
            }
            NodeData::ForStatement(data) => {
                writer.write_keyword("for");
                writer.write_space(" ");
                writer.write_punctuation("(");
                if let Some(initializer) = data.initializer {
                    self.emit_node_id(transformation, node.source(), initializer, writer)?;
                }
                writer.write_punctuation(";");
                if let Some(condition) = data.condition {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), condition, writer)?;
                }
                writer.write_punctuation(";");
                if let Some(incrementor) = data.incrementor {
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), incrementor, writer)?;
                }
                writer.write_punctuation(")");
                self.emit_embedded_statement(
                    transformation,
                    node.source(),
                    data.statement,
                    SyntaxKind::ForStatement,
                    writer,
                )
            }
            NodeData::ForInStatement(data) => {
                writer.write_keyword("for");
                writer.write_space(" ");
                writer.write_punctuation("(");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.initializer,
                    SyntaxKind::ForInStatement,
                    "initializer",
                    writer,
                )?;
                writer.write_space(" ");
                writer.write_keyword("in");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::ForInStatement,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation(")");
                self.emit_embedded_statement(
                    transformation,
                    node.source(),
                    data.statement,
                    SyntaxKind::ForInStatement,
                    writer,
                )
            }
            NodeData::ForOfStatement(data) => {
                writer.write_keyword("for");
                writer.write_space(" ");
                if let Some(await_modifier) = data.await_modifier {
                    self.emit_node_id(transformation, node.source(), await_modifier, writer)?;
                    writer.write_space(" ");
                }
                writer.write_punctuation("(");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.initializer,
                    SyntaxKind::ForOfStatement,
                    "initializer",
                    writer,
                )?;
                writer.write_space(" ");
                writer.write_keyword("of");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::ForOfStatement,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation(")");
                self.emit_embedded_statement(
                    transformation,
                    node.source(),
                    data.statement,
                    SyntaxKind::ForOfStatement,
                    writer,
                )
            }
            NodeData::IfStatement(data) => {
                writer.write_keyword("if");
                writer.write_space(" ");
                writer.write_punctuation("(");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::IfStatement,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation(")");
                self.emit_embedded_statement(
                    transformation,
                    node.source(),
                    data.then_statement,
                    SyntaxKind::IfStatement,
                    writer,
                )?;
                if let Some(else_statement) = data.else_statement {
                    writer.write_line(false);
                    writer.write_keyword("else");
                    let else_is_if = transformation
                        .arena()
                        .node_ref(node.source(), else_statement)
                        .is_some_and(|statement| {
                            transformation
                                .arena()
                                .node(statement)
                                .is_ok_and(|statement| statement.kind == SyntaxKind::IfStatement)
                        });
                    if else_is_if {
                        writer.write_space(" ");
                        self.emit_node_id(transformation, node.source(), else_statement, writer)
                    } else {
                        self.emit_embedded_statement(
                            transformation,
                            node.source(),
                            Some(else_statement),
                            SyntaxKind::IfStatement,
                            writer,
                        )
                    }
                } else {
                    Ok(())
                }
            }
            NodeData::SwitchStatement(data) => {
                writer.write_keyword("switch");
                writer.write_space(" ");
                writer.write_punctuation("(");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::SwitchStatement,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation(")");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.case_block,
                    SyntaxKind::SwitchStatement,
                    "case_block",
                    writer,
                )
            }
            NodeData::CaseBlock(data) => {
                self.emit_case_block(transformation, node.source(), data.clauses, writer)
            }
            NodeData::CaseClause(data) => {
                writer.write_keyword("case");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::CaseClause,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation(":");
                self.emit_case_clause_statements(
                    transformation,
                    node,
                    node.source(),
                    data.statements,
                    writer,
                )
            }
            NodeData::DefaultClause(data) => {
                writer.write_keyword("default");
                writer.write_punctuation(":");
                self.emit_case_clause_statements(
                    transformation,
                    node,
                    node.source(),
                    data.statements,
                    writer,
                )
            }
            NodeData::BreakStatement(data) => {
                writer.write_keyword("break");
                if let Some(label) = data.label {
                    writer.write_space(" ");
                    self.emit_identifier_name(transformation, node.source(), label, writer)?;
                }
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::LabeledStatement(data) => {
                self.emit_required_identifier_name(
                    transformation,
                    node.source(),
                    data.label,
                    SyntaxKind::LabeledStatement,
                    "label",
                    writer,
                )?;
                writer.write_punctuation(":");
                self.emit_embedded_statement(
                    transformation,
                    node.source(),
                    data.statement,
                    SyntaxKind::LabeledStatement,
                    writer,
                )
            }
            NodeData::WithStatement(data) => {
                writer.write_keyword("with");
                writer.write_space(" ");
                writer.write_punctuation("(");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::WithStatement,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation(")");
                self.emit_embedded_statement(
                    transformation,
                    node.source(),
                    data.statement,
                    SyntaxKind::WithStatement,
                    writer,
                )
            }
            NodeData::PartiallyEmittedExpression(data) => self.emit_required_node_with_context(
                transformation,
                node.source(),
                data.expression,
                SyntaxKind::PartiallyEmittedExpression,
                "expression",
                expression_context,
                writer,
            ),
            NodeData::NewExpression(data) => {
                writer.write_keyword("new");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::NewExpression,
                    "expression",
                    writer,
                )?;
                if data.arguments.is_some() {
                    writer.write_punctuation("(");
                    self.emit_call_arguments(
                        transformation,
                        node.source(),
                        data.arguments,
                        multi_line,
                        writer,
                    )?;
                    writer.write_punctuation(")");
                }
                Ok(())
            }
            NodeData::WhileStatement(data) => {
                writer.write_keyword("while");
                writer.write_space(" ");
                writer.write_punctuation("(");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::WhileStatement,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation(")");
                self.emit_embedded_statement(
                    transformation,
                    node.source(),
                    data.statement,
                    SyntaxKind::WhileStatement,
                    writer,
                )
            }
            NodeData::DoStatement(data) => {
                writer.write_keyword("do");
                self.emit_embedded_statement(
                    transformation,
                    node.source(),
                    data.statement,
                    SyntaxKind::DoStatement,
                    writer,
                )?;
                writer.write_space(" ");
                writer.write_keyword("while");
                writer.write_space(" ");
                writer.write_punctuation("(");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::DoStatement,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation(")");
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::TryStatement(data) => {
                writer.write_keyword("try");
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.try_block,
                    SyntaxKind::TryStatement,
                    "try_block",
                    writer,
                )?;
                if let Some(catch_clause) = data.catch_clause {
                    writer.write_line(false);
                    self.emit_node_id(transformation, node.source(), catch_clause, writer)?;
                }
                if let Some(finally_block) = data.finally_block {
                    writer.write_line(false);
                    writer.write_keyword("finally");
                    writer.write_space(" ");
                    self.emit_node_id(transformation, node.source(), finally_block, writer)?;
                }
                Ok(())
            }
            NodeData::CatchClause(data) => {
                writer.write_keyword("catch");
                writer.write_space(" ");
                if let Some(variable) = data.variable_declaration {
                    writer.write_punctuation("(");
                    self.emit_node_id(transformation, node.source(), variable, writer)?;
                    writer.write_punctuation(")");
                    writer.write_space(" ");
                }
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.block,
                    SyntaxKind::CatchClause,
                    "block",
                    writer,
                )
            }
            NodeData::CallExpression(data) => {
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::CallExpression,
                    "expression",
                    expression_context,
                    writer,
                )?;
                if data.question_dot_token.is_some() {
                    writer.write_punctuation("?.");
                }
                writer.write_punctuation("(");
                self.emit_call_arguments(
                    transformation,
                    node.source(),
                    data.arguments,
                    multi_line,
                    writer,
                )?;
                writer.write_punctuation(")");
                Ok(())
            }
            NodeData::TaggedTemplateExpression(data) => {
                if data.type_arguments.is_some() || data.question_dot_token.is_some() {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.tag,
                    SyntaxKind::TaggedTemplateExpression,
                    "tag",
                    expression_context,
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.template,
                    SyntaxKind::TaggedTemplateExpression,
                    "template",
                    writer,
                )
            }
            NodeData::ParenthesizedExpression(data) => {
                writer.write_punctuation("(");
                let expression = data
                    .expression
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                if let Some(expression) = expression {
                    self.emit_leading_comments_for_node(transformation, expression, writer)?;
                }
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::ParenthesizedExpression,
                    "expression",
                    writer,
                )?;
                if let Some(expression) = expression {
                    self.emit_trailing_comments_for_node(transformation, expression, writer)?;
                }
                writer.write_punctuation(")");
                Ok(())
            }
            NodeData::PrefixUnaryExpression(data) => {
                let operator = tsc_syntax::tokens::token_to_string(data.operator).ok_or(
                    PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    },
                )?;
                writer.write_operator(operator);
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.operand,
                    SyntaxKind::PrefixUnaryExpression,
                    "operand",
                    writer,
                )
            }
            NodeData::PostfixUnaryExpression(data) => {
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.operand,
                    SyntaxKind::PostfixUnaryExpression,
                    "operand",
                    expression_context,
                    writer,
                )?;
                let operator = tsc_syntax::tokens::token_to_string(data.operator).ok_or(
                    PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    },
                )?;
                writer.write_operator(operator);
                Ok(())
            }
            NodeData::PropertyAccessExpression(data) => {
                let expression = data
                    .expression
                    .ok_or(PrinterError::MissingTransformedChild {
                        parent: SyntaxKind::PropertyAccessExpression,
                        field: "expression",
                    })?;
                let name = data.name.ok_or(PrinterError::MissingTransformedChild {
                    parent: SyntaxKind::PropertyAccessExpression,
                    field: "name",
                })?;
                self.emit_node_id_with_context(
                    transformation,
                    node.source(),
                    expression,
                    expression_context,
                    writer,
                )?;
                let break_before_name = self.source_gap_has_line_break(
                    transformation,
                    node.source(),
                    expression,
                    name,
                )?;
                if break_before_name {
                    writer.write_line(false);
                    writer.increase_indent();
                }
                writer.write_punctuation(if data.question_dot_token.is_some() {
                    "?."
                } else {
                    "."
                });
                self.emit_identifier_name(transformation, node.source(), name, writer)?;
                if break_before_name {
                    writer.decrease_indent();
                }
                Ok(())
            }
            NodeData::ElementAccessExpression(data) => {
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::ElementAccessExpression,
                    "expression",
                    expression_context,
                    writer,
                )?;
                if data.question_dot_token.is_some() {
                    writer.write_punctuation("?.");
                }
                writer.write_punctuation("[");
                if let Some(argument) = data
                    .argument_expression
                    .and_then(|id| transformation.arena().node_ref(node.source(), id))
                {
                    self.emit_leading_comments_for_node(transformation, argument, writer)?;
                }
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.argument_expression,
                    SyntaxKind::ElementAccessExpression,
                    "argument_expression",
                    writer,
                )?;
                writer.write_punctuation("]");
                Ok(())
            }
            NodeData::BinaryExpression(data) => {
                let left_node = data
                    .left
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                let operator_node = data
                    .operator_token
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                let right_node = data
                    .right
                    .and_then(|id| transformation.arena().node_ref(node.source(), id));
                let line_before_operator = match (left_node, operator_node) {
                    (Some(left), Some(operator)) => !self
                        .source_node_end_and_node_start_are_on_same_line(
                            transformation,
                            left,
                            operator,
                        )?,
                    _ => false,
                };
                let line_after_operator = match (operator_node, right_node) {
                    (Some(operator), Some(right)) => {
                        transformation
                            .arena()
                            .metadata(right)
                            .and_then(|metadata| metadata.starts_on_new_line())
                            == Some(true)
                            || !self.source_node_end_and_node_start_are_on_same_line(
                                transformation,
                                operator,
                                right,
                            )?
                    }
                    _ => false,
                };
                self.emit_required_node_with_context(
                    transformation,
                    node.source(),
                    data.left,
                    SyntaxKind::BinaryExpression,
                    "left",
                    expression_context,
                    writer,
                )?;
                let operator = data
                    .operator_token
                    .and_then(|id| transformation.arena().node_ref(node.source(), id))
                    .map(|operator| transformation.arena().node(operator))
                    .transpose()?
                    .map(|operator| operator.kind);
                if line_before_operator {
                    writer.write_line(false);
                    writer.increase_indent();
                } else if operator != Some(SyntaxKind::CommaToken) {
                    writer.write_space(" ");
                }
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.operator_token,
                    SyntaxKind::BinaryExpression,
                    "operator_token",
                    writer,
                )?;
                if line_after_operator {
                    writer.write_line(false);
                    if !line_before_operator {
                        writer.increase_indent();
                    }
                } else {
                    writer.write_space(" ");
                }
                let result = self.emit_required_node(
                    transformation,
                    node.source(),
                    data.right,
                    SyntaxKind::BinaryExpression,
                    "right",
                    writer,
                );
                if line_before_operator || line_after_operator {
                    writer.decrease_indent();
                }
                result
            }
            NodeData::Block(data) => {
                writer.write_punctuation("{");
                let function_body = self.is_function_body_block(transformation, node)?;
                let array = data
                    .statements
                    .and_then(|id| transformation.arena().node_array_ref(node.source(), id));
                let statements = array
                    .map(|array| transformation.arena().node_array(array))
                    .transpose()?
                    .map(|array| array.nodes.clone())
                    .unwrap_or_default();
                if statements.is_empty() {
                    let emitted_comments = self.emit_empty_block_comments(
                        transformation,
                        node,
                        multi_line,
                        function_body,
                        writer,
                    )?;
                    if !emitted_comments && multi_line {
                        writer.write_line(false);
                    } else if !emitted_comments {
                        writer.write_space(" ");
                    }
                } else if !multi_line {
                    if !function_body {
                        self.emit_comment_after_open_brace(transformation, node, writer)?;
                    }
                    writer.write_space(" ");
                    for (index, statement) in statements.into_iter().enumerate() {
                        if index != 0 {
                            writer.write_space(" ");
                        }
                        let statement_node = transformation
                            .arena()
                            .node_ref(node.source(), statement)
                            .ok_or(PrinterError::UnknownStatement(statement.0))?;
                        self.emit_node_id(transformation, node.source(), statement, writer)?;
                        self.emit_trailing_comments_for_node(
                            transformation,
                            statement_node,
                            writer,
                        )?;
                    }
                    writer.write_space(" ");
                } else {
                    if !function_body {
                        self.emit_comment_after_open_brace(transformation, node, writer)?;
                    }
                    writer.write_line(false);
                    writer.increase_indent();
                    let last_statement = statements.last().copied();
                    let mut has_previous_original_statement = false;
                    for statement in statements {
                        let statement = transformation
                            .arena()
                            .node_ref(node.source(), statement)
                            .ok_or(PrinterError::UnknownStatement(statement.0))?;
                        let original = transformation.arena().get_original_node(statement);
                        let original_source =
                            transformation.arena().source(original.source())?.syntax();
                        let original_record = transformation.arena().node(original)?;
                        let has_original_range = matches!(
                            SourceRange::from_raw(
                                original_record.pos,
                                original_record.end,
                                original_source.positions(),
                            )?,
                            SourceRange::Original(_)
                        );
                        if has_previous_original_statement && has_original_range {
                            self.emit_leading_comments_for_node_after_sibling(
                                transformation,
                                statement,
                                writer,
                            )?;
                        } else {
                            self.emit_leading_comments_for_node(transformation, statement, writer)?;
                        }
                        if has_original_range {
                            has_previous_original_statement = true;
                        }
                        self.emit_node_id(transformation, node.source(), statement.node(), writer)?;
                        self.emit_trailing_comments_for_node(transformation, statement, writer)?;
                        writer.write_line(false);
                    }
                    if let Some(last_statement) = last_statement
                        .and_then(|id| transformation.arena().node_ref(node.source(), id))
                    {
                        self.emit_comments_before_close_brace(
                            transformation,
                            node,
                            last_statement,
                            writer,
                        )?;
                    }
                    writer.decrease_indent();
                }
                writer.write_punctuation("}");
                Ok(())
            }
            _ if !changed => {
                self.write_original_without_leading_trivia(transformation, node, writer)
            }
            _ => Err(PrinterError::UnsupportedTransformedSyntax {
                node,
                kind: record.kind,
            }),
        }
    }

    fn is_prologue_statement(
        &self,
        transformation: &TransformationResult<'_>,
        statement: TransformNode,
    ) -> bool {
        let Ok(record) = transformation.arena().node(statement) else {
            return false;
        };
        let NodeData::ExpressionStatement(data) = &record.data else {
            return false;
        };
        data.expression
            .and_then(|expression| {
                transformation
                    .arena()
                    .node_ref(statement.source(), expression)
            })
            .and_then(|expression| transformation.arena().node(expression).ok())
            .is_some_and(|expression| matches!(expression.data, NodeData::StringLiteral(_)))
    }

    fn emit_helpers(
        &self,
        helpers: &[EmitHelper],
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        for helper in helpers {
            let text = helper
                .text()
                .ok_or_else(|| PrinterError::EmitHelperTextUnavailable(helper.name().into()))?;
            for line in text.lines() {
                writer.write(line);
                writer.write_line(false);
            }
        }
        Ok(())
    }

    fn emit_case_block(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        clauses: Option<tsc_syntax::NodeArrayId>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        writer.write_punctuation("{");
        let clauses = clauses
            .and_then(|array| transformation.arena().node_array_ref(source, array))
            .map(|array| transformation.arena().node_array(array))
            .transpose()?
            .map(|array| array.nodes.clone())
            .unwrap_or_default();
        if clauses.is_empty() {
            writer.write_space(" ");
        } else {
            writer.write_line(false);
            writer.increase_indent();
            for (index, clause) in clauses.into_iter().enumerate() {
                let clause = transformation
                    .arena()
                    .node_ref(source, clause)
                    .ok_or(PrinterError::UnknownStatement(clause.0))?;
                if index == 0 {
                    self.emit_leading_comments_for_node(transformation, clause, writer)?;
                } else {
                    self.emit_leading_comments_for_node_after_sibling(
                        transformation,
                        clause,
                        writer,
                    )?;
                }
                self.emit_node_id(transformation, source, clause.node(), writer)?;
                let clause_has_statements = match &transformation.arena().node(clause)?.data {
                    NodeData::CaseClause(data) => data.statements.is_some_and(|statements| {
                        transformation
                            .arena()
                            .node_array_ref(source, statements)
                            .and_then(|statements| {
                                transformation.arena().node_array(statements).ok()
                            })
                            .is_some_and(|statements| !statements.nodes.is_empty())
                    }),
                    NodeData::DefaultClause(data) => data.statements.is_some_and(|statements| {
                        transformation
                            .arena()
                            .node_array_ref(source, statements)
                            .and_then(|statements| {
                                transformation.arena().node_array(statements).ok()
                            })
                            .is_some_and(|statements| !statements.nodes.is_empty())
                    }),
                    _ => false,
                };
                // The final statement already owns the clause's same-line
                // trailing comment. Emitting the clause range again duplicates
                // that comment immediately before the next case/default arm.
                if !clause_has_statements {
                    self.emit_trailing_comments_for_node(transformation, clause, writer)?;
                }
                writer.write_line(false);
            }
            writer.decrease_indent();
        }
        writer.write_punctuation("}");
        Ok(())
    }

    fn emit_case_clause_statements(
        &self,
        transformation: &mut TransformationResult<'_>,
        clause: TransformNode,
        source: TransformSourceId,
        statements: Option<tsc_syntax::NodeArrayId>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let statements = statements
            .and_then(|array| transformation.arena().node_array_ref(source, array))
            .map(|array| transformation.arena().node_array(array))
            .transpose()?
            .map(|array| array.nodes.clone())
            .unwrap_or_default();
        if statements.is_empty() {
            return Ok(());
        }
        let first = transformation
            .arena()
            .node_ref(source, statements[0])
            .ok_or(PrinterError::UnknownStatement(statements[0].0))?;
        let single_line = statements.len() == 1
            && self.source_nodes_start_on_same_line(transformation, clause, first)?;
        if single_line {
            writer.write_space(" ");
            self.emit_leading_comments_for_node(transformation, first, writer)?;
            self.emit_node_id(transformation, source, first.node(), writer)?;
            self.emit_trailing_comments_for_node(transformation, first, writer)?;
            return Ok(());
        }
        writer.write_line(false);
        writer.increase_indent();
        for (index, statement) in statements.into_iter().enumerate() {
            let statement = transformation
                .arena()
                .node_ref(source, statement)
                .ok_or(PrinterError::UnknownStatement(statement.0))?;
            if index == 0 {
                self.emit_leading_comments_for_node(transformation, statement, writer)?;
            } else {
                self.emit_leading_comments_for_node_after_sibling(
                    transformation,
                    statement,
                    writer,
                )?;
            }
            self.emit_node_id(transformation, source, statement.node(), writer)?;
            self.emit_trailing_comments_for_node(transformation, statement, writer)?;
            writer.write_line(false);
        }
        writer.decrease_indent();
        Ok(())
    }

    fn source_nodes_start_on_same_line(
        &self,
        transformation: &TransformationResult<'_>,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<bool, PrinterError> {
        let left = transformation.arena().get_original_node(left);
        let right = transformation.arena().get_original_node(right);
        let source = transformation.arena().source(left.source())?.syntax();
        let left_record = transformation.arena().node(left)?;
        let right_record = transformation.arena().node(right)?;
        let SourceRange::Original(left_range) =
            SourceRange::from_raw(left_record.pos, left_record.end, source.positions())?
        else {
            return Ok(true);
        };
        let SourceRange::Original(right_range) =
            SourceRange::from_raw(right_record.pos, right_record.end, source.positions())?
        else {
            return Ok(true);
        };
        let left_start = skip_trivia(source.text(), left_range.start().value() as usize);
        let right_start = skip_trivia(source.text(), right_range.start().value() as usize);
        if left_start > right_start || right_start > source.text().len() {
            return Ok(false);
        }
        Ok(!source.text()[left_start..right_start]
            .bytes()
            .any(|byte| matches!(byte, b'\r' | b'\n')))
    }

    fn source_node_end_and_node_start_are_on_same_line(
        &self,
        transformation: &TransformationResult<'_>,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<bool, PrinterError> {
        let left = transformation.arena().get_original_node(left);
        let right = transformation.arena().get_original_node(right);
        if left.source() != right.source() {
            return Ok(true);
        }
        let source = transformation.arena().source(left.source())?.syntax();
        let left_record = transformation.arena().node(left)?;
        let right_record = transformation.arena().node(right)?;
        let (SourceRange::Original(left_range), SourceRange::Original(right_range)) = (
            SourceRange::from_raw(left_record.pos, left_record.end, source.positions())?,
            SourceRange::from_raw(right_record.pos, right_record.end, source.positions())?,
        ) else {
            return Ok(true);
        };
        let left_end = left_range.end().value() as usize;
        let right_start = skip_trivia(source.text(), right_range.start().value() as usize);
        if left_end > right_start || right_start > source.text().len() {
            return Ok(true);
        }
        Ok(!source.text()[left_end..right_start]
            .bytes()
            .any(|byte| matches!(byte, b'\r' | b'\n')))
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_delimited_expression_list(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        elements: Option<tsc_syntax::NodeArrayId>,
        open: &str,
        close: &str,
        multi_line: bool,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        writer.write_punctuation(open);
        let array = elements.and_then(|id| transformation.arena().node_array_ref(source, id));
        let (ids, trailing_comma) = if let Some(array) = array {
            let record = transformation.arena().node_array(array)?;
            (record.nodes.clone(), record.has_trailing_comma)
        } else {
            (Vec::new(), false)
        };
        if ids.is_empty() {
            writer.write_punctuation(close);
            return Ok(());
        }

        if multi_line {
            writer.write_line(false);
            writer.increase_indent();
            let count = ids.len();
            for (index, id) in ids.into_iter().enumerate() {
                let child = transformation
                    .arena()
                    .node_ref(source, id)
                    .ok_or(PrinterError::UnknownStatement(id.0))?;
                if index == 0 {
                    self.emit_leading_comments_for_node(transformation, child, writer)?;
                } else {
                    self.emit_leading_comments_for_node_after_sibling(
                        transformation,
                        child,
                        writer,
                    )?;
                }
                self.emit_node_id(transformation, source, id, writer)?;
                if index + 1 < count || trailing_comma {
                    writer.write_punctuation(",");
                }
                self.emit_delimited_trailing_comments_for_node(transformation, child, writer)?;
                writer.write_line(false);
            }
            writer.decrease_indent();
        } else {
            let space_between_braces = open == "{";
            if space_between_braces {
                writer.write_space(" ");
            }
            let count = ids.len();
            for (index, id) in ids.into_iter().enumerate() {
                if index != 0 {
                    writer.write_punctuation(",");
                    writer.write_space(" ");
                }
                self.emit_node_id(transformation, source, id, writer)?;
                if index + 1 == count && trailing_comma {
                    writer.write_punctuation(",");
                }
            }
            if space_between_braces {
                writer.write_space(" ");
            }
        }
        writer.write_punctuation(close);
        Ok(())
    }

    /// tsc-port: emitNodeListItems @6.0.3
    /// tsc-hash: 8b9d9ba40ccad81aa5e0a79b002bc7be89f4a18456ebba923d01aefdfb315901
    /// tsc-span: _tsc.js:120068-120360
    ///
    /// This is the JSON subset of the `PreserveLines | Indented` list
    /// formats used by array and object literals. Object trailing commas are
    /// suppressed for a JSON SourceFile; array trailing commas remain the
    /// upstream parser/printer behavior.
    #[allow(clippy::too_many_arguments)]
    fn emit_json_delimited_expression_list(
        &self,
        transformation: &mut TransformationResult<'_>,
        parent: TransformNode,
        elements: Option<tsc_syntax::NodeArrayId>,
        open: &str,
        close: &str,
        prefer_new_line: bool,
        allow_trailing_comma: bool,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        writer.write_punctuation(open);
        let array =
            elements.and_then(|id| transformation.arena().node_array_ref(parent.source(), id));
        let (ids, trailing_comma) = if let Some(array) = array {
            let record = transformation.arena().node_array(array)?;
            (record.nodes.clone(), record.has_trailing_comma)
        } else {
            (Vec::new(), false)
        };
        if ids.is_empty() {
            writer.write_punctuation(close);
            return Ok(());
        }

        let first = transformation
            .arena()
            .node_ref(parent.source(), ids[0])
            .ok_or(PrinterError::UnknownStatement(ids[0].0))?;
        let leading_line = prefer_new_line
            || self.json_node_start_line(transformation, parent)?
                != self.json_node_start_line(transformation, first)?;
        if leading_line {
            writer.write_line(false);
        } else if open == "{" {
            writer.write_space(" ");
        }
        writer.increase_indent();

        for (index, id) in ids.iter().copied().enumerate() {
            let child = transformation
                .arena()
                .node_ref(parent.source(), id)
                .ok_or(PrinterError::UnknownStatement(id.0))?;
            if index == 0 {
                self.emit_leading_comments_for_node(transformation, child, writer)?;
            } else {
                self.emit_leading_comments_for_node_after_sibling(transformation, child, writer)?;
            }
            self.emit_node_id(transformation, parent.source(), id, writer)?;

            let has_next = index + 1 < ids.len();
            if has_next || trailing_comma && allow_trailing_comma {
                writer.write_punctuation(",");
            }
            self.emit_delimited_trailing_comments_for_node(transformation, child, writer)?;
            if has_next {
                let next = transformation
                    .arena()
                    .node_ref(parent.source(), ids[index + 1])
                    .ok_or(PrinterError::UnknownStatement(ids[index + 1].0))?;
                if self.json_node_end_line(transformation, child)?
                    != self.json_node_start_line(transformation, next)?
                {
                    writer.write_line(false);
                } else {
                    writer.write_space(" ");
                }
            }
        }

        let last = transformation
            .arena()
            .node_ref(parent.source(), *ids.last().expect("nonempty JSON list"))
            .expect("validated JSON list child");
        let closing_line = prefer_new_line
            || self.json_node_end_line(transformation, parent)?
                != self.json_node_end_line(transformation, last)?;
        writer.decrease_indent();
        if closing_line {
            writer.write_line(false);
        } else if open == "{" {
            writer.write_space(" ");
        }
        writer.write_punctuation(close);
        Ok(())
    }

    fn json_node_start_line(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<u32, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Err(PrinterError::SyntheticNodeWorkerUnavailable(node));
        };
        let start = u32::try_from(skip_trivia(source.text(), range.start().value() as usize))
            .expect("JSON source position exceeds u32");
        Ok(SourceUtf16Location::from_byte(
            SourceBytePosition::new(start, source.positions())?,
            source.positions(),
        )?
        .line())
    }

    fn json_node_end_line(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<u32, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Err(PrinterError::SyntheticNodeWorkerUnavailable(node));
        };
        Ok(SourceUtf16Location::from_byte(range.end(), source.positions())?.line())
    }

    fn emit_embedded_statement(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        statement: Option<tsc_syntax::NodeId>,
        parent: SyntaxKind,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let statement = statement.ok_or(PrinterError::MissingTransformedChild {
            parent,
            field: "statement",
        })?;
        let statement_node = transformation
            .arena()
            .node_ref(source, statement)
            .ok_or(PrinterError::UnknownStatement(statement.0))?;
        let metadata = transformation.arena().metadata(statement_node).cloned();
        if transformation.arena().node(statement_node)?.kind == SyntaxKind::Block
            || metadata
                .is_some_and(|metadata| metadata.flags().contains(crate::EmitFlags::SINGLE_LINE))
        {
            writer.write_space(" ");
            self.emit_node_id(transformation, source, statement, writer)
        } else {
            writer.write_line(false);
            writer.increase_indent();
            self.emit_leading_comments_for_node(transformation, statement_node, writer)?;
            self.emit_node_id(transformation, source, statement, writer)?;
            writer.decrease_indent();
            Ok(())
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_class(
        &self,
        transformation: &mut TransformationResult<'_>,
        class_node: TransformNode,
        source: TransformSourceId,
        modifiers: Option<tsc_syntax::NodeArrayId>,
        name: Option<tsc_syntax::NodeId>,
        heritage_clauses: Option<tsc_syntax::NodeArrayId>,
        members: Option<tsc_syntax::NodeArrayId>,
        expression: bool,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let anonymous_default_declaration = !expression
            && modifiers
                .and_then(|array| transformation.arena().node_array_ref(source, array))
                .map(|array| transformation.arena().node_array(array))
                .transpose()?
                .is_some_and(|array| {
                    array.nodes.iter().any(|id| {
                        transformation
                            .arena()
                            .node_ref(source, *id)
                            .and_then(|modifier| transformation.arena().node(modifier).ok())
                            .is_some_and(|modifier| modifier.kind == SyntaxKind::DefaultKeyword)
                    })
                });
        if self.emit_modifiers(transformation, source, modifiers, writer)? {
            writer.write_space(" ");
        }
        writer.write_keyword("class");
        if let Some(name) = name {
            writer.write_space(" ");
            self.emit_identifier_name(transformation, source, name, writer)?;
        } else if !expression && !anonymous_default_declaration {
            return Err(PrinterError::MissingTransformedChild {
                parent: SyntaxKind::ClassDeclaration,
                field: "name",
            });
        }
        let indented = transformation
            .arena()
            .metadata(class_node)
            .is_some_and(|metadata| metadata.flags().contains(crate::EmitFlags::INDENTED));
        if indented {
            writer.increase_indent();
        }
        let has_heritage_clauses = heritage_clauses
            .and_then(|id| transformation.arena().node_array_ref(source, id))
            .map(|array| transformation.arena().node_array(array))
            .transpose()?
            .is_some_and(|array| !array.nodes.is_empty());
        if has_heritage_clauses {
            writer.write_space(" ");
            self.emit_node_array(transformation, source, heritage_clauses, " ", writer)?;
        }
        writer.write_space(" ");
        writer.write_punctuation("{");
        let member_array = members.and_then(|id| transformation.arena().node_array_ref(source, id));
        if let Some(member_array) = member_array {
            let member_ids = transformation
                .arena()
                .node_array(member_array)?
                .nodes
                .clone();
            if !member_ids.is_empty() {
                writer.write_line(false);
                writer.increase_indent();
                for (index, member) in member_ids.into_iter().enumerate() {
                    let member_node = transformation
                        .arena()
                        .node_ref(source, member)
                        .ok_or(PrinterError::UnknownStatement(member.0))?;
                    if index == 0 {
                        self.emit_leading_comments_for_node(transformation, member_node, writer)?;
                    } else {
                        self.emit_leading_comments_for_node_after_sibling(
                            transformation,
                            member_node,
                            writer,
                        )?;
                    }
                    self.emit_node_id(transformation, source, member, writer)?;
                    self.emit_trailing_comments_for_node(transformation, member_node, writer)?;
                    writer.write_line(false);
                }
                writer.decrease_indent();
            } else {
                writer.write_line(false);
            }
        } else {
            writer.write_line(false);
        }
        writer.write_punctuation("}");
        if indented {
            writer.decrease_indent();
        }
        Ok(())
    }

    fn emit_parameter_list(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        parameters: Option<tsc_syntax::NodeArrayId>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        writer.write_punctuation("(");
        self.emit_node_array(transformation, source, parameters, ", ", writer)?;
        writer.write_punctuation(")");
        Ok(())
    }

    fn source_gap_has_line_break(
        &self,
        transformation: &TransformationResult<'_>,
        source: TransformSourceId,
        left: tsc_syntax::NodeId,
        right: tsc_syntax::NodeId,
    ) -> Result<bool, PrinterError> {
        let left = transformation
            .arena()
            .node_ref(source, left)
            .ok_or(PrinterError::UnknownStatement(left.0))?;
        let right = transformation
            .arena()
            .node_ref(source, right)
            .ok_or(PrinterError::UnknownStatement(right.0))?;
        let left = transformation.arena().get_original_node(left);
        let right = transformation.arena().get_original_node(right);
        let syntax = transformation.arena().source(source)?.syntax();
        let SourceRange::Original(left_range) = SourceRange::from_raw(
            transformation.arena().node(left)?.pos,
            transformation.arena().node(left)?.end,
            syntax.positions(),
        )?
        else {
            return Ok(false);
        };
        let SourceRange::Original(right_range) = SourceRange::from_raw(
            transformation.arena().node(right)?.pos,
            transformation.arena().node(right)?.end,
            syntax.positions(),
        )?
        else {
            return Ok(false);
        };
        let start = left_range.end().value() as usize;
        let end = right_range.start().value() as usize;
        if start > end || end > syntax.text().len() {
            return Ok(false);
        }
        Ok(syntax.text()[start..end].contains('\r') || syntax.text()[start..end].contains('\n'))
    }

    fn simple_arrow_parameter(
        &self,
        transformation: &TransformationResult<'_>,
        arrow: TransformNode,
        data: &tsc_syntax::nodes::ArrowFunctionData,
    ) -> Result<Option<tsc_syntax::NodeId>, PrinterError> {
        if data.r#type.is_some() || data.modifiers.is_some() || data.type_parameters.is_some() {
            return Ok(None);
        }
        let Some(parameters) = data
            .parameters
            .and_then(|id| transformation.arena().node_array_ref(arrow.source(), id))
        else {
            return Ok(None);
        };
        let parameters = transformation.arena().node_array(parameters)?;
        if parameters.nodes.len() != 1 {
            return Ok(None);
        }
        let parameter_id = parameters.nodes[0];
        let parameter = transformation
            .arena()
            .node_ref(arrow.source(), parameter_id)
            .ok_or(PrinterError::UnknownStatement(parameter_id.0))?;
        let NodeData::Parameter(parameter_data) = &transformation.arena().node(parameter)?.data
        else {
            return Ok(None);
        };
        let simple_name = parameter_data
            .name
            .and_then(|id| transformation.arena().node_ref(arrow.source(), id))
            .is_some_and(|name| {
                transformation
                    .arena()
                    .node(name)
                    .is_ok_and(|name| name.kind == SyntaxKind::Identifier)
            });
        let arrow_pos = transformation.arena().node(arrow)?.pos;
        let parameter_pos = transformation.arena().node(parameter)?.pos;
        Ok((arrow_pos == parameter_pos
            && simple_name
            && parameter_data.modifiers.is_none()
            && parameter_data.dot_dot_dot_token.is_none()
            && parameter_data.question_token.is_none()
            && parameter_data.r#type.is_none()
            && parameter_data.initializer.is_none())
        .then_some(parameter_id))
    }

    fn emit_named_import_or_export_list(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        elements: Option<tsc_syntax::NodeArrayId>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        writer.write_punctuation("{");
        let non_empty = elements
            .and_then(|id| transformation.arena().node_array_ref(source, id))
            .is_some_and(|array| {
                transformation
                    .arena()
                    .node_array(array)
                    .is_ok_and(|array| !array.nodes.is_empty())
            });
        if non_empty {
            writer.write_space(" ");
            self.emit_node_array(transformation, source, elements, ", ", writer)?;
            writer.write_space(" ");
        }
        writer.write_punctuation("}");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_renamed_specifier(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        property_name: Option<tsc_syntax::NodeId>,
        name: Option<tsc_syntax::NodeId>,
        parent: SyntaxKind,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if let Some(property_name) = property_name {
            self.emit_identifier_name(transformation, source, property_name, writer)?;
            writer.write_space(" ");
            writer.write_keyword("as");
            writer.write_space(" ");
        }
        self.emit_required_identifier_name(transformation, source, name, parent, "name", writer)
    }

    fn emit_modifiers(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        modifiers: Option<tsc_syntax::NodeArrayId>,
        writer: &mut TextWriter,
    ) -> Result<bool, PrinterError> {
        let Some(array) =
            modifiers.and_then(|id| transformation.arena().node_array_ref(source, id))
        else {
            return Ok(false);
        };
        let items = transformation
            .arena()
            .node_array(array)?
            .nodes
            .iter()
            .copied()
            .map(|node| {
                let kind = transformation
                    .arena()
                    .node(TransformNode::new(source, node))?
                    .kind;
                Ok(ModifierListItem {
                    node,
                    kind: ModifierListItemKind::from_syntax_kind(kind),
                })
            })
            .collect::<Result<Vec<_>, TransformError>>()?;

        for (index, item) in items.iter().enumerate() {
            match item.kind {
                ModifierListItemKind::Decorator => {
                    writer.write_line(false);
                    self.emit_node_id(transformation, source, item.node, writer)?;
                    writer.write_line(false);
                }
                ModifierListItemKind::Modifier => {
                    if index != 0 && items[index - 1].kind == ModifierListItemKind::Modifier {
                        writer.write_space(" ");
                    }
                    self.emit_node_id(transformation, source, item.node, writer)?;
                    if items
                        .get(index + 1)
                        .is_some_and(|next| next.kind == ModifierListItemKind::Decorator)
                    {
                        writer.write_space(" ");
                    }
                }
            }
        }

        Ok(items
            .last()
            .is_some_and(|item| item.kind == ModifierListItemKind::Modifier))
    }

    fn parenthesized_no_asi_expression(
        &self,
        transformation: &TransformationResult<'_>,
        expression: TransformNode,
    ) -> Result<Option<ParenthesizedNoAsiExpression>, PrinterError> {
        let NodeData::PartiallyEmittedExpression(data) =
            &transformation.arena().node(expression)?.data
        else {
            return Ok(None);
        };
        let Some(inner) = data.expression else {
            return Ok(None);
        };
        let original = transformation
            .arena()
            .metadata(expression)
            .and_then(crate::EmitMetadata::original)
            .unwrap_or_else(|| transformation.arena().get_original_node(expression));
        if transformation.arena().node(original)?.kind != SyntaxKind::ParenthesizedExpression {
            return Ok(None);
        }

        let comment_anchor = TransformNode::new(expression.source(), inner);
        let original_anchor = transformation
            .arena()
            .metadata(comment_anchor)
            .and_then(crate::EmitMetadata::original)
            .unwrap_or_else(|| transformation.arena().get_original_node(comment_anchor));
        let source = transformation
            .arena()
            .source(original_anchor.source())?
            .syntax();
        let record = transformation.arena().node(original_anchor)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(None);
        };
        let start = range.start().value() as usize;
        let code_start = skip_trivia(source.text(), start);
        let leading = &source.text()[start..code_start];
        if !leading
            .as_bytes()
            .windows(2)
            .any(|pair| pair == b"/*" || pair == b"//")
        {
            return Ok(None);
        }

        Ok(Some(ParenthesizedNoAsiExpression {
            inner,
            comment_anchor,
        }))
    }

    fn emit_node_array(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        array: Option<tsc_syntax::NodeArrayId>,
        separator: &str,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let Some(array) = array.and_then(|id| transformation.arena().node_array_ref(source, id))
        else {
            return Ok(());
        };
        let ids = transformation.arena().node_array(array)?.nodes.clone();
        for (index, id) in ids.into_iter().enumerate() {
            if index != 0 {
                writer.write(separator);
            }
            self.emit_node_id(transformation, source, id, writer)?;
        }
        Ok(())
    }

    fn emit_call_arguments(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        arguments: Option<tsc_syntax::NodeArrayId>,
        multi_line: bool,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let ids = arguments
            .and_then(|id| transformation.arena().node_array_ref(source, id))
            .map(|array| transformation.arena().node_array(array))
            .transpose()?
            .map(|array| array.nodes.clone())
            .unwrap_or_default();
        let mut increased_indent = false;
        for (index, id) in ids.iter().copied().enumerate() {
            let node = TransformNode::new(source, id);
            if index == 0 {
                self.emit_leading_comments_for_node(transformation, node, writer)?;
            } else {
                writer.write_punctuation(",");
                self.emit_delimited_trailing_comments_for_node(
                    transformation,
                    TransformNode::new(source, ids[index - 1]),
                    writer,
                )?;
                if multi_line && index >= 2 {
                    writer.write_line(false);
                    if index == 2 {
                        writer.increase_indent();
                        increased_indent = true;
                    }
                } else {
                    writer.write_space(" ");
                }
            }
            self.emit_node_id(transformation, source, id, writer)?;
        }
        if increased_indent {
            writer.decrease_indent();
        }
        Ok(())
    }

    fn emit_required_node(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: Option<tsc_syntax::NodeId>,
        parent: SyntaxKind,
        field: &'static str,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_required_node_with_context(
            transformation,
            source,
            id,
            parent,
            field,
            ExpressionEmissionContext::Normal,
            writer,
        )
    }

    fn emit_required_identifier_name(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: Option<NodeId>,
        parent: SyntaxKind,
        field: &'static str,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let id = id.ok_or(PrinterError::MissingTransformedChild { parent, field })?;
        self.emit_identifier_name(transformation, source, id, writer)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_required_node_with_context(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: Option<NodeId>,
        parent: SyntaxKind,
        field: &'static str,
        expression_context: ExpressionEmissionContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let id = id.ok_or(PrinterError::MissingTransformedChild { parent, field })?;
        self.emit_node_id_with_context(transformation, source, id, expression_context, writer)
    }

    fn emit_node_id(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: tsc_syntax::NodeId,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_node_id_with_context(
            transformation,
            source,
            id,
            ExpressionEmissionContext::Normal,
            writer,
        )
    }

    fn emit_node_id_with_context(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: NodeId,
        expression_context: ExpressionEmissionContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let node = transformation
            .arena()
            .node_ref(source, id)
            .ok_or(PrinterError::UnknownStatement(id.0))?;
        let hint = if transformation.arena().node(node)?.kind == SyntaxKind::Identifier {
            EmitHint::Expression
        } else {
            EmitHint::Unspecified
        };
        self.emit_node_with_hint(transformation, node, hint, expression_context, writer)
    }

    fn emit_identifier_name(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: NodeId,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let node = transformation
            .arena()
            .node_ref(source, id)
            .ok_or(PrinterError::UnknownStatement(id.0))?;
        let hint = if transformation.arena().node(node)?.kind == SyntaxKind::Identifier {
            EmitHint::IdentifierName
        } else {
            EmitHint::Unspecified
        };
        self.emit_node_with_hint(
            transformation,
            node,
            hint,
            ExpressionEmissionContext::Normal,
            writer,
        )
    }

    fn emit_node_with_hint(
        &self,
        transformation: &mut TransformationResult<'_>,
        node: TransformNode,
        hint: EmitHint,
        expression_context: ExpressionEmissionContext,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let substituted = transformation.substitute_node(hint, node)?;
        self.emit_synthetic_leading_comments_for_node(transformation, substituted, writer)?;
        self.emit_transformed_node(transformation, substituted, expression_context, writer)?;
        self.emit_synthetic_trailing_comments_for_node(transformation, substituted, writer)
    }

    fn write_original_without_leading_trivia(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        let range = SourceRange::from_raw(record.pos, record.end, source.positions())?;
        let SourceRange::Original(range) = range else {
            return Err(PrinterError::SyntheticNodeWorkerUnavailable(node));
        };
        let start = u32::try_from(skip_trivia(source.text(), range.start().value() as usize))
            .expect("source trivia position exceeds u32");
        let end = range.end().value();
        let slice = source
            .text()
            .get(start as usize..end as usize)
            .ok_or(PrinterError::InvalidTextSlice { start, end })?;
        let normalized = normalize_new_lines(slice, self.options.new_line.text());
        writer.write(&normalized);
        Ok(())
    }

    fn write_original_without_leading_trivia_verbatim(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Err(PrinterError::SyntheticNodeWorkerUnavailable(node));
        };
        let start = skip_trivia(source.text(), range.start().value() as usize);
        let end = range.end().value() as usize;
        let slice = source
            .text()
            .get(start..end)
            .ok_or(PrinterError::InvalidTextSlice {
                start: u32::try_from(start).expect("source position exceeds u32"),
                end: u32::try_from(end).expect("source position exceeds u32"),
            })?;
        writer.write(slice);
        Ok(())
    }

    fn write_original_module_statement(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Err(PrinterError::SyntheticNodeWorkerUnavailable(node));
        };
        let ends_with_semicolon = source.text()
            [range.start().value() as usize..range.end().value() as usize]
            .trim_end()
            .ends_with(';');
        self.write_original_without_leading_trivia(transformation, node, writer)?;
        if !ends_with_semicolon {
            writer.write_trailing_semicolon(";");
        }
        Ok(())
    }

    fn original_node_has_internal_comments(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<bool, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(false);
        };
        let text = source.text();
        let start = u32::try_from(skip_trivia(text, range.start().value() as usize))
            .expect("source trivia position exceeds u32");
        let end = range.end().value();
        let mut cursor = start;
        for token in scan_tokens(text, source.language_variant) {
            let token_start = source.positions().utf16_to_byte(token.start).ok_or(
                PrinterError::TokenPositionNotScalarBoundary {
                    position: token.start,
                },
            )?;
            let token_end = source.positions().utf16_to_byte(token.end).ok_or(
                PrinterError::TokenPositionNotScalarBoundary {
                    position: token.end,
                },
            )?;
            if token_end <= start {
                continue;
            }
            if token_start >= end {
                break;
            }
            let gap_end = token_start.min(end);
            if cursor < gap_end
                && text.as_bytes()[cursor as usize..gap_end as usize]
                    .windows(2)
                    .any(|pair| pair == b"/*" || pair == b"//")
            {
                return Ok(true);
            }
            cursor = cursor.max(token_end.min(end));
        }
        Ok(cursor < end
            && text.as_bytes()[cursor as usize..end as usize]
                .windows(2)
                .any(|pair| pair == b"/*" || pair == b"//"))
    }

    fn emit_leading_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_leading_comments_for_node_worker(transformation, node, false, 0, writer)
    }

    fn emit_leading_comments_for_node_after_sibling(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        self.emit_leading_comments_for_node_worker(transformation, node, true, 0, writer)
    }

    fn emit_leading_comments_for_node_worker(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        after_sibling: bool,
        skipped_prefix_bytes: usize,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments
            || transformation
                .arena()
                .metadata(node)
                .is_some_and(|metadata| metadata.flags().intersects(EmitFlags::NO_LEADING_COMMENTS))
        {
            return Ok(());
        }
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        let start = range.start().value() as usize;
        let code_start = skip_trivia(source.text(), start);
        if code_start > start {
            let trivia = &source.text()[start..code_start];
            let mut trivia =
                trivia
                    .get(skipped_prefix_bytes..)
                    .ok_or(PrinterError::InvalidTextSlice {
                        start: u32::try_from(start + skipped_prefix_bytes).unwrap_or(u32::MAX),
                        end: u32::try_from(code_start).unwrap_or(u32::MAX),
                    })?;
            if after_sibling
                || start > 0
                    && matches!(
                        source.text().as_bytes()[start - 1],
                        b';' | b',' | b'{' | b'}' | b')'
                    )
            {
                trivia = strip_same_line_comment_prefix(trivia);
            }
            emit_leading_comments(trivia, writer);
        }
        Ok(())
    }

    fn detached_source_prefix(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<Option<DetachedSourcePrefix>, PrinterError> {
        if self.options.remove_comments {
            return Ok(None);
        }
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(None);
        };
        let start = range.start().value() as usize;
        let code_start = skip_trivia(source.text(), start);
        Ok(
            detached_leading_trivia(&source.text()[start..code_start]).map(|detached| {
                DetachedSourcePrefix {
                    anchor: original,
                    byte_len: detached.len(),
                }
            }),
        )
    }

    /// Mirrors the source-file branch of tsc's
    /// `emitBodyWithDetachedComments`: the transformed statement array owns
    /// the detached prefix only while it still has a parsed range. A parsed
    /// leading directive owns its own comments; a synthesized directive lets
    /// the source-file body own them after the directive has been emitted.
    fn source_file_owns_detached_prefix(
        &self,
        transformation: &TransformationResult<'_>,
        statements: Option<TransformNodeArray>,
        first_statement: Option<NodeId>,
    ) -> Result<bool, PrinterError> {
        let Some(statements) = statements else {
            return Ok(false);
        };
        let source = transformation.arena().source(statements.source())?.syntax();
        let statement_array = transformation.arena().node_array(statements)?;
        if !matches!(
            SourceRange::from_raw(statement_array.pos, statement_array.end, source.positions())?,
            SourceRange::Original(_)
        ) {
            return Ok(false);
        }
        let Some(first_statement) = first_statement.and_then(|statement| {
            transformation
                .arena()
                .node_ref(statements.source(), statement)
        }) else {
            return Ok(true);
        };
        if !self.is_prologue_statement(transformation, first_statement) {
            return Ok(true);
        }
        let first = transformation.arena().node(first_statement)?;
        Ok(matches!(
            SourceRange::from_raw(first.pos, first.end, source.positions())?,
            SourceRange::Synthesized
        ))
    }

    fn emit_detached_source_prefix(
        &self,
        transformation: &TransformationResult<'_>,
        prefix: Option<DetachedSourcePrefix>,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let Some(prefix) = prefix else {
            return Ok(());
        };
        let source = transformation
            .arena()
            .source(prefix.anchor.source())?
            .syntax();
        let record = transformation.arena().node(prefix.anchor)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        let start = range.start().value() as usize;
        let end = start.saturating_add(prefix.byte_len);
        let detached = source
            .text()
            .get(start..end)
            .ok_or(PrinterError::InvalidTextSlice {
                start: u32::try_from(start).unwrap_or(u32::MAX),
                end: u32::try_from(end).unwrap_or(u32::MAX),
            })?;
        emit_leading_comments(detached, writer);
        Ok(())
    }

    fn emit_trailing_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments
            || transformation
                .arena()
                .metadata(node)
                .is_some_and(|metadata| {
                    metadata.flags().intersects(EmitFlags::NO_TRAILING_COMMENTS)
                })
        {
            return Ok(());
        }
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        emit_same_line_trailing_comments(&source.text()[range.end().value() as usize..], writer);
        Ok(())
    }

    fn emit_trailing_block_comments_before_semicolon(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments
            || transformation
                .arena()
                .metadata(node)
                .is_some_and(|metadata| {
                    metadata.flags().intersects(EmitFlags::NO_TRAILING_COMMENTS)
                })
        {
            return Ok(());
        }
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        emit_same_line_trailing_block_comments(
            &source.text()[range.end().value() as usize..],
            writer,
        );
        Ok(())
    }

    fn emit_synthetic_leading_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let comments = transformation
            .arena()
            .metadata(node)
            .map(|metadata| metadata.leading_comments().to_vec())
            .unwrap_or_default();
        for comment in comments {
            if comment.has_leading_new_line() {
                writer.write_line(false);
            }
            write_synthetic_comment(&comment, writer);
            if comment.has_trailing_new_line() {
                writer.write_line(false);
            } else {
                writer.write_space(" ");
            }
        }
        Ok(())
    }

    fn emit_synthetic_trailing_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let comments = transformation
            .arena()
            .metadata(node)
            .map(|metadata| metadata.trailing_comments().to_vec())
            .unwrap_or_default();
        for comment in comments {
            if comment.has_leading_new_line() {
                writer.write_line(false);
            } else {
                writer.write_space(" ");
            }
            write_synthetic_comment(&comment, writer);
            if comment.has_trailing_new_line() {
                writer.write_line(false);
            }
        }
        Ok(())
    }

    fn emit_source_file_trailing_comments(
        &self,
        transformation: &TransformationResult<'_>,
        last_statement: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let source = transformation
            .arena()
            .source(last_statement.source())?
            .syntax();
        let record = transformation.arena().node(last_statement)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        let mut start = range.end().value() as usize;
        if source.text().as_bytes().get(start) == Some(&b';') {
            start += 1;
        }
        let tail = strip_same_line_comment_prefix(&source.text()[start..]);
        if skip_trivia(tail, 0) == tail.len()
            && tail
                .as_bytes()
                .windows(2)
                .any(|pair| pair == b"/*" || pair == b"//")
        {
            emit_leading_comments(tail, writer);
        }
        Ok(())
    }

    fn emit_comment_after_open_brace(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        let start = skip_trivia(source.text(), range.start().value() as usize);
        let end = range.end().value() as usize;
        if start < end && source.text().as_bytes()[start] == b'{' {
            emit_same_line_trailing_comments(&source.text()[start + 1..end], writer);
        }
        Ok(())
    }

    fn emit_empty_block_comments(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        multi_line: bool,
        function_body: bool,
        writer: &mut TextWriter,
    ) -> Result<bool, PrinterError> {
        if self.options.remove_comments {
            return Ok(false);
        }
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(false);
        };
        let start = skip_trivia(source.text(), range.start().value() as usize);
        let end = range.end().value() as usize;
        if start >= end || source.text().as_bytes()[start] != b'{' {
            return Ok(false);
        }
        let inner_end = source.text()[start + 1..end]
            .rfind('}')
            .map_or(end, |offset| start + 1 + offset);
        let inner = &source.text()[start + 1..inner_end];
        let inner = if function_body {
            strip_same_line_comment_prefix(inner)
        } else {
            inner
        };
        if !inner
            .as_bytes()
            .windows(2)
            .any(|pair| pair == b"/*" || pair == b"//")
        {
            return Ok(false);
        }
        if multi_line {
            writer.write_line(false);
            writer.increase_indent();
            emit_leading_comments(inner, writer);
            writer.decrease_indent();
        } else {
            writer.write_space(" ");
            emit_leading_comments(inner, writer);
        }
        Ok(true)
    }

    fn is_function_body_block(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<bool, PrinterError> {
        let original = transformation.arena().get_original_node(node);
        let Some(parent) = transformation.arena().node(original)?.parent else {
            return Ok(false);
        };
        let parent = transformation
            .arena()
            .node_ref(original.source(), parent)
            .ok_or(PrinterError::UnknownStatement(parent.0))?;
        Ok(matches!(
            transformation.arena().node(parent)?.kind,
            SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::Constructor
                | SyntaxKind::ClassStaticBlockDeclaration
        ))
    }

    fn emit_comments_before_close_brace(
        &self,
        transformation: &TransformationResult<'_>,
        block: TransformNode,
        last_statement: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let original_block = transformation.arena().get_original_node(block);
        let original_statement = transformation.arena().get_original_node(last_statement);
        let source = transformation
            .arena()
            .source(original_block.source())?
            .syntax();
        let block_record = transformation.arena().node(original_block)?;
        let statement_record = transformation.arena().node(original_statement)?;
        let SourceRange::Original(block_range) =
            SourceRange::from_raw(block_record.pos, block_record.end, source.positions())?
        else {
            return Ok(());
        };
        let SourceRange::Original(statement_range) = SourceRange::from_raw(
            statement_record.pos,
            statement_record.end,
            source.positions(),
        )?
        else {
            return Ok(());
        };
        let block_end = block_range.end().value() as usize;
        let close = source.text()[block_range.start().value() as usize..block_end]
            .rfind('}')
            .map(|offset| block_range.start().value() as usize + offset)
            .unwrap_or(block_end);
        let start = statement_range.end().value() as usize;
        if start >= close {
            return Ok(());
        }
        let trivia = strip_same_line_comment_prefix(&source.text()[start..close]);
        if trivia
            .as_bytes()
            .windows(2)
            .any(|pair| pair == b"/*" || pair == b"//")
        {
            emit_leading_comments(trivia, writer);
        }
        Ok(())
    }

    fn emit_delimited_trailing_comments_for_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.options.remove_comments {
            return Ok(());
        }
        let original = transformation.arena().get_original_node(node);
        let source = transformation.arena().source(original.source())?.syntax();
        let record = transformation.arena().node(original)?;
        let SourceRange::Original(range) =
            SourceRange::from_raw(record.pos, record.end, source.positions())?
        else {
            return Ok(());
        };
        let rest = &source.text()[range.end().value() as usize..];
        let mut cursor = 0usize;
        while rest
            .as_bytes()
            .get(cursor)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            cursor += 1;
        }
        if rest.as_bytes().get(cursor) == Some(&b',') {
            cursor += 1;
        }
        emit_same_line_trailing_comments(&rest[cursor..], writer);
        Ok(())
    }

    fn node_range(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
    ) -> Result<SourceByteRange, PrinterError> {
        let source = transformation.arena().source(node.source())?.syntax();
        let record = transformation.arena().node(node)?;
        match SourceRange::from_raw(record.pos, record.end, source.positions())? {
            SourceRange::Original(range) => Ok(range),
            SourceRange::Synthesized => Err(PrinterError::SyntheticNodeWorkerUnavailable(node)),
        }
    }

    fn write_original_node(
        &self,
        transformation: &TransformationResult<'_>,
        node: TransformNode,
        original: OriginalNodeText<'_>,
        writer: &mut TextWriter,
        recorder: &mut dyn SourceMapRecorder,
    ) -> Result<(), PrinterError> {
        let flags = transformation
            .arena()
            .metadata(node)
            .map_or(EmitFlags::NONE, |metadata| metadata.flags());
        let mut cursor = original.range.start().value();
        for token in original.tokens.iter().filter(|token| {
            token.start >= original.range.start().value()
                && token.end <= original.range.end().value()
        }) {
            if token.start < cursor {
                continue;
            }
            raw_write_range(writer, original.text, cursor, token.start)?;
            if !flags.intersects(EmitFlags::NO_NESTED_SOURCE_MAPS) {
                self.record_token_hook(
                    transformation,
                    recorder,
                    SourceMapHookPhase::BeforeToken,
                    node,
                    *token,
                    writer,
                )?;
            }
            raw_write_range(writer, original.text, token.start, token.end)?;
            if !flags.intersects(EmitFlags::NO_NESTED_SOURCE_MAPS) {
                self.record_token_hook(
                    transformation,
                    recorder,
                    SourceMapHookPhase::AfterToken,
                    node,
                    *token,
                    writer,
                )?;
            }
            cursor = token.end;
        }
        raw_write_range(writer, original.text, cursor, original.range.end().value())
    }

    fn record_node_hook(
        &self,
        transformation: &TransformationResult<'_>,
        recorder: &mut dyn SourceMapRecorder,
        phase: SourceMapHookPhase,
        node: TransformNode,
        writer: &TextWriter,
    ) -> Result<(), PrinterError> {
        if !recorder.enabled() {
            return Ok(());
        }
        let arena = transformation.arena();
        let metadata = arena.metadata(node);
        let flags = metadata.map_or(EmitFlags::NONE, |metadata| metadata.flags());
        if phase == SourceMapHookPhase::BeforeNode
            && flags.intersects(EmitFlags::NO_LEADING_SOURCE_MAP)
            || phase == SourceMapHookPhase::AfterNode
                && flags.intersects(EmitFlags::NO_TRAILING_SOURCE_MAP)
        {
            return Ok(());
        }
        let default_range = {
            let source = arena.source(node.source())?.syntax();
            let record = arena.node(node)?;
            SourceMapRange::new(
                node.source(),
                SourceRange::from_raw(record.pos, record.end, source.positions())?,
            )
        };
        let range = metadata
            .and_then(|metadata| metadata.source_map_range())
            .unwrap_or(default_range);
        let SourceRange::Original(range_value) = range.range() else {
            return Ok(());
        };
        let mapped_source = arena.source(range.source())?.syntax();
        let raw = match phase {
            SourceMapHookPhase::BeforeNode => u32::try_from(skip_trivia(
                mapped_source.text(),
                range_value.start().value() as usize,
            ))
            .expect("source trivia position exceeds u32"),
            SourceMapHookPhase::AfterNode => range_value.end().value(),
            SourceMapHookPhase::BeforeToken | SourceMapHookPhase::AfterToken => unreachable!(),
        };
        let source_position = SourceBytePosition::new(raw, mapped_source.positions())?;
        recorder.record(SourceMapHookEvent {
            phase,
            node,
            token: None,
            source: range.source(),
            source_position,
            generated: writer.location(),
        });
        Ok(())
    }

    fn record_token_hook(
        &self,
        transformation: &TransformationResult<'_>,
        recorder: &mut dyn SourceMapRecorder,
        phase: SourceMapHookPhase,
        node: TransformNode,
        token: TokenSpan,
        writer: &TextWriter,
    ) -> Result<(), PrinterError> {
        if !recorder.enabled() {
            return Ok(());
        }
        let arena = transformation.arena();
        let metadata = arena.metadata(node);
        let flags = metadata.map_or(EmitFlags::NONE, |metadata| metadata.flags());
        if phase == SourceMapHookPhase::BeforeToken
            && flags.intersects(EmitFlags::NO_TOKEN_LEADING_SOURCE_MAPS)
            || phase == SourceMapHookPhase::AfterToken
                && flags.intersects(EmitFlags::NO_TOKEN_TRAILING_SOURCE_MAPS)
        {
            return Ok(());
        }
        let explicit = metadata
            .and_then(|metadata| metadata.token_source_map_ranges().get(&token.kind))
            .copied();
        let default = SourceMapRange::new(
            node.source(),
            SourceRange::Original(SourceByteRange::new(
                token.start,
                token.end,
                arena.source(node.source())?.syntax().positions(),
            )?),
        );
        let range = explicit.unwrap_or(default);
        let SourceRange::Original(range_value) = range.range() else {
            return Ok(());
        };
        let source = arena.source(range.source())?.syntax();
        let raw = match phase {
            SourceMapHookPhase::BeforeToken => u32::try_from(skip_trivia(
                source.text(),
                range_value.start().value() as usize,
            ))
            .expect("source trivia position exceeds u32"),
            SourceMapHookPhase::AfterToken => range_value.end().value(),
            SourceMapHookPhase::BeforeNode | SourceMapHookPhase::AfterNode => unreachable!(),
        };
        recorder.record(SourceMapHookEvent {
            phase,
            node,
            token: Some(token.kind),
            source: range.source(),
            source_position: SourceBytePosition::new(raw, source.positions())?,
            generated: writer.location(),
        });
        Ok(())
    }
}

fn emit_same_line_trailing_comments(rest: &str, writer: &mut TextWriter) {
    let bytes = rest.as_bytes();
    let mut cursor = 0usize;
    loop {
        while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
        }
        if cursor >= bytes.len() || matches!(bytes[cursor], b'\r' | b'\n') {
            return;
        }
        let (end, line_comment) = if bytes.get(cursor..cursor + 2) == Some(b"//") {
            let mut end = cursor + 2;
            while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
                end += 1;
            }
            (end, true)
        } else if bytes.get(cursor..cursor + 2) == Some(b"/*") {
            let mut end = cursor + 2;
            while end + 1 < bytes.len() && &bytes[end..end + 2] != b"*/" {
                end += 1;
            }
            ((end + 2).min(bytes.len()), false)
        } else {
            return;
        };
        if !writer.has_trailing_whitespace() {
            writer.write_space(" ");
        }
        write_comment_with_normalized_newlines(&rest[cursor..end], writer);
        cursor = end;
        if line_comment {
            return;
        }
    }
}

fn emit_same_line_trailing_block_comments(rest: &str, writer: &mut TextWriter) {
    let bytes = rest.as_bytes();
    let mut cursor = 0usize;
    loop {
        while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
        }
        if bytes.get(cursor..cursor + 2) != Some(b"/*") {
            return;
        }
        let start = cursor;
        cursor += 2;
        while cursor + 1 < bytes.len() && &bytes[cursor..cursor + 2] != b"*/" {
            cursor += 1;
        }
        cursor = (cursor + 2).min(bytes.len());
        if !writer.has_trailing_whitespace() {
            writer.write_space(" ");
        }
        write_comment_with_normalized_newlines(&rest[start..cursor], writer);
    }
}

fn strip_same_line_comment_prefix(trivia: &str) -> &str {
    let bytes = trivia.as_bytes();
    let mut cursor = 0usize;
    let mut found = false;
    loop {
        while cursor < bytes.len() && matches!(bytes[cursor], b' ' | b'\t') {
            cursor += 1;
        }
        if bytes.get(cursor..cursor + 2) == Some(b"//") {
            while cursor < bytes.len() && !matches!(bytes[cursor], b'\r' | b'\n') {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'\r' {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'\n' {
                cursor += 1;
            }
            return &trivia[cursor..];
        }
        if bytes.get(cursor..cursor + 2) == Some(b"/*") {
            found = true;
            cursor += 2;
            while cursor + 1 < bytes.len() && &bytes[cursor..cursor + 2] != b"*/" {
                cursor += 1;
            }
            cursor = (cursor + 2).min(bytes.len());
            continue;
        }
        return if found { &trivia[cursor..] } else { trivia };
    }
}

fn detached_leading_trivia(trivia: &str) -> Option<&str> {
    let bytes = trivia.as_bytes();
    let mut cursor = 0usize;
    let mut previous_line_end = None;
    while cursor < bytes.len() {
        let line_start = cursor;
        if bytes[cursor] == b'\r' {
            cursor += 1;
            if bytes.get(cursor) == Some(&b'\n') {
                cursor += 1;
            }
        } else if bytes[cursor] == b'\n' {
            cursor += 1;
        } else {
            cursor += trivia[cursor..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        if previous_line_end.is_some_and(|end| {
            bytes[end..line_start]
                .iter()
                .all(|byte| matches!(byte, b' ' | b'\t'))
        }) {
            let detached = &trivia[..cursor];
            if detached
                .as_bytes()
                .windows(2)
                .any(|pair| pair == b"/*" || pair == b"//")
            {
                return Some(detached);
            }
        }
        previous_line_end = Some(cursor);
    }
    None
}

/// tsc's `emitShebangIfNeeded` writes the source-file shebang before helpers,
/// prologue directives, and comments. It is not governed by removeComments.
fn source_shebang(text: &str) -> Option<&str> {
    text.strip_prefix("#!").map(|rest| {
        let end = rest
            .find(['\r', '\n', '\u{2028}', '\u{2029}'])
            .unwrap_or(rest.len());
        &text[..end + 2]
    })
}

fn emit_leading_comments(trivia: &str, writer: &mut TextWriter) {
    let bytes = trivia.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let whitespace_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let comment_follows = bytes.get(cursor..cursor + 2) == Some(b"//")
            || bytes.get(cursor..cursor + 2) == Some(b"/*");
        if comment_follows
            && (trivia[whitespace_start..cursor].contains('\r')
                || trivia[whitespace_start..cursor].contains('\n'))
            && !writer.is_at_start_of_line()
        {
            writer.write_line(false);
        }

        let (comment_end, line_comment) = if bytes.get(cursor..cursor + 2) == Some(b"//") {
            let mut end = cursor + 2;
            while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
                end += 1;
            }
            (end, true)
        } else if bytes.get(cursor..cursor + 2) == Some(b"/*") {
            let mut end = cursor + 2;
            while end + 1 < bytes.len() && &bytes[end..end + 2] != b"*/" {
                end += 1;
            }
            end = (end + 2).min(bytes.len());
            (end, false)
        } else {
            if cursor < bytes.len() {
                cursor += trivia[cursor..].chars().next().map_or(1, char::len_utf8);
            }
            continue;
        };

        write_comment_with_normalized_newlines(&trivia[cursor..comment_end], writer);
        cursor = comment_end;

        let gap_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let gap_has_line_break =
            trivia[gap_start..cursor].contains('\r') || trivia[gap_start..cursor].contains('\n');
        if line_comment || gap_has_line_break {
            writer.write_line(false);
        } else if gap_start < cursor || cursor < bytes.len() {
            writer.write_space(" ");
        }
    }
}

fn write_comment_with_normalized_newlines(comment: &str, writer: &mut TextWriter) {
    let normalized = comment.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.split('\n').peekable();
    while let Some(line) = lines.next() {
        writer.write_comment(line);
        if lines.peek().is_some() {
            writer.write_line(false);
        }
    }
}

fn normalize_new_lines(text: &str, new_line: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if new_line == "\n" {
        normalized
    } else {
        normalized.replace('\n', new_line)
    }
}

fn quote_string_literal(text: &str, single_quote: bool, no_ascii_escaping: bool) -> String {
    quote_javascript_string(
        &text.encode_utf16().collect::<Vec<_>>(),
        single_quote,
        no_ascii_escaping,
    )
}

fn quote_javascript_string(units: &[u16], single_quote: bool, no_ascii_escaping: bool) -> String {
    let quote = if single_quote { '\'' } else { '"' };
    let mut quoted = String::with_capacity(units.len() + 2);
    quoted.push(quote);
    let mut index = 0usize;
    while index < units.len() {
        let unit = units[index];
        if no_ascii_escaping
            && (0xd800..=0xdbff).contains(&unit)
            && units
                .get(index + 1)
                .is_some_and(|next| (0xdc00..=0xdfff).contains(next))
        {
            let next = units[index + 1];
            let scalar = 0x10000 + (((unit - 0xd800) as u32) << 10) + (next - 0xdc00) as u32;
            if let Some(character) = char::from_u32(scalar) {
                push_quoted_character(&mut quoted, character, quote, false);
            }
            index += 2;
            continue;
        }
        if !no_ascii_escaping && unit > 0x7f || (0xd800..=0xdfff).contains(&unit) {
            use std::fmt::Write;
            let _ = write!(quoted, "\\u{unit:04X}");
            index += 1;
            continue;
        }
        if let Some(character) = char::from_u32(unit as u32) {
            push_quoted_character(&mut quoted, character, quote, !no_ascii_escaping);
        }
        index += 1;
    }
    quoted.push(quote);
    quoted
}

fn push_quoted_character(
    quoted: &mut String,
    character: char,
    quote: char,
    escape_non_ascii: bool,
) {
    match character {
        character if character == quote => {
            quoted.push('\\');
            quoted.push(character);
        }
        '\\' => quoted.push_str("\\\\"),
        '\n' => quoted.push_str("\\n"),
        '\r' => quoted.push_str("\\r"),
        '\t' => quoted.push_str("\\t"),
        '\u{0008}' => quoted.push_str("\\b"),
        '\u{000c}' => quoted.push_str("\\f"),
        '\u{2028}' => quoted.push_str("\\u2028"),
        '\u{2029}' => quoted.push_str("\\u2029"),
        character if character < '\u{0020}' => {
            use std::fmt::Write;
            let _ = write!(quoted, "\\u{:04X}", character as u32);
        }
        character if escape_non_ascii && !character.is_ascii() => {
            for unit in character.encode_utf16(&mut [0; 2]) {
                use std::fmt::Write;
                let _ = write!(quoted, "\\u{unit:04X}");
            }
        }
        character => quoted.push(character),
    }
}

fn write_synthetic_comment(comment: &SyntheticComment, writer: &mut TextWriter) {
    match comment.kind() {
        SyntheticCommentKind::SingleLine => {
            writer.write_comment("//");
            writer.write_comment(comment.text());
        }
        SyntheticCommentKind::MultiLine => {
            writer.write_comment("/*");
            writer.write_comment(comment.text());
            writer.write_comment("*/");
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TokenSpan {
    kind: SyntaxKind,
    start: u32,
    end: u32,
}

#[derive(Clone, Copy, Debug)]
struct OriginalNodeText<'a> {
    range: SourceByteRange,
    text: &'a str,
    tokens: &'a [TokenSpan],
}

fn raw_write_range(
    writer: &mut TextWriter,
    text: &str,
    start: u32,
    end: u32,
) -> Result<(), PrinterError> {
    if start == end {
        return Ok(());
    }
    if start > end || end as usize > text.len() {
        return Err(PrinterError::InvalidTextSlice { start, end });
    }
    let slice = text
        .get(start as usize..end as usize)
        .ok_or(PrinterError::InvalidTextSlice { start, end })?;
    writer.raw_write(slice);
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrinterError {
    Unsupported(UnsupportedEmitFeature),
    OptionUnavailable(&'static str),
    Transform(TransformError),
    Position(SourcePositionError),
    SourceIsNotATransformedRoot(TransformSourceId),
    RootIsNotSourceFile(TransformNode),
    UnknownStatement(u32),
    SyntheticNodeWorkerUnavailable(TransformNode),
    TransformedNodeWorkerUnavailable(TransformNode),
    UnsupportedTransformedSyntax {
        node: TransformNode,
        kind: SyntaxKind,
    },
    MissingTransformedChild {
        parent: SyntaxKind,
        field: &'static str,
    },
    OverlappingSourceRange {
        previous_end: u32,
        start: u32,
    },
    InvalidTextSlice {
        start: u32,
        end: u32,
    },
    TokenPositionNotScalarBoundary {
        position: u32,
    },
    EmitHelperTextUnavailable(Box<str>),
}

impl From<TransformError> for PrinterError {
    fn from(value: TransformError) -> Self {
        Self::Transform(value)
    }
}

impl From<SourcePositionError> for PrinterError {
    fn from(value: SourcePositionError) -> Self {
        Self::Position(value)
    }
}

impl fmt::Display for PrinterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(feature) => {
                write!(formatter, "unsupported printer request: {}", feature.name())
            }
            Self::OptionUnavailable(option) => {
                write!(formatter, "printer option {option} is not active in H1.2")
            }
            Self::Transform(error) => error.fmt(formatter),
            Self::Position(error) => error.fmt(formatter),
            Self::SourceIsNotATransformedRoot(source) => write!(
                formatter,
                "transform source {} is not a completed root",
                source.raw()
            ),
            Self::RootIsNotSourceFile(node) => write!(
                formatter,
                "transform root {}:{} is not a SourceFile",
                node.source().raw(),
                node.node().0
            ),
            Self::UnknownStatement(node) => {
                write!(formatter, "source-file statement node {node} is unknown")
            }
            Self::SyntheticNodeWorkerUnavailable(node) => write!(
                formatter,
                "synthetic node {}:{} requires the H1.3 node worker",
                node.source().raw(),
                node.node().0
            ),
            Self::TransformedNodeWorkerUnavailable(node) => write!(
                formatter,
                "transformed node {}:{} requires the H1.3 node worker",
                node.source().raw(),
                node.node().0
            ),
            Self::UnsupportedTransformedSyntax { node, kind } => write!(
                formatter,
                "transformed {kind:?} node {}:{} has no active H1 printer worker",
                node.source().raw(),
                node.node().0
            ),
            Self::MissingTransformedChild { parent, field } => {
                write!(formatter, "transformed {parent:?} is missing child {field}")
            }
            Self::OverlappingSourceRange {
                previous_end,
                start,
            } => write!(
                formatter,
                "source statement range starts at {start} before prior end {previous_end}"
            ),
            Self::InvalidTextSlice { start, end } => {
                write!(formatter, "invalid source text slice {start}..{end}")
            }
            Self::TokenPositionNotScalarBoundary { position } => write!(
                formatter,
                "UTF-16 token position {position} does not map to a source scalar boundary"
            ),
            Self::EmitHelperTextUnavailable(helper) => {
                write!(formatter, "emit helper {helper} has no printable text")
            }
        }
    }
}

impl Error for PrinterError {}
