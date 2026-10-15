//! One language service request over the program's query checker: the
//! checker, the position conversions, the declaration maps and the program
//! (what tsgo's `LanguageService` methods reach with a type checker).

use tsc_checker::state::{CheckResult, CheckerState};
use tsc_lsproto::{Position, ResolvedClientCapabilities};

use crate::language_service::{Host, LanguageService, LsError};
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
    /// The snapshot's files and file system (tsgo `l.host`).
    pub(crate) host: &'s dyn Host,
    /// The messages of the request's locale.
    pub(crate) catalog: Option<&'s dyn tsc_diagnostics::MessageCatalog>,
    /// tsgo `projectID`: the program's project.
    pub(crate) project_id: Option<&'s str>,
}

/// Where a request is: an LSP position, or a position of the file.
enum RequestPosition<'p> {
    Lsp(&'p Position),
    Offset(u32),
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
        self.with_request_where(file, RequestPosition::Lsp(position), links, run)
    }

    /// [`Self::with_request`] at a position of the file (a completion
    /// item's data).
    pub(crate) fn with_request_at<T>(
        &mut self,
        file: usize,
        position: u32,
        links: bool,
        run: impl FnOnce(&mut Request<'_, '_>, u32) -> CheckResult<T>,
    ) -> Result<Option<T>, LsError> {
        self.with_request_where(file, RequestPosition::Offset(position), links, run)
    }

    fn with_request_where<T>(
        &mut self,
        file: usize,
        position: RequestPosition<'_>,
        links: bool,
        run: impl FnOnce(&mut Request<'_, '_>, u32) -> CheckResult<T>,
    ) -> Result<Option<T>, LsError> {
        self.with_program_request(links, |request| {
            let position = match position {
                RequestPosition::Lsp(position) => {
                    let source = request.state.binder.source(file);
                    request
                        .converters
                        .from_lsp_position(&script_of(source), position) as u32
                }
                RequestPosition::Offset(position) => position,
            };
            run(request, position)
        })
    }

    /// Run `run` over the program's query checker (or the API checker, for
    /// the API's service) without a position: a request about a node or a
    /// symbol; none for a program without files.
    pub(crate) fn with_program_request<T>(
        &mut self,
        links: bool,
        run: impl FnOnce(&mut Request<'_, '_>) -> CheckResult<T>,
    ) -> Result<Option<T>, LsError> {
        let converters = self.converters;
        let preferences = self.user_preferences_ref();
        let capabilities = self.capabilities;
        let host = self.host;
        let catalog = self.catalog;
        let project_id = self.project_id.clone();
        let source_maps = SourceMaps::new(host, converters);
        let prepared = self.program.shared_prepared();
        let request_over = |state: &mut CheckerState<'_>| {
            let mut request = Request {
                state,
                converters,
                source_maps: &source_maps,
                prepared: &prepared,
                links,
                preferences,
                capabilities,
                host,
                catalog,
                project_id: project_id.as_deref(),
            };
            run(&mut request)
        };
        let result = if self.api_checker {
            self.program.with_checker(request_over)
        } else {
            self.program.with_query_checker(request_over)
        };
        match result {
            Some(Ok(result)) => Ok(Some(result)),
            Some(Err(abort)) => Err(LsError(format!("checker aborted: {}", abort.description()))),
            None => Ok(None),
        }
    }
}
