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
    get_js_syntactic_diagnostics(&source)
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

fn rows(diagnostics: &[tsc_diagnostics::Diagnostic]) -> Vec<(u32, u32, u32)> {
    let mut rows = diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code(),
                diagnostic.start.expect("start"),
                diagnostic.length.expect("length"),
            )
        })
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

#[test]
fn typescript_only_syntax_reports_at_tsgo_ranges() {
    // tsgo parser checkJSSyntax (7.1 at 19dadef8): a signature without a
    // body spans the whole node (a class index signature included), `x!` is
    // checked inside `as`, and a function type's parameters are not checked.
    let text = "let f: (x?: number) => void;\n\
                let o = x! as Foo;\n\
                class K { [k: string]: number; m(): void; }\n\
                interface I { m(): void }\n\
                function g<T>(a: T): T {}\n";
    let at = |needle: &str| text.find(needle).expect("needle") as u32;
    let mut expected = vec![
        (8010, at("(x?"), "(x?: number) => void".len() as u32),
        (8013, at("x!"), 2),
        (8016, at("Foo"), 3),
        (8017, at("[k"), "[k: string]: number;".len() as u32),
        (8017, at("m(): void;"), "m(): void;".len() as u32),
        (8006, at("I {"), 1),
        (8004, at("T>"), 1),
        (8010, at("T)"), 1),
        (8010, at("T {}"), 1),
    ];
    expected.sort();
    assert_eq!(rows(&js_syntactic_diagnostics(text)), expected);
}

#[test]
fn unchecked_js_parameter_decorators_report_at_the_decorator() {
    // tsgo getAdditionalJSSyntacticDiagnostics: without experimentalDecorators
    // a parameter decorator of an unchecked JavaScript file is TS1206 at the
    // decorator (parameterDecoratorInJsFile(checkjs=false)).
    let text = "class Foo {\n    method(@dec x) {}\n}\n";
    let source = parse_source_file(
        "a.js".to_owned(),
        text.to_owned(),
        ParseOptions {
            javascript_file: true,
            ..ParseOptions::default()
        },
        None,
    );
    assert_eq!(
        rows(&super::get_additional_js_syntactic_diagnostics(&source)),
        [(1206, text.find("@dec").expect("decorator") as u32, 4)]
    );
}
