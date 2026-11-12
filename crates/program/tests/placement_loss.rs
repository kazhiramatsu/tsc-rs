use std::{num::NonZeroUsize, path::PathBuf};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_no_lib_program, CompilerOptions, ProgramLoadLimits, ProgramOptions, WorkerBudget,
};
use tsc_syntax::placement;

fn options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        module: Some(199),
        module_resolution: Some(3),
        ..Default::default()
    }
}
fn limits(count: usize) -> ProgramLoadLimits {
    ProgramLoadLimits::new(100, 100, 100, 1 << 20, 1 << 20)
        .with_workers(WorkerBudget::new(NonZeroUsize::new(count).unwrap()))
}
fn program_options() -> ProgramOptions {
    ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new())
        .with_identity_domain(tsc_types::IdentityDomain::new())
}

#[test]
fn duplicate_case_aliases_reuse_the_first_tree_without_an_extra_hole() {
    let host = MemoryCompilerHost::builder("/p")
        .case_sensitive(false)
        .file("/p/a.ts", b"export const a = 1;".to_vec())
        .build()
        .unwrap();
    let roots = ["/p/a.ts", "/p/A.ts"].map(PathBuf::from);
    for count in [1, 4, 16] {
        let program =
            load_no_lib_program(&host, &roots, options(), program_options(), limits(count))
                .unwrap();
        assert_eq!(program.source_files().len(), 1);
        let prepared = &program.source_files()[0];
        let layout = prepared.preparsed_syntax().direct_layout().unwrap();
        assert_eq!(layout.len(), 1);
        let actual = prepared.preparsed_syntax().take().unwrap().into_source();
        assert!(actual.arena.is_published());
        let statistics = placement::adoption_statistics(layout, [&actual]);
        assert_eq!(
            (statistics.adopted_sources, statistics.dropped_sources),
            (1, 0)
        );
        assert_eq!(statistics.dropped_ids, [0; 4]);
    }
}

#[test]
fn import_selected_spelling_keeps_the_rejected_trees_position_pinned_and_counted() {
    let host = MemoryCompilerHost::builder("/p")
        .case_sensitive(false)
        .file("/p/main.ts", b"import './A';".to_vec())
        .file("/p/a.ts", b"export const a = 1;".to_vec())
        .build()
        .unwrap();
    let roots = ["/p/main.ts", "/p/a.ts"].map(PathBuf::from);
    for count in [1, 4, 16] {
        let program =
            load_no_lib_program(&host, &roots, options(), program_options(), limits(count))
                .unwrap();
        assert_eq!(program.source_files().len(), 2);
        let prepared = program
            .source_files()
            .iter()
            .find(|source| source.path().canonical().as_js() == "/p/a.ts")
            .unwrap();
        let layout = prepared.preparsed_syntax().direct_layout().unwrap();
        assert_eq!(layout.len(), 2);
        let original = layout
            .iter()
            .find(|placement| placement.name == "/p/a.ts")
            .unwrap();
        let planned = prepared.preparsed_syntax().take().unwrap();
        let parse_options = planned.parse_options().clone();
        let planned = planned.into_source();
        assert!(planned.arena.is_published());
        assert_ne!(
            planned.file_name.as_js(),
            prepared.path().display(),
            "adoption's filename guard rejects this tree"
        );
        drop(planned);
        let replacement = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
            prepared.path().display().to_owned(),
            std::sync::Arc::clone(prepared.snapshot()),
            parse_options,
            None,
            program.program_options().identity_domain().unwrap(),
        )
        .unwrap();
        assert!(!replacement
            .arena
            .identity_leases()
            .unwrap()
            .nodes
            .range()
            .overlaps(original.ranges()[0]));
        let main = program
            .source_files()
            .iter()
            .find(|source| source.path().display() == "/p/main.ts")
            .unwrap()
            .preparsed_syntax()
            .take()
            .unwrap()
            .into_source();
        let statistics = placement::adoption_statistics(layout, [&replacement, &main]);
        assert_eq!(
            (
                statistics.planned_sources,
                statistics.adopted_sources,
                statistics.dropped_sources
            ),
            (2, 1, 1)
        );
        assert_eq!(
            statistics.dropped_ids,
            original.ranges().map(|range| u64::from(range.len()))
        );
    }
}
