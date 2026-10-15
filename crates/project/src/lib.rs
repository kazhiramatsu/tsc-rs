//! tsgo's project system (`internal/project`). A [`SnapshotHost`] derives
//! immutable [`Snapshot`]s from a base snapshot with file changes, the
//! editor's open documents and an API request (tsgo `CloneSnapshot`); each
//! snapshot holds the configured, synthetic and inferred projects with
//! their programs ([`tsc_compiler::LiveProgram`]), the parsed configs and
//! the files read so far. What a build leaves unchanged is shared with the
//! base snapshot. The language server's [`Session`] keeps the editor's
//! notifications as pending changes and adopts the snapshots it derives.
//!
//! Not ported yet: file watching, published diagnostics, automatic type
//! acquisition, auto-imports and logging (the LSP packet,
//! `docs/design/greenfield/slices/ts71-lsp`).

mod background;
mod builder;
mod client;
mod config;
mod fs;
mod id;
mod overlay;
mod project;
mod session;
mod snapshot;
mod watch;

pub use client::{Client, Logger};
pub use fs::FileChangeSummary;
pub use id::{ProjectId, ProjectKind};
pub use overlay::{FileChange, FileChangeKind, Overlay};
pub use project::{CommandLine, ProgramRoots, ProgramUpdateKind, Project, ProjectProgram};
pub use session::{DocumentProjectError, Session};
pub use snapshot::{
    ApiSnapshotRequest, ContentMapperContributions, CreateProgramRequest, ProjectError,
    ProjectTreeRequest, RebasableFileSystem, ReconfigureProgramRequest, SessionOptions, Snapshot,
    SnapshotChange, SnapshotHost,
};
