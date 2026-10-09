//! P5-1a: a [`LiveProgram`] keeps a Program and its checker between
//! requests. Its diagnostics, asked file by file, equal the batch session's
//! (the native harness run, which checks every file), and a query over its
//! checker equals the batch harness walk.

use std::path::PathBuf;

use tsc_checker::type_writer::{self, TypeWriterLine};
use tsc_compiler::{LiveProgram, NativeHarnessCollection, ProgramSession};
use tsc_diagnostics::{sort_and_dedupe_diagnostics, Diagnostic};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_program, CompilerOptions, LibraryCatalog, PreparedProgram, ProgramLoadLimits,
    ProgramOptions,
};

const MINIMAL_GLOBALS: &str = r#"
interface IArguments { length: number; callee: Function; }
interface Array<T> { length: number; [index: number]: T; push(...items: T[]): number; }
interface Object {}
interface Function {}
interface CallableFunction extends Function {}
interface NewableFunction extends Function {}
interface String { readonly length: number; }
interface Number {}
interface Boolean {}
interface RegExp {}
"#;

fn prepared(files: &[(&str, &str)], options: CompilerOptions) -> PreparedProgram {
    prepared_with_lib(files, options, true)
}

fn prepared_with_lib(
    files: &[(&str, &str)],
    options: CompilerOptions,
    with_lib: bool,
) -> PreparedProgram {
    prepared_with_links(files, &[], options, with_lib)
}

/// `links` are `(target, link)` directory symlinks (the harness's
/// `@link: target -> link`): every file under the target is also read
/// through the link, whose real path is the target's.
fn prepared_with_links(
    files: &[(&str, &str)],
    links: &[(&str, &str)],
    options: CompilerOptions,
    with_lib: bool,
) -> PreparedProgram {
    let mut builder = MemoryCompilerHost::builder("/work").file(
        "/typescript/lib/lib.es5.d.ts",
        MINIMAL_GLOBALS.as_bytes().to_vec(),
    );
    for (name, text) in files {
        builder = builder.file(*name, text.as_bytes().to_vec());
    }
    for (target, link) in links {
        builder = builder.realpath(*link, *target);
        for (name, text) in files {
            if let Some(relative) = name.strip_prefix(&format!("{target}/")) {
                let linked = format!("{link}/{relative}");
                builder = builder
                    .file(&linked, text.as_bytes().to_vec())
                    .realpath(linked, *name);
            }
        }
    }
    let host = builder.build().expect("build the program host");
    let roots = files
        .iter()
        .filter(|(name, _)| !name.ends_with(".json"))
        .map(|(name, _)| PathBuf::from(name))
        .collect::<Vec<_>>();
    let program_options = ProgramOptions::default().with_types(Vec::new());
    load_program(
        &host,
        &roots,
        CompilerOptions {
            no_emit: Some(true),
            lib: with_lib.then(|| vec!["es5".to_owned()]),
            ..options
        },
        if with_lib {
            program_options
        } else {
            program_options.with_no_lib(true)
        },
        &LibraryCatalog::typescript_7_1("/typescript/lib"),
        ProgramLoadLimits::new(64, 256, 16, 256 * 1_024, 1_024 * 1_024),
    )
    .expect("load the prepared program")
}

fn sorted(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    sort_and_dedupe_diagnostics(&mut diagnostics);
    diagnostics
}

/// The live Program's diagnostics of every kind, every file asked in
/// Program order (libraries first, as the batch session checks them).
struct LiveDiagnostics {
    config: Vec<Diagnostic>,
    program: Vec<Diagnostic>,
    syntactic: Vec<Diagnostic>,
    global: Vec<Diagnostic>,
    semantic: Vec<Diagnostic>,
}

fn live_diagnostics(prepared: PreparedProgram) -> LiveDiagnostics {
    let mut live = LiveProgram::new(prepared).expect("create the live program");
    let mut syntactic = Vec::new();
    let mut semantic = Vec::new();
    for file in 0..live.file_count() {
        syntactic.extend(live.syntactic_diagnostics(file));
    }
    // The batch session checks the Program's sources, then completes the
    // library prefix.
    let lib_count = live.prepared().library_files().len();
    let order = (lib_count..live.file_count()).chain(0..lib_count);
    for file in order {
        semantic.extend(
            live.semantic_diagnostics(file)
                .expect("check the file on demand"),
        );
    }
    LiveDiagnostics {
        config: live.config_file_parsing_diagnostics().to_vec(),
        program: live.program_diagnostics(),
        syntactic: sorted(syntactic),
        global: live.global_diagnostics(),
        semantic: sorted(semantic),
    }
}

/// A named Program: its files, its options and whether it has the library.
type ProgramCase<'a> = (&'a str, Vec<(&'a str, &'a str)>, CompilerOptions, bool);

#[test]
fn live_diagnostics_equal_the_batch_session() {
    let cases: Vec<ProgramCase<'_>> = vec![
        (
            "type errors across imports",
            vec![
                (
                    "/work/a.ts",
                    "import { b } from './b';\nexport const a: string = b;\nconst c: number = 'c';\n",
                ),
                (
                    "/work/b.ts",
                    "export const b = 1;\nexport function f(x: number) { return x.length; }\n",
                ),
            ],
            CompilerOptions::default(),
            true,
        ),
        (
            "missing globals without a library",
            vec![("/work/a.ts", "const values = [1, 2];\nexport {};\n")],
            CompilerOptions::default(),
            false,
        ),
        (
            "parse errors",
            vec![
                ("/work/a.ts", "const x = ;\nlet y: number = 'y';\n"),
                ("/work/b.ts", "function (\n"),
            ],
            CompilerOptions::default(),
            true,
        ),
        (
            "checked JavaScript",
            vec![
                ("/work/a.js", "/** @type {number} */\nconst n = 'n';\nmodule.exports = n;\n"),
                ("/work/b.ts", "const s: string = 1;\nexport {};\n"),
            ],
            CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
            true,
        ),
        (
            "an options row and semantic rows",
            vec![("/work/a.ts", "const z: boolean = 0;\nexport {};\n")],
            CompilerOptions {
                base_url: Some("/work".to_owned().into()),
                ..CompilerOptions::default()
            },
            true,
        ),
        (
            "a skipped declaration file",
            vec![
                ("/work/types.d.ts", "declare const q: number = 'q';\n"),
                ("/work/a.ts", "const r: string = q;\nexport {};\n"),
            ],
            CompilerOptions {
                skip_lib_check: Some(true),
                ..CompilerOptions::default()
            },
            true,
        ),
    ];
    for (name, files, options, with_lib) in cases {
        let prepared = prepared_with_lib(&files, options, with_lib);
        let batch = ProgramSession::new(prepared.clone())
            .run_for_native_harness(NativeHarnessCollection {
                capture_suggestions: false,
            })
            .expect("run the batch session");
        let live = live_diagnostics(prepared);
        assert_eq!(live.config, batch.config_diagnostics(), "{name}: config");
        assert_eq!(
            live.syntactic,
            batch.syntactic_diagnostics(),
            "{name}: syntactic"
        );
        // The batch outcome's getters gate the options, global and semantic
        // rows behind the earlier kinds (getPreEmitDiagnostics); a Program's
        // getters do not. The native harness's union is ungated.
        let mut union = live.config.clone();
        union.extend(live.program.iter().cloned());
        union.extend(live.syntactic.iter().cloned());
        union.extend(live.semantic.iter().cloned());
        union.extend(live.global.iter().cloned());
        assert_eq!(
            sorted(union),
            batch.native_harness_diagnostics(),
            "{name}: every kind"
        );
        if live.syntactic.is_empty() {
            assert_eq!(live.program, batch.options_diagnostics(), "{name}: program");
            assert_eq!(live.global, batch.global_diagnostics(), "{name}: global");
        }
        assert!(
            !(live.syntactic.is_empty() && live.semantic.is_empty() && live.global.is_empty()),
            "{name}: the case reports something"
        );
    }
}

#[test]
fn live_queries_equal_the_batch_walk() {
    let files = [
        (
            "/work/a.ts",
            "import { make } from './b';\nconst box = make(1);\nexport const value = box.value + 1;\nexport type Pair<T> = [T, T];\nconst pair: Pair<string> = ['x', 'y'];\n",
        ),
        (
            "/work/b.ts",
            "export interface Box<T> { value: T }\nexport function make<T>(value: T): Box<T> { return { value }; }\n",
        ),
    ];
    let prepared = prepared(&files, CompilerOptions::default());
    let mut batch_lines: Vec<(Vec<TypeWriterLine>, Vec<TypeWriterLine>)> = Vec::new();
    let mut walk = |snapshot: &tsc_checker::program::ProgramSnapshot,
                    session: &tsc_checker::emit::CheckerSession<'_>| {
        for (file, document) in snapshot.documents().iter().enumerate() {
            if !document.source().file_name.as_js().starts_with("/work/") {
                continue;
            }
            batch_lines.push(
                session
                    .with_state_for_harness(|state| {
                        Ok::<_, String>((
                            type_writer::write_types(state, file)?,
                            type_writer::write_symbols(state, file)?,
                        ))
                    })
                    .expect("walk the batch session"),
            );
        }
    };
    ProgramSession::new(prepared.clone())
        .run_for_native_harness_with_walk(
            NativeHarnessCollection {
                capture_suggestions: false,
            },
            &mut walk,
        )
        .expect("run the batch session");
    assert_eq!(batch_lines.len(), 2);

    // Unchecked: the queries resolve what they need.
    let mut live = LiveProgram::new(prepared.clone()).expect("create the live program");
    let mut live_lines = Vec::new();
    for file in 0..live.file_count() {
        if !live
            .file_name(file)
            .is_some_and(|name| name.starts_with("/work/"))
        {
            continue;
        }
        live_lines.push(live.with_checker(|state| {
            (
                type_writer::write_types(state, file).expect("write the types"),
                type_writer::write_symbols(state, file).expect("write the symbols"),
            )
        }));
    }
    assert_eq!(live_lines, batch_lines, "queries before any check");

    // Checked first, as the batch walk is.
    let mut live = LiveProgram::new(prepared).expect("create the live program");
    for file in 0..live.file_count() {
        live.semantic_diagnostics(file).expect("check the file");
    }
    let mut live_lines = Vec::new();
    for file in 0..live.file_count() {
        if !live
            .file_name(file)
            .is_some_and(|name| name.starts_with("/work/"))
        {
            continue;
        }
        live_lines.push(live.with_checker(|state| {
            (
                type_writer::write_types(state, file).expect("write the types"),
                type_writer::write_symbols(state, file).expect("write the symbols"),
            )
        }));
    }
    assert_eq!(live_lines, batch_lines, "queries after every check");
}

/// A module reached through a symlinked package is printed by the link's
/// package name with no declaration emit before the query: tsgo's node
/// builder takes the module specifiers from the Program, which knows its
/// symlinks (nodebuilder.go:285, `Program.GetSymlinkCache`). The lines are
/// tsgo's `symlinkedWorkspaceDependenciesNoDirectLinkGeneratesNonrelativeName.types`.
#[test]
fn a_printed_type_names_a_module_through_the_programs_symlinks() {
    let files = [
        (
            "/work/packageA/index.d.ts",
            "export declare class Foo {\n    private f: any;\n}\n",
        ),
        (
            "/work/packageB/package.json",
            r#"{ "private": true, "dependencies": { "package-a": "file:../packageA" } }"#,
        ),
        (
            "/work/packageB/index.d.ts",
            "import { Foo } from \"package-a\";\nexport declare function invoke(): Foo;\n",
        ),
        (
            "/work/packageC/package.json",
            r#"{ "private": true, "dependencies": { "package-b": "file:../packageB", "package-a": "file:../packageA" } }"#,
        ),
        (
            "/work/packageC/index.ts",
            "import * as pkg from \"package-b\";\n\nexport const a = pkg.invoke();\n",
        ),
    ];
    let links = [
        ("/work/packageA", "/work/packageC/node_modules/package-a"),
        ("/work/packageA", "/work/packageB/node_modules/package-a"),
        ("/work/packageB", "/work/packageC/node_modules/package-b"),
    ];
    let prepared = prepared_with_links(
        &files,
        &links,
        CompilerOptions {
            module: Some(1),
            ..CompilerOptions::default()
        },
        true,
    );
    let unit = "/work/packageC/index.ts";
    // The batch walk: `noEmit`, so no declaration emit precedes it.
    let mut batch_lines = Vec::new();
    let mut walk = |snapshot: &tsc_checker::program::ProgramSnapshot,
                    session: &tsc_checker::emit::CheckerSession<'_>| {
        let file = snapshot
            .documents()
            .iter()
            .position(|document| document.source().file_name.to_string_lossy() == unit)
            .expect("the unit is in the Program");
        batch_lines.push(
            session
                .with_state_for_harness(|state| type_writer::write_types(state, file))
                .expect("walk the batch session"),
        );
    };
    ProgramSession::new(prepared.clone())
        .run_for_native_harness_with_walk(
            NativeHarnessCollection {
                capture_suggestions: false,
            },
            &mut walk,
        )
        .expect("run the batch session");

    let mut live = LiveProgram::new(prepared).expect("create the live program");
    let file = live.file_index(unit).expect("the unit is in the Program");
    let live_lines = live
        .with_checker(|state| type_writer::write_types(state, file))
        .expect("write the types");
    let printed = live_lines
        .iter()
        .map(|line| format!(">{} : {}", line.source_text, line.text))
        .collect::<Vec<_>>();
    assert_eq!(
        printed,
        [
            ">pkg : typeof pkg",
            ">a : import(\"package-a\").Foo",
            ">pkg.invoke() : import(\"package-a\").Foo",
            ">pkg.invoke : () => import(\"package-a\").Foo",
            ">pkg : typeof pkg",
            ">invoke : () => import(\"package-a\").Foo",
        ]
    );
    assert_eq!(batch_lines, [live_lines]);
}

/// A CommonJS file that imports its own package dynamically does not reuse
/// that ESM-mode specifier for a CommonJS-mode one: tsgo asks the Program
/// for the existing import's usage mode (`GetModeForUsageLocation`). The
/// lines are tsgo's
/// `nodeModulesDeclarationEmitDynamicImportWithPackageExports(module=nodenext).types`.
#[test]
fn a_printed_type_does_not_reuse_an_import_of_another_mode() {
    let files = [
        (
            "/work/package.json",
            r#"{ "name": "package", "private": true, "type": "module", "exports": { "./cjs": "./index.cjs", "./mjs": "./index.mjs", ".": "./index.js" } }"#,
        ),
        ("/work/index.cts", "// cjs format file\nexport {};\n"),
        (
            "/work/other.cts",
            "// cjs format file, no TLA\nexport const a = import(\"package/cjs\");\n",
        ),
        ("/work/promise.d.ts", "interface Promise<T> { value: T; }\n"),
    ];
    let prepared = prepared(
        &files,
        CompilerOptions {
            module: Some(199),
            ..CompilerOptions::default()
        },
    );
    let mut live = LiveProgram::new(prepared).expect("create the live program");
    let file = live
        .file_index("/work/other.cts")
        .expect("the file is in the Program");
    let printed = live
        .with_checker(|state| type_writer::write_types(state, file))
        .expect("write the types")
        .iter()
        .map(|line| format!(">{} : {}", line.source_text, line.text))
        .collect::<Vec<_>>();
    assert_eq!(
        printed,
        [
            ">a : Promise<{ default: typeof import(\"./index.cjs\"); }>",
            ">import(\"package/cjs\") : Promise<{ default: typeof import(\"./index.cjs\"); }>",
            ">\"package/cjs\" : \"package/cjs\"",
        ]
    );
}

/// A file's semantic rows are those the checker holds when the file is
/// asked (tsgo `Checker.GetDiagnostics`), so checking a later file can add
/// rows to an earlier one. A global interface merged with a library
/// declaration has its duplicate index signatures checked with the first
/// declaration (`checkTypeForDuplicateIndexSignatures`): the script's row
/// appears once the library is checked. The batch run collects after every
/// file is checked (duplicateNumericIndexers.errors.txt).
#[test]
fn a_file_asked_again_reports_the_rows_a_later_check_added() {
    let prepared = prepared(
        &[(
            "/work/a.ts",
            "interface Array<T> {\n    [x: number]: T;\n}\n",
        )],
        CompilerOptions::default(),
    );
    let codes = |rows: &[Diagnostic]| rows.iter().map(Diagnostic::code).collect::<Vec<_>>();
    let batch = ProgramSession::new(prepared.clone())
        .run_for_native_harness(NativeHarnessCollection {
            capture_suggestions: false,
        })
        .expect("run the batch session");
    assert_eq!(codes(batch.native_harness_diagnostics()), [2374, 2374]);

    let mut live = LiveProgram::new(prepared).expect("create the live program");
    let script = live.file_index("/work/a.ts").expect("the script");
    let library = live
        .file_index("/typescript/lib/lib.es5.d.ts")
        .expect("the library");
    let first = live.semantic_diagnostics(script).expect("check the script");
    assert_eq!(codes(&first), [0_u32; 0]);
    let library_rows = live
        .semantic_diagnostics(library)
        .expect("check the library");
    assert_eq!(codes(&library_rows), [2374]);
    let again = live.semantic_diagnostics(script).expect("ask again");
    assert_eq!(codes(&again), [2374]);
}

#[test]
fn a_live_program_moves_between_threads() {
    fn assert_send<T: Send>() {}
    assert_send::<LiveProgram>();
    let prepared = prepared(
        &[("/work/a.ts", "const n: number = 's';\nexport {};\n")],
        CompilerOptions::default(),
    );
    let mut live = LiveProgram::new(prepared).expect("create the live program");
    let file = live
        .file_index("/work/a.ts")
        .expect("the file is in the Program");
    let diagnostics = std::thread::spawn(move || live.semantic_diagnostics(file))
        .join()
        .expect("the thread finishes")
        .expect("check the file");
    assert_eq!(
        diagnostics.iter().map(Diagnostic::code).collect::<Vec<_>>(),
        [2322]
    );
}
