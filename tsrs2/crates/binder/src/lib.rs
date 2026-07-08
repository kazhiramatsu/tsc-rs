#![forbid(unsafe_code)]

use tsrs2_diags::DiagnosticList;
use tsrs2_syntax::SourceFile;

pub fn bind_source_file(_source_file: &SourceFile) -> DiagnosticList {
    Vec::new()
}

pub fn is_scaffolded() -> bool {
    true
}
