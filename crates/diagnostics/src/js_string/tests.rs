use super::*;
use std::collections::HashMap;

#[derive(Default)]
struct HashWrites(Vec<u8>);

impl Hasher for HashWrites {
    fn write(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn finish(&self) -> u64 {
        0
    }
}

fn hash_input(value: &(impl Hash + ?Sized)) -> Vec<u8> {
    let mut state = HashWrites::default();
    value.hash(&mut state);
    state.0
}

fn assert_contract(units: &[u16]) {
    let text = JsString::from_code_units(units);
    assert_eq!(text.to_utf16(), units);
    assert_eq!(text.len_units(), units.len());
    assert_eq!(text.as_js().to_owned(), text);
    assert_eq!(hash_input(&text), hash_input(text.as_bytes()));
    assert_eq!(hash_input(&text.as_js()), hash_input(text.as_bytes()));
    let scalar = String::from_utf16(units).ok();
    assert_eq!(text.as_str(), scalar.as_deref());
    assert_eq!(text.to_string_lossy(), String::from_utf16_lossy(units));
    if let Some(scalar) = scalar {
        assert_eq!(text.as_bytes(), scalar.as_bytes());
        assert_eq!(JsString::from(scalar.as_str()), text);
        assert_eq!(JsStr::from_str(&scalar), text.as_js());
    }
    let mut iterator = text.code_units();
    for remaining in (1..=units.len()).rev() {
        let (lower, upper) = iterator.size_hint();
        assert!(lower <= remaining && remaining <= upper.unwrap());
        assert!(iterator.next().is_some());
    }
    assert_eq!(iterator.next(), None);
    assert_eq!(iterator.next(), None);
}

#[test]
fn every_single_utf16_unit_round_trips_without_replacement() {
    for unit in 0..=u16::MAX {
        assert_contract(&[unit]);
    }
    assert_contract(&[]);
}

#[test]
fn every_unicode_scalar_has_exact_utf8_bytes() {
    for point in 0..=0x10FFFF {
        let Some(ch) = char::from_u32(point) else {
            continue;
        };
        let mut utf16 = [0; 2];
        let mut utf8 = [0; 4];
        let encoded = ch.encode_utf8(&mut utf8);
        let text = JsString::from_code_units(ch.encode_utf16(&mut utf16));
        assert_eq!(text.as_bytes(), encoded.as_bytes());
        let mut by_point = JsString::new();
        by_point.push_code_point(point);
        assert_eq!(by_point, text);
    }
}

#[test]
fn selected_pairs_and_every_split_preserve_concatenation() {
    let alphabet = [
        0, 0x41, 0x7F, 0x80, 0x7FF, 0x800, 0xD800, 0xDBFF, 0xDC00, 0xDFFF, 0xE000, 0xFFFD, 0xFFFF,
    ];
    for a in alphabet {
        for b in alphabet {
            assert_contract(&[a, b]);
            for c in alphabet {
                let units = [a, b, c];
                let complete = JsString::from_code_units(&units);
                for split in 0..=units.len() {
                    let mut left = JsString::from_code_units(&units[..split]);
                    left.push_js(JsString::from_code_units(&units[split..]).as_js());
                    assert_eq!(left, complete, "units {units:X?}, split {split}");
                }
            }
        }
    }
}

#[test]
fn arbitrary_sequences_have_canonical_associative_concatenation() {
    // Fixed seed and deliberately frequent surrogates keep failures reproducible.
    let mut state = 0x7F4A7C15_u32;
    for sample in 0..4096 {
        let mut units = Vec::new();
        for index in 0..sample % 65 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            units.push(match index % 4 {
                0 => 0xD800 | (state as u16 & 0x3FF),
                1 => 0xDC00 | (state as u16 & 0x3FF),
                _ => state as u16,
            });
        }
        assert_contract(&units);
        let first = units.len() / 3;
        let second = 2 * units.len() / 3;
        let a = JsString::from_code_units(&units[..first]);
        let b = JsString::from_code_units(&units[first..second]);
        let c = JsString::from_code_units(&units[second..]);
        let mut left = a.clone();
        left.push_js(b.as_js());
        left.push_js(c.as_js());
        let mut bc = b.clone();
        bc.push_js(c.as_js());
        let mut right = a;
        right.push_js(bc.as_js());
        assert_eq!(left, right);
        assert_eq!(left, JsString::from_code_units(&units));
    }
}

#[test]
fn escape_spellings_pair_to_the_same_key() {
    let mut separate = JsString::new();
    separate.push_code_point(0xD83D);
    separate.push_code_point(0xDE00);
    let mut extended = JsString::new();
    extended.push_code_point(0x1F600);
    let raw = JsString::from("😀");
    assert_eq!(separate, raw);
    assert_eq!(extended, raw);
    assert_eq!(raw.as_bytes(), [0xF0, 0x9F, 0x98, 0x80]);
    let d800 = JsString::from_code_units(&[0xD800]);
    assert_eq!(d800.as_bytes(), [0xED, 0xA0, 0x80]);
    for other in [
        JsString::from_code_units(&[0xD801]),
        JsString::from_code_units(&[0xDC00]),
        JsString::from("\u{FFFD}"),
        JsString::from("\\uD800"),
    ] {
        assert_ne!(d800, other);
    }
}

#[test]
fn borrowed_canonical_queries_match_owned_keys_and_hash_inputs() {
    let scalar = "a😀\0__";
    let view = JsStr::from_str(scalar);
    assert_eq!(view.as_bytes().as_ptr(), scalar.as_bytes().as_ptr());
    let mut table = HashMap::new();
    table.insert(JsString::from(scalar), 1);
    assert_eq!(table.get(view.as_bytes()), Some(&1));
    let surrogate = JsString::from_code_units(&[0xD800]);
    table.insert(surrogate.clone(), 2);
    assert_eq!(table.get(surrogate.as_js().as_bytes()), Some(&2));
    assert_eq!(table.get(JsStr::from_str("\u{FFFD}").as_bytes()), None);
    let borrowed: &[u8] = surrogate.borrow();
    assert_eq!(hash_input(&surrogate), hash_input(borrowed));
}

#[test]
fn utf16_comparison_is_explicit_and_byte_ord_satisfies_borrow() {
    let bmp = JsString::from("\u{E000}");
    let non_bmp = JsString::from("\u{10000}");
    assert_eq!(bmp.cmp(&non_bmp), Ordering::Less);
    assert_eq!(bmp.cmp_utf16(non_bmp.as_js()), Ordering::Greater);
    assert_eq!(bmp.cmp(&non_bmp), bmp.as_bytes().cmp(non_bmp.as_bytes()));
    let lone = JsString::from_code_units(&[0xD800]);
    assert_eq!(lone.cmp_utf16(non_bmp.as_js()), Ordering::Less);
}

#[test]
fn lossy_conversion_is_only_at_the_explicit_sink_and_debug_preserves_units() {
    let raw = JsString::from_code_units(&[0xD800, 0x41, 0xDC00, 0xD83D, 0xDE00]);
    assert_eq!(raw.to_string_lossy(), "�A�😀");
    assert_eq!(raw.to_utf16(), [0xD800, 0x41, 0xDC00, 0xD83D, 0xDE00]);
    let debug = format!("{raw:?}");
    assert!(debug.contains("\\u{D800}"));
    assert!(debug.contains("\\u{DC00}"));
    assert!(matches!(
        JsStr::from_str("scalar").to_string_lossy(),
        Cow::Borrowed(_)
    ));
}

#[test]
fn prefix_views_and_buffer_reuse_preserve_canonical_values() {
    let mut text = JsString::from("___");
    text.push_code_unit(0xD800);
    let stripped = text.as_js().strip_prefix("_").unwrap();
    assert_eq!(stripped.to_utf16(), [0x5F, 0x5F, 0xD800]);
    assert!(stripped.starts_with("__"));
    assert!(stripped.strip_prefix("x").is_none());
    text.clear();
    assert!(text.is_empty());
    text.push('😀');
    text.push_str("!");
    assert_eq!(text.as_str(), Some("😀!"));
}

#[test]
#[should_panic(expected = "JavaScript code point is at most U+10FFFF")]
fn out_of_range_code_point_requires_scanner_recovery() {
    JsString::new().push_code_point(0x110000);
}
