//! Paths share their bytes the way tsgo's Go strings do: a clone copies no bytes, while equality, ordering and hashing
//! stay those of the text.

use std::hash::BuildHasher;

use super::*;

fn bytes_at(text: JsStr<'_>) -> *const u8 {
    text.as_bytes().as_ptr()
}

#[test]
fn a_display_spelling_equal_to_the_canonical_path_shares_its_bytes() {
    let same = ProgramPath::from_js_parts("/src/a.ts".into(), "/src/a.ts".into()).unwrap();
    assert_eq!(bytes_at(same.display()), bytes_at(same.canonical().as_js()));
    let cased = ProgramPath::from_js_parts("/Src/A.ts".into(), "/src/a.ts".into()).unwrap();
    assert_ne!(
        bytes_at(cased.display()),
        bytes_at(cased.canonical().as_js())
    );
    assert_eq!(cased.display(), JsStr::from("/Src/A.ts"));
    assert_eq!(cased.canonical().as_js(), JsStr::from("/src/a.ts"));
}

#[test]
fn a_cloned_path_shares_its_bytes_and_owned_copies_keep_the_text() {
    let path = ProgramPath::from_js_parts("/Src/A.ts".into(), "/src/a.ts".into()).unwrap();
    let clone = path.clone();
    assert_eq!(clone, path);
    assert_eq!(bytes_at(clone.display()), bytes_at(path.display()));
    assert_eq!(
        bytes_at(clone.canonical().as_js()),
        bytes_at(path.canonical().as_js())
    );
    let (display, canonical) = clone.into_parts();
    assert_eq!(display.as_js(), path.display());
    assert_eq!(&canonical, path.canonical());
    assert_eq!(canonical.into_js_string().as_js(), path.canonical().as_js());
}

#[test]
fn canonical_paths_order_and_hash_as_their_text() {
    let a = CanonicalPath::from_js_normalized("/src/a.ts".into()).unwrap();
    let b = CanonicalPath::from_js_normalized("/src/b.ts".into()).unwrap();
    let other_a = CanonicalPath::from_js_normalized("/src/a.ts".into()).unwrap();
    assert!(a < b);
    assert_eq!(a, other_a);
    let state = std::collections::hash_map::RandomState::new();
    assert_eq!(state.hash_one(&a), state.hash_one(&other_a));
    assert_eq!(
        state.hash_one(&a),
        state.hash_one(JsStr::from("/src/a.ts").to_owned())
    );
}
