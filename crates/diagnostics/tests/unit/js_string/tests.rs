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
                    let right = JsString::from_code_units(&units[split..]);
                    let mut length = JsStringByteLength::default();
                    length.append(left.as_js()).unwrap();
                    length.append(JsStr::from_str("")).unwrap();
                    length.append(right.as_js()).unwrap();
                    assert_eq!(length.bytes(), complete.as_bytes().len());
                    left.push_js(right.as_js());
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
fn arbitrary_prefixes_suffixes_and_substrings_operate_on_utf16_units() {
    let alphabet = [0, 0x41, 0xd800, 0xd801, 0xdc00, 0xdfff, 0xfffd];
    for a in alphabet {
        for b in alphabet {
            for c in alphabet {
                let units = [a, b, c];
                let text = JsString::from_code_units(&units);
                for start in 0..=4 {
                    for end in 0..=4 {
                        let left = start.min(3);
                        let right = end.min(3);
                        let expected = &units[left.min(right)..left.max(right)];
                        let substring = text.as_js().substring(start, end);
                        assert_eq!(substring.to_utf16(), expected);
                        assert_eq!(substring, JsString::from_code_units(expected));
                        assert_eq!(
                            text.as_js().starts_with_js(substring.as_js()),
                            units.starts_with(expected)
                        );
                        assert_eq!(
                            text.as_js().ends_with_js(substring.as_js()),
                            units.ends_with(expected)
                        );
                    }
                }
                assert_eq!(text.as_js().substring(usize::MAX, 0), text);
                let longer = JsString::from_code_units(&[a, b, c, a]);
                assert!(!text.as_js().starts_with_js(longer.as_js()));
                assert!(!text.as_js().ends_with_js(longer.as_js()));
            }
        }
    }
    let pair = JsString::from("😀");
    let lead = JsString::from_code_units(&[0xd83d]);
    let trail = JsString::from_code_units(&[0xde00]);
    assert!(pair.as_js().starts_with_js(lead.as_js()));
    assert!(pair.as_js().ends_with_js(trail.as_js()));
    assert!(!pair.as_bytes().starts_with(lead.as_bytes()));
    assert!(!pair.as_bytes().ends_with(trail.as_bytes()));
}

#[test]
fn scalar_separators_return_canonical_views_around_surrogates() {
    let text = JsString::from_code_units(&[0xd800, 0x2a, 0xdc00, 0x2f]);
    let (prefix, suffix) = text.as_js().split_once("*").unwrap();
    assert_eq!(prefix.to_utf16(), [0xd800]);
    assert_eq!(suffix.to_utf16(), [0xdc00, 0x2f]);
    assert!(text.contains("*"));
    assert!(text.ends_with("/"));
    assert_eq!(suffix.strip_suffix("/").unwrap().to_utf16(), [0xdc00]);
    assert_eq!(prefix.strip_suffix(""), Some(prefix));
    assert!(text.contains(""));
    assert!(!text.contains("😀"));
}

#[test]
fn last_separator_preserves_path_components_and_matches_scalar_str() {
    for text in ["", "/", "a/b/c", "//server/a/", "a𐀀b𐀀c", "é/é"] {
        let value = JsString::from(text);
        for separator in ["", "/", "𐀀", "é", "missing"] {
            let actual = value.as_js().rsplit_once(separator);
            assert_eq!(
                actual.map(|(left, right)| (left.as_str().unwrap(), right.as_str().unwrap())),
                text.rsplit_once(separator),
            );
        }
    }
    let units = [0xd800, 0x2f, 0xd801, 0x2f, 0xd800, 0xdc00, 0x2f, 0xdc01];
    let path = JsString::from_code_units(&units);
    let (directory, basename) = path.as_js().rsplit_once("/").unwrap();
    assert_eq!(directory.to_utf16(), units[..6]);
    assert_eq!(basename.to_utf16(), [0xdc01]);
    let (left, right) = path.as_js().rsplit_once("𐀀").unwrap();
    assert_eq!(left.to_utf16(), units[..4]);
    assert_eq!(right.to_utf16(), [0x2f, 0xdc01]);
    assert_eq!(
        path.as_js().rsplit_once(""),
        Some((path.as_js(), "".into()))
    );
    assert_eq!(path.as_js().rsplit_once("missing"), None);
}

#[test]
fn byte_views_require_whole_wtf8_code_points() {
    let text = JsString::from_code_units(&[0x41, 0xd800, 0xdc00, 0xd801, 0x2a, 0xdc01]);
    let boundaries = [0, 1, 5, 8, 9, 12];
    for index in 0..=text.as_bytes().len() + 1 {
        let pieces = text.as_js().split_at_byte(index);
        assert_eq!(pieces.is_some(), boundaries.contains(&index));
        if let Some((before, after)) = pieces {
            assert_eq!(before.as_bytes().len(), index);
            let mut round_trip = before.to_owned();
            round_trip.push_js(after);
            assert_eq!(round_trip, text);
        }
    }
    assert!(text.as_js().split_at_byte(usize::MAX).is_none());
    assert_eq!(text.as_js().substring(1, 2).to_utf16(), [0xd800]);
}

#[test]
#[should_panic(expected = "JavaScript code point is at most U+10FFFF")]
fn out_of_range_code_point_requires_scanner_recovery() {
    JsString::new().push_code_point(0x110000);
}

#[test]
fn byte_truncation_preserves_canonical_prefixes_and_rejects_partial_code_points() {
    let original = JsString::from_code_units(&[0x41, 0xd83d, 0xde00, 0xd800, 0x2f, 0xdc00]);
    for index in 0..=original.as_bytes().len() + 1 {
        let mut truncated = original.clone();
        if let Some((prefix, _)) = original.as_js().split_at_byte(index) {
            assert!(truncated.truncate_bytes(index));
            assert_eq!(truncated, prefix.to_owned());
            assert_eq!(truncated, JsString::from_code_units(&truncated.to_utf16()));
        } else {
            assert!(!truncated.truncate_bytes(index));
            assert_eq!(truncated, original);
        }
    }
}
