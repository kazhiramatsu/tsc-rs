use std::{num::NonZeroUsize, path::PathBuf};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_no_lib_program, CompilerOptions, ProgramLoadLimits, ProgramOptions, WorkerBudget,
};
use tsc_syntax::{placement, source_files_structurally_equal};

// Absolute/relative prefix layouts describe an isolated finite command. The
// process-wide stores share collision-safe numeric allocation across domains;
// unrelated commands must not interleave during this isolated layout comparison.
static COMMAND: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        module: Some(199),
        module_resolution: Some(3),
        ..Default::default()
    }
}
fn limits(workers: usize) -> ProgramLoadLimits {
    ProgramLoadLimits::new(100, 1000, 100, 1 << 20, 4 << 20)
        .with_workers(WorkerBudget::new(NonZeroUsize::new(workers).unwrap()))
}

#[test]
fn roots_are_dense_in_root_order_even_when_imported_first() {
    let _command = COMMAND.lock().unwrap();
    let mut builder = MemoryCompilerHost::builder("/p");
    let roots: Vec<_> = (0..9)
        .map(|i| PathBuf::from(format!("/p/r{i}.ts")))
        .collect();
    for (i, path) in roots.iter().enumerate() {
        let text = if i == 0 {
            "import './r8'; import './dep'; export const x = 0;".to_owned()
        } else {
            format!("export const x{i} = {i};")
        };
        builder = builder.file(path, text.into_bytes());
    }
    let host = builder
        .file("/p/dep.ts", b"export const dep = 1;".to_vec())
        .build()
        .unwrap();
    let mut expected = None;
    for workers in [1, 4, 16] {
        let program = load_no_lib_program(
            &host,
            &roots,
            options(),
            ProgramOptions::default()
                .with_no_lib(true)
                .with_identity_domain(tsc_types::IdentityDomain::new()),
            limits(workers),
        )
        .unwrap();
        let mut relative = Vec::new();
        let mut bases = [0; 4];
        let mut previous = [0; 4];
        for (ordinal, path) in roots.iter().enumerate() {
            let source = program
                .source_files()
                .iter()
                .find(|s| s.path().display() == path.to_str().unwrap())
                .unwrap();
            assert!(source.preparsed_syntax().has_direct_layout());
            let parsed = source.preparsed_syntax().take().unwrap();
            let tree = parsed.source();
            assert!(tree.arena.is_published(), "{path:?}");
            let held = tree.arena.identity_leases().unwrap();
            let ranges = [&held.nodes, &held.arrays, &held.array_items, &held.texts]
                .map(|lease| lease.range());
            if ordinal == 0 {
                bases = ranges.map(|r| r.start());
            }
            let local = tsc_syntax::parse_source_file_from_snapshot(
                source.path().display().to_owned(),
                std::sync::Arc::clone(source.snapshot()),
                parsed.parse_options().clone(),
                None,
            );
            assert!(source_files_structurally_equal(tree, &local));
            for i in 0..4 {
                if ordinal != 0 && ordinal % placement::stripe_width(roots.len()) != 0 {
                    assert_eq!(
                        ranges[i].start(),
                        previous[i],
                        "dense root {ordinal} space {i}"
                    );
                }
                previous[i] = ranges[i].end();
            }
            let normalized: [(u32, u32); 4] =
                std::array::from_fn(|i| (ranges[i].start() - bases[i], ranges[i].len()));
            relative.push(normalized);
        }
        if let Some(expected) = &expected {
            assert_eq!(&relative, expected);
        } else {
            expected = Some(relative);
        }
        let dependency = program
            .source_files()
            .iter()
            .find(|s| s.path().display() == "/p/dep.ts")
            .unwrap();
        // A source found through an import is parsed into a room of its own:
        // at its final identities, by a read-ahead worker or by the walk.
        assert!(dependency.preparsed_syntax().has_direct_layout());
        let dependency = dependency.preparsed_syntax().take().unwrap();
        assert!(
            dependency.source().arena.is_published(),
            "the discovered source lies at its final identities ({workers} workers)"
        );
    }
}

/// The sources a load finds through imports are parsed into rooms of their
/// own, at their final identities, whichever thread reaches them: a
/// read-ahead worker (four and sixteen workers) or the walk (one). A source
/// larger than a room gets a room of its size. A source no room can hold
/// (far denser than the bounds a room is leased by) keeps the private parse
/// that admission relocates, and the sources after it are placed again.
/// Which identities the placed sources get is not asserted: it depends on
/// the workers' timing.
#[test]
fn import_found_sources_are_parsed_at_their_final_identities() {
    let _command = COMMAND.lock().unwrap();
    // Larger than a room's text and, with its comments, far inside the
    // bounds its own room is leased by.
    let big: String = (0..16_000)
        .map(|i| format!("export const big{i} = {i}; // {}\n", "-".repeat(48)))
        .collect();
    assert!(big.len() > 1 << 20, "larger than a room's text");
    // A node a byte, where a room is leased for nine nodes in thirty-two
    // bytes: more nodes than a room of any source here holds.
    let dense = ";".repeat(500_000);
    let host = MemoryCompilerHost::builder("/p")
        .file(
            "/p/root.ts",
            b"import './big'; import './dense'; import './one'; import './two'; export const root = 0;"
                .to_vec(),
        )
        .file("/p/big.ts", big.into_bytes())
        .file("/p/dense.ts", dense.into_bytes())
        .file(
            "/p/one.ts",
            b"import './two'; export const one = 1;".to_vec(),
        )
        .file("/p/two.ts", b"export const two = 2;".to_vec())
        .build()
        .unwrap();
    let roots = [PathBuf::from("/p/root.ts")];
    for workers in [1, 4, 16] {
        let program = load_no_lib_program(
            &host,
            &roots,
            options(),
            ProgramOptions::default()
                .with_no_lib(true)
                .with_identity_domain(tsc_types::IdentityDomain::new()),
            ProgramLoadLimits::new(100, 1000, 100, 4 << 20, 16 << 20)
                .with_workers(WorkerBudget::new(NonZeroUsize::new(workers).unwrap())),
        )
        .unwrap();
        assert_eq!(program.source_files().len(), 5);
        let layout = program.source_files()[0]
            .preparsed_syntax()
            .direct_layout()
            .unwrap();
        assert_eq!(
            layout.len(),
            4,
            "the root and the three sources a room holds ({workers} workers)"
        );
        let mut node_ranges = Vec::new();
        for source in program.source_files() {
            let name = source.path().display().to_string_lossy().into_owned();
            let parsed = source.preparsed_syntax().take().unwrap();
            let tree = parsed.source();
            assert_eq!(
                tree.arena.is_published(),
                name != "/p/dense.ts",
                "{name} ({workers} workers)"
            );
            let local = tsc_syntax::parse_source_file_from_snapshot(
                source.path().display().to_owned(),
                std::sync::Arc::clone(source.snapshot()),
                parsed.parse_options().clone(),
                None,
            );
            assert!(source_files_structurally_equal(tree, &local), "{name}");
            if tree.arena.is_published() {
                let nodes = tree.arena.identity_leases().unwrap().nodes.range();
                node_ranges.push((nodes.start(), nodes.end()));
            }
        }
        node_ranges.sort_unstable();
        for pair in node_ranges.windows(2) {
            assert!(pair[0].1 <= pair[1].0, "two sources share node identities");
        }
    }
}

#[test]
fn interrupted_root_prefix_preserves_the_walks_limit_error() {
    let _command = COMMAND.lock().unwrap();
    let host = MemoryCompilerHost::builder("/p")
        .file("/p/a.ts", b"import './dep'; export const a = 1;".to_vec())
        .file("/p/b.ts", b"export const b = 2;".to_vec())
        .file("/p/dep.ts", b"export const d = 3;".to_vec())
        .build()
        .unwrap();
    let roots = [PathBuf::from("/p/a.ts"), PathBuf::from("/p/b.ts")];
    for workers in [1, 4] {
        let limit = ProgramLoadLimits::new(1, 100, 100, 1 << 20, 1 << 20)
            .with_workers(WorkerBudget::new(NonZeroUsize::new(workers).unwrap()));
        let ordinary = load_no_lib_program(
            &host,
            &roots,
            options(),
            ProgramOptions::default().with_no_lib(true),
            limit,
        )
        .unwrap_err();
        let direct = load_no_lib_program(
            &host,
            &roots,
            options(),
            ProgramOptions::default()
                .with_no_lib(true)
                .with_identity_domain(tsc_types::IdentityDomain::new()),
            limit,
        )
        .unwrap_err();
        assert_eq!(format!("{direct:?}"), format!("{ordinary:?}"));
    }
}
