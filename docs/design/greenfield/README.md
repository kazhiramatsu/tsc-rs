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
ordered delivery plan from M4 close through M9. It owns sequence,
dependencies, and acceptance gates; the definition of done still owns
the end state. Its supporting contracts are
[measurement-integrity.md](measurement-integrity.md) for A1/A2/A3/A5
schemas, anchors, and identities, and
[evidence-and-steady-state.md](evidence-and-steady-state.md) for B1-B4
producers and M9. Read the plan first; open a support contract only when
implementing or reviewing that mechanism.

**M8 execution contract:**
[m8-execution-and-close.md](m8-execution-and-close.md) — the fixed M8 entry
baseline, exact D2 trace/static owner-cluster method, per-slice evidence,
global T0-T4/recovery order, M8 close record, and current M9 handoff. The
readiness page is retained as the historical gate that opened M8.

**M9 execution contract:**
[m9-execution-and-close.md](m9-execution-and-close.md) — the M9 preflight,
generator-domain and resource audit, bounded streaming producer,
class/witness/recurrence registry, diagnostic-D2/pipeline-native owner-task
closure, attested burn-in, fingerprint freeze, 14-window qualification, and
final close. It is paused after the landed M9.1b true-replay foundation.

**H0 filesystem-hosted no-emit contract:**
[noemit-cli.md](noemit-cli.md) — the completed frozen profile for the host
boundary: owned program/session seams, exact module and package resolution,
filesystem/config loading, no-emit diagnostics, rendering, and exit behavior.

**H1 JavaScript emit contract:**
[h1-emit.md](h1-emit.md) — H1.0a now has a generated report-only owner-graph
draft plus a frozen bootstrap profile, callback-level TypeScript oracle, and
current-Rust omission baseline, plus the complete content-addressed upstream
transpile source universe in additive suite pin v2, its exact 37-row
`TranspileRunner` classification, and the exact 38-file FourSlash batch-emit
witness projection in additive suite pin v3, plus the complete 5,908-file
conformance source tree in additive suite pin v4 and its exact
5,907-fixture/7,697-case/46,182-observation runner expansion, plus exact
effective-option dispositions for all 7,697 cases. All remain `not-run`, and
zero enter the bootstrap profile. The existing compiler expansion now also
has exact dispositions for all 7,276 rows: seven match the required
target/module pair, two pass effective-option preflight into source analysis,
and one remains a bootstrap candidate; all execution and baseline-comparison
states stay `not-run`/zero. The project runner now has exact root selection and
effective-option dispositions for all 632 CommonJS/AMD rows as well. Every
row has both a target and module blocker, admissions are zero, and execution
and baseline-comparison states stay `not-run`/zero.
The 38 projected FourSlash emit witnesses now have exact route/profile
dispositions too: their 47 selected calls are targeted Language Service
`Program.emit(sourceFile)` requests, all 38 use `target=ES2025` and a
non-`Preserve` module state, and therefore none is promoted as an H1
whole-Program control. Every row remains deferred/`not-run`, with zero
reference baselines compared.
The bounded, tsc-architected
JavaScript emitter preserves the H0 `--noEmit` entry without
initializing the emitter and ports the exact resolver/transform/printer/output
spine. Owner-disposition review is the remaining H1.0a inventory item; runtime
implementation is unblocked by the completed L0 persistent-source and L1
incremental-parser proof, while build/watch and LSP remain separate
compatibility tracks.

**Compiler compatibility residual:**
[compiler-compatibility-residual.md](compiler-compatibility-residual.md) — the
audited current implementation boundary, L0/L1 prerequisites, H1 blocking
dependency packages, exact built-in transformer/resolver expansion map, and
the work remaining after H1 for declarations, maps, build/watch, public APIs,
and the later L-track. It is a cross-track inventory, not an expansion of H1
or the normative M8/M9 definition of done.

**Complete JSDoc subsystem:**
[m8-jsdoc-ast-materialization.md](m8-jsdoc-ast-materialization.md) — the
landed TypeScript 6.0.3 scanner/parser/arena/binder/checker port, its
performance rules, and the distinction between subsystem completion and its
subsequently completed formal A1/A3 corpus gates.

**Terminal residue protocol:**
[terminal-residue-protocol.md](terminal-residue-protocol.md) — the
last-mile method used after a supported FN sweep becomes a small,
heterogeneous tail. It classifies exact rows by producer, verdict,
renderer, publication, and grading layers; uses the landed attached JSDoc
arena rather than semantic trivia rescans; and requires target/full
identity-diff evidence before close.

**Band strategy (2XXX first):**
[2xxx-first-order.md](2xxx-first-order.md) — the build is ordered
around one goal: complete 2XXX-band parity first (phases 0-9,
re-sequencing the milestone table below; no emitter is built).
Milestone landing order — including the recorded phase-7/8 swap (M5
flow before M6 full inference) — is owned by the convergence plan's
§4 table; this doc owns the band goal, phase content, and band gates.
The impl companions carry copy-level code and port tables:
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
[lsp-and-incremental.md](lsp-and-incremental.md) records the audited tsc
snapshot, incremental-parser, DocumentRegistry, bind, old-Program, and
resolution-reuse architecture; the current Rust ownership/ID/cache gaps; and
the required L0/L1-before-H1 landing order. Full Language Service, tsserver,
and LSP products remain later L2-L5 tracks. This is not an active M8 plan.
Work a phase by reading: this README → 2xxx-first-order.md → the phase's steps
doc → its impl companion → the cited parent-doc sections.

**Non-2XXX companion:**
[non-2xxx-first-order.md](non-2xxx-first-order.md) — the family map
and scheduling skeleton for everything outside codes 2000-2999
(2xxx-first-order.md leaves those diffs invisible by design). It
decomposes the non-2XXX bands into implementation-owner families
keyed by (code, pass), records their measured baselines, and defines
the per-family acceptance that C4/M7 stage gates and the M8 residual
snapshot consume. The convergence plan's A5 slice turns it into a
machine map + rollup.
[m7-band-and-owner-strategy.md](m7-band-and-owner-strategy.md) adapts
the successful 2XXX survey/mining method to those A5 virtual bands:
mandatory pre-implementation owner reconnaissance, immutable slice
evidence, and the concrete 8.1a-g checker-grammar producer split.

This implementation began as a FROM-SCRATCH build, isolated from the former
v1 `src/`. It now occupies the repository-root virtual Cargo workspace:
`Cargo.toml` owns the members under `crates/`, Rust sources live in each
`crates/*/src`, and there is no top-level `src/`. The implementation
references remain the vendored tsc and these documents; v1 is retained only
at tag `v1-final`.

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
| M8 diagnostics close | [m8-readiness.md](m8-readiness.md) + [M8 execution](m8-execution-and-close.md) | supported-scope T0-T3 and T4 complete; escapes zero; all-corpus FP=0 |
| M9 differential-fuzzer steady state — paused after M9.1b | [M9 execution](m9-execution-and-close.md) + [evidence contract](evidence-and-steady-state.md#31-m9-steady-state) | preflight/domain/owner burn-in green, then `fuzz steady-state --require-ready`: 14 frozen-fingerprint 100,000-case windows, rate < 1 new class/window, no untriaged incident or unresolved owner task |
| H0 filesystem-hosted `--noEmit` — complete | [H0 execution](noemit-cli.md) | exact closure of the 241 host-resolution identities, MemoryHost/FsHost equivalence, config/CLI/output parity, embedded libraries, no emitted files |
| L0/L1 persistent source + incremental parser — complete and performance-qualified | [persistent Program design](lsp-and-incremental.md) | shared text/position snapshots, domain-scoped identity leases, generated relocation, non-contiguous ownership, owned bind/Program snapshots, immutable incremental parse/rebind, exact fresh equivalence, reclamation stress, and approved large-edit evidence |
| H1 JavaScript emit — H1.0a profile/oracle/Rust omissions/upstream source evidence frozen, conformance/compiler/project classification complete, inventory closure active | [H1 execution](h1-emit.md) | report-only active-root graph, exact callback oracle, generated Rust omission baseline, complete transpile inputs and exact 37-row runner classification, 38-file inventory-only FourSlash emit projection, exact 7,697-row conformance expansion/classification, exact 7,276-row compiler classification, and exact 632-row project classification first; then FourSlash/owner-disposition closure, zero-cost H0 `--noEmit` route, byte-identical bounded JavaScript output, and resource qualification |

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

## M8 close and M9 handoff

M8 applied the
[execution contract](m8-execution-and-close.md) to the exact frozen entry
residual. Each mismatch followed: exact residual identity → exact D2 emitter
→ diagnostic trace → non-emitting sibling difference → static dependency
closure/SCC → Rust boundary → one dependency-closed slice. A moving top-code
list and a printed function name were never slice identities.

The accepted close state has supported T0, T1, T2, and T3 at
48,783 / 48,783 each, supported T4 at 7,691 / 7,691 cases, all-corpus
`FP=0`, and zero escapes. Exact T1-T3 accepted sets are active. A3's
schema-3 T4 state pins the genuine vendored formatter output and preserves
present-but-empty `relatedInformation` in a formatter-only sparse sidecar
without changing the structured oracle records or diagnostic identities.

The historical [readiness contract](m8-readiness.md) remains the reproducible
entry gate. The fresh close report confirms completion rows 1-10 green,
row 11 `m9-steady-state` pending, and `STAGE=M8`. M9 steady state remains the
sole pending completion row, but its execution is paused after M9.1b while
the independent H0 productization track runs. The fixed corpus continues to
be reported whole, while only exact reviewed A2 identities leave the
supported denominator. All historical `jsdoc-semantics` exclusions have
returned through tombstones.

M9 follows the dedicated
[execution contract](m9-execution-and-close.md): first correct and audit the
M8 entry fuzzer, then freeze a measured generator domain, run
non-qualifying burn-in, and close every owner task. Diagnostic tasks use
exact A5/2XXX and D2 owner slices; terminal/parser/binder/pure-T4 tasks use
their exact pipeline-native owner. Only then does M9 freeze the producer
fingerprint and start the 14 qualifying UTC windows. The M8
32-case/eight-template smoke earns no window credit.

H0 follows its separate
[filesystem-hosted no-emit contract](noemit-cli.md). Its entry inventory is
the 241 remaining exact host-resolution identities, not a new diagnostic
band. It first freezes those identities in
`ratchets/host-resolution.v1.json` with exact vendored request chains, owner
declarations, positive canaries, and reviewed typed controls. The shared
host/session and resolution-table seam follows, then package/module owners
close under `MemoryCompilerHost` before the same implementation connects to
the filesystem, tsconfig, and CLI. The recorded resource observations remain
pre-H0 references; `ratchets/h0-qualification.v1.json` freezes H0.6's final
CLI/local-gate profiles and budgets.

## Conventions

- Workspace paths are relative to the repository root (greenfield §2 layout).
- Rust: `#[repr(u16)]`/bitflags types come ONLY from `xtask codegen`
  output; iteration over any symbol/member table uses ordered maps
  (IndexMap) or sorted keys — `cargo clippy` denies raw `HashMap`
  iteration in checker crates (M0 sets up the lint).
- One exact tsc declaration identity = one Rust ported function, tsc's
  name in snake_case with `tsc-span`/`tsc-hash` selecting the declaration
  (greenfield §5/§8). Same-named bundle declarations are not one port;
  coalescing "for elegance" is a stop condition.
- Diagnostics are emitted ONLY via `&'static DiagnosticMessage`
  references from the generated table.
