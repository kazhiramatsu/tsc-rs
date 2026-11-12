//! Own loader API contracts with no matching tsgo resource-limit option.
use std::path::PathBuf;

use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_no_lib_program, CompilerOptions, ProgramLoadLimit, ProgramLoadLimits, ProgramOptions,
    WorkerBudget,
};

#[test]
fn competing_root_limits_name_the_execution_prefix_overflow() {
    let host = MemoryCompilerHost::builder("/project")
        .file("/project/a.ts", b"export {};".to_vec())
        .file("/project/b.ts", b"export {};".to_vec())
        .file("/project/c.ts", b"export {};".to_vec())
        .build()
        .unwrap();
    let roots: Vec<_> = ["a.ts", "b.ts", "c.ts"]
        .map(|p| PathBuf::from(format!("/project/{p}")))
        .into();
    for workers in [WorkerBudget::serial(), WorkerBudget::automatic()] {
        for (limits, kind, path, maximum, observed) in [
            (
                ProgramLoadLimits::new(1, 128, 64, 128, 1024),
                ProgramLoadLimit::SourceFiles,
                "/project/b.ts",
                1,
                2,
            ),
            (
                ProgramLoadLimits::new(128, 128, 64, 128, 15),
                ProgramLoadLimit::TotalSourceBytes,
                "/project/b.ts",
                15,
                20,
            ),
            (
                ProgramLoadLimits::new(128, 128, 64, 9, 1024),
                ProgramLoadLimit::SourceFileBytes,
                "/project/c.ts",
                9,
                10,
            ),
        ] {
            let error = load_no_lib_program(
                &host,
                &roots,
                CompilerOptions {
                    no_emit: Some(true),
                    ..Default::default()
                },
                ProgramOptions::default()
                    .with_no_lib(true)
                    .with_types(Vec::new()),
                limits.with_workers(workers),
            )
            .unwrap_err();
            let exceeded = error.limit_exceeded().expect("resource overflow");
            assert_eq!(exceeded.limit(), kind);
            assert_eq!(exceeded.js_path().unwrap(), path);
            assert_eq!(exceeded.maximum(), maximum);
            assert_eq!(exceeded.observed(), observed);
        }
    }
}

#[test]
fn occurrence_budget_is_charged_in_execution_order_before_target_loading() {
    let host = MemoryCompilerHost::builder("/project")
        .file("/project/a.ts", b"import './a-dep';".to_vec())
        .file(
            "/project/b.ts",
            b"import './b-dep';\nimport './b-dep';".to_vec(),
        )
        .file("/project/a-dep.ts", b"export {};".to_vec())
        .file("/project/b-dep.ts", b"export {};".to_vec())
        .build()
        .unwrap();
    for workers in [WorkerBudget::serial(), WorkerBudget::automatic()] {
        let error = load_no_lib_program(
            &host,
            &[
                PathBuf::from("/project/a.ts"),
                PathBuf::from("/project/b.ts"),
            ],
            CompilerOptions {
                no_emit: Some(true),
                ..Default::default()
            },
            ProgramOptions::default()
                .with_no_lib(true)
                .with_types(Vec::new()),
            ProgramLoadLimits::new(128, 2, 64, 1024, 1024).with_workers(workers),
        )
        .unwrap_err();
        let exceeded = error.limit_exceeded().unwrap();
        assert_eq!(exceeded.limit(), ProgramLoadLimit::RequestEdges);
        assert_eq!(exceeded.js_path().unwrap(), "/project/a.ts");
        assert_eq!(exceeded.maximum(), 2);
        assert_eq!(exceeded.observed(), 3);
    }
}

#[test]
fn first_admission_tree_depth_is_not_replaced_by_a_later_shallow_route() {
    let host = MemoryCompilerHost::builder("/project")
        .file("/project/shallow.ts", b"import './shared';".to_vec())
        .file("/project/deep.ts", b"import './bridge';".to_vec())
        .file("/project/bridge.ts", b"import './shared';".to_vec())
        .file("/project/shared.ts", b"export {};".to_vec())
        .build()
        .unwrap();
    let error = load_no_lib_program(
        &host,
        &[
            PathBuf::from("/project/shallow.ts"),
            PathBuf::from("/project/deep.ts"),
        ],
        CompilerOptions {
            no_emit: Some(true),
            ..Default::default()
        },
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(Vec::new()),
        ProgramLoadLimits::new(128, 128, 1, 1024, 4096),
    )
    .unwrap_err();
    let exceeded = error.limit_exceeded().unwrap();
    assert_eq!(exceeded.limit(), ProgramLoadLimit::SourceDepth);
    assert_eq!(exceeded.js_path().unwrap(), "/project/shared.ts");
    assert_eq!(exceeded.observed(), 2);
}
