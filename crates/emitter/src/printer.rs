use std::error::Error;
use std::fmt;

use tsc_syntax::{scan_tokens, skip_trivia, NodeData, SyntaxKind};

use crate::{
    create_text_writer, EmitFlags, EmitHint, GeneratedUtf16Location, NewLineKind,
    SourceBytePosition, SourceByteRange, SourceMapRange, SourcePositionError, SourceRange,
    TextWriter, TransformBundle, TransformError, TransformNode, TransformNodeArray,
    TransformSourceId, TransformationResult, UnsupportedEmitFeature,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrinterOptions {
    new_line: NewLineKind,
    remove_comments: bool,
    no_implicit_use_strict: bool,
    no_emit_helpers: bool,
}

impl PrinterOptions {
    pub const fn new(new_line: NewLineKind) -> Self {
        Self {
            new_line,
            remove_comments: false,
            no_implicit_use_strict: false,
            no_emit_helpers: false,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Printer {
    options: PrinterOptions,
}

/// tsc-port: createPrinter @6.0.3
/// tsc-hash: c1ca84c9b6e0fd047ed1c89b6585be47d6bdfc44ab5e2e527411386695cd7e20
/// tsc-span: _tsc.js:116912-121378
///
/// H1.2 implements the pipeline foundation and whole-source identity arm.
pub const fn create_printer(options: PrinterOptions) -> Printer {
    Printer { options }
}

impl Printer {
    pub const fn options(self) -> PrinterOptions {
        self.options
    }

    /// The H1.2 executable printer surface. Its dispatch is already generic,
    /// while only the whole-source JavaScript arm is constructible in this
    /// slice. Later node workers reuse this pipeline rather than replacing it.
    pub fn print(
        &self,
        transformation: &mut TransformationResult,
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
        &self,
        transformation: &mut TransformationResult,
        source_id: TransformSourceId,
        recorder: &mut dyn SourceMapRecorder,
    ) -> Result<PrintedText, PrinterError> {
        if !transformation.roots().iter().any(
            |root| matches!(root, crate::TransformRoot::SourceFile(source) if *source == source_id),
        ) {
            return Err(PrinterError::SourceIsNotATransformedRoot(source_id));
        }

        let root = transformation.arena().root(source_id)?;
        let (text, language_variant, statements, token_spans) = {
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
                statements,
                token_spans,
            )
        };
        let _ = language_variant;

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

    fn node_range(
        &self,
        transformation: &TransformationResult,
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
        transformation: &TransformationResult,
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
        transformation: &TransformationResult,
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
        transformation: &TransformationResult,
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
    OverlappingSourceRange { previous_end: u32, start: u32 },
    InvalidTextSlice { start: u32, end: u32 },
    TokenPositionNotScalarBoundary { position: u32 },
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
        }
    }
}

impl Error for PrinterError {}
