use super::*;

#[test]
fn all_embedded_source_reads_borrow_the_validated_catalog() {
    let host = NativeCompilerHost::new(FsCompilerHost::from_process().unwrap());
    for (name, original) in embedded_libraries::EMBEDDED_LIBRARIES {
        let path = format!("{EMBEDDED_LIBRARY_DIRECTORY}/{name}");
        let bytes = host.read_file_js(path.as_str().into()).unwrap().unwrap();
        assert_eq!(bytes, original.as_bytes());
        let tsc_host::SourceRead::Text(text) = host
            .read_file_contents_js(path.as_str().into())
            .unwrap()
            .unwrap()
        else {
            panic!("embedded text was copied: {name}");
        };
        assert!(text.ptr_eq(original));
        assert!(!text.in_text_heap());
        assert!(!text.starts_with('\u{feff}'));
        let tsc_host::SourceRead::Text(parallel) = host
            .read_source_contents_js(path.as_str().into())
            .unwrap()
            .unwrap()
        else {
            panic!("parallel embedded text was copied: {name}");
        };
        assert!(text.ptr_eq(&parallel));
    }
    let custom = tsc_host::MemoryCompilerHost::builder("/p")
        .file("/p/lib.dom.d.ts", b"interface Custom {}".to_vec())
        .build()
        .unwrap();
    assert!(matches!(
        custom
            .read_file_contents_js("/p/lib.dom.d.ts".into())
            .unwrap(),
        Some(tsc_host::SourceRead::Bytes(_))
    ));
}

#[test]
fn vfs_wrappers_keep_the_embedded_source_view() {
    use tsc_host::vfs::{CachedFs, MemFs, VfsCompilerHost};
    let fs = Arc::new(CachedFs::new(BundledFs::new(MemFs::new(
        true,
        Arc::new(tsc_host::vfs::SystemClock),
    ))));
    let host = VfsCompilerHost::new(fs, "/");
    let tsc_host::SourceRead::Text(text) = host
        .read_file_contents_js("bundled:///libs/lib.dom.d.ts".into())
        .unwrap()
        .unwrap()
    else {
        panic!("VFS wrapper copied the embedded source");
    };
    assert!(text.ptr_eq(&embedded_library_text("lib.dom.d.ts").unwrap()));
}

#[test]
fn source_loading_keeps_static_text_through_each_worker_budget() {
    let host = NativeCompilerHost::new(FsCompilerHost::from_process().unwrap());
    let roots: Vec<_> = embedded_libraries::EMBEDDED_LIBRARIES
        .iter()
        .filter(|(_, text)| !text.contains("/// <reference") && !text.contains("import"))
        .take(9)
        .map(|(name, _)| PathBuf::from(format!("{EMBEDDED_LIBRARY_DIRECTORY}/{name}")))
        .collect();
    assert_eq!(roots.len(), 9);
    for workers in [1, 4, 16] {
        let limits = tsc_program::ProgramLoadLimits::new(200, 1000, 100, 4 << 20, 16 << 20)
            .with_workers(tsc_program::WorkerBudget::new(
                std::num::NonZeroUsize::new(workers).unwrap(),
            ));
        let program = tsc_program::load_no_lib_program(
            &host,
            &roots,
            tsc_types::CompilerOptions {
                no_emit: Some(true),
                ..Default::default()
            },
            tsc_program::ProgramOptions::default()
                .with_no_lib(true)
                .with_identity_domain(tsc_types::IdentityDomain::new()),
            limits,
        )
        .unwrap();
        for source in program.source_files() {
            assert!(
                !source.snapshot().shared_text().in_text_heap(),
                "{} at {workers} workers",
                source.path().display().to_string_lossy()
            );
            let name = source.path().display().to_string_lossy();
            let expected = embedded_library_text(name.rsplit('/').next().unwrap()).unwrap();
            assert!(expected.ptr_eq(&source.snapshot().shared_text()));
        }
    }
}
