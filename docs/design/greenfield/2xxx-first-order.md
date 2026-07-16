# 2XXX-first build order — the master plan

GOAL (redefines the milestone gates): full parity on the 2XXX
diagnostic band — every code in 2000-2999, FP = 0 against the oracle
across the corpus and FN = 0 on the supported scope — BEFORE
investing in the other bands. Scope exclusions are exact
oracle-record identities under definition-of-done.md's out-of-scope
contract (host-resolution, jsdoc-semantics; TS2307 host misses are
the pinned exemplar): excluded records stay FN in the all-corpus
visibility metric and are not chased. The metric is `T0-2xxx` (T0
comparison restricted to codes 2000-2999); full-band T0 is tracked
but secondary until phase 9.

This doc re-sequences the m*-steps docs toward the band goal (they
remain the stage-level instructions; the impl-*.md companions carry
copy-level code). Phase numbers are content identities, not a
landing sequence: landing order is owned by
completion-convergence-plan.md §4 — including the recorded
phase-8-before-7 swap — and a phase starts only after the gate of
every phase that §4 lands before it is green.

## Why this order (the dependency analysis)

The 2XXX band is the checker's semantic surface. Working backward
from what each 2XXX family needs:

| 2XXX family (top codes by corpus frequency) | Needs |
|---|---|
| relation/assignability (2322, 2411, 2741, 2344, 2415, 2430) | types + relation engine + member resolution + declaration checking |
| member/name/access (2339, 2304, 2503, 2538, 2536, 2708) | binder scopes + member resolution + apparent types |
| call/construct/overload (2345, 2554, 2769, 2349, 2351) | signatures + resolveCall + inference + relations |
| grammar/declaration (2300, 2364, 2369, 2393, 2403) | binder merge engine + checker grammar region |
| class semantics (2415, 2417, 2420, 2445, 2446, 2337, 2340) | heritage resolution + member compare + relations |
| instantiation (2313, 2314, 2315, 2344, 2589) | type-reference resolution + instantiate + constraints |
| iteration/destructuring (2461, 2488, 2493, 2698, 2739) | iteration protocol + destructuring checking |
| flow/operators (2365, 2367, 2454, 2564, 2678) | flow graph + narrowing + operator checks + comparable relation |

Everything sits on: symbols (binder), which sit on the AST (parser),
which sits on tokens (scanner). Hence the classical order the user
of this plan expects — scanner → parser → binder → checker — is also
the dependency-true order. TWO deliberate deviations from a naive
reading:

1. **The parser must be COMPLETE and recovery-exact even though 1XXX
   codes are not the goal.** Reasons: (a) 2XXX spans/columns come
   from node positions; (b) tsc self-censors semantic checks around
   parse-error nodes (`containsParseError`) — 2XXX FN parity on
   malformed fixtures is impossible without tsc-exact recovery and
   per-node error flags. The first implementation's largest residual
   2XXX FN cluster (parser/ecmascript5, 244 files) is exactly this
   tax. Do not economize here.
2. **The emitter is NOT built.** No 2XXX diagnostic requires emit.
   The emit-adjacent diagnostic families are 4XXX (declaration-emit
   visibility) and the 6XXX suggestion band's emit-marking — both
   out of the first goal, both implementable later as checker-side
   rules (greenfield.md §6) without ever writing an emitter. The
   "emitter phase" of the classical pipeline is therefore replaced
   by phase 9's band expansion.

## Phase plan

| Phase | Content | Steps doc | Impl companion | Gate (all gates also require: ratchet non-regression, ledger check green) |
|---|---|---|---|---|
| 0 | infra: codegen (enums + NODE SCHEMA, [impl-nodes.md](impl-nodes.md)), harness, oracle, goldens, `--band 2xxx` comparator | m0-foundations-steps.md | impl-nodes.md §1-3 | oracle goldens full corpus; `xtask conformance --band 2xxx` runs (0%) |
| 1 | scanner, complete | m1-scanner-steps.md | [impl-scanner.md](impl-scanner.md) | token parity on corpus |
| 2 | parser + recovery, complete, emitting tsc-field-compatible nodes | m1-parser-steps.md | [impl-parser.md](impl-parser.md) + impl-nodes.md | syntactic T0 ≥ 99.5% AND ast-diff clean (impl-nodes §5) |
| 3 | binder: symbols/merge/scopes/flow graph + program layer/module resolution ([program-and-modules.md](program-and-modules.md) §1-2). UNLOCKS first 2XXX: 2300/2451/2440-family duplicates | m2-binder-steps.md | [impl-binder.md](impl-binder.md) | symbol audit + first 2XXX pins green |
| 4 | types core + relation engine (all 5 relations) | m3-types-relations-steps.md | [impl-checker-2xxx.md](impl-checker-2xxx.md) §2 | relation pin suite |
| 5 | checker spine: checker INIT (program-and-modules.md §3), resolution stack, symbol typing, type-from-nodes, member access, driver. UNLOCKS: 2304/2339/2314/2313/2749-family | m4-checker-skeleton-steps.md 5.0-5.4 | impl-checker-2xxx.md §1,§3-4 | T0-2xxx ≥ 25% |
| 6 | expressions/statements/declarations/classes/enums/modules/iteration. UNLOCKS: 2322/2403/2415-class family/2461-iteration family | m4-checker-skeleton-steps.md 5.5-5.8 | impl-checker-2xxx.md §5-7 | T0-2xxx ≥ 55% |
| 7 | calls with stubbed inference, then full inference. UNLOCKS: 2554/2349/2351 then 2345/2769/2344 | m4 5.7 + m6-inference-calls-steps.md | impl-checker-2xxx.md §8 | T0-2xxx ≥ 75% |
| 8 | flow narrowing + operators. UNLOCKS: 2365/2367/2454/2564/2678 + removes the narrowing-dependent 2322/2339 residue | m5-flow-steps.md | impl-checker-2xxx.md §9 | T0-2xxx ≥ 90% |
| 9 | 2XXX completion sweep: adjudicate 2XXX scope exclusions (exact A2 identities), mine the supported-scope band residue to zero, then expand bands (1xxx exact, 7xxx, 6xxx, suggestion, 4xxx) | README M8 loop | — | **all-corpus 2XXX FP = 0, supported-scope T0-2xxx = 100%** (exclusions pinned first by the A2 `2xxx` band-freeze record), then full-band ratchets |

Phase-gate percentages are calibration priors (from the first
implementation's trajectory), not physics; the hard requirements are
the 100% endpoint and monotone ratchets.

Stage 5.7 appears in two rows by design, not by conflict: phase 6's
steps-doc range (5.5-5.8) covers 5.7's *stubbed-inference* half
executed in checker-steps order, and phase 7 is the M6 swap that
makes those calls fully live. The 5.7-with-stub work therefore does
not wait on phase 6's gate percentage. The lib-loaded corpus also
re-based the priors: at 5.7a the measured T0-2xxx was 8.67% with the
M5/M7-owned code families (2454/2564/18050, 6133/6196) still fully
contained — the 25%/55% figures predate lib loading and overstate
what phases 5-6 can show before flow and the unused-band land.
(Clarified 2026-07-13 after the stage-5.7a external review.)

Note the resequencing vs the original milestone table: flow (old M5)
was moved AFTER calls/inference (old M6) on the planning-time bet
that the call/overload 2XXX family was the larger one and does not
depend on narrowing; the same note allowed swapping phases 7 and 8
back "if mid-build mining shows narrowing-blocked codes dominating
earlier" — both orders are dependency-legal. DECIDED 2026-07-16: the
swap-back is exercised. At the 5.9c baseline (52c47bbb; band FN
9,995) the phase-8 unlock family 2365/2367/2454/2564/2678 holds
4,357 FNs — 2454 alone 3,962 — against 477 for the phase-7 unlock
family 2554/2349/2351/2345/2769/2344, whose call half was already
crushed by 5.7's stubbed-inference calls; 2322 (1,382) and 2339
(558) hold further narrowing-dependent residue. M6 also gained the
speculation-transaction start gate (2026-07-14 external review).
Execution order is therefore phase 8 (M5 flow) then phase 7 (M6 full
inference), fixed by completion-convergence-plan.md §4 rows 7-8, the
execution-order authority. Phase numbers stay attached to their
content — the impl-checker-2xxx.md §8/§9 port tables are unchanged —
and the 75%/90% calibration priors attach to landing order (first of
the two, then both), not to phase numbers. §4 row 9 lands phase 9's
first half (scope adjudication, then supported-scope band residue to
zero) between M6 and M7; the band-expansion half is M7/M8
themselves.

## The band comparator (phase 0 addition)

Add to m0 stage 0.7: `cargo xtask conformance --band 2xxx` filters
BOTH sides' diagnostics to codes 2000-2999 before the T0 set compare,
and reports: band exact-match files, band FP/FN diag counts, top
band codes each side, per-fixture mismatch list. `ratchet.toml` gains
a `[t0-2xxx]` entry ratcheted from phase 3 onward. IMPORTANT
measurement rule: a fixture counts as band-matched only if the 2XXX
subsets agree — non-2XXX diffs are invisible to this metric by
design; that is the point of the phase plan, and the full-band T0
line below it keeps honesty about the rest.

## What "complete 2XXX" means concretely (the phase-9 checklist)

- `xtask conformance --band 2xxx` reports 0 FP corpus-wide and 0 FN
  on the supported scope, all matrix points included. Every 2XXX
  scope exclusion is an exact record identity adjudicated under
  definition-of-done.md's out-of-scope contract (host-resolution,
  jsdoc-semantics; TS2307 host misses are the pinned exemplar) and
  is pinned before the sweep closes by the A2 `2xxx` band-freeze
  record — the band's enumerated identity set inside `m8-scope.json`,
  anchored to its adjudication commit and shrinkable only through
  tombstoned `resolved` proofs (A1 accepted-match membership;
  duplicate T0 buckets additionally require the bucket to be
  multiplicity-complete), machine-verified while the global manifest
  stays draft until M7 close; excluded records stay FN in the
  all-corpus metric by design.
- Every 2XXX code the ORACLE ever emits on the corpus appears in the
  engine's ledger with its emitting function ported (the emission map
  in impl-checker-2xxx.md is the working inventory; phase 9 mines
  the residue).
- Every 2XXX code the ENGINE emits appears in the oracle's corpus
  output at the same positions (no invented checks).
- The invariant suites (idempotence, jobs, prefix, encodings, matrix)
  are green — band parity that depends on run order is not parity.
- Message ARGUMENTS are not yet compared (that is T2); code+position
  is the contract. Do not spend phase 1-9 effort on chain text beyond
  what the code+position work produces naturally.

## Working rules for the impl companions

The impl-*.md files contain two kinds of material, marked as such:

- **[COPY]** blocks: Rust that can be pasted as-is (module-level
  scaffolding, state machines, the algorithm spines). Paste, then
  fill only the `todo_port!()` holes each block declares, each hole
  citing its tsc anchor.
- **[PORT TABLE]** checklists: function-by-function port lists in
  dependency order — name, tsc anchor, what it emits (2XXX codes),
  verification fixture. For these, the tsc source IS the code; the
  agent transcribes arm by arm. Transcription rules: keep tsc's
  branch ORDER; keep early returns; every `Diagnostics.X` becomes
  `gen::X`; every `error(...)` position argument is ported literally
  (getErrorSpanForNode discipline); unknown helpers encountered
  mid-transcription go onto the port table as new rows, never
  inlined approximations.

`todo_port!()` is a real macro (define in phase 0):

```rust
macro_rules! todo_port {
    ($anchor:literal) => {
        unimplemented!(concat!("port pending: ", $anchor))
    };
}
```

`xtask ledger check` counts `todo_port!` sites; a phase gate requires
zero remaining holes for that phase's modules.
