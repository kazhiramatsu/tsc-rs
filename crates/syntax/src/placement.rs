//! Deterministic dense syntax placement. The scheduler owns
//! ordered leasing and dispatch; this module owns a sequential stripe cursor.
use super::{NodeArena, SyntaxLeases};
use crate::{parser, ParseOptions, SourceFile};
use std::{cell::RefCell, sync::Arc};
use tsc_diagnostics::{JsString, TextSnapshot};
use tsc_store::StoreVec;
use tsc_types::identity::direct::Rooms;
use tsc_types::{IdentityDomain, IdentityError, IdentityRange, IdentitySpace};

thread_local! {
    static PENDING: RefCell<Option<NodeArena>> = const { RefCell::new(None) };
}
struct PendingGuard;
impl Drop for PendingGuard {
    fn drop(&mut self) {
        PENDING.with(|slot| slot.borrow_mut().take());
    }
}

pub const SPACES: [IdentitySpace; 4] = [
    IdentitySpace::Node,
    IdentitySpace::NodeArray,
    IdentitySpace::NodeArrayItem,
    IdentitySpace::NodeText,
];

/// Count policy independent of both worker and checker budgets.
pub fn stripe_width(entries: usize) -> usize {
    entries.div_ceil(96).max(8)
}

/// Empirical library/root corpus bounds with a 20% margin, rounded upward
/// to a multiple of 1/32. Each entry also contributes 16 units of slack.
/// Exact overflow fallback remains necessary for texts outside that corpus.
pub fn bounds(bytes: usize, entries: usize) -> Option<Vec<(IdentitySpace, u32)>> {
    let bytes = u64::try_from(bytes).ok()?;
    let margin = u64::try_from(entries).ok()?.checked_mul(16)?;
    SPACES
        .into_iter()
        .zip([9, 3, 3, 16])
        .map(|(space, factor)| {
            let count = bytes
                .checked_mul(factor)?
                .div_ceil(32)
                .checked_add(margin)?;
            u32::try_from(count).ok().map(|n| (space, n))
        })
        .collect()
}

fn syntax_leases(leases: Vec<tsc_types::IdentityLease>) -> SyntaxLeases {
    let mut leases = leases.into_iter();
    SyntaxLeases {
        nodes: leases.next().expect("nodes"),
        arrays: leases.next().expect("arrays"),
        array_items: leases.next().expect("items"),
        texts: leases.next().expect("texts"),
    }
}
fn lease_counts(domain: &IdentityDomain, counts: [u32; 4]) -> Result<SyntaxLeases, IdentityError> {
    domain
        .lease_batch(&SPACES.into_iter().zip(counts).collect::<Vec<_>>())
        .map(syntax_leases)
}

impl NodeArena {
    pub fn new() -> Self {
        PENDING
            .with(|slot| slot.borrow_mut().take())
            .unwrap_or_default()
    }
    fn at_ranges(ranges: &[IdentityRange]) -> Self {
        let [nodes, arrays, items, texts] = ranges else {
            panic!("four syntax spaces")
        };
        Self {
            nodes: StoreVec::claim_room(nodes.start() + 1, nodes.len()),
            arrays: StoreVec::claim_room(arrays.start() + 1, arrays.len()),
            array_items: StoreVec::claim_room(items.start(), items.len()),
            texts: StoreVec::claim_room(texts.start() + 1, texts.len()),
            node_base: nodes.start(),
            array_base: arrays.start(),
            item_base: items.start(),
            text_base: texts.start(),
            ..Self::default()
        }
    }
    fn finish_reserved(&mut self) -> bool {
        if !self.nodes.is_published()
            || !self.arrays.is_published()
            || !self.array_items.is_published()
            || !self.texts.is_published()
        {
            return false;
        }
        self.nodes.publish(self.node_base + 1);
        self.arrays.publish(self.array_base + 1);
        self.array_items.publish(self.item_base);
        self.texts.publish(self.text_base + 1);
        true
    }
    fn placement_counts(&self) -> [u32; 4] {
        [
            self.nodes.len(),
            self.arrays.len(),
            self.array_items.len(),
            self.texts.len(),
        ]
        .map(|n| u32::try_from(n).expect("parser store exceeds u32"))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Statistics {
    pub direct_sources: usize,
    pub direct_bytes: usize,
    pub fallbacks: usize,
    pub stripes: usize,
    pub lease_batches: usize,
    pub holes: [u64; 4],
    /// Logical initialized-page overhead at 16 KiB, NOT resident memory.
    pub page_end_bytes: usize,
}

/// Immutable provenance; the exact views keep dropped trees' input-assigned
/// positions reserved until the command has finished adopting its sources.
#[derive(Clone, Debug)]
pub struct Placement {
    pub name: JsString,
    pub bytes: usize,
    leases: SyntaxLeases,
}
impl Placement {
    pub fn ranges(&self) -> [IdentityRange; 4] {
        [
            &self.leases.nodes,
            &self.leases.arrays,
            &self.leases.array_items,
            &self.leases.texts,
        ]
        .map(|lease| lease.range())
    }
    pub fn matches(&self, source: &SourceFile) -> bool {
        source.arena.is_published()
            && source.arena.identity_leases().is_some_and(|leases| {
                leases.nodes == self.leases.nodes
                    && leases.arrays == self.leases.arrays
                    && leases.array_items == self.leases.array_items
                    && leases.texts == self.leases.texts
            })
    }
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AdoptionStatistics {
    pub planned_sources: usize,
    pub adopted_sources: usize,
    pub dropped_sources: usize,
    pub dropped_bytes: usize,
    pub dropped_ids: [u64; 4],
    /// Pages initialized by a placed tree with no final program syntax in
    /// them, assuming 16-KiB pages. Logical guide, not measured residency.
    pub dropped_only_pages_guide: [u64; 4],
}
pub fn adoption_statistics<'a>(
    layout: &[Placement],
    sources: impl IntoIterator<Item = &'a SourceFile>,
) -> AdoptionStatistics {
    let key = |ranges: [IdentityRange; 4]| ranges.map(|range| (range.start(), range.end()));
    let actual: std::collections::HashMap<_, _> = sources
        .into_iter()
        .filter_map(|source| {
            if !source.arena.is_published() {
                return None;
            }
            let held = source.arena.identity_leases()?;
            Some((
                key([&held.nodes, &held.arrays, &held.array_items, &held.texts]
                    .map(|lease| lease.range())),
                source,
            ))
        })
        .collect();
    let mut statistics = AdoptionStatistics {
        planned_sources: layout.len(),
        ..Default::default()
    };
    for placement in layout {
        if actual
            .get(&key(placement.ranges()))
            .is_some_and(|source| placement.matches(source))
        {
            statistics.adopted_sources += 1;
        } else {
            statistics.dropped_sources += 1;
            statistics.dropped_bytes += placement.bytes;
            for (sum, range) in statistics.dropped_ids.iter_mut().zip(placement.ranges()) {
                *sum += u64::from(range.len());
            }
        }
    }
    let widths = [
        std::mem::size_of::<crate::Node>(),
        std::mem::size_of::<crate::nodes::NodeArrayRecord>(),
        std::mem::size_of::<crate::NodeId>(),
        std::mem::size_of::<tsc_store::texts::TextWord>(),
    ];
    for (space, width) in widths.into_iter().enumerate() {
        let pages = |range: IdentityRange| {
            let offset = u64::from(space != 2);
            let first = (u64::from(range.start()) + offset) * width as u64;
            let end = (u64::from(range.end()) + offset) * width as u64;
            (first / 16384, end.div_ceil(16384))
        };
        let touched = merge_pages(
            layout
                .iter()
                .map(|placement| placement.ranges()[space])
                .filter(|r| !r.is_empty())
                .map(pages)
                .collect(),
        );
        let live = merge_pages(
            actual
                .keys()
                .filter_map(|ranges| {
                    let (a, b) = ranges[space];
                    (a != b).then(|| pages(IdentityRange::new(a, b)))
                })
                .collect(),
        );
        let mut next_live = 0;
        for (a, b) in touched {
            let mut cursor = a;
            while next_live < live.len() && live[next_live].1 <= a {
                next_live += 1;
            }
            let mut i = next_live;
            while i < live.len() && live[i].0 < b {
                let (start, end) = live[i];
                statistics.dropped_only_pages_guide[space] += start.min(b).saturating_sub(cursor);
                cursor = cursor.max(end).min(b);
                i += 1;
            }
            statistics.dropped_only_pages_guide[space] += b.saturating_sub(cursor);
        }
    }
    statistics
}
fn merge_pages(mut pages: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    pages.sort_unstable();
    let mut result: Vec<(u64, u64)> = Vec::new();
    for (start, end) in pages {
        if let Some(previous) = result.last_mut() {
            if start <= previous.1 {
                previous.1 = previous.1.max(end);
                continue;
            }
        }
        result.push((start, end));
    }
    result
}

/// A scheduler can move this value to any existing worker. Entries within
/// it must run sequentially in the input order. Overflow is sticky: the
/// current and every remaining entry return a PRIVATE tree for exact leasing
/// later in Program order. Parsing never takes another domain lease.
pub struct SyntaxStripe {
    rooms: Rooms,
    exhausted: bool,
    initial: Vec<IdentityRange>,
    placements: Vec<Placement>,
    pub statistics: Statistics,
}
impl SyntaxStripe {
    pub fn new(rooms: Rooms) -> Self {
        assert_eq!(
            rooms
                .remaining()
                .iter()
                .map(|(s, _)| *s)
                .collect::<Vec<_>>(),
            SPACES
        );
        let initial: Vec<IdentityRange> = rooms
            .remaining()
            .into_iter()
            .map(|(_, range)| range)
            .collect();
        let exhausted = initial[0].is_empty();
        Self {
            rooms,
            initial,
            exhausted,
            placements: Vec::new(),
            statistics: Statistics {
                stripes: 1,
                ..Statistics::default()
            },
        }
    }
    /// Clone these views into the scheduler's immutable adoption ledger before
    /// consuming the stripe. No fresh domain lease is taken.
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }
    pub fn parse(
        &mut self,
        file_name: JsString,
        snapshot: Arc<TextSnapshot>,
        mut options: ParseOptions,
    ) -> SourceFile {
        options.node_id_base = 0;
        options.node_array_id_base = 0;
        if self.exhausted {
            self.statistics.fallbacks += 1;
            return parser::parse_source_file_from_snapshot(file_name, snapshot, options, None);
        }
        let ranges: Vec<_> = self
            .rooms
            .remaining()
            .into_iter()
            .map(|(_, range)| range)
            .collect();
        PENDING.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "nested reserved parse");
            *slot = Some(NodeArena::at_ranges(&ranges));
        });
        let _guard = PendingGuard;
        let bytes = snapshot.text().len();
        let mut source =
            parser::parse_source_file_from_snapshot(file_name, snapshot, options, None);
        if source.arena.finish_reserved() {
            source.arena.leases = Some(syntax_leases(
                self.rooms
                    .finish_entry(&source.arena.placement_counts())
                    .expect("sealed counts fit room"),
            ));
            self.statistics.direct_sources += 1;
            self.statistics.direct_bytes += bytes;
            self.placements.push(Placement {
                name: source.file_name.clone(),
                bytes,
                leases: source
                    .arena
                    .identity_leases()
                    .expect("placed source leases")
                    .clone(),
            });
        } else {
            source.arena = source.arena.detached_clone();
            self.exhausted = true;
            self.statistics.fallbacks += 1;
        }
        source
    }
    /// Call in stripe order after all stripes have been leased and joined.
    pub fn seal(mut self) -> Result<Statistics, IdentityError> {
        for (index, (_, range)) in self.rooms.unused().into_iter().enumerate() {
            self.statistics.holes[index] = u64::from(range.len());
            let widths = [
                std::mem::size_of::<crate::Node>(),
                std::mem::size_of::<crate::nodes::NodeArrayRecord>(),
                std::mem::size_of::<crate::NodeId>(),
                std::mem::size_of::<tsc_store::texts::TextWord>(),
            ];
            let count = (range.start() - self.initial[index].start()) as usize;
            if count != 0 {
                let offset = usize::from(index != 2);
                let start = (self.initial[index].start() as usize + offset) * widths[index];
                let bytes = count * widths[index];
                self.statistics.page_end_bytes +=
                    (start % 16384 + bytes).div_ceil(16384) * 16384 - bytes;
            }
        }
        self.rooms.seal()?;
        Ok(self.statistics)
    }
}

impl SourceFile {
    pub fn lease_identities(&self, domain: &IdentityDomain) -> Result<SyntaxLeases, IdentityError> {
        if let Some(leases) = self.arena.identity_leases() {
            if self.arena.is_published() && leases.nodes.belongs_to(domain) {
                return Ok(leases.clone());
            }
        }
        let count = |len: usize, space| {
            u32::try_from(len).map_err(|_| IdentityError::Exhausted {
                space,
                requested: u32::MAX,
                limit: u32::MAX,
            })
        };
        lease_counts(
            domain,
            [
                count(self.arena.nodes().len(), IdentitySpace::Node)?,
                count(self.arena.node_arrays().len(), IdentitySpace::NodeArray)?,
                count(self.arena.array_item_count(), IdentitySpace::NodeArrayItem)?,
                count(self.arena.text_word_count(), IdentitySpace::NodeText)?,
            ],
        )
    }

    pub fn relocate_with_leases(&mut self, leases: SyntaxLeases) -> Result<(), IdentityError> {
        if let Some(held) = self.arena.identity_leases() {
            if self.arena.is_published()
                && held.nodes == leases.nodes
                && held.arrays == leases.arrays
                && held.array_items == leases.array_items
                && held.texts == leases.texts
            {
                return Ok(());
            }
            // Harness sessions can adopt a loader parse into a private domain.
            // Their fallback retains the usual domain checks and exact leases.
            self.arena = self.arena.detached_clone();
        }
        let relocation = self.arena.identity_relocation(&leases)?;
        relocation.node(&mut self.root)?;
        if let Some(indicator) = &mut self.external_module_indicator {
            relocation.node(indicator)?;
        }
        self.arena.apply_identity_relocation(relocation, leases)
    }
}

pub fn library_metadata(text: &str) -> Option<(bool, Vec<crate::FileReference>)> {
    parser::direct_library_metadata(text)
}

pub fn trace_statistics(label: &str, statistics: &Statistics) {
    if tsc_types::trace::enabled() {
        eprintln!("[syntax-placement] {label}: direct_sources={} direct_bytes={} fallbacks={} stripes={} lease_batches={} holes={:?} page_end_bytes_guide={}",
            statistics.direct_sources, statistics.direct_bytes, statistics.fallbacks,
            statistics.stripes, statistics.lease_batches, statistics.holes, statistics.page_end_bytes);
    }
}

/// Leading library directives without constructing a syntax tree.
pub fn leading_library_references(text: &str) -> Vec<crate::FileReference> {
    parser::leading_library_references(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{published_node, published_node_array, source_files_structurally_equal};
    use tsc_diagnostics::DocumentVersion;

    const TEXT: &str = "export class C { #x = 'hello'; f<T>(x: T) { return [x, this.#x]; } }";
    fn snapshot(text: &str) -> Arc<TextSnapshot> {
        TextSnapshot::new(text.to_owned(), DocumentVersion::default())
    }
    fn stripe(domain: &IdentityDomain, counts: [u32; 4]) -> SyntaxStripe {
        SyntaxStripe::new(
            domain
                .lease_rooms(&[SPACES.into_iter().zip(counts).collect()])
                .unwrap()
                .pop()
                .unwrap(),
        )
    }
    fn check_published(source: &SourceFile) {
        assert!(source.arena.is_published());
        for id in source.arena.node_ids() {
            assert!(std::ptr::eq(source.arena.node(id), published_node(id)));
        }
        for index in 0..source.arena.node_arrays().len() as u32 {
            let id = crate::NodeArrayId::new(source.arena.array_base() + index);
            assert_eq!(source.arena.node_array(id), published_node_array(id));
        }
    }
    #[test]
    fn consecutive_sources_are_dense_and_adoption_keeps_final_addresses() {
        let domain = IdentityDomain::new();
        let mut stripe = stripe(&domain, [1024; 4]);
        let mut sources: Vec<_> = (0..3)
            .map(|_| stripe.parse("direct.ts".into(), snapshot(TEXT), ParseOptions::default()))
            .collect();
        for pair in sources.windows(2) {
            let left = pair[0].arena.identity_leases().unwrap();
            let right = pair[1].arena.identity_leases().unwrap();
            for (a, b) in [
                (&left.nodes, &right.nodes),
                (&left.arrays, &right.arrays),
                (&left.array_items, &right.array_items),
                (&left.texts, &right.texts),
            ] {
                assert_eq!(a.range().end(), b.range().start());
            }
        }
        let stats = stripe.seal().unwrap();
        assert_eq!(stats.direct_sources, 3);
        assert_eq!(stats.fallbacks, 0);
        for source in &mut sources {
            check_published(source);
            let address = source.arena.node(source.root) as *const _;
            source
                .relocate_with_leases(source.lease_identities(&domain).unwrap())
                .unwrap();
            assert_eq!(address, source.arena.node(source.root) as *const _);
            assert!(source_files_structurally_equal(
                source,
                &crate::parse_source_file("direct.ts", TEXT, ParseOptions::default(), None)
            ));
        }
    }
    #[test]
    fn every_space_overflow_is_sticky_and_defers_exact_leases() {
        for space in 0..4 {
            let domain = IdentityDomain::new();
            let mut counts = [1024; 4];
            counts[space] = 1;
            let mut stripe = stripe(&domain, counts);
            let mut sources: Vec<_> = (0..3)
                .map(|_| {
                    stripe.parse(
                        "overflow.ts".into(),
                        snapshot(TEXT),
                        ParseOptions::default(),
                    )
                })
                .collect();
            let stats = stripe.seal().unwrap();
            assert_eq!(stats.fallbacks, 3, "space {space}");
            for source in &mut sources {
                assert!(!source.arena.has_identity_leases());
                assert!(!source.arena.is_published());
                assert!(source_files_structurally_equal(
                    source,
                    &crate::parse_source_file("overflow.ts", TEXT, ParseOptions::default(), None)
                ));
                source.relocate_into_identity_domain(&domain).unwrap();
                check_published(source);
            }
        }
    }
    #[test]
    fn scratch_reservation_hints_do_not_spill_an_actual_fitting_room() {
        let domain = IdentityDomain::new();
        let mut stripe = stripe(&domain, [32; 4]);
        let text = format!("/*{}*/ const value = 1;", "comment".repeat(1024));
        let source = stripe.parse(
            "comments.ts".into(),
            snapshot(&text),
            ParseOptions::default(),
        );
        check_published(&source);
        let stats = stripe.seal().unwrap();
        assert_eq!(stats.direct_sources, 1);
        assert_eq!(stats.fallbacks, 0);
        assert!(source_files_structurally_equal(
            &source,
            &crate::parse_source_file("comments.ts", text, ParseOptions::default(), None)
        ));
    }

    #[test]
    fn policy_does_not_read_worker_budget() {
        assert_eq!(stripe_width(0), 8);
        assert_eq!(stripe_width(768), 8);
        assert_eq!(stripe_width(769), 9);
        assert_eq!(stripe_width(10196), 107);
    }
}

#[cfg(test)]
mod adoption_tests {
    use super::*;
    use tsc_diagnostics::{DocumentVersion, TextSnapshot};

    #[test]
    fn lost_tree_views_pin_their_positions_and_count_only_pages_without_live_syntax() {
        let texts = [
            "export const x = [".to_owned() + &"1,".repeat(4000) + "];",
            "export const y = 2;".to_owned(),
        ];
        let snapshots = texts.map(|text| TextSnapshot::new(text, DocumentVersion::default()));
        let local: Vec<_> = snapshots
            .iter()
            .enumerate()
            .map(|(i, snapshot)| {
                parser::parse_source_file_from_snapshot(
                    format!("/{i}.ts").into(),
                    Arc::clone(snapshot),
                    ParseOptions::default(),
                    None,
                )
            })
            .collect();
        let mut counts = [0; 4];
        for source in &local {
            for (sum, count) in counts.iter_mut().zip(source.arena.placement_counts()) {
                *sum += count;
            }
        }
        let domain = IdentityDomain::new();
        let mut stripe = SyntaxStripe::new(
            domain
                .lease_rooms(&[SPACES.into_iter().zip(counts).collect()])
                .unwrap()
                .pop()
                .unwrap(),
        );
        let mut sources: Vec<_> = snapshots
            .iter()
            .enumerate()
            .map(|(i, snapshot)| {
                stripe.parse(
                    format!("/{i}.ts").into(),
                    Arc::clone(snapshot),
                    ParseOptions::default(),
                )
            })
            .collect();
        let layout = stripe.placements().to_vec();
        assert_eq!(layout.len(), 2);
        stripe.seal().unwrap();
        let all = adoption_statistics(&layout, sources.iter());
        assert_eq!(
            (
                all.planned_sources,
                all.adopted_sources,
                all.dropped_sources
            ),
            (2, 2, 0)
        );
        assert_eq!(all.dropped_only_pages_guide, [0; 4]);
        let first = sources.remove(0);
        let first_ranges = first.arena.identity_leases().unwrap().clone();
        drop(first);
        drop(first_ranges);
        let lost = adoption_statistics(&layout, sources.iter());
        assert_eq!((lost.adopted_sources, lost.dropped_sources), (1, 1));
        assert_eq!(
            lost.dropped_ids,
            layout[0].ranges().map(|range| u64::from(range.len()))
        );
        assert!(lost.dropped_only_pages_guide[0] > 0);
        let replacement = parser::parse_source_file_from_snapshot(
            "/0.ts".into(),
            Arc::clone(&snapshots[0]),
            ParseOptions::default(),
            None,
        );
        let leases = replacement.lease_identities(&domain).unwrap();
        assert!(
            !leases.nodes.range().overlaps(layout[0].ranges()[0]),
            "ledger pins a dropped tree's identities"
        );
    }
}
