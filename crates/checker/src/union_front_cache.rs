//! Exact, bounded memo in front of checker union construction.
//!
//! Keys preserve input order, reduction and the alias's symbol/arguments.
//! A miss is stored only when it created nothing except its returned union,
//! instantiated nothing, avoided the two state-dependent reductions, and
//! did not return errorType. See the lane's DESIGN.md for the cache argument.
//! No state is shared between checkers. Verification and statistics exist
//! only with union-cache-shadow (unit tests also keep local statistics).
//!
//! A key of up to eight words lives in its slot. A longer one (a union built
//! again and again from the same few dozen or few thousand inputs) has a
//! table of its own beside it: the same complete key, held in a buffer its
//! slot keeps, under the same store rule.

use tsc_types::{SymbolId, TypeFlags, TypeId, UnionReduction};

use crate::state::{CheckResult, CheckerState};

const MAX_WORDS: usize = 8;
const SLOTS: usize = 1 << 10;
/// The table of keys that do not fit a slot: its size, and the longest key
/// it takes (a longer list is built without a probe, as before).
const LONG_SLOTS: usize = 1 << 10;
const MAX_LONG_WORDS: usize = 1 << 13;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Key {
    words: [u32; MAX_WORDS],
    // Nonzero for occupied keys; reduction, input count, alias and optional
    // argument-list presence. Here None and Some([]) are distinct AliasKeys.
    meta: u16,
}

impl Key {
    fn new(
        types: &[TypeId],
        reduction: UnionReduction,
        alias: Option<SymbolId>,
        arguments: Option<&[TypeId]>,
    ) -> Option<(Self, usize)> {
        let has_arguments = alias.is_some() && arguments.is_some();
        let arguments = if alias.is_some() {
            arguments.unwrap_or_default()
        } else {
            &[]
        };
        if types.len() + usize::from(alias.is_some()) + arguments.len() > MAX_WORDS {
            return None;
        }
        let mut key = Self {
            words: [0; MAX_WORDS],
            meta: 1
                | ((reduction as u16) << 1)
                | ((types.len() as u16) << 3)
                | (u16::from(alias.is_some()) << 7)
                | (u16::from(has_arguments) << 8),
        };
        let mut hash = u64::from(key.meta);
        let mut n = 0;
        let mut push = |word: u32| {
            key.words[n] = word;
            n += 1;
            hash = (hash ^ u64::from(word)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        };
        for &ty in types {
            push(ty.position());
        }
        if let Some(alias) = alias {
            push(alias.index());
            for &ty in arguments {
                push(ty.position());
            }
        }
        hash ^= hash >> 29;
        Some((key, (hash >> (64 - 10)) as usize))
    }
}

#[derive(Clone, Copy, Default)]
struct Slot {
    key: Key,
    result: Option<TypeId>,
}

const _: () = assert!(std::mem::size_of::<Slot>() == 40);

/// A key too long for a `Slot`: `words` is the whole key - the meta word,
/// the inputs in order, then the alias and its arguments. An evicted
/// entry's buffer is reused by the one that replaces it.
#[derive(Default)]
struct LongSlot {
    hash: u64,
    words: Vec<u32>,
    result: Option<TypeId>,
}

/// The words of a long key in order, without building them.
fn long_key_words<'a>(
    types: &'a [TypeId],
    reduction: UnionReduction,
    alias: Option<SymbolId>,
    arguments: Option<&'a [TypeId]>,
) -> impl Iterator<Item = u32> + 'a {
    let has_arguments = alias.is_some() && arguments.is_some();
    let arguments = if alias.is_some() {
        arguments.unwrap_or_default()
    } else {
        &[]
    };
    // The input count separates the inputs from the alias's arguments; the
    // low bits keep None and Some([]) arguments apart, as the short key does.
    let meta = (reduction as u32)
        | (u32::from(alias.is_some()) << 2)
        | (u32::from(has_arguments) << 3)
        | ((types.len() as u32) << 4);
    std::iter::once(meta)
        .chain(types.iter().map(|ty| ty.position()))
        .chain(alias.map(|alias| alias.index()))
        .chain(arguments.iter().map(|ty| ty.position()))
}

fn long_key_hash(words: impl Iterator<Item = u32>) -> (u64, usize) {
    let mut hash = 0u64;
    for word in words {
        hash = (hash ^ u64::from(word)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    hash ^= hash >> 29;
    (hash, (hash >> (64 - 10)) as usize)
}

const _: () = assert!(LONG_SLOTS == 1 << 10);

#[derive(Default)]
pub(crate) struct UnionFrontCache {
    slots: Vec<Slot>,
    long_slots: Vec<LongSlot>,
    /// Increment before either unstable reduction, including in nested calls.
    pub(crate) impure: u64,
    #[cfg(any(test, feature = "union-cache-shadow"))]
    stats: Stats,
}

#[cfg(any(test, feature = "union-cache-shadow"))]
#[derive(Default, Debug)]
struct Stats {
    lookups: u64,
    hits: u64,
    stores: u64,
    rejected: [u64; 4],
    origin: u64,
    long: u64,
    aborted: u64,
    long_lookups: u64,
    long_hits: u64,
    long_stores: u64,
}

impl UnionFrontCache {
    fn lookup(&mut self, key: Key, index: usize) -> Option<TypeId> {
        if self.slots.is_empty() {
            self.slots = vec![Slot::default(); SLOTS];
        }
        let slot = self.slots[index];
        (slot.key == key).then_some(slot.result).flatten()
    }

    fn store(&mut self, key: Key, index: usize, result: TypeId) {
        self.slots[index] = Slot {
            key,
            result: Some(result),
        };
    }

    fn lookup_long(
        &self,
        hash: u64,
        index: usize,
        words: impl Iterator<Item = u32>,
    ) -> Option<TypeId> {
        let slot = self.long_slots.get(index)?;
        (slot.hash == hash && slot.result.is_some() && slot.words.iter().copied().eq(words))
            .then_some(slot.result)
            .flatten()
    }

    fn store_long(
        &mut self,
        hash: u64,
        index: usize,
        words: impl Iterator<Item = u32>,
        result: TypeId,
    ) {
        if self.long_slots.is_empty() {
            self.long_slots.resize_with(LONG_SLOTS, LongSlot::default);
        }
        let slot = &mut self.long_slots[index];
        slot.hash = hash;
        slot.words.clear();
        slot.words.extend(words);
        slot.result = Some(result);
    }
}

#[derive(Clone, Copy)]
struct Counters {
    types: usize,
    local: u64,
    total: u64,
    impure: u64,
    #[cfg(feature = "union-cache-shadow")]
    diagnostics: (usize, usize, usize),
}

impl Counters {
    fn read(state: &CheckerState<'_>) -> Self {
        Self {
            types: state.tables.len(),
            local: state.instantiation_count,
            total: state.total_instantiation_count,
            impure: state.union_front_cache.impure,
            #[cfg(feature = "union-cache-shadow")]
            diagnostics: (
                state.diagnostics.len(),
                state.visible_global_diagnostics.len(),
                state.deferred_diagnostics.len(),
            ),
        }
    }

    fn rejections(self, state: &CheckerState<'_>, result: TypeId) -> [bool; 4] {
        let after = Self::read(state);
        [
            self.types != after.types
                && !(after.types == self.types + 1
                    && result.index() as usize + 1 == after.types
                    && state.tables.flags_of(result).intersects(TypeFlags::UNION)),
            self.local != after.local || self.total != after.total,
            self.impure != after.impure,
            result == state.tables.intrinsics.error,
        ]
    }
}

impl CheckerState<'_> {
    /// Only 2+ input, origin-free calls whose complete key fits may probe.
    pub(crate) fn get_union_type_front_cached(
        &mut self,
        types: &[TypeId],
        reduction: UnionReduction,
        alias: Option<SymbolId>,
        arguments: Option<&[TypeId]>,
        origin: Option<TypeId>,
    ) -> CheckResult<TypeId> {
        if origin.is_some() {
            #[cfg(any(test, feature = "union-cache-shadow"))]
            {
                self.union_front_cache.stats.origin += 1;
            }
            return self.get_union_type_ex_uncached(types, reduction, alias, arguments, origin);
        }
        let Some((key, index)) = Key::new(types, reduction, alias, arguments) else {
            return self.get_union_type_long_cached(types, reduction, alias, arguments);
        };
        #[cfg(any(test, feature = "union-cache-shadow"))]
        {
            self.union_front_cache.stats.lookups += 1;
        }
        if let Some(cached) = self.union_front_cache.lookup(key, index) {
            #[cfg(any(test, feature = "union-cache-shadow"))]
            {
                self.union_front_cache.stats.hits += 1;
            }
            #[cfg(feature = "union-cache-shadow")]
            self.union_cache_shadow(types, reduction, alias, arguments, cached);
            return Ok(cached);
        }
        let before = Counters::read(self);
        let result = self.get_union_type_ex_uncached(types, reduction, alias, arguments, None);
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                #[cfg(any(test, feature = "union-cache-shadow"))]
                {
                    self.union_front_cache.stats.aborted += 1;
                }
                return Err(error);
            }
        };
        let rejected = before.rejections(self, result);
        if !rejected.iter().any(|&no| no) {
            self.union_front_cache.store(key, index, result);
            #[cfg(any(test, feature = "union-cache-shadow"))]
            {
                self.union_front_cache.stats.stores += 1;
            }
        } else {
            #[cfg(any(test, feature = "union-cache-shadow"))]
            for (count, no) in self
                .union_front_cache
                .stats
                .rejected
                .iter_mut()
                .zip(rejected)
            {
                *count += u64::from(no);
            }
        }
        Ok(result)
    }

    /// The probe for a key that does not fit a slot: the same hit and the
    /// same store rule over the long keys' table.
    fn get_union_type_long_cached(
        &mut self,
        types: &[TypeId],
        reduction: UnionReduction,
        alias: Option<SymbolId>,
        arguments: Option<&[TypeId]>,
    ) -> CheckResult<TypeId> {
        let argument_count = if alias.is_some() {
            arguments.map_or(0, <[TypeId]>::len)
        } else {
            0
        };
        if types.len() + argument_count >= MAX_LONG_WORDS {
            #[cfg(any(test, feature = "union-cache-shadow"))]
            {
                self.union_front_cache.stats.long += 1;
            }
            return self.get_union_type_ex_uncached(types, reduction, alias, arguments, None);
        }
        let (hash, index) = long_key_hash(long_key_words(types, reduction, alias, arguments));
        #[cfg(any(test, feature = "union-cache-shadow"))]
        {
            self.union_front_cache.stats.long_lookups += 1;
        }
        if let Some(cached) = self.union_front_cache.lookup_long(
            hash,
            index,
            long_key_words(types, reduction, alias, arguments),
        ) {
            #[cfg(any(test, feature = "union-cache-shadow"))]
            {
                self.union_front_cache.stats.long_hits += 1;
            }
            #[cfg(feature = "union-cache-shadow")]
            self.union_cache_shadow(types, reduction, alias, arguments, cached);
            return Ok(cached);
        }
        let before = Counters::read(self);
        let result = self.get_union_type_ex_uncached(types, reduction, alias, arguments, None)?;
        if !before.rejections(self, result).iter().any(|&no| no) {
            self.union_front_cache.store_long(
                hash,
                index,
                long_key_words(types, reduction, alias, arguments),
                result,
            );
            #[cfg(any(test, feature = "union-cache-shadow"))]
            {
                self.union_front_cache.stats.long_stores += 1;
            }
        }
        Ok(result)
    }

    /// Run the call a hit skipped and assert it answers `cached` without an
    /// effect.
    #[cfg(feature = "union-cache-shadow")]
    fn union_cache_shadow(
        &mut self,
        types: &[TypeId],
        reduction: UnionReduction,
        alias: Option<SymbolId>,
        arguments: Option<&[TypeId]>,
        cached: TypeId,
    ) {
        let before = Counters::read(self);
        let fresh = self
            .get_union_type_ex_uncached(types, reduction, alias, arguments, None)
            .expect("union-cache-shadow: uncached hit aborted");
        let after = Counters::read(self);
        assert_eq!(
            fresh, cached,
            "union-cache-shadow: answer for {types:?} {reduction:?} {alias:?} {arguments:?}"
        );
        assert_eq!(
            before.types, after.types,
            "union-cache-shadow: type creation"
        );
        assert_eq!(
            before.local, after.local,
            "union-cache-shadow: local instantiation"
        );
        assert_eq!(
            before.total, after.total,
            "union-cache-shadow: total instantiation"
        );
        assert_eq!(
            before.impure, after.impure,
            "union-cache-shadow: impure reduction"
        );
        assert_eq!(
            before.diagnostics, after.diagnostics,
            "union-cache-shadow: diagnostic"
        );
    }
}

#[cfg(feature = "union-cache-shadow")]
impl UnionFrontCache {
    pub(crate) fn report(&self) {
        let s = &self.stats;
        eprintln!(
            "union-front-cache: lookups={} hits={} stores={} reject_created={} reject_instantiated={} reject_impure={} reject_error={} origin={} long={} aborted={} long_lookups={} long_hits={} long_stores={}",
            s.lookups, s.hits, s.stores, s.rejected[0], s.rejected[1],
            s.rejected[2], s.rejected[3], s.origin, s.long, s.aborted,
            s.long_lookups, s.long_hits, s.long_stores,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_binder::bind_source_file;
    use tsc_syntax::{parse_published_source_file, ParseOptions, ScriptKind};
    use tsc_types::{CompilerOptions, IntersectionFlags, ObjectFlags, TypeData};

    fn with_state(text: &str, run: impl FnOnce(&mut CheckerState<'_>)) {
        let options = CompilerOptions::default();
        let mut source = parse_published_source_file(
            "union-cache-test.ts".to_owned(),
            text.to_owned(),
            ParseOptions {
                script_kind: ScriptKind::Ts,
                ..ParseOptions::default()
            },
            None,
        );
        assert!(source.parse_diagnostics.is_empty());
        let binder = bind_source_file(&mut source);
        let mut state = CheckerState::new(binder.source, &binder, &options);
        run(&mut state);
    }

    fn call(state: &mut CheckerState<'_>, types: &[TypeId]) -> TypeId {
        state
            .get_union_type_ex(types, UnionReduction::Literal)
            .unwrap()
    }

    fn annotation(state: &mut CheckerState<'_>, name: &str) -> TypeId {
        let node = crate::relpin::find_probe_annotation(state.binder.source(0), name).unwrap();
        state.get_type_from_type_node(node).unwrap()
    }

    fn cached(state: &mut CheckerState<'_>, types: &[TypeId], expected: TypeId) {
        let before = Counters::read(state);
        let hits = state.union_front_cache.stats.hits;
        assert_eq!(call(state, types), expected);
        let after = Counters::read(state);
        assert_eq!(after.types, before.types);
        assert_eq!(after.local, before.local);
        assert_eq!(after.total, before.total);
        assert_eq!(state.union_front_cache.stats.hits, hits + 1);
    }

    #[test]
    fn full_keys_separate_order_reduction_alias_and_argument_presence() {
        let a = TypeId::new(1);
        let b = TypeId::new(2);
        let symbol = SymbolId::new(3);
        let key =
            |types: &[TypeId], mode, alias, args| Key::new(types, mode, alias, args).unwrap().0;
        let plain = key(&[a, b], UnionReduction::Literal, None, None);
        let others = [
            key(&[b, a], UnionReduction::Literal, None, None),
            key(&[a, b], UnionReduction::Subtype, None, None),
            key(&[a, b], UnionReduction::None, None, None),
            key(&[a, b], UnionReduction::Literal, Some(symbol), None),
            key(&[a, b], UnionReduction::Literal, Some(symbol), Some(&[])),
            key(&[a, b], UnionReduction::Literal, Some(symbol), Some(&[a])),
            key(&[a, b], UnionReduction::Literal, Some(symbol), Some(&[b])),
        ];
        let mut cache = UnionFrontCache::default();
        assert!(cache.slots.is_empty());
        assert_eq!(cache.lookup(plain, 0), None);
        for (i, &other) in others.iter().enumerate() {
            assert_ne!(plain, other);
            for &previous in &others[..i] {
                assert_ne!(previous, other);
            }
            // Deliberately force a collision. The full key controls the hit.
            cache.store(other, 0, a);
            assert_eq!(cache.lookup(plain, 0), None);
            assert_eq!(cache.lookup(other, 0), Some(a));
        }
        assert_eq!(std::mem::size_of::<Slot>() * SLOTS, 40 * 1024);
        assert!(Key::new(&[a; 8], UnionReduction::Literal, None, None).is_some());
        assert!(Key::new(&[a; 9], UnionReduction::Literal, None, None).is_none());
        assert!(Key::new(&[a; 8], UnionReduction::Literal, Some(symbol), None).is_none());
    }

    #[test]
    fn long_keys_hit_without_creating_and_keep_every_part_of_the_key() {
        with_state("", |state| {
            let types: Vec<TypeId> = (0..12)
                .map(|i| {
                    state
                        .tables
                        .get_string_literal_type(&format!("long-key-{i}"))
                })
                .collect();
            let before = state.tables.len();
            let result = call(state, &types);
            assert_eq!(state.tables.len(), before + 1);
            assert_eq!(state.union_front_cache.stats.long_stores, 1);
            let before = Counters::read(state);
            assert_eq!(call(state, &types), result);
            let after = Counters::read(state);
            assert_eq!(state.union_front_cache.stats.long_hits, 1);
            assert_eq!(after.types, before.types);
            assert_eq!(after.local, before.local);
            assert_eq!(after.total, before.total);
            // Another order of the same inputs is another key for the same union.
            let mut reversed = types.clone();
            reversed.reverse();
            assert_eq!(call(state, &reversed), result);
            assert_eq!(state.union_front_cache.stats.long_hits, 1);
            assert_eq!(state.union_front_cache.stats.long_lookups, 3);
        });
        let a = TypeId::new(1);
        let b = TypeId::new(2);
        let symbol = SymbolId::new(3);
        let words = |types: &[TypeId], mode, alias, args: Option<&[TypeId]>| {
            long_key_words(types, mode, alias, args).collect::<Vec<_>>()
        };
        let nine = [a; 9];
        let keys = [
            words(&nine, UnionReduction::Literal, None, None),
            words(&nine, UnionReduction::Subtype, None, None),
            words(&nine, UnionReduction::None, None, None),
            words(&nine, UnionReduction::Literal, Some(symbol), None),
            words(&nine, UnionReduction::Literal, Some(symbol), Some(&[])),
            words(&nine, UnionReduction::Literal, Some(symbol), Some(&[a])),
            words(&nine, UnionReduction::Literal, Some(symbol), Some(&[b])),
            words(&[a; 10], UnionReduction::Literal, None, None),
            // Where the inputs end is part of the key.
            words(
                &[a; 8],
                UnionReduction::Literal,
                Some(symbol),
                Some(&[a, a]),
            ),
        ];
        let mut cache = UnionFrontCache::default();
        assert!(cache.long_slots.is_empty());
        for (i, key) in keys.iter().enumerate() {
            for previous in &keys[..i] {
                assert_ne!(previous, key);
            }
            // Deliberately the same hash and slot: the whole key controls the hit.
            cache.store_long(7, 0, key.iter().copied(), b);
            assert_eq!(cache.lookup_long(7, 0, key.iter().copied()), Some(b));
            for previous in &keys[..i] {
                assert_eq!(cache.lookup_long(7, 0, previous.iter().copied()), None);
            }
            assert_eq!(cache.lookup_long(8, 0, key.iter().copied()), None);
        }
        assert_eq!(cache.long_slots.len(), LONG_SLOTS);
    }

    #[test]
    fn caches_new_union_and_non_union_answers_without_creating_on_hit() {
        with_state("", |state| {
            let i = state.tables.intrinsics.clone();
            let types = [i.string, i.number];
            let before = state.tables.len();
            let result = call(state, &types);
            assert!(state.tables.flags_of(result).intersects(TypeFlags::UNION));
            // This pair may already be interned by intrinsic initialization.
            assert!(state.tables.len() <= before + 1);
            cached(state, &types, result);
            for (types, result) in [
                ([i.never, i.never], i.never),
                ([i.string, i.string], i.string),
                ([i.any, i.string], i.any),
                ([i.unknown, i.string], i.unknown),
            ] {
                assert_eq!(call(state, &types), result);
                cached(state, &types, result);
            }
            let a = state.tables.get_string_literal_type("cache-new-a");
            let b = state.tables.get_string_literal_type("cache-new-b");
            let before = state.tables.len();
            let result = call(state, &[a, b]);
            assert_eq!(state.tables.len(), before + 1);
            assert_eq!(result.index() as usize + 1, state.tables.len());
            cached(state, &[a, b], result);
        });
    }

    #[test]
    fn aliases_and_optional_empty_arguments_have_distinct_answers() {
        with_state("type A = string | number; declare var a: A;", |state| {
            let named = annotation(state, "a");
            let alias = state.tables.alias_symbol(named).expect("alias A");
            let inputs = [
                state.tables.intrinsics.string,
                state.tables.intrinsics.number,
            ];
            let plain = call(state, &inputs);
            let mut results = Vec::new();
            for args in [None, Some(&[][..]), Some(&inputs[..1])] {
                let result = state
                    .get_union_type_ex_with_origin(
                        &inputs,
                        UnionReduction::Literal,
                        Some(alias),
                        args,
                        None,
                    )
                    .unwrap();
                let hits = state.union_front_cache.stats.hits;
                assert_eq!(
                    state
                        .get_union_type_ex_with_origin(
                            &inputs,
                            UnionReduction::Literal,
                            Some(alias),
                            args,
                            None,
                        )
                        .unwrap(),
                    result
                );
                assert_eq!(state.union_front_cache.stats.hits, hits + 1);
                assert_ne!(plain, result);
                assert!(!results.contains(&result));
                results.push(result);
            }
        });
    }

    #[test]
    fn origin_and_overlong_keys_bypass_the_tables() {
        with_state("", |state| {
            let inputs = [
                state.tables.intrinsics.string,
                state.tables.intrinsics.number,
            ];
            let origin = state
                .tables
                .create_origin_union_or_intersection_type(TypeFlags::UNION, inputs);
            let lookups = state.union_front_cache.stats.lookups;
            let bypass = state.union_front_cache.stats.origin;
            let first = state
                .get_union_type_ex_with_origin(
                    &inputs,
                    UnionReduction::Literal,
                    None,
                    None,
                    Some(origin),
                )
                .unwrap();
            let second = state
                .get_union_type_ex_with_origin(
                    &inputs,
                    UnionReduction::Literal,
                    None,
                    None,
                    Some(origin),
                )
                .unwrap();
            assert_eq!(first, second);
            assert_eq!(state.union_front_cache.stats.lookups, lookups);
            assert_eq!(state.union_front_cache.stats.origin, bypass + 2);
            // Nine inputs do not fit a slot: they probe the long keys' table,
            // not the slots'.
            let long = [inputs[0]; 9];
            assert_eq!(call(state, &long), inputs[0]);
            assert_eq!(state.union_front_cache.stats.lookups, lookups);
            assert_eq!(state.union_front_cache.stats.long_lookups, 1);
            assert_eq!(state.union_front_cache.stats.long, 0);
            // A list beyond the long table's limit is built without a probe.
            let overlong = vec![inputs[0]; MAX_LONG_WORDS];
            assert_eq!(call(state, &overlong), inputs[0]);
            assert_eq!(call(state, &overlong[..MAX_LONG_WORDS - 1]), inputs[0]);
            assert_eq!(state.union_front_cache.stats.lookups, lookups);
            assert_eq!(state.union_front_cache.stats.long_lookups, 2);
            assert_eq!(state.union_front_cache.stats.long, 1);
            assert_eq!(call(state, &[]), state.tables.intrinsics.never);
            assert_eq!(call(state, &inputs[..1]), inputs[0]);
            assert_eq!(state.union_front_cache.stats.lookups, lookups);
        });
    }

    #[test]
    fn error_type_is_recomputed_on_every_call() {
        with_state("", |state| {
            let inputs = [
                state.tables.intrinsics.error,
                state.tables.intrinsics.string,
            ];
            let rejected = state.union_front_cache.stats.rejected[3];
            let hits = state.union_front_cache.stats.hits;
            for _ in 0..2 {
                assert_eq!(call(state, &inputs), state.tables.intrinsics.error);
            }
            assert_eq!(state.union_front_cache.stats.rejected[3], rejected + 2);
            assert_eq!(state.union_front_cache.stats.hits, hits);
        });
    }

    #[test]
    fn template_and_constrained_reductions_are_not_stored() {
        with_state("declare var t: `a${string}`;", |state| {
            let template = annotation(state, "t");
            let literal = state.tables.get_string_literal_type("abc");
            let rejected = state.union_front_cache.stats.rejected[2];
            for _ in 0..2 {
                assert_eq!(call(state, &[literal, template]), template);
            }
            assert_eq!(state.union_front_cache.stats.rejected[2], rejected + 2);

            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let constraint = call(state, &[string, number]);
            let parameter = state
                .tables
                .create_synthesized_type_parameter(Some(constraint));
            let a = state
                .get_intersection_type(&[parameter, string], IntersectionFlags::NONE)
                .unwrap();
            let b = state
                .get_intersection_type(&[parameter, number], IntersectionFlags::NONE)
                .unwrap();
            assert!(state
                .tables
                .object_flags_of(a)
                .intersects(ObjectFlags::IS_CONSTRAINED_TYPE_VARIABLE));
            assert!(state
                .tables
                .object_flags_of(b)
                .intersects(ObjectFlags::IS_CONSTRAINED_TYPE_VARIABLE));
            let rejected = state.union_front_cache.stats.rejected[2];
            for _ in 0..2 {
                assert_eq!(call(state, &[a, b]), parameter);
            }
            assert_eq!(state.union_front_cache.stats.rejected[2], rejected + 2);
        });
    }

    #[test]
    fn rejects_type_creation_and_both_instantiation_counters() {
        with_state("", |state| {
            let before = Counters::read(state);
            let fresh = state.tables.create_type(
                TypeFlags::TYPE_PARAMETER,
                TypeData::TypeParameter {
                    is_this_type: false,
                    is_distributed: false,
                    constraint: None,
                    target: None,
                    mapper: None,
                },
            );
            // Even a sole newest allocation must be a union to be stored.
            assert!(before.rejections(state, fresh)[0]);
            let before = Counters::read(state);
            state.tables.create_synthesized_type_parameter(None);
            assert!(before.rejections(state, state.tables.intrinsics.string)[0]);
            let before = Counters::read(state);
            state.instantiation_count += 1;
            assert!(before.rejections(state, fresh)[1]);
            let before = Counters::read(state);
            state.instantiation_count = 0;
            assert!(before.rejections(state, fresh)[1]);
            let before = Counters::read(state);
            state.total_instantiation_count += 1;
            assert!(before.rejections(state, fresh)[1]);
            let before = Counters::read(state);
            state.union_front_cache.impure += 1;
            assert!(before.rejections(state, fresh)[2]);
        });
    }

    #[test]
    fn fresh_named_union_origin_is_rejected_but_union_of_unions_can_warm() {
        with_state("type A = string | number; type B = boolean | symbol; declare var a: A; declare var b: B;", |state| {
            let a = annotation(state, "a");
            let b = annotation(state, "b");
            let rejected = state.union_front_cache.stats.rejected[0];
            let result = call(state, &[a, b]);
            assert_eq!(state.union_front_cache.stats.rejected[0], rejected + 1);
            // The pre-existing two-union cache now answers without allocating.
            assert_eq!(call(state, &[a, b]), result);
            cached(state, &[a, b], result);
        });
    }

    #[test]
    fn subtype_reduction_cache_makes_repeats_counter_neutral() {
        with_state(
            "declare var a: { x: number; y: string }; declare var b: { x: number };",
            |state| {
                let a = annotation(state, "a");
                let b = annotation(state, "b");
                let run = |state: &mut CheckerState<'_>| {
                    state
                        .get_union_type_ex(&[a, b], UnionReduction::Subtype)
                        .unwrap()
                };
                assert_eq!(run(state), b);
                // The first pass may resolve members; the second is safe to store.
                assert_eq!(run(state), b);
                let hits = state.union_front_cache.stats.hits;
                let before = Counters::read(state);
                assert_eq!(run(state), b);
                assert_eq!(state.union_front_cache.stats.hits, hits + 1);
                assert_eq!(state.tables.len(), before.types);
                assert_eq!(state.total_instantiation_count, before.total);
            },
        );
    }
}
