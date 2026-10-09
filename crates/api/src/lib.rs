//! TypeScript 7.1's API surface for tsc-rs.
//!
//! [`encoder`] writes a source file in the binary format tsgo's API sends
//! its clients (`internal/api/encoder`, protocol 9). The API server itself
//! (tsgo `internal/api`'s session and protocol) is planned separately
//! (docs/design/greenfield/post-emitter-roadmap.md, P5).

pub mod encoder;
pub mod references;

use encoder::ScriptKind;
use tsc_syntax::{LanguageVariant, ParseOptions, SourceFile};

/// Parses `text` as tsgo's API parses a file (`parser.ParseSourceFile`
/// with the script kind of the file name): the JSX variant for every kind
/// but TypeScript, the JavaScript context for JavaScript, every JSDoc
/// comment, and a JSON file as one JSON value.
pub fn parse_source_file(file_name: &str, text: &str) -> SourceFile {
    let script_kind = ScriptKind::from_file_name(file_name);
    if script_kind == ScriptKind::Json {
        return tsc_syntax::parse_json_text(file_name, text);
    }
    let options = ParseOptions {
        language_variant: match script_kind {
            ScriptKind::Ts | ScriptKind::Unknown => LanguageVariant::Standard,
            _ => LanguageVariant::Jsx,
        },
        javascript_file: matches!(script_kind, ScriptKind::Js | ScriptKind::Jsx),
        ..ParseOptions::default()
    };
    tsc_syntax::parse_source_file(file_name, text, options, None)
}
