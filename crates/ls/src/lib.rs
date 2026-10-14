//! tsgo's language service (`internal/ls`): the service over a program
//! ([`LanguageService`]) with its pulled diagnostics, its utilities' user
//! preferences and format settings (`lsutil`) and its position conversions
//! (`lsconv`).

pub mod astnav;
pub mod autoimport;
mod change;
mod code_actions;
mod completions;
mod definition;
mod diagnostics;
mod document_highlights;
mod file_rename;
mod find_all_references;
mod format;
mod formatting;
mod hover;
mod hovericon;
mod jsdoc;
mod language_service;
pub mod lsconv;
pub mod lsutil;
mod meaning;
mod organize_imports;
pub mod references;
mod rename;
pub use rename::RenameInfo;
mod request;
mod source_map;
pub mod tree;
mod utilities;

pub use formatting::format_node_for_insertion;
pub use language_service::{Host, LanguageService, LsError};
