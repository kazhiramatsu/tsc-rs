//! A live server copies each edited text into the text heap
//! (`tsc_store::SharedText`): the replaced text must be freed once its last
//! holder drops it, so a session of edits keeps the heap's live bytes
//! bounded. Its own test binary: the probe walks the process-wide heap and
//! needs no other test allocating texts meanwhile.

use std::path::PathBuf;
use tsc_compiler::{DocumentRegistry, LiveProgram};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_program, CanonicalPath, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

const GLOBALS: &str = "interface Array<T> { length: number; }\ninterface Object {}\ninterface Function {}\ninterface String {}\ninterface Number {}\ninterface Boolean {}\ninterface RegExp {}\ninterface IArguments {}\n";

#[test]
fn a_session_of_edits_keeps_the_text_heap_bounded() {
    let base = "export const value: number = 1;\n".repeat(1_500);
    let host = MemoryCompilerHost::builder("/work")
        .file("/typescript/lib/lib.es5.d.ts", GLOBALS.as_bytes().to_vec())
        .file("/work/a.ts", base.as_bytes().to_vec())
        .build()
        .expect("build the program host");
    let prepared = load_program(
        &host,
        &[PathBuf::from("/work/a.ts")],
        CompilerOptions {
            no_emit: Some(true),
            lib: Some(vec!["es5".to_owned()]),
            ..CompilerOptions::default()
        },
        ProgramOptions::default().with_types(Vec::new()),
        &LibraryCatalog::typescript_7_1("/typescript/lib"),
        ProgramLoadLimits::new(64, 256, 16, 256 * 1_024, 1_024 * 1_024),
    )
    .expect("load the prepared program");
    let documents = DocumentRegistry::new();
    let mut live =
        LiveProgram::with_documents(prepared, &documents).expect("create the live program");
    let path = CanonicalPath::from_trusted_normalized("/work/a.ts").expect("a canonical path");
    let before = tsc_store::shared_text::text_heap_live_bytes();
    // The probe sees the program's texts: the file and the library.
    assert!(before >= base.len(), "the text heap holds {before} bytes");
    for edit in 0..20 {
        let text = format!("{base}export const edit{edit} = {edit};\n");
        live = LiveProgram::reuse(live.prepared(), &path, text, &host, &documents)
            .expect("the edit keeps the file's requests, so the program is reused")
            .expect("rebuild the program");
        documents.purge();
    }
    let after = tsc_store::shared_text::text_heap_live_bytes();
    // Unbounded, 20 edits would keep 20 copies (≈ 960 KB); bounded, the
    // live program holds one version of the file (and the libraries).
    assert!(
        after <= before + 4 * base.len(),
        "the text heap grew from {before} to {after} bytes over 20 edits of {} bytes",
        base.len()
    );
}
