use indexmap::{map, set, IndexMap, IndexSet};
use rustc_hash::FxBuildHasher;
use tsc_types::{EscapedName, JsStr, SymbolId};

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

type SymbolMap = IndexMap<EscapedName, SymbolId, FxBuildHasher>;

// Whether the map was ever allocated is a storage detail: an unallocated
// table and an allocated empty one hold the same entries.
impl PartialEq for SymbolTable {
    fn eq(&self, other: &Self) -> bool {
        self.map() == other.map()
    }
}

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
    pub fn contains<'a>(&self, name: impl Into<JsStr<'a>>) -> bool {
        self.0.contains(name.into().as_bytes())
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

    pub fn get<'a>(&self, key: impl Into<JsStr<'a>>) -> Option<&SymbolId> {
        self.map().get(key.into().as_bytes())
    }

    pub fn get_mut<'a>(&mut self, key: impl Into<JsStr<'a>>) -> Option<&mut SymbolId> {
        self.0.as_deref_mut()?.get_mut(key.into().as_bytes())
    }

    pub fn contains_key<'a>(&self, key: impl Into<JsStr<'a>>) -> bool {
        self.map().contains_key(key.into().as_bytes())
    }

    pub fn shift_remove<'a>(&mut self, key: impl Into<JsStr<'a>>) -> Option<SymbolId> {
        self.0.as_deref_mut()?.shift_remove(key.into().as_bytes())
    }

    pub fn get_full<'a>(
        &self,
        key: impl Into<JsStr<'a>>,
    ) -> Option<(usize, &EscapedName, &SymbolId)> {
        self.map().get_full(key.into().as_bytes())
    }

    pub fn get_index(&self, index: usize) -> Option<(&EscapedName, &SymbolId)> {
        self.map().get_index(index)
    }

    pub fn insert(&mut self, key: EscapedName, value: SymbolId) -> Option<SymbolId> {
        self.map_mut().insert(key, value)
    }

    pub fn entry(&mut self, key: EscapedName) -> map::Entry<'_, EscapedName, SymbolId> {
        self.map_mut().entry(key)
    }

    pub fn iter(&self) -> map::Iter<'_, EscapedName, SymbolId> {
        self.map().iter()
    }
    pub fn iter_mut(&mut self) -> map::IterMut<'_, EscapedName, SymbolId> {
        self.map_mut().iter_mut()
    }
    pub fn keys(&self) -> map::Keys<'_, EscapedName, SymbolId> {
        self.map().keys()
    }
    pub fn values(&self) -> map::Values<'_, EscapedName, SymbolId> {
        self.map().values()
    }
    pub fn values_mut(&mut self) -> map::ValuesMut<'_, EscapedName, SymbolId> {
        self.map_mut().values_mut()
    }
}

impl FromIterator<(EscapedName, SymbolId)> for SymbolTable {
    fn from_iter<T: IntoIterator<Item = (EscapedName, SymbolId)>>(iter: T) -> Self {
        let map: SymbolMap = iter.into_iter().collect();
        Self((!map.is_empty()).then(|| Box::new(map)))
    }
}

impl Extend<(EscapedName, SymbolId)> for SymbolTable {
    fn extend<T: IntoIterator<Item = (EscapedName, SymbolId)>>(&mut self, iter: T) {
        let mut iter = iter.into_iter().peekable();
        if iter.peek().is_some() {
            self.map_mut().extend(iter);
        }
    }
}

impl IntoIterator for SymbolTable {
    type Item = (EscapedName, SymbolId);
    type IntoIter = map::IntoIter<EscapedName, SymbolId>;
    fn into_iter(self) -> Self::IntoIter {
        match self.0 {
            Some(map) => (*map).into_iter(),
            None => SymbolMap::default().into_iter(),
        }
    }
}

impl<'a> IntoIterator for &'a SymbolTable {
    type Item = (&'a EscapedName, &'a SymbolId);
    type IntoIter = map::Iter<'a, EscapedName, SymbolId>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut SymbolTable {
    type Item = (&'a EscapedName, &'a mut SymbolId);
    type IntoIter = map::IterMut<'a, EscapedName, SymbolId>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<'a, Q: Into<JsStr<'a>>> std::ops::Index<Q> for SymbolTable {
    type Output = SymbolId;
    fn index(&self, key: Q) -> &Self::Output {
        self.get(key).expect("symbol table key exists")
    }
}
