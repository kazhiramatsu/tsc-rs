//! tsgo `ls/autoimport`: the exports a file can import — the program's
//! modules' exports and the packages' in the `node_modules` directories
//! above it, indexed by the words of their names ([`Registry`]) — and the
//! fixes that import them.

mod alias_resolver;
mod edits;
mod export;
mod extract;
mod import_adder;
mod index;
mod node_modules;
mod package_json;
mod program_facts;
mod registry;
mod view;

pub(crate) use edits::{file_edits, fix_edits};
pub(crate) use extract::{symbol_to_export, Extractor};
pub(crate) use import_adder::{try_get_auto_importable_reference_from_type_node, ImportAdder};
pub use index::Index;
pub use package_json::{PackageJsonContents, PackageJsonInfo};
pub use program_facts::program_package_facts;
pub use registry::{
    build_project_bucket, indexed_file_paths, ProgramPackageFacts, Registry, RegistryChange,
    RegistryCloneHost,
};
pub(crate) use view::{get_import_kind, Fix, FixAndExport, QueryKind, View};
