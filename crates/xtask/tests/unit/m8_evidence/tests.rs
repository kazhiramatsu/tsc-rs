use super::*;

#[test]
fn workspace_cleanliness_pathspec_supports_nested_and_root_layouts() {
    let root = Path::new("/workspace");
    assert_eq!(
        workspace_git_pathspec(root, &root.join("tsc-rs")).unwrap(),
        PathBuf::from("tsc-rs")
    );
    assert_eq!(
        workspace_git_pathspec(root, root).unwrap(),
        PathBuf::from(".")
    );
}

#[test]
fn evidence_paths_cannot_escape_the_workspace() {
    let workspace = Path::new("/workspace/tsc-rs");
    assert_eq!(
        secure_workspace_path(workspace, "target/m8/evidence")
            .unwrap()
            .to_string_lossy(),
        "/workspace/tsc-rs/target/m8/evidence"
    );
    assert!(secure_workspace_path(workspace, "../outside").is_err());
    assert!(secure_workspace_path(workspace, "/tmp/outside").is_err());
}

#[cfg(unix)]
#[test]
fn invalidation_rejects_parent_symlinks_before_removing_the_target() {
    use std::os::unix::fs::symlink;

    let nonce = now_unix_ms().unwrap();
    let workspace = std::env::temp_dir().join(format!(
        "tsc-rs-m8-invalidation-workspace-{}-{nonce}",
        std::process::id()
    ));
    let outside = std::env::temp_dir().join(format!(
        "tsc-rs-m8-invalidation-outside-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let outside_artifact = outside.join("performance.json");
    fs::write(&outside_artifact, b"must survive\n").unwrap();
    symlink(&outside, workspace.join("target")).unwrap();
    let apparent_artifact = workspace.join("target/performance.json");

    assert!(invalidate_published_files(&workspace, [&apparent_artifact]).is_err());
    assert_eq!(fs::read(&outside_artifact).unwrap(), b"must survive\n");

    fs::remove_file(workspace.join("target")).unwrap();
    fs::remove_dir_all(workspace).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn divergence_signature_ignores_positions_in_canonical_rows() {
    let oracle = vec![json!({"code":2322,"head":"Type mismatch","start":1})];
    let tsrs = vec![json!({"code":2322,"head":"Type mismatch","start":99})];
    let signature = divergence_signature("t2", &oracle, &tsrs).unwrap();
    assert!(signature.contains("2322:Type mismatch"));
    assert!(!signature.contains("start"));
}

#[test]
fn t4_renderer_classifier_pins_precedence_and_stable_signature() {
    let a = json!({"code":2322,"head":"Type mismatch","start":1});
    let b = json!({"code":2345,"head":"Bad argument","start":9});
    assert_eq!(
        renderer_divergence_class(
            &[a.clone(), b.clone()],
            &[b.clone(), a.clone()],
            "oracle",
            "tsrs",
        ),
        "order"
    );
    assert_eq!(
        renderer_divergence_class(
            &[a.clone(), a.clone()],
            std::slice::from_ref(&a),
            "oracle",
            "tsrs",
        ),
        "dedupe"
    );
    let oracle_path = "/oracle/main.ts:1:1 - error TS2322: Type mismatch\n";
    let tsrs_path = "C:/work/main.ts:1:1 - error TS2322: Type mismatch\n";
    assert_eq!(
        renderer_divergence_class(
            std::slice::from_ref(&a),
            std::slice::from_ref(&a),
            oracle_path,
            tsrs_path,
        ),
        "path"
    );
    assert_eq!(
        renderer_divergence_class(
            std::slice::from_ref(&a),
            std::slice::from_ref(&a),
            "error TS2322: x\r\n",
            "error TS2322: x\n",
        ),
        "newline"
    );
    assert_eq!(
        renderer_divergence_class(
            std::slice::from_ref(&a),
            std::slice::from_ref(&a),
            "error TS2322: x\n",
            "error TS2322: y\n",
        ),
        "text"
    );

    let signature = t4_divergence_signature(
        std::slice::from_ref(&a),
        std::slice::from_ref(&a),
        oracle_path,
        tsrs_path,
    )
    .unwrap();
    assert!(signature.contains("tier=t4"));
    assert!(signature.contains("renderer=path"));
    assert!(signature.contains("key=2322:Type mismatch"));
    assert!(!signature.contains("/oracle"));
    assert!(!signature.contains("start"));
}

#[test]
fn fuzzer_pool_is_pinned_and_has_no_normal_oracle_worker() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let pool = verified_fuzzer_oracle_pool(&workspace).unwrap();

    assert!(
        pool.node_version().is_err(),
        "fuzzer must not eagerly launch or retain a normal oracle worker"
    );
    assert!(pool.render_node_version().is_ok());
}

#[test]
fn rss_parsers_pin_backend_units() {
    assert_eq!(
        parse_max_rss("  123456  maximum resident set size\n", "bsd-time-l").unwrap(),
        123456
    );
    assert_eq!(
        parse_max_rss("Maximum resident set size (kbytes): 1024\n", "gnu-time-v").unwrap(),
        1024 * 1024
    );
}

#[test]
fn fuzzer_raw_rejects_uncompared_and_unreduced_artifacts() {
    let header = ArtifactHeader {
        schema: ARTIFACT_SCHEMA,
        producer_version: PRODUCER_VERSION.to_owned(),
        kind: "differential-fuzzer".to_owned(),
        producer_commit: "0".repeat(40),
        command: "fuzz".to_owned(),
        started_unix_ms: 1,
        finished_unix_ms: 2,
        exit_status: 0,
        fingerprint: Fingerprint {
            sha256: "a".repeat(64),
            inputs: Vec::new(),
        },
    };
    let mut artifact = FuzzerArtifact {
        header,
        seed: 1,
        requested_cases: 1,
        cases: Vec::new(),
        reducer: ReducerObservation {
            exercised: false,
            mutation_canary: false,
            original_signature: None,
            reduced_signature: None,
            original_bytes: 0,
            reduced_bytes: 0,
            reduced_source: None,
        },
        dedupe: DedupeObservation {
            exercised: false,
            observed_signatures: Vec::new(),
            unique_signatures: Vec::new(),
        },
    };
    assert!(verify_fuzzer_raw(&artifact).is_err());
    artifact.requested_cases = 0;
    assert!(verify_fuzzer_raw(&artifact).is_err());
}

#[test]
fn m9_reduce_is_fail_closed_before_reading_an_artifact() {
    let error = fuzz_reduce(["/tmp/does-not-need-to-exist.json".to_owned()].into_iter())
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "fuzz reduce is fail-closed until the M9.1d real-replay reducer lands"
    );
}

#[test]
fn runtime_reuse_requires_fresh_inputs_and_exact_zero_hit_reviews() {
    let fingerprint = Fingerprint {
        sha256: "a".repeat(64),
        inputs: Vec::new(),
    };
    let mut artifact = RuntimeArtifact {
        header: ArtifactHeader {
            schema: ARTIFACT_SCHEMA,
            producer_version: PRODUCER_VERSION.to_owned(),
            kind: "runtime-coverage".to_owned(),
            producer_commit: "0".repeat(40),
            command: "cargo xtask coverage emitters --corpus".to_owned(),
            started_unix_ms: 1,
            finished_unix_ms: 2,
            exit_status: 0,
            fingerprint: fingerprint.clone(),
        },
        inventory_sha256: "inventory".to_owned(),
        raw_counts: BTreeMap::from([("hit".to_owned(), 1)]),
        zero_hit_reviews: vec![ZeroHitReviewArtifact {
            declaration: "zero".to_owned(),
            evidence: "reviewed exact declaration".to_owned(),
        }],
    };
    let direct = BTreeSet::from(["hit", "zero"]);
    assert!(validate_runtime_artifact(&artifact, &fingerprint, "inventory", &direct).ready);

    artifact.zero_hit_reviews[0].declaration = "hit".to_owned();
    assert!(!validate_runtime_artifact(&artifact, &fingerprint, "inventory", &direct).ready);
    artifact.zero_hit_reviews[0].declaration = "zero".to_owned();
    artifact.header.fingerprint.sha256 = "b".repeat(64);
    assert!(!validate_runtime_artifact(&artifact, &fingerprint, "inventory", &direct).ready);

    artifact.header.fingerprint = fingerprint.clone();
    artifact.header.schema = 1;
    assert!(
        !validate_runtime_artifact(&artifact, &fingerprint, "inventory", &direct).ready,
        "schema-1 evidence must not survive the schema-2 receipt transition"
    );
    artifact.header.schema = ARTIFACT_SCHEMA;
    artifact.header.producer_version = "m8-evidence-v1".to_owned();
    assert!(
        !validate_runtime_artifact(&artifact, &fingerprint, "inventory", &direct).ready,
        "the retired producer version must fail closed"
    );
}

#[test]
fn artifact_header_requires_exact_identity_head_and_time_order() {
    let fingerprint = Fingerprint {
        sha256: "a".repeat(64),
        inputs: Vec::new(),
    };
    let head = "b".repeat(40);
    let mut header = ArtifactHeader {
        schema: ARTIFACT_SCHEMA,
        producer_version: PRODUCER_VERSION.to_owned(),
        kind: "performance".to_owned(),
        producer_commit: head.clone(),
        command: "fixed-command".to_owned(),
        started_unix_ms: 1,
        finished_unix_ms: 2,
        exit_status: 0,
        fingerprint: fingerprint.clone(),
    };
    assert!(artifact_header_matches(
        &header,
        "performance",
        "fixed-command",
        &fingerprint,
        Some(&head)
    ));

    header.producer_commit = "c".repeat(40);
    assert!(!artifact_header_matches(
        &header,
        "performance",
        "fixed-command",
        &fingerprint,
        Some(&head)
    ));
    header.producer_commit = head.clone();
    header.finished_unix_ms = 0;
    assert!(!artifact_header_matches(
        &header,
        "performance",
        "fixed-command",
        &fingerprint,
        Some(&head)
    ));
    header.finished_unix_ms = 2;
    header.kind = "runtime-coverage".to_owned();
    assert!(!artifact_header_matches(
        &header,
        "performance",
        "fixed-command",
        &fingerprint,
        Some(&head)
    ));
}

#[test]
fn receipt_bound_manifest_schema_rejects_unknown_fields() {
    let manifest = json!({
        "schema": ARTIFACT_SCHEMA,
        "producer_version": PRODUCER_VERSION,
        "producer_commit": "0".repeat(40),
        "artifacts": [],
        "ci_conformance": {
            "receipt": {
                "kind": "receipt",
                "path": "target/m8/evidence/ci-conformance-receipt.json",
                "bytes": 1,
                "sha256": "a".repeat(64)
            },
            "outputs": []
        },
        "unreviewed_extension": true
    });
    assert!(serde_json::from_value::<EvidenceManifest>(manifest).is_err());
}

#[test]
fn ci_output_binding_rejects_order_and_content_tampering() {
    let workspace = std::env::temp_dir().join(format!(
        "tsc-rs-m8-binding-{}-{}",
        std::process::id(),
        now_unix_ms().unwrap()
    ));
    let config = EvidenceConfig {
        schema: 2,
        artifact_dir: "target/m8/evidence".to_owned(),
        runtime_coverage: RuntimeConfig {
            artifact: "runtime.json".to_owned(),
            max_workers: 1,
            programs_per_process: 1,
            max_lib_cache_buckets: 1,
            diagnostic_canary_programs: 1,
            zero_hit_reviews: Vec::new(),
        },
        workspace_tests: WorkspaceTestsConfig { max_workers: 4 },
        conformance_runner: ConformanceRunnerConfig { max_workers: 4 },
        fuzzer: FuzzerConfig {
            artifact: "fuzz.json".to_owned(),
            seed: 1,
            cases: 1,
        },
        performance: PerformanceConfig {
            artifact: "performance.json".to_owned(),
            default_runner_profile: "test".to_owned(),
            runners: Vec::new(),
        },
    };
    let paths = ci_conformance_paths(&workspace, &config).unwrap();
    assert_eq!(
        paths.families,
        workspace.join("target/families/report.json")
    );
    for (path, bytes) in [
        (&paths.receipt, b"receipt".as_slice()),
        (&paths.all, b"all".as_slice()),
        (&paths.two_xxx, b"2xxx".as_slice()),
        (&paths.syntactic, b"syntactic".as_slice()),
        (&paths.families, b"families".as_slice()),
    ] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    let binding = bind_ci_conformance(&workspace, &paths).unwrap();
    verify_ci_conformance_binding(&workspace, &config, &binding).unwrap();

    let mut reordered = binding.clone();
    reordered.outputs.swap(0, 1);
    assert!(verify_ci_conformance_binding(&workspace, &config, &reordered).is_err());

    fs::write(&paths.two_xxx, b"tampered").unwrap();
    assert!(verify_ci_conformance_binding(&workspace, &config, &binding).is_err());
    fs::remove_dir_all(workspace).unwrap();
}
