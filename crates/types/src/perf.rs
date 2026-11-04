//! Aggregate performance counters for the compiler-performance redesign's
//! measurement slice (target/benchmarks/fable51-parity-20260922/
//! compiler-performance-redesign-plan.md, slice M).
//!
//! tsrs-native diagnostic instrumentation. Every counter is a process-wide
//! relaxed atomic that exists only when the `perf-counters` Cargo feature is
//! enabled; without the feature every call compiles to nothing and the
//! candidate binaries carry no trace of it. Counters never change results:
//! they observe cache lookups, hits, sentinel returns, publications and key
//! construction, and are written once at process exit by the CLI to the
//! sidecar file named by `TSRS_PERF_COUNTERS` (stdout/stderr stay byte-exact).
//! Aggregate process-wide totals only — never per-lookup logging, and never a
//! per-checker or per-epoch attribution: a repeated publication counted here
//! is a candidate for review, not proof of duplicate work.

/// One named counter. The order of variants is the report order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum PerfCounter {
    // ---- links tables (per-checker-invocation link records) ----
    /// Reads through the crate-private projection helpers.
    LinksNodeReads,
    /// The aggregate record for the id was absent (not a missing facet of
    /// a present record).
    LinksNodeReadAbsent,
    LinksSymbolReads,
    LinksSymbolReadAbsent,
    LinksTypeReads,
    LinksTypeReadAbsent,
    /// A slot went Vacant -> Resolving (a computation started).
    LinksSlotResolvingStarted,
    /// A slot went Resolving -> Resolved (a computation published).
    LinksSlotResolvedFromResolving,
    /// A slot went Vacant -> Resolved directly (published without a
    /// Resolving phase).
    LinksSlotResolvedDirect,
    /// A journaled symbol-type slot was restored by speculation rollback.
    LinksSymbolTypeRollbacks,
    // ---- Resolving sentinel observations (in-progress returns) ----
    SentinelDeclaredTypeResolving,
    SentinelTypeOfSymbolResolving,
    SentinelAliasResolving,
    SentinelVarianceInProgress,
    // ---- declared type / type of symbol getters ----
    DeclaredTypeQueries,
    DeclaredTypeHits,
    TypeOfSymbolQueries,
    TypeOfSymbolHits,
    ResolvedMembersQueries,
    ResolvedMembersHits,
    // ---- type interners (String keys today) ----
    TypeListIdCalls,
    TypeListIdBytes,
    AliasIdCalls,
    UnionLookups,
    UnionHits,
    UnionOfUnionLookups,
    UnionOfUnionHits,
    IntersectionLookups,
    IntersectionHits,
    InstantiationLookups,
    InstantiationHits,
    /// Bytes copied into owned keys on instantiation lookups.
    InstantiationKeyBytesCopied,
    StringLiteralLookups,
    StringLiteralHits,
    NumberLiteralLookups,
    NumberLiteralHits,
    // ---- instantiation caches ----
    MapperCacheLookups,
    MapperCacheHits,
    /// Active-mapper scope pushes: a map header is created (the map
    /// allocates on its first insertion, so this is not a heap count).
    MapperScopePushes,
    /// Entries into `instantiate_type_worker` (the actual instantiation
    /// work), whatever path reached it.
    InstantiateTypeWorkerCalls,
    SignatureInstantiationLookups,
    SignatureInstantiationHits,
    // ---- relation caches ----
    RelationLookups,
    /// A cache entry existed for the key (before any reporting-mode
    /// decision).
    RelationEntryFound,
    /// The cached verdict was returned to the caller.
    RelationReturnedCached,
    /// A cached failure was deliberately recomputed in reporting mode to
    /// rebuild the nested error path (tsc 65739-65741).
    RelationRecomputedForDiagnostic,
    RelationSets,
    // ---- other checker caches ----
    SubtypeReductionLookups,
    SubtypeReductionHits,
    ContextualCachedTypeLookups,
    ContextualCachedTypeHits,
    NarrowCachedTypeLookups,
    NarrowCachedTypeHits,
    // ---- emit facts (slice M2) ----
    TransformFlagWalks,
    TransformFlagWalkNodes,
    TransformAddSourceNodes,
    TransformUpdateNodeCalls,
    TransformCloneNodeCalls,
    PrinterPlanNodes,
    // ---- checker sharding (slice W): requested budget, shards run, threads ----
    CheckerShardsRequested,
    CheckerShardsRun,
    CheckerShardThreads,
    CheckerSerialReplays,
    CheckerReplayDisplay,
    CheckerReplayRepresentative,
    CheckerReplaySubtypeTie,
    CheckerReplayUnionSignatures,
    CheckerReplayIntersectionSignatures,
    CheckerReplayCircularity,
    CheckerReplayBestMatch,
    CheckerReplayInitDivergence,
    CheckerReplayFirstFailure,
    CheckerReplayContextualSignature,
    CheckerReplayUnionIndexInfos,
    CheckerReplayIntersectionLast,
    // ---- R29 union collection and content searches ----
    UnionCollectionTables01,
    UnionCollectionTables2To8,
    UnionCollectionTables9To12,
    UnionCollectionTables13To20,
    UnionCollectionTables21To64,
    UnionCollectionTables65To256,
    UnionCollectionTables257Plus,
    UnionCollectionChecker01,
    UnionCollectionChecker2To8,
    UnionCollectionChecker9To12,
    UnionCollectionChecker13To20,
    UnionCollectionChecker21To64,
    UnionCollectionChecker65To256,
    UnionCollectionChecker257Plus,
    UnionCollectionRecursive01,
    UnionCollectionRecursive2To8,
    UnionCollectionRecursive9To12,
    UnionCollectionRecursive13To20,
    UnionCollectionRecursive21To64,
    UnionCollectionRecursive65To256,
    UnionCollectionRecursive257Plus,
    TypeCompareOuterCalls,
    TypeCompareRecursiveCalls,
    UnionSortComparisons01,
    UnionSortComparisons2To8,
    UnionSortComparisons9To12,
    UnionSortComparisons13To20,
    UnionSortComparisons21To64,
    UnionSortComparisons65To256,
    UnionSortComparisons257Plus,
    ContentSearchHits,
    ContentSearchMisses,
    // ---- the name table ----
    NamesInternCalls,
    NamesInternInserts,
    NamesTextReads,
    NamesTextKeyLookups,
    NamesEscapeCalls,
    /// Sentinel: number of counters (not a counter).
    Count,
}

pub const COUNT: usize = PerfCounter::Count as usize;

/// Whether the counter has at least one instrumented site in this source.
/// An unwired counter reports `unwired`, never zero: zero cannot mean zero
/// work. Keep in variant order.
pub const WIRED: [bool; COUNT] = [
    true,  // links.node.reads
    true,  // links.node.read_absent
    true,  // links.symbol.reads
    true,  // links.symbol.read_absent
    true,  // links.type.reads
    true,  // links.type.read_absent
    true,  // links.slot.resolving_started
    true,  // links.slot.resolved_from_resolving
    true,  // links.slot.resolved_direct
    true,  // links.symbol_type.rollbacks
    false, // sentinel.declared_type.resolving (circularity uses the resolution stack, not a slot read)
    false, // sentinel.type_of_symbol.resolving (same)
    true,  // sentinel.alias.resolving
    true,  // sentinel.variance.in_progress
    true,  // declared_type.queries
    true,  // declared_type.hits (early-return slot hits in annotate.rs)
    true,  // type_of_symbol.queries
    true,  // type_of_symbol.hits (early-return slot hits in annotate.rs)
    false, // resolved_members.queries (M1 scope limit)
    false, // resolved_members.hits (M1 scope limit)
    true,  // type_list_id.calls
    true,  // type_list_id.bytes
    true,  // alias_id.calls
    true,  // union.lookups
    true,  // union.hits
    true,  // union_of_union.lookups
    true,  // union_of_union.hits
    true,  // intersection.lookups
    true,  // intersection.hits
    true,  // instantiation.lookups
    true,  // instantiation.hits
    true,  // instantiation.key_bytes_copied
    true,  // string_literal.lookups
    true,  // string_literal.hits
    true,  // number_literal.lookups
    true,  // number_literal.hits
    true,  // mapper_cache.lookups
    true,  // mapper_cache.hits
    true,  // mapper_scope.pushes
    true,  // instantiate_type_worker.calls
    true,  // signature_instantiation.lookups
    true,  // signature_instantiation.hits
    true,  // relation.lookups
    true,  // relation.entry_found
    true,  // relation.returned_cached
    true,  // relation.recomputed_for_diagnostic
    true,  // relation.sets
    true,  // subtype_reduction.lookups
    true,  // subtype_reduction.hits
    true,  // contextual_cached_type.lookups
    true,  // contextual_cached_type.hits
    true,  // narrow_cached_type.lookups
    true,  // narrow_cached_type.hits
    false, // transform.flag_walks (M2)
    false, // transform.flag_walk_nodes (M2)
    false, // transform.add_source_nodes (M2)
    false, // transform.update_node_calls (M2)
    false, // transform.clone_node_calls (M2)
    false, // printer.plan_nodes (M2)
    true,  // checker_shards.requested
    true,  // checker_shards.run
    true,  // checker_shards.threads
    true,  // checker_shards.replays
    true,  // checker_replay.display
    true,  // checker_replay.representative
    true,  // checker_replay.subtype_tie
    true,  // checker_replay.union_signatures
    true,  // checker_replay.intersection_signatures
    true,  // checker_replay.circularity
    true,  // checker_replay.best_match
    true,  // checker_replay.init_divergence
    true,  // checker_replay.first_failure
    true,  // checker_replay.contextual_signature
    true,  // checker_replay.union_index_infos
    true,  // checker_replay.intersection_last
    true,  // union_collect.tables.len_0_1
    true,  // union_collect.tables.len_2_8
    true,  // union_collect.tables.len_9_12
    true,  // union_collect.tables.len_13_20
    true,  // union_collect.tables.len_21_64
    true,  // union_collect.tables.len_65_256
    true,  // union_collect.tables.len_257_plus
    true,  // union_collect.checker.len_0_1
    true,  // union_collect.checker.len_2_8
    true,  // union_collect.checker.len_9_12
    true,  // union_collect.checker.len_13_20
    true,  // union_collect.checker.len_21_64
    true,  // union_collect.checker.len_65_256
    true,  // union_collect.checker.len_257_plus
    true,  // union_collect.recursive.len_0_1
    true,  // union_collect.recursive.len_2_8
    true,  // union_collect.recursive.len_9_12
    true,  // union_collect.recursive.len_13_20
    true,  // union_collect.recursive.len_21_64
    true,  // union_collect.recursive.len_65_256
    true,  // union_collect.recursive.len_257_plus
    true,  // compare_types.outer_calls
    true,  // compare_types.recursive_calls
    true,  // union_sort.comparisons.len_0_1
    true,  // union_sort.comparisons.len_2_8
    true,  // union_sort.comparisons.len_9_12
    true,  // union_sort.comparisons.len_13_20
    true,  // union_sort.comparisons.len_21_64
    true,  // union_sort.comparisons.len_65_256
    true,  // union_sort.comparisons.len_257_plus
    true,  // type_search.content.hits
    true,  // type_search.content.misses
    true,  // names.intern.calls
    true,  // names.intern.inserts
    true,  // names.text.reads
    true,  // names.key.text_lookups
    true,  // names.escape.calls
];

/// Report names in variant order.
pub const NAMES: [&str; COUNT] = [
    "links.node.reads",
    "links.node.read_absent",
    "links.symbol.reads",
    "links.symbol.read_absent",
    "links.type.reads",
    "links.type.read_absent",
    "links.slot.resolving_started",
    "links.slot.resolved_from_resolving",
    "links.slot.resolved_direct",
    "links.symbol_type.rollbacks",
    "sentinel.declared_type.resolving",
    "sentinel.type_of_symbol.resolving",
    "sentinel.alias.resolving",
    "sentinel.variance.in_progress",
    "declared_type.queries",
    "declared_type.hits",
    "type_of_symbol.queries",
    "type_of_symbol.hits",
    "resolved_members.queries",
    "resolved_members.hits",
    "type_list_id.calls",
    "type_list_id.bytes",
    "alias_id.calls",
    "union.lookups",
    "union.hits",
    "union_of_union.lookups",
    "union_of_union.hits",
    "intersection.lookups",
    "intersection.hits",
    "instantiation.lookups",
    "instantiation.hits",
    "instantiation.key_bytes_copied",
    "string_literal.lookups",
    "string_literal.hits",
    "number_literal.lookups",
    "number_literal.hits",
    "mapper_cache.lookups",
    "mapper_cache.hits",
    "mapper_scope.pushes",
    "instantiate_type_worker.calls",
    "signature_instantiation.lookups",
    "signature_instantiation.hits",
    "relation.lookups",
    "relation.entry_found",
    "relation.returned_cached",
    "relation.recomputed_for_diagnostic",
    "relation.sets",
    "subtype_reduction.lookups",
    "subtype_reduction.hits",
    "contextual_cached_type.lookups",
    "contextual_cached_type.hits",
    "narrow_cached_type.lookups",
    "narrow_cached_type.hits",
    "transform.flag_walks",
    "transform.flag_walk_nodes",
    "transform.add_source_nodes",
    "transform.update_node_calls",
    "transform.clone_node_calls",
    "printer.plan_nodes",
    "checker_shards.requested",
    "checker_shards.run",
    "checker_shards.threads",
    "checker_shards.replays",
    "checker_replay.display",
    "checker_replay.representative",
    "checker_replay.subtype_tie",
    "checker_replay.union_signatures",
    "checker_replay.intersection_signatures",
    "checker_replay.circularity",
    "checker_replay.best_match",
    "checker_replay.init_divergence",
    "checker_replay.first_failure",
    "checker_replay.contextual_signature",
    "checker_replay.union_index_infos",
    "checker_replay.intersection_last",
    "union_collect.tables.len_0_1",
    "union_collect.tables.len_2_8",
    "union_collect.tables.len_9_12",
    "union_collect.tables.len_13_20",
    "union_collect.tables.len_21_64",
    "union_collect.tables.len_65_256",
    "union_collect.tables.len_257_plus",
    "union_collect.checker.len_0_1",
    "union_collect.checker.len_2_8",
    "union_collect.checker.len_9_12",
    "union_collect.checker.len_13_20",
    "union_collect.checker.len_21_64",
    "union_collect.checker.len_65_256",
    "union_collect.checker.len_257_plus",
    "union_collect.recursive.len_0_1",
    "union_collect.recursive.len_2_8",
    "union_collect.recursive.len_9_12",
    "union_collect.recursive.len_13_20",
    "union_collect.recursive.len_21_64",
    "union_collect.recursive.len_65_256",
    "union_collect.recursive.len_257_plus",
    "compare_types.outer_calls",
    "compare_types.recursive_calls",
    "union_sort.comparisons.len_0_1",
    "union_sort.comparisons.len_2_8",
    "union_sort.comparisons.len_9_12",
    "union_sort.comparisons.len_13_20",
    "union_sort.comparisons.len_21_64",
    "union_sort.comparisons.len_65_256",
    "union_sort.comparisons.len_257_plus",
    "type_search.content.hits",
    "type_search.content.misses",
    "names.intern.calls",
    "names.intern.inserts",
    "names.text.reads",
    "names.key.text_lookups",
    "names.escape.calls",
];

#[cfg(feature = "perf-counters")]
mod enabled {
    use super::{PerfCounter, COUNT, NAMES};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[allow(clippy::declare_interior_mutable_const)]
    const ZERO: AtomicU64 = AtomicU64::new(0);
    static COUNTERS: [AtomicU64; COUNT] = [ZERO; COUNT];

    #[inline(always)]
    pub fn bump(counter: PerfCounter) {
        COUNTERS[counter as usize].fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn add(counter: PerfCounter, amount: u64) {
        COUNTERS[counter as usize].fetch_add(amount, Ordering::Relaxed);
    }

    pub fn snapshot() -> Vec<(&'static str, Option<u64>)> {
        NAMES
            .iter()
            .zip(COUNTERS.iter())
            .zip(super::WIRED.iter())
            .map(|((name, counter), wired)| (*name, wired.then(|| counter.load(Ordering::Relaxed))))
            .collect()
    }
}

/// Call-site attribution of name-table traffic (measurement builds only):
/// which callers intern text keys or construct names from text.
#[cfg(feature = "perf-counters")]
mod sites {
    use std::collections::HashMap;
    use std::panic::Location;
    use std::sync::Mutex;

    static SITES: Mutex<Option<HashMap<&'static Location<'static>, u64>>> = Mutex::new(None);

    pub fn note(location: &'static Location<'static>) {
        let mut sites = SITES.lock().expect("name sites poisoned");
        *sites
            .get_or_insert_with(HashMap::new)
            .entry(location)
            .or_insert(0) += 1;
    }

    pub fn snapshot() -> Vec<(String, u64)> {
        let sites = SITES.lock().expect("name sites poisoned");
        let mut rows: Vec<(String, u64)> = sites
            .as_ref()
            .map(|sites| sites.iter().map(|(l, n)| (l.to_string(), *n)).collect())
            .unwrap_or_default();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        rows
    }
}

/// Record the caller of a name-table entry point (no-op without the feature).
#[inline(always)]
pub fn name_site(location: &'static std::panic::Location<'static>) {
    #[cfg(feature = "perf-counters")]
    sites::note(location);
    #[cfg(not(feature = "perf-counters"))]
    let _ = location;
}

/// Name-table call sites with their counts, most frequent first; empty
/// without the feature.
pub fn name_sites() -> Vec<(String, u64)> {
    #[cfg(feature = "perf-counters")]
    {
        sites::snapshot()
    }
    #[cfg(not(feature = "perf-counters"))]
    {
        Vec::new()
    }
}

/// The two production entry sites and recursive union expansion. Only
/// measurement builds classify calls or update the length histograms.
#[cfg(feature = "perf-counters")]
#[derive(Clone, Copy)]
pub enum UnionCollectionSite {
    Tables,
    Checker,
    Recursive,
}

/// Count source-slice lengths at the two outer collection sites; recursive
/// calls count the expanded union's member-slice length, without being
/// counted again as outer calls. Sum a site's buckets for its call count.
#[cfg(feature = "perf-counters")]
pub fn union_collection(site: UnionCollectionSite, length: usize) {
    use PerfCounter::*;
    let counters = match site {
        UnionCollectionSite::Tables => [
            UnionCollectionTables01,
            UnionCollectionTables2To8,
            UnionCollectionTables9To12,
            UnionCollectionTables13To20,
            UnionCollectionTables21To64,
            UnionCollectionTables65To256,
            UnionCollectionTables257Plus,
        ],
        UnionCollectionSite::Checker => [
            UnionCollectionChecker01,
            UnionCollectionChecker2To8,
            UnionCollectionChecker9To12,
            UnionCollectionChecker13To20,
            UnionCollectionChecker21To64,
            UnionCollectionChecker65To256,
            UnionCollectionChecker257Plus,
        ],
        UnionCollectionSite::Recursive => [
            UnionCollectionRecursive01,
            UnionCollectionRecursive2To8,
            UnionCollectionRecursive9To12,
            UnionCollectionRecursive13To20,
            UnionCollectionRecursive21To64,
            UnionCollectionRecursive65To256,
            UnionCollectionRecursive257Plus,
        ],
    };
    let bucket = match length {
        0..=1 => 0,
        2..=8 => 1,
        9..=12 => 2,
        13..=20 => 3,
        21..=64 => 4,
        65..=256 => 5,
        _ => 6,
    };
    bump(counters[bucket]);
}

/// Count outer GST comparisons by the number of flattened members supplied
/// to the sort, including duplicate occurrences and identity comparisons.
#[cfg(feature = "perf-counters")]
pub(crate) fn union_sort_comparison(length: usize) {
    use PerfCounter::*;
    bump(match length {
        0..=1 => UnionSortComparisons01,
        2..=8 => UnionSortComparisons2To8,
        9..=12 => UnionSortComparisons9To12,
        13..=20 => UnionSortComparisons13To20,
        21..=64 => UnionSortComparisons21To64,
        65..=256 => UnionSortComparisons65To256,
        _ => UnionSortComparisons257Plus,
    });
}

#[cfg(feature = "perf-counters")]
std::thread_local! {
    static TYPE_COMPARE_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A measurement-only scope spanning comparator recursion, including calls
/// back through the checker-owned mapper comparison.
#[cfg(feature = "perf-counters")]
pub(crate) struct TypeComparisonScope;

#[cfg(feature = "perf-counters")]
impl Drop for TypeComparisonScope {
    fn drop(&mut self) {
        TYPE_COMPARE_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

#[cfg(feature = "perf-counters")]
pub(crate) fn type_comparison_scope() -> TypeComparisonScope {
    let recursive = TYPE_COMPARE_DEPTH.with(|depth| {
        let previous = depth.get();
        depth.set(previous + 1);
        previous != 0
    });
    bump(if recursive {
        PerfCounter::TypeCompareRecursiveCalls
    } else {
        PerfCounter::TypeCompareOuterCalls
    });
    TypeComparisonScope
}

/// Whether counters are compiled in.
pub const fn enabled() -> bool {
    cfg!(feature = "perf-counters")
}

/// Increment `counter` by one (no-op without the feature).
#[inline(always)]
pub fn bump(counter: PerfCounter) {
    #[cfg(feature = "perf-counters")]
    enabled::bump(counter);
    #[cfg(not(feature = "perf-counters"))]
    let _ = counter;
}

/// Increment `counter` by `amount` (no-op without the feature).
#[inline(always)]
pub fn add(counter: PerfCounter, amount: u64) {
    #[cfg(feature = "perf-counters")]
    enabled::add(counter, amount);
    #[cfg(not(feature = "perf-counters"))]
    let _ = (counter, amount);
}

/// Every counter in report order with its value, or `None` when the
/// counter has no instrumented site (reported as `unwired`); empty without
/// the feature.
pub fn snapshot() -> Vec<(&'static str, Option<u64>)> {
    #[cfg(feature = "perf-counters")]
    {
        enabled::snapshot()
    }
    #[cfg(not(feature = "perf-counters"))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cover_every_counter() {
        assert_eq!(NAMES.len(), COUNT);
        assert_eq!(WIRED.len(), COUNT);
        assert_eq!(NAMES[0], "links.node.reads");
        assert_eq!(NAMES[COUNT - 1], "names.escape.calls");
    }

    #[test]
    fn counters_are_inert_or_additive() {
        bump(PerfCounter::UnionLookups);
        add(PerfCounter::TypeListIdBytes, 7);
        let rows = snapshot();
        if enabled() {
            assert_eq!(rows.len(), COUNT);
            let value = |wanted: &str| {
                rows.iter()
                    .find(|(name, _)| *name == wanted)
                    .map(|(_, v)| *v)
            };
            assert!(value("union.lookups").flatten().is_some_and(|v| v >= 1));
            assert!(value("type_list_id.bytes")
                .flatten()
                .is_some_and(|v| v >= 7));
            // An unwired counter reports None, never zero.
            assert_eq!(value("printer.plan_nodes"), Some(None));
        } else {
            assert!(rows.is_empty());
        }
    }
}
