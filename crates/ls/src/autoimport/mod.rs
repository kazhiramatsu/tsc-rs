//! tsgo `ls/autoimport`: the exports a file can import — the program's
//! modules' exports indexed by the words of their names ([`Registry`]) —
//! and the fixes that import them.

mod edits;
mod export;
mod extract;
mod import_adder;
mod index;
mod registry;
mod view;

pub(crate) use edits::fix_edits;
pub(crate) use extract::{symbol_to_export, Extractor};
pub(crate) use import_adder::ImportAdder;
pub use index::Index;
pub use registry::{
    build_project_bucket, indexed_file_paths, Registry, RegistryChange, RegistryCloneHost,
};
pub(crate) use view::{get_import_kind, Fix, FixAndExport, QueryKind, View};
