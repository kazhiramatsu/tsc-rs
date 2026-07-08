#![forbid(unsafe_code)]

pub mod flags;

pub use flags::*;

pub fn is_scaffolded() -> bool {
    true
}
