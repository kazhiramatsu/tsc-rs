# tsc-rs

A Rust port of the TypeScript compiler (tsc 6.0.3). The Oxc-style virtual
Cargo workspace at the repository root is the only codebase: `Cargo.toml`
owns the members under `crates/`, each member has its own `src/`, and no
top-level `src/` exists. The paused v1 codebase is preserved at tag
`v1-final` (check out that tag to resume it; `scripts/bootstrap.sh` there
rebuilds its corpus/oracle).

The emitter was completed in
[PR #561](https://github.com/kazhiramatsu/tsc-rs/pull/561) (2026-09-21).
Performance work in PRs #570–#589 and #594 (2026-09-22 to 2026-09-28) made
the parallel compile pipeline faster and reduced its memory use, and PR #593
aligned the emitted output with tsc apart from declaration-file ordering. The
root [README](README.md) describes the command, its options and limitations,
and the current measurements.
[docs/design/README.md](docs/design/README.md) indexes the design documents:
the [emitter architecture](docs/design/greenfield/emitter-architecture.md)
describes the validated emitter, and the
[post-emitter roadmap](docs/design/greenfield/post-emitter-roadmap.md) owns
the planned follow-on work and links its packets. The completed H1/H2 slice
packets and the M-stage guides are historical records of how the port was
qualified, not instructions for new work.

Repository-owned verification artifacts belong under `ratchets/`, with M8
files in `ratchets/m8/`; see [the location guide](ratchets/README.md). Keep new
project-specific generated data out of the repository root. Historical
records retain their original paths and hashes; relocating a file does not
authorize regenerating or requalifying those records.

## Current verification policy

The full local CI (`cargo xtask ci`) and the reference-hash chain walk were
retired on 2026-09-09. Hosted acceptance and witness jobs replaced them in
[PR #524](https://github.com/kazhiramatsu/tsc-rs/pull/524) (merge
`bb2d51c89`), and the [witness guide](docs/witness-testing.md) owns the
current commands, groups and timings. These rules supersede the
walk/full-local merge requirements recorded in older packets.

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
   the change, such as `fix/<topic>` or `docs/<topic>`. Continue an existing
   authorized integration branch when batching related work.
2. Commit coherent changes with focused validation in their commit or PR
   record. Small follow-ups and related changes belong on the same branch,
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
6. Change `ratchets/ratchet.toml` only with a reviewed accepted-state change; change
   `ratchets/STAGE` only when its milestone closes. Neither is updated merely to
   obtain a successful validation result.
7. **Markdown-only changes:** when all changed paths relative to the trusted
   base end in `.md` and the generated `STATUS` block in
   `docs/verification-status.md` (historically `README.md`) is unchanged, run
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
9. **Pinned sources:** `.github/ci/qualification-policy.v2.json` pins the
   SHA-256 of the acceptance Rust sources (`rust_source_sha256`) and of the CI
   execution sources (`execution_source_sha256`). A change to a pinned file
   also updates its pin; `node .github/ci/qualification.mjs check-policy`
   reports a stale pin, and the hosted `plan` job fails on one.

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
shared integration files: `plan.rs`, `execute.rs`, `ratchets/`,
`crates/oracle/*.mjs`, `h2_2c_acceptance.rs`, pin surfaces (including
`.github/ci/qualification-policy.v2.json`), `contracts.rs` registrations and
integration documentation.

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
- The historical tools listed below remain available for explicitly requested,
  scoped investigation. This list is not a merge checklist and does not
  authorize a full local gate, corpus sweep or hash-chain regeneration.
  If a legacy walk is explicitly requested, use its maintained driver rather
  than a handwritten loop, and keep interrupted runs unqualified.

## Historical tooling

These commands were built for the M8, H0–H2 and L0/L1 qualification
milestones. They are not maintained against the current tree, so a failure
may mean a stale tool rather than a regression; an old result is not current
evidence.

- Frozen H1/H2 evidence: `node crates/oracle/h1-owner-inventory.mjs --check`,
  `node crates/oracle/h2-transition.mjs --check` and
  `node crates/oracle/h2-baseline.mjs --check` compare their inventories or
  evidence with the recorded artifacts under `ratchets/`. The H2 baseline can
  no longer be minted: its `--compare` mode ran a qualification example that
  reported the removed runtime-activity counters. The H1 no-emit/emit
  performance artifacts are immutable lineage as well.
- Conformance: `cargo xtask conformance [--band all|2xxx|syntactic]` runs
  the upstream diagnostic corpus and enforces the accepted-set ratchet
  (partial `--files`/`--limit` runs gate only the executed fixtures).
  `cargo xtask conformance-diff <before.json> <after.json>` compares two
  `--out-json` reports; it does not update or enforce a ratchet.
- Accepted-set and registry audits: `cargo xtask ratchet check [--baseline
  origin/main]` verifies the `ratchets/` artifacts and their lineage, and
  `cargo xtask ratchet update` re-measures and only adds identities (never
  run it to hide a regression). `cargo xtask scope audit`, `cargo xtask
  host-resolution check`, `cargo xtask families check|report` and
  `cargo xtask escapes --stale $(cat ratchets/STAGE)` audit
  `ratchets/m8/m8-scope.json`, the H0 host-owner registry,
  `ratchets/diag-families.json` and `ratchets/escapes.toml`; after adding or
  retiring an escape, `cargo xtask escapes --write-manifest` regenerates the
  manifest for review.
- Slice evidence: before a change, `cargo xtask slice-evidence snapshot
  --slice <name> --targets <csv> --band <all|2xxx|syntactic> --out-dir
  <new-before-dir>`; after it, `cargo xtask slice-evidence verify --before-dir
  <before-dir> --out-dir <new-after-dir> --baseline origin/main`. The
  report-only verify rejects false positives, tier losses, universe drift and
  stale before evidence. Both directories must be new and outside the Git
  worktree.
- Completion report: `cargo xtask completion` writes the definition-of-done
  rows to `target/completion/report.json`; `--require-done` fails while any
  row is pending.
- One-fixture TypeScript diagnostics: `cargo xtask expand <fixture> --out-dir
  <dir>` writes the fixture's program JSON, and `node crates/oracle/driver.mjs`
  reads `{"id":…,"programJsonPath":…}` lines on stdin and prints the vendored
  TypeScript diagnostics for each.
