# Greenfield execution guide (READ FIRST, FOLLOW EXACTLY)

This directory turns the greenfield DESIGN set into an executable
build plan. The five parent documents are the design authority:

- [../greenfield.md](../greenfield.md) — architecture, crate layout,
  harness, milestone plan (§12 is the master schedule).
- [../core-interfaces.md](../core-interfaces.md) — the data contracts
  (Node/Symbol/Type/Signature/FlowNode/InferenceInfo/Diagnostic/
  CompilerOptions) with the must-match-vs-may-differ table.
- [../syntax-and-binder.md](../syntax-and-binder.md) — scanner, parser,
  recovery, binder algorithms.
- [../checker-foundations.md](../checker-foundations.md) — lazy
  resolution, check ordering, contextual typing, construction,
  widening, instantiation, member access.
- [../checker-key-functions.md](../checker-key-functions.md) — the
  relation, inference, overload, and flow algorithms.

The steps docs here SEQUENCE those designs into stages a low-capability
agent can implement one commit at a time. They do not restate the
algorithm skeletons — each stage names the parent-doc section and the
tsc anchor to port from. If a steps doc and a parent doc disagree, the
parent doc plus the tsc source win; file a doc fix.

**Completion authority:**
[definition-of-done.md](definition-of-done.md) — the normative one
page for WHAT "done" means (version pin, tiers, exclusions,
go/no-go checkpoints). It wins over every other doc on that
question.

**Completion convergence plan:**
[completion-convergence-plan.md](completion-convergence-plan.md) — the
ordered remediation and delivery plan that makes progress set-monotone,
repairs exact scope/T4/evidence gates, and sequences M4 close through the
final M9 completion command. It owns execution order and acceptance
artifacts; the definition of done still owns the end state.

**Scheduling authority:**
[2xxx-first-order.md](2xxx-first-order.md) — the build is ordered
around one goal: complete 2XXX-band parity first (phases 0-9,
re-sequencing the milestone table below; no emitter is built). The
impl companions carry copy-level code and port tables:
[impl-nodes.md](impl-nodes.md) (the tsc-field-compatible Node
contract: generated node structs + for_each_child from
forEachChildTable, line map, externalModuleIndicator, the AST tree
differ), [impl-scanner.md](impl-scanner.md),
[impl-parser.md](impl-parser.md),
[impl-binder.md](impl-binder.md),
[impl-checker-2xxx.md](impl-checker-2xxx.md) (which also holds the
2XXX emission-map inventory that defines "complete"), backed by
[2xxx-emitter-inventory.md](2xxx-emitter-inventory.md) — the
generated, complete checklist of all 246 tsc functions that emit
band codes, each with its Rust module home — and
[2xxx-emitter-descriptions.md](2xxx-emitter-descriptions.md), the
hand-audited companion describing what each of those functions
implements and when each code fires.
[program-and-modules.md](program-and-modules.md) closes the three
architecture holes outside the classic four phases: the Program/host
layer, module resolution, and checker initialization (globals
merging, getGlobalType environment).
[lsp-and-incremental.md](lsp-and-incremental.md) records tsc's
incremental architecture (syntaxCursor-fed parser, disposable
checker), the rules phases 0-9 must follow to keep the LSP door
open (reserved cursor parameter, per-parse NodeIds, no
node-id-keyed cross-program caches), and the future L-track. Work a
phase by reading: this README → 2xxx-first-order.md → the phase's
steps doc → its impl companion → the cited parent-doc sections.

**Non-2XXX companion:**
[non-2xxx-first-order.md](non-2xxx-first-order.md) — the family map
and scheduling skeleton for everything outside codes 2000-2999
(2xxx-first-order.md leaves those diffs invisible by design). It
decomposes the non-2XXX bands into implementation-owner families
keyed by (code, pass), records their measured baselines, and defines
the per-family acceptance that C4/M7 stage gates and the M8 residual
snapshot consume. The convergence plan's A5 slice turns it into a
machine map + rollup.

This is a FROM-SCRATCH build (workspace `tsrs2/`). Nothing in the
existing `src/` is consulted; the only implementation references are
the vendored tsc and these documents.

## The prime directive

PORT, never improvise. The tsc source is the specification and the
oracle binary is the ground truth:

1. Before writing a function, read its cited tsc source. Anchors are
   given as `_tsc.js` line numbers at the 6.0.3 pin — they drift on
   re-vendor, so ALWAYS re-locate with
   `grep -n "function <name>(" vendor/typescript-6.0.3/lib/_tsc.js`.
2. Never answer a semantics question from memory of TypeScript. Write
   a micro-fixture, run the oracle, read the answer. (Proof this rule
   is load-bearing: tsc 6.0 renumbered `TypeFlags` — `StringLiteral`
   is 1024, not the 5.x-era 128 that any model memory will claim.)
3. Expected strings in tests come from an oracle probe, never from
   your expectation.
4. Every ported function gets a ledger comment at port time
   (greenfield §8): `tsc-port` name, `tsc-span`, `tsc-hash`. This is
   not optional cleanup; `xtask ledger check` (M0) enforces it.
5. A ledger or code comment claiming an arm is unreachable/DEAD in
   the current milestone must carry a constructibility argument (why
   no in-scope input can reach it) or a pin that would catch the arm
   going live. Proof this rule is load-bearing: the M3 review
   (2026-07-11) traced four wrong verdicts to false unreachability
   claims ("Instantiable is unconstructible" — template literals are
   Instantiable; "non-unit source properties never discriminate" —
   fresh literal members do), each silently converting a
   should-be-Unsupported into a wrong verdict.

## Milestones and their steps docs

No milestone starts before the previous gate is green
(greenfield §12). Within a milestone, stages are ordered; each stage
is one commit.

| Milestone | Steps doc | Acceptance gate |
|---|---|---|
| M0 harness + codegen | [m0-foundations-steps.md](m0-foundations-steps.md) | oracle goldens for full corpus; empty-engine plumbing green |
| M1 scanner | [m1-scanner-steps.md](m1-scanner-steps.md) | token-stream parity vs oracle scanner on the corpus |
| M1 parser | [m1-parser-steps.md](m1-parser-steps.md) | syntactic-diagnostic T0 parity ≥ 99.5%; prefix-determinism green |
| M2 binder | [m2-binder-steps.md](m2-binder-steps.md) | crash-free bind of corpus; symbol spot-audit vs oracle on 50 fixtures |
| M3 types + relations | [m3-types-relations-steps.md](m3-types-relations-steps.md) | ~200 oracle-probed relation pins green |
| M4 checker skeleton | [m4-checker-skeleton-steps.md](m4-checker-skeleton-steps.md) | T0 ≥ 35% |
| M5 flow narrowing | [m5-flow-steps.md](m5-flow-steps.md) | T0 ≥ 50%; idempotence/jobs invariants green |
| M6 inference + overloads | [m6-inference-calls-steps.md](m6-inference-calls-steps.md) | T0 ≥ 58% |
| M7 unused/grammar/suggestion | [m7-tail-steps.md](m7-tail-steps.md) | T0 ≥ 63%; T1 measured and ratcheted |
| M8 long tail | [m8-readiness.md](m8-readiness.md) + the mining loop below | supported-scope T2/T3 activated; all-corpus FP=0 |
| M9 fuzzer hardening + coverage | greenfield §7.7 + §8 | the M8-entry differential loop reaches new-signature rate < 1/night |

The T0 percentages are calibration points from the first
implementation's history, not promises; the gate is "meets or beats,
and the ratchet never regresses."

## The loop (per stage; never deviate)

1. Read the stage's parent-doc section and its tsc anchor lines.
2. Implement EXACTLY the stage's scope. Do not refactor neighboring
   code, do not fix unrelated issues (note them in
   `docs/NOTES-<date>.md`).
3. `cargo build && cargo test` — green, no warnings introduced.
4. Run the stage's OWN verification command; compare to its
   "expect:" line.
5. Add/refresh ledger comments for every function the stage ported.
6. Commit: `m<N> <stage>: <what>` (e.g. `m1 3.2: Pratt loop with
   reScanGreaterToken per iteration`).

From M4 onward, additionally run the conformance gate
(`cargo xtask conformance`) after each stage: the ratchet must not
regress, and any NEW one-sided diagnostic against a previously-matching
fixture must be triaged before commit.

## Stop conditions (write NOTES and halt the milestone)

- The tsc source for a cited anchor does not match what the steps doc
  describes (re-vendor drift or doc error) — record both.
- A stage needs a data-model field the core-interfaces contract does
  not have — that is a design change, not an implementation detail.
- An acceptance gate is missed by more than 2 points after the last
  stage — do not "borrow" work from a later milestone to close it.
- You are about to hand-write any value that M0's codegen should
  produce (a flag bit, a SyntaxKind number, a message text).

## M8 in one paragraph

After M7 the build enters the mining loop this playbook already runs:
full conformance snapshot → top one-sided codes → owner → smallest
probe → port the missing tsc branch → gate. The comparison tiers climb
from T0 to T2/T3 by turning on stricter comparators fixture-family by
fixture-family in `ratchet.toml`. The classifier, snapshot procedure,
and triage discipline are the same as the parent repo's
EXECUTION-GUIDE; only the engine under test differs.

M8 has an executable entry contract: see
[m8-readiness.md](m8-readiness.md) and run `cargo xtask m8 readiness`.
The fixed corpus is always reported whole; only exact reviewed
host-resolution/JSDoc/emit-dependent oracle diagnostics leave the
supported-scope T1-T4 denominator. The minimal differential fuzzer
is running before M8 begins; M9 hardens it rather than introducing it.

## Conventions

- Workspace paths are relative to `tsrs2/` (greenfield §2 layout).
- Rust: `#[repr(u16)]`/bitflags types come ONLY from `xtask codegen`
  output; iteration over any symbol/member table uses ordered maps
  (IndexMap) or sorted keys — `cargo clippy` denies raw `HashMap`
  iteration in checker crates (M0 sets up the lint).
- One tsc function = one Rust function, tsc's name in snake_case
  (greenfield §5). Coalescing "for elegance" is a stop condition.
- Diagnostics are emitted ONLY via `&'static DiagnosticMessage`
  references from the generated table.
