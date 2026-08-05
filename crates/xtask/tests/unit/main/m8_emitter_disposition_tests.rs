use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

static TEMP_REPO_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempRepo(PathBuf);

impl TempRepo {
    fn new() -> Self {
        let sequence = TEMP_REPO_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tsc-rs-emitter-history-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        git_test(&path, &["init", "-q"]);
        git_test(&path, &["config", "user.email", "tests@example.invalid"]);
        git_test(&path, &["config", "user.name", "tsc-rs tests"]);
        Self(path)
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git_test(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn function(id_byte: char, line: usize, hash_byte: char) -> M8EmitterFunction {
    M8EmitterFunction {
        id: format!("d2:{}", id_byte.to_string().repeat(64)),
        name: format!("function{line}"),
        kind: "FunctionDeclaration".to_owned(),
        lexical_owner: None,
        lexical_path: format!("<top>/function{line}@{line}:1"),
        source_range: M8SourceRange {
            start: M8SourcePosition {
                offset: line * 10,
                line,
                character: 1,
            },
            end: M8SourcePosition {
                offset: line * 10 + 9,
                line: line + 1,
                character: 2,
            },
        },
        source_slice_sha256: hash_byte.to_string().repeat(64),
        direct_emitter: false,
        sites: Vec::new(),
        scc: format!("scc-{line}"),
        shortest_emitter_path: vec![format!("d2:{}", id_byte.to_string().repeat(64))],
    }
}

fn inventory() -> M8EmitterInventory {
    M8EmitterInventory {
        schema: 2,
        status: "draft/report-only".to_owned(),
        source: "vendor/typescript-6.0.3/lib/_tsc.js".to_owned(),
        source_sha256: "f".repeat(64),
        band: "all".to_owned(),
        summary: M8EmitterInventorySummary {
            source_declarations: 2,
            emitter_declarations: 0,
            diagnostic_references: 0,
            closure_declarations: 2,
            sccs: 2,
            nontrivial_sccs: 0,
            static_edges: 0,
            property_dispatch_edges: 0,
            unresolved_calls: 0,
        },
        functions: vec![function('a', 10, '1'), function('b', 20, '2')],
        graph: M8EmitterGraph {
            edges: Vec::new(),
            sccs: Vec::new(),
            unresolved_calls: Vec::new(),
        },
    }
}

fn dispositions() -> M8EmitterDispositions {
    M8EmitterDispositions {
        schema: 2,
        status: "draft".to_owned(),
        adjudication_commit: None,
        inventory_sha256: "inventory".to_owned(),
        entries: vec![
            M8EmitterDisposition {
                declaration: format!("d2:{}", "a".repeat(64)),
                disposition: "ported".to_owned(),
                owner: "Rust exact port ledger".to_owned(),
                evidence: "exact join".to_owned(),
            },
            M8EmitterDisposition {
                declaration: format!("d2:{}", "b".repeat(64)),
                disposition: "deferred".to_owned(),
                owner: "D2 static dependency closure".to_owned(),
                evidence: "exact path".to_owned(),
            },
        ],
    }
}

#[test]
fn historical_emitter_dispositions_follow_the_workspace_promotion() {
    let repo = TempRepo::new();
    let legacy = repo.0.join("tsrs2");
    fs::create_dir_all(&legacy).unwrap();
    let expected = dispositions();
    fs::write(
        legacy.join("m8-emitter-dispositions.json"),
        serde_json::to_vec_pretty(&expected).unwrap(),
    )
    .unwrap();
    git_test(&repo.0, &["add", "tsrs2/m8-emitter-dispositions.json"]);
    git_test(
        &repo.0,
        &["commit", "-q", "-m", "legacy emitter dispositions"],
    );
    let commit = String::from_utf8(git_test(&repo.0, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();

    assert_eq!(
        m8_emitter_dispositions_at(&repo.0, &commit).unwrap(),
        expected
    );
}

fn ledger() -> Vec<LedgerEntry> {
    vec![LedgerEntry {
        rust_path: PathBuf::from("crates/checker/src/example.rs"),
        rust_line: 7,
        rust_fn: "ported".to_owned(),
        port_name: "function10".to_owned(),
        version: "6.0.3".to_owned(),
        span_file: "_tsc.js".to_owned(),
        span_start: 10,
        span_end: 11,
        hash: "1".repeat(64),
    }]
}

#[test]
fn exact_identity_coverage_and_ledger_disposition_are_required() {
    let inventory = inventory();
    let ledger = ledger();
    let valid = dispositions();
    let stats =
        validate_m8_emitter_dispositions(Path::new("."), &inventory, "inventory", &ledger, &valid)
            .expect("complete exact dispositions");
    assert_eq!(
        stats,
        M8EmitterDispositionStats {
            ported: 1,
            deferred: 1,
            not_applicable: 0,
        }
    );

    let mut missing = valid.clone();
    missing.entries.pop();
    assert!(validate_m8_emitter_dispositions(
        Path::new("."),
        &inventory,
        "inventory",
        &ledger,
        &missing,
    )
    .is_err());

    let mut false_deferred = valid;
    false_deferred.entries[0].disposition = "deferred".to_owned();
    assert!(validate_m8_emitter_dispositions(
        Path::new("."),
        &inventory,
        "inventory",
        &ledger,
        &false_deferred,
    )
    .is_err());
}

#[test]
fn frozen_deferred_disposition_accepts_a_later_exact_port() {
    let inventory = inventory();
    let ledger = ledger();
    let mut frozen = dispositions();
    frozen.status = "frozen".to_owned();
    frozen.adjudication_commit = Some("a".repeat(40));
    frozen.entries[0].disposition = "deferred".to_owned();
    frozen.entries[0].owner = "D2 static dependency closure".to_owned();
    frozen.entries[0].evidence = "exact frozen path".to_owned();

    validate_m8_emitter_dispositions(Path::new("."), &inventory, "inventory", &ledger, &frozen)
        .expect("a post-freeze exact port is monotone implementation evidence");

    frozen.entries[0].disposition = "not-applicable".to_owned();
    assert!(validate_m8_emitter_dispositions(
        Path::new("."),
        &inventory,
        "inventory",
        &ledger,
        &frozen,
    )
    .is_err());
}

#[test]
fn frozen_anchor_requires_a_full_lowercase_commit() {
    assert!(is_full_lower_hex_commit(&"a".repeat(40)));
    assert!(!is_full_lower_hex_commit(&"A".repeat(40)));
    assert!(!is_full_lower_hex_commit(&"a".repeat(39)));
    assert!(!is_full_lower_hex_commit(&format!("{}g", "a".repeat(39))));
}
