//! The members table of a resolved type.

use std::hash::{Hash, Hasher};

use hashbrown::hash_table::{Entry, HashTable};
use rustc_hash::FxHasher;
use tsc_binder::{NameKey, SymbolId, SymbolTable};
use tsc_types::EscapedName;

use crate::program::ProgramBinder;

/// tsc's resolved-type members (setStructuredTypeMembers 50198): the member
/// symbols in insertion order, found by name.
///
/// tsrs-native: a checker resolves the members of nearly every object type
/// it touches (1.2 million tables on VS Code with one checker), and a
/// member's key is always the symbol's own escaped name. The table therefore
/// stores each member as its four-byte identity and hashes the name ids
/// through the symbol records, where the binder's [`SymbolTable`] keeps a
/// name beside every symbol. Iteration follows insertion order, never the
/// hash.
#[derive(Clone, Debug, Default)]
pub struct MemberTable {
    symbols: Vec<SymbolId>,
    /// Positions in `symbols`, hashed by the symbol's escaped name. A table
    /// of at most [`LINEAR_MEMBERS`] members keeps no index (the empty
    /// table, which allocates nothing) and is searched in order: most
    /// resolved types have a handful of members, and the index cost more
    /// than the members themselves.
    positions: HashTable<u32>,
}

/// The most members a table is searched in order for.
const LINEAR_MEMBERS: usize = 8;

/// One hash shared by an own-member lookup and its global fallbacks.
#[derive(Clone, Copy)]
pub(crate) struct HashedName {
    pub name: EscapedName,
    hash: u64,
}

impl HashedName {
    pub fn new(name: EscapedName) -> Self {
        Self {
            name,
            hash: hash_name(name),
        }
    }
}

/// A negative answer proves absence from every table added to the filter.
#[derive(Clone, Copy, Default)]
pub(crate) struct NameFilter([u64; 4]);

impl NameFilter {
    pub fn insert(&mut self, name: HashedName) {
        for bit in [name.hash as u8, (name.hash >> 32) as u8] {
            self.0[usize::from(bit / 64)] |= 1 << (bit % 64);
        }
    }

    pub fn may_contain(&self, name: HashedName) -> bool {
        [name.hash as u8, (name.hash >> 32) as u8]
            .into_iter()
            .all(|bit| self.0[usize::from(bit / 64)] & (1 << (bit % 64)) != 0)
    }
}

fn hash_name(name: EscapedName) -> u64 {
    let mut hasher = FxHasher::default();
    name.index().hash(&mut hasher);
    hasher.finish()
}

fn name_of(binder: &ProgramBinder<'_>, symbol: SymbolId) -> EscapedName {
    binder.symbol(symbol).escaped_name
}

impl MemberTable {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            symbols: Vec::with_capacity(capacity),
            positions: if capacity > LINEAR_MEMBERS {
                HashTable::with_capacity(capacity)
            } else {
                HashTable::new()
            },
        }
    }

    /// Whether the members are searched in order (see the `positions` field).
    fn linear(&self) -> bool {
        self.positions.capacity() == 0
    }

    /// Index the members by name once there are more than a few.
    fn index(binder: &ProgramBinder<'_>, symbols: &[SymbolId], capacity: usize) -> HashTable<u32> {
        let mut positions = HashTable::with_capacity(capacity);
        for (position, &symbol) in symbols.iter().enumerate() {
            let name = name_of(binder, symbol);
            positions.insert_unique(hash_name(name), position as u32, |&position| {
                hash_name(name_of(binder, symbols[position as usize]))
            });
        }
        positions
    }

    /// The members of `table`, whose keys are the members' names.
    pub fn from_symbol_table(binder: &ProgramBinder<'_>, table: &SymbolTable) -> Self {
        let mut members = Self::with_capacity(table.len());
        for (name, &symbol) in table.iter() {
            debug_assert_eq!(
                *name,
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
        if !self.linear() {
            self.positions.reserve(additional, |&position| {
                hash_name(name_of(binder, symbols[position as usize]))
            });
        } else if symbols.len() + additional > LINEAR_MEMBERS {
            self.positions = Self::index(binder, symbols, symbols.len() + additional);
        }
    }

    pub fn get(&self, binder: &ProgramBinder<'_>, name: impl NameKey) -> Option<SymbolId> {
        let name = name.name();
        if self.linear() {
            return self
                .symbols
                .iter()
                .copied()
                .find(|&symbol| name_of(binder, symbol) == name);
        }
        self.positions
            .find(hash_name(name), |&position| {
                name_of(binder, self.symbols[position as usize]) == name
            })
            .map(|&position| self.symbols[position as usize])
    }

    pub(crate) fn get_hashed(
        &self,
        binder: &ProgramBinder<'_>,
        key: HashedName,
    ) -> Option<SymbolId> {
        let name = key.name;
        if self.linear() {
            return self
                .symbols
                .iter()
                .copied()
                .find(|&symbol| name_of(binder, symbol) == name);
        }
        self.positions
            .find(key.hash, |&position| {
                name_of(binder, self.symbols[position as usize]) == name
            })
            .map(|&position| self.symbols[position as usize])
    }

    /// Add `symbol` under its own name; a member of that name already present
    /// is replaced in its position, as in a JavaScript `Map`, and returned.
    pub fn insert(&mut self, binder: &ProgramBinder<'_>, symbol: SymbolId) -> Option<SymbolId> {
        let name = name_of(binder, symbol);
        if self.linear() {
            if let Some(position) = self
                .symbols
                .iter()
                .position(|&present| name_of(binder, present) == name)
            {
                return Some(std::mem::replace(&mut self.symbols[position], symbol));
            }
            if self.symbols.len() < LINEAR_MEMBERS {
                self.symbols.push(symbol);
                return None;
            }
            self.positions = Self::index(binder, &self.symbols, self.symbols.len() + 1);
        }
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
