use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tsc_compiler::{
    DriverError, EmitArtifact, EmitFailure, EmitFileSystem, EmitIoError, EmitWriteDisposition,
    FsOutputSink, MemoryOutputSink, NativeHarnessCollection, OutputSink, ProgramSession,
};
use tsc_diagnostics::Diagnostic;
use tsc_program::ResolutionMode;
use tsc_program::{
    CompilerOptions, ModuleExtension, ModuleResolution, PathContext, PathMapping, PreparedProgram,
    PreparedSourceFile, ProgramOptions, ProgramPath, ResolutionKey, ResolvedModule,
    ResolvedModuleTarget, SourceFileId,
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

#[derive(Default)]
struct CountingSink {
    writes: usize,
}

struct InjectedFileSystem {
    fail_path: PathBuf,
    attempts: Vec<PathBuf>,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl EmitFileSystem for InjectedFileSystem {
    fn write_file(
        &mut self,
        path: tsc_diagnostics::JsStr<'_>,
        bytes: &[u8],
    ) -> Result<(), tsc_diagnostics::JsString> {
        let path = std::path::Path::new(path.as_str().expect("scalar fault-injection path"));
        (|| -> Result<(), String> {
            self.attempts.push(path.to_path_buf());
            if path == self.fail_path {
                return Err("injected stable write failure".to_owned());
            }
            self.files.insert(path.to_path_buf(), bytes.to_vec());
            Ok(())
        })()
        .map_err(Into::into)
    }

    fn create_directory(
        &mut self,
        path: tsc_diagnostics::JsStr<'_>,
    ) -> Result<(), tsc_diagnostics::JsString> {
        let path = std::path::Path::new(path.as_str().expect("scalar fault-injection path"));
        panic!(
            "existing project parent must not be created: {}",
            path.display()
        )
    }

    fn directory_exists(&mut self, path: tsc_diagnostics::JsStr<'_>) -> bool {
        let path = std::path::Path::new(path.as_str().expect("scalar fault-injection path"));

        path == Path::new("/project")
    }
}

impl OutputSink for CountingSink {
    fn write(&mut self, _artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.writes += 1;
        Ok(EmitWriteDisposition::Written)
    }
}

fn path(value: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(value, value).expect("trusted test path")
}

fn prepared_for_emit() -> PreparedProgram {
    prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(200),
            ..CompilerOptions::default()
        },
        &[("/project/input.ts", "export const value: number = 1;\n")],
    )
}

fn prepared_with_sources(options: CompilerOptions, sources: &[(&str, &str)]) -> PreparedProgram {
    let mut builder =
        PreparedProgram::emitting_builder(PathContext::new(path("/project"), true), options);
    for (file_name, text) in sources {
        let source = builder
            .add_source_file(PreparedSourceFile::new(path(file_name), *text))
            .expect("add source");
        builder.add_root_file(source).expect("add root");
    }
    builder.build().expect("prepared program")
}

fn prepared_with_sources_and_minimal_lib(
    options: CompilerOptions,
    sources: &[(&str, &str)],
) -> PreparedProgram {
    let mut builder =
        PreparedProgram::emitting_builder(PathContext::new(path("/project"), true), options);
    let library = builder
        .add_source_file(PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS))
        .expect("add minimal library");
    builder
        .add_library_file(library)
        .expect("register minimal library");
    for (file_name, text) in sources {
        let source = builder
            .add_source_file(PreparedSourceFile::new(path(file_name), *text))
            .expect("add source");
        builder.add_root_file(source).expect("add root");
    }
    builder
        .build()
        .expect("prepared program with minimal library")
}

fn prepared_with_package_import(options: CompilerOptions, input: &str) -> PreparedProgram {
    let mut builder =
        PreparedProgram::emitting_builder(PathContext::new(path("/project"), true), options);
    let input_id = builder
        .add_source_file(PreparedSourceFile::new(path("/project/input.ts"), input))
        .expect("add package-import root");
    let package_id = builder
        .add_source_file(PreparedSourceFile::new(
            path("/project/pkg.d.ts"),
            "declare const value: unknown;\nexport = value;\n",
        ))
        .expect("add package declaration");
    builder
        .add_root_file(input_id)
        .expect("add package-import root file");
    for mode in [ResolutionMode::Unspecified, ResolutionMode::CommonJs] {
        builder
            .add_module_resolution(
                ResolutionKey::new(path("/project/input.ts").canonical().clone(), "pkg", mode),
                Ok(source_resolution(
                    package_id,
                    "/project/pkg.d.ts",
                    ModuleExtension::Dts,
                )),
            )
            .expect("add package resolution");
    }
    builder.build().expect("prepared package-import program")
}

fn prepared_with_jsx_package_import(options: CompilerOptions, input: &str) -> PreparedProgram {
    let mut builder =
        PreparedProgram::emitting_builder(PathContext::new(path("/project"), true), options);
    let library = builder
        .add_source_file(PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS))
        .expect("add minimal library");
    builder
        .add_library_file(library)
        .expect("register minimal library");
    let input_id = builder
        .add_source_file(PreparedSourceFile::new(path("/project/view.tsx"), input))
        .expect("add JSX package-import root");
    let package_id = builder
        .add_source_file(PreparedSourceFile::new(
            path("/project/react.d.ts"),
            "declare const React: any;\nexport default React;\n",
        ))
        .expect("add React declaration");
    builder
        .add_root_file(input_id)
        .expect("add JSX package-import root file");
    for mode in [ResolutionMode::Unspecified, ResolutionMode::CommonJs] {
        builder
            .add_module_resolution(
                ResolutionKey::new(path("/project/view.tsx").canonical().clone(), "react", mode),
                Ok(source_resolution(
                    package_id,
                    "/project/react.d.ts",
                    ModuleExtension::Dts,
                )),
            )
            .expect("add React package resolution");
    }
    builder
        .build()
        .expect("prepared JSX package-import program")
}

fn prepared_with_owned_sources(
    options: CompilerOptions,
    sources: Vec<PreparedSourceFile>,
) -> PreparedProgram {
    let mut builder =
        PreparedProgram::emitting_builder(PathContext::new(path("/project"), true), options);
    for source in sources {
        let source = builder.add_source_file(source).expect("add source");
        builder.add_root_file(source).expect("add root");
    }
    builder.build().expect("prepared program")
}

fn source_resolution(
    source: SourceFileId,
    resolved_file: &str,
    extension: ModuleExtension,
) -> ModuleResolution {
    ModuleResolution::resolved(ResolvedModule::new(
        ResolvedModuleTarget::Source {
            source,
            resolved_file: path(resolved_file),
        },
        extension,
    ))
}

fn empty_no_emit_program() -> PreparedProgram {
    PreparedProgram::builder(
        PathContext::new(path("/project"), true),
        CompilerOptions {
            no_emit: Some(true),
            ..CompilerOptions::default()
        },
    )
    .build()
    .expect("empty no-emit program")
}

fn empty_emit_program() -> PreparedProgram {
    PreparedProgram::emitting_builder(
        PathContext::new(path("/project"), true),
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(200),
            list_emitted_files: Some(true),
            ..CompilerOptions::default()
        },
    )
    .build()
    .expect("empty emit program")
}

#[test]
fn h1_4_emit_entry_runs_the_checked_transform_and_memory_sink_path() {
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared_for_emit())
        .emit(&mut sink)
        .expect("H1.4 checked JavaScript emit");

    assert!(!outcome.emit_skipped());
    assert!(outcome.diagnostics().is_empty());
    assert!(outcome.emitted_files().is_none());
    assert!(outcome.source_maps().is_none());
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].path().scalar_test_path(),
        std::path::Path::new("/project/input.js")
    );
    assert_eq!(
        sink.writes()[0].callback_text(),
        "export const value = 1;\n"
    );
    assert!(!sink.writes()[0].write_byte_order_mark());
}

#[test]
fn a_program_emit_writes_no_build_info_like_compiler_program_emit() {
    // tsgo's `compiler.Program.Emit` writes the JavaScript and declarations
    // of an `incremental` or `composite` program; only the command's
    // incremental program adds the build info
    // (execute/incremental/program.go:243-273).
    for (incremental, composite) in [(Some(true), None), (None, Some(true))] {
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(99),
                incremental,
                composite,
                ..CompilerOptions::default()
            },
            &[("/project/input.ts", "export const value: number = 1;\n")],
        );
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("Program emit");
        assert!(!outcome.emit_skipped());
        let written: Vec<_> = sink
            .writes()
            .iter()
            .map(|artifact| artifact.path().scalar_test_path().to_path_buf())
            .collect();
        let mut expected = vec![PathBuf::from("/project/input.js")];
        if composite.is_some() {
            expected.push(PathBuf::from("/project/input.d.ts"));
        }
        assert_eq!(written, expected);
    }
}

#[test]
fn h2_1a_omitted_and_explicit_esnext_select_the_exact_esm_path() {
    for module in [None, Some(99)] {
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module,
                ..CompilerOptions::default()
            },
            &[("/project/input.ts", "export const value: number = 1;\n")],
        );
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.1a ESNext emit");
        assert!(outcome.diagnostics().is_empty());
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(
            sink.writes()[0].callback_text(),
            "export const value = 1;\n"
        );
    }
}

#[test]
fn h2_1b_explicit_and_implied_commonjs_select_the_exact_path() {
    let cases = [
        (
            Some(1),
            PreparedSourceFile::new(
                path("/project/commonjs.ts"),
                "export const value: number = 1;\n",
            ),
        ),
        (
            Some(99),
            PreparedSourceFile::new(
                path("/project/package-commonjs.ts"),
                "export const value: number = 1;\n",
            )
            .with_implied_node_formats(
                Some(ResolutionMode::CommonJs),
                Some(ResolutionMode::CommonJs),
            ),
        ),
    ];
    for (module, source) in cases {
        let prepared = prepared_with_owned_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module,
                ..CompilerOptions::default()
            },
            vec![source],
        );
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.1b CommonJS emit");
        assert!(outcome.diagnostics().is_empty());
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(
            sink.writes()[0].callback_text(),
            concat!(
                "\"use strict\";\n",
                "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
                "exports.value = void 0;\n",
                "exports.value = 1;\n",
            )
        );
    }
}

#[test]
fn commonjs_erased_final_import_retains_statement_list_tail_comments() {
    let options = CompilerOptions {
        no_emit: Some(false),
        target: Some(2),
        module: Some(1),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut builder =
        PreparedProgram::emitting_builder(PathContext::new(path("/project"), true), options);
    let dependency = builder
        .add_source_file(PreparedSourceFile::new(
            path("/project/a.ts"),
            "export default 0;\n",
        ))
        .expect("add import dependency");
    let importer = builder
        .add_source_file(PreparedSourceFile::new(
            path("/project/b.ts"),
            concat!(
                "import unused from \"./a\";\n",
                "\n",
                "// statement-list tail after erased import\n",
            ),
        ))
        .expect("add importer");
    builder
        .add_root_file(dependency)
        .expect("add dependency root");
    builder.add_root_file(importer).expect("add importer root");
    for mode in [ResolutionMode::Unspecified, ResolutionMode::CommonJs] {
        builder
            .add_module_resolution(
                ResolutionKey::new(path("/project/b.ts").canonical().clone(), "./a", mode),
                Ok(source_resolution(
                    dependency,
                    "/project/a.ts",
                    ModuleExtension::Ts,
                )),
            )
            .expect("add authoritative import resolution");
    }

    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(builder.build().expect("prepared erased-import program"))
        .emit(&mut sink)
        .expect("CommonJS erased-import emit");
    assert!(outcome.diagnostics().is_empty());
    let importer_output = sink
        .writes()
        .iter()
        .find(|write| write.path().scalar_test_path() == Path::new("/project/b.js"))
        .expect("b.js output");
    assert_eq!(
        importer_output.callback_text(),
        concat!(
            "\"use strict\";\n",
            "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "// statement-list tail after erased import\n",
        ),
    );
}

#[test]
fn module_kind_none_is_unspecified_and_follows_the_target() {
    // TypeScript 7.1 has no `module: none`: 0 is the unspecified value, so
    // target ES2015 selects ES2015 module emit.
    let prepared = prepared_with_sources_and_minimal_lib(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(2),
            module: Some(0),
            ..CompilerOptions::default()
        },
        &[("/project/input.ts", "export const value: number = 1;\n")],
    );
    let mut sink = MemoryOutputSink::new();
    let (_, diagnostics) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .expect("module=0 emit");

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].callback_text(),
        "export const value = 1;\n"
    );
}

#[test]
fn paths_option_diagnostics_restore_the_emit_report_semantic_gate() {
    let options = CompilerOptions {
        no_emit: Some(false),
        target: Some(2),
        module: Some(1),
        ..CompilerOptions::default()
    };
    let mut builder =
        PreparedProgram::emitting_builder(PathContext::new(path("/project"), true), options);
    builder.set_program_options(
        ProgramOptions::default()
            .with_paths(vec![PathMapping::new("*", vec!["bare".to_owned().into()])]),
    );
    let library = builder
        .add_source_file(PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS))
        .expect("add minimal library");
    builder
        .add_library_file(library)
        .expect("register minimal library");
    let input = builder
        .add_source_file(PreparedSourceFile::new(
            path("/project/input.ts"),
            "import \"someModule\";\n",
        ))
        .expect("add side-effect import root");
    builder
        .add_root_file(input)
        .expect("add side-effect import root file");
    for mode in [ResolutionMode::Unspecified, ResolutionMode::CommonJs] {
        builder
            .add_module_resolution(
                ResolutionKey::new(
                    path("/project/input.ts").canonical().clone(),
                    "someModule",
                    mode,
                ),
                Ok(ModuleResolution::not_found()),
            )
            .expect("add authoritative side-effect import miss");
    }

    let mut sink = MemoryOutputSink::new();
    let (_, diagnostics) = ProgramSession::new(builder.build().expect("prepared paths program"))
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .expect("emit with paths option diagnostic");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5090]
    );
    assert_eq!(sink.writes().len(), 1);
}

#[test]
fn commonjs_namespace_import_does_not_consume_the_generated_module_binding() {
    let prepared = prepared_with_package_import(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(2),
            module: Some(1),
            always_strict: Some(false),
            ..CompilerOptions::default()
        },
        concat!(
            "import * as pkg from \"pkg\";\n",
            "import { value } from \"pkg\";\n",
            "pkg;\n",
            "value;\n",
        ),
    );
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .expect("CommonJS namespace and named imports emit");
    let text = sink.writes()[0].callback_text();

    assert!(text.contains("const pkg = __importStar(require(\"pkg\"));\n"));
    assert!(text.contains("const pkg_1 = require(\"pkg\");\n"));
    assert!(!text.contains("const pkg_2 ="));
}

#[test]
fn commonjs_export_star_requests_its_complete_helper_dependency_graph() {
    let prepared = prepared_with_package_import(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(2),
            module: Some(1),
            always_strict: Some(false),
            ..CompilerOptions::default()
        },
        "export * from \"pkg\";\n",
    );
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .expect("CommonJS export-star emit");
    let text = sink.writes()[0].callback_text();

    let create_binding = text
        .find("var __createBinding =")
        .expect("create-binding dependency");
    let export_star = text.find("var __exportStar =").expect("export-star helper");
    let call = text
        .find("__exportStar(require(\"pkg\"), exports);")
        .expect("export-star call");
    assert!(create_binding < export_star && export_star < call);
}

#[test]
fn commonjs_export_list_preserves_hoisted_function_initialization() {
    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(2),
            module: Some(1),
            always_strict: Some(false),
            ..CompilerOptions::default()
        },
        &[(
            "/project/input.ts",
            concat!(
                "function predicate(value: unknown) {}\n",
                "export { predicate };\n"
            ),
        )],
    );
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .expect("CommonJS export-list function emit");
    let text = sink.writes()[0].callback_text();

    let initialization = text
        .find("exports.predicate = predicate;")
        .expect("hoisted function export initialization");
    let declaration = text
        .find("function predicate(value) {")
        .expect("function declaration");
    assert!(initialization < declaration);
    assert!(!text.contains("exports.predicate = void 0;"));
}

#[test]
fn commonjs_namespace_initializers_follow_the_checker_export_owner() {
    // tsgo's CommonJS transform keeps a namespace's declaration name local
    // even when an exported interface makes the checker own it
    // (commonjsmodule.go:2064-2068).
    let cases = [
        (
            concat!(
                "export default function Foo() {}\n",
                "namespace Foo { export var x; }\n",
                "interface Foo {}\n",
                "export interface Foo {}\n",
            ),
            ")(Foo || (Foo = {}));",
        ),
        (
            concat!(
                "export default function Foo() {}\n",
                "namespace Foo { export var x; }\n",
            ),
            ")(Foo || (exports.Foo = Foo = {}));",
        ),
        (
            concat!("export {};\n", "namespace Local { export var y; }\n",),
            ")(Local || (Local = {}));",
        ),
        (
            concat!(
                "export function Bar() {}\n",
                "namespace Bar { export var x; }\n",
            ),
            ")(Bar || (exports.Bar = Bar = {}));",
        ),
    ];

    for (source, expected_initializer) in cases {
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(2),
                module: Some(1),
                always_strict: Some(false),
                ..CompilerOptions::default()
            },
            &[("/project/input.ts", source)],
        );
        let mut sink = MemoryOutputSink::new();
        ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("CommonJS namespace export-owner emit");
        let text = sink.writes()[0].callback_text();

        assert!(text.contains(expected_initializer), "{text}");
    }
}

#[test]
fn amd_marker_does_not_borrow_comments_from_an_erased_ambient_module() {
    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(2),
            module: Some(2),
            always_strict: Some(false),
            ..CompilerOptions::default()
        },
        &[(
            "/project/input.ts",
            concat!(
                "export {};\n",
                "// augmentation belongs only to erased TypeScript syntax\n",
                "declare namespace TypesOnly { interface Shape {} }\n",
            ),
        )],
    );
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .expect("AMD ambient-module erasure emit");

    assert_eq!(
        sink.writes()[0].callback_text(),
        concat!(
            "define([\"require\", \"exports\"], function (require, exports) {\n",
            "    \"use strict\";\n",
            "    Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "});\n",
        ),
    );
}

#[test]
fn h2_1c_amd_and_umd_wrappers_match_the_pinned_transform() {
    let cases = [
        (
            2,
            concat!(
                "define([\"require\", \"exports\"], function (require, exports) {\n",
                "    \"use strict\";\n",
                "    Object.defineProperty(exports, \"__esModule\", { value: true });\n",
                "    exports.value = void 0;\n",
                "    exports.value = 1;\n",
                "});\n",
            ),
        ),
        (
            3,
            concat!(
                "(function (factory) {\n",
                "    if (typeof module === \"object\" && typeof module.exports === \"object\") {\n",
                "        var v = factory(require, exports);\n",
                "        if (v !== undefined) module.exports = v;\n",
                "    }\n",
                "    else if (typeof define === \"function\" && define.amd) {\n",
                "        define([\"require\", \"exports\"], factory);\n",
                "    }\n",
                "})(function (require, exports) {\n",
                "    \"use strict\";\n",
                "    Object.defineProperty(exports, \"__esModule\", { value: true });\n",
                "    exports.value = void 0;\n",
                "    exports.value = 1;\n",
                "});\n",
            ),
        ),
    ];
    for (module, expected) in cases {
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(module),
                ..CompilerOptions::default()
            },
            &[("/project/input.ts", "export const value: number = 1;\n")],
        );
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.1c asynchronous module emit");
        assert!(outcome.diagnostics().is_empty());
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(sink.writes()[0].callback_text(), expected);
    }
}

#[test]
fn es5_arrow_return_keeps_its_expression_leading_comment() {
    // The complete JavaScript text from the frozen H2.5h
    // optionalChainingInArrow observation, with the same ES5/CRLF profile.
    let source = concat!(
        "// https://github.com/microsoft/TypeScript/issues/41814\n",
        "const test = (names: string[]) =>\n",
        "    // single-line comment\n",
        "    names?.filter(x => x);\n",
    );
    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(1),
            always_strict: Some(true),
            new_line: Some(0),
            ..CompilerOptions::default()
        },
        &[("/project/input.ts", source)],
    );
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .expect("ES5 optional-chain arrow emit");
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].callback_text(),
        concat!(
            "\"use strict\";\r\n",
            "// https://github.com/microsoft/TypeScript/issues/41814\r\n",
            "var test = function (names) {\r\n",
            "    // single-line comment\r\n",
            "    return names === null || names === void 0 ? void 0 : names.filter(function (x) { return x; });\r\n",
            "};\r\n",
        )
    );
}

#[test]
fn system_relocated_body_retakes_leading_comments_without_repeating_detached_header() {
    // Vendored tsc 6.0.3, ES2015/System: a hoisted namespace declaration
    // leaves its ordinary leading comment at the relocated runtime IIFE.
    // A detached file header remains outside System.register.
    let expected_body = concat!(
        "System.register([], function (exports_1, context_1) {\n",
        "    \"use strict\";\n",
        "    var M;\n",
        "    var __moduleName = context_1 && context_1.id;\n",
        "    return {\n",
        "        setters: [],\n",
        "        execute: function () {\n",
        "            // namespace comment\n",
        "            (function (M) {\n",
        "                var x = 1;\n",
        "            })(M || (exports_1(\"M\", M = {})));\n",
        "        }\n",
        "    };\n",
        "});\n",
    );
    for (prefix, emitted_prefix) in [("", ""), ("// detached header\n\n", "// detached header\n")] {
        let source = format!("{prefix}// namespace comment\nexport namespace M {{ var x = 1; }}\n");
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(2),
                module: Some(4),
                ..CompilerOptions::default()
            },
            &[("/project/input.ts", &source)],
        );
        let mut sink = MemoryOutputSink::new();
        ProgramSession::new(prepared)
            .emit_with_reported_diagnostics_for_harness(&mut sink)
            .expect("System relocated namespace emit");
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(
            sink.writes()[0].callback_text(),
            format!("{emitted_prefix}{expected_body}")
        );
    }
}

#[test]
fn h2_1d_system_wrapper_matches_the_pinned_transform() {
    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(4),
            ..CompilerOptions::default()
        },
        &[("/project/input.ts", "export const value: number = 1;\n")],
    );
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("H2.1d System emit");
    assert!(outcome.diagnostics().is_empty());
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].callback_text(),
        concat!(
            "System.register([], function (exports_1, context_1) {\n",
            "    \"use strict\";\n",
            "    var value;\n",
            "    var __moduleName = context_1 && context_1.id;\n",
            "    return {\n",
            "        setters: [],\n",
            "        execute: function () {\n",
            "            exports_1(\"value\", value = 1);\n",
            "        }\n",
            "    };\n",
            "});\n",
        )
    );
}

#[test]
fn empty_emit_program_preserves_present_empty_observations_without_a_resolver() {
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported_diagnostics) = ProgramSession::new(empty_emit_program())
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .expect("empty H1.4 emit");

    assert!(reported_diagnostics.is_empty());
    assert!(!outcome.emit_skipped());
    assert!(outcome.diagnostics().is_empty());
    assert_eq!(outcome.emitted_files(), Some([].as_slice()));
    assert!(outcome.source_maps().is_none());
    assert!(sink.writes().is_empty());
}

#[test]
fn session_entries_reject_the_opposite_prepared_program_mode() {
    let run_error = ProgramSession::new(prepared_for_emit())
        .run()
        .expect_err("emit program cannot enter the H0 path");
    assert!(matches!(run_error, DriverError::InvalidProgramMode { .. }));

    let mut sink = CountingSink::default();
    let emit_error = ProgramSession::new(empty_no_emit_program())
        .emit(&mut sink)
        .expect_err("no-emit program cannot enter the H1 path");
    assert!(matches!(emit_error, DriverError::InvalidProgramMode { .. }));
    assert_eq!(sink.writes, 0);
}

#[test]
fn unsupported_options_and_unadmitted_extensions_fail_before_the_first_sink_call() {
    let base = || CompilerOptions {
        no_emit: Some(false),
        target: Some(99),
        module: Some(200),
        ..CompilerOptions::default()
    };

    // Keep this historical profile entry name; its former refusal now emits.
    // The complete original command is also frozen in emitter-session-retirements.
    let mut out_file = base();
    out_file.module = Some(2);
    out_file.out_file = Some("/project/bundle.js".to_owned().into());
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared_with_sources(
        out_file,
        &[("/project/bundled.ts", "export const bundled = true;\n")],
    ))
    .emit(&mut sink)
    .expect("AMD outFile emits through the production bundle path");
    assert!(!outcome.emit_skipped());
    assert!(outcome.diagnostics().is_empty());
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].path().scalar_test_path(),
        Path::new("/project/bundle.js")
    );
    assert_eq!(
        sink.writes()[0].callback_text(),
        concat!(
            "define(\"bundled\", [\"require\", \"exports\"], function (require, exports) {\n",
            "    \"use strict\";\n",
            "    Object.defineProperty(exports, \"__esModule\", { value: true });\n",
            "    exports.bundled = void 0;\n",
            "    exports.bundled = true;\n",
            "});\n",
        )
    );

    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared_with_sources(
        base(),
        &[("/project/module.tsx", "export const value = true;\n")],
    ))
    .emit(&mut sink)
    .expect("H2.3b admits TSX source/output routing");
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].path().scalar_test_path(),
        Path::new("/project/module.js")
    );
    assert_eq!(
        sink.writes()[0].callback_text(),
        "export const value = true;\n"
    );
}

#[test]
fn h2_3a_allow_js_routes_through_program_and_blocks_only_the_colliding_output() {
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        no_emit: Some(false),
        target: Some(99),
        module: Some(200),
        list_emitted_files: Some(true),
        ..CompilerOptions::default()
    };
    let prepared = prepared_with_sources_and_minimal_lib(
        options,
        &[
            (
                "/project/input.js",
                "#!/usr/bin/env node\n\"use strict\";\n/** @type {number} */\nconst answer = 42;\n",
            ),
            ("/project/sibling.ts", "export const sibling: number = 1;\n"),
        ],
    );
    let mut sink = MemoryOutputSink::new();
    let (outcome, diagnostics) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .expect("H2.3a checked JavaScript Program emit");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5055]
    );
    assert!(outcome.emit_skipped());
    assert_eq!(
        (outcome.emitted_files())
            .map(|names| names
                .iter()
                .map(|name| name.as_js().scalar_test_path().to_path_buf())
                .collect::<Vec<_>>())
            .as_deref(),
        Some([PathBuf::from("/project/sibling.js")].as_slice())
    );
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].path().scalar_test_path(),
        Path::new("/project/sibling.js")
    );
}

#[test]
fn h2_3a_check_js_changes_diagnostics_without_changing_source_routing() {
    const SOURCE: &str = "function checked() { return 5 || true; }\n";
    for (check_js, expected_codes) in [
        (None, Vec::<u32>::new()),
        (Some(false), Vec::new()),
        (Some(true), vec![2872]),
    ] {
        let prepared = prepared_with_sources_and_minimal_lib(
            CompilerOptions {
                allow_js: true,
                check_js,
                no_emit: Some(false),
                target: Some(99),
                module: Some(200),
                out_dir: Some("/project/dist".to_owned().into()),
                ..CompilerOptions::default()
            },
            &[("/project/checked.js", SOURCE)],
        );
        let mut sink = MemoryOutputSink::new();
        let (outcome, diagnostics) = ProgramSession::new(prepared)
            .emit_with_reported_diagnostics_for_harness(&mut sink)
            .expect("H2.3a JavaScript diagnostic routing");
        let codes = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>();
        assert_eq!(codes, expected_codes, "checkJs={check_js:?}");
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(
            sink.writes()[0].path().scalar_test_path(),
            Path::new("/project/dist/checked.js")
        );
        assert_eq!(
            sink.writes()[0].callback_text(),
            format!("\"use strict\";\n{SOURCE}")
        );
        assert!(!outcome.emit_skipped());
    }
}

#[test]
fn h2_3a_mjs_and_cjs_roots_materialize_the_planned_extension() {
    const SOURCE: &str = "\"use strict\";\n// retained\nconst value = 1;\n";
    for extension in ["mjs", "cjs"] {
        let input = format!("/project/input.{extension}");
        let output = format!("/project/dist/input.{extension}");
        let prepared = prepared_with_sources_and_minimal_lib(
            CompilerOptions {
                allow_js: true,
                no_emit: Some(false),
                target: Some(99),
                module: Some(200),
                out_dir: Some("/project/dist".to_owned().into()),
                ..CompilerOptions::default()
            },
            &[(input.as_str(), SOURCE)],
        );
        let mut sink = MemoryOutputSink::new();
        let (outcome, diagnostics) = ProgramSession::new(prepared)
            .emit_with_reported_diagnostics_for_harness(&mut sink)
            .expect("H2.3a explicit JavaScript-family emit");
        assert!(diagnostics.is_empty(), "{extension}: {diagnostics:#?}");
        assert!(!outcome.emit_skipped());
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(
            sink.writes()[0].path().scalar_test_path(),
            Path::new(&output)
        );
        assert_eq!(sink.writes()[0].callback_text(), SOURCE);
    }
}

#[test]
fn h2_3b_classic_jsx_factories_fragments_namespaces_and_ranges_match_typescript() {
    const SOURCE: &str = concat!(
        "/** @jsx Preact.h */\n",
        "/** @jsxFrag Preact.Fragment */\n",
        "declare const Preact: any, Comp: any, value: any, props: any, items: any[];\n",
        "const a = <div disabled data-x=\"a&amp;b\" {...props}>  hello\n",
        "  world {value as string}<Comp.Member x={1} />{...items}</div>;\n",
        "const f = <>x<span />{value}</>;\n",
        "const n = <svg:path xml:lang='a&amp;b' />;\n",
    );
    const EXPECTED: &str = concat!(
        "const a = Preact.h(\"div\", { disabled: true, \"data-x\": \"a&b\", ...props },\n",
        "    \"  hello world \",\n",
        "    value,\n",
        "    Preact.h(Comp.Member, { x: 1 }),\n",
        "    ...items);\n",
        "const f = Preact.h(Preact.Fragment, null,\n",
        "    \"x\",\n",
        "    Preact.h(\"span\", null),\n",
        "    value);\n",
        "const n = Preact.h(\"svg:path\", { \"xml:lang\": 'a&b' });\n",
    );
    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(200),
            jsx: Some(2),
            strict: Some(false),
            always_strict: Some(false),
            new_line: Some(1),
            ..CompilerOptions::default()
        },
        &[("/project/emoji-😀.tsx", SOURCE)],
    );
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("H2.3b classic JSX emit");
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].path().scalar_test_path(),
        Path::new("/project/emoji-😀.js")
    );
    assert_eq!(sink.writes()[0].callback_text(), EXPECTED);
}

#[test]
fn h2_3b_classic_factory_import_substitution_and_lexical_shadowing_match_typescript() {
    const SOURCE: &str = concat!(
        "import React from \"react\";\n",
        "export const top = <div />;\n",
        "export function local(React: any) {\n",
        "  return <span />;\n",
        "}\n",
    );
    const EXPECTED: &str = concat!(
        "\"use strict\";\n",
        "var __importDefault = (this && this.__importDefault) || function (mod) {\n",
        "    return (mod && mod.__esModule) ? mod : { \"default\": mod };\n",
        "};\n",
        "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
        "exports.top = void 0;\n",
        "exports.local = local;\n",
        "const react_1 = __importDefault(require(\"react\"));\n",
        "exports.top = react_1.default.createElement(\"div\", null);\n",
        "function local(React) {\n",
        "    return React.createElement(\"span\", null);\n",
        "}\n",
    );
    let prepared = prepared_with_jsx_package_import(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(1),
            jsx: Some(2),
            es_module_interop: Some(true),
            new_line: Some(1),
            ..CompilerOptions::default()
        },
        SOURCE,
    );
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("H2.3b classic JSX CommonJS emit");
    assert!(outcome.diagnostics().is_empty());
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].path().scalar_test_path(),
        Path::new("/project/view.js")
    );
    assert_eq!(sink.writes()[0].callback_text(), EXPECTED);
}

#[test]
fn h2_3b_preserve_and_react_native_reconstruct_jsx_with_exact_extensions() {
    const SOURCE: &str = "const view: unknown = <Box value={answer as number}><span /></Box>;\n";
    for (jsx, extension) in [(1, "jsx"), (3, "js")] {
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(200),
                jsx: Some(jsx),
                strict: Some(false),
                always_strict: Some(false),
                ..CompilerOptions::default()
            },
            &[("/project/view.tsx", SOURCE)],
        );
        let mut sink = MemoryOutputSink::new();
        ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.3b preserved JSX emit");
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(
            sink.writes()[0].path().scalar_test_path(),
            Path::new(&format!("/project/view.{extension}"))
        );
        assert_eq!(
            sink.writes()[0].callback_text(),
            "const view = <Box value={answer}><span /></Box>;\n"
        );
    }
}

#[test]
fn h2_3a_narrow_out_dir_and_source_family_boundary_fails_closed() {
    let options = |out_dir: &str, allow_js| CompilerOptions {
        allow_js,
        no_emit: Some(false),
        target: Some(99),
        module: Some(200),
        out_dir: Some(out_dir.to_owned().into()),
        ..CompilerOptions::default()
    };
    for (case, compiler_options, source, output) in [
        (
            "TypeScript-only outDir",
            options("/project/dist", false),
            ("/project/input.ts", "const value: number = 1;\n"),
            "/project/dist/input.js",
        ),
        (
            "relative JavaScript outDir",
            options("dist", true),
            ("/project/input.js", "const value = 1;\n"),
            "dist/input.js",
        ),
    ] {
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared_with_sources(compiler_options, &[source]))
            .emit(&mut sink)
            .expect(case);
        assert!(!outcome.emit_skipped(), "{case}");
        assert!(outcome.diagnostics().is_empty(), "{case}");
        assert_eq!(sink.writes().len(), 1, "{case}");
        assert_eq!(
            sink.writes()[0].path().scalar_test_path(),
            Path::new(output),
            "{case}"
        );
        assert_eq!(
            sink.writes()[0].callback_text(),
            "\"use strict\";\nconst value = 1;\n",
            "{case}"
        );
    }

    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared_with_sources(
        options("/project/dist", true),
        &[
            ("/project/input.js", "const js = 1;\n"),
            ("/project/input.ts", "const ts: number = 1;\n"),
        ],
    ))
    .emit(&mut sink)
    .expect("mixed outDir is supported; colliding output is blocked");
    assert!(outcome.emit_skipped());
    assert!(sink.writes().is_empty());

    let mut sink = CountingSink::default();
    let error = ProgramSession::new(prepared_with_sources(
        CompilerOptions {
            allow_js: false,
            no_emit: Some(false),
            target: Some(99),
            module: Some(200),
            ..CompilerOptions::default()
        },
        &[("/project/input.js", "const value = 1;\n")],
    ))
    .emit(&mut sink)
    .expect_err("unadmitted JavaScript source must fail closed");
    assert!(matches!(
        error,
        DriverError::Emit(EmitFailure::UnsupportedSourceExtension { .. })
    ));
    assert_eq!(sink.writes, 0);

    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared_with_sources(
        CompilerOptions {
            allow_js: true,
            no_emit: Some(false),
            target: Some(99),
            module: Some(200),
            ..CompilerOptions::default()
        },
        &[("/project/input.jsx", "const value = 1;\n")],
    ))
    .emit(&mut sink)
    .expect("H2.3b admits allowJs JSX-family source routing");
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].path().scalar_test_path(),
        Path::new("/project/input.js")
    );
}

#[test]
fn h2_3d_json_text_paths_bom_newlines_and_module_invariance_match_typescript() {
    const SOURCE: &str = concat!(
        "{\n",
        "  \"a\":1,\n",
        "  \"same\": [true,{\"emoji\":\"😀\"},],\n",
        "}\n",
    );
    const EXPECTED: &str = concat!(
        "{\n",
        "    \"a\": 1,\n",
        "    \"same\": [true, { \"emoji\": \"😀\" },]\n",
        "}\n",
    );
    for module in [200, 99, 1, 2, 3, 4, 100, 101, 102, 199] {
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(module),
                resolve_json_module: Some(true),
                out_dir: Some("/project/dist".to_owned().into()),
                new_line: Some(1),
                ..CompilerOptions::default()
            },
            &[("/project/data.json", SOURCE)],
        );
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .unwrap_or_else(|error| panic!("module={module} JSON emit failed: {error}"));
        assert!(!outcome.emit_skipped(), "module={module}");
        assert_eq!(sink.writes().len(), 1, "module={module}");
        assert_eq!(
            sink.writes()[0].path().scalar_test_path(),
            Path::new("/project/dist/data.json"),
            "module={module}"
        );
        assert_eq!(
            sink.writes()[0].callback_text(),
            EXPECTED,
            "module={module}"
        );
        assert!(!sink.writes()[0].write_byte_order_mark(), "module={module}");
    }

    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(1),
            resolve_json_module: Some(true),
            out_dir: Some("/project/dist".to_owned().into()),
            new_line: Some(0),
            emit_bom: Some(true),
            ..CompilerOptions::default()
        },
        &[("/project/bom.json", "\u{feff}{\r\n\t\"a\":1,\r\n}\r\n")],
    );
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("JSON CRLF/BOM emit");
    assert_eq!(
        sink.writes()[0].callback_text(),
        "{\r\n    \"a\": 1\r\n}\r\n"
    );
    assert!(sink.writes()[0].write_byte_order_mark());
    assert!(sink.writes()[0]
        .materialized_bytes()
        .starts_with(&[0xef, 0xbb, 0xbf]));
}

#[test]
fn h2_3d_json_without_distinct_output_location_is_not_written() {
    for out_dir in [None, Some(tsc_diagnostics::JsString::from("/project"))] {
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(200),
                resolve_json_module: Some(true),
                out_dir,
                ..CompilerOptions::default()
            },
            &[("/project/data.json", "{\"value\":1}")],
        );
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("same-location JSON emit suppression");
        assert!(!outcome.emit_skipped());
        assert!(sink.writes().is_empty());
    }
}

#[test]
fn h2_3d_resolve_json_module_option_diagnostics_match_typescript_and_gate_no_emit_on_error() {
    // TypeScript 7.1 has no resolveJsonModule relationship rows; the removed
    // values (`moduleResolution=Classic`, `module=UMD`/`module=System` and
    // `moduleResolution=node10`) report TS5108 and still gate the emit under
    // noEmitOnError.
    for (module, module_resolution, expected_codes) in [
        (200, 1, &[5108][..]),
        (3, 2, &[5108, 5108][..]),
        (4, 2, &[5108, 5108][..]),
    ] {
        for no_emit_on_error in [false, true] {
            let prepared = prepared_with_sources_and_minimal_lib(
                CompilerOptions {
                    no_emit: Some(false),
                    no_emit_on_error: Some(no_emit_on_error),
                    target: Some(99),
                    module: Some(module),
                    module_resolution: Some(module_resolution),
                    resolve_json_module: Some(true),
                    out_dir: Some("/project/dist".to_owned().into()),
                    ..CompilerOptions::default()
                },
                &[("/project/data.json", "{\"value\":1}")],
            );
            let mut sink = MemoryOutputSink::new();
            let (outcome, diagnostics) = ProgramSession::new(prepared)
                .emit_with_reported_diagnostics_for_harness(&mut sink)
                .expect("resolveJsonModule option diagnostic emit");

            assert_eq!(
                diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code())
                    .collect::<Vec<_>>(),
                expected_codes,
                "module={module} moduleResolution={module_resolution} noEmitOnError={no_emit_on_error}"
            );
            assert_eq!(outcome.emit_skipped(), no_emit_on_error);
            assert_eq!(sink.writes().len(), usize::from(!no_emit_on_error));
        }
    }
}

#[test]
fn h2_4b_standard_decorator_source_joins_the_atomic_multi_source_emit() {
    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(200),
            list_emitted_files: Some(true),
            ..CompilerOptions::default()
        },
        &[
            ("/project/first.ts", "export const first: number = 1;\n"),
            ("/project/second.ts", "@dec class Runtime {}\n"),
        ],
    );
    let mut sink = CountingSink::default();
    let outcome = ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("standard decorators are admitted by H2.4b");
    assert!(!outcome.emit_skipped());
    assert_eq!(sink.writes, 2);
}

#[test]
fn filesystem_failure_at_each_write_index_preserves_partial_set_and_continuation() {
    assert_filesystem_failure_at_each_write_index(200);
}

#[test]
fn h2_1a_filesystem_failure_preserves_partial_set_continuation() {
    assert_filesystem_failure_at_each_write_index(99);
}

#[test]
fn h2_1b_commonjs_filesystem_failure_preserves_partial_set_continuation() {
    assert_filesystem_failure_at_each_write_index(1);
}

#[test]
fn h2_1c_amd_umd_filesystem_failure_preserves_partial_set_continuation() {
    for module in [2, 3] {
        assert_filesystem_failure_at_each_write_index(module);
    }
}

#[test]
fn h2_1d_system_filesystem_failure_preserves_partial_set_continuation() {
    assert_filesystem_failure_at_each_write_index(4);
}

#[test]
fn h2_1e_node_format_filesystem_failure_preserves_partial_set_continuation() {
    assert_filesystem_failure_at_each_write_index(199);
}

#[test]
fn h2_1e_dynamic_import_attributes_are_observed_on_the_esnext_path() {
    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(99),
            list_emitted_files: Some(true),
            ..CompilerOptions::default()
        },
        &[(
            "/project/input.ts",
            concat!(
                "const specifier = \"./runtime.cts\";\n",
                "export const loaded = import(specifier, { with: { type: \"javascript\" } });\n",
            ),
        )],
    );
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("dynamic import attributes emit");
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].callback_text(),
        concat!(
            "const specifier = \"./runtime.cts\";\n",
            "export const loaded = import(specifier, { with: { type: \"javascript\" } });\n",
        )
    );
}

#[test]
fn h2_2a_runtime_and_const_enum_emit_matches_typescript_shapes() {
    let cases = [
        (
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(99),
                ..CompilerOptions::default()
            },
            "const BAR = 2..toFixed(0);\n\
             enum Foo {\n\
                 A = `${BAR}`,\n\
                 B = \"2\" + BAR,\n\
                 F = BAR,\n\
                 H = A,\n\
             }\n",
            concat!(
                "\"use strict\";\n",
                "const BAR = 2..toFixed(0);\n",
                "var Foo;\n",
                "(function (Foo) {\n",
                "    Foo[\"A\"] = `${BAR}`;\n",
                "    Foo[\"B\"] = \"2\" + BAR;\n",
                "    Foo[Foo[\"F\"] = BAR] = \"F\";\n",
                "    Foo[\"H\"] = Foo.A;\n",
                "})(Foo || (Foo = {}));\n",
            ),
        ),
        (
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(99),
                ..CompilerOptions::default()
            },
            "const enum Props { k = 'k' }\n\
             declare const foo: { [key: string]: string[] };\n\
             foo[Props.k] = ['foo'];\n",
            "\"use strict\";\nfoo[\"k\" /* Props.k */] = ['foo'];\n",
        ),
        (
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(99),
                preserve_const_enums: Some(true),
                ..CompilerOptions::default()
            },
            "const enum A { Foo };\nexport { A };\n",
            concat!(
                "var A;\n",
                "(function (A) {\n",
                "    A[A[\"Foo\"] = 0] = \"Foo\";\n",
                "})(A || (A = {}));\n",
                ";\n",
                "export { A };\n",
            ),
        ),
    ];

    for (options, source, expected) in cases {
        let prepared = prepared_with_sources(options, &[("/project/input.ts", source)]);
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.2a enum emit");
        assert!(outcome.diagnostics().is_empty());
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(sink.writes()[0].callback_text(), expected);
    }
}

#[test]
fn h2_2b_runtime_namespace_emit_matches_typescript_shapes() {
    let cases = [
        (
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(99),
                ..CompilerOptions::default()
            },
            concat!(
                "export namespace Foo {\n",
                "    export const key = Symbol();\n",
                "}\n",
                "export class C {\n",
                "    [Foo.key]: string;\n",
                "    constructor() { this[Foo.key] = \"hello\"; }\n",
                "}\n",
            ),
            concat!(
                "export var Foo;\n",
                "(function (Foo) {\n",
                "    Foo.key = Symbol();\n",
                "})(Foo || (Foo = {}));\n",
                "export class C {\n",
                "    [Foo.key];\n",
                "    constructor() { this[Foo.key] = \"hello\"; }\n",
                "}\n",
            ),
        ),
        (
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(99),
                ..CompilerOptions::default()
            },
            concat!(
                "export namespace ns {\n",
                "    export namespace undefined {\n",
                "        export const s = Symbol();\n",
                "    };\n",
                "    export function x(p: undefined): undefined { return p; }\n",
                "}\n",
            ),
            concat!(
                "export var ns;\n",
                "(function (ns) {\n",
                "    let undefined;\n",
                "    (function (undefined) {\n",
                "        undefined.s = Symbol();\n",
                "    })(undefined = ns.undefined || (ns.undefined = {}));\n",
                "    ;\n",
                "    function x(p) { return p; }\n",
                "    ns.x = x;\n",
                "})(ns || (ns = {}));\n",
            ),
        ),
    ];

    for (options, source, expected) in cases {
        let prepared = prepared_with_sources(options, &[("/project/input.ts", source)]);
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.2b namespace emit");
        assert!(outcome.diagnostics().is_empty());
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(sink.writes()[0].callback_text(), expected);
    }
}

#[test]
fn namespace_generated_names_use_semantic_local_scope_ownership() {
    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(2),
            ..CompilerOptions::default()
        },
        &[(
            "/project/input.ts",
            concat!(
                "namespace Z.M { export function bar() {} }\n",
                "namespace A.M { export import M = Z.M; M.bar(); }\n",
                "namespace B.M { import M = Z.M; M.bar(); }\n",
            ),
        )],
    );
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("namespace generated-name scope emit");

    assert!(outcome.diagnostics().is_empty());
    assert_eq!(sink.writes().len(), 1);
    let text = sink.writes()[0].callback_text();
    assert!(
        text.contains(concat!(
            "    (function (M) {\n",
            "        M.M = Z.M;\n",
            "        M.M.bar();\n",
        )),
        "exported aliases are namespace properties, not local-name reservations: {text}",
    );
    assert!(
        text.contains(concat!(
            "    (function (M_1) {\n",
            "        var M = Z.M;\n",
            "        M.bar();\n",
        )),
        "non-exported aliases reserve the namespace IIFE local name: {text}",
    );
}

#[test]
fn h2_2c_parameter_property_emit_matches_typescript_shapes() {
    let cases = [
        (
            concat!(
                "export class Service {\n",
                "    constructor(public value: number) {}\n",
                "}\n",
            ),
            concat!(
                "export class Service {\n",
                "    value;\n",
                "    constructor(value) {\n",
                "        this.value = value;\n",
                "    }\n",
                "}\n",
            ),
        ),
        (
            concat!(
                "class Base {}\n",
                "class Derived extends Base {\n",
                "    constructor(public value: number) {\n",
                "        try { super(); } finally {}\n",
                "    }\n",
                "}\n",
            ),
            concat!(
                "\"use strict\";\n",
                "class Base {\n",
                "}\n",
                "class Derived extends Base {\n",
                "    value;\n",
                "    constructor(value) {\n",
                "        try {\n",
                "            super();\n",
                "            this.value = value;\n",
                "        }\n",
                "        finally { }\n",
                "    }\n",
                "}\n",
            ),
        ),
    ];

    for (source, expected) in cases {
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(99),
                use_define_for_class_fields: Some(true),
                ..CompilerOptions::default()
            },
            &[("/project/input.ts", source)],
        );
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.2c parameter-property emit");
        assert!(outcome.diagnostics().is_empty());
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(sink.writes()[0].callback_text(), expected);
    }
}

#[test]
fn h2_2d_module_format_interactions_match_typescript_shapes() {
    let cases = [
        (1, concat!("\"use strict\";\n", "module.exports = 42;\n")),
        (
            2,
            concat!(
                "define([\"require\", \"exports\"], function (require, exports) {\n",
                "    \"use strict\";\n",
                "    return 42;\n",
                "});\n",
            ),
        ),
        (
            3,
            concat!(
                "(function (factory) {\n",
                "    if (typeof module === \"object\" && typeof module.exports === \"object\") {\n",
                "        var v = factory(require, exports);\n",
                "        if (v !== undefined) module.exports = v;\n",
                "    }\n",
                "    else if (typeof define === \"function\" && define.amd) {\n",
                "        define([\"require\", \"exports\"], factory);\n",
                "    }\n",
                "})(function (require, exports) {\n",
                "    \"use strict\";\n",
                "    return 42;\n",
                "});\n",
            ),
        ),
        (
            4,
            concat!(
                "System.register([], function (exports_1, context_1) {\n",
                "    \"use strict\";\n",
                "    var __moduleName = context_1 && context_1.id;\n",
                "    return {\n",
                "        setters: [],\n",
                "        execute: function () {\n",
                "        }\n",
                "    };\n",
                "});\n",
            ),
        ),
        (99, "export {};\n"),
    ];
    for (module, expected) in cases {
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(module),
                ..CompilerOptions::default()
            },
            &[("/project/input.ts", "export = 42;\n")],
        );
        let mut sink = MemoryOutputSink::new();
        ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.2d export-equals emit");
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(sink.writes()[0].callback_text(), expected);
    }

    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(99),
            ..CompilerOptions::default()
        },
        &[(
            "/project/input.ts",
            concat!(
                "declare namespace Runtime { const value: number; }\n",
                "import value = Runtime.value;\n",
            ),
        )],
    );
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("H2.2d internal import-equals emit");
    assert_eq!(
        sink.writes()[0].callback_text(),
        concat!("\"use strict\";\n", "var value = Runtime.value;\n")
    );

    let prepared = prepared_with_package_import(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(4),
            ..CompilerOptions::default()
        },
        concat!(
            "export import value = require(\"pkg\");\n",
            "console.log(value);\n",
        ),
    );
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("H2.2d System import-equals emit");
    assert_eq!(
        sink.writes()[0].callback_text(),
        concat!(
            "System.register([\"pkg\"], function (exports_1, context_1) {\n",
            "    \"use strict\";\n",
            "    var value;\n",
            "    var __moduleName = context_1 && context_1.id;\n",
            "    return {\n",
            "        setters: [\n",
            "            function (value_1) {\n",
            "                value = value_1;\n",
            "                exports_1(\"value\", value_1);\n",
            "            }\n",
            "        ],\n",
            "        execute: function () {\n",
            "            console.log(value);\n",
            "        }\n",
            "    };\n",
            "});\n",
        )
    );

    for (module, source, expected) in [
        (
            1,
            concat!(
                "export import value = require(\"pkg\");\n",
                "console.log(value);\n",
            ),
            concat!(
                "\"use strict\";\n",
                "Object.defineProperty(exports, \"__esModule\", { value: true });\n",
                "exports.value = require(\"pkg\");\n",
                "console.log(exports.value);\n",
            ),
        ),
        (
            2,
            concat!(
                "export import value = require(\"pkg\");\n",
                "console.log(value);\n",
            ),
            concat!(
                "define([\"require\", \"exports\", \"pkg\"], function (require, exports, value) {\n",
                "    \"use strict\";\n",
                "    Object.defineProperty(exports, \"__esModule\", { value: true });\n",
                "    exports.value = value;\n",
                "    console.log(exports.value);\n",
                "});\n",
            ),
        ),
        (
            3,
            concat!(
                "import value = require(\"pkg\");\n",
                "console.log(value);\n",
            ),
            concat!(
                "(function (factory) {\n",
                "    if (typeof module === \"object\" && typeof module.exports === \"object\") {\n",
                "        var v = factory(require, exports);\n",
                "        if (v !== undefined) module.exports = v;\n",
                "    }\n",
                "    else if (typeof define === \"function\" && define.amd) {\n",
                "        define([\"require\", \"exports\", \"pkg\"], factory);\n",
                "    }\n",
                "})(function (require, exports) {\n",
                "    \"use strict\";\n",
                "    Object.defineProperty(exports, \"__esModule\", { value: true });\n",
                "    const value = require(\"pkg\");\n",
                "    console.log(value);\n",
                "});\n",
            ),
        ),
    ] {
        let prepared = prepared_with_package_import(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(module),
                ..CompilerOptions::default()
            },
            source,
        );
        let mut sink = MemoryOutputSink::new();
        ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.2d CommonJS-family import-equals emit");
        assert_eq!(sink.writes()[0].callback_text(), expected);
    }

    for (source, expected) in [
        (
            concat!(
                "import value = require(\"pkg\");\n",
                "console.log(value);\n",
            ),
            concat!("const value = require(\"pkg\");\n", "console.log(value);\n",),
        ),
        (
            concat!("const value = 42;\n", "export = value;\n"),
            concat!("const value = 42;\n", "module.exports = value;\n"),
        ),
    ] {
        let options = CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(200),
            ..CompilerOptions::default()
        };
        let prepared = if source.contains("require(\"pkg\")") {
            prepared_with_package_import(options, source)
        } else {
            prepared_with_sources(options, &[("/project/input.ts", source)])
        };
        let mut sink = MemoryOutputSink::new();
        ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.2d Preserve import/export-equals emit");
        assert_eq!(sink.writes()[0].callback_text(), expected);
    }
}

fn assert_filesystem_failure_at_each_write_index(module: i32) {
    let output_paths = [
        PathBuf::from("/project/first.js"),
        PathBuf::from("/project/second.js"),
    ];
    for failed_index in 0..output_paths.len() {
        let prepared = prepared_with_sources(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(module),
                list_emitted_files: Some(true),
                ..CompilerOptions::default()
            },
            &[
                ("/project/first.ts", "export const first: number = 1;\n"),
                ("/project/second.ts", "export const second: number = 2;\n"),
            ],
        );
        let mut filesystem = InjectedFileSystem {
            fail_path: output_paths[failed_index].clone(),
            attempts: Vec::new(),
            files: BTreeMap::new(),
        };
        let mut sink = FsOutputSink::new(&mut filesystem);
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("filesystem write errors remain emit diagnostics");

        assert_eq!(
            outcome
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [5033],
            "failure index {failed_index}"
        );
        assert!(!outcome.emit_skipped(), "failure index {failed_index}");
        assert_eq!(
            (outcome.emitted_files())
                .map(|names| names
                    .iter()
                    .map(|name| name.as_js().scalar_test_path().to_path_buf())
                    .collect::<Vec<_>>())
                .as_deref(),
            Some(output_paths.as_slice()),
            "failure index {failed_index}"
        );
        assert_eq!(
            filesystem
                .attempts
                .iter()
                .filter(|path| *path == &output_paths[failed_index])
                .count(),
            2,
            "failure index {failed_index} retries exactly once"
        );
        let expected_partial = output_paths
            .iter()
            .enumerate()
            .filter_map(|(index, path)| (index != failed_index).then_some(path.clone()))
            .collect::<Vec<_>>();
        assert_eq!(
            filesystem.files.keys().cloned().collect::<Vec<_>>(),
            expected_partial,
            "failure index {failed_index} partial output set"
        );
    }
}

use crate::utf16_scalar_path::ScalarTestPath as _;

/// The diagnostics of the native compiler runner's two Programs for one
/// source: the first only reports, the second emits and then reports.
fn native_harness_programs(
    options: CompilerOptions,
    source: &str,
) -> (Vec<Diagnostic>, Vec<Diagnostic>, MemoryOutputSink) {
    let prepared =
        || prepared_with_sources_and_minimal_lib(options.clone(), &[("/project/input.ts", source)]);
    let first = ProgramSession::new(prepared())
        .run_for_native_harness(NativeHarnessCollection::default())
        .expect("first Program");
    let mut sink = MemoryOutputSink::new();
    let Ok((second, emit)) = ProgramSession::new(prepared())
        .emit_then_run_for_native_harness(NativeHarnessCollection::default(), &mut sink)
        .expect("second Program")
    else {
        panic!("the Program emits before its diagnostics are known");
    };
    emit.expect("the second Program emits")
        .expect("emit before the check");
    let in_source = |diagnostics: &[Diagnostic]| {
        diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic
                    .file_name
                    .as_ref()
                    .is_some_and(|name| name == "/project/input.ts")
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    (
        in_source(first.native_harness_diagnostics()),
        in_source(second.native_harness_diagnostics()),
        sink,
    )
}

#[test]
fn the_native_harness_second_program_reports_where_its_emit_first_resolved() {
    // tsgo's test harness compiles a configuration twice (compileFilesWithHost,
    // testutil/harnessutil/harnessutil.go:647-712) and baselines the
    // diagnostics of the second Program, which emits first. The const enum
    // inliner asks the checker about each property access the emitted file
    // keeps (GetConstantValue checks an access that has no resolved symbol,
    // checker/services.go:869-888), so `this.a` in `m2` is the first place
    // that needs the instantiations of `T`, and the circularity is reported
    // there. The first Program, like the command line, reaches it from the
    // class declaration. The positions are the ones of tsgo's baseline
    // (mutuallyRecursiveInference) and of its command line.
    let source = concat!(
        "class T<A> {\n",
        "    a: A;\n",
        "    b: any\n",
        "}\n",
        "class L<RT extends { a: 'a' | 'b', b: any }> extends T<RT[RT['a']]> {\n",
        "    m() { this.a }\n",
        "}\n",
        "class X extends L<X> {\n",
        "    a: 'a' | 'b'\n",
        "    b: number\n",
        "    m2() {\n",
        "        this.a\n",
        "    }\n",
        "}\n",
    );
    let (first, second, _) = native_harness_programs(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(2),
            ..CompilerOptions::default()
        },
        source,
    );
    let circularity = |diagnostics: &[Diagnostic]| {
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code() == 5114)
            .map(|diagnostic| (diagnostic.start, diagnostic.length))
            .collect::<Vec<_>>()
    };
    let class_name = u32::try_from(source.find("X extends").expect("class X")).expect("offset");
    let access = u32::try_from(source.rfind("this.a").expect("this.a in m2")).expect("offset");
    assert_eq!(circularity(&first), [(Some(class_name), Some(1))]);
    assert_eq!(circularity(&second), [(Some(access), Some(6))]);
    assert_eq!(first.len(), second.len());
}

#[test]
fn an_emit_before_the_check_reports_nothing_the_check_does_not() {
    // A query of the emit resolver about an unchecked source resolves what it
    // needs; it must leave no diagnostic of its own. Each source below made
    // the emit report one before the resolver's lookups were silent
    // (getReferencedValueSymbol, binder/referenceresolver.go:83-105) and the
    // reference walk kept to the places the check resolves
    // (markLinkedReferences, checker/checker.go:28654-28817): the name of a
    // namespace merged with a later enum (TS2450), `implements` of a
    // qualified interface name (TS2689), `this` in a type query (TS2304) and
    // the names of an import alias (TS2708).
    for source in [
        "namespace x {\n    export let y = 123\n}\nenum x {\n    z = y\n}\n",
        "namespace NS {\n    export interface Dep {}\n}\nclass Src implements NS.Dep {}\n",
        "class C {\n    foo = 1;\n    bar: typeof this.foo = 2;\n}\n",
        "namespace a {\n    export type A = number;\n}\nnamespace b {\n    export import A = a.A;\n    export namespace A {}\n}\n",
    ] {
        let (first, second, _) = native_harness_programs(
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                ..CompilerOptions::default()
            },
            source,
        );
        assert_eq!(first, second, "{source}");
    }
}

#[test]
fn an_emit_before_the_check_reports_no_implicit_any_for_a_parameter_its_call_types() {
    // The reference walk of the emit asks for the type of `k`, the left of
    // `k.length` (markPropertyAliasReferenced, checker/checker.go:28903-28907),
    // before the call around it was resolved. The call is resolved from
    // inside that request and gives `k` the type `unknown`; the request
    // itself then finds no contextual signature, because the argument's
    // contextual type has become `Mapped<unknown>`. tsgo reports no implicit
    // `any` from there for a parameter of a context-sensitive signature
    // (getTypeOfVariableOrParameterOrPropertyWorker, checker.go:16927-16929);
    // tsc 6.0.3 did, so the second Program had one diagnostic more than the
    // first. Both report tsgo's one diagnostic, at the position of its
    // command line and of its baseline (reverseMappedPartiallyInferableTypes).
    let source = concat!(
        "type Box<T> = {\n",
        "    contents?: T;\n",
        "    contains?(content: T): boolean;\n",
        "};\n",
        "type Mapped<T> = {\n",
        "    [K in keyof T]: Box<T[K]>;\n",
        "};\n",
        "declare function id<T>(arg: Mapped<T>): Mapped<T>;\n",
        "const obj3 = id({\n",
        "    foo: {\n",
        "        contains(k) {\n",
        "            return k.length > 0;\n",
        "        }\n",
        "    }\n",
        "});\n",
    );
    let (first, second, _) = native_harness_programs(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(2),
            strict: Some(true),
            ..CompilerOptions::default()
        },
        source,
    );
    let reported = |diagnostics: &[Diagnostic]| {
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
            .collect::<Vec<_>>()
    };
    let access = u32::try_from(source.find("k.length").expect("k.length")).expect("offset");
    assert_eq!(reported(&first), [(18046, Some(access), Some(1))]);
    assert_eq!(reported(&second), reported(&first));
}

#[test]
fn an_emit_before_the_check_keeps_the_aliases_the_file_uses() {
    // Import elision reads the `referenced` marks of the aliases. The check
    // leaves them; before it, tsgo's import elision walks the file and marks
    // them itself (MarkLinkedReferencesRecursively,
    // transformers/tstransforms/importelision.go:29). Without the walk an
    // emit that comes first drops the import alias the file uses.
    let options = || CompilerOptions {
        no_emit: Some(false),
        target: Some(99),
        ..CompilerOptions::default()
    };
    let source = "namespace N {\n    export const y = 1;\n}\nimport a = N.y;\nconst b = a;\n";
    let (first, second, sink) = native_harness_programs(options(), source);
    assert_eq!(first, second);
    let mut checked = MemoryOutputSink::new();
    ProgramSession::new(prepared_with_sources_and_minimal_lib(
        options(),
        &[("/project/input.ts", source)],
    ))
    .emit(&mut checked)
    .expect("emit after the check");
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(
        sink.writes()[0].callback_text(),
        checked.writes()[0].callback_text()
    );
    assert!(sink.writes()[0].callback_text().contains("var a = N.y;"));
}

#[test]
fn a_program_that_reports_before_it_emits_keeps_the_first_programs_order() {
    // `Emit` asks for every diagnostic first under noEmitOnError
    // (HandleNoEmitOptions, compiler/program.go:1976-2008): such a Program
    // has no emit-first order, and the session is handed back unconsumed.
    let mut sink = MemoryOutputSink::new();
    let session = ProgramSession::new(prepared_with_sources_and_minimal_lib(
        CompilerOptions {
            no_emit: Some(false),
            no_emit_on_error: Some(true),
            target: Some(99),
            ..CompilerOptions::default()
        },
        &[("/project/input.ts", "export const value: number = 1;\n")],
    ));
    let handed_back = session
        .emit_then_run_for_native_harness(NativeHarnessCollection::default(), &mut sink)
        .expect("session");
    assert!(handed_back.is_err());
    assert!(sink.writes().is_empty());
}
