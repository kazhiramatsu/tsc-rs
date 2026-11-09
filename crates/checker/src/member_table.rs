//! The members table of a resolved type.

use std::hash::{Hash, Hasher};

use hashbrown::hash_table::{Entry, HashTable};
use rustc_hash::FxHasher;
use tsc_binder::{NameKey, SymbolId, SymbolTable};
use tsc_types::EscapedName;

use crate::program::ProgramBinder;

/// A resolved type's members, in insertion order, indexed by their own names.
/// Construction uses the default Vec storage. Publication moves the index and
/// freezes that Vec once; properties can share the resulting immutable list.
/// The storage parameter is resolved at compile time, never tagged at lookup.
#[derive(Clone, Debug, Default)]
pub struct MemberTable<S = Vec<SymbolId>> {
    symbols: S,
    positions: Option<Box<HashTable<u32>>>,
}

/// Published storage; cloning shares the IDs until a mutable builder is needed.
pub type FrozenMembers = MemberTable<crate::state::SymbolIds>;

/// A finished symbol table whose final member count is already known. This is
/// only a construction input: publication writes directly into the final Arc,
/// and never retains a binder-table handle or changes the lookup representation.
pub(crate) struct MemberTableSource<'a> {
    table: &'a SymbolTable,
}

impl<'a> MemberTableSource<'a> {
    #[inline]
    pub(crate) fn new(binder: &ProgramBinder<'_>, table: &'a SymbolTable) -> Self {
        debug_assert!(table
            .iter()
            .all(|(&name, &symbol)| name == name_of(binder, symbol)));
        Self { table }
    }

    #[inline]
    fn freeze(self, mut visit: impl FnMut(usize, SymbolId)) -> FrozenMembers {
        if self.table.is_empty() {
            return FrozenMembers::default();
        }
        // SymbolTable keys are unique and are the symbols' own names. Its
        // exact-length insertion-order iterator fills the Arc and index once;
        // no mutable ID Vec, duplicate-name probe or freeze copy is needed.
        let (symbols, positions): (std::sync::Arc<[SymbolId]>, _) =
            if self.table.len() > LINEAR_MEMBERS {
                let mut positions = Box::new(HashTable::with_capacity(self.table.len()));
                let symbols = self
                    .table
                    .iter()
                    .enumerate()
                    .map(|(position, (&name, &symbol))| {
                        positions.insert_unique(hash_name(name), position as u32, |&position| {
                            hash_name(*self.table.get_index(position as usize).unwrap().0)
                        });
                        visit(position, symbol);
                        symbol
                    })
                    .collect();
                (symbols, Some(positions))
            } else {
                let symbols = self
                    .table
                    .values()
                    .enumerate()
                    .map(|(position, &symbol)| {
                        visit(position, symbol);
                        symbol
                    })
                    .collect();
                (symbols, None)
            };
        FrozenMembers {
            symbols: crate::state::SymbolIds::from_nonempty_arc(symbols),
            positions,
        }
    }
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
            positions: (capacity > LINEAR_MEMBERS)
                .then(|| Box::new(HashTable::with_capacity(capacity))),
        }
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

    /// Room for `additional` more members without regrowing.
    pub fn reserve(&mut self, binder: &ProgramBinder<'_>, additional: usize) {
        self.symbols.reserve(additional);
        let symbols = &self.symbols;
        if let Some(positions) = &mut self.positions {
            positions.reserve(additional, |&position| {
                hash_name(name_of(binder, symbols[position as usize]))
            });
        } else if symbols.len() + additional > LINEAR_MEMBERS {
            self.positions = Some(Box::new(Self::index(
                binder,
                symbols,
                symbols.len() + additional,
            )));
        }
    }

    /// Add `symbol` under its own name; a member of that name already present
    /// is replaced in its position, as in a JavaScript `Map`, and returned.
    pub fn insert(&mut self, binder: &ProgramBinder<'_>, symbol: SymbolId) -> Option<SymbolId> {
        let name = name_of(binder, symbol);
        if self.positions.is_none() {
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
            self.positions = Some(Box::new(Self::index(
                binder,
                &self.symbols,
                self.symbols.len() + 1,
            )));
        }
        let symbols = &self.symbols;
        let entry = self.positions.as_mut().unwrap().entry(
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
        self.symbols.capacity() * std::mem::size_of::<SymbolId>() + self.index_heap_bytes()
    }
}

impl<S: std::ops::Deref<Target = [SymbolId]>> MemberTable<S> {
    #[inline]
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    /// The members in insertion order.
    #[inline]
    pub fn symbols(&self) -> &[SymbolId] {
        &self.symbols
    }

    pub fn get(&self, binder: &ProgramBinder<'_>, name: impl NameKey) -> Option<SymbolId> {
        let name = name.name();
        let symbols = self.symbols();
        let Some(positions) = self.positions.as_deref() else {
            return symbols
                .iter()
                .copied()
                .find(|&symbol| name_of(binder, symbol) == name);
        };
        positions
            .find(hash_name(name), |&position| {
                name_of(binder, symbols[position as usize]) == name
            })
            .map(|&position| symbols[position as usize])
    }

    pub(crate) fn get_hashed(
        &self,
        binder: &ProgramBinder<'_>,
        key: HashedName,
    ) -> Option<SymbolId> {
        let name = key.name;
        let symbols = self.symbols();
        let Some(positions) = self.positions.as_deref() else {
            return symbols
                .iter()
                .copied()
                .find(|&symbol| name_of(binder, symbol) == name);
        };
        positions
            .find(key.hash, |&position| {
                name_of(binder, symbols[position as usize]) == name
            })
            .map(|&position| symbols[position as usize])
    }

    fn index_heap_bytes(&self) -> usize {
        self.positions.as_ref().map_or(0, |positions| {
            std::mem::size_of::<HashTable<u32>>() + positions.capacity() * 5
        })
    }
}

/// Publication supports both a fresh builder and an already frozen snapshot.
/// The implementations are selected at compile time.
pub trait FreezeMembers {
    fn freeze_with_properties(
        self,
        properties: Vec<SymbolId>,
    ) -> (FrozenMembers, crate::state::SymbolIds);

    /// Select named properties while writing the final ID allocation. When
    /// every ID is selected, no temporary property vector or equality pass is
    /// needed. A frozen input keeps its existing allocation.
    fn freeze_with_filter(
        self,
        keep: impl FnMut(SymbolId) -> bool,
    ) -> (FrozenMembers, crate::state::SymbolIds);
}

impl FreezeMembers for MemberTableSource<'_> {
    #[inline]
    fn freeze_with_properties(
        self,
        properties: Vec<SymbolId>,
    ) -> (FrozenMembers, crate::state::SymbolIds) {
        let same_len = self.table.len() == properties.len();
        let mut equal = same_len;
        let members = if same_len {
            self.freeze(|position, symbol| equal &= symbol == properties[position])
        } else {
            self.freeze(|_, _| {})
        };
        let properties = if equal {
            members.symbol_list()
        } else {
            properties.into()
        };
        (members, properties)
    }

    #[inline]
    fn freeze_with_filter(
        self,
        mut keep: impl FnMut(SymbolId) -> bool,
    ) -> (FrozenMembers, crate::state::SymbolIds) {
        let table = self.table;
        let mut properties: Option<Vec<SymbolId>> = None;
        let members = self.freeze(|position, symbol| {
            if keep(symbol) {
                if let Some(properties) = &mut properties {
                    properties.push(symbol);
                }
            } else if properties.is_none() {
                let mut selected = Vec::with_capacity(table.len() - 1);
                selected.extend(table.values().take(position).copied());
                properties = Some(selected);
            }
        });
        let properties = properties.map_or_else(|| members.symbol_list(), Into::into);
        (members, properties)
    }
}

impl FreezeMembers for MemberTable {
    #[inline]
    fn freeze_with_properties(
        self,
        properties: Vec<SymbolId>,
    ) -> (FrozenMembers, crate::state::SymbolIds) {
        if self.symbols.len() != properties.len() || self.symbols.is_empty() {
            return (self.into(), properties.into());
        }
        // Zip/Map of the Vec and slice iterators retains TrustedLen. Arc's
        // collector writes directly into its one allocation. Compare while
        // copying each ID into the frozen list, without a second equality pass.
        let symbols_len = self.symbols.len();
        let mut equal = true;
        let symbols: std::sync::Arc<[SymbolId]> = self
            .symbols
            .into_iter()
            .zip(&properties)
            .map(|(symbol, &property)| {
                equal &= symbol == property;
                symbol
            })
            .collect();
        let symbols = crate::state::SymbolIds::from_nonempty_arc(symbols);
        let properties = if equal {
            symbols.clone()
        } else {
            properties.into()
        };
        (
            FrozenMembers {
                symbols,
                positions: (symbols_len > LINEAR_MEMBERS)
                    .then_some(self.positions)
                    .flatten(),
            },
            properties,
        )
    }

    #[inline]
    fn freeze_with_filter(
        self,
        mut keep: impl FnMut(SymbolId) -> bool,
    ) -> (FrozenMembers, crate::state::SymbolIds) {
        if self.symbols.is_empty() {
            return (FrozenMembers::default(), crate::state::SymbolIds::new());
        }
        let mut properties = None;
        // The exact-length map writes directly into Arc's allocation. Keep the
        // source slice for the prefix only if the first excluded ID is found.
        let symbols: std::sync::Arc<[SymbolId]> = self
            .symbols
            .iter()
            .enumerate()
            .map(|(position, &symbol)| {
                select_property(
                    &mut properties,
                    &self.symbols,
                    position,
                    symbol,
                    keep(symbol),
                );
                symbol
            })
            .collect();
        let members = FrozenMembers {
            symbols: crate::state::SymbolIds::from_nonempty_arc(symbols),
            positions: if self.symbols.len() > LINEAR_MEMBERS {
                self.positions
            } else {
                None
            },
        };
        let properties = properties.map_or_else(|| members.symbol_list(), Into::into);
        (members, properties)
    }
}

impl FreezeMembers for FrozenMembers {
    #[inline]
    fn freeze_with_properties(
        self,
        properties: Vec<SymbolId>,
    ) -> (FrozenMembers, crate::state::SymbolIds) {
        let properties = if properties == self.symbols() {
            self.symbol_list()
        } else {
            properties.into()
        };
        (self, properties)
    }

    #[inline]
    fn freeze_with_filter(
        self,
        keep: impl FnMut(SymbolId) -> bool,
    ) -> (FrozenMembers, crate::state::SymbolIds) {
        let properties = self.filter_properties(keep);
        (self, properties)
    }
}

impl From<MemberTable> for FrozenMembers {
    #[inline]
    fn from(table: MemberTable) -> Self {
        let positions = if table.symbols.len() > LINEAR_MEMBERS {
            table.positions
        } else {
            None
        };
        Self {
            // Collect an exact-length iterator directly into the Arc rather
            // than converting a Vec with Arc::from (which copies its buffer).
            symbols: if table.symbols.is_empty() {
                crate::state::SymbolIds::new()
            } else {
                crate::state::SymbolIds::from_nonempty_arc(table.symbols.into_iter().collect())
            },
            positions,
        }
    }
}

impl FrozenMembers {
    #[inline]
    pub(crate) fn filter_properties(
        &self,
        mut keep: impl FnMut(SymbolId) -> bool,
    ) -> crate::state::SymbolIds {
        let symbols = self.symbols();
        let mut properties = None;
        for (position, &symbol) in symbols.iter().enumerate() {
            select_property(&mut properties, symbols, position, symbol, keep(symbol));
        }
        properties.map_or_else(|| self.symbol_list(), Into::into)
    }

    /// Start a private mutable builder, leaving any shared list snapshots intact.
    /// The existing index moves with the builder; no names are rehashed.
    pub fn into_builder(self) -> MemberTable {
        MemberTable {
            symbols: self.symbols.to_vec(),
            positions: self.positions,
        }
    }

    /// Copy on write for the rare direct mutation of a published record.
    pub fn insert(&mut self, binder: &ProgramBinder<'_>, symbol: SymbolId) -> Option<SymbolId> {
        let mut builder = std::mem::take(self).into_builder();
        let previous = builder.insert(binder, symbol);
        *self = builder.into();
        previous
    }

    #[inline]
    pub(crate) fn symbol_list(&self) -> crate::state::SymbolIds {
        self.symbols.clone()
    }

    pub fn heap_bytes(&self) -> usize {
        self.symbols.heap_bytes() + self.index_heap_bytes()
    }

    pub(crate) fn counted_heap_bytes(
        &self,
        symbols: &mut rustc_hash::FxHashSet<*const SymbolId>,
    ) -> usize {
        self.symbols.counted_heap_bytes(symbols) + self.index_heap_bytes()
    }
}

/// Delay allocating a filtered list until an ID is actually excluded. Before
/// that point the selected properties are exactly the source prefix.
#[inline]
fn select_property(
    properties: &mut Option<Vec<SymbolId>>,
    symbols: &[SymbolId],
    position: usize,
    symbol: SymbolId,
    keep: bool,
) {
    if keep {
        if let Some(properties) = properties {
            properties.push(symbol);
        }
    } else if properties.is_none() {
        let mut selected = Vec::with_capacity(symbols.len() - 1);
        selected.extend_from_slice(&symbols[..position]);
        *properties = Some(selected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_support::with_program_state;
    use crate::state::{IndexInfo, MembersExtras, ResolvedMembers, SignatureId};
    use tsc_types::{CompilerOptions, SymbolFlags, TypeId};

    fn property(binder: &mut ProgramBinder<'_>, name: &str) -> SymbolId {
        binder.create_symbol(
            SymbolFlags::PROPERTY,
            EscapedName::from_identifier_escaped_text(name),
        )
    }

    #[test]
    fn declared_tables_use_own_index_and_keep_binder_insertion_order() {
        let source = "interface Small { z: number; a: number }\n\
            interface Large { z: number; a: number; y: number; b: number; x: number;\
            c: number; w: number; d: number; v: number }\n\
            class Statics { static z: number; static a: number; static y: number;\
            static b: number; static x: number; static c: number; static w: number;\
            static d: number; static v: number; }";
        with_program_state(&[("a.ts", source)], &CompilerOptions::default(), |state| {
            for (name, exports) in [("Small", false), ("Large", false), ("Statics", true)] {
                let owner = *state.globals.get(name).unwrap();
                let symbol = state.binder.symbol(owner);
                let table = if exports {
                    symbol.exports()
                } else {
                    symbol.members()
                };
                let expected = table.values().copied().collect::<Vec<_>>();
                let members = state.member_table_source(table);
                let previous = table.get("z").copied();
                let record =
                    ResolvedMembers::new(members, expected.clone(), vec![], vec![], vec![]);
                assert_eq!(record.members.symbols(), expected);
                assert_eq!(record.members.len(), expected.len());
                assert!(!record.members.is_empty());
                assert_eq!(
                    record.members.positions.is_some(),
                    expected.len() > LINEAR_MEMBERS
                );
                assert_eq!(record.members.get(&state.binder, "z"), previous);
                assert_eq!(
                    record.members.get_hashed(
                        &state.binder,
                        HashedName::new(EscapedName::from_identifier_escaped_text("a"))
                    ),
                    table.get("a").copied()
                );
                assert_eq!(record.members.get(&state.binder, "absent"), None);
                assert_eq!(
                    record.properties.as_ptr(),
                    record.members.symbols().as_ptr()
                );
                let mut cloned = record.clone();
                let replacement = property(&mut state.binder, "z");
                assert_eq!(cloned.members.insert(&state.binder, replacement), previous);
                let position = expected
                    .iter()
                    .position(|&id| Some(id) == previous)
                    .unwrap();
                assert_eq!(cloned.members.symbols()[position], replacement);
                assert_eq!(record.members.symbols(), expected);
                assert_eq!(&*cloned.properties, expected);
                let symbol = state.binder.symbol(owner);
                let table = if exports {
                    symbol.exports()
                } else {
                    symbol.members()
                };
                assert_eq!(table.values().copied().collect::<Vec<_>>(), expected);
            }
        });
    }

    #[test]
    fn builder_crosses_index_boundary_and_freezes_without_rebuilding_index() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let ids = (0..10)
                .map(|i| property(&mut state.binder, &format!("p{i}")))
                .collect::<Vec<_>>();
            let mut table = MemberTable::with_capacity(1);
            for &id in &ids[..8] {
                assert_eq!(table.insert(&state.binder, id), None);
            }
            assert!(table.positions.is_none());
            assert_eq!(table.get(&state.binder, "p7"), Some(ids[7]));
            let small = table.clone();
            table.reserve(&state.binder, 2);
            table.insert(&state.binder, ids[8]);
            let index = &**table.positions.as_ref().unwrap() as *const HashTable<u32>;
            let record = ResolvedMembers::new(table, ids[..9].to_vec(), vec![], vec![], vec![]);
            assert_eq!(
                &**record.members.positions.as_ref().unwrap() as *const HashTable<u32>,
                index
            );
            assert_eq!(record.members.get(&state.binder, "p8"), Some(ids[8]));
            assert_eq!(
                record.members.get_hashed(
                    &state.binder,
                    HashedName::new(EscapedName::from_identifier_escaped_text("missing"))
                ),
                None
            );
            assert_eq!(small.symbols(), &ids[..8]);
            let mut cloned = record.clone();
            assert_eq!(
                record.members.symbols().as_ptr(),
                cloned.members.symbols().as_ptr()
            );
            let replacement = property(&mut state.binder, "p3");
            assert_eq!(
                cloned.members.insert(&state.binder, replacement),
                Some(ids[3])
            );
            cloned.members.insert(&state.binder, ids[9]);
            assert_eq!(cloned.members.symbols()[3], replacement);
            assert_eq!(record.members.symbols(), &ids[..9]);
            assert_eq!(&*cloned.properties, &ids[..9]);
            let mut symbols = Default::default();
            assert_eq!(
                record.members.counted_heap_bytes(&mut symbols),
                record.members.heap_bytes()
            );
            assert_eq!(record.properties.counted_heap_bytes(&mut symbols), 0);
            // Clones share the ID allocation and own a private boxed index.
            assert_eq!(
                record.clone().members.counted_heap_bytes(&mut symbols),
                record.members.index_heap_bytes()
            );
        });
    }

    #[test]
    fn named_selection_visits_once_and_preserves_filtered_and_shared_snapshots() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let ids = (0..10)
                .map(|i| property(&mut state.binder, &format!("p{i}")))
                .collect::<Vec<_>>();
            let table: SymbolTable = ids
                .iter()
                .map(|&id| (state.binder.symbol(id).escaped_name, id))
                .collect();
            for excluded in [None, Some(0), Some(4), Some(9)] {
                let mut visited = Vec::new();
                let (direct, properties) =
                    state.member_table_source(&table).freeze_with_filter(|id| {
                        visited.push(id);
                        excluded.is_none_or(|position| id != ids[position])
                    });
                assert_eq!(visited, ids);
                assert_eq!(direct.symbols(), ids);
                let expected = ids
                    .iter()
                    .copied()
                    .filter(|id| excluded.is_none_or(|position| *id != ids[position]))
                    .collect::<Vec<_>>();
                assert_eq!(&*properties, expected);
                assert_eq!(
                    properties.as_ptr() == direct.symbols().as_ptr(),
                    excluded.is_none()
                );
                let builder = MemberTable::from_symbol_table(&state.binder, &table);
                let index = &**builder.positions.as_ref().unwrap() as *const HashTable<u32>;
                let (frozen, from_builder) = builder
                    .clone()
                    .freeze_with_filter(|id| expected.contains(&id));
                assert_eq!(frozen.symbols(), direct.symbols());
                assert_eq!(&*from_builder, expected);
                let (moved, _) = builder.freeze_with_filter(|_| true);
                assert_eq!(
                    &**moved.positions.as_ref().unwrap() as *const HashTable<u32>,
                    index
                );
                let index = &**direct.positions.as_ref().unwrap() as *const HashTable<u32>;
                let frozen_ids = direct.symbols().as_ptr();
                let (refrozen, selected_again) =
                    direct.freeze_with_filter(|id| expected.contains(&id));
                assert_eq!(refrozen.symbols().as_ptr(), frozen_ids);
                assert_eq!(
                    &**refrozen.positions.as_ref().unwrap() as *const HashTable<u32>,
                    index
                );
                assert_eq!(&*selected_again, expected);
                for (position, &id) in ids.iter().enumerate() {
                    assert_eq!(
                        refrozen.get(&state.binder, format!("p{position}").as_str()),
                        Some(id)
                    );
                }
            }
        });
    }

    #[test]
    fn properties_share_only_on_ordered_equality_and_extras_keep_snapshots() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let ids = [
                property(&mut state.binder, "z"),
                property(&mut state.binder, "a"),
            ];
            let table = MemberTable::from_symbols(&state.binder, &ids);
            let empty = ResolvedMembers::new(table.clone(), ids.to_vec(), vec![], vec![], vec![]);
            assert_eq!(empty.properties.as_ptr(), empty.members.symbols().as_ptr());
            let mut published = empty.clone();
            published.set_properties(&ids);
            assert_eq!(
                published.properties.as_ptr(),
                published.members.symbols().as_ptr()
            );
            published.set_properties(&[ids[1], ids[0]]);
            assert_eq!(&*published.properties, &[ids[1], ids[0]]);
            assert_ne!(
                published.properties.as_ptr(),
                published.members.symbols().as_ptr()
            );
            let refrozen =
                ResolvedMembers::new(empty.members.clone(), ids.to_vec(), vec![], vec![], vec![]);
            assert_eq!(
                refrozen.members.symbols().as_ptr(),
                empty.members.symbols().as_ptr()
            );
            assert_eq!(refrozen.properties.as_ptr(), empty.properties.as_ptr());
            assert_eq!(empty.extras_heap_bytes(), 0);
            assert!(empty.call_signatures().is_empty());
            assert!(empty.construct_signatures().is_empty());
            assert!(empty.index_infos().is_empty());
            for properties in [vec![ids[1]], vec![ids[1], ids[0]], vec![ids[0], ids[0]]] {
                let record =
                    ResolvedMembers::new(table.clone(), properties.clone(), vec![], vec![], vec![]);
                assert_eq!(&*record.properties, properties);
                assert_ne!(
                    record.properties.as_ptr(),
                    record.members.symbols().as_ptr()
                );
            }
            let info = IndexInfo {
                key_type: TypeId::new(1),
                value_type: TypeId::new(2),
                is_readonly: false,
                declaration: None,
                components: None,
                is_enum_number_index_info: false,
                is_any_base_type_index_info: false,
            };
            let record = ResolvedMembers::new(
                table,
                ids.to_vec(),
                vec![SignatureId::new(1)],
                vec![SignatureId::new(2)],
                vec![info],
            );
            let mut cloned = record.clone();
            assert_eq!(cloned.call_signatures(), &[SignatureId::new(1)]);
            assert_eq!(cloned.construct_signatures(), &[SignatureId::new(2)]);
            assert_eq!(cloned.index_infos().len(), 1);
            assert!(cloned.extras_heap_bytes() > std::mem::size_of::<MembersExtras>());
            cloned.set_lists(vec![], vec![], vec![]);
            assert_eq!(record.call_signatures(), &[SignatureId::new(1)]);
            assert_eq!(&*record.construct_signature_list(), &[SignatureId::new(2)]);
            assert_eq!(record.index_info_list().len(), 1);
            assert_eq!(cloned.extras_heap_bytes(), 0);
            cloned.set_properties(&[ids[1]]);
            assert_eq!(&*record.properties, ids);
            assert_eq!(&*cloned.properties, &ids[1..]);
            cloned.set_properties(&ids);
            assert_eq!(
                cloned.properties.as_ptr(),
                cloned.members.symbols().as_ptr()
            );
        });
    }

    #[test]
    fn builder_clones_and_published_copy_on_write_keep_snapshots() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let ids = (0..9)
                .map(|i| property(&mut state.binder, &format!("p{i}")))
                .collect::<Vec<_>>();
            let mut builder = MemberTable::with_capacity(16);
            builder.insert(&state.binder, ids[0]);
            let mut cloned = builder.clone();
            cloned.reserve(&state.binder, 16);
            cloned.insert(&state.binder, ids[1]);
            let record = ResolvedMembers::new(builder, ids[..1].to_vec(), vec![], vec![], vec![]);
            assert_eq!(record.members.symbols(), &ids[..1]);
            assert!(record.members.positions.is_none());
            assert_eq!(cloned.symbols(), &ids[..2]);
            let snapshot = record.members.clone();
            let mut builder = record.members.into_builder();
            builder.insert(&state.binder, ids[1]);
            let copied = ResolvedMembers::new(builder, ids[..2].to_vec(), vec![], vec![], vec![]);
            assert_eq!(snapshot.symbols(), &ids[..1]);
            assert_eq!(copied.members.symbols(), &ids[..2]);
            assert_eq!(&*record.properties, &ids[..1]);
            let empty = ResolvedMembers::new(
                MemberTable::with_capacity(0),
                vec![],
                vec![],
                vec![],
                vec![],
            );
            assert!(empty.members.is_empty());
            assert_eq!(empty.members.heap_bytes(), 0);
            assert_eq!(empty.properties.heap_bytes(), 0);
        });
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn published_layout_is_48_bytes_with_optional_boxed_index() {
        assert_eq!(std::mem::size_of::<MemberTable>(), 32);
        assert_eq!(std::mem::size_of::<FrozenMembers>(), 24);
        assert_eq!(std::mem::size_of::<ResolvedMembers>(), 48);
        assert_eq!(std::mem::size_of::<MembersExtras>(), 48);
    }
}
