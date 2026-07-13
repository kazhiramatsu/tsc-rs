# tsc-rs

A Rust port of the TypeScript compiler (tsc 6.0.3). Active development
lives in `tsrs2/` (the greenfield rewrite); `src/` is the paused v1
codebase. Design docs under `docs/design/greenfield/` are authoritative;
implementers start from the stage step docs referenced there.

## Branch workflow (trunk-based)

`main` is the trunk and must always be green (`cargo xtask ci`).

1. **Before implementing anything**, cut a short-lived branch from
   `main`, named for the slice: `m4/5.7b-call-tail`,
   `m5/flow-narrowing`, `fix/<topic>`, `docs/<topic>`.
2. Commit the slice on that branch (one slice = one branch; commit
   messages follow the existing `m4 5.x: ...` style with gates in the
   body).
3. **Merge criteria** — all gates green on the branch:
   `cargo xtask ci` (fmt --check, clippy -D warnings, build, tests,
   relpin, conformance all + 2xxx with FP=0 and integer-ratchet
   non-regression, invariants, ledger check, `escapes --stale $(cat
   tsrs2/STAGE)` incl. the untagged ceiling).
4. Merge back with a merge commit marking the slice boundary:
   `git checkout main && git merge --no-ff <branch>`, then delete the
   branch (local and remote). Bump `tsrs2/ratchet.toml` and
   `tsrs2/STAGE` as part of the slice, not the merge.
5. Trivial process/docs-only changes may land directly on `main`.
6. **Pushing to `origin` is allowed and expected**: push the slice
   branch with `-u` while working (backup/visibility), push `main`
   after every merge. PR-based review via `gh` is available if ever
   preferred, but the default is local `--no-ff` merge + push.

## Verification quick reference

- Full gate suite: `cargo xtask ci` (from `tsrs2/`)
- Conformance single band: `cargo xtask conformance [--band 2xxx]`
- Escape expiry audit: `cargo xtask escapes --stale $(cat STAGE)`
- Oracle probe for pins: see scratchpad `probe.sh` pattern
  (`cargo xtask expand <fixture> --out-dir ...` + `node
  crates/oracle/driver.mjs`)
