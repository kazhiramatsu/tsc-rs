use std::sync::atomic::{AtomicBool, AtomicUsize};

use super::*;

fn finish_empty_fixed_views(ratchet_path: &Path) -> Vec<MeasuredView> {
    initialize_view_accumulators(
        &ratchet::FIXED_VIEWS,
        SetGate::Enforce,
        DiagnosticBand::All,
        ratchet_path,
        ReportIdentityMode::AllViews,
    )
    .into_iter()
    .map(|pending| {
        let result = pending
            .state
            .and_then(|accumulator| accumulator.finish(0, "draft", 0));
        MeasuredView {
            band: pending.band,
            result,
        }
    })
    .collect()
}

fn read_summary_band(path: &Path) -> String {
    decode_ci_conformance_summary(&fs::read(path).unwrap())
        .unwrap()
        .as_summary()
        .band
        .clone()
}

#[test]
fn later_view_initialization_error_preserves_prior_callback_order() {
    let directory = test_git::temp_dir("deferred-fixed-view-error");
    let ratchet_path = directory.join("ratchet.toml");
    fs::write(
        &ratchet_path,
        "[t0]\nrate = 0.0\n\
         [t1]\nrate = 0.0\n\
         [t0-2xxx]\nrate = 0.0\n\
         [t0-syntactic]\nrate = \"invalid\"\n",
    )
    .unwrap();

    let measured = finish_empty_fixed_views(&ratchet_path);
    let out_json = directory.join("out.json");
    let mut callbacks = Vec::new();
    let mut all_finishes = 0usize;
    let result = complete_ci_views(
        measured,
        [&out_json; 3],
        None,
        &BTreeSet::new(),
        false,
        |_| {
            all_finishes += 1;
            Ok(())
        },
        |summary| callbacks.push((summary.band.clone(), read_summary_band(&out_json))),
    );
    let error = match result {
        Ok(_) => panic!("invalid syntactic ratchet must fail at the syntactic gate"),
        Err(error) => error,
    };

    assert_eq!(all_finishes, 1);
    assert_eq!(
        callbacks,
        [
            ("all".to_owned(), "all".to_owned()),
            ("2xxx".to_owned(), "2xxx".to_owned()),
        ]
    );
    assert_eq!(read_summary_band(&out_json), "2xxx");
    assert!(
        error
            .to_string()
            .contains("[t0-syntactic].rate must be a number"),
        "{error}"
    );
}

#[test]
fn later_view_gate_error_writes_its_summary_without_callback() {
    let directory = test_git::temp_dir("fixed-view-gate-error");
    let ratchet_path = directory.join("ratchet.toml");
    fs::write(
        &ratchet_path,
        "[t0]\nrate = 0.0\n\
         [t1]\nrate = 0.0\n\
         [t0-2xxx]\nrate = 0.0\n\
         [t0-syntactic]\nrate = 2.0\n",
    )
    .unwrap();

    let out_json = directory.join("out.json");
    let mut callbacks = Vec::new();
    let result = complete_ci_views(
        finish_empty_fixed_views(&ratchet_path),
        [&out_json; 3],
        None,
        &BTreeSet::new(),
        true,
        |_| Ok(()),
        |summary| callbacks.push(summary.band.clone()),
    );
    let error = match result {
        Ok(_) => panic!("syntactic ratchet regression must fail at its gate"),
        Err(error) => error,
    };

    assert_eq!(callbacks, ["all", "2xxx"]);
    assert_eq!(read_summary_band(&out_json), "syntactic");
    assert!(
        error.to_string().contains("T0 ratchet regression"),
        "{error}"
    );
}

#[test]
fn fixed_view_processor_rejects_noncanonical_order() {
    let mut callbacks = Vec::new();
    let result = process_fixed_views(
        [
            (DiagnosticBand::TwoXxx, Ok(())),
            (DiagnosticBand::All, Ok(())),
            (DiagnosticBand::Syntactic, Ok(())),
        ],
        |band, ()| {
            callbacks.push(band);
            Ok(())
        },
    );
    let error = match result {
        Ok(_) => panic!("reordered fixed views must fail in release builds"),
        Err(error) => error,
    };

    assert!(callbacks.is_empty());
    assert!(
        error.to_string().contains("expected all, observed 2xxx"),
        "{error}"
    );
}

#[test]
fn ratchet_collection_retains_full_state_only_for_selected_view() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap();
    let accumulators = initialize_view_accumulators(
        &ratchet::FIXED_VIEWS,
        SetGate::Collect,
        DiagnosticBand::All,
        &workspace.join("ratchet.toml"),
        ReportIdentityMode::AllViews,
    );

    assert!(matches!(
        accumulators[0].state,
        Ok(ViewAccumulatorKind::Full(_))
    ));
    assert!(accumulators[1..]
        .iter()
        .all(|pending| matches!(pending.state, Ok(ViewAccumulatorKind::RunSetsOnly(_)))));
}

#[test]
fn fused_and_pipelined_fixed_views_match_single_view_grading_and_execute_each_case_once() {
    fn full_view(measured: &MeasuredView) -> &FinishedConformanceView {
        match measured.result.as_ref() {
            Ok(FinishedViewMeasurement::Full(view)) => view.as_ref(),
            Ok(FinishedViewMeasurement::RunSetsOnly { band, .. }) => {
                panic!("{} unexpectedly retained only run sets", band.name())
            }
            Err(error) => panic!("view measurement failed: {error}"),
        }
    }

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_owned();
    let options = ConformanceOptions {
        workspace: workspace.clone(),
        limit: Some(1),
        files: Vec::new(),
        out_json: test_git::temp_dir("fused-conformance-parity").join("unused.json"),
        band: DiagnosticBand::All,
        checker_workers: 2,
    };
    let executions = AtomicUsize::new(0);
    let fused = measure_conformance_with(
        &options,
        &ratchet::FIXED_VIEWS,
        SetGate::Enforce,
        false,
        None,
        None,
        false,
        ReportIdentityMode::AllViews,
        |fixture, program, vendor_lib_dir| {
            executions.fetch_add(1, Ordering::Relaxed);
            current_case_tsrs(fixture, program, vendor_lib_dir)
        },
    )
    .unwrap();
    let executions = executions.load(Ordering::Relaxed);
    assert_eq!(fused.views.len(), ratchet::FIXED_VIEWS.len());
    assert_eq!(executions, full_view(&fused.views[0]).summary.cases_total);
    assert!(fused
        .views
        .iter()
        .all(|view| full_view(view).summary.cases_total == executions));

    let pipeline_executions = AtomicUsize::new(0);
    let pipelined = measure_conformance_with_schedule(
        &options,
        &ratchet::FIXED_VIEWS,
        SetGate::Enforce,
        false,
        None,
        None,
        false,
        ReportIdentityMode::AllViews,
        MeasurementSchedule::CiCheckerGradingPipeline,
        |fixture, program, vendor_lib_dir| {
            pipeline_executions.fetch_add(1, Ordering::Relaxed);
            current_case_tsrs(fixture, program, vendor_lib_dir)
        },
    )
    .unwrap();
    assert_eq!(pipeline_executions.load(Ordering::Relaxed), executions);
    for (sequential, pipelined) in fused.views.iter().zip(&pipelined.views) {
        let sequential = full_view(sequential);
        let pipelined = full_view(pipelined);
        assert_eq!(sequential.sets, pipelined.sets);
        assert_eq!(
            serde_json::to_vec(&sequential.summary).unwrap(),
            serde_json::to_vec(&pipelined.summary).unwrap(),
            "bounded CI pipeline changed the sequential summary"
        );
    }

    let projected = measure_conformance(
        &options,
        &ratchet::FIXED_VIEWS,
        SetGate::Enforce,
        false,
        None,
        None,
        false,
        ReportIdentityMode::AllViewOnly,
    )
    .unwrap();
    for (band, (full, projected)) in ratchet::FIXED_VIEWS
        .iter()
        .copied()
        .zip(fused.views.iter().zip(&projected.views))
    {
        let full = full_view(full);
        let projected = full_view(projected);
        assert_eq!(full.sets, projected.sets);

        let mut expected = full.summary.clone();
        if band != DiagnosticBand::All {
            expected.shadow_tier_identities.t1_matched.clear();
            expected.shadow_tier_identities.t2_matched.clear();
            expected.shadow_tier_identities.t3_matched.clear();
            expected.supported_shadow_tier_identities.t1_matched.clear();
            expected.supported_shadow_tier_identities.t2_matched.clear();
            expected.supported_shadow_tier_identities.t3_matched.clear();
        }
        assert_eq!(
            serde_json::to_vec(&expected).unwrap(),
            serde_json::to_vec(&projected.summary).unwrap(),
            "CI {} projection changed non-report summary fields",
            band.name()
        );

        let artifact = serde_json::to_vec(&CiSummaryArtifactRef {
            schema: CI_SUMMARY_ARTIFACT_SCHEMA,
            projection: CI_SUMMARY_PROJECTION,
            summary: &projected.summary,
        })
        .unwrap();
        let decoded = decode_ci_conformance_summary(&artifact).unwrap();
        assert_eq!(
            serde_json::to_vec(decoded.as_summary()).unwrap(),
            serde_json::to_vec(&projected.summary).unwrap()
        );
        assert!(
            decode_ci_conformance_summary(&serde_json::to_vec(&projected.summary).unwrap())
                .is_err()
        );

        if band != DiagnosticBand::All {
            let mut invalid = projected.summary.clone();
            invalid.shadow_tier_identities.schema += 1;
            let artifact = serde_json::to_vec(&CiSummaryArtifactRef {
                schema: CI_SUMMARY_ARTIFACT_SCHEMA,
                projection: CI_SUMMARY_PROJECTION,
                summary: &invalid,
            })
            .unwrap();
            assert!(decode_ci_conformance_summary(&artifact).is_err());

            let mut invalid = projected.summary.clone();
            invalid.band = "bogus".to_owned();
            let artifact = serde_json::to_vec(&CiSummaryArtifactRef {
                schema: CI_SUMMARY_ARTIFACT_SCHEMA,
                projection: CI_SUMMARY_PROJECTION,
                summary: &invalid,
            })
            .unwrap();
            assert!(decode_ci_conformance_summary(&artifact).is_err());
        }
    }

    let force_t4_measurement = fused
        .accepted
        .as_ref()
        .is_some_and(|accepted| accepted.t4_active);
    let collected = run_conformance_inner(
        &options,
        SetGate::Collect,
        false,
        None,
        None,
        force_t4_measurement,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&collected.summary).unwrap(),
        serde_json::to_vec(&full_view(&fused.views[0]).summary).unwrap(),
        "Collect selected-view summary differs from Full measurement"
    );
    let ordinary: ConformanceSummary =
        serde_json::from_slice(&fs::read(&options.out_json).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_vec(&ordinary).unwrap(),
        serde_json::to_vec(&collected.summary).unwrap(),
        "ordinary output must remain a naked ConformanceSummary"
    );
    assert!(decode_ci_conformance_summary(&fs::read(&options.out_json).unwrap()).is_err());
    let mut full_sets = ratchet::RunSets::new();
    for measured in &fused.views {
        full_sets.extend(full_view(measured).sets.clone());
    }
    assert_eq!(
        collected.sets, full_sets,
        "RunSets-only projections differ from Full accumulators"
    );

    for (index, band) in ratchet::FIXED_VIEWS.iter().copied().enumerate() {
        let mut single_options = options.clone();
        single_options.band = band;
        let single = measure_conformance(
            &single_options,
            std::slice::from_ref(&band),
            SetGate::Enforce,
            false,
            None,
            None,
            false,
            ReportIdentityMode::AllViews,
        )
        .unwrap();
        assert_eq!(single.views.len(), 1);
        assert_eq!(fused.views[index].band, band);
        let fused_view = full_view(&fused.views[index]);
        let single_view = full_view(&single.views[0]);
        assert_eq!(
            serde_json::to_vec(&fused_view.summary).unwrap(),
            serde_json::to_vec(&single_view.summary).unwrap(),
            "fused {} summary differs from its single-view grade",
            band.name()
        );
        assert_eq!(fused_view.sets, single_view.sets);
    }
}

#[test]
fn ci_pipeline_joins_checker_producer_before_returning_its_error() {
    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_owned();
    let out_json = test_git::temp_dir("ci-pipeline-error-join").join("must-not-exist.json");
    let options = ConformanceOptions {
        workspace,
        limit: Some(1),
        files: Vec::new(),
        out_json: out_json.clone(),
        band: DiagnosticBand::All,
        checker_workers: 1,
    };
    let producer_dropped = Arc::new(AtomicBool::new(false));
    let attempts = Arc::new(AtomicUsize::new(0));
    let guard = DropFlag(Arc::clone(&producer_dropped));
    let producer_attempts = Arc::clone(&attempts);
    let result = measure_conformance_with_schedule(
        &options,
        &ratchet::FIXED_VIEWS,
        SetGate::Enforce,
        false,
        None,
        None,
        false,
        ReportIdentityMode::AllViewOnly,
        MeasurementSchedule::CiCheckerGradingPipeline,
        move |_, _, _| {
            let _keep_guard_alive = &guard;
            producer_attempts.fetch_add(1, Ordering::Relaxed);
            Err("injected CI checker failure".into())
        },
    );
    let error = match result {
        Ok(_) => panic!("injected checker failure must fail the pipeline"),
        Err(error) => error,
    };

    // The ordered stream lets the single worker run ahead through its
    // bounded result channel before the in-order consume raises the first
    // error, so the attempt count is a small bound rather than exactly one:
    // at one worker the channel holds at most two results plus one
    // in-flight execution beyond the failing case.
    let attempted = attempts.load(Ordering::Relaxed);
    assert!(
        (1..=4).contains(&attempted),
        "expected bounded checker lookahead, executed {attempted} cases"
    );
    assert!(producer_dropped.load(Ordering::Acquire));
    assert!(error.to_string().contains("injected CI checker failure"));
    assert!(!out_json.exists());
}

fn diag(category: &str, start: u32, text: &str) -> GoldenDiag {
    GoldenDiag {
        file: Some("a.ts".to_owned()),
        start: Some(start),
        length: Some(1),
        line: Some(1),
        col: Some(1),
        code: 2322,
        pass: None,
        category: category.to_owned(),
        chain: GoldenMessageChain {
            text: text.to_owned(),
            code: 2322,
            category: category.to_owned(),
            next: Vec::new(),
        },
        related: Vec::new(),
        reports_unnecessary: false,
        reports_deprecated: false,
        source: None,
    }
}

fn diag_with_code(code: u32, category: &str, start: u32, text: &str) -> GoldenDiag {
    let mut diagnostic = diag(category, start, text);
    diagnostic.code = code;
    diagnostic.chain.code = code;
    diagnostic
}

#[test]
fn two_xxx_projection_is_exact_for_every_bucket_set_and_t0_universe() {
    let oracle = [
        diag_with_code(1999, "error", 1, "outside-low"),
        diag_with_code(2000, "error", 2, "boundary-low"),
        diag_with_code(2322, "error", 3, "duplicate-a"),
        diag_with_code(2322, "warning", 4, "duplicate-b"),
        diag_with_code(2500, "error", 5, "oracle-text"),
        diag_with_code(2807, "error", 6, "oracle-only"),
        diag_with_code(3000, "error", 7, "outside-high"),
    ];
    let actual = [
        diag_with_code(1999, "error", 1, "outside-low"),
        diag_with_code(2000, "error", 2, "boundary-low"),
        // The 2322 bucket is present but multiplicity-incomplete.
        diag_with_code(2322, "error", 3, "duplicate-a"),
        // The 2500 bucket matches T1 but not T2/T3.
        diag_with_code(2500, "error", 5, "tsrs-text"),
        diag_with_code(2999, "error", 8, "tsrs-only"),
        diag_with_code(3000, "error", 7, "outside-high"),
    ];

    let all = ratchet::bucket_grading(oracle.iter(), actual.iter());
    let projected = project_two_xxx_grading(&all);
    let direct = ratchet::bucket_grading(
        oracle
            .iter()
            .filter(|diagnostic| DiagnosticBand::TwoXxx.contains(diagnostic.code)),
        actual
            .iter()
            .filter(|diagnostic| DiagnosticBand::TwoXxx.contains(diagnostic.code)),
    );

    assert_eq!(projected, direct);
    assert!(direct.expected.iter().any(|key| key.code == 2807));
    assert!(direct.actual.iter().any(|key| key.code == 2999));
    assert!(direct.sets.matched.iter().any(|key| key.code == 2322));
    assert!(!direct
        .sets
        .multiplicity_complete
        .iter()
        .any(|key| key.code == 2322));
    assert!(direct.sets.t1.iter().any(|key| key.code == 2500));
    assert!(!direct.sets.t2.iter().any(|key| key.code == 2500));
    assert!(!direct.expected.iter().any(|key| key.code == 1999));
    assert!(!direct.expected.iter().any(|key| key.code == 3000));
}

/// Review round 3: tiers compare independent multisets — a
/// category-multiset match must register T1 even when the
/// category↔text CORRESPONDENCE differs (which is a T2 miss),
/// and multiplicity differences miss every tier.
#[test]
fn shadow_tiers_grade_buckets_as_independent_multisets() {
    // Same T0 key (same file/code/line/col): one error + one
    // warning per side, texts swapped across categories.
    let actual = [diag("error", 5, "A"), diag("warning", 5, "B")];
    let expected = [diag("error", 5, "B"), diag("warning", 5, "A")];
    let matched = shadow_tier_matches(actual.iter(), expected.iter());
    assert_eq!(
        (matched.t1.len(), matched.t2.len(), matched.t3.len()),
        (1, 0, 0)
    );

    // Identical buckets → all tiers.
    let actual = [diag("error", 5, "A"), diag("warning", 5, "B")];
    let expected = [diag("warning", 5, "B"), diag("error", 5, "A")];
    let matched = shadow_tier_matches(actual.iter(), expected.iter());
    assert_eq!(
        (matched.t1.len(), matched.t2.len(), matched.t3.len()),
        (1, 1, 1)
    );

    // Multiplicity difference on a shared key → no tier.
    let actual = [diag("error", 5, "A")];
    let expected = [diag("error", 5, "A"), diag("error", 5, "A")];
    let matched = shadow_tier_matches(actual.iter(), expected.iter());
    assert_eq!(
        (matched.t1.len(), matched.t2.len(), matched.t3.len()),
        (0, 0, 0)
    );

    // Chain-tail divergence: T2 matches, T3 misses.
    let mut deep = diag("error", 5, "A");
    deep.chain.next.push(GoldenMessageChain {
        text: "tail".to_owned(),
        code: 1,
        category: "error".to_owned(),
        next: Vec::new(),
    });
    let actual = [deep];
    let expected = [diag("error", 5, "A")];
    let matched = shadow_tier_matches(actual.iter(), expected.iter());
    assert_eq!(
        (matched.t1.len(), matched.t2.len(), matched.t3.len()),
        (1, 1, 0)
    );
}

#[test]
fn supported_tier_residual_preserves_both_bucket_shapes() {
    let actual = [diag("error", 5, "actual")];
    let expected = [diag("suggestion", 6, "expected")];
    let key = t0_key(&expected[0]);
    let matches = shadow_tier_matches(actual.iter(), expected.iter());
    let residual = collect_supported_tier_mismatches(
        "conformance/a.ts",
        "",
        &actual,
        &expected,
        DiagnosticBand::All,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeSet::from([key.clone()]),
        ShadowTierMatchRefs::from(&matches),
    );
    assert_eq!(residual.len(), 1);
    assert_eq!(residual[0].diagnostic, key);
    assert_eq!(residual[0].first_failed_tier, "t1");
    assert_eq!(residual[0].actual, actual);
    assert_eq!(residual[0].expected, expected);
}

#[test]
fn fn_partial_boundary_audit_requires_a_reached_semantic_range() {
    let mut semantic = diag("error", 5, "A");
    semantic.pass = Some("semantic".to_owned());
    let key = t0_key(&semantic);
    let partial = PartialCheck {
        file_name: "a.ts".to_owned(),
        start: 4,
        length: 3,
        reason: "recognized ceiling".to_owned(),
    };
    let classified = classify_fn_partial_boundaries(
        std::slice::from_ref(&key),
        std::slice::from_ref(&semantic),
        std::slice::from_ref(&partial),
    );
    assert!(classified[0].reached_partial_boundary);
    assert_eq!(classified[0].reasons, ["recognized ceiling"]);

    semantic.pass = Some("syntactic".to_owned());
    let classified =
        classify_fn_partial_boundaries(&[key], &[semantic], std::slice::from_ref(&partial));
    assert!(!classified[0].reached_partial_boundary);
}

#[test]
fn supported_false_negative_plan_uses_exact_nonexcluded_occurrences() {
    let mut first = diag("error", 5, "missing");
    first.pass = Some("semantic".to_owned());
    let second = first.clone();
    let mut unrelated = diag("error", 9, "other");
    unrelated.pass = Some("semantic".to_owned());
    unrelated.line = Some(2);
    unrelated.col = Some(3);
    let oracle = vec![first, second, unrelated];
    let missing = BTreeSet::from([t0_key(&oracle[0])]);

    let identities = exact_supported_false_negative_identities(
        "conformance/a.ts",
        "strict=true",
        &oracle,
        DiagnosticBand::All,
        &BTreeSet::from([0]),
        &missing,
    )
    .unwrap();

    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].fixture, "conformance/a.ts");
    assert_eq!(identities[0].matrix_key, "strict=true");
    assert_eq!(identities[0].pass, "semantic");
    assert_eq!(identities[0].occurrence, 1);
    assert_eq!(identities[0].start, Some(5));
}

/// The harness serializes @lib as OptionValue::StringList; the
/// conversion must lowercase and forward it (a String-only match
/// silently dropped the option, leaving CompilerOptions.lib None).
#[test]
fn lib_string_list_reaches_compiler_options() {
    let program = tsc_harness::ProgramJson {
        schema: 1,
        cwd: ".".to_owned(),
        options: [(
            "lib".to_owned(),
            tsc_harness::OptionValue::StringList(vec!["ES2015".to_owned(), " Dom ".to_owned()]),
        )]
        .into_iter()
        .collect(),
        libs: Vec::new(),
        files: Vec::new(),
        matrix_key: String::new(),
    };
    let options = tsc_harness::compiler_options_from_program(&program);
    assert_eq!(
        options.lib,
        Some(vec!["es2015".to_owned(), "dom".to_owned()])
    );
}

#[test]
fn lib_comma_string_still_supported() {
    let program = tsc_harness::ProgramJson {
        schema: 1,
        cwd: ".".to_owned(),
        options: [(
            "lib".to_owned(),
            tsc_harness::OptionValue::String("ES2020, dom".to_owned()),
        )]
        .into_iter()
        .collect(),
        libs: Vec::new(),
        files: Vec::new(),
        matrix_key: String::new(),
    };
    let options = tsc_harness::compiler_options_from_program(&program);
    assert_eq!(
        options.lib,
        Some(vec!["es2020".to_owned(), "dom".to_owned()])
    );
}

#[test]
fn package_resolution_conditions_reach_compiler_options() {
    let program = tsc_harness::ProgramJson {
        schema: 1,
        cwd: ".".to_owned(),
        options: [
            (
                "resolvePackageJsonExports".to_owned(),
                tsc_harness::OptionValue::Bool(false),
            ),
            (
                "resolvePackageJsonImports".to_owned(),
                tsc_harness::OptionValue::Bool(true),
            ),
            (
                "customConditions".to_owned(),
                tsc_harness::OptionValue::StringList(vec![
                    "webpack".to_owned(),
                    "browser".to_owned(),
                ]),
            ),
            (
                "noDtsResolution".to_owned(),
                tsc_harness::OptionValue::Bool(true),
            ),
        ]
        .into_iter()
        .collect(),
        libs: Vec::new(),
        files: Vec::new(),
        matrix_key: String::new(),
    };
    let options = tsc_harness::compiler_options_from_program(&program);
    assert_eq!(options.resolve_package_json_exports, Some(false));
    assert_eq!(options.resolve_package_json_imports, Some(true));
    assert_eq!(
        options.custom_conditions,
        Some(vec!["webpack".to_owned(), "browser".to_owned()])
    );
    assert_eq!(options.no_dts_resolution, Some(true));
}

#[test]
fn module_detection_reaches_compiler_options() {
    let program = tsc_harness::ProgramJson {
        schema: 1,
        cwd: ".".to_owned(),
        options: [(
            "moduleDetection".to_owned(),
            tsc_harness::OptionValue::String("force".to_owned()),
        )]
        .into_iter()
        .collect(),
        libs: Vec::new(),
        files: Vec::new(),
        matrix_key: String::new(),
    };
    let options = tsc_harness::compiler_options_from_program(&program);
    assert_eq!(options.module_detection, Some(3));
    assert_eq!(options.emit_module_detection_kind(), 3);
}

#[test]
fn import_helpers_reaches_compiler_options() {
    let program = tsc_harness::ProgramJson {
        schema: 1,
        cwd: ".".to_owned(),
        options: [(
            "importHelpers".to_owned(),
            tsc_harness::OptionValue::Bool(true),
        )]
        .into_iter()
        .collect(),
        libs: Vec::new(),
        files: Vec::new(),
        matrix_key: String::new(),
    };
    assert_eq!(
        tsc_harness::compiler_options_from_program(&program).import_helpers,
        Some(true)
    );
}

#[test]
fn allow_arbitrary_extensions_reaches_compiler_options() {
    let program = tsc_harness::ProgramJson {
        schema: 1,
        cwd: ".".to_owned(),
        options: [(
            "allowArbitraryExtensions".to_owned(),
            tsc_harness::OptionValue::Bool(false),
        )]
        .into_iter()
        .collect(),
        libs: Vec::new(),
        files: Vec::new(),
        matrix_key: String::new(),
    };
    assert_eq!(
        tsc_harness::compiler_options_from_program(&program).allow_arbitrary_extensions,
        Some(false)
    );
}

#[test]
fn no_error_truncation_reaches_compiler_options() {
    let program = tsc_harness::ProgramJson {
        schema: 1,
        cwd: ".".to_owned(),
        options: [(
            "noErrorTruncation".to_owned(),
            tsc_harness::OptionValue::Bool(true),
        )]
        .into_iter()
        .collect(),
        libs: Vec::new(),
        files: Vec::new(),
        matrix_key: String::new(),
    };
    assert_eq!(
        tsc_harness::compiler_options_from_program(&program).no_error_truncation,
        Some(true)
    );
}

/// Integer ratchets gate exactly: one lost diagnostic must fail
/// even when the rounded rate would still pass.
#[test]
fn ratchet_integer_counts_parse() {
    let dir = temp_root("tsc-rs-ratchet-test");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ratchet.toml");
    fs::write(
        &path,
        "[t0]\nrate = 0.0979\nmatched = 4758\ntotal = 48573\nallowed_regression = 0.0\n",
    )
    .unwrap();
    let ratchet = read_ratchet(&path, DiagnosticBand::All).unwrap();
    assert_eq!(ratchet.matched, Some(4758));
    assert_eq!(ratchet.total, Some(48573));
    assert_eq!(ratchet.allowed_regression, 0.0);
    // The exact-compare shape used by the gate: losing one matched
    // diagnostic on the same corpus regresses.
    let (matched, total) = (ratchet.matched.unwrap(), ratchet.total.unwrap());
    assert!((4758u128) * (total as u128) >= (matched as u128) * (48573u128));
    assert!((4757u128) * (total as u128) < (matched as u128) * (48573u128));
    fs::remove_file(&path).ok();
}

#[test]
fn ratchet_parser_rejects_duplicate_sections_and_keys() {
    let dir = temp_root("tsc-rs-ratchet-duplicates-test");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ratchet.toml");

    fs::write(
        &path,
        "[t0]\nrate = 0.1\nmatched = 1\ntotal = 10\n\
         [t0]\nrate = 0.1\nmatched = 1\ntotal = 10\n",
    )
    .unwrap();
    let err = read_ratchet(&path, DiagnosticBand::All)
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalid ratchet.toml"), "{err}");

    fs::write(
        &path,
        "[t0]\nrate = 0.1\nrate = 0.1\nmatched = 1\ntotal = 10\n",
    )
    .unwrap();
    let err = read_ratchet(&path, DiagnosticBand::All)
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalid ratchet.toml"), "{err}");

    // Quoted and bare keys are the same TOML key. A text-level
    // duplicate checker must not let this semantic duplicate
    // bypass validation.
    fs::write(
        &path,
        "[t0]\nrate = 0.1\n\"rate\" = 0.1\nmatched = 1\ntotal = 10\n",
    )
    .unwrap();
    let err = read_ratchet(&path, DiagnosticBand::All)
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalid ratchet.toml"), "{err}");

    // Dotted and table syntax also share one semantic namespace.
    // The TOML parser must reject a repeated dotted path.
    fs::write(
        &path,
        "t0.rate = 0.1\nt0.\"rate\" = 0.1\nt0.matched = 1\nt0.total = 10\n",
    )
    .unwrap();
    let err = read_ratchet(&path, DiagnosticBand::All)
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalid ratchet.toml"), "{err}");

    // Valid quoted names are resolved by their TOML meaning.
    fs::write(&path, "[\"t0\"]\n\"rate\" = 0.1\nmatched = 1\ntotal = 10\n").unwrap();
    let ratchet = read_ratchet(&path, DiagnosticBand::All).unwrap();
    assert_eq!(ratchet.rate, 0.1);
    assert_eq!(ratchet.matched, Some(1));

    // A section expressed entirely with dotted keys is equivalent
    // to the table form and must be accepted too.
    fs::write(&path, "t0.rate = 0.1\nt0.matched = 1\nt0.total = 10\n").unwrap();
    let ratchet = read_ratchet(&path, DiagnosticBand::All).unwrap();
    assert_eq!(ratchet.rate, 0.1);
    assert_eq!(ratchet.total, Some(10));

    fs::write(
        &path,
        "[t0]\nrate = 0.1\nmatched = 1\ntotal = 10\nallowed_regression = nan\n",
    )
    .unwrap();
    let err = read_ratchet(&path, DiagnosticBand::All)
        .unwrap_err()
        .to_string();
    assert!(err.contains("allowed_regression must be finite"), "{err}");

    fs::remove_file(&path).ok();
}
