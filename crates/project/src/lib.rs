//! tsgo's project system (`internal/project`) on the path its standalone API
//! takes. A [`SnapshotHost`] derives immutable [`Snapshot`]s from a base
//! snapshot with file changes and an API request (tsgo `CloneSnapshot`);
//! each snapshot holds the configured and synthetic projects with their
//! programs ([`tsc_compiler::LiveProgram`]), the parsed configs and the
//! files read so far. What a build leaves unchanged is shared with the base
//! snapshot.
//!
//! Not ported: the language server's open files (overlays), automatic type
//! acquisition, auto-imports, content mappers, logging, file watching and
//! client notifications. Files opened through the API and the inferred
//! project, the parse cache, program reuse after a one-file change and the
//! checker pool come in the later slices of P5-1
//! (`docs/design/greenfield/slices/ts71-api-server`).

mod builder;
mod config;
mod fs;
mod id;
mod project;
mod snapshot;

pub use fs::FileChangeSummary;
pub use id::{ProjectId, ProjectKind};
pub use project::{CommandLine, ProgramRoots, ProgramUpdateKind, Project, ProjectProgram};
pub use snapshot::{
    ApiSnapshotRequest, CreateProgramRequest, ProjectError, ReconfigureProgramRequest,
    SessionOptions, Snapshot, SnapshotHost,
};
