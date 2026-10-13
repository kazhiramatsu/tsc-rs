//! One language service request over the program's query checker: the
//! checker, the position conversions, the declaration maps and the program
//! (what tsgo's `LanguageService` methods reach with a type checker).

use tsc_checker::state::{CheckResult, CheckerState};
use tsc_lsproto::{Position, ResolvedClientCapabilities};

use crate::language_service::{LanguageService, LsError};
use crate::lsconv::Converters;
use crate::lsutil::UserPreferences;
use crate::source_map::SourceMaps;
use crate::utilities::script_of;

/// A request over the query checker.
pub(crate) struct Request<'s, 'a> {
    pub(crate) state: &'s mut CheckerState<'a>,
    pub(crate) converters: &'s Converters,
    pub(crate) source_maps: &'s SourceMaps<'s>,
    pub(crate) prepared: &'s tsc_program::PreparedProgram,
    /// Whether the client takes definition links (go to definition and type
    /// definition).
    pub(crate) links: bool,
    pub(crate) preferences: &'s UserPreferences,
    pub(crate) capabilities: &'s ResolvedClientCapabilities,
}

impl LanguageService<'_> {
    /// Run `run` over the program's query checker at the position of a
    /// document without a content mapper (one position); none for a program
    /// without files.
    pub(crate) fn with_request<T>(
        &mut self,
        file: usize,
        position: &Position,
        links: bool,
        run: impl FnOnce(&mut Request<'_, '_>, u32) -> CheckResult<T>,
    ) -> Result<Option<T>, LsError> {
        let converters = self.converters;
        let preferences = self.user_preferences_ref();
        let capabilities = self.capabilities;
        let source_maps = SourceMaps::new(self.host, converters);
        let prepared = self.program.shared_prepared();
        let result = self.program.with_query_checker(|state| {
            let source = state.binder.source(file);
            let position = converters.from_lsp_position(&script_of(source), position) as u32;
            let mut request = Request {
                state,
                converters,
                source_maps: &source_maps,
                prepared: &prepared,
                links,
                preferences,
                capabilities,
            };
            run(&mut request, position)
        });
        match result {
            Some(Ok(result)) => Ok(Some(result)),
            Some(Err(abort)) => Err(LsError(format!("checker aborted: {}", abort.description()))),
            None => Ok(None),
        }
    }
}
