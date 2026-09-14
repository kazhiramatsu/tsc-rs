use serde_json::{json, Value};
use tsc_diagnostics::Diagnostic;
use tsc_syntax::{LanguageVariant, ParseOptions, ParseRecoveryKind};

fn diagnostics(rows: &[Diagnostic]) -> Vec<Value> {
    rows.iter()
        .map(|diagnostic| {
            assert!(diagnostic.message.next.is_empty());
            json!({
                "code": diagnostic.code(), "start": diagnostic.start, "length": diagnostic.length,
                "message_utf16": diagnostic.message.text.to_utf16(),
            })
        })
        .collect()
}

#[test]
fn recovery_controls_preserve_repeated_typescript_syntax_diagnostics() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/utf16-recovery-boundary.json")).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    for case in fixture["cases"].as_array().unwrap() {
        let file_name = case["file_name"].as_str().unwrap();
        let parsed = tsc_syntax::parse_source_file(
            file_name,
            case["source"].as_str().unwrap(),
            ParseOptions {
                javascript_file: file_name.ends_with(".js"),
                language_variant: if file_name.ends_with(".tsx") {
                    LanguageVariant::Jsx
                } else {
                    LanguageVariant::Standard
                },
                ..ParseOptions::default()
            },
            None,
        );
        assert_eq!(
            json!({
                "diagnostics": diagnostics(&parsed.parse_diagnostics),
                "jsdoc_diagnostics": diagnostics(&parsed.js_doc_diagnostics),
            }),
            case["expected"],
            "{}",
            case["case_id"]
        );

        let recovery = parsed.parse_recovery();
        assert_eq!(
            recovery.diagnostic_origins().len(),
            parsed.parse_diagnostics.len()
        );
        let mut covered = vec![false; parsed.parse_diagnostics.len()];
        for event in recovery.events() {
            if let Some(index) = event.diagnostic_index {
                assert!(!covered[index]);
                covered[index] = true;
                assert_eq!(
                    event.kind,
                    ParseRecoveryKind::Diagnostic(recovery.diagnostic_origins()[index])
                );
                assert_eq!(Some(event.start), parsed.parse_diagnostics[index].start);
                assert_eq!(Some(event.length), parsed.parse_diagnostics[index].length);
            }
        }
        assert!(covered.into_iter().all(|covered| covered));
        // The admission verdict is the reviewed parser policy. The preceding
        // diagnostic comparison is the separate TypeScript observation.
        assert_eq!(
            parsed.has_only_literal_recovery(),
            case["literal_only_contract"].as_bool().unwrap(),
            "{}",
            case["case_id"]
        );
    }
}
