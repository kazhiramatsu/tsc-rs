use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).expect("read JSON")).expect("parse JSON")
}

#[test]
fn h2_7b_validator_accepts_only_the_frozen_artifact() {
    let workspace = workspace();
    let mut artifact = read_json(&workspace.join(super::H2_7B_QUALIFICATION_RELATIVE_PATH));
    assert_eq!(
        super::validate_h2_7b_qualification(&artifact)
            .expect("frozen H2.7b artifact")
            .len(),
        1_593
    );
    artifact["qualification_fingerprint_sha256"] = serde_json::json!("0".repeat(64));
    assert!(super::validate_h2_7b_qualification(&artifact).is_err());
}

#[test]
fn h2_vectorizer_kind_profiles_cover_both_frozen_artifacts() {
    let workspace = workspace();
    for (relative, profile, expected) in [
        (
            super::H2_6C_QUALIFICATION_RELATIVE_PATH,
            super::H2MismatchProfile::H2_6c,
            BTreeSet::from(["declaration", "javascript", "jsx", "other", "source-map"]),
        ),
        (
            super::H2_7B_QUALIFICATION_RELATIVE_PATH,
            super::H2MismatchProfile::H2_7b,
            BTreeSet::from([
                "cjs",
                "declaration",
                "javascript",
                "jsx",
                "mjs",
                "source-map",
            ]),
        ),
    ] {
        let artifact = read_json(&workspace.join(relative));
        let mut observed = BTreeSet::new();
        for case in artifact["cases"].as_array().expect("cases") {
            let Some(observation) = case.get("typescript_observation") else {
                continue;
            };
            for write in observation["writes"].as_array().expect("writes") {
                let kind = write["kind"].as_str().expect("kind");
                let path = write["path"].as_str().expect("path");
                super::normalized_oracle_write_kind(profile, kind, path)
                    .unwrap_or_else(|error| panic!("{relative}: {path}: {error}"));
                observed.insert(kind);
            }
        }
        assert_eq!(observed, expected, "{relative}");
    }
}

#[test]
fn h2_7b_manifest_contract_covers_absent_scratch_and_empty_target() {
    let directory = std::env::temp_dir().join(format!(
        "tsc-rs-h2-7b-manifest-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir(&directory).expect("create temp directory");
    let absent = directory.join("absent.json");
    let loaded = super::load_h2_vector_divergence_manifest(
        &absent,
        "H2.7b",
        super::H2_7B_DIVERGENCE_OWNER,
        false,
    )
    .expect("absent manifest");
    assert!(loaded.entries.is_empty());

    let scratch = directory.join("scratch.json");
    let mut divergence = super::H2VectorDivergence {
        writes_diverging: 1,
        ..super::H2VectorDivergence::default()
    };
    super::push_mismatch(
        &mut divergence.mismatch_vector,
        super::H2MismatchProfile::H2_7b,
        "write",
        0,
        "presence",
        serde_json::Value::Null,
    );
    divergence.finish_vector();
    super::write_h2_7b_divergence_manifest(&scratch, &[("case".to_owned(), divergence.clone())])
        .expect("scratch manifest");
    assert!(!absent.exists());
    assert_eq!(
        super::load_h2_vector_divergence_manifest(
            &scratch,
            "H2.7b",
            super::H2_7B_DIVERGENCE_OWNER,
            false,
        )
        .expect("load scratch")
        .entries
        .get("case"),
        Some(&divergence)
    );

    let canonical_empty = directory.join("canonical.json");
    fs::copy(&scratch, &canonical_empty).expect("seed canonical target");
    super::write_h2_7b_divergence_manifest(&canonical_empty, &[])
        .expect("remove empty canonical target");
    assert!(!canonical_empty.exists());
    fs::remove_dir_all(&directory).expect("remove temp directory");
}

#[test]
fn h2_6c_refused_option_map_has_the_two_frozen_totals() {
    let workspace = workspace();
    let manifest = read_json(&workspace.join(super::H2_6C_KNOWN_DIVERGENCES_RELATIVE_PATH));
    let mut pre_flip = BTreeMap::<String, u64>::new();
    for case in manifest["cases"].as_array().expect("manifest cases") {
        if let Some(option) = case["refused_option"].as_str() {
            *pre_flip.entry(option.to_owned()).or_default() += 1;
        }
    }
    assert_eq!(
        pre_flip,
        BTreeMap::from([
            ("isolatedModules".to_owned(), 1),
            ("outDir".to_owned(), 130),
            ("outFile".to_owned(), 144),
            ("rootDir".to_owned(), 4),
        ])
    );

    let qualification = read_json(&workspace.join(super::H2_6C_QUALIFICATION_RELATIVE_PATH));
    let mut transitions = BTreeMap::<String, u64>::new();
    let mut transition_ids = BTreeSet::new();
    for case in qualification["cases"]
        .as_array()
        .expect("qualification cases")
    {
        let Some(input) = case.get("input").filter(|input| input.is_object()) else {
            continue;
        };
        let settings = input["settings"].as_array().expect("settings");
        let has_declaration_map = settings.iter().any(|setting| {
            setting["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("declarationMap"))
        });
        let has_out_file = settings.iter().any(|setting| {
            setting["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("outFile"))
        });
        let option = if has_declaration_map {
            Some("declarationMap")
        } else if has_out_file {
            Some("outFile")
        } else {
            None
        };
        if let Some(option) = option {
            *transitions.entry(option.to_owned()).or_default() += 1;
            transition_ids.insert(case["case_id"].as_str().expect("case_id"));
        }
    }
    assert_eq!(
        transitions,
        BTreeMap::from([("declarationMap".to_owned(), 6), ("outFile".to_owned(), 27),])
    );
    assert_eq!(transition_ids.len(), 33);
    for (option, count) in transitions {
        *pre_flip.entry(option).or_default() += count;
    }
    assert_eq!(
        pre_flip,
        BTreeMap::from([
            ("declarationMap".to_owned(), 6),
            ("isolatedModules".to_owned(), 1),
            ("outDir".to_owned(), 130),
            ("outFile".to_owned(), 171),
            ("rootDir".to_owned(), 4),
        ])
    );
}

#[test]
fn qualified_vfs_virtual_config_precedes_directives_and_retains_floor_refusals() {
    let workspace = workspace();
    let files = vec![
        (
            PathBuf::from("/project/input.ts"),
            b"export const value = 1;\n".to_vec(),
        ),
        (
            PathBuf::from("/project/tsconfig.json"),
            br#"{
                "compilerOptions": {
                    "declaration": true,
                    "emitDeclarationOnly": true,
                    "module": "esnext",
                    "moduleResolution": "bundler",
                    "types": []
                }
            }"#
            .to_vec(),
        ),
    ];
    let roots = vec![PathBuf::from("/project/input.ts")];
    let settings = vec![
        ("emitDeclarationOnly".to_owned(), "false".to_owned()),
        ("module".to_owned(), "commonjs".to_owned()),
    ];
    let prepared =
        tsc_harness::upstream_suites::execution::load_qualified_compiler_emit_with_option_floor(
            &workspace,
            "/project",
            &files,
            &roots,
            &settings,
            super::limits(),
            tsc_harness::upstream_suites::execution::EmitOptionFloor::DeclarationFamily,
        )
        .expect("virtual config and directive layers");
    assert_eq!(prepared.compiler_options().declaration, Some(true));
    assert_eq!(
        prepared.compiler_options().emit_declaration_only,
        Some(false),
        "directive overrides the config value"
    );
    assert_eq!(prepared.compiler_options().module, Some(1));
    assert_eq!(prepared.compiler_options().module_resolution, Some(100));
    assert_eq!(prepared.program_options().types(), Some([].as_slice()));

    let later_owned_files = vec![
        (
            PathBuf::from("/project/input.ts"),
            b"export const value = 1;\n".to_vec(),
        ),
        (
            PathBuf::from("/project/tsconfig.json"),
            br#"{"compilerOptions":{"declaration":true,"composite":true,"types":[]}}"#.to_vec(),
        ),
    ];
    let prepared =
        tsc_harness::upstream_suites::execution::load_qualified_compiler_emit_with_option_floor(
            &workspace,
            "/project",
            &later_owned_files,
            &roots,
            &[],
            super::limits(),
            tsc_harness::upstream_suites::execution::EmitOptionFloor::DeclarationFamily,
        )
        .expect("config-sourced later-owned option reaches the emitter floor");
    let mut sink = tsc_compiler::MemoryOutputSink::new();
    assert!(matches!(
        tsc_compiler::ProgramSession::new(prepared).emit(&mut sink),
        Err(tsc_compiler::DriverError::Emit(
            tsc_compiler::EmitFailure::UnsupportedCompilerOption {
                option: "composite"
            }
        ))
    ));
    assert!(sink.writes().is_empty());
}
