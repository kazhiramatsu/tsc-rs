//! Replay independent tsgo order fixtures with the finite-command adapter.
use std::{num::NonZeroUsize, path::PathBuf};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_config_program_with_no_emit_override, parse_config_root_plan, CompilerConfigHost,
    ConfigRootPlanRequest, LibraryCatalog, ProgramLoadLimits, WorkerBudget,
};
#[path = "support/loader_order.rs"]
#[allow(dead_code)]
mod corpus;

#[test]
fn level_zero_preserves_reference_order_reasons_diagnostics_and_trace() {
    let fixtures: Vec<corpus::Fixture> =
        serde_json::from_str(include_str!("fixtures/loader-order.json")).unwrap();
    let scan_fixtures: Vec<corpus::Fixture> =
        serde_json::from_str(include_str!("fixtures/import-scan.json")).unwrap();
    for fixture in fixtures.into_iter().chain(scan_fixtures) {
        for count in [1, 4, 16] {
            let workers = WorkerBudget::new(NonZeroUsize::new(count).unwrap());
            for trace in [false, true] {
                let mut config: serde_json::Value =
                    serde_json::from_str(&fixture.files["tsconfig.json"]).unwrap();
                config["compilerOptions"]["traceResolution"] = trace.into();
                let config = config.to_string();
                let mut host =
                    MemoryCompilerHost::builder("/project").case_sensitive(fixture.case_sensitive);
                for (name, text) in &fixture.files {
                    let text = if name == "tsconfig.json" {
                        &config
                    } else {
                        text
                    };
                    host = host.file(format!("/project/{name}"), text.as_bytes().to_vec());
                }
                for (name, target) in &fixture.symlinks {
                    host = host.realpath(format!("/project/{name}"), format!("/project/{target}"));
                }
                let host = host.build().unwrap();
                let plan = parse_config_root_plan(
                    &CompilerConfigHost::new(&host),
                    ConfigRootPlanRequest {
                        file_name: "/project/tsconfig.json".into(),
                        text: config,
                        base_path: "/project".into(),
                    },
                )
                .unwrap()
                .with_identity_domain(tsc_types::IdentityDomain::new());
                let program = load_config_program_with_no_emit_override(
                    &host,
                    &plan,
                    &LibraryCatalog::typescript_7_1(PathBuf::from("/project/lib")),
                    ProgramLoadLimits::new(128, 1024, 64, 1 << 20, 1 << 24).with_workers(workers),
                )
                .unwrap();
                if trace {
                    assert!(
                        program
                            .source_files()
                            .iter()
                            .all(|source| !source.preparsed_syntax().has_direct_layout()),
                        "trace inputs take the ordinary path"
                    );
                }
                if !trace && fixture.name == "project_redirect" {
                    let layout = program.source_files()[0]
                        .preparsed_syntax()
                        .direct_layout()
                        .unwrap();
                    assert_eq!(
                        layout.len(),
                        2,
                        "only globals and the root project's main file are placed"
                    );
                    assert!(layout
                        .iter()
                        .all(|placement| !placement.name.ends_with("lib/src/value.ts")));
                    assert!(layout
                        .iter()
                        .all(|placement| !placement.name.ends_with("lib/dist/value.d.ts")));
                }
                let baseline = corpus::load(&fixture, workers, trace);
                let paths: Vec<_> = program
                    .source_files()
                    .iter()
                    .map(|s| s.path().display().to_string_lossy().into_owned())
                    .collect();
                assert_eq!(
                    paths, fixture.list_files,
                    "{} / {count} / {trace}",
                    fixture.name
                );
                assert_eq!(
                    program.explain_files("/project".into()),
                    baseline.explain_files("/project".into()),
                    "{} reasons",
                    fixture.name
                );
                assert_eq!(
                    program.diagnostics(),
                    baseline.diagnostics(),
                    "{} diagnostics",
                    fixture.name
                );
                assert_eq!(
                    program.resolution_trace(),
                    baseline.resolution_trace(),
                    "{} trace / {count}",
                    fixture.name
                );
                for source in program.source_files() {
                    let domain = tsc_types::IdentityDomain::new();
                    let room = domain
                        .lease_rooms(&[
                            tsc_syntax::placement::bounds(source.text().len(), 1).unwrap()
                        ])
                        .unwrap()
                        .pop()
                        .unwrap();
                    let mut stripe = tsc_syntax::placement::SyntaxStripe::new(room);
                    let (direct_plan, _) = tsc_program::plan_source_requests_in_stripe(
                        source,
                        program.compiler_options(),
                        &mut stripe,
                    )
                    .unwrap();
                    let (ordinary, _) = tsc_program::plan_source_requests_retaining_syntax(
                        source,
                        program.compiler_options(),
                        None,
                    )
                    .unwrap();
                    assert_eq!(
                        direct_plan,
                        ordinary,
                        "{} occurrence inventory: {:?}",
                        fixture.name,
                        source.path().display()
                    );
                    stripe.seal().unwrap();
                }
                let resolutions: Vec<_> = program
                    .resolutions()
                    .modules()
                    .map(|(key, value)| format!("{key:?} {value:?}"))
                    .collect();
                let expected: Vec<_> = baseline
                    .resolutions()
                    .modules()
                    .map(|(key, value)| format!("{key:?} {value:?}"))
                    .collect();
                assert_eq!(resolutions, expected, "{} resolutions", fixture.name);
            }
        }
    }
}
