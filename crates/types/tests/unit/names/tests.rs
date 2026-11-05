use super::*;
use crate::EscapedName;

/// Texts across the cache's keys: empty, every short length (1–3 bytes by
/// their bytes, 4–7 and 8–16 by two overlapping words), 17–24 bytes (text
/// kept in the slot), longer (text compared with the table's), multi-byte
/// UTF-8, and the `__` names the escape changes.
fn texts() -> Vec<String> {
    let mut texts: Vec<String> = [
        "",
        "a",
        "ab",
        "abc",
        "__",
        "__proto__",
        "___x",
        "_x",
        "é",
        "名前",
        "αβγδεζηθ",
        "constructor",
        "prototype",
    ]
    .iter()
    .map(|text| text.to_string())
    .collect();
    for len in 1..=40 {
        let base: Vec<u8> = (0..len).map(|i| b'a' + (i % 26) as u8).collect();
        texts.push(String::from_utf8(base.clone()).expect("ASCII"));
        // The same length, one byte changed: each position must be part of
        // the key (the short key's words overlap in the middle).
        for at in 0..len {
            let mut changed = base.clone();
            changed[at] = b'Z';
            texts.push(String::from_utf8(changed).expect("ASCII"));
        }
    }
    texts
}

#[test]
fn a_held_cache_gives_the_escaped_name_of_every_text_in_every_round() {
    let texts = texts();
    let mut held = HeldNameCache::take();
    for round in 0..3 {
        for text in &texts {
            let raw = JsStr::from_str(text);
            assert_eq!(
                held.escape(raw),
                EscapedName::escape(raw),
                "{text:?} (round {round})"
            );
        }
    }
}

#[test]
fn colliding_texts_keep_their_own_names() {
    // Far more texts than slots: each slot is overwritten many times, and
    // a hit must still be the slot's own text.
    let texts: Vec<String> = (0..50_000)
        .flat_map(|i| {
            [
                format!("n{i}"),
                format!("a_longer_name_number_{i}"),
                format!("x{i}_and_a_name_past_twenty_four_bytes"),
            ]
        })
        .collect();
    let mut held = HeldNameCache::take();
    for round in 0..2 {
        for text in &texts {
            let raw = JsStr::from_str(text);
            assert_eq!(
                held.escape(raw),
                EscapedName::escape(raw),
                "{text:?} (round {round})"
            );
        }
    }
}

#[test]
fn a_parse_inside_a_parse_holds_its_own_cache_and_the_thread_keeps_one() {
    let mut outer = HeldNameCache::take();
    let mut inner = HeldNameCache::take();
    let raw = JsStr::from_str("inner");
    assert_eq!(inner.escape(raw), EscapedName::escape(raw));
    drop(inner);
    let raw = JsStr::from_str("outer");
    assert_eq!(outer.escape(raw), EscapedName::escape(raw));
    drop(outer);
    let mut again = HeldNameCache::take();
    assert_eq!(again.escape(raw), EscapedName::escape(raw));
    let raw = JsStr::from_str("inner");
    assert_eq!(again.escape(raw), EscapedName::escape(raw));
}
