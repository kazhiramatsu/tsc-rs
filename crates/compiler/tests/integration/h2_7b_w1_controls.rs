//! h2-7b-w1 controls — package `exports` blocks unsafe declaration specifiers.
//!
//! These are the 11 frozen closure-wave rows where the prepared program owns
//! a package manifest whose `exports` field prevents the declaration emitter
//! from naming an inferred type through a `/node_modules/` relative path.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
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

fn assert_frozen_observation(case_id: &str, expect_ts_2883: bool) {
    let case = frozen_case(case_id);
    assert_eq!(case["disposition"], "admitted-for-execution", "{case_id}");
    let prepared = prepared_band_row(&case);

    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: the production emit completes: {error}"));

    let expected_diagnostics = case["typescript_observation"]["reported_diagnostics"]
        .as_array()
        .expect("frozen reported diagnostics")
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["code"].as_u64().expect("diagnostic code") as u32,
                diagnostic["message"]
                    .as_str()
                    .expect("diagnostic message")
                    .to_owned(),
                diagnostic["file"].as_str().map(str::to_owned),
                diagnostic["start"].as_u64().map(|value| value as u32),
                diagnostic["length"].as_u64().map(|value| value as u32),
            )
        })
        .collect::<Vec<_>>();
    if expect_ts_2883 {
        assert!(
            expected_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.0 == 2883),
            "{case_id}: the frozen row carries TS2883"
        );
    }
    let actual_diagnostics = reported
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code(),
                diagnostic.message_text().to_owned(),
                diagnostic.file_name.clone(),
                diagnostic.start,
                diagnostic.length,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual_diagnostics, expected_diagnostics,
        "{case_id}: exact reported diagnostics"
    );
    let expected_emit_diagnostics = case["typescript_observation"]["emit_result"]["diagnostics"]
        .as_array()
        .expect("frozen emit diagnostics")
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["code"].as_u64().expect("diagnostic code") as u32,
                diagnostic["message"]
                    .as_str()
                    .expect("diagnostic message")
                    .to_owned(),
                diagnostic["file"].as_str().map(str::to_owned),
                diagnostic["start"].as_u64().map(|value| value as u32),
                diagnostic["length"].as_u64().map(|value| value as u32),
            )
        })
        .collect::<Vec<_>>();
    let actual_emit_diagnostics = outcome
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code(),
                diagnostic.message_text().to_owned(),
                diagnostic.file_name.clone(),
                diagnostic.start,
                diagnostic.length,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual_emit_diagnostics, expected_emit_diagnostics,
        "{case_id}: exact emit diagnostics"
    );
    let expected_emit_skipped = case["typescript_observation"]["emit_result"]["emit_skipped"]
        .as_bool()
        .expect("frozen emitSkipped");
    assert_eq!(
        outcome.emit_skipped(),
        expected_emit_skipped,
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
            write.callback_bytes(),
            decode(&expected["callback_utf8_base64"]),
            "{case_id}: exact callback bytes for {}",
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
        let mut materialized = Vec::new();
        if write.write_byte_order_mark() {
            materialized.extend_from_slice(&[0xef, 0xbb, 0xbf]);
        }
        materialized.extend_from_slice(write.callback_bytes());
        assert_eq!(
            materialized,
            decode(&expected["materialized_utf8_base64"]),
            "{case_id}: exact materialized bytes for {}",
            write.path().display()
        );
    }
}

macro_rules! frozen_exports_control {
    ($name:ident, $case_id:literal) => {
        #[test]
        fn $name() {
            assert_frozen_observation(concat!("typescript-6.0.3/", $case_id), true);
        }
    };
}

macro_rules! frozen_observation_control {
    ($name:ident, $case_id:literal) => {
        #[test]
        fn $name() {
            assert_frozen_observation(concat!("typescript-6.0.3/", $case_id), false);
        }
    };
}

frozen_exports_control!(
    node_modules_exports_blocks_specifier_resolution_node16,
    "conformance/node/nodeModulesExportsBlocksSpecifierResolution.ts#module%3Dnode16"
);
frozen_exports_control!(
    node_modules_exports_blocks_specifier_resolution_node18,
    "conformance/node/nodeModulesExportsBlocksSpecifierResolution.ts#module%3Dnode18"
);
frozen_exports_control!(
    node_modules_exports_blocks_specifier_resolution_node20,
    "conformance/node/nodeModulesExportsBlocksSpecifierResolution.ts#module%3Dnode20"
);
frozen_exports_control!(
    node_modules_exports_blocks_specifier_resolution_nodenext,
    "conformance/node/nodeModulesExportsBlocksSpecifierResolution.ts#module%3Dnodenext"
);
frozen_exports_control!(
    node_modules_exports_source_ts_node16,
    "conformance/node/nodeModulesExportsSourceTs.ts#module%3Dnode16"
);
frozen_exports_control!(
    node_modules_exports_source_ts_node18,
    "conformance/node/nodeModulesExportsSourceTs.ts#module%3Dnode18"
);
frozen_exports_control!(
    node_modules_exports_source_ts_node20,
    "conformance/node/nodeModulesExportsSourceTs.ts#module%3Dnode20"
);
frozen_exports_control!(
    node_modules_exports_source_ts_nodenext,
    "conformance/node/nodeModulesExportsSourceTs.ts#module%3Dnodenext"
);
frozen_exports_control!(
    legacy_node_modules_exports_specifier_generation_conditions,
    "conformance/node/legacyNodeModulesExportsSpecifierGenerationConditions.ts#default"
);
frozen_exports_control!(
    declaration_emit_object_assigned_default_export,
    "compiler/declarationEmitObjectAssignedDefaultExport.ts#default"
);
frozen_exports_control!(
    declaration_emit_using_type_alias_1,
    "compiler/declarationEmitUsingTypeAlias1.ts#default"
);
frozen_observation_control!(
    declaration_emit_using_alternative_containing_modules_1,
    "compiler/declarationEmitUsingAlternativeContainingModules1.ts#default"
);
frozen_observation_control!(
    declaration_emit_using_alternative_containing_modules_2,
    "compiler/declarationEmitUsingAlternativeContainingModules2.ts#default"
);
frozen_observation_control!(
    declaration_emit_using_type_alias_2,
    "compiler/declarationEmitUsingTypeAlias2.ts#default"
);
