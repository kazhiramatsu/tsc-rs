# tsc-rs

A Rust port of the TypeScript compiler. The reference is TypeScript 7.1
(`tsgo`) at the commit vendored in `vendor/typescript-native/<profile>`;
`scripts/typescript7.py` builds it when you need its exact behavior. The tsc
6.0.3 line is frozen at tag `v0.1.0`, and `main` keeps no 6.0.3 behavior.

Current work and its records: the
[API server packet](docs/design/greenfield/slices/ts71-api-server/README.md).
Developer setup and CI: [docs/setup.md](docs/setup.md). Keep the README a
short user guide.

## Rules

- Match `tsgo` by changing the existing code path; do not add parallel
  variants, mode flags or compatibility shims.
- Pin new behavior with unit tests against `tsgo`'s output, and run the
  affected conformance cases
  (`target/release/conformance-ts71 --case <suite>/<path>.ts --no-report --dump <dir>`).
- When a change reaches the compiler's paths, run once at the final bytes
  `scripts/conformance_ts71.py --workers 2 --check` and
  `scripts/suites_ts71.py --check` (rebuild both release binaries first):
  0 regressions. After a checker change, also run the parallel control
  (`--checkers 4`, then `scripts/conformance_ts71_compare.py`). Change
  `ratchets/ts71/` only with `--update` at the final bytes.
- Keep the machine's load low: one heavy job at a time, `nice -n 20`,
  `cargo -j 2`, one or two workers. No per-slice performance measurement
  for now.
- Inspect each check's real exit status (do not pipe it through `tail` or
  `grep`), and report only checks actually run, with their source identity.

## Merging

- Work on a short-lived branch and open a PR whose body records the
  commands, source identity, results and remaining bounded differences.
- When the hosted `gates` check passes, merge without asking:
  `gh pr merge --merge --delete-branch` (merge commit only, never squash or
  rebase). Then record the hosted run and the merge in the owning packet on
  `main`.
- Markdown-only changes may land directly on `main` after `git diff --check`
  and a check of the changed links.
