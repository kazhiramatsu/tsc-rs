
use super::*;
use crate::upstream_suites::execution::{load_recorded_execution_plans, UpstreamExecutionInput};

#[test]
fn absent_descriptor_roots_override_config_roots_like_existing_options() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let corpus = load_recorded_execution_plans(&workspace).unwrap();
    let plan = corpus
        .plans
        .iter()
        .find_map(|record| {
            if record.provenance.case_id.as_ref()
                != "typescript-6.0.3/project/baseline.json#module%3Damd"
            {
                return None;
            }
            match &record.input {
                UpstreamExecutionInput::Project(plan) => Some(plan),
                _ => None,
            }
        })
        .unwrap();
    for no_emit in [false, true] {
        let mut options = CompilerOptions {
            map_root: Some("config/maps".into()),
            source_root: Some("config/src".into()),
            ..CompilerOptions::default()
        };
        if no_emit {
            apply_project_runner_existing_options(plan, &mut options).unwrap();
        } else {
            apply_project_emit_options(plan, &mut options).unwrap();
        }
        assert_eq!(options.map_root, None);
        assert_eq!(options.source_root, None);
    }
}
