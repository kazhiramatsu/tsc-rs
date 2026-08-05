use super::*;

#[test]
fn js_to_int32_wraps_like_ecmascript() {
    assert_eq!(js_to_int32(0.0), 0);
    assert_eq!(js_to_int32(-0.0), 0);
    assert_eq!(js_to_int32(3.9), 3);
    assert_eq!(js_to_int32(-3.9), -3);
    assert_eq!(js_to_int32(4294967296.0), 0);
    assert_eq!(js_to_int32(4294967295.0), -1);
    assert_eq!(js_to_int32(2147483648.0), -2147483648);
    assert_eq!(js_to_int32(f64::NAN), 0);
    assert_eq!(js_to_int32(f64::INFINITY), 0);
}

#[test]
fn js_pow_matches_ecmascript_special_cases() {
    assert!(js_pow(1.0, f64::INFINITY).is_nan());
    assert!(js_pow(-1.0, f64::NEG_INFINITY).is_nan());
    assert!(js_pow(2.0, f64::NAN).is_nan());
    assert_eq!(js_pow(f64::NAN, 0.0), 1.0);
    assert_eq!(js_pow(2.0, 10.0), 1024.0);
}

#[test]
fn radix_string_to_number_has_no_u128_ceiling_and_rounds_to_even() {
    assert_eq!(
        js_string_to_number("0x100000000000000000000000000000000"),
        2.0f64.powi(128)
    );
    assert_eq!(
        js_string_to_number(&format!("0b{}", "1".repeat(129))),
        2.0f64.powi(129)
    );
    assert_eq!(
        js_string_to_number(&format!("0o{}", "7".repeat(50))),
        2.0f64.powi(150)
    );
    assert_eq!(js_string_to_number("0x20000000000001"), 2.0f64.powi(53));
    assert_eq!(
        js_string_to_number("0x20000000000003"),
        2.0f64.powi(53) + 4.0
    );
    assert!(js_string_to_number(&format!("0x{}", "f".repeat(256))).is_infinite());
    assert!(js_string_to_number("0x").is_nan());
    assert!(js_string_to_number("0b102").is_nan());
}

#[test]
fn numeric_literal_names_round_trip() {
    assert!(is_numeric_literal_name("0"));
    assert!(is_numeric_literal_name("10"));
    assert!(is_numeric_literal_name("1.5"));
    assert!(is_numeric_literal_name("-1"));
    assert!(is_numeric_literal_name("Infinity"));
    assert!(is_numeric_literal_name("NaN"));
    assert!(!is_numeric_literal_name("1.0"));
    assert!(!is_numeric_literal_name("01"));
    assert!(!is_numeric_literal_name("1e2")); // "100" round-trip
    assert!(!is_numeric_literal_name("A"));
    assert!(!is_numeric_literal_name(""));
}
