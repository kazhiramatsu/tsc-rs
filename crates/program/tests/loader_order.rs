//! Order-sensitive loader fixtures captured from tsgo 7.1.0-dev aa8149273.
//! Expectations are static; running these tests does not require tsgo or disk fixtures.
use std::num::NonZeroUsize;
use tsc_program::WorkerBudget;
#[path = "support/loader_order.rs"]
#[allow(dead_code)]
mod corpus;
use corpus::{fixture, load};

fn check(name: &str) {
    let fixture = fixture(name);
    for workers in [
        WorkerBudget::serial(),
        WorkerBudget::new(NonZeroUsize::new(4).expect("four workers")),
        WorkerBudget::automatic(),
    ] {
        let program = load(&fixture, workers, false);
        let paths: Vec<_> = program
            .source_files()
            .iter()
            .map(|source| source.path().display().to_string_lossy().into_owned())
            .collect();
        assert_eq!(paths, fixture.list_files, "{name}: listFiles {workers:?}");
        let mut explanation = String::new();
        for (path, reasons) in program.explain_files("/project".into()) {
            explanation.push_str(&path.to_string_lossy());
            explanation.push('\n');
            for reason in reasons {
                explanation.push_str("   ");
                explanation.push_str(&reason.to_string_lossy());
                explanation.push('\n');
            }
        }
        assert_eq!(
            explanation, fixture.explain_files,
            "{name}: explainFiles {workers:?}"
        );
        if fixture.check_trace {
            let traced = load(&fixture, workers, true);
            let trace = format!("{}\n", traced.resolution_trace().join("\n"));
            assert_eq!(trace, fixture.trace, "{name}: traceResolution {workers:?}");
        }
    }
}

#[test]
fn dfs_reasons() {
    check("dfs_reasons");
}
#[test]
fn respell_parent() {
    check("respell_parent");
}
#[test]
fn package_redirect() {
    check("package_redirect");
}
#[test]
fn package_no_dedupe() {
    check("package_no_dedupe");
}
#[test]
fn depth_restart() {
    check("depth_restart");
}
#[test]
fn trace_occurrences() {
    check("trace_occurrences");
}
#[test]
fn missing_occurrences() {
    check("missing_occurrences");
}
#[test]
fn automatic_order() {
    check("automatic_order");
}
#[test]
fn project_redirect() {
    check("project_redirect");
}
#[test]
fn extension_suggestion() {
    check("extension_suggestion");
}
#[test]
fn replacement_library() {
    check("replacement_library");
}
#[test]
fn library_trace_tail_order() {
    check("library_trace_tail_order");
}
#[test]
fn incremental_package_order() {
    check("incremental_package_order");
}
#[test]
fn missing_directives() {
    check("missing_directives");
}
#[test]
fn project_options() {
    check("project_options");
}
#[test]
fn project_options_incremental() {
    check("project_options_incremental");
}
#[test]
fn conflicting_package_identity() {
    check("conflicting_package_identity");
}
#[test]
fn package_mixed_versions() {
    check("package_mixed_versions");
}
#[test]
fn package_mixed_versions_reversed() {
    check("package_mixed_versions_reversed");
}

#[test]
fn resolution_iteration_uses_canonical_keys_independently_of_file_order() {
    let program = load(&fixture("dfs_reasons"), WorkerBudget::serial(), false);
    let keys: Vec<_> = program
        .resolutions()
        .modules()
        .map(|(key, _)| {
            (
                key.source().as_js().to_string_lossy().into_owned(),
                key.specifier().to_string_lossy().into_owned(),
            )
        })
        .collect();
    // An own API contract: tsgo exposes a map, while ours exposes a stable
    // sorted iterator. The fixture's file order starts globals, leaf, path.
    assert_eq!(
        keys,
        [
            ("/project/last.ts".to_owned(), "./shared".to_owned()),
            ("/project/main.ts".to_owned(), "./shared".to_owned()),
            ("/project/main.ts".to_owned(), "./side".to_owned()),
            ("/project/path.ts".to_owned(), "./leaf".to_owned()),
            ("/project/shared.ts".to_owned(), "./leaf".to_owned()),
            ("/project/side.ts".to_owned(), "./main".to_owned()),
        ]
    );
}

#[test]
fn package_redirect_discards_the_copys_own_resolution_rows() {
    let program = load(&fixture("package_redirect"), WorkerBudget::serial(), false);
    let keys: Vec<_> = program
        .resolutions()
        .modules()
        .map(|(key, _)| {
            (
                key.source().as_js().to_string_lossy().into_owned(),
                key.specifier().to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(
        keys,
        [
            ("/project/a/main.ts".to_owned(), "pkg".to_owned()),
            (
                "/project/a/node_modules/pkg/index.d.ts".to_owned(),
                "./a-only".to_owned()
            ),
            ("/project/b/main.ts".to_owned(), "pkg".to_owned()),
        ]
    );
}

#[test]
fn package_probe_inventory_is_published_in_canonical_order() {
    for workers in [WorkerBudget::serial(), WorkerBudget::automatic()] {
        let program = load(&fixture("package_redirect"), workers, false);
        let probes: Vec<_> = program
            .package_json_probes()
            .iter()
            .map(|probe| (probe.path.to_string_lossy().into_owned(), probe.exists))
            .collect();
        assert_eq!(
            probes,
            [
                ("/project/a/node_modules/pkg/package.json".to_owned(), true),
                ("/project/a/package.json".to_owned(), false),
                ("/project/b/node_modules/pkg/package.json".to_owned(), true),
                ("/project/b/package.json".to_owned(), false),
                ("/project/package.json".to_owned(), true),
            ]
        );
    }
}

#[test]
fn extension_suggestion_inventory_preserves_execution_order() {
    for workers in [WorkerBudget::serial(), WorkerBudget::automatic()] {
        let program = load(&fixture("extension_suggestion"), workers, false);
        let files: Vec<_> = program
            .import_extension_files()
            .iter()
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        // Last root executes first; the CLI diagnostic text is tested by
        // tsc-rs-compiler against the same reference fixture.
        assert_eq!(files, ["/project/other.mts", "/project/value.ts"]);
    }
}

#[test]
fn package_type_aggregate_ors_declarations_across_versions_in_either_root_order() {
    for name in ["package_mixed_versions", "package_mixed_versions_reversed"] {
        let program = load(&fixture(name), WorkerBudget::serial(), false);
        let flags: Vec<_> = program
            .resolutions()
            .modules()
            .map(|(_, row)| row.package_bundles_types())
            .collect();
        assert_eq!(flags, [true, true], "{name}");
    }
}

#[test]
fn raw_processing_diagnostics_keep_execution_order_and_type_occurrences() {
    for workers in [WorkerBudget::serial(), WorkerBudget::automatic()] {
        let program = load(&fixture("missing_directives"), workers, false);
        let raw: Vec<_> = program
            .diagnostics()
            .program()
            .iter()
            .map(|d| {
                (
                    d.file_name.as_ref().unwrap().to_string_lossy().into_owned(),
                    d.start.unwrap(),
                    d.code(),
                )
            })
            .collect();
        // CLI sorts these by location; the loader API retains last-root-first
        // processing order. These are separate compatibility contracts.
        assert_eq!(
            raw,
            [
                ("/project/a.ts".to_owned(), 21, 6053),
                ("/project/z.ts".to_owned(), 21, 6053),
                ("/project/z.ts".to_owned(), 58, 6053),
            ]
        );
        let types: Vec<_> = program
            .resolutions()
            .type_references()
            .flat_map(|(_, row)| {
                row.diagnostics()
                    .iter()
                    .map(|d| (d.start.unwrap(), d.code()))
            })
            .collect();
        assert_eq!(types, [(96, 2688), (127, 2688)]);
    }
}

#[test]
fn symlink_fact_vectors_follow_frozen_resolution_key_order() {
    let program = load(
        &fixture("conflicting_package_identity"),
        WorkerBudget::serial(),
        false,
    );
    let facts = tsc_program::discover_symlink_facts(&program);
    let files: Vec<_> = facts
        .files
        .iter()
        .map(|(real, link)| {
            (
                real.to_string_lossy().into_owned(),
                link.to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(
        files,
        [
            (
                "/project/shared/index.d.ts".to_owned(),
                "/project/node_modules/one/index.d.ts".to_owned()
            ),
            (
                "/project/shared/index.d.ts".to_owned(),
                "/project/node_modules/two/index.d.ts".to_owned()
            ),
        ]
    );
    let directories: Vec<_> = facts
        .directories
        .iter()
        .map(|(real, link)| {
            (
                real.to_string_lossy().into_owned(),
                link.to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(
        directories,
        [
            (
                "/project/shared".to_owned(),
                "/project/node_modules/one".to_owned()
            ),
            (
                "/project/shared".to_owned(),
                "/project/node_modules/two".to_owned()
            ),
        ]
    );
}

#[test]
fn runtime_dependency_links_follow_staged_admission_order() {
    use tsc_host::MemoryCompilerHost;
    use tsc_program::{load_no_lib_program, CompilerOptions, ProgramLoadLimits, ProgramOptions};
    let mut host = MemoryCompilerHost::builder("/project");
    for copy in ["a", "b"] {
        host = host
            .file(format!("/project/{copy}/main.ts"), b"export {};".to_vec())
            .file(
                format!("/project/{copy}/package.json"),
                br#"{"dependencies":{"dep":"*"}}"#.to_vec(),
            )
            .file(
                format!("/project/{copy}/node_modules/dep/package.json"),
                br#"{"name":"dep"}"#.to_vec(),
            )
            .file(
                format!("/project/shared/{copy}/package.json"),
                br#"{"name":"dep"}"#.to_vec(),
            )
            .realpath(
                format!("/project/{copy}/node_modules/dep"),
                format!("/project/shared/{copy}"),
            );
    }
    let host = host.build().unwrap();
    let program = load_no_lib_program(
        &host,
        &["/project/a/main.ts".into(), "/project/b/main.ts".into()],
        CompilerOptions {
            no_emit: Some(true),
            ..Default::default()
        },
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(Vec::new()),
        ProgramLoadLimits::new(128, 128, 64, 1024, 4096),
    )
    .unwrap();
    let links: Vec<_> = program
        .dependency_symlinks()
        .iter()
        .map(|d| {
            (
                d.real.to_string_lossy().into_owned(),
                d.symlink.to_string_lossy().into_owned(),
            )
        })
        .collect();
    // Go iterates a metadata map here; this ordering is our own API's
    // compatibility behavior, not reference CLI output.
    assert_eq!(
        links,
        [
            (
                "/project/shared/b/package.json".to_owned(),
                "/project/b/node_modules/dep/package.json".to_owned()
            ),
            (
                "/project/shared/a/package.json".to_owned(),
                "/project/a/node_modules/dep/package.json".to_owned()
            ),
        ]
    );
}
