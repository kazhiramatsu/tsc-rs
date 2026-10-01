//! The TypeScript test-suite harness: expansion of the vendored native
//! (TypeScript 7.x) compiler and conformance cases and the compiler-runner
//! fixture semantics tsc-rs's conformance runner executes them with.

#![forbid(unsafe_code)]

pub mod upstream_suites;

use std::error::Error;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessError {
    message: String,
}

impl HarnessError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for HarnessError {}

pub type HarnessResult<T> = Result<T, HarnessError>;
