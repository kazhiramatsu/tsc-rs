# tsc-rs

A Rust port of the TypeScript compiler (tsc 6.0.3). Active development
lives in `tsrs2/` (the greenfield rewrite). The paused v1 codebase was
removed from the working tree and is preserved at tag `v1-final`
(check out that tag to resume it; `scripts/bootstrap.sh` there rebuilds
its corpus/oracle). Design docs under `docs/design/greenfield/` are
authoritative; implementers start from the stage step docs referenced
there.

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
   relpin, accepted-state lineage + trusted `origin/main` comparison,
   exact-scope audit (A2) against the same base, conformance all +
   2xxx + syntactic with FP=0 and set/integer-ratchet non-regression,
   invariants, ledger check, `escapes --stale $(cat tsrs2/STAGE)`
   incl. the untagged ceiling).
4. **Merge via GitHub PR** (`gh` CLI): when the slice is done and
   gates are green, push the branch and open a PR whose body carries
   the gate summary (conformance rates + FP=0, escapes, tests). The
   user runs their external review against the PR; fixes land as
   additional commits on the same branch. On approval, merge with
   `gh pr merge --merge --delete-branch` — **merge commit ONLY,
   never squash/rebase**: commit hashes are cross-referenced from
   design docs, memory, and commit bodies, and must survive.
5. Bump `tsrs2/ratchet.toml` and `tsrs2/STAGE` as part of the slice,
   not the merge. Pull `main` after merging.
6. Trivial process/docs-only changes may land directly on `main`
   and be pushed.
7. Pushing to `origin` is allowed and expected: push the slice branch
   with `-u` while working. PR Actions runs the same full gate with the
   immutable PR-base SHA; local `cargo xtask ci` remains required before
   opening and before merging.

## Verification quick reference

- Full gate suite: `cargo xtask ci [--baseline <trusted-ref-or-sha>]`
  (from `tsrs2/`; PR Actions supplies the immutable base SHA)
- Conformance single band: `cargo xtask conformance [--band 2xxx]`
  (every gating run also enforces the A1 accepted-set ratchet;
  partial `--files`/`--limit` runs gate the executed-fixture
  projection instead of the integer counts)
- Accepted-set state: `cargo xtask ratchet check [--baseline
  origin/main]` verifies `ratchets/` artifacts + lineage;
  `cargo xtask ratchet update` re-measures and adds identities only
  (never run it to "fix" a regression — fix the regression)
- Exact scope (A2): `cargo xtask scope audit [--baseline origin/main]`
  verifies `m8-scope.json` schema-2 identities against goldens, the
  duplicate-bucket canaries (68/65), the Node/Rust canonical-encoder
  cross-check (`crates/oracle/identity.mjs`), band-pin/global-freeze
  anchors, and tombstone standing proofs
- Escape expiry audit: `cargo xtask escapes --stale $(cat STAGE)`
  (also verifies `escapes.toml`; after adding/retiring an escape run
  `cargo xtask escapes --write-manifest` — the manifest diff is the
  review surface)
- Oracle probe for pins: see scratchpad `probe.sh` pattern
  (`cargo xtask expand <fixture> --out-dir ...` + `node
  crates/oracle/driver.mjs`)
