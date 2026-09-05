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

## Branch workflow (trunk-based)

`main` is the trunk and must always be green (`cargo xtask ci`).

1. **Before implementing anything**, cut a short-lived branch from
   `main`, named for the slice: `m4/5.7b-call-tail`,
   `m5/flow-narrowing`, `fix/<topic>`, `docs/<topic>`.
2. Commit the slice on that branch (one slice = one branch; commit
   messages follow the existing `m4 5.x: ...` style with gates in the
   body).
   **Size-based merge cadence (user directive, 2026-08-17):** few-line
   follow-ups and endgame repairs do NOT get their own PR/full gate.
   They accumulate as commits on a closure-train branch, each gated in
   the edit loop by a lightweight targeted test written with the change
   (the focused-test discipline of `noemit-cli.md` §11, generalized).
   The full local gate runs once when the train reaches a substantive
   size (multi-file substance, a subsystem boundary, a dependent
   needing `main`, or roughly daily), and that train merges as ONE
   slice (precedent: PR #445's cumulative closure). Slice-early large
   changes keep the per-slice full gate as always. Batching reduces
   merge count, never gate strength: whatever merges still passes the
   complete local gate at its final head.
   **Thick ladder trains (user directive, 2026-08-25):** a slice's ladder
   rungs (design-gate m-*/ca-* pairs and their follow-ups) bundle onto
   1–2 trains, NOT one PR per rung; intra-train rungs verify with
   targeted tests (walks, qualification check, focused suites) and the
   full local gate runs ONCE at each train's final head (precedent:
   PR #475, the H2.6a m-2..ca-2 train).
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
4. **Merge via GitHub PR** (`gh` CLI): push the branch early and open
   the PR as soon as a merge candidate exists so the hosted acceptance
   check runs in parallel with the local gate (user directive,
   2026-08-17; wall cost drops from the sum to the max of the two).
   Before merging, the PR body must carry the completed local gate
   summary (conformance rates + FP=0, escapes, tests) recorded at the
   final candidate head. GitHub Actions runs only
   the fixed `ts-tests` acceptance entrypoint; it does not repeat the complete
   local gate.
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
   full-corpus developer gate. A pull request still receives the same hosted
   `ts-tests` acceptance check as every other candidate.
8. Pushing to `origin` is allowed and expected: push the slice branch
   with `-u` while working. The current PR Actions workflow has one stable job,
   `gates`, and one test command, `cargo xtask acceptance`. That command is the
   unsplit acceptance boundary sourced from `ts-tests`; it currently executes
   the full diagnostic conformance corpus and is the extension point for H1
   emit acceptance. Actions does not run formatting, Cargo check/test/Clippy,
   owner-focused tests, Windows canaries, stress, performance, evidence
   production, or `cargo xtask ci`. Cargo build parallelism remains capped at
   two. The workflow runs for pull requests, merge groups, pushes to `main`,
   and manual dispatch. Local `cargo xtask ci --baseline <trusted-base>`
   remains required before merging except for the exact Markdown-only
   rule above (opening the PR earlier to overlap the hosted check is
   expected); its result and trusted baseline are recorded in the PR
   body before the merge. Manually dispatched approved-runner performance workflows are
   separate qualification tools, not ordinary GitHub CI.

## Train loading (standing directive, 2026-08-30)

Maximize passengers per train: every walk+gate cycle costs ~3-4h of
fixed tax, so bundle EVERY ready item into the departing train —
all signed fix classes, facet/manifest shrinks, packet updates,
docs, and any small repairs — rather than giving each its own train.
Sequence conflicting-file work WITHIN the train (implement lane B on
top of lane A's committed state in the same worktree) instead of
splitting into two trains. A train departs (walk+gate) only when
nothing else is ready to board or a dependent needs main.

## Parallel work during waits (standing directive, 2026-08-30)

Whenever a long-running step is in flight (walk, gate, hosted check,
mint, delegated implementation), ALWAYS look for parallel work and
execute it — never idle on a wait. Rules:

1. **Isolation**: parallel work runs in its own `git worktree` (or is
   read-only analysis / docs / scratchpad work). Never touch the
   canonical checkout's crates/oracle/ratchet surfaces while a walk,
   gate, or canonical mint is running there.
2. **Non-interference**: no heavy competing builds during a gate's
   measurement phases (the perf ceiling is wall-clock sensitive;
   codex sandbox builds run non-demoted — schedule them off the
   measurement window). Observation minting stays canonical-path-only.
3. **Typical wait-time work**: next-wave investigation from existing
   probe/census data (read-only), design-packet drafting and
   cross-review rounds, delegation SPEC authoring + launching
   implementation lanes in worktrees, memory/docs updates, residue
   analysis for the following train.

## Staffing / parallelization (standing directive, 2026-09-05)

A slice runs as a small cell (3–4 roles), never as several implementers on
one pod:

1. **Integrator + core = the single writer** of `plan.rs` / `execute.rs`,
   the profiles, the transition, ORDER, `ratchets/`, `crates/oracle/*.mjs`,
   the runner (`h2_2c_acceptance.rs`), `scripts/chain-walk.sh`, every pin
   surface, `contracts.rs`-style registration files and docs; also final
   integration, the canonical mint, the walk and the gate. A lane never
   touches these.
2. **Independent implementer lanes** (1–2, Codex worktrees) own
   census-separated cause classes with NON-overlapping file sets
   (`docs/design/greenfield/post-h1-completion-slices.md:224`).
3. **Evidence owner**: causal classification, upstream investigation,
   fixtures, expected results, focused tests, the packet draft; no
   production code.
4. **Optional ahead lane**: the next slice's inventories/packets, never
   inside the current implementation.

Work tickets (one SPEC per assignee) fix: base SHA, target case ids,
allowed paths, expected results, focused test, stop conditions, forbidden
surfaces. Allowed paths are the census-derived concrete files, FIXED at
launch and checked by a preflight (pairwise disjoint, no single-writer
surface, every file present); each lane's changed files are re-verified
against its ticket before integration. A row whose proven owner lies
outside the set is recorded OUT-OF-SCOPE with evidence and skipped, never
a STOP that waits for a widened ticket. Lane battery in two stages: light
after every closed class; FULL (`cargo test -p tsc-rs-xtask`, the FULL
5g, the cross-band runners, the whole compiler/checker/emitter suites)
before handoff. No lane battery while the gate runs: the performance
ceiling is measured on the same machine under the same unloaded
conditions, and ahead lanes launch only after the gate exits. No
per-worker PR/train — the integrator folds every lane into 1–2 thick
trains.

Serial regardless of headcount: canonical minting, the chain walk, the
full gate at the final head, hosted acceptance, transition landing, edits
to shared hotspots. A closure wave is judged on rows closed per train with
the fixed walk+gate tail kept (w2 criteria: one walk launch, zero
train-battery-first regressions, zero allowed-path STOPs, zero
perf-contention reruns, more rows than the previous wave in no more wall
time, zero lane conflicts / multi-writer pins). Precedents: PR #504 (m-2,
lanes A–D), PR #505 (w1, the first application), the w2 train.

## Verification quick reference

- **Merge cadence:** few-line changes ride closure trains — verify each
  with its own lightweight targeted test, and run the full gate once per
  substantive train (see branch-workflow item 2). Every merge candidate
  still passes the complete gate at its final head.
- **Markdown-only changes:** if and only if every trusted-base diff path is
  `*.md` and README's generated `STATUS` block is unchanged, run no
  Cargo/Node/full-corpus CI. Use `git diff --check` and review links, anchors,
  and generated-block boundaries. Hosted `classify` skips the
  `host_platform` job and lets the lightweight required `gates` sentinel
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
- **Background priority (user directive, 2026-08-17):** launch every
  heavy multi-minute command demoted so the machine stays usable:
  `taskpolicy -b nice -n 15 cargo xtask ci ...` (children inherit).
  Exception: wall-clock performance observations are invalid under
  demotion (measured 60.7s vs 16s normal). If a demoted gate fails
  ONLY on the reviewed performance ceiling, rerun the same command at
  normal priority — the resume journal retains the green phases, so
  only the measurement-bearing phase repeats. Never raise a ceiling
  to compensate.
- **Oracle chain walk = `bash scripts/chain-walk.sh [readiness-slice-id]`,
  NEVER a hand-written session loop.** The driver hard-refuses to start
  until `cargo fmt --all -- --check` and workspace clippy are green:
  re-minting the ladder before the Rust tree reaches final bytes re-stales
  the profile ladder (h2_2c_acceptance.rs bytes are pinned by 15 profile
  ratchets + the hosted qualification policy) and repeats the entire
  converge (paid twice: slice A 57-min re-observation; H2.6a ca-2
  2026-08-25, a 3-line post-walk fmt reflow). The driver also self-checks
  its ORDER list against `crates/oracle/h2-*.mjs` on disk and refuses to
  walk on drift, so the slice that adds or retires an oracle script MUST
  extend/trim ORDER in that same slice (verify with `WALK_DRY=1`);
  `scripts/chain-walk-repin.py` is its stale-generator-pin fallback.
  The walk tail and the gate's structural-preflight both run
  `scripts/pin-audit.py`: every Rust-side artifact pin literal (harness
  integration tests' RECORDED/lineage hashes — the m-1 "walk repair"
  class) is verified in seconds instead of failing workspace-tests ~40
  minutes into the gate; repair with `pin-audit.py --fix` + the affected
  harness integration tests, and classify newly discovered pin-holding
  files in the script's AUDITED/EXEMPT lists (it refuses until you do).
  **Red-suite-first (gate-tax 5): when a Rust fix answers a red suite,
  run that failing band/fixture on the fixed binary BEFORE walking —
  set `PRE_SUITE="<command>"` and the driver runs it in preflight;
  never converge unvalidated bytes.** The driver also reports ALL stale
  pin surfaces at once in preflight and at the tail
  (`scripts/walk-preflight.py`: harness, policy, schema-const, fuzz
  manifests, pin-index), prints the prospective stale-cone plan, and
  enforces zero 5g re-observation on pin-only cascades via the check
  outcome record (`WALK_EXPECT_OBS` / `TSRS_H2_5G_FRESH` are the
  recorded escapes). Gate-tax 8 (2026-08-30): the walk is ONE
  invocation per converge — a §3 recovery phase repairs the two
  pure-function surfaces (the enumerated h2-5g-profile schema-const
  five via `scripts/schema-const-repin.py`, and the harness pin
  manifest `ratchets/pins/harness-expected.v1.json` values via
  `scripts/harness-pins.py`) before fmt/clippy; the schema-const
  repins in-round right after h2-1a-qualification writes; the
  manifest refreshes after the final minting round. The converted
  harness tests hold NO raw artifact-hash literals (pin-audit's
  discovery guard refuses reintroduction; descriptor changes need the
  dual-anchor update in the same slice). The planner must cover
  ORDER 65/65 (`scripts/walk-planner-coverage.py` refuses drift). A
  report-only restamp shadow (scripts/shadow/) runs at walk end and
  never reds a walk.
- Full gate suite: `cargo xtask ci [--baseline <trusted-ref-or-sha>]`
  (from the repository root, using the trusted base recorded in the PR). Its
  full-corpus B2 producer reuses an existing exact-fingerprint artifact
  only after raw schema/hash/inventory/count/review validation; otherwise
  it regenerates the artifact with one single-threaded worker.
- GitHub acceptance: `.github/workflows/ci.yml` runs only `cargo xtask
  acceptance`, the fixed `ts-tests` acceptance boundary, in the stable
  `gates` job. The optional
  `cargo xtask ci --lane hosted` static diagnostic and its
  `--history-sensitive --baseline <trusted-ref-or-sha>` mode remain available
  locally but are never selected automatically by Actions. The legacy
  `--lane rust|semantic [--baseline <trusted-ref-or-sha>]` split remains
  available for diagnosing either half of the full local gate. Except for the
  exact Markdown-only rule, slice acceptance still requires the unsplit local
  command above; a green GitHub acceptance result is never a replacement for
  the complete local gate. Runtime stress and approved-runner performance
  evidence are produced locally or by their explicitly dispatched
  qualification workflows, never by ordinary CI.
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
- Hosted acceptance: `cargo xtask acceptance` is fixed and unsplit; it accepts
  no file/band/limit selectors and runs only suites sourced from `ts-tests`.
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
