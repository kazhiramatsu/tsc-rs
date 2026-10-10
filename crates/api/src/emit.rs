//! tsgo `api/session.go` (19dadef8) emit methods (session.go:3564-3720):
//! `emit` writes a program's outputs through to the session's file system
//! (or, when the snapshot reads a full file system, returns them),
//! `emitToString` returns them, and `getJavaScriptEmit`/
//! `getDeclarationEmit` return the forced outputs of some of its files.

use tsc_compiler::system::SystemEmitFileSystem;
use tsc_compiler::{
    EmitArtifact, EmitIoError, EmitOnly, EmitResult, EmitWriteDisposition, FsOutputSink, OutputSink,
};

use crate::ipc::Payload;
use crate::proto::{
    EmitOutputFile, EmitOutputResponse, EmitParams, EmitResponse, SelectedFilesEmitParams,
    SnapshotId,
};
use crate::session::{client_error, json, parse, Session};

impl Session {
    /// tsgo's emit methods; `None` for another method.
    pub(crate) fn handle_emit_request(
        &self,
        method: &str,
        params: &[u8],
    ) -> Option<Result<Payload, String>> {
        Some(match method {
            "emit" => {
                parse::<EmitParams>("EmitParams", params).and_then(|params| self.emit(&params))
            }
            "emitToString" => parse::<EmitParams>("EmitParams", params)
                .and_then(|params| self.emit_to_string(&params)),
            "getJavaScriptEmit" | "getDeclarationEmit" => {
                parse::<SelectedFilesEmitParams>("SelectedFilesEmitParams", params).and_then(
                    |params| self.selected_files_emit(&params, method == "getJavaScriptEmit"),
                )
            }
            _ => return None,
        })
    }

    /// tsgo `handleEmit`: a full file system is immutable, so the outputs
    /// come back in the response; another writes through to the session's
    /// file system (tsgo `snapshotHost.FS().WriteFile`).
    fn emit(&self, params: &EmitParams) -> Result<Payload, String> {
        let (_, program) = self.program(params.snapshot, &params.project)?;
        let emit_only = emit_only(params.emit_only)?;
        let full_file_system = self
            .snapshot_file_system(params.snapshot)?
            .is_some_and(|file_system| file_system.is_full());
        let (result, outputs) = if full_file_system {
            let mut outputs = OutputFiles::default();
            let result = program
                .with_live(|live| live.emit(emit_only, &mut outputs))
                .map_err(|error| error.to_string())?;
            (result, Some(outputs))
        } else {
            let mut file_system = SystemEmitFileSystem::new(self.file_system(), false);
            let mut sink = FsOutputSink::new(&mut file_system);
            let result = program
                .with_live(|live| live.emit(emit_only, &mut sink))
                .map_err(|error| error.to_string())?;
            (result, None)
        };
        let emitted_files = result
            .emitted_files
            .iter()
            .map(|file| file.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let emitted_files_contents = match outputs {
            Some(outputs) => emitted_files
                .iter()
                .map(|file| outputs.text(file))
                .collect(),
            None => Vec::new(),
        };
        Ok(json(&EmitResponse {
            emit_skipped: result.emit_skipped,
            diagnostics: self.diagnostic_responses(
                params.snapshot,
                &program,
                &result.diagnostics,
            )?,
            emitted_files,
            emitted_files_contents,
        }))
    }

    /// tsgo `handleEmitToString`.
    fn emit_to_string(&self, params: &EmitParams) -> Result<Payload, String> {
        let (_, program) = self.program(params.snapshot, &params.project)?;
        let emit_only = emit_only(params.emit_only)?;
        let mut outputs = OutputFiles::default();
        let result = program
            .with_live(|live| live.emit(emit_only, &mut outputs))
            .map_err(|error| error.to_string())?;
        self.emit_output(params.snapshot, &program, result, outputs)
    }

    /// tsgo `handleSelectedFilesEmit`: the files (each resolved first) emitted
    /// with `ForceEmit` and `EmitOnlyJs` or `EmitOnlyDts`.
    fn selected_files_emit(
        &self,
        params: &SelectedFilesEmitParams,
        javascript: bool,
    ) -> Result<Payload, String> {
        let (_, program) = self.program(params.snapshot, &params.project)?;
        let files = params
            .files
            .as_ref()
            .ok_or_else(|| client_error("files is required"))?
            .iter()
            .map(|file| {
                self.required_source_file(&program, file)
                    .map(|file| file.index)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut outputs = OutputFiles::default();
        let result = program
            .with_live(|live| live.emit_forced(&files, javascript, &mut outputs))
            .map_err(|error| error.to_string())?;
        self.emit_output(params.snapshot, &program, result, outputs)
    }

    /// tsgo `emitToOutput`'s response: the outputs sorted by name.
    fn emit_output(
        &self,
        snapshot: SnapshotId,
        program: &tsc_project::ProjectProgram,
        result: EmitResult,
        mut outputs: OutputFiles,
    ) -> Result<Payload, String> {
        outputs
            .0
            .sort_by(|a, b| a.file_name.as_bytes().cmp(b.file_name.as_bytes()));
        Ok(json(&EmitOutputResponse {
            emit_skipped: result.emit_skipped,
            diagnostics: self.diagnostic_responses(snapshot, program, &result.diagnostics)?,
            output_files: outputs.0,
        }))
    }
}

/// tsgo `getEmitOnly`: all outputs when absent; a value past `EmitOnlyDts`
/// is the client's error.
fn emit_only(value: Option<u32>) -> Result<EmitOnly, String> {
    match value {
        None | Some(0) => Ok(EmitOnly::All),
        Some(1) => Ok(EmitOnly::Js),
        Some(2) => Ok(EmitOnly::Dts),
        Some(value) => Err(client_error(format!("invalid emitOnly value: {value}"))),
    }
}

/// The outputs as the emit writes them (tsgo's write callback of
/// `emitToOutput`): each file's name, text and source file.
#[derive(Default)]
struct OutputFiles(Vec<EmitOutputFile>);

impl OutputFiles {
    /// The text last written to `file_name` (tsgo's map of outputs).
    fn text(&self, file_name: &str) -> String {
        self.0
            .iter()
            .rev()
            .find(|output| output.file_name == file_name)
            .map(|output| output.text.clone())
            .unwrap_or_default()
    }
}

impl OutputSink for OutputFiles {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.0.push(EmitOutputFile {
            file_name: artifact.path().to_string_lossy().into_owned(),
            text: String::from_utf8_lossy(&artifact.materialized_bytes()).into_owned(),
            source_file_name: artifact
                .source_files()
                .and_then(<[_]>::first)
                .map(|file| file.to_string_lossy().into_owned()),
        });
        Ok(EmitWriteDisposition::Written)
    }
}
