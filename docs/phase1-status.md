# tsc-rs Phase 1 — Status Report

## Summary

**Phase 1 goal**: Restore `fn_stack` to bracketed push/pop discipline (original design) and fix the pre-existing bugs it surfaced.

**Progress**: Phase 1 started with **600 new FPs** immediately after adding `fn_stack.pop()`. The last banked archive was at **120 new FPs**; the current unbanked work now passes the low-load `golden-check` gate with **0 new FP/FN** against the standing golden snapshot.

| Metric | Phase 1 start | Current |
|---|---|---|
| Improvement FP removals | −908 | **−426** vs standing golden |
| Improvement OK_ADD | +535 | **+2912** vs standing golden |
| New FP | 600 | **0** vs standing golden |
| New FN | 7 | **0** vs standing golden |

Note: the Current column is the latest `golden-check` classification against
the standing golden snapshot, not a refreshed all-time banked FP inventory.

## Completed Bugs

### Bug 1: `declare let` erroneously firing 2454 (341 FPs)
- `binder.rs`: added `flags::AMBIENT` (bit 21)
- `bind_var_stmt`: variables with `declare` modifier get AMBIENT flag
- `mod.rs::check_use_before_declaration`: skip AMBIENT symbols
- `flow/definite_assignment.rs::da_var`: skip `declare` VarStmts entirely; mark bound names as pre-assigned

### Bug 2: `super` in static context (78 FPs)
- `Expr::Super` in `exprs.rs`: walk `this_container_stack` skipping Arrows to determine `is_static`; return `current_class_base_statics()` in static context, `current_class_base()` otherwise
- Fixed `superInStaticMembers1.ts`, `esDecorators-classDeclaration-classSuper.3.ts`, etc.

### Bug 3: 2454 duplicate emission + narrowing awareness (varied, mixed)
- `mod.rs::check_use_before_declaration`: added `fact_for(&key)` check — if narrowing has produced a fact at this read position, skip 2454 emission (matches tsc's flow-aware behavior: narrowed reads are known well-defined)
- The initial attempt with a `da_flow_handled` symbol set was **incorrect** (over-suppressed cross-container reads); reverted and replaced with the narrowing-based approach

### Bug 4: `decl_container != use_container` guard — refined (60+ FPs)
- Previously removed as too broad, now restored **only for top-level lets**:
  - `decl_container == 0 && use_container != 0` (top-level let, inner-function use) → skip 2454
  - tsc's conservative rule: module-level bindings can be initialized by any subsequent top-level statement before the inner function runs
- Function-local `let` used from a nested class/function **still fires** 2454 (matches tsc; was the original bug 3 improvement)
- Fixed `controlFlow*.ts` (do/while, while, for), `classStaticBlock22.ts` (partially), `literalTypes2.ts`, etc.

### Bug 5: `this_container_stack` pop missing (9+ FPs)
- Adding `fn_stack.pop()` at exit of `check_function_body` was the initial Phase 1 change; `this_container_stack.push()` was already paired at entry but **pop was missing**
- Added `this_container_stack.pop()` alongside `fn_stack.pop()`
- Fixed `superInStaticMembers1.ts` completely (9 remaining 2576 FPs), various other `super`/`this` issues in nested contexts

### Bug 6: namespace-body `this` tracking (targeted local fix)
- Added checker-side `namespace_stack` tracking with the function/class stack
  depths at namespace entry.
- `Expr::This` now reports namespace-body `this` only while still directly in
  the namespace body; nested class/function bodies keep their own `this` rules.
- Targeted regression test added. Full golden reclassification has not yet been
  rerun, so the aggregate FP counts below are still the previous baseline.

### Bug 7: `await using` empty declaration list in for-of
- Parser now recognizes `for await (await using of x)` / `for (await using of x)`
  as an `await using` declaration list followed by a for-of expression, instead
  of parsing `await using` as an await expression.
- The empty declaration list is represented as `decls: []` rather than a dummy
  empty-name variable, and for-in/of checking reports TS1123 instead of TS6133.
- Low-load `golden-check` classified the change as +9 correct additions and -4
  correct FP removals across 7 changed files, with 0 new FP/FN.

### Bug 8: static `super` with expression bases
- `super` in static members now resolves expression-style bases needed by ES
  decorator fixtures, specifically class expressions and `as any` bases such as
  `class C extends class {}` and `class C extends (function() {} as any)`.
- The change is intentionally limited to static `super`; arbitrary expression
  bases are not fed into normal inheritance relation checks, avoiding mixin and
  circular-base regressions.
- Class static property access now falls back to `Function` members for lookups
  such as `super.name` without merging those members into assignability shapes.
- Low-load `golden-check` after this fix reported 0 new FP/FN, with the
  cumulative snapshot delta classified as +9 correct additions and -65 correct
  FP removals.

### Bug 9: classic class-field constructor scope and `typeof this`
- `useDefineForClassFields` is now modeled as an option/directive. Classic
  instance field initializers/type annotations reject constructor
  parameter/local references with TS2301/TS2844 instead of falling through to
  outer-scope lookup or TS2304.
- `typeof this.x` now resolves through the active class `this` type, and
  `typeof this` participates in existing `this instanceof C` narrowing via the
  synthetic polymorphic `this` parameter.
- Parameter properties are treated as initialized before classic field
  initializers, but still report TS2729 under `useDefineForClassFields`.
- The golden classifier now passes `useDefineForClassFields` to the tsc oracle
  and compares diagnostics by line/column rather than byte offsets, matching the
  directive-preserving harness output more robustly.
- Low-load `golden-check` on the current state reports 0 new FP/FN, with the
  cumulative snapshot delta classified as +14 correct additions and -67 correct
  FP removals.

### Bug 10: type-guard receiver narrowing, boolean-branch merges, and DA-safe facts
- User-defined `this is T` predicates now narrow the method call receiver, so
  guards such as `box.isSupplies()` and nested receiver paths narrow the value
  that tsc narrows.
- Flow narrowing now separates type facts from facts that are strong enough to
  suppress TS2454. `facts` can merge aggressively for type checking, while
  `da_facts` only survive when every possible branch proves the read is
  definitely assigned.
- `&&`/`||` condition handling now merges alternate branch facts for true/false
  senses, including `A || B` true-branch and `A && B` false-branch narrowing.
  Duplicate guard expressions keep tsc-compatible negative-side behavior.
- Definite-assignment analysis now understands the same lightweight condition
  proofs used by short-circuit and conditional expressions: boolean constants,
  type-predicate calls, `instanceof`, `in`, normal equality operands, catch
  bindings, and `let x!` / `var x!` assertions.
- Low-load `golden-check` on the current state reports 0 new FP/FN, with the
  cumulative snapshot delta classified as +2885 correct additions and -197
  correct FP removals.

### Bug 11: lazy field-initializer scope and ambient TDZ
- Lazy `type_of_symbol(PropertyDecl)` initializer inference now evaluates under
  the declaration scope rather than whatever `current_scope` happened to be
  active when the property type was demanded. This keeps class field
  initializers inside nested functions bound to the original lexical parameters,
  fixing the `localTypes2/3` TS2448/TS2454 false positives without special-case
  name filtering.
- Ambient symbols are skipped before TDZ/use-before-declaration checks, so a
  later `declare const` does not produce TS2448 at earlier value reads.
- Added regression tests for both paths.
- Low-load `golden-check` on the current state reports 0 new FP/FN, with the
  cumulative snapshot delta classified as +2891 correct additions and -210
  correct FP removals.

### Bug 12: constructor/class-value architecture for mixins and class expressions
- Class values now preserve captured generic substitutions with mapper-backed
  static and instance type variants. This fixes class-expression factories such
  as `B1<number>()`, `new B2<number>().anon`, and generic returned inner
  classes without falling back to missing-property diagnostics or unsound
  `ClassStatics` erasure.
- Mixin constructor intersections are normalized at construct-signature lookup:
  rest-`any[]` mixin constructors contribute their instance members while the
  concrete base constructor controls arity and parameter checking. Protected
  access now recognizes the class instance carried by those constructor returns.
- Heritage evaluation distinguishes expression-base property lookup from real
  base-type cycles, while namespace-qualified class bases (`N.E`, `M.D`) are
  part of the nominal base chain. Cyclic bases report TS2506 directly and avoid
  cascading TS2415 compatibility errors.
- Overloaded `new` resolution now delays context-sensitive function arguments
  until the selected constructor signature is known, restoring contextual arrow
  diagnostics, and suppresses duplicate argument cascades for construct
  overload fallback.
- The global `Object` relation shortcut is limited to boxed Object without an
  index signature, preserving TS2411 checks for user-augmented `Object`.
- Added regression tests for generic class-expression captures, mixin protected
  access, heritage `this` cycle suppression, and overloaded constructor
  cascade behavior.
- Low-load `golden-check` on the current state reports 0 new FP/FN, with the
  cumulative snapshot delta classified as +2912 correct additions and -426
  correct FP removals.

## Remaining new FPs — categorized (last banked count: 120)

### Harness feature gaps — locally wired, baseline refreshed

`--check-batch` now parses fixture directives with `src/harness/mod.rs`, applies
known compiler-option directives, accepts common harness-only directives as
no-ops, expands `@filename:` into multi-file programs, and passes
`@extraRootFiles:` through `check_program_with_roots`. The golden baseline was
refreshed after this large behavior change.

| Code | Count | Cause |
|---|---|---|
| 2307 | TBD after focused audit | `@filename:` is now wired; `golden-check` now runs the tsc oracle over the expanded fixture for `main.ts` / `main.tsx` diagnostics |
| 1378 | TBD after focused audit | `@module:` / `@target:` are now wired and top-level await option gating is fixed |
| 2314 | TBD after focused audit | `.d.ts` / `@lib:` behavior still needs focused review |

Note: `scripts/parallel_classify.py` is now harness-aware for the standing
`main.ts` / `main.tsx` new-FP gate: it parses fixture directives, materializes
synthetic multi-file programs, passes directive-derived compiler options to the
tsc oracle, and compares locations by line/column. The legacy fallback
`difftest/golden_classify.py` still treats a file as plain `main.ts`, but uses
the same line/column location key.

### Deep engine improvements — Phase 1 scope but requires focused work

| Code | Count | Cause | Approach |
|---|---|---|---|
| 2683 | 4 (of 6 total) | `namespace M { var x = this; }` fires in tsc (namespace this) — requires `namespace_depth` tracking. Attempted fix introduced 9 new FPs (`this` inside a class-in-namespace mis-classified); needs the check refined to only fire when class_stack is truly empty at the correct scope level | Track namespace_depth AND ensure class_stack is checked in the right container |
| 2683 | 2 (computedPropertyNames19_ES5/ES6) | Same as above; also namespace_depth-driven |

### Existing separate bugs surfaced by pop (21 FPs)

Not Phase 1 in origin but exposed by cleaner state:

| Code | Files | Cause |
|---|---|---|
| 2322 | historical | Several mixin-class false positives were fixed by Bug 12; refresh this table before banking a new remaining-FP inventory |
| Others | small counts | 1431, 1108, 6133, 2702, 1103, 1163, 2339, 80007, 2403, 1375 |

## Regression tracking

Compared to the previous banked state (`tsc-rs-p1bug3.zip`, 199 FPs), the current state (`tsc-rs-p1bug5.zip`, 120 FPs) has:

- **77 new improvements**: 17 files, notably `superInStaticMembers1.ts: 2576x9`, `controlFlowDoWhileStatement.ts: 2454x9`, `controlFlowWhileStatement.ts: 2454x9`, `literalTypes2.ts: 2454x9`, `classStaticBlock22.ts: 2454x14` (mostly resolved), `typeGuardsInDoStatement/ForStatement/WhileStatement.ts` (all 2454x2 each).
- **1 net regression**: `classStaticBlock22.ts: 2454x1` — a class field initializer inside a static block that tsc's "implicit function boundary" rule silently skips. Pre-existing behavior in tsc's scope resolution.

## Files that will change if you resume

The current local state includes unbanked implementation/doc updates after the
last archived zip; run the verification commands below before banking a new
snapshot.

## Reproducible commands

Rebuild and verify:
```
cargo build --release && cargo build
./verify.sh golden-check
```

Low-load golden check:
```
TSRS_BATCH_JOBS=1 TSRS_CLASSIFY_JOBS=1 ./verify.sh golden-check
```

Or if `verify.sh golden-check` times out (tsc invocation dominates), use the parallel classifier:
```
python3 scripts/parallel_classify.py \
    /tmp/golden_diag.txt /tmp/golden_now.txt \
    lib/lib.tsrs.d.ts
```
By default the classifier uses 4 workers with a per-tsc timeout of 45s. Override
with `TSRS_CLASSIFY_JOBS` and `TSRS_TSC_TIMEOUT`.

## Banked artifacts

- `tsc-rs-p1a.zip` (343KB) — Phase 1 start (after just `fn_stack.pop()`; 600 FPs)
- `tsc-rs-p1bug1.zip` (344KB) — after Bug 1 (`declare let` AMBIENT; 259 FPs)
- `tsc-rs-p1bug3.zip` (345KB) — after Bug 3 (narrowing; 199 FPs)
- `tsc-rs-p1bug5.zip` (345KB) — after Bugs 4 & 5 (top-level guard, `this_container_stack.pop`; 120 FPs)
- **`tsc-rs-p1-final.zip` — current state (120 FPs), authoritative**

## Recommended next steps

1. **Focused audit of namespace-body `this` counts**:
   - `namespace_stack` tracking is implemented and the current low-load
     `golden-check` has 0 new FP/FN.
   - Refresh the categorized 2683/2331 counts when banking a new baseline.

2. **Decide whether to widen golden classification beyond the main file**:
   - `scripts/parallel_classify.py` currently keeps the historical gate scope:
     only `main.ts` / `main.tsx` diagnostics are compared.
   - A broader all-file gate would also classify helper-file diagnostics, but
     it should be introduced with an explicit baseline refresh.

3. **Remaining flow follow-ups**:
   - IIFE argument-eval flow tracking
   - Broader all-file diagnostic classification before refreshing the
     categorized remaining-FP table

4. **Refresh the remaining-FP inventory** after banking the current state:
   - Bug 12 removed the known mixin class-value cluster, so the historical
     categorized table above should not be treated as current counts.
