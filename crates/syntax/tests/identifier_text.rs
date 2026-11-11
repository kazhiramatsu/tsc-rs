//! tsc `idText`: an identifier's text is its escaped text without the
//! underscore `escapeLeadingUnderscores` adds to a name that starts with two
//! underscores. A private identifier's text starts with `#`, so it is never
//! escaped and never unescaped. The text is read from the name table as
//! stored; these cases pin the spellings the readers receive.
use tsc_syntax::{ParseOptions, ScriptKind};

#[test]
fn identifier_text_is_the_escaped_text_without_the_escape_underscore() {
    let file_name = "/identifier-text.ts";
    let source = "var _a, __b, ___c, ____d, \u{3c0}, __\u{3c0}, a\\u0062, $__e;\n\
                  class C { #p = 0; #__q = 0; #___r = 0; }\n";
    let parsed = tsc_syntax::parse_source_file(
        file_name,
        source,
        ParseOptions {
            script_kind: ScriptKind::ensure_from_file_name(file_name),
            ..ParseOptions::default()
        },
        None,
    );
    assert!(parsed.parse_diagnostics.is_empty());

    let mut identifiers = Vec::new();
    let mut private_identifiers = Vec::new();
    for node in parsed.arena.nodes() {
        if let Some(data) = node.as_identifier() {
            identifiers.push((
                data.text(),
                data.escaped_text
                    .as_str()
                    .expect("identifier name is UTF-8"),
            ));
        }
        if let Some(data) = node.as_private_identifier() {
            private_identifiers.push((
                data.text(),
                data.escaped_text.as_str().expect("private name is UTF-8"),
            ));
        }
    }
    identifiers.sort_unstable();
    identifiers.dedup();
    private_identifiers.sort_unstable();
    private_identifiers.dedup();

    assert_eq!(
        identifiers,
        [
            ("$__e", "$__e"),
            ("C", "C"),
            ("____d", "_____d"),
            ("___c", "____c"),
            ("__b", "___b"),
            ("__\u{3c0}", "___\u{3c0}"),
            ("_a", "_a"),
            ("ab", "ab"),
            ("\u{3c0}", "\u{3c0}"),
        ]
    );
    assert_eq!(
        private_identifiers,
        [("#___r", "#___r"), ("#__q", "#__q"), ("#p", "#p")]
    );
}
