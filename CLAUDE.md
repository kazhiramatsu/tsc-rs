# tsc-rs

A Rust port of the TypeScript compiler. The reference is TypeScript 7.1
(`tsgo`) at the commit vendored in `vendor/typescript-native/<profile>`;
`scripts/typescript7.py` builds it when you need its exact behavior. The tsc
6.0.3 line is frozen at tag `v0.1.0`, and `main` keeps no 6.0.3 behavior.

Recent work and its records: the
[tsrs comparison packet](docs/design/greenfield/slices/ts71-perf-tsrs/README.md)
(started 2026-11-01, closed 11-06 and reopened for prototype/structure review
on 11-07; the current outcome is its first section, the prototypes are listed in
[PROTOTYPES.md](docs/design/greenfield/slices/ts71-perf-tsrs/PROTOTYPES.md));
before it,
the [real-project differences packet](docs/design/greenfield/slices/ts71-real-projects/README.md)
(the 2026-10-16 handover; LSP performance in
[ts71-perf-lsp](docs/design/greenfield/slices/ts71-perf-lsp/README.md)).
Developer setup and CI: [docs/setup.md](docs/setup.md). Keep the README a
short user guide.

## Rules

- Match `tsgo` by changing the existing code path; do not add parallel
  variants, mode flags or compatibility shims.
- Pin new behavior with unit tests against `tsgo`'s output, and run the
  affected conformance cases
  (`target/release/conformance-ts71 --case <suite>/<path>.ts --no-report --dump <dir>`).
- Local checks at a slice's final bytes (owner's decision 2026-11-03): the
  focused tests of the crates the slice changed (`cargo test -p <crate>`),
  the release build, the eight-project output identity and one-thread
  measurements (`target/claude-tools/perf11/diffout.py`, `serial.py`), and
  for a front-end change the fingerprint oracle
  (`scripts/syntax_fingerprint.py --compare` against the recorded
  baseline). The workspace tests, the one-checker conformance run and the
  suites are the hosted `ci`'s job (same scripts, same ratchets); do not
  repeat them locally. After a checker change, run locally
  `scripts/conformance_ts71.py --workers 1 --checkers 4` (the script refuses
  `--check` together with `--checkers`), then compare its
  `checkers-4/report.json` with the one-checker report using
  `scripts/conformance_ts71_compare.py`; the known four differing
  configurations are recorded in the packet. Change `ratchets/ts71/` only
  with `--update` at the final bytes.
- Keep the machine's load low: one heavy job at a time, `nice -n 20`,
  `cargo -j 2`, one or two workers. Keep wall-time measurements in dedicated
  windows without concurrent builds or tests.
- Inspect each check's real exit status (do not pipe it through `tail` or
  `grep`), and report only checks actually run, with their source identity.
- Performance work (making the compiler faster or lighter, comparing against
  `tsrs`/`tsgo`, running or measuring perf lanes) follows the project skill
  `measured-port-optimization`
  ([SKILL.md](.claude/skills/measured-port-optimization/SKILL.md)) and uses
  the tools in `scripts/perf/`: outputs byte-identical is the law, the
  `--extendedDiagnostics` work counters are recorded (not gated), and a
  candidate is judged on retired instructions at one thread, 9 rounds, low
  mode (`scripts/perf/lowmode.py`), with its own release binary on the common
  base.

## Merging

- Work on a short-lived branch and open a PR whose body records the
  commands, source identity, results and remaining bounded differences.
- When the hosted `gates` check passes, merge without asking:
  `gh pr merge --merge --delete-branch` (merge commit only, never squash or
  rebase). Then record the hosted run and the merge in the owning packet on
  `main`.
- Markdown-only changes may land directly on `main` after `git diff --check`
  and a check of the changed links.
