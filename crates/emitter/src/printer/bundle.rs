//! Shared-writer bundle printing. Runtime outFile routing remains a separate
//! admission boundary; declaration, external-module and map composition are
//! retained typed controls until their corresponding owners are connected.
use super::*;

struct BundleSource {
    source_id: TransformSourceId,
    root: TransformNode,
    statement_array: Option<TransformNodeArray>,
    statements: Vec<NodeId>,
    helpers: Vec<EmitHelper>,
}

impl Printer {
    pub(super) fn print_bundle(
        &mut self,
        transformation: &mut TransformationResult<'_>,
        bundle: &TransformBundle,
        recording: Option<crate::source_map::SourceMapRecordingInputs>,
    ) -> Result<PrintedText, PrinterError> {
        if recording.is_some()
            || self.options.declaration_syntax
            || !transformation.roots().iter().any(|root| {
                matches!(root, crate::TransformRoot::Bundle(transformed) if transformed == bundle)
            })
        {
            return Err(PrinterError::Unsupported(UnsupportedEmitFeature::BundleRoot));
        }
        let mut sources = Vec::with_capacity(bundle.sources().len());
        for &source_id in bundle.sources() {
            let source = transformation.arena().source(source_id)?.syntax();
            if source.is_declaration_file
                || (source.external_module_indicator.is_some()
                    && !matches!(self.options.module_kind, Some(2 | 4)))
                || source.file_name.to_ascii_lowercase().ends_with(".json")
            {
                return Err(PrinterError::Unsupported(
                    UnsupportedEmitFeature::BundleRoot,
                ));
            }
            let root = transformation.arena().root(source_id)?;
            let NodeData::SourceFile(data) = &transformation.arena().node(root)?.data else {
                return Err(PrinterError::RootIsNotSourceFile(root));
            };
            let statement_array = data
                .statements
                .map(|array| TransformNodeArray::new(source_id, array));
            let statements = statement_array
                .map(|array| {
                    transformation
                        .arena()
                        .node_array(array)
                        .map(|array| array.nodes.clone())
                })
                .transpose()?
                .unwrap_or_default();
            let helpers = self.sorted_source_emit_helpers(transformation, source_id)?;
            sources.push(BundleSource {
                source_id,
                root,
                statement_array,
                statements,
                helpers,
            });
        }
        transformation.finalize_bundle_generated_names_for_print(bundle.sources())?;
        let mut writer = create_text_writer(self.options.new_line);
        // writeBundle / emitShebangIfNeeded (_tsc.js:117058-117070,
        // 119824-119840): the first shebang anywhere in the bundle wins.
        for source in &sources {
            if let Some(shebang) = source_shebang(
                transformation
                    .arena()
                    .source(source.source_id)?
                    .syntax()
                    .text(),
            ) {
                writer.write_comment(shebang);
                writer.write_line(false);
                break;
            }
        }
        let mut prologues = BTreeSet::new();
        for source in &sources {
            self.prepare_emission_plan(transformation, source.root)?;
            for &raw_statement in &source.statements {
                let statement = transformation
                    .arena()
                    .node_ref(source.source_id, raw_statement)
                    .ok_or(PrinterError::UnknownStatement(raw_statement.0))?;
                let Some(value) = self.bundle_prologue_value(transformation, statement)? else {
                    break;
                };
                if prologues.insert(value) {
                    self.write_bundle_prologue(transformation, statement, &mut writer)?;
                }
            }
        }
        let mut emitted_helpers = BTreeSet::<Box<str>>::new();
        // ModuleKind.None leaves helpers next to the first requesting source.
        // Other module kinds hoist unscoped helpers into the bundle header.
        if self.options.module_kind != Some(0) {
            for source in &sources {
                let helpers = source
                    .helpers
                    .iter()
                    .filter(|helper| {
                        !helper.scoped() && emitted_helpers.insert(helper.name().into())
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                self.emit_helpers(&helpers, &mut writer)?;
            }
        }
        for mut source in sources {
            source
                .helpers
                .retain(|helper| helper.scoped() || emitted_helpers.insert(helper.name().into()));
            self.prepare_emission_plan(transformation, source.root)?;
            self.write_transformed_source_file(
                transformation,
                SourceFilePrintBody {
                    source_id: source.source_id,
                    root: source.root,
                    statement_array: source.statement_array,
                    statements: source.statements,
                    helpers: &source.helpers,
                    mode: SourceFileEmitMode::Bundle,
                },
                &mut writer,
            )?;
        }
        Ok(PrintedText {
            text: writer.text().to_owned(),
            end: writer.location(),
            source_map: None,
        })
    }

    fn bundle_prologue_value(
        &self,
        transformation: &TransformationResult<'_>,
        statement: TransformNode,
    ) -> Result<Option<Vec<u16>>, PrinterError> {
        let NodeData::ExpressionStatement(data) = &transformation.arena().node(statement)?.data
        else {
            return Ok(None);
        };
        let Some(expression) = data
            .expression
            .and_then(|node| transformation.arena().node_ref(statement.source(), node))
        else {
            return Ok(None);
        };
        let NodeData::StringLiteral(data) = &transformation.arena().node(expression)?.data else {
            return Ok(None);
        };
        if let Some(value) = transformation
            .arena()
            .metadata(expression)
            .and_then(crate::EmitMetadata::javascript_string_value)
        {
            return Ok(Some(value.code_units().to_vec()));
        }
        // The parser's UTF-8-facing token value cannot retain surrogate code
        // units. A valid string literal has the same raw escape decoding as
        // the existing lossless template-fragment side channel. Only borrow
        // the original spelling when this literal retains its original value.
        let original = transformation.arena().get_original_node(expression);
        let record = transformation.arena().node(original)?;
        if matches!(&record.data, NodeData::StringLiteral(original) if original.text == data.text) {
            let source = transformation.arena().source(original.source())?.syntax();
            if let SourceRange::Original(range) =
                SourceRange::from_raw(record.pos, record.end, source.positions())?
            {
                let start = skip_trivia(source.text(), range.start().value() as usize);
                if let Some(raw) = source.text().get(start..range.end().value() as usize) {
                    if raw.len() >= 2
                        && matches!(raw.as_bytes()[0], b'\'' | b'"')
                        && raw.as_bytes().last() == raw.as_bytes().first()
                    {
                        return Ok(Some(tsc_syntax::template_text_utf16(
                            &data.text,
                            Some(&raw[1..raw.len() - 1]),
                        )));
                    }
                }
            }
        }
        Ok(Some(data.text.encode_utf16().collect()))
    }

    fn write_bundle_prologue(
        &self,
        transformation: &mut TransformationResult<'_>,
        statement: TransformNode,
        writer: &mut TextWriter,
    ) -> Result<(), PrinterError> {
        writer.write_line(false);
        transformation.before_emit_node(EmitHint::Unspecified, statement)?;
        let emitted = transformation.substitute_node(EmitHint::Unspecified, statement)?;
        self.emit_statement_leading_comments(transformation, emitted, writer)?;
        self.emit_transformed_node(transformation, emitted, EmitContext::file_root(), writer)?;
        self.emit_statement_trailing_comments(transformation, emitted, writer)?;
        transformation.after_emit_node(EmitHint::Unspecified, statement)?;
        writer.write_line(false);
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/bundle_printer/tests.rs"]
mod bundle_printer_tests;
