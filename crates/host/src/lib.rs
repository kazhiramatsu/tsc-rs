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
mod memory;

use std::path::{Path, PathBuf};

pub use error::{HostError, HostErrorKind, HostOperation};
pub use filesystem::FsCompilerHost;
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

/// The read-only host surface used by program construction and resolution.
///
/// `Ok(None)` and `Ok(false)` mean that an entry is absent. An inability to
/// answer the question is a [`HostError`] and must not be converted into a
/// resolution miss. There is intentionally no write operation on this
/// interface: H0 is a mandatory no-emit execution track.
pub trait CompilerHost {
    fn current_directory(&self) -> Result<PathBuf, HostError>;

    fn use_case_sensitive_file_names(&self) -> bool;

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError>;

    fn file_exists(&self, path: &Path) -> Result<bool, HostError>;

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError>;

    /// Return the immediate file and directory entries below `path` in a
    /// deterministic order. An absent directory has no entries; a host
    /// failure is returned as `Err`.
    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError>;

    /// Return the physical path for an existing entry. An absent or dangling
    /// entry is `Ok(None)`; inability to inspect it is `Err`.
    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError>;
}
