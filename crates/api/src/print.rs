//! tsgo `api/session.go` (19dadef8) `printNode` (session.go:3529-3563): the
//! nodes a client encoded, decoded (`crate::decoder`) and printed as tsgo's
//! printer prints a decoded tree: a source file's statements with its
//! comments, any other node without source text.

use base64::Engine;
use tsc_emitter::{
    create_printer, transform_nodes, NewLineKind, PrintRequest, PrinterOptions, SourceFileTextMode,
    StandaloneWriter, TransformArena, TransformNode, TransformRoot,
};
use tsc_types::JsString;

use crate::decoder::decode_nodes;
use crate::ipc::Payload;
use crate::proto::PrintNodeParams;
use crate::session::{client_error, json, parse, Session};

impl Session {
    /// tsgo's `printNode`; `None` for another method.
    pub(crate) fn handle_print_request(
        &self,
        method: &str,
        params: &[u8],
    ) -> Option<Result<Payload, String>> {
        (method == "printNode").then(|| {
            parse::<PrintNodeParams>("PrintNodeParams", params)
                .and_then(|params| print_node(&params))
                .map(|text| json(&text))
        })
    }
}

/// tsgo `handlePrintNode`.
fn print_node(params: &PrintNodeParams) -> Result<String, String> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(&params.data)
        .map_err(|error| {
            client_error(format!(
                "invalid base64 data: illegal base64 data at input byte {}",
                base64_error_offset(&error, params.data.len())
            ))
        })?;
    let tree = decode_nodes(&data)
        .map_err(|error| client_error(format!("failed to decode AST: {error}")))?;
    let mut arena = TransformArena::new();
    let source = arena.add_source(&tree.source_file, None);
    for &literal in &tree.single_quoted {
        arena
            .literal_properties_mut(TransformNode::new(source, literal))
            .map_err(|error| format!("printNode: {error:?}"))?
            .set_string_literal_single_quote(true);
    }
    let roots = if tree.is_source_file {
        vec![TransformRoot::SourceFile(source)]
    } else {
        Vec::new()
    };
    let mut transformation = transform_nodes(arena, roots, Vec::new(), true)
        .map_err(|error| format!("printNode: {error:?}"))?;
    // tsgo `newPrinter`: its printer writes TypeScript syntax, and a source
    // file's tree rather than its text.
    let options = PrinterOptions::new(NewLineKind::LineFeed)
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
