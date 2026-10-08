# tsc-rs

A Rust port of the TypeScript compiler. Its reference is TypeScript 7.1 (the
native compiler, `tsgo`, at the vendored commit); the tsc 6.0.3 line ended
with release v0.1.0 (tag `v0.1.0`, branch `release/6.0.3`), and `main` keeps
no 6.0.3 code, record, oracle or test. The Oxc-style virtual Cargo workspace
at the repository root is the only codebase: `Cargo.toml` owns the members
under `crates/`, each member has its own `src/`, and no top-level `src/`
exists. The paused v1 codebase is preserved at tag `v1-final`.

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

The TypeScript 7.1 ratchet lives in `ratchets/ts71/`. Keep new
project-specific generated data out of the repository root.

## Current verification policy

The compiler follows TypeScript 7.1 at the vendored native profile
(`vendor/typescript-native/<profile>`); tsgo built from the same commit is
the reference implementation (`scripts/typescript7.py` holds the checkout and
`go -C tsc build ./cmd/tsc` builds it). tsc 6.0.3 compatibility ended with
release v0.1.0 (tag `v0.1.0` at `ed173ea36`, branch `release/6.0.3`): `main`
keeps no 6.0.3 behavior, record or oracle, and no change is required to keep
6.0.3 output. The
[cutover packet](docs/design/greenfield/slices/ts71-cutover/README.md)
records the decisions, the retired surfaces and the remaining steps.

- Merge criteria: the hosted `gates` check of `.github/workflows/ci.yml`
  succeeds for the final candidate. It requires the jobs
  `.github/ci/replay.py plan` selected: `rust` (formatting, Clippy and the
  Rust test targets) and `conformance-ts71` (the TypeScript 7.1 error and
  emit baselines on one checker, then the transpile suite, checked against
  `ratchets/ts71/`). A
  change under `docs/` or to the root `README.md`, `CONTRIBUTING.md` or
  `LICENSE` selects neither; any other change selects both. The PR body
  records the commands, source identity, results and remaining bounded
  limitations.
- During implementation, run the affected conformance cases locally
  (`target/release/conformance-ts71 --case <suite>/<path> --dump <dir>`,
  `scripts/conformance_ts71.py --filter <text> --check`), probe tsgo for the
  exact behavior, add adjacent unit tests pinned to tsgo's output, and keep
  formatting and Clippy clean.
- The ratchet must report 0 regressions (`scripts/conformance_ts71.py
  --workers 4 --check`, release build); raise it with `--update` at the
  final bytes. Lowering a row is a reviewed edit recorded in the owning
  packet with the 7.1 evidence.
- After a checker change, run the parallel control (`--checkers 4` and
  `scripts/conformance_ts71_compare.py`); differences beyond the recorded
  partition-dependent cases are defects to fix or to record.
- Compare the README corpora with tsgo (speed, peak memory and output)
  before a release and after a slice that touches the checker or the
  emitter; a regression is fixed before the work is called done.
- Report only checks actually performed, with their source identity and
  scope. An omitted or interrupted check is not a passing result.

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
   implementation, and the hosted `gates` check succeeds on it. The PR body
   records the actual commands, source identity, results, and remaining
   bounded limitations. Earlier-head results do not qualify changed
   behavior.
4. **Merge via GitHub PR** (`gh` CLI): publish the combined candidate and
   monitor its hosted jobs. Fix failures on the same branch. Once required checks succeed and the PR is mergeable, merge
   automatically with `gh pr merge --merge --delete-branch`; do not request
   another approval. Use a **merge commit only, never squash/rebase** because
   evidence and design records reference the original commits. Verify the
   merged head and preserve unrelated dirty worktrees when updating locally.
5. **Explicit approval is exceptional.** A substantial design change or
   comparable expansion of scope may need approval. Ordinary authorized
   implementation, focused evidence updates, PR creation, CI fixes and
   successful PR merges do not. Existing user authorization takes priority.
6. Change `ratchets/ts71/` only through `scripts/conformance_ts71.py
   --update` (`scripts/suites_ts71.py --update` for the suites ratchet) at
   the final bytes, or by a reviewed lowering recorded in the owning
   packet. The ratchet is never edited merely to obtain a successful
   validation result.
7. **Markdown-only changes:** when all changed paths relative to the trusted
   base end in `.md`, run no local Cargo CI. Review the rendered diff, run
   `git diff --check`, and verify changed relative links and anchors. Simple
   process/docs-only changes may land directly on `main`. Hosted selection
   is a separate path-based rule: `docs/**`, root `README.md`,
   `CONTRIBUTING.md` and `LICENSE` select no job; every other path,
   including `CLAUDE.md`, workflows, scripts, `vendor/` and `ratchets/`,
   selects both hosted jobs even when it ends in `.md`. Follow the planner's
   actual selection.
8. **Hosted execution:** `.github/workflows/ci.yml` runs `plan`, then the
   selected `rust` and `conformance (TypeScript 7.1)` jobs, then the `gates`
   aggregate that requires every selected job to succeed. Cargo build
   workers stay capped at two. Do not reduce this coverage. Stress and
   performance measurements are separate explicitly scoped work.

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
shared integration files: `ratchets/ts71/`, `vendor/typescript-native/`,
`.github/workflows/ci.yml`, `.github/ci/replay.py`,
`scripts/conformance_ts71*.py`, `scripts/suites_ts71.py`, the conformance
runner (`crates/conformance/src/ts71*`) and integration documentation.

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

- Run checks directly to a log and inspect their real exit status. Never pipe
  a gating command through `tail`, `head` or `grep`: the last pipeline command
  can hide the test's failure. Read/filter the saved log only after recording
  the test exit status.
- Demote authorized heavy local work on macOS with `taskpolicy -b nice -n 15`
  so the machine stays usable. Prefer hosted runners for heavy verification.
  Wall-clock performance measurements need a separately recorded, unloaded,
  consistent priority setting; demoted development timings are not benchmark
  results. Never raise a performance ceiling to compensate for interference.
- The local equivalents of the hosted jobs are `python3 .github/ci/replay.py
  rust` (formatting, Clippy, `cargo test --workspace`, `cargo xtask codegen
  diagnostics-check`) and, after `cargo build --release -p
  tsc-rs-conformance --bin conformance-ts71`, `python3
  scripts/conformance_ts71.py --workers 4 --check`; the README's "Run CI"
  section describes both.
- `cargo xtask codegen diagnostics` regenerates `crates/diagnostics/src/gen.rs`
  from the vendored 7.1 `diagnosticMessages.json`; it is the only xtask
  command. The other generated sources (syntax nodes, enums, scanner tables)
  are hand-maintained since their 6.0.3 inputs left the tree.

## Retired tooling

The tsc 6.0.3-era tools (the `xtask` acceptance, conformance, ratchet,
escapes, ledger, slice-evidence, completion and H0–H2/L0/L1 qualification
commands, `crates/oracle`, `crates/fuzz`, `ratchets/` outside `ts71/`,
`scripts/witness.py` and the witness suites, `ts-tests/`,
`vendor/typescript-6.0.3` and the tests that compared with tsc 6.0.3
observations) were deleted by P3-3 of the
[cutover packet](docs/design/greenfield/slices/ts71-cutover/README.md).
Their last maintained state is tag `v0.1.0` (branch `release/6.0.3`); the
`tsc-port … @6.0.3` headers in the sources cite spans of that tag's
`vendor/typescript-6.0.3/lib/_tsc.js`. Historical slice packets under
`docs/design/` describe them as they were; they are not instructions.
