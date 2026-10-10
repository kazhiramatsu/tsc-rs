//! tsgo `api/session.go` (19dadef8) diagnostics methods (session.go:
//! 4779-4900): a program's diagnostics of each kind, for some of its files
//! (each file's in turn) or for all of them (sorted), located in their
//! files' texts.

use tsc_compiler::LiveProgram;
use tsc_diagnostics::{sort_and_dedupe_diagnostics, DiagnosticList};

use crate::ipc::Payload;
use crate::proto::{GetDiagnosticsParams, ProjectParams};
use crate::session::{json, parse, Session};

/// The per-file diagnostics methods (tsgo `getDiagnostics` and its
/// getters).
#[derive(Clone, Copy)]
enum FileDiagnostics {
    Syntactic,
    Bind,
    Semantic,
    Suggestion,
    Declaration,
}

impl Session {
    /// tsgo's diagnostics methods; `None` for another method.
    pub(crate) fn handle_diagnostics_request(
        &self,
        method: &str,
        params: &[u8],
    ) -> Option<Result<Payload, String>> {
        let kind = match method {
            "getSyntacticDiagnostics" => FileDiagnostics::Syntactic,
            "getBindDiagnostics" => FileDiagnostics::Bind,
            "getSemanticDiagnostics" => FileDiagnostics::Semantic,
            "getSuggestionDiagnostics" => FileDiagnostics::Suggestion,
            "getDeclarationDiagnostics" => FileDiagnostics::Declaration,
            "getProgramDiagnostics"
            | "getGlobalDiagnostics"
            | "getConfigFileParsingDiagnostics" => {
                return Some(
                    parse::<ProjectParams>("GetProjectDiagnosticsParams", params)
                        .and_then(|params| self.project_diagnostics(method, &params)),
                );
            }
            _ => return None,
        };
        Some(
            parse::<GetDiagnosticsParams>("GetDiagnosticsParams", params)
                .and_then(|params| self.file_diagnostics(kind, &params)),
        )
    }

    /// tsgo `getDiagnostics`: the diagnostics of each of the files in turn
    /// (each resolved before its diagnostics are computed), or the whole
    /// program's (its libraries included), sorted.
    fn file_diagnostics(
        &self,
        kind: FileDiagnostics,
        params: &GetDiagnosticsParams,
    ) -> Result<Payload, String> {
        let (_, program) = self.program(params.snapshot, &params.project)?;
        let diagnostics = match &params.files {
            Some(files) => {
                let mut diagnostics = Vec::new();
                for file in files {
                    let file = self.required_source_file(&program, file)?;
                    diagnostics
                        .extend(program.with_live(|live| of_files(live, kind, &[file.index]))?);
                }
                diagnostics
            }
            None => {
                let mut diagnostics = program.with_live(|live| {
                    let files = (0..live.file_count()).collect::<Vec<_>>();
                    of_files(live, kind, &files)
                })?;
                sort_and_dedupe_diagnostics(&mut diagnostics);
                diagnostics
            }
        };
        Ok(json(&self.diagnostic_responses(
            params.snapshot,
            &program,
            &diagnostics,
        )?))
    }

    /// tsgo `handleGetConfigFileParsingDiagnostics`,
    /// `handleGetProgramDiagnostics` and `handleGetGlobalDiagnostics` (the
    /// project's file-less rows after a whole-program semantic pass on the
    /// diagnostics checker: its config, program and checker rows).
    fn project_diagnostics(&self, method: &str, params: &ProjectParams) -> Result<Payload, String> {
        let (_, program) = self.program(params.snapshot, &params.project)?;
        let diagnostics = program.with_live(|live| -> Result<DiagnosticList, String> {
            Ok(match method {
                "getConfigFileParsingDiagnostics" => {
                    live.config_file_parsing_diagnostics().to_vec()
                }
                "getProgramDiagnostics" => live.program_diagnostics(),
                _ => {
                    for file in 0..live.file_count() {
                        live.semantic_diagnostics(file)
                            .map_err(|error| error.to_string())?;
                    }
                    let mut diagnostics = live.config_file_parsing_diagnostics().to_vec();
                    diagnostics.extend(live.program_diagnostics());
                    diagnostics.extend(live.global_diagnostics());
                    sort_and_dedupe_diagnostics(&mut diagnostics);
                    diagnostics.retain(|diagnostic| diagnostic.file_name.is_none());
                    diagnostics
                }
            })
        })?;
        Ok(json(&self.diagnostic_responses(
            params.snapshot,
            &program,
            &diagnostics,
        )?))
    }
}

/// The diagnostics of `kind` of the program's `files` (checker indices),
/// each file's sorted, in turn.
fn of_files(
    live: &mut LiveProgram,
    kind: FileDiagnostics,
    files: &[usize],
) -> Result<DiagnosticList, String> {
    let lists = match kind {
        FileDiagnostics::Syntactic => files
            .iter()
            .map(|&file| live.syntactic_diagnostics(file))
            .collect(),
        FileDiagnostics::Bind => files
            .iter()
            .map(|&file| live.bind_diagnostics(file))
            .collect(),
        FileDiagnostics::Semantic => files
            .iter()
            .map(|&file| live.semantic_diagnostics(file))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?,
        FileDiagnostics::Suggestion => files
            .iter()
            .map(|&file| live.suggestion_diagnostics(file))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?,
        FileDiagnostics::Declaration => live
            .declaration_diagnostics(files)
            .map_err(|error| error.to_string())?,
    };
    Ok(lists.concat())
}
