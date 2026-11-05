#![forbid(unsafe_code)]

pub mod assignment;
pub mod bind;
pub mod containers;
pub mod declare;
pub mod flow;
pub mod hosted;
pub mod node_util;
pub mod symbols;

use tsc_syntax::SourceFile;

pub use assignment::{
    get_assignment_declaration_kind, get_assignment_declaration_property_access_kind,
    AssignmentDeclarationKind,
};
pub use declare::{published_has_locals, published_node_flags, published_node_flow, NodeSymbols};
pub use declare::{BindData, Binder, BinderMemory, BinderWorker, TableRef};
pub use hosted::jsdoc_hosted;
pub use symbols::{
    escape_leading_underscores, published_symbol, unescape_leading_underscores, Declarations,
    InternalSymbolName, NameKey, Symbol, SymbolArena, SymbolArenaExhausted, SymbolExtras, SymbolId,
    SymbolTable,
};

/// tsc bindSourceFile (42408): runs the binder over one parsed file and
/// returns it with its symbol tables, node links, flow graph, and bind
/// diagnostics.
///
/// The symbols of a published tree are published in the tree's domain.
pub fn bind_source_file(source_file: &SourceFile) -> Binder<'_> {
    if let Some(leases) = source_file.arena.identity_leases() {
        return Binder::bind_in_identity_domain(source_file, &leases.nodes.domain())
            .expect("bind identity allocation failed");
    }
    let mut binder = Binder::new(source_file);
    binder.bind_source_file();
    binder
}
