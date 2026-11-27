use std::hash::{Hash, Hasher};

use hashbrown::hash_table::{Entry, HashTable};
use indexmap::{set, IndexSet};
use rustc_hash::{FxBuildHasher, FxHasher};
use smallvec::SmallVec;
use tsc_store::InlineOrBox;
use tsc_types::{EscapedName, JsStr, JsString, SymbolId};

/// A key of a symbol table: a name, or an already escaped text, which is
/// interned verbatim (a text no symbol carries then misses in the table).
/// Lookups by name cost an id comparison; prefer them on hot paths.
pub trait NameKey {
    fn name(self) -> EscapedName;
}

impl NameKey for EscapedName {
    fn name(self) -> EscapedName {
        self
    }
}

impl NameKey for &EscapedName {
    fn name(self) -> EscapedName {
        *self
    }
}

impl NameKey for &str {
    #[cfg_attr(feature = "perf-counters", track_caller)]
    fn name(self) -> EscapedName {
        tsc_types::perf::name_site(std::panic::Location::caller());
        tsc_types::perf::bump(tsc_types::perf::PerfCounter::NamesTextKeyLookups);
        EscapedName::from_escaped_text(JsStr::from(self))
    }
}

impl NameKey for &String {
    #[cfg_attr(feature = "perf-counters", track_caller)]
    fn name(self) -> EscapedName {
        tsc_types::perf::name_site(std::panic::Location::caller());
        tsc_types::perf::bump(tsc_types::perf::PerfCounter::NamesTextKeyLookups);
        EscapedName::from_escaped_text(JsStr::from(self))
    }
}

impl NameKey for JsStr<'_> {
    #[cfg_attr(feature = "perf-counters", track_caller)]
    fn name(self) -> EscapedName {
        tsc_types::perf::name_site(std::panic::Location::caller());
        tsc_types::perf::bump(tsc_types::perf::PerfCounter::NamesTextKeyLookups);
        EscapedName::from_escaped_text(self)
    }
}

impl NameKey for &JsString {
    #[cfg_attr(feature = "perf-counters", track_caller)]
    fn name(self) -> EscapedName {
        tsc_types::perf::name_site(std::panic::Location::caller());
        tsc_types::perf::bump(tsc_types::perf::PerfCounter::NamesTextKeyLookups);
        EscapedName::from_escaped_text(self.as_js())
    }
}

/// Ordered escaped-name identity storage. Only canonical JavaScript strings
/// can cross the public query boundary; byte borrowing is an internal detail.
/// In particular, arbitrary noncanonical WTF-8 bytes cannot silently miss.
#[derive(Clone, Debug, Default)]
// Iteration follows insertion order, never the hash, so the faster
// lookup-only hasher changes no observable order.
//
// The map is boxed and allocated on the first write: most symbols never own
// members or exports, and a binder `Symbol` carries three tables, so an
// empty table costs one pointer instead of a whole inline map (memory
// bandwidth is a measured cost of the parallel checkers).
pub struct SymbolTable(Option<Box<SymbolMap>>);

/// The locals of a scope that has locals and no entry: a bound file keeps
/// only the tables with entries ([`crate::BindData::locals`]).
pub static EMPTY_SYMBOL_TABLE: SymbolTable = SymbolTable(None);

/// An insertion-ordered name -> symbol map: the entries in one vector and,
/// once the map has [`INDEXED_FROM`] entries, a boxed hash table of their
/// positions, hashed by the name's id. An entry costs its 8 bytes plus about
/// five bytes of index, where an `IndexMap` also keeps every entry's hash and
/// indexes it by `usize` (about 60 bytes). A checker builds such a table for
/// every class or interface instantiation, including the inherited members,
/// so a large program holds millions of entries; most maps are small, and a
/// small one uses a fingerprint to reject misses before scanning its entries.
///
/// The first [`INLINE_ENTRIES`] entries live in the map itself: most binder
/// tables (a scope's locals, a symbol's members or exports) hold one or two
/// entries — two of three on the largest bench project, four of five on the
/// smallest — and such a table is then this one block instead of the map and
/// a separate vector of four slots.
#[derive(Default)]
struct SymbolMap {
    entries: EntryVec,
    positions: InlineOrBox<HashTable<u32>>,
}

// A copy's vector holds exactly its entries, as a `Vec`'s copy does (the
// small vector's own `clone` rounds the size up to a power of two).
impl Clone for SymbolMap {
    fn clone(&self) -> Self {
        let mut entries = EntryVec::new();
        if self.entries.len() > INLINE_ENTRIES {
            entries.reserve_exact(self.entries.len());
        }
        entries.extend_from_slice(&self.entries);
        Self {
            entries,
            positions: self.positions.clone(),
        }
    }
}

/// The entries a map holds without a vector of its own. Two entries take the
/// sixteen bytes a vector's pointer and length take, so the map keeps its
/// size (asserted below).
const INLINE_ENTRIES: usize = 2;
type EntryVec = SmallVec<[(EscapedName, SymbolId); INLINE_ENTRIES]>;

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SymbolMap>() == 32);

/// The size from which a map indexes its entries; a smaller one is scanned
/// (a name is a 4-byte id, so a scan is a few compares).
const INDEXED_FROM: usize = 9;

fn hash_name(name: EscapedName) -> u64 {
    let mut hasher = FxHasher::default();
    name.index().hash(&mut hasher);
    hasher.finish()
}

/// One bit per name, with the last lane folded into its neighbor to fit the
/// tagged word. Exact comparisons resolve collisions.
#[inline]
fn name_bit(name: EscapedName) -> usize {
    let index = name.index() as usize;
    let lane = (index ^ (index >> 6)) & (usize::BITS as usize - 1);
    1 << lane.min(usize::BITS as usize - 2)
}

#[inline]
fn find_unindexed(
    entries: &[(EscapedName, SymbolId)],
    bits: usize,
    name: EscapedName,
) -> Option<usize> {
    match entries {
        [] => None,
        [(first, _)] => (*first == name).then_some(0),
        [(first, _), (second, _)] => {
            if *first == name {
                Some(0)
            } else {
                (*second == name).then_some(1)
            }
        }
        _ if bits & name_bit(name) == 0 => None,
        _ => entries.iter().position(|(candidate, _)| *candidate == name),
    }
}

impl SymbolMap {
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Room in the entries for `additional` more, asked for in advance: at
    /// least four slots, at least twice the present ones, and what was asked
    /// for — the vector a `Vec` would have reserved.
    fn reserve_entries(&mut self, additional: usize) {
        let required = self.entries.len() + additional;
        let capacity = self.entries.capacity();
        if required > capacity {
            let target = required.max(capacity * 2).max(4);
            self.entries.reserve_exact(target - self.entries.len());
        }
    }

    fn reserve(&mut self, additional: usize) {
        self.reserve_entries(additional);
        if let Some(positions) = self.positions.as_boxed_mut() {
            let entries = self.entries.as_slice();
            positions.reserve(additional, |&position| {
                hash_name(entries[position as usize].0)
            });
        }
    }

    /// Index the entries (the map reached [`INDEXED_FROM`] entries).
    fn index(&mut self) {
        let entries = self.entries.as_slice();
        let mut positions = HashTable::with_capacity(entries.len());
        for (position, (name, _)) in entries.iter().enumerate() {
            positions.insert_unique(hash_name(*name), position as u32, |&position| {
                hash_name(entries[position as usize].0)
            });
        }
        self.positions = InlineOrBox::boxed(positions);
    }

    /// The entry of `name` and its position. A read decides once whether the
    /// entries live in the map or in a vector, here.
    #[inline]
    fn find(&self, name: EscapedName) -> Option<(usize, &(EscapedName, SymbolId))> {
        let entries = self.entries.as_slice();
        let position = match self.positions.inline_bits() {
            Some(bits) => find_unindexed(entries, bits, name),
            None => self
                .positions
                .as_boxed()
                .unwrap()
                .find(hash_name(name), |&position| {
                    entries[position as usize].0 == name
                })
                .map(|&position| position as usize),
        }?;
        Some((position, &entries[position]))
    }

    fn position(&self, name: EscapedName) -> Option<usize> {
        self.find(name).map(|(position, _)| position)
    }

    /// Append in insertion order to a map without an index (`bits` are its
    /// name bits) and build the index at the usual threshold.
    fn append_unindexed(&mut self, bits: usize, name: EscapedName, value: SymbolId) {
        // A full vector grows to the next power of two: four slots after
        // the two inline entries, then doubling.
        self.entries.push((name, value));
        if self.entries.len() >= INDEXED_FROM {
            self.index();
        } else {
            self.positions = InlineOrBox::inline(bits | name_bit(name));
        }
    }

    fn insert_absent(&mut self, name: EscapedName, value: SymbolId) {
        debug_assert!(self.find(name).is_none());
        if let Some(bits) = self.positions.inline_bits() {
            self.append_unindexed(bits, name, value);
            return;
        }
        let positions = self.positions.as_boxed_mut().unwrap();
        let entries = self.entries.as_slice();
        let position = u32::try_from(entries.len()).expect("symbol table size");
        positions.insert_unique(hash_name(name), position, |&position| {
            hash_name(entries[position as usize].0)
        });
        self.entries.push((name, value));
    }

    /// Insert or replace; a replaced entry keeps its position, as in a
    /// JavaScript `Map`.
    fn insert(&mut self, name: EscapedName, value: SymbolId) -> Option<SymbolId> {
        if let Some(bits) = self.positions.inline_bits() {
            if let Some(position) = find_unindexed(&self.entries, bits, name) {
                return Some(std::mem::replace(&mut self.entries[position].1, value));
            }
            self.append_unindexed(bits, name, value);
            return None;
        }
        let positions = self.positions.as_boxed_mut().unwrap();
        let entries = self.entries.as_slice();
        let entry = positions.entry(
            hash_name(name),
            |&position| entries[position as usize].0 == name,
            |&position| hash_name(entries[position as usize].0),
        );
        match entry {
            Entry::Occupied(slot) => {
                let position = *slot.get() as usize;
                Some(std::mem::replace(&mut self.entries[position].1, value))
            }
            Entry::Vacant(slot) => {
                let position = u32::try_from(self.entries.len()).expect("symbol table size");
                slot.insert(position);
                // Only the entries grow here: the index names the new
                // position already and is not rehashed before the entry
                // exists.
                self.entries.push((name, value));
                None
            }
        }
    }

    /// Remove an entry and close the gap, keeping the others in order.
    fn shift_remove(&mut self, name: EscapedName) -> Option<SymbolId> {
        let position = self.position(name)?;
        if let Some(positions) = self.positions.as_boxed_mut() {
            let removed = position as u32;
            positions
                .find_entry(hash_name(name), |&candidate| candidate == removed)
                .ok()?
                .remove();
            for candidate in positions.iter_mut() {
                if *candidate > removed {
                    *candidate -= 1;
                }
            }
        }
        Some(self.entries.remove(position).1)
    }
}

impl std::fmt::Debug for SymbolMap {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_map()
            .entries(self.entries.iter().map(|(name, symbol)| (name, symbol)))
            .finish()
    }
}

// Map equality: the same names bound to the same symbols, in any order.
// Whether the map was ever allocated is a storage detail: an unallocated
// table and an allocated empty one hold the same entries.
impl PartialEq for SymbolTable {
    fn eq(&self, other: &Self) -> bool {
        let (this, other) = (self.map(), other.map());
        this.len() == other.len()
            && this.entries.iter().all(|(name, symbol)| {
                other
                    .find(*name)
                    .is_some_and(|(_, entry)| entry.1 == *symbol)
            })
    }
}

type Entries<'a> = std::slice::Iter<'a, (EscapedName, SymbolId)>;
type EntriesMut<'a> = std::slice::IterMut<'a, (EscapedName, SymbolId)>;
/// In-order `(name, symbol)` pairs.
pub type Iter<'a> =
    std::iter::Map<Entries<'a>, fn(&'a (EscapedName, SymbolId)) -> (&'a EscapedName, &'a SymbolId)>;
pub type IterMut<'a> = std::iter::Map<
    EntriesMut<'a>,
    fn(&'a mut (EscapedName, SymbolId)) -> (&'a EscapedName, &'a mut SymbolId),
>;
pub type Keys<'a> = std::iter::Map<Entries<'a>, fn(&'a (EscapedName, SymbolId)) -> &'a EscapedName>;
pub type Values<'a> = std::iter::Map<Entries<'a>, fn(&'a (EscapedName, SymbolId)) -> &'a SymbolId>;
pub type ValuesMut<'a> =
    std::iter::Map<EntriesMut<'a>, fn(&'a mut (EscapedName, SymbolId)) -> &'a mut SymbolId>;
pub type IntoIter = smallvec::IntoIter<[(EscapedName, SymbolId); INLINE_ENTRIES]>;

fn empty_symbol_map() -> &'static SymbolMap {
    static EMPTY: std::sync::OnceLock<SymbolMap> = std::sync::OnceLock::new();
    EMPTY.get_or_init(SymbolMap::default)
}

/// Ordered classifiable-name membership with the same canonical query
/// boundary as `SymbolTable`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EscapedNameSet(IndexSet<EscapedName, FxBuildHasher>);

impl EscapedNameSet {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }
    pub fn insert(&mut self, name: EscapedName) -> bool {
        self.0.insert(name)
    }
    pub fn contains(&self, name: impl NameKey) -> bool {
        self.0.contains(&name.name())
    }
    pub fn iter(&self) -> set::Iter<'_, EscapedName> {
        self.0.iter()
    }
}

impl IntoIterator for EscapedNameSet {
    type Item = EscapedName;
    type IntoIter = set::IntoIter<EscapedName>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl SymbolTable {
    /// Bytes this table owns on the heap, for memory accounting.
    pub fn heap_bytes(&self) -> usize {
        self.0.as_ref().map_or(0, |map| {
            let vector = if map.entries.spilled() {
                map.entries.capacity() * std::mem::size_of::<(EscapedName, SymbolId)>()
            } else {
                0
            };
            std::mem::size_of::<SymbolMap>()
                + vector
                + map.positions.as_boxed().map_or(0, |positions| {
                    std::mem::size_of::<HashTable<u32>>()
                        + positions.capacity() * (std::mem::size_of::<u32>() + 1)
                })
        })
    }

    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    fn map(&self) -> &SymbolMap {
        match &self.0 {
            Some(map) => map,
            None => empty_symbol_map(),
        }
    }
    #[inline]
    fn map_mut(&mut self) -> &mut SymbolMap {
        self.0.get_or_insert_with(Box::default)
    }
    pub fn len(&self) -> usize {
        self.map().len()
    }
    pub fn is_empty(&self) -> bool {
        self.map().is_empty()
    }
    pub fn clear(&mut self) {
        // Dropping the map keeps the cleared table at pointer size, exactly
        // like a table that was never written.
        self.0 = None;
    }
    pub fn reserve(&mut self, additional: usize) {
        if additional > 0 {
            self.map_mut().reserve(additional);
        }
    }

    pub fn get(&self, key: impl NameKey) -> Option<&SymbolId> {
        let (_, entry) = self.0.as_deref()?.find(key.name())?;
        Some(&entry.1)
    }

    pub fn get_mut(&mut self, key: impl NameKey) -> Option<&mut SymbolId> {
        let map = self.0.as_deref_mut()?;
        let position = map.position(key.name())?;
        Some(&mut map.entries[position].1)
    }

    pub fn contains_key(&self, key: impl NameKey) -> bool {
        self.get(key).is_some()
    }

    pub fn shift_remove(&mut self, key: impl NameKey) -> Option<SymbolId> {
        self.0.as_deref_mut()?.shift_remove(key.name())
    }

    pub fn get_full(&self, key: impl NameKey) -> Option<(usize, &EscapedName, &SymbolId)> {
        let (position, (name, symbol)) = self.0.as_deref()?.find(key.name())?;
        Some((position, name, symbol))
    }

    pub fn get_index(&self, index: usize) -> Option<(&EscapedName, &SymbolId)> {
        self.map()
            .entries
            .get(index)
            .map(|(name, symbol)| (name, symbol))
    }

    pub fn insert(&mut self, key: EscapedName, value: SymbolId) -> Option<SymbolId> {
        self.map_mut().insert(key, value)
    }

    /// Insert after a miss in this table, with no intervening table writes.
    pub(crate) fn insert_absent(&mut self, key: EscapedName, value: SymbolId) {
        self.map_mut().insert_absent(key, value);
    }

    pub fn iter(&self) -> Iter<'_> {
        self.map()
            .entries
            .iter()
            .map(|(name, symbol)| (name, symbol))
    }
    pub fn iter_mut(&mut self) -> IterMut<'_> {
        self.map_mut()
            .entries
            .iter_mut()
            .map(|(name, symbol)| (&*name, symbol))
    }
    pub fn keys(&self) -> Keys<'_> {
        self.map().entries.iter().map(|(name, _)| name)
    }
    pub fn values(&self) -> Values<'_> {
        self.map().entries.iter().map(|(_, symbol)| symbol)
    }
    pub fn values_mut(&mut self) -> ValuesMut<'_> {
        self.map_mut().entries.iter_mut().map(|(_, symbol)| symbol)
    }
}

impl FromIterator<(EscapedName, SymbolId)> for SymbolTable {
    fn from_iter<T: IntoIterator<Item = (EscapedName, SymbolId)>>(iter: T) -> Self {
        let mut table = Self::default();
        table.extend(iter);
        table
    }
}

impl Extend<(EscapedName, SymbolId)> for SymbolTable {
    fn extend<T: IntoIterator<Item = (EscapedName, SymbolId)>>(&mut self, iter: T) {
        let mut iter = iter.into_iter().peekable();
        if iter.peek().is_some() {
            let map = self.map_mut();
            map.reserve(iter.size_hint().0);
            for (name, symbol) in iter {
                map.insert(name, symbol);
            }
        }
    }
}

impl IntoIterator for SymbolTable {
    type Item = (EscapedName, SymbolId);
    type IntoIter = IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        match self.0 {
            Some(map) => map.entries.into_iter(),
            None => EntryVec::new().into_iter(),
        }
    }
}

impl<'a> IntoIterator for &'a SymbolTable {
    type Item = (&'a EscapedName, &'a SymbolId);
    type IntoIter = Iter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut SymbolTable {
    type Item = (&'a EscapedName, &'a mut SymbolId);
    type IntoIter = IterMut<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<Q: NameKey> std::ops::Index<Q> for SymbolTable {
    type Output = SymbolId;
    fn index(&self, key: Q) -> &Self::Output {
        self.get(key).expect("symbol table key exists")
    }
}

impl EscapedNameSet {
    /// IndexSet's public entry capacity, plus an estimated hash index.
    /// The dense entries carry both a cached hash and an EscapedName.
    pub fn memory_usage(&self) -> (usize, usize, usize) {
        let record = std::mem::size_of::<(usize, EscapedName)>();
        let cap = self.0.capacity();
        let buckets = if cap == 0 {
            0
        } else {
            (cap + 1).next_power_of_two()
        };
        let controls = if cap == 0 { 0 } else { buckets + 16 };
        (
            self.0.len(),
            self.0.len() * (record + std::mem::size_of::<usize>()) + controls,
            cap * record + buckets * std::mem::size_of::<usize>() + controls,
        )
    }
}
