# Workstream: U6 — unused-diagnostics FP finish (design + steps)

**Yield**: unused-family FPs remaining @3242fdc: 6133=156, 6196=11,
6198=6 (total ~173). Self-contained, well-understood subsystem (U1–U5c
already mirrored tsc's checkUnusedLocalsAndParameters /
checkUnusedClassMembers — see the U-sweep commit messages
355de8d..86d2dc8 for the semantics already in place). Good FIRST
workstream for a new agent: small blast radius, all mechanics
established.

**Do NOT touch the FN side**: unused FNs (6133×387) are dominated by
parse-error-gated files (parserRealSource11 = 87, parserharness = 36,
typeGuardFunctionErrors = 29) — that is parse-error-gate.md territory,
plus `.d.ts` suggestion collection (knowledge-base.md §4) and the 7043
family (unimplemented, out of scope).

## Mined FP buckets (2026-07-06 snapshot — re-mine per README before starting)

Top FP files (unused codes only, count = FP diagnostics):

```
 9  memberFunctionsWithPrivateOverloads.ts   <- root cause A (identified)
 9  parameterInitializersForwardReferencing.ts
 7  asyncFunctionDeclarationParameterEvaluation.ts   <- root cause B family
 5  awaitUnion_es5.ts                                <- B
 4  asyncMethodWithSuper_es5.ts                      <- B
 4  genericObjectRest.ts
 4  genericCallWithObjectTypeArgsAndInitializers.ts
 3  controlFlowBindingPatternOrder.ts
 3  destructuringArrayBindingPatternAndAssignment3.ts
 3  asiPreventsParsingAsInterface02.ts
 2× asyncArrowFunction7_es2017 / asyncFunctionDeclaration10_es2017 /
    asyncFunctionDeclaration7_es2017 / asyncArrowFunction7_es5 /
    asyncFunctionDeclaration10_es5 (all B)
```

### Root cause A — per-overload reporting on private class members [M]

tsrs reports 6133 for an unused private OVERLOADED method at EVERY
overload declaration; tsc reports ONCE per member symbol.

- Where: the unused main loop's per-declaration reporting introduced in
  U5b (src/checker/mod.rs — grep `per DECLARATION` or the 6133 report
  site in the unused loop). U5b's per-decl rule is correct for
  MERGED SYMBOLS of different kinds (func+ns → 6133+6133); it
  over-applies to class-member overload sets, which tsc reports via
  checkUnusedClassMembers ONCE (anchor = the symbol's
  valueDeclaration-equivalent: tsrs convention from U3/U4 = the FIRST
  declaration).
- NOTE (interaction, 2026-07-06): the relation-core-1 commit @3242fdc
  made the BINDER merge class-method overload declarations into one
  member symbol. Re-probe memberFunctionsWithPrivateOverloads FIRST —
  the merge may have changed the shape of this bucket (fewer, different
  positions, or already fixed). If probe shows 0 one-sided unused
  lines, this bucket is done; record in NOTES and move on.
- Fix shape if still present: in the class-member unused reporting
  path, report at `decls.first()` only (or at the implementation-less
  first overload), matching the U3 anchor convention
  ("setter-with-getter-pair carried by the getter" precedent shows how
  anchors are chosen there).
- Pin fixture: memberFunctionsWithPrivateOverloads.ts must go to ZERO
  one-sided unused lines (other codes in that file are separate).

### Root cause B — async/await ES5 family (~25 FPs) [P]

All fixtures are `async` functions compiled with `@target: es5`-era
settings. HYPOTHESIS (unverified — probe first): the ES5 async
DOWNLEVEL path involves helper references (`__awaiter`-style capture of
parameters / `_this`/`_arguments`) that tsc's emit-marking or
checkUnusedLocals treats as USES, or tsc simply does not run the
suggestion for parameters captured by the async transform. Compare
with the U5b ORACLE ARTIFACT (knowledge-base.md §1): the oracle runs
`program.emit()` BEFORE `getSuggestionDiagnostics`, and emit TRANSFORMS
can mark symbols referenced — the es5-async transform touches
parameters and `this`. Expected fix shape: extend
`unused_suggestion_emit_suppressed` / the emit-marking mirror (grep
`module_body_instance_state` and `unused_suggestion_emit_suppressed`
in the checker for the existing precedent) to cover what the async-es5
transform references.

Procedure:
1. Probe asyncFunctionDeclarationParameterEvaluation.ts; for each FP,
   note whether the symbol is a parameter/local and whether it is
   in an `async` function.
2. Micro-fixture: an unused param in an async fn at `@target: es5`,
   the same at `@target: es2017`. The oracle's difference between the
   two pins the rule.
3. Implement as a narrow suppression keyed on (async container +
   script_target_rank below the async-native threshold) in the unused
   reporting path — NOT in use-marking (suppression is safer: it can
   only remove FPs).
4. Pin fixtures: all `*_es5`/`*_es2017` files in the bucket list.

### Root cause C — remaining singles [T]

parameterInitializersForwardReferencing (9), genericObjectRest (4),
controlFlowBindingPatternOrder (3), destructuring* (3), asi* (3):
probe each, classify against the U1–U5 semantics in the commit
messages (write-only access, self-reference, pattern grouping,
param-default scoping #36295). Each is likely a small use-marking gap.
Weak models: fix only the ones whose diagnosis EXACTLY matches a rule
already described in a U-sweep commit message; stop-note the rest.

## Gate

Standard loop (EXECUTION-GUIDE): per-bucket commits, 0 NEW_FP /
0 NEW_FN each, full gate + golden-save at the end. The unused subsystem
has 89 pinned tests — several assert exact unused output; if one fails
your change contradicts a pinned tsc behavior: re-probe before touching
the test.
