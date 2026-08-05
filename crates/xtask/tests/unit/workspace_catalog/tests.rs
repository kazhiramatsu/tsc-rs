use super::*;

#[test]
fn resolves_workspace_packages_by_stable_role() {
    let catalog = WorkspaceCatalog::from_metadata_json(
        metadata_json(
            r#"{"tsc-rs":{"role":"checker","dev-profile-opt-level":3}}"#,
            r#"{"tsc-rs":{"role":"fuzz"}}"#,
        )
        .as_bytes(),
    )
    .expect("valid metadata");

    assert_eq!(catalog.workspace_root(), Path::new("/workspace"));
    let checker = catalog.require_package("checker").expect("checker role");
    assert_eq!(checker.role(), "checker");
    assert_eq!(checker.package_name(), "renamed-checker");
    assert_eq!(checker.default_run, None);
    assert_eq!(checker.dev_profile_opt_level(), Some(3));
    assert_eq!(
        checker.manifest_path(),
        Path::new("/workspace/crates/checker/Cargo.toml")
    );
    assert_eq!(checker.targets.len(), 1);
    assert_eq!(checker.targets[0].name, "checker_core");
    assert_eq!(checker.targets[0].kinds, &["lib"]);

    let fuzz = catalog.require_package("fuzz").expect("fuzz role");
    assert_eq!(fuzz.default_run.as_deref(), Some("fuzz-producer"));
    assert_eq!(fuzz.dev_profile_opt_level(), None);
    assert_eq!(
        fuzz.require_default_run_target()
            .expect("valid default-run")
            .name,
        "fuzz-producer"
    );
    assert_eq!(
        fuzz.targets
            .iter()
            .filter(|target| target.is_kind("bin"))
            .map(|target| target.name.as_str())
            .collect::<Vec<_>>(),
        ["fuzz-producer"]
    );
    assert_eq!(catalog.packages().count(), 2);
    assert!(catalog.package_for_role("dependency").is_none());
}

#[test]
fn rejects_a_workspace_package_without_a_role() {
    let error = WorkspaceCatalog::from_metadata_json(
        metadata_json("{}", r#"{"tsc-rs":{"role":"fuzz"}}"#).as_bytes(),
    )
    .expect_err("role is required");

    assert!(error
        .to_string()
        .contains("`renamed-checker` (/workspace/crates/checker/Cargo.toml) is missing"));
}

#[test]
fn rejects_empty_and_non_string_roles() {
    let empty = WorkspaceCatalog::from_metadata_json(
        metadata_json(
            r#"{"tsc-rs":{"role":"  "}}"#,
            r#"{"tsc-rs":{"role":"fuzz"}}"#,
        )
        .as_bytes(),
    )
    .expect_err("empty role is invalid");
    assert!(empty.to_string().contains("has an empty"));

    let non_string = WorkspaceCatalog::from_metadata_json(
        metadata_json(r#"{"tsc-rs":{"role":42}}"#, r#"{"tsc-rs":{"role":"fuzz"}}"#).as_bytes(),
    )
    .expect_err("role must be a string");
    assert!(non_string.to_string().contains("has a non-string"));
}

#[test]
fn rejects_duplicate_roles() {
    let error = WorkspaceCatalog::from_metadata_json(
        metadata_json(
            r#"{"tsc-rs":{"role":"compiler"}}"#,
            r#"{"tsc-rs":{"role":"compiler"}}"#,
        )
        .as_bytes(),
    )
    .expect_err("roles must be unique");

    assert_eq!(
        error.to_string(),
        "workspace role `compiler` is assigned to both `renamed-checker` and `renamed-fuzz`"
    );
}

#[test]
fn rejects_invalid_dev_profile_opt_level() {
    let error = WorkspaceCatalog::from_metadata_json(
        metadata_json(
            r#"{"tsc-rs":{"role":"checker","dev-profile-opt-level":99}}"#,
            r#"{"tsc-rs":{"role":"fuzz"}}"#,
        )
        .as_bytes(),
    )
    .expect_err("opt level must be supported by Cargo");

    assert!(error.to_string().contains("supported integer range 0..=3"));
}

#[test]
fn default_run_must_name_a_binary_target() {
    let metadata = metadata_json(
        r#"{"tsc-rs":{"role":"checker"}}"#,
        r#"{"tsc-rs":{"role":"fuzz"}}"#,
    )
    .replace(
        r#""default_run": "fuzz-producer""#,
        r#""default_run": "missing-producer""#,
    );
    let catalog = WorkspaceCatalog::from_metadata_json(metadata.as_bytes()).expect("valid catalog");

    assert!(catalog
        .require_package("fuzz")
        .expect("fuzz role")
        .require_default_run_target()
        .unwrap_err()
        .to_string()
        .contains("no matching bin target"));
    assert!(catalog
        .require_package("checker")
        .expect("checker role")
        .require_default_run_target()
        .unwrap_err()
        .to_string()
        .contains("does not define package.default-run"));
}

#[test]
fn reports_unknown_roles_with_available_choices() {
    let catalog = WorkspaceCatalog::from_metadata_json(
        metadata_json(
            r#"{"tsc-rs":{"role":"checker"}}"#,
            r#"{"tsc-rs":{"role":"fuzz"}}"#,
        )
        .as_bytes(),
    )
    .expect("valid metadata");

    assert_eq!(
        catalog.require_package("parser").unwrap_err().to_string(),
        "unknown workspace role `parser` (available: checker, fuzz)"
    );
}

fn metadata_json(checker_metadata: &str, fuzz_metadata: &str) -> String {
    format!(
        r#"{{
            "workspace_root": "/workspace",
            "workspace_members": ["checker-id", "fuzz-id"],
            "packages": [
                {{
                    "id": "checker-id",
                    "name": "renamed-checker",
                    "manifest_path": "/workspace/crates/checker/Cargo.toml",
                    "metadata": {checker_metadata},
                    "default_run": null,
                    "targets": [{{
                        "name": "checker_core",
                        "kind": ["lib"],
                        "crate_types": ["lib"],
                        "src_path": "/workspace/crates/checker/src/lib.rs"
                    }}]
                }},
                {{
                    "id": "fuzz-id",
                    "name": "renamed-fuzz",
                    "manifest_path": "/workspace/crates/fuzz/Cargo.toml",
                    "metadata": {fuzz_metadata},
                    "default_run": "fuzz-producer",
                    "targets": [
                        {{
                            "name": "fuzz_core",
                            "kind": ["lib"],
                            "crate_types": ["lib"],
                            "src_path": "/workspace/crates/fuzz/src/lib.rs"
                        }},
                        {{
                            "name": "fuzz-producer",
                            "kind": ["bin"],
                            "crate_types": ["bin"],
                            "src_path": "/workspace/crates/fuzz/src/bin/producer.rs"
                        }}
                    ]
                }},
                {{
                    "id": "external-id",
                    "name": "external-dependency",
                    "manifest_path": "/registry/external/Cargo.toml",
                    "metadata": {{}},
                    "default_run": null,
                    "targets": []
                }}
            ]
        }}"#
    )
}
