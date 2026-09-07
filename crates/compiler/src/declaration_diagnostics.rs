use std::collections::{btree_map::Entry, BTreeMap};

use tsc_checker::emit::CheckerSession;
use tsc_checker::AuthoritativeSourceToken;
use tsc_diagnostics::{sort_and_dedupe_diagnostics, DiagnosticList};
use tsc_emitter::{
    get_declaration_diagnostics, EmitContractViolation, EmitFailure, EmitHost, EmitSelection,
    H2ActivityCanary, H2ActivityCounters, H2RuntimeSlice, PlanDeclarationPaths,
};
use tsc_program::{PreparedProgram, SourceFileId};

use super::{map_authoritative_failure, DriverError};

/// Declaration getters and forced emits over one initialized Program and checker.
/// Each uncached non-declaration source prepares its resolver before the
/// emitter applies source eligibility. Only owned diagnostics are cached.
pub struct DeclarationSession<'session, 'program> {
    prepared: &'session PreparedProgram,
    host: &'session dyn EmitHost,
    checker: Option<&'session CheckerSession<'program>>,
    paths: PlanDeclarationPaths,
    cache: BTreeMap<SourceFileId, DiagnosticList>,
    activity: H2ActivityCanary,
}

impl<'session, 'program> DeclarationSession<'session, 'program> {
    pub(super) fn new(
        prepared: &'session PreparedProgram,
        host: &'session dyn EmitHost,
        checker: Option<&'session CheckerSession<'program>>,
    ) -> Result<Self, DriverError> {
        let paths =
            PlanDeclarationPaths::for_declaration_diagnostics(host).map_err(DriverError::Emit)?;
        Ok(Self {
            prepared,
            host,
            checker,
            paths,
            cache: BTreeMap::new(),
            activity: H2ActivityCanary::h2_7c_profile(),
        })
    }

    /// tsc-port: getDiagnosticsHelper @6.0.3
    /// tsc-hash: e39fa16e7de6dea8d5d79332ae4afc22514aa1600c873cfe18cb7dd596fff252
    /// tsc-span: _tsc.js:123631-123641
    /// The per-file worker below retains the declaration getter's cache and
    /// source preparation order (_tsc.js:124005-124023).
    pub fn get_declaration_diagnostics(
        &mut self,
        selection: EmitSelection,
    ) -> Result<DiagnosticList, DriverError> {
        self.activity.observe_runtime_slice(H2RuntimeSlice::H2_7c);
        let sources = match selection {
            EmitSelection::WholeProgram => self.host.source_file_ids().to_vec(),
            EmitSelection::TargetSourceFile(source) => vec![source],
        };
        let mut diagnostics = Vec::new();
        for source in sources {
            let file = self.host.source_file(source).ok_or({
                DriverError::Emit(EmitFailure::Contract(
                    EmitContractViolation::PlannedSourceMissing(source),
                ))
            })?;
            let syntax = file.syntax().ok_or({
                DriverError::Emit(EmitFailure::Contract(
                    EmitContractViolation::CheckedSyntaxUnavailable(source),
                ))
            })?;
            if syntax.is_declaration_file {
                continue;
            }
            let cached = match self.cache.entry(source) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let checker = self.checker.ok_or({
                        DriverError::Emit(EmitFailure::Contract(
                            EmitContractViolation::CheckedSyntaxUnavailable(source),
                        ))
                    })?;
                    let partials = checker
                        .prepare_declaration_source(AuthoritativeSourceToken(source.raw()))
                        .map_err(|failure| map_authoritative_failure(self.prepared, failure))?;
                    if let Some(partial) = partials.first() {
                        return Err(DriverError::IncompleteCheck {
                            file_name: partial.file_name.clone(),
                            start: partial.start,
                            length: partial.length,
                            reason: partial.reason.clone(),
                            additional_partial_checks: partials.len().saturating_sub(1),
                        });
                    }
                    self.activity.borrow_emit_resolver();
                    let result = checker
                        .with_emit_resolver(|resolver| {
                            get_declaration_diagnostics(
                                resolver,
                                self.host,
                                &self.paths,
                                source,
                                &mut self.activity,
                            )
                        })
                        .map_err(DriverError::Emit)?;
                    entry.insert(result)
                }
            };
            diagnostics.extend_from_slice(cached);
        }
        sort_and_dedupe_diagnostics(&mut diagnostics);
        Ok(diagnostics)
    }

    /// tsc-port: emitWorker @6.0.3 (forced declaration-only branch)
    /// tsc-hash: 55f7ba7bf02c6e2cae7d1c89577e5c98990352d46d16dd5092e68989e93fab80
    /// tsc-span: _tsc.js:123595-123625
    /// Force declaration-only output without scheduling semantic diagnostics.
    /// Every nonempty Program call borrows this same initialized checker.
    /// A source-less Program retains the existing unavailable-resolver adapter.
    pub fn emit_forced_declarations(
        &mut self,
        selection: EmitSelection,
        sink: &mut dyn tsc_emitter::OutputSink,
    ) -> Result<tsc_emitter::EmitOutcome, DriverError> {
        tsc_emitter::validate_forced_declaration_request(self.host).map_err(DriverError::Emit)?;
        if let Some(checker) = self.checker {
            self.activity.borrow_emit_resolver();
            checker
                .with_emit_resolver(|resolver| {
                    tsc_emitter::emit_forced_declarations_with_activity(
                        resolver,
                        self.host,
                        selection,
                        sink,
                        &mut self.activity,
                    )
                })
                .map_err(DriverError::Emit)
        } else {
            // Source-less Programs retain the existing Rust no-checker
            // adaptation. The unavailable resolver rejects any accidental
            // query; the forced plan has no source that can reach one.
            if !self.host.source_file_ids().is_empty() {
                return Err(DriverError::AuthoritativeResolution(
                    tsc_checker::AuthoritativeModuleFailure::InvalidMetadata {
                        detail: "nonempty declaration session requires an initialized checker"
                            .to_owned(),
                    },
                ));
            }
            tsc_emitter::emit_forced_declarations_with_activity(
                &tsc_emitter::UnavailableEmitResolver,
                self.host,
                selection,
                sink,
                &mut self.activity,
            )
            .map_err(DriverError::Emit)
        }
    }

    pub fn activity(&self) -> H2ActivityCounters {
        self.activity.counters()
    }
}
