use std::path::{Path, PathBuf};

use tsc_compiler::ProgramSession;
use tsc_host::{HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    load_no_lib_program, plan_source_requests, CompilerOptions, PreparedProgram, ProgramLoadLimits,
    ProgramOptions, ProgramPath, ResolutionOutcome, ResolvedModuleTarget, UnloadedModuleReason,
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

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(1_024, 1_024, 1_024, 1_024 * 1_024, 4 * 1_024 * 1_024)
}

fn program_options() -> ProgramOptions {
    ProgramOptions::default()
        .with_no_lib(true)
        .with_types(Vec::new())
}

fn package_resolution(program: &PreparedProgram) -> &tsc_program::ResolvedModule {
    let root = program
        .source_files()
        .iter()
        .find(|source| source.path().display() == Path::new("/work/root.ts"))
        .expect("root source is owned");
    let key = plan_source_requests(root, program.compiler_options())
        .expect("plan root requests")
        .module_requests()
        .iter()
        .find(|key| key.specifier() == "pkg")
        .expect("package request exists")
        .clone();
    let resolution = program
        .resolutions()
        .require_module(&key)
        .expect("package request has an authoritative row");
    let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
        panic!("package request must resolve");
    };
    module
}

#[test]
fn memory_loader_and_session_accept_loaded_external_original_path() {
    let lexical = "/work/node_modules/pkg/index.ts";
    let physical = "/store/pkg/index.ts";
    let host = MemoryCompilerHost::builder("/work")
        .file("/lib.d.ts", MINIMAL_GLOBALS.as_bytes().to_vec())
        .file(
            "/work/root.ts",
            b"import { value } from 'pkg';\nconst checked: number = value;\n".to_vec(),
        )
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0","exports":"./index.ts"}"#.to_vec(),
        )
        .file(lexical, b"export const value = 1;\n".to_vec())
        .file(physical, b"export const value = 1;\n".to_vec())
        .realpath(lexical, physical)
        .failure(HostError::new(
            HostErrorKind::Other,
            HostOperation::Realpath,
            Some(PathBuf::from("/work/root.ts")),
            "root loading must not make a blanket realpath observation",
        ))
        .build()
        .expect("build external TypeScript realpath host");
    let prepared = load_no_lib_program(
        &host,
        &[PathBuf::from("/lib.d.ts"), PathBuf::from("/work/root.ts")],
        CompilerOptions {
            no_emit: Some(true),
            module: Some(199),
            module_resolution: Some(99),
            ..CompilerOptions::default()
        },
        program_options(),
        limits(),
    )
    .expect("load physical external TypeScript source");

    let module = package_resolution(&prepared);
    let ResolvedModuleTarget::Source {
        source,
        resolved_file,
    } = module.target()
    else {
        panic!("external TypeScript target must be loaded");
    };
    assert_eq!(resolved_file.display(), Path::new(physical));
    assert_eq!(
        prepared.source_file(*source).unwrap().path().display(),
        Path::new(physical)
    );
    assert_eq!(
        module.original_path().map(ProgramPath::display),
        Some(Path::new(lexical))
    );

    let outcome = ProgramSession::new(prepared)
        .run()
        .expect("validated originalPath projects through the checker provider");
    assert!(outcome.semantic_diagnostics().is_empty());
}

#[test]
fn memory_loader_and_session_report_physical_unloaded_javascript_paths() {
    for (physical, allow_js, expected_reason) in [
        (
            "/store/pkg/index.js",
            false,
            UnloadedModuleReason::JavaScriptNotAdmitted,
        ),
        (
            "/store/node_modules/pkg/index.js",
            true,
            UnloadedModuleReason::NodeModulesDepth,
        ),
    ] {
        let lexical = "/work/node_modules/pkg/index.js";
        let host = MemoryCompilerHost::builder("/work")
            .file("/lib.d.ts", MINIMAL_GLOBALS.as_bytes().to_vec())
            .file(
                "/work/root.ts",
                b"import { value } from 'pkg';\nvalue;\n".to_vec(),
            )
            .file(
                "/work/node_modules/pkg/package.json",
                br#"{"name":"pkg","version":"1.0.0","exports":"./index.js"}"#.to_vec(),
            )
            .file(lexical, b"exports.value = 1;\n".to_vec())
            .file(physical, b"exports.value = 1;\n".to_vec())
            .realpath(lexical, physical)
            .failure(HostError::new(
                HostErrorKind::Other,
                HostOperation::ReadFile,
                Some(PathBuf::from(physical)),
                "unloaded JavaScript must not enter source membership",
            ))
            .build()
            .expect("build external JavaScript realpath host");
        let prepared = load_no_lib_program(
            &host,
            &[PathBuf::from("/lib.d.ts"), PathBuf::from("/work/root.ts")],
            CompilerOptions {
                no_emit: Some(true),
                no_implicit_any: Some(true),
                allow_js,
                module: Some(199),
                module_resolution: Some(99),
                ..CompilerOptions::default()
            },
            program_options(),
            limits(),
        )
        .expect("retain unloaded physical JavaScript resolution");

        let module = package_resolution(&prepared);
        let ResolvedModuleTarget::Unloaded {
            resolved_file,
            reason,
        } = module.target()
        else {
            panic!("external JavaScript target must remain unloaded");
        };
        assert_eq!(resolved_file.display(), Path::new(physical));
        assert_eq!(*reason, expected_reason);
        assert_eq!(
            module.original_path().map(ProgramPath::display),
            Some(Path::new(lexical))
        );

        let outcome = ProgramSession::new(prepared)
            .run()
            .expect("unloaded originalPath projects through the checker provider");
        let diagnostics = outcome.semantic_diagnostics();
        assert_eq!(
            diagnostics
                .iter()
                .map(tsc_diagnostics::Diagnostic::code)
                .collect::<Vec<_>>(),
            [7016]
        );
        assert!(diagnostics[0].message_text().contains(physical));
    }
}
