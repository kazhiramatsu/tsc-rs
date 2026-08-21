# gate-tax 3 — 5g check-side receipt

CI slice (gate-tax 1/2 lineage: PR #448/#450/#458), branch
`ci/gate-tax-3`, trusted base `6ee3de18` (main after the CS-6 merge).
Ratified 2026-08-21 as its own slice after CS-6 and before B-1. Goal:
the gate's `h2-5g-oracle` phase re-runs on every train because its
`NodeRuntimeOracle` input scope includes `ratchets/`, and every train's
walk rewrites ratchet pin lines; the phase then pays the full 9,027-case
(18,054-observation) TypeScript re-observation, ~10–40 min per train,
even though no observation-relevant byte changed. This slice makes the
qualification `--check` skip the re-observation — and only the
re-observation — when a machine-local receipt proves this exact
observation state was already fully re-observed here.

## 0. Relation to gate-tax 2's adjudication

`gate-tax-2.md` dropped "check-side adoption" after review and wrote
"do not resurrect without a new review"; its §4 records why: check-side
**adoption** — trusting stored observations whenever inputs match, with
no proof anchor — would delete the keystone that makes the loose
write-side reuse key sound. The same §4 names the proper fix: "the
post-H2.9 evidence-DAG sub-node receipts."

This slice is that fix pulled forward, ratified 2026-08-21 as the new
review gate-tax 2 required. It is a different mechanism in kind:

- **Dropped (adoption):** stored observation bytes are trusted because
  their recorded inputs match the disk. Nothing proves the recording
  itself; a fabricated-but-internally-consistent artifact passes
  forever.
- **Landed (receipt):** stored observation bytes are trusted only
  while they are byte-identical to a set that a **local full
  re-observation `--check` proved**, at the same workspace path, on
  the same runtime, under the same generator and library bytes. The
  proof is a machine-local receipt minted exclusively by that full
  check; one observation-relevant byte of drift and the check falls
  back to today's full re-observation.

## 1. Mechanism

`crates/oracle/h2-5g-qualification.mjs` only; no xtask, schema,
contract, or registry changes. The gate's phase structure and every
other mode (`--write`, `--preflight`, shard child, upgrade/rebind) are
byte-for-byte in behavior.

- **Receipt file** `target/h2-5g/check-receipt.v1.json`: machine-local
  (`/target/` is gitignored), self-fingerprinted
  (`receipt_fingerprint_sha256`), written atomically (same-directory
  deterministic temp + rename, the gate-tax 2 house pattern).
- **Minted only** by a green full-re-observation `--check` (serial, or
  the sharded parent after its whole-artifact byte comparison). Never
  by `--write` (fresh write output has not been check-verified), never
  by a shard child, never on a receipt hit (nothing changed).
- **Consulted only** by `--check`, before any observation and before
  any shard child is spawned. The decision is printed either way.
- **On hit**: the stored case records are adopted through the
  unchanged write-side per-case guards (`storedCaseReusable`, every
  case re-verified against current disk bytes), then the unchanged
  assembly runs — every `requireCondition`, the summary/contract
  consts, the full envelope re-derivation — and the rendered artifact
  is byte-compared against the stored file exactly as today. Only the
  `observeTypeScript` runs (2 × 9,027) are skipped.
- **On any miss** — receipt absent/invalid, workspace path, node
  version, generator bytes, library inventory, global observation
  records, observation-content roll, or any single case whose stored
  record no longer matches the disk — the attempt aborts before any
  observation and the check runs today's full path unchanged
  (fail-closed, all-or-nothing), minting a fresh receipt on success.

## 2. Receipt key (all terms must match)

1. **workspace realpath** — 5g observations embed absolute vendored
   lib paths in diagnostics (the known oracle path leak), so stored
   observations are only proven for the path they were observed at. A
   copied repo or worktree misses and keeps today's red instead of
   masking it.
2. **node `process.version`** — plus `validateRuntime()`'s pins,
   which run on every invocation regardless.
3. **generator bytes** (`pathHash` of the mjs itself) — the check
   logic is part of what the mint proved. The write-side key
   deliberately has no own-generator term because the per-gate full
   proof backstopped it (gate-tax 2 §2); the receipt carries that
   burden now, so the receipt includes the generator.
4. **vendored library inventory digest** (`lib.*.d.ts`, name+bytes
   roll) — the observations resolve default libraries from disk
   through the real compiler host; those bytes are not covered by the
   bundle/implementation hashes (gate-tax 2 R3-2, applied to 5g).
5. **global observation records**, canonical:
   `{typescript, observationInputs(inputs), execution_contract,
   owner_closure}` — the same projection as the write-side key.
   `owner_inventory` and `global_candidate_dispositions` raw bytes are
   excluded for the same reviewed reason: they are pin-carrying
   ratchet artifacts whose observation-relevant projections (the owner
   closure rows; the candidate selection) are compared exactly, the
   selection through the per-case identity and the 9,027-count
   enumeration guards that re-run from current disk on every path.
6. **observation-content roll** `cases_observation_sha256` — sha256
   over the sorted per-case `case_fingerprint_sha256` list of the
   artifact the mint verified. Binds the exact observation bytes that
   were proven; a forged artifact with recomputed, internally-valid
   case fingerprints cannot hit.

**Deliberately outside the key** (each would only add false misses):
the contract schema and `h2-5f-profile` bytes (assembly/envelope-side;
their consts and pins are re-checked by the guards that run on every
path, and the whole envelope is re-derived and byte-compared), the
artifact's own envelope sections (same), and
`TSRS_H2_5G_CHECK_SHARDS` (execution shape only; the 4-shard slice's
round-robin + whole-artifact byte-compare equivalence argument, and
the receipt path performs no observation at all).

## 3. The keystone, amended

- gate-tax 2 §2 licensed the loose write-side reuse key with: unsound
  reuse cannot survive the **next gate run** (per-gate full proof).
- After this slice: observation content enters the trusted state
  **only through a local full re-observation**, and carries forward
  only while every observation-relevant byte (§2) is unchanged at the
  same path on the same runtime. The write key's two deliberate
  loosenesses are now carried by receipt terms instead of by
  re-observation frequency: no-own-generator-term → key term 3;
  no-lib-term → key term 4.

Updated asymmetry table (supersedes the 5g row of gate-tax 2 §2):

| | adoption key | full re-observation backstop |
|---|---|---|
| `h2-5g-qualification` | loose (write) / receipt-anchored (check) | every check at which any §2 key term differs from the last locally-proven state |
| H2.5h-a witnesses/foundation | strict (own generator sha + typescript record + lib digest) | once per slice (packet checker) |

## 4. Failure modes, adjudicated

- **Forged artifact, valid per-case fingerprints**: observation-content
  roll (key 6) misses → full re-observation → today's stale red.
- **Generator / lib / vendor / fixture / contract-projection edit**:
  key 3/4/5 or a per-case guard misses → full path. (This train's own
  generator edit demonstrates the generator term live.)
- **Artifact envelope hand-edit, observations intact**: receipt hits,
  adoption succeeds, and the unchanged whole-artifact byte comparison
  fails with today's exact stale message — a hit still verifies
  everything except the TypeScript runs themselves.
- **Repo copied / worktree**: workspace term misses; the oracle
  path-leak red is preserved, not masked.
- **Receipt tampered or torn**: self-fingerprint fails → treated as
  absent → full path. Kill mid-check writes nothing (mint is the last
  step); kill mid-mint leaves the previous receipt or none (atomic
  rename).
- **Receipt fabricated wholesale**: requires local write access to
  `target/` — the same trust boundary as the resume journal and every
  `target/` evidence artifact; an actor with that access already owns
  the gate binary. Machine-local by design; a fresh clone pays one
  full check.
- **Concurrent minters**: deterministic temp name is single-writer by
  walk/gate discipline (gate-tax 2 R-impl-1); a violation is
  fail-noisy (rename race), never a torn receipt.
- **Candidate-set drift** (dispositions/classification change): the
  enumeration and its 9,027-count and distribution guards re-run from
  current disk on every path; a changed set misses per-case or fails
  the guards before any adoption is trusted.

## 5. Accepted, documented costs

- All-or-nothing: one stale case pays the full 9,027 re-observation.
  Per-case partial re-observation stays deferred to the post-H2.9
  evidence-DAG sub-node receipts, where the receipt store and the
  observation store unify.
- The first `--check` after any fresh `--write` observation pays full
  (fresh writes never mint — the check is still the only prover).
- The receipt attempt costs the enumeration + per-case hashing +
  57.6 MB render/compare (~seconds to low minutes), paid again by the
  full path on a miss. Negligible against the ~10–40 min it replaces.
- `local_ci_resume.rs`'s `NodeRuntimeOracle` phase receipt is
  unchanged: the *phase* still re-runs per train; the qualification
  step inside it becomes cheap on hit. Owner-controls/profile checks
  keep their cost.

## 6. Acceptance (executed, recorded in the PR)

1. Full sharded `--check` green → receipt minted (the train's walk
   step 9 is the mint run).
2. Immediate `--check` again → `check_receipt=hit
   reused_observations=9027`, byte-green, measured wall time.
3. Decision canaries, kill-after-decision (safe: a check writes
   nothing before mint): generator perturbation → `miss (generator)`
   and full observation starts; artifact observation tamper with a
   recomputed case fingerprint → `miss (observation-content)`; receipt
   `workspace` edit → `miss (workspace)`; receipt corruption →
   `miss (invalid)`.
4. Walk verify battery + qualification registry green.
5. Full local gate green at the final head; the gate's own
   `h2-5g-oracle` phase records which receipt path it took.

## 7. Canonical commands

The chain walk is unchanged (qualification-before-profile order and
all gate-tax 2 rules apply). The receipt needs no new commands:
`--check` decides and prints on its own. Canary snippets live with the
acceptance evidence in the PR body, not in `scripts/` (gate-tax 2 §3
scope rule).
