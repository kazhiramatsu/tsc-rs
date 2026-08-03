use std::path::{Path, PathBuf};

use tsc_compiler::ProgramSession;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

const MINIMAL_GLOBALS: &str = r#"
interface IArguments { length: number; callee: Function; }
interface Array<T> { length: number; [index: number]: T; }
interface Object {}
interface Function {}
interface CallableFunction extends Function {}
interface NewableFunction extends Function {}
interface String {}
interface Number {}
interface Boolean {}
interface RegExp {}
"#;

#[test]
fn catalog_loaded_library_prefix_flows_through_the_owned_program_session() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"const values: number[] = [];".to_vec())
        .file(
            "/typescript/lib/lib.es5.d.ts",
            MINIMAL_GLOBALS.as_bytes().to_vec(),
        )
        .build()
        .expect("build in-memory program and library host");
    let catalog = LibraryCatalog::typescript_6_0_3("/typescript/lib");
    let prepared = load_program(
        &host,
        &[PathBuf::from("/work/root.ts")],
        CompilerOptions {
            no_emit: Some(true),
            lib: Some(vec!["es5".to_owned()]),
            ..CompilerOptions::default()
        },
        ProgramOptions::default().with_types(Vec::new()),
        &catalog,
        ProgramLoadLimits::new(8, 32, 8, 16 * 1_024, 64 * 1_024),
    )
    .expect("load catalog-backed prepared program");

    assert_eq!(prepared.library_files().len(), 1);
    assert_eq!(
        prepared.source_files()[0].path().display(),
        Path::new("/typescript/lib/lib.es5.d.ts")
    );
    assert_eq!(
        prepared.source_files()[1].path().display(),
        Path::new("/work/root.ts")
    );

    let outcome = ProgramSession::new(prepared)
        .run()
        .expect("execute owned no-emit session");
    assert!(outcome.config_diagnostics().is_empty());
    assert!(outcome.options_diagnostics().is_empty());
    assert!(outcome.global_diagnostics().is_empty());
    assert!(outcome.syntactic_diagnostics().is_empty());
    assert!(outcome.semantic_diagnostics().is_empty());
}
