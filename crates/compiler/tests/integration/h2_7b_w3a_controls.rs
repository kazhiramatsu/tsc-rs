//! h2-7b-w3 lane A controls — the frozen closure rows of the checker NodeBuilder /
//! declaration-serialization classes (filled by lane W3-A; registered at E1).

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::MessageChain;
use tsc_emitter::{EmitArtifactKind, EmitWriteMetadata};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-7b-qualification.v1.json";

const INFERRED_PREDICATE_CASES: &[&str] =
    &["typescript-6.0.3/compiler/inferTypePredicates.ts#default"];

const REMAPPED_JS_FUNCTION_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/jsDeclarationsGlobalFileConstFunction.ts#default",
    "typescript-6.0.3/compiler/jsDeclarationsGlobalFileConstFunctionNamed.ts#default",
];

const OBJECT_LITERAL_CONTAINER_CASES: &[&str] =
    &["typescript-6.0.3/compiler/uniqueSymbolPropertyDeclarationEmit.ts#default"];

const DECLARATIONLESS_NAME_TYPE_CASES: &[&str] =
    &["typescript-6.0.3/compiler/declarationEmitMappedTypeTemplateTypeofSymbol.ts#default"];

const GENERIC_MAPPED_INDEX_CASES: &[&str] =
    &["typescript-6.0.3/compiler/indexSignatureAndMappedType.ts#default"];

const W4_A0_JSDOC_ALIAS_REUSE_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/declarationEmitCastReusesTypeNode4.ts#strictnullchecks%3Dfalse",
    "typescript-6.0.3/compiler/declarationEmitCastReusesTypeNode4.ts#strictnullchecks%3Dtrue",
    "typescript-6.0.3/compiler/reuseTypeAnnotationImportTypeInGlobalThisTypeArgument.ts#default",
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsImportAliasExposedWithinNamespace.ts#default",
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsImportAliasExposedWithinNamespaceCjs.ts#default",
];

const W4_A0_REUSED_TYPE_REFERENCE_CASES: &[&str] =
    &["typescript-6.0.3/compiler/declarationEmitFirstTypeArgumentGenericFunctionType.ts#default"];

const W4_A0_EXPANDO_SCOPE_CASES: &[&str] =
    &["typescript-6.0.3/compiler/declarationEmitDefaultExportWithStaticAssignment.ts#default"];

const W4_A0_IMPORTED_ALIAS_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/declarationEmitExportAssignedNamespaceNoTripleSlashTypesReference.ts#default",
];

const W4_A0_INVALID_PACKAGE_CASES: &[&str] =
    &["typescript-6.0.3/compiler/declarationEmitWithInvalidPackageJsonTypings.ts#default"];

const W4_A0_ACCESSIBLE_CONTAINER_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/es5ExportEqualsDts.ts#target%3Des2015",
    "typescript-6.0.3/compiler/es5ExportEqualsDts.ts#target%3Des5",
];

const W4_A0_SYNTHETIC_SCOPE_REGRESSION_CASES: &[&str] =
    &["typescript-6.0.3/compiler/declarationEmitGlobalThisPreserved.ts#default"];

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
        "{case_id}: writes"
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
                .expect("frozen callback byte count"),
            "{case_id}: callback byte count for {}",
            write.path().display()
        );
        assert_eq!(
            write.write_byte_order_mark(),
            expected["write_byte_order_mark"]
                .as_bool()
                .expect("frozen BOM flag"),
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
                .expect("frozen materialized byte count"),
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
        let actual_data_diagnostics = match write.metadata() {
            Some(EmitWriteMetadata::Text(metadata)) => actual_diagnostics(metadata.diagnostics()),
            _ => Vec::new(),
        };
        assert_eq!(
            actual_data_diagnostics,
            expected_diagnostics(&expected["data_diagnostics"]),
            "{case_id}: callback diagnostics"
        );
    }
}

#[test]
fn inferred_predicate_names_are_unescaped_at_construction() {
    for case_id in INFERRED_PREDICATE_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn js_const_function_expressions_serialize_through_their_public_symbol() {
    for case_id in REMAPPED_JS_FUNCTION_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn object_and_type_literal_members_use_their_owning_variable_container() {
    for case_id in OBJECT_LITERAL_CONTAINER_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn declarationless_late_bound_properties_use_name_type_spelling() {
    for case_id in DECLARATIONLESS_NAME_TYPE_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn generic_mapped_sources_relate_template_to_string_index_value() {
    for case_id in GENERIC_MAPPED_INDEX_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn w4_a0_jsdoc_alias_reuse_is_byte_exact() {
    for case_id in W4_A0_JSDOC_ALIAS_REUSE_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
#[ignore = "W4-A0 trace proved the remaining owner is emitter/declarations/subtree.rs (out of scope)"]
fn w4_a0_reused_type_references_apply_factory_parenthesization() {
    for case_id in W4_A0_REUSED_TYPE_REFERENCE_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn w4_a0_expando_replacements_and_scope_are_byte_exact() {
    for case_id in W4_A0_EXPANDO_SCOPE_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn w4_a0_imported_alias_display_arguments_are_byte_exact() {
    for case_id in W4_A0_IMPORTED_ALIAS_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn w4_a0_invalid_package_existing_import_is_byte_exact() {
    for case_id in W4_A0_INVALID_PACKAGE_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn w4_a0_directly_accessible_container_precedes_export_equals() {
    for case_id in W4_A0_ACCESSIBLE_CONTAINER_CASES {
        assert_frozen_observation(case_id);
    }
}

#[test]
fn w4_a0_expando_scope_does_not_escape_to_synthetic_signature_scopes() {
    for case_id in W4_A0_SYNTHETIC_SCOPE_REGRESSION_CASES {
        assert_frozen_observation(case_id);
    }
}
