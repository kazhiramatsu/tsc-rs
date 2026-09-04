//! h2-7a checker/harness L4/L5 machine controls: the m-1 audit closure and the
//! `declaration_syntax` dormancy allowlist.
//!
//! L4: after the P5 flips + the P7 re-mint, the frozen owner inventory
//! carries no `audit-foundation-needed` row: every printer-subgraph and
//! factory/parenthesizer row is `audit-already-exact` with a
//! `crates/emitter/**` anchor (the mint-time header verification covers
//! the 190 rows flipped here; the m-2 partition projection has its own
//! focused test).
//!
//! L5: the option that unlocks TypeScript-syntax printing is constructed
//! in production code ONLY at the enumerated dormant sites (the checker's
//! symbolToString worker and the harness-armed replay printer); every
//! other occurrence lives under a tests directory. tsrs-native control.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const INVENTORY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-7a-owner-inventory.v1.json"
));

/// Production files allowed to construct a printer with
/// `with_declaration_syntax(true)` (packet §6 L5). Both sites are dormant:
/// the worker has no production caller before m-4/H2.7b, and the replay
/// printer runs only when the harness arms the replay observer.
const DECLARATION_SYNTAX_ALLOWLIST: &[&str] = &[
    "crates/checker/src/declaration_emit.rs",
    "crates/emitter/src/declarations/orchestration.rs",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

#[test]
fn owner_inventory_audit_is_closed_after_m35() {
    let inventory: Value = serde_json::from_slice(INVENTORY).expect("inventory is valid JSON");
    let audit = &inventory["summary"]["audit"];
    assert_eq!(
        audit["foundation_needed"], 0,
        "every m-1 foundation row flipped by the checker/harness lane"
    );
    assert_eq!(audit["pending"], 0);
    assert_eq!(audit["already_exact"], 308);

    let mut exact = 0usize;
    for row in inventory["rows"].as_array().expect("rows") {
        let surface = row["surface"].as_str().expect("surface");
        if surface != "printer-subgraph" && surface != "factory-parenthesizer" {
            continue;
        }
        assert_eq!(
            row["disposition"], "audit-already-exact",
            "{} still carries {}",
            row["name"], row["disposition"]
        );
        assert!(
            row["target_rung"].is_null(),
            "{} still targets a rung",
            row["name"]
        );
        let anchor = row["rust_anchor"].as_str().expect("anchor string");
        assert!(
            anchor.starts_with("crates/emitter/"),
            "{} anchor {anchor} is outside crates/emitter",
            row["name"]
        );
        exact += 1;
    }
    assert_eq!(exact, 308, "the audit covers exactly the 308 measured rows");
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable directory") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn declaration_syntax_is_constructed_only_at_the_dormant_allowlist() {
    let workspace = workspace();
    let crates = workspace.join("crates");
    let mut files = Vec::new();
    collect_rs_files(&crates, &mut files);

    let mut production_sites = BTreeSet::new();
    let mut occurrences = 0usize;
    for path in files {
        let relative = path
            .strip_prefix(&workspace)
            .expect("inside the workspace")
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read_to_string(&path).expect("readable source");
        let count = text.matches("with_declaration_syntax(true)").count();
        if count == 0 {
            continue;
        }
        occurrences += count;
        let is_test_path = relative.contains("/tests/");
        if !is_test_path {
            production_sites.insert(relative);
        }
    }
    assert!(
        occurrences > 0,
        "the option is exercised by the rung's tests"
    );
    let allowlist: BTreeSet<String> = DECLARATION_SYNTAX_ALLOWLIST
        .iter()
        .map(|entry| (*entry).to_owned())
        .collect();
    assert_eq!(
        production_sites, allowlist,
        "declaration_syntax production consumers must be exactly the dormant allowlist"
    );
}
