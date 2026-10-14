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

    /// tsgo `getAllDiagnostics` (a content mapper's supplemental outputs
    /// are not ported yet).
    pub(crate) fn all_diagnostics(&mut self, file: usize) -> Result<Vec<Diagnostic>, LsError> {
        let program_error = |error: tsc_compiler::DriverError| LsError(error.to_string());
        let program = &mut *self.program;
        let mut diagnostics = program.syntactic_diagnostics(file);
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
        Ok(diagnostics)
    }

    /// tsgo `toLSPDiagnostics` (without a content mapper's synthesized
    /// rows, which come with the content mapper's language service).
    fn to_lsp_diagnostics(&self, diagnostics: &[Diagnostic]) -> Vec<lsproto::Diagnostic> {
        let options = DiagnosticOptions::pull(
            self.capabilities,
            self.user_preferences().report_style_checks_as_warnings == Some(true),
        );
        diagnostics
            .iter()
            .map(|diagnostic| {
                diagnostic_to_lsp(
                    self.converters,
                    diagnostic,
                    &options,
                    self.catalog,
                    &|file_name| self.diagnostic_file(file_name),
                )
            })
            .collect()
    }
}
