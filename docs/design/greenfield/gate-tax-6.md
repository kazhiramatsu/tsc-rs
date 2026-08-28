# gate-tax 6 — workspace-test target receipts + the darwin RSS fix

Status: design packet for the ratified infrastructure train between the
H2.6b close and H2.6c (new-ci-evidence-dag.md §Sequencing amendment,
2026-08-27 roadmap review). Two file-disjoint deliverables, one train:
per-test-TARGET receipts for the ~40-minute workspace-test phase, and
the darwin high-water RSS measurement fix for the L1 stress ceiling.

## 1. The measured tax

- `ci_workspace_tests` (main.rs:7867): one workspace compile (`cargo
  test --workspace --all-targets --no-run`, NEVER skippable) then ~42
  test executables through the bounded pipeline — ~30-40 min per gate
  regardless of change scope. Expected recovery ≈20-30 min per local
  gate across the remaining Rust trains (roadmap review).
- L1 stress RSS: the darwin arm of `peak_rss_bytes`
  (l1_incremental_stress.rs:490) reads ONE CURRENT `ps -o rss=` value
  of the whole xtask process at phase end — neither a high-water nor
  work-scoped. Measured false-red: 836MB/256MB on a full-decline run
  with perfect functional metrics (2026-08-26).

## 2. gt6 — per-target receipts (design)

**Attach point:** the runner already enumerates executables
(`cargo_test_executables` from the compile's JSON) and runs each
independently — the receipt is per EXECUTABLE (= package+target, the
Codex-recommended granularity).

**Key (fail-closed):**
`H(test binary bytes, declared runtime input tree, environment
projection, harness-thread count, toolchain)`. The binary hash binds
transitive sources, proc macros, features, linker, and the test
inventory in one term (anything compiled in changes the binary). The
RUNTIME inputs a test reads from disk (ratchets, witness artifacts,
vendor files) are the soundness crux and are DECLARED per executable:

- A curated `TEST_TARGET_INPUT_SCOPES` table (xtask-side, the
  local-ci-resume `InputScope` idiom one level down) maps executable
  labels to typed input trees.
- **Only executables with a declared scope are receipt-eligible.** An
  undeclared executable ALWAYS runs (the uncached lane) — fail-closed
  by construction, no tracing dependency. State-inspecting tests stay
  undeclared permanently.
- A declared scope that misses a real input is the residual risk; the
  curation rule is conservative (declare only executables whose input
  surface is obvious and stable — the harness contract suites reading
  named ratchets), and the PILOT (below) validates every declared
  scope against reality before enforcement.

**Mechanics:** receipts live under `target/ci-test-receipts/` —
machine-local, append-only, atomic per-target mint after a green run
(the gate-tax 3 trust class verbatim); on hit the executable is
SKIPPED with a printed `receipt hit` line naming the key terms; any
term mismatch runs the test and re-mints. `--fresh` forces the full
pipeline. The workspace COMPILE always runs; the phase summary prints
`hit/run` counts so a broken producer is visible (the gt5-C lesson).

**Pilot before enforcement (Codex bar):** replay 3-5 representative
historical Rust changes (an emitter slice, an xtask-only change, an
oracle-only change) with receipts in REPORT-ONLY mode — every
would-skip decision is validated by actually running the target and
comparing outcomes; a single would-skip-but-failed case blocks
enforcement until its scope is fixed.

## 3. The darwin RSS fix (design)

Replace the darwin arm of `peak_rss_bytes` with
`getrusage(RUSAGE_SELF).ru_maxrss` (bytes on darwin — a TRUE lifetime
high-water, the exact analog of the Linux `VmHWM` arm). That fixes the
"current value" half of the trap. The CUMULATIVE-process half (earlier
gate phases inflating the high-water before L1 runs) is fixed by
SCOPE: the stress loop's measured work runs in a SPAWNED child
process, and the ceiling compares the CHILD's peak (`wait4` rusage
`ru_maxrss`) — the interim resume-rerun remedy becomes unnecessary.
The ceiling value itself does not move (never raise a ceiling to
compensate). The Linux arm is untouched.

**§3-A Ceiling recalibration (2026-08-28, user-ratified).** The "value
does not move" sentence above assumed the true child peak fit under
the 256 MiB ceiling; the first honest measurement FALSIFIED that. The
old darwin arm read the parent's CURRENT rss after cleanup and never
observed the high-water, so 256 MiB was never validated against the
quantity it now names. Measured child peaks (same fixture/seed,
`wait4 ru_maxrss`): 4 edits = 339.2 MiB (already over the old
ceiling), 16 edits = 1064.5/1064.6 MiB across two runs (0.1 MiB
repeatability — a deterministic working set, not noise), 32 edits =
1445.8 MiB. The monotone per-edit growth is the parser arena's
append-only generation retention (each edit's `node_range` extends
the arena; ~17 generations at 16 edits), not a leak
(`final_registry_entries` 0). New reviewed ceiling for the 16-edit CI
invocation: **1.25 GiB (1342177280)** = measured + 17.5% headroom,
tight enough to catch a per-edit-cost regression. This is a metric
semantics change, not a compensating raise. BACKLOG (parser-owned,
not this train): incremental reuse consumes only the immediately
prior generation (~195k reused nodes/edit), so a two-generation
arena window could bring the true peak back to the ~300-400 MiB
class; re-review the ceiling downward when that slice lands.

## 4. Train shape

Both deliverables are xtask-side Rust (+ the L1 harness spawn seam) —
crate-byte train: the standard walk + full gate; the receipts phase
lands REPORT-ONLY in this train and flips to enforcement in a
follow-up commit once the pilot bank is green. Implementation lane:
known-complex (cache-key soundness + process measurement) →
`gpt-5.6-sol` at xhigh per the delegation policy; the pilot replays
and the enforcement flip stay with the operator.

## 5. Prohibitions

The workspace compile is never receipt-skipped; no receipt for an
undeclared executable; no ceiling changes; no hosted-side caching (the
sequencing amendment's limits); no enforcement before the pilot bank
is green.
