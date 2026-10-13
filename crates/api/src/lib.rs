//! TypeScript 7.1's API (tsgo `internal/api` and `internal/ipc`) for
//! tsc-rs.
//!
//! [`server`] runs `tsc-rs --api`: a [`session::Session`] over tsgo's
//! project system ([`tsc_project`]), reached through an [`ipc::Conn`] with
//! the MessagePack ([`msgpack`]) or JSON-RPC ([`ipc::jsonrpc`]) protocol.
//! [`encoder`] writes a source file in the binary format tsgo's API sends
//! its clients (`internal/api/encoder`, protocol 9).

mod astnav;
mod build;
pub mod callback_fs;
mod checker;
pub mod decoder;
mod diagnostics;
mod emit;
pub mod encoder;
pub mod ipc;
pub mod module_resolution;
pub mod msgpack;
mod print;
pub mod proto;
pub mod references;
pub mod request_fs;
pub mod server;
pub mod session;
mod symbols;

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
    tsc_syntax::parse_source_file(file_name, text, created_parse_options(script_kind), None)
}

/// The parse options of a file of `script_kind` with no module-indicator
/// option: those a Program computes for the same file when none applies
/// (tsgo's created file has the zero `ExternalModuleIndicatorOptions`).
fn created_parse_options(script_kind: ScriptKind) -> ParseOptions {
    if script_kind == ScriptKind::Json {
        return ParseOptions::default();
    }
    ParseOptions {
        language_variant: match script_kind {
            ScriptKind::Ts | ScriptKind::Unknown => LanguageVariant::Standard,
            _ => LanguageVariant::Jsx,
        },
        javascript_file: matches!(script_kind, ScriptKind::Js | ScriptKind::Jsx),
        ..ParseOptions::default()
    }
}

/// tsgo `AcquireSourceFile`'s parse of a created file: allocated in `domain`
/// (the shared documents'), with the parse options it is found by.
pub(crate) fn parse_created_source_file(
    file_name: &str,
    text: &str,
    script_kind: ScriptKind,
    domain: &tsc_types::IdentityDomain,
) -> (SourceFile, ParseOptions) {
    let snapshot = tsc_diagnostics::TextSnapshot::new(text, Default::default());
    let options = created_parse_options(script_kind);
    let source = if script_kind == ScriptKind::Json {
        tsc_syntax::parse_json_text_from_snapshot_in_identity_domain(file_name, snapshot, domain)
    } else {
        tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
            file_name,
            snapshot,
            options.clone(),
            None,
            domain,
        )
    }
    .expect("created source identity allocation failed");
    (source, options)
}
