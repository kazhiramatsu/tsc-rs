//! m2-binder-steps.md stage 3.1: the Symbol model (core-interfaces §2)
//! and the leading-underscore name escape.

use indexmap::IndexMap;
use tsrs2_syntax::NodeId;
use tsrs2_types::SymbolFlags;

pub use tsrs2_types::InternalSymbolName;
/// Symbol allocation identity. Defined in tsrs2-types (ty.rs) so
/// Type.symbol can reference symbols without a dependency cycle; the
/// binder owns the arena and the id space.
pub use tsrs2_types::SymbolId;

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
    /// getMergedSymbol chases this (checker-side merging, M4).
    pub merged_into: Option<SymbolId>,
    pub const_enum_only_module: Option<bool>,
    pub is_replaceable_by_method: bool,
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
            merged_into: None,
            const_enum_only_module: None,
            is_replaceable_by_method: false,
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

// The escape lives in tsrs2-syntax (the parser factory applies it to
// every Identifier escapedText); re-exported here for binder callers.
pub use tsrs2_syntax::{escape_leading_underscores, unescape_leading_underscores};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_adds_underscore_only_for_double_underscore_prefix() {
        assert_eq!(escape_leading_underscores("__proto__"), "___proto__");
        assert_eq!(escape_leading_underscores("__"), "___");
        assert_eq!(escape_leading_underscores("_x"), "_x");
        assert_eq!(escape_leading_underscores("x"), "x");
        assert_eq!(escape_leading_underscores(""), "");
        // Multi-byte first char must not satisfy the byte checks.
        assert_eq!(escape_leading_underscores("あ__"), "あ__");
    }

    #[test]
    fn unescape_strips_exactly_one_of_three_underscores() {
        assert_eq!(unescape_leading_underscores("___proto__"), "__proto__");
        assert_eq!(unescape_leading_underscores("__x"), "__x");
        assert_eq!(unescape_leading_underscores("___"), "__");
        assert_eq!(unescape_leading_underscores("x"), "x");
    }

    #[test]
    fn user_names_cannot_collide_with_internal_names() {
        // Internal names are inserted verbatim; the user spelling of the
        // same text escapes to a distinct key.
        assert_ne!(
            escape_leading_underscores("__call"),
            InternalSymbolName::CALL
        );
    }

    #[test]
    fn symbol_table_preserves_insertion_order() {
        let mut arena = SymbolArena::default();
        let mut table = SymbolTable::default();
        for name in ["z", "a", "m"] {
            let id = arena.alloc(SymbolFlags::NONE, name.to_owned());
            table.insert(name.to_owned(), id);
        }
        let keys: Vec<&str> = table.keys().map(String::as_str).collect();
        assert_eq!(keys, ["z", "a", "m"]);
    }

    #[test]
    fn arena_allocates_sequential_ids() {
        let mut arena = SymbolArena::default();
        let first = arena.alloc(SymbolFlags::NONE, "a".to_owned());
        let second = arena.alloc(SymbolFlags::NONE, "b".to_owned());
        assert_eq!(first, SymbolId(0));
        assert_eq!(second, SymbolId(1));
        assert_eq!(arena.symbol(second).escaped_name, "b");
        assert_eq!(arena.len(), 2);
    }
}
