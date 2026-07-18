# Candidate speculation boundary steps

Companion to `type-checking-2xxx-execution-plan.md` section B
(candidate speculation boundary + diagnostic/cache transaction
primitive), `type-checking-2xxx-roadmap.md` Phase 3, and
`candidate-call-resolution.md`. Every stage here is byte-identical:
the scaffold ends where behavior would begin (Stage 4 is a stop
boundary, not a step).

Key design decision this workstream implements: the checker ALREADY
has a speculation mechanism — the flow resolver's `quiet` machinery
(`src/checker/mod.rs`, the `FlowResolve` struct's `quiet` field near
line 140). Under
`quiet > 0`: `report_once_node` / `report_once_sym` /
`report_used_before_assigned` do not consume their once-guards
(`mod.rs` near lines 5423-5447), `expr_type_cache` is not populated
(`exprs.rs` near line 315), and `fact_for` sees only frames pushed
since the innermost scaffold (`flow/mod.rs` near line 27, via
`scaffold_base`). Call sites additionally truncate `diags` back to the
pre-run length. The candidate boundary is built by EXTRACTING and
EXTENDING this mechanism — never by adding a second parallel
speculation system.

This workstream is order-independent of the relation-kind facade
(`relation-kind-facade-steps.md`); either may land first.

## Files this workstream may touch

- `src/checker/mod.rs` (the new primitive + its doc comment)
- `src/checker/flow/resolver.rs` (adopting the primitive: one site in
  Stage 1, the remaining three in optional Stage 1b)
- `src/checker/calls.rs` (trial record fields, Stage 3)
- `docs/design/NOTES-<date>-candidate-boundary.md` (Stage 2 audit)

Explicitly out of scope: `src/checker/classes.rs`. Its
`check_expr_for_usage_only` (near line 24) is a quiet variant that
deliberately does NOT push `scaffold_base`; unifying it changes
`fact_for` visibility and needs its own audit. Record it in the notes,
do not touch it.

## Stage 0: Baseline and canary probes [P]

```sh
git status                      # must be clean; note HEAD
cargo build --release
cargo test --release            # expect: first suite 98 passed (or current baseline)
./verify.sh golden-save
```

Probe the top call-family FP fixtures and save the outputs; every
stage below must leave them byte-identical:

```sh
python3 scripts/probe.py ts-tests/tests/cases/conformance/types/typeParameters/typeArgumentLists/functionConstraintSatisfaction.ts > /tmp/canary_2345.txt
python3 scripts/probe.py ts-tests/tests/cases/conformance/expressions/functionCalls/callWithMissingVoid.ts > /tmp/canary_2554.txt
python3 scripts/probe.py ts-tests/tests/cases/conformance/es6/templates/taggedTemplateStringsWithOverloadResolution3.ts > /tmp/canary_2769.txt
```

## Stage 1: Extract the `speculate()` primitive [M]

In `src/checker/mod.rs` (`impl Checker`), extract the exact inline
pattern used by the resolver scaffold at
`src/checker/flow/resolver.rs` near lines 947-958:

```rust
/// Run checker code exploratorily: diagnostics emitted inside are
/// rolled back, once-per-node/symbol report guards are not consumed,
/// `expr_type_cache` is not populated, and `fact_for` sees only fact
/// frames pushed inside this scope (see `FlowResolve::quiet`).
/// This is the transaction primitive for candidate probing; extend
/// its coverage here, never with a parallel mechanism.
pub(crate) fn speculate<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
    self.fresolve.scaffold_base.push(self.flow.facts.len());
    self.fresolve.quiet += 1;
    let dlen = self.diags.len();
    let r = f(self);
    self.diags.truncate(dlen);
    self.fresolve.quiet -= 1;
    self.fresolve.scaffold_base.pop();
    r
}
```

Adopt it ONLY at the resolver site near `resolver.rs:947` (the
back-edge assignment scaffold, the `None =>` arm), keeping its scope
save/restore outside the closure. Exact conversion:

```rust
// before
let saved_scope = self.current_scope;
self.current_scope = scope;
self.fresolve.scaffold_base.push(self.flow.facts.len());
self.fresolve.quiet += 1;
let dlen = self.diags.len();
let t = self.check_expr(e, ctx);
self.diags.truncate(dlen);
self.fresolve.quiet -= 1;
self.fresolve.scaffold_base.pop();
self.current_scope = saved_scope;
FlowRes::Ty(t)

// after
let saved_scope = self.current_scope;
self.current_scope = scope;
let t = self.speculate(|c| c.check_expr(e, ctx));
self.current_scope = saved_scope;
FlowRes::Ty(t)
```

The other three resolver `quiet` sites (pushes near lines 544, 583,
656: condition narrowing, switch-clause narrowing, assertion
narrowing) share the SAME seven-line shape — push / `quiet += 1` /
`dlen` / run a `narrowed(..)` closure / truncate / `quiet -= 1` / pop,
with the same outer `current_scope` save/restore. Stage 1 still
converts only the one site above so the commit stays minimal and
reviewable; the other three belong to Stage 1b below, never to this
commit as silent extras.

Verify:

```sh
cargo build --release 2>&1 | grep -E "^error" -A5   # expect: nothing
cargo test --release 2>&1 | grep "test result:"      # expect: all ok, same count
./verify.sh golden-check                             # expect: 0 NEW_FP / 0 NEW_FN, zero adds/removes
```

Commit: `candidate-boundary 1: extract speculate() primitive`.

## Stage 1b (optional): adopt speculate() at the remaining resolver sites [M]

Only after Stage 1's gate is green. Convert the three remaining
resolver sites (`resolver.rs`, `scaffold_base` pushes near lines 544,
583, 656) the same way, all three in one commit. Template — the
`narrowed` closure body stays verbatim; only the seven scaffold lines
are replaced:

```rust
// before
self.fresolve.scaffold_base.push(self.flow.facts.len());
self.fresolve.quiet += 1;
let dlen = self.diags.len();
let out = self.narrowed(|c| { /* body — unchanged */ });
self.diags.truncate(dlen);
self.fresolve.quiet -= 1;
self.fresolve.scaffold_base.pop();

// after
let out = self.speculate(|c| c.narrowed(|c| { /* body — unchanged */ }));
```

The `current_scope` save/restore around each site stays outside,
unchanged. Afterwards
`grep -n "quiet += 1" src/checker/flow/resolver.rs` must return
nothing (`speculate()` in `mod.rs` and the deliberately-different
`check_expr_for_usage_only` in `classes.rs` are the only writers
left).

Verify with the same three commands as Stage 1, plus the canary
diffs from Stage 0.

Commit: `candidate-boundary 1b: adopt speculate() at remaining resolver scaffolds`.

## Stage 2: State-write audit [P]

No code change. Produce
`docs/design/NOTES-<date>-candidate-boundary.md` with one row per
checker state field a candidate probe could write. A field is
boundary-relevant when a probe can write it AND the written value
depends on the candidate (its mapper or contextual types).

Starting table (verified anchors; complete and correct the "audit"
rows by grepping each field's writers):

| Field | Speculation coverage today | Verdict |
|---|---|---|
| `diags` | truncated by `speculate()` call sites | covered |
| `reported_once_node` / `reported_once_sym` | not consumed under `quiet` (mod.rs ~5423) | covered |
| `reported_2454` | gated under `quiet` (mod.rs ~5440) | covered |
| `expr_type_cache` | not populated under `quiet` (exprs.rs ~315) | covered |
| fact frames | `scaffold_base` scoping (flow/mod.rs ~27) | covered |
| `node_type_cache` | none known | audit |
| `param_ctx_types` | none known | audit |
| `checked_decls` | none known | audit |
| `sym_type`, `sig_ret_cache` | none known | audit |
| `fresh_obj_props` | none known | audit |
| `used_symbols` / `assigned_symbols` | none (access.rs ~478, operators.rs) — probe marks symbols used → unused-family (6133) FN risk | audit |
| flow `memo` / `reach_lazy` / `reach_structural` | none known | audit |
| `relation_cache` / `comparable_cache` | intentionally exempt: top-level-only writes, candidate-independent (execution plan §B) | exempt |

Each "audit" row must end as either "covered" (with the gating site),
"must stage" (with the writer sites a candidate probe reaches), or
"unreachable from candidate probes" (with the reasoning). This table
is the acceptance list for the boundary; the execution plan's
"Before Call Behavior" checklist refers to it.

## Stage 3: Trial record extension (recording only) [M]

Extend `CallCandidateTrial` (`src/checker/calls.rs`, near line 14 —
it already derives `Clone`; both new field types are `Clone`) with
record-only fields, populated inside `call_candidate_trial` (near
line 1204) from values it ALREADY computes:

```rust
#[derive(Clone)]
struct CallCandidateTrial {
    // ...existing fields...
    /// instantiated parameter type per argument slot the trial checked;
    /// `None` = slot skipped (context-sensitive arrow with no
    /// precomputed type). Shorter than the arg list when the trial
    /// failed early: it records exactly the slots checked.
    param_slot_types: Vec<Option<TypeId>>,
    /// explicit type arguments were applied to this candidate
    /// (`type_args` present AND its arity matched `s.type_params`)
    used_explicit_type_args: bool,
}
```

Mechanical edits inside `call_candidate_trial` (every value is
already computed; nothing new may be evaluated):

1. Hoist the explicit-type-args condition into a named local where
   the mapper is chosen — same condition, evaluated once
   (`type_args` is `Option<&[TypeNode]>`, which is `Copy`):

```rust
// before
let mapper: HashMap<SymbolId, TypeId> =
    if let Some(targs) = type_args.filter(|targs| targs.len() == s.type_params.len()) {

// after
let explicit_targs = type_args.filter(|targs| targs.len() == s.type_params.len());
let used_explicit_type_args = explicit_targs.is_some();
let mapper: HashMap<SymbolId, TypeId> =
    if let Some(targs) = explicit_targs {
```

2. Declare `let mut param_slot_types: Vec<Option<TypeId>> = Vec::new();`
   immediately after the `mapper` binding.

3. Spread path (the `if let Some(plan) = &expanded_args` slot loop):
   right after the per-slot header
   `let pt = self.instantiate_type(pt0, &mapper);` add
   `param_slot_types.push(Some(pt));`. The header `pt` is computed
   once per slot for every arm including `ArraySpread` (which then
   recomputes per-param types internally — do NOT record those), and
   including arrow expressions (their skip happens after the header).
   That asymmetry with the non-spread path mirrors the code; do not
   "fix" it.

4. Non-spread path (`for (i, at) in arg_types.iter().enumerate()`):
   the arrow skip becomes
   `let Some(at) = at else { param_slot_types.push(None); continue; };`
   (keep the trailing comment), and after its
   `let pt = self.instantiate_type(pt0, &mapper);` add
   `param_slot_types.push(Some(pt));`.

5. Fill the new fields at every construction literal. The compiler
   enumerates them for you (each missing-field error is the
   checklist); expected sites and values at current HEAD:

| Literal (near line) | `param_slot_types` | `used_explicit_type_args` |
|---|---|---|
| `calls.rs:1236` arity-fail early return (before the mapper exists) | `Vec::new()` | `false` (nothing applied yet) |
| `calls.rs:1289`, `1302`, `1319`, `1332`, `1348` spread-path argument failures | `param_slot_types` (move; partial vec is correct) | `used_explicit_type_args` |
| `calls.rs:1371` non-spread argument failure | `param_slot_types` (move) | `used_explicit_type_args` |
| `calls.rs:1383` success literal at function end | `param_slot_types` | `used_explicit_type_args` |
| `calls.rs:949` `first_trial = Some(CallCandidateTrial { .. })` field-by-field copy outside the function | `trial.param_slot_types.clone()` | `trial.used_explicit_type_args` |

Rules for this stage:

- populate the fields only from values already computed for the
  existing verdicts; no new `check_expr`, no new instantiation, no new
  relation query (the only allowed restructure is the hoisted
  `explicit_targs` local in edit 1, which re-evaluates nothing);
- no read sites: nothing may branch on the new fields yet.

Verify (same three commands as Stage 1, plus canary diffs):

```sh
python3 scripts/probe.py ts-tests/tests/cases/conformance/types/typeParameters/typeArgumentLists/functionConstraintSatisfaction.ts | diff - /tmp/canary_2345.txt   # expect: no output
python3 scripts/probe.py ts-tests/tests/cases/conformance/expressions/functionCalls/callWithMissingVoid.ts | diff - /tmp/canary_2554.txt                          # expect: no output
python3 scripts/probe.py ts-tests/tests/cases/conformance/es6/templates/taggedTemplateStringsWithOverloadResolution3.ts | diff - /tmp/canary_2769.txt              # expect: no output
```

Commit: `candidate-boundary 2: record-only trial extension`.

## Stage 4: STOP — scaffold boundary

The scaffold ends here. Everything further is behavior:

- wrapping per-candidate argument checks in `speculate()` changes
  which diagnostics reach `diags` and when caches fill;
- candidate-local contextual argument types change inferred types;
- failure-diagnostic selection changes which candidate reports.

Those are execution plan section D steps 1-6 and require: a fresh
mining ledger, the "Before Call Behavior" readiness checklist, and
the Stage 2 audit table fully resolved (no "audit" rows left). Write
the handoff note and stop.

Final gate for the scaffold series:

```sh
cargo fmt
cargo build --release
cargo test --release
./verify.sh golden-check      # expect: 0 / 0, zero adds/removes
./verify.sh golden-save
```

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Golden movement after Stage 1/1b | The conversion moved the `current_scope` save/restore inside the closure, changed the push/quiet/truncate order, or touched a site not listed in the stage | Revert that adoption; the primitive must replace the listed seven-line pattern verbatim, scope handling stays outside |
| Flow test failure after Stage 1 | `scaffold_base` push/pop imbalance | The primitive owns push/pop around the closure; no early return may skip them |
| Golden movement after Stage 3 | Recording computed something new (extra `check_expr`/instantiation) instead of reusing existing values | Recording must be pure bookkeeping of already-computed values |
| Canary diff but golden-check clean | Non-diagnostic output change (ordering, display) | Treat as movement; diagnose before continuing |
| Unused-family (6133) movement at any stage | A probe path started marking `used_symbols` differently | This is a Stage 2 "must stage" field surfacing early; stop and record |
