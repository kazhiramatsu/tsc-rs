//! Freeze each reference's member identities at its original eager boundary.
//! Resolved decisions and immutable syntax proofs can be reused across visits.
use crate::state::{CheckerState, MembersId, SymbolIds};
use rustc_hash::FxHashMap;
use tsc_binder::SymbolId;
use tsc_types::{MapperId, SymbolFlags, TypeId};

const MAX_ENTRIES: usize = 1 << 15;
const MAX_BYTES: usize = 16 << 20;

struct Entry {
    table: MembersId,
    len: usize,
    maps_this_only: bool,
    /// Sorted membership as of the last visit, shared with references frozen then.
    snapshot: SymbolIds,
    /// Decisions to revisit, in their original declaration order.
    pending: Box<[SymbolId]>,
}

impl Entry {
    fn heap_bytes(&self) -> usize {
        self.snapshot.heap_bytes() + std::mem::size_of_val(self.pending.as_ref())
    }
}

#[derive(Default)]
pub(crate) struct UnaffectedMembers {
    epoch: u64,
    entries: FxHashMap<TypeId, Entry>,
    payload_bytes: usize,
    // Public capacity can fall after deletion without releasing buckets.
    // Keep a conservative backing high-water mark until the map is dropped.
    map_buckets: usize,
}

impl UnaffectedMembers {
    // Bound the map's backing, including spare buckets/control tail and
    // alignment, rather than just charging occupied entries. This also
    // accounts for capacity retained after removing a stale entry.
    fn map_bytes(buckets: usize) -> usize {
        if buckets == 0 {
            0
        } else {
            buckets * (std::mem::size_of::<(TypeId, Entry)>() + 1) + 32
        }
    }

    fn insertion_buckets(&self) -> usize {
        if self.map_buckets == 0 {
            4
        } else if self.entries.len() == self.entries.capacity() {
            self.map_buckets * 2
        } else {
            self.map_buckets
        }
    }

    fn admits(&self, len: usize) -> bool {
        if self.entries.len() >= MAX_ENTRIES {
            return false;
        }
        let buckets = self.insertion_buckets();
        // Keep the original admission check before walking any properties.
        // A pending positive occurs in both lists, so check the actual
        // retained bytes before publishing the entry too.
        len.checked_mul(std::mem::size_of::<SymbolId>())
            .and_then(|n| n.checked_add(self.payload_bytes))
            .and_then(|n| n.checked_add(Self::map_bytes(buckets)))
            .is_some_and(|bytes| bytes <= MAX_BYTES)
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        Self::map_bytes(self.map_buckets) + self.payload_bytes
    }

    // Cold ownership totals have no observable traversal order.
    #[allow(clippy::iter_over_hash_type)]
    pub(crate) fn add_memory_usage(&self, usage: &mut tsc_diagnostics::MemoryUsage) {
        usage.hash_map("memo: unaffected members", &self.entries);
        for entry in self.entries.values() {
            let bytes = entry.snapshot.heap_bytes_once(usage);
            if bytes != 0 {
                usage.add(
                    "memo: unaffected member lists",
                    entry.snapshot.len(),
                    bytes,
                    bytes,
                );
            }
            let list = &entry.pending;
            if !list.is_empty() && usage.claim(list.as_ptr() as usize) {
                let bytes = std::mem::size_of_val(list.as_ref());
                usage.add("memo: unaffected member lists", list.len(), bytes, bytes);
            }
        }
    }
}

impl CheckerState<'_> {
    fn unaffected_decision_settled(&self, symbol: SymbolId) -> bool {
        self.links
            .symbol(symbol)
            .type_of_symbol
            .resolved()
            .is_some()
            && (!self
                .symbol_flags(symbol)
                .intersects(SymbolFlags::SET_ACCESSOR)
                || self
                    .links
                    .symbol_cold()
                    .write_type
                    .resolved(symbol)
                    .is_some())
    }

    /// The original walk, also used when the bounded memo cannot admit a key.
    fn fresh_unaffected_members(&mut self, properties: &[SymbolId], mapper: MapperId) -> SymbolIds {
        let mut result = Vec::new();
        for &symbol in properties {
            if self.symbol_unaffected_by_instantiation(symbol, mapper) {
                if result.is_empty() {
                    result.reserve(properties.len());
                }
                result.push(symbol);
            }
        }
        result.sort_unstable();
        result.dedup();
        result.into()
    }

    pub(crate) fn freeze_unaffected_members(
        &mut self,
        target: TypeId,
        table: MembersId,
        mapper: MapperId,
        properties: &[SymbolId],
    ) -> SymbolIds {
        if properties.is_empty() {
            return SymbolIds::new();
        }
        let epoch = self.links.members_reset_epoch;
        if self.unaffected_members.epoch != epoch {
            // Drop backing too, so the byte cap includes all retained memory.
            self.unaffected_members = UnaffectedMembers {
                epoch,
                ..Default::default()
            };
        }
        let maps_this_only = self.maps_this_only(mapper);
        let matches = match self.unaffected_members.entries.get(&target) {
            Some(entry)
                if entry.table == table
                    && entry.len == properties.len()
                    && entry.maps_this_only == maps_this_only =>
            {
                if entry.pending.is_empty() {
                    return entry.snapshot.clone();
                }
                true
            }
            _ => false,
        };
        // The predicate only reads already resolved types and immutable syntax;
        // it cannot reenter member preparation or allocate a semantic identity.
        let previous = self.unaffected_members.entries.remove(&target);
        if let Some(entry) = &previous {
            self.unaffected_members.payload_bytes -= entry.heap_bytes();
        }
        if !self.unaffected_members.admits(properties.len()) {
            return self.fresh_unaffected_members(properties, mapper);
        }
        let (mut snapshot, mut pending) = match previous.filter(|_| matches) {
            Some(entry) => (entry.snapshot, entry.pending.into_vec()),
            None => (SymbolIds::new(), properties.to_vec()),
        };
        let mut added = Vec::new();
        let mut removed = Vec::new();
        pending.retain(|&symbol| {
            let was_unaffected = snapshot.binary_search(&symbol).is_ok();
            // A positive result with an unresolved read or write type can
            // only have come from is_thisless. Published binder declarations
            // are immutable; transient symbols can acquire new declarations.
            let syntax_proven = was_unaffected
                && maps_this_only
                && symbol.index() & tsc_types::TRANSIENT_SYMBOL_BIT == 0;
            let unaffected =
                syntax_proven || self.symbol_unaffected_by_instantiation(symbol, mapper);
            if unaffected != was_unaffected {
                if unaffected {
                    added.push(symbol);
                } else {
                    removed.push(symbol);
                }
            }
            !self.unaffected_decision_settled(symbol)
        });
        if !added.is_empty() || !removed.is_empty() {
            let mut next = snapshot.to_vec();
            if !removed.is_empty() {
                removed.sort_unstable();
                next.retain(|symbol| removed.binary_search(symbol).is_err());
            }
            next.append(&mut added);
            next.sort_unstable();
            next.dedup();
            snapshot = next.into();
        }
        let entry = Entry {
            table,
            len: properties.len(),
            maps_this_only,
            snapshot: snapshot.clone(),
            pending: pending.into_boxed_slice(),
        };
        let bytes = entry.heap_bytes();
        let buckets = self.unaffected_members.insertion_buckets();
        if self.unaffected_members.payload_bytes + bytes + UnaffectedMembers::map_bytes(buckets)
            <= MAX_BYTES
        {
            self.unaffected_members.payload_bytes += bytes;
            self.unaffected_members.map_buckets = buckets;
            self.unaffected_members.entries.insert(target, entry);
        }
        snapshot
    }
}

#[cfg(test)]
#[path = "../tests/unit/unaffected_members/tests.rs"]
mod tests;
