use super::*;
use crate::{DiagnosticCategory, RelatedInfo};

fn chain(code: u32, category: DiagnosticCategory, text: &str) -> MessageChain {
    MessageChain {
        code,
        category,
        text: text.into(),
        key: None,
        args: Vec::new(),
        next_present: false,
        next: Vec::new(),
        related: Vec::new(),
        repopulate: None,
        without_location: false,
    }
}

/// The ANSI pieces of tsgo's writer, spelled out in the expectations.
fn location(name: &str, line: u32, column: u32) -> String {
    format!("{CYAN}{name}{RESET}:{YELLOW}{line}{RESET}:{YELLOW}{column}{RESET}")
}

fn head(category: &str, color: &str, code: u32) -> String {
    format!("{color}{category}{RESET}{GREY} TS{code}: {RESET}")
}

fn gutter(number: &str) -> String {
    format!("{GUTTER_STYLE}{number}{RESET} ")
}

#[test]
fn js_file_names_select_distinct_sources() {
    let cwd = JsString::from_code_units(&[0x2f, 0xd800]);
    let path = |unit| JsString::from_code_units(&[0x2f, 0xd800, 0x2f, unit, 0x2e, 0x74, 0x73]);
    let mut files = BTreeMap::new();
    files.insert(path(0xd800), "first".to_owned());
    files.insert(path(0xd801), "second".to_owned());
    files.insert(path(0xfffd), "replacement".to_owned());
    let host = FormatDiagnosticsHost::new_js(cwd.as_js(), &files);
    let diagnostics = [0xfffd, 0xd801, 0xd800].map(|unit| {
        Diagnostic::new_js(
            Some(path(unit)),
            Some(0),
            Some(1),
            chain(1000, DiagnosticCategory::Error, "name"),
        )
    });
    assert_eq!(
        sort_and_dedupe_diagnostic_indices_with_context(&diagnostics, &host),
        [2, 1, 0]
    );
    // Each name reads its own text, and the location keeps the lone
    // surrogate of the name.
    let output = format_diagnostics_with_color_and_context(&diagnostics[1..2], &host, "\n")
        .unwrap()
        .to_utf16();
    let mut expected = CYAN.encode_utf16().collect::<Vec<_>>();
    expected.push(0xd801);
    expected.extend(
        format!(
            ".ts{RESET}:{YELLOW}1{RESET}:{YELLOW}1{RESET} - {}name\n\n{}second\n{}{RED}~{RESET}\n",
            head("error", RED, 1000),
            gutter("1"),
            gutter(" "),
        )
        .encode_utf16(),
    );
    assert_eq!(output, expected);
    let mut alternate = path(0xd801).to_utf16();
    alternate[2] = 0x5c;
    assert_eq!(
        host.file_text(JsString::from_code_units(&alternate).as_js()),
        Some("second")
    );
}

#[test]
fn renders_tsgo_pretty_shape_with_chains_and_related() {
    // tsgo FormatDiagnosticWithColorAndContext: the snippet follows a blank
    // line; each related location carries its message on the same line and
    // its snippet indented under it.
    let mut files = BTreeMap::new();
    files.insert(
        "/workspace/src/main.ts".to_owned(),
        "const\tface = \"😀\";\r\nconst value = 1;\r\n".to_owned(),
    );
    files.insert(
        "/workspace/src/origin.ts".to_owned(),
        "export const origin = 1;\n".to_owned(),
    );
    let mut message = chain(2322, DiagnosticCategory::Error, "Head");
    message.next_present = true;
    message.next = vec![chain(2322, DiagnosticCategory::Error, "Child")];
    let mut diagnostic = Diagnostic::new(
        Some("/workspace/src/main.ts".to_owned()),
        Some(6),
        Some(6),
        message,
    );
    diagnostic.related.push(RelatedInfo {
        file_name: Some("/workspace/src/origin.ts".into()),
        start: Some(13),
        length: Some(6),
        message: chain(2728, DiagnosticCategory::Message, "Origin"),
    });
    let host = FormatDiagnosticsHost::new("/workspace", &files);
    let expected = format!(
        "{} - {}Head\n  Child\n\n{}const face = \"😀\";\n{}{RED}      ~~~~~~{RESET}\n\n  {} - Origin\n    {}export const origin = 1;\n    {}{CYAN}             ~~~~~~{RESET}\n",
        location("src/main.ts", 1, 7),
        head("error", RED, 2322),
        gutter("1"),
        gutter(" "),
        location("src/origin.ts", 1, 14),
        gutter("1"),
        gutter(" "),
    );
    assert_eq!(
        format_diagnostics_with_color_and_context(&[diagnostic], &host, "\n")
            .unwrap()
            .to_string_lossy(),
        expected
    );
}

#[test]
fn snippets_follow_tsgo_write_code_snippet() {
    let mut files = BTreeMap::new();
    files.insert("multi.ts".to_owned(), "a\nb\nc\nd\ne\nf\n".to_owned());
    files.insert("space.ts".to_owned(), "x \u{85}\u{feff}\n".to_owned());
    let host = FormatDiagnosticsHost::new("/", &files);
    // Over five lines: the first two and the last two around an ellipsis;
    // a suggestion squiggles in grey.
    let suggestion = Diagnostic::new(
        Some("multi.ts".to_owned()),
        Some(0),
        Some(10),
        chain(80001, DiagnosticCategory::Suggestion, "hint"),
    );
    let ellipsis = format!("{GUTTER_STYLE}...{RESET} ");
    assert_eq!(
        format_diagnostics_with_color_and_context(&[suggestion], &host, "\n")
            .unwrap()
            .to_string_lossy(),
        format!(
            "{} - {}hint\n\n{}a\n{}{GREY}~{RESET}\n{}b\n{}{GREY}~{RESET}\n{ellipsis}\n{}e\n{}{GREY}~{RESET}\n{}f\n{}{GREY}{RESET}\n",
            location("multi.ts", 1, 1),
            head("suggestion", GREY, 80001),
            gutter("  1"),
            gutter("   "),
            gutter("  2"),
            gutter("   "),
            gutter("  5"),
            gutter("   "),
            gutter("  6"),
            gutter("   "),
        )
    );
    // A zero-length span squiggles the next character; Go's unicode.IsSpace
    // trims U+0085 but not U+FEFF. A message is blue, and a related row
    // without a file is only a line break.
    let mut zero = Diagnostic::new(
        Some("space.ts".to_owned()),
        Some(0),
        Some(0),
        chain(1, DiagnosticCategory::Message, "zero"),
    );
    zero.related.push(RelatedInfo {
        file_name: None,
        start: None,
        length: None,
        message: chain(2, DiagnosticCategory::Message, "unseen"),
    });
    assert_eq!(
        format_diagnostics_with_color_and_context(&[zero], &host, "\r\n")
            .unwrap()
            .to_string_lossy(),
        format!(
            "{} - {}zero\r\n\r\n{}x \u{85}\u{feff}\r\n{}{BLUE}~{RESET}\r\n\r\n",
            location("space.ts", 1, 1),
            head("message", BLUE, 1),
            gutter("1"),
            gutter(" "),
        )
    );
}

#[test]
fn diagnostics_are_separated_by_the_new_line_and_files_are_optional() {
    let files = BTreeMap::new();
    let host = FormatDiagnosticsHost::new("/", &files);
    let diagnostics = [
        Diagnostic::new(
            None,
            None,
            None,
            chain(1, DiagnosticCategory::Error, "first"),
        ),
        Diagnostic::new(
            None,
            None,
            None,
            chain(2, DiagnosticCategory::Warning, "second"),
        ),
    ];
    assert_eq!(
        format_diagnostics_with_color_and_context(&diagnostics, &host, "\n")
            .unwrap()
            .to_string_lossy(),
        format!(
            "{}first\n{}second",
            head("error", RED, 1),
            head("warning", YELLOW, 2)
        )
    );
}

#[test]
fn error_summary_follows_tsgo_write_error_summary_text() {
    let mut files = BTreeMap::new();
    files.insert("/work/b.ts".to_owned(), "x\ny\n".to_owned());
    files.insert("/work/a.ts".to_owned(), "x\n".to_owned());
    let host = FormatDiagnosticsHost::new("/work", &files);
    let error = |file: &str, start: u32| {
        Diagnostic::new(
            Some(file.to_owned()),
            Some(start),
            Some(1),
            chain(1, DiagnosticCategory::Error, "e"),
        )
    };
    let summary = |diagnostics: &[Diagnostic]| {
        write_error_summary_text(diagnostics, &host, "\n")
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    let global = Diagnostic::new(None, None, None, chain(1, DiagnosticCategory::Error, "g"));
    let warning = Diagnostic::new(None, None, None, chain(1, DiagnosticCategory::Warning, "w"));
    assert_eq!(summary(&[warning]), "");
    assert_eq!(
        summary(std::slice::from_ref(&global)),
        "\nFound 1 error.\n\n"
    );
    assert_eq!(
        summary(&[error("/work/b.ts", 2)]),
        format!("\nFound 1 error in b.ts{GREY}:2{RESET}\n\n")
    );
    assert_eq!(
        summary(&[global.clone(), global.clone()]),
        "\nFound 2 errors.\n\n"
    );
    assert_eq!(
        summary(&[error("/work/b.ts", 2), error("/work/b.ts", 0)]),
        format!("\nFound 2 errors in the same file, starting at: b.ts{GREY}:2{RESET}\n\n")
    );
    // Files in name order; the line is that of the file's first error.
    assert_eq!(
        summary(&[error("/work/b.ts", 2), error("/work/a.ts", 0), global]),
        format!(
            "\nFound 3 errors in 2 files.\n\nErrors  Files\n     1  a.ts{GREY}:1{RESET}\n     1  b.ts{GREY}:2{RESET}\n\n"
        )
    );
}

#[test]
fn cwd_aware_selection_returns_the_retained_input_occurrence() {
    let files = BTreeMap::new();
    let host = FormatDiagnosticsHost::new("/work", &files);
    let first = Diagnostic::new(
        Some("src/../a.ts".to_owned()),
        Some(0),
        Some(1),
        chain(1, DiagnosticCategory::Error, "same"),
    );
    let second = Diagnostic::new(
        Some("a.ts".to_owned()),
        Some(0),
        Some(1),
        chain(1, DiagnosticCategory::Error, "same"),
    );

    assert_eq!(
        sort_and_dedupe_diagnostic_indices_with_context(&[first, second], &host,),
        [0]
    );
}

#[test]
fn sorts_by_virtual_absolute_path_and_prints_relative_names() {
    assert_eq!(
        relative_file_name("//server/share/a.ts", "/work"),
        "//server/share/a.ts"
    );
    assert_eq!(
        absolute_virtual_path("/z/../a.ts", "/work"),
        "/a.ts",
        "the sort twin is SourceFile.path, not the raw SourceFile.fileName"
    );
    assert_eq!(
        relative_file_name("/z/../a.ts", "/work"),
        "../a.ts",
        "display conversion reduces the raw absolute SourceFile.fileName"
    );
    let files = BTreeMap::new();
    let diagnostic = |file: &str, code| {
        Diagnostic::new(
            Some(file.to_owned()),
            Some(0),
            Some(1),
            chain(code, DiagnosticCategory::Error, "x"),
        )
    };
    let host = FormatDiagnosticsHost::new("/work", &files);
    assert_eq!(
        sort_and_dedupe_diagnostic_indices_with_context(
            &[
                diagnostic("../z.ts", 2),
                diagnostic("./nested/../dot.ts", 3),
                diagnostic("a.ts", 1),
            ],
            &host
        ),
        [2, 1, 0]
    );
}
