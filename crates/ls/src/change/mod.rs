//! tsgo `ls/change`: the change tracker. A feature records edits to source
//! files (text, a node or nodes to print in place of a range, nodes to
//! delete), and [`Tracker::get_changes`] turns them into text edits: each
//! node printed with tsgo's change tracker printer (`PrintAndPositionNode`
//! with its file) and formatted where it goes.
//!
//! The nodes live in the tracker's emit arena: a file's own nodes keep
//! their ids there ([`Tracker::node`]), and a feature creates new ones with
//! the arena's factory ([`Tracker::arena`]). A content mapper's output's
//! edits are written back to its original file, each node formatted at every
//! exact projection of its position; a file whose edits cannot be is left
//! out (tsgo `unmappableFiles`).

mod delete;
mod insert;

pub(crate) use insert::is_statement;

use tsc_emitter::{
    create_printer, print_and_position, transform_nodes, EmitFlags, NewLineKind, PositionedPrint,
    PrintRequest, PrinterError, PrinterOptions, StandaloneWriter, TransformArena, TransformError,
    TransformNode, TransformSourceId,
};
use tsc_syntax::{
    comment_ranges, skip_trivia_ex, NodeArena, NodeId, SkipTriviaOptions, SourceFile,
};

use std::collections::BTreeSet;

use tsc_checker::program::ProgramBinder;
use tsc_diagnostics::spanmap::FEATURES_ALL;
use tsc_lsproto::TextEdit;

use crate::astnav::Found;
use crate::format::{self, File, TextChange};
use crate::lsconv::{compare_ranges, Converters, Script};
use crate::lsutil::{FormatCodeSettings, SemicolonPreference};
use crate::request::source_projections_of;
use crate::tree::View;
use crate::utilities::script_of;

/// tsgo `NodeOptions`.
#[derive(Clone, Debug, Default)]
pub(crate) struct NodeOptions {
    /// Text to be inserted before the new node.
    pub(crate) prefix: String,
    /// Text to be inserted after the new node.
    pub(crate) suffix: String,
    /// The indentation of the inserted node's text, instead of the one of
    /// its position.
    pub(crate) indentation: Option<i64>,
    /// The delta of the inserted node's text, instead of the one of its
    /// kind.
    pub(crate) delta: Option<i64>,
    pub(crate) leading_trivia_option: LeadingTriviaOption,
    pub(crate) trailing_trivia_option: TrailingTriviaOption,
    /// What joins the texts of several nodes (a new line when none).
    pub(crate) joiner: Option<String>,
}

/// tsgo `LeadingTriviaOption`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum LeadingTriviaOption {
    #[default]
    None,
    Exclude,
    IncludeAll,
    JSDoc,
    StartLine,
}

/// tsgo `TrailingTriviaOption`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TrailingTriviaOption {
    #[default]
    None,
    Exclude,
    ExcludeWhitespace,
    Include,
}

/// tsgo `trackerEdit`'s kinds and their content.
#[derive(Clone, Debug)]
enum EditContent {
    Text(String),
    ReplaceWithSingleNode(TransformNode),
    ReplaceWithMultipleNodes(Vec<TransformNode>),
}

/// tsgo `trackerEdit`.
#[derive(Clone, Debug)]
struct TrackerEdit {
    pos: usize,
    end: usize,
    content: EditContent,
    options: NodeOptions,
}

/// tsgo `GetChanges`' result: each original file's text edits (the files
/// in the order the tracker first edited them), and the original files whose
/// edits could not be written back to their text, sorted (their edits are
/// left out).
pub(crate) struct Changes {
    pub(crate) files: Vec<(String, Vec<TextEdit>)>,
    pub(crate) unmappable: Vec<String>,
}

impl Changes {
    /// The edits of an original file (tsgo `changes[fileName]`).
    pub(crate) fn of(self, file_name: &str) -> Vec<TextEdit> {
        self.files
            .into_iter()
            .find(|(name, _)| name == file_name)
            .map(|(_, edits)| edits)
            .unwrap_or_default()
    }
}

/// An edit the tracker cannot make: a node it could not print, or two edits
/// that overlap (tsgo panics on both).
#[derive(Debug)]
pub(crate) enum TrackerError {
    Print(PrinterError),
    Transform(TransformError),
    Overlap,
}

impl std::fmt::Display for TrackerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Print(error) => write!(formatter, "{error}"),
            Self::Transform(error) => write!(formatter, "{error}"),
            Self::Overlap => formatter.write_str("changes overlap"),
        }
    }
}

impl From<PrinterError> for TrackerError {
    fn from(error: PrinterError) -> Self {
        Self::Print(error)
    }
}

impl From<TransformError> for TrackerError {
    fn from(error: TransformError) -> Self {
        Self::Transform(error)
    }
}

/// tsgo `Tracker`.
pub(crate) struct Tracker<'a> {
    format_settings: FormatCodeSettings,
    /// The compiler options' new line (tsgo `newLine`).
    new_line: NewLineKind,
    arena: TransformArena,
    files: Vec<(&'a SourceFile, TransformSourceId)>,
    /// Each file's edits, in the order they were made (tsgo's `changes`).
    changes: Vec<(usize, TrackerEdit)>,
    deleted_nodes: Vec<(usize, NodeId)>,
    /// tsgo `nodesWithInsertionsAtStart`: the nodes a member was inserted
    /// at the start of, by file, in the order of their first insertion.
    nodes_with_insertions_at_start: Vec<(usize, NodeId)>,
    /// tsgo `unmappableFiles`: the original files an edit could not be
    /// represented in (within a single verbatim span); their edits are
    /// dropped.
    unmappable: BTreeSet<String>,
}

impl<'a> Tracker<'a> {
    /// tsgo `NewTracker`.
    pub(crate) fn new(new_line: NewLineKind, format_settings: FormatCodeSettings) -> Self {
        // The tracker prints its nodes with their positions assigned: no
        // node may appear twice in a tree.
        let mut arena = TransformArena::new();
        arena.copy_position_free_subtrees();
        Self {
            format_settings,
            new_line,
            arena,
            files: Vec::new(),
            changes: Vec::new(),
            deleted_nodes: Vec::new(),
            nodes_with_insertions_at_start: Vec::new(),
            unmappable: BTreeSet::new(),
        }
    }

    /// The index of `file` among the tracker's files, its emit copy added
    /// on first use.
    fn file_index(&mut self, file: &'a SourceFile) -> usize {
        if let Some(index) = self
            .files
            .iter()
            .position(|(known, _)| std::ptr::eq(*known, file))
        {
            return index;
        }
        let source = self.arena.add_source(file, None);
        self.files.push((file, source));
        self.files.len() - 1
    }

    /// [`Self::source`] mounted under its program source id, so that a node
    /// builder writing into the tracker's arena finds this copy of the file
    /// rather than mounting another (the first use of the file decides).
    pub(crate) fn source_in_program(
        &mut self,
        file: &'a SourceFile,
        program_source: tsc_program::SourceFileId,
    ) -> TransformSourceId {
        if let Some((_, source)) = self
            .files
            .iter()
            .find(|(known, _)| std::ptr::eq(*known, file))
        {
            return *source;
        }
        let source = self.arena.add_source(file, Some(program_source));
        self.files.push((file, source));
        source
    }

    /// The emit copy of `file`, where the tracker prints its nodes and a
    /// feature creates nodes for it.
    pub(crate) fn source(&mut self, file: &'a SourceFile) -> TransformSourceId {
        let index = self.file_index(file);
        self.files[index].1
    }

    /// The arena of the tracker's nodes, whose factory creates new ones.
    pub(crate) fn arena(&mut self) -> &mut TransformArena {
        &mut self.arena
    }

    /// The syntax of `file`'s emit copy: its parsed nodes and the ones a
    /// feature created for it.
    pub(crate) fn syntax(&mut self, file: &'a SourceFile) -> &NodeArena {
        let source = self.source(file);
        &self
            .arena
            .source(source)
            .expect("a file of the tracker")
            .syntax()
            .arena
    }

    /// tsgo `SetEmitFlags` (the tracker's emit context).
    pub(crate) fn set_emit_flags(&mut self, node: TransformNode, flags: EmitFlags) {
        self.arena.metadata_mut(node).set_flags(flags);
    }

    /// The emit flags of `node`.
    pub(crate) fn emit_flags(&self, node: TransformNode) -> EmitFlags {
        self.arena
            .metadata(node)
            .map_or(EmitFlags::NONE, |metadata| metadata.flags())
    }

    /// tsgo `typeToStringForDiag`'s print: `node` on one line by a printer
    /// over the tracker's emit context, so the names it generates are the
    /// edits'; cut to 157 bytes and `...` past 160.
    pub(crate) fn print_single_line(
        &mut self,
        node: TransformNode,
    ) -> Result<String, TrackerError> {
        let saved_flags = self.emit_flags(node);
        self.set_emit_flags(node, saved_flags | EmitFlags::SINGLE_LINE);
        let arena = std::mem::replace(&mut self.arena, TransformArena::new());
        let mut transformation = transform_nodes(arena, Vec::new(), Vec::new(), true)?;
        let printed = create_printer(
            PrinterOptions::new(NewLineKind::LineFeed).with_declaration_syntax(true),
        )
        .print(
            &mut transformation,
            PrintRequest::StandaloneNode {
                node,
                writer: StandaloneWriter::SingleLine,
            },
            None,
        );
        self.arena = transformation.into_arena();
        self.set_emit_flags(node, saved_flags);
        let text = printed?.text().to_owned();
        if text.len() <= 160 {
            return Ok(text);
        }
        // Go slices the bytes: a character cut in two leaves its bytes, each
        // one replacement character in the response.
        let bytes = &text.as_bytes()[..157];
        let valid = match std::str::from_utf8(bytes) {
            Ok(valid) => valid,
            Err(error) => std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or_default(),
        };
        let mut result = valid.to_owned();
        result.extend(std::iter::repeat_n('\u{FFFD}', bytes.len() - valid.len()));
        result.push_str("...");
        Ok(result)
    }

    /// tsgo `snippetPrinter.printAndFormatNodeWithSettings`: `node` printed
    /// by tsgo's snippet printer for a program of `target` (with the
    /// settings' new line), formatted as a node of its own by `settings`,
    /// with the `$` it wrote escaped.
    pub(crate) fn print_and_format_snippet(
        &mut self,
        node: TransformNode,
        file: &SourceFile,
        target: Option<tsc_types::ScriptTarget>,
        settings: &FormatCodeSettings,
    ) -> Result<String, TrackerError> {
        let positioned = self.print_snippet(node, file, target, settings)?;
        let synthetic = File::new(&positioned.file);
        let printed = synthetic
            .node_of(Some(positioned.node))
            .expect("the positioned node is in its synthetic file");
        let mut changes =
            format::format_node_given_indentation(&synthetic, settings, printed, 0, 0);
        if !positioned.snippet_escapes.is_empty() {
            changes.extend(positioned.snippet_escapes.iter().map(|escape| TextChange {
                pos: escape.pos,
                end: escape.end,
                new_text: escape.text.clone(),
            }));
            changes.sort_by_key(|change| (change.pos, change.end));
        }
        Ok(crate::formatting::apply_bulk_edits(
            &positioned.text,
            &changes,
        ))
    }

    /// tsgo `snippetPrinter.printUnescapedNode`: `node` printed by tsgo's
    /// snippet printer, as written.
    pub(crate) fn print_unescaped_snippet(
        &mut self,
        node: TransformNode,
        file: &SourceFile,
        target: Option<tsc_types::ScriptTarget>,
        settings: &FormatCodeSettings,
    ) -> Result<String, TrackerError> {
        Ok(self.print_snippet(node, file, target, settings)?.text)
    }

    /// tsgo `printObjectLiteralMethodLabelDetail`'s printer: `node` without
    /// comments or a trailing semicolon, for a program of `target`.
    pub(crate) fn print_signature_label(
        &mut self,
        node: TransformNode,
        target: tsc_types::ScriptTarget,
        new_line: NewLineKind,
    ) -> Result<String, TrackerError> {
        let arena = std::mem::replace(&mut self.arena, TransformArena::new());
        let mut transformation = transform_nodes(arena, Vec::new(), Vec::new(), true)?;
        let printed = create_printer(
            PrinterOptions::new(new_line)
                .with_remove_comments(true)
                .with_omit_trailing_semicolon(true)
                .with_target(target)
                .with_declaration_syntax(true),
        )
        .print(
            &mut transformation,
            PrintRequest::StandaloneNode {
                node,
                writer: StandaloneWriter::MultiLine,
            },
            None,
        );
        self.arena = transformation.into_arena();
        Ok(printed?.text().to_owned())
    }

    /// tsgo `createSnippetPrinter`'s print: without comments, by a change
    /// tracker writer of the default indentation and the settings' new line.
    fn print_snippet(
        &mut self,
        node: TransformNode,
        file: &SourceFile,
        target: Option<tsc_types::ScriptTarget>,
        settings: &FormatCodeSettings,
    ) -> Result<tsc_emitter::PositionedNode, TrackerError> {
        // tsgo `core.GetNewLineKind` of the settings' new line character.
        let new_line = if settings.editor_settings.new_line_character == "\r\n" {
            NewLineKind::CarriageReturnLineFeed
        } else {
            NewLineKind::LineFeed
        };
        let arena = std::mem::replace(&mut self.arena, TransformArena::new());
        let mut transformation = transform_nodes(arena, Vec::new(), Vec::new(), true)?;
        let positioned = print_and_position(
            &mut transformation,
            node,
            PositionedPrint {
                new_line,
                indent_size: 4,
                with_source_file: false,
                language_variant: file.language_variant,
                snippet: Some(target),
            },
        );
        self.arena = transformation.into_arena();
        Ok(positioned?)
    }

    fn add(&mut self, file: &'a SourceFile, edit: TrackerEdit) {
        let index = self.file_index(file);
        self.changes.push((index, edit));
    }

    /// tsgo `ReplaceNode`: `old` replaced by `new`, its trivia by default
    /// kept.
    pub(crate) fn replace_node(
        &mut self,
        file: &'a SourceFile,
        old: NodeId,
        new: TransformNode,
        options: Option<NodeOptions>,
    ) {
        let options = options.unwrap_or(NodeOptions {
            leading_trivia_option: LeadingTriviaOption::Exclude,
            trailing_trivia_option: TrailingTriviaOption::Exclude,
            ..NodeOptions::default()
        });
        let range = self.get_adjusted_range(
            file,
            old,
            old,
            options.leading_trivia_option,
            options.trailing_trivia_option,
        );
        self.replace_range(file, range, new, options);
    }

    /// tsgo `ReplaceNodeWithNodes`.
    pub(crate) fn replace_node_with_nodes(
        &mut self,
        file: &'a SourceFile,
        old: NodeId,
        new: Vec<TransformNode>,
        options: Option<NodeOptions>,
    ) {
        let options = options.unwrap_or(NodeOptions {
            leading_trivia_option: LeadingTriviaOption::Exclude,
            trailing_trivia_option: TrailingTriviaOption::Exclude,
            ..NodeOptions::default()
        });
        let range = self.get_adjusted_range(
            file,
            old,
            old,
            options.leading_trivia_option,
            options.trailing_trivia_option,
        );
        self.replace_range_with_nodes(file, range, new, options);
    }

    /// tsgo `ReplaceRange`.
    pub(crate) fn replace_range(
        &mut self,
        file: &'a SourceFile,
        (pos, end): (usize, usize),
        node: TransformNode,
        options: NodeOptions,
    ) {
        self.add(
            file,
            TrackerEdit {
                pos,
                end,
                content: EditContent::ReplaceWithSingleNode(node),
                options,
            },
        );
    }

    /// tsgo `ReplaceRangeWithText`: text in place of an LSP range of the
    /// document. For a content-mapped file, the range is in the original
    /// document; text may be placed at any exact projection of it in the
    /// file because it does not need formatting context. A range with none
    /// cannot be written back and marks the file unmappable (tsgo
    /// `fromLSPEditRange`).
    pub(crate) fn replace_lsp_range_with_text(
        &mut self,
        file: &'a SourceFile,
        converters: &Converters,
        range: &tsc_lsproto::Range,
        text: String,
    ) {
        let script = script_of(file);
        let spans = converters.from_lsp_range_for_scripts(&[&script], range, FEATURES_ALL);
        let span = match spans.iter().find(|span| span.fidelity.is_exact()) {
            Some(span) => span.span.clone(),
            None => {
                self.unmappable
                    .insert(script.original_file_name().to_owned());
                spans.first().map_or(0..0, |span| span.span.clone())
            }
        };
        self.replace_range_with_text(file, (span.start, span.end), text);
    }

    /// tsgo `ReplaceTextRangeWithText`.
    pub(crate) fn replace_range_with_text(
        &mut self,
        file: &'a SourceFile,
        (pos, end): (usize, usize),
        text: String,
    ) {
        self.add(
            file,
            TrackerEdit {
                pos,
                end,
                content: EditContent::Text(text),
                options: NodeOptions::default(),
            },
        );
    }

    /// tsgo `ReplaceRangeWithNodes`.
    pub(crate) fn replace_range_with_nodes(
        &mut self,
        file: &'a SourceFile,
        range: (usize, usize),
        mut nodes: Vec<TransformNode>,
        options: NodeOptions,
    ) {
        if nodes.len() == 1 {
            self.replace_range(file, range, nodes.remove(0), options);
            return;
        }
        self.add(
            file,
            TrackerEdit {
                pos: range.0,
                end: range.1,
                content: EditContent::ReplaceWithMultipleNodes(nodes),
                options,
            },
        );
    }

    /// tsgo `Delete`: `node` queued for deletion, with the handling of list
    /// items, imports and declarations that `get_changes` applies.
    pub(crate) fn delete(&mut self, file: &'a SourceFile, node: NodeId) {
        let index = self.file_index(file);
        self.deleted_nodes.push((index, node));
    }

    /// tsgo `DeleteNodeRange`.
    pub(crate) fn delete_node_range(
        &mut self,
        file: &'a SourceFile,
        start: NodeId,
        end: NodeId,
        leading: LeadingTriviaOption,
        trailing: TrailingTriviaOption,
    ) {
        let start = self.get_adjusted_start_position(file, start, leading, false);
        let end = self.get_adjusted_end_position(file, end, trailing);
        self.replace_range_with_text(file, (start, end), String::new());
    }

    /// tsgo `GetAdjustedRange`.
    pub(crate) fn get_adjusted_range(
        &self,
        file: &SourceFile,
        start: NodeId,
        end: NodeId,
        leading: LeadingTriviaOption,
        trailing: TrailingTriviaOption,
    ) -> (usize, usize) {
        (
            self.get_adjusted_start_position(file, start, leading, false),
            self.get_adjusted_end_position(file, end, trailing),
        )
    }

    /// tsgo `getAdjustedStartPosition`.
    fn get_adjusted_start_position(
        &self,
        file: &SourceFile,
        node: NodeId,
        leading: LeadingTriviaOption,
        has_trailing_comment: bool,
    ) -> usize {
        let navigator = crate::astnav::Navigator::new(file);
        self.get_adjusted_start_position_of(
            file,
            Found::Node(View::Node(node)),
            &navigator,
            leading,
            has_trailing_comment,
        )
    }

    /// tsgo `getAdjustedStartPosition` of a node or a token.
    fn get_adjusted_start_position_of(
        &self,
        file: &SourceFile,
        node: Found,
        navigator: &crate::astnav::Navigator<'_>,
        leading: LeadingTriviaOption,
        has_trailing_comment: bool,
    ) -> usize {
        let text = file.text();
        let (pos, end) = (navigator.pos(node) as usize, navigator.end(node) as usize);
        if leading == LeadingTriviaOption::JSDoc {
            if let Some(&(comment_start, _)) =
                tsc_syntax::jsdoc_comment_ranges(text, navigator.kind(node), pos).first()
            {
                return line_start_of(file, comment_start);
            }
        }
        let start = navigator.start_of_node(node, false) as usize;
        let start_of_line = line_start_of(file, start);
        match leading {
            LeadingTriviaOption::Exclude => return start,
            LeadingTriviaOption::StartLine => {
                // tsgo `node.Loc.ContainsInclusive(startOfLinePos)`.
                if pos <= start_of_line && start_of_line <= end {
                    return start_of_line;
                }
                return start;
            }
            _ => {}
        }
        let full_start = pos;
        if full_start == start {
            return start;
        }
        let full_start_line = line_of(file, full_start);
        let full_start_line_pos = line_start_at(file, full_start_line);
        if start_of_line == full_start_line_pos {
            // full start and start of the node are on the same line
            //   a,     b;
            //    ^     ^
            //    |   start
            // fullstart
            // when b is replaced - we usually want to keep the leading trvia
            // when b is deleted - we delete it
            if leading == LeadingTriviaOption::IncludeAll {
                return full_start;
            }
            return start;
        }
        // if node has a trailing comments, use comment end position as the text has already been included.
        if has_trailing_comment {
            // Check first for leading comments as if the node is the first import, we want to exclude the trivia;
            // otherwise we get the trailing comments.
            let mut comments = comment_ranges(text, full_start, false);
            if comments.is_empty() {
                comments = comment_ranges(text, full_start, true);
            }
            if let Some(&(_, comment_end)) = comments.first() {
                return skip_trivia_ex(
                    text,
                    comment_end,
                    SkipTriviaOptions {
                        stop_after_line_break: true,
                        stop_at_comments: true,
                        ..Default::default()
                    },
                );
            }
        }
        // get start position of the line following the line that contains fullstart position
        // (but only if the fullstart isn't the very beginning of the file)
        let next_line_start = usize::from(full_start > 0);
        let adjusted = line_start_at(file, full_start_line + next_line_start);
        // skip whitespaces/newlines
        let adjusted = skip_trivia_ex(
            text,
            adjusted,
            SkipTriviaOptions {
                stop_at_comments: true,
                ..Default::default()
            },
        );
        line_start_at(file, line_of(file, adjusted))
    }

    /// tsgo `getEndPositionOfMultilineTrailingComment`: the end of a
    /// multi-line comment after a node ending at `end` that goes on past
    /// its line.
    fn get_end_position_of_multiline_trailing_comment(
        &self,
        file: &SourceFile,
        end: usize,
        trailing: TrailingTriviaOption,
    ) -> Option<usize> {
        if trailing != TrailingTriviaOption::Include {
            return None;
        }
        let text = file.text();
        let node_end_line = line_of(file, end);
        for (comment_start, comment_end) in comment_ranges(text, end, true) {
            // Single line can break the loop as trivia will only be this line.
            // Comments on subsequent lines are also ignored.
            if text[comment_start..].starts_with("//")
                || line_of(file, comment_start) > node_end_line
            {
                break;
            }
            // Get the end line of the comment and compare against the end line of the node.
            // If the comment end line position and the multiline comment extends to multiple lines,
            // then is safe to return the end position.
            if line_of(file, comment_end) > node_end_line {
                return Some(skip_trivia_ex(
                    text,
                    comment_end,
                    SkipTriviaOptions {
                        stop_after_line_break: true,
                        stop_at_comments: true,
                        ..Default::default()
                    },
                ));
            }
        }
        None
    }

    /// tsgo `getAdjustedEndPosition`.
    fn get_adjusted_end_position(
        &self,
        file: &SourceFile,
        node: NodeId,
        trailing: TrailingTriviaOption,
    ) -> usize {
        self.get_adjusted_end_position_of(file, file.arena.node(node).end as usize, trailing)
    }

    /// tsgo `getAdjustedEndPosition` of a node or a token ending at `end`.
    fn get_adjusted_end_position_of(
        &self,
        file: &SourceFile,
        end: usize,
        trailing: TrailingTriviaOption,
    ) -> usize {
        let text = file.text();
        if trailing == TrailingTriviaOption::Exclude {
            return end;
        }
        if trailing == TrailingTriviaOption::ExcludeWhitespace {
            let mut comments = comment_ranges(text, end, true);
            comments.extend(comment_ranges(text, end, false));
            if let Some(&(_, real_end)) = comments.last() {
                if real_end != 0 {
                    return real_end;
                }
            }
            return end;
        }
        if let Some(multiline_end) =
            self.get_end_position_of_multiline_trailing_comment(file, end, trailing)
        {
            return multiline_end;
        }
        let new_end = skip_trivia_ex(
            text,
            end,
            SkipTriviaOptions {
                stop_after_line_break: true,
                ..Default::default()
            },
        );
        if new_end != end
            && (trailing == TrailingTriviaOption::Include
                || text[..new_end]
                    .chars()
                    .next_back()
                    .is_some_and(tsc_syntax::is_line_break))
        {
            return new_end;
        }
        end
    }

    /// tsgo `GetChanges`: the text edits of each original file, in order,
    /// and the files whose edits could not be written back (the tracker is
    /// spent). `binder` gives a content mapper's outputs' supplemental
    /// outputs, the projections an edit's text is formatted at.
    pub(crate) fn get_changes(
        mut self,
        converters: &Converters,
        binder: &ProgramBinder<'a>,
    ) -> Result<Changes, TrackerError> {
        self.finish_delete_declarations();
        self.finish_nodes_with_insertions_at_start();
        let Tracker {
            format_settings,
            new_line,
            arena,
            files,
            changes,
            unmappable,
            ..
        } = self;
        let mut transformation = transform_nodes(arena, Vec::new(), Vec::new(), true)?;
        let mut writer = EditWriter {
            transformation: &mut transformation,
            format_settings: &format_settings,
            new_line,
            converters,
            unmappable,
        };
        // tsgo `getTextChangesFromChanges`. A content-mapped file can have
        // several projections, each a file here but all sharing one original
        // file. Their edits are collected together before being ordered and
        // checked, so duplicate edits are emitted once and conflicting edits
        // are rejected.
        let mut by_original: Vec<(String, Vec<TextEdit>, usize)> = Vec::new();
        for (index, &(file, _)) in files.iter().enumerate() {
            let script = script_of(file);
            let file_name = script.original_file_name().to_owned();
            if writer.unmappable.contains(&file_name) {
                continue;
            }
            // For a content-mapped file, a mapper may reorder source text
            // relative to the original document. Convert first, then sort and
            // check for overlap in the space where edits are actually
            // applied.
            let projections = source_projections_of(binder, file);
            let mut text_changes = Vec::new();
            for (_, edit) in changes.iter().filter(|(edited, _)| *edited == index) {
                let new_text = writer.compute_new_text(file, &script, &projections, edit)?;
                let range = writer.lsp_edit_range(&script, edit.pos..edit.end);
                text_changes.push(TextEdit { range, new_text });
            }
            if text_changes.is_empty() {
                continue;
            }
            match by_original
                .iter_mut()
                .find(|(name, _, _)| *name == file_name)
            {
                Some((_, edits, projections)) => {
                    edits.extend(text_changes);
                    *projections += 1;
                }
                None => by_original.push((file_name, text_changes, 1)),
            }
        }
        let mut unmappable = writer.unmappable;
        let mut result = Vec::new();
        for (file_name, mut edits, projections) in by_original {
            // Converting the edits above may have found that this file cannot
            // be represented in its original text: it is dropped, and the
            // best-effort ranges left behind may overlap in ways the check
            // below would refuse.
            if unmappable.contains(&file_name) {
                continue;
            }
            // order changes by start position
            // If the start position is the same, put the shorter range first, since an empty range (x, x) may precede (x, y) but not vice-versa.
            edits.sort_by(|a, b| compare_ranges(&a.range, &b.range));
            if projections > 1 {
                // tsgo `dedupeIdenticalEdits`: when a mapper copies one span
                // of the original into more than one projection, an edit
                // computed against each projection describes the same
                // change to the same original range.
                edits.dedup_by(|b, a| a.range == b.range && a.new_text == b.new_text);
            }
            // verify that change intervals do not overlap, except possibly at end points.
            if edits
                .windows(2)
                .any(|pair| text_edits_conflict(&pair[0], &pair[1], projections > 1))
            {
                if projections > 1 {
                    // Projections of one original range disagree about how to edit it. That is a property of
                    // the mapper's output rather than a bug here, so drop the file instead of failing.
                    unmappable.insert(file_name);
                    continue;
                }
                return Err(TrackerError::Overlap);
            }
            result.push((file_name, edits));
        }
        Ok(Changes {
            files: result,
            unmappable: unmappable.into_iter().collect(),
        })
    }
}

/// tsgo `textEditsConflict`: overlapping edits, or (from several
/// projections) different insertions at one position.
fn text_edits_conflict(a: &TextEdit, b: &TextEdit, multiple_projections: bool) -> bool {
    let position = |position: &tsc_lsproto::Position| (position.line, position.character);
    if position(&a.range.end) > position(&b.range.start) {
        return true;
    }
    // Different insertions at the same position are ambiguous when they may come from different projections.
    multiple_projections
        && a.range.start == a.range.end
        && a.range == b.range
        && a.new_text != b.new_text
}

/// What [`Tracker::get_changes`] writes the edits' texts and ranges with,
/// and the original files it found it cannot write back to.
struct EditWriter<'t, 'p> {
    transformation: &'t mut tsc_emitter::TransformationResult<'p>,
    format_settings: &'t FormatCodeSettings,
    new_line: NewLineKind,
    converters: &'t Converters,
    unmappable: BTreeSet<String>,
}

impl EditWriter<'_, '_> {
    /// tsgo `toLSPEditRange`: a range of a file as a range of its original
    /// file; one that does not fall within a single verbatim span cannot be
    /// written back, and its file is recorded so its edits are dropped (the
    /// best-effort range keeps the accumulated edits well-formed).
    fn lsp_edit_range(
        &mut self,
        script: &dyn Script,
        range: std::ops::Range<usize>,
    ) -> tsc_lsproto::Range {
        let (range, fidelity) = self.converters.to_lsp_range_with_fidelity(script, range);
        if !fidelity.is_exact() {
            self.unmappable
                .insert(script.original_file_name().to_owned());
        }
        range
    }

    /// tsgo `computeNewText`: the text of an edit, its nodes printed and
    /// formatted at each exact projection of its position (which must all
    /// agree).
    fn compute_new_text(
        &mut self,
        file: &SourceFile,
        script: &dyn Script,
        projections: &[&SourceFile],
        edit: &TrackerEdit,
    ) -> Result<String, TrackerError> {
        let nodes = match &edit.content {
            EditContent::Text(text) => return Ok(text.clone()),
            EditContent::ReplaceWithSingleNode(node) => std::slice::from_ref(node),
            EditContent::ReplaceWithMultipleNodes(nodes) => nodes.as_slice(),
        };
        let start = self.lsp_edit_range(script, edit.pos..edit.end).start;
        let scripts = projections
            .iter()
            .map(|projection| script_of(projection))
            .collect::<Vec<_>>();
        let scripts = scripts
            .iter()
            .map(|script| script as &dyn Script)
            .collect::<Vec<_>>();
        let positions =
            self.converters
                .from_lsp_position_for_scripts(&scripts, &start, FEATURES_ALL);
        let new_line_text = self.new_line.text();
        let mut result: Option<String> = None;
        // The original range may have multiple verbatim copies; it is safe to lose their identity only when
        // formatting at every exact projection produces the same edit.
        for mapped in positions {
            if !mapped.fidelity.is_exact() {
                continue;
            }
            let projection = projections[mapped.script];
            let pos = mapped.position;
            let mut texts = Vec::with_capacity(nodes.len());
            for &node in nodes {
                texts.push(get_formatted_text_of_node(
                    self.transformation,
                    self.format_settings,
                    self.new_line,
                    node,
                    file,
                    projection,
                    pos,
                    &edit.options,
                )?);
            }
            let text = match &edit.content {
                EditContent::ReplaceWithMultipleNodes(_) => {
                    let joiner = edit.options.joiner.as_deref().unwrap_or(new_line_text);
                    texts
                        .iter()
                        .map(|text| text.strip_suffix(new_line_text).unwrap_or(text))
                        .collect::<Vec<_>>()
                        .join(joiner)
                }
                _ => texts.remove(0),
            };
            // Strip initial indentation if text will be inserted in the middle of the line.
            let no_indent = if edit.options.indentation.is_some()
                || format::get_line_start_position_for_position(&File::new(projection), pos) == pos
            {
                text.as_str()
            } else {
                text.trim_start_matches(is_go_space)
            };
            let suffix = if no_indent.ends_with(edit.options.suffix.as_str()) {
                ""
            } else {
                edit.options.suffix.as_str()
            };
            let candidate = format!("{}{no_indent}{suffix}", edit.options.prefix);
            if result.as_ref().is_some_and(|result| *result != candidate) {
                self.unmappable
                    .insert(script.original_file_name().to_owned());
                return Ok(String::new());
            }
            result = Some(candidate);
        }
        let result = result.unwrap_or_else(|| {
            self.unmappable
                .insert(script.original_file_name().to_owned());
            String::new()
        });
        Ok(reindent_inserted_lines(script, self.new_line, edit, result))
    }
}

/// Go's `unicode.IsSpace`.
fn is_go_space(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r' | ' ' | '\u{0085}' | '\u{00A0}'
    ) || (ch > '\u{00FF}' && ch.is_whitespace())
}

/// tsgo `reindentInsertedLines`: an insertion that ends with a new line
/// gives the line it splits its indentation. The indentation is read from
/// the original text at the edit's original position, so it holds for a
/// content-mapped file whose projection is indented differently from the
/// document the edit is applied to.
fn reindent_inserted_lines(
    script: &dyn Script,
    new_line: NewLineKind,
    edit: &TrackerEdit,
    text: String,
) -> String {
    if text.is_empty() || edit.pos != edit.end || edit.options.indentation.is_some() {
        return text;
    }
    if !text.ends_with(new_line.text()) {
        return text;
    }
    let original = script.original_text();
    let mut pos = edit.pos;
    if let Some(span_map) = script.span_map() {
        let (mapped, fidelity) = span_map.virtual_to_original_position(pos as i32);
        if !fidelity.is_exact() {
            return text;
        }
        pos = mapped as usize;
    }
    if pos > original.len() {
        return text;
    }
    let line_start = original[..pos]
        .rfind(['\r', '\n'])
        .map_or(0, |index| index + 1);
    let before_point = &original[line_start..pos];
    if before_point.is_empty() {
        // At the start of a line: the existing text keeps its indentation, and the inserted line needs it —
        // but only when the formatter left the text at column zero. Where the formatter already indented it
        // (inserting into a multi-line list, say), that indentation is the correct one.
        if !leading_indentation(&text).is_empty() {
            return text;
        }
        return format!("{}{text}", leading_indentation(&original[line_start..]));
    }
    if leading_indentation(before_point) != before_point {
        return text;
    }
    // Just past the indentation: the inserted line already has it, the text pushed down needs it back.
    format!("{text}{before_point}")
}

fn leading_indentation(text: &str) -> &str {
    let end = text
        .bytes()
        .position(|byte| byte != b' ' && byte != b'\t')
        .unwrap_or(text.len());
    &text[..end]
}

/// tsgo `getFormattedTextOfNode`: `node` printed for `file` and formatted
/// as if at `pos` of `projection` (one of the file's projections).
#[allow(clippy::too_many_arguments)]
fn get_formatted_text_of_node(
    transformation: &mut tsc_emitter::TransformationResult<'_>,
    format_settings: &FormatCodeSettings,
    new_line: NewLineKind,
    node: TransformNode,
    file: &SourceFile,
    projection: &SourceFile,
    pos: usize,
    options: &NodeOptions,
) -> Result<String, TrackerError> {
    // tsgo `getNonformattedText`: printed with the file, positioned in a
    // synthetic file of the same name.
    let indent_size = usize::try_from(format_settings.editor_settings.indent_size).unwrap_or(4);
    let positioned = print_and_position(
        transformation,
        node,
        PositionedPrint {
            new_line,
            indent_size,
            with_source_file: true,
            language_variant: file.language_variant,
            snippet: None,
        },
    )?;
    let format_options = get_format_code_settings_for_writing(format_settings, file);
    let target = File::new(projection);
    let synthetic = File::new(&positioned.file);
    let new_line_text = new_line.text();
    let initial_indentation = match options.indentation {
        Some(indentation) => indentation,
        None => format::get_indentation(
            &target,
            pos,
            &format_options,
            options.prefix == new_line_text
                || format::get_line_start_position_for_position(&target, pos) == pos,
        ),
    };
    let delta = match options.delta {
        Some(delta) => delta,
        None => {
            let printed = synthetic
                .node_of(Some(positioned.node))
                .expect("the positioned node is in its synthetic file");
            if format_options.editor_settings.indent_size != 0
                && format::should_indent_child_node(
                    &format_options,
                    &synthetic,
                    printed,
                    None,
                    false,
                    false,
                )
            {
                format_options.editor_settings.indent_size
            } else {
                0
            }
        }
    };
    // tsgo formats with the tracker's settings, the synthetic file's whole
    // text.
    let changes = format::format_node_given_indentation(
        &synthetic,
        format_settings,
        synthetic.root(),
        initial_indentation,
        delta,
    );
    Ok(crate::formatting::apply_bulk_edits(
        &positioned.text,
        &changes,
    ))
}

/// tsgo `GetFormatCodeSettingsForWriting`: semicolons removed where the
/// file probably does without them.
pub(crate) fn get_format_code_settings_for_writing(
    options: &FormatCodeSettings,
    file: &SourceFile,
) -> FormatCodeSettings {
    let mut options = options.clone();
    let auto_detect = options.semicolons == SemicolonPreference::IGNORE;
    if options.semicolons == SemicolonPreference::REMOVE
        || auto_detect && !crate::lsutil::probably_uses_semicolons(file)
    {
        options.semicolons = SemicolonPreference::REMOVE;
    }
    options
}

/// tsgo `format.GetLineStartPositionForPosition`.
fn line_start_of(file: &SourceFile, position: usize) -> usize {
    line_start_at(file, line_of(file, position))
}

/// tsgo `scanner.ComputeLineOfPosition` over the file's ECMAScript lines.
fn line_of(file: &SourceFile, position: usize) -> usize {
    let position = position.min(file.text().len()) as u32;
    file.positions().line_of_byte(position).unwrap_or(0) as usize
}

/// The start of a line (the text's end past its last line).
fn line_start_at(file: &SourceFile, line: usize) -> usize {
    u32::try_from(line)
        .ok()
        .and_then(|line| file.positions().line_start_byte(line))
        .map_or(file.text().len(), |start| start as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tsgo `TestTextEditsConflictAtSameInsertionPointAcrossProjections`.
    #[test]
    fn text_edits_conflict_at_same_insertion_point_across_projections() {
        let position = tsc_lsproto::Position {
            line: 1,
            character: 2,
        };
        let edit = |text: &str| TextEdit {
            range: tsc_lsproto::Range {
                start: position.clone(),
                end: position.clone(),
            },
            new_text: text.to_owned(),
        };
        let (a, b) = (edit("a"), edit("b"));
        assert!(
            text_edits_conflict(&a, &b, true),
            "different insertions at the same position across projections should conflict"
        );
        assert!(
            !text_edits_conflict(&a, &b, false),
            "insertions at the same position within one projection should retain their existing ordering"
        );
    }
}
