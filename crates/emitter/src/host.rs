use std::path::{Path, PathBuf};

use tsc_program::{ResolutionMode, SourceFileId};
use tsc_syntax::{FileReference, SourceFile};
use tsc_types::CompilerOptions;

/// Borrowed, read-only facts for one Program source visible to emission.
///
/// The display path is callback-visible while the canonical path is used only
/// for equality and collision checks. `may_be_emitted` is the Program-owned
/// `sourceFileMayBeEmitted` verdict; it is deliberately not reconstructed
/// from resolution provenance in this crate.
#[derive(Clone, Copy, Debug)]
pub struct EmitSource<'host> {
    id: SourceFileId,
    path: &'host Path,
    canonical_path: &'host Path,
    may_be_emitted: bool,
    implied_node_format_for_emit: Option<ResolutionMode>,
    syntax: Option<&'host SourceFile>,
}

impl<'host> EmitSource<'host> {
    pub const fn new(
        id: SourceFileId,
        path: &'host Path,
        canonical_path: &'host Path,
        may_be_emitted: bool,
        implied_node_format_for_emit: Option<ResolutionMode>,
        syntax: Option<&'host SourceFile>,
    ) -> Self {
        Self {
            id,
            path,
            canonical_path,
            may_be_emitted,
            implied_node_format_for_emit,
            syntax,
        }
    }

    pub const fn id(self) -> SourceFileId {
        self.id
    }

    pub const fn path(self) -> &'host Path {
        self.path
    }

    pub const fn canonical_path(self) -> &'host Path {
        self.canonical_path
    }

    pub const fn may_be_emitted(self) -> bool {
        self.may_be_emitted
    }

    /// The already-computed `getImpliedNodeFormatForEmitWorker` result owned
    /// by the Program loader. `None` means emission falls back to the
    /// effective compiler `module` kind.
    pub const fn implied_node_format_for_emit(self) -> Option<ResolutionMode> {
        self.implied_node_format_for_emit
    }

    pub const fn syntax(self) -> Option<&'host SourceFile> {
        self.syntax
    }
}

/// Read-only projection of the Program facts reached by the H1 emitter.
///
/// This protocol has no filesystem operations and no write callback. A
/// planning-only adapter may return an [`EmitSource`] without syntax; the
/// checked adapter used by [`crate::emit_files`] supplies the same source's
/// immutable parsed tree while its checker session remains alive.
pub trait EmitHost {
    fn compiler_options(&self) -> &CompilerOptions;
    fn current_directory(&self) -> &Path;
    fn common_source_directory(&self) -> &Path;
    fn config_file_path(&self) -> Option<&Path>;
    fn use_case_sensitive_file_names(&self) -> bool;
    fn source_file_ids(&self) -> &[SourceFileId];
    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>>;

    /// Resolve a preserved triple-slash path reference against its containing
    /// source. Hosts with redirect-aware Program resolution may override this
    /// lexical default; the declaration transformer observes only the resolved
    /// source identity.
    ///
    /// tsc-port: getSourceFileFromReference @6.0.3
    /// tsc-hash: fd088e64de540c2728db419ad570897cc4baebd184a4c4b847a66537bd43f0cf
    /// tsc-span: _tsc.js:124170-124172
    fn source_file_from_reference(
        &self,
        referencing_file: SourceFileId,
        reference: &FileReference,
    ) -> Option<EmitSource<'_>> {
        let referencing = self.source_file(referencing_file)?;
        let directory = referencing.path().parent().unwrap_or_else(|| Path::new(""));
        let target = self.canonical_output_path(&directory.join(&reference.file_name));
        self.source_file_ids().iter().find_map(|&id| {
            let candidate = self.source_file(id)?;
            (candidate.canonical_path() == target).then_some(candidate)
        })
    }

    /// tsc-port: getEmitModuleFormatOfFileWorker @6.0.3
    /// tsc-hash: ffe7b58092e4af38c9484bef12201ef7524d2e3d26ba829ea59087f1a2c0d2a1
    /// tsc-span: _tsc.js:125493-125495
    fn get_emit_module_format_of_file(&self, id: SourceFileId) -> Option<i32> {
        let source = self.source_file(id)?;
        Some(match source.implied_node_format_for_emit() {
            Some(ResolutionMode::CommonJs) => 1,
            Some(ResolutionMode::EsNext) => 99,
            Some(ResolutionMode::Unspecified) | None => self.compiler_options().emit_module_kind(),
        })
    }

    /// Canonicalize one output spelling with the same case policy used for
    /// source identities. Implementations may override this when their path
    /// model is richer than the frozen POSIX H1 profile.
    fn canonical_output_path(&self, path: &Path) -> PathBuf {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.current_directory().join(path)
        };
        let normalized = normalize_lexical_path(&absolute);
        if self.use_case_sensitive_file_names() {
            normalized
        } else {
            PathBuf::from(normalized.to_string_lossy().to_lowercase())
        }
    }
}

/// Lexically normalize a path without consulting the filesystem.
///
/// Prepared Program paths and emitting option paths are already absolute and
/// validated at their owning boundaries. This helper preserves a root and
/// performs only the `.`/`..` simplification needed for output equality.
pub(crate) fn normalize_lexical_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}
