use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use sha2::{Digest, Sha256};
use tsc_harness::upstream_suites::execution::{
    load_recorded_execution_plans, CompilerExecutionPlan, CompilerExplicitRootReason,
    CompilerRootSelection, CompilerSymlinkPhase, CompilerUnitId, ProjectExecutionPlan,
    ProjectRootSelection, UpstreamExecutionCorpus, UpstreamExecutionInput, UpstreamExecutionPlan,
};
use tsc_harness::upstream_suites::{ExecutionState, ProjectModule};

static CORPUS: OnceLock<UpstreamExecutionCorpus> = OnceLock::new();

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus() -> &'static UpstreamExecutionCorpus {
    CORPUS.get_or_init(|| {
        load_recorded_execution_plans(&workspace_root())
            .unwrap_or_else(|error| panic!("failed to load upstream execution plans: {error}"))
    })
}

fn plan(case_id: &str) -> &'static UpstreamExecutionPlan {
    corpus()
        .plans
        .iter()
        .find(|plan| plan.provenance.case_id.as_ref() == case_id)
        .unwrap_or_else(|| panic!("missing execution plan {case_id}"))
}

fn compiler(plan: &UpstreamExecutionPlan) -> &CompilerExecutionPlan {
    match &plan.input {
        UpstreamExecutionInput::Compiler(plan) => plan,
        UpstreamExecutionInput::Project(_) => panic!("expected a compiler plan"),
    }
}

fn json_u32s(value: &serde_json::Value, field: &str) -> Vec<CompilerUnitId> {
    value[field]
        .as_array()
        .unwrap_or_else(|| panic!("oracle field {field} is not an array"))
        .iter()
        .map(|value| {
            CompilerUnitId(
                u32::try_from(
                    value
                        .as_u64()
                        .unwrap_or_else(|| panic!("oracle field {field} contains a non-u64")),
                )
                .expect("oracle unit id fits u32"),
            )
        })
        .collect()
}

fn json_values_equivalent(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    match (left, right) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            left.as_f64() == right.as_f64()
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| json_values_equivalent(left, right))
        }
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left)| {
                    right
                        .get(key)
                        .is_some_and(|right| json_values_equivalent(left, right))
                })
        }
        _ => left == right,
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn project(plan: &UpstreamExecutionPlan) -> &ProjectExecutionPlan {
    match &plan.input {
        UpstreamExecutionInput::Project(plan) => plan,
        UpstreamExecutionInput::Compiler(_) => panic!("expected a project plan"),
    }
}

#[test]
fn plans_cover_the_recorded_order_and_decode_each_blob_only_once() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<UpstreamExecutionCorpus>();
    assert_send_sync::<UpstreamExecutionPlan>();

    let corpus = corpus();
    assert_eq!(corpus.plans.len(), 7_908);
    assert_eq!(corpus.manifest.cases.len(), corpus.plans.len());
    for (index, (recorded, plan)) in corpus
        .manifest
        .cases
        .iter()
        .zip(corpus.plans.iter())
        .enumerate()
    {
        assert_eq!(plan.provenance.case_index, index as u32);
        assert_eq!(plan.provenance.case_id.as_ref(), recorded.id);
        assert_eq!(plan.provenance.source_index, recorded.source);
        assert_eq!(
            plan.provenance.initial_execution_state,
            ExecutionState::NotRun
        );
    }
    assert_eq!(
        corpus.plans.first().unwrap().provenance.case_id.as_ref(),
        "typescript-6.0.3/compiler/2dArrays.ts#default"
    );
    assert_eq!(
        corpus.plans[7_275].provenance.case_id.as_ref(),
        "typescript-6.0.3/compiler/yieldStringLiteral.ts#default"
    );
    assert_eq!(
        corpus.plans[7_276].provenance.case_id.as_ref(),
        "typescript-6.0.3/project/baseline.json#module%3Dcommonjs"
    );
    assert_eq!(
        corpus.plans.last().unwrap().provenance.case_id.as_ref(),
        "typescript-6.0.3/project/visibilityOfTypeUsedAcrossModules2.json#module%3Damd"
    );

    let stats = corpus.cache_stats;
    assert_eq!(stats.verified_source_paths, 7_086);
    assert_eq!(stats.verified_source_bytes, 4_718_142);
    assert_eq!(stats.unique_raw_blobs, 6_993);
    assert_eq!(stats.reused_raw_blobs, 93);
    assert_eq!(stats.decode_requests, 6_853);
    assert_eq!(stats.unique_decoded_blobs, 6_839);
    assert_eq!(stats.reused_decoded_blobs, 14);

    let default = compiler(plan("typescript-6.0.3/compiler/2dArrays.ts#default"));
    assert!(Arc::ptr_eq(
        &default.fixture.settings,
        &default.effective_settings
    ));
}

#[test]
fn compiler_root_policies_preserve_occurrences_and_resolve_all_configs() {
    let mut all_units = 0_usize;
    let mut last_unit = 0_usize;
    let mut config_driven = 0_usize;
    let mut all_roots = 0_usize;
    let mut last_others = 0_usize;
    let mut config_candidates = 0_usize;
    let mut config_roots = 0_usize;
    let mut config_others = 0_usize;

    for plan in corpus().plans.iter() {
        let UpstreamExecutionInput::Compiler(plan) = &plan.input else {
            continue;
        };
        match &plan.root_selection {
            CompilerRootSelection::Explicit {
                reason: CompilerExplicitRootReason::AllUnits,
                root_units,
                other_units,
                vfs_write_order,
                program_root_units,
            } => {
                all_units += 1;
                all_roots += root_units.len();
                assert!(other_units.is_empty());
                assert_eq!(vfs_write_order, root_units);
                assert!(program_root_units.len() <= root_units.len());
            }
            CompilerRootSelection::Explicit {
                reason: CompilerExplicitRootReason::LastUnitImplicitReferences,
                root_units,
                other_units,
                vfs_write_order,
                program_root_units,
            } => {
                last_unit += 1;
                assert_eq!(root_units.len(), 1);
                last_others += other_units.len();
                assert_eq!(vfs_write_order.len(), root_units.len() + other_units.len());
                assert_eq!(program_root_units.len(), root_units.len());
            }
            CompilerRootSelection::Config {
                config_host_units,
                root_units,
                other_units,
                vfs_write_order,
                program_root_units,
                ..
            } => {
                config_driven += 1;
                config_candidates += root_units.len() + other_units.len();
                config_roots += root_units.len();
                config_others += other_units.len();
                assert_eq!(
                    config_host_units.len(),
                    root_units.len() + other_units.len() + 1,
                    "the config parse host must retain the config occurrence"
                );
                assert_eq!(vfs_write_order.len(), root_units.len() + other_units.len());
                assert!(program_root_units.len() <= root_units.len());
            }
        }
    }
    assert_eq!((all_units, last_unit, config_driven), (6_765, 405, 106));
    assert_eq!(all_roots, 8_576);
    assert_eq!(last_others, 506);
    assert_eq!(config_candidates, 306);
    assert_eq!(config_roots, 170);
    assert_eq!(config_others, 136);

    let duplicate = compiler(plan(
        "typescript-6.0.3/compiler/augmentExportEquals2.ts#default",
    ));
    assert_eq!(
        duplicate
            .fixture
            .units
            .iter()
            .map(|unit| (unit.id, unit.name.as_ref(), unit.content.is_some()))
            .collect::<Vec<_>>(),
        [
            (CompilerUnitId(0), "file1.ts", true),
            (CompilerUnitId(1), "file2.ts", true),
            (CompilerUnitId(2), "file3.ts", false),
            (CompilerUnitId(3), "file3.ts", true),
        ]
    );
    assert_eq!(
        duplicate.root_selection,
        CompilerRootSelection::Explicit {
            reason: CompilerExplicitRootReason::LastUnitImplicitReferences,
            root_units: Arc::from([CompilerUnitId(3)]),
            other_units: Arc::from([CompilerUnitId(0), CompilerUnitId(1)]),
            vfs_write_order: Arc::from([CompilerUnitId(3), CompilerUnitId(0), CompilerUnitId(1),]),
            program_root_units: Arc::from([CompilerUnitId(3)]),
        }
    );

    let json = compiler(plan(
        "typescript-6.0.3/compiler/isolatedModules_resolveJsonModule.ts#default",
    ));
    let CompilerRootSelection::Explicit {
        root_units,
        vfs_write_order,
        program_root_units,
        ..
    } = &json.root_selection
    else {
        panic!("JSON canary must have explicit roots");
    };
    assert_eq!(root_units.as_ref(), [CompilerUnitId(0), CompilerUnitId(1)]);
    assert_eq!(
        vfs_write_order.as_ref(),
        [CompilerUnitId(0), CompilerUnitId(1)]
    );
    assert_eq!(program_root_units.as_ref(), [CompilerUnitId(0)]);

    let config = compiler(plan(
        "typescript-6.0.3/compiler/allowJsCrossMonorepoPackage.ts#default",
    ));
    assert_eq!(
        config.root_selection,
        CompilerRootSelection::Config {
            config_unit: CompilerUnitId(5),
            config_host_units: Arc::from([
                CompilerUnitId(0),
                CompilerUnitId(1),
                CompilerUnitId(2),
                CompilerUnitId(3),
                CompilerUnitId(4),
                CompilerUnitId(5),
                CompilerUnitId(6),
            ]),
            root_units: Arc::from([CompilerUnitId(6)]),
            other_units: Arc::from([
                CompilerUnitId(0),
                CompilerUnitId(1),
                CompilerUnitId(2),
                CompilerUnitId(3),
                CompilerUnitId(4),
            ]),
            vfs_write_order: Arc::from([
                CompilerUnitId(6),
                CompilerUnitId(0),
                CompilerUnitId(1),
                CompilerUnitId(2),
                CompilerUnitId(3),
                CompilerUnitId(4),
            ]),
            program_root_units: Arc::from([CompilerUnitId(6)]),
        }
    );
    let config_plan = config
        .fixture
        .config_root_plan
        .as_ref()
        .expect("config fixture owns a parsed root plan");
    assert_eq!(config_plan.file_names(), ["/packages/main/index.ts"]);
}

#[test]
fn compiler_config_root_plans_match_the_frozen_typescript_oracle() {
    let workspace = workspace_root();
    let bytes = fs::read(workspace.join("vendor/typescript-6.0.3/compiler-config-plans.v1.json"))
        .expect("read compiler config oracle");
    let oracle: serde_json::Value =
        serde_json::from_slice(&bytes).expect("compiler config oracle is JSON");
    assert_eq!(oracle["schema"], 1);
    assert_eq!(oracle["typescript_version"], "6.0.3");
    assert_eq!(
        oracle["source_commit"],
        "050880ce59e30b356b686bd3144efe24f875ebc8"
    );
    assert_eq!(oracle["node_version"], "25.2.1");
    assert_eq!(
        oracle["producer"]["path"],
        "vendor/typescript-6.0.3/lib/typescript.js"
    );
    assert_eq!(
        oracle["producer"]["sha256"],
        "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39"
    );
    assert_eq!(
        oracle["manifest"]["path"],
        "vendor/typescript-6.0.3/test-suite-expansion.v1.json"
    );
    assert_eq!(
        oracle["manifest"]["sha256"],
        "9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188"
    );
    for metadata in ["producer", "manifest"] {
        let relative_path = oracle[metadata]["path"]
            .as_str()
            .unwrap_or_else(|| panic!("oracle {metadata} path is a string"));
        let recorded_hash = oracle[metadata]["sha256"]
            .as_str()
            .unwrap_or_else(|| panic!("oracle {metadata} hash is a string"));
        assert_eq!(
            sha256(
                &fs::read(workspace.join(relative_path))
                    .unwrap_or_else(|error| panic!("read oracle {metadata} input: {error}"))
            ),
            recorded_hash,
            "oracle {metadata} input drifted"
        );
    }
    assert_eq!(oracle["summary"]["config_plans"]["fixture_total"], 103);
    assert_eq!(oracle["summary"]["config_plans"]["case_total"], 106);
    assert_eq!(oracle["summary"]["extended_sources"]["fixture_total"], 5);
    assert_eq!(oracle["summary"]["extended_sources"]["case_total"], 5);

    let mut by_source = BTreeMap::new();
    let mut configuration_counts = BTreeMap::<u32, usize>::new();
    for plan in corpus().plans.iter() {
        let UpstreamExecutionInput::Compiler(plan) = &plan.input else {
            continue;
        };
        if plan.fixture.config_unit.is_some() {
            by_source.entry(plan.fixture.source.index).or_insert(plan);
            *configuration_counts
                .entry(plan.fixture.source.index)
                .or_default() += 1;
        }
    }
    assert_eq!(by_source.len(), 103);

    let fixtures = oracle["fixtures"]
        .as_array()
        .expect("oracle fixtures is an array");
    assert_eq!(fixtures.len(), 103);
    let mut roots = 0;
    let mut others = 0;
    let mut candidates = 0;
    let mut parsed_file_names = 0;
    let mut extended_sources = 0;
    let mut weighted_candidates = 0;
    let mut weighted_file_names = 0;
    let mut weighted_extended_sources = 0;
    let mut weighted_roots = 0;
    let mut weighted_others = 0;
    let mut seen_sources = BTreeSet::new();
    let mut previous_source = None;
    for expected in fixtures {
        let source = u32::try_from(
            expected["source"]["index"]
                .as_u64()
                .expect("oracle source index is u64"),
        )
        .expect("oracle source index fits u32");
        assert!(
            seen_sources.insert(source),
            "duplicate oracle source {source}"
        );
        assert!(
            previous_source.is_none_or(|previous| previous < source),
            "oracle fixtures are not in canonical source order"
        );
        previous_source = Some(source);
        let plan = by_source
            .get(&source)
            .unwrap_or_else(|| panic!("missing config fixture source {source}"));
        assert_eq!(
            plan.fixture.source.relative_path.as_ref(),
            expected["source"]["path"]
                .as_str()
                .expect("oracle source path is a string")
        );
        assert_eq!(
            configuration_counts[&source],
            expected["configuration_count"]
                .as_u64()
                .expect("configuration count is u64") as usize
        );
        let config_unit = CompilerUnitId(
            u32::try_from(
                expected["config_unit"]["id"]
                    .as_u64()
                    .expect("config id is u64"),
            )
            .expect("config id fits u32"),
        );
        assert_eq!(plan.fixture.config_unit, Some(config_unit));
        assert_eq!(
            plan.fixture.units[config_unit.0 as usize].name.as_ref(),
            expected["config_unit"]["name"]
                .as_str()
                .expect("config name is string")
        );
        let config_plan = plan
            .fixture
            .config_root_plan
            .as_ref()
            .expect("config fixture owns a root plan");
        assert!(
            json_values_equivalent(config_plan.raw(), &expected["raw_config"]),
            "raw config drifted for source {source}: Rust={:?} oracle={:?}",
            config_plan.raw(),
            expected["raw_config"]
        );
        assert_eq!(
            config_plan.file_names(),
            expected["parsed_file_names"]
                .as_array()
                .expect("parsed file names is an array")
                .iter()
                .map(|value| value.as_str().expect("parsed file name is a string"))
                .collect::<Vec<_>>(),
            "parsed file-name order drifted for source {source}"
        );
        let expected_extended_sources = expected["extended_sources"]
            .as_array()
            .expect("extended sources is an array");
        assert_eq!(
            config_plan.extended_sources().len(),
            expected_extended_sources.len(),
            "extended source count drifted for source {source}"
        );
        for (actual, expected_extended) in config_plan
            .extended_sources()
            .iter()
            .zip(expected_extended_sources)
        {
            let unit_id = usize::try_from(
                expected_extended["unit_id"]
                    .as_u64()
                    .expect("extended source unit id is u64"),
            )
            .expect("extended source unit id fits usize");
            let unit = &plan.fixture.units[unit_id];
            assert_eq!(
                actual.file_name,
                expected_extended["file_name"]
                    .as_str()
                    .expect("extended source file name is a string")
            );
            assert_eq!(actual.file_name, unit.name.as_ref());
            assert_eq!(actual.text.as_str(), unit.content.as_deref().unwrap_or(""));
            assert_eq!(
                actual.text.len() as u64,
                expected_extended["content"]["utf8_bytes"]
                    .as_u64()
                    .expect("extended source byte count is u64")
            );
            assert_eq!(
                sha256(actual.text.as_bytes()),
                expected_extended["content"]["sha256"]
                    .as_str()
                    .expect("extended source hash is a string")
            );
        }
        let discovery = config_plan.discovery_options();
        let expected_discovery = &expected["discovery_options"];
        assert_eq!(
            discovery.allow_js(),
            expected_discovery["allow_js"]
                .as_bool()
                .expect("allow_js is boolean")
        );
        assert_eq!(
            discovery.resolve_json_module(),
            expected_discovery["resolve_json_module"]
                .as_bool()
                .expect("resolve_json_module is boolean")
        );
        assert_eq!(discovery.out_dir(), expected_discovery["out_dir"].as_str());
        assert_eq!(
            discovery.declaration_dir(),
            expected_discovery["declaration_dir"].as_str()
        );
        assert!(
            expected["diagnostics"]
                .as_array()
                .expect("diagnostics is an array")
                .is_empty(),
            "the fixed compiler config corpus has no parse diagnostics"
        );

        let CompilerRootSelection::Config {
            config_host_units,
            root_units,
            other_units,
            vfs_write_order,
            program_root_units,
            ..
        } = &plan.root_selection
        else {
            panic!("source {source} did not resolve through config roots");
        };
        assert_eq!(
            config_host_units.as_ref(),
            plan.fixture
                .units
                .iter()
                .map(|unit| unit.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(root_units.as_ref(), json_u32s(expected, "root_unit_ids"));
        assert_eq!(other_units.as_ref(), json_u32s(expected, "other_unit_ids"));
        assert_eq!(
            program_root_units.as_ref(),
            json_u32s(expected, "program_root_unit_ids")
        );
        assert_eq!(
            vfs_write_order.as_ref(),
            root_units
                .iter()
                .chain(other_units.iter())
                .copied()
                .collect::<Vec<_>>()
        );
        let expected_candidates = expected["candidate_units"]
            .as_array()
            .expect("candidate units is an array")
            .iter()
            .map(|unit| {
                (
                    CompilerUnitId(
                        u32::try_from(unit["id"].as_u64().expect("candidate id is u64"))
                            .expect("candidate id fits u32"),
                    ),
                    unit["name"].as_str().expect("candidate name is string"),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            plan.fixture
                .units
                .iter()
                .filter(|unit| unit.id != config_unit)
                .map(|unit| (unit.id, unit.name.as_ref()))
                .collect::<Vec<_>>(),
            expected_candidates
        );
        roots += root_units.len();
        others += other_units.len();
        let case_count = configuration_counts[&source];
        candidates += expected_candidates.len();
        parsed_file_names += config_plan.file_names().len();
        extended_sources += config_plan.extended_sources().len();
        weighted_candidates += expected_candidates.len() * case_count;
        weighted_file_names += config_plan.file_names().len() * case_count;
        weighted_extended_sources += config_plan.extended_sources().len() * case_count;
        weighted_roots += root_units.len() * case_count;
        weighted_others += other_units.len() * case_count;
    }
    assert_eq!((roots, others), (167, 133));
    assert_eq!(
        (candidates, parsed_file_names, extended_sources),
        (300, 167, 5)
    );
    assert_eq!(
        (
            weighted_candidates,
            weighted_file_names,
            weighted_extended_sources,
            weighted_roots,
            weighted_others,
        ),
        (306, 170, 5, 170, 136)
    );
    assert_eq!(
        seen_sources,
        by_source.keys().copied().collect::<BTreeSet<_>>(),
        "oracle config fixture membership drifted"
    );
}

#[test]
fn compiler_symlink_phases_and_fixture_sharing_remain_observable() {
    let mut global_plans = 0_usize;
    let mut global_operations = 0_usize;
    let mut document_plans = 0_usize;
    let mut document_operations = 0_usize;
    for plan in corpus().plans.iter() {
        let UpstreamExecutionInput::Compiler(plan) = &plan.input else {
            continue;
        };
        if !plan.fixture.global_symlinks.is_empty() {
            global_plans += 1;
            global_operations += plan.fixture.global_symlink_directives.len();
            assert_eq!(
                plan.fixture.global_symlinks.len(),
                plan.fixture.global_symlink_directives.len(),
                "the pinned corpus has no duplicate normalized global link key"
            );
        }
        let documents = plan
            .fixture
            .units
            .iter()
            .map(|unit| unit.document_symlinks.len())
            .sum::<usize>();
        if documents != 0 {
            document_plans += 1;
            document_operations += documents;
        }
    }
    assert_eq!((global_plans, global_operations), (18, 37));
    assert_eq!((document_plans, document_operations), (5, 7));

    let document = compiler(plan(
        "typescript-6.0.3/compiler/moduleResolutionWithSymlinks_preserveSymlinks.ts#default",
    ));
    assert!(document.fixture.global_symlinks.is_empty());
    let links = &document.fixture.units[0].document_symlinks;
    assert_eq!(links.len(), 2);
    assert!(links
        .iter()
        .all(|operation| operation.phase == CompilerSymlinkPhase::Document));
    assert!(links
        .iter()
        .all(|operation| operation.anchor.as_ref() == "/.src"));
    assert!(links
        .iter()
        .all(|operation| operation.normalized_target.as_ref() == "/linked/index.d.ts"));
    assert_eq!(
        links
            .iter()
            .map(|operation| operation.normalized_link_path.as_ref())
            .collect::<Vec<_>>(),
        [
            "/app/node_modules/linked/index.d.ts",
            "/app/node_modules/linked2/index.d.ts",
        ]
    );

    let global_variants = corpus()
        .plans
        .iter()
        .filter(|plan| plan.provenance.source_path.as_ref() == "declarationEmitSymlinkPaths.ts")
        .map(compiler)
        .collect::<Vec<_>>();
    assert_eq!(global_variants.len(), 2);
    assert!(Arc::ptr_eq(
        &global_variants[0].fixture,
        &global_variants[1].fixture
    ));
    assert_eq!(global_variants[0].fixture.global_symlinks.len(), 2);
    assert!(global_variants[0]
        .fixture
        .global_symlinks
        .iter()
        .all(|operation| operation.phase == CompilerSymlinkPhase::Global
            && operation.anchor.as_ref() == "/.src"));

    let relative = compiler(plan(
        "typescript-6.0.3/compiler/declarationEmitPathMappingMonorepo.ts#default",
    ));
    assert_eq!(
        relative
            .fixture
            .global_symlinks
            .iter()
            .map(|operation| {
                (
                    operation.normalized_target.as_ref(),
                    operation.normalized_link_path.as_ref(),
                )
            })
            .collect::<Vec<_>>(),
        [
            ("/.src/packages/a", "/.src/node_modules/@ts-bug/a"),
            ("/.src/packages/b", "/.src/node_modules/@ts-bug/b"),
        ]
    );

    let config_variants = corpus()
        .plans
        .iter()
        .filter(|plan| plan.provenance.source_path.as_ref() == "sideEffectImports2.ts")
        .map(compiler)
        .collect::<Vec<_>>();
    assert_eq!(config_variants.len(), 4);
    assert!(config_variants
        .windows(2)
        .all(|pair| Arc::ptr_eq(&pair[0].fixture, &pair[1].fixture)));

    let mut source_by_blob: BTreeMap<&str, Vec<&CompilerExecutionPlan>> = BTreeMap::new();
    for plan in corpus().plans.iter() {
        let UpstreamExecutionInput::Compiler(plan) = &plan.input else {
            continue;
        };
        source_by_blob
            .entry(plan.fixture.source.git_blob_sha1.as_ref())
            .or_default()
            .push(plan);
    }
    let shared_blob = source_by_blob
        .values()
        .find(|plans| {
            plans.iter().any(|left| {
                plans.iter().any(|right| {
                    left.fixture.source.index != right.fixture.source.index
                        && Arc::ptr_eq(&left.fixture.source.raw, &right.fixture.source.raw)
                        && Arc::ptr_eq(&left.fixture.source.decoded, &right.fixture.source.decoded)
                })
            })
        })
        .expect("at least one distinct compiler path must share its pinned blob");
    assert!(shared_blob.len() >= 2);
}

#[test]
fn project_plans_preserve_descriptor_order_mount_and_pending_config_modes() {
    let mut explicit = 0_usize;
    let mut project_config = 0_usize;
    let mut discover = 0_usize;
    for plan in corpus().plans.iter() {
        let UpstreamExecutionInput::Project(plan) = &plan.input else {
            continue;
        };
        match &plan.fixture.root_selection {
            ProjectRootSelection::Explicit { input_names } => {
                explicit += 1;
                assert!(!input_names.is_empty());
            }
            ProjectRootSelection::ProjectConfig { .. } => {
                project_config += 1;
            }
            ProjectRootSelection::DiscoverConfig => {
                discover += 1;
            }
        }
    }
    assert_eq!((explicit, project_config, discover), (570, 32, 30));

    let commonjs = project(plan(
        "typescript-6.0.3/project/baseline.json#module%3Dcommonjs",
    ));
    let amd = project(plan("typescript-6.0.3/project/baseline.json#module%3Damd"));
    assert!(Arc::ptr_eq(&commonjs.fixture, &amd.fixture));
    assert!(Arc::ptr_eq(&commonjs.fixture.mount, &amd.fixture.mount));
    assert_eq!(commonjs.module_variant, ProjectModule::Commonjs);
    assert_eq!(commonjs.baseline_folder.as_ref(), "node");
    assert_eq!(amd.module_variant, ProjectModule::Amd);
    assert_eq!(amd.baseline_folder.as_ref(), "amd");
    assert_eq!(
        commonjs
            .fixture
            .properties
            .iter()
            .map(|property| property.name.as_ref())
            .collect::<Vec<_>>(),
        [
            "scenario",
            "projectRoot",
            "inputFiles",
            "baselineCheck",
            "runTest",
        ]
    );
    let ProjectRootSelection::Explicit { input_names } = &commonjs.fixture.root_selection else {
        panic!("baseline project must retain its explicit roots");
    };
    assert_eq!(input_names.as_ref(), [Arc::<str>::from("emit.ts")]);
    assert_eq!(
        commonjs.fixture.current_directory.as_ref(),
        "/.src/tests/cases/projects/baseline"
    );
    assert_eq!(commonjs.fixture.mount.virtual_path.as_ref(), "/.src/tests");
    assert_eq!(
        commonjs.fixture.mount.workspace_path.as_ref(),
        &workspace_root().join("ts-tests/tests")
    );
    assert!(commonjs.fixture.mount.case_sensitive);
    assert!(commonjs.fixture.mount.read_only);

    let configured = project(plan(
        "typescript-6.0.3/project/jsFileCompilationDifferentNamesNotSpecified.json#module%3Dcommonjs",
    ));
    assert_eq!(
        configured.fixture.root_selection,
        ProjectRootSelection::ProjectConfig {
            raw_project: Arc::from("DifferentNamesNotSpecified"),
            config_file_name: Arc::from("DifferentNamesNotSpecified/tsconfig.json"),
            resolved_config_path: Arc::from(
                "/.src/tests/cases/projects/jsFileCompilation/DifferentNamesNotSpecified/tsconfig.json"
            ),
        }
    );

    let discovered = project(plan(
        "typescript-6.0.3/project/defaultExcludeNodeModulesAndOutDir.json#module%3Dcommonjs",
    ));
    assert_eq!(
        discovered.fixture.root_selection,
        ProjectRootSelection::DiscoverConfig
    );
}
