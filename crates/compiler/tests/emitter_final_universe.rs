//! EF7 (H2.8a-A-RES-EMITTER-FINAL): the 217 PLAN-BASE case IDs that no emit profile ever
//! selected (`universe:H2.0a`, ledger state RM), replayed twice each as ordinary complete
//! commands against fresh TypeScript 6.0.3 observations
//! (`fixtures/emitter-final-universe.json`, minted by
//! `scripts/observe-emitter-final-universe.mjs`). The comparator is the H2.8a original
//! output-matrix comparator; `KNOWN` lists the attributed open rows and the retire
//! assertion refuses a stale entry.
use std::collections::BTreeSet;
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

fn replay(fixture: &str, expected_rows: usize, known_rows: &[(&str, &str)]) {
    let report = h2_7d_original_corpus_shared::replay_universe_fixture(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        fixture,
    );
    let known = known_rows
        .iter()
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    assert_eq!(known.len(), known_rows.len(), "duplicate KNOWN row");
    let total = report.exact.len() + report.diverging.len();
    assert_eq!(total, expected_rows, "every {fixture} row replays");
    let stale = known
        .iter()
        .filter(|id| report.exact.contains(**id))
        .collect::<Vec<_>>();
    let unattributed = report
        .diverging
        .iter()
        .filter(|(id, _)| !known.contains(id.as_str()))
        .map(|(id, detail)| format!("{id}: {detail}"))
        .collect::<Vec<_>>();
    eprintln!(
        "emitter-final universe {fixture}: exact {} / known {} / failed {}",
        report.exact.len(),
        report.diverging.len() - unattributed.len(),
        unattributed.len()
    );
    assert!(
        stale.is_empty(),
        "KNOWN rows replay exact now; retire them: {stale:?}"
    );
    assert!(
        unattributed.is_empty(),
        "{} unattributed universe divergences:\n{}",
        unattributed.len(),
        unattributed.join("\n")
    );
}
