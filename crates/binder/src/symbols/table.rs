use std::hash::{Hash, Hasher};

use hashbrown::hash_table::{Entry, HashTable};
use indexmap::{set, IndexSet};
use rustc_hash::{FxBuildHasher, FxHasher};
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

/// An insertion-ordered name -> symbol map: the entries in one vector and a
/// hash table of their positions, hashed by the name's id. An entry costs its
/// 8 bytes plus about five bytes of index, where an `IndexMap` also keeps
/// every entry's hash and indexes it by `usize` (about 60 bytes). A checker
/// builds such a table for every class or interface instantiation, including
/// the inherited members, so a large program holds millions of entries.
#[derive(Clone, Default)]
struct SymbolMap {
    entries: Vec<(EscapedName, SymbolId)>,
    positions: HashTable<u32>,
}

fn hash_name(name: EscapedName) -> u64 {
    let mut hasher = FxHasher::default();
    name.index().hash(&mut hasher);
    hasher.finish()
}

impl SymbolMap {
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn reserve(&mut self, additional: usize) {
        self.entries.reserve(additional);
        let entries = &self.entries;
        self.positions.reserve(additional, |&position| {
            hash_name(entries[position as usize].0)
        });
    }

    fn position(&self, name: EscapedName) -> Option<usize> {
        let entries = &self.entries;
        self.positions
            .find(hash_name(name), |&position| {
                entries[position as usize].0 == name
            })
            .map(|&position| position as usize)
    }

    /// Insert or replace; a replaced entry keeps its position, as in a
    /// JavaScript `Map`.
    fn insert(&mut self, name: EscapedName, value: SymbolId) -> Option<SymbolId> {
        let entries = &self.entries;
        let entry = self.positions.entry(
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
                self.entries.push((name, value));
                None
            }
        }
    }

    /// Remove an entry and close the gap, keeping the others in order.
    fn shift_remove(&mut self, name: EscapedName) -> Option<SymbolId> {
        let position = self.position(name)?;
        let removed = position as u32;
        self.positions
            .find_entry(hash_name(name), |&candidate| candidate == removed)
            .ok()?
            .remove();
        for candidate in self.positions.iter_mut() {
            if *candidate > removed {
                *candidate -= 1;
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
                    .position(*name)
                    .is_some_and(|position| other.entries[position].1 == *symbol)
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
pub type IntoIter = std::vec::IntoIter<(EscapedName, SymbolId)>;

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
            std::mem::size_of::<SymbolMap>()
                + map.entries.capacity() * std::mem::size_of::<(EscapedName, SymbolId)>()
                + map.positions.capacity() * (std::mem::size_of::<u32>() + 1)
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
        let map = self.0.as_deref()?;
        let position = map.position(key.name())?;
        Some(&map.entries[position].1)
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
        let map = self.0.as_deref()?;
        let position = map.position(key.name())?;
        let (name, symbol) = &map.entries[position];
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
            None => Vec::new().into_iter(),
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
