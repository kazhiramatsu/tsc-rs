#![forbid(unsafe_code)]

pub mod assignment;
pub mod bind;
pub mod containers;
pub mod declare;
pub mod flow;
pub mod node_util;
pub mod symbols;

use tsrs2_syntax::SourceFile;
use tsrs2_types::CompilerOptions;

pub use assignment::{get_assignment_declaration_kind, AssignmentDeclarationKind};
pub use declare::{Binder, TableRef};
pub use symbols::{
    escape_leading_underscores, unescape_leading_underscores, InternalSymbolName, Symbol,
    SymbolArena, SymbolId, SymbolTable,
};

/// tsc bindSourceFile (42408): runs the binder over one parsed file and
/// returns it with its symbol tables, node links, flow graph, and bind
/// diagnostics.
pub fn bind_source_file<'a>(
    source_file: &'a SourceFile,
    options: &'a CompilerOptions,
) -> Binder<'a> {
    let mut binder = Binder::new(source_file, options);
    binder.bind_source_file();
    binder
}

pub fn is_scaffolded() -> bool {
    true
}
