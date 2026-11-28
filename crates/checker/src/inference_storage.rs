//! Physical storage for inference objects with permanent logical identities.

use std::ops::{Index, IndexMut};

use rustc_hash::FxHashSet;
use tsc_types::{InferencePriority, InlineList, MapperId, TypeId};

use crate::inference::{InferenceContextId, InferenceInfoId, InferenceInfos};
use crate::instantiate::{DeferredMapperTargets, TypeMapper};
use crate::state::CheckerState;

const HAS_CONTEXT: u64 = 1;
const RETAINED: u64 = 2;

pub(crate) trait InferenceHandle: Copy {
    fn from_parts(index: u32, slot: u32) -> Self;
    fn position(self) -> u32;
    fn slot(self) -> u32;
}

pub(crate) trait InferenceRecord {
    type Id: InferenceHandle;
    fn generation(&self) -> u32;
    fn set_generation(&mut self, generation: u32);
}

macro_rules! inference_id {
    ($(#[$attribute:meta])* $name:ident) => {
        $(#[$attribute])*
        #[derive(Clone, Copy)]
        #[repr(C)]
        pub struct $name {
            logical: ::std::num::NonZeroU32,
            slot: u32,
        }
        impl $name {
            pub const fn new(index: u32) -> Self {
                match Self::checked_new(index) {
                    Some(id) => id,
                    None => panic!("inference identity index out of range"),
                }
            }
            pub const fn checked_new(index: u32) -> Option<Self> {
                match ::std::num::NonZeroU32::new(index.wrapping_add(1)) {
                    Some(logical) => Some(Self { logical, slot: index }),
                    None => None,
                }
            }
            #[inline(always)]
            pub const fn index(self) -> u32 { self.logical.get() - 1 }
            #[inline(always)]
            pub const fn position(self) -> u32 { self.logical.get() }
            pub const fn from_position(position: u32) -> Option<Self> {
                match ::std::num::NonZeroU32::new(position) {
                    Some(logical) => Some(Self { logical, slot: position - 1 }),
                    None => None,
                }
            }
        }
        impl crate::inference_storage::InferenceHandle for $name {
            #[inline(always)]
            fn from_parts(index: u32, slot: u32) -> Self {
                let mut id = Self::new(index);
                id.slot = slot;
                id
            }
            #[inline(always)]
            fn position(self) -> u32 { self.logical.get() }
            #[inline(always)]
            fn slot(self) -> u32 { self.slot }
        }
        impl Default for $name { fn default() -> Self { Self::new(0) } }
        impl PartialEq for $name {
            #[inline(always)]
            fn eq(&self, other: &Self) -> bool { self.logical == other.logical }
        }
        impl Eq for $name {}
        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<::std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for $name {
            fn cmp(&self, other: &Self) -> ::std::cmp::Ordering {
                self.logical.cmp(&other.logical)
            }
        }
        impl ::std::hash::Hash for $name {
            fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) {
                ::std::hash::Hash::hash(&self.index(), state);
            }
        }
        impl ::std::fmt::Debug for $name {
            fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                formatter.debug_tuple(stringify!($name)).field(&self.index()).finish()
            }
        }
        const _: () = {
            assert!(::std::mem::size_of::<$name>() == 8);
            assert!(::std::mem::align_of::<$name>() == 4);
            assert!(::std::mem::size_of::<Option<$name>>() == 8);
        };
    };
}
pub(crate) use inference_id;

impl InferenceRecord for crate::inference::InferenceContext {
    type Id = InferenceContextId;
    #[inline(always)]
    fn generation(&self) -> u32 {
        self.generation
    }
    #[inline(always)]
    fn set_generation(&mut self, generation: u32) {
        self.generation = generation;
    }
}
impl InferenceRecord for crate::inference::InferenceInfo {
    type Id = InferenceInfoId;
    #[inline(always)]
    fn generation(&self) -> u32 {
        self.generation
    }
    #[inline(always)]
    fn set_generation(&mut self, generation: u32) {
        self.generation = generation;
    }
}

/// Logical identities never repeat; the handle also carries its checked slot.
#[derive(Debug)]
pub(crate) struct InferencePool<T> {
    records: Vec<T>,
    next: u32,
    free: Vec<u32>,
}
impl<T> Default for InferencePool<T> {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            next: 0,
            free: Vec::new(),
        }
    }
}
impl<T: InferenceRecord> InferencePool<T> {
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.next as usize
    }
    pub(crate) fn reserve(&mut self, additional: usize) {
        self.records.reserve(additional);
    }
    #[inline(always)]
    pub(crate) fn next_id(&self) -> T::Id {
        let slot =
            self.free.last().copied().unwrap_or_else(|| {
                u32::try_from(self.records.len()).expect("inference slots fit u32")
            });
        T::Id::from_parts(self.next, slot)
    }
    #[inline(always)]
    pub(crate) fn push(&mut self, mut record: T) -> T::Id {
        let id = self.next_id();
        record.set_generation(id.position());
        if let Some(slot) = self.free.pop() {
            self.records[slot as usize] = record;
        } else {
            self.records.push(record);
        }
        self.next += 1;
        id
    }
    #[inline]
    pub(crate) fn retire(&mut self, id: T::Id) -> &mut T {
        let slot = id.slot();
        let record = &mut self.records[slot as usize];
        assert!(
            record.generation() == id.position(),
            "retired inference identity"
        );
        record.set_generation(0);
        self.free.push(slot);
        record
    }
    #[inline]
    pub(crate) fn last_free_mut(&mut self) -> Option<&mut T> {
        let slot = *self.free.last()?;
        Some(&mut self.records[slot as usize])
    }
    pub(crate) fn records(&self) -> &Vec<T> {
        &self.records
    }
    pub(crate) fn add_memory_usage(&self, usage: &mut tsc_diagnostics::MemoryUsage) {
        usage.vec("inference free slots", &self.free);
    }
    pub(crate) fn print_reserved(&self, label: &str, name: &str) -> usize {
        crate::memory::print_reserved_vec(label, name, &self.records)
            + crate::memory::print_reserved_vec(label, "inference free slots", &self.free)
    }
}
impl<T: InferenceRecord> Index<T::Id> for InferencePool<T> {
    type Output = T;
    #[inline(always)]
    fn index(&self, id: T::Id) -> &T {
        let record = &self.records[id.slot() as usize];
        assert!(
            record.generation() == id.position(),
            "retired inference identity"
        );
        record
    }
}
impl<T: InferenceRecord> IndexMut<T::Id> for InferencePool<T> {
    #[inline(always)]
    fn index_mut(&mut self, id: T::Id) -> &mut T {
        let record = &mut self.records[id.slot() as usize];
        assert!(
            record.generation() == id.position(),
            "retired inference identity"
        );
        record
    }
}

/// Reusing an oversized buffer must not enlarge a context that escapes.
#[inline(always)]
pub(crate) fn exact_buffer<T: Copy, const N: usize>(
    buffer: InlineList<T, N>,
    count: usize,
) -> InlineList<T, N> {
    debug_assert!(buffer.is_empty());
    if count <= N {
        InlineList::new()
    } else {
        exact_heap_buffer(buffer, count)
    }
}

#[inline(never)]
fn exact_heap_buffer<T: Copy, const N: usize>(
    buffer: InlineList<T, N>,
    count: usize,
) -> InlineList<T, N> {
    if buffer.capacity() == count {
        buffer
    } else {
        drop(buffer);
        InlineList::with_capacity(count)
    }
}

#[derive(Default)]
pub(crate) struct InferenceLifetimes {
    contexts: Vec<InferenceContextId>,
    pub(crate) last_claimed_info: usize,
    todo: Vec<(MapperId, bool)>,
    visited: FxHashSet<MapperId>,
}

impl InferenceLifetimes {
    pub(crate) fn created_context(&mut self, id: InferenceContextId) {
        self.contexts.push(id);
    }

    pub(crate) fn add_memory_usage(&self, usage: &mut tsc_diagnostics::MemoryUsage) {
        usage.vec("inference outstanding contexts", &self.contexts);
        usage.vec("inference retention scratch", &self.todo);
        usage.hash_set("inference retention scratch", &self.visited);
    }
}

impl CheckerState<'_> {
    #[inline]
    pub(crate) fn reset_inference_candidates(
        &mut self,
        id: InferenceInfoId,
        priority: InferencePriority,
    ) {
        let info = &mut self.inference_info_arena[id];
        info.candidates = None;
        info.contra_candidates = None;
        info.top_level = true;
        info.priority = Some(priority);
    }

    #[inline]
    pub(crate) fn push_inference_candidate(
        &mut self,
        id: InferenceInfoId,
        candidate: TypeId,
        contravariant: bool,
    ) {
        let info = &mut self.inference_info_arena[id];
        let candidates = if contravariant {
            &mut info.contra_candidates
        } else {
            &mut info.candidates
        };
        candidates
            .get_or_insert_with(InlineList::new)
            .push(candidate);
    }

    /// The inference lifetime flags a new `mapper` is stored with (in its
    /// record, see `MapperStore`): whether it reaches an inference context.
    #[inline(always)]
    pub(crate) fn mapper_lifetime_flags(&self, mapper: TypeMapper) -> u64 {
        let has_context = match mapper {
            TypeMapper::Deferred(
                DeferredMapperTargets::InferenceFixing(_)
                | DeferredMapperTargets::InferenceNonFixing(_),
            ) => true,
            TypeMapper::Composite { mapper1, mapper2 }
            | TypeMapper::Merged { mapper1, mapper2 } => {
                (self.mappers.lifetime_flags(mapper1) | self.mappers.lifetime_flags(mapper2))
                    & HAS_CONTEXT
                    != 0
            }
            _ => false,
        };
        if has_context {
            HAS_CONTEXT
        } else {
            0
        }
    }

    /// Stored type, signature and link mappers outlive their creating call.
    #[inline]
    pub(crate) fn retain_inference_mapper(&mut self, mapper: Option<MapperId>) {
        if let Some(mapper) = mapper {
            if self.mappers.lifetime_flags(mapper) == HAS_CONTEXT {
                self.walk_inference_mappers(mapper, None);
            }
        }
    }

    /// An older active context can hold a younger context past its creator.
    #[inline]
    pub(crate) fn retain_context_mapper(
        &mut self,
        owner: InferenceContextId,
        mapper: Option<MapperId>,
    ) {
        if let Some(mapper) = mapper {
            if self.mappers.lifetime_flags(mapper) == HAS_CONTEXT {
                let older_than = (!self.inference_context(owner).escaped).then_some(owner);
                self.walk_inference_mappers(mapper, older_than);
            }
        }
    }

    #[inline]
    fn walk_inference_mappers(&mut self, root: MapperId, older_than: Option<InferenceContextId>) {
        if older_than.is_some_and(|owner| {
            matches!(self.mapper(root),
                TypeMapper::Deferred(
                    DeferredMapperTargets::InferenceFixing(context)
                    | DeferredMapperTargets::InferenceNonFixing(context)
                ) if context.index() <= owner.index())
        }) {
            return;
        }
        self.walk_inference_mapper_graph(root, older_than);
    }

    #[inline(never)]
    fn walk_inference_mapper_graph(
        &mut self,
        root: MapperId,
        older_than: Option<InferenceContextId>,
    ) {
        let mut todo = std::mem::take(&mut self.inference_lifetimes.todo);
        let mut visited = std::mem::take(&mut self.inference_lifetimes.visited);
        todo.push((root, older_than.is_none()));
        while let Some((id, force)) = todo.pop() {
            let flags = self.mappers.lifetime_flags(id);
            if flags & HAS_CONTEXT == 0 || flags & RETAINED != 0 {
                continue;
            }
            if !force && !visited.insert(id) {
                continue;
            }
            if force {
                self.mappers.add_lifetime_flags(id, RETAINED);
            }
            match self.mappers.get(id) {
                TypeMapper::Composite { mapper1, mapper2 }
                | TypeMapper::Merged { mapper1, mapper2 } => {
                    todo.push((mapper1, force));
                    todo.push((mapper2, force));
                }
                TypeMapper::Deferred(
                    DeferredMapperTargets::InferenceFixing(context)
                    | DeferredMapperTargets::InferenceNonFixing(context),
                ) => {
                    if force || older_than.is_some_and(|owner| context.index() > owner.index()) {
                        self.mappers.add_lifetime_flags(id, RETAINED);
                        let context = self.inference_context_mut(context);
                        if !std::mem::replace(&mut context.escaped, true) {
                            todo.extend(
                                [
                                    context.mapper,
                                    context.non_fixing_mapper,
                                    context.return_mapper,
                                    context.outer_return_mapper,
                                ]
                                .into_iter()
                                .flatten()
                                .map(|mapper| (mapper, true)),
                            );
                        }
                    }
                }
                _ => unreachable!("only inference mapper graphs have context flags"),
            }
        }
        visited.clear();
        let buckets = if visited.capacity() == 0 {
            0
        } else {
            visited.capacity().next_power_of_two()
        };
        let requested = todo.capacity() * std::mem::size_of::<(MapperId, bool)>()
            + buckets * (std::mem::size_of::<MapperId>() + 1)
            + 16;
        // Keep requested storage below half the charged 16 KiB workspace bound.
        if requested <= 8192 {
            self.inference_lifetimes.todo = todo;
            self.inference_lifetimes.visited = visited;
        }
    }

    #[inline]
    pub(crate) fn inference_scope_start(&self) -> usize {
        self.inference_lifetimes.contexts.len()
    }

    /// The semantic worker has returned, including a candidate's normal retry.
    #[inline(always)]
    pub(crate) fn finish_inference_scope(&mut self, start: usize) {
        debug_assert!(start <= self.inference_lifetimes.contexts.len());
        if self.inference_lifetimes.contexts.len() > start {
            self.retire_inference_contexts(start);
        }
    }

    #[inline(never)]
    fn retire_inference_contexts(&mut self, start: usize) {
        while self.inference_lifetimes.contexts.len() > start {
            let id = self
                .inference_lifetimes
                .contexts
                .pop()
                .expect("inference scope context");
            if self.inference_context(id).escaped {
                continue;
            }
            let context = self.inference_context_arena.retire(id);
            for &slot in &context.mapper_infos {
                let info = &self.inference_info_arena[slot];
                if !info.shared {
                    let info = self.inference_info_arena.retire(slot);
                    info.candidates = None;
                    info.contra_candidates = None;
                }
            }
            context.inferences.clear();
            context.mapper_sources.clear();
            context.mapper_infos.clear();
            context.inferred_type_parameters = None;
            context.intra_expression_inference_sites = None;
        }
    }

    pub(crate) fn take_inference_buffer(&mut self, count: usize) -> InferenceInfos {
        let buffer = self
            .inference_context_arena
            .last_free_mut()
            .map(|context| std::mem::take(&mut context.inferences))
            .unwrap_or_default();
        exact_buffer(buffer, count)
    }
}

#[cfg(test)]
mod tests {
    use super::{InferenceHandle, InferencePool};
    use crate::inference::{create_inference_info, InferenceInfoId};
    use tsc_types::TypeId;

    #[test]
    fn a_mappers_lifetime_flags_live_in_its_record() {
        use super::{HAS_CONTEXT, RETAINED};
        use crate::instantiate::{FunctionMapper, MapperStore, TypeMapper};

        let arena = tsc_arena::LocalArena::new(4242);
        let mut store = MapperStore::default();
        let plain = store.alloc(&arena, TypeMapper::Function(FunctionMapper::Permissive), 0);
        let context = store.alloc(
            &arena,
            TypeMapper::Function(FunctionMapper::Restrictive),
            HAS_CONTEXT,
        );
        assert_eq!(store.lifetime_flags(plain), 0);
        assert_eq!(store.lifetime_flags(context), HAS_CONTEXT);
        store.add_lifetime_flags(context, RETAINED);
        assert_eq!(store.lifetime_flags(context), HAS_CONTEXT | RETAINED);
        assert_eq!(store.lifetime_flags(plain), 0);
        // The flags leave the mapper itself unchanged.
        assert!(matches!(
            store.get(context),
            TypeMapper::Function(FunctionMapper::Restrictive)
        ));
    }

    #[test]
    fn reused_records_keep_distinct_logical_ids() {
        let mut pool = InferencePool::default();
        let first = pool.push(create_inference_info(TypeId::new(0)));
        let kept = pool.push(create_inference_info(TypeId::new(1)));
        pool.retire(first);
        let new = pool.push(create_inference_info(TypeId::new(2)));
        assert_eq!((first.index(), kept.index(), new.index()), (0, 1, 2));
        assert_eq!(first.slot(), new.slot());
        assert_eq!(pool.len(), 3);
        assert_eq!(pool.records().len(), 2);
        assert_eq!(pool[kept].type_parameter, TypeId::new(1));
        assert_eq!(pool[new].type_parameter, TypeId::new(2));
    }

    #[test]
    #[should_panic(expected = "retired inference identity")]
    fn a_retired_id_does_not_resolve_to_its_reused_record() {
        let mut pool = InferencePool::default();
        let first = pool.push(create_inference_info(TypeId::new(0)));
        pool.retire(first);
        pool.push(create_inference_info(TypeId::new(1)));
        let _ = pool[first];
    }

    #[test]
    fn handle_hash_order_and_debug_use_the_permanent_identity() {
        use std::hash::{Hash, Hasher};
        let a = InferenceInfoId::from_parts(7, 0);
        let b = InferenceInfoId::from_parts(7, 99);
        assert_eq!(a, b);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
        let hash = |id: InferenceInfoId| {
            let mut hasher = rustc_hash::FxHasher::default();
            id.hash(&mut hasher);
            hasher.finish()
        };
        assert_eq!(hash(a), hash(b));
        assert_eq!(format!("{a:?}"), "InferenceInfoId(7)");
    }
}
