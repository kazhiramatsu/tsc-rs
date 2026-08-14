use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_compiler::{
    DriverError, EmitArtifact, EmitFailure, EmitFileSystem, EmitIoError, EmitWriteDisposition,
    FsOutputSink, H2ActivityCounters, H2RuntimeSlice, MemoryOutputSink, OutputSink, ProgramSession,
};
use tsc_program::ResolutionMode;
use tsc_program::{
    CompilerOptions, ModuleExtension, ModuleResolution, PathContext, PathMapping, PreparedProgram,
    PreparedSourceFile, ProgramOptions, ProgramPath, ResolutionKey, ResolvedModule,
    ResolvedModuleTarget, SourceFileId,
};

const H2_1C_OWNER_CONTROLS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-1c-owner-controls.v1.json"
));
const H2_1D_OWNER_CONTROLS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-1d-owner-controls.v1.json"
));
const H2_3A_OWNER_CONTROLS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-3a-owner-controls.v1.json"
));

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
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.attempts.push(path.to_path_buf());
        if path == self.fail_path {
            return Err("injected stable write failure".to_owned());
        }
        self.files.insert(path.to_path_buf(), bytes.to_vec());
        Ok(())
    }

    fn create_directory(&mut self, path: &Path) -> Result<(), String> {
        panic!(
            "existing project parent must not be created: {}",
            path.display()
        )
    }

    fn directory_exists(&mut self, path: &Path) -> bool {
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

fn oracle_text(record: &Value) -> String {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(record["utf8_base64"].as_str().expect("base64 text"))
        .expect("decode oracle text");
    assert_eq!(
        bytes.len() as u64,
        record["utf8_bytes"].as_u64().expect("oracle byte count")
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        record["utf8_sha256"].as_str().expect("oracle text hash")
    );
    String::from_utf8(bytes).expect("oracle UTF-8")
}

fn oracle_callback_text(record: &Value) -> String {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(
            record["callback_utf8_base64"]
                .as_str()
                .expect("callback base64 text"),
        )
        .expect("decode oracle callback text");
    assert_eq!(
        bytes.len() as u64,
        record["callback_utf8_bytes"]
            .as_u64()
            .expect("oracle callback byte count")
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        record["callback_utf8_sha256"]
            .as_str()
            .expect("oracle callback text hash")
    );
    String::from_utf8(bytes).expect("oracle callback UTF-8")
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

fn assert_h2_runtime_zero(counters: H2ActivityCounters) {
    assert!(counters.h2_runtime_is_zero());
    for slice in H2RuntimeSlice::ALL {
        assert_eq!(
            counters.runtime_slice(slice),
            0,
            "{} activity",
            slice.name()
        );
    }
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
        sink.writes()[0].path(),
        std::path::Path::new("/project/input.js")
    );
    assert_eq!(
        sink.writes()[0].callback_text(),
        "export const value = 1;\n"
    );
    assert!(!sink.writes()[0].write_byte_order_mark());
    let activity = outcome.h2_activity();
    assert_eq!(activity.emit_session_constructions(), 1);
    assert_eq!(activity.output_plan_constructions(), 1);
    assert_eq!(activity.emit_resolver_borrows(), 1);
    assert_eq!(activity.script_transformer_list_constructions(), 1);
    assert_eq!(activity.transform_typescript_constructions(), 1);
    assert_eq!(activity.transform_class_fields_constructions(), 1);
    assert_eq!(activity.transform_ecmascript_module_constructions(), 1);
    assert_eq!(activity.transform_context_constructions(), 1);
    assert_eq!(activity.printer_constructions(), 1);
    assert_eq!(activity.javascript_artifact_creations(), 1);
    assert_eq!(activity.output_sink_write_attempts(), 1);
    assert_eq!(activity.output_sink_failures(), 0);
    assert_h2_runtime_zero(activity);
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
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_1a),
            1
        );
        for slice in H2RuntimeSlice::ALL {
            if slice != H2RuntimeSlice::H2_1a {
                assert_eq!(outcome.h2_activity().runtime_slice(slice), 0);
            }
        }
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
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_1a),
            1
        );
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_1b),
            1
        );
        for slice in H2RuntimeSlice::ALL {
            if !matches!(slice, H2RuntimeSlice::H2_1a | H2RuntimeSlice::H2_1b) {
                assert_eq!(outcome.h2_activity().runtime_slice(slice), 0);
            }
        }
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
        .find(|write| write.path() == Path::new("/project/b.js"))
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
fn deprecated_module_none_selects_transform_modules_commonjs_delegate() {
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
    let (outcome, diagnostics) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .expect("module=None CommonJS-delegate emit");

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5107]
    );
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
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_1a),
        1
    );
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_1b),
        1
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
        ProgramOptions::default().with_paths(vec![PathMapping::new("*", vec!["bare".to_owned()])]),
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
    let cases = [
        (
            concat!(
                "export default function Foo() {}\n",
                "namespace Foo { export var x; }\n",
                "interface Foo {}\n",
                "export interface Foo {}\n",
            ),
            ")(exports.Foo || (exports.Foo = {}));",
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
        for slice in [
            H2RuntimeSlice::H2_1a,
            H2RuntimeSlice::H2_1b,
            H2RuntimeSlice::H2_1c,
        ] {
            assert_eq!(outcome.h2_activity().runtime_slice(slice), 1);
        }
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
    for slice in H2RuntimeSlice::ALL {
        let expected = u64::from(slice == H2RuntimeSlice::H2_1d);
        assert_eq!(outcome.h2_activity().runtime_slice(slice), expected);
    }
}

#[test]
fn h2_1d_system_owner_closure_matches_the_pinned_transform() {
    let artifact: Value =
        serde_json::from_slice(H2_1D_OWNER_CONTROLS).expect("H2.1d owner controls JSON");
    assert_eq!(artifact["phase"], "H2.1d-system-owner-controls");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["summary"]["exact_outputs"], 1);
    let control = &artifact["controls"][0];
    assert_eq!(control["control_id"], "system-register-owner-closure");

    let mut builder = PreparedProgram::emitting_builder(
        PathContext::new(path("/project"), true),
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(4),
            ignore_deprecations: Some("6.0".to_owned()),
            es_module_interop: Some(true),
            ..CompilerOptions::default()
        },
    );
    let mut source_ids = BTreeMap::new();
    for file in control["input"]["files"]
        .as_array()
        .expect("owner-control files")
    {
        let file_name = file["path"].as_str().expect("owner-control path");
        let text = oracle_text(file);
        let source = PreparedSourceFile::new(path(file_name), text)
            .with_may_be_emitted(file["emit_eligible"].as_bool().expect("emit eligibility"));
        let source_id = builder.add_source_file(source).expect("add control source");
        source_ids.insert(file_name.to_owned(), source_id);
    }
    let root = control["input"]["root"].as_str().expect("control root");
    builder
        .add_root_file(source_ids[root])
        .expect("add control root");
    for resolution in control["input"]["module_resolutions"]
        .as_array()
        .expect("module resolutions")
    {
        let origin = resolution["origin"].as_str().expect("resolution origin");
        let specifier = resolution["specifier"]
            .as_str()
            .expect("resolution specifier");
        let resolved_file = resolution["target"].as_str().expect("resolution target");
        builder
            .add_module_resolution(
                ResolutionKey::new(
                    path(origin).canonical().clone(),
                    specifier,
                    ResolutionMode::Unspecified,
                ),
                Ok(source_resolution(
                    source_ids[resolved_file],
                    resolved_file,
                    ModuleExtension::Ts,
                )),
            )
            .expect("add authoritative source resolution");
    }
    let prepared = builder.build().expect("prepared System owner program");
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("H2.1d System owner emit");
    assert!(outcome.diagnostics().is_empty());
    assert_eq!(sink.writes().len(), 1);
    let expected = oracle_text(&control["runs"][0]["output"]);
    assert_eq!(sink.writes()[0].callback_text(), expected.as_str());
}

#[test]
fn h2_1c_amd_pragmas_and_static_dependency_order_match_the_pinned_transform() {
    let artifact: Value =
        serde_json::from_slice(H2_1C_OWNER_CONTROLS).expect("H2.1c owner controls JSON");
    assert_eq!(artifact["phase"], "H2.1c-amd-umd-owner-controls");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["summary"]["exact_outputs"], 2);
    let control = &artifact["controls"][0];
    assert_eq!(
        control["control_id"],
        "amd-module-dependency-and-static-import-order"
    );

    for run in control["runs"].as_array().expect("owner-control runs") {
        let module = run["module_value"].as_i64().expect("module value") as i32;
        let expected = oracle_text(&run["output"]);
        let mut builder = PreparedProgram::emitting_builder(
            PathContext::new(path("/project"), true),
            CompilerOptions {
                no_emit: Some(false),
                target: Some(99),
                module: Some(module),
                ignore_deprecations: Some("6.0".to_owned()),
                es_module_interop: Some(true),
                ..CompilerOptions::default()
            },
        );
        let mut source_ids = BTreeMap::new();
        for file in control["input"]["files"]
            .as_array()
            .expect("owner-control files")
        {
            let file_name = file["path"].as_str().expect("owner-control path");
            let text = oracle_text(file);
            let source = PreparedSourceFile::new(path(file_name), text)
                .with_may_be_emitted(file["emit_eligible"].as_bool().expect("emit eligibility"));
            let source_id = builder.add_source_file(source).expect("add control source");
            source_ids.insert(file_name.to_owned(), source_id);
        }
        let root = control["input"]["root"].as_str().expect("control root");
        builder
            .add_root_file(source_ids[root])
            .expect("add control root");
        for resolution in control["input"]["module_resolutions"]
            .as_array()
            .expect("module resolutions")
        {
            let origin = resolution["origin"].as_str().expect("resolution origin");
            let specifier = resolution["specifier"]
                .as_str()
                .expect("resolution specifier");
            let resolved_file = resolution["target"].as_str().expect("resolution target");
            builder
                .add_module_resolution(
                    ResolutionKey::new(
                        path(origin).canonical().clone(),
                        specifier,
                        ResolutionMode::Unspecified,
                    ),
                    Ok(source_resolution(
                        source_ids[resolved_file],
                        resolved_file,
                        ModuleExtension::Ts,
                    )),
                )
                .expect("add authoritative source resolution");
        }
        let prepared = builder.build().expect("prepared named module program");
        let mut sink = MemoryOutputSink::new();
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.1c named asynchronous module emit");
        assert!(outcome.diagnostics().is_empty());
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(sink.writes()[0].callback_text(), expected.as_str());
    }
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
    let activity = outcome.h2_activity();
    assert_eq!(activity.emit_session_constructions(), 1);
    assert_eq!(activity.output_plan_constructions(), 1);
    assert_eq!(activity.emit_resolver_borrows(), 0);
    assert_eq!(activity.script_transformer_list_constructions(), 0);
    assert_eq!(activity.transform_context_constructions(), 0);
    assert_eq!(activity.printer_constructions(), 1);
    assert_eq!(activity.javascript_artifact_creations(), 0);
    assert_eq!(activity.output_sink_write_attempts(), 0);
    assert_eq!(activity.output_sink_failures(), 0);
    assert_h2_runtime_zero(activity);
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

    let mut source_map = base();
    source_map.source_map = Some(true);
    let mut sink = CountingSink::default();
    let error = ProgramSession::new(prepared_with_sources(
        source_map,
        &[("/project/mapped.ts", "export const mapped = true;\n")],
    ))
    .emit(&mut sink)
    .expect_err("source maps are outside H1");
    assert_eq!(
        error,
        DriverError::Emit(EmitFailure::UnsupportedCompilerOption {
            option: "sourceMap"
        })
    );
    assert_eq!(sink.writes, 0);

    let mut out_file = base();
    out_file.module = Some(2);
    out_file.out_file = Some("/project/bundle.js".to_owned());
    let mut sink = CountingSink::default();
    let error = ProgramSession::new(prepared_with_sources(
        out_file,
        &[("/project/bundled.ts", "export const bundled = true;\n")],
    ))
    .emit(&mut sink)
    .expect_err("AMD outFile remains owned by a later bundle slice");
    assert_eq!(
        error,
        DriverError::Emit(EmitFailure::UnsupportedCompilerOption { option: "outFile" })
    );
    assert_eq!(sink.writes, 0);

    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared_with_sources(
        base(),
        &[("/project/module.tsx", "export const value = true;\n")],
    ))
    .emit(&mut sink)
    .expect("H2.3b admits TSX source/output routing");
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(sink.writes()[0].path(), Path::new("/project/module.js"));
    assert_eq!(
        sink.writes()[0].callback_text(),
        "export const value = true;\n"
    );
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3b),
        1
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
        outcome.emitted_files(),
        Some([PathBuf::from("/project/sibling.js")].as_slice())
    );
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(sink.writes()[0].path(), Path::new("/project/sibling.js"));
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3a),
        1
    );
    for slice in H2RuntimeSlice::ALL {
        if slice != H2RuntimeSlice::H2_3a {
            assert_eq!(outcome.h2_activity().runtime_slice(slice), 0);
        }
    }
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
                out_dir: Some("/project/dist".to_owned()),
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
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3a),
            1
        );
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(
            sink.writes()[0].path(),
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
                out_dir: Some("/project/dist".to_owned()),
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
        assert_eq!(sink.writes()[0].path(), Path::new(&output));
        assert_eq!(sink.writes()[0].callback_text(), SOURCE);
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3a),
            1
        );
    }
}

#[test]
fn h2_3a_javascript_owner_controls_match_pinned_typescript() {
    let artifact: Value =
        serde_json::from_slice(H2_3A_OWNER_CONTROLS).expect("H2.3a owner controls JSON");
    assert_eq!(artifact["phase"], "H2.3a-javascript-source-owner-controls");
    assert_eq!(artifact["status"], "qualified");
    assert_eq!(artifact["summary"]["exact_outputs"], 9);
    let control = &artifact["controls"][0];
    assert_eq!(
        control["control_id"],
        "javascript-family-relocation-and-checking"
    );
    let sources = control["input"]["files"]
        .as_array()
        .expect("owner-control files")
        .iter()
        .map(|file| {
            (
                file["path"].as_str().expect("owner-control path"),
                oracle_text(file),
            )
        })
        .collect::<Vec<_>>();

    for variant in control["variants"]
        .as_array()
        .expect("owner-control variants")
    {
        let check_js = match variant["check_js_state"].as_str().expect("checkJs state") {
            "absent" => None,
            "false" => Some(false),
            "true" => Some(true),
            other => panic!("unexpected checkJs state {other}"),
        };
        let source_refs = sources
            .iter()
            .map(|(file_name, text)| (*file_name, text.as_str()))
            .collect::<Vec<_>>();
        let prepared = prepared_with_sources_and_minimal_lib(
            CompilerOptions {
                allow_js: true,
                check_js,
                no_emit: Some(false),
                target: Some(99),
                module: Some(200),
                out_dir: Some("/project/dist".to_owned()),
                new_line: Some(1),
                ignore_deprecations: Some("6.0".to_owned()),
                ..CompilerOptions::default()
            },
            &source_refs,
        );
        let mut sink = MemoryOutputSink::new();
        let (outcome, diagnostics) = ProgramSession::new(prepared)
            .emit_with_reported_diagnostics_for_harness(&mut sink)
            .expect("H2.3a JavaScript owner-control emit");
        let observation = &variant["observation"];
        assert_eq!(outcome.emit_skipped(), observation["emit_skipped"] == true);
        assert!(outcome.diagnostics().is_empty());

        let expected_diagnostics = observation["reported_diagnostics"]
            .as_array()
            .expect("owner-control diagnostics");
        assert_eq!(diagnostics.len(), expected_diagnostics.len());
        for (actual, expected) in diagnostics.iter().zip(expected_diagnostics) {
            assert_eq!(u64::from(actual.code()), expected["code"]);
            assert_eq!(format!("{:?}", actual.category()), expected["category"]);
            assert_eq!(actual.file_name.as_deref(), expected["file"].as_str());
            assert_eq!(actual.start.map(u64::from), expected["start"].as_u64());
            assert_eq!(actual.length.map(u64::from), expected["length"].as_u64());
            assert_eq!(actual.message_text(), expected["message"]);
        }

        let expected_writes = observation["writes"]
            .as_array()
            .expect("owner-control writes");
        assert_eq!(sink.writes().len(), expected_writes.len());
        for (actual, expected) in sink.writes().iter().zip(expected_writes) {
            assert_eq!(
                actual.path(),
                Path::new(expected["path"].as_str().expect("owner output path"))
            );
            assert_eq!(actual.callback_text(), oracle_callback_text(expected));
            assert_eq!(
                actual.write_byte_order_mark(),
                expected["write_byte_order_mark"] == true
            );
            let expected_sources = expected["source_files"]
                .as_array()
                .expect("owner output sources")
                .iter()
                .map(|source| source.as_str().expect("owner source path"))
                .collect::<Vec<_>>();
            let actual_sources = actual
                .source_files()
                .expect("owner output provenance")
                .iter()
                .map(|source| source.to_string_lossy())
                .collect::<Vec<_>>();
            assert_eq!(
                actual_sources
                    .iter()
                    .map(|source| source.as_ref())
                    .collect::<Vec<_>>(),
                expected_sources
            );
        }
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3a),
            sources.len() as u64
        );
        for slice in H2RuntimeSlice::ALL {
            if slice != H2RuntimeSlice::H2_3a {
                assert_eq!(outcome.h2_activity().runtime_slice(slice), 0);
            }
        }
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
            ignore_deprecations: Some("6.0".to_owned()),
            ..CompilerOptions::default()
        },
        &[("/project/emoji-😀.tsx", SOURCE)],
    );
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("H2.3b classic JSX emit");
    assert_eq!(sink.writes().len(), 1);
    assert_eq!(sink.writes()[0].path(), Path::new("/project/emoji-😀.js"));
    assert_eq!(sink.writes()[0].callback_text(), EXPECTED);
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3b),
        1
    );
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
            ignore_deprecations: Some("6.0".to_owned()),
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
    assert_eq!(sink.writes()[0].path(), Path::new("/project/view.js"));
    assert_eq!(sink.writes()[0].callback_text(), EXPECTED);
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_1b),
        1
    );
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3b),
        1
    );
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
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.3b preserved JSX emit");
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(
            sink.writes()[0].path(),
            Path::new(&format!("/project/view.{extension}"))
        );
        assert_eq!(
            sink.writes()[0].callback_text(),
            "const view = <Box value={answer}><span /></Box>;\n"
        );
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3b),
            1
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
        out_dir: Some(out_dir.to_owned()),
        ..CompilerOptions::default()
    };
    for (case, compiler_options, source) in [
        (
            "TypeScript-only outDir remains H2.8a",
            options("/project/dist", false),
            ("/project/input.ts", "const value: number = 1;\n"),
        ),
        (
            "relative JavaScript outDir remains H2.8a",
            options("dist", true),
            ("/project/input.js", "const value = 1;\n"),
        ),
    ] {
        let mut sink = CountingSink::default();
        let error = ProgramSession::new(prepared_with_sources(compiler_options, &[source]))
            .emit(&mut sink)
            .expect_err(case);
        assert_eq!(
            error,
            DriverError::Emit(EmitFailure::UnsupportedCompilerOption { option: "outDir" }),
            "{case}"
        );
        assert_eq!(sink.writes, 0, "{case}");
    }

    let mut sink = CountingSink::default();
    let error = ProgramSession::new(prepared_with_sources(
        options("/project/dist", true),
        &[
            ("/project/input.js", "const js = 1;\n"),
            ("/project/input.ts", "const ts: number = 1;\n"),
        ],
    ))
    .emit(&mut sink)
    .expect_err("mixed-source outDir remains H2.8a");
    assert_eq!(
        error,
        DriverError::Emit(EmitFailure::UnsupportedCompilerOption { option: "outDir" })
    );
    assert_eq!(sink.writes, 0);

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
    let outcome = ProgramSession::new(prepared_with_sources(
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
    assert_eq!(sink.writes()[0].path(), Path::new("/project/input.js"));
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3a),
        1
    );
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3b),
        1
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
                out_dir: Some("/project/dist".to_owned()),
                new_line: Some(1),
                ignore_deprecations: Some("6.0".to_owned()),
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
            sink.writes()[0].path(),
            Path::new("/project/dist/data.json"),
            "module={module}"
        );
        assert_eq!(
            sink.writes()[0].callback_text(),
            EXPECTED,
            "module={module}"
        );
        assert!(!sink.writes()[0].write_byte_order_mark(), "module={module}");
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3d),
            1,
            "module={module}"
        );
    }

    let prepared = prepared_with_sources(
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(1),
            resolve_json_module: Some(true),
            out_dir: Some("/project/dist".to_owned()),
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
    for out_dir in [None, Some("/project".to_owned())] {
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
    for (module, module_resolution, expected_code) in [(200, 1, 5070), (3, 2, 5071), (4, 2, 5071)] {
        for no_emit_on_error in [false, true] {
            let prepared = prepared_with_sources_and_minimal_lib(
                CompilerOptions {
                    no_emit: Some(false),
                    no_emit_on_error: Some(no_emit_on_error),
                    target: Some(99),
                    module: Some(module),
                    module_resolution: Some(module_resolution),
                    resolve_json_module: Some(true),
                    out_dir: Some("/project/dist".to_owned()),
                    ignore_deprecations: Some("6.0".to_owned()),
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
                [expected_code],
                "module={module} moduleResolution={module_resolution} noEmitOnError={no_emit_on_error}"
            );
            assert_eq!(outcome.emit_skipped(), no_emit_on_error);
            assert_eq!(sink.writes().len(), usize::from(!no_emit_on_error));
            assert_eq!(
                outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_3d),
                1
            );
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
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_4b),
        1
    );
}

#[test]
fn filesystem_failure_at_each_write_index_preserves_partial_set_and_continuation() {
    assert_filesystem_failure_at_each_write_index(200, &[]);
}

#[test]
fn h2_1a_filesystem_failure_preserves_partial_set_continuation_and_activity() {
    assert_filesystem_failure_at_each_write_index(99, &[(H2RuntimeSlice::H2_1a, 2)]);
}

#[test]
fn h2_1b_commonjs_filesystem_failure_preserves_partial_set_continuation_and_activity() {
    assert_filesystem_failure_at_each_write_index(
        1,
        &[(H2RuntimeSlice::H2_1a, 2), (H2RuntimeSlice::H2_1b, 2)],
    );
}

#[test]
fn h2_1c_amd_umd_filesystem_failure_preserves_partial_set_continuation_and_activity() {
    for module in [2, 3] {
        assert_filesystem_failure_at_each_write_index(
            module,
            &[
                (H2RuntimeSlice::H2_1a, 2),
                (H2RuntimeSlice::H2_1b, 2),
                (H2RuntimeSlice::H2_1c, 2),
            ],
        );
    }
}

#[test]
fn h2_1d_system_filesystem_failure_preserves_partial_set_continuation_and_activity() {
    assert_filesystem_failure_at_each_write_index(4, &[(H2RuntimeSlice::H2_1d, 2)]);
}

#[test]
fn h2_1e_node_format_filesystem_failure_preserves_partial_set_continuation_and_activity() {
    assert_filesystem_failure_at_each_write_index(
        199,
        &[(H2RuntimeSlice::H2_1a, 2), (H2RuntimeSlice::H2_1e, 2)],
    );
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
    let outcome = ProgramSession::new(prepared)
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
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_1a),
        1
    );
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_1e),
        1
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
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_2a),
            1
        );
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
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_2b),
            1
        );
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
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_2c),
            1
        );
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
        let outcome = ProgramSession::new(prepared)
            .emit(&mut sink)
            .expect("H2.2d export-equals emit");
        assert_eq!(sink.writes().len(), 1);
        assert_eq!(sink.writes()[0].callback_text(), expected);
        assert_eq!(
            outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_2d),
            1
        );
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
    let outcome = ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect("H2.2d internal import-equals emit");
    assert_eq!(
        sink.writes()[0].callback_text(),
        concat!("\"use strict\";\n", "var value = Runtime.value;\n")
    );
    assert_eq!(
        outcome.h2_activity().runtime_slice(H2RuntimeSlice::H2_2d),
        1
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

fn assert_filesystem_failure_at_each_write_index(
    module: i32,
    expected_runtime_activity: &[(H2RuntimeSlice, u64)],
) {
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
            outcome.emitted_files(),
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
        let activity = outcome.h2_activity();
        assert_eq!(activity.emit_session_constructions(), 1);
        assert_eq!(activity.output_plan_constructions(), 1);
        assert_eq!(activity.emit_resolver_borrows(), 1);
        assert_eq!(activity.script_transformer_list_constructions(), 2);
        assert_eq!(activity.transform_typescript_constructions(), 2);
        assert_eq!(activity.transform_class_fields_constructions(), 2);
        assert_eq!(
            activity.transform_ecmascript_module_constructions(),
            if module == 4 { 0 } else { 2 }
        );
        assert_eq!(activity.transform_context_constructions(), 2);
        assert_eq!(activity.printer_constructions(), 1);
        assert_eq!(activity.javascript_artifact_creations(), 2);
        assert_eq!(activity.output_sink_write_attempts(), 2);
        assert_eq!(activity.output_sink_failures(), 1);
        for slice in H2RuntimeSlice::ALL {
            let expected = expected_runtime_activity
                .iter()
                .find_map(|(expected_slice, count)| (*expected_slice == slice).then_some(*count))
                .unwrap_or(0);
            assert_eq!(activity.runtime_slice(slice), expected);
        }
    }
}
