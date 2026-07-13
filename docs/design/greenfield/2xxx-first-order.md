# 2XXX-first build order — the master plan

GOAL (redefines the milestone gates): full parity on the 2XXX
diagnostic band — every code in 2000-2999, FP = 0 and FN = 0 against
the oracle across the corpus — BEFORE investing in the other bands.
The metric is `T0-2xxx` (T0 comparison restricted to codes
2000-2999); full-band T0 is tracked but secondary until phase 9.

This doc supersedes the milestone ORDER of README.md for scheduling
(the m*-steps docs remain the stage-level instructions; this doc
re-sequences them, tightens their scopes toward 2XXX, and adds the
impl-*.md companions that carry copy-level code). Phase N+1 never
starts before phase N's gate.

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
| 9 | 2XXX completion sweep: mine the band residue to zero, then expand bands (1xxx exact, 7xxx, 6xxx, suggestion, 4xxx) | README M8 loop | — | **T0-2xxx = 100%**, then full-band ratchets |

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
moves AFTER calls/inference (old M6) because the call/overload 2XXX
family is larger than the flow-dependent one and does not depend on
narrowing; narrowing DOES improve relation/member codes, so it lands
last before the sweep, where its effect is purely additive. If
mid-build mining shows narrowing-blocked codes dominating earlier,
swapping phases 7 and 8 back is allowed — both orders are
dependency-legal; record the decision in NOTES.

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

- `xtask conformance --band 2xxx` reports 0 FP / 0 FN diagnostics
  corpus-wide, all matrix points included.
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
