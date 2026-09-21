//! Locations of repository-owned verification artifacts.
//!
//! Frozen records retain their original path strings and byte hashes. This
//! finite relocation map resolves those names without editing the records or
//! redirecting arbitrary basenames from unrelated directories.

use std::path::{Path, PathBuf};

const RELOCATIONS: &[(&str, &str)] = &[
    ("STAGE", "ratchets/STAGE"),
    ("ratchet.toml", "ratchets/ratchet.toml"),
    ("escapes.toml", "ratchets/escapes.toml"),
    ("fn-dispositions.toml", "ratchets/fn-dispositions.toml"),
    (
        "nodes-missing-fields.txt",
        "ratchets/nodes-missing-fields.txt",
    ),
    ("diag-families.json", "ratchets/diag-families.json"),
    ("m8-scope.json", "ratchets/m8/m8-scope.json"),
    ("m8-evidence.json", "ratchets/m8/m8-evidence.json"),
    (
        "m8-emitter-inventory.json",
        "ratchets/m8/m8-emitter-inventory.json",
    ),
    (
        "m8-emitter-dispositions.json",
        "ratchets/m8/m8-emitter-dispositions.json",
    ),
    ("m8-owner-plan.json", "ratchets/m8/m8-owner-plan.json"),
    (
        "m8-owner-plan-review.json",
        "ratchets/m8/m8-owner-plan-review.json",
    ),
];

/// Exact old/current names of a relocated verification artifact.
pub fn relocation(relative: &str) -> Option<(&'static str, &'static str)> {
    RELOCATIONS
        .iter()
        .copied()
        .find(|(old, current)| relative == *old || relative == *current)
}

/// Current spelling for a frozen workspace-relative artifact name.
pub fn current_relative(relative: &str) -> &str {
    relocation(relative).map_or(relative, |(_, current)| current)
}

/// Resolve an already validated workspace-relative frozen input path.
/// Missing or malformed current files are errors at the caller; there is no
/// filesystem fallback to a stale root copy.
pub fn workspace_path(workspace: &Path, relative: &str) -> PathBuf {
    workspace.join(current_relative(relative))
}
