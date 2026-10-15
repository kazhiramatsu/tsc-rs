//! tsgo spanmap_test.go.

use super::*;

fn segment(
    virtual_start: i32,
    virtual_end: i32,
    original_start: i32,
    original_end: i32,
    kind: SegmentKind,
) -> Segment {
    Segment {
        virtual_start,
        virtual_end,
        original_start,
        original_end,
        kind,
        features: FEATURES_ALL,
    }
}

#[test]
fn virtual_to_original_span_verbatim() {
    // Virtual [0,10) is a verbatim copy of original [100,110).
    let map = SpanMap::new(vec![segment(0, 10, 100, 110, KIND_VERBATIM)]);
    assert_eq!(
        map.virtual_to_original_span(3, 7),
        ((103, 107), Fidelity::Exact)
    );
}

#[test]
fn virtual_to_original_position_exact() {
    let map = SpanMap::new(vec![
        segment(0, 10, 100, 110, KIND_VERBATIM),
        segment(10, 20, 110, 120, KIND_ATOM),
        segment(20, 30, 120, 130, KIND_VERBATIM),
    ]);
    for (pos, want, exact) in [
        (5, 105, true),
        (10, 110, false),
        (15, 110, false),
        (20, 120, false),
        (25, 125, true),
    ] {
        assert_eq!(
            map.virtual_to_original_position_exact(pos),
            (want, exact),
            "{pos}"
        );
    }
}

#[test]
fn virtual_to_original_position_exact_rejects_discontinuous_boundary() {
    let map = SpanMap::new(vec![
        Segment {
            features: 0,
            ..segment(0, 10, 0, 10, KIND_VERBATIM)
        },
        Segment {
            features: 0,
            ..segment(10, 20, 100, 110, KIND_VERBATIM)
        },
    ]);
    assert_eq!(map.virtual_to_original_position_exact(10), (100, false));
}

#[test]
fn virtual_to_original_span_atom() {
    // Virtual [3,14) ("MyComponent") is an atom of the original [60,71).
    let map = SpanMap::new(vec![segment(3, 14, 60, 71, KIND_ATOM)]);
    assert_eq!(
        map.virtual_to_original_span(5, 9),
        ((60, 71), Fidelity::Atom)
    );
}

#[test]
fn virtual_alias() {
    let map = SpanMap::new(vec![segment(3, 6, 10, 11, KIND_ALIAS)]);
    assert_eq!(
        map.virtual_to_original_span(3, 6),
        ((10, 11), Fidelity::Atom)
    );
    assert_eq!(
        map.alias_for_virtual_span(3, 6).map(|alias| alias.kind),
        Some(KIND_ALIAS)
    );
    assert_eq!(map.alias_for_virtual_span(4, 6), None);
}

#[test]
fn virtual_to_original_span_synthesized_gap() {
    // A gap between two verbatim segments maps to the insertion point (the
    // preceding segment's original end), synthesized.
    let map = SpanMap::new(vec![
        segment(0, 10, 100, 110, KIND_VERBATIM),
        segment(20, 30, 200, 210, KIND_VERBATIM),
    ]);
    assert_eq!(
        map.virtual_to_original_span(12, 15),
        ((110, 110), Fidelity::None)
    );
}

#[test]
fn virtual_to_original_span_empty_is_synthesized() {
    let map = SpanMap::new(Vec::new());
    assert_eq!(
        map.virtual_to_original_span(5, 10),
        ((0, 0), Fidelity::None)
    );
}

#[test]
fn virtual_to_original_span_crossing_segments() {
    let map = SpanMap::new(vec![
        Segment {
            features: 0,
            ..segment(0, 10, 100, 110, KIND_VERBATIM)
        },
        Segment {
            features: 0,
            ..segment(10, 20, 200, 210, KIND_VERBATIM)
        },
    ]);
    assert_eq!(
        map.virtual_to_original_span(5, 15),
        ((105, 205), Fidelity::Approximate)
    );
}

#[test]
fn virtual_to_original_position() {
    // Virtual [0,10) is a verbatim copy of original [100,110); [20,30) is an
    // atom of original [200,210).
    let map = SpanMap::new(vec![
        segment(0, 10, 100, 110, KIND_VERBATIM),
        segment(20, 30, 200, 210, KIND_ATOM),
    ]);
    for (pos, want, fidelity) in [
        (3, 103, Fidelity::Exact),
        (25, 200, Fidelity::Atom),
        (15, 110, Fidelity::None),
    ] {
        assert_eq!(map.virtual_to_original_position(pos), (want, fidelity));
        // A position agrees with the zero-length range at it.
        let ((start, _), span_fidelity) = map.virtual_to_original_span(pos, pos);
        assert_eq!((start, span_fidelity), (want, fidelity));
    }
}

#[test]
fn zero_length_spans_at_segment_ends() {
    let map = SpanMap::new(vec![
        segment(0, 10, 100, 110, KIND_VERBATIM),
        segment(20, 30, 200, 210, KIND_VERBATIM),
    ]);
    assert_eq!(map.virtual_to_original_position(30), (210, Fidelity::Exact));
    assert_eq!(
        map.virtual_to_original_span(30, 30),
        ((210, 210), Fidelity::Exact)
    );
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn validate() {
    let transformed = "const greeting = 1;\n";
    let original = "<x>const greeting = 1;\n</x>";
    let length = transformed.len() as i32;
    let script_start = 3;
    for (name, segments, want) in [
        (
            "valid verbatim",
            vec![segment(
                0,
                length,
                script_start,
                script_start + length,
                KIND_VERBATIM,
            )],
            None,
        ),
        ("empty is valid", Vec::new(), None),
        (
            "gap is allowed",
            vec![segment(3, length, 0, 0, KIND_ATOM)],
            None,
        ),
        (
            "overlap",
            vec![
                segment(0, 10, 0, 0, KIND_ATOM),
                segment(5, length, 0, 0, KIND_ATOM),
            ],
            Some(MappingErrorKind::Overlap),
        ),
        (
            "original out of bounds",
            vec![segment(0, length, 0, original.len() as i32 + 10, KIND_ATOM)],
            Some(MappingErrorKind::OutOfBounds),
        ),
        (
            "verbatim text mismatch",
            vec![segment(0, length, 0, length, KIND_VERBATIM)],
            Some(MappingErrorKind::VerbatimMismatch),
        ),
        (
            "unknown kind",
            vec![segment(0, 1, 0, 1, 3)],
            Some(MappingErrorKind::Kind),
        ),
    ] {
        let problem = SpanMap::new(segments).validate(&units(transformed), &units(original));
        assert_eq!(problem.map(|problem| problem.kind), want, "{name}");
    }
}

#[test]
fn validate_original_overlap_and_features() {
    const HOVER: Features = 1;
    const DEFINITION: Features = 1 << 3;
    let with = |segment: Segment, features: Features| Segment {
        features,
        ..segment
    };
    for (name, segments, want) in [
        (
            "identical duplicate group",
            vec![
                with(segment(0, 3, 0, 3, KIND_VERBATIM), DEFINITION),
                with(segment(3, 6, 0, 3, KIND_VERBATIM), HOVER),
            ],
            None,
        ),
        (
            "partial original overlap is valid",
            vec![
                with(segment(0, 3, 0, 3, KIND_ATOM), 0),
                with(segment(3, 6, 2, 5, KIND_ATOM), 0),
            ],
            None,
        ),
        (
            "nested original overlap is valid",
            vec![
                with(segment(0, 5, 0, 5, KIND_ATOM), 0),
                with(segment(5, 6, 1, 4, KIND_ATOM), 0),
            ],
            None,
        ),
        (
            "duplicate without explicit features is tolerant",
            vec![
                with(segment(0, 3, 0, 3, KIND_ATOM), 0),
                with(segment(3, 6, 0, 3, KIND_ATOM), DEFINITION),
            ],
            None,
        ),
        (
            "duplicate with shared feature members is tolerant",
            vec![
                with(segment(0, 3, 0, 3, KIND_ATOM), HOVER),
                with(segment(3, 6, 0, 3, KIND_ATOM), HOVER | DEFINITION),
            ],
            None,
        ),
        (
            "features on sole cover are valid",
            vec![with(segment(0, 3, 0, 3, KIND_ATOM), DEFINITION)],
            None,
        ),
        (
            "unknown feature flag",
            vec![with(segment(0, 3, 0, 3, KIND_ATOM), 1 << 22)],
            Some(MappingErrorKind::Feature),
        ),
    ] {
        let problem = SpanMap::new(segments).validate(&units("abcabc"), &units("abcdef"));
        assert_eq!(problem.map(|problem| problem.kind), want, "{name}");
    }
}

#[test]
fn mapping_errors_describe_their_offsets_as_tsgo_does() {
    let error = |kind| MappingError {
        kind,
        virtual_pos: 4,
        original_pos: 9,
    };
    assert_eq!(
        error(MappingErrorKind::Overlap).to_string(),
        "content mapper position mappings overlap or are out of order near virtual offset 4"
    );
    assert_eq!(
        error(MappingErrorKind::OutOfBounds).to_string(),
        "content mapper position mapping points outside the original content at original offset 9"
    );
    assert_eq!(
        error(MappingErrorKind::VerbatimMismatch).to_string(),
        "content mapper verbatim mapping does not match the original content at virtual offset 4, original offset 9"
    );
    assert_eq!(
        error(MappingErrorKind::Kind).to_string(),
        "content mapper position mapping has an invalid kind at virtual offset 4"
    );
    assert_eq!(
        error(MappingErrorKind::Feature).to_string(),
        "content mapper position mappings have invalid features near original offset 9"
    );
}

fn featured(
    virtual_start: i32,
    virtual_end: i32,
    original_start: i32,
    original_end: i32,
    kind: SegmentKind,
    features: Features,
) -> Segment {
    Segment {
        features,
        ..segment(
            virtual_start,
            virtual_end,
            original_start,
            original_end,
            kind,
        )
    }
}

fn span(pos: i32, end: i32, fidelity: Fidelity) -> MappedSpan {
    MappedSpan { pos, end, fidelity }
}

fn position(position: i32, fidelity: Fidelity) -> MappedPosition {
    MappedPosition { position, fidelity }
}

#[test]
fn original_to_virtual_intersecting_spans_allows_uncovered_endpoints() {
    let map = SpanMap::new(vec![featured(
        10,
        20,
        100,
        110,
        KIND_VERBATIM,
        FEATURE_SEMANTIC_TOKENS | FEATURE_INLAY_HINTS,
    )]);
    for feature in [FEATURE_SEMANTIC_TOKENS, FEATURE_INLAY_HINTS] {
        assert_eq!(
            map.original_to_virtual_intersecting_spans(90, 120, feature),
            [span(10, 20, Fidelity::Exact)]
        );
    }
}

#[test]
fn original_to_virtual_span_verbatim() {
    let map = SpanMap::new(vec![segment(0, 10, 100, 110, KIND_VERBATIM)]);
    assert_eq!(
        map.original_to_virtual_spans(103, 107, FEATURES_ALL),
        [span(3, 7, Fidelity::Exact)]
    );
}

#[test]
fn original_to_virtual_span_atom() {
    let map = SpanMap::new(vec![segment(3, 14, 60, 71, KIND_ATOM)]);
    assert_eq!(
        map.original_to_virtual_spans(63, 67, FEATURES_ALL),
        [span(3, 14, Fidelity::Atom)]
    );
}

#[test]
fn original_to_virtual_span_gap() {
    let map = SpanMap::new(vec![
        segment(0, 10, 100, 110, KIND_VERBATIM),
        segment(20, 30, 200, 210, KIND_VERBATIM),
    ]);
    assert!(map
        .original_to_virtual_spans(150, 160, FEATURES_ALL)
        .is_empty());
}

#[test]
fn original_to_virtual_positions() {
    let map = SpanMap::new(vec![
        segment(0, 10, 100, 110, KIND_VERBATIM),
        segment(20, 30, 200, 210, KIND_ATOM),
    ]);
    for (pos, expected) in [
        (103, Some(position(3, Fidelity::Exact))),
        (205, Some(position(20, Fidelity::Atom))),
        (150, None),
    ] {
        let positions = map.original_to_virtual_positions(pos, FEATURES_ALL);
        let spans = map.original_to_virtual_spans(pos, pos, FEATURES_ALL);
        match expected {
            None => {
                assert!(positions.is_empty());
                assert!(spans.is_empty());
            }
            Some(expected) => {
                assert_eq!(positions, [expected]);
                assert_eq!(
                    spans,
                    [span(
                        expected.position,
                        expected.position,
                        expected.fidelity
                    )]
                );
            }
        }
    }
}

#[test]
fn original_to_virtual_positions_at_endpoint() {
    let map = SpanMap::new(vec![
        featured(2, 5, 10, 13, KIND_VERBATIM, FEATURE_COMPLETION),
        featured(8, 11, 13, 16, KIND_VERBATIM, FEATURE_COMPLETION),
        featured(20, 23, 30, 35, KIND_ATOM, FEATURE_COMPLETION),
    ]);
    assert_eq!(
        map.original_to_virtual_positions(13, FEATURE_COMPLETION),
        [position(5, Fidelity::Exact), position(8, Fidelity::Exact)]
    );
    assert_eq!(
        map.original_to_virtual_positions(35, FEATURE_COMPLETION),
        [position(23, Fidelity::Atom)]
    );
    let filtered = SpanMap::new(vec![
        featured(20, 23, 10, 13, KIND_VERBATIM, FEATURE_HOVER),
        featured(2, 5, 13, 16, KIND_VERBATIM, FEATURE_COMPLETION),
    ]);
    assert_eq!(
        filtered.original_to_virtual_positions(13, FEATURES_ALL),
        [position(2, Fidelity::Exact), position(23, Fidelity::Exact)]
    );
    assert_eq!(
        filtered.original_to_virtual_positions(13, FEATURE_COMPLETION),
        [position(2, Fidelity::Exact)]
    );
}

#[test]
fn original_to_virtual_duplicate_group() {
    let map = SpanMap::new(vec![
        featured(0, 3, 10, 13, KIND_VERBATIM, FEATURE_DEFINITION),
        featured(10, 13, 10, 13, KIND_VERBATIM, FEATURE_HOVER),
        featured(20, 25, 10, 13, KIND_ATOM, FEATURE_DEFINITION),
    ]);
    assert_eq!(
        map.original_to_virtual_positions(11, FEATURE_HOVER),
        [position(11, Fidelity::Exact)]
    );
    assert_eq!(
        map.original_to_virtual_positions(11, FEATURE_DEFINITION),
        [position(1, Fidelity::Exact), position(20, Fidelity::Atom)]
    );
    assert_eq!(
        map.original_to_virtual_spans(10, 13, FEATURE_DEFINITION),
        [span(0, 3, Fidelity::Exact), span(20, 25, Fidelity::Atom)]
    );
}

#[test]
fn original_to_virtual_overlapping_spans() {
    let map = SpanMap::new(vec![
        featured(0, 6, 0, 6, KIND_VERBATIM, FEATURE_HOVER),
        featured(10, 12, 2, 4, KIND_VERBATIM, FEATURE_HOVER),
        featured(20, 24, 3, 7, KIND_VERBATIM, FEATURE_HOVER),
    ]);
    assert_eq!(
        map.original_to_virtual_positions(3, FEATURE_HOVER),
        [
            position(3, Fidelity::Exact),
            position(11, Fidelity::Exact),
            position(20, Fidelity::Exact)
        ]
    );
    assert_eq!(
        map.original_to_virtual_spans(3, 4, FEATURE_HOVER),
        [
            span(3, 4, Fidelity::Exact),
            span(11, 12, Fidelity::Exact),
            span(20, 21, Fidelity::Exact)
        ]
    );
}

#[test]
fn original_to_virtual_position_finds_early_covering_segment() {
    // A search lands near [90, 95), which does not contain 97: the interval
    // index still finds the earlier [0, 100) without scanning every segment
    // that starts before the position.
    let map = SpanMap::new(vec![
        featured(0, 100, 0, 100, KIND_VERBATIM, FEATURE_HOVER),
        featured(100, 105, 80, 85, KIND_VERBATIM, FEATURE_HOVER),
        featured(105, 110, 90, 95, KIND_VERBATIM, FEATURE_HOVER),
        featured(110, 113, 100, 103, KIND_VERBATIM, FEATURE_HOVER),
    ]);
    assert_eq!(
        map.original_to_virtual_positions(97, FEATURE_HOVER),
        [position(97, Fidelity::Exact)]
    );
    assert_eq!(
        map.original_to_virtual_spans(97, 98, FEATURE_HOVER),
        [span(97, 98, Fidelity::Exact)]
    );
    // A point takes both sides of a shared end; a range treats ends as
    // exclusive and takes the right segment only.
    assert_eq!(
        map.original_to_virtual_positions(100, FEATURE_HOVER),
        [
            position(100, Fidelity::Exact),
            position(110, Fidelity::Exact)
        ]
    );
    assert_eq!(
        map.original_to_virtual_spans(100, 101, FEATURE_HOVER),
        [span(110, 111, Fidelity::Exact)]
    );
}

#[test]
fn original_to_virtual_overlap_falls_back_from_disabled_container() {
    let map = SpanMap::new(vec![
        featured(0, 6, 0, 6, KIND_VERBATIM, FEATURE_DEFINITION),
        featured(10, 13, 0, 3, KIND_VERBATIM, FEATURE_HOVER),
        featured(13, 16, 3, 6, KIND_VERBATIM, FEATURE_HOVER),
    ]);
    assert_eq!(
        map.original_to_virtual_spans(1, 5, FEATURE_HOVER),
        [span(11, 15, Fidelity::Approximate)]
    );
}

#[test]
fn original_to_virtual_cross_group_projections() {
    let map = SpanMap::new(vec![
        featured(0, 2, 0, 2, KIND_VERBATIM, FEATURE_HOVER),
        featured(2, 4, 2, 4, KIND_VERBATIM, FEATURE_HOVER),
        featured(10, 12, 0, 2, KIND_VERBATIM, FEATURE_HOVER),
        featured(12, 14, 2, 4, KIND_VERBATIM, FEATURE_HOVER),
    ]);
    assert_eq!(
        map.original_to_virtual_spans(1, 3, FEATURE_HOVER),
        [
            span(1, 3, Fidelity::Approximate),
            span(11, 13, Fidelity::Approximate)
        ]
    );
}

#[test]
fn original_to_virtual_explicit_zero_features() {
    let map = SpanMap::new(vec![featured(0, 3, 10, 13, KIND_VERBATIM, FEATURES_NONE)]);
    assert!(map
        .original_to_virtual_positions(11, FEATURE_HOVER)
        .is_empty());
    assert!(map
        .original_to_virtual_positions(11, FEATURE_DEFINITION)
        .is_empty());
    assert!(map
        .original_to_virtual_spans(10, 13, FEATURE_HOVER)
        .is_empty());
}

#[test]
fn feature_participation_original_and_virtual() {
    // TestFeatureParticipationOriginalAndVirtual.
    let map = SpanMap::new(vec![
        featured(0, 3, 10, 13, KIND_VERBATIM, FEATURE_HOVER),
        featured(3, 6, 20, 23, KIND_VERBATIM, FEATURE_COMPLETION),
    ]);
    assert_eq!(
        map.original_to_virtual_positions(11, FEATURE_HOVER).len(),
        1
    );
    assert!(map
        .original_to_virtual_positions(11, FEATURE_COMPLETION)
        .is_empty());
    assert_eq!(
        map.virtual_to_original_span_for_feature(0, 3, FEATURE_HOVER),
        ((10, 13), Fidelity::Exact)
    );
    assert_eq!(
        map.virtual_to_original_span_for_feature(0, 3, FEATURE_COMPLETION)
            .1,
        Fidelity::None
    );
    // Diagnostics use the unfiltered geometry.
    assert_eq!(
        map.virtual_to_original_span(0, 3),
        ((10, 13), Fidelity::Exact)
    );
}

#[test]
fn feature_participation_of_virtual_positions_and_spanning_ranges() {
    // tsgo VirtualToOriginalPositionForFeature and virtualSpanSupportsFeature:
    // a position takes its segment's features; a range needs contiguous
    // segments that all take part; an empty range its segment.
    let map = SpanMap::new(vec![
        featured(
            0,
            3,
            10,
            13,
            KIND_VERBATIM,
            FEATURE_HOVER | FEATURE_COMPLETION,
        ),
        featured(3, 6, 13, 16, KIND_VERBATIM, FEATURE_HOVER),
        featured(8, 10, 20, 22, KIND_VERBATIM, FEATURE_HOVER),
    ]);
    assert_eq!(
        map.virtual_to_original_position_for_feature(4, FEATURE_HOVER),
        (14, Fidelity::Exact)
    );
    assert_eq!(
        map.virtual_to_original_position_for_feature(4, FEATURE_COMPLETION),
        (14, Fidelity::None)
    );
    // In a gap: no fidelity whatever the feature.
    assert_eq!(
        map.virtual_to_original_position_for_feature(7, FEATURE_HOVER)
            .1,
        Fidelity::None
    );
    // Across two contiguous hover segments.
    assert_eq!(
        map.virtual_to_original_span_for_feature(1, 5, FEATURE_HOVER)
            .1,
        Fidelity::Approximate
    );
    assert_eq!(
        map.virtual_to_original_span_for_feature(1, 5, FEATURE_COMPLETION)
            .1,
        Fidelity::None
    );
    // Across the gap.
    assert_eq!(
        map.virtual_to_original_span_for_feature(5, 9, FEATURE_HOVER)
            .1,
        Fidelity::None
    );
    assert_eq!(
        map.virtual_to_original_span_for_feature(2, 2, FEATURE_COMPLETION),
        ((12, 12), Fidelity::Exact)
    );
}

#[test]
fn a_map_in_utf16_units_as_utf8_bytes() {
    // The compiler's UTF-16 positions over texts with characters outside
    // ASCII, as the language service's UTF-8 bytes (tsgo's units).
    let virtual_text = "é = \u{1F600};x";
    let original_text = "<a>é = \u{1F600};x</a>";
    let map = SpanMap::new(vec![
        segment(0, 7, 3, 10, KIND_VERBATIM),
        featured(7, 8, 10, 11, KIND_ATOM, FEATURE_HOVER),
    ]);
    let utf8 = map.in_utf8(virtual_text, original_text);
    assert_eq!(
        utf8.segments(),
        &[
            segment(0, 10, 3, 13, KIND_VERBATIM),
            featured(10, 11, 13, 14, KIND_ATOM, FEATURE_HOVER),
        ]
    );
}

#[test]
fn original_to_virtual_span_round_trip() {
    // Original spans out of the virtual order exercise the original index.
    let map = SpanMap::new(vec![
        segment(0, 10, 200, 210, KIND_VERBATIM),
        segment(10, 20, 100, 110, KIND_VERBATIM),
    ]);
    for (pos, end) in [(2, 8), (12, 18)] {
        let ((original_pos, original_end), fidelity) = map.virtual_to_original_span(pos, end);
        assert_eq!(fidelity, Fidelity::Exact);
        assert_eq!(
            map.original_to_virtual_spans(original_pos, original_end, FEATURES_ALL),
            [span(pos, end, Fidelity::Exact)]
        );
    }
}

#[test]
fn a_clone_and_an_equal_map_ignore_the_built_index() {
    let map = SpanMap::new(vec![segment(0, 10, 100, 110, KIND_VERBATIM)]);
    let fresh = map.clone();
    assert_eq!(
        map.original_to_virtual_positions(103, FEATURES_ALL).len(),
        1
    );
    assert_eq!(map, fresh);
    assert_eq!(map.clone(), fresh);
}
