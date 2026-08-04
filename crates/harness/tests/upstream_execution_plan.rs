use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

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
fn compiler_root_policies_preserve_occurrences_and_fail_closed_for_configs() {
    let mut all_units = 0_usize;
    let mut last_unit = 0_usize;
    let mut config_driven = 0_usize;
    let mut all_roots = 0_usize;
    let mut last_others = 0_usize;
    let mut config_candidates = 0_usize;

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
            CompilerRootSelection::ConfigDriven {
                config_host_units,
                candidate_units,
                ..
            } => {
                config_driven += 1;
                config_candidates += candidate_units.len();
                assert_eq!(
                    config_host_units.len(),
                    candidate_units.len() + 1,
                    "the config parse host must retain the config occurrence"
                );
            }
        }
    }
    assert_eq!((all_units, last_unit, config_driven), (6_765, 405, 106));
    assert_eq!(all_roots, 8_576);
    assert_eq!(last_others, 506);
    assert_eq!(config_candidates, 306);

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
        CompilerRootSelection::ConfigDriven {
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
            candidate_units: Arc::from([
                CompilerUnitId(0),
                CompilerUnitId(1),
                CompilerUnitId(2),
                CompilerUnitId(3),
                CompilerUnitId(4),
                CompilerUnitId(6),
            ]),
        }
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
