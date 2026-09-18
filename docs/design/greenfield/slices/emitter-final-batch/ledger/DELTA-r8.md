# Ledger delta r8 (second request: the remaining attributed causes; continues DELTA-r7.md)

Same conventions as [DELTA-r7.md](DELTA-r7.md): `ledger.v1.json` stays the start-SHA state
(`c35e00ccb`); rows below changed state in the r8 round; evidence = `records/measure/*.meta.json`
(argv, env, head, dirty count, exit, seconds, log sha256). Counts are not additive.

| case ID (+ profile) | before | after | evidence |
| --- | --- | --- | --- |
| `decoratedBlockScopedClass2` (H2.5h es5) | known (RD) | exact ×2 | `ef2-ef3-rows-20-r8c` → `-r8` (EF2-BLOCK-SCOPED-DECORATED + EF2-ALIAS-NUMBERING) |
| `decoratedBlockScopedClass3` (H2.5h es5) | known (RD) | exact ×2 | `ef2-ef3-rows-20-r8d` → `-r8` (EF2-ALIAS-NUMBERING: print-time substitution spelling) |
| `ES5For-of37` (H2.5h es5) | known (RD) | exact ×2 | `ef2-ef3-rows-20-r8c` → `-r8` (EF2-DETACHED-COMMENT) |
| `awaitUsingDeclarationsInForAwaitOf.3`, `awaitUsingDeclarationsInForAwaitOf` (H2.5h es5) | known (RD) | exact ×2 | `ef2-ef3-rows-20-r8c` → `-r8` (EF2-AWAIT-USING-MISSING-NAME) |
| `awaitUsingDeclarationsInForOf.1`, `awaitUsingDeclarationsInForOf.5` (H2.5h es5) | known (RD) | exact ×2 | `ef2-ef3-rows-20-r8d` → `-r8` (EF2-AWAIT-USING-MISSING-NAME + EF2-LOOP-VARIABLE-POLICY) |
| `sourceMapValidationDestructuringForArrayBindingPattern` es2015 (H2.6c map-observation + H2.6a source-map) | known (RD) ×2 | exact ×2 ×2 | `ef2-ef3-rows-20-r8c` → `-r8` (EF3-ITERABLE-2318, checker) |
| `jsDeclarationsExportAssignedClassExpressionAnonymousWithSub` ×2 (global 14) | failed | exact ×2 (global 14 = 14/14) | `ef6-global-14-r8` (EF6-IMPORT-TYPE-SELF, checker) |
| `sourceMapWithNonCaseSensitiveFileNames`, `…AndOutDir` (H2.6c) | known (RD; oracle host contract) | **unchanged: known** — tsc-rs matches the proposed directive-honoring observation completely (both rows), the frozen identity-canonical observation differs only in map `sources`; retire only after the integrator re-mints (`records/oracle/case-canonical-proposal.md`) | `ef2-ef3-rows-20-r8c` (`compare-outfile-candidate.py`), `-r8` |

Rows: EF2/EF3 21 profile memberships = **19 exact / 2 known / 0 failed** at the final r8 bytes
(`ef2-ef3-rows-20-r8`, exit 0). Class 40 and global 14: see REPORT §8.1 (chain10).

Still open: only the two case-insensitive host rows above (oracle contract, 方針 1 adopted;
integrator re-mint pending). No attributed emitter/checker cause remains in this batch's scope;
the design-only items (EF2-COMMENT-BOUNDARY for the converted-loop trailing comments seen in
probe2/B, no row) stay in DESIGN §5.1.
