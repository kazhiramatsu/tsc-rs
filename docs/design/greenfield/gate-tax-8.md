# gate-tax 8 — one-walk converge + shadow restamp foundation

Status: ratified for implementation 2026-08-30 (user directive: sol
cross-review to agreement, then implement; calibration directive same
day: operator judgment has standing over non-gating scope). This
document consolidates draft v3–v8 (session scratchpad) after EIGHT
rounds of adversarial cross-review by Codex gpt-5.6-sol at max
reasoning (read-only repository access, findings verified against
sources). Round-8 record: "No new defect was found in Stage-1 trusted
behavior or the core one-walk mechanism." The two residual round-8
blockers are dispositioned in §9 (one adopted, one phased) under the
calibration directive.

## 1. Problem and measured baseline (2026-08-30, W4 train)

One emitter-byte train (~450 changed lines) paid ~143 min of walk
activity plus the ~53 min gate:

| phase | wall | attribution (corrected in review round 1) |
|---|---|---|
| walk #1 (16:02→17:14) | ~72 min | 57/65 rungs re-minted: 23 direct Rust-scanning roots (22 profile generators + gap-matrix, per the measured plan classification) + their transitive artifact-hash pin cone; 2 full 65-script `--check` sweeps |
| manual inter-walk repair | ~2 min | (i) `pin-audit --fix` on harness .rs raw-hash literals — closure-EXCLUDED, cascades nothing; (ii) an ad-hoc rewrite of the 5× h2-1a-qualification sha256 constants inside `.github/ci/contracts/h2-5g-profile.schema.json` — the real walk-#2 root |
| walk #2 (17:25→18:34) | ~69 min | the schema-const→contract-pin→h2-5g-profile cone (7 rungs, matching the run-2 plan) + 2 more full sweeps |

Walk #2 exists for two independent reasons; BOTH must die for a
one-walk converge:
1. the schema-constant repair happens outside the walk transaction
   (killed by S2 below);
2. the tail refuses on stale harness pin literals BEFORE writing the
   convergence certificate (chain-walk.sh:326) — walk #1 ended CLEAN
   without a certificate; only the post-repair walk #2 certified
   (killed by S3 below).

## 2. Goals

- G1: Rust-byte train converge = ONE walk invocation that writes the
  convergence certificate. Target ≈ walk #1's floor (~72–78 min: the
  23 Rust-scanning roots, the transitive cone, both full sweeps, plus
  sub-minute new overheads); measured at landing vs the 143-min
  baseline. No claim below that floor.
- G2: prospective planner covers ORDER 65/65 (today 63/65 — its
  ladder stops at h2-6b); the driver refuses on planner/ORDER
  coverage drift.
- G3: a report-only restamp shadow with a reviewed decision model
  runs per walk, maturing the model for a LATER, separately ratified
  Stage-2 promotion. Zero trust changes now: `--check` staleness
  detection, the full final 65-sweep, and the complete 5g gt5
  receipt/outcome/enforcement path are untouched.
- G4: every new mechanism is typed, atomic, journaled, and
  crash-recoverable; no new escape hatches.

## 3. S1 — driver recovery phase

Insertion: immediately after lock acquisition + run-directory
creation, BEFORE the fmt/clippy preflight and any PRE_SUITE command.
The recoverable class is closed and enumerated — membership rule:
"the repaired value is a pure function of current on-disk artifacts":
(a) the five S2 schema-const leaves; (b) every S3 manifest `values`
row. Nothing else — all other pin surfaces keep strict-refusal
semantics. Descriptor-section anomalies (S3) are NEVER recoverable.

Procedure per stale member: apply the same typed repair the walk
itself uses; write-ahead journaling (INTENT row → atomic rename →
COMPLETION row; startup reconciliation re-derives on a dangling
INTENT — idempotent by construction); then rerun the FULL strict
preflight; remaining staleness refuses exactly as today. WALK_DRY is
check-only: it reports what recovery would repair, never writes.

## 4. S2 — schema-constant repin inside the walk

Target, positively enumerated: the five leaves
`/properties/current_exact_promotions/const/{0..4}/historical_qualification/sha256`
of `.github/ci/contracts/h2-5g-profile.schema.json` (all carry the
h2-1a-qualification artifact sha256; verified against schema bytes
and the five promotion-row case identities). Extending coverage to
any other schema constant is a NEW pin-grammar entry with its own
enumerated pointers, landed by the slice introducing it (the ORDER
extension rule applied to grammar).

Procedure (typed entry in scripts/pin-grammar.py; executor in the
chain-walk-repin.py family): parse; assert schema identity/version,
the five case identities, pointer paths, count==5; compute the new
value from the just-minted h2-1a-qualification artifact; apply at the
five pointers; reparse; prove the byte-diff is confined to the five
value spans (pointer-masked compare); atomic same-directory
temp+rename; journal row (pointers, old/new, pre/post file digests).
Any assert failure refuses the walk; no partial write persists.

Driver point: immediately after the h2-1a-qualification rung's write,
before any rung pinning the contract hash — the gt5-D repin-early
discipline extended from generator-source pins to this enumerated
schema-const pin. The induced cone (measured: 7 rungs) mints in
round 1 of the same walk. Crash recovery: §3 (kill window exercised
by acceptance).

## 5. S3 — harness pin manifest (the certificate killer)

Convert the volatile raw hash literals in the harness integration
tests (the pin-audit AUDITED class: `sha256(RECORDED)` self-hash
literals and `assert_recorded_exact(...)` expected hashes) into reads
from ONE committed manifest, `ratchets/pins/harness-expected.v1.json`,
embedded via `include_bytes!` of the MANIFEST — the .rs source bytes
then never change when artifacts re-mint, so the tail has nothing to
refuse and certifies in the same invocation.

- **Two sections.** `descriptors` (reviewed structural authority —
  writers never touch a byte): rows
  `{test_file, check_id, kind: "self-hash"|"recorded-exact", artifact_path, pointer?}`,
  identity `(test_file, check_id)` unique; this section IS the
  machine-readable equivalence table (every removed literal → one
  row + the retained in-test structural guarantee, enumerated in the
  implementation record). `values`: `{test_file, check_id, sha256}` —
  the only writable section, span-confined to sha256 value leaves
  with a pre/post byte-equality assert on `descriptors`.
- **Frozen dual anchor.** sha256(descriptors section) is frozen in
  (1) a typed `.github/ci/pin-index.v1.json` row and (2) a stable
  const in the harness test that includes the manifest. Both are
  structural anchors (change only on reviewed descriptor-set changes,
  never per-mint). Verifier check (0) validates the anchor BEFORE any
  other check and before any recovery write; a mismatch is never
  recoverable.
- **Verifier** (read-only; via walk-preflight.py at startup AND in
  the strict tail before the certificate): (0) anchor; (1) descriptor
  identity uniqueness; (2) values↔descriptors bijection (detects
  deleted/extra/duplicate rows against the authority); (3) every
  value re-derives from the on-disk artifact (`self-hash` =
  sha256(artifact bytes); `recorded-exact` = value at `pointer`).
  Failure → strict-preflight failure at startup (recoverable only
  when confined to `values` sha256 leaves) / certificate refusal at
  tail.
- **Refresh point**: after the FINAL minting round (later rounds may
  re-mint), before the tail; typed, atomic, journaled (§3 WAL).
- **Residue**: `h2_baseline.rs` stays UNCONVERTED — its hashes pin
  the frozen approved-runner baseline artifact (immutable by design);
  it is an enumerated permanent residue row of the equivalence table
  with today's pin-audit semantics.
- **pin-audit inversion**: for converted files pin-audit becomes a
  prohibition (refuses if a raw artifact-hash literal reappears);
  unconverted residue keeps today's behavior.
- **Closure constraint**: the manifest path must be OUTSIDE every
  profile runtime closure (the harness test files already are, per
  the h2-5g-profile exclusion list); a canary asserts this for the
  manifest AND every converted .rs file.
- All structural asserts (schema/kind/internal-consistency/
  self-fingerprint validation) remain in the .rs tests.

## 6. S4 — planner completeness + drift refusal

Add the missing H2.6c rows to `new-ci/src/bin/plan.rs` (its ladder
stops at h2-6b-qualification; report-only tooling, in scope); add a
mechanical planner↔ORDER coverage check to driver preflight that
refuses on drift (mirror of the ORDER↔crates/oracle self-check). The
slice extending ORDER extends the planner in the same commit
(WALK_DRY-verifiable). new-ci/ is outside the root workspace and the
crates/ pin scope — no ladder cascade.

## 7. S5 — restamp shadow (report-only)

**Stage-1 pragmatic scope** (this train): the machinery that matures
the decision model —
- selector manifest (typed, versioned): one row per PRODUCER OUTPUT
  `{producer, output_path, schema_guard: {id, version}, leaves: [{pointer, type, cardinality, class: semantic|replaceable-hash, recipe?, order}]}`
  plus a per-producer `consumes` section binding every input edge to
  its consumed projection (`whole-file` | `whole-payload` |
  [RFC-6901 pointers]). Selector language: RFC 6901 + at most one
  `/*` array-wildcard segment with a declared element shape; anything
  unaddressable is `semantic`. `/schema`, schema versions, and input
  path strings are always `semantic`. Producers declare complete
  output sets (h2-transition: 3, h1-emit-oracle: 2).
- recipe vocabulary v1 (closed): `identity-copy`, `sha256-of-file`,
  `sha256-of-canonical-json` (over the NEW canonical-json.v1 encoder,
  §7a), `self-sha256-after` (model-level member exclusion via JSON
  pointers, order-last). Missing/out-of-vocabulary derivations, and
  any construction not expressible as a value-span replacement on the
  pre-capture bytes, are typed abstains.
- per-event capture: a MINT EVENT is one rung write invocation
  (producer × round) over its complete written output set; the
  driver captures `runs/<id>/pre/<round>/<output>` before and
  `runs/<id>/post/<round>/<output>` after each write (post-capture
  adopted from review round 8 — later rounds overwrite minted bytes,
  and without the post copy an earlier event's ground truth is
  unrecoverable).
- truth table (per event, evaluated after the walk from journal +
  captures): predict SKIP iff (1) generator+contract diffs are
  pin-literal-only (gt5 normalizer); (2) every changed input leaf is
  `replaceable-hash` with its new value equal to the recipe-derived
  value from the upstream output of this walk; (3) every touched edge
  is declared; (4) no open undeclared-read flag; (5) 1–4 hold for
  every output of the event. Anything else → EXECUTE; any refused
  construction → event UNKNOWN. Construction is attempted regardless
  of prediction; classification: SKIP+match=true-skip,
  SKIP+mismatch=precision-miss, EXECUTE+match=false-negative,
  EXECUTE+mismatch=true-execute, refusal=UNKNOWN (excluded from both
  metrics, reported as coverage). Prediction inputs never include the
  event's own post-capture bytes.
- undeclared-read inventory: seeded with the review survey
  (h2-5g-profile: `git diff` + untracked enumeration; h2-3c:
  `.node-version`/`process.version`), extended mechanically
  (fs/child_process/process.env grep) and by review; flagged
  producers are permanent abstains until declared.
- per-run shadow report (journal + summary line): predictions,
  outcomes, abstain reasons, coverage by event count and by measured
  event cost (wall timestamps from the driver's new per-phase journal
  rows — advisory weight only in Stage 1), and a basic model digest
  (sha256 over the framed selector manifest + inventory + shadow
  implementation files).

### 7a. canonical-json.v1 (normative)

UTF-8; object members sorted by key code-point order; array order
preserved; no insignificant whitespace; strings minimally escaped
(`\"` `\\` `\b` `\f` `\n` `\r` `\t`, other control chars as lowercase
`\u00xx`, no `\/`, non-ASCII as raw UTF-8; keys and values alike);
numbers: integer lexical scope only (optional `-`, no leading zeros,
magnitude ≤ 2^53−1). NORMATIVE REJECTIONS (subtree → typed abstain,
golden-tested identically in the py and mjs implementations): any
non-integer/out-of-range number, `-0`, duplicate object member names
(parsed-name equality, incl. the escaped-equivalent case), lone
surrogates. Canonicalization operates on the parsed model
(self-fingerprint exclusion = model-level member removal); output
edits remain hash-string span replacements. Producers canonicalize
with local implementations: mismatches surface as construction
mismatches and reclassify the leaf abstain `encoder-mismatch` —
coverage shrinks honestly; encoders are never adjusted outside a
versioned revision.

**Window-opening scope** (spec frozen here; lands with the Stage-2
window, NOT this train — see §9): the promotion-grade evidence
protocol — single-process `time.monotonic_ns()` tick helper with
duration validation and invalid-train window-break; model snapshot
EXECUTION (shadow AND acquisition orchestration run from the frozen
run-dir snapshot, tail re-hash, mutation canary); the framed
byte-inventory model digest over the full transitive implementation
+ interpreter versions (octet-count length framing, newline-strip
rule); hermetic runtime binding; window rules (≥5 consecutive valid
trains counted only post-hermetic, ≥1 predicted SKIP per train, ≥1
whole-window true payload-change EXECUTE, precision=1.000,
recall≥0.98, coverage ≥60% events / ≥50% event cost, all under one
digest; any digest change or invalid train breaks the window).

## 8. Stage-2 preconditions (trusted restamp — a later, separate slice)

All IMPLEMENTED AND ENFORCED before any narrowed confirmation sweep
or trusted skip: (1) the window-opening evidence protocol of §7 with
a green window; (2) complete per-generator read manifests
(directory-negative + ambient inputs) + a final-snapshot/TOCTOU
fence; (3) a cheap pre-execution stale detector; (4) typed artifact
materializer + trusted cross-run snapshot store + multi-output
atomicity + fingerprint recomputation + side-effect policy;
(5) hermetic runtime binding (the countable window starts at its
digest reset); (6) a normative roadmap amendment ratified by the
user (gt5 reserves restamp for new-CI; the pre-H2.9 pull-forward
list permits report-only adapters — Stage 1's shadow is within it,
Stage 2 is not and says so). Stage 2 may well BE new-CI Phase 0
substrate work; that decision belongs to the amendment.

## 9. Cross-review record and dispositions

Eight rounds, Codex gpt-5.6-sol, max reasoning, read-only repo
access, 2026-08-30 (scratchpad: gate-tax-8-draft-v1..v8 +
gt8-review-r1..r8.log). Round 1 (9 blocking) rebuilt the incident
attribution and killed v1's unsound certificate; rounds 2–3 forced
the two-stage split, S3's promotion to core, and the five-pointer
enumeration; rounds 4–6 hardened recovery placement, the descriptor
authority (dual anchor), encoder semantics, and metric definitions;
rounds 7–8 pinned the remaining evidence-protocol details. Round 8:
no defect in Stage-1 trusted behavior or the one-walk mechanism;
residual blockers dispositioned under the user calibration directive
(2026-08-30: "sol はオーバーエンジニアリング気味 — operator 判断も尊重"):

- r8 #2 (post-write capture) — ADOPTED into Stage 1 (§7): cheap, and
  without it even model-maturation ground truth is corrupted.
- r8 #4 (snapshot-execution of acquisition orchestration) — PHASED to
  the window-opening rung (§7): it protects evidence that, by the
  sol-accepted §7/§8 structure, is not countable until the
  post-hermetic window exists. Spec frozen verbatim.

## 10. Honest arithmetic and bootstrap

Steady state: one certifying invocation ≈72–78 min vs 143
(walk-#2 and the manual repair eliminated; the 23 roots and both
full sweeps remain; new overheads sub-minute except capture I/O).
Bootstrap: the S3 .rs conversion commits BEFORE this train's own
walk, so its tail should have nothing to repair and certify in one
invocation; honest fallback — an unforeseen stale surface at that
tail pays today's two-invocation protocol one final time; the landing
record reports which occurred.

## 11. Acceptance battery

1. WALK_DRY: ORDER 65/65, planner 65/65, topology, planner-drift
   refusal canary.
2. S3: per-hash equivalence table complete; refresh idempotent +
   atomic (kill mid-refresh → §3 reconciliation converges); frozen-
   anchor tamper canary (descriptor edit without anchor update →
   refused at startup AND tail); deleted-value-row bijection canary;
   closure-exclusion canary (manifest + every converted file);
   prohibition canary (reintroduced literal → pin-audit refuses);
   harness suite green against a re-minted artifact with only a
   `values` update (no .rs diff).
3. S2: green-schema → re-mint h2-1a-qualification in a worktree →
   five leaves repinned in-round, cone mints same-walk, journal
   complete; kill between the h2-1a write and the schema repin →
   restart → recovery repairs → converge, final bytes identical to
   an un-killed run; count/shape-mismatch refusal; non-pin schema
   edit untouched by repin.
4. S5: shadow report present on the replay walk; truth-table paths
   exercised (agreement; synthetic payload-change → EXECUTE;
   missing-recipe → UNKNOWN; class-flip-with-recipe-retained →
   false-negative); pre/post captures present per event; encoder
   golden set incl. the four rejection domains, byte-identical
   verdicts from the py and mjs implementations.
5. End-to-end: replay the W4 cascade class (emitter .rs comment byte
   in a throwaway worktree) → ONE invocation, certificate written;
   wall-clock + rung table recorded vs the 2026-08-30 baseline runs.
6. 5g parity: receipts/outcome records byte-equivalent to a
   pre-slice walk on the same state; full final sweep present.
7. Full local gate green at the train head; hosted acceptance green.

## 12. Landing shape

Branch `ci/gate-tax-8`, rungs in order: (1) this packet; (2) S4
planner + drift check; (3) S2 grammar/repin/driver + canaries;
(4) §3 recovery phase + WAL; (5) S3 manifest infra + harness
conversion + pin-audit inversion + anchors; (6) S5 pragmatic shadow +
captures + report; (7) CLAUDE.md quick-ref + gt5 row-7 disposition
note + memory. Rungs verify with targeted tests (gate-tax-5.test.mjs
pattern); ONE full walk (Rust train — S3 touches harness .rs) + the
complete local gate at the head; PR opens early per standing
workflow.
