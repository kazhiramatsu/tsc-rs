//! tsgo host_test.go's response cases (directives, supplemental outputs,
//! virtual extensions, position encodings), decoded as the host decodes a
//! mapper's transform result. Positions are the port's UTF-16 units where
//! tsgo's are UTF-8 bytes.

use super::*;
use tsc_diagnostics::spanmap::{Fidelity, KIND_VERBATIM};

fn decode(json: &str, content: &str) -> Result<TransformOutput, ResponseFailure> {
    decode_transform_result(json.as_bytes(), content, PositionEncoding::Utf8, "mapper")
}

fn directive_error(failure: ResponseFailure) -> DiagnosticDirectiveError {
    match failure {
        ResponseFailure::DiagnosticDirective(error) => error,
        failure => panic!("expected a directive error, got {failure:?}"),
    }
}

#[test]
fn diagnostic_directives_take_their_unused_diagnostics() {
    let output = decode(
        r#"{"text":"virtual source","extension":".ts","diagnosticDirectives":{"unusedExpectDirectiveDiagnostics":[{"code":2578,"messageText":"Unused framework directive."}],"directives":[[0,9,8,14,1]]}}"#,
        "directive\nsource",
    )
    .unwrap();
    assert_eq!(
        output.canonical.diagnostic_directives,
        [MappedDiagnosticDirective {
            original_range: (0, 9),
            virtual_range: (8, 14),
            policy: DirectivePolicy::Expect,
            unused_code: 2578,
            unused_message_text: "Unused framework directive.".to_owned(),
            source: "mapper".to_owned(),
        }]
    );
    let output = decode(
        r#"{"text":"virtual source","extension":".ts","diagnosticDirectives":{"unusedExpectDirectiveDiagnostics":[{"code":1,"messageText":"first"},{"code":2,"messageText":"second"}],"directives":[[0,9,8,14,1,1]]}}"#,
        "directive\nsource",
    )
    .unwrap();
    let directive = &output.canonical.diagnostic_directives[0];
    assert_eq!(
        (
            directive.unused_code,
            directive.unused_message_text.as_str()
        ),
        (2, "second")
    );
    // An ignore directive does not need an original range.
    assert!(decode(
        r#"{"text":"x","extension":".ts","diagnosticDirectives":{"unusedExpectDirectiveDiagnostics":[{}],"directives":[[-1,0,0,0,0]]}}"#,
        "directive\nsource",
    )
    .is_ok());
}

#[test]
fn invalid_diagnostic_directives_are_rejected_by_kind() {
    for (name, text, directives, unused, kind) in [
        (
            "invalid range",
            "x",
            "[[0,0,-1,0,0]]",
            "[]",
            DiagnosticDirectiveErrorKind::InvalidRange,
        ),
        (
            "unknown policy",
            "x",
            "[[0,0,0,0,2]]",
            "[]",
            DiagnosticDirectiveErrorKind::InvalidPolicy,
        ),
        (
            "expect requires unused diagnostic",
            "x",
            "[[0,0,0,0,1]]",
            "[]",
            DiagnosticDirectiveErrorKind::ExpectMissingUnusedDiagnostic,
        ),
        (
            "multiple unused diagnostics require index",
            "x",
            "[[0,0,0,0,1]]",
            "[{},{}]",
            DiagnosticDirectiveErrorKind::ExpectMissingUnusedDiagnostic,
        ),
        (
            "original range out of bounds",
            "x",
            "[[99,0,0,0,1]]",
            "[{}]",
            DiagnosticDirectiveErrorKind::InvalidRange,
        ),
        (
            "virtual range out of bounds",
            "x",
            "[[0,0,99,0,0]]",
            "[]",
            DiagnosticDirectiveErrorKind::InvalidRange,
        ),
        (
            "overlap",
            "abc",
            "[[0,0,0,2,0],[0,0,1,3,0]]",
            "[]",
            DiagnosticDirectiveErrorKind::Overlap,
        ),
    ] {
        let json = format!(
            r#"{{"text":"{text}","extension":".ts","diagnosticDirectives":{{"unusedExpectDirectiveDiagnostics":{unused},"directives":{directives}}}}}"#
        );
        let failure = decode(&json, "directive\nsource").expect_err(name);
        assert_eq!(directive_error(failure).kind, kind, "{name}");
    }
}

#[test]
fn supplemental_outputs_decode_with_their_own_directives() {
    let output = decode(
        r#"{"text":"export default 1;","extension":".ts","supplemental":[{"text":"declare const first: string;","extension":".ts","diagnosticDirectives":{"unusedExpectDirectiveDiagnostics":[],"directives":[[0,0,0,7,0]]}},{"text":"declare const second: number;","extension":".mjs"}]}"#,
        "component",
    )
    .unwrap();
    assert_eq!(output.supplemental.len(), 2);
    assert_eq!(output.supplemental[0].text, "declare const first: string;");
    assert_eq!(output.supplemental[0].virtual_extension, ".ts");
    assert_eq!(
        output.supplemental[0].diagnostic_directives[0].virtual_range,
        (0, 7)
    );
    assert_eq!(output.supplemental[1].virtual_extension, ".mjs");
    assert!(output.supplemental[1].mappings.segments().is_empty());

    let failure = decode(
        r#"{"text":"export {};","extension":".ts","supplemental":[{"text":"export {};","extension":".ts"},{"text":"export {};","extension":".ts","diagnosticDirectives":{"unusedExpectDirectiveDiagnostics":[],"directives":[[0,0,0,0,1]]}}]}"#,
        "component",
    )
    .unwrap_err();
    let error = directive_error(failure);
    assert_eq!(
        (error.kind, error.index, error.supplemental_index),
        (
            DiagnosticDirectiveErrorKind::ExpectMissingUnusedDiagnostic,
            0,
            Some(1)
        )
    );
}

#[test]
fn unsupported_virtual_extensions_are_rejected() {
    for extension in ["", ".coffee"] {
        for supplemental in [false, true] {
            let json = if supplemental {
                format!(
                    r#"{{"text":"export {{}};","extension":".ts","supplemental":[{{"text":"export {{}};","extension":"{extension}"}}]}}"#
                )
            } else {
                format!(r#"{{"text":"export {{}};","extension":"{extension}"}}"#)
            };
            let failure = decode(&json, "component").unwrap_err();
            assert!(
                failure.to_string().contains("invalid virtual extension"),
                "{failure}"
            );
        }
    }
}

#[test]
fn positions_in_either_encoding_normalize_to_the_compiler_s() {
    // "é" is two UTF-8 bytes and one UTF-16 unit.
    for (encoding, first, length) in [
        (PositionEncoding::Utf8, 2, 3),
        (PositionEncoding::Utf16, 1, 2),
    ] {
        let json = format!(
            r#"{{"text":"éx","extension":".ts","mappings":[[0,{first},0,{first},0],[{first},{rest},{first},{rest},0]],"diagnosticDirectives":{{"unusedExpectDirectiveDiagnostics":[],"directives":[[{first},{rest},{first},{length},0]]}},"diagnostics":[{{"messageText":"after non-ASCII character","start":{first},"length":{rest},"code":1001}}]}}"#,
            rest = length - first
        );
        let output = decode_transform_result(json.as_bytes(), "éx", encoding, "mapper").unwrap();
        let segments = output.canonical.mappings.segments();
        assert_eq!(segments.len(), 2);
        assert_eq!((segments[0].virtual_end, segments[0].original_end), (1, 1));
        assert_eq!(
            (segments[1].virtual_start, segments[1].original_start),
            (1, 1)
        );
        assert_eq!(segments[0].kind, KIND_VERBATIM);
        let units: Vec<u16> = "éx".encode_utf16().collect();
        assert_eq!(output.canonical.mappings.validate(&units, &units), None);
        assert_eq!(
            output.canonical.mappings.virtual_to_original_position(1),
            (1, Fidelity::Exact)
        );
        assert_eq!(
            (output.diagnostics[0].start, output.diagnostics[0].end),
            (1, 2)
        );
        let directive = &output.canonical.diagnostic_directives[0];
        assert_eq!(directive.original_range, (1, 2));
        assert_eq!(directive.virtual_range, (1, 2));
    }
}

#[test]
fn positions_inside_a_code_point_are_rejected() {
    for (encoding, content) in [
        (PositionEncoding::Utf8, "é"),
        (PositionEncoding::Utf16, "😀"),
    ] {
        let failure = decode_transform_result(
            br#"{"extension":".ts","diagnostics":[{"messageText":"invalid boundary","start":1,"code":1002}]}"#,
            content,
            encoding,
            "mapper",
        )
        .unwrap_err();
        assert!(
            failure.to_string().contains("splits a Unicode code point"),
            "{failure}"
        );
    }
}

#[test]
fn a_malformed_result_is_rejected() {
    assert!(matches!(
        decode(r#"{"text":1}"#, "a"),
        Err(ResponseFailure::Malformed(_))
    ));
}

#[test]
fn span_maps_decode_from_five_or_six_number_tuples() {
    let map = decode_span_map("[[3,11,60,11,1],[14,10,71,10,0,5]]").unwrap();
    assert_eq!(
        map.segments()[0].features,
        tsc_diagnostics::spanmap::FEATURES_ALL
    );
    assert_eq!(
        (
            map.segments()[1].virtual_end,
            map.segments()[1].original_end,
            map.segments()[1].features
        ),
        (24, 81, 5)
    );
    assert_eq!(
        decode_span_map("[[1,2,3]]").unwrap_err(),
        "span map segment 0: expected 5 or 6 values, got 3"
    );
}
