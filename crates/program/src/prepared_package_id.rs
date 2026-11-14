//! Immutable package values shared by resolution facts and ordered graph edges.
use super::*;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) struct PreparedPackageId {
    value: PackageId,
    display: OnceLock<Arc<JsString>>,
}
impl PreparedPackageId {
    pub(crate) fn new(value: PackageId) -> Self {
        Self {
            value,
            display: OnceLock::new(),
        }
    }
    pub(crate) fn value(&self) -> &PackageId {
        &self.value
    }
    pub(crate) fn into_value(owner: Arc<Self>) -> PackageId {
        match Arc::try_unwrap(owner) {
            Ok(owner) => owner.value,
            Err(owner) => owner.value.clone(),
        }
    }
    pub(crate) fn display(&self) -> Arc<JsString> {
        Arc::clone(
            self.display
                .get_or_init(|| Arc::new(self.value.display_text())),
        )
    }
}
impl std::ops::Deref for PreparedPackageId {
    type Target = PackageId;
    fn deref(&self) -> &PackageId {
        &self.value
    }
}
impl std::fmt::Debug for PreparedPackageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}
impl PartialEq for PreparedPackageId {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}
impl Eq for PreparedPackageId {}
impl PartialOrd for PreparedPackageId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PreparedPackageId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.value.cmp(&other.value)
    }
}
impl Hash for PreparedPackageId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}
impl std::borrow::Borrow<PackageId> for Arc<PreparedPackageId> {
    fn borrow(&self) -> &PackageId {
        &self.value
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedPackageRef {
    pub(crate) id: Arc<PreparedPackageId>,
    pub(crate) index: usize,
}
impl PreparedPackageRef {
    pub(crate) fn value(&self) -> &PackageId {
        self.id.value()
    }
}

pub(crate) struct PreparedPackageIds {
    shards: Vec<Mutex<rustc_hash::FxHashMap<Arc<PreparedPackageId>, usize>>>,
    count: std::sync::atomic::AtomicUsize,
}
impl Default for PreparedPackageIds {
    fn default() -> Self {
        Self {
            shards: (0..64)
                .map(|_| Mutex::new(rustc_hash::FxHashMap::default()))
                .collect(),
            count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}
impl PreparedPackageIds {
    pub(crate) fn intern(&self, id: PackageId) -> PreparedPackageRef {
        let mut hash = rustc_hash::FxHasher::default();
        id.hash(&mut hash);
        let mut shard = self.shards[(hash.finish() >> 58) as usize]
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((id, &index)) = shard.get_key_value(&id) {
            return PreparedPackageRef {
                id: Arc::clone(id),
                index,
            };
        }
        let index = self
            .count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = Arc::new(PreparedPackageId::new(id));
        shard.insert(Arc::clone(&id), index);
        PreparedPackageRef { id, index }
    }
    pub(crate) fn len(&self) -> usize {
        self.count.load(std::sync::atomic::Ordering::Relaxed)
    }
}
