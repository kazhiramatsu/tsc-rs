//! m2-binder-steps.md stage 3.1: the Symbol model (core-interfaces §2)
//! and the leading-underscore name escape.

use indexmap::IndexMap;
use tsc_syntax::NodeId;
use tsc_types::{
    IdentityError, IdentityLease, IdentityRange, IdentitySpace, SymbolFlags, TRANSIENT_SYMBOL_BIT,
};

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
#[derive(Clone, Debug, PartialEq)]
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
    lease: Option<IdentityLease>,
}

impl PartialEq for SymbolArena {
    fn eq(&self, other: &Self) -> bool {
        self.symbols == other.symbols && self.base == other.base
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SymbolIdentityRelocation {
    old: IdentityRange,
    new: IdentityRange,
}

impl SymbolIdentityRelocation {
    pub(crate) fn symbol(&self, id: &mut SymbolId) -> Result<(), IdentityError> {
        if self.old.len() != self.new.len() {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Symbol,
                detail: "symbol relocation ranges have different lengths",
            });
        }
        let offset =
            id.0.checked_sub(self.old.start())
                .filter(|offset| *offset < self.old.len())
                .ok_or(IdentityError::InvalidLease {
                    space: IdentitySpace::Symbol,
                    detail: "relocated SymbolId is outside its source arena",
                })?;
        id.0 = self
            .new
            .start()
            .checked_add(offset)
            .ok_or(IdentityError::InvalidLease {
                space: IdentitySpace::Symbol,
                detail: "relocated SymbolId overflowed",
            })?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SymbolArenaExhausted {
    pub transient: bool,
    pub limit: u32,
}

impl std::fmt::Display for SymbolArenaExhausted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} symbol identity space exhausted below {}",
            if self.transient {
                "checker-transient"
            } else {
                "persistent"
            },
            self.limit
        )
    }
}

impl std::error::Error for SymbolArenaExhausted {}

impl SymbolArena {
    pub fn with_base(base: u32) -> Self {
        Self {
            symbols: Vec::new(),
            base,
            lease: None,
        }
    }

    pub fn base(&self) -> u32 {
        self.base
    }

    pub fn identity_lease(&self) -> Option<&IdentityLease> {
        self.lease.as_ref()
    }

    /// One past the last allocated SymbolId — the next arena's base.
    pub fn next_id(&self) -> SymbolId {
        SymbolId(
            self.base
                .checked_add(
                    u32::try_from(self.symbols.len()).expect("symbol arena length exceeds u32"),
                )
                .expect("symbol identity space exhausted"),
        )
    }

    pub fn contains(&self, id: SymbolId) -> bool {
        id.0 >= self.base && id.0 < self.next_id().0
    }

    pub fn alloc(&mut self, flags: SymbolFlags, escaped_name: String) -> SymbolId {
        self.try_alloc(flags, escaped_name)
            .expect("symbol identity space exhausted")
    }

    pub fn try_alloc(
        &mut self,
        flags: SymbolFlags,
        escaped_name: String,
    ) -> Result<SymbolId, SymbolArenaExhausted> {
        let transient = self.base >= TRANSIENT_SYMBOL_BIT;
        let limit = if transient {
            u32::MAX
        } else {
            TRANSIENT_SYMBOL_BIT
        };
        let offset = u32::try_from(self.symbols.len())
            .map_err(|_| SymbolArenaExhausted { transient, limit })?;
        let raw = self
            .base
            .checked_add(offset)
            .filter(|raw| *raw < limit)
            .ok_or(SymbolArenaExhausted { transient, limit })?;
        let id = SymbolId(raw);
        self.symbols.push(Symbol::new(flags, escaped_name));
        Ok(id)
    }

    pub(crate) fn identity_relocation(
        &self,
        lease: &IdentityLease,
    ) -> Result<SymbolIdentityRelocation, IdentityError> {
        if self.lease.is_some() {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Symbol,
                detail: "symbol arena is already identity-owned",
            });
        }
        if lease.space() != IdentitySpace::Symbol {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Symbol,
                detail: "symbol arena received a non-symbol lease",
            });
        }
        let count = u32::try_from(self.symbols.len()).map_err(|_| IdentityError::Exhausted {
            space: IdentitySpace::Symbol,
            requested: u32::MAX,
            limit: TRANSIENT_SYMBOL_BIT,
        })?;
        if lease.range().len() != count {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Symbol,
                detail: "symbol lease length differs from the arena allocation count",
            });
        }
        Ok(SymbolIdentityRelocation {
            old: IdentityRange::new(
                self.base,
                self.base
                    .checked_add(count)
                    .ok_or(IdentityError::InvalidLease {
                        space: IdentitySpace::Symbol,
                        detail: "source symbol arena end overflowed",
                    })?,
            ),
            new: lease.range(),
        })
    }

    pub(crate) fn apply_identity_relocation(
        &mut self,
        relocation: SymbolIdentityRelocation,
        lease: IdentityLease,
    ) -> Result<(), IdentityError> {
        for symbol in &mut self.symbols {
            relocate_symbol_table_values(&mut symbol.members, &relocation)?;
            relocate_symbol_table_values(&mut symbol.exports, &relocation)?;
            relocate_symbol_table_values(&mut symbol.global_exports, &relocation)?;
            if let Some(parent) = &mut symbol.parent {
                relocation.symbol(parent)?;
            }
            if let Some(export_symbol) = &mut symbol.export_symbol {
                relocation.symbol(export_symbol)?;
            }
        }
        self.base = relocation.new.start();
        self.lease = Some(lease);
        Ok(())
    }

    pub(crate) fn attach_identity_lease(
        &mut self,
        lease: IdentityLease,
    ) -> Result<(), IdentityError> {
        let relocation = self.identity_relocation(&lease)?;
        if relocation.old != relocation.new {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Symbol,
                detail: "direct-construction symbol lease base differs from the arena base",
            });
        }
        self.lease = Some(lease);
        Ok(())
    }

    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    pub(crate) fn symbols_mut(&mut self) -> &mut [Symbol] {
        &mut self.symbols
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

pub(crate) fn relocate_symbol_table_values(
    table: &mut SymbolTable,
    relocation: &SymbolIdentityRelocation,
) -> Result<(), IdentityError> {
    for symbol in table.values_mut() {
        relocation.symbol(symbol)?;
    }
    Ok(())
}

// The escape lives in tsc-rs-syntax (the parser factory applies it to
// every Identifier escapedText); re-exported here for binder callers.
pub use tsc_syntax::{escape_leading_underscores, unescape_leading_underscores};

#[cfg(test)]
#[path = "../tests/unit/symbols/tests.rs"]
mod tests;
