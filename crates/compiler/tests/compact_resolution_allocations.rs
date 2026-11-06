//! A single-test binary keeps the allocator region independent of other tests.
use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};
use std::alloc::System;
use std::hint::black_box;
use tsc_program::{
    CompilerOptions, ModuleExtension, ModuleResolution, PathContext, PreparedProgram,
    PreparedSourceFile, ProgramPath, ResolutionKey, ResolutionMode, ResolutionOutcome,
    ResolvedModule, ResolvedModuleTarget,
};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn borrowed_module_lookup_hits_misses_and_views_allocate_nothing() {
    let path = ProgramPath::from_trusted_parts(
        "/work/a/long/importing/source/path/main.ts",
        "/work/a/long/importing/source/path/main.ts",
    )
    .unwrap();
    let mut builder = PreparedProgram::builder(
        PathContext::new(
            ProgramPath::from_trusted_parts("/work", "/work").unwrap(),
            true,
        ),
        CompilerOptions {
            no_emit: Some(true),
            ..CompilerOptions::default()
        },
    );
    let source = builder
        .add_source_file(PreparedSourceFile::new(path.clone(), ""))
        .unwrap();
    builder.add_root_file(source).unwrap();
    let specifier = "a-long-package-name/with/a-long-submodule-name";
    for mode in [ResolutionMode::CommonJs, ResolutionMode::EsNext] {
        let row = if mode == ResolutionMode::EsNext {
            ModuleResolution::resolved(ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source,
                    resolved_file: path.clone(),
                },
                ModuleExtension::Ts,
            ))
        } else {
            ModuleResolution::not_found()
        };
        builder
            .add_module_resolution(
                ResolutionKey::new(path.canonical().clone(), specifier, mode),
                Ok(row),
            )
            .unwrap();
    }
    let program = builder.build().unwrap();
    let region = Region::new(GLOBAL);
    let mut hits = 0;
    for _ in 0..128 {
        for mode in [ResolutionMode::CommonJs, ResolutionMode::EsNext] {
            let row = black_box(program.module_resolution(source, specifier.into(), mode));
            hits += usize::from(row.is_some());
            if let ResolutionOutcome::Resolved(module) = row.unwrap().outcome() {
                black_box(module.target().resolved_file());
                black_box(module.extension());
                black_box(module.package_id());
            }
            black_box(program.module_resolution(source, "missing-long-specifier".into(), mode));
            black_box(program.module_resolution_by_path(path.canonical(), specifier.into(), mode));
        }
        black_box(program.resolutions().modules().count());
    }
    let allocations = region.change();
    assert_eq!(hits, 256);
    assert_eq!(allocations.allocations, 0, "{allocations:?}");
    assert_eq!(allocations.reallocations, 0, "{allocations:?}");
}
