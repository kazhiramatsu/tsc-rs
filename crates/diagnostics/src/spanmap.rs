//! tsgo `spanmap`: the mapping between a content mapper's virtual text and
//! the original text it was produced from.
//!
//! Unlike a source map, which records point correspondences, a span map
//! records explicit segments for the parts of the virtual text that
//! correspond to the original; positions outside every segment are
//! synthesized. Positions are the compiler's (UTF-16 code units here, UTF-8
//! bytes in tsgo): the content mapper host normalizes a mapper's positions
//! before it builds a map. The language-service half of tsgo's package (the
//! original-to-virtual projections) comes with the language service.

use std::fmt;

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
}

impl SpanMap {
    /// tsgo `spanmap.New`: the segments ordered by virtual start (a stable
    /// sort, as tsgo's `slices.SortFunc` over its comparison is applied to
    /// segments that are distinct in practice).
    pub fn new(mut segments: Vec<Segment>) -> Self {
        segments.sort_by_key(|segment| segment.virtual_start);
        Self { segments }
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

#[cfg(test)]
#[path = "../tests/unit/spanmap_tests.rs"]
mod spanmap_tests;
