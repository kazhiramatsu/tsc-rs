//! tsgo's language service (`internal/ls`): the service over a program
//! ([`LanguageService`]) with its pulled diagnostics, its utilities' user
//! preferences and format settings (`lsutil`) and its position conversions
//! (`lsconv`).

pub mod astnav;
mod definition;
mod diagnostics;
mod language_service;
pub mod lsconv;
pub mod lsutil;
pub mod references;
mod source_map;
pub mod tree;
mod utilities;

pub use language_service::{Host, LanguageService, LsError};
