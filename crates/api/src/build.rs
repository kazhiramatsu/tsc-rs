//! tsgo `api/session.go` (19dadef8) build orchestrators
//! (session.go:1621-1745): `createBuildOrchestrator`,
//! `disposeBuildOrchestrator`, `build`, `buildReferences`, `cleanBuild` and
//! `cleanReferences`, over tsc-rs's `tsc -b` orchestrator
//! ([`ApiOrchestrator`]) and a build system over the session's file system
//! (tsgo `apiBuildSystem`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tsc_compiler::system::System;
use tsc_compiler::{ApiBuildOptions, ApiOrchestrator, OrchestratorResult};
use tsc_host::vfs::FileSystem;

use crate::ipc::Payload;
use crate::proto::{
    BuildParams, BuildResponse, CreateBuildOrchestratorParams, CreateBuildOrchestratorResponse,
    DiagnosticResponse, DisposeBuildOrchestratorParams, StatisticsResponse,
};
use crate::session::{json, parse, Session};

/// tsgo `nextBuildOrchestratorId`: process-wide, from 1.
static NEXT_BUILD_ORCHESTRATOR_ID: AtomicU64 = AtomicU64::new(0);

impl Session {
    /// tsgo's build orchestrator methods; `None` for another method.
    pub(crate) fn handle_build_request(
        &self,
        method: &str,
        params: &[u8],
    ) -> Option<Result<Payload, String>> {
        Some(match method {
            "createBuildOrchestrator" => {
                parse::<CreateBuildOrchestratorParams>("CreateBuildOrchestratorParams", params)
                    .map(|params| json(&self.create_build_orchestrator(params)))
            }
            "disposeBuildOrchestrator" => {
                parse::<DisposeBuildOrchestratorParams>("DisposeBuildOrchestratorParams", params)
                    .and_then(|params| self.dispose_build_orchestrator(&params))
                    .map(|()| json(&true))
            }
            "build" | "buildReferences" => parse::<BuildParams>("BuildParams", params)
                .and_then(|params| self.build(&params, method == "buildReferences"))
                .map(|response| json(&response)),
            "cleanBuild" | "cleanReferences" => parse::<BuildParams>("CleanBuildParams", params)
                .and_then(|params| self.clean_build(&params, method == "cleanReferences"))
                .map(|response| json(&response)),
            _ => return None,
        })
    }

    /// tsgo `handleCreateBuildOrchestrator`.
    fn create_build_orchestrator(
        &self,
        params: CreateBuildOrchestratorParams,
    ) -> CreateBuildOrchestratorResponse {
        let catalog = self.host().options().library_catalog.clone();
        let current_directory = if params.cwd.is_empty() {
            self.host().options().current_directory.clone()
        } else {
            params.cwd
        };
        let system = Arc::new(ApiBuildSystem {
            fs: Arc::clone(self.fs()),
            current_directory,
            default_library_path: catalog.directory().to_string_lossy().into_owned(),
            start: Instant::now(),
        });
        let build_options = params.build_options.map(|options| ApiBuildOptions {
            dry: options.dry,
            force: options.force,
            verbose: options.verbose,
            stop_build_on_errors: options.stop_build_on_errors,
            clean: options.clean,
        });
        let orchestrator = ApiOrchestrator::new(
            system,
            catalog,
            &params.root_names,
            build_options,
            params.compiler_options.map(|options| options.0),
        );
        let id = NEXT_BUILD_ORCHESTRATOR_ID.fetch_add(1, Ordering::Relaxed) + 1;
        self.build_orchestrators()
            .lock()
            .expect("build orchestrators lock")
            .insert(id, orchestrator);
        CreateBuildOrchestratorResponse {
            build_orchestrator_id: id,
        }
    }

    /// tsgo `handleDisposeBuildOrchestrator`.
    fn dispose_build_orchestrator(
        &self,
        params: &DisposeBuildOrchestratorParams,
    ) -> Result<(), String> {
        self.build_orchestrators()
            .lock()
            .expect("build orchestrators lock")
            .remove(&params.build_orchestrator_id)
            .map(drop)
            .ok_or_else(|| "build orchestrator not found while disposing".to_owned())
    }

    /// tsgo `handleBuild` and `handleBuildReferences`.
    fn build(&self, params: &BuildParams, references: bool) -> Result<BuildResponse, String> {
        let mut orchestrators = self
            .build_orchestrators()
            .lock()
            .expect("build orchestrators lock");
        let orchestrator = orchestrators
            .get_mut(&params.build_orchestrator_id)
            .ok_or_else(|| {
                if references {
                    format!(
                        "build orchestrator not found for building references for {}",
                        params.project
                    )
                } else {
                    format!(
                        "build orchestrator not found while building {}",
                        params.project
                    )
                }
            })?;
        let result = if references {
            orchestrator.build_references(&params.project)?
        } else {
            orchestrator.build(&params.project)?
        };
        Ok(self.build_response(result, false))
    }

    /// tsgo `handleCleanBuild` and `handleCleanReferences`.
    fn clean_build(&self, params: &BuildParams, references: bool) -> Result<BuildResponse, String> {
        let mut orchestrators = self
            .build_orchestrators()
            .lock()
            .expect("build orchestrators lock");
        let orchestrator = orchestrators
            .get_mut(&params.build_orchestrator_id)
            .ok_or_else(|| {
                if references {
                    format!(
                        "build orchestrator not found while cleaning references for {}",
                        params.project
                    )
                } else {
                    format!(
                        "build orchestrator not found while cleaning {}",
                        params.project
                    )
                }
            })?;
        let result = if references {
            orchestrator.clean_references(&params.project)?
        } else {
            orchestrator.clean(&params.project)?
        };
        Ok(self.build_response(result, true))
    }

    /// tsgo `BuildResponse` (or, for a clean, `CleanBuildResponse`): the
    /// diagnostics located in their files' texts.
    fn build_response(&self, result: OrchestratorResult, clean: bool) -> BuildResponse {
        BuildResponse {
            status: result.status,
            diagnostics: DiagnosticResponse::list(&result.errors, &|file_name| {
                self.read_file_text(file_name)
            }),
            statistics: StatisticsResponse {
                projects: result.statistics.projects,
                projects_built: result.statistics.projects_built,
                timestamp_updates: result.statistics.timestamp_updates,
            },
            files_deleted: (clean && !result.files_to_delete.is_empty())
                .then_some(result.files_to_delete),
        }
    }
}

/// tsgo `apiBuildSystem`: the build orchestrator's system over the session's
/// file system. Its output goes nowhere and it has no environment.
struct ApiBuildSystem {
    fs: Arc<dyn FileSystem>,
    current_directory: String,
    default_library_path: String,
    start: Instant,
}

impl System for ApiBuildSystem {
    fn fs(&self) -> &dyn FileSystem {
        &*self.fs
    }

    fn current_directory(&self) -> &str {
        &self.current_directory
    }

    fn default_library_path(&self) -> &str {
        &self.default_library_path
    }

    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn since_start(&self) -> Duration {
        self.start.elapsed()
    }

    fn env_var(&self, _name: &str) -> Option<String> {
        None
    }

    fn output_is_terminal(&self) -> bool {
        false
    }

    fn terminal_width(&self) -> Option<usize> {
        None
    }

    fn write_output(&self, _text: &str) {}

    fn write_error(&self, _text: &str) {}
}
