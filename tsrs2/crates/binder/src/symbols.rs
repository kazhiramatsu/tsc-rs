//! m2-binder-steps.md stage 3.1: the Symbol model (core-interfaces §2)
//! and the leading-underscore name escape.

use indexmap::IndexMap;
use tsrs2_syntax::NodeId;
use tsrs2_types::SymbolFlags;

pub use tsrs2_types::InternalSymbolName;

/// Symbol allocation identity within one binder run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SymbolId(pub u32);

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
            parent: None,
            export_symbol: None,
            merged_into: None,
            const_enum_only_module: None,
            is_replaceable_by_method: false,
        }
    }
}

/// All symbols created while binding one source file.
#[derive(Debug, Default)]
pub struct SymbolArena {
    symbols: Vec<Symbol>,
}

impl SymbolArena {
    pub fn alloc(&mut self, flags: SymbolFlags, escaped_name: String) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(Symbol::new(flags, escaped_name));
        id
    }

    pub fn symbol(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.0 as usize]
    }

    pub fn symbol_mut(&mut self, id: SymbolId) -> &mut Symbol {
        &mut self.symbols[id.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

/// tsc-port: escapeLeadingUnderscores @6.0.3
/// tsc-hash: 86d7f97e898c96c6de2e47109d4583e4446ba8a518842f34d0d3cd4aa1b0b3c4
/// tsc-span: _tsc.js:11438-11440
///
/// A name beginning with two underscores gains ONE more, so user
/// `__proto__` cannot collide with internal names (`__call` etc. are
/// stored unescaped). The charCodeAt checks are byte checks: `_` is
/// ASCII, so a multi-byte first char never matches.
pub fn escape_leading_underscores(name: &str) -> String {
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'_' && bytes[1] == b'_' {
        format!("_{name}")
    } else {
        name.to_owned()
    }
}

/// tsc-port: unescapeLeadingUnderscores @6.0.3
/// tsc-hash: e8294a1e4ef10b8ca2bcce06045e22adab6689e46b655acf51bacc3810ef5271
/// tsc-span: _tsc.js:11441-11444
///
/// Display-time inverse: exactly three leading underscores drop one.
pub fn unescape_leading_underscores(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'_' && bytes[1] == b'_' && bytes[2] == b'_' {
        &name[1..]
    } else {
        name
    }
}

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
