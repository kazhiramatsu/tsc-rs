# tsc-rs

A Rust port of the TypeScript compiler (tsc 6.0.3). Active development uses
the Oxc-style virtual Cargo workspace at the repository root: `Cargo.toml`
owns members under `crates/`, each member has its own `src/`, and no
top-level `src/` exists. The paused v1 codebase was removed from the working
tree and is preserved at tag `v1-final`
(check out that tag to resume it; `scripts/bootstrap.sh` there rebuilds
its corpus/oracle). Start active work at `docs/design/README.md`, which defines
document roles and precedence. Post-H1 emitter work then follows the current
emitter architecture, the post-H1 schedule, and the selected slice packet in
that order. The retained greenfield M-stage guides are historical lineage, not
current H2 implementation maps.

## Current verification policy

The user retired the former full local CI and reference-hash chain walk on
2026-09-09. The 2026-09-15 hosted policy and 2026-09-16 batching instruction
continue that workflow; the user reconfirmed it on 2026-09-21.
The [post-H1 schedule](docs/design/greenfield/post-h1-completion-slices.md)
and [witness guide](docs/witness-testing.md) record the current execution policy,
implemented in [PR #524](https://github.com/kazhiramatsu/tsc-rs/pull/524)
(merge `bb2d51c89`).
This section supersedes the older walk/full-local merge requirements that
previously appeared here and remain in historical packets.

- During implementation, use fresh complete TypeScript observations for the
  changed behavior, focused native comparisons, adjacent regression tests,
  and relevant formatting/Clippy checks.
- Batch compatible changes on one integration branch. Send the final candidate
  to the relevant hosted acceptance and witness jobs. Keep their selected
  coverage, exact comparisons, worker limits and time budgets intact.
- Do not run `cargo xtask ci`, `scripts/chain-walk.sh`, the reference-hash
  regeneration chain, full-corpus invariants, or legacy escape/ledger gates
  as routine edit, handoff, finalization or merge prerequisites. Final
  integration is not an exception. These legacy tools are opt-in only when
  the user explicitly requests the particular operation. Requests to work
  carefully, audit thoroughly or finish the task do not reactivate them.
- Heavy full-suite and full-witness replays normally run on hosted CI. Do not
  repeat them locally when the selected hosted jobs already cover them.
  Fix a hosted failure with its focused local reproducer and rerun the
  affected hosted coverage at the updated candidate.
- Preserve frozen fixtures, historical qualification records, and known
  failure evidence. Do not re-sign a historical record, alter a comparison,
  or omit a selected test to make a result green. A stale legacy certificate
  is not a reason to restart the retired chain. Keep its historical scope
  explicit; use current focused and hosted evidence for the new candidate.
- Report only checks actually performed, with their source identity and
  scope. An omitted or interrupted legacy gate is not a passing result.
  The Functional-CI framework migration remains paused.

## Branch workflow (trunk-based)

`main` is the trunk and must satisfy the current focused/hosted merge criteria.

1. **Before implementing**, use a short-lived branch from `main`, named for
   the slice, such as `fix/<topic>` or `docs/<topic>`. Continue an existing
   authorized integration branch when batching related work.
2. Commit coherent changes with focused validation in their commit or PR
   record. Small follow-ups and related slice rungs belong on the same train,
   not separate PRs and repeated full replay cycles. Finish compatible ready
   implementation before starting hosted validation.
3. **Merge criteria:** relevant focused local checks pass for the final
   implementation, and every hosted job selected for that final candidate
   succeeds. The PR body records the actual commands, source identity,
   results, and remaining bounded limitations. Earlier-head results do not
   qualify changed behavior. No legacy full-local or chain-walk certificate
   is required.
4. **Merge via GitHub PR** (`gh` CLI): publish the combined candidate and
   monitor its selected acceptance/witness jobs. Fix failures on the same
   branch. Once required checks succeed and the PR is mergeable, merge
   automatically with `gh pr merge --merge --delete-branch`; do not request
   another approval. Use a **merge commit only, never squash/rebase** because
   evidence and design records reference the original commits. Verify the
   merged head and preserve unrelated dirty worktrees when updating locally.
5. **Explicit approval is exceptional.** A substantial design change or
   comparable expansion of scope may need approval. Ordinary authorized
   implementation, focused evidence updates, PR creation, CI fixes and
   successful PR merges do not. Existing user authorization takes priority.
6. Change `ratchet.toml` only with a reviewed accepted-state change; change
   `STAGE` only when its milestone closes. Neither is updated merely to
   obtain a successful validation result.
7. **Markdown-only changes:** when all changed paths relative to the trusted
   base end in `.md` and README's generated `STATUS` block is unchanged, run
   no local Cargo/Node/full-corpus CI. Review the rendered diff, run
   `git diff --check`, and verify changed relative links/anchors and generated
   block boundaries. Simple process/docs-only changes may land directly on
   `main`. Hosted selection is a separate path-based rule: after accounting
   for explicitly registered witness inputs, the planner skips `docs/**`, root
   `README.md`, `CONTRIBUTING.md` and `LICENSE`. Other unmatched paths,
   including `CLAUDE.md`, select the full hosted acceptance/witness
   coverage even when they end in `.md`. Follow the planner's actual selection;
   do not infer a hosted skip from the local Markdown-only rule. A workflow,
   schema, golden, generated artifact or generated-status change follows the
   relevant focused/hosted path; it is not a documentation-only exception.
8. **Hosted execution:** `.github/workflows/ci.yml` runs the affected
   acceptance groups (`early`, `wide`, `late`) selected by
   `.github/ci/replay.py`; `.github/workflows/witness.yml` runs the selected
   witness groups. The `gates` and `witness-gates` aggregates require every
   selected job to succeed. Cargo build workers stay capped at two. Do not
   replace or reduce this coverage, or run the full acceptance command
   locally just because the command remains available. Stress and performance
   measurements are separate explicitly scoped work.

## Isolation and work during waits

While a long authorized check is running, look for useful independent work:
read-only analysis, documentation drafts, or work in an isolated checkout.
Keep the source snapshot being checked fixed. Do not edit its crates,
fixtures, oracle inputs or ratchets while a producer or test reads them.
Do not run competing heavy builds during local performance measurements.
Observation production uses the designated canonical path.

Batch compatible fixes into the integration branch before publishing the
candidate. Sequence overlapping-file work. Avoid separate trains for each
small repair, and do not start a legacy walk/full-local gate as a batching
ritual.

## Collaboration and ownership

When independent lanes are used, the integrator is the single writer of
shared integration files: `plan.rs`, `execute.rs`, profiles, transitions,
`ratchets/`, `crates/oracle/*.mjs`, `h2_2c_acceptance.rs`, pin surfaces,
`contracts.rs` registrations and integration documentation.

Each lane's ticket fixes its base SHA, case IDs, permitted non-overlapping
paths, expected results, focused tests, and stop conditions. Verify changed
files against that ticket before integration. Record evidence when a cause
belongs to another owner; do not silently expand a lane's scope. Evidence
research and future design work may remain read-only.

Lane handoffs carry focused and adjacent results. Large combined replays go
to hosted CI under the current policy; there is no mandatory full local
battery or fixed walk/gate tail. Serialize canonical evidence writes and
shared-file edits, preserve the tested source snapshot, and let the configured
hosted jobs use their existing parallelism. Delegation itself needs a concrete
independent task, not merely an available agent slot.

## Verification quick reference

- Use [the witness guide](docs/witness-testing.md) to list/select current
  cases and inspect the relevant hosted groups before running a local test.
- Run checks directly to a log and inspect their real exit status. Never pipe
  a gating command through `tail`, `head` or `grep`: the last pipeline command
  can hide the test's failure. Read/filter the saved log only after recording
  the test exit status.
- Demote authorized heavy local work on macOS with `taskpolicy -b nice -n 15`
  so the machine stays usable. Prefer hosted runners for heavy verification.
  Wall-clock performance measurements need a separately recorded, unloaded,
  consistent priority setting; demoted development timings are not benchmark
  results. Never raise a performance ceiling to compensate for interference.
- The legacy tools listed below remain available for explicitly requested,
  scoped investigation. This list is not a merge checklist and does not
  authorize a full local gate, corpus sweep or hash-chain regeneration.
  If a legacy walk is explicitly requested, use its maintained driver rather
  than a handwritten loop, and keep interrupted runs unqualified.

## Optional legacy diagnostics and historical tooling

- H1 owner inventory: `node crates/oracle/h1-owner-inventory.mjs --check`
  regenerates in memory and byte-compares the report-only H1.0a active-root
  graph, declaration/body/ledger hashes, unresolved calls, and dormant seams.
- H2 transition inventory: `node crates/oracle/h2-transition.mjs --check`
  regenerates in memory and byte-compares the H2 owner/Rust-converse graph,
  all 15,642 compiler/conformance/project/transpile dispositions, and the
  39-row pre-runtime profile transition while pinning every H1 input hash and
  selecting H2.1a next.
- H2 pre-runtime baseline: `node crates/oracle/h2-baseline.mjs --check`
  validates the same-runner alternating H2.0a/candidate evidence for three H0
  no-emit workloads, the exact H1 emit case, L1 fresh/incremental edit,
  binaries/startup, two sink faults, positive H1 controls, and zero activity
  across all 37 unadmitted H2 runtime slices. Only the approved macOS arm64
  profile may mint it with `--compare`. The older H1 no-emit/emit performance
  artifacts are immutable historical lineage; their generators remain syntax
  checked but do not validate a later runtime tree.
- Conformance single band: `cargo xtask conformance [--band 2xxx]`
  (every gating run also enforces the A1 accepted-set ratchet;
  partial `--files`/`--limit` runs gate the executed-fixture
  projection instead of the integer counts)
- Completion report: `cargo xtask completion` writes all eleven
  definition-of-done rows to `target/completion/report.json` and
  succeeds while rows remain pending during M8/M9.
  `cargo xtask completion --require-done` is the post-M9 release gate
  and fails with every pending row named.
- Tier before/after report: run conformance twice with distinct
  `--out-json` paths, then `cargo xtask conformance-diff <before.json>
  <after.json>` (optional `--out-json <path>`; default
  `target/conformance/shadow-diff.json`). This is exact T1/T2/T3 review
  evidence: shadow/report-only before formal activation and supplemental
  slice evidence afterwards. It does not itself update or enforce a ratchet.
- Terminal-slice evidence: before editing, run `cargo xtask
  slice-evidence snapshot --slice <name> --targets <csv> --band
  <all|2xxx|syntactic> --out-dir </tmp/new-before-dir>`; after the
  implementation, run `cargo xtask slice-evidence verify --before-dir
  </tmp/before-dir> --out-dir </tmp/new-after-dir> --baseline
  origin/main`. Both directories must be new and outside the Git
  worktree. The report-only command hashes inputs/snapshots/logs,
  rejects FP, tier losses, universe drift, or stale before evidence,
  and runs the read-only repository evidence gates.
- Accepted-set state: `cargo xtask ratchet check [--baseline
  origin/main]` verifies `ratchets/` artifacts + lineage;
  `cargo xtask ratchet update` re-measures and adds identities only
  (never run it to "fix" a regression — fix the regression)
- Exact scope (A2): `cargo xtask scope audit [--baseline origin/main]`
  verifies `m8-scope.json` schema-2 identities against goldens, the
  duplicate-bucket canaries (68/65), the Node/Rust canonical-encoder
  cross-check (`crates/oracle/identity.mjs`), band-pin/global-freeze
  anchors, and tombstone standing proofs
- H0 host owner registry: `cargo xtask host-resolution check [--baseline
  origin/main]` verifies all 241 frozen identities, exact vendored owner
  spans/hashes and resolution-request chains, positive canaries plus reviewed
  typed controls, bounded pre-H0 reference profiles, and trusted-base
  open/closed/lapsed transitions, including historical T0--T4 evidence for
  every row carrying closure provenance
- Family map (A5): `cargo xtask families check [--baseline
  origin/main]` verifies `diag-families.json` — every corpus-exercised
  non-2XXX (code, pass) row mapped exactly once, canary existence,
  freeze/universe-extension anchors, trusted-base compare;
  `cargo xtask families report` writes the per-family supported
  rollup (`target/families/report.json`) from one full gating
  band=all run (`--verify` re-checks a stored report's input
  fingerprints)
- Escape expiry audit: `cargo xtask escapes --stale $(cat STAGE)`
  (also verifies `escapes.toml`; after adding/retiring an escape run
  `cargo xtask escapes --write-manifest` — the manifest diff is the
  review surface)
- Symbol audit vs oracle (full corpus): `cargo xtask symbol-diff
  --sample 5908 --expected symbol-diff-known.txt` gates
  unknown-diff-zero against the known stage-3.4c expando allowlist;
  regenerate with `--write-expected` (manifest diff = review
  surface). Retire the allowlist at 9.8.
- Oracle probe for pins: see scratchpad `probe.sh` pattern
  (`cargo xtask expand <fixture> --out-dir ...` + `node
  crates/oracle/driver.mjs`)
