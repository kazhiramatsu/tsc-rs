//! tsgo's language service (`internal/ls`): the service over a program
//! ([`LanguageService`]) with its pulled diagnostics, its utilities' user
//! preferences and format settings (`lsutil`) and its position conversions
//! (`lsconv`).

mod diagnostics;
mod language_service;
pub mod lsconv;
pub mod lsutil;

pub use language_service::{LanguageService, LsError};
