//! tsgo's language service (`internal/ls`): the service over a program
//! ([`LanguageService`]) with its pulled diagnostics, its utilities' user
//! preferences and format settings (`lsutil`) and its position conversions
//! (`lsconv`).

pub mod astnav;
pub mod autoimport;
mod call_hierarchy;
mod change;
mod code_actions;
mod code_lens;
mod completions;
mod cross_project;
mod definition;
mod diagnostics;
mod document_highlights;
mod file_rename;
mod find_all_references;
mod folding;
mod format;
mod formatting;
mod hover;
mod hovericon;
mod inlay_hints;
mod jsdoc;
mod jsx_tags;
mod language_service;
pub mod lsconv;
pub mod lsutil;
mod meaning;
mod organize_imports;
pub mod references;
mod rename;
mod symbols;
pub use call_hierarchy::incoming_calls_of_declarations;
pub use code_lens::code_lens_with_command;
pub use rename::RenameInfo;
pub use symbols::{workspace_symbols, WorkspaceSymbolMatch};
mod request;
mod selection_ranges;
mod semantic_tokens;
mod signature_help;
mod source_definition;
mod source_map;
pub mod tree;
mod utilities;

pub use cross_project::{
    combine_implementations, combine_incoming_calls, combine_references, combine_rename,
    combine_vs_references, NonLocalDefinition, ProjectSearch, SearchPosition, TextDocumentPosition,
};
pub use formatting::format_node_for_insertion;
pub use language_service::{Host, LanguageService, LsError};
pub use semantic_tokens::{semantic_tokens_legend, TOKEN_MODIFIERS, TOKEN_TYPES};
