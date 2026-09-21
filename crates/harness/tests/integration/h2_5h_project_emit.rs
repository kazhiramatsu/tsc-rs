//! H2.5h / CA-4 focused loader-shape checks for `load_project_emit`: the
//! CA-3 §5.3a option floor over the shared project mount, one explicit-root
//! descriptor and one config-arm descriptor. The band-wide byte gate is
//! `cargo xtask h2-5h-acceptance` (`run_h2_5h`); these tests pin the loader
//! contract the packet's §5.1 specifies.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsc_harness::upstream_suites::execution::{
    load_project_emit, load_project_no_emit, load_recorded_execution_plans, OrderedJsonProperty,
    ProjectExecutionPlan, UpstreamExecutionInput,
};
use tsc_program::ProgramLoadLimits;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

fn with_properties(
    plan: &ProjectExecutionPlan,
    values: &[(&str, serde_json::Value)],
) -> ProjectExecutionPlan {
    let mut plan = plan.clone();
    let mut fixture = (*plan.fixture).clone();
    let mut properties = fixture.properties.to_vec();
    properties.extend(values.iter().map(|(name, value)| OrderedJsonProperty {
        name: Arc::from(*name),
        value: value.clone(),
    }));
    fixture.properties = Arc::from(properties);
    plan.fixture = Arc::new(fixture);
    plan
}

#[test]
fn descriptor_resolve_flags_use_the_source_mount_and_preserve_fallbacks() {
    use serde_json::json;
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace).expect("recorded plans");
    let original = project_plan(
        &corpus,
        "typescript-6.0.3/project/baseline.json#module%3Damd",
    );
    for (root, flag) in [
        ("mapRoot", "resolveMapRoot"),
        ("sourceRoot", "resolveSourceRoot"),
    ] {
        let cases = [
            (vec![(root, json!("tests/maps"))], Some("tests/maps")),
            (
                vec![(root, json!("tests/maps")), (flag, json!(false))],
                Some("tests/maps"),
            ),
            (
                vec![(root, json!("tests/maps")), (flag, json!(true))],
                Some("/.src/tests/maps"),
            ),
            (
                vec![(flag, json!(true)), (root, json!("tests/maps"))],
                Some("/.src/tests/maps"),
            ),
            (vec![(flag, json!(true))], None),
            (vec![(flag, json!(true)), (root, json!(""))], Some("")),
        ];
        for (properties, expected) in cases {
            let plan = with_properties(&original, &properties);
            let loaded =
                load_project_emit(&workspace, &plan, limits()).expect("bounded project options");
            let options = &loaded.effective_compiler_options;
            let actual = if root == "mapRoot" {
                &options.map_root
            } else {
                &options.source_root
            };
            assert_eq!(
                actual.as_ref().map(|value| value.as_str().unwrap()),
                expected,
                "{properties:?}"
            );
        }
        for unsupported in [
            "../maps",
            "./maps",
            "a//b",
            "/maps",
            "C:/maps",
            "https://example.test/maps",
            "maps/",
            "maps\\nested",
        ] {
            let plan = with_properties(
                &original,
                &[(root, json!(unsupported)), (flag, json!(true))],
            );
            let error = load_project_emit(&workspace, &plan, limits())
                .unwrap_err()
                .to_string();
            assert!(error.contains("requires a plain relative path"), "{error}");
        }
        for properties in [
            vec![(root, json!(1)), (flag, json!(true))],
            vec![(root, json!("tests/maps")), (flag, json!("true"))],
        ] {
            assert!(load_project_emit(
                &workspace,
                &with_properties(&original, &properties),
                limits()
            )
            .is_err());
        }
    }
}

#[test]
fn declaration_directory_and_output_metadata_keep_their_distinct_meanings() {
    use serde_json::json;
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace).expect("recorded plans");
    let original = project_plan(
        &corpus,
        "typescript-6.0.3/project/baseline.json#module%3Damd",
    );
    let plan = with_properties(
        &original,
        &[
            ("declaration", json!(true)),
            ("declarationDir", json!("declarations")),
            ("emittedFiles", json!(["metadata.js"])),
        ],
    );
    let loaded =
        load_project_emit(&workspace, &plan, limits()).expect("declaration directory projection");
    assert_eq!(
        loaded.effective_compiler_options.declaration_dir,
        Some("declarations".into())
    );
    assert_eq!(loaded.effective_compiler_options.list_emitted_files, None);
    assert!(
        load_project_no_emit(&workspace, &plan, limits()).is_err(),
        "no-emit route keeps its emit-option refusal"
    );
    assert!(load_project_emit(
        &workspace,
        &with_properties(&original, &[("declarationDir", json!(false))]),
        limits()
    )
    .is_err());
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024)
}

fn project_plan(
    corpus: &tsc_harness::upstream_suites::execution::UpstreamExecutionCorpus,
    case_id: &str,
) -> tsc_harness::upstream_suites::execution::ProjectExecutionPlan {
    corpus
        .plans
        .iter()
        .find_map(|recorded| match &recorded.input {
            UpstreamExecutionInput::Project(plan)
                if recorded.provenance.case_id.as_ref() == case_id =>
            {
                Some(plan.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing project plan {case_id}"))
}

#[test]
fn explicit_root_descriptor_loads_with_the_observation_floor() {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace).expect("recorded plans");
    let plan = project_plan(
        &corpus,
        "typescript-6.0.3/project/baseline.json#module%3Damd",
    );
    let loaded = load_project_emit(&workspace, &plan, limits()).expect("emit load");
    let options = &loaded.effective_compiler_options;
    assert_eq!(options.module, Some(2), "amd variant");
    assert_eq!(options.module_resolution, Some(1), "Classic floor");
    assert_eq!(options.new_line, Some(0), "CRLF pin");
    assert_eq!(options.no_emit, None, "no forced noEmit");
    assert_eq!(options.no_error_truncation, Some(false));
    assert_eq!(options.skip_default_lib_check, Some(false));
    assert_eq!(
        loaded.root_names.as_ref(),
        [tsc_diagnostics::JsString::from(
            "/.src/tests/cases/projects/baseline/emit.ts"
        )],
        "every requested root, normalized against the project cwd"
    );
}

#[test]
fn map_root_descriptor_applies_the_emit_options_instead_of_rejecting() {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace).expect("recorded plans");
    let plan = project_plan(
        &corpus,
        "typescript-6.0.3/project/mapRootWithNoSourceMapOption.json#module%3Dcommonjs",
    );
    let loaded = load_project_emit(&workspace, &plan, limits())
        .expect("the emit lane applies mapRoot as an ordinary option");
    assert_eq!(
        loaded
            .effective_compiler_options
            .map_root
            .as_ref()
            .map(|value| value.as_str().expect("scalar legacy option observation")),
        Some("../mapFiles"),
        "the H0 adapter's rejection is not the emit-lane record"
    );
    assert_eq!(loaded.effective_compiler_options.module, Some(1));
}
