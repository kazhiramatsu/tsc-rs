#![forbid(unsafe_code)]

//! Read-only compiler host boundary for filesystem-hosted programs.
//!
//! This crate deliberately does not normalize paths, join them to the
//! current directory, decode source bytes, or resolve modules. Those are
//! program-layer responsibilities. A host answers questions about the exact
//! path identity it receives and reports I/O failures separately from an
//! ordinary missing entry.

mod error;
mod filesystem;
mod js_path;
mod memory;
mod ordering;

use std::path::{Path, PathBuf};
use tsc_diagnostics::{JsStr, JsString};

pub use error::{HostError, HostErrorKind, HostOperation};
pub use filesystem::FsCompilerHost;
pub use js_path::to_file_name_lower_case_js;
pub use memory::{MemoryCompilerHost, MemoryCompilerHostBuilder};

/// TypeScript's locale-independent file-name case fold.
///
/// TypeScript protects U+0130, U+0131, and U+00DF from the Unicode lowercase
/// expansion used for other non-canonical runs. Folding a run at once
/// preserves context-sensitive Unicode lowercasing. This performs case
/// folding only; lexical path normalization remains a program-layer
/// responsibility.
///
/// tsc-port: toFileNameLowerCase @6.0.3
/// tsc-hash: 65dcaf0c3334d707bf26b65cb7670523b964dc52d8f70cde69526e8af1d66186
/// tsc-span: _tsc.js:874-876
pub fn to_file_name_lower_case(path: &str) -> String {
    fn protected(character: char) -> bool {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(
                character,
                '\u{0130}' | '\u{0131}' | '\u{00df}' | '/' | '\\' | ':' | '-' | '_' | '.' | ' '
            )
    }

    let mut folded = String::with_capacity(path.len());
    let mut run = String::new();
    for character in path.chars() {
        if protected(character) {
            if !run.is_empty() {
                folded.push_str(&run.to_lowercase());
                run.clear();
            }
            folded.push(character);
        } else {
            run.push(character);
        }
    }
    if !run.is_empty() {
        folded.push_str(&run.to_lowercase());
    }
    folded
}

/// The kind an immediate directory entry resolves to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectoryListingKind {
    File,
    Directory,
}

/// One immediate entry of a directory listing, as
/// [`CompilerHost::read_directory_listing_js`] reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryListingEntry {
    /// The entry's path, spelled as [`CompilerHost::read_directory_js`]
    /// spells it.
    pub path: JsString,
    pub kind: DirectoryListingKind,
    /// The entry is, or may be, a symbolic link, so its real path is not
    /// its parent's real path plus its name. A host that cannot tell
    /// reports true.
    pub symlink: bool,
}

/// The read-only host surface used by program construction and resolution.
///
/// `Ok(None)` and `Ok(false)` mean that an entry is absent. An inability to
/// answer the question is a [`HostError`] and must not be converted into a
/// resolution miss. There is intentionally no write operation on this
/// interface: H0 remains a mandatory no-emit execution track, and H1 writes
/// through its separate `OutputSink` boundary.
pub trait CompilerHost {
    /// Compiler-facing queries retain JavaScript path values. Implementations
    /// must select their own boundary: a filesystem host encodes a native
    /// filename at I/O, while a virtual host compares the original JS value.
    fn current_directory_js(&self) -> Result<JsString, HostError>;

    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError>;

    /// The byte length of the file at `path` when the host can tell without
    /// reading it, or `None`. The loader reads the largest roots first so the
    /// longest parse starts earliest; a host without a cheap answer keeps
    /// this default, and its roots are read in their own order.
    fn file_size_hint_js(&self, _path: JsStr<'_>) -> Result<Option<u64>, HostError> {
        Ok(None)
    }

    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError>;

    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError>;

    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError>;

    /// The immediate file and directory entries below `path` with the kind
    /// each one resolves to, in [`Self::read_directory_js`] order. A host
    /// that learns the kinds while listing overrides this (the filesystem
    /// host reads them from the directory entries themselves); the default
    /// asks [`Self::directory_exists_js`] per entry and cannot tell links
    /// apart.
    fn read_directory_listing_js(
        &self,
        path: JsStr<'_>,
    ) -> Result<Vec<DirectoryListingEntry>, HostError> {
        self.read_directory_js(path)?
            .into_iter()
            .map(|entry| {
                let kind = if self.directory_exists_js(entry.as_js())? {
                    DirectoryListingKind::Directory
                } else {
                    DirectoryListingKind::File
                };
                Ok(DirectoryListingEntry {
                    path: entry,
                    kind,
                    symlink: true,
                })
            })
            .collect()
    }

    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError>;

    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError>;

    /// Native-path compatibility entry points. Compiler identity-bearing
    /// callers use the JS methods above; these accept scalar native paths.
    fn current_directory(&self) -> Result<PathBuf, HostError>;

    fn use_case_sensitive_file_names(&self) -> bool;

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError>;

    fn file_exists(&self, path: &Path) -> Result<bool, HostError>;

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError>;

    /// Return the immediate file and directory entries below `path`, ordered
    /// by display name in JavaScript's lexicographic UTF-16 code-unit order.
    /// The order is independent of the host's case-sensitivity profile. An
    /// absent directory has no entries; a host failure is returned as `Err`.
    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError>;

    /// Return only the immediate directory entries below `path` in the same
    /// display-name order used by [`Self::read_directory`].
    ///
    /// This is the exact host shape consumed by TypeScript's automatic type
    /// directive discovery. Built-in hosts override it to preserve a single
    /// listing's failure order. The compatibility default keeps existing host
    /// implementations source-compatible, but adds one fallible
    /// `directory_exists` observation per mixed directory entry; hosts needing
    /// exact discovery observability must override it.
    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.read_directory(path)?
            .into_iter()
            .filter_map(|entry| match self.directory_exists(&entry) {
                Ok(true) => Some(Ok(entry)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    /// Return the physical path for an existing entry. An absent or dangling
    /// entry is `Ok(None)`; inability to inspect it is `Err`.
    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError>;

    /// Whether program construction may read root source files ahead of the
    /// position at which its sequential discovery would read them.
    ///
    /// The default is `false`: program discovery then reads every source at
    /// its original position, so hosts that observe call order, count calls,
    /// or answer differently on repeated reads see exactly the sequential
    /// trace. A host returns `true` only when, for the duration of one
    /// program construction, `read_file_js` is a pure function of the path:
    /// no side effects, no order dependence, and the same bytes, absence or
    /// error on every call. Program construction then uses read-ahead only
    /// under an explicitly parallel worker budget. A retained read-ahead
    /// result (bytes, `Ok(None)` or `Err`) is applied at the original visit
    /// position instead of a second call; a payload that cannot be retained
    /// within the caller's program load limits is dropped and the path is
    /// read again at its visit, which the purity contract makes equivalent.
    /// Sources the sequential walk never reaches have their retained result
    /// dropped. Retained payloads count against the load limits together
    /// with the admitted sources.
    fn permits_source_read_ahead(&self) -> bool {
        false
    }

    /// A reader for the source read-ahead's parallel host reads: `Some` when
    /// this host answers `read_file_js` for any path from any thread with the
    /// result the loading thread would get, and reads leave no observable
    /// order (a filesystem host, the immutable memory host). Hosts whose
    /// reads are ordered observations (query ledgers) or that are not `Sync`
    /// keep the default `None`: the loading thread then reads every
    /// read-ahead root itself, in root order. Consulted only when
    /// [`Self::permits_source_read_ahead`] is true.
    fn parallel_source_reader(&self) -> Option<&(dyn ParallelSourceReader + Sync)> {
        None
    }

    /// This host as a host shared by several threads at once, for work that
    /// resolves modules ahead of the program walk (each thread constructs its
    /// own resolver over it), or `None` (the default) to keep every
    /// resolution on the loading thread.
    fn parallel_resolution_host(&self) -> Option<&(dyn CompilerHost + Sync)> {
        None
    }
}

/// `CompilerHost::read_file_js` callable from several threads at once during
/// source read-ahead; see [`CompilerHost::parallel_source_reader`].
pub trait ParallelSourceReader {
    fn read_source_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError>;
}
