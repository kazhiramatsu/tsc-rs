//! The members table of a resolved type.

use std::hash::{Hash, Hasher};

use hashbrown::hash_table::{Entry, HashTable};
use rustc_hash::FxHasher;
use tsc_binder::{SymbolId, SymbolTable};
use tsc_diagnostics::JsStr;

use crate::program::ProgramBinder;

/// tsc's resolved-type members (setStructuredTypeMembers 50198): the member
/// symbols in insertion order, found by name.
///
/// tsrs-native: a checker resolves the members of nearly every object type
/// it touches (1.2 million tables on VS Code with one checker), and a
/// member's key is always the symbol's own escaped name. The table therefore
/// stores each member as its four-byte identity and hashes the names through
/// the symbol records, where the binder's [`SymbolTable`] copies every name
/// into its entries. Iteration follows insertion order, never the hash.
#[derive(Clone, Debug, Default)]
pub struct MemberTable {
    symbols: Vec<SymbolId>,
    /// Positions in `symbols`, hashed by the symbol's escaped name.
    positions: HashTable<u32>,
}

fn hash_name(name: &[u8]) -> u64 {
    let mut hasher = FxHasher::default();
    name.hash(&mut hasher);
    hasher.finish()
}

fn name_of<'b>(binder: &'b ProgramBinder<'_>, symbol: SymbolId) -> &'b [u8] {
    binder.symbol(symbol).escaped_name.as_js().as_bytes()
}

impl MemberTable {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            symbols: Vec::with_capacity(capacity),
            positions: HashTable::with_capacity(capacity),
        }
    }

    /// The members of `table`, whose keys are the members' names.
    pub fn from_symbol_table(binder: &ProgramBinder<'_>, table: &SymbolTable) -> Self {
        let mut members = Self::with_capacity(table.len());
        for (name, &symbol) in table.iter() {
            debug_assert_eq!(
                name.as_js().as_bytes(),
                name_of(binder, symbol),
                "a member table key is the member's escaped name"
            );
            members.insert(binder, symbol);
        }
        members
    }

    /// The listed symbols, in order, under their own names.
    pub fn from_symbols(binder: &ProgramBinder<'_>, symbols: &[SymbolId]) -> Self {
        let mut members = Self::with_capacity(symbols.len());
        for &symbol in symbols {
            members.insert(binder, symbol);
        }
        members
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    /// The members in insertion order.
    pub fn symbols(&self) -> &[SymbolId] {
        &self.symbols
    }

    /// Room for `additional` more members without regrowing.
    pub fn reserve(&mut self, binder: &ProgramBinder<'_>, additional: usize) {
        self.symbols.reserve(additional);
        let symbols = &self.symbols;
        self.positions.reserve(additional, |&position| {
            hash_name(name_of(binder, symbols[position as usize]))
        });
    }

    pub fn get<'a>(
        &self,
        binder: &ProgramBinder<'_>,
        name: impl Into<JsStr<'a>>,
    ) -> Option<SymbolId> {
        let name = name.into();
        let name = name.as_bytes();
        self.positions
            .find(hash_name(name), |&position| {
                name_of(binder, self.symbols[position as usize]) == name
            })
            .map(|&position| self.symbols[position as usize])
    }

    /// Add `symbol` under its own name; a member of that name already present
    /// is replaced in its position, as in a JavaScript `Map`, and returned.
    pub fn insert(&mut self, binder: &ProgramBinder<'_>, symbol: SymbolId) -> Option<SymbolId> {
        let name = name_of(binder, symbol);
        let symbols = &self.symbols;
        let entry = self.positions.entry(
            hash_name(name),
            |&position| name_of(binder, symbols[position as usize]) == name,
            |&position| hash_name(name_of(binder, symbols[position as usize])),
        );
        match entry {
            Entry::Occupied(slot) => {
                let position = *slot.get() as usize;
                Some(std::mem::replace(&mut self.symbols[position], symbol))
            }
            Entry::Vacant(slot) => {
                let position = u32::try_from(self.symbols.len()).expect("member table size");
                slot.insert(position);
                self.symbols.push(symbol);
                None
            }
        }
    }

    /// Bytes this table owns on the heap, for memory accounting.
    pub fn heap_bytes(&self) -> usize {
        self.symbols.capacity() * std::mem::size_of::<SymbolId>()
            + self.positions.capacity() * (std::mem::size_of::<u32>() + 1)
    }
}
