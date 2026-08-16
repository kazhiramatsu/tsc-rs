use tsc_ci_core::{
    decode_canonical, decode_object_with_keys, BoundedBytesSink, CanonicalDecoder, CanonicalEncode,
    CanonicalError, CanonicalValue, DecodeError, StrictJsonDecoder,
};

fn bytes(value: &CanonicalValue) -> Vec<u8> {
    let mut sink = BoundedBytesSink::new(4096);
    value
        .encode_canonical(&mut sink)
        .expect("fixture fits the bounded sink");
    sink.into_bytes()
}

#[test]
fn canonical_encoder_sorts_keys_and_uses_exact_string_escapes() {
    let value = CanonicalValue::Object(vec![
        (
            "z".to_owned(),
            CanonicalValue::String("/é\u{2028}\u{0000}\u{0008}\u{000b}".to_owned()),
        ),
        ("a".to_owned(), CanonicalValue::Signed(-42)),
        (
            "m".to_owned(),
            CanonicalValue::Array(vec![CanonicalValue::Unsigned(9)]),
        ),
    ]);
    assert_eq!(
        bytes(&value),
        "{\"a\":-42,\"m\":[9],\"z\":\"/é \\u0000\\b\\u000b\"}".as_bytes()
    );
}

#[test]
fn duplicate_keys_and_sink_limits_fail_before_partial_success() {
    let duplicate = CanonicalValue::Object(vec![
        ("same".to_owned(), CanonicalValue::Null),
        ("same".to_owned(), CanonicalValue::Bool(true)),
    ]);
    let mut sink = BoundedBytesSink::new(64);
    assert_eq!(
        duplicate.encode_canonical(&mut sink),
        Err(CanonicalError::DuplicateKey)
    );

    let value = CanonicalValue::String("too long".to_owned());
    let mut limited = BoundedBytesSink::new(2);
    assert_eq!(
        value.encode_canonical(&mut limited),
        Err(CanonicalError::LimitExceeded)
    );
    assert!(limited.bytes().len() <= 2);
}

#[test]
fn strict_decoder_accepts_chunked_canonical_input() {
    let input = "{\"a\":[-9223372036854775808,0,9223372036854775808],\"z\":\"/é\\n\"}".as_bytes();
    let mut decoder = StrictJsonDecoder::new(input.len() as u64, 16);
    for chunk in input.chunks(3) {
        decoder.push(chunk).expect("chunk remains below the limit");
    }
    let decoded = decoder.finish().expect("canonical bytes decode");
    assert_eq!(bytes(&decoded), input);
}

#[test]
fn strict_decoder_rejects_noncanonical_forms_and_surrogate_spelling() {
    let inputs: &[&[u8]] = &[
        br#" {"a":1}"#,
        br#"{"b":1,"a":2}"#,
        br#"{"a":1,"a":2}"#,
        br#"{"a":01}"#,
        br#"{"a":1.0}"#,
        br#"{"a":"\u0061"}"#,
        br#"{"a":"\u000A"}"#,
        br#"{"a":"\/"}"#,
        br#"{"a":"\ud83d\ude00"}"#,
        br#"{"a":"\ud800"}"#,
    ];
    for input in inputs {
        assert!(
            decode_canonical(input, 4096, 16).is_err(),
            "noncanonical input unexpectedly accepted: {input:?}"
        );
    }
}

#[test]
fn decoder_limits_bytes_and_depth() {
    let input = br#"[[[0]]]"#;
    assert_eq!(
        decode_canonical(input, 2, 16),
        Err(DecodeError::LimitExceeded)
    );
    assert_eq!(
        decode_canonical(input, 64, 1),
        Err(DecodeError::DepthExceeded)
    );

    let mut decoder = StrictJsonDecoder::new(2, 16);
    assert_eq!(decoder.push(b"123"), Err(DecodeError::LimitExceeded));
}

#[test]
fn typed_key_projection_rejects_unknown_fields_after_canonical_decode() {
    let input = br#"{"a":1,"b":2}"#;
    assert_eq!(
        decode_object_with_keys(input, 64, 8, &["a"]),
        Err(DecodeError::UnknownField)
    );
    assert!(decode_object_with_keys(input, 64, 8, &["a", "b"]).is_ok());
}
