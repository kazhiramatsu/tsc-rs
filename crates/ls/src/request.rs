//! One language service request over the program's query checker: the
//! checker, the position conversions, the declaration maps and the program
//! (what tsgo's `LanguageService` methods reach with a type checker).

use tsc_checker::program::ProgramBinder;
use tsc_checker::state::{CheckResult, CheckerState};
use tsc_lsproto::{Position, ResolvedClientCapabilities};
use tsc_syntax::SourceFile;
use tsc_types::JsString;

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

impl LanguageService<'_> {
    /// Run `run` over the program's query checker (or the API checker, for
    /// the API's service); none for a program without files. A request at a
    /// document position finds its projections in the request.
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

impl Request<'_, '_> {
    /// tsgo `FromLSPRangeForSourceFile`: a document range in the file and its
    /// supplemental outputs, each projection covered by segments that take
    /// part in `feature`.
    pub(crate) fn range_projections(
        &self,
        file: usize,
        range: &tsc_lsproto::Range,
        feature: tsc_diagnostics::spanmap::Features,
    ) -> Vec<(usize, std::ops::Range<usize>)> {
        self.with_projection_scripts(file, |converters, scripts| {
            converters.from_lsp_range_for_scripts(scripts, range, feature)
        })
    }

    /// tsgo `FromLSPRangeIntersectingForSourceFile`: in the file and its
    /// supplemental outputs, the parts of a document range in segments that
    /// take part in `feature` (an ordinary file's whole range).
    pub(crate) fn range_projections_intersecting(
        &self,
        file: usize,
        range: &tsc_lsproto::Range,
        feature: tsc_diagnostics::spanmap::Features,
    ) -> Vec<(usize, std::ops::Range<usize>)> {
        self.with_projection_scripts(file, |converters, scripts| {
            converters.from_lsp_range_intersecting_for_scripts(scripts, range, feature)
        })
    }

    /// The spans `convert` finds in the scripts of the file's projections,
    /// by file.
    fn with_projection_scripts(
        &self,
        file: usize,
        convert: impl FnOnce(
            &Converters,
            &[&dyn crate::lsconv::Script],
        ) -> Vec<crate::lsconv::ScriptSpan>,
    ) -> Vec<(usize, std::ops::Range<usize>)> {
        let files = self.source_projections(file);
        let scripts = files
            .iter()
            .map(|&file| script_of(self.state.binder.source(file)))
            .collect::<Vec<_>>();
        let scripts = scripts
            .iter()
            .map(|script| script as &dyn crate::lsconv::Script)
            .collect::<Vec<_>>();
        convert(self.converters, &scripts)
            .into_iter()
            .map(|projection| (files[projection.script], projection.span))
            .collect()
    }
}

/// tsgo `MappedPosition[*ast.SourceFile]`: a document's position in one of
/// its file's outputs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FileProjection {
    pub(crate) file: usize,
    pub(crate) position: u32,
    pub(crate) fidelity: tsc_diagnostics::spanmap::Fidelity,
}

impl Request<'_, '_> {
    /// tsgo `sourceFileProjections`: a file, then its content mapper's
    /// supplemental outputs.
    pub(crate) fn source_projections(&self, file: usize) -> Vec<usize> {
        let binder = &self.state.binder;
        let mut files = vec![file];
        if let Some(output) = &binder.source(file).content_mapper {
            files.extend(
                output
                    .supplemental
                    .iter()
                    .filter_map(|name| file_named(binder, name)),
            );
        }
        files
    }

    /// tsgo `supplementalFileIndex`: a supplemental output's index among its
    /// canonical output's; none for any other file.
    pub(crate) fn supplemental_file_index(&self, file: usize) -> Option<i32> {
        let binder = &self.state.binder;
        let source = binder.source(file);
        let canonical = file_named(binder, source.content_mapper.as_ref()?.canonical.as_ref()?)?;
        let index = binder
            .source(canonical)
            .content_mapper
            .as_ref()
            .and_then(|output| {
                output
                    .supplemental
                    .iter()
                    .position(|name| *name == source.file_name)
            })
            .expect("supplemental source file is not linked from its canonical source file");
        Some(index as i32)
    }

    /// tsgo `sourceFileForSupplementalFileIndex`: the file, or its
    /// supplemental output of the index (none when it has no such output).
    pub(crate) fn source_for_supplemental_file_index(
        &self,
        file: usize,
        index: Option<i32>,
    ) -> Option<usize> {
        let Some(index) = index else {
            return Some(file);
        };
        let binder = &self.state.binder;
        let output = binder.source(file).content_mapper.as_ref()?;
        let name = usize::try_from(index)
            .ok()
            .and_then(|index| output.supplemental.get(index))?;
        file_named(binder, name)
    }

    /// tsgo `FromLSPPositionForSourceFile`: a document's position in its
    /// file and its file's supplemental outputs, each projection whose
    /// segment takes part in `feature`.
    pub(crate) fn position_projections(
        &self,
        file: usize,
        position: &Position,
        feature: tsc_diagnostics::spanmap::Features,
    ) -> Vec<FileProjection> {
        let files = self.source_projections(file);
        let scripts = files
            .iter()
            .map(|&file| script_of(self.state.binder.source(file)))
            .collect::<Vec<_>>();
        let scripts = scripts
            .iter()
            .map(|script| script as &dyn crate::lsconv::Script)
            .collect::<Vec<_>>();
        self.converters
            .from_lsp_position_for_scripts(&scripts, position, feature)
            .into_iter()
            .map(|projection| FileProjection {
                file: files[projection.script],
                position: projection.position as u32,
                fidelity: projection.fidelity,
            })
            .collect()
    }
}

/// The program file of a name (a content mapper's output names its others).
fn file_named(binder: &ProgramBinder<'_>, name: &JsString) -> Option<usize> {
    (0..binder.file_count()).find(|&index| binder.source(index).file_name == *name)
}

/// [`Request::source_projections`] of a parsed source: the file, then its
/// content mapper's supplemental outputs.
pub(crate) fn source_projections_of<'a>(
    binder: &ProgramBinder<'a>,
    source: &'a SourceFile,
) -> Vec<&'a SourceFile> {
    let mut sources = vec![source];
    if let Some(output) = &source.content_mapper {
        sources.extend(
            output
                .supplemental
                .iter()
                .filter_map(|name| file_named(binder, name))
                .map(|index| binder.source(index)),
        );
    }
    sources
}
