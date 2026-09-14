use super::*;
use crate::{DiagnosticCategory, RelatedInfo};

fn chain(code: u32, category: DiagnosticCategory, text: &str) -> MessageChain {
    MessageChain {
        code,
        category,
        text: text.into(),
        next_present: false,
        next: Vec::new(),
    }
}

#[test]
fn js_file_names_select_distinct_sources_and_survive_location_rendering() {
    let cwd = JsString::from_code_units(&[0x2f, 0xd800]);
    let path = |unit| JsString::from_code_units(&[0x2f, 0xd800, 0x2f, unit, 0x2e, 0x74, 0x73]);
    let mut files = BTreeMap::new();
    files.insert(path(0xd800), "first".to_owned());
    files.insert(path(0xd801), "second".to_owned());
    files.insert(path(0xfffd), "replacement".to_owned());
    let host = FormatDiagnosticsHost::new_js(cwd.as_js(), &files);
    let mut diagnostics = [0xfffd, 0xd801, 0xd800].map(|unit| {
        Diagnostic::new_js(
            Some(path(unit)),
            Some(0),
            Some(1),
            chain(1000, DiagnosticCategory::Error, "name"),
        )
    });
    diagnostics[2].related.push(RelatedInfo {
        file_name: Some(path(0xd801)),
        start: Some(0),
        length: Some(1),
        message: chain(1001, DiagnosticCategory::Message, "origin"),
    });
    let output = format_diagnostics_with_context_raw(&diagnostics, &host).unwrap();
    let mut expected = vec![0xd800];
    expected.extend(".ts:1:1 - error TS1000: name\n\n1 first\n  ~\n\n  ".encode_utf16());
    expected.push(0xd801);
    expected.extend(".ts:1:1\n    1 second\n      ~\n    origin\n".encode_utf16());
    for (unit, source) in [(0xd801, "second"), (0xfffd, "replacement")] {
        expected.push(unit);
        expected
            .extend(format!(".ts:1:1 - error TS1000: name\n\n1 {source}\n  ~\n").encode_utf16());
    }
    assert_eq!(output.to_utf16(), expected);
    assert_eq!(
        sort_and_dedupe_diagnostic_indices_with_context(&diagnostics, &host),
        [2, 1, 0]
    );
    let mut alternate = path(0xd801).to_utf16();
    alternate[2] = 0x5c;
    assert_eq!(
        host.file_text(JsString::from_code_units(&alternate).as_js()),
        Some("second")
    );
}

#[test]
fn renders_tsc_context_shape_with_utf16_tabs_chains_and_related() {
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
    assert_eq!(
        format_diagnostics_with_context(&[diagnostic], &host).unwrap(),
        concat!(
            "src/main.ts:1:7 - error TS2322: Head\n",
            "  Child\n",
            "\n",
            "1 const face = \"😀\";\n",
            "        ~~~~~~\n",
            "\n",
            "  src/origin.ts:1:14\n",
            "    1 export const origin = 1;\n",
            "                   ~~~~~~\n",
            "    Origin\n",
        )
    );
}

#[test]
fn owns_order_dedupe_multiline_fileless_and_suggestion_rendering() {
    let mut files = BTreeMap::new();
    files.insert("multi.ts".to_owned(), "a\nb\nc\nd\ne\nf\n".to_owned());
    let fileless = Diagnostic::new(
        None,
        None,
        None,
        chain(999, DiagnosticCategory::Message, "global"),
    );
    let suggestion = Diagnostic::new(
        Some("multi.ts".to_owned()),
        Some(0),
        Some(10),
        chain(80001, DiagnosticCategory::Suggestion, "hint"),
    );
    let host = FormatDiagnosticsHost::new("/", &files);
    let output =
        format_diagnostics_with_context(&[suggestion.clone(), fileless, suggestion], &host)
            .unwrap();
    assert_eq!(
        output,
        concat!(
            "message TS999: global\n",
            "multi.ts:1:1 - suggestion TS80001: hint\n",
            "\n",
            "  1 a\n",
            "    ~\n",
            "  2 b\n",
            "    ~\n",
            "... \n",
            "  5 e\n",
            "    ~\n",
            "  6 f\n",
            "    \n",
        )
    );
}

#[test]
fn present_empty_related_information_emits_the_tsc_blank_line() {
    let files = BTreeMap::new();
    let mut present_empty = Diagnostic::new(
        None,
        None,
        None,
        chain(1, DiagnosticCategory::Error, "first"),
    );
    present_empty.related_information_present = true;
    let absent = Diagnostic::new(
        None,
        None,
        None,
        chain(2, DiagnosticCategory::Error, "second"),
    );
    let host = FormatDiagnosticsHost::new("/", &files);

    assert_eq!(
        format_sorted_diagnostics_with_context(&[present_empty, absent], &host).unwrap(),
        "error TS1: first\n\nerror TS2: second\n"
    );
}

#[test]
fn raw_formatter_preserves_message_newlines() {
    let files = BTreeMap::new();
    let diagnostic = Diagnostic::new(
        None,
        None,
        None,
        chain(1, DiagnosticCategory::Error, "head\rbody\r\ntail"),
    );
    let host = FormatDiagnosticsHost::new("/", &files);

    assert_eq!(
        format_sorted_diagnostics_with_context_raw(std::slice::from_ref(&diagnostic), &host,)
            .unwrap(),
        "error TS1: head\rbody\r\ntail\n"
    );
    assert_eq!(
        format_sorted_diagnostics_with_context(&[diagnostic], &host).unwrap(),
        "error TS1: head\nbody\ntail\n"
    );
}

#[test]
fn raw_and_normalized_formatters_keep_message_surrogates() {
    let files = BTreeMap::new();
    let host = FormatDiagnosticsHost::new("/", &files);
    let mut message = chain(1, DiagnosticCategory::Error, "");
    message.text = JsString::from_code_units(&[0xD800, 13, 10, 0xDC00]);
    let diagnostic = Diagnostic::new(None, None, None, message);
    let mut expected = "error TS1: ".encode_utf16().collect::<Vec<_>>();
    expected.extend([0xD800, 13, 10, 0xDC00, 10]);
    assert_eq!(
        format_sorted_diagnostics_with_context_raw(std::slice::from_ref(&diagnostic), &host)
            .unwrap()
            .to_utf16(),
        expected
    );
    expected.remove("error TS1: ".len() + 1);
    assert_eq!(
        format_sorted_diagnostics_with_context(&[diagnostic], &host)
            .unwrap()
            .to_utf16(),
        expected
    );
}

#[test]
fn removing_sgr_canonicalizes_a_new_surrogate_pair() {
    let value = JsString::from_code_units(&[0xD800, 0x1B, 0x5B, 0x33, 0x31, 0x6D, 0xDC00]);
    let stripped = strip_ansi_sgr(value.as_js());
    assert_eq!(stripped.as_bytes(), "\u{10000}".as_bytes());
    let unclosed = JsString::from_code_units(&[0xD800, 0x1B, 0x5B, 0x33, 0x31, 0xDC00]);
    assert_eq!(strip_ansi_sgr(unclosed.as_js()), unclosed);
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
fn sorts_by_virtual_absolute_path_and_clamps_trimmed_line_spans() {
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
    let mut files = BTreeMap::new();
    files.insert("../z.ts".to_owned(), "x   \n".to_owned());
    files.insert("./nested/../dot.ts".to_owned(), "d\n".to_owned());
    files.insert("a.ts".to_owned(), "y\n".to_owned());
    let z = Diagnostic::new(
        Some("../z.ts".to_owned()),
        Some(4),
        Some(0),
        chain(2, DiagnosticCategory::Error, "z"),
    );
    let a = Diagnostic::new(
        Some("a.ts".to_owned()),
        Some(0),
        Some(1),
        chain(1, DiagnosticCategory::Error, "a"),
    );
    let dot = Diagnostic::new(
        Some("./nested/../dot.ts".to_owned()),
        Some(0),
        Some(1),
        chain(3, DiagnosticCategory::Error, "dot"),
    );
    let host = FormatDiagnosticsHost::new("/work", &files);
    assert_eq!(
        format_diagnostics_with_context(&[z, dot, a], &host).unwrap(),
        concat!(
            "a.ts:1:1 - error TS1: a\n",
            "\n",
            "1 y\n",
            "  ~\n",
            "dot.ts:1:1 - error TS3: dot\n",
            "\n",
            "1 d\n",
            "  ~\n",
            "../z.ts:1:5 - error TS2: z\n",
            "\n",
            "1 x\n",
            "   \n",
        )
    );
}

#[test]
fn strips_input_sgr_after_rendering_like_the_oracle_adapter() {
    let mut files = BTreeMap::new();
    files.insert("./\u{1b}[31ma.ts".to_owned(), "x\u{1b}[32my\n".to_owned());
    let diagnostic = Diagnostic::new(
        Some("./\u{1b}[31ma.ts".to_owned()),
        Some(0),
        Some(7),
        chain(4, DiagnosticCategory::Error, "bad \u{1b}[33mcolor"),
    );
    let host = FormatDiagnosticsHost::new("/work", &files);

    assert_eq!(
        format_diagnostics_with_context(&[diagnostic], &host).unwrap(),
        concat!(
            "a.ts:1:1 - error TS4: bad color\n",
            "\n",
            "1 xy\n",
            "  ~~~~~~~\n",
        )
    );
}
