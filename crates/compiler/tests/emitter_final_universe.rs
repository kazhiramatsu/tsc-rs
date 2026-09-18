//! EF7 (H2.8a-A-RES-EMITTER-FINAL): the 217 PLAN-BASE case IDs that no emit profile ever
//! selected (`universe:H2.0a`, ledger state RM), replayed twice each as ordinary complete
//! commands against fresh TypeScript 6.0.3 observations
//! (`fixtures/emitter-final-universe.json`, minted by
//! `scripts/observe-emitter-final-universe.mjs`). The comparator is the H2.8a original
//! output-matrix comparator; `KNOWN` lists the attributed open rows and the retire
//! assertion refuses a stale entry.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[allow(dead_code)] // The shared source also holds the D/E and output-matrix entries.
#[path = "integration/h2_7d_original_corpus_shared.rs"]
mod h2_7d_original_corpus_shared;

/// Attributed open rows (case id → cause id in DESIGN.md §9.1). A row listed
/// here must still diverge; one that replays exact must be retired from this list.
const KNOWN: &[(&str, &str)] = &[
    ("typescript-6.0.3/conformance/esDecorators/esDecorators-decoratorExpression.3.ts#experimentaldecorators%3Dfalse", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
];

/// Same contract for the never-observed PLAN-BASE set
/// (`emitter-final-universe-plan-base.json[.zst]`, 1,798 rows).
const KNOWN_PLAN_BASE: &[(&str, &str)] = &[
    ("typescript-6.0.3/conformance/async/es2017/asyncArrowFunction/asyncArrowFunction6_es2017.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es2017/asyncArrowFunction/asyncArrowFunction7_es2017.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es2017/asyncArrowFunction/asyncArrowFunction8_es2017.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es2017/asyncArrowFunction/asyncArrowFunction9_es2017.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es2017/functionDeclarations/asyncFunctionDeclaration10_es2017.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es2017/functionDeclarations/asyncFunctionDeclaration6_es2017.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es2017/functionDeclarations/asyncFunctionDeclaration7_es2017.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es2017/functionDeclarations/asyncFunctionDeclaration9_es2017.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/asyncArrowFunction/asyncArrowFunction6_es5.ts#target%3Des2015", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/asyncArrowFunction/asyncArrowFunction6_es5.ts#target%3Des5", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/asyncArrowFunction/asyncArrowFunction7_es5.ts#target%3Des2015", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/asyncArrowFunction/asyncArrowFunction7_es5.ts#target%3Des5", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/asyncArrowFunction/asyncArrowFunction8_es5.ts#target%3Des2015", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/asyncArrowFunction/asyncArrowFunction8_es5.ts#target%3Des5", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/asyncArrowFunction/asyncArrowFunction9_es5.ts#target%3Des2015", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/asyncArrowFunction/asyncArrowFunction9_es5.ts#target%3Des5", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/functionDeclarations/asyncFunctionDeclaration10_es5.ts#target%3Des2015", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/functionDeclarations/asyncFunctionDeclaration10_es5.ts#target%3Des5", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/functionDeclarations/asyncFunctionDeclaration6_es5.ts#target%3Des2015", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/functionDeclarations/asyncFunctionDeclaration6_es5.ts#target%3Des5", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/functionDeclarations/asyncFunctionDeclaration7_es5.ts#target%3Des2015", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/functionDeclarations/asyncFunctionDeclaration7_es5.ts#target%3Des5", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/functionDeclarations/asyncFunctionDeclaration9_es5.ts#target%3Des2015", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es5/functionDeclarations/asyncFunctionDeclaration9_es5.ts#target%3Des5", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es6/asyncArrowFunction/asyncArrowFunction6_es6.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es6/asyncArrowFunction/asyncArrowFunction7_es6.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es6/asyncArrowFunction/asyncArrowFunction8_es6.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es6/asyncArrowFunction/asyncArrowFunction9_es6.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es6/functionDeclarations/asyncFunctionDeclaration10_es6.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es6/functionDeclarations/asyncFunctionDeclaration6_es6.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es6/functionDeclarations/asyncFunctionDeclaration7_es6.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/async/es6/functionDeclarations/asyncFunctionDeclaration9_es6.ts#default", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/esDecorators/esDecorators-decoratorExpression.3.ts#experimentaldecorators%3Dtrue", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/externalModules/topLevelAwaitErrors.1.ts#module%3Des2022", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/externalModules/topLevelAwaitErrors.1.ts#module%3Desnext", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
];

#[test]
fn universe_rows_match_complete_production_commands() {
    replay("emitter-final-universe.json", 217, KNOWN);
}

#[test]
fn plan_base_rows_match_complete_production_commands() {
    replay(
        "emitter-final-universe-plan-base.json",
        1798,
        KNOWN_PLAN_BASE,
    );
}

fn shard(value: &str) -> (usize, usize) {
    let (index, count) = value.split_once('/').expect("shard must be INDEX/COUNT");
    let (index, count): (usize, usize) = (index.parse().unwrap(), count.parse().unwrap());
    assert!(
        count > 0 && count <= 16 && index < count,
        "invalid universe shard"
    );
    (index, count)
}

fn known_failures(
    native: &BTreeMap<&str, &Value>,
    report: &h2_7d_original_corpus_shared::UniverseReplay,
) -> (Vec<String>, Vec<String>) {
    let stale = report
        .exact
        .iter()
        .filter(|id| native.contains_key(id.as_str()))
        .cloned()
        .collect();
    let failures = report
        .diverging
        .iter()
        .filter_map(|(id, actual)| match native.get(id.as_str()) {
            None => Some(format!("{id}: unattributed divergence: {actual}")),
            Some(row) if actual != &row["native"] => Some(format!(
                "{id}: recorded native divergence changed; owner/cause: {}",
                row["cause"]
            )),
            Some(_) => None,
        })
        .collect();
    (stale, failures)
}

fn replay(fixture: &str, expected_rows: usize, known_rows: &[(&str, &str)]) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let artifact: Value = serde_json::from_slice(
        &std::fs::read(
            workspace.join("crates/compiler/tests/fixtures/emitter-final-known-native.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(artifact["schema"], 1);
    assert_eq!(artifact["kind"], "emitter-final-known-native");
    let rows = artifact["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["fixture"] == fixture)
        .collect::<Vec<_>>();
    let native = rows
        .iter()
        .map(|row| (row["case_id"].as_str().unwrap(), *row))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(native.len(), rows.len(), "duplicate native row");
    let known = known_rows
        .iter()
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    assert_eq!(known.len(), known_rows.len(), "duplicate KNOWN row");
    assert_eq!(
        native.keys().copied().collect::<BTreeSet<_>>(),
        known,
        "native observations and KNOWN IDs must have identical membership"
    );
    for (id, cause) in known_rows {
        assert_eq!(native[id]["cause"], *cause, "{id}: owner/cause changed");
    }
    let shard = std::env::var("TSC_RS_EMITTER_FINAL_SHARD")
        .ok()
        .map(|s| shard(&s));
    let filter = std::env::var("TSC_RS_EMITTER_FINAL_CASE_FILTER").ok();
    let set = std::env::var("TSC_RS_EMITTER_FINAL_CASE_SET").unwrap_or_else(|_| "all".into());
    assert!(
        matches!(set.as_str(), "all" | "known"),
        "case set must be all or known"
    );
    let select = |index: usize, id: &str| {
        shard.is_none_or(|(shard, count)| index % count == shard)
            && filter.as_ref().is_none_or(|filter| id.contains(filter))
            && (set == "all" || known.contains(id))
    };
    let report = h2_7d_original_corpus_shared::replay_universe_fixture(&workspace, fixture, select);
    assert_eq!(
        report.all_ids.len(),
        expected_rows,
        "frozen universe membership"
    );
    assert!(
        known.iter().all(|id| report.all_ids.contains(*id)),
        "KNOWN row absent from fixture"
    );
    let selected = report
        .all_ids
        .iter()
        .enumerate()
        .filter(|(i, id)| select(*i, id))
        .map(|(_, id)| id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(!selected.is_empty(), "empty universe selection");
    let executed = report
        .exact
        .iter()
        .chain(report.diverging.keys())
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(executed, selected, "every selected row must replay");
    let (stale, failures) = known_failures(&native, &report);
    eprintln!(
        "emitter-final universe {fixture}: selected {}/{} / exact {} / known {} / failed {}",
        selected.len(),
        expected_rows,
        report.exact.len(),
        report.diverging.len() - failures.len(),
        failures.len()
    );
    assert!(
        stale.is_empty(),
        "KNOWN rows replay exact now; retire them: {stale:?}"
    );
    assert!(
        failures.is_empty(),
        "{} universe divergences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn known_checker_divergence_rejects_changed_output_and_diagnostics() {
    // Keep this comparison guard after the live checker KNOWN set becomes
    // empty: the archived pre-repair observation is still a real divergence.
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../docs/design/greenfield/slices/emitter-final-batch/integration/records/retired-checker-known.v1.json"
    )).unwrap();
    let row = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["native"]["kind"] == "complete"
                && !row["native"]["observation"]["writes"]
                    .as_array()
                    .unwrap()
                    .is_empty()
        })
        .unwrap();
    let id = row["case_id"].as_str().unwrap();
    let native = BTreeMap::from([(id, row)]);
    let mut report = h2_7d_original_corpus_shared::UniverseReplay::default();
    report.diverging.insert(id.into(), row["native"].clone());
    assert_eq!(known_failures(&native, &report), (vec![], vec![]));
    report.diverging.get_mut(id).unwrap()["observation"]["writes"][0]["callback_utf8_base64"] =
        Value::String("bmV3IGJ1Zw==".into());
    assert_eq!(known_failures(&native, &report).1.len(), 1);
    report.diverging.insert(id.into(), row["native"].clone());
    report.diverging.get_mut(id).unwrap()["observation"]["reported_diagnostics"] =
        serde_json::json!([]);
    assert_eq!(known_failures(&native, &report).1.len(), 1);
    // An exact row must retire; relabelling a new case as known also fails.
    report.diverging.clear();
    report.exact.insert(id.into());
    assert_eq!(known_failures(&native, &report).0, vec![id.to_owned()]);
    report.exact.clear();
    report
        .diverging
        .insert("unlisted case".into(), row["native"].clone());
    assert_eq!(known_failures(&native, &report).1.len(), 1);
}

#[test]
fn known_refusal_rejects_changed_error_or_partial_writes() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/emitter-final-known-native.json")).unwrap();
    let row = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["native"]["kind"] == "failure"
                && row["native"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("partial callback paths/SHA256: []")
        })
        .unwrap();
    let id = row["case_id"].as_str().unwrap();
    let native = BTreeMap::from([(id, row)]);
    let mut report = h2_7d_original_corpus_shared::UniverseReplay::default();
    report.diverging.insert(id.into(), row["native"].clone());
    assert_eq!(known_failures(&native, &report), (vec![], vec![]));
    report.diverging.get_mut(id).unwrap()["message"] =
        Value::String("unexpected emitter panic".into());
    assert_eq!(known_failures(&native, &report).1.len(), 1);
    report.diverging.get_mut(id).unwrap()["message"] =
        Value::String(row["native"]["message"].as_str().unwrap().replace(
            "partial callback paths/SHA256: []",
            "partial callback paths/SHA256: [(unexpected.js, changed-hash)]",
        ));
    assert_eq!(known_failures(&native, &report).1.len(), 1);
}

#[test]
fn universe_shards_are_disjoint_and_complete() {
    for size in [217, 1798] {
        let mut selected = BTreeSet::new();
        for part in 0..4 {
            let (index, count) = shard(&format!("{part}/4"));
            for row in (0..size).filter(|row| row % count == index) {
                assert!(selected.insert(row));
            }
        }
        assert_eq!(selected.len(), size);
    }
}
