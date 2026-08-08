use std::path::{Path, PathBuf};

use tsc_program::SourceFileId;
use tsc_syntax::SourceFile;
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
    syntax: Option<&'host SourceFile>,
}

impl<'host> EmitSource<'host> {
    pub const fn new(
        id: SourceFileId,
        path: &'host Path,
        canonical_path: &'host Path,
        may_be_emitted: bool,
        syntax: Option<&'host SourceFile>,
    ) -> Self {
        Self {
            id,
            path,
            canonical_path,
            may_be_emitted,
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
