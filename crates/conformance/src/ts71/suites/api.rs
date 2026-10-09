//! tsgo's API encoder tests (`api/encoder/encoder_test.go`):
//! `TestEncodeSourceFile` and `TestEncodeSourceFileWithUnicodeEscapes`
//! encode a parsed `/test.ts` and write `api/<name>.txt` with
//! `formatEncodedSourceFile`.

use tsc_api::encoder::{
    encode_source_file, format_encoded_source_file, ScriptKind, SourceFileFacts,
};
use tsc_api::parse_source_file;

/// The baselines: name and the test's source text.
pub(super) fn cases() -> Vec<(String, String)> {
    vec![
        (
            "encodeSourceFile.txt".to_owned(),
            "import { bar } from \"bar\";\nexport function foo<T, U>(a: string, b: string): any {}\nfoo();"
                .to_owned(),
        ),
        (
            "encodeSourceFileWithUnicodeEscapes.txt".to_owned(),
            // The Go test's raw string, `~` standing for each escape's backslash.
            "let a = \"\u{1F603}\"; let b = \"~ud83d~ude03\"; let c = \"~udc00~ud83d~ude03\"; let d = \"~ud83d~ud83d~ude03\""
                .replace('~', "\\"),
        ),
    ]
}

/// The baseline of one case: the source parsed as `/test.ts` (the tests
/// parse without a program, so the file has no imports beyond its own),
/// encoded and formatted.
pub(super) fn render(text: &str) -> String {
    let file = parse_source_file("/test.ts", text, ScriptKind::Ts);
    let facts = SourceFileFacts {
        script_kind: ScriptKind::Ts,
        ..SourceFileFacts::default()
    };
    format_encoded_source_file(&encode_source_file(&file, &facts).0)
}
