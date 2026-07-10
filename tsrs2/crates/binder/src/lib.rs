#![forbid(unsafe_code)]

pub mod declare;
pub mod node_util;
pub mod symbols;

use tsrs2_diags::DiagnosticList;
use tsrs2_syntax::SourceFile;

pub use declare::{Binder, TableRef};
pub use symbols::{
    escape_leading_underscores, unescape_leading_underscores, InternalSymbolName, Symbol,
    SymbolArena, SymbolId, SymbolTable,
};

pub fn bind_source_file(_source_file: &SourceFile) -> DiagnosticList {
    Vec::new()
}

pub fn is_scaffolded() -> bool {
    true
}
