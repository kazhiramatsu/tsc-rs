//! tsgo `ls/autoimport`: the exports a file can import — the program's
//! modules' exports indexed by the words of their names ([`Registry`]) —
//! and the fixes that import them.

mod edits;
mod export;
mod extract;
mod index;
mod registry;
mod view;

pub(crate) use edits::fix_edits;
pub(crate) use extract::Extractor;
pub use index::Index;
pub use registry::{
    build_project_bucket, indexed_file_paths, Registry, RegistryChange, RegistryCloneHost,
};
pub(crate) use view::{get_import_kind, FixAndExport, View};
