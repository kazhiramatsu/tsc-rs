# Ledger delta r9–r11 (reviewer pushback round: the never-observed PLAN-BASE range; continues DELTA-r8.md)

Same conventions as [DELTA-r7.md](DELTA-r7.md) / [DELTA-r8.md](DELTA-r8.md): `ledger.v1.json` stays the
start-SHA state (`c35e00ccb`); the rows below changed state in the r9–r11 rounds; evidence =
`records/measure/*.meta.json` (argv, env, head, dirty count, exit, seconds, log sha256). Counts are
not additive. The universe rows are ledger state RM (never observed) at the start SHA; "exact ×2" =
the complete command (diagnostics, writes, emit result, status, exit) replays twice identically
against the fresh TypeScript 6.0.3 observation.

## 217 rows (`fixtures/emitter-final-universe.json`, `ef7/universe-217.v1.json`)

| rows | before | after | evidence / cause (DESIGN §9.1.1, §9.1.2) |
| --- | --- | --- | --- |
| 106 noEmit-route rows | RM | exact ×2 | `universe-r9c` (EF7-NOEMIT-ROUTE, replay-side `load_program` branch) |
| 12 `usingDeclarationsWithESClassDecorators.*#module=system,target=esnext` | RM | exact ×2 | `universe-r9c` (EF7-SYSTEM-USING-EXPORT) |
| 3 `esDecorators-*-commentPreservation` (esnext) | RM | exact ×2 | `universe-r9d` (EF7-MEMBER-NAME-LEADING-COMMENT incl. accessor keyword) |
| `esDecorators-decoratorExpression.1` | RM | exact ×2 | `universe-r9c` (EF7-DECORATOR-PARSE-PARENS) |
| 4 `verbatimModuleSyntax*` | RM (typed refusal) | 2 exact ×2 (`universe-r9c`), 2 exact ×2 after EF7-VERBATIM-EXPORT-ASSIGNMENT (`universe-r11`) | EF7-VERBATIM-GATE, EF7-VERBATIM-EXPORT-ASSIGNMENT |
| `esDecorators-emitDecoratorMetadata#target=esnext` | RM (typed refusal) | exact ×2 | `universe-r9c` (EF7-METADATA-INERT) |
| `computedEnumMemberSyntacticallyString2#isolatedmodules=true` | RM | exact ×2 | `universe-r9c` (EF7-ENUM-18055, checker) |
| the other 89 rows | RM | exact ×2 at the start-SHA producer | `universe-r9b` |
| `esDecorators-decoratorExpression.3#experimentaldecorators=false` | RM | **KNOWN (UP, owner H2.9)** — typed refusal `ParseDiagnosticsDeferred` (parse-recovery emit boundary) | `universe-r11` |

## 1,798 rows (`fixtures/emitter-final-universe-plan-base.json.zst`, `ef7/universe-plan-base.v1.json`)

First complete replay at the r9 bytes (`universe-plan-base-r9`, chain15): **exact 1,626 / failed 172**.
The 172 rows split into the r10 causes (DESIGN §9.1.2) and the r11 causes (§9.1.3); the final
state is `universe-plan-base-r11` (chain16b) — REPORT §9.6 has the counts.

| rows (r9 census) | before | after | cause |
| --- | --- | --- | --- |
| 76 emit-bytes rows (System `using`/default exports, async lowering, helpers imports, decorators, verbatim elision, …) | RM | exact ×2 except the KNOWN rows below | r10: EF7-YIELD-PARENS, EF7-ASYNC-ARROW-LEXICAL-THIS, EF7-GENERATOR-LOOP-VARIABLE, EF7-OBJECT-LITERAL-INDENTED, EF7-EXPORTED-REST-HOIST, EF7-CJS-FLATTENED-EXPORT-NAME, EF7-CJS-EXPORT-INLINE, EF7-SYSTEM-IMPORT-HELPERS, EF7-HELPERS-IMPORT-PROLOGUE, EF7-VERBATIM-EXPORT-ASSIGNMENT, EF7-ASYNC-ACCESSOR-BODY, EF7-ASYNC-SHORTHAND-ASSIGNMENT, EF7-SYSTEM-IMPORT-EQUALS-EXPORTS, EF7-SYSTEM-NAMESPACE-ALIAS; r11: EF7-USING-HOISTED-CLASS-NAME, EF7-SYSTEM-EXECUTE-ASYNC, EF7-SYSTEM-HOISTED-DEFAULT-EXPORT, EF7-VERBATIM-IMPORT-EXPORT-ELISION, EF7-ASYNC-ARROW-SUPER-CAPTURE, EF7-DECORATED-STATIC-FIELD-COMMENT, EF7-SCOPED-NUMBERED-NAMES, EF7-PRESERVE-CJS-HELPERS, EF7-EXPORT-EQUALS-EMPTY-ASSIGNED-NAME |
| 13 `RequiredChildRemoved { promoted class name }` (ES5 anonymous default decorated classes) | RM (typed refusal) | exact ×2 | r11: EF7-ES5-ANONYMOUS-DEFAULT-CLASS-NAME |
| 4 `exportEmpty{Array,Object}BindingPattern` (amd/commonjs, es5) | RM (refusal `MixedSyntheticRange`) | exact ×2 | r10: EF7-CJS-FLATTENED-EXPORT-NAME |
| `esDecorators-classDeclaration-fields-staticAccessor#target=es2022` | RM (panic) | exact ×2 | r11: EF7-STATIC-ACCESSOR-RECEIVER |
| 2 `noCheck*` | RM (typed refusal) | exact ×2 | r10: EF7-NOCHECK-ROUTE |
| `isolatedModulesNoEmitOnError`, `isolatedModulesRequiresPreserveConstEnum` | RM (harness projection missing) | replayed (`universe-plan-base-r11`) | r11 harness projections |
| checker rows: `arrowExpressionBodyJSDoc`, `errorInUnnamedClassExpression`, `blockScopedEnumVariablesUseBeforeDef_*` ×2, `enumNoInitializerFollowsNonLiteralInitializer`, `tslibMissingHelper`, `tslibMultipleMissingHelper`, `tslibNotFoundDifferentModules`, `ctsFileInEsnextHelpers` | RM (diagnostics) | exact ×2 (see REPORT §9.6 for any that stay open) | r10: EF7-JSDOC-CHECK-NODE, EF7-SYMBOL-WRITTEN-FACE, EF7-ENUM-ISOLATED, EF7-ENUM-18056, EF7-ASYNC-HELPER-CHECKS |
| 35 parse-recovery rows (`asyncArrowFunction{6,7,8,9}_*`, `asyncFunctionDeclaration{6,7,9,10}_*`, `esDecorators-decoratorExpression.3#experimentaldecorators=true`, `topLevelAwaitErrors.1` ×2) | RM (typed refusal) | **KNOWN (UP, owner H2.9)** — `ParseDiagnosticsDeferred` (継続中) | `universe-plan-base-r11` |
| 4 bundler resolution rows (`bundlerDirectoryModule` ×3, `bundlerOptionsCompat`) | RM (typed refusal) | **KNOWN (UP, owner: module resolution request plan, `static-module-request-plan`)** (継続中) | `universe-plan-base-r11` |
| `importHelpersWithLocalCollisions#module=es2015` | RM (typed refusal) | **KNOWN (UP, owner H2.5h corpus adoption: helper import aliasing a colliding source identifier)** (継続中) | `universe-plan-base-r11` |
| remaining checker diagnostic rows (REPORT §9.6 table: isolatedModules alias diagnostics TS2865/2866/1269, JS checking, relatedInformation elaboration, type display, TS2589/TS2859, TS2343 through an ESM tslib entry) | RM (diagnostics) | **KNOWN (RD, owner: checker)** (継続中, cause per row in `KNOWN_PLAN_BASE`) | `universe-plan-base-r11` |

Still open after r11: only the KNOWN rows above (each with an owner and a cause string in
`crates/compiler/tests/emitter_final_universe.rs`), plus the two case-insensitive host rows of
DELTA-r8.md (oracle contract, integrator re-mint pending).
