//! Blocking Functional-CI effect-boundary vocabulary.
//!
//! FCI-2a deliberately exposes no executor, worker, snapshot, process,
//! cache, or publication API. It only makes infrastructure failure explicit
//! so later effect seams cannot accidentally turn it into model data.

#![forbid(unsafe_code)]

mod bounded;
mod error;
mod resource;
mod snapshot;

pub use bounded::{BoundedChunk, ByteLimit, ChunkSource, EffectResult};
pub use error::{EffectPhase, InfraError, InfraErrorFamily, IoKind, RunCancellation};
pub use resource::{BoundedQueue, ResourceClaimV1, ResourcePolicyV1};
pub use snapshot::{
    read_regular_file_bounded, stage_no_replace, BoundedFileBytes, GuardedProcessObservationV1,
    MountedSourceSnapshot, PathError, RelativePathV1, Sandbox, SandboxExecutionGuardV1,
    SourceSnapshotLimits, SourceSnapshotProvider, SourceSnapshotRequestV1, SourceSnapshotV1,
    VerifiedSourceSnapshot,
};
