//! Cold ownership accounting. Requested layouts are not allocator usable
//! sizes, resident bytes, or physical footprint. Collection scratch is excluded.
use std::alloc::Layout;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::BuildHasher;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryRow {
    pub count: usize,
    pub initialized: usize,
    pub requested: usize,
}

#[derive(Default)]
pub struct MemoryUsage {
    seen: HashSet<usize>,
    seen_containers: HashSet<usize>,
    rows: BTreeMap<&'static str, MemoryRow>,
    pub available_slots: usize,
    pub consumed_slots: usize,
    pub empty_slots: usize,
    pub poisoned_slots: usize,
    pub source_text_bytes: usize,
}

impl MemoryUsage {
    /// Allocation identity, never text/content equality. The owner must stay
    /// alive throughout collection. Empty Vecs have no allocation to claim.
    pub fn claim(&mut self, pointer: usize) -> bool {
        self.seen.insert(pointer)
    }

    pub fn add(&mut self, name: &'static str, count: usize, initialized: usize, requested: usize) {
        let row = self.rows.entry(name).or_default();
        row.count += count;
        row.initialized += initialized;
        row.requested += requested;
    }

    pub fn row(&self, name: &str) -> MemoryRow {
        self.rows.get(name).copied().unwrap_or_default()
    }

    fn arc_allocation<T: ?Sized>(value: &Arc<T>) -> (usize, usize) {
        let (layout, offset) = Layout::new::<[usize; 2]>()
            .extend(Layout::for_value(value.as_ref()))
            .expect("Arc layout");
        // In particular, a zero-sized payload's pointer is one past its
        // allocation and can coincide with a neighboring buffer's start.
        let base = Arc::as_ptr(value).cast::<()>() as usize - offset;
        (base, layout.pad_to_align().size())
    }

    pub fn claim_arc<T: ?Sized>(&mut self, value: &Arc<T>) -> bool {
        self.claim(Self::arc_allocation(value).0)
    }

    /// The Arc counters and alignment padding belong to the allocation;
    /// Arc handles embedded in another counted owner do not add bytes.
    pub fn arc<T: ?Sized>(&mut self, name: &'static str, value: &Arc<T>) -> bool {
        let (base, bytes) = Self::arc_allocation(value);
        if !self.claim(base) {
            return false;
        }
        self.add(name, 1, bytes, bytes);
        true
    }

    pub fn vec<T>(&mut self, name: &'static str, value: &Vec<T>) -> bool {
        if value.capacity() == 0
            || std::mem::size_of::<T>() == 0
            || !self.claim(value.as_ptr() as usize)
        {
            return false;
        }
        self.add(
            name,
            value.len(),
            std::mem::size_of_val(value.as_slice()),
            value.capacity() * std::mem::size_of::<T>(),
        );
        true
    }

    pub fn string(&mut self, name: &'static str, value: &String) {
        if value.capacity() != 0 && self.claim(value.as_ptr() as usize) {
            self.add(name, 1, value.len(), value.capacity());
        }
    }

    pub fn js_string(&mut self, name: &'static str, value: &crate::JsString) {
        let capacity = value.heap_bytes();
        if capacity != 0 && self.claim(value.as_bytes().as_ptr() as usize) {
            self.add(name, 1, value.as_bytes().len(), capacity);
        }
    }

    pub fn diagnostics(&mut self, value: &Vec<crate::Diagnostic>) {
        if self.vec("diagnostic vectors", value) {
            for diagnostic in value {
                self.diagnostic(diagnostic);
            }
        }
    }

    pub fn diagnostic(&mut self, value: &crate::Diagnostic) {
        if let Some(name) = &value.file_name {
            self.js_string("strings: diagnostic owners", name);
        }
        if let Some(source) = &value.source {
            self.string("strings: diagnostic owners", source);
        }
        self.message_chain(&value.message);
        self.related(&value.related);
    }

    fn message_chain(&mut self, value: &crate::MessageChain) {
        self.js_string("strings: diagnostic owners", &value.text);
        if self.vec("diagnostic vectors", &value.args) {
            for arg in &value.args {
                self.js_string("strings: diagnostic owners", arg);
            }
        }
        if self.vec("diagnostic vectors", &value.next) {
            for next in &value.next {
                self.message_chain(next);
            }
        }
        self.related(&value.related);
        if let Some(crate::Repopulate::ModuleNotFound {
            module_reference,
            package_name,
            ..
        }) = &value.repopulate
        {
            self.js_string("strings: diagnostic owners", module_reference);
            if let Some(name) = package_name {
                self.js_string("strings: diagnostic owners", name);
            }
        }
    }

    fn related(&mut self, values: &Vec<crate::RelatedInfo>) {
        if self.vec("diagnostic vectors", values) {
            for value in values {
                if let Some(name) = &value.file_name {
                    self.js_string("strings: diagnostic owners", name);
                }
                self.message_chain(&value.message);
            }
        }
    }

    /// std/hashbrown's public capacity is usable entry room, not raw
    /// buckets. This is a layout estimate including control bytes, with
    /// 16 bytes for the control-group tail; implementation padding can vary.
    pub fn hash_table<K, V>(&mut self, name: &'static str, len: usize, capacity: usize) {
        if capacity == 0 {
            return;
        }
        let buckets = (capacity + 1).next_power_of_two();
        let controls = buckets + 16;
        self.add(
            name,
            len,
            len * std::mem::size_of::<(K, V)>() + controls,
            buckets * std::mem::size_of::<(K, V)>() + controls,
        );
    }

    pub fn hash_map<K, V, S: BuildHasher>(&mut self, name: &'static str, value: &HashMap<K, V, S>) {
        if self.seen_containers.insert(value as *const _ as usize) {
            self.hash_table::<K, V>(name, value.len(), value.capacity());
        }
    }

    pub fn hash_set<K, S: BuildHasher>(&mut self, name: &'static str, value: &HashSet<K, S>) {
        if self.seen_containers.insert(value as *const _ as usize) {
            self.hash_table::<K, ()>(name, value.len(), value.capacity());
        }
    }

    /// BTreeMap exposes no allocated-node capacity. This estimate assumes
    /// eight occupied slots per eleven-slot node, plus node links. It is
    /// deliberately labelled an estimate, never a measured allocation sum.
    pub fn btree<K, V>(&mut self, name: &'static str, value: &BTreeMap<K, V>) {
        if value.is_empty() || !self.seen_containers.insert(value as *const _ as usize) {
            return;
        }
        let nodes = value.len().div_ceil(8);
        self.add(
            name,
            value.len(),
            value.len() * std::mem::size_of::<(K, V)>(),
            nodes * (11 * std::mem::size_of::<(K, V)>() + 14 * std::mem::size_of::<usize>()),
        );
    }

    pub fn total(&self) -> MemoryRow {
        self.rows
            .values()
            .fold(MemoryRow::default(), |mut sum, row| {
                sum.count += row.count;
                sum.initialized += row.initialized;
                sum.requested += row.requested;
                sum
            })
    }

    pub fn print(&self, label: &str) {
        for (name, row) in &self.rows {
            eprintln!("[memory] {label} {name}: count={} initialized={} B ({:.3} MiB) requested={} B ({:.3} MiB)",
                row.count, row.initialized, row.initialized as f64 / 1048576.0,
                row.requested, row.requested as f64 / 1048576.0);
        }
        let total = self.total();
        eprintln!(
            "[memory] {label} added owners total: initialized={} B requested={} B ({:.3} MiB)",
            total.initialized,
            total.requested,
            total.requested as f64 / 1048576.0
        );
        eprintln!(
            "[memory] {label} preparsed slots: available={} consumed={} empty={} poisoned={}",
            self.available_slots, self.consumed_slots, self.empty_slots, self.poisoned_slots
        );
    }
}
