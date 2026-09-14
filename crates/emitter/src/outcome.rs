use tsc_diagnostics::JsString;

use tsc_diagnostics::{Diagnostic, DiagnosticList};

use crate::H2ActivityCounters;

/// Normalized source-map observation reserved by the H1 result shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMapObservation {
    input_source_files: Box<[JsString]>,
    canonical_json: Box<str>,
}

impl SourceMapObservation {
    // H1.1 froze the result slot; h2-6a-m-3 is the producer.
    pub(crate) fn new(input_source_files: Vec<JsString>, canonical_json: Box<str>) -> Self {
        Self {
            input_source_files: input_source_files.into_boxed_slice(),
            canonical_json,
        }
    }

    pub fn input_source_files(&self) -> &[JsString] {
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
    emitted_files: Option<Box<[JsString]>>,
    source_maps: Option<Box<[SourceMapObservation]>>,
    h2_activity: H2ActivityCounters,
}

impl EmitOutcome {
    // Construction stays inside the emitter so callback and outcome ordering
    // cannot be conflated by callers.
    pub(crate) fn new(
        diagnostics: DiagnosticList,
        emit_skipped: bool,
        emitted_files: Option<Vec<JsString>>,
        source_maps: Option<Vec<SourceMapObservation>>,
        h2_activity: H2ActivityCounters,
    ) -> Self {
        Self {
            diagnostics,
            emit_skipped,
            emitted_files: emitted_files.map(Vec::into_boxed_slice),
            source_maps: source_maps.map(Vec::into_boxed_slice),
            h2_activity,
        }
    }

    /// The whole-program `noEmit` command still calls `emitBuildInfo` upstream.
    /// Without incremental/composite output, that operation has no output path
    /// and returns the initial emitFiles collections. It does not report a
    /// skipped source emit. This value-only result needs no printer or resolver.
    ///
    /// _tsc.js:125636-125640, 123526-123548, 116530-116553.
    #[doc(hidden)]
    pub fn no_emit_without_build_info(
        options: &tsc_types::CompilerOptions,
    ) -> Result<Self, crate::EmitFailure> {
        if options.no_emit != Some(true) {
            return Err(crate::EmitFailure::UnsupportedCompilerOption { option: "noEmit" });
        }
        if options.incremental == Some(true) || options.composite == Some(true) {
            return Err(crate::EmitFailure::Unsupported(
                crate::UnsupportedEmitFeature::BuildInfo,
            ));
        }
        let maps = options.source_map == Some(true)
            || options.inline_source_map == Some(true)
            || options.declaration_map == Some(true) && options.declaration == Some(true);
        Ok(Self::new(
            Vec::new(),
            false,
            (options.list_emitted_files == Some(true)).then(Vec::new),
            maps.then(Vec::new),
            H2ActivityCounters::default(),
        ))
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub const fn emit_skipped(&self) -> bool {
        self.emit_skipped
    }

    pub fn emitted_files(&self) -> Option<&[JsString]> {
        self.emitted_files.as_deref()
    }

    pub fn source_maps(&self) -> Option<&[SourceMapObservation]> {
        self.source_maps.as_deref()
    }

    /// Session-owned H1 positive controls and H2 runtime-slice canaries.
    pub const fn h2_activity(&self) -> H2ActivityCounters {
        self.h2_activity
    }
}
