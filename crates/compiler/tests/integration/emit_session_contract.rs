use tsc_compiler::{
    DriverError, EmitArtifact, EmitFailure, EmitIoError, EmitStage, EmitWriteDisposition,
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
    let mut builder = PreparedProgram::emitting_builder(
        PathContext::new(path("/project"), true),
        CompilerOptions {
            no_emit: Some(false),
            target: Some(99),
            module: Some(200),
            ..CompilerOptions::default()
        },
    );
    let source = builder
        .add_source_file(PreparedSourceFile::new(
            path("/project/input.ts"),
            "export const value: number = 1;\n",
        ))
        .expect("add source");
    builder.add_root_file(source).expect("add root");
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

#[test]
fn h1_1_emit_entry_fails_typed_before_the_first_sink_call() {
    let mut sink = CountingSink::default();
    let error = ProgramSession::new(prepared_for_emit())
        .emit(&mut sink)
        .expect_err("H1.2 transform and print is not connected yet");

    assert_eq!(
        error,
        DriverError::Emit(EmitFailure::StageUnavailable(EmitStage::TransformAndPrint))
    );
    assert_eq!(sink.writes, 0);
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
