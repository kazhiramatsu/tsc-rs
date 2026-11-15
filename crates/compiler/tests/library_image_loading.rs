//! Generated trees retain the finite Program's identities through binding.
use std::{num::NonZeroUsize, path::PathBuf, sync::Arc};
use tsc_binder::{library_image::BindingImage, Binder};
use tsc_checker::DocumentRegistry;
use tsc_compiler::{
    system::{NativeSystem, System},
    LiveProgram,
};
use tsc_program::{load_no_lib_program, ProgramLoadLimits, ProgramOptions, WorkerBudget};
use tsc_syntax::{library_image::SyntaxImage, JSDocParsingMode, ParseOptions};
use tsc_types::CompilerOptions;

#[test]
fn generated_trees_keep_snapshot_and_placed_addresses_at_each_worker_budget() {
    tsc_program::set_default_js_doc_parsing_mode(JSDocParsingMode::ParseForTypeErrors);
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs",
    );
    let mut entries: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    let roots: Vec<_> = entries
        .into_iter()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".d.ts") {
                return None;
            }
            let text = std::fs::read_to_string(entry.path()).unwrap();
            (!text.contains("/// <reference") && !text.contains("import"))
                .then(|| PathBuf::from(format!("bundled:///libs/{name}")))
        })
        .take(9)
        .collect();
    assert_eq!(roots.len(), 9);
    let system = NativeSystem::from_process().unwrap();
    let host = system.compiler_host();
    for direct in [true, false] {
        for workers in [1, 4, 16] {
            let registry = DocumentRegistry::new();
            let domain = registry.identity_domain().clone();
            let program_options = ProgramOptions::default()
                .with_no_lib(true)
                .with_types(Vec::new());
            let program_options = if direct {
                program_options.with_identity_domain(domain.clone())
            } else {
                program_options
            };
            let program = load_no_lib_program(
                &*host,
                &roots,
                CompilerOptions {
                    no_emit: Some(true),
                    ..Default::default()
                },
                program_options,
                ProgramLoadLimits::new(200, 1000, 100, 4 << 20, 16 << 20)
                    .with_workers(WorkerBudget::new(NonZeroUsize::new(workers).unwrap())),
            )
            .unwrap();
            assert_eq!(program.source_files().len(), roots.len());
            for source in program.source_files() {
                assert_eq!(
                    source.preparsed_syntax().is_available(),
                    direct,
                    "{:?}: direct={direct}, workers={workers}",
                    source.path().display()
                );
            }
            let layout = program.source_files()[0]
                .preparsed_syntax()
                .direct_layout()
                .cloned();
            assert_eq!(
                layout.as_ref().map_or(0, |layout| layout.len()),
                if direct { roots.len() } else { 0 }
            );
            let snapshots: Vec<_> = program
                .source_files()
                .iter()
                .map(|source| Arc::clone(source.snapshot()))
                .collect();
            let live = LiveProgram::with_documents(program, &registry).unwrap();
            let documents: Vec<_> = (0..roots.len())
                .map(|index| live.document(index).unwrap())
                .collect();
            for (document, snapshot) in documents.iter().zip(snapshots) {
                let actual = document.source();
                if direct {
                    assert!(actual.identity_owned_by(&domain));
                }
                assert!(Arc::ptr_eq(actual.snapshot(), &snapshot));
                assert!(!snapshot.shared_text().in_text_heap());
                assert!(std::ptr::eq(
                    actual.arena.node(actual.root),
                    tsc_syntax::published_node(actual.root)
                ));
                let mut ordinary = tsc_syntax::parse_source_file_from_snapshot(
                    actual.file_name.clone(),
                    snapshot,
                    ParseOptions {
                        js_doc_parsing_mode: JSDocParsingMode::ParseForTypeErrors,
                        ..Default::default()
                    },
                    None,
                );
                let binding = Binder::bind_local(&mut ordinary).into_bind_data();
                assert_eq!(
                    SyntaxImage::capture(actual).unwrap(),
                    SyntaxImage::capture(&ordinary).unwrap()
                );
                assert_eq!(
                    BindingImage::capture(actual, &document.data).unwrap(),
                    BindingImage::capture(&ordinary, &binding).unwrap()
                );
            }
            for placement in layout.iter().flat_map(|layout| layout.iter()) {
                assert!(documents
                    .iter()
                    .any(|document| placement.matches(document.source())));
            }
        }
    }
}
