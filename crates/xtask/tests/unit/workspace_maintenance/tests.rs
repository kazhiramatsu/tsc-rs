use super::*;

#[test]
fn inline_test_modules_are_rejected_but_external_test_modules_are_allowed() {
    assert_eq!(
        first_inline_test_module_line("#[cfg(test)]\nmod tests {\n}\n"),
        Some(1)
    );
    assert_eq!(
        first_inline_test_module_line("#[cfg(test)]\npub(crate) mod tests {\n}\n"),
        Some(1)
    );
    assert_eq!(
        first_inline_test_module_line(
            "#[cfg(test)]\n#[path = \"../tests/unit/lib/tests.rs\"]\nmod tests;\n"
        ),
        None
    );
    assert_eq!(
        first_inline_test_module_line("#[cfg(test)]\nfn test_helper() {}\n"),
        None
    );
}

#[test]
fn compound_cfg_test_module_is_rejected() {
    assert_eq!(
        first_inline_test_module_line(
            "#[cfg(all(test, target_os = \"macos\"))]\n#[allow(dead_code)]\npub mod tests {\n}\n"
        ),
        Some(1)
    );
}

#[test]
fn cfg_not_test_module_is_allowed() {
    for fixture in [
        "#[cfg(not(test))]\nmod tests {\n}\n",
        "#[cfg(all(not(test), not(any(test, unix))))]\nmod tests {\n}\n",
    ] {
        assert!(
            test_module_layout_violations(fixture).is_empty(),
            "fixture: {fixture}"
        );
    }
}

#[test]
fn src_resident_test_module_declaration_is_rejected() {
    assert_eq!(
        test_module_layout_violations(
            "#[cfg(test)]\npub(crate) mod tests; // resolves inside src\n"
        ),
        [TestModuleLayoutViolation {
            line: 1,
            kind: TestModuleLayoutViolationKind::SrcResidentDeclaration,
        }]
    );
}

#[test]
fn path_routed_test_module_declaration_is_allowed() {
    assert!(test_module_layout_violations(
        "#[cfg(all(test, unix))]\n#[allow(dead_code)]\n#[path = \"../tests/unit/lib/tests.rs\"]\nmod tests;\n"
    )
    .is_empty());
}
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_WORKSPACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempWorkspace(PathBuf);

impl TempWorkspace {
    fn new(label: &str) -> Self {
        let sequence = TEMP_WORKSPACE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tsc-rs-workspace-maintenance-{}-{label}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unit_test_layout_reports_all_violations_in_stable_order() {
    let workspace = TempWorkspace::new("all-layout-violations");
    workspace.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"first-package\", \"second-package\"]\nresolver = \"2\"\n",
    );
    for (directory, package, role) in [
        ("first-package", "audit-fixture-first", "alpha"),
        ("second-package", "audit-fixture-second", "beta"),
    ] {
        workspace.write(
            &format!("{directory}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{package}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[package.metadata.tsc-rs]\nrole = \"{role}\"\n"
            ),
        );
        workspace.write(&format!("{directory}/src/lib.rs"), "");
        workspace.write(&format!("{directory}/tests/.keep"), "");
    }
    workspace.write(
        "first-package/src/a_declaration.rs",
        "#[cfg(test)]\nmod tests;\n",
    );
    workspace.write(
        "first-package/src/z_inline.rs",
        "#[cfg(test)]\nmod tests {\n}\n",
    );
    workspace.write(
        "second-package/src/compound.rs",
        "#[cfg(all(test, unix))]\npub(super) mod tests {\n}\n",
    );

    let catalog = WorkspaceCatalog::discover(workspace.path()).unwrap();
    let error = audit_unit_test_layout(&catalog).unwrap_err().to_string();
    assert_eq!(
        error.lines().collect::<Vec<_>>(),
        [
            format!(
                "{}:1 declares a test module in src without #[path]; move the module body below the crate's tests/unit/ tree and add a #[path] attribute to the declaration in src",
                workspace.path().join("first-package/src/a_declaration.rs").display()
            ),
            format!(
                "{}:1 defines tests inline; move the module body below the crate's tests/unit/ tree and retain only a #[path] declaration in src",
                workspace.path().join("first-package/src/z_inline.rs").display()
            ),
            format!(
                "{}:1 defines tests inline; move the module body below the crate's tests/unit/ tree and retain only a #[path] declaration in src",
                workspace.path().join("second-package/src/compound.rs").display()
            ),
        ]
    );
}

#[test]
fn profile_markers_are_strict_and_replacement_is_idempotent() {
    let initial = format!("before\n{PROFILE_BLOCK_BEGIN}\nold\n{PROFILE_BLOCK_END}\nafter\n");
    let expected = format!("{PROFILE_BLOCK_BEGIN}\nnew\n{PROFILE_BLOCK_END}");
    let updated = replace_profile_block(&initial, &expected).unwrap();
    assert_eq!(
        updated,
        format!("before\n{PROFILE_BLOCK_BEGIN}\nnew\n{PROFILE_BLOCK_END}\nafter\n")
    );
    assert_eq!(replace_profile_block(&updated, &expected).unwrap(), updated);

    assert!(profile_block_range("no markers").is_err());
    assert!(profile_block_range(&format!("{PROFILE_BLOCK_END}\n{PROFILE_BLOCK_BEGIN}")).is_err());
    assert!(profile_block_range(&format!(
        "{PROFILE_BLOCK_BEGIN}\n{PROFILE_BLOCK_BEGIN}\n{PROFILE_BLOCK_END}"
    ))
    .is_err());
}

#[test]
fn automation_detects_name_independent_cargo_selectors() {
    for (command, expected) in [
        (
            format!("cargo test {PACKAGE_SHORT_FLAG} tsc-rs-checker"),
            PACKAGE_SHORT_FLAG.to_owned(),
        ),
        (
            format!("cargo test {PACKAGE_SHORT_FLAG}legacy-checker"),
            format!("{PACKAGE_SHORT_FLAG}legacy-checker"),
        ),
        (
            format!("cargo test {PACKAGE_SHORT_FLAG}=future-checker"),
            format!("{PACKAGE_SHORT_FLAG}=future-checker"),
        ),
        (
            format!("cargo test {PACKAGE_LONG_FLAG}='unknown-checker'"),
            format!("{PACKAGE_LONG_FLAG}=unknown-checker"),
        ),
        (
            format!("cargo run {BINARY_LONG_FLAG} $PRODUCER"),
            BINARY_LONG_FLAG.to_owned(),
        ),
        (
            format!("cargo test --workspace {EXCLUDE_LONG_FLAG}=old-package"),
            format!("{EXCLUDE_LONG_FLAG}=old-package"),
        ),
        (
            format!("run: BUILD_KIND=ci cargo test \\\n  {PACKAGE_LONG_FLAG} old-package"),
            PACKAGE_LONG_FLAG.to_owned(),
        ),
        (
            format!("$CARGO +1.93.0 --color always test {PACKAGE_SHORT_FLAG} next-name"),
            PACKAGE_SHORT_FLAG.to_owned(),
        ),
        (
            format!("cargo {PACKAGE_SHORT_FLAG} xtask test"),
            PACKAGE_SHORT_FLAG.to_owned(),
        ),
    ] {
        assert_eq!(automation_cargo_selector(&command), Some(expected));
    }
}

#[test]
fn automation_ignores_non_cargo_flags_comments_prose_and_forwarded_arguments() {
    for text in [
        "mkdir -p target/generated".to_owned(),
        "cancel-in-progress: true".to_owned(),
        format!("# cargo test {PACKAGE_SHORT_FLAG} old-package"),
        format!("printf 'cargo test {PACKAGE_SHORT_FLAG} old-package'"),
        format!("note: \"cargo test {PACKAGE_LONG_FLAG} old-package\""),
        format!("cargo test -- {PACKAGE_LONG_FLAG} is-a-test-argument"),
        format!("cargo xtask custom-command {BINARY_LONG_FLAG} forwarded-value"),
        format!("cargo metadata; mkdir {PACKAGE_SHORT_FLAG} target/generated"),
        format!("echo cargo test {PACKAGE_SHORT_FLAG} old-package"),
    ] {
        assert_eq!(automation_cargo_selector(&text), None, "input: {text}");
    }
}

#[test]
fn workflow_extracts_only_local_step_action_references() {
    let workflow = "\
shared_steps: &shared_steps
  - uses: ./tools/local-action
  - uses: actions/checkout@v6
jobs:
  reusable:
    uses: ./not-a-step-action
  check:
    steps: *shared_steps
env:
  uses: ./not-an-action-field
";
    let analysis = analyze_automation_yaml(workflow).unwrap();
    assert_eq!(analysis.local_action_uses, ["./tools/local-action"]);

    for (value, value_kind) in [
        ("", "null"),
        ("true", "boolean"),
        ("[./tools/action]", "sequence"),
        ("{path: ./tools/action}", "mapping"),
    ] {
        let workflow = format!("steps:\n  - uses: {value}\n");
        assert_eq!(
            analyze_automation_yaml(&workflow),
            Err(AutomationYamlError::NonStringStepField {
                field: "uses",
                value_kind,
            }),
            "workflow:\n{workflow}"
        );
    }
}

#[test]
fn workflow_detects_plain_quoted_flow_block_and_empty_start_run_scalars() {
    for workflow in [
            format!("steps:\n  - run: cargo test {PACKAGE_LONG_FLAG} old-package\n"),
            format!("steps:\n  - run: \"cargo test {PACKAGE_LONG_FLAG} old-package\"\n"),
            format!("steps:\n  - run: 'cargo test {PACKAGE_LONG_FLAG} old-package'\n"),
            format!("steps: [{{run: cargo test {PACKAGE_LONG_FLAG} old-package}}]\n"),
            format!("steps:\n  - run:\n      cargo test {PACKAGE_LONG_FLAG} old-package\n"),
            format!("steps:\n  - run: !!str cargo test {PACKAGE_LONG_FLAG} old-package\n"),
            format!(
                "name: local action\nruns:\n  using: composite\n  steps:\n    - run: cargo test {PACKAGE_LONG_FLAG} old-package\n      shell: bash\n"
            ),
        ] {
            assert_eq!(
                workflow_cargo_selector(&workflow).unwrap().as_deref(),
                Some(PACKAGE_LONG_FLAG),
                "workflow:\n{workflow}"
            );
        }

    for header in [">", ">-", ">+"] {
        let workflow = format!(
                "jobs:\n  check:\n    steps:\n      - run: {header}\n          cargo test\n          {PACKAGE_LONG_FLAG} old-package\n"
            );
        assert_eq!(
            workflow_cargo_selector(&workflow).unwrap().as_deref(),
            Some(PACKAGE_LONG_FLAG),
            "workflow:\n{workflow}"
        );
    }

    let literal = format!(
            "jobs:\n  check:\n    steps:\n      - run: |\n          cargo test \\\n          {PACKAGE_LONG_FLAG} old-package\n"
        );
    assert_eq!(
        workflow_cargo_selector(&literal).unwrap().as_deref(),
        Some(PACKAGE_LONG_FLAG)
    );
}

#[test]
fn workflow_detects_multiline_quoted_and_plain_run_scalars() {
    for workflow in [
        format!("steps:\n  - run: \"cargo test\n      {PACKAGE_LONG_FLAG} old-package\"\n"),
        format!("steps:\n  - run: 'cargo test\n      {PACKAGE_LONG_FLAG} old-package'\n"),
        format!("steps:\n  - run: cargo test\n      {PACKAGE_LONG_FLAG} old-package\n"),
    ] {
        assert_eq!(
            workflow_cargo_selector(&workflow).unwrap().as_deref(),
            Some(PACKAGE_LONG_FLAG),
            "workflow:\n{workflow}"
        );
    }
}

#[test]
fn workflow_ignores_non_run_prose_and_forwarded_run_arguments() {
    for workflow in [
            format!("steps:\n  - name: \"cargo test {PACKAGE_LONG_FLAG} old-package\"\n"),
            format!(
                "steps:\n  - run: \"printf 'cargo test {PACKAGE_LONG_FLAG} old-package'\"\n"
            ),
            format!("steps:\n  - run: 'cargo test -- {PACKAGE_LONG_FLAG} test-argument'\n"),
            format!("steps:\n  - run: cargo xtask custom {BINARY_LONG_FLAG} forwarded\n"),
            format!(
                "steps:\n  - run: >-\n      cargo test\n\n      {PACKAGE_LONG_FLAG} separate-command\n"
            ),
            format!(
                "steps:\n  - run: >\n      cargo test\n        {PACKAGE_LONG_FLAG} more-indented-command\n"
            ),
            format!(
                "steps:\n  - run: cargo test\n    name: cargo test {PACKAGE_LONG_FLAG} old-package\n"
            ),
            format!(
                "steps:\n  - run: \"printf cargo-test\n      cargo test\"\n    name: cargo test {PACKAGE_LONG_FLAG} old-package\n"
            ),
            format!(
                "steps:\n  - run: cargo test --\n      {PACKAGE_LONG_FLAG} test-argument\n"
            ),
            format!(
                "defaults:\n  run:\n    shell: bash\nenv:\n  run: cargo test {PACKAGE_LONG_FLAG} env-value\non:\n  workflow_call:\n    inputs:\n      run:\n        type: string\n        default: cargo test {PACKAGE_LONG_FLAG} input-default\njobs:\n  check:\n    steps:\n      - name: safe step\n"
            ),
        ] {
            assert_eq!(
                workflow_cargo_selector(&workflow),
                Ok(None),
                "workflow:\n{workflow}"
            );
        }
}

#[test]
fn workflow_stops_quoted_continuations_before_the_next_key_and_list_item() {
    let workflow = format!(
            "steps:\n  - run: \"cargo test\n      --\"\n    name: cargo test {PACKAGE_LONG_FLAG} old-package\n  - name: cargo test {PACKAGE_LONG_FLAG} old-package\n  - run: echo done\n"
        );
    assert_eq!(
        workflow_run_scripts(&workflow).unwrap(),
        ["cargo test --", "echo done"]
    );
    assert_eq!(workflow_cargo_selector(&workflow), Ok(None));
}

#[test]
fn workflow_resolves_run_anchors_aliases_and_tags() {
    for workflow in [
        format!("steps:\n  - run: &test cargo test {PACKAGE_LONG_FLAG} old-package\n"),
        format!(
            "command: &test cargo test {PACKAGE_LONG_FLAG} old-package\nsteps:\n  - run: *test\n"
        ),
        format!(
            "step: &test\n  run: cargo test {PACKAGE_LONG_FLAG} old-package\nsteps:\n  - *test\n"
        ),
        format!("steps:\n  - run: !shell cargo test {PACKAGE_LONG_FLAG} old-package\n"),
    ] {
        assert_eq!(
            workflow_cargo_selector(&workflow).unwrap().as_deref(),
            Some(PACKAGE_LONG_FLAG),
            "workflow:\n{workflow}"
        );
    }
}

#[test]
fn workflow_rejects_invalid_yaml_and_non_string_run_values() {
    let error = workflow_cargo_selector("steps:\n  - run: [unterminated\n").unwrap_err();
    let AutomationYamlError::InvalidYaml(message) = error else {
        panic!("expected invalid YAML error, got {error:?}");
    };
    assert!(!message.is_empty());

    for (value, value_kind) in [
        ("", "null"),
        ("null", "null"),
        ("true", "boolean"),
        ("42", "number"),
        ("[echo, done]", "sequence"),
        ("{shell: bash}", "mapping"),
    ] {
        let workflow = format!("steps:\n  - run: {value}\n");
        assert_eq!(
            workflow_cargo_selector(&workflow),
            Err(AutomationYamlError::NonStringStepField {
                field: "run",
                value_kind,
            }),
            "workflow:\n{workflow}"
        );
        assert!(workflow_cargo_selector(&workflow)
            .unwrap_err()
            .to_string()
            .contains("must be a YAML string"));
    }
}

#[test]
fn yaml_automation_manifest_names_are_recognized() {
    assert!(is_yaml_manifest(Path::new(".github/workflows/ci.yml")));
    assert!(is_yaml_manifest(Path::new(
        ".github/workflows/release.yaml"
    )));
    assert!(!is_yaml_manifest(Path::new(".github/workflows/README.md")));
    assert!(!is_yaml_manifest(Path::new(
        ".github/workflows/ci.yml.disabled"
    )));

    assert!(is_action_manifest(Path::new(
        ".github/actions/check/action.yml"
    )));
    assert!(is_action_manifest(Path::new(
        ".github/actions/check/action.yaml"
    )));
    assert!(!is_action_manifest(Path::new(
        ".github/actions/check/README.md"
    )));
    assert!(!is_action_manifest(Path::new(".github/workflows/ci.yml")));
}

#[test]
fn local_action_manifest_resolution_is_workspace_bounded_and_fail_closed() {
    let workspace = TempWorkspace::new("action-resolution");
    workspace.write("tools/action/action.yml", "runs:\n  using: composite\n");
    let workspace_root = fs::canonicalize(workspace.path()).unwrap();
    let expected = fs::canonicalize(workspace.path().join("tools/action/action.yml")).unwrap();
    assert_eq!(
        resolve_local_action_manifest(&workspace_root, "./tools/action").unwrap(),
        expected
    );
    assert!(local_action_relative_directory("./../outside")
        .unwrap_err()
        .contains("escape"));
    assert!(local_action_relative_directory("./tools/other/../action")
        .unwrap_err()
        .contains("may not contain"));
    assert!(
        resolve_local_action_manifest(&workspace_root, "./tools/missing")
            .unwrap_err()
            .contains("does not exist")
    );

    fs::create_dir_all(workspace.path().join("tools/no-manifest")).unwrap();
    assert!(
        resolve_local_action_manifest(&workspace_root, "./tools/no-manifest")
            .unwrap_err()
            .contains("has no action.yml or action.yaml")
    );

    workspace.write("tools/ambiguous/action.yml", "name: first\n");
    workspace.write("tools/ambiguous/action.yaml", "name: second\n");
    assert!(
        resolve_local_action_manifest(&workspace_root, "./tools/ambiguous")
            .unwrap_err()
            .contains("contains both")
    );
}

#[cfg(unix)]
#[test]
fn local_action_manifest_resolution_rejects_symlink_escape() {
    let workspace = TempWorkspace::new("action-symlink-workspace");
    let outside = TempWorkspace::new("action-symlink-outside");
    outside.write("action.yml", "runs:\n  using: composite\n");
    fs::create_dir_all(workspace.path().join("tools")).unwrap();
    std::os::unix::fs::symlink(
        outside.path(),
        workspace.path().join("tools/escaped-action"),
    )
    .unwrap();

    let workspace_root = fs::canonicalize(workspace.path()).unwrap();
    assert!(
        resolve_local_action_manifest(&workspace_root, "./tools/escaped-action")
            .unwrap_err()
            .contains("outside workspace")
    );
}

#[test]
fn referenced_local_actions_are_recursively_audited_with_cycle_protection() {
    let workspace = TempWorkspace::new("recursive-actions");
    workspace.write(
        ".github/workflows/ci.yml",
        "jobs:\n  check:\n    steps:\n      - uses: ./tools/action-a\n",
    );
    workspace.write(
        "tools/action-a/action.yml",
        "runs:\n  using: composite\n  steps:\n    - uses: ./tools/action-b\n",
    );
    let forbidden = format!(
            "runs:\n  using: composite\n  steps:\n    - uses: ./tools/action-a\n    - run: cargo test {PACKAGE_LONG_FLAG} old-package\n      shell: bash\n"
        );
    workspace.write("tools/action-b/action.yaml", &forbidden);
    workspace.write(".github/actions/unused/action.yml", &forbidden);

    let error = audit_automation_package_selectors(workspace.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("tools/action-b/action.yaml"));
    assert!(error.contains(PACKAGE_LONG_FLAG));

    workspace.write(
            "tools/action-b/action.yaml",
            "runs:\n  using: composite\n  steps:\n    - uses: ./tools/action-a\n    - run: cargo test --workspace\n      shell: bash\n",
        );
    audit_automation_package_selectors(workspace.path()).unwrap();
}

#[test]
fn rust_source_detects_separate_and_joined_selectors_in_string_literals() {
    let selectors = [
        PACKAGE_SHORT_FLAG.to_owned(),
        format!("{PACKAGE_SHORT_FLAG}legacy-checker"),
        format!("{PACKAGE_SHORT_FLAG}=future-checker"),
        PACKAGE_LONG_FLAG.to_owned(),
        format!("{PACKAGE_LONG_FLAG}=unknown-checker"),
        BINARY_LONG_FLAG.to_owned(),
        format!("{BINARY_LONG_FLAG}=old-producer"),
        EXCLUDE_LONG_FLAG.to_owned(),
        format!("{EXCLUDE_LONG_FLAG}=old-package"),
    ];
    for selector in selectors {
        for source in [
            format!(".arg({selector:?})"),
            format!(r###".arg(r#"{selector}"#)"###),
            format!(r####".arg(br##"{selector}"##)"####),
            format!(".arg(b{selector:?})"),
        ] {
            assert_eq!(
                rust_source_cargo_selector(&source).as_deref(),
                Some(selector.as_str()),
                "source: {source}"
            );
        }
    }

    let escaped = format!(r#".arg("\x2d{}legacy-checker")"#, "p");
    assert_eq!(
        rust_source_cargo_selector(&escaped).as_deref(),
        Some(format!("{PACKAGE_SHORT_FLAG}legacy-checker").as_str())
    );

    let embedded = format!("cargo test {PACKAGE_SHORT_FLAG} legacy-checker");
    let embedded_source = format!("Command::new(\"sh\").arg(\"-c\").arg({embedded:?})");
    assert_eq!(
        rust_source_cargo_selector(&embedded_source).as_deref(),
        Some(PACKAGE_SHORT_FLAG)
    );
}

#[test]
fn rust_source_ignores_comments_and_non_selector_literals() {
    let joined = format!("{PACKAGE_LONG_FLAG}=legacy-checker");
    let commented =
        format!("// .arg({joined:?})\n/* outer .arg({joined:?}) /* nested */ still comment */\n");
    assert_eq!(rust_source_cargo_selector(&commented), None);

    for allowed in [
        ".arg(\"--manifest-path\")",
        ".arg(\"--profile\")",
        "let message = \"documentation mentions cargo test -p old-package\";",
        "let lifetime: &'a str = \"ordinary prose\";",
    ] {
        assert_eq!(
            rust_source_cargo_selector(allowed),
            None,
            "source: {allowed}"
        );
    }
}

#[test]
fn retired_comment_scope_identifiers_are_denied_and_the_threaded_family_is_legal() {
    // Positive canaries: the rule fires on the constructor token and on
    // every retired shim definition form.
    assert_eq!(
        first_retired_comment_scope_identifier("    EmitContext::detached_transitional()\n"),
        Some((1, "detached_transitional")),
    );
    for (definition, expected) in [
        ("    fn emit_required_node(\n", "fn emit_required_node("),
        ("    fn emit_node_id(\n", "fn emit_node_id("),
        ("    fn emit_identifier_name(\n", "fn emit_identifier_name("),
        (
            "    fn emit_required_identifier_name(\n",
            "fn emit_required_identifier_name(",
        ),
        (
            "    fn emit_child_after_token(\n",
            "fn emit_child_after_token(",
        ),
    ] {
        assert_eq!(
            first_retired_comment_scope_identifier(definition),
            Some((1, expected)),
            "{definition:?}",
        );
    }
    // The threaded family and its compound variants stay legal.
    let legal = concat!(
        "    fn emit_node_id_with_context(\n",
        "    fn emit_required_node_with_context(\n",
        "    fn emit_identifier_name_with_context(\n",
        "    fn emit_child_after_token_with_context_and_source_extent(\n",
        "    self.emit_required_identifier_name_with_context(\n",
    );
    assert_eq!(first_retired_comment_scope_identifier(legal), None);
}

#[test]
fn automation_selector_skips_binary_script_artifacts() {
    assert_eq!(
        automation_selector_in_bytes(&[0xff, 0xfe, 0x00, 0x2a]),
        None
    );
    assert_eq!(
        automation_selector_in_bytes(format!("cargo test {PACKAGE_SHORT_FLAG} xtask").as_bytes()),
        Some(PACKAGE_SHORT_FLAG.to_owned())
    );
}
