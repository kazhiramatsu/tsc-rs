//! tsgo's language service (`internal/ls`): the service over a program
//! ([`LanguageService`]) with its pulled diagnostics, its utilities' user
//! preferences and format settings (`lsutil`) and its position conversions
//! (`lsconv`).

pub mod astnav;
mod diagnostics;
mod language_service;
pub mod lsconv;
pub mod lsutil;
pub mod tree;

pub use language_service::{LanguageService, LsError};
