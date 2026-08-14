use super::*;

#[test]
fn owner_options_maps_the_json_emit_surface() {
    let options = owner_options(&json!({
        "emitBOM": true,
        "ignoreDeprecations": "6.0",
        "module": 3,
        "moduleResolution": 2,
        "newLine": 0,
        "outDir": "/project/dist",
        "resolveJsonModule": true,
        "target": 99
    }))
    .unwrap();

    assert_eq!(options.emit_bom, Some(true));
    assert_eq!(options.ignore_deprecations.as_deref(), Some("6.0"));
    assert_eq!(options.module, Some(3));
    assert_eq!(options.module_resolution, Some(2));
    assert_eq!(options.new_line, Some(0));
    assert_eq!(options.out_dir.as_deref(), Some("/project/dist"));
    assert_eq!(options.resolve_json_module, Some(true));
    assert_eq!(options.target, Some(99));
    assert_eq!(options.no_emit, Some(false));
}

#[test]
fn pinned_h2_3d_acceptance_is_exact() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    run(&workspace).unwrap();
    run_owner_controls(&workspace).unwrap();
}

#[test]
fn pinned_h2_4a_owner_controls_are_exact() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    run_h2_4a_owner_controls(&workspace).unwrap();
}

#[test]
fn pinned_h2_4b_owner_controls_are_exact() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    run_h2_4b_owner_controls(&workspace).unwrap();
}

fn h2_5g_owner_control_artifact(controls: usize) -> Value {
    json!({
        "schema": 1,
        "phase": "H2.5g-es2016-target-owner-controls",
        "status": "qualified",
        "controls": vec![Value::Null; controls],
        "summary": {
            "controls": 22,
            "exact_outputs": 21,
            "typescript_runs": 44,
            "reported_diagnostics": 2,
            "emit_diagnostics": 1,
            "no_emit_on_error_controls": 1,
            "es2015_controls": 21,
            "es2016_controls": 1,
            "exponentiation_controls": 22,
            "exponentiation_assignment_controls": 15,
            "property_assignment_controls": 6,
            "element_assignment_controls": 5,
            "parameter_controls": 1,
            "collision_controls": 1,
            "super_controls": 1,
            "precedence_controls": 1,
            "comment_controls": 1,
            "class_composition_controls": 5,
            "commonjs_controls": 1,
            "async_composition_controls": 2,
            "using_controls": 1,
            "h2_5a_active_controls": 21,
            "h2_5b_active_controls": 21,
            "h2_5c_active_controls": 21,
            "h2_5d_active_controls": 21,
            "h2_5e_active_controls": 21,
            "h2_5f_active_controls": 21,
            "h2_5g_active_controls": 20
        }
    })
}

#[test]
fn h2_5g_owner_control_header_and_denominator_are_validated_without_execution() {
    let valid = h2_5g_owner_control_artifact(22);
    assert_eq!(
        validate_h2_5g_owner_control_artifact(&valid).unwrap().len(),
        22
    );

    let mut open_header = valid.clone();
    open_header["summary"]["h2_5g_active_controls"] = json!(21);
    assert_eq!(
        validate_h2_5g_owner_control_artifact(&open_header)
            .unwrap_err()
            .to_string(),
        "H2.5g owner-control header is not closed"
    );

    assert_eq!(
        validate_h2_5g_owner_control_artifact(&h2_5g_owner_control_artifact(21))
            .unwrap_err()
            .to_string(),
        "H2.5g owner-control denominator changed"
    );
}

#[test]
fn h2_5g_owner_activity_projection_is_pure_and_slice_specific() {
    let control = json!({
        "control_id": "projection",
        "input": { "compiler_options": { "module": 1 } },
        "observation": { "writes": [{}] },
        "runtime_expectation": {
            "h2_5f_sources": 0,
            "h2_5g_sources": 1
        }
    });

    assert_eq!(
        expected_h2_5g_owner_activity_for_slice(&control, H2RuntimeSlice::H2_1a).unwrap(),
        1
    );
    assert_eq!(
        expected_h2_5g_owner_activity_for_slice(&control, H2RuntimeSlice::H2_1b).unwrap(),
        1
    );
    assert_eq!(
        expected_h2_5g_owner_activity_for_slice(&control, H2RuntimeSlice::H2_1d).unwrap(),
        0
    );
    assert_eq!(
        expected_h2_5g_owner_activity_for_slice(&control, H2RuntimeSlice::H2_3d).unwrap(),
        0
    );
    assert_eq!(
        expected_h2_5g_owner_activity_for_slice(&control, H2RuntimeSlice::H2_5f).unwrap(),
        0
    );
    assert_eq!(
        expected_h2_5g_owner_activity_for_slice(&control, H2RuntimeSlice::H2_5g).unwrap(),
        1
    );

    let missing = json!({
        "input": { "compiler_options": {} },
        "observation": { "writes": [] },
        "runtime_expectation": {}
    });
    assert_eq!(
        expected_h2_5g_owner_activity_for_slice(&missing, H2RuntimeSlice::H2_5g)
            .unwrap_err()
            .to_string(),
        "H2.5g owner control lacks h2_5g_sources"
    );
}
