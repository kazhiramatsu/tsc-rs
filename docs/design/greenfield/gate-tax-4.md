# gate-tax 4 — exact-fingerprint reuse for the two full-corpus Rust executions

CI slice (gate-tax 1/2/3 lineage: PR #448/#450/#458/#462), branch
`ci/gate-tax-4`, forked at `065d9f2e` (the H2.6a train, pre-merge; merge
`main` after PR #475 lands and before this slice's PR). Ratified
2026-08-25 as the next slice after the H2.6a train merges. Goal: the two
full-corpus Rust executions inside the semantic lane re-run on every
train even when no execution-relevant byte changed —

- **B4 conformance/performance producer** (`m8_evidence::produce_all` →
  `produce_performance`): the timed full-corpus checker child that
  yields the `all`/`2xxx`/`syntactic` views + the A5 families report,
  unconditionally invalidated and re-executed each run;
- **full-corpus invariants** (`cargo xtask invariants --suite all
  --full-corpus`): six suites over every expanded program,
  attestation unconditionally invalidated at entry.

After this slice, an exactly-unchanged execution state (same compiler
binary, corpus, ratchet/scope anchors, workspace) reuses the standing
proven artifact instead of re-executing, and a re-run / mjs-only /
docs+ratchet-only train's gate drops from ~tens of minutes of semantic
execution to seconds of validation. One changed byte in any key term
pays today's full path (fail-closed, all-or-nothing).

## 0. Relation to the gate-tax 2/3 adjudication

gate-tax 2 §4 rejected check-side **adoption** for 5g: trusting stored
observation bytes because their recorded inputs match, with a LOOSE key
(no generator, no lib term) and no proof anchor. gate-tax 3 landed the
receipt alternative: content is trusted only while byte-anchored to a
state a local full run proved, under a TOTAL key.

This slice applies the same standard, and its precedent already lives
in production: the B2 runtime-coverage artifact ("its own strict reuse
validator", CLAUDE.md; `runtime_artifact_is_current`) reuses on exact
`header.fingerprint` match after raw validation. The B4 and invariants
keys here are TOTAL in the same sense:

- `performance_fingerprint` (existing, reviewed) already rolls every
  compiler/xtask crate source, `Cargo.toml`/`Cargo.lock`/toolchain,
  `ratchet.toml`, `m8-scope.json`, `diag-families.json`, the immutable
  oracle-state ratchets, `pins/recovery.json`, `goldens/`, the
  conformance corpus, the vendored TS libs, **and the producer
  executable hash** (`include_executable=true`).
- `controlled_input_fingerprints` (existing, reviewed) rolls the same
  semantic surface for invariants across 14 named groups, and
  `verify_inner` already re-fingerprints and compares them plus the
  canonical workspace path — it is the SAME authority `m8 readiness`
  trusts for completion row 10.

A fabricated-but-internally-consistent artifact would require write
access to `target/` — the same trust boundary as the resume journal and
the gate-tax 3 receipt (§4 there). Machine-local by design: current_exe
and workspace terms make a copied artifact miss.

## 1. Mechanism — B4 (`m8_evidence.rs`)

- The **PerformanceArtifact is the receipt**: it already embeds
  `header.fingerprint` (the total key) and `ci_conformance` (role,
  path, bytes, sha256 for `all`/`2xxx`/`syntactic`/`families`).
- `produce_all` consults `reuse_performance(...)` BEFORE invalidating
  the performance artifact and the four CI outputs (manifest and fuzz
  invalidation are unchanged). On reuse:
  1. artifact parses, `schema`/`producer_version` current, runner
     profile resolves and matches the observed os/arch, observation
     within its recorded runner ceilings, non-zero corpus counts;
  2. `header.fingerprint == performance_fingerprint(workspace)` — a
     fresh recomputation, never a cached value;
  3. every `ci_conformance` binding re-reads its output file and
     byte-verifies length + sha256 (missing/tampered → miss);
  4. the four verified outputs are bound through the UNCHANGED
     in-process move-only flow (`ci_conformance_invocation` → `begin`
     → `publish`) so `consume_ci_conformance` and every downstream
     consumer (recovery census, readiness, manifest) see exactly
     today's token discipline — only the child executions are skipped.
  5. the decision is printed either way
     (`b4 conformance: receipt hit …` / `… full run (<reason>)`).
- On ANY miss the full path runs unchanged (invalidate → timed child →
  ceilings → publish → fresh artifact = fresh receipt). `--write`-side
  and hosted flows are untouched; the cache-off smoke and wall/RSS
  observation carry forward on a hit (identical binary + inputs: the
  measured claim is unchanged; a re-measure is exactly what the
  fingerprint says it would be — and any doubt re-measures by touching
  any key input).

## 2. Mechanism — invariants (`main.rs` + `invariant_attestation.rs`)

- Entry of `fn invariants`, only for the exact full-corpus all-suite
  invocation (`--suite all --full-corpus`, no `--limit`): run
  `verify_inner` (the readiness authority). A valid standing
  attestation — outcome passed, six suites, canonical workspace match,
  all 14 controlled-input groups byte-identical — prints the hit
  decision and returns WITHOUT invalidating; the attestation remains
  the standing proof.
- Any probe red (missing/invalid/partial/stale) prints the miss reason
  and runs today's path unchanged: invalidate first, execute all six
  suites, `write_success` re-mints.
- Sampled/partial invocations never consult and never mint (unchanged).

## 3. Failure modes, adjudicated

- **Crate/corpus/ratchet/goldens/toolchain edit** → fingerprint /
  controlled-input miss → full path. (Every profile-ladder walk edits
  only `crates/oracle/*.mjs` + `ratchets/h2-*` pin artifacts — neither
  is in either key — so post-walk trains hit; that is the ratified
  point of this slice.)
- **Rebuilt binary, same sources** → cargo does not rebuild an
  unchanged binary; if it does (toolchain change), the exe hash and
  rust-build group miss → full path.
- **Output file tampered/removed** → byte re-verification miss (B4) /
  readiness re-probe red (invariants) → full path.
- **Artifact forged wholesale** → requires `target/` write access =
  the existing local trust boundary (gate-tax 3 §4); a fresh clone or
  another machine misses on exe + workspace.
- **Copied repo / worktree** → workspace canonical-path term
  (invariants) and exe-path-independent but workspace-validated
  binding (B4 `secure_output_path`) keep locality; runner profile
  os/arch must also match.
- **Torn writes** → both artifacts publish via same-directory temp +
  atomic rename (existing house pattern); a torn file fails parsing →
  miss → full path.
- **Ceiling regression hidden by reuse** → the ceilings were enforced
  at mint; reuse revalidates the recorded observation against the
  CURRENT runner profile ceilings, so a ceiling tightened in
  `m8-evidence.json` (config) forces… (config is inside CONFIG_REL =
  key term) → miss → fresh measured run.

## 4. Accepted, documented costs

- All-or-nothing: one changed key byte re-executes the whole B4 child
  and/or all six invariant suites. Per-case receipts stay deferred to
  the post-H2.9 evidence DAG.
- The reuse attempt costs one full fingerprint recomputation (hash of
  all crate sources + corpus, a few seconds) plus output hashing —
  paid again by the full path on a miss.
- The first gate after any crate byte change (every m-*/ca-* rung that
  touches Rust) pays both full executions exactly as today.
- Wall/RSS numbers on a hit are the mint run's numbers; performance
  regressions are only re-observed when a key input changes (which is
  the only way the binary can change).

## 5. Acceptance (to execute at slice PR time, canonical path)

1. Full local gate green at head (mints both artifacts).
2. Immediate re-run: `b4 conformance: receipt hit` +
   `full-corpus invariants: attestation hit`, semantic lane wall time
   recorded (expect minutes, dominated by non-target phases).
3. Canaries: touch one checker source byte → both miss (`fingerprint`
   / `controlled inputs changed: checker`); revert; tamper one CI
   output file → B4 miss (`output digest`); delete attestation →
   invariants miss (`missing`); corrupt performance artifact JSON →
   B4 miss (`invalid`).
4. `cargo xtask ci` from a copied directory → both miss (workspace).
5. Unit suites for the two reuse validators green; the walk +
   qualification registry green; hosted acceptance green.

## 6. Canonical commands

No new commands. `produce_all` and `invariants --suite all
--full-corpus` decide and print on their own. The walk driver and
`structural-preflight` are unchanged by this slice.
