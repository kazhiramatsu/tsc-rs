# Ledger delta (this batch's own evidence; the generated ledger stays the start-SHA state)

`ledger.v1.json` records the PLAN-BASE memberships at the start SHA `c35e00ccb`. The rows below
changed state in this batch; each points at the measurement record (`records/measure/*.meta.json`
carries head, dirty count, argv, env, exit, seconds, log sha256). Counts are not additive.

| case ID (+ profile) | before | after | evidence |
| --- | --- | --- | --- |
| 5 `post-t1-residuals/decorator-comments/*` (EF1) | known-native (RD) | exact ×2 (OB) | `records/ef1/*`, original projection kept |
| `isolatedModulesSourceMap`, `jsFileCompilationWithMapFileAsJsWithOutDir`, `requireOfJsonFileWithSourceMap`, `sourceMapValidationVarInDownLevelGenerator`, `sourceMapWithCaseSensitiveFileNamesAndOutDir` (H2.6c) | known (RD) | exact ×2 | `ef2-ef3-rows-20-r2`..`-r6` |
| `asyncAwait_es5`, `emitAccessExpressionOfCastedObjectLiteralExpressionInArrowFunctionES5` (H2.5h) | known | exact ×2 | `ef2-ef3-rows-20-r4`..`-r6` |
| `destructuringVariableDeclaration1ES5iterable` (H2.5h) | known | exact ×2 | `ef2-ef3-rows-20-r6` |
| `asyncImportedPromise_es5`, `emitter.asyncGenerators.classMethods.es5` (H2.5h) | known | exact ×2 | `ef2-ef3-rows-20-r7b` |
| class 40: nested-computed-name ×8, legacy-bound-this ×4, static-block-arrow ×4 | failed | exact ×2 | `ef4-ef5-class-40-r6` |
| class 40: concise-arrow ×4, field-arrow ×4, legacy-static-block ×4, direct-escaped ×2 | failed | exact ×2 (class 40 = 40/40) | `ef4-ef5-class-40-r7b` |
| global 14: `reactImportDropped` | failed | exact ×2 | `ef6-global-14-r3` |
| global 14: `linkTagEmit1` | failed | exact ×2 | `ef6-global-14-r6` |
| h2-8c transpile-routes known-open: `transpile-js/lang/es5-flags`, `program-no-check/flags/es5-loop-capture`, `…/es5-loop-capture-commonjs`, `…/control-checked/es5-flags` | known-open (inherited emitter `_super_1`) | exact (retired, original saved under `records/ef2/`) | `witness-transpile-routes-all-r7` |

Still open after r7 (all but the two case-insensitive rows closed in r8, see [DELTA-r8.md](DELTA-r8.md)): `decoratedBlockScopedClass2/3` (EF2-ALIAS-NUMBERING),
`awaitUsingDeclarationsInFor*` ×4 (EF2-AWAIT-USING-MISSING-NAME), `ES5For-of37`
(EF2-DETACHED-COMMENT), `sourceMapValidationDestructuringForArrayBindingPattern` es2015
(EF3-ITERABLE-2318), `sourceMapWithNonCaseSensitiveFileNames*` ×2 (oracle host contract),
`jsDeclarationsExportAssignedClassExpressionAnonymousWithSub` ×2 (EF6-IMPORT-TYPE-SELF).
