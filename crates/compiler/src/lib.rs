#![forbid(unsafe_code)]

//! One-shot execution of an owned H0 prepared program.
//!
//! This crate is the dependency boundary between the owned program contract
//! and the parser/binder/checker implementation. A [`ProgramSession`] owns
//! exactly one [`PreparedProgram`], projects its already-final source order
//! into the checker, and is consumed by [`ProgramSession::run`].

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use tsc_checker::{check_program_with_owned_libs_at, InputFile};
use tsc_diagnostics::{sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList};
use tsc_program::{MissingResolutionError, PreparedProgram, PreparedSourceFile, SourceFileId};

/// A one-shot owner for one prepared no-emit program.
///
/// The consuming [`run`](Self::run) method keeps every parser, binder, and
/// checker borrow inside the call. No retained checker or self-referential
/// session escapes this boundary.
#[derive(Debug)]
pub struct ProgramSession {
    prepared: PreparedProgram,
}

impl ProgramSession {
    pub fn new(prepared: PreparedProgram) -> Self {
        Self { prepared }
    }

    /// Consume the prepared program and execute the no-emit diagnostic pass.
    ///
    /// H0.1d connects owned source, option, path, and diagnostic data. It does
    /// not yet route [`PreparedProgram::resolutions`] into checker lookups;
    /// authoritative resolution-table consumption remains H0.2 work. The
    /// [`DriverError::MissingResolution`] variant is reserved for that fail-
    /// closed connection and is never fabricated by this slice.
    pub fn run(self) -> Result<NoEmitOutcome, DriverError> {
        let (libs, files, current_directory) = project_checker_inputs(&self.prepared)?;
        let has_roots = !self.prepared.roots().is_empty();
        let checked = check_program_with_owned_libs_at(
            &libs,
            &files,
            self.prepared.compiler_options(),
            &current_directory,
        );

        let preparation = self.prepared.diagnostics();
        let config_diagnostics = preparation.config().to_vec();
        let mut syntactic_diagnostics = checked.syntactic_diagnostics;
        sort_and_dedupe_diagnostics(&mut syntactic_diagnostics);
        let partial_checks = checked.partial_checks;

        // Program-construction diagnostics are part of tsc's
        // combined diagnostic map. File-less rows and rows owned by config
        // or other auxiliary files feed getOptionsDiagnostics; rows owned by
        // a program SourceFile feed that source's getSemanticDiagnostics.
        // Each public getter applies sortAndDeduplicateDiagnostics to its
        // combined result.
        let mut available_options = preparation.options().to_vec();
        let mut available_semantic = checked.semantic_diagnostics;
        for diagnostic in preparation.program() {
            if diagnostic
                .file_name
                .as_deref()
                .is_some_and(|file_name| prepared_source_owns_diagnostic(&self.prepared, file_name))
            {
                available_semantic.push(diagnostic.clone());
            } else {
                available_options.push(diagnostic.clone());
            }
        }
        sort_and_dedupe_diagnostics(&mut available_options);
        sort_and_dedupe_diagnostics(&mut available_semantic);

        // emitFilesAndReportErrors compares the aggregate length with the
        // original config-diagnostic length. Config errors therefore remain
        // visible but do not themselves close any of the later gates.
        let (options_diagnostics, global_diagnostics, semantic_diagnostics) =
            if syntactic_diagnostics.is_empty() {
                let options_diagnostics = available_options;
                let global_diagnostics = if has_roots {
                    checked.global_diagnostics
                } else {
                    Vec::new()
                };
                let semantic_diagnostics =
                    if options_diagnostics.is_empty() && global_diagnostics.is_empty() {
                        if let Some(partial) = partial_checks.first() {
                            return Err(DriverError::IncompleteCheck {
                                file_name: partial.file_name.clone(),
                                start: partial.start,
                                length: partial.length,
                                reason: partial.reason.clone(),
                                additional_partial_checks: partial_checks.len().saturating_sub(1),
                            });
                        }
                        available_semantic
                    } else {
                        Vec::new()
                    };
                (
                    options_diagnostics,
                    global_diagnostics,
                    semantic_diagnostics,
                )
            } else {
                (Vec::new(), Vec::new(), Vec::new())
            };

        // `checked.suggestion_diagnostics` is deliberately dropped here.
        // Suggestions remain a legacy per-file getter surface and are not
        // part of `tsc --noEmit` command output.
        Ok(NoEmitOutcome {
            config_diagnostics,
            syntactic_diagnostics,
            options_diagnostics,
            global_diagnostics,
            semantic_diagnostics,
        })
    }
}

/// The five diagnostic collections exposed by the no-emit driver.
///
/// Buckets retain their getter-local ordering. [`diagnostics`](Self::diagnostics)
/// and [`into_diagnostics`](Self::into_diagnostics) expose the command driver
/// order without re-sorting across bucket boundaries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NoEmitOutcome {
    config_diagnostics: DiagnosticList,
    syntactic_diagnostics: DiagnosticList,
    options_diagnostics: DiagnosticList,
    global_diagnostics: DiagnosticList,
    semantic_diagnostics: DiagnosticList,
}

impl NoEmitOutcome {
    pub fn config_diagnostics(&self) -> &[Diagnostic] {
        &self.config_diagnostics
    }

    pub fn syntactic_diagnostics(&self) -> &[Diagnostic] {
        &self.syntactic_diagnostics
    }

    pub fn options_diagnostics(&self) -> &[Diagnostic] {
        &self.options_diagnostics
    }

    pub fn global_diagnostics(&self) -> &[Diagnostic] {
        &self.global_diagnostics
    }

    pub fn semantic_diagnostics(&self) -> &[Diagnostic] {
        &self.semantic_diagnostics
    }

    /// Iterate in the no-emit command's bucket order.
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.config_diagnostics
            .iter()
            .chain(&self.syntactic_diagnostics)
            .chain(&self.options_diagnostics)
            .chain(&self.global_diagnostics)
            .chain(&self.semantic_diagnostics)
    }

    /// Consume the outcome and flatten it in no-emit command bucket order.
    pub fn into_diagnostics(self) -> DiagnosticList {
        let capacity = self.config_diagnostics.len()
            + self.syntactic_diagnostics.len()
            + self.options_diagnostics.len()
            + self.global_diagnostics.len()
            + self.semantic_diagnostics.len();
        let mut diagnostics = Vec::with_capacity(capacity);
        diagnostics.extend(self.config_diagnostics);
        diagnostics.extend(self.syntactic_diagnostics);
        diagnostics.extend(self.options_diagnostics);
        diagnostics.extend(self.global_diagnostics);
        diagnostics.extend(self.semantic_diagnostics);
        diagnostics
    }
}

/// A fail-closed failure while projecting trusted prepared data into the
/// checker execution boundary.
///
/// [`PreparedProgram`] construction already rejects the projection variants;
/// `IncompleteCheck` rejects checker containment after execution, and
/// `MissingResolution` reserves the fail-closed H0.2 table connection. The
/// typed boundary prevents any of them from becoming a partial success.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DriverError {
    InvalidLibraryPrefix {
        position: usize,
        source_file: SourceFileId,
    },
    MissingPreparedSource {
        source_file: SourceFileId,
    },
    NonUnicodeDisplayPath {
        source_file: Option<SourceFileId>,
        path: PathBuf,
    },
    IncompleteCheck {
        file_name: String,
        start: u32,
        length: u32,
        reason: String,
        additional_partial_checks: usize,
    },
    MissingResolution(MissingResolutionError),
}

impl fmt::Display for DriverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLibraryPrefix {
                position,
                source_file,
            } => write!(
                formatter,
                "project prepared program for no-emit execution: library prefix position {position} names SourceFileId {}, expected SourceFileId {position}",
                source_file.raw()
            ),
            Self::MissingPreparedSource { source_file } => write!(
                formatter,
                "project prepared program for no-emit execution: library prefix names missing SourceFileId {}",
                source_file.raw()
            ),
            Self::NonUnicodeDisplayPath { path, .. } => write!(
                formatter,
                "project prepared program for no-emit execution for {}: prepared display path is not valid Unicode",
                path.display()
            ),
            Self::IncompleteCheck {
                file_name,
                start,
                length,
                reason,
                additional_partial_checks,
            } => write!(
                formatter,
                "no-emit check was incomplete at {file_name}:{start}+{length}: {reason} ({additional_partial_checks} additional partial checks)",
            ),
            Self::MissingResolution(error) => error.fmt(formatter),
        }
    }
}

impl Error for DriverError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MissingResolution(error) => Some(error),
            _ => None,
        }
    }
}

impl From<MissingResolutionError> for DriverError {
    fn from(error: MissingResolutionError) -> Self {
        Self::MissingResolution(error)
    }
}

fn project_checker_inputs(
    prepared: &PreparedProgram,
) -> Result<(Vec<InputFile>, Vec<InputFile>, String), DriverError> {
    let sources = prepared.source_files();
    let library_ids = prepared.library_files();
    let mut libs = Vec::with_capacity(library_ids.len());

    for (position, source_file) in library_ids.iter().copied().enumerate() {
        if source_file.index() != position {
            return Err(DriverError::InvalidLibraryPrefix {
                position,
                source_file,
            });
        }
        let source = prepared
            .source_file(source_file)
            .ok_or(DriverError::MissingPreparedSource { source_file })?;
        libs.push(project_source(source, Some(source_file))?);
    }

    let mut files = Vec::with_capacity(sources.len().saturating_sub(library_ids.len()));
    for (index, source) in sources.iter().enumerate().skip(library_ids.len()) {
        let source_file = u32::try_from(index).ok().map(SourceFileId::from_raw);
        files.push(project_source(source, source_file)?);
    }

    let current_directory_path = prepared.current_directory().display();
    let current_directory = current_directory_path
        .to_str()
        .ok_or_else(|| DriverError::NonUnicodeDisplayPath {
            source_file: None,
            path: current_directory_path.to_path_buf(),
        })?
        .to_owned();
    Ok((libs, files, current_directory))
}

fn project_source(
    source: &PreparedSourceFile,
    source_file: Option<SourceFileId>,
) -> Result<InputFile, DriverError> {
    let display_path = source.path().display();
    let name = display_path
        .to_str()
        .ok_or_else(|| DriverError::NonUnicodeDisplayPath {
            source_file,
            path: display_path.to_path_buf(),
        })?
        .to_owned();
    Ok(InputFile {
        name,
        text: source.text().to_owned(),
    })
}

fn prepared_source_owns_diagnostic(prepared: &PreparedProgram, file_name: &str) -> bool {
    let names_equal = |candidate: &std::path::Path| {
        candidate.to_str().is_some_and(|candidate| {
            candidate == file_name || candidate.replace('\\', "/") == file_name.replace('\\', "/")
        })
    };
    prepared.source_files().iter().any(|source| {
        names_equal(source.path().display())
            || names_equal(source.path().canonical().as_path())
            || source
                .alternate_display_paths()
                .iter()
                .any(|path| names_equal(path))
            || source.real_path().is_some_and(|path| {
                names_equal(path.display()) || names_equal(path.canonical().as_path())
            })
    })
}
