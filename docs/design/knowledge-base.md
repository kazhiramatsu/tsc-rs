# Knowledge base: pinned non-obvious facts

Facts that cost real debugging time to establish. Check here BEFORE
investigating a confusing FP/FN — several of these masquerade as bugs.
Each entry says how it was pinned. (This file exists because earlier
session notes lived in a private memory other agents cannot read.)

## 1. Oracle emit artifact (suggestion-band diagnostics)

`difftest/diag_oracle.js` runs `program.emit()` BEFORE
`getSuggestionDiagnostics()`. Emit TRANSFORMS mark symbols as
referenced. Consequence, pinned during U5b: **instantiated non-ambient
namespaces and non-const enums NEVER surface as unused suggestions**
in the oracle output (errors under `noUnusedLocals` still fire — those
are collected pre-emit). tsrs mirrors this via
`module_body_instance_state` (tsc getModuleInstanceState projection)
and `unused_suggestion_emit_suppressed` (honors noEmit /
preserveConstEnums) in the checker. If a suggestion-band FP/FN makes
no sense, CHECK EMIT-MARKING FIRST. The es5-async transform is the
suspected next instance (archive/workstreams/u6-unused-fp.md root cause B).

## 2. Suggestion vs error band

7043–7050 and option-off 6133/6196/6198/6199 are SUGGESTION-category
in tsc; the oracle collects suggestions for all files; the non-strict
comparison (everything the sweep currently gates on) ignores category.
So a "6133 FN" may be a suggestion tsc emits and tsrs doesn't — same
bucket as errors for our metric.

## 3. Standing probe noise (ignore these lines in every probe)

- `lib.tsrs.d.ts:189:11 TS2430 Interface 'Generator...'` (tsrs side)
- `lib.tsrs.d.ts:504:19 TS6133 'T' is declared but...` (oracle side)
- `lib.tsrs.d.ts:...   TS2317 Global type 'Iterable' must have 3 type
  parameter(s)` family (oracle side) — curated-lib arity gap; fixing it
  is part of lib-gap Stage 3 and would clean every probe.

These are lib-internal and filtered by the classifier's LIBCODES; they
appear one-sided in probes but are NOT part of your delta.

## 4. Known standing families with established root causes

(From the U-sweep and operator-sweep sessions; each verified then
deliberately deferred. Do not re-diagnose from scratch.)

- **.d.ts unused**: oracle collects suggestions for .d.ts files; tsrs
  skips unused analysis there → standing FNs.
- **functionNameConflicts anomaly**: in SCRIPT (non-module) containers
  tsc does NOT report func-site 6133 for func-first func+var merges,
  but DOES in modules. Unexplained by source reading; pinned by probes;
  standing FP on fn1/fn5-style fixtures.
- **2554 arity FPs** on optionalChaining pattern-parameter fixtures:
  tsc getMinArgumentCount nuance (binding-pattern params with defaults)
  not mirrored.
- **requiresScopeChange approximation** (param-default scoping #36295,
  U4): tsrs walks object-literal property VALUES where tsc walks names
  only — minor over-trigger risk, no corpus damage seen yet.
- **import-equals require-form dotted alias** (`import u = require(m);
  u.T`) → 2702 standing FP: alias→module-exports namespace modeling
  missing.
- **Grammar families unimplemented**: 1235 (namespace only at top
  level), 1184 (modifiers-cannot-appear-here), 1232
  (import-decl-in-fn).
- **declarationFileForTsJsImport** (12×6133): harness infra gap
  (package.json root + multi-value `@module:` matrix → tsrs bails
  6054). See stall-playbook §2.5.
- **`debugger` scans as an identifier** (only reserved word missing
  from the scanner). Two suppressions live in
  `reportable_namespace_name_span` (name == "debugger", cross-line
  namespace names); the real lookahead fix is coupled to parser
  recovery (archive/workstreams/parse-error-gate.md work) — remove the suppressions when that
  lands.
- **Dotted-namespace flattening**: `namespace a.b.c` parses as ONE
  NamespaceDecl (name = first part). Causes 2339/2708 FPs and
  shadowedInternalModule phantoms. Design provenance:
  archive/workstreams/relation-core-2.md §A.4.
- **Documented FNs** (accepted): unknownControlFlow fx2/fx4 (anon-`{}`
  declaration identity — stall-playbook §2.3);
  typeArgumentsWithStringLiteralTypes01 (order-dependent literal
  widening — stall-playbook §2.2 /
  archive/workstreams/relation-core-2-steps.md STAGE I).

## 5. Harness & corpus facts

- **BOM**: 602/5907 fixtures carry U+FEFF; ONE leading BOM is stripped
  before fixture parsing in BOTH `src/harness/mod.rs::parse_fixture`
  and `scripts/parallel_classify.py::parse_fixture`. Any change to
  fixture parsing must keep the two in lockstep (double-blind hazard).
- **Batch base options are strict**: `tsrs --check-batch` and the
  classifier both apply strict-on defaults unless the fixture's
  directives override. Plain `tsrs --check file.ts` does NOT — probe
  results from `--check` can differ from batch; prefer probe.py.
- **Oracle tsc defaults strict too**: when invoking diag_oracle.js
  manually for a non-strict experiment, pass options explicitly.
- **main.ts line numbers** = post-directive-strip; alignment against
  the raw fixture varies by directive count (blank line after
  directives is also stripped).
- **Determinism**: `TSRS_JOBS=1` is the default and the only
  deterministic intra-fixture mode; `--check-batch --jobs N`
  (fixture-level) is deterministic and carries throughput. Never set
  `TSRS_JOBS>1` in verification.
- **node_key(x)**: address-based; unique and stable WITHIN one process
  run only. Never persist or compare across runs.
- **Fresh vs regular literals**: `types.fresh(t)`/`regular(t)`;
  `boolean` is the interned union `true | false` (`types.boolean` id
  check, no `TypeKind::Boolean` variant).
- **`cargo fmt` rewrites files** — run it only immediately before the
  final commit of a change, never mid-investigation (it invalidates
  your mental line numbers and pending Edit anchors).

## 6. Relation-engine specifics (post operator/relation-core sweeps)

- The comparable relation runs under `rel.erase_generic_sigs == true`
  with results in `rel.comparable_cache` (assignable results in
  `rel.relation_cache`). Entry point: `cast_comparable(a, b)` =
  `comparable_dir(a,b) || comparable_dir(b,a)`.
- Inside `related()` under the comparable flag: reversed-simple rules
  (base primitive ~ its literal, `unknown` ~ anything, `object` ~ `{}`)
  and union-SOURCE = SOME-member.
- `signature_related(s, t, is_ctor, force_erase, ctx)`: force_erase is
  true for multi-signature (overloaded) list comparisons in ANY
  relation, and erase also applies when the comparable flag is on.
  Variance: params contravariant; bivariant fallback when
  `!strict_function_types` OR (rest slot) `t.from_method`.
- `Signature.from_method` = declared as class/interface METHOD (tsc
  strictVariance keys on the TARGET declaration kind).
- Private/protected nominality (2325/2442) enforced in the per-prop
  loop COMPARABLE-MODE ONLY (see
  archive/workstreams/relation-core-2-steps.md STAGE N for the
  historical assignable-side plan).
- Assignment narrowing (flow resolver `assigned_type`): union declared
  → getAssignmentReducedType filter; auto query → widened RHS;
  everything else → DECLARED. Pinned by oracle probe
  (`let x: string; x = "a"` reads `string`).

## 7. Mining/tooling landmines

- Per-file deltas: ALWAYS diff `/tmp/golden_now.txt` (gate's snapshot)
  vs `/tmp/golden_diag.txt`, keyed by full absolute path with
  `endswith('/'+basename)`. Ad-hoc `--check-batch` on hand-built lists
  produced two false alarms (relative-path keys; fixture-name prefix
  collisions like `partiallyAnnotatedFunctionInference*`).
- `grep <name> golden` can match MULTIPLE fixtures (same basename in
  compiler/ and conformance/ trees, or prefix collisions) — verify
  with `grep -c` first.
- The classifier does NOT fail on NEW_FN (exit code gates FP only) —
  read the summary text, don't trust exit codes for FN.
- A truncated-fixture probe differing from the full fixture = check-
  order sensitivity (see stall-playbook §2.2), not a tool bug.
