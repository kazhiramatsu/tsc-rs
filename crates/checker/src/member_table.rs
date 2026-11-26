//! The members table of a resolved type.

use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};

use hashbrown::hash_table::{Entry, HashTable};
use rustc_hash::FxHasher;
use tsc_binder::{NameKey, SymbolId, SymbolTable};
use tsc_types::{EscapedName, InlineOrShared, InlineOrSharedView};

use crate::program::ProgramBinder;

/// A resolved type's members, in insertion order, found by their own names.
/// The one exception is a table made from a symbol table that holds a member
/// under another key than its name (the clone of an alias's target in the
/// alias's place, see `ProgramBinder::note_member_keyed_apart`): it is found
/// by that table's keys, as the reference's map is, and always has an index.
///
/// A lookup reads as few separate regions as it can. A table of at most
/// [`LINEAR_MEMBERS`] members keeps one seven-bit tag of each member's name in
/// its own index word: a name that is absent is answered from the record
/// alone, and a present one reads the ID list and one symbol record. A larger
/// table has a [`NameIndex`], which holds the names themselves, so its lookups
/// read no symbol record; tables that repeat a name sequence (the
/// instantiations of one generic type) share one index: a builder follows
/// the index of the last table published for the same source
/// ([`MemberShape`]), and a published table takes the index of its name
/// sequence whichever path built it ([`NameIndexes`]).
///
/// Construction uses the default Vec storage. Publication moves the index word
/// and freezes that Vec once; properties can share the resulting immutable
/// list. The storage parameter is resolved at compile time, never tagged at
/// lookup.
#[derive(Clone, Debug, Default)]
pub struct MemberTable<S = Vec<SymbolId>> {
    symbols: S,
    /// Inline: the name tags of at most [`LINEAR_MEMBERS`] members, member `i`
    /// in byte `i`, with zero for an unused byte. Shared: an index whose first
    /// `symbols.len()` names are the members' names in order; it may continue
    /// with names this table does not have.
    index: InlineOrShared<NameIndex>,
}

/// Published storage; cloning shares the IDs and the index.
pub type FrozenMembers = MemberTable<crate::state::SymbolIds>;

/// The names of a table in insertion order, and their positions by name.
#[derive(Debug, Default)]
pub(crate) struct NameIndex {
    names: Vec<EscapedName>,
    positions: HashTable<(EscapedName, u32)>,
    /// Set once a [`NameIndexes`] holds this index as the one index of its
    /// name sequence; such an index always has another handle, so it is
    /// never written again.
    of_sequence: AtomicBool,
    /// The names are a symbol table's keys and one of them is not its
    /// member's own name. Tags are confirmed against the members' own names,
    /// so a table with such an index is read through it however small it is.
    keyed_apart: bool,
}

impl Clone for NameIndex {
    /// A copy is a builder's own index, not the index of its sequence.
    fn clone(&self) -> Self {
        Self {
            names: self.names.clone(),
            positions: self.positions.clone(),
            of_sequence: AtomicBool::new(false),
            keyed_apart: self.keyed_apart,
        }
    }
}

impl NameIndex {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            names: Vec::with_capacity(capacity),
            positions: HashTable::with_capacity(capacity),
            of_sequence: AtomicBool::new(false),
            keyed_apart: false,
        }
    }

    /// The index of `names`, which are distinct, with room for `capacity`.
    fn from_names(names: impl IntoIterator<Item = EscapedName>, capacity: usize) -> Self {
        let mut index = Self::with_capacity(capacity);
        for name in names {
            index.push(name, hash_name(name));
        }
        index
    }

    #[inline]
    fn find(&self, name: EscapedName, hash: u64) -> Option<u32> {
        self.positions
            .find(hash, |&(present, _)| present == name)
            .map(|&(_, position)| position)
    }

    /// Add `name`, which is absent, at the next position.
    fn push(&mut self, name: EscapedName, hash: u64) {
        let position = u32::try_from(self.names.len()).expect("member table size");
        self.positions
            .insert_unique(hash, (name, position), |&(present, _)| hash_name(present));
        self.names.push(name);
    }

    fn reserve(&mut self, additional: usize) {
        self.names.reserve(additional);
        self.positions
            .reserve(additional, |&(present, _)| hash_name(present));
    }

    fn heap_bytes(&self) -> usize {
        // The shared allocation's two counts, the index record, the names and
        // the positions' slots with one control byte each.
        2 * std::mem::size_of::<usize>()
            + std::mem::size_of::<Self>()
            + self.names.capacity() * std::mem::size_of::<EscapedName>()
            + self.positions.capacity() * (std::mem::size_of::<(EscapedName, u32)>() + 1)
    }
}

/// The name index of a published table, kept for a builder that will
/// probably repeat its name sequence (see [`MemberTable::following`]).
#[derive(Clone, Debug)]
pub(crate) struct MemberShape(InlineOrShared<NameIndex>);

impl MemberShape {
    /// How many members the table it was taken from had at most.
    pub(crate) fn len(&self) -> usize {
        self.0.as_shared().map_or(0, |index| index.names.len())
    }
}

/// One index per name sequence. A published table whose names repeat the
/// sequence of an earlier table gives up its own index for that table's:
/// builders that had no [`MemberShape`] to follow (spreads, mapped types,
/// object literals) and builders whose shape came from elsewhere end on the
/// same index as the instantiations that followed one. Sharing never changes
/// an answer: equal sequences have equal positions.
#[derive(Debug, Default)]
pub(crate) struct NameIndexes {
    /// The sequence's hash with its index.
    by_sequence: HashTable<(u64, InlineOrShared<NameIndex>)>,
}

impl NameIndexes {
    /// Give `members` the index of its name sequence: the index of an equal
    /// sequence published before replaces the table's own, otherwise the
    /// table's own becomes the sequence's. A table that keeps tags, one that
    /// already has its sequence's index and one that shares a longer
    /// sequence's index are left alone.
    pub(crate) fn share(&mut self, members: &mut FrozenMembers) {
        let Some(index) = members.index.as_shared() else {
            return;
        };
        if index.of_sequence.load(Ordering::Relaxed) || index.names.len() != members.len() {
            return;
        }
        let hash = sequence_hash(&index.names);
        let earlier = match self.by_sequence.entry(
            hash,
            |(_, present)| {
                present
                    .as_shared()
                    .is_some_and(|present| present.names == index.names)
            },
            |&(hash, _)| hash,
        ) {
            Entry::Occupied(slot) => Some(slot.get().1.clone()),
            Entry::Vacant(slot) => {
                index.of_sequence.store(true, Ordering::Relaxed);
                slot.insert((hash, members.index.clone()));
                None
            }
        };
        if let Some(earlier) = earlier {
            members.index = earlier;
        }
    }

    /// How many name sequences have an index.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.by_sequence.len()
    }
}

fn sequence_hash(names: &[EscapedName]) -> u64 {
    let mut hasher = FxHasher::default();
    for name in names {
        name.index().hash(&mut hasher);
    }
    hasher.finish()
}

/// A finished symbol table whose final member count is already known. This is
/// only a construction input: publication writes directly into the final Arc,
/// and never retains a binder-table handle or changes the lookup representation.
pub(crate) struct MemberTableSource<'a> {
    table: &'a SymbolTable,
    keyed_apart: bool,
}

impl<'a> MemberTableSource<'a> {
    #[inline]
    pub(crate) fn new(binder: &ProgramBinder<'_>, table: &'a SymbolTable) -> Self {
        Self {
            table,
            keyed_apart: keyed_apart(binder, table),
        }
    }

    #[inline]
    fn freeze(self, mut visit: impl FnMut(usize, SymbolId)) -> FrozenMembers {
        if self.table.is_empty() {
            return FrozenMembers::default();
        }
        // SymbolTable keys are unique and, unless the table is keyed apart,
        // the symbols' own names. Its exact-length insertion-order iterator
        // fills the Arc once; the tags or the index come from the keys,
        // without reading a symbol record.
        let symbols: std::sync::Arc<[SymbolId]> = self
            .table
            .values()
            .enumerate()
            .map(|(position, &symbol)| {
                visit(position, symbol);
                symbol
            })
            .collect();
        FrozenMembers {
            symbols: crate::state::SymbolIds::from_nonempty_arc(symbols),
            index: index_of_keys(self.table, self.keyed_apart),
        }
    }
}

/// The most members a table keeps name tags for instead of an index.
pub(crate) const LINEAR_MEMBERS: usize = 8;

/// A nonzero seven-bit fingerprint leaves each unused byte distinguishable
/// from every search tag. The eighth tag fits below bit 63 of the word.
#[inline]
fn name_tag(hash: u64) -> u64 {
    (hash >> 57).max(1)
}

/// Set the high bit of every byte with the requested fingerprint.
#[inline]
fn tag_matches(tags: u64, tag: u64) -> u64 {
    let difference = tags ^ (tag * 0x0101_0101_0101_0101);
    // Each difference fits in seven bits, so the addition stays in its byte.
    !(difference + 0x7f7f_7f7f_7f7f_7f7f) & 0x8080_8080_8080_8080
}

/// The tags of at most [`LINEAR_MEMBERS`] names, or their index.
fn index_of_names(
    names: impl IntoIterator<Item = EscapedName>,
    len: usize,
) -> InlineOrShared<NameIndex> {
    if len > LINEAR_MEMBERS {
        return InlineOrShared::shared(NameIndex::from_names(names, len));
    }
    let mut tags = 0;
    for (position, name) in names.into_iter().enumerate() {
        tags |= name_tag(hash_name(name)) << (8 * position);
    }
    InlineOrShared::inline(tags)
}

/// Whether `table` holds a member under a key that is not the member's own
/// name. Only a merge through an alias makes such a table and the binder
/// notes the first one, so every other program's tables are answered without
/// reading a symbol record.
#[inline]
fn keyed_apart(binder: &ProgramBinder<'_>, table: &SymbolTable) -> bool {
    let apart = binder.members_keyed_apart()
        && table
            .iter()
            .any(|(&name, &symbol)| name != name_of(binder, symbol));
    debug_assert!(
        apart
            || table
                .iter()
                .all(|(&name, &symbol)| name == name_of(binder, symbol))
    );
    apart
}

/// The tags or the index of a symbol table's keys. A table keyed apart from
/// its members' names gets an index of its keys whatever its size.
fn index_of_keys(table: &SymbolTable, keyed_apart: bool) -> InlineOrShared<NameIndex> {
    let keys = table.iter().map(|(&name, _)| name);
    if keyed_apart {
        let mut index = NameIndex::from_names(keys, table.len());
        index.keyed_apart = true;
        InlineOrShared::shared(index)
    } else {
        index_of_names(keys, table.len())
    }
}

/// The index word of a published table of `len` members built with `index`:
/// a table of at most [`LINEAR_MEMBERS`] members keeps tags, unless its index
/// holds keys that are not its members' names.
fn published_index(index: InlineOrShared<NameIndex>, len: usize) -> InlineOrShared<NameIndex> {
    let tags = match index.view() {
        InlineOrSharedView::Shared(names) if len <= LINEAR_MEMBERS && !names.keyed_apart => {
            Some(index_of_names(names.names[..len].iter().copied(), len))
        }
        _ => None,
    };
    tags.unwrap_or(index)
}

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
            index: if capacity > LINEAR_MEMBERS {
                InlineOrShared::shared(NameIndex::with_capacity(capacity))
            } else {
                InlineOrShared::default()
            },
        }
    }

    /// A builder that expects the name sequence of `shape`: members inserted
    /// in that sequence are appended without hashing, and the published table
    /// shares the index. The first member that leaves the sequence gives the
    /// builder an index of its own.
    pub(crate) fn following(shape: &MemberShape, capacity: usize) -> Self {
        Self {
            symbols: Vec::with_capacity(capacity.max(shape.len())),
            index: shape.0.clone(),
        }
    }

    /// The members of `table` under its keys, which are the members' names
    /// unless the table is keyed apart from them.
    pub fn from_symbol_table(binder: &ProgramBinder<'_>, table: &SymbolTable) -> Self {
        Self {
            symbols: table.values().copied().collect(),
            index: index_of_keys(table, keyed_apart(binder, table)),
        }
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
        if self.index.inline_bits().is_some() {
            if self.symbols.len() + additional > LINEAR_MEMBERS {
                self.index = InlineOrShared::shared(NameIndex::from_names(
                    self.symbols.iter().map(|&symbol| name_of(binder, symbol)),
                    self.symbols.len() + additional,
                ));
            }
        } else if let Some(own) = self.index.unique_mut() {
            // An index followed from another table is left alone: it has the
            // room when the sequence repeats.
            if own.names.len() == self.symbols.len() {
                own.reserve(additional);
            }
        }
    }

    /// The builder's own index holding exactly its members' names.
    fn own_index(&mut self) -> &mut NameIndex {
        let len = self.symbols.len();
        let exact = self
            .index
            .as_shared()
            .is_some_and(|index| index.names.len() == len);
        if !exact {
            // The prefix of a longer followed sequence: its names come from
            // the index, not from the symbol records.
            let followed = self.index.as_shared().expect("an indexed builder");
            let mut prefix = NameIndex::from_names(
                followed.names[..len].iter().copied(),
                self.symbols.capacity().max(len + 1),
            );
            prefix.keyed_apart = followed.keyed_apart;
            self.index = InlineOrShared::shared(prefix);
        }
        self.index.make_unique().expect("an indexed builder")
    }

    /// Add `symbol` under its own name; a member of that name already present
    /// is replaced in its position, as in a JavaScript `Map`, and returned.
    pub fn insert(&mut self, binder: &ProgramBinder<'_>, symbol: SymbolId) -> Option<SymbolId> {
        let name = name_of(binder, symbol);
        let hash = hash_name(name);
        let len = self.symbols.len();
        if let Some(tags) = self.index.inline_bits() {
            let mut matches = tag_matches(tags, name_tag(hash));
            while matches != 0 {
                let position = (matches.trailing_zeros() >> 3) as usize;
                if name_of(binder, self.symbols[position]) == name {
                    return Some(std::mem::replace(&mut self.symbols[position], symbol));
                }
                matches &= matches - 1;
            }
            if len < LINEAR_MEMBERS {
                self.index = InlineOrShared::inline(tags | (name_tag(hash) << (8 * len)));
            } else {
                // The first member beyond the tags: index the present ones.
                let mut index = NameIndex::from_names(
                    self.symbols.iter().map(|&present| name_of(binder, present)),
                    len + 1,
                );
                index.push(name, hash);
                self.index = InlineOrShared::shared(index);
            }
            self.symbols.push(symbol);
            return None;
        }
        // The next name of the index — the builder's own or a followed one.
        let follows = self
            .index
            .as_shared()
            .is_some_and(|index| index.names.get(len) == Some(&name));
        if follows {
            self.symbols.push(symbol);
            return None;
        }
        // The builder's own index of exactly its members: one probe finds the
        // name or the slot for it.
        if let Some(own) = self.index.unique_mut().filter(|own| own.names.len() == len) {
            return match own.positions.entry(
                hash,
                |&(present, _)| present == name,
                |&(present, _)| hash_name(present),
            ) {
                Entry::Occupied(slot) => {
                    let position = slot.get().1 as usize;
                    Some(std::mem::replace(&mut self.symbols[position], symbol))
                }
                Entry::Vacant(slot) => {
                    slot.insert((name, u32::try_from(len).expect("member table size")));
                    own.names.push(name);
                    self.symbols.push(symbol);
                    None
                }
            };
        }
        // An index shared with other tables, or one that continues beyond
        // this builder's members.
        let present = self
            .index
            .as_shared()
            .and_then(|index| index.find(name, hash))
            .filter(|&position| (position as usize) < len);
        match present {
            Some(position) => Some(std::mem::replace(
                &mut self.symbols[position as usize],
                symbol,
            )),
            None => {
                self.own_index().push(name, hash);
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
        self.get_hashed(binder, HashedName::new(name.name()))
    }

    #[inline]
    pub(crate) fn get_hashed(
        &self,
        binder: &ProgramBinder<'_>,
        key: HashedName,
    ) -> Option<SymbolId> {
        match self.index.view() {
            InlineOrSharedView::Inline(tags) => {
                let mut matches = tag_matches(tags, name_tag(key.hash));
                while matches != 0 {
                    let symbol = self.symbols[(matches.trailing_zeros() >> 3) as usize];
                    if name_of(binder, symbol) == key.name {
                        return Some(symbol);
                    }
                    matches &= matches - 1;
                }
                None
            }
            // A followed index may continue beyond this table's members.
            InlineOrSharedView::Shared(index) => index
                .find(key.name, key.hash)
                .and_then(|position| self.symbols.get(position as usize).copied()),
        }
    }

    fn index_heap_bytes(&self) -> usize {
        self.index.as_shared().map_or(0, NameIndex::heap_bytes)
    }

    /// The identity of the shared index, if the table has one.
    #[cfg(test)]
    fn index_address(&self) -> Option<usize> {
        self.index.shared_address()
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
                index: published_index(self.index, symbols_len),
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
            index: published_index(self.index, self.symbols.len()),
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
        let index = published_index(table.index, table.symbols.len());
        Self {
            // Collect an exact-length iterator directly into the Arc rather
            // than converting a Vec with Arc::from (which copies its buffer).
            symbols: if table.symbols.is_empty() {
                crate::state::SymbolIds::new()
            } else {
                crate::state::SymbolIds::from_nonempty_arc(table.symbols.into_iter().collect())
            },
            index,
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
    /// The index word moves with the builder; no names are rehashed, and a
    /// shared index is copied only if the builder adds a name.
    pub fn into_builder(self) -> MemberTable {
        MemberTable {
            symbols: self.symbols.to_vec(),
            index: self.index,
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

    /// The index of this table for the builders of tables that will
    /// probably repeat its name sequence; none for a table that keeps tags.
    pub(crate) fn shape(&self) -> Option<MemberShape> {
        self.index
            .as_shared()
            .map(|_| MemberShape(self.index.clone()))
    }

    /// Whether this table shares `shape`'s index.
    pub(crate) fn has_shape(&self, shape: &MemberShape) -> bool {
        self.index.same(&shape.0)
    }

    pub fn heap_bytes(&self) -> usize {
        self.symbols.heap_bytes() + self.index_heap_bytes()
    }

    /// The bytes of the ID list and of the index, each counted the first
    /// time its allocation is seen.
    pub(crate) fn counted_heap_bytes(
        &self,
        seen: &mut rustc_hash::FxHashSet<*const SymbolId>,
    ) -> usize {
        let index = match self.index.shared_address() {
            Some(address) if seen.insert(std::ptr::without_provenance(address)) => {
                self.index_heap_bytes()
            }
            _ => 0,
        };
        self.symbols.counted_heap_bytes(seen) + index
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
                    record.members.index_address().is_some(),
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
            assert!(table.index_address().is_none());
            assert_eq!(table.get(&state.binder, "p7"), Some(ids[7]));
            let small = table.clone();
            table.reserve(&state.binder, 2);
            table.insert(&state.binder, ids[8]);
            let index = table.index_address().unwrap();
            let record = ResolvedMembers::new(table, ids[..9].to_vec(), vec![], vec![], vec![]);
            assert_eq!(record.members.index_address(), Some(index));
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
            // Clones share the ID allocation and the index.
            assert_eq!(record.clone().members.counted_heap_bytes(&mut symbols), 0);
            assert!(record.members.index_heap_bytes() > 0);
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
                let index = builder.index_address().unwrap();
                let (frozen, from_builder) = builder
                    .clone()
                    .freeze_with_filter(|id| expected.contains(&id));
                assert_eq!(frozen.symbols(), direct.symbols());
                assert_eq!(&*from_builder, expected);
                let (moved, _) = builder.freeze_with_filter(|_| true);
                assert_eq!(moved.index_address(), Some(index));
                let index = direct.index_address().unwrap();
                let frozen_ids = direct.symbols().as_ptr();
                let (refrozen, selected_again) =
                    direct.freeze_with_filter(|id| expected.contains(&id));
                assert_eq!(refrozen.symbols().as_ptr(), frozen_ids);
                assert_eq!(refrozen.index_address(), Some(index));
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
            assert!(record.members.index_address().is_none());
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
    fn one_call_or_one_construct_signature_lives_in_the_records_word() {
        let record =
            |calls: Vec<SignatureId>, constructs: Vec<SignatureId>, infos: Vec<IndexInfo>| {
                ResolvedMembers::new(
                    MemberTable::with_capacity(0),
                    Vec::new(),
                    calls,
                    constructs,
                    infos,
                )
            };
        let info = IndexInfo {
            key_type: TypeId::new(1),
            value_type: TypeId::new(2),
            is_readonly: false,
            declaration: None,
            components: None,
            is_enum_number_index_info: false,
            is_any_base_type_index_info: false,
        };
        // The largest index an identity can have is still held inline.
        let (a, b) = (SignatureId::new(7), SignatureId::new(u32::MAX - 1));
        for id in [a, b] {
            // One call signature alone, or one construct signature alone:
            // no allocation, and both readings give it back.
            let call = record(vec![id], vec![], vec![]);
            assert_eq!(call.extras_heap_bytes(), 0);
            assert_eq!(call.call_signatures(), &[id]);
            assert_eq!(&*call.call_signature_list(), &[id]);
            assert!(call.construct_signatures().is_empty());
            assert!(call.construct_signature_list().is_empty());
            assert!(call.index_infos().is_empty());
            let construct = record(vec![], vec![id], vec![]);
            assert_eq!(construct.extras_heap_bytes(), 0);
            assert!(construct.call_signatures().is_empty());
            assert!(construct.call_signature_list().is_empty());
            assert_eq!(construct.construct_signatures(), &[id]);
            assert_eq!(&*construct.construct_signature_list(), &[id]);
            // A copy keeps the signature, and never a verdict.
            assert_eq!(call.clone().call_signatures(), &[id]);
            assert_eq!(construct.clone().construct_signatures(), &[id]);
            assert_eq!(call.weak_properties(), None);
            // The readings of the word alone agree with the lists.
            assert!(call.has_call_signatures() && call.has_signatures());
            assert!(!call.has_construct_signatures());
            assert_eq!(call.single_signature(false), Some(id));
            assert_eq!(call.single_signature(true), None);
            assert!(construct.has_construct_signatures() && construct.has_signatures());
            assert!(!construct.has_call_signatures());
            assert_eq!(construct.single_signature(true), Some(id));
            assert_eq!(construct.single_signature(false), None);
        }
        // Every other combination is a boxed record with the same readings.
        for (calls, constructs, infos) in [
            (vec![a, b], vec![], vec![]),
            (vec![], vec![a, b], vec![]),
            (vec![a], vec![b], vec![]),
            (vec![a], vec![], vec![info]),
            (vec![], vec![b], vec![info]),
            (vec![], vec![], vec![info]),
        ] {
            let boxed = record(calls.clone(), constructs.clone(), infos.clone());
            assert!(boxed.extras_heap_bytes() >= std::mem::size_of::<MembersExtras>());
            assert_eq!(&*boxed.call_signatures(), &calls[..]);
            assert_eq!(&*boxed.call_signature_list(), &calls[..]);
            assert_eq!(&*boxed.construct_signatures(), &constructs[..]);
            assert_eq!(&*boxed.construct_signature_list(), &constructs[..]);
            assert_eq!(boxed.index_infos().len(), infos.len());
            let copy = boxed.clone();
            assert_eq!(copy.call_signatures(), boxed.call_signatures());
            assert_eq!(copy.construct_signatures(), boxed.construct_signatures());
            assert_eq!(boxed.has_call_signatures(), !calls.is_empty());
            assert_eq!(boxed.has_construct_signatures(), !constructs.is_empty());
            assert_eq!(
                boxed.has_signatures(),
                !calls.is_empty() || !constructs.is_empty()
            );
            assert_eq!(
                boxed.single_signature(false),
                (calls.len() == 1 && constructs.is_empty()).then(|| calls[0])
            );
            assert_eq!(
                boxed.single_signature(true),
                (constructs.len() == 1 && calls.is_empty()).then(|| constructs[0])
            );
        }
        // The setters write one list at a time and keep the others; the
        // word follows what the three lists hold.
        let mut written = record(vec![], vec![], vec![]);
        written.set_construct_signatures(vec![]);
        written.set_index_infos(vec![]);
        assert!(written.call_signatures().is_empty());
        assert!(!written.has_signatures() && !written.has_call_signatures());
        assert!(!written.has_construct_signatures());
        assert_eq!(written.single_signature(false), None);
        assert_eq!(written.single_signature(true), None);
        written.set_call_signatures(vec![a]);
        assert_eq!(written.extras_heap_bytes(), 0);
        assert_eq!(written.call_signatures(), &[a]);
        written.set_construct_signatures(vec![b]);
        assert_eq!(written.call_signatures(), &[a]);
        assert_eq!(written.construct_signatures(), &[b]);
        assert!(written.extras_heap_bytes() > 0);
        written.set_call_signatures(vec![]);
        assert!(written.call_signatures().is_empty());
        assert_eq!(written.construct_signatures(), &[b]);
        assert_eq!(written.extras_heap_bytes(), 0);
        written.set_index_infos(vec![info]);
        assert_eq!(written.construct_signatures(), &[b]);
        assert_eq!(written.index_infos().len(), 1);
        written.set_index_infos(vec![]);
        assert_eq!(written.construct_signatures(), &[b]);
        assert_eq!(written.extras_heap_bytes(), 0);
        written.set_lists(vec![a, b], vec![], vec![]);
        assert_eq!(written.call_signatures(), &[a, b]);
        assert!(written.construct_signatures().is_empty());
        written.set_lists(vec![a], vec![], vec![]);
        assert_eq!(written.call_signatures(), &[a]);
        assert_eq!(written.extras_heap_bytes(), 0);
        written.set_call_signatures(vec![]);
        assert!(written.call_signatures().is_empty() && written.construct_signatures().is_empty());
        assert_eq!(written.weak_properties(), None);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn published_layout_is_48_bytes_with_one_index_word() {
        assert_eq!(std::mem::size_of::<MemberTable>(), 32);
        assert_eq!(std::mem::size_of::<FrozenMembers>(), 24);
        assert_eq!(std::mem::size_of::<ResolvedMembers>(), 48);
        assert_eq!(std::mem::size_of::<MembersExtras>(), 48);
    }

    #[test]
    fn a_table_keyed_apart_from_a_members_name_answers_by_its_keys() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let key = |name: &str| EscapedName::from_identifier_escaped_text(name);
            for len in [2usize, 8, 9, 12] {
                let ids = (0..len)
                    .map(|i| property(&mut state.binder, &format!("m{i}")))
                    .collect::<Vec<_>>();
                // The keys of a table before any merge are its members' names.
                let mut plain = SymbolTable::default();
                for (i, &id) in ids.iter().enumerate() {
                    plain.insert(key(&format!("m{i}")), id);
                }
                // The second member sits under another key than its name, as
                // the clone of an alias's target sits under the alias's name.
                let mut apart = SymbolTable::default();
                for (i, &id) in ids.iter().enumerate() {
                    let name = if i == 1 {
                        key("alias")
                    } else {
                        key(&format!("m{i}"))
                    };
                    apart.insert(name, id);
                }
                state.binder.note_member_keyed_apart();

                let frozen = MemberTableSource::new(&state.binder, &apart).freeze(|_, _| {});
                let builder = MemberTable::from_symbol_table(&state.binder, &apart);
                let republished: FrozenMembers = builder.clone().into();
                let again: FrozenMembers = frozen.clone().into_builder().into();
                let extra = property(&mut state.binder, "extra");
                let mut grown = frozen.clone();
                assert_eq!(grown.insert(&state.binder, extra), None);
                for members in [&frozen, &republished, &again, &grown] {
                    assert!(members.index.inline_bits().is_none());
                    assert_eq!(members.get(&state.binder, "alias"), Some(ids[1]));
                    assert_eq!(members.get(&state.binder, "m1"), None);
                    assert_eq!(members.get(&state.binder, "m0"), Some(ids[0]));
                    assert_eq!(members.symbols()[..len], ids[..]);
                }
                assert_eq!(builder.get(&state.binder, "alias"), Some(ids[1]));
                assert_eq!(builder.get(&state.binder, "m1"), None);
                assert_eq!(grown.get(&state.binder, "extra"), Some(extra));

                // A table whose keys are its members' names keeps its form.
                let named = MemberTableSource::new(&state.binder, &plain).freeze(|_, _| {});
                assert_eq!(named.index.inline_bits().is_some(), len <= LINEAR_MEMBERS);
                assert_eq!(named.get(&state.binder, "m1"), Some(ids[1]));
                assert_eq!(named.get(&state.binder, "alias"), None);
            }
        });
    }

    #[test]
    fn name_tags_answer_small_tables_and_survive_equal_tags() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            // Enough names that some share a seven-bit tag with a member.
            let names = (0..600).map(|i| format!("n{i}")).collect::<Vec<_>>();
            let ids = names[..8]
                .iter()
                .map(|name| property(&mut state.binder, name))
                .collect::<Vec<_>>();
            for len in 0..=8 {
                let mut builder = MemberTable::with_capacity(0);
                for &id in &ids[..len] {
                    assert_eq!(builder.insert(&state.binder, id), None);
                }
                assert!(builder.index_address().is_none());
                let frozen: FrozenMembers = builder.clone().into();
                let mut equal_tags = 0;
                for (position, name) in names.iter().enumerate() {
                    let expected = (position < len).then(|| ids[position]);
                    let key = HashedName::new(EscapedName::from_identifier_escaped_text(name));
                    assert_eq!(builder.get(&state.binder, name.as_str()), expected);
                    assert_eq!(frozen.get_hashed(&state.binder, key), expected);
                    if position >= len {
                        let tags = frozen.index.inline_bits().unwrap();
                        equal_tags += usize::from(tag_matches(tags, name_tag(key.hash)) != 0);
                    }
                }
                // The absent names that share a tag were refused by their names.
                // A name's tag is seven bits of its process-wide id times a
                // constant. The names here get nearly consecutive ids, so
                // their tags repeat almost exactly every 44 names and drift
                // slowly: for about one first id in eighteen none of the six
                // hundred returns to the first member's tag, and which first
                // id a run gets depends on what the suite's other tests
                // interned before. So the search goes on until a name shares
                // a tag (within a thousand further names of consecutive ids).
                if len != 0 && equal_tags == 0 {
                    let tags = frozen.index.inline_bits().unwrap();
                    let (name, key) = (names.len()..)
                        .map(|i| {
                            let name = format!("n{i}");
                            let key =
                                HashedName::new(EscapedName::from_identifier_escaped_text(&name));
                            (name, key)
                        })
                        .find(|(_, key)| tag_matches(tags, name_tag(key.hash)) != 0)
                        .expect("some further name shares a member's tag");
                    assert_eq!(builder.get(&state.binder, name.as_str()), None);
                    assert_eq!(frozen.get_hashed(&state.binder, key), None);
                    equal_tags += 1;
                }
                assert_eq!(equal_tags == 0, len == 0);
                assert!(frozen.index.inline_bits().unwrap().to_le_bytes()[len..]
                    .iter()
                    .all(|&lane| lane == 0));
            }
            // A member of a present name is replaced in its position.
            let mut builder = MemberTable::from_symbols(&state.binder, &ids);
            let replacement = property(&mut state.binder, "n3");
            assert_eq!(builder.insert(&state.binder, replacement), Some(ids[3]));
            assert_eq!(builder.symbols()[3], replacement);
            assert_eq!(builder.len(), 8);
        });
    }

    #[test]
    fn tag_lanes_are_judged_exactly() {
        assert_eq!(name_tag(0), 1);
        assert_eq!(name_tag(1 << 57), 1);
        for len in 0..=LINEAR_MEMBERS {
            for seed in 0..256u64 {
                let mut lanes = [0u8; LINEAR_MEMBERS];
                for (position, lane) in lanes[..len].iter_mut().enumerate() {
                    let raw = seed
                        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
                        .rotate_left(position as u32 * 8);
                    *lane = name_tag(raw) as u8;
                }
                let tags = u64::from_le_bytes(lanes);
                assert_eq!(tags >> 63, 0);
                for tag in 1..=127 {
                    let expected = lanes
                        .iter()
                        .enumerate()
                        .filter(|(_, &lane)| u64::from(lane) == tag)
                        .fold(0u64, |bits, (i, _)| bits | (0x80 << (8 * i)));
                    assert_eq!(tag_matches(tags, tag), expected);
                }
            }
        }
    }

    #[test]
    fn builders_that_repeat_a_name_sequence_share_one_index() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let names = (0..12).map(|i| format!("m{i}")).collect::<Vec<_>>();
            let make = |state: &mut crate::state::CheckerState<'_>| {
                names
                    .iter()
                    .map(|name| property(&mut state.binder, name))
                    .collect::<Vec<_>>()
            };
            let first_ids = make(state);
            let first: FrozenMembers = MemberTable::from_symbols(&state.binder, &first_ids).into();
            let shape = first.shape().unwrap();
            assert_eq!(shape.len(), 12);
            assert!(first.has_shape(&shape));

            // The same sequence: no index is built, the published table shares it.
            let ids = make(state);
            let mut builder = MemberTable::following(&shape, 0);
            for (position, &id) in ids.iter().enumerate() {
                assert_eq!(builder.get(&state.binder, names[position].as_str()), None);
                assert_eq!(builder.insert(&state.binder, id), None);
                assert_eq!(
                    builder.get(&state.binder, names[position].as_str()),
                    Some(id)
                );
            }
            builder.reserve(&state.binder, 4);
            let same: FrozenMembers = builder.into();
            assert!(same.has_shape(&shape));
            assert_eq!(same.symbols(), ids);
            assert_eq!(same.get(&state.binder, "m11"), Some(ids[11]));
            assert_eq!(first.get(&state.binder, "m11"), Some(first_ids[11]));
            let mut seen = Default::default();
            assert!(first.counted_heap_bytes(&mut seen) > 0);
            assert_eq!(
                same.counted_heap_bytes(&mut seen),
                same.symbols.heap_bytes()
            );

            // A prefix of the sequence shares the index too; the later names are absent.
            let ids = make(state);
            let mut builder = MemberTable::following(&shape, 0);
            for &id in &ids[..10] {
                builder.insert(&state.binder, id);
            }
            let replacement = property(&mut state.binder, "m2");
            assert_eq!(builder.insert(&state.binder, replacement), Some(ids[2]));
            let prefix: FrozenMembers = builder.into();
            assert!(prefix.has_shape(&shape));
            assert_eq!(prefix.len(), 10);
            assert_eq!(prefix.get(&state.binder, "m9"), Some(ids[9]));
            assert_eq!(prefix.get(&state.binder, "m10"), None);
            assert_eq!(prefix.symbols()[2], replacement);

            // A short prefix is published with tags.
            let ids = make(state);
            let mut builder = MemberTable::following(&shape, 0);
            for &id in &ids[..3] {
                builder.insert(&state.binder, id);
            }
            let short: FrozenMembers = builder.into();
            assert!(short.index_address().is_none());
            assert_eq!(short.get(&state.binder, "m2"), Some(ids[2]));
            assert_eq!(short.get(&state.binder, "m3"), None);

            // Leaving the sequence gives the builder its own index; the shared one is untouched.
            let ids = make(state);
            let other = property(&mut state.binder, "other");
            let mut builder = MemberTable::following(&shape, 0);
            for &id in &ids[..9] {
                builder.insert(&state.binder, id);
            }
            assert_eq!(builder.insert(&state.binder, ids[11]), None);
            assert_eq!(builder.insert(&state.binder, other), None);
            assert_eq!(builder.insert(&state.binder, ids[9]), None);
            let left: FrozenMembers = builder.into();
            assert!(!left.has_shape(&shape));
            let expected = [&ids[..9], &[ids[11], other, ids[9]]].concat();
            assert_eq!(left.symbols(), expected);
            for (position, &id) in expected.iter().enumerate() {
                let name = state.binder.symbol(id).escaped_name;
                assert_eq!(
                    left.get_hashed(&state.binder, HashedName::new(name)),
                    Some(id)
                );
                assert_eq!(left.symbols()[position], id);
            }
            assert_eq!(left.get(&state.binder, "m10"), None);
            assert_eq!(same.get(&state.binder, "other"), None);
            assert_eq!(same.get(&state.binder, "m10"), Some(same.symbols()[10]));

            // Appending to a complete followed sequence copies the index for the builder.
            let ids = make(state);
            let mut builder = MemberTable::following(&shape, 0);
            for &id in &ids {
                builder.insert(&state.binder, id);
            }
            assert_eq!(builder.insert(&state.binder, other), None);
            let longer: FrozenMembers = builder.into();
            assert!(!longer.has_shape(&shape));
            assert_eq!(longer.len(), 13);
            assert_eq!(longer.get(&state.binder, "other"), Some(other));
            assert_eq!(first.get(&state.binder, "other"), None);

            // A published table that is written again copies the shared index first.
            let mut written = same.clone();
            assert_eq!(written.insert(&state.binder, other), None);
            assert!(!written.has_shape(&shape));
            assert!(same.has_shape(&shape));
            assert_eq!(same.len(), 12);
            assert_eq!(written.get(&state.binder, "other"), Some(other));
        });
    }

    #[test]
    fn published_tables_of_one_name_sequence_end_on_one_index() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            fn properties(
                state: &mut crate::state::CheckerState<'_>,
                names: &[String],
            ) -> Vec<SymbolId> {
                names
                    .iter()
                    .map(|name| property(&mut state.binder, name))
                    .collect()
            }
            let names = (0..12).map(|i| format!("m{i}")).collect::<Vec<_>>();
            let mut indexes = NameIndexes::default();

            // The first table of a sequence keeps its index: it is the sequence's.
            let first_ids = properties(state, &names);
            let mut first: FrozenMembers =
                MemberTable::from_symbols(&state.binder, &first_ids).into();
            let of_sequence = first.index_address();
            assert!(of_sequence.is_some());
            indexes.share(&mut first);
            assert_eq!(first.index_address(), of_sequence);
            assert_eq!(indexes.len(), 1);

            // A table built on its own with the same sequence gives up its index.
            let ids = properties(state, &names);
            let mut second: FrozenMembers = MemberTable::from_symbols(&state.binder, &ids).into();
            assert_ne!(second.index_address(), of_sequence);
            indexes.share(&mut second);
            assert_eq!(second.index_address(), of_sequence);
            assert_eq!(second.symbols(), ids);
            for (position, name) in names.iter().enumerate() {
                assert_eq!(
                    second.get(&state.binder, name.as_str()),
                    Some(ids[position])
                );
                assert_eq!(
                    first.get(&state.binder, name.as_str()),
                    Some(first_ids[position])
                );
            }
            assert_eq!(second.get(&state.binder, "absent"), None);
            let mut seen = Default::default();
            assert!(first.counted_heap_bytes(&mut seen) > first.symbols.heap_bytes());
            assert_eq!(
                second.counted_heap_bytes(&mut seen),
                second.symbols.heap_bytes()
            );
            // Asking again changes nothing.
            indexes.share(&mut second);
            assert_eq!(second.index_address(), of_sequence);
            assert_eq!(indexes.len(), 1);

            // The same names in another order are another sequence.
            let mut swapped = names.clone();
            swapped.swap(0, 1);
            let ids = properties(state, &swapped);
            let mut reordered: FrozenMembers =
                MemberTable::from_symbols(&state.binder, &ids).into();
            let own = reordered.index_address();
            indexes.share(&mut reordered);
            assert_eq!(reordered.index_address(), own);
            assert_ne!(own, of_sequence);
            assert_eq!(indexes.len(), 2);
            assert_eq!(reordered.get(&state.binder, "m0"), Some(ids[1]));
            assert_eq!(reordered.get(&state.binder, "m1"), Some(ids[0]));

            // A table that keeps tags has no index to share.
            let ids = properties(state, &names[..4]);
            let mut small: FrozenMembers = MemberTable::from_symbols(&state.binder, &ids).into();
            indexes.share(&mut small);
            assert!(small.index_address().is_none());
            assert_eq!(indexes.len(), 2);

            // A builder that follows the sequence's index and adds a name writes a copy; the
            // copy is its own sequence's index, and the sequence's index is untouched.
            let shape = second.shape().unwrap();
            let ids = properties(state, &names);
            let extra = property(&mut state.binder, "extra");
            let mut builder = MemberTable::following(&shape, 0);
            for &id in &ids {
                builder.insert(&state.binder, id);
            }
            assert_eq!(builder.insert(&state.binder, extra), None);
            let mut longer: FrozenMembers = builder.into();
            let longer_own = longer.index_address();
            assert_ne!(longer_own, of_sequence);
            indexes.share(&mut longer);
            assert_eq!(longer.index_address(), longer_own);
            assert_eq!(indexes.len(), 3);
            assert_eq!(longer.get(&state.binder, "extra"), Some(extra));
            assert_eq!(first.get(&state.binder, "extra"), None);
            assert_eq!(second.get(&state.binder, "extra"), None);
            // The next table of the longer sequence, built on its own, finds that copy.
            let ids = [
                properties(state, &names),
                vec![property(&mut state.binder, "extra")],
            ]
            .concat();
            let mut again: FrozenMembers = MemberTable::from_symbols(&state.binder, &ids).into();
            indexes.share(&mut again);
            assert_eq!(again.index_address(), longer_own);
            assert_eq!(again.get(&state.binder, "extra"), Some(ids[12]));

            // A table that shares a longer sequence's index as its prefix stays on it.
            let ids = properties(state, &names);
            let mut builder = MemberTable::following(&shape, 0);
            for &id in &ids[..10] {
                builder.insert(&state.binder, id);
            }
            let mut prefix: FrozenMembers = builder.into();
            indexes.share(&mut prefix);
            assert_eq!(prefix.index_address(), of_sequence);
            assert_eq!(indexes.len(), 3);
            assert_eq!(prefix.get(&state.binder, "m9"), Some(ids[9]));
            assert_eq!(prefix.get(&state.binder, "m10"), None);

            // A published table of the sequence that is written again leaves it.
            let mut written = second.clone();
            let other = property(&mut state.binder, "other");
            assert_eq!(written.insert(&state.binder, other), None);
            assert_ne!(written.index_address(), of_sequence);
            assert_eq!(second.get(&state.binder, "other"), None);
            assert_eq!(second.index_address(), of_sequence);
        });
    }
}
