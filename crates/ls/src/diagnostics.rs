//! tsgo `ls/diagnostics.go`: a document's pulled diagnostics.

use tsc_diagnostics::Diagnostic;
use tsc_lsproto::{
    self as lsproto, DocumentDiagnosticResponse, DocumentUri, RelatedFullDocumentDiagnosticReport,
    RelatedFullDocumentDiagnosticReportOrUnchangedDocumentDiagnosticReport,
};

use crate::language_service::{LanguageService, LsError};
use crate::lsconv::{diagnostic_to_lsp, DiagnosticOptions};

impl LanguageService<'_> {
    /// tsgo `ProvideDiagnostics`: the document's syntactic, semantic,
    /// suggestion and (when declarations are emitted) declaration
    /// diagnostics; none when the user turned validation off.
    pub fn provide_diagnostics(
        &mut self,
        uri: &DocumentUri,
    ) -> Result<DocumentDiagnosticResponse, LsError> {
        let file = self.file(uri);
        let items = if self.user_preferences().validate_enabled == Some(false) {
            Vec::new()
        } else {
            let diagnostics = self.all_diagnostics(file)?;
            self.to_lsp_diagnostics(&diagnostics)
        };
        Ok(
            RelatedFullDocumentDiagnosticReportOrUnchangedDocumentDiagnosticReport::FullDocumentDiagnosticReport(
                RelatedFullDocumentDiagnosticReport {
                    items,
                    ..RelatedFullDocumentDiagnosticReport::default()
                },
            ),
        )
    }

    /// tsgo `getAllDiagnostics`: the syntactic, semantic, suggestion and
    /// (when declarations are emitted) declaration diagnostics of the file
    /// and of its content mapper's supplemental outputs.
    pub(crate) fn all_diagnostics(&mut self, file: usize) -> Result<Vec<Diagnostic>, LsError> {
        let program_error = |error: tsc_compiler::DriverError| LsError(error.to_string());
        let mut files = vec![file];
        if let Some(output) = self
            .program
            .document(file)
            .and_then(|document| document.source().content_mapper.clone())
        {
            files.extend(
                output
                    .supplemental
                    .iter()
                    .filter_map(|name| self.program.file_index(name.as_js())),
            );
        }
        let program = &mut *self.program;
        let mut diagnostics = Vec::new();
        for file in files {
            diagnostics.extend(program.syntactic_diagnostics(file));
            diagnostics.extend(program.semantic_diagnostics(file).map_err(program_error)?);
            diagnostics.extend(
                program
                    .suggestion_diagnostics(file)
                    .map_err(program_error)?,
            );
            if tsc_compiler::get_emit_declarations(program.prepared().compiler_options()) {
                for rows in program
                    .declaration_diagnostics(&[file])
                    .map_err(program_error)?
                {
                    diagnostics.extend(rows);
                }
            }
        }
        Ok(diagnostics)
    }

    /// tsgo `toLSPDiagnostics`: each diagnostic, and for a content mapper's
    /// output, one diagnostic at its top for those entirely in synthesized
    /// code (they have no original location), after the others.
    fn to_lsp_diagnostics(&self, diagnostics: &[Diagnostic]) -> Vec<lsproto::Diagnostic> {
        let options = DiagnosticOptions::pull(
            self.capabilities,
            self.user_preferences().report_style_checks_as_warnings == Some(true),
        );
        let convert = |diagnostic: &Diagnostic| {
            diagnostic_to_lsp(
                self.converters,
                diagnostic,
                &options,
                self.catalog,
                &|file_name| self.diagnostic_file(file_name),
            )
        };
        let mut items = Vec::with_capacity(diagnostics.len());
        // By file, in the order of their first diagnostics.
        let mut synthesized: Vec<(String, Vec<&Diagnostic>)> = Vec::new();
        for diagnostic in diagnostics {
            match self.synthesized_content_mapped_diagnostic(diagnostic) {
                Some(mapper) => match synthesized.iter_mut().find(|(name, _)| *name == mapper) {
                    Some((_, rows)) => rows.push(diagnostic),
                    None => synthesized.push((mapper, vec![diagnostic])),
                },
                None => items.push(convert(diagnostic)),
            }
        }
        for (mapper, rows) in synthesized {
            items.push(convert(&aggregate_synthesized_diagnostics(&mapper, &rows)));
        }
        items
    }

    /// tsgo `isSynthesizedContentMappedDiagnostic`: a compiler diagnostic
    /// of a content mapper's output entirely in synthesized code; the
    /// output's file name and mapper (by which the aggregate is made).
    fn synthesized_content_mapped_diagnostic(&self, diagnostic: &Diagnostic) -> Option<String> {
        if diagnostic
            .source
            .as_deref()
            .is_some_and(|source| !source.is_empty())
        {
            return None;
        }
        let file_name = diagnostic.file_name.as_ref()?;
        let index = self.program.file_index(file_name.as_js())?;
        let document = self.program.document(index)?;
        let source = document.source();
        let span_map = source.span_map.as_ref()?;
        let start = diagnostic.start.unwrap_or(0);
        let end = start.saturating_add(diagnostic.length.unwrap_or(0));
        let (_, fidelity) = span_map.virtual_to_original_span(start as i32, end as i32);
        if !fidelity.is_none() {
            return None;
        }
        let mapper = source
            .content_mapper
            .as_ref()
            .map_or("", |output| &output.mapper);
        Some(format!("{}\0{mapper}", file_name.to_string_lossy()))
    }
}

/// tsgo `aggregateSynthesizedDiagnostics`: one diagnostic at the top of a
/// content mapper's output for its diagnostics in synthesized code, which
/// are its related information, with the worst of their categories.
/// `key` is the output's file name and its mapper.
fn aggregate_synthesized_diagnostics(key: &str, diagnostics: &[&Diagnostic]) -> Diagnostic {
    let (file_name, mapper) = key.split_once('\0').unwrap_or((key, ""));
    let mut message = tsc_diagnostics::MessageChain::new(
        &tsc_diagnostics::gen::Virtual_code_produced_by_the_content_mapper_0_has_problems_with_no_corresponding_location_in_this_file,
        &[mapper.to_owned()],
    );
    message.category = worst_category(diagnostics);
    let mut aggregate = Diagnostic::new(Some(file_name.to_owned()), Some(0), Some(0), message);
    aggregate.related = diagnostics
        .iter()
        .map(|diagnostic| tsc_diagnostics::RelatedInfo {
            file_name: diagnostic.file_name.clone(),
            start: diagnostic.start,
            length: diagnostic.length,
            message: diagnostic.message.clone(),
        })
        .collect();
    aggregate.related_information_present = true;
    aggregate
}

/// tsgo `worstCategory`: an error if any is one, else a warning if any is
/// one, else the first's.
fn worst_category(diagnostics: &[&Diagnostic]) -> tsc_diagnostics::DiagnosticCategory {
    use tsc_diagnostics::DiagnosticCategory;
    let mut worst = diagnostics[0].category();
    for diagnostic in diagnostics {
        match diagnostic.category() {
            DiagnosticCategory::Error => return DiagnosticCategory::Error,
            DiagnosticCategory::Warning => worst = DiagnosticCategory::Warning,
            _ => {}
        }
    }
    worst
}
