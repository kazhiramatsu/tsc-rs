//! Command diagnostics for the program loader's static tsgo order fixtures.
use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use serde_json::json;
use tsc_compiler::ProgramSession;
use tsc_diagnostics::{
    flattened_message, sort_and_dedupe_diagnostic_indices_with_context, DocumentVersion,
    FormatDiagnosticsHost, TextSnapshot,
};
use tsc_program::WorkerBudget;

#[path = "../../program/tests/support/loader_order.rs"]
#[allow(dead_code)]
mod corpus;

fn check(name: &str) {
    let fixture = corpus::fixture(name);
    let files: BTreeMap<_, _> = fixture
        .files
        .iter()
        .map(|(name, text)| (format!("/project/{name}"), text.clone()))
        .collect();
    for workers in [
        WorkerBudget::serial(),
        WorkerBudget::new(NonZeroUsize::new(4).unwrap()),
    ] {
        let program = corpus::load(&fixture, workers, false);
        let outcome = ProgramSession::new(program)
            .run_no_emit_command()
            .expect("fixture command session");
        let diagnostics = outcome.into_diagnostics();
        let host = FormatDiagnosticsHost::new("/project", &files);
        let rows: Vec<_> = sort_and_dedupe_diagnostic_indices_with_context(&diagnostics, &host)
            .into_iter()
            .map(|index| {
                let diagnostic = &diagnostics[index];
                let file = diagnostic.file_name.as_ref().map(|f| {
                    f.to_string_lossy()
                        .strip_prefix("/project/")
                        .unwrap_or(&f.to_string_lossy())
                        .to_owned()
                });
                let position = file.as_ref().and_then(|file| {
                    let (_, text) = fixture.files.iter().find(|(name, _)| {
                        if fixture.case_sensitive {
                            *name == file
                        } else {
                            name.eq_ignore_ascii_case(file)
                        }
                    })?;
                    TextSnapshot::new(text.clone(), DocumentVersion::default())
                        .positions()
                        .line_and_character_utf16(diagnostic.start?)
                });
                json!({
                    "file": file,
                    "line": position.map(|p| p.line + 1),
                    "column": position.map(|p| p.character + 1),
                    "code": diagnostic.code(),
                    "message": flattened_message(&diagnostic.message, "\n", None).to_string_lossy(),
                })
            })
            .collect();
        assert_eq!(rows, fixture.diagnostics, "{name}: diagnostics {workers:?}");
    }
}

#[test]
fn missing_directives() {
    check("missing_directives");
}
#[test]
fn missing_roots_gate_directive_diagnostics() {
    check("missing_occurrences");
}
#[test]
fn respelled_parent_diagnostics() {
    check("respell_parent");
}
#[test]
fn extension_suggestions() {
    check("extension_suggestion");
}
#[test]
fn redirected_project_options() {
    check("project_options");
}
#[test]
fn automatic_duplicate_names() {
    check("automatic_order");
}

#[test]
fn incremental_file_root_reference_and_probe_order() {
    let fixture = corpus::fixture("incremental_package_order");
    for workers in [
        WorkerBudget::serial(),
        WorkerBudget::new(NonZeroUsize::new(4).unwrap()),
    ] {
        let outcome = ProgramSession::new(corpus::load(&fixture, workers, false))
            .run_no_emit_command()
            .expect("incremental fixture session");
        let document = outcome.build_info().expect("incremental build info");
        let value: serde_json::Value = serde_json::from_str(&document.text).unwrap();
        let expected = fixture.build_info.as_ref().unwrap().as_object().unwrap();
        let actual: serde_json::Map<_, _> = value
            .as_object()
            .unwrap()
            .iter()
            .filter(|(key, _)| {
                [
                    "fileNames",
                    "root",
                    "resolvedRoot",
                    "referencedMap",
                    "fileIdsList",
                    "packageJsons",
                    "missingPackageJsons",
                ]
                .contains(&key.as_str())
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        assert_eq!(&actual, expected, "build-info ordering {workers:?}");
    }
}
