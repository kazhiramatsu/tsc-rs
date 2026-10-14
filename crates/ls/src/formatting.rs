//! tsgo `ls/format.go`: the formatting requests (document, range and on
//! type) and their text edits.

use tsc_lsproto::{
    DocumentFormattingParams, DocumentFormattingResponse, DocumentOnTypeFormattingParams,
    DocumentOnTypeFormattingResponse, DocumentRangeFormattingParams,
    DocumentRangeFormattingResponse, TextEdit, TextEditsOrNull,
};
use tsc_syntax::SourceFile;

use crate::format::{self, File, TextChange};
use crate::language_service::{LanguageService, LsError};
use crate::lsutil::FormatCodeSettings;
use crate::utilities::script_of;

impl LanguageService<'_> {
    /// tsgo `toLSProtoTextEdits`.
    fn to_ls_proto_text_edits(
        &self,
        source: &SourceFile,
        changes: Vec<TextChange>,
    ) -> Vec<TextEdit> {
        let script = script_of(source);
        changes
            .into_iter()
            .map(|change| TextEdit {
                range: self
                    .converters
                    .to_lsp_range(&script, change.pos..change.end),
                new_text: change.new_text,
            })
            .collect()
    }

    /// tsgo `FormatOptions`.
    fn format_options(&self) -> &FormatCodeSettings {
        &self.user_preferences().format_code_settings
    }

    /// The program's parse of a document's file.
    fn source_of_document(&self, uri: &tsc_lsproto::DocumentUri) -> Result<&SourceFile, LsError> {
        let file = self.file(uri);
        self.program
            .document(file)
            .map(|document| document.source())
            .ok_or_else(|| LsError(format!("file not found: {}", uri.as_str())))
    }

    /// tsgo `ProvideFormatDocument`.
    pub fn provide_format_document(
        &self,
        params: &DocumentFormattingParams,
    ) -> Result<DocumentFormattingResponse, LsError> {
        if self.user_preferences().format_enabled == Some(false) {
            return Ok(TextEditsOrNull::Null);
        }
        let source = self.source_of_document(&params.text_document.uri)?;
        let format_opts = self
            .format_options()
            .from_ls_format_options(&params.options);
        let file = File::new(source);
        let edits = format::format_document(&file, &format_opts);
        Ok(TextEditsOrNull::TextEdits(
            self.to_ls_proto_text_edits(source, edits),
        ))
    }

    /// tsgo `ProvideFormatDocumentRange`.
    pub fn provide_format_document_range(
        &self,
        params: &DocumentRangeFormattingParams,
    ) -> Result<DocumentRangeFormattingResponse, LsError> {
        if self.user_preferences().format_enabled == Some(false) {
            return Ok(TextEditsOrNull::Null);
        }
        let source = self.source_of_document(&params.text_document.uri)?;
        let format_opts = self
            .format_options()
            .from_ls_format_options(&params.options);
        let range = self
            .converters
            .from_lsp_range(&script_of(source), &params.range);
        let file = File::new(source);
        let edits = format::format_selection(&file, &format_opts, range.start, range.end);
        Ok(TextEditsOrNull::TextEdits(
            self.to_ls_proto_text_edits(source, edits),
        ))
    }

    /// tsgo `ProvideFormatDocumentOnType`.
    pub fn provide_format_document_on_type(
        &self,
        params: &DocumentOnTypeFormattingParams,
    ) -> Result<DocumentOnTypeFormattingResponse, LsError> {
        if self.user_preferences().format_enabled == Some(false) {
            return Ok(TextEditsOrNull::Null);
        }
        let source = self.source_of_document(&params.text_document.uri)?;
        let format_opts = self
            .format_options()
            .from_ls_format_options(&params.options);
        let position = self
            .converters
            .from_lsp_position(&script_of(source), &params.position);
        let file = File::new(source);
        let edits = get_formatting_edits_after_keystroke(&file, &format_opts, position, &params.ch);
        Ok(TextEditsOrNull::TextEdits(
            self.to_ls_proto_text_edits(source, edits),
        ))
    }
}

/// tsgo `getFormattingEditsAfterKeystroke`: nothing in a comment.
fn get_formatting_edits_after_keystroke(
    file: &File<'_>,
    options: &FormatCodeSettings,
    position: usize,
    key: &str,
) -> Vec<TextChange> {
    let token_at_position = file.token_at_position(position);
    if crate::utilities::is_in_comment(&file.navigator, position as u32, token_at_position) {
        return Vec::new();
    }
    match key {
        "{" => format::format_on_opening_curly(file, options, position),
        "}" => format::format_on_closing_curly(file, options, position),
        ";" => format::format_on_semicolon(file, options, position),
        "\n" => format::format_on_enter(file, options, position),
        _ => Vec::new(),
    }
}
