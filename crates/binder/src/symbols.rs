//! m2-binder-steps.md stage 3.1: the Symbol model (core-interfaces §2)
//! and the leading-underscore name escape.

use indexmap::IndexMap;
use std::sync::Arc;

use tsc_syntax::NodeId;
use tsc_types::{
    EscapedName, IdentityError, IdentityLease, IdentityRange, IdentitySpace, JsStr, SymbolFlags,
    TRANSIENT_SYMBOL_BIT,
};

pub use tsc_types::InternalSymbolName;
/// Symbol allocation identity. Defined in tsc-rs-types (ty.rs) so
/// Type.symbol can reference symbols without a dependency cycle; the
/// binder owns the arena and the id space.
pub use tsc_types::SymbolId;

/// tsc SymbolTable: ORDERED name → symbol map. Iteration order is
/// observable (member synthesis and display order downstream), so it is
/// insertion order, never hash order. Keys are stored PRE-escaped.
mod table;
pub use table::{EscapedNameSet, SymbolTable};

mod declarations;
pub use declarations::Declarations;

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
    pub escaped_name: EscapedName,
    pub declarations: Declarations,
    /// addDeclarationToSymbol: FIRST value declaration wins.
    pub value_declaration: Option<NodeId>,
    /// Shared with every checker that resolves this symbol's members: the
    /// binder fills the table in place, readers clone the handle.
    pub members: Arc<SymbolTable>,
    pub exports: Arc<SymbolTable>,
    /// tsc Symbol.globalExports (bindNamespaceExportDeclaration).
    pub global_exports: Arc<SymbolTable>,
    pub parent: Option<SymbolId>,
    /// local ↔ export link installed by declareModuleMember.
    pub export_symbol: Option<SymbolId>,
    pub const_enum_only_module: Option<bool>,
    pub is_replaceable_by_method: bool,
    /// tsc Symbol.assignmentDeclarationMembers: dynamically named JS
    /// assignments are late-bound when the containing symbol's
    /// members/exports are resolved.
    /// Allocated on the first assignment-declaration member: the map is
    /// empty for almost every symbol, so it costs one pointer until then.
    pub assignment_declaration_members: Option<Box<IndexMap<NodeId, NodeId>>>,
}

impl Symbol {
    pub fn new(flags: SymbolFlags, escaped_name: EscapedName) -> Self {
        Self {
            flags,
            escaped_name,
            declarations: Declarations::new(),
            value_declaration: None,
            members: empty_symbol_table(),
            exports: empty_symbol_table(),
            global_exports: empty_symbol_table(),
            parent: None,
            export_symbol: None,
            const_enum_only_module: None,
            is_replaceable_by_method: false,
            assignment_declaration_members: None,
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
#[derive(Clone, Debug, Default)]
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
    /// Symbol count, the arena's bytes, and the heap bytes of the symbols'
    /// names and declaration lists; `tables` collects each distinct
    /// member/export table once (they are shared between symbols).
    pub fn memory_usage(
        &self,
        tables: &mut rustc_hash::FxHashMap<*const SymbolTable, usize>,
    ) -> (usize, usize, usize) {
        let mut owned = 0;
        for symbol in &self.symbols {
            owned += symbol.escaped_name.heap_bytes() + symbol.declarations.heap_bytes();
            for table in [&symbol.members, &symbol.exports, &symbol.global_exports] {
                tables
                    .entry(std::sync::Arc::as_ptr(table))
                    .or_insert_with(|| table.heap_bytes());
            }
            if let Some(members) = &symbol.assignment_declaration_members {
                owned += members.capacity() * (2 * std::mem::size_of::<NodeId>() + 16);
            }
        }
        (
            self.symbols.len(),
            self.symbols.capacity() * std::mem::size_of::<Symbol>(),
            owned,
        )
    }

    pub fn with_base(base: u32) -> Self {
        Self::with_base_and_capacity(base, 0)
    }

    /// An arena at `base` with room for `capacity` symbols before it grows.
    pub fn with_base_and_capacity(base: u32, capacity: usize) -> Self {
        Self {
            symbols: Vec::with_capacity(capacity),
            base,
            lease: None,
        }
    }

    /// Room for `additional` more symbols without moving the arena: a
    /// checker sizes its transient arena from its share of the program
    /// instead of doubling (and copying 176 bytes per symbol) as it grows.
    pub fn reserve(&mut self, additional: usize) {
        self.symbols.reserve(additional);
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

    pub fn alloc(&mut self, flags: SymbolFlags, escaped_name: EscapedName) -> SymbolId {
        self.try_alloc(flags, escaped_name)
            .expect("symbol identity space exhausted")
    }

    pub fn try_alloc(
        &mut self,
        flags: SymbolFlags,
        escaped_name: EscapedName,
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
            for table in [
                &mut symbol.members,
                &mut symbol.exports,
                &mut symbol.global_exports,
            ] {
                if !table.is_empty() {
                    relocate_symbol_table_values(Arc::make_mut(table), &relocation)?;
                }
            }
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

    /// Attach a lease reserved before binding: it starts at the arena base
    /// and may run past the allocated count (an over-approximation leased
    /// from the file's node count). `Ok(false)` reports an arena that
    /// outgrew its reservation and must relocate instead; nothing is
    /// attached in that case.
    pub(crate) fn attach_reserved_identity_lease(
        &mut self,
        lease: IdentityLease,
    ) -> Result<bool, IdentityError> {
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
        if lease.range().start() != self.base {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Symbol,
                detail: "reserved symbol lease base differs from the arena base",
            });
        }
        let count = u32::try_from(self.symbols.len()).map_err(|_| IdentityError::Exhausted {
            space: IdentitySpace::Symbol,
            requested: u32::MAX,
            limit: TRANSIENT_SYMBOL_BIT,
        })?;
        if lease.range().len() < count {
            return Ok(false);
        }
        self.lease = Some(lease);
        Ok(true)
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
pub fn escape_leading_underscores<'a>(raw: impl Into<JsStr<'a>>) -> EscapedName {
    EscapedName::escape(raw.into())
}

pub fn unescape_leading_underscores<'a>(escaped: impl Into<JsStr<'a>>) -> JsStr<'a> {
    let text = escaped.into();
    if text.starts_with("___") {
        text.strip_prefix("_")
            .expect("three underscores start with one")
    } else {
        text
    }
}

#[cfg(test)]
#[path = "../tests/unit/symbols/tests.rs"]
mod tests;

/// The empty member table every fresh symbol starts with: one shared
/// allocation per thread, so that binding files on several threads never
/// contends on one reference count (a process-wide table made every symbol
/// creation an atomic write to the same cache line, and the parallel bind
/// six times slower). The first insertion into a symbol's table makes that
/// symbol its own copy.
pub fn empty_symbol_table() -> Arc<SymbolTable> {
    thread_local! {
        static EMPTY: Arc<SymbolTable> = Arc::new(SymbolTable::default());
    }
    EMPTY.with(Arc::clone)
}
