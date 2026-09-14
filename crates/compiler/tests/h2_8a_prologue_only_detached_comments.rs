//! Source files whose statements are all prologue directives keep tsc's
//! emission order: the prologues first, then the file's detached comment
//! prefix again (emitBodyWithDetachedComments runs for the source file after
//! the prologues), with nothing else to follow. The fixture is the pinned
//! compiler's emitted JavaScript text, exit code and diagnostic codes.
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, PreparedProgram, ProgramLoadLimits,
    ProgramOptions,
};

const FIXTURE: &[u8] = include_bytes!("fixtures/prologue-only-detached-comments.json");

fn prepare(source: &str) -> PreparedProgram {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut host = MemoryCompilerHost::builder("/project")
        .file("/project/main.ts", source.as_bytes().to_vec());
    for entry in std::fs::read_dir(workspace.join("vendor/typescript-6.0.3/lib")).unwrap() {
        let entry = entry.unwrap();
        let name = entry
            .file_name()
            .into_string()
            .expect("scalar vendored library name");
        if name.starts_with("lib.") && name.ends_with(".d.ts") {
            host = host.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
        }
    }
    let host = host.build().unwrap();
    let options = CompilerOptions {
        target: Some(2),
        new_line: Some(1),
        skip_default_lib_check: Some(true),
        no_error_truncation: Some(true),
        ..Default::default()
    };
    load_emitting_program(
        &host,
        &[PathBuf::from("/project/main.ts")],
        options,
        ProgramOptions::default(),
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .unwrap()
}

#[test]
fn prologue_only_files_emit_the_detached_prefix_after_the_prologues() {
    let fixture: Value = serde_json::from_slice(FIXTURE).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["typescript"], "6.0.3");
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(source.as_bytes())),
            case["source_sha256"]
        );
        for attempt in 0..2 {
            let mut sink = MemoryOutputSink::new();
            let command = ProgramSession::new(prepare(source))
                .emit_command_for_harness(&mut sink)
                .unwrap_or_else(|error| panic!("{id}: production command failed: {error}"));
            let writes = sink.writes();
            let codes = command
                .diagnostics()
                .iter()
                .map(|diagnostic| Value::from(diagnostic.code()))
                .collect::<Vec<_>>();
            let actual = (
                writes.len(),
                writes.first().map(|write| write.callback_text().to_owned()),
                command.exit_code(),
                Value::Array(codes),
            );
            let expected = (
                1usize,
                Some(case["js_text"].as_str().unwrap().to_owned()),
                case["exit_code"].as_i64().unwrap() as i32,
                case["diagnostic_codes"].clone(),
            );
            if actual != expected {
                failures.push(format!(
                    "{id} (attempt {attempt}):\n  expected {expected:?}\n  actual   {actual:?}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "prologue-only emission divergences:\n{}",
        failures.join("\n")
    );
}
