//! Blocking Functional-CI effect-boundary vocabulary.
//!
//! FCI-2a deliberately exposes no executor, worker, snapshot, process,
//! cache, or publication API. It only makes infrastructure failure explicit
//! so later effect seams cannot accidentally turn it into model data.

#![forbid(unsafe_code)]

mod error;

pub use error::{EffectPhase, InfraError, InfraErrorFamily, IoKind, RunCancellation};
