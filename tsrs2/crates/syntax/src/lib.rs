#![forbid(unsafe_code)]

pub mod for_each_child;
pub mod kind;
pub mod nodes;

use tsrs2_diags::DiagnosticList;

pub use for_each_child::{for_each_child, NodeLookup};
pub use kind::SyntaxKind;
pub use nodes::{Node, NodeArray, NodeArrayId, NodeData, NodeId, NodePayload};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    pub file_name: String,
    pub text: String,
}

pub fn parse_source_file(
    file_name: impl Into<String>,
    text: impl Into<String>,
) -> (SourceFile, DiagnosticList) {
    (
        SourceFile {
            file_name: file_name.into(),
            text: text.into(),
        },
        Vec::new(),
    )
}
