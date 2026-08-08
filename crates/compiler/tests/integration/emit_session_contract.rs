use tsc_compiler::{
    DriverError, EmitArtifact, EmitFailure, EmitIoError, EmitWriteDisposition, MemoryOutputSink,
    OutputSink, ProgramSession,
};
use tsc_program::{CompilerOptions, PathContext, PreparedProgram, PreparedSourceFile, ProgramPath};

#[derive(Default)]
struct CountingSink {
    writes: usize,
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
        sink.writes()[0].path(),
        std::path::Path::new("/project/input.js")
    );
    assert_eq!(
        sink.writes()[0].callback_text(),
        "export const value = 1;\n"
    );
    assert!(!sink.writes()[0].write_byte_order_mark());
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
