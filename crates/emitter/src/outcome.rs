use std::path::PathBuf;

use tsc_diagnostics::{Diagnostic, DiagnosticList};

/// Normalized source-map observation reserved by the H1 result shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMapObservation {
    input_source_files: Box<[PathBuf]>,
    canonical_json: Box<str>,
}

impl SourceMapObservation {
    // H1.1 freezes the result slot before H1.2/H1.4 can produce it.
    #[allow(dead_code)]
    pub(crate) fn new(input_source_files: Vec<PathBuf>, canonical_json: Box<str>) -> Self {
        Self {
            input_source_files: input_source_files.into_boxed_slice(),
            canonical_json,
        }
    }

    pub fn input_source_files(&self) -> &[PathBuf] {
        &self.input_source_files
    }

    pub fn canonical_json(&self) -> &str {
        &self.canonical_json
    }
}

/// Observable result of one emitting session.
///
/// Construction remains crate-owned. In particular, callback order lives in
/// the sink and is never derived from `emitted_files`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitOutcome {
    diagnostics: DiagnosticList,
    emit_skipped: bool,
    emitted_files: Option<Box<[PathBuf]>>,
    source_maps: Option<Box<[SourceMapObservation]>>,
}

impl EmitOutcome {
    // H1.1 keeps construction inside the emitter; execution lands later.
    #[allow(dead_code)]
    pub(crate) fn new(
        diagnostics: DiagnosticList,
        emit_skipped: bool,
        emitted_files: Option<Vec<PathBuf>>,
        source_maps: Option<Vec<SourceMapObservation>>,
    ) -> Self {
        Self {
            diagnostics,
            emit_skipped,
            emitted_files: emitted_files.map(Vec::into_boxed_slice),
            source_maps: source_maps.map(Vec::into_boxed_slice),
        }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub const fn emit_skipped(&self) -> bool {
        self.emit_skipped
    }

    pub fn emitted_files(&self) -> Option<&[PathBuf]> {
        self.emitted_files.as_deref()
    }

    pub fn source_maps(&self) -> Option<&[SourceMapObservation]> {
        self.source_maps.as_deref()
    }
}
