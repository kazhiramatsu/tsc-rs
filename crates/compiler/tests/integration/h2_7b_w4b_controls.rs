//! h2-7b-w4 lane B controls — the frozen closure rows of lane W4-B's classes
//! (filled by lane W4-B; registered at E1 so the lane's allowed set names a
//! concrete file). Every closed row replays through the production entry and
//! compares the full observation (reported and emit diagnostic tuples, the emit
//! result, every write's callback and materialized bytes).

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::MessageChain;
use tsc_emitter::{EmitArtifactKind, EmitWriteMetadata};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::{PreparedProgram, ProgramLoadLimits};

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-7b-qualification.v1.json";

/// B1 — upstream `hasDynamicName` excludes literal-like and signed numeric
/// computed names at both declaration-subtree gates.
const LITERAL_COMPUTED_NAME_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/declarationEmitComputedPropertyName1.ts#default",
    "typescript-6.0.3/compiler/mappedTypeGenericIndexedAccess.ts#default",
    "typescript-6.0.3/conformance/classes/propertyMemberDeclarations/strictPropertyInitialization.ts#default",
];

/// B2 — nested callback fields are parser-produced JSDoc parameter tags, and
/// both the syntactic visitor and semantic optional-union override consume
/// their common property-tag fields.
const NESTED_CALLBACK_FIELD_CASES: &[&str] =
    &["typescript-6.0.3/conformance/jsdoc/callbackTagNestedParameter.ts#default"];

/// B3 — declaration `ensureModifiers` retains modifier tokens but never the
/// decorators which share the parser's modifier-like array.
const DECLARATION_MODIFIER_CASES: &[&str] =
    &["typescript-6.0.3/conformance/types/tuple/readonlyArraysAndTuples2.ts#default"];

/// B4 — the row contains explicit/inferred, first/later, and already-wrapped
/// generic function type arguments; the declaration subtree must use the
/// typed TypeReference updater without changing parsed reprints.
const TYPED_TYPE_REFERENCE_CASES: &[&str] =
    &["typescript-6.0.3/compiler/declarationEmitFirstTypeArgumentGenericFunctionType.ts#default"];

/// B5 — a nonempty expando property table still produces a namespace when all
/// of its properties are filtered from the module body.
const EMPTY_EXPANDO_NAMESPACE_CASES: &[&str] =
    &["typescript-6.0.3/compiler/declarationEmitLateBoundAssignments2.ts#default"];

/// B6 — AMD dependencies and generated require calls resolve the original
/// declaration to the imported source's `moduleName` before extension
/// rewriting, without disturbing independent named AMD dependencies.
const EXTERNAL_MODULE_NAME_CASES: &[&str] =
    &["typescript-6.0.3/compiler/declarationEmitAmdModuleNameDirective.ts#default"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn frozen_case(case_id: &str) -> Value {
    let artifact: Value = serde_json::from_slice(
        &std::fs::read(workspace_root().join(QUALIFICATION_RELATIVE_PATH))
            .expect("the frozen H2.7b qualification artifact"),
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

fn prepared_band_row(case: &Value) -> PreparedProgram {
    assert_ne!(
        case["execution_route"], "project-mount",
        "W4-B controls contain no project rows"
    );
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
fn literal_computed_names_bypass_late_bound_removal_and_cleanup() {
    assert_every_row(LITERAL_COMPUTED_NAME_CASES);
}

#[test]
fn nested_callback_parameter_tags_keep_fields_and_semantic_optional_types() {
    assert_every_row(NESTED_CALLBACK_FIELD_CASES);
}

#[test]
fn unchanged_declaration_modifiers_exclude_decorators() {
    assert_every_row(DECLARATION_MODIFIER_CASES);
}

#[test]
fn changed_type_references_apply_typed_factory_parenthesization() {
    assert_every_row(TYPED_TYPE_REFERENCE_CASES);
}

#[test]
fn filtered_expando_properties_still_produce_an_empty_namespace() {
    assert_every_row(EMPTY_EXPANDO_NAMESPACE_CASES);
}

#[test]
fn imported_source_module_names_feed_amd_dependencies_and_require_calls() {
    assert_every_row(EXTERNAL_MODULE_NAME_CASES);
}
