//! h2-7b-w4 lane A controls. Every target row is replayed through the
//! declaration-family production entry and compared with its complete frozen
//! TypeScript observation.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::MessageChain;
use tsc_emitter::{EmitArtifactKind, EmitWriteMetadata};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-7b-qualification.v1.json";

const OWN_EXPORT_TABLE_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/jsDeclarationEmitExportAssignedArray.ts#default",
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsImportNamespacedType.ts#default",
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsReferenceToClassInstanceCrossFile.ts#default",
    "typescript-6.0.3/conformance/jsdoc/moduleExportsElementAccessAssignment.ts#default",
    "typescript-6.0.3/conformance/salsa/expandoOnAlias.ts#default",
];

const IMPORT_EQUALS_EXPORT_CASES: &[&str] = &[
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsCommonjsRelativePath.ts#default",
];

const PROPERTY_ALIAS_LIVENESS_CASES: &[&str] =
    &["typescript-6.0.3/compiler/defaultDeclarationEmitShadowedNamedCorrectly.ts#default"];

const CORE_CONTEXT_AND_BINDING_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/contextuallyTypedSymbolNamedProperties.ts#default",
    "typescript-6.0.3/compiler/declarationEmitInvalidReference.ts#default",
];

const EXPANDO_SCOPE_CASES: &[&str] =
    &["typescript-6.0.3/compiler/expandoFunctionNestedAssigments.ts#default"];

const ERROR_BASE_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/declarationEmitExpressionInExtends4.ts#default",
    "typescript-6.0.3/compiler/declarationEmitExpressionInExtends7.ts#default",
];

const COMMONJS_SYMBOL_CASES: &[&str] =
    &["typescript-6.0.3/compiler/jsExportAssignmentNonMutableLocation.ts#default"];

const CUMULATIVE_OVERLOAD_CHAIN_CASES: &[&str] =
    &["typescript-6.0.3/compiler/bigintWithLib.ts#default"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn frozen_case(case_id: &str) -> Value {
    let artifact: Value = serde_json::from_slice(
        &std::fs::read(workspace_root().join(QUALIFICATION_RELATIVE_PATH))
            .expect("the frozen qualification artifact"),
    )
    .expect("valid qualification JSON");
    artifact["cases"]
        .as_array()
        .expect("qualification cases")
        .iter()
        .find(|case| case["case_id"] == case_id)
        .cloned()
        .unwrap_or_else(|| panic!("{case_id} is not a frozen band row"))
}

fn decode(value: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value.as_str().expect("base64 text"))
        .expect("valid base64")
}

fn prepared_band_row(case: &Value) -> tsc_program::PreparedProgram {
    let input = &case["input"];
    let mut files = input["files"]
        .as_array()
        .expect("case input files")
        .iter()
        .map(|file| {
            (
                PathBuf::from(file["path"].as_str().expect("file path")),
                decode(&file["utf8_base64"]),
            )
        })
        .collect::<Vec<_>>();
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        files.push((
            PathBuf::from(config["path"].as_str().expect("config path")),
            decode(&config["utf8_base64"]),
        ));
    }
    let roots = input["roots"]
        .as_array()
        .expect("case roots")
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root path")))
        .collect::<Vec<_>>();
    let settings = input["settings"]
        .as_array()
        .expect("case settings")
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().expect("setting name").to_owned(),
                setting["value"].as_str().expect("setting value").to_owned(),
            )
        })
        .collect::<Vec<_>>();
    load_qualified_compiler_emit_with_option_floor(
        &workspace_root(),
        input["current_directory"]
            .as_str()
            .expect("case current directory"),
        &files,
        &roots,
        &settings,
        ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024),
        EmitOptionFloor::DeclarationFamily,
    )
    .expect("the frozen row loads through the declaration-family floor")
}

type DiagnosticTuple = (
    u32,
    String,
    Option<String>,
    Option<u32>,
    Option<u32>,
    String,
);

fn expected_diagnostics(value: &Value) -> Vec<DiagnosticTuple> {
    value
        .as_array()
        .expect("frozen diagnostics")
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["code"].as_u64().expect("diagnostic code") as u32,
                diagnostic["category"]
                    .as_str()
                    .expect("diagnostic category")
                    .to_owned(),
                diagnostic["file"].as_str().map(str::to_owned),
                diagnostic["start"].as_u64().map(|value| value as u32),
                diagnostic["length"].as_u64().map(|value| value as u32),
                diagnostic["message"]
                    .as_str()
                    .expect("diagnostic message")
                    .to_owned(),
            )
        })
        .collect()
}

fn actual_diagnostics(diagnostics: &[tsc_diagnostics::Diagnostic]) -> Vec<DiagnosticTuple> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let mut message = String::new();
            flatten_message_chain(&diagnostic.message, 0, &mut message);
            (
                diagnostic.code(),
                format!("{:?}", diagnostic.category()),
                diagnostic.file_name.clone(),
                diagnostic.start,
                diagnostic.length,
                message,
            )
        })
        .collect()
}

fn flatten_message_chain(chain: &MessageChain, indent: usize, output: &mut String) {
    if indent != 0 {
        output.push('\n');
        for _ in 0..indent {
            output.push_str("  ");
        }
    }
    output.push_str(&chain.text);
    for child in &chain.next {
        flatten_message_chain(child, indent + 1, output);
    }
}

fn artifact_kind(kind: EmitArtifactKind, path: &Path) -> &'static str {
    match kind {
        EmitArtifactKind::JavaScript
            if path.extension().and_then(|value| value.to_str()) == Some("mjs") =>
        {
            "mjs"
        }
        EmitArtifactKind::JavaScript
            if path.extension().and_then(|value| value.to_str()) == Some("cjs") =>
        {
            "cjs"
        }
        EmitArtifactKind::JavaScript
            if path.extension().and_then(|value| value.to_str()) == Some("jsx") =>
        {
            "jsx"
        }
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::JavaScriptMap => "javascript-map",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::DeclarationMap => "declaration-map",
        EmitArtifactKind::BuildInfo => "build-info",
    }
}

fn emitted_files_value(actual: Option<&[PathBuf]>) -> Value {
    actual.map_or(Value::Null, |paths| {
        Value::Array(
            paths
                .iter()
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .collect(),
        )
    })
}

fn expected_emitted_files_from_writes(writes: &[Value]) -> Value {
    let mut paths = Vec::new();
    let mut pending_map: Option<String> = None;
    for write in writes {
        let path = write["path"].as_str().expect("frozen write path");
        match write["kind"].as_str().expect("frozen write kind") {
            "source-map" | "javascript-map" => pending_map = Some(path.to_owned()),
            "javascript" | "mjs" | "cjs" | "jsx" => {
                paths.push(Value::String(path.to_owned()));
                if let Some(map) = pending_map.take() {
                    paths.push(Value::String(map));
                }
            }
            "declaration" => paths.push(Value::String(path.to_owned())),
            kind => panic!("unexpected frozen write kind {kind}"),
        }
    }
    assert!(pending_map.is_none(), "source map has no JavaScript member");
    Value::Array(paths)
}

fn source_maps_value(actual: Option<&[tsc_compiler::SourceMapObservation]>) -> Value {
    actual.map_or(Value::Null, |maps| {
        Value::Array(
            maps.iter()
                .map(|map| {
                    json!({
                        "input_source_file_names": map.input_source_files().iter()
                            .map(|path| path.to_string_lossy().into_owned())
                            .collect::<Vec<_>>(),
                        "source_map_json": map.canonical_json(),
                    })
                })
                .collect(),
        )
    })
}

fn assert_frozen_observation(case_id: &str) {
    let case = frozen_case(case_id);
    assert_eq!(case["disposition"], "admitted-for-execution", "{case_id}");
    let prepared = prepared_band_row(&case);
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: production emit completes: {error}"));
    let expected = &case["typescript_observation"];

    assert_eq!(
        actual_diagnostics(&reported),
        expected_diagnostics(&expected["reported_diagnostics"]),
        "{case_id}: exact ordered reported diagnostics"
    );
    assert_eq!(
        actual_diagnostics(outcome.diagnostics()),
        expected_diagnostics(&expected["emit_result"]["diagnostics"]),
        "{case_id}: exact ordered emit diagnostics"
    );
    assert_eq!(
        outcome.emit_skipped(),
        expected["emit_result"]["emit_skipped"]
            .as_bool()
            .expect("frozen emitSkipped"),
        "{case_id}: emitSkipped"
    );
    assert_eq!(
        outcome.emit_skipped(),
        expected["emit_refused"]
            .as_bool()
            .expect("frozen emitRefused"),
        "{case_id}: emitRefused alias"
    );
    let expected_writes = expected["writes"].as_array().expect("frozen writes");
    assert_eq!(
        emitted_files_value(outcome.emitted_files()),
        expected_emitted_files_from_writes(expected_writes),
        "{case_id}: exact emitted-file listing"
    );
    assert_eq!(
        source_maps_value(outcome.source_maps()),
        expected["emit_result"]["source_maps"],
        "{case_id}: exact source-map result"
    );
    assert_eq!(
        expected["status_writes"],
        json!([]),
        "{case_id}: no status writes"
    );
    let actual_exit_code = if outcome.emit_skipped() && !reported.is_empty() {
        1
    } else if !reported.is_empty() {
        2
    } else {
        0
    };
    assert_eq!(
        actual_exit_code,
        expected["exit_code"].as_u64().expect("frozen exit code"),
        "{case_id}: exact exit code"
    );

    assert_eq!(
        sink.writes().len(),
        expected_writes.len(),
        "{case_id}: write count"
    );
    for (write, expected) in sink.writes().iter().zip(expected_writes) {
        assert_eq!(
            write.path(),
            Path::new(expected["path"].as_str().expect("frozen write path")),
            "{case_id}: output path"
        );
        assert_eq!(
            artifact_kind(write.kind(), write.path()),
            expected["kind"].as_str().expect("frozen write kind"),
            "{case_id}: write kind for {}",
            write.path().display()
        );
        assert_eq!(
            write.callback_bytes(),
            decode(&expected["callback_utf8_base64"]),
            "{case_id}: callback bytes for {}",
            write.path().display()
        );
        assert_eq!(
            write.callback_bytes().len() as u64,
            expected["callback_utf8_bytes"]
                .as_u64()
                .expect("callback byte count"),
            "{case_id}: callback byte count for {}",
            write.path().display()
        );
        assert_eq!(
            write.write_byte_order_mark(),
            expected["write_byte_order_mark"]
                .as_bool()
                .expect("BOM flag"),
            "{case_id}: BOM flag for {}",
            write.path().display()
        );
        assert_eq!(
            write.materialized_bytes().as_ref(),
            decode(&expected["materialized_utf8_base64"]),
            "{case_id}: materialized bytes for {}",
            write.path().display()
        );
        assert_eq!(
            write.materialized_bytes().len() as u64,
            expected["materialized_utf8_bytes"]
                .as_u64()
                .expect("materialized byte count"),
            "{case_id}: materialized byte count for {}",
            write.path().display()
        );
        let actual_sources = write.source_files().map(|sources| {
            sources
                .iter()
                .map(|source| source.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        });
        let expected_sources = expected["source_files"].as_array().map(|sources| {
            sources
                .iter()
                .map(|source| source.as_str().expect("source file path").to_owned())
                .collect::<Vec<_>>()
        });
        assert_eq!(actual_sources, expected_sources, "{case_id}: source files");
        assert_eq!(
            write.metadata().is_some(),
            expected["data_present"]
                .as_bool()
                .expect("data-present flag"),
            "{case_id}: callback data presence"
        );
        let (actual_data_diagnostics, actual_source_map_url_pos) = match write.metadata() {
            Some(EmitWriteMetadata::Text(metadata)) => (
                actual_diagnostics(metadata.diagnostics()),
                metadata
                    .source_map_url_position()
                    .map(|position| u64::from(position.value())),
            ),
            _ => (Vec::new(), None),
        };
        assert_eq!(
            actual_data_diagnostics,
            expected_diagnostics(&expected["data_diagnostics"]),
            "{case_id}: callback diagnostics"
        );
        assert_eq!(
            actual_source_map_url_pos,
            expected["data_source_map_url_pos"].as_u64(),
            "{case_id}: callback source-map URL position"
        );
    }
}

fn assert_every_row(case_ids: &[&str]) {
    let mut failures = Vec::new();
    for case_id in case_ids {
        let result = std::panic::catch_unwind(|| assert_frozen_observation(case_id));
        if let Err(payload) = result {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_owned())
                })
                .unwrap_or_else(|| "non-string panic".to_owned());
            failures.push(format!("{case_id}: {message}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} rows diverge:\n{}",
        failures.len(),
        case_ids.len(),
        failures.join("\n---\n")
    );
}

#[test]
#[ignore = "S1 requires the sibling-owned syntactic type builder for the export-assigned array"]
fn source_module_serializes_its_own_export_table() {
    assert_every_row(OWN_EXPORT_TABLE_CASES);
}

#[test]
fn import_equals_keeps_a_separate_export_declaration() {
    assert_every_row(IMPORT_EQUALS_EXPORT_CASES);
}

#[test]
#[ignore = "J10 introduces 21 TS2304 false positives in the required 2xxx guard"]
fn property_alias_liveness_resolves_a_cold_identifier_cache() {
    assert_every_row(PROPERTY_ALIAS_LIVENESS_CASES);
}

#[test]
fn core_context_and_reference_match_upstream() {
    assert_every_row(CORE_CONTEXT_AND_BINDING_CASES);
}

#[test]
fn expando_scope_matches_upstream() {
    assert_every_row(EXPANDO_SCOPE_CASES);
}

#[test]
fn error_bases_use_any_singleton_identity() {
    assert_every_row(ERROR_BASE_CASES);
}

#[test]
fn commonjs_symbol_positions_use_the_assignment_left_hand_side() {
    assert_every_row(COMMONJS_SYMBOL_CASES);
}

#[test]
fn overload_diagnostics_share_the_completed_cumulative_prefix() {
    assert_every_row(CUMULATIVE_OVERLOAD_CHAIN_CASES);
}
