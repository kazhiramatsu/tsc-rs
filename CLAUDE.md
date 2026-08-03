# tsc-rs

A Rust port of the TypeScript compiler (tsc 6.0.3). Active development uses
the Oxc-style virtual Cargo workspace at the repository root: `Cargo.toml`
owns members under `crates/`, each member has its own `src/`, and no
top-level `src/` exists. The paused v1 codebase was removed from the working
tree and is preserved at tag `v1-final`
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
   exact-scope audit (A2) + family-map check (A5) against the same
   base, conformance all + 2xxx + syntactic with FP=0 and
   set/integer-ratchet non-regression, the A5 families rollup,
   full-corpus invariants (`invariants --suite all --full-corpus`),
   ledger check, `escapes --stale $(cat STAGE)`
   incl. the untagged ceiling, and generated README-status freshness).
   **Documentation-only exception:** when every changed path relative to the
   trusted base ends in `.md` and README's generated `STATUS` block is
   byte-identical to the base, do not run local Cargo/Node/full-corpus CI.
   Review the rendered diff, run `git diff --check`, and verify changed
   relative links/anchors plus generated-block boundaries. Any non-Markdown
   path—or any workflow, policy, schema, golden, generated artifact, or
   generated-status change—uses the complete gate above.
4. **Merge via GitHub PR** (`gh` CLI): when the slice is done and
   the required local gate (or the documentation-only checks above) is
   green, push the branch and open a PR whose body carries the local gate
   summary (conformance rates + FP=0, escapes, tests). GitHub Actions is a
   bounded hosted guardrail, not the full-corpus acceptance authority.
   Monitor the PR and fix failures as additional commits on the same branch.
   As soon as the required hosted checks are successful and the PR is
   mergeable, merge automatically with
   `gh pr merge --merge --delete-branch` — do not wait for a separate
   user approval. Use a **merge commit ONLY, never squash/rebase**:
   commit hashes are cross-referenced from design docs, memory, and
   commit bodies, and must survive.
5. **Explicit user approval is exceptional.** Pause for approval only
   when the slice requires a substantial design change from the
   authoritative design docs or a comparable expansion of project
   scope/architecture. Ordinary producer-owned implementation,
   evidence/ratchet updates, PR creation, CI fixes, and green-PR
   merges do not require approval.
6. Update `ratchet.toml` in the slice that changes accepted state,
   and update `STAGE` only in the slice that closes a milestone.
   Neither update is deferred to the merge operation. Pull `main` after
   merging.
7. Trivial process/docs-only changes may land directly on `main`
   and be pushed. Markdown-only changes intentionally skip the local and
   hosted lane; the hosted workflow runs only its lightweight
   change classifier and required `gates` sentinel.
8. Pushing to `origin` is allowed and expected: push the slice branch
   with `-u` while working. PR Actions runs `cargo xtask ci --lane hosted`
   with Cargo parallelism capped at two. It keeps workspace audit,
   format/clippy over all targets, Node syntax, generated-schema/inventory,
   relation pins, ledger, and escapes. Focused Windows host contracts run only
   when host/path infrastructure changes. Workspace tests, semantic history,
   corpus conformance/recovery, invariants, evidence, readiness, and
   performance remain local-only. The workflow runs for pull requests and
   manual dispatch, not again after every merge to `main`; the final `gates`
   job requires the applicable hosted checks. Local `cargo xtask ci` remains
   required before opening and before merging except for the exact
   Markdown-only rule above; its result and trusted baseline are recorded in
   the PR body.

## Verification quick reference

- **Markdown-only changes:** if and only if every trusted-base diff path is
  `*.md` and README's generated `STATUS` block is unchanged, run no
  Cargo/Node/full-corpus CI. Use `git diff --check` and review links, anchors,
  and generated-block boundaries. Hosted `classify` skips the `hosted` job
  and lets the lightweight required `gates` sentinel
  succeed. Do not label a workflow/config/generated-artifact or generated-
  status change as documentation-only.
- **NEVER pipe a gate command through `tail`/`head`/`grep` — the
  pipeline's exit status is the LAST command's (tail's = 0), which
  has repeatedly masked red gates.** Run gates to a file and check
  the code explicitly, then read the log:
  `cargo xtask ci > /tmp/ci.log 2>&1; echo "ci exit: $?"`.
  This applies to every gating command below (`ci`, `conformance`,
  `ratchet check`, `scope audit`, `families check`, `escapes`,
  `ledger check`, `invariants`).
- Full gate suite: `cargo xtask ci [--baseline <trusted-ref-or-sha>]`
  (from the repository root, using the trusted base recorded in the PR). Its
  full-corpus B2 producer reuses an existing exact-fingerprint artifact
  only after raw schema/hash/inventory/count/review validation; otherwise
  it regenerates the artifact with one single-threaded worker.
- Hosted guardrail: `CARGO_BUILD_JOBS=2 cargo xtask ci --lane hosted`. It
  compiles all targets through clippy and runs static repository contracts,
  but no workspace tests, semantic history, conformance, recovery census,
  invariants, evidence, readiness, or performance gates. For manual history
  diagnosis only, add `--history-sensitive --baseline
  <trusted-ref-or-sha>`. The legacy `--lane rust|semantic [--baseline
  <trusted-ref-or-sha>]` split remains available for diagnosing either half
  of the full local gate. Except for the exact Markdown-only rule, slice
  acceptance still requires the unsplit local command above; a green hosted
  lane is never a replacement for it.
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
