#![forbid(unsafe_code)]

pub mod flags;
pub mod options;

pub use flags::*;
pub use options::CompilerOptions;

pub fn is_scaffolded() -> bool {
    true
}
