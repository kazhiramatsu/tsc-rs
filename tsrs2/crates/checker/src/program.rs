//! ProgramBinder — the checker's program-wide view over per-file
//! binder runs (M4 5.0).
//!
//! tsc's checker sees one heap: nodes and symbols from every file (and
//! the checker's own transient symbols) share an identity space. The
//! greenfield equivalent: each file parses with a NodeId/NodeArrayId
//! base and binds with a SymbolId base (both continuing where the
//! previous file ended), so ids are program-unique by construction and
//! this struct only routes an id to its owning per-file arena. Checker
//! transient symbols (tsc createSymbol 47652) allocate above all files.

use tsrs2_binder::{Binder, Symbol, SymbolArena, SymbolId, SymbolTable};
use tsrs2_syntax::{NodeArray, NodeArrayId, NodeId, SourceFile};
use tsrs2_types::SymbolFlags;

pub struct ProgramBinder<'a> {
    /// Per-file binder runs in program order.
    file_binders: Vec<Binder<'a>>,
    /// Cached per-file node-id bases (ascending) for owner lookup.
    node_bases: Vec<u32>,
    /// Cached per-file node-array-id bases (ascending).
    array_bases: Vec<u32>,
    /// Cached per-file symbol-id bases (ascending) for owner lookup.
    symbol_bases: Vec<u32>,
    /// Checker-side symbols (tsc createSymbol 47652 adds Transient).
    transient: SymbolArena,
}

impl<'a> ProgramBinder<'a> {
    pub fn new(file_binders: Vec<Binder<'a>>) -> Self {
        assert!(
            !file_binders.is_empty(),
            "a program has at least one source file"
        );
        let node_bases: Vec<u32> = file_binders
            .iter()
            .map(|binder| binder.source.arena.node_base())
            .collect();
        let array_bases: Vec<u32> = file_binders
            .iter()
            .map(|binder| binder.source.arena.array_base())
            .collect();
        let symbol_bases: Vec<u32> = file_binders
            .iter()
            .map(|binder| binder.symbols.base())
            .collect();
        // The bases must be ascending and contiguous with each file's
        // allocation count, or owner lookup by range is meaningless.
        for pair in file_binders.windows(2) {
            assert_eq!(
                pair[1].source.arena.node_base(),
                pair[0].source.arena.node_end(),
                "program files must parse with contiguous node bases"
            );
            assert_eq!(
                pair[1].symbols.base(),
                pair[0].symbols.next_id().0,
                "program files must bind with contiguous symbol bases"
            );
        }
        let transient_base = file_binders.last().expect("non-empty").symbols.next_id().0;
        Self {
            file_binders,
            node_bases,
            array_bases,
            symbol_bases,
            transient: SymbolArena::with_base(transient_base),
        }
    }

    pub fn file_count(&self) -> usize {
        self.file_binders.len()
    }

    pub fn files(&self) -> impl Iterator<Item = &Binder<'a>> {
        self.file_binders.iter()
    }

    pub fn file(&self, index: usize) -> &Binder<'a> {
        &self.file_binders[index]
    }

    pub fn source(&self, index: usize) -> &'a SourceFile {
        self.file_binders[index].source
    }

    /// Owning file of a node id (nodes allocate contiguously per file).
    pub fn file_index_of_node(&self, node: NodeId) -> usize {
        match self.node_bases.binary_search(&node.0) {
            Ok(index) => index,
            Err(insert) => insert - 1,
        }
    }

    pub fn source_of_node(&self, node: NodeId) -> &'a SourceFile {
        let source = self.file_binders[self.file_index_of_node(node)].source;
        debug_assert!(
            source.arena.contains_node(node),
            "NodeId {node:?} out of range"
        );
        source
    }

    fn binder_of_node(&self, node: NodeId) -> &Binder<'a> {
        &self.file_binders[self.file_index_of_node(node)]
    }

    /// Owning file's arena lookup for a node-array id (arrays allocate
    /// contiguously per file, like nodes).
    pub fn node_array(&self, id: NodeArrayId) -> &'a NodeArray {
        let index = match self.array_bases.binary_search(&id.0) {
            Ok(index) => index,
            Err(insert) => insert - 1,
        };
        self.file_binders[index].source.arena.node_array(id)
    }

    fn owner_of_symbol(&self, id: SymbolId) -> Result<usize, ()> {
        if self.transient.contains(id) {
            return Err(());
        }
        match self.symbol_bases.binary_search(&id.0) {
            Ok(index) => Ok(index),
            Err(insert) => Ok(insert - 1),
        }
    }

    pub fn symbol(&self, id: SymbolId) -> &Symbol {
        match self.owner_of_symbol(id) {
            Ok(file) => self.file_binders[file].symbols.symbol(id),
            Err(()) => self.transient.symbol(id),
        }
    }

    pub fn symbol_mut(&mut self, id: SymbolId) -> &mut Symbol {
        match self.owner_of_symbol(id) {
            Ok(file) => self.file_binders[file].symbols.symbol_mut(id),
            Err(()) => self.transient.symbol_mut(id),
        }
    }

    /// tsc-port: createSymbol @6.0.3
    /// tsc-hash: b9b2c65d71ec1e9d3a55d36fe5224e5f31dd618ee1428293b371d2f2881ad16a
    /// tsc-span: _tsc.js:47652-47658
    ///
    /// Checker-side symbol creation: always Transient. (tsc also seeds
    /// links.checkFlags here; ours live in LinksTables and default 0 —
    /// callers that need CheckFlags set them through the links API.)
    pub fn create_symbol(&mut self, flags: SymbolFlags, escaped_name: String) -> SymbolId {
        self.transient
            .alloc(flags | SymbolFlags::TRANSIENT, escaped_name)
    }

    /// tsc container.locals of a scope-owning node.
    pub fn locals_of(&self, scope: NodeId) -> Option<&SymbolTable> {
        self.binder_of_node(scope).locals.get(&scope)
    }

    /// tsc node.symbol (addDeclarationToSymbol).
    pub fn node_symbol(&self, node: NodeId) -> Option<SymbolId> {
        self.binder_of_node(node).node_symbol.get(&node).copied()
    }

    /// The binder's mutable node-flags view (ContainsThis etc.).
    pub fn flags_of(&self, node: NodeId) -> tsrs2_types::NodeFlags {
        self.binder_of_node(node).flags_of(node)
    }
}
