//! h2-7b-w2 lane A controls — frozen NodeBuilder and declaration-serialization
//! closure rows replayed through the production declaration-family entry.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_emitter::{EmitArtifactKind, EmitWriteMetadata};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-7b-qualification.v1.json";

const JSDOC_IMPORT_ALIAS_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/jsDeclarationEmitDoesNotRenameImport.ts#default",
    "typescript-6.0.3/conformance/jsdoc/importTag5.ts#default",
    "typescript-6.0.3/conformance/jsdoc/importTag15.ts#module%3Des2015",
    "typescript-6.0.3/conformance/jsdoc/importTag15.ts#module%3Desnext",
    "typescript-6.0.3/conformance/jsdoc/importTag16.ts#default",
    "typescript-6.0.3/conformance/jsdoc/importTag18.ts#default",
    "typescript-6.0.3/conformance/jsdoc/importTag19.ts#default",
    "typescript-6.0.3/conformance/jsdoc/importTag20.ts#default",
];

const REUSED_NODE_ARRAY_CASES: &[&str] =
    &["typescript-6.0.3/compiler/declarationEmitBindingPatternWithReservedWord.ts#default"];

const SETTER_BINDING_NAME_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/declarationEmitClassSetAccessorParamNameInJs2.ts#default",
    "typescript-6.0.3/compiler/declarationEmitClassSetAccessorParamNameInJs3.ts#default",
];

const SYNTHETIC_SIGNATURE_SCOPE_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/declarationEmitGlobalThisPreserved.ts#default",
    "typescript-6.0.3/compiler/declarationEmitTypeParameterNameInOuterScope.ts#default",
    "typescript-6.0.3/compiler/renamingDestructuredPropertyInFunctionType.ts#default",
];

const IMPORT_TYPE_SPECIFIER_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/declarationEmitDoesNotUseReexportedNamespaceAsLocal.ts#default",
    "typescript-6.0.3/conformance/importAssertion/importAssertion1.ts#module%3Desnext",
    "typescript-6.0.3/conformance/importAttributes/importAttributes1.ts#module%3Desnext",
];

const PACKAGE_EXPORTS_SPECIFIER_CASES: &[&str] = &[
    "typescript-6.0.3/conformance/node/nodeModulesDeclarationEmitDynamicImportWithPackageExports.ts#module%3Dnode18",
    "typescript-6.0.3/conformance/node/nodeModulesDeclarationEmitDynamicImportWithPackageExports.ts#module%3Dnode20",
    "typescript-6.0.3/conformance/node/nodeModulesDeclarationEmitDynamicImportWithPackageExports.ts#module%3Dnodenext",
];

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

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
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
        limits(),
        EmitOptionFloor::DeclarationFamily,
    )
    .expect("the frozen band row loads through the declaration-family floor")
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
            (
                diagnostic.code(),
                format!("{:?}", diagnostic.category()),
                diagnostic.file_name.clone(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text().to_owned(),
            )
        })
        .collect()
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

fn assert_frozen_observation(case_id: &str) {
    let case = frozen_case(case_id);
    assert_eq!(case["disposition"], "admitted-for-execution", "{case_id}");
    let prepared = prepared_band_row(&case);
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: the production emit completes: {error}"));

    assert_eq!(
        actual_diagnostics(&reported),
        expected_diagnostics(&case["typescript_observation"]["reported_diagnostics"]),
        "{case_id}: exact reported diagnostic tuples"
    );
    assert_eq!(
        actual_diagnostics(outcome.diagnostics()),
        expected_diagnostics(&case["typescript_observation"]["emit_result"]["diagnostics"]),
        "{case_id}: exact emit diagnostic tuples"
    );
    assert_eq!(
        outcome.emit_skipped(),
        case["typescript_observation"]["emit_result"]["emit_skipped"]
            .as_bool()
            .expect("frozen emitSkipped"),
        "{case_id}: emitSkipped"
    );

    let expected_writes = case["typescript_observation"]["writes"]
        .as_array()
        .expect("frozen writes");
    assert_eq!(
        sink.writes().len(),
        expected_writes.len(),
        "{case_id}: exact write count"
    );
    for (write, expected) in sink.writes().iter().zip(expected_writes) {
        assert_eq!(
            write.path(),
            Path::new(expected["path"].as_str().expect("frozen write path")),
            "{case_id}: exact output path"
        );
        assert_eq!(
            artifact_kind(write.kind(), write.path()),
            expected["kind"].as_str().expect("frozen write kind"),
            "{case_id}: exact write kind for {}",
            write.path().display()
        );
        assert_eq!(
            write.callback_bytes(),
            decode(&expected["callback_utf8_base64"]),
            "{case_id}: exact callback bytes for {}",
            write.path().display()
        );
        assert_eq!(
            write.callback_bytes().len() as u64,
            expected["callback_utf8_bytes"]
                .as_u64()
                .expect("frozen callback byte count"),
            "{case_id}: exact callback byte count for {}",
            write.path().display()
        );
        assert_eq!(
            write.write_byte_order_mark(),
            expected["write_byte_order_mark"]
                .as_bool()
                .expect("frozen BOM flag"),
            "{case_id}: exact BOM flag for {}",
            write.path().display()
        );
        assert_eq!(
            write.materialized_bytes().as_ref(),
            decode(&expected["materialized_utf8_base64"]),
            "{case_id}: exact materialized bytes for {}",
            write.path().display()
        );
        assert_eq!(
            write.materialized_bytes().len() as u64,
            expected["materialized_utf8_bytes"]
                .as_u64()
                .expect("frozen materialized byte count"),
            "{case_id}: exact materialized byte count for {}",
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
        assert_eq!(
            actual_sources,
            expected_sources,
            "{case_id}: exact source files for {}",
            write.path().display()
        );
        assert_eq!(
            write.metadata().is_some(),
            expected["data_present"]
                .as_bool()
                .expect("data-present flag"),
            "{case_id}: exact callback data presence for {}",
            write.path().display()
        );
        let actual_data_diagnostics = match write.metadata() {
            Some(EmitWriteMetadata::Text(metadata)) => actual_diagnostics(metadata.diagnostics()),
            _ => Vec::new(),
        };
        assert_eq!(
            actual_data_diagnostics,
            expected_diagnostics(&expected["data_diagnostics"]),
            "{case_id}: exact callback diagnostic tuples for {}",
            write.path().display()
        );
    }
}

#[test]
fn jsdoc_import_aliases_reuse_type_only_source_syntax() {
    for case_id in JSDOC_IMPORT_ALIAS_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn reused_node_arrays_preserve_trailing_comma_and_ranges() {
    for case_id in REUSED_NODE_ARRAY_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn js_setter_parameter_names_preserve_binding_patterns() {
    for case_id in SETTER_BINDING_NAME_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn synthetic_signature_scopes_qualify_shadowed_globals() {
    for case_id in SYNTHETIC_SIGNATURE_SCOPE_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn import_type_specifiers_follow_upstream_mode_and_container_selection() {
    for case_id in IMPORT_TYPE_SPECIFIER_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn import_type_specifiers_reuse_resolved_package_exports_names() {
    for case_id in PACKAGE_EXPORTS_SPECIFIER_CASES {
        assert_frozen_observation(case_id);
    }
}
