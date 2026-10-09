//! tsgo `api/proto_test.go`, and tsgo's URI conversion
//! (`DocumentUri.FileName`) pinned to its output.

use tsc_diagnostics::{Diagnostic, MessageChain};

use super::*;

#[test]
fn document_identifiers_decode_like_tsgo() {
    // TestDocumentIdentifierUnmarshalJSON.
    let cases: [(&str, &str, &str, Option<&str>); 5] = [
        (r#""foo.ts""#, "foo.ts", "", None),
        (r#"{"uri":"file:///foo.ts"}"#, "", "file:///foo.ts", None),
        (
            r#"{"uri":"file:///foo.ts","extra":true}"#,
            "",
            "file:///foo.ts",
            None,
        ),
        ("{}", "", "", None),
        ("42", "", "", Some("expected string or object, got number")),
    ];
    for (input, file_name, uri, error) in cases {
        match serde_json::from_str::<DocumentIdentifier>(input) {
            Ok(document) => {
                assert_eq!(error, None, "{input}");
                assert_eq!(document.file_name, file_name, "{input}");
                assert_eq!(document.uri, uri, "{input}");
            }
            Err(actual) => {
                let expected = error.unwrap_or_else(|| panic!("{input}: {actual}"));
                assert!(actual.to_string().contains(expected), "{input}: {actual}");
            }
        }
    }
}

#[test]
fn ensure_programs_decode_like_tsgo() {
    // TestEnsureProgramsUnmarshalJSON.
    assert_eq!(
        serde_json::from_str::<EnsurePrograms>("true").unwrap(),
        EnsurePrograms::All
    );
    assert_eq!(
        serde_json::from_str::<EnsurePrograms>(r#"["/tsconfig.json","/dev/null/synthetic/1"]"#)
            .unwrap(),
        EnsurePrograms::Projects(vec![
            "/tsconfig.json".to_owned(),
            "/dev/null/synthetic/1".to_owned(),
        ])
    );
    let error = serde_json::from_str::<EnsurePrograms>("false").unwrap_err();
    assert!(
        error.to_string().contains("must be true or an array"),
        "{error}"
    );
}

#[test]
fn synthetic_project_ids_are_validated_and_canonical() {
    // tsgo `SyntheticProjectID.UnmarshalJSONFrom`.
    assert_eq!(
        serde_json::from_str::<SyntheticProjectIdParam>(r#""/dev/null/synthetic/01""#)
            .unwrap()
            .0,
        "/dev/null/synthetic/1"
    );
    let error = serde_json::from_str::<SyntheticProjectIdParam>(r#""/tsconfig.json""#)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("invalid synthetic project ID: /tsconfig.json"),
        "{error}"
    );
}

fn diagnostic_at(file_name: &str, start: u32, length: u32) -> Diagnostic {
    Diagnostic::new(
        Some(file_name.to_owned()),
        Some(start),
        Some(length),
        MessageChain::new(&tsc_diagnostics::gen::Expression_expected, &[]),
    )
}

#[test]
fn a_diagnostic_response_has_its_formatting_context() {
    // TestNewDiagnosticResponseIncludesFormattingContext: positions in
    // UTF-16 units, the emoji two of them.
    let text = "const 💩 = 1;";
    let start = text[..text.find('=').unwrap()].encode_utf16().count() as u32;
    let response = DiagnosticResponse::new(&diagnostic_at("/unicode.ts", start, 1), &|_| {
        Some(text.to_owned())
    });
    assert_eq!(response.pos, 9);
    assert_eq!(response.end, 10);
    assert_eq!(
        response.start_position,
        Some(DiagnosticPositionResponse {
            line: 0,
            character: 9
        })
    );
    assert_eq!(
        response.end_position,
        Some(DiagnosticPositionResponse {
            line: 0,
            character: 10
        })
    );
    assert_eq!(
        response.source_lines,
        [DiagnosticSourceLineResponse {
            line: 0,
            text: text.to_owned()
        }]
    );
}

#[test]
fn a_long_diagnostic_context_keeps_its_first_and_last_two_lines() {
    // TestNewDiagnosticResponseTruncatesLongFormattingContext.
    let text = "one\ntwo\nthree\nfour\nfive\nsix\nseven";
    let length = text.encode_utf16().count() as u32;
    let response = DiagnosticResponse::new(&diagnostic_at("/multiline.ts", 0, length), &|_| {
        Some(text.to_owned())
    });
    assert_eq!(
        response.start_position,
        Some(DiagnosticPositionResponse {
            line: 0,
            character: 0
        })
    );
    assert_eq!(
        response.end_position,
        Some(DiagnosticPositionResponse {
            line: 6,
            character: 5
        })
    );
    let lines = response
        .source_lines
        .iter()
        .map(|line| (line.line, line.text.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        [(0, "one\n"), (1, "two\n"), (5, "six\n"), (6, "seven")]
    );
}

#[test]
fn a_diagnostic_response_round_trips_to_a_diagnostic() {
    // tsgo `ToDiagnostic`: a client's config file parsing diagnostic.
    let response: DiagnosticResponse = serde_json::from_str(
        r#"{"pos":1,"end":3,"code":6046,"category":1,"text":"given","messageChain":[{"pos":0,"end":0,"code":1,"category":3,"text":"detail"}]}"#,
    )
    .unwrap();
    let diagnostic = response.to_diagnostic();
    assert_eq!(diagnostic.file_name, None);
    assert_eq!((diagnostic.start, diagnostic.length), (Some(1), Some(2)));
    assert_eq!(diagnostic.message.code, 6046);
    assert_eq!(diagnostic.message.text, "given");
    assert_eq!(diagnostic.message.next[0].text, "detail");
    let again = DiagnosticResponse::new(&diagnostic, &|_| None);
    assert_eq!(
        serde_json::to_string(&again).unwrap(),
        r#"{"pos":1,"end":3,"code":6046,"category":1,"text":"given","messageChain":[{"pos":1,"end":3,"code":1,"category":3,"text":"detail"}]}"#
    );
}

#[test]
fn uris_become_file_names_like_tsgo() {
    // tsgo `DocumentUri.FileName` at 19dadef8, probed.
    for (uri, file_name) in [
        ("file:///D%3A/repo/index.ts", "D:/repo/index.ts"),
        ("file:///a/b%20c.ts", "/a/b c.ts"),
        ("file://server/share/x.ts", "//server/share/x.ts"),
        ("bundled:///libs/lib.d.ts", "bundled:///libs/lib.d.ts"),
        (
            "untitled:Untitled-1",
            "^/untitled/ts-nul-authority/Untitled-1",
        ),
        ("vscode-vfs://github/x/y.ts", "^/vscode-vfs/github/x/y.ts"),
        ("file:///c:/x.ts", "c:/x.ts"),
        ("file:///home/p/a.ts?query#frag", "/home/p/a.ts"),
        ("file:///%E3%81%82.ts", "/あ.ts"),
    ] {
        assert_eq!(uri_to_file_name(uri).as_deref(), Ok(file_name), "{uri}");
    }
    assert!(uri_to_file_name("file:///a%zz").is_err());
}
