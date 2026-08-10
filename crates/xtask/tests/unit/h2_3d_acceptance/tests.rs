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
}

#[test]
fn pinned_h2_4a_owner_controls_are_exact() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    run_h2_4a_owner_controls(&workspace).unwrap();
}
