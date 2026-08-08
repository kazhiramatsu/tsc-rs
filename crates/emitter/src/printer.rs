use std::error::Error;
use std::fmt;

use tsc_syntax::{scan_tokens, skip_trivia, NodeData, SyntaxKind};
use tsc_types::NodeFlags;

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
/// tsc-hash: b227b66a85178f81faf58d6de65ed31fe2a87de1448ec6ec61e535fd36194697
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

    /// The generic H1 printer surface. H1.2 established the exact whole-source
    /// identity arm; H1.3 adds the bounded changed-node JavaScript workers
    /// while the remaining request/product axes stay typed controls.
    pub fn print(
        &self,
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
        &self,
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

        if transformation
            .arena()
            .metadata(root)
            .and_then(crate::EmitMetadata::original)
            .is_some()
        {
            return self.print_transformed_source_file(
                transformation,
                source_id,
                root,
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

    fn print_transformed_source_file(
        &self,
        transformation: &mut TransformationResult<'_>,
        source_id: TransformSourceId,
        root: TransformNode,
        statements: Vec<tsc_syntax::NodeId>,
        recorder: &mut dyn SourceMapRecorder,
    ) -> Result<PrintedText, PrinterError> {
        transformation.before_emit_node(EmitHint::SourceFile, root)?;
        let emitted_root = transformation.substitute_node(EmitHint::SourceFile, root)?;
        if emitted_root != root {
            transformation.after_emit_node(EmitHint::SourceFile, root)?;
            return Err(PrinterError::TransformedNodeWorkerUnavailable(emitted_root));
        }

        let mut writer = create_text_writer(self.options.new_line);
        for raw_statement in statements {
            let statement = transformation
                .arena()
                .node_ref(source_id, raw_statement)
                .ok_or(PrinterError::UnknownStatement(raw_statement.0))?;
            transformation.before_emit_node(EmitHint::Unspecified, statement)?;
            let emitted = transformation.substitute_node(EmitHint::Unspecified, statement)?;
            self.record_node_hook(
                transformation,
                recorder,
                SourceMapHookPhase::BeforeNode,
                emitted,
                &writer,
            )?;
            self.emit_transformed_node(transformation, emitted, &mut writer)?;
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
        transformation.after_emit_node(EmitHint::SourceFile, root)?;
        Ok(PrintedText {
            text: writer.text().to_owned(),
            end: writer.location(),
        })
    }

    fn emit_transformed_node(
        &self,
        transformation: &mut TransformationResult<'_>,
        node: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let record = transformation.arena().node(node)?.clone();
        let changed = transformation
            .arena()
            .metadata(node)
            .and_then(crate::EmitMetadata::original)
            .is_some()
            || NodeFlags::from_bits(record.flags).contains(NodeFlags::SYNTHESIZED);
        if !changed {
            return self.write_original_without_leading_trivia(transformation, node, writer);
        }

        match record.data {
            NodeData::ImportDeclaration(data) => {
                if data.attributes.is_some() {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
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
                writer.write_trailing_semicolon(";");
                Ok(())
            }
            NodeData::ImportClause(data) => {
                if data.is_type_only || data.phase_modifier.is_some() {
                    return Err(PrinterError::UnsupportedTransformedSyntax {
                        node,
                        kind: record.kind,
                    });
                }
                if let Some(name) = data.name {
                    self.emit_node_id(transformation, node.source(), name, writer)?;
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
                self.emit_required_node(
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
                if data.is_type_only || data.attributes.is_some() {
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
                writer.write_trailing_semicolon(";");
                Ok(())
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
                self.emit_required_node(
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
                writer.write_keyword(if flags.contains(NodeFlags::CONST) {
                    "const"
                } else if flags.contains(NodeFlags::LET) {
                    "let"
                } else {
                    "var"
                });
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
                self.emit_required_node(
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
                    self.emit_node_id(transformation, node.source(), name, writer)?;
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
                    self.emit_node_id(transformation, node.source(), name, writer)?;
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
                self.emit_parameter_list(transformation, node.source(), data.parameters, writer)?;
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
                self.emit_required_node(
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
                node.source(),
                data.modifiers,
                data.name,
                data.heritage_clauses,
                data.members,
                true,
                writer,
            ),
            NodeData::PropertyDeclaration(data) => {
                if self.emit_modifiers(transformation, node.source(), data.modifiers, writer)? {
                    writer.write_space(" ");
                }
                self.emit_required_node(
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
                self.emit_required_node(
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
                self.emit_required_node(
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
                self.emit_required_node(
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
            NodeData::PartiallyEmittedExpression(data) => self.emit_required_node(
                transformation,
                node.source(),
                data.expression,
                SyntaxKind::PartiallyEmittedExpression,
                "expression",
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
                    self.emit_node_array(
                        transformation,
                        node.source(),
                        data.arguments,
                        ", ",
                        writer,
                    )?;
                    writer.write_punctuation(")");
                }
                Ok(())
            }
            NodeData::CallExpression(data) => {
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::CallExpression,
                    "expression",
                    writer,
                )?;
                if data.question_dot_token.is_some() {
                    writer.write_punctuation("?.");
                }
                writer.write_punctuation("(");
                self.emit_node_array(transformation, node.source(), data.arguments, ", ", writer)?;
                writer.write_punctuation(")");
                Ok(())
            }
            NodeData::ParenthesizedExpression(data) => {
                writer.write_punctuation("(");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.expression,
                    SyntaxKind::ParenthesizedExpression,
                    "expression",
                    writer,
                )?;
                writer.write_punctuation(")");
                Ok(())
            }
            NodeData::BinaryExpression(data) => {
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.left,
                    SyntaxKind::BinaryExpression,
                    "left",
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.operator_token,
                    SyntaxKind::BinaryExpression,
                    "operator_token",
                    writer,
                )?;
                writer.write_space(" ");
                self.emit_required_node(
                    transformation,
                    node.source(),
                    data.right,
                    SyntaxKind::BinaryExpression,
                    "right",
                    writer,
                )
            }
            NodeData::Block(data) => {
                writer.write_punctuation("{");
                if data.statements.is_some() {
                    writer.write_line(false);
                    writer.increase_indent();
                    let array = data
                        .statements
                        .and_then(|id| transformation.arena().node_array_ref(node.source(), id));
                    if let Some(array) = array {
                        let statements = transformation.arena().node_array(array)?.nodes.clone();
                        for statement in statements {
                            self.emit_node_id(transformation, node.source(), statement, writer)?;
                            writer.write_line(false);
                        }
                    }
                    writer.decrease_indent();
                }
                writer.write_punctuation("}");
                Ok(())
            }
            _ => Err(PrinterError::UnsupportedTransformedSyntax {
                node,
                kind: record.kind,
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_class(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        modifiers: Option<tsc_syntax::NodeArrayId>,
        name: Option<tsc_syntax::NodeId>,
        heritage_clauses: Option<tsc_syntax::NodeArrayId>,
        members: Option<tsc_syntax::NodeArrayId>,
        expression: bool,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        if self.emit_modifiers(transformation, source, modifiers, writer)? {
            writer.write_space(" ");
        }
        writer.write_keyword("class");
        if let Some(name) = name {
            writer.write_space(" ");
            self.emit_node_id(transformation, source, name, writer)?;
        } else if !expression {
            return Err(PrinterError::MissingTransformedChild {
                parent: SyntaxKind::ClassDeclaration,
                field: "name",
            });
        }
        if heritage_clauses.is_some() {
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
                for member in member_ids {
                    self.emit_node_id(transformation, source, member, writer)?;
                    writer.write_line(false);
                }
                writer.decrease_indent();
            }
        }
        writer.write_punctuation("}");
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
            self.emit_node_id(transformation, source, property_name, writer)?;
            writer.write_space(" ");
            writer.write_keyword("as");
            writer.write_space(" ");
        }
        self.emit_required_node(transformation, source, name, parent, "name", writer)
    }

    fn emit_modifiers(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        modifiers: Option<tsc_syntax::NodeArrayId>,
        writer: &mut TextWriter,
    ) -> Result<bool, PrinterError> {
        let present = modifiers
            .and_then(|id| transformation.arena().node_array_ref(source, id))
            .is_some_and(|array| {
                transformation
                    .arena()
                    .node_array(array)
                    .is_ok_and(|array| !array.nodes.is_empty())
            });
        self.emit_node_array(transformation, source, modifiers, " ", writer)?;
        Ok(present)
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

    fn emit_required_node(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: Option<tsc_syntax::NodeId>,
        parent: SyntaxKind,
        field: &'static str,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let id = id.ok_or(PrinterError::MissingTransformedChild { parent, field })?;
        self.emit_node_id(transformation, source, id, writer)
    }

    fn emit_node_id(
        &self,
        transformation: &mut TransformationResult<'_>,
        source: TransformSourceId,
        id: tsc_syntax::NodeId,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        let node = transformation
            .arena()
            .node_ref(source, id)
            .ok_or(PrinterError::UnknownStatement(id.0))?;
        let substituted = transformation.substitute_node(EmitHint::Unspecified, node)?;
        self.emit_transformed_node(transformation, substituted, writer)
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
        writer.write(slice);
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
        }
    }
}

impl Error for PrinterError {}
