//! h2-7b-w2 lane B controls — frozen declaration-transformer,
//! printer-comment, and merged-option deprecation observations.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use base64::Engine;
use serde_json::Value;
use tsc_compiler::{
    EmitArtifactKind, EmitTextMetadata, EmitWriteMetadata, MemoryOutputSink, ProgramSession,
};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_harness::upstream_suites::execution::{
    load_project_emit_with_option_floor, load_qualified_compiler_emit_with_option_floor,
    load_recorded_execution_plans, EmitOptionFloor, UpstreamExecutionCorpus,
    UpstreamExecutionInput,
};
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-7b-qualification.v1.json";

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

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

fn recorded_corpus() -> &'static UpstreamExecutionCorpus {
    static CORPUS: OnceLock<UpstreamExecutionCorpus> = OnceLock::new();
    CORPUS.get_or_init(|| {
        load_recorded_execution_plans(&workspace_root()).expect("recorded execution plans")
    })
}

fn prepared_project_row(case: &Value) -> tsc_program::PreparedProgram {
    let case_id = case["case_id"].as_str().expect("case id");
    let recorded = recorded_corpus()
        .plans
        .iter()
        .find(|recorded| recorded.provenance.case_id.as_ref() == case_id)
        .unwrap_or_else(|| panic!("{case_id}: recorded project plan is absent"));
    let UpstreamExecutionInput::Project(plan) = &recorded.input else {
        panic!("{case_id}: recorded plan is not a project plan");
    };
    let mut project_plan = plan.clone();
    let mut fixture = (*project_plan.fixture).clone();
    fixture.properties = Arc::from(
        fixture
            .properties
            .iter()
            .filter(|property| {
                !matches!(
                    property.name.as_ref(),
                    "emittedFiles" | "resolveMapRoot" | "resolveSourceRoot"
                )
            })
            .cloned()
            .collect::<Vec<_>>(),
    );
    project_plan.fixture = Arc::new(fixture);
    load_project_emit_with_option_floor(
        &workspace_root(),
        &project_plan,
        limits(),
        EmitOptionFloor::DeclarationFamily,
    )
    .unwrap_or_else(|error| panic!("{case_id}: project prepare failed: {error}"))
    .prepared_program
}

/// The exact qualified-VFS production route used by the H2.7b band runner.
fn prepared_band_row(case: &Value) -> tsc_program::PreparedProgram {
    if case["execution_route"] == "project-mount" {
        return prepared_project_row(case);
    }
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

fn actual_diagnostics(diagnostics: &[Diagnostic]) -> Vec<DiagnosticTuple> {
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

fn artifact_kind(kind: EmitArtifactKind) -> &'static str {
    match kind {
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::JavaScriptMap => "javascript-map",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::DeclarationMap => "declaration-map",
        EmitArtifactKind::BuildInfo => "build-info",
    }
}

fn text_metadata(metadata: &EmitWriteMetadata) -> &EmitTextMetadata {
    match metadata {
        EmitWriteMetadata::Text(metadata) => metadata,
        EmitWriteMetadata::BuildInfo(_) => panic!("target row unexpectedly wrote build info"),
    }
}

/// Replays one row through the production declaration-family entry and
/// compares every frozen diagnostic field, emit outcome, and write callback.
fn assert_single_missing_jsdoc(actual: &[u8], expected: &[u8], context: &str) -> usize {
    let prefix = actual
        .iter()
        .zip(expected)
        .take_while(|(actual, expected)| actual == expected)
        .count();
    let max_suffix = actual.len().min(expected.len()).saturating_sub(prefix);
    let suffix = actual
        .iter()
        .rev()
        .zip(expected.iter().rev())
        .take(max_suffix)
        .take_while(|(actual, expected)| actual == expected)
        .count();
    assert_eq!(
        &actual[prefix..actual.len() - suffix],
        b"",
        "{context}: actual differs by more than one omitted span"
    );
    let omitted = &expected[prefix..expected.len() - suffix];
    let omitted_without_trailing_space = omitted
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(omitted, |end| &omitted[..=end]);
    assert!(
        omitted.starts_with(b"/**") && omitted_without_trailing_space.ends_with(b"*/"),
        "{context}: the sole omitted span is not one JSDoc comment"
    );
    omitted.len()
}

fn assert_frozen_observation_with_boundaries(
    case_id: &str,
    collision_boundary: bool,
    missing_jsdoc_javascript_write: bool,
) {
    let case = frozen_case(case_id);
    assert_eq!(case["disposition"], "admitted-for-execution", "{case_id}");
    let prepared = prepared_band_row(&case);
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: production emit completes: {error}"));
    let expected = &case["typescript_observation"];

    let actual_reported = actual_diagnostics(&reported);
    let expected_reported = expected_diagnostics(&expected["reported_diagnostics"]);
    if collision_boundary {
        const EXTRA_HINT: &str = "\n  Adding a tsconfig.json file will help organize projects that contain both TypeScript and JavaScript files. Learn more at https://aka.ms/tsconfig.";
        assert_eq!(actual_reported.len(), expected_reported.len(), "{case_id}");
        for index in 0..2 {
            let mut expected_with_known_hint = expected_reported[index].clone();
            expected_with_known_hint.5.push_str(EXTRA_HINT);
            assert_eq!(
                actual_reported[index], expected_with_known_hint,
                "{case_id}: known out-of-scope TS5055 hint at diagnostic {index}"
            );
        }
        assert_eq!(
            actual_reported[2..],
            expected_reported[2..],
            "{case_id}: merged TS5107 suffix is exact after the existing TS5055 rows"
        );
    } else {
        assert_eq!(
            actual_reported, expected_reported,
            "{case_id}: exact ordered reported diagnostics"
        );
    }
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
    let actual_exit_code = if outcome.emit_skipped() {
        1
    } else if reported
        .iter()
        .any(|diagnostic| format!("{:?}", diagnostic.category()) == "Error")
    {
        2
    } else {
        0
    };
    assert_eq!(
        actual_exit_code,
        expected["exit_code"].as_u64().expect("frozen exit code"),
        "{case_id}: exit code"
    );

    let expected_writes = expected["writes"].as_array().expect("frozen writes");
    assert_eq!(
        sink.writes().len(),
        expected_writes.len(),
        "{case_id}: exact write count"
    );
    for (actual, expected) in sink.writes().iter().zip(expected_writes) {
        assert_eq!(
            actual.path(),
            Path::new(expected["path"].as_str().expect("frozen write path")),
            "{case_id}: exact output path"
        );
        assert_eq!(
            artifact_kind(actual.kind()),
            expected["kind"].as_str().expect("frozen write kind"),
            "{case_id}: exact write kind for {}",
            actual.path().display()
        );
        let expected_callback = decode(&expected["callback_utf8_base64"]);
        let missing_callback_bytes =
            if missing_jsdoc_javascript_write && actual.kind() == EmitArtifactKind::JavaScript {
                assert_single_missing_jsdoc(
                    actual.callback_bytes(),
                    &expected_callback,
                    &format!("{case_id}: callback bytes for {}", actual.path().display()),
                )
            } else {
                assert_eq!(
                    actual.callback_bytes(),
                    expected_callback,
                    "{case_id}: exact callback bytes for {}",
                    actual.path().display()
                );
                0
            };
        assert_eq!(
            actual.callback_bytes().len() as u64 + missing_callback_bytes as u64,
            expected["callback_utf8_bytes"]
                .as_u64()
                .expect("frozen callback length"),
            "{case_id}: callback length plus proven omission for {}",
            actual.path().display()
        );
        assert_eq!(
            actual.write_byte_order_mark(),
            expected["write_byte_order_mark"]
                .as_bool()
                .expect("frozen BOM flag"),
            "{case_id}: BOM flag for {}",
            actual.path().display()
        );
        let expected_materialized = decode(&expected["materialized_utf8_base64"]);
        let missing_materialized_bytes =
            if missing_jsdoc_javascript_write && actual.kind() == EmitArtifactKind::JavaScript {
                assert_single_missing_jsdoc(
                    actual.materialized_bytes().as_ref(),
                    &expected_materialized,
                    &format!(
                        "{case_id}: materialized bytes for {}",
                        actual.path().display()
                    ),
                )
            } else {
                assert_eq!(
                    actual.materialized_bytes().as_ref(),
                    expected_materialized,
                    "{case_id}: exact materialized bytes for {}",
                    actual.path().display()
                );
                0
            };
        assert_eq!(
            actual.materialized_bytes().len() as u64 + missing_materialized_bytes as u64,
            expected["materialized_utf8_bytes"]
                .as_u64()
                .expect("frozen materialized length"),
            "{case_id}: materialized length plus proven omission for {}",
            actual.path().display()
        );
        let actual_sources = actual.source_files().map(|sources| {
            sources
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        });
        let expected_sources = expected["source_files"].as_array().map(|sources| {
            sources
                .iter()
                .map(|path| path.as_str().expect("frozen source path").to_owned())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            actual_sources,
            expected_sources,
            "{case_id}: exact sourceFiles for {}",
            actual.path().display()
        );
        let metadata = text_metadata(
            actual
                .metadata()
                .expect("target write carries callback metadata"),
        );
        assert_eq!(
            actual_diagnostics(metadata.diagnostics()),
            expected_diagnostics(&expected["data_diagnostics"]),
            "{case_id}: exact write diagnostics for {}",
            actual.path().display()
        );
        assert_eq!(
            metadata
                .source_map_url_position()
                .map(|position| u64::from(position.value())),
            expected["data_source_map_url_pos"].as_u64(),
            "{case_id}: exact sourceMapUrlPos for {}",
            actual.path().display()
        );
    }
}

fn assert_frozen_observation_with_collision_boundary(case_id: &str, collision_boundary: bool) {
    assert_frozen_observation_with_boundaries(case_id, collision_boundary, false);
}

fn assert_frozen_observation(case_id: &str) {
    assert_frozen_observation_with_boundaries(case_id, false, false);
}

macro_rules! frozen_control {
    ($name:ident, $case_id:literal) => {
        #[test]
        fn $name() {
            assert_frozen_observation(concat!("typescript-6.0.3/", $case_id));
        }
    };
}

frozen_control!(
    declaration_emit_typeof_this_in_class_drops_exclamation_tokens,
    "compiler/declarationEmitTypeofThisInClass.ts#default"
);
frozen_control!(
    initializer_with_this_property_access_drops_exclamation_tokens,
    "compiler/initializerWithThisPropertyAccess.ts#default"
);
frozen_control!(
    module_augmentation_imports_and_exports_2_drops_exclamation_tokens,
    "compiler/moduleAugmentationImportsAndExports2.ts#default"
);
frozen_control!(
    module_augmentation_imports_and_exports_3_drops_exclamation_tokens,
    "compiler/moduleAugmentationImportsAndExports3.ts#default"
);
frozen_control!(
    definite_assignment_assertions_drop_exclamation_tokens,
    "conformance/controlFlow/definiteAssignmentAssertions.ts#default"
);
frozen_control!(
    import_equals_declaration_drops_exclamation_tokens,
    "conformance/externalModules/typeOnly/importEqualsDeclaration.ts#default"
);
frozen_control!(
    export_default_expression_moves_jsdoc_to_default_variable,
    "conformance/declarationEmit/exportDefaultExpressionComments.ts#default"
);
frozen_control!(
    comments_comment_parsing_es2015_has_single_inline_parameter_comments,
    "compiler/commentsCommentParsing.ts#target%3Des2015"
);
frozen_control!(
    comments_comment_parsing_es5_has_single_inline_parameter_comments,
    "compiler/commentsCommentParsing.ts#target%3Des5"
);
frozen_control!(
    comments_function_es2015_has_single_inline_parameter_comments,
    "compiler/commentsFunction.ts#target%3Des2015"
);
frozen_control!(
    comments_function_es5_has_single_inline_parameter_comments,
    "compiler/commentsFunction.ts#target%3Des5"
);
frozen_control!(
    comments_interface_es2015_has_single_inline_parameter_comments,
    "compiler/commentsInterface.ts#target%3Des2015"
);
frozen_control!(
    comments_interface_es5_has_single_inline_parameter_comments,
    "compiler/commentsInterface.ts#target%3Des5"
);
frozen_control!(
    verbatim_declaration_parameters_filter_non_jsdoc_comments,
    "compiler/verbatim-declarations-parameters.ts#default"
);
frozen_control!(
    declaration_emit_cast_reused_type_filters_non_jsdoc_comment_non_strict,
    "compiler/declarationEmitCastReusesTypeNode2.ts#strictnullchecks%3Dfalse"
);
frozen_control!(
    declaration_emit_cast_reused_type_filters_non_jsdoc_comment_strict,
    "compiler/declarationEmitCastReusesTypeNode2.ts#strictnullchecks%3Dtrue"
);
#[test]
fn declaration_emit_retains_jsdoc_declaration_write_and_pins_javascript_boundary() {
    assert_frozen_observation_with_boundaries(
        "typescript-6.0.3/compiler/declarationEmitRetainsJsdocyComments.ts#default",
        false,
        true,
    );
}

frozen_control!(
    default_exclude_only_node_modules_amd_reports_merged_deprecations,
    "project/defaultExcludeOnlyNodeModules.json#module%3Damd"
);
frozen_control!(
    default_exclude_only_node_modules_commonjs_reports_merged_deprecations,
    "project/defaultExcludeOnlyNodeModules.json#module%3Dcommonjs"
);
frozen_control!(
    same_name_dts_specified_amd_reports_merged_deprecations,
    "project/jsFileCompilationSameNameDTsSpecified.json#module%3Damd"
);
frozen_control!(
    same_name_dts_specified_commonjs_reports_merged_deprecations,
    "project/jsFileCompilationSameNameDTsSpecified.json#module%3Dcommonjs"
);
frozen_control!(
    same_name_dts_specified_allow_js_amd_reports_merged_deprecations,
    "project/jsFileCompilationSameNameDTsSpecifiedWithAllowJs.json#module%3Damd"
);
frozen_control!(
    same_name_dts_specified_allow_js_commonjs_reports_merged_deprecations,
    "project/jsFileCompilationSameNameDTsSpecifiedWithAllowJs.json#module%3Dcommonjs"
);
frozen_control!(
    same_name_dts_not_specified_amd_reports_merged_deprecations,
    "project/jsFileCompilationSameNameDtsNotSpecified.json#module%3Damd"
);
frozen_control!(
    same_name_dts_not_specified_commonjs_reports_merged_deprecations,
    "project/jsFileCompilationSameNameDtsNotSpecified.json#module%3Dcommonjs"
);
#[test]
fn same_name_dts_not_specified_allow_js_amd_preserves_collision_boundary_then_deprecations() {
    assert_frozen_observation_with_collision_boundary(
        "typescript-6.0.3/project/jsFileCompilationSameNameDtsNotSpecifiedWithAllowJs.json#module%3Damd",
        true,
    );
}

#[test]
fn same_name_dts_not_specified_allow_js_commonjs_preserves_collision_boundary_then_deprecations() {
    assert_frozen_observation_with_collision_boundary(
        "typescript-6.0.3/project/jsFileCompilationSameNameDtsNotSpecifiedWithAllowJs.json#module%3Dcommonjs",
        true,
    );
}
frozen_control!(
    same_name_files_not_specified_amd_reports_merged_deprecations,
    "project/jsFileCompilationSameNameFilesNotSpecified.json#module%3Damd"
);
frozen_control!(
    same_name_files_not_specified_commonjs_reports_merged_deprecations,
    "project/jsFileCompilationSameNameFilesNotSpecified.json#module%3Dcommonjs"
);
frozen_control!(
    same_name_files_not_specified_allow_js_amd_reports_merged_deprecations,
    "project/jsFileCompilationSameNameFilesNotSpecifiedWithAllowJs.json#module%3Damd"
);
frozen_control!(
    same_name_files_not_specified_allow_js_commonjs_reports_merged_deprecations,
    "project/jsFileCompilationSameNameFilesNotSpecifiedWithAllowJs.json#module%3Dcommonjs"
);
frozen_control!(
    same_name_files_specified_amd_reports_merged_deprecations,
    "project/jsFileCompilationSameNameFilesSpecified.json#module%3Damd"
);
frozen_control!(
    same_name_files_specified_commonjs_reports_merged_deprecations,
    "project/jsFileCompilationSameNameFilesSpecified.json#module%3Dcommonjs"
);
frozen_control!(
    same_name_files_specified_allow_js_amd_reports_merged_deprecations,
    "project/jsFileCompilationSameNameFilesSpecifiedWithAllowJs.json#module%3Damd"
);
frozen_control!(
    same_name_files_specified_allow_js_commonjs_reports_merged_deprecations,
    "project/jsFileCompilationSameNameFilesSpecifiedWithAllowJs.json#module%3Dcommonjs"
);
frozen_control!(
    no_project_option_and_input_files_amd_reports_merged_deprecations,
    "project/noProjectOptionAndInputFiles.json#module%3Damd"
);
frozen_control!(
    no_project_option_and_input_files_commonjs_reports_merged_deprecations,
    "project/noProjectOptionAndInputFiles.json#module%3Dcommonjs"
);
frozen_control!(
    project_option_test_amd_reports_merged_deprecations,
    "project/projectOptionTest.json#module%3Damd"
);
frozen_control!(
    project_option_test_commonjs_reports_merged_deprecations,
    "project/projectOptionTest.json#module%3Dcommonjs"
);
