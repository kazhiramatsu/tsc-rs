//! Deterministic acceptance impact planning.
//!
//! This is deliberately separate from the fixed acceptance entrypoint. The
//! latter is the current H2 closure authority and must remain an unsplit,
//! argument-free command. The planner is a conservative shadow: an unknown
//! input selects every acceptance slice, while only an explicitly disconnected
//! input can select none.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde::Serialize;

pub(crate) const SLICE_IDS: &[&str] = &[
    "conformance",
    "h1",
    "h2-1a",
    "h2-1b",
    "h2-1c",
    "h2-1d",
    "h2-1e",
    "h2-2a",
    "h2-2b",
    "h2-2c",
    "h2-2d",
    "h2-3a",
    "h2-3b",
    "h2-3c",
    "h2-3d",
    "h2-4a",
    "h2-4b",
    "h2-5a",
    "h2-5b",
    "h2-5c",
    "h2-5d",
    "h2-5e",
    "h2-5f",
    "h2-5g",
];

const H2_2C_SLICES: &[&str] = &[
    "h2-2c", "h2-4a", "h2-4b", "h2-5a", "h2-5b", "h2-5c", "h2-5d", "h2-5e", "h2-5f", "h2-5g",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Impact {
    All,
    Selected,
    None,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SliceSelection {
    pub(crate) id: String,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AcceptancePlan {
    pub(crate) schema: u32,
    pub(crate) planner: &'static str,
    pub(crate) mode: &'static str,
    pub(crate) changed_paths: Vec<String>,
    pub(crate) selected: Vec<SliceSelection>,
    pub(crate) skipped: Vec<String>,
    pub(crate) failure_policy: FailurePolicy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct FailurePolicy {
    pub(crate) environment: &'static str,
    pub(crate) semantic: &'static str,
}

impl AcceptancePlan {
    pub(crate) fn json(&self) -> Result<String, Box<dyn Error>> {
        Ok(format!("{}\n", serde_json::to_string_pretty(self)?))
    }
}

pub(crate) fn plan_from_paths(paths: &[String]) -> AcceptancePlan {
    let mut normalized = paths
        .iter()
        .map(|path| path.trim().replace('\\', "/"))
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();

    let mut selected = BTreeSet::new();
    let mut reasons = BTreeSet::new();
    let mut impact = Impact::None;
    for path in &normalized {
        let (path_impact, path_slices, reason) = classify_path(path);
        match path_impact {
            Impact::All => {
                impact = Impact::All;
                reasons.insert(reason.to_owned());
            }
            Impact::Selected => {
                if impact != Impact::All {
                    impact = Impact::Selected;
                }
                selected.extend(path_slices);
                reasons.insert(reason.to_owned());
            }
            Impact::None => {
                if impact == Impact::None {
                    reasons.insert(reason.to_owned());
                }
            }
        }
    }

    if impact == Impact::All {
        selected.extend(SLICE_IDS.iter().copied());
    }
    let mode = match impact {
        Impact::All => "all",
        Impact::Selected => "affected",
        Impact::None => "none",
    };
    let selected = selected
        .into_iter()
        .map(|id| SliceSelection {
            id: id.to_owned(),
            reason: if impact == Impact::All {
                "conservative closure: shared or unknown input".to_owned()
            } else {
                reasons.iter().cloned().collect::<Vec<_>>().join("; ")
            },
        })
        .collect();

    AcceptancePlan {
        schema: 1,
        planner: "tsc-rs-acceptance-impact-v1",
        mode,
        changed_paths: normalized,
        selected,
        skipped: if mode == "none" {
            reasons.into_iter().collect()
        } else {
            Vec::new()
        },
        failure_policy: FailurePolicy {
            environment: "retry-same-slice",
            semantic: "preserve-failure-and-fix-before-rerun",
        },
    }
}

pub(crate) fn full_plan() -> AcceptancePlan {
    let mut plan = plan_from_paths(&["Cargo.toml".to_owned()]);
    plan.changed_paths.clear();
    plan
}

pub(crate) fn plan_from_file(path: &Path) -> Result<AcceptancePlan, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let text = std::str::from_utf8(&bytes)?;
    Ok(plan_from_paths(
        &text.lines().map(str::to_owned).collect::<Vec<_>>(),
    ))
}

fn classify_path(path: &str) -> (Impact, Vec<&'static str>, &'static str) {
    if path.starts_with("docs/")
        || path.ends_with(".md")
        || path.starts_with("ratchets/fci-")
        || path.starts_with(".github/ci/fci-")
        || path.starts_with("crates/ci-")
    {
        return (
            Impact::None,
            Vec::new(),
            "non-acceptance documentation/framework input",
        );
    }

    if path == "README.md" || path.starts_with(".codex/") || path.starts_with("target/") {
        return (Impact::None, Vec::new(), "non-acceptance repository input");
    }

    if let Some(id) = exact_xtask_slice(path) {
        return (Impact::Selected, vec![id], "single acceptance module");
    }
    if let Some(slices) = dependent_xtask_slices(path) {
        return (
            Impact::Selected,
            slices.to_vec(),
            "acceptance module with downstream slice callers",
        );
    }
    if path == "crates/xtask/src/h2_2c_acceptance.rs" {
        return (
            Impact::Selected,
            H2_2C_SLICES.to_vec(),
            "shared H2.2c/H2.4/H2.5 acceptance module",
        );
    }
    if path == "crates/conformance/src/lib.rs" || path.starts_with("crates/conformance/") {
        return (Impact::Selected, vec!["conformance"], "conformance engine");
    }

    // A product/compiler, fixture, oracle, manifest, or workflow change may
    // alter any case's observed semantics. Do not guess a narrower set.
    if path.starts_with("crates/")
        || path.starts_with("ts-tests/")
        || path.starts_with("vendor/")
        || path.starts_with("ratchets/")
        || path.starts_with(".github/")
        || matches!(
            path,
            "Cargo.toml" | "Cargo.lock" | ".gitignore" | ".node-version" | "rust-toolchain.toml"
        )
    {
        return (Impact::All, Vec::new(), "shared or unknown semantic input");
    }

    (
        Impact::All,
        Vec::new(),
        "unknown input; fail closed to all acceptance slices",
    )
}

fn exact_xtask_slice(path: &str) -> Option<&'static str> {
    Some(match path {
        "crates/xtask/src/h1_emit_acceptance.rs" => "h1",
        "crates/xtask/src/h2_1a_acceptance.rs" => "h2-1a",
        "crates/xtask/src/h2_1b_acceptance.rs" => "h2-1b",
        "crates/xtask/src/h2_1c_acceptance.rs" => "h2-1c",
        "crates/xtask/src/h2_1d_acceptance.rs" => "h2-1d",
        "crates/xtask/src/h2_1e_acceptance.rs" => "h2-1e",
        "crates/xtask/src/h2_2a_acceptance.rs" => "h2-2a",
        "crates/xtask/src/h2_2b_acceptance.rs" => "h2-2b",
        "crates/xtask/src/h2_3a_acceptance.rs" => "h2-3a",
        "crates/xtask/src/h2_3b_acceptance.rs" => "h2-3b",
        "crates/xtask/src/h2_3d_acceptance.rs" => "h2-3d",
        _ => return None,
    })
}

fn dependent_xtask_slices(path: &str) -> Option<&'static [&'static str]> {
    Some(match path {
        "crates/xtask/src/h2_2d_acceptance.rs" => &[
            "h2-1a", "h2-1b", "h2-1c", "h2-1e", "h2-2a", "h2-2b", "h2-2d",
        ],
        "crates/xtask/src/h2_3c_acceptance.rs" => &["h2-3b", "h2-3c"],
        _ => return None,
    })
}

#[cfg(test)]
#[path = "../tests/unit/acceptance_plan/tests.rs"]
mod tests;
