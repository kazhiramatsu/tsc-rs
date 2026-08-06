//! m2-binder-steps.md stage 3.1: the Symbol model (core-interfaces §2)
//! and the leading-underscore name escape.

use indexmap::IndexMap;
use tsc_syntax::NodeId;
use tsc_types::SymbolFlags;

pub use tsc_types::InternalSymbolName;
/// Symbol allocation identity. Defined in tsc-rs-types (ty.rs) so
/// Type.symbol can reference symbols without a dependency cycle; the
/// binder owns the arena and the id space.
pub use tsc_types::SymbolId;

/// tsc SymbolTable: ORDERED name → symbol map. Iteration order is
/// observable (member synthesis and display order downstream), so this
/// is an IndexMap, never a HashMap. Keys are stored PRE-escaped.
pub type SymbolTable = IndexMap<String, SymbolId>;

/// core-interfaces §2 (tsc Symbol, D6533). tsc creates `members`/
/// `exports` lazily on first insertion; here an empty table means
/// "absent" — the audit format cannot distinguish the two, and no
/// ported code branches on table existence alone.
#[derive(Clone, Debug)]
pub struct Symbol {
    pub flags: SymbolFlags,
    /// tsc escapedName: stored pre-escaped via
    /// [`escape_leading_underscores`]; internal names (`__call`, …)
    /// are inserted verbatim, which is exactly why user `__call`
    /// escapes to `___call` and cannot collide.
    pub escaped_name: String,
    pub declarations: Vec<NodeId>,
    /// addDeclarationToSymbol: FIRST value declaration wins.
    pub value_declaration: Option<NodeId>,
    pub members: SymbolTable,
    pub exports: SymbolTable,
    /// tsc Symbol.globalExports (bindNamespaceExportDeclaration).
    pub global_exports: SymbolTable,
    pub parent: Option<SymbolId>,
    /// local ↔ export link installed by declareModuleMember.
    pub export_symbol: Option<SymbolId>,
    pub const_enum_only_module: Option<bool>,
    pub is_replaceable_by_method: bool,
    /// tsc Symbol.assignmentDeclarationMembers: dynamically named JS
    /// assignments are late-bound when the containing symbol's
    /// members/exports are resolved.
    pub assignment_declaration_members: IndexMap<NodeId, NodeId>,
}

impl Symbol {
    pub fn new(flags: SymbolFlags, escaped_name: String) -> Self {
        Self {
            flags,
            escaped_name,
            declarations: Vec::new(),
            value_declaration: None,
            members: SymbolTable::default(),
            exports: SymbolTable::default(),
            global_exports: SymbolTable::default(),
            parent: None,
            export_symbol: None,
            const_enum_only_module: None,
            is_replaceable_by_method: false,
            assignment_declaration_members: IndexMap::new(),
        }
    }
}

/// All symbols created while binding one source file.
///
/// Program-wide id base (M4 5.0): tsc symbols are heap objects with
/// program-unique identity; per-file arenas get the same property by
/// allocating SymbolId from a per-file base (the checker binds file N
/// with the base continuing where file N-1 ended, then allocates its
/// own transient symbols above all files). Single-file paths keep 0.
#[derive(Debug, Default)]
pub struct SymbolArena {
    symbols: Vec<Symbol>,
    base: u32,
}

impl SymbolArena {
    pub fn with_base(base: u32) -> Self {
        Self {
            symbols: Vec::new(),
            base,
        }
    }

    pub fn base(&self) -> u32 {
        self.base
    }

    /// One past the last allocated SymbolId — the next arena's base.
    pub fn next_id(&self) -> SymbolId {
        SymbolId(self.base + self.symbols.len() as u32)
    }

    pub fn contains(&self, id: SymbolId) -> bool {
        id.0 >= self.base && id.0 < self.base + self.symbols.len() as u32
    }

    pub fn alloc(&mut self, flags: SymbolFlags, escaped_name: String) -> SymbolId {
        let id = self.next_id();
        self.symbols.push(Symbol::new(flags, escaped_name));
        id
    }

    fn index(&self, id: SymbolId) -> usize {
        assert!(
            id.0 >= self.base,
            "SymbolId below arena base: {id:?} (base {})",
            self.base
        );
        (id.0 - self.base) as usize
    }

    pub fn symbol(&self, id: SymbolId) -> &Symbol {
        &self.symbols[self.index(id)]
    }

    pub fn symbol_mut(&mut self, id: SymbolId) -> &mut Symbol {
        let index = self.index(id);
        &mut self.symbols[index]
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

// The escape lives in tsc-rs-syntax (the parser factory applies it to
// every Identifier escapedText); re-exported here for binder callers.
pub use tsc_syntax::{escape_leading_underscores, unescape_leading_underscores};

#[cfg(test)]
#[path = "../tests/unit/symbols/tests.rs"]
mod tests;
