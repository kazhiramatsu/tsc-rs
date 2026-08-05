use std::collections::BTreeSet;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::time::Instant;

use serde_json::{json, Value};
use tsc_compiler::ProgramSession;
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_harness::upstream_suites::execution::{
    load_compiler_no_emit, load_node_modules_search_project, load_recorded_execution_plans,
    CompilerExecutionPlan, UpstreamExecutionInput, UpstreamExecutionPlan,
};
use tsc_program::ProgramLoadLimits;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn plan<'a>(plans: &'a [UpstreamExecutionPlan], case_id: &str) -> &'a CompilerExecutionPlan {
    let plan = plans
        .iter()
        .find(|plan| plan.provenance.case_id.as_ref() == case_id)
        .unwrap_or_else(|| panic!("missing upstream compiler case {case_id}"));
    match &plan.input {
        UpstreamExecutionInput::Compiler(plan) => plan,
        UpstreamExecutionInput::Project(_) => panic!("expected compiler case {case_id}"),
    }
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

#[test]
fn compiler_session_runs_recorded_no_emit_programs() {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace)
        .unwrap_or_else(|error| panic!("failed to load upstream plans: {error}"));
    for case_id in [
        "typescript-6.0.3/compiler/2dArrays.ts#default",
        "typescript-6.0.3/compiler/augmentExportEquals2.ts#default",
        "typescript-6.0.3/compiler/typeReferenceDirectives3.ts#default",
        "typescript-6.0.3/compiler/configFileExtendsAsList.ts#default",
        "typescript-6.0.3/compiler/moduleResolutionWithSymlinks_preserveSymlinks.ts#default",
        "typescript-6.0.3/compiler/arrayIterationLibES5TargetDifferent.ts#nolib%3Dtrue%2Ctarget%3Des5",
        "typescript-6.0.3/compiler/declarationEmitForGlobalishSpecifierSymlink.ts#default",
        "typescript-6.0.3/compiler/binderBinaryExpressionStress.ts#default",
        "typescript-6.0.3/compiler/binderBinaryExpressionStressJs.ts#default",
        "typescript-6.0.3/compiler/moduleResolutionPackageIdWithRelativeAndAbsolutePath.ts#default",
        "typescript-6.0.3/compiler/pathMappingBasedModuleResolution_rootImport_aliasWithRoot_differentRootTypes.ts#default",
        "typescript-6.0.3/compiler/bindingPatternCannotBeOnlyInferenceSource.ts#default",
        "typescript-6.0.3/compiler/contextuallyTypedSymbolNamedProperties.ts#default",
        "typescript-6.0.3/compiler/largeTupleTypes.ts#default",
        "typescript-6.0.3/compiler/mismatchedExplicitTypeParameterAndArgumentType.ts#default",
        "typescript-6.0.3/compiler/overloadresolutionWithConstraintCheckingDeferred.ts#default",
        "typescript-6.0.3/compiler/unspecializedConstraints.ts#default",
        "typescript-6.0.3/compiler/unterminatedRegexAtEndOfSource1.ts#default",
        "typescript-6.0.3/compiler/parseAssertEntriesError.ts#default",
        "typescript-6.0.3/compiler/parseImportAttributesError.ts#default",
    ] {
        let prepared = load_compiler_no_emit(&workspace, plan(&corpus.plans, case_id), limits())
            .unwrap_or_else(|error| panic!("failed to load {case_id}: {error}"));
        let outcome = ProgramSession::new(prepared)
            .run()
            .unwrap_or_else(|error| panic!("failed to execute {case_id}: {error:?}"));
        assert!(outcome.config_diagnostics().is_empty(), "{case_id}");
        if case_id.ends_with("/largeTupleTypes.ts#default") {
            assert_eq!(
                outcome.diagnostics().count(),
                0,
                "{case_id}: the official regression is diagnostic-free"
            );
        }
    }
}

fn message_record(message: &MessageChain) -> Value {
    json!({
        "code": message.code,
        "category": message.category.name(),
        "text": message.text,
        "next": message.next_present.then(|| {
            message.next.iter().map(message_record).collect::<Vec<_>>()
        }),
    })
}

fn diagnostic_record(diagnostic: &Diagnostic) -> Value {
    json!({
        "file": diagnostic.file_name,
        "start": diagnostic.start,
        "length": diagnostic.length,
        "code": diagnostic.code(),
        "category": diagnostic.category().name(),
        "message": message_record(&diagnostic.message),
    })
}

fn string_field<'a>(value: &'a Value, field: &str, case_id: &str) -> &'a str {
    value[field]
        .as_str()
        .unwrap_or_else(|| panic!("{case_id}: oracle {field} is a string"))
}

#[test]
fn compiler_package_redirects_match_typescript_oracle() {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace)
        .unwrap_or_else(|error| panic!("failed to load upstream plans: {error}"));
    let oracle: Value = serde_json::from_slice(
        &fs::read(workspace.join("vendor/typescript-6.0.3/compiler-package-redirects.v1.json"))
            .expect("read compiler package-redirect oracle"),
    )
    .expect("compiler package-redirect oracle is JSON");
    let fixtures = oracle["fixtures"]
        .as_array()
        .expect("compiler package-redirect fixtures are an array");
    assert_eq!(fixtures.len(), 8, "focused fixture inventory drifted");

    for expected in fixtures {
        let case_id = string_field(expected, "case_id", "package-redirect oracle");
        let prepared = load_compiler_no_emit(&workspace, plan(&corpus.plans, case_id), limits())
            .unwrap_or_else(|error| panic!("failed to load {case_id}: {error}"));

        let library_paths = prepared
            .library_files()
            .iter()
            .map(|source| {
                prepared
                    .source_file(*source)
                    .expect("library source is owned")
                    .path()
                    .display()
                    .display()
                    .to_string()
            })
            .collect::<BTreeSet<_>>();
        let mut actual_primary_sources = Vec::new();
        let mut actual_redirects = Vec::new();
        for source in prepared.source_files() {
            let target = source.path().display().display().to_string();
            if !library_paths.contains(&target) {
                actual_primary_sources.push(target.clone());
            }
            for redirect in source.package_redirect_paths() {
                let redirected = redirect.display().display().to_string();
                assert_eq!(
                    prepared.source_id(redirect.canonical()),
                    prepared.source_id(source.path().canonical()),
                    "{case_id}: redirect must join the target SourceFileId",
                );
                actual_redirects.push((redirected, target.clone()));
            }
        }
        actual_primary_sources.sort();
        actual_redirects.sort();

        let source_rows = expected["sources"]
            .as_array()
            .unwrap_or_else(|| panic!("{case_id}: oracle sources are an array"));
        let mut expected_primary_sources = source_rows
            .iter()
            .filter(|source| source["redirect_target"].is_null())
            .map(|source| string_field(source, "file", case_id).to_owned())
            .collect::<Vec<_>>();
        let mut expected_redirects = source_rows
            .iter()
            .filter_map(|source| {
                source["redirect_target"].as_str().map(|target| {
                    (
                        string_field(source, "file", case_id).to_owned(),
                        target.to_owned(),
                    )
                })
            })
            .collect::<Vec<_>>();
        expected_primary_sources.sort();
        expected_redirects.sort();
        assert_eq!(
            actual_primary_sources, expected_primary_sources,
            "{case_id}: primary source identities drifted",
        );
        assert_eq!(
            actual_redirects, expected_redirects,
            "{case_id}: package redirects drifted",
        );

        let outcome = ProgramSession::new(prepared)
            .run()
            .unwrap_or_else(|error| panic!("failed to execute {case_id}: {error:?}"));
        let actual_diagnostics = outcome
            .diagnostics()
            .map(diagnostic_record)
            .collect::<Vec<_>>();
        assert_eq!(
            actual_diagnostics,
            expected["diagnostics"]
                .as_array()
                .unwrap_or_else(|| panic!("{case_id}: oracle diagnostics are an array"))
                .clone(),
            "{case_id}: TypeScript diagnostics drifted",
        );
    }
}

#[test]
fn project_session_runs_focused_node_modules_search_programs() {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace)
        .unwrap_or_else(|error| panic!("failed to load upstream plans: {error}"));
    let oracle: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join("vendor/typescript-6.0.3/project-node-modules-search.v1.json"))
            .expect("read NodeModulesSearch project oracle"),
    )
    .expect("NodeModulesSearch project oracle is JSON");
    for upstream in corpus.plans.iter().filter(|plan| {
        plan.provenance
            .case_id
            .as_ref()
            .contains("project/nodeModules")
    }) {
        let UpstreamExecutionInput::Project(project) = &upstream.input else {
            panic!("focused project case is not a project plan");
        };
        let execution = load_node_modules_search_project(&workspace, project, limits())
            .unwrap_or_else(|error| {
                panic!("failed to load {}: {error}", upstream.provenance.case_id)
            });
        let expected = oracle["cases"]
            .as_array()
            .expect("project oracle cases is an array")
            .iter()
            .find(|expected| expected["case_id"] == upstream.provenance.case_id.as_ref())
            .unwrap_or_else(|| panic!("oracle is missing {}", upstream.provenance.case_id));
        let outcome = ProgramSession::new(execution.prepared_program)
            .run()
            .unwrap_or_else(|error| {
                panic!(
                    "failed to execute {}: {error:?}",
                    upstream.provenance.case_id
                )
            });
        let expected_diagnostics = expected["pre_emit_diagnostics"]
            .as_array()
            .expect("project oracle diagnostics is an array")
            .iter()
            // Project option deprecations are retained on ConfigRootPlan;
            // ProgramSession owns source/global diagnostics only.
            .filter(|diagnostic| diagnostic["code"] != 5107)
            .collect::<Vec<_>>();
        let actual_diagnostics = outcome.diagnostics().collect::<Vec<_>>();
        assert_eq!(
            actual_diagnostics.len(),
            expected_diagnostics.len(),
            "semantic diagnostic count drifted for {}",
            upstream.provenance.case_id
        );
        let current_directory = project.fixture.current_directory.as_ref();
        for (actual, expected) in actual_diagnostics.iter().zip(expected_diagnostics) {
            assert_eq!(actual.code(), expected["code"].as_u64().unwrap() as u32);
            assert_eq!(actual.message_text(), expected["message"].as_str().unwrap());
            assert_eq!(
                actual.start,
                expected["start"].as_u64().map(|value| value as u32)
            );
            assert_eq!(
                actual.length,
                expected["length"].as_u64().map(|value| value as u32)
            );
            let actual_file = actual
                .file_name
                .as_deref()
                .and_then(|name| name.strip_prefix(current_directory))
                .and_then(|name| name.strip_prefix('/'));
            assert_eq!(actual_file, expected["file"].as_str());
        }
    }
}

#[test]
#[ignore = "local H0 compiler session coverage audit; not a checked-in gate"]
fn audit_all_recorded_compiler_no_emit_sessions_locally() {
    const PROGRESS_INTERVAL: usize = 250;

    let workspace = workspace_root();
    let started = Instant::now();
    let start = std::env::var("TSRS_COMPILER_AUDIT_START")
        .ok()
        .map(|value| {
            value
                .parse::<usize>()
                .unwrap_or_else(|_| panic!("invalid TSRS_COMPILER_AUDIT_START {value:?}"))
        })
        .unwrap_or(0);
    let corpus = load_recorded_execution_plans(&workspace)
        .unwrap_or_else(|error| panic!("failed to load upstream plans: {error}"));
    let compiler_plan_count = corpus
        .plans
        .iter()
        .filter(|upstream| matches!(upstream.input, UpstreamExecutionInput::Compiler(_)))
        .count();
    assert!(
        start < compiler_plan_count,
        "TSRS_COMPILER_AUDIT_START {start} is outside {compiler_plan_count} compiler plans"
    );
    let mut attempted = 0_usize;
    let mut loaded = 0_usize;
    let mut executed = 0_usize;
    let mut failures = Vec::new();
    for upstream in corpus
        .plans
        .iter()
        .filter(|upstream| matches!(upstream.input, UpstreamExecutionInput::Compiler(_)))
        .skip(start)
    {
        let UpstreamExecutionInput::Compiler(compiler) = &upstream.input else {
            unreachable!("filtered to compiler plans")
        };
        attempted += 1;
        let prepared = match load_compiler_no_emit(&workspace, compiler, limits()) {
            Ok(prepared) => {
                loaded += 1;
                prepared
            }
            Err(error) => {
                failures.push((
                    upstream.provenance.case_id.to_string(),
                    format!("load: {error}"),
                ));
                continue;
            }
        };
        match catch_unwind(AssertUnwindSafe(|| {
            ProgramSession::new(prepared).run_for_harness_with_lib_cache()
        })) {
            Ok(Ok(_)) => executed += 1,
            Ok(Err(error)) => failures.push((
                upstream.provenance.case_id.to_string(),
                format!("session: {error:?}"),
            )),
            Err(payload) => {
                let detail = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("non-string panic payload");
                let failure = (
                    upstream.provenance.case_id.to_string(),
                    format!("panic: {detail}"),
                );
                eprintln!("FAIL {}: {}", failure.0, failure.1);
                failures.push(failure);
            }
        }
        if attempted.is_multiple_of(PROGRESS_INTERVAL) {
            eprintln!(
                "compiler session audit: start={start} attempted={attempted} loaded={loaded} executed={executed} failures={} elapsed={:.1?}",
                failures.len(),
                started.elapsed(),
            );
        }
    }
    eprintln!(
        "compiler session audit: start={start} attempted={attempted} loaded={loaded} executed={executed} failures={} elapsed={:.1?}",
        failures.len(),
        started.elapsed(),
    );
    for (case_id, error) in failures.iter().take(200) {
        eprintln!("FAIL {case_id}: {error}");
    }
    assert_eq!(loaded, attempted, "every compiler fixture must load");
    assert_eq!(
        executed, attempted,
        "every loaded compiler fixture must run"
    );
    assert!(failures.is_empty(), "compiler session audit found failures");
}
