#![forbid(unsafe_code)]

use tsrs2_diags::DiagnosticList;

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
