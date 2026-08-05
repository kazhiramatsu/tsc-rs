use tsc_syntax::{parse_source_file, ParseOptions};

use super::get_js_syntactic_diagnostics;

fn js_syntactic_diagnostics(text: &str) -> Vec<tsc_diagnostics::Diagnostic> {
    let source = parse_source_file(
        "a.js".to_owned(),
        text.to_owned(),
        ParseOptions {
            javascript_file: true,
            ..ParseOptions::default()
        },
        None,
    );
    get_js_syntactic_diagnostics(&source, false)
}

#[test]
fn decorators_split_by_export_carry_1486_related_information_in_js() {
    for (text, trailing_start) in [
        ("@dec export @dec class C6 {}", 12),
        ("@dec export default @dec class C7 {}", 20),
    ] {
        let diagnostics = js_syntactic_diagnostics(text);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 8038)
            .expect("TS8038");
        assert_eq!(
            (diagnostic.start, diagnostic.length),
            (Some(trailing_start), Some(4))
        );
        assert_eq!(diagnostic.related.len(), 1);
        let related = &diagnostic.related[0];
        assert_eq!(related.message.code, 1486);
        assert_eq!(related.message.text, "Decorator used before 'export' here.");
        assert_eq!((related.start, related.length), (Some(0), Some(4)));
    }
}

#[test]
fn decorators_on_only_one_side_of_export_do_not_report_8038_in_js() {
    for text in [
        "@dec export class C1 {}",
        "@dec export default class C2 {}",
        "export @dec class C4 {}",
        "export default @dec class C5 {}",
    ] {
        assert!(
            js_syntactic_diagnostics(text)
                .iter()
                .all(|diagnostic| diagnostic.code() != 8038),
            "unexpected TS8038 for {text}"
        );
    }
}
