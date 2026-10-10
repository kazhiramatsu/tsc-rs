use super::*;

use tsc_diagnostics::DiagnosticCategory;

fn chain(code: u32, text: &str, next: Vec<MessageChain>) -> MessageChain {
    MessageChain {
        code,
        category: DiagnosticCategory::Error,
        text: text.into(),
        key: None,
        args: Vec::new(),
        next_present: !next.is_empty(),
        next,
        related: Vec::new(),
        repopulate: None,
        without_location: false,
    }
}

fn diagnostic(file: Option<&str>, start: u32, length: u32, message: MessageChain) -> Diagnostic {
    Diagnostic::new_js(
        file.map(Into::into),
        file.map(|_| start),
        file.map(|_| length),
        message,
    )
}

#[test]
fn test_path_prefixes_are_replaced_like_a_strings_replacer() {
    assert_eq!(
        remove_test_path_prefixes("/.src/a.ts(1,2): x /.lib/b"),
        "a.ts(1,2): x b"
    );
    assert_eq!(
        remove_test_path_prefixes("file:///./src/a.ts"),
        "file:///a.ts"
    );
    assert_eq!(
        remove_test_path_prefixes("bundled:///libs/lib.es5.d.ts"),
        "lib.es5.d.ts"
    );
    assert_eq!(remove_test_path_prefixes("/.src/.src/"), ".src/");
}

#[test]
fn library_locations_are_masked_where_the_runner_masks_them() {
    let summary = "lib.es5.d.ts(12,3): error TS2300: Duplicate identifier 'x'.\r\n\
                   a.ts(1,1): error TS2300: See lib.es5.d.ts(1,1).\r\n";
    assert_eq!(
        mask_summary_library_locations(summary),
        "lib.es5.d.ts(--,--): error TS2300: Duplicate identifier 'x'.\r\n\
         a.ts(1,1): error TS2300: See lib.es5.d.ts(1,1).\r\n"
    );
    assert_eq!(
        mask_related_library_location(" lib.dom.d.ts:5:17"),
        " lib.dom.d.ts:--:--"
    );
    assert_eq!(mask_related_library_location(" a.ts:5:17"), " a.ts:5:17");
}

#[test]
fn lines_split_and_start_where_the_runner_splits_and_starts_them() {
    assert_eq!(split_lines("a\r\nb\n\r"), ["a", "b", "\r"]);
    assert_eq!(split_lines(""), [""]);
    // A lone CR and U+2028 start ECMA lines without splitting baseline lines.
    assert_eq!(ecma_line_starts("a\rb\u{2028}c\r\nd"), [0, 2, 6, 9]);
}

/// Global and located diagnostics, a message chain, related information
/// in a fixture file and in a library file, a span that ends one character
/// into the next line, and a non-ASCII line whose squiggles count
/// characters.
#[test]
fn a_baseline_renders_like_the_native_runner() {
    let content = "let é = 1;\r\nfoo(\r\n  x);\r\n";
    let files = [InputFile {
        name: "/.src/a.ts",
        content,
    }];
    let mut located = diagnostic(
        Some("/.src/a.ts"),
        4,
        1,
        chain(
            2322,
            "Type 'number' is not assignable.",
            vec![chain(2322, "Because.", vec![])],
        ),
    );
    located.related.push(RelatedInfo {
        file_name: Some("/.src/a.ts".into()),
        start: Some(0),
        length: Some(3),
        message: chain(2728, "Declared here.", vec![]),
    });
    located.related.push(RelatedInfo {
        file_name: Some("/vendor/lib/lib.es5.d.ts".into()),
        start: Some(10),
        length: Some(3),
        message: chain(2728, "And here.", vec![]),
    });
    let spanning = diagnostic(
        Some("/.src/a.ts"),
        12,
        7,
        chain(2304, "Cannot find name.", vec![]),
    );
    let global = diagnostic(None, 0, 0, chain(2318, "Cannot find global type.", vec![]));
    let rendered = render(&[global, located, spanning], &files, &[], false).expect("diagnostics");
    let expected = [
        "error TS2318: Cannot find global type.",
        "a.ts(1,5): error TS2322: Type 'number' is not assignable.",
        "  Because.",
        "a.ts(2,1): error TS2304: Cannot find name.",
        "",
        "",
        "!!! error TS2318: Cannot find global type.",
        "==== a.ts (2 errors) ====",
        "    let é = 1;",
        "        ~",
        "!!! error TS2322: Type 'number' is not assignable.",
        "!!! error TS2322:   Because.",
        "!!! related TS2728 a.ts:1:1: Declared here.",
        "!!! related TS2728 lib.es5.d.ts:--:--: And here.",
        "    foo(",
        "    ~~~~",
        "      x);",
        "    ~",
        "!!! error TS2304: Cannot find name.",
        "    ",
    ]
    .join("\r\n");
    assert_eq!(rendered, expected);
}

/// A `@pretty` baseline: tsgo's pretty diagnostics, the file sections, then
/// the error summary. The expected text is the vendored
/// `multiLineContextDiagnosticWithPretty.errors.txt`.
#[test]
fn a_pretty_baseline_renders_like_the_native_runner() {
    let content = "const x: {c: string} = {\n    a: {\n        b: '',\n    }\n};\n";
    let files = [InputFile {
        name: "/.src/multiLineContextDiagnosticWithPretty.ts",
        content,
    }];
    let excess = diagnostic(
        Some("/.src/multiLineContextDiagnosticWithPretty.ts"),
        29,
        1,
        chain(
            2353,
            "Object literal may only specify known properties, and 'a' does not exist in type '{ c: string; }'.",
            vec![],
        ),
    );
    let rendered = render(&[excess], &files, &[], true).expect("diagnostics");
    let expected = [
        "\u{1b}[96mmultiLineContextDiagnosticWithPretty.ts\u{1b}[0m:\u{1b}[93m2\u{1b}[0m:\u{1b}[93m5\u{1b}[0m - \u{1b}[91merror\u{1b}[0m\u{1b}[90m TS2353: \u{1b}[0mObject literal may only specify known properties, and 'a' does not exist in type '{ c: string; }'.",
        "",
        "\u{1b}[7m2\u{1b}[0m     a: {",
        "\u{1b}[7m \u{1b}[0m \u{1b}[91m    ~\u{1b}[0m",
        "",
        "",
        "==== multiLineContextDiagnosticWithPretty.ts (1 errors) ====",
        "    const x: {c: string} = {",
        "        a: {",
        "        ~",
        "!!! error TS2353: Object literal may only specify known properties, and 'a' does not exist in type '{ c: string; }'.",
        "            b: '',",
        "        }",
        "    };",
        "    ",
        "Found 1 error in multiLineContextDiagnosticWithPretty.ts\u{1b}[90m:2\u{1b}[0m",
        "",
        "",
    ]
    .join("\r\n");
    assert_eq!(rendered, expected);
}

#[test]
fn no_diagnostics_means_no_baseline() {
    assert_eq!(render(&[], &[], &[], false), None);
    assert_eq!(render(&[], &[], &[], true), None);
}
