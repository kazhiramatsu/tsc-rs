//! tsgo `IsRecognizedTripleSlashComment` and `IsPinnedComment`
//! (printer/utilities.go): the triple-slash pragmas the parser understands,
//! and the `/*!` comments longer than `/*!*/`.

use crate::{is_pinned_comment, is_recognized_triple_slash_comment};

#[test]
fn triple_slash_pragmas_tsgo_recognizes() {
    for comment in [
        "/// <reference path=\"a.ts\" />",
        "///<reference types='node'/>",
        "/// <reference lib=\"es2015\" />",
        // `/>` anywhere after the value.
        "/// <reference no-default-lib=\"true\" x/>",
        "/// <amd-dependency path=\"a\" />",
        // No space needed after `amd-module`.
        "///<amd-module/>",
        "/// <amd-module name=\"a\"/>",
    ] {
        assert!(is_recognized_triple_slash_comment(comment), "{comment}");
    }
    for comment in [
        "// <reference path=\"a.ts\" />",
        "/// reference path=\"a.ts\" />",
        "/// <referencex path=\"a.ts\" />",
        "/// <reference path=\"a.ts\" >",
        "/// <reference path=a.ts />",
        "/// <reference foo=\"a.ts\" />",
        "/// <amd-dependency name=\"a\" />",
        "/// <amd-modulex",
    ] {
        assert!(!is_recognized_triple_slash_comment(comment), "{comment}");
    }
}

#[test]
fn pinned_comments_are_longer_than_the_empty_one() {
    let text = "/*!*/ /*! a */ /* b */";
    assert!(!is_pinned_comment(text, 0, 5));
    assert!(is_pinned_comment(text, 6, 14));
    assert!(!is_pinned_comment(text, 15, 22));
}
