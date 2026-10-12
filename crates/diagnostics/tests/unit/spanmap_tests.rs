//! tsgo spanmap_test.go, the virtual-to-original half (the
//! original-to-virtual projections come with the language service).

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
