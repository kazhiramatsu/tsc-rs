use indexmap::{map, set, IndexMap, IndexSet};
use tsc_types::{EscapedName, JsStr, SymbolId};

/// Ordered escaped-name identity storage. Only canonical JavaScript strings
/// can cross the public query boundary; byte borrowing is an internal detail.
/// In particular, arbitrary noncanonical WTF-8 bytes cannot silently miss.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SymbolTable(IndexMap<EscapedName, SymbolId>);

/// Ordered classifiable-name membership with the same canonical query
/// boundary as `SymbolTable`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EscapedNameSet(IndexSet<EscapedName>);

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
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }

    pub fn get<'a>(&self, key: impl Into<JsStr<'a>>) -> Option<&SymbolId> {
        self.0.get(key.into().as_bytes())
    }

    pub fn get_mut<'a>(&mut self, key: impl Into<JsStr<'a>>) -> Option<&mut SymbolId> {
        self.0.get_mut(key.into().as_bytes())
    }

    pub fn contains_key<'a>(&self, key: impl Into<JsStr<'a>>) -> bool {
        self.0.contains_key(key.into().as_bytes())
    }

    pub fn shift_remove<'a>(&mut self, key: impl Into<JsStr<'a>>) -> Option<SymbolId> {
        self.0.shift_remove(key.into().as_bytes())
    }

    pub fn get_full<'a>(
        &self,
        key: impl Into<JsStr<'a>>,
    ) -> Option<(usize, &EscapedName, &SymbolId)> {
        self.0.get_full(key.into().as_bytes())
    }

    pub fn get_index(&self, index: usize) -> Option<(&EscapedName, &SymbolId)> {
        self.0.get_index(index)
    }

    pub fn insert(&mut self, key: EscapedName, value: SymbolId) -> Option<SymbolId> {
        self.0.insert(key, value)
    }

    pub fn entry(&mut self, key: EscapedName) -> map::Entry<'_, EscapedName, SymbolId> {
        self.0.entry(key)
    }

    pub fn iter(&self) -> map::Iter<'_, EscapedName, SymbolId> {
        self.0.iter()
    }
    pub fn iter_mut(&mut self) -> map::IterMut<'_, EscapedName, SymbolId> {
        self.0.iter_mut()
    }
    pub fn keys(&self) -> map::Keys<'_, EscapedName, SymbolId> {
        self.0.keys()
    }
    pub fn values(&self) -> map::Values<'_, EscapedName, SymbolId> {
        self.0.values()
    }
    pub fn values_mut(&mut self) -> map::ValuesMut<'_, EscapedName, SymbolId> {
        self.0.values_mut()
    }
}

impl FromIterator<(EscapedName, SymbolId)> for SymbolTable {
    fn from_iter<T: IntoIterator<Item = (EscapedName, SymbolId)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl Extend<(EscapedName, SymbolId)> for SymbolTable {
    fn extend<T: IntoIterator<Item = (EscapedName, SymbolId)>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}

impl IntoIterator for SymbolTable {
    type Item = (EscapedName, SymbolId);
    type IntoIter = map::IntoIter<EscapedName, SymbolId>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
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
