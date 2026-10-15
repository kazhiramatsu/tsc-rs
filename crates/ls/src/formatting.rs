//! tsgo `ls/format.go`: the formatting requests (document, range and on
//! type) and their text edits.

use tsc_emitter::PositionedNode;
use tsc_lsproto::{
    DocumentFormattingParams, DocumentFormattingResponse, DocumentOnTypeFormattingParams,
    DocumentOnTypeFormattingResponse, DocumentRangeFormattingParams,
    DocumentRangeFormattingResponse, TextEdit, TextEditsOrNull,
};
use tsc_syntax::SourceFile;

use crate::format::{self, File, TextChange};
use crate::language_service::{LanguageService, LsError};
use crate::lsconv::Script;
use crate::lsutil::FormatCodeSettings;
use crate::utilities::script_of;
use tsc_diagnostics::spanmap::{Segment, FEATURE_FORMATTING, KIND_VERBATIM};

impl LanguageService<'_> {
    /// tsgo `toLSProtoTextEdits`: none when a change does not map exactly
    /// (in a content mapper's output).
    fn to_ls_proto_text_edits(
        &self,
        source: &SourceFile,
        changes: Vec<TextChange>,
    ) -> Vec<TextEdit> {
        let script = script_of(source);
        let mut edits = Vec::with_capacity(changes.len());
        for change in changes {
            let (range, fidelity) = self
                .converters
                .to_lsp_range_with_fidelity(&script, change.pos..change.end);
            if !fidelity.is_exact() {
                return Vec::new();
            }
            edits.push(TextEdit {
                range,
                new_text: change.new_text,
            });
        }
        edits
    }

    /// tsgo `sourceFileProjections` of a document's file: the program's
    /// parse of it, then of its content mapper's supplemental outputs.
    fn document_projections<'s>(&'s self, source: &'s SourceFile) -> Vec<&'s SourceFile> {
        let mut projections = vec![source];
        if let Some(output) = &source.content_mapper {
            projections.extend(output.supplemental.iter().filter_map(|name| {
                let index = self.program.file_index(name.as_js())?;
                self.program
                    .document(index)
                    .map(|document| document.source())
            }));
        }
        projections
    }

    /// tsgo `getFormattingEditsForMappedRange`: the edits of each
    /// formatting verbatim segment's part of the original range, in the
    /// file's outputs (each original position formatted once, the earliest
    /// then longest segment first), only those inside the part that map
    /// exactly, in order.
    fn mapped_formatting_edits(
        &self,
        source: &SourceFile,
        options: &FormatCodeSettings,
        original: std::ops::Range<usize>,
    ) -> Vec<TextEdit> {
        let mut candidates = Vec::new();
        for projection in self.document_projections(source) {
            let Some(span_map) = projection
                .content_mapper
                .as_ref()
                .and_then(|output| output.span_map.as_ref())
            else {
                continue;
            };
            for segment in span_map.segments() {
                if segment.kind != KIND_VERBATIM || segment.features & FEATURE_FORMATTING == 0 {
                    continue;
                }
                let start = original.start.max(segment.original_start as usize);
                let end = original.end.min(segment.original_end as usize);
                if start >= end {
                    continue;
                }
                candidates.push((projection, *segment, start..end));
            }
        }
        let mut edits = Vec::new();
        for (projection, segment, original) in non_overlapping_formatting_ranges(candidates) {
            let virtual_start =
                segment.virtual_start as usize + original.start - segment.original_start as usize;
            let virtual_end =
                segment.virtual_start as usize + original.end - segment.original_start as usize;
            let file = File::new(projection);
            let script = script_of(projection);
            for change in format::format_selection(&file, options, virtual_start, virtual_end) {
                if change.pos < virtual_start || change.end > virtual_end {
                    continue;
                }
                let (range, fidelity) = self.converters.to_lsp_range_for_feature(
                    &script,
                    change.pos..change.end,
                    FEATURE_FORMATTING,
                );
                if !fidelity.is_exact() {
                    continue;
                }
                edits.push(TextEdit {
                    range,
                    new_text: change.new_text,
                });
            }
        }
        edits.sort_by(|left, right| {
            let key = |edit: &TextEdit| {
                (
                    edit.range.start.line,
                    edit.range.start.character,
                    edit.range.end.line,
                    edit.range.end.character,
                )
            };
            key(left)
                .cmp(&key(right))
                .then_with(|| left.new_text.cmp(&right.new_text))
        });
        edits
    }

    /// tsgo `FormatOptions`.
    pub(crate) fn format_options(&self) -> &FormatCodeSettings {
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
        // A content mapper's output: its segments over the whole original.
        if let Some(output) = &source.content_mapper {
            return Ok(TextEditsOrNull::TextEdits(self.mapped_formatting_edits(
                source,
                &format_opts,
                0..output.original_text.len(),
            )));
        }
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
        if source.content_mapper.is_some() {
            let original = self
                .converters
                .from_lsp_range_to_original(&script_of(source), &params.range);
            return Ok(TextEditsOrNull::TextEdits(self.mapped_formatting_edits(
                source,
                &format_opts,
                original,
            )));
        }
        // tsgo `FromLSPRangeForSourceFile`: one exact projection.
        let projections = self.document_projections(source);
        let scripts = projections
            .iter()
            .map(|source| script_of(source))
            .collect::<Vec<_>>();
        let scripts = scripts
            .iter()
            .map(|script| script as &dyn Script)
            .collect::<Vec<_>>();
        let ranges =
            self.converters
                .from_lsp_range_for_scripts(&scripts, &params.range, FEATURE_FORMATTING);
        let [range] = ranges.as_slice() else {
            return Ok(TextEditsOrNull::TextEdits(Vec::new()));
        };
        if !range.fidelity.is_exact() {
            return Ok(TextEditsOrNull::TextEdits(Vec::new()));
        }
        let source = projections[range.script];
        let file = File::new(source);
        let edits = format::format_selection(&file, &format_opts, range.span.start, range.span.end);
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
        // tsgo `FromLSPPositionForSourceFile`: one exact projection.
        let projections = self.document_projections(source);
        let scripts = projections
            .iter()
            .map(|source| script_of(source))
            .collect::<Vec<_>>();
        let scripts = scripts
            .iter()
            .map(|script| script as &dyn Script)
            .collect::<Vec<_>>();
        let positions = self.converters.from_lsp_position_for_scripts(
            &scripts,
            &params.position,
            FEATURE_FORMATTING,
        );
        let [position] = positions.as_slice() else {
            return Ok(TextEditsOrNull::TextEdits(Vec::new()));
        };
        if !position.fidelity.is_exact() {
            return Ok(TextEditsOrNull::TextEdits(Vec::new()));
        }
        let source = projections[position.script];
        let file = File::new(source);
        let edits = get_formatting_edits_after_keystroke(
            &file,
            &format_opts,
            position.position,
            &params.ch,
        );
        Ok(TextEditsOrNull::TextEdits(
            self.to_ls_proto_text_edits(source, edits),
        ))
    }
}

/// tsgo `handleFormatNodeForInsertion` (`api/session.go`) once the node is
/// printed and positioned: its text with the lines indented for an
/// insertion at byte `position` of `target`.
pub fn format_node_for_insertion(
    target: &SourceFile,
    position: usize,
    positioned: &PositionedNode,
    options: &FormatCodeSettings,
) -> String {
    let target_file = File::new(target);
    let synthetic = File::new(&positioned.file);
    let node = synthetic
        .node_of(Some(positioned.node))
        .expect("the positioned node is in its synthetic file");
    let is_at_line_start =
        format::get_line_start_position_for_position(&target_file, position) == position;
    let initial_indentation =
        format::get_indentation(&target_file, position, options, is_at_line_start);
    // tsgo asks with the decoded node and no source file: only its kind
    // decides.
    let indent_size = options.editor_settings.indent_size;
    let delta = if indent_size != 0
        && format::should_indent_child_node(options, &synthetic, node, None, false, false)
    {
        indent_size
    } else {
        0
    };
    let changes = format::format_node_given_indentation(
        &synthetic,
        options,
        node,
        initial_indentation,
        delta,
    );
    apply_bulk_edits(&positioned.text, &changes)
}

/// tsgo `core.ApplyBulkEdits`: the text with the sorted, disjoint changes
/// applied.
pub(crate) fn apply_bulk_edits(text: &str, changes: &[TextChange]) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;
    for change in changes {
        if change.pos != last_end {
            result.push_str(&text[last_end..change.pos]);
        }
        result.push_str(&change.new_text);
        last_end = change.end;
    }
    result.push_str(&text[last_end..]);
    result
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

/// tsgo `nonOverlappingFormattingRanges`: at most one output's segment for
/// each original position — by start, then the longer first; a later
/// candidate starts where the last kept one ends, and is dropped when
/// nothing is left of it.
fn non_overlapping_formatting_ranges(
    mut candidates: Vec<(&SourceFile, Segment, std::ops::Range<usize>)>,
) -> Vec<(&SourceFile, Segment, std::ops::Range<usize>)> {
    candidates.sort_by(|left, right| {
        left.2
            .start
            .cmp(&right.2.start)
            .then_with(|| right.2.end.cmp(&left.2.end))
    });
    let mut result: Vec<(&SourceFile, Segment, std::ops::Range<usize>)> = Vec::new();
    for (projection, segment, mut original) in candidates {
        if let Some((_, _, last)) = result.last() {
            original.start = original.start.max(last.end);
        }
        if original.start < original.end {
            result.push((projection, segment, original));
        }
    }
    result
}
