//! tsgo `crossProjectOrchestrator` (`internal/lsp/server.go`) and the loop of
//! `handleCrossProject` (`internal/ls/crossproject.go`): a search (find all
//! references, implementations, rename, incoming calls, a code lens's count)
//! in the document's default project and the other projects that have it,
//! in the projects of its definitions' original locations, and in the
//! projects whose trees reference the projects searched; the answers are
//! combined when more than one project was asked.

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

use tsc_ls::{LanguageService, LsError, NonLocalDefinition, ProjectSearch, SearchPosition};
use tsc_lsproto::{DocumentUri, Position};
use tsc_project::{DocumentProjectError, Project, ProjectId, ProjectTreeRequest, Snapshot};

use super::{language_service_failure, Failure, RequestContext, Server};

/// tsgo `getLanguageServiceAndCrossProjectOrchestrator`'s start: the
/// document's snapshot and default project, and the language service
/// projects that have the document.
pub(super) struct CrossProjectStart {
    pub(super) snapshot: Arc<Snapshot>,
    pub(super) default_project: Arc<Project>,
    pub(super) all_projects: Vec<Arc<Project>>,
}

/// tsgo `projectAndTextDocumentPosition`: a project to search and where.
struct Item {
    project: Arc<Project>,
    /// The default project's snapshot (tsgo's item with its language
    /// service); another project's is the session's when it is searched.
    snapshot: Option<Arc<Snapshot>>,
    uri: DocumentUri,
    at: SearchPosition,
}

/// tsgo `response`: a project's answer once its search completed.
struct Response<R> {
    project: ProjectId,
    result: Option<R>,
    for_original_location: bool,
}

impl Server {
    /// tsgo `getLanguageServiceAndCrossProjectOrchestrator` (tsgo
    /// `GetLanguageServiceAndProjectsForFile`).
    pub(super) fn cross_project_start(
        &self,
        uri: &DocumentUri,
    ) -> Result<CrossProjectStart, DocumentProjectError> {
        let (snapshot, default_project) = self
            .initialized_session()
            .get_snapshot_and_default_project(uri)?;
        let file_name = tsc_program::uri::uri_to_file_name(uri.as_str()).unwrap_or_default();
        let all_projects = snapshot
            .projects_containing_file(&file_name)
            .into_iter()
            .cloned()
            .collect();
        Ok(CrossProjectStart {
            snapshot,
            default_project,
            all_projects,
        })
    }

    /// tsgo `handleCrossProject` with the server's orchestrator: `search`
    /// in the default project at `default_at`, in the other projects with
    /// the document at `position`, in the projects of the definitions'
    /// original locations, and — when the default project's definition is
    /// visible outside it — in the projects whose trees reference the
    /// projects searched, at the definition (or where its declaration map
    /// leads, or in its project's declaration file). The answers come in
    /// tsgo's order (the default project's, the other projects' with the
    /// document, the definition's projects', the original locations'),
    /// combined when more than one project was asked; none when no search
    /// completed.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_across_projects<R>(
        &self,
        context: &RequestContext,
        start: &CrossProjectStart,
        uri: &DocumentUri,
        position: &Position,
        default_at: SearchPosition,
        search: impl Fn(
            &mut LanguageService<'_>,
            &DocumentUri,
            &SearchPosition,
            bool,
        ) -> Result<Option<ProjectSearch<R>>, LsError>,
        combine: fn(Vec<R>) -> R,
    ) -> Result<Option<R>, Failure> {
        let session = self.initialized_session();
        let default_id = start.default_project.id().clone();
        let mut responses: Vec<Response<R>> = Vec::new();
        let mut queue: VecDeque<(Item, usize)> = VecDeque::new();
        let mut default_definition: Option<NonLocalDefinition> = None;
        let searched =
            |responses: &[Response<R>], id: &ProjectId| responses.iter().any(|r| &r.project == id);
        let enqueue = |responses: &mut Vec<Response<R>>,
                       queue: &mut VecDeque<(Item, usize)>,
                       item: Item,
                       for_original_location: bool| {
            if searched(responses, item.project.id()) {
                return;
            }
            responses.push(Response {
                project: item.project.id().clone(),
                result: None,
                for_original_location,
            });
            queue.push_back((item, responses.len() - 1));
        };
        // The initial projects and locations, the default project first.
        enqueue(
            &mut responses,
            &mut queue,
            Item {
                project: Arc::clone(&start.default_project),
                snapshot: Some(Arc::clone(&start.snapshot)),
                uri: uri.clone(),
                at: default_at,
            },
            false,
        );
        for project in &start.all_projects {
            if project.id() != &default_id {
                enqueue(
                    &mut responses,
                    &mut queue,
                    Item {
                        project: Arc::clone(project),
                        snapshot: None,
                        uri: uri.clone(),
                        at: SearchPosition::Lsp(position.clone()),
                    },
                    false,
                );
            }
        }
        loop {
            while let Some((item, index)) = queue.pop_front() {
                context.check()?;
                let (snapshot, project) = match item.snapshot {
                    Some(snapshot) => (snapshot, item.project),
                    // tsgo `GetLanguageServiceForProjectWithFile`: the
                    // project brought up to date, if it still has the file.
                    None => {
                        let (snapshot, project) = session
                            .get_project_with_file(item.project.id(), &item.uri)
                            .map_err(|error| tsc_lsproto::LspError::internal(error.to_string()))?;
                        let Some(project) = project else {
                            continue;
                        };
                        (snapshot, project)
                    }
                };
                let with_definition = project.id() == &default_id && default_definition.is_none();
                let found =
                    self.with_language_service(context, &snapshot, &project, |service| {
                        search(service, &item.uri, &item.at, with_definition)
                            .map_err(language_service_failure)
                    })?;
                context.check()?;
                let Some(found) = found else {
                    continue;
                };
                if with_definition {
                    default_definition = found.definition;
                }
                for location in found.original_locations {
                    // tsgo `GetProjectsForFile`: the configured projects of
                    // the file loaded.
                    let Ok((_, projects)) = session.get_projects_for_file(&location.uri) else {
                        continue;
                    };
                    for project in projects {
                        enqueue(
                            &mut responses,
                            &mut queue,
                            Item {
                                project,
                                snapshot: None,
                                uri: location.uri.clone(),
                                at: SearchPosition::Lsp(location.position.clone()),
                            },
                            true,
                        );
                    }
                }
                responses[index].result = Some(found.response);
            }
            // More projects from the default project's definition: the
            // trees of the projects that reference the projects searched.
            let Some(definition) = &default_definition else {
                break;
            };
            let requested = responses
                .iter()
                .filter(|response| response.result.is_some())
                .map(|response| response.project.as_str().to_owned())
                .collect::<BTreeSet<_>>();
            let snapshot = session
                .get_snapshot_loading_project_trees(ProjectTreeRequest::referencing(requested))
                .map_err(|error| tsc_lsproto::LspError::internal(error.to_string()))?;
            context.check()?;
            let has_file = |project: &Project, uri: &DocumentUri| {
                let file_name =
                    tsc_program::uri::uri_to_file_name(uri.as_str()).unwrap_or_default();
                snapshot.project_has_file(project, &file_name)
            };
            let mut more = false;
            for project in snapshot.language_service_projects() {
                // Without the check, the loop never ends.
                if searched(&responses, project.id()) || project.program().is_none() {
                    continue;
                }
                let target = [
                    Some(&definition.position),
                    definition.source_position.as_ref(),
                    definition.generated_position.as_ref(),
                ]
                .into_iter()
                .flatten()
                .find(|target| has_file(project, &target.uri));
                if let Some(target) = target {
                    enqueue(
                        &mut responses,
                        &mut queue,
                        Item {
                            project: Arc::clone(project),
                            snapshot: None,
                            uri: target.uri.clone(),
                            at: SearchPosition::Lsp(target.position.clone()),
                        },
                        false,
                    );
                    more = true;
                }
            }
            if !more {
                break;
            }
        }
        let asked = responses.len();
        let mut ordered = Vec::new();
        let mut take = |responses: &mut Vec<Response<R>>, id: &ProjectId| {
            if let Some(response) = responses
                .iter_mut()
                .find(|response| &response.project == id)
            {
                if let Some(result) = response.result.take() {
                    ordered.push(result);
                }
            }
        };
        take(&mut responses, &default_id);
        for project in &start.all_projects {
            take(&mut responses, project.id());
        }
        // The searches from the default definition's locations, then from
        // the original locations.
        for for_original_location in [false, true] {
            for response in &mut responses {
                if response.for_original_location == for_original_location {
                    if let Some(result) = response.result.take() {
                        ordered.push(result);
                    }
                }
            }
        }
        if asked > 1 {
            return Ok(Some(combine(ordered)));
        }
        Ok(ordered.into_iter().next())
    }
}
