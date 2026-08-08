use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tsc_compiler::{
    DriverError, EmitArtifact, EmitFailure, EmitFileSystem, EmitIoError, EmitWriteDisposition,
    FsOutputSink, H2ActivityCounters, H2RuntimeSlice, MemoryOutputSink, OutputSink, ProgramSession,
};
use tsc_program::ResolutionMode;
use tsc_program::{CompilerOptions, PathContext, PreparedProgram, PreparedSourceFile, ProgramPath};

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
fn empty_emit_program_preserves_present_empty_observations_without_a_resolver() {
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(empty_emit_program())
        .emit(&mut sink)
        .expect("empty H1.4 emit");

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
fn unsupported_options_and_extensions_fail_before_the_first_sink_call() {
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

    let mut sink = CountingSink::default();
    let error = ProgramSession::new(prepared_with_sources(
        base(),
        &[("/project/module.mts", "export const value = true;\n")],
    ))
    .emit(&mut sink)
    .expect_err("mts output is outside H1");
    assert!(matches!(
        error,
        DriverError::Emit(EmitFailure::UnsupportedSourceExtension { .. })
    ));
    assert_eq!(sink.writes, 0);
}

#[test]
fn a_later_unsupported_source_cannot_leave_an_earlier_partial_write() {
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
            ("/project/second.ts", "export enum Direction { Up, Down }\n"),
        ],
    );
    let mut sink = CountingSink::default();
    let error = ProgramSession::new(prepared)
        .emit(&mut sink)
        .expect_err("runtime enum is outside the bootstrap syntax profile");
    assert!(matches!(
        error,
        DriverError::Emit(EmitFailure::Transform(_))
    ));
    assert_eq!(sink.writes, 0);
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
        assert_eq!(activity.transform_ecmascript_module_constructions(), 2);
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
