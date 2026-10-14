//! tsgo `api/session.go` (19dadef8) `printNode` (session.go:3529-3563): the
//! nodes a client encoded, decoded (`crate::decoder`) and printed as tsgo's
//! printer prints a decoded tree: a source file's statements with its
//! comments, any other node without source text. And `formatNodeForInsertion`:
//! a decoded node printed for an insertion in a program's file, indented
//! and formatted there.

use base64::Engine;
use tsc_emitter::{
    create_printer, print_and_position_node, transform_nodes, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileTextMode, StandaloneWriter, TransformArena, TransformNode,
    TransformRoot,
};
use tsc_types::JsString;

use crate::decoder::{decode_nodes, DecodedTree};
use crate::encoder::PositionMap;
use crate::ipc::Payload;
use crate::proto::{FormatNodeForInsertionParams, PrintNodeParams};
use crate::session::{client_error, json, parse, Session};

impl Session {
    /// tsgo's `printNode` and `formatNodeForInsertion`; `None` for another
    /// method.
    pub(crate) fn handle_print_request(
        &self,
        method: &str,
        params: &[u8],
    ) -> Option<Result<Payload, String>> {
        match method {
            "printNode" => Some(
                parse::<PrintNodeParams>("PrintNodeParams", params)
                    .and_then(|params| print_node(&params))
                    .map(|text| json(&text)),
            ),
            "formatNodeForInsertion" => Some(
                parse::<FormatNodeForInsertionParams>("FormatNodeForInsertionParams", params)
                    .and_then(|params| self.format_node_for_insertion(&params))
                    .map(|text| json(&text)),
            ),
            _ => None,
        }
    }

    /// tsgo `handleFormatNodeForInsertion`: the node printed and positioned
    /// (`printer.PrintAndPositionNode`), then formatted as if inserted at
    /// the position of the file with the snapshot's format settings.
    fn format_node_for_insertion(
        &self,
        params: &FormatNodeForInsertionParams,
    ) -> Result<String, String> {
        let snapshot = self.snapshot(params.snapshot)?;
        let (_, program) = self.program(params.snapshot, &params.project)?;
        let target = self.required_source_file(&program, &params.file)?;
        let tree = decode(&params.data)?;
        let target = target.document.source();
        let position = PositionMap::new(target.text()).utf8(params.position) as usize;
        let options = &snapshot.user_preferences().format_code_settings;
        let new_line = if options.editor_settings.new_line_character == "\r\n" {
            NewLineKind::CarriageReturnLineFeed
        } else {
            NewLineKind::LineFeed
        };
        // tsgo `NewChangeTrackerWriter`: a negative indent size is the
        // default one.
        let indent_size = usize::try_from(options.editor_settings.indent_size).unwrap_or(4);
        let positioned = print_and_position_node(
            &tree.source_file,
            tree.root,
            &tree.single_quoted,
            new_line,
            indent_size,
            target.language_variant,
        )
        .map_err(|error| format!("formatNodeForInsertion: {error:?}"))?;
        Ok(tsc_ls::format_node_for_insertion(
            target,
            position,
            &positioned,
            options,
        ))
    }
}

/// tsgo `decodePrintNode`: the client's base64 nodes, decoded.
fn decode(data: &str) -> Result<DecodedTree, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|error| {
            client_error(format!(
                "invalid base64 data: illegal base64 data at input byte {}",
                base64_error_offset(&error, data.len())
            ))
        })?;
    decode_nodes(&bytes).map_err(|error| client_error(format!("failed to decode AST: {error}")))
}

/// tsgo `handlePrintNode`.
fn print_node(params: &PrintNodeParams) -> Result<String, String> {
    let tree = decode(&params.data)?;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&tree.source_file, None);
    arena
        .mark_single_quoted(source, &tree.single_quoted)
        .map_err(|error| format!("printNode: {error:?}"))?;
    let roots = if tree.is_source_file {
        vec![TransformRoot::SourceFile(source)]
    } else {
        Vec::new()
    };
    let mut transformation = transform_nodes(arena, roots, Vec::new(), true)
        .map_err(|error| format!("printNode: {error:?}"))?;
    // tsgo `newPrinter`: its printer writes TypeScript syntax, and a source
    // file's tree rather than its text. A node of no source file has no
    // comments to write (tsgo reads comments from the current source file).
    let options = PrinterOptions::new(NewLineKind::LineFeed)
        .with_remove_comments(!tree.is_source_file)
        .with_declaration_syntax(true)
        .with_never_ascii_escape(params.never_ascii_escape)
        .with_terminate_unterminated_literals(params.terminate_unterminated_literals)
        .with_source_file_text_mode(SourceFileTextMode::Canonical);
    let request = if tree.is_source_file {
        PrintRequest::SourceFile(source)
    } else {
        PrintRequest::StandaloneNode {
            node: TransformNode::new(source, tree.root),
            writer: StandaloneWriter::MultiLine,
        }
    };
    let printed = create_printer(options)
        .print(&mut transformation, request, None)
        .map_err(|error| format!("printNode: {error:?}"))?;
    Ok(JsString::from_code_units(printed.text_utf16().as_ref())
        .to_string_lossy()
        .into_owned())
}

/// The input byte Go's base64 decoder reports for `error`.
fn base64_error_offset(error: &base64::DecodeError, length: usize) -> usize {
    match *error {
        base64::DecodeError::InvalidByte(offset, _)
        | base64::DecodeError::InvalidLastSymbol(offset, _) => offset,
        base64::DecodeError::InvalidLength(_) | base64::DecodeError::InvalidPadding => length,
    }
}
