use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_harness::upstream_suites::{
    check_recorded_manifest, generate_manifest, render_manifest, validate_manifest,
    CaseConfiguration, ExecutionState, ExpansionManifest, ExpansionSummary, ProjectInputFiles,
    ProjectInputPresence, ProjectModule, SourceEncoding, SuiteName, UnitContent,
    MANIFEST_RELATIVE_PATH,
};

const SUITES: [(SuiteName, &str); 3] = [
    (SuiteName::Compiler, "ts-tests/tests/cases/compiler"),
    (SuiteName::Project, "ts-tests/tests/cases/project"),
    (SuiteName::Projects, "ts-tests/tests/cases/projects"),
];

static MANIFEST: OnceLock<ExpansionManifest> = OnceLock::new();

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest() -> &'static ExpansionManifest {
    MANIFEST.get_or_init(|| {
        generate_manifest(&workspace_root())
            .unwrap_or_else(|error| panic!("failed to expand the pinned upstream suites: {error}"))
    })
}

#[test]
fn expands_the_complete_pinned_corpus_with_exact_counts() {
    let manifest = manifest();
    let expected = ExpansionSummary {
        corpus_files: 7_086,
        corpus_bytes: 4_718_142,
        compiler_sources: 6_537,
        compiler_default_fixtures: 5_982,
        compiler_matrix_fixtures: 555,
        compiler_cases: 7_276,
        compiler_normal_units: 8_592,
        compiler_virtual_configs: 103,
        compiler_present_empty_units: 27,
        compiler_missing_content_units: 1,
        compiler_link_directives: 35,
        compiler_document_symlink_directives: 5,
        compiler_document_symlink_paths: 7,
        project_descriptors: 316,
        project_backing_files: 233,
        project_cases: 632,
        project_declared_inputs: 302,
        project_missing_backing_inputs: 3,
        total_cases: 7_908,
        not_run_cases: 7_908,
    };

    assert_eq!(manifest.summary, expected);
    assert_eq!(validate_manifest(manifest).unwrap(), expected);
    assert_eq!(manifest.sources.len(), 7_086);
    assert_eq!(manifest.compiler_fixtures.len(), 6_537);
    assert_eq!(manifest.project_fixtures.len(), 316);
    assert_eq!(manifest.cases.len(), 7_908);

    let mut encodings = [0_u64; 4];
    for encoding in manifest
        .compiler_fixtures
        .iter()
        .map(|fixture| fixture.encoding)
        .chain(
            manifest
                .project_fixtures
                .iter()
                .map(|fixture| fixture.encoding),
        )
    {
        let index = match encoding {
            SourceEncoding::Utf8 => 0,
            SourceEncoding::Utf8Bom => 1,
            SourceEncoding::Utf16Le => 2,
            SourceEncoding::Utf16Be => 3,
        };
        encodings[index] += 1;
    }
    assert_eq!(encodings, [6_604, 242, 6, 1]);
}

#[test]
fn source_inventory_covers_every_vendored_path_exactly_once() {
    let manifest = manifest();
    let workspace = workspace_root();
    let mut expected = Vec::with_capacity(7_086);

    for (suite, relative_root) in SUITES {
        let root = workspace.join(relative_root);
        let mut paths = Vec::new();
        collect_paths(&root, &root, &mut paths);
        paths.sort_unstable();
        expected.extend(paths.into_iter().map(|path| (suite, path)));
    }

    let actual = manifest
        .sources
        .iter()
        .map(|source| (source.suite, source.path.clone()))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "source inventory must cover every path");
    assert_eq!(
        actual.iter().collect::<BTreeSet<_>>().len(),
        actual.len(),
        "source inventory identities must be unique"
    );

    let per_suite = SUITES.map(|(suite, _)| {
        let entries = manifest
            .sources
            .iter()
            .filter(|source| source.suite == suite)
            .collect::<Vec<_>>();
        (
            entries.len(),
            entries.iter().map(|source| source.bytes).sum::<u64>(),
        )
    });
    assert_eq!(
        per_suite,
        [(6_537, 4_588_680), (316, 100_994), (233, 28_468)]
    );
}

#[test]
fn compiler_expansion_preserves_all_unit_and_variation_distinctions() {
    let manifest = manifest();
    let mut variation_distribution = BTreeMap::new();
    let mut normal_units = 0_usize;
    let mut virtual_configs = 0_usize;
    let mut present_empty = 0_usize;
    let mut missing_sources = Vec::new();
    let mut links = 0_usize;
    let mut document_symlink_directives = 0_usize;
    let mut document_symlink_paths = 0_usize;

    for fixture in &manifest.compiler_fixtures {
        *variation_distribution
            .entry(fixture.configurations.len())
            .or_insert(0_usize) += 1;
        normal_units += fixture.normal_units.len();
        virtual_configs += usize::from(fixture.virtual_config.is_some());
        links += fixture.links.len();

        let source_path = &manifest.sources[fixture.source as usize].path;
        for unit in fixture
            .normal_units
            .iter()
            .chain(fixture.virtual_config.iter())
        {
            document_symlink_directives += unit
                .file_options
                .iter()
                .filter(|setting| setting.name == "symlink")
                .count();
            document_symlink_paths += unit.document_symlinks.len();
            match &unit.content {
                UnitContent::Present { utf8_bytes: 0, .. } => present_empty += 1,
                UnitContent::Present { .. } => {}
                UnitContent::Missing => missing_sources.push(source_path.clone()),
            }
        }
    }

    assert_eq!(
        variation_distribution,
        BTreeMap::from([
            (1, 5_982),
            (2, 510),
            (3, 9),
            (4, 17),
            (5, 2),
            (6, 4),
            (7, 1),
            (10, 10),
            (14, 1),
            (24, 1),
        ])
    );
    assert_eq!(variation_distribution.last_key_value().unwrap().0, &24);
    assert_eq!(normal_units, 8_592);
    assert_eq!(virtual_configs, 103);
    assert_eq!(present_empty, 27);
    assert_eq!(missing_sources, ["augmentExportEquals2.ts"]);
    assert_eq!(links, 35);
    assert_eq!(document_symlink_directives, 5);
    assert_eq!(document_symlink_paths, 7);
}

#[test]
fn project_expansion_keeps_absent_inputs_and_missing_backing_files_distinct() {
    let manifest = manifest();
    let mut absent = 0_usize;
    let mut declared = 0_usize;
    let mut missing = Vec::new();

    for fixture in &manifest.project_fixtures {
        match &fixture.input_files {
            ProjectInputFiles::Absent => absent += 1,
            ProjectInputFiles::Present { inputs } => {
                declared += inputs.len();
                for input in inputs {
                    if matches!(&input.presence, ProjectInputPresence::Missing) {
                        missing.push((
                            manifest.sources[fixture.source as usize].path.clone(),
                            input.path.clone(),
                        ));
                    }
                }
            }
        }
    }

    assert_eq!(absent, 31);
    assert_eq!(declared, 302);
    assert_eq!(
        missing,
        [
            ("invalidRootFile.json".to_owned(), "a".to_owned()),
            ("invalidRootFile.json".to_owned(), "a.t".to_owned()),
            ("invalidRootFile.json".to_owned(), "a.ts".to_owned()),
        ]
    );
}

#[test]
fn case_ids_are_unique_and_cases_follow_the_canonical_upstream_order() {
    let manifest = manifest();
    let ids = manifest
        .cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 7_908, "expanded case IDs must be unique");

    let expected_compiler_sources = manifest
        .sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.suite == SuiteName::Compiler)
        .map(|(index, _)| index as u32)
        .collect::<Vec<_>>();
    assert_eq!(
        manifest
            .compiler_fixtures
            .iter()
            .map(|fixture| fixture.source)
            .collect::<Vec<_>>(),
        expected_compiler_sources
    );

    let expected_project_sources = manifest
        .sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.suite == SuiteName::Project)
        .map(|(index, _)| index as u32)
        .collect::<Vec<_>>();
    assert_eq!(
        manifest
            .project_fixtures
            .iter()
            .map(|fixture| fixture.source)
            .collect::<Vec<_>>(),
        expected_project_sources
    );

    let mut cursor = 0_usize;
    for fixture in &manifest.compiler_fixtures {
        for configuration in 0..fixture.configurations.len() {
            let case = &manifest.cases[cursor];
            let source_path = &manifest.sources[fixture.source as usize].path;
            let expected_id = upstream_case_id(
                SuiteName::Compiler,
                source_path,
                &fixture.configurations[configuration].variant,
            );
            assert_eq!(case.id, expected_id);
            assert_eq!(case.suite, SuiteName::Compiler);
            assert_eq!(case.source, fixture.source);
            assert!(matches!(
                case.initial_execution_state,
                ExecutionState::NotRun
            ));
            assert!(matches!(
                &case.configuration,
                CaseConfiguration::Compiler {
                    configuration: actual
                } if *actual as usize == configuration
            ));
            cursor += 1;
        }
    }
    for fixture in &manifest.project_fixtures {
        for (module, baseline_folder) in [
            (ProjectModule::Commonjs, "node"),
            (ProjectModule::Amd, "amd"),
        ] {
            let case = &manifest.cases[cursor];
            let source_path = &manifest.sources[fixture.source as usize].path;
            let variant = match module {
                ProjectModule::Commonjs => "module=commonjs",
                ProjectModule::Amd => "module=amd",
            };
            assert_eq!(
                case.id,
                upstream_case_id(SuiteName::Project, source_path, variant)
            );
            assert_eq!(case.suite, SuiteName::Project);
            assert_eq!(case.source, fixture.source);
            assert!(matches!(
                case.initial_execution_state,
                ExecutionState::NotRun
            ));
            assert!(matches!(
                &case.configuration,
                CaseConfiguration::Project {
                    module: actual_module,
                    baseline_folder: actual_folder,
                } if *actual_module == module && actual_folder == baseline_folder
            ));
            cursor += 1;
        }
    }
    assert_eq!(cursor, manifest.cases.len());
}

#[test]
fn rendering_is_canonical_round_trippable_and_contains_no_result_claims() {
    let manifest = manifest();
    let first = render_manifest(manifest).expect("manifest must render");
    let second = render_manifest(manifest).expect("manifest must render deterministically");
    assert_eq!(first, second);
    assert_eq!(first.last(), Some(&b'\n'));
    assert!(!first.ends_with(b"\n\n"));

    let reparsed: ExpansionManifest =
        serde_json::from_slice(&first).expect("rendered manifest must deserialize");
    assert_eq!(&reparsed, manifest);
    assert_eq!(validate_manifest(&reparsed).unwrap(), manifest.summary);

    let json: Value = serde_json::from_slice(&first).expect("rendered manifest must be JSON");
    assert_no_result_or_skip_keys(&json);
}

#[test]
fn validator_rejects_a_coordinated_corpus_identity_rewrite() {
    let mut tampered = manifest().clone();
    let compiler = tampered
        .corpus_pin
        .suites
        .iter_mut()
        .find(|suite| suite.name == SuiteName::Compiler)
        .expect("compiler suite identity must be recorded");
    compiler.git_tree_sha1 = "0000000000000000000000000000000000000000".to_owned();

    assert!(
        validate_manifest(&tampered).is_err(),
        "validation must anchor suite identities instead of trusting self-consistent manifest data"
    );
}

#[test]
fn validator_rejects_a_coordinated_raw_source_hash_rewrite() {
    let mut tampered = manifest().clone();
    tampered.sources[0].sha256 =
        "0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    tampered.source_inventory_sha256 = source_inventory_sha256(&tampered);

    assert!(
        validate_manifest(&tampered).is_err(),
        "validation must anchor raw source hashes instead of accepting a rewritten aggregate"
    );
}

#[test]
fn recorded_manifest_is_the_exact_current_expansion() {
    let workspace = workspace_root();
    assert!(
        workspace.join(MANIFEST_RELATIVE_PATH).is_file(),
        "the fixed expansion artifact must be recorded at {MANIFEST_RELATIVE_PATH}"
    );
    assert_eq!(
        check_recorded_manifest(&workspace).unwrap(),
        manifest().summary
    );
}

fn collect_paths(root: &Path, directory: &Path, paths: &mut Vec<String>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| panic!("failed to enumerate {}: {error}", directory.display()));
    assert!(
        !entries.is_empty(),
        "vendored suite contains an empty directory {}",
        directory.display()
    );

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("failed to inspect {}: {error}", path.display()));
        assert!(
            !file_type.is_symlink(),
            "vendored suite contains a symlink {}",
            path.display()
        );
        if file_type.is_dir() {
            collect_paths(root, &path, paths);
        } else {
            assert!(
                file_type.is_file(),
                "vendored suite contains an unsupported entry {}",
                path.display()
            );
            let relative = path
                .strip_prefix(root)
                .expect("visited path must remain below its suite root");
            let relative = relative
                .components()
                .map(|component| {
                    component
                        .as_os_str()
                        .to_str()
                        .expect("vendored paths must be UTF-8")
                })
                .collect::<Vec<_>>()
                .join("/");
            paths.push(relative);
        }
    }
}

fn assert_no_result_or_skip_keys(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                assert!(
                    !matches!(
                        key.as_str(),
                        "result"
                            | "results"
                            | "skip"
                            | "skipped"
                            | "exclusion"
                            | "exclusions"
                            | "excluded"
                            | "pending"
                            | "pass"
                            | "passed"
                    ),
                    "inventory-only manifest must not contain a {key:?} field"
                );
                assert_no_result_or_skip_keys(child);
            }
        }
        Value::Array(array) => {
            for child in array {
                assert_no_result_or_skip_keys(child);
            }
        }
        _ => {}
    }
}

fn upstream_case_id(suite: SuiteName, source_path: &str, variant: &str) -> String {
    let suite = match suite {
        SuiteName::Compiler => "compiler",
        SuiteName::Project => "project",
        SuiteName::Projects => "projects",
    };
    format!(
        "typescript-6.0.3/{suite}/{}#{}",
        percent_encode(source_path, true),
        percent_encode(variant, false)
    )
}

fn percent_encode(value: &str, preserve_slash: bool) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~')
            || preserve_slash && byte == b'/'
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

fn source_inventory_sha256(manifest: &ExpansionManifest) -> String {
    let mut digest = Sha256::new();
    for source in &manifest.sources {
        for bytes in [
            source.suite.as_str().as_bytes(),
            source.path.as_bytes(),
            source.mode.as_bytes(),
        ] {
            digest.update((bytes.len() as u64).to_be_bytes());
            digest.update(bytes);
        }
        digest.update(source.bytes.to_be_bytes());
        for bytes in [source.sha256.as_bytes(), source.git_blob_sha1.as_bytes()] {
            digest.update((bytes.len() as u64).to_be_bytes());
            digest.update(bytes);
        }
    }
    format!("{:x}", digest.finalize())
}
