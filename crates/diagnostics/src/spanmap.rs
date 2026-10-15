//! tsgo `spanmap`: the mapping between a content mapper's virtual text and
//! the original text it was produced from.
//!
//! Unlike a source map, which records point correspondences, a span map
//! records explicit segments for the parts of the virtual text that
//! correspond to the original; positions outside every segment are
//! synthesized. Positions are the compiler's (UTF-16 code units here, UTF-8
//! bytes in tsgo): the content mapper host normalizes a mapper's positions
//! before it builds a map.

use std::fmt;
use std::sync::OnceLock;

/// tsgo `spanmap.Kind`: how positions inside a segment relate its virtual
/// span to its original span. A segment keeps the value it was given, so a
/// map with an unknown kind is reported by [`SpanMap::validate`].
pub type SegmentKind = i32;

/// Length-preserving: interior positions map one to one.
pub const KIND_VERBATIM: SegmentKind = 0;
/// The virtual span maps to the original span as a whole; interior
/// positions clamp to its ends (renamed identifiers, short expressions).
pub const KIND_ATOM: SegmentKind = 1;
/// An atom whose virtual and original texts name the same entity: a
/// diagnostic may name it by its original text.
pub const KIND_ALIAS: SegmentKind = 2;

/// tsgo `spanmap.Feature`: the language-service operations a segment takes
/// part in. Diagnostics never consult them.
pub type Features = i32;

pub const FEATURE_HOVER: Features = 1;
pub const FEATURE_SIGNATURE_HELP: Features = 1 << 1;
pub const FEATURE_COMPLETION: Features = 1 << 2;
pub const FEATURE_DEFINITION: Features = 1 << 3;
pub const FEATURE_TYPE_DEFINITION: Features = 1 << 4;
pub const FEATURE_IMPLEMENTATION: Features = 1 << 5;
pub const FEATURE_REFERENCES: Features = 1 << 6;
pub const FEATURE_DOCUMENT_HIGHLIGHTS: Features = 1 << 7;
pub const FEATURE_RENAME: Features = 1 << 8;
pub const FEATURE_CALL_HIERARCHY: Features = 1 << 9;
pub const FEATURE_CODE_ACTIONS: Features = 1 << 10;
pub const FEATURE_FORMATTING: Features = 1 << 11;
pub const FEATURE_INLAY_HINTS: Features = 1 << 12;
pub const FEATURE_SEMANTIC_TOKENS: Features = 1 << 13;
pub const FEATURE_FOLDING_RANGES: Features = 1 << 14;
pub const FEATURE_SELECTION_RANGES: Features = 1 << 15;
pub const FEATURE_LINKED_EDITING: Features = 1 << 16;
pub const FEATURE_AUTO_INSERT: Features = 1 << 17;
pub const FEATURE_DOCUMENT_SYMBOLS: Features = 1 << 18;
pub const FEATURE_CODE_LENS: Features = 1 << 19;
/// No feature (`FeatureNone`).
pub const FEATURES_NONE: Features = 0;
/// Every feature tsgo defines (`FeatureAll`, 20 flags).
pub const FEATURES_ALL: Features = (1 << 20) - 1;

/// tsgo `spanmap.Fidelity`: how faithfully a mapped span reflects the
/// original.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fidelity {
    /// Within one verbatim segment: maps exactly.
    Exact,
    /// Within one atom segment: maps to the atom's span.
    Atom,
    /// Across segment boundaries: the ends were mapped and clamped.
    Approximate,
    /// No original counterpart: synthesized.
    None,
}

impl Fidelity {
    pub const fn is_exact(self) -> bool {
        matches!(self, Self::Exact)
    }

    pub const fn is_single_segment(self) -> bool {
        matches!(self, Self::Exact | Self::Atom)
    }

    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

/// tsgo `spanmap.Segment`: the half-open virtual range
/// `[virtual_start, virtual_end)` maps to the half-open original range
/// `[original_start, original_end)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Segment {
    pub virtual_start: i32,
    pub virtual_end: i32,
    pub original_start: i32,
    pub original_end: i32,
    pub kind: SegmentKind,
    pub features: Features,
}

/// tsgo `spanmap.MappedPosition`: a projection of a position and how
/// faithful it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MappedPosition {
    pub position: i32,
    pub fidelity: Fidelity,
}

/// tsgo `spanmap.MappedSpan`: a projection of a range `[pos, end)` and how
/// faithful it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MappedSpan {
    pub pos: i32,
    pub end: i32,
    pub fidelity: Fidelity,
}

/// tsgo `spanmap.MappingErrorKind`: how a mapper's span map is malformed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingErrorKind {
    /// The segments overlap, run backwards or pass the virtual text's end.
    Overlap,
    /// A segment's original span lies outside the original text.
    OutOfBounds,
    /// A verbatim segment's virtual and original texts differ.
    VerbatimMismatch,
    /// A segment has an unknown kind.
    Kind,
    /// A segment has an unknown feature flag.
    Feature,
}

/// tsgo `spanmap.MappingError`: the first violation [`SpanMap::validate`]
/// finds, with the offsets that locate it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MappingError {
    pub kind: MappingErrorKind,
    pub virtual_pos: i32,
    pub original_pos: i32,
}

impl fmt::Display for MappingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            MappingErrorKind::Overlap => write!(
                formatter,
                "content mapper position mappings overlap or are out of order near virtual offset {}",
                self.virtual_pos
            ),
            MappingErrorKind::OutOfBounds => write!(
                formatter,
                "content mapper position mapping points outside the original content at original offset {}",
                self.original_pos
            ),
            MappingErrorKind::VerbatimMismatch => write!(
                formatter,
                "content mapper verbatim mapping does not match the original content at virtual offset {}, original offset {}",
                self.virtual_pos, self.original_pos
            ),
            MappingErrorKind::Kind => write!(
                formatter,
                "content mapper position mapping has an invalid kind at virtual offset {}",
                self.virtual_pos
            ),
            MappingErrorKind::Feature => write!(
                formatter,
                "content mapper position mappings have invalid features near original offset {}",
                self.original_pos
            ),
        }
    }
}

impl std::error::Error for MappingError {}

/// tsgo `spanmap.SpanMap`: segments ordered by virtual start. A position in
/// no segment is synthesized, so an empty map describes fully synthesized
/// text. A file without a map (`None` where one is optional) maps
/// identically.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpanMap {
    segments: Vec<Segment>,
    original_index: OriginalIndexCell,
}

/// The original-text interval index, built on first use (tsgo `origOnce`).
/// It is derived from the segments: it takes no part in a map's equality,
/// and a clone builds its own.
#[derive(Default)]
struct OriginalIndexCell(OnceLock<OriginalIndex>);

impl Clone for OriginalIndexCell {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for OriginalIndexCell {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for OriginalIndexCell {}

impl fmt::Debug for OriginalIndexCell {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OriginalIndexCell")
    }
}

impl SpanMap {
    /// tsgo `spanmap.New`: the segments ordered by virtual start (a stable
    /// sort, as tsgo's `slices.SortFunc` over its comparison is applied to
    /// segments that are distinct in practice).
    pub fn new(mut segments: Vec<Segment>) -> Self {
        segments.sort_by_key(|segment| segment.virtual_start);
        Self {
            segments,
            original_index: OriginalIndexCell::default(),
        }
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// tsgo `SpanMap.Validate`: the first way the map breaks the content
    /// mapper contract over its texts, given as UTF-16 code units: the
    /// segments ordered and disjoint in virtual space and within the virtual
    /// text, every original span within the original text, every verbatim
    /// segment's texts equal, and only known kinds and features. Gaps are
    /// allowed and an empty map is valid.
    pub fn validate(&self, virtual_text: &[u16], original_text: &[u16]) -> Option<MappingError> {
        let virtual_len = virtual_text.len() as i64;
        let original_len = original_text.len() as i64;
        let mut previous_virtual_end = 0i64;
        for segment in &self.segments {
            let (virtual_start, virtual_end) = (
                i64::from(segment.virtual_start),
                i64::from(segment.virtual_end),
            );
            let (original_start, original_end) = (
                i64::from(segment.original_start),
                i64::from(segment.original_end),
            );
            if virtual_start < previous_virtual_end
                || virtual_end < virtual_start
                || virtual_end > virtual_len
            {
                return Some(segment.error(MappingErrorKind::Overlap, 0));
            }
            previous_virtual_end = virtual_end;
            if original_start < 0 || original_end < original_start || original_end > original_len {
                return Some(segment.error(MappingErrorKind::OutOfBounds, segment.original_end));
            }
            if !matches!(segment.kind, KIND_VERBATIM | KIND_ATOM | KIND_ALIAS) {
                return Some(segment.error(MappingErrorKind::Kind, segment.original_start));
            }
            if segment.kind == KIND_VERBATIM
                && (virtual_end - virtual_start != original_end - original_start
                    || virtual_text[virtual_start as usize..virtual_end as usize]
                        != original_text[original_start as usize..original_end as usize])
            {
                return Some(
                    segment.error(MappingErrorKind::VerbatimMismatch, segment.original_start),
                );
            }
            if segment.features & !FEATURES_ALL != 0 {
                return Some(segment.error(MappingErrorKind::Feature, segment.original_start));
            }
        }
        None
    }

    /// tsgo `SpanMap.VirtualToOriginalSpan`: the original range of the
    /// virtual range `[pos, end)` and how faithfully it maps. A range in a
    /// gap maps to the gap's insertion point, synthesized.
    pub fn virtual_to_original_span(&self, pos: i32, end: i32) -> ((i32, i32), Fidelity) {
        let virtual_start = pos;
        let virtual_end = end.max(virtual_start);
        if virtual_start == virtual_end {
            let (position, fidelity) = self.virtual_to_original_position(virtual_start);
            return ((position, position), fidelity);
        }
        let (start_index, start_in) = self.segment_index_at(virtual_start);
        let (end_index, end_in) = self.segment_index_at(virtual_end - 1);
        if start_index == end_index && start_in == end_in {
            if start_in {
                let segment = &self.segments[start_index as usize];
                if segment.kind == KIND_VERBATIM {
                    let original_start = clamp(
                        segment.original_start + (virtual_start - segment.virtual_start),
                        segment.original_start,
                        segment.original_end,
                    );
                    let original_end = clamp(
                        segment.original_start + (virtual_end - segment.virtual_start),
                        original_start,
                        segment.original_end,
                    );
                    return ((original_start, original_end), Fidelity::Exact);
                }
                return (
                    (segment.original_start, segment.original_end),
                    Fidelity::Atom,
                );
            }
            // Entirely within one synthesized gap.
            let position = self.insertion_point(start_index);
            return ((position, position), Fidelity::None);
        }
        let original_start = self.map_low(virtual_start, start_index, start_in);
        let original_end = self
            .map_high(virtual_end, end_index, end_in)
            .max(original_start);
        ((original_start, original_end), Fidelity::Approximate)
    }

    /// tsgo `SpanMap.VirtualToOriginalSpanForFeature`: the mapping of the
    /// virtual range, with no fidelity unless contiguous segments taking
    /// part in `feature` cover it (an empty range: its segment does).
    pub fn virtual_to_original_span_for_feature(
        &self,
        pos: i32,
        end: i32,
        feature: Features,
    ) -> ((i32, i32), Fidelity) {
        let (mapped, fidelity) = self.virtual_to_original_span(pos, end);
        if self.virtual_span_supports_feature(pos, end, feature) {
            return (mapped, fidelity);
        }
        (mapped, Fidelity::None)
    }

    /// tsgo `virtualSpanSupportsFeature`.
    fn virtual_span_supports_feature(&self, pos: i32, end: i32, feature: Features) -> bool {
        let start = pos;
        let end = end.max(start);
        let (index, inside) = self.segment_index_at(start);
        if start == end {
            return inside && self.segments[index as usize].features & feature != 0;
        }
        if !inside {
            return false;
        }
        let mut covered_through = start;
        let mut index = index as usize;
        while index < self.segments.len() && covered_through < end {
            let segment = &self.segments[index];
            if segment.virtual_start > covered_through
                || segment.virtual_end <= covered_through
                || segment.features & feature == 0
            {
                return false;
            }
            covered_through = segment.virtual_end;
            index += 1;
        }
        covered_through >= end
    }

    /// tsgo `SpanMap.VirtualToOriginalPositionForFeature`: the mapping of
    /// the virtual position, with no fidelity unless its segment takes part
    /// in `feature`.
    pub fn virtual_to_original_position_for_feature(
        &self,
        pos: i32,
        feature: Features,
    ) -> (i32, Fidelity) {
        let (mapped, fidelity) = self.virtual_to_original_position(pos);
        let (index, inside) = self.segment_index_at(pos);
        if !inside || self.segments[index as usize].features & feature == 0 {
            return (mapped, Fidelity::None);
        }
        (mapped, fidelity)
    }

    /// The map in UTF-8 bytes (tsgo's units, the language service's) of a
    /// map in UTF-16 code units (the compiler's) over its texts.
    pub fn in_utf8(&self, virtual_text: &str, original_text: &str) -> SpanMap {
        let virtual_offsets = Utf16ToUtf8::new(virtual_text);
        let original_offsets = Utf16ToUtf8::new(original_text);
        SpanMap::new(
            self.segments
                .iter()
                .map(|segment| Segment {
                    virtual_start: virtual_offsets.map(segment.virtual_start),
                    virtual_end: virtual_offsets.map(segment.virtual_end),
                    original_start: original_offsets.map(segment.original_start),
                    original_end: original_offsets.map(segment.original_end),
                    ..*segment
                })
                .collect(),
        )
    }

    /// tsgo `SpanMap.VirtualToOriginalPosition`: the original position of a
    /// virtual one; a position in a gap maps to the gap's insertion point,
    /// synthesized.
    pub fn virtual_to_original_position(&self, pos: i32) -> (i32, Fidelity) {
        let (index, inside) = self.segment_index_at(pos);
        if !inside {
            return (self.insertion_point(index), Fidelity::None);
        }
        let segment = &self.segments[index as usize];
        if segment.kind == KIND_VERBATIM {
            return (
                clamp(
                    segment.original_start + (pos - segment.virtual_start),
                    segment.original_start,
                    segment.original_end,
                ),
                Fidelity::Exact,
            );
        }
        (segment.original_start, Fidelity::Atom)
    }

    /// tsgo `SpanMap.VirtualToOriginalPositionExact`: the original position
    /// of a virtual one, and whether it is exact: inside a verbatim segment
    /// and not where one segment ends that the verbatim one does not
    /// continue in the original.
    pub fn virtual_to_original_position_exact(&self, pos: i32) -> (i32, bool) {
        let (mapped, fidelity) = self.virtual_to_original_position(pos);
        if fidelity != Fidelity::Exact {
            return (mapped, false);
        }
        let (index, inside) = self.segment_index_at(pos);
        if !inside || self.segments[index as usize].kind != KIND_VERBATIM {
            return (mapped, false);
        }
        if index > 0 {
            let segment = &self.segments[index as usize];
            let previous = &self.segments[index as usize - 1];
            if previous.virtual_end == pos
                && (previous.kind != KIND_VERBATIM
                    || previous.original_end != segment.original_start)
            {
                return (mapped, false);
            }
        }
        (mapped, true)
    }

    /// tsgo `SpanMap.AliasForVirtualSpan`: the alias segment that covers the
    /// virtual range `[pos, end)` exactly, if any.
    pub fn alias_for_virtual_span(&self, pos: i32, end: i32) -> Option<Segment> {
        let (index, inside) = self.segment_index_at(pos);
        if !inside {
            return None;
        }
        let segment = self.segments[index as usize];
        (segment.kind == KIND_ALIAS && pos == segment.virtual_start && end == segment.virtual_end)
            .then_some(segment)
    }

    /// The index of the segment containing `pos` and `true`; for a position
    /// in a gap, the index of the segment before it (-1 for none) and
    /// `false`. The last segment contains its end.
    fn segment_index_at(&self, pos: i32) -> (isize, bool) {
        match self
            .segments
            .binary_search_by(|segment| segment.virtual_start.cmp(&pos))
        {
            Ok(index) => (index as isize, true),
            Err(index) => {
                let previous = index as isize - 1;
                if previous >= 0 {
                    let segment = &self.segments[previous as usize];
                    if pos < segment.virtual_end
                        || (previous as usize == self.segments.len() - 1
                            && pos == segment.virtual_end)
                    {
                        return (previous, true);
                    }
                }
                (previous, false)
            }
        }
    }

    /// Where synthesized text after the segment `previous` sits in the
    /// original: that segment's original end, or 0 before the first.
    fn insertion_point(&self, previous: isize) -> i32 {
        if previous < 0 {
            0
        } else {
            self.segments[previous as usize].original_end
        }
    }

    fn map_low(&self, pos: i32, index: isize, inside: bool) -> i32 {
        if !inside {
            return self.insertion_point(index);
        }
        let segment = &self.segments[index as usize];
        if segment.kind == KIND_VERBATIM {
            clamp(
                segment.original_start + (pos - segment.virtual_start),
                segment.original_start,
                segment.original_end,
            )
        } else {
            segment.original_start
        }
    }

    fn map_high(&self, pos: i32, index: isize, inside: bool) -> i32 {
        if !inside {
            return self.insertion_point(index);
        }
        let segment = &self.segments[index as usize];
        if segment.kind == KIND_VERBATIM {
            clamp(
                segment.original_start + (pos - segment.virtual_start),
                segment.original_start,
                segment.original_end,
            )
        } else {
            segment.original_end
        }
    }
}

/// The language service's half: projections of original positions and
/// ranges into the virtual text (a content mapper's own directives use them
/// too).
impl SpanMap {
    fn original_index(&self) -> &OriginalIndex {
        self.original_index
            .0
            .get_or_init(|| OriginalIndex::new(&self.segments))
    }

    /// tsgo `OriginalToVirtualPositions`: every virtual projection of an
    /// original position whose segment takes part in `feature`, ordered by
    /// virtual position. Segment ends are inclusive for a point, so a
    /// position shared by adjacent original spans projects through both. An
    /// uncovered position, or one whose segments all reject `feature`, has
    /// none.
    pub fn original_to_virtual_positions(
        &self,
        pos: i32,
        feature: Features,
    ) -> Vec<MappedPosition> {
        let mut results: Vec<MappedPosition> = Vec::new();
        for group in self
            .original_index()
            .segment_groups_at_original_position(pos)
        {
            for segment in &group.segments {
                if !supports_feature(segment, feature) {
                    continue;
                }
                let mapped = if segment.kind == KIND_VERBATIM {
                    MappedPosition {
                        position: clamp(
                            segment.virtual_start + (pos - segment.original_start),
                            segment.virtual_start,
                            segment.virtual_end,
                        ),
                        fidelity: Fidelity::Exact,
                    }
                } else if group.at_end {
                    MappedPosition {
                        position: segment.virtual_end,
                        fidelity: Fidelity::Atom,
                    }
                } else {
                    MappedPosition {
                        position: segment.virtual_start,
                        fidelity: Fidelity::Atom,
                    }
                };
                if !results.contains(&mapped) {
                    results.push(mapped);
                }
            }
        }
        results.sort_by_key(|mapped| mapped.position);
        results
    }

    /// tsgo `OriginalToVirtualSpans`: every `feature` projection of the
    /// original range `[pos, end)`. A range within segments gives one exact
    /// or atom result per segment; a range from one group of equal original
    /// ranges into another gives the smallest virtual range around each
    /// possible start, approximately. An uncovered or disabled end gives
    /// none.
    pub fn original_to_virtual_spans(
        &self,
        pos: i32,
        end: i32,
        feature: Features,
    ) -> Vec<MappedSpan> {
        let start = pos;
        let end = end.max(start);
        if start == end {
            return self
                .original_to_virtual_positions(start, feature)
                .into_iter()
                .map(|position| MappedSpan {
                    pos: position.position,
                    end: position.position,
                    fidelity: position.fidelity,
                })
                .collect();
        }
        let index = self.original_index();
        let start_segments = index.segments_at_original_position(start);
        let end_segments = index.segments_at_original_position(end - 1);
        if start_segments.is_empty() || end_segments.is_empty() {
            return Vec::new();
        }
        let containing: Vec<Segment> = start_segments
            .iter()
            .filter(|segment| end <= segment.original_end)
            .copied()
            .collect();
        if !containing.is_empty() {
            let mut results =
                original_to_virtual_spans_in_segments(&containing, start, end, feature);
            if !results.is_empty() {
                results.sort_by_key(|mapped| mapped.pos);
                return results;
            }
        }
        let mut starts = original_start_projections(&start_segments, start, feature);
        let mut ends = original_end_projections(&end_segments, end, feature);
        if starts.is_empty() || ends.is_empty() {
            return Vec::new();
        }
        starts.sort_unstable();
        ends.sort_unstable();
        let mut results = Vec::with_capacity(starts.len().min(ends.len()));
        for (position, &virtual_start) in starts.iter().enumerate() {
            let end_index = ends.partition_point(|&end| end < virtual_start);
            if end_index == ends.len()
                || (position + 1 < starts.len() && starts[position + 1] <= ends[end_index])
            {
                continue;
            }
            results.push(MappedSpan {
                pos: virtual_start,
                end: ends[end_index],
                fidelity: Fidelity::Approximate,
            });
        }
        results
    }

    /// tsgo `OriginalToVirtualIntersectingSpans`: every `feature` segment's
    /// intersection with `[pos, end)`, in virtual order; unlike
    /// [`Self::original_to_virtual_spans`], uncovered ends do not hide the
    /// covered interior.
    pub fn original_to_virtual_intersecting_spans(
        &self,
        pos: i32,
        end: i32,
        feature: Features,
    ) -> Vec<MappedSpan> {
        if pos == end {
            return self.original_to_virtual_spans(pos, end, feature);
        }
        self.segments
            .iter()
            .filter(|segment| supports_feature(segment, feature))
            .filter_map(|segment| {
                let start = pos.max(segment.original_start);
                let end = end.min(segment.original_end);
                if start >= end {
                    return None;
                }
                Some(if segment.kind == KIND_VERBATIM {
                    MappedSpan {
                        pos: segment.virtual_start + (start - segment.original_start),
                        end: segment.virtual_start + (end - segment.original_start),
                        fidelity: Fidelity::Exact,
                    }
                } else {
                    MappedSpan {
                        pos: segment.virtual_start,
                        end: segment.virtual_end,
                        fidelity: Fidelity::Atom,
                    }
                })
            })
            .collect()
    }
}

/// tsgo `originalStartProjections`: the inclusive start of an original range
/// through each `feature` segment (a verbatim segment keeps the offset, an
/// atom gives its virtual start).
fn original_start_projections(segments: &[Segment], start: i32, feature: Features) -> Vec<i32> {
    segments
        .iter()
        .filter(|segment| supports_feature(segment, feature))
        .map(|segment| {
            if segment.kind == KIND_VERBATIM {
                clamp(
                    segment.virtual_start + (start - segment.original_start),
                    segment.virtual_start,
                    segment.virtual_end,
                )
            } else {
                segment.virtual_start
            }
        })
        .collect()
}

/// tsgo `originalEndProjections`: the exclusive end of an original range
/// through each `feature` segment holding its last character (a verbatim
/// segment keeps the offset, an atom gives its virtual end).
fn original_end_projections(segments: &[Segment], end: i32, feature: Features) -> Vec<i32> {
    segments
        .iter()
        .filter(|segment| supports_feature(segment, feature))
        .map(|segment| {
            if segment.kind == KIND_VERBATIM {
                clamp(
                    segment.virtual_start + (end - segment.original_start),
                    segment.virtual_start,
                    segment.virtual_end,
                )
            } else {
                segment.virtual_end
            }
        })
        .collect()
}

/// tsgo `originalToVirtualSpansInSegments`: a range each segment contains.
fn original_to_virtual_spans_in_segments(
    segments: &[Segment],
    start: i32,
    end: i32,
    feature: Features,
) -> Vec<MappedSpan> {
    segments
        .iter()
        .filter(|segment| supports_feature(segment, feature))
        .map(|segment| {
            if segment.kind == KIND_VERBATIM {
                let virtual_start = clamp(
                    segment.virtual_start + (start - segment.original_start),
                    segment.virtual_start,
                    segment.virtual_end,
                );
                let virtual_end = clamp(
                    segment.virtual_start + (end - segment.original_start),
                    virtual_start,
                    segment.virtual_end,
                );
                MappedSpan {
                    pos: virtual_start,
                    end: virtual_end,
                    fidelity: Fidelity::Exact,
                }
            } else {
                MappedSpan {
                    pos: segment.virtual_start,
                    end: segment.virtual_end,
                    fidelity: Fidelity::Atom,
                }
            }
        })
        .collect()
}

fn supports_feature(segment: &Segment, feature: Features) -> bool {
    segment.features & feature != 0
}

/// tsgo `originalIndex`: the segments in original order and a complete
/// binary tree over them whose nodes hold the greatest original end below
/// them, so a point lookup skips every subtree that cannot reach it.
struct OriginalIndex {
    segments: Vec<Segment>,
    leaf_count: usize,
    max_ends: Vec<i32>,
}

/// tsgo `segmentGroupAtOriginalPosition`: segments of one original range,
/// and whether the position is that range's end.
struct SegmentGroup {
    segments: Vec<Segment>,
    at_end: bool,
}

impl OriginalIndex {
    fn new(segments: &[Segment]) -> Self {
        let mut segments = segments.to_vec();
        segments.sort_by(|left, right| {
            left.original_start
                .cmp(&right.original_start)
                .then(left.original_end.cmp(&right.original_end))
                .then(left.virtual_start.cmp(&right.virtual_start))
        });
        let mut leaf_count = 1;
        while leaf_count < segments.len() {
            leaf_count *= 2;
        }
        let mut max_ends = vec![0; 2 * leaf_count];
        for (index, segment) in segments.iter().enumerate() {
            max_ends[leaf_count + index] = segment.original_end;
        }
        for node in (1..leaf_count).rev() {
            max_ends[node] = max_ends[2 * node].max(max_ends[2 * node + 1]);
        }
        Self {
            segments,
            leaf_count,
            max_ends,
        }
    }

    /// tsgo `segmentsAtOriginalPosition`: every segment containing `pos`
    /// (ends exclusive; a segment starting there, even an empty one, is
    /// contained).
    fn segments_at_original_position(&self, pos: i32) -> Vec<Segment> {
        let start = self
            .segments
            .partition_point(|segment| segment.original_start < pos);
        let mut results = Vec::new();
        self.collect(1, 0, self.leaf_count, start, pos, false, &mut results);
        let end = self
            .segments
            .partition_point(|segment| segment.original_start <= pos);
        results.extend_from_slice(&self.segments[start..end]);
        results
    }

    /// tsgo `collectSegmentsEndingAtOrAfter`: the segments before `limit`
    /// whose end reaches `pos` (passes it unless `include_end`), in original
    /// order.
    #[allow(clippy::too_many_arguments)]
    fn collect(
        &self,
        node: usize,
        start: usize,
        end: usize,
        limit: usize,
        pos: i32,
        include_end: bool,
        results: &mut Vec<Segment>,
    ) {
        if start >= limit
            || self.max_ends[node] < pos
            || (!include_end && self.max_ends[node] == pos)
        {
            return;
        }
        if end - start == 1 {
            results.push(self.segments[start]);
            return;
        }
        let middle = start + (end - start) / 2;
        self.collect(2 * node, start, middle, limit, pos, include_end, results);
        self.collect(2 * node + 1, middle, end, limit, pos, include_end, results);
    }

    /// tsgo `segmentGroupsAtOriginalPosition`: each group of segments with
    /// one original range containing or touching `pos` (ends included); at a
    /// shared boundary the segments ending there and those starting there
    /// are separate groups.
    fn segment_groups_at_original_position(&self, pos: i32) -> Vec<SegmentGroup> {
        let limit = self
            .segments
            .partition_point(|segment| segment.original_start <= pos);
        let mut segments = Vec::new();
        self.collect(1, 0, self.leaf_count, limit, pos, true, &mut segments);
        let mut groups = Vec::new();
        let mut start = 0;
        while start < segments.len() {
            let mut end = start + 1;
            while end < segments.len()
                && segments[start].original_start == segments[end].original_start
                && segments[start].original_end == segments[end].original_end
            {
                end += 1;
            }
            let segment = segments[start];
            if pos <= segment.original_end {
                groups.push(SegmentGroup {
                    segments: segments[start..end].to_vec(),
                    at_end: pos == segment.original_end && pos != segment.original_start,
                });
            }
            start = end;
        }
        groups
    }
}

impl Segment {
    fn error(&self, kind: MappingErrorKind, original_pos: i32) -> MappingError {
        MappingError {
            kind,
            virtual_pos: self.virtual_start,
            original_pos,
        }
    }
}

fn clamp(value: i32, low: i32, high: i32) -> i32 {
    low.max(value.min(high))
}

/// tsgo `ast.MappedDiagnosticDirectivePolicy`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DirectivePolicy {
    /// Diagnostics in the virtual range are not reported.
    #[default]
    Ignore,
    /// As `Ignore`, and the directive's own diagnostic is reported when the
    /// range has none (an unused `@ts-expect-error`).
    Expect,
}

/// tsgo `ast.MappedDiagnosticDirective`: a mapper's directive over the
/// TypeScript diagnostics of a virtual range, with the original range it
/// came from, the diagnostic an unused `Expect` reports and the mapper's
/// diagnostic source. Ranges are `(pos, end)` in the compiler's positions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MappedDiagnosticDirective {
    pub original_range: (i32, i32),
    pub virtual_range: (i32, i32),
    pub policy: DirectivePolicy,
    pub unused_code: i32,
    pub unused_message_text: String,
    pub source: String,
}

#[cfg(test)]
#[path = "../tests/unit/spanmap_tests.rs"]
mod spanmap_tests;

/// UTF-16 offsets of a text as UTF-8 offsets: each non-ASCII character's
/// offsets after it.
struct Utf16ToUtf8 {
    entries: Vec<(i32, i32)>,
}

impl Utf16ToUtf8 {
    fn new(text: &str) -> Self {
        let mut entries = Vec::new();
        let mut utf16 = 0i32;
        for (byte, character) in text.char_indices() {
            utf16 += character.len_utf16() as i32;
            if !character.is_ascii() {
                entries.push((utf16, (byte + character.len_utf8()) as i32));
            }
        }
        Self { entries }
    }

    fn map(&self, utf16: i32) -> i32 {
        let after = self.entries.partition_point(|&(units, _)| units <= utf16);
        match after.checked_sub(1) {
            None => utf16,
            Some(index) => {
                let (units, bytes) = self.entries[index];
                bytes + (utf16 - units)
            }
        }
    }
}
