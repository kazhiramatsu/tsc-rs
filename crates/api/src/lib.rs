//! TypeScript 7.1's API (tsgo `internal/api` and `internal/ipc`) for
//! tsc-rs.
//!
//! [`server`] runs `tsc-rs --api`: a [`session::Session`] over tsgo's
//! project system ([`tsc_project`]), reached through an [`ipc::Conn`] with
//! the MessagePack ([`msgpack`]) or JSON-RPC ([`ipc::jsonrpc`]) protocol.
//! [`encoder`] writes a source file in the binary format tsgo's API sends
//! its clients (`internal/api/encoder`, protocol 9).

mod astnav;
pub mod callback_fs;
mod checker;
mod diagnostics;
mod emit;
pub mod encoder;
pub mod ipc;
pub mod module_resolution;
pub mod msgpack;
pub mod proto;
pub mod references;
pub mod request_fs;
pub mod server;
pub mod session;

use encoder::ScriptKind;
use tsc_syntax::{LanguageVariant, ParseOptions, SourceFile};

/// Parses `text` as tsgo's API parses a file (`parser.ParseSourceFile`)
/// of `script_kind`: the JSX variant for every kind but TypeScript, the
/// JavaScript context for JavaScript, every JSDoc comment, and a JSON file
/// as one JSON value.
pub fn parse_source_file(file_name: &str, text: &str, script_kind: ScriptKind) -> SourceFile {
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
