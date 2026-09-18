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
    ("typescript-6.0.3/compiler/contextuallyTypedParametersOptionalInJSDoc.ts#default", "checker (JS): contextual typing of JSDoc optional parameters (TS2345 position/type)"),
    ("typescript-6.0.3/compiler/deeplyNestedMappedTypes.ts#default", "checker: mapped-type instantiation depth / type display (TS2322 set)"),
    ("typescript-6.0.3/compiler/excessPropertyCheckIntersectionWithRecursiveType.ts#default", "checker: relatedInformation (6501) elaboration on lib index signature"),
    ("typescript-6.0.3/compiler/genericDefaultsJs.ts#default", "checker (JS): TS2351 reported for JS generic defaults"),
    ("typescript-6.0.3/compiler/importHelpersWithLocalCollisions.ts#module%3Des2015", "H2.5h corpus-adoption: tslib helper import aliasing a colliding source identifier (typed refusal)"),
    ("typescript-6.0.3/compiler/importNonExportedMember9.ts#default", "checker: TS2616 vs TS2597 diagnostic choice"),
    ("typescript-6.0.3/compiler/isolatedModulesExportDeclarationType.ts#default", "checker (isolatedModules): TS2865 sketchy alias local merge"),
    ("typescript-6.0.3/compiler/isolatedModulesExportImportUninstantiatedNamespace.ts#default", "checker (isolatedModules): TS1269"),
    ("typescript-6.0.3/compiler/isolatedModulesGlobalNamespacesAndEnums.ts#default", "checker (isolatedModules): global namespace/enum diagnostics"),
    ("typescript-6.0.3/compiler/isolatedModulesReExportType.ts#default", "checker (isolatedModules): TS1269 export import of a type"),
    ("typescript-6.0.3/compiler/isolatedModulesShadowGlobalTypeNotValue.ts#isolatedmodules%3Dtrue%2Cverbatimmodulesyntax%3Dfalse", "checker (isolatedModules): TS2866"),
    ("typescript-6.0.3/compiler/isolatedModulesShadowGlobalTypeNotValue.ts#isolatedmodules%3Dtrue%2Cverbatimmodulesyntax%3Dtrue", "checker (isolatedModules): TS2866"),
    ("typescript-6.0.3/compiler/isolatedModulesSketchyAliasLocalMerge.ts#isolatedmodules%3Dtrue%2Cverbatimmodulesyntax%3Dfalse", "checker (isolatedModules): TS2865"),
    ("typescript-6.0.3/compiler/isolatedModulesSketchyAliasLocalMerge.ts#isolatedmodules%3Dtrue%2Cverbatimmodulesyntax%3Dtrue", "checker (isolatedModules): TS2865"),
    ("typescript-6.0.3/compiler/jsExportMemberMergedWithModuleAugmentation2.ts#default", "checker (JS): TS2300 duplicate identifier across module augmentation"),
    ("typescript-6.0.3/compiler/jsExtendsImplicitAny.ts#default", "checker (JS): TS2351 reported for JS class heritage"),
    ("typescript-6.0.3/compiler/jsFileCompilationConstructorOverloadSyntax.ts#default", "checker (JS): diagnostic span length"),
    ("typescript-6.0.3/compiler/jsdocArrayObjectPromiseNoImplicitAny.ts#default", "checker (JS): relatedInformation (6500) elaboration on lib property"),
    ("typescript-6.0.3/compiler/jsdocTypedefBeforeParenthesizedExpression.ts#default", "checker (JS): spurious TS2300 for a JSDoc typedef"),
    ("typescript-6.0.3/compiler/jsxIntrinsicDeclaredUsingTemplateLiteralTypeSignatures.tsx#default", "checker: literal type display in TS2322 message"),
    ("typescript-6.0.3/compiler/mappedArrayTupleIntersections.ts#default", "checker: spurious diagnostic on mapped tuple intersection"),
    ("typescript-6.0.3/compiler/recursiveConditionalCrash4.ts#default", "checker: missing TS2589 (excessive instantiation depth)"),
    ("typescript-6.0.3/compiler/relationComplexityError.ts#default", "checker: TS2859 relation complexity budget"),
    ("typescript-6.0.3/compiler/tslibMultipleMissingHelper.ts#default", "checker: TS2343 missing tslib helpers"),
    ("typescript-6.0.3/compiler/tslibReExportHelpers2.ts#default", "checker: TS2343 for a helper re-exported through an ESM tslib entry"),
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
    ("typescript-6.0.3/conformance/expressions/typeSatisfaction/typeSatisfaction_errorLocations1.ts#default", "checker: relatedInformation (6500) elaboration on lib property"),
    ("typescript-6.0.3/conformance/externalModules/topLevelAwaitErrors.1.ts#module%3Des2022", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/externalModules/topLevelAwaitErrors.1.ts#module%3Desnext", "H2.9 parse-recovery emit boundary (typed refusal ParseDiagnosticsDeferred)"),
    ("typescript-6.0.3/conformance/jsdoc/checkJsdocSatisfiesTag9.ts#default", "checker (JS): relatedInformation (6500) elaboration"),
    ("typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsTypedefAndLatebound.ts#default", "checker (JS): extra diagnostic for late-bound typedef"),
    ("typescript-6.0.3/conformance/moduleResolution/bundler/bundlerDirectoryModule.ts#module%3Dnode18%2Cmoduleresolution%3Dbundler", "module resolution request plan (typed refusal static-module-request-plan; L2-3/H2 resolution owner)"),
    ("typescript-6.0.3/conformance/moduleResolution/bundler/bundlerDirectoryModule.ts#module%3Dnode20%2Cmoduleresolution%3Dbundler", "module resolution request plan (typed refusal static-module-request-plan; L2-3/H2 resolution owner)"),
    ("typescript-6.0.3/conformance/moduleResolution/bundler/bundlerDirectoryModule.ts#module%3Dnodenext%2Cmoduleresolution%3Dbundler", "module resolution request plan (typed refusal static-module-request-plan; L2-3/H2 resolution owner)"),
    ("typescript-6.0.3/conformance/moduleResolution/bundler/bundlerOptionsCompat.ts#default", "module resolution request plan (typed refusal static-module-request-plan; L2-3/H2 resolution owner)"),
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

fn native_matches(actual: &Value, frozen: &Value) -> bool {
    // The entire native command (or precise refusal), not just the case ID or
    // the first diagnostic. A known checker mismatch must not mask emit drift.
    actual == frozen
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
    let stale = known
        .iter()
        .filter(|id| report.exact.contains(**id))
        .collect::<Vec<_>>();
    let failures = report
        .diverging
        .iter()
        .filter_map(|(id, actual)| match native.get(id.as_str()) {
            None => Some(format!("{id}: unattributed divergence: {actual}")),
            Some(row) if !native_matches(actual, &row["native"]) => Some(format!(
                "{id}: recorded native divergence changed; owner/cause: {}",
                row["cause"]
            )),
            Some(_) => None,
        })
        .collect::<Vec<_>>();
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
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/emitter-final-known-native.json")).unwrap();
    let frozen = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| &row["native"])
        .find(|native| {
            native["kind"] == "complete"
                && !native["observation"]["writes"]
                    .as_array()
                    .unwrap()
                    .is_empty()
        })
        .unwrap();
    assert!(native_matches(frozen, frozen));
    let mut changed = frozen.clone();
    changed["observation"]["writes"][0]["callback_utf8_base64"] =
        Value::String("bmV3IGJ1Zw==".into());
    assert!(!native_matches(&changed, frozen));
    let mut changed = frozen.clone();
    changed["observation"]["reported_diagnostics"] = serde_json::json!([]);
    assert_ne!(changed, *frozen);
    assert!(!native_matches(&changed, frozen));
}

#[test]
fn known_refusal_rejects_changed_error_or_partial_writes() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/emitter-final-known-native.json")).unwrap();
    let frozen = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| &row["native"])
        .find(|native| native["kind"] == "failure")
        .unwrap();
    let mut changed = frozen.clone();
    changed["message"] = Value::String("unexpected emitter panic".into());
    assert!(!native_matches(&changed, frozen));
    changed["message"] = Value::String(format!("{}; additional partial write", frozen["message"]));
    assert!(!native_matches(&changed, frozen));
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
