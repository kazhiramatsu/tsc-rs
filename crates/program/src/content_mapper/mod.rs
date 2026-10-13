//! tsgo `contentmapper` (contentmapper.go): a content mapper is a plugin that
//! turns otherwise unsupported file content (`.vue`, …) into virtual
//! TypeScript while the program is built.
//!
//! A mapper is declared in a tsconfig's `contentMappers` ([`Definition`]),
//! its npm package's package.json says how to run it ([`Manifest`]), and the
//! two together, once the package is resolved, are a [`Mapper`]. The
//! [`host::Host`] runs the mappers (hostimpl.go).

pub mod errors;
pub mod host;
pub mod protocol;
pub mod source;

use std::sync::Arc;

use errors::{ProjectFailure, TransformFailure};
use host::{OptionDiagnostic, Request};
use protocol::TransformOutput;

/// tsgo `contentmapper.Project`: one program's view of its mappers — what
/// they are called in it, what they transform its files to, and what they
/// report about their options. [`host::ProjectView`] is the host's; tests
/// supply their own.
pub trait Project: Send + Sync {
    /// tsgo `Refresh`: the mapper projects are opened again on next use.
    fn refresh(&self) -> Result<(), String>;
    /// tsgo `Identities`: every mapper's transform identity, in order.
    fn identities(&self) -> Result<Vec<String>, ProjectFailure>;
    /// tsgo `Identity`: a mapper's transform identity in this project.
    fn identity(&self, mapper: &Arc<Mapper>) -> Result<String, ProjectFailure>;
    /// tsgo `WatchedFiles`: the files dynamic mappers depend on.
    fn watched_files(&self) -> Result<Vec<String>, ProjectFailure>;
    /// tsgo `Diagnostics`: the mappers' diagnostics about their options.
    fn diagnostics(&self) -> Vec<OptionDiagnostic>;
    /// tsgo `Transform`: one file's virtual TypeScript.
    fn transform(
        &self,
        mapper: &Arc<Mapper>,
        request: Request<'_>,
    ) -> Result<TransformOutput, TransformFailure>;
    /// tsgo `Close`: the view is released.
    fn close(&self) -> Result<(), String>;
    /// tsgo's project system compiler host: the transform outputs the
    /// session's programs share (tsgo `contentMappedParseCache`). A command
    /// line's project has none (tsgo's compiler host transforms every file).
    fn content_mapped_cache(&self) -> Option<&source::ContentMappedCache> {
        None
    }
}

/// tsgo `contentmapper.Definition`: a `contentMappers` entry, the package
/// that implements the mapper and the extensions it registers. Lists and
/// options keep whether they were given at all (tsgo's nil), which mapper
/// equality observes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Definition {
    pub package: String,
    pub extensions: Option<Vec<String>>,
    /// The JSON text of the entry's `options`.
    pub options: Option<String>,
}

/// tsgo `contentmapper.Manifest`: the package's name and version (the
/// mapper's identity), the command that runs it and the compiler options it
/// depends on.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub exec: Option<Vec<String>>,
    pub compiler_options: Option<Vec<String>>,
    pub dynamic_config: bool,
}

/// tsgo `contentmapper.Mapper`: a definition with its resolved manifest and
/// the package directory the mapper runs in.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Mapper {
    pub definition: Definition,
    pub manifest: Manifest,
    /// The real path of the package directory.
    pub package_directory: String,
    /// The LSP client contribution an inferred project's mapper comes from.
    pub contribution_id: String,
}

/// tsgo `supportedVirtualExtensions`: what a mapper's output may be.
const SUPPORTED_VIRTUAL_EXTENSIONS: &[&str] = &[
    ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts", ".json",
];

/// tsgo `IsSupportedVirtualExtension`.
pub fn is_supported_virtual_extension(extension: &str) -> bool {
    SUPPORTED_VIRTUAL_EXTENSIONS.contains(&extension)
}

impl Mapper {
    /// tsgo `DiagnosticName`: the best name for a diagnostic, also when the
    /// manifest could not be resolved.
    pub fn diagnostic_name(&self) -> &str {
        if !self.manifest.name.is_empty() {
            &self.manifest.name
        } else if !self.definition.package.is_empty() {
            &self.definition.package
        } else {
            &self.contribution_id
        }
    }

    /// tsgo `Identity`: `name@version` (just the name without a version,
    /// nothing before the manifest is resolved), after a contribution's ID.
    pub fn identity(&self) -> String {
        if self.contribution_id.is_empty() {
            self.manifest_identity()
        } else {
            format!("{} ({})", self.contribution_id, self.manifest_identity())
        }
    }

    fn manifest_identity(&self) -> String {
        match (self.manifest.name.as_str(), self.manifest.version.as_str()) {
            ("", _) => String::new(),
            (name, "") => name.to_owned(),
            (name, version) => format!("{name}@{version}"),
        }
    }

    /// tsgo `MarshalDeclaredOptions` and its `json.Marshal`: the options the
    /// manifest declares, in declared order, as one compact JSON object;
    /// `option_json` gives an option's JSON text (tsgo's `CompilerOptions`
    /// field), or nothing for an unknown or unset one, which is left out.
    pub fn declared_options_json(&self, option_json: impl Fn(&str) -> Option<String>) -> String {
        let mut json = String::from("{");
        for name in self.manifest.compiler_options.iter().flatten() {
            let Some(value) = option_json(name) else {
                continue;
            };
            if json.len() > 1 {
                json.push(',');
            }
            json.push_str(&serde_json::to_string(name).expect("a string serializes"));
            json.push(':');
            json.push_str(&value);
        }
        json.push('}');
        json
    }

    /// tsgo `TransformIdentity`: the fingerprint of everything besides a
    /// file's content that decides how this mapper transforms it — its
    /// identity, its `options` and the declared compiler options (see
    /// [`Self::declared_options_json`]) — as xxh3-128 in canonical
    /// (big-endian) byte order.
    pub fn transform_identity(&self, declared_options_json: &str) -> [u8; 16] {
        let identity = self.identity();
        let options = self.definition.options.as_deref().unwrap_or_default();
        let mut buffer =
            Vec::with_capacity(identity.len() + 2 + options.len() + declared_options_json.len());
        buffer.extend_from_slice(identity.as_bytes());
        buffer.push(0);
        buffer.extend_from_slice(options.as_bytes());
        buffer.push(0);
        buffer.extend_from_slice(declared_options_json.as_bytes());
        xxhash_rust::xxh3::xxh3_128(&buffer).to_be_bytes()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/content_mapper/mapper_tests.rs"]
mod mapper_tests;
