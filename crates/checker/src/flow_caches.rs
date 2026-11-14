//! Exact file-local flow identities, packed without changing cache lifetime.

use std::hash::{Hash, Hasher};

use hashbrown::HashTable;
use rustc_hash::{FxHashMap, FxHasher};
use tsc_binder::flow::FlowId;

fn map_bytes<K, V>(map: &FxHashMap<K, V>) -> usize {
    map.capacity() * (std::mem::size_of::<(K, V)>() + 1) * 8 / 7
}

/// Outer loop identities use the full 32-bit FlowId and a 32-bit file.
/// Files outside that budget retain their exact usize identity.
#[derive(Debug)]
pub(crate) struct FlowIdentityCache<V> {
    packed: FxHashMap<u64, V>,
    wide: FxHashMap<(usize, FlowId), V>,
}

impl<V> Default for FlowIdentityCache<V> {
    fn default() -> Self {
        Self {
            packed: FxHashMap::default(),
            wide: FxHashMap::default(),
        }
    }
}

impl<V> FlowIdentityCache<V> {
    #[inline]
    fn key((file, flow): (usize, FlowId)) -> Option<u64> {
        let file = u32::try_from(file).ok()?;
        Some((u64::from(file) << 32) | u64::from(flow.index()))
    }

    #[inline]
    pub(crate) fn get(&self, key: &(usize, FlowId)) -> Option<&V> {
        match Self::key(*key) {
            Some(key) => self.packed.get(&key),
            None => self.wide.get(key),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.packed.len() + self.wide.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &V> {
        self.packed.values().chain(self.wide.values())
    }

    pub(crate) fn memory_bytes(&self) -> usize {
        map_bytes(&self.packed) + map_bytes(&self.wide)
    }
}

impl<V: Default> FlowIdentityCache<V> {
    #[inline]
    pub(crate) fn get_or_default(&mut self, key: (usize, FlowId)) -> &mut V {
        match Self::key(key) {
            Some(key) => self.packed.entry(key).or_default(),
            None => self.wide.entry(key).or_default(),
        }
    }
}

/// A shared-node boolean answer occupies one u64 bucket. The low bit is
/// the answer, then 32 FlowId bits and 31 file bits. Larger files use the
/// exact original key. Lookup chooses its table from the input file alone,
/// so a normal miss never probes an additional fallback table.
#[derive(Debug, Default)]
pub(crate) struct FlowNodeCache {
    packed: HashTable<u64>,
    wide: FxHashMap<(usize, FlowId), bool>,
}

impl FlowNodeCache {
    #[inline]
    fn key((file, flow): (usize, FlowId)) -> Option<u64> {
        if file > (u32::MAX >> 1) as usize {
            return None;
        }
        Some(((file as u64) << 33) | (u64::from(flow.index()) << 1))
    }

    #[inline]
    fn hash(key: u64) -> u64 {
        let mut hasher = FxHasher::default();
        (key & !1).hash(&mut hasher);
        hasher.finish()
    }

    #[inline]
    pub(crate) fn get(&self, key: &(usize, FlowId)) -> Option<bool> {
        match Self::key(*key) {
            Some(key) => self
                .packed
                .find(Self::hash(key), |slot| slot & !1 == key)
                .map(|slot| slot & 1 != 0),
            None => self.wide.get(key).copied(),
        }
    }

    #[inline]
    pub(crate) fn insert(&mut self, key: (usize, FlowId), answer: bool) -> Option<bool> {
        let Some(key) = Self::key(key) else {
            return self.wide.insert(key, answer);
        };
        let hash = Self::hash(key);
        let slot = key | u64::from(answer);
        if let Some(old) = self.packed.find_mut(hash, |old| old & !1 == key) {
            let previous = *old & 1 != 0;
            *old = slot;
            return Some(previous);
        }
        self.packed
            .insert_unique(hash, slot, |slot| Self::hash(*slot));
        None
    }

    pub(crate) fn len(&self) -> usize {
        self.packed.len() + self.wide.len()
    }

    pub(crate) fn memory_bytes(&self) -> usize {
        self.packed.capacity() * (std::mem::size_of::<u64>() + 1) * 8 / 7 + map_bytes(&self.wide)
    }
}

/// Nodes whose immutable ancestor chain has no conditional or mapped
/// flow context before the statement/JSDoc boundary. This syntax fact
/// survives speculative rollback; it does not cache an annotation type.
#[derive(Debug, Default)]
pub(crate) struct NoFlowContextNodes {
    pages: Vec<Option<Box<[u64; 512]>>>,
}

impl NoFlowContextNodes {
    #[inline]
    pub(crate) fn contains(&self, node: tsc_syntax::NodeId) -> bool {
        let index = node.index() as usize;
        let Some(Some(page)) = self.pages.get(index >> 15) else {
            return false;
        };
        page[(index >> 6) & 511] & (1u64 << (index & 63)) != 0
    }

    #[inline]
    pub(crate) fn insert(&mut self, node: tsc_syntax::NodeId) {
        let index = node.index() as usize;
        let page_index = index >> 15;
        if page_index >= self.pages.len() {
            self.pages.resize_with(page_index + 1, || None);
        }
        let page = self.pages[page_index].get_or_insert_with(Self::new_page);
        page[(index >> 6) & 511] |= 1u64 << (index & 63);
    }

    // Keep allocation and zero initialization out of ordinary inserts.
    #[cold]
    #[inline(never)]
    fn new_page() -> Box<[u64; 512]> {
        Box::new([0; 512])
    }

    /// The one-shot worker has published its result and will not query again.
    #[cold]
    pub(crate) fn release_finished_buffers(&mut self) {
        self.pages = Vec::new();
    }

    pub(crate) fn memory_usage(&self) -> (usize, usize) {
        let entries = self
            .pages
            .iter()
            .flatten()
            .flat_map(|page| page.iter())
            .map(|word| word.count_ones() as usize)
            .sum();
        let bytes = self.pages.capacity() * std::mem::size_of::<Option<Box<[u64; 512]>>>()
            + self.pages.iter().flatten().count() * 4096;
        (entries, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_bits_keep_sparse_pages_and_word_boundaries_independent() {
        use tsc_syntax::NodeId;
        let mut nodes = NoFlowContextNodes::default();
        for index in [0, 63, 64, 32767, 32768, u32::MAX - 1] {
            let node = NodeId::new(index);
            assert!(!nodes.contains(node));
            nodes.insert(node);
            nodes.insert(node);
            assert!(nodes.contains(node));
        }
        for index in [1, 62, 65, 32766, 32769, 65536, u32::MAX - 2] {
            assert!(!nodes.contains(NodeId::new(index)));
        }
        assert_eq!(nodes.memory_usage().0, 6);
        assert_eq!(nodes.pages.iter().flatten().count(), 3);
        assert!(!NoFlowContextNodes::default().contains(NodeId::new(64)));
    }

    #[test]
    fn boolean_slots_round_trip_both_answers_at_every_bit_bound() {
        let mut cache = FlowNodeCache::default();
        for file in [0, 1, (1usize << 31) - 1] {
            for flow in [0, 1, (1u32 << 31) - 1, 1u32 << 31, u32::MAX - 1] {
                let key = (file, FlowId::new(flow));
                let packed = FlowNodeCache::key(key).unwrap();
                assert_eq!(packed >> 33, file as u64);
                assert_eq!(((packed >> 1) & u64::from(u32::MAX)) as u32, flow);
                assert_eq!(cache.get(&key), None);
                assert_eq!(cache.insert(key, false), None);
                assert_eq!(cache.get(&key), Some(false));
                assert_eq!(cache.insert(key, true), Some(false));
                assert_eq!(cache.get(&key), Some(true));
                assert_eq!(cache.insert(key, false), Some(true));
            }
        }
        assert_eq!(cache.len(), 15);
        assert_eq!(cache.wide.len(), 0);
    }

    #[test]
    fn boolean_wide_files_preserve_exact_identity_and_replacements() {
        let mut cache = FlowNodeCache::default();
        let mut files = vec![1usize << 31, u32::MAX as usize, usize::MAX];
        files.sort_unstable();
        files.dedup();
        for &file in &files {
            let key = (file, FlowId::new(u32::MAX - 1));
            assert_eq!(FlowNodeCache::key(key), None);
            assert_eq!(cache.get(&key), None);
            assert_eq!(cache.insert(key, true), None);
            assert_eq!(cache.get(&key), Some(true));
            assert_eq!(cache.insert(key, false), Some(true));
            assert_eq!(cache.get(&key), Some(false));
        }
        assert_eq!(cache.len(), files.len());
        assert_eq!(cache.packed.len(), 0);
        let low = (0, FlowId::new(u32::MAX - 1));
        assert_eq!(cache.get(&low), None);
        cache.insert(low, true);
        assert_eq!(cache.len(), files.len() + 1);
    }

    #[test]
    fn loop_identities_keep_full_flow_bits_and_exact_wide_files() {
        let mut cache = FlowIdentityCache::<FxHashMap<String, u32>>::default();
        assert!(cache.is_empty());
        for file in [0, u32::MAX as usize] {
            for flow in [0, u32::MAX - 1] {
                let key = (file, FlowId::new(flow));
                let packed = FlowIdentityCache::<()>::key(key).unwrap();
                assert_eq!(packed >> 32, file as u64);
                assert_eq!(packed as u32, flow);
                cache.get_or_default(key).insert("reference".into(), flow);
                assert_eq!(cache.get(&key).unwrap().get("reference"), Some(&flow));
                assert_eq!(
                    cache.get_or_default(key).insert("reference".into(), 7),
                    Some(flow)
                );
            }
        }
        if let Some(file) = (u32::MAX as usize).checked_add(1) {
            let key = (file, FlowId::new(u32::MAX - 1));
            assert_eq!(FlowIdentityCache::<()>::key(key), None);
            cache.get_or_default(key).insert("reference".into(), 42);
            assert_eq!(cache.get(&key).unwrap().get("reference"), Some(&42));
            assert_eq!(cache.wide.len(), 1);
        }
        assert_eq!(
            cache.values().map(FxHashMap::len).sum::<usize>(),
            cache.len()
        );
        assert!(cache.memory_bytes() > 0);
    }
}
