# gate-tax 5 — linear-time converge + bounded recovery

Slice `ci/gate-tax-5`. Ratified 2026-08-26 (user directive: goals G1/G2
below) after the PR #475 incident; draft v1–v6 written in-session while the
#475 reconverge ran, v6 integrating two external Codex-CLI reviews the user
requested; this document is the landed packet (v6 plus the implementation
record at the end). The pin-index and prospective-planner prototypes were
built and independently verified out-of-workspace under `new-ci/`
(PR #477) and this slice consumes them as its bootstrap.

## Ratified goals (user directive 2026-08-26)

- **G1 linearity**: wall(converge + gate | change Δ) is proportional to the
  evidence truly invalidated by Δ. Every evidence node executes at most
  once per converge: one mint, plus at most ONE check-side trust-anchor
  re-observation — and zero observations when only pins moved.
  *Honest scope*: gt5 makes the OBSERVATION cost linear in Δ (the 89-min
  term) and fixes the round count at 2. The mint round itself stays
  linear in the STALE-CONE size (a Rust-byte change stales all ~55 rungs
  ≈ ~45 min with store reuse, since the h1 roots hash Rust sources; an
  mjs-only change stales only its downstream cone); true Δ-linearity of
  pin restamps (envelope/body separation, pins.lock) is the new-CI
  redesign, explicitly out of gt5 scope.
- **G2 bounded recovery**: the ≤2 h end-to-end incident bound (red
  discovered → fix → merged) is the invariant, reformulated BY CHANGE
  CLASS with p95 reporting (Codex consult #2): pin-only incident floor =
  hosted 33–35 min (local fits beside it); observation-changing incident
  floor = one ~61 min 5g observation + gate; full gate floor = longest
  invalidated node once DAG-cached (new-CI era). Granularity backing it:
  per-CASE resume for the one legitimate anchor observation (loss ≤1
  case); receipt/phase granularity everywhere else (a kill mid-sweep
  loses at most that phase, which gate-tax 3/4 receipts already cap).

## Measured incident (2026-08-26, PR #475 hosted red)

15-line printer.rs fix (EmitFlags::NO_COMMENTS pickup) after a hosted
H2.5h NEW divergence (aliasUsageInAccessorsOfClass#es5):

- crates/*.rs byte change → all 55 ladder rungs stale → chain-walk
  round 1 re-minted 55 rungs (~45 min; write-side observation-store reuse
  kept the 5g mint fast), repins rewrote pin constants in 44 oracle .mjs.
- Round 2: `H2.5g check receipt: miss (generator); running the full
  re-observation` → **89-minute** 9,027-case tsc-6.0.3 observation whose
  result is byte-independent of the Rust change. The key's generator term
  hashes raw script bytes; the walk's own repins broke it.
- Round 2 also re-minted h2-5h-a-{owner-graph,gap-matrix,dispositions,
  es2015-generators-witnesses} (4 rungs): ORDER ran owner-graph BEFORE
  comment-scope-witnesses, which it pins; the round-2 owner-graph re-mint
  cascades to its three consumers → a third full round every converge.
- Total: hosted red at +34 min; reconverge pipeline ~3 h; + local gate +
  hosted rerun ≈ **4.5 h wall for 15 lines**.

**End-to-end key proof (artifact-section diff HEAD↔worktree across the
full cascade):** `typescript`, `execution_contract`, `owner_closure`, the
`observationInputs` projection, and all 9,027 `cases`
sections/fingerprints are byte-EQUAL; the only key-relevant difference is
`generator`. **The generator term was the single blocking term: gt5-A
alone turns this incident's round-2 check into a full receipt HIT.**

**Live HIT-path validation (round 3, 13:42–13:45):** with generator bytes
stable, the check HIT the receipt and adopted all 9,027 stored
observations under the full per-case guards in **~3–4 minutes**. Two
facts for C: (1) the adoption pass REUSES the observation progress
printer, so "did 5g re-observe?" must never be detected from
progress-line presence; (2) a HIT is a guarded adoption pass, ~4 min at
9,027 cases — that constant belongs in every walk-time budget.

**Prototype evidence:** with the EXACT repin grammar, 44/44 modified
oracle scripts classified PIN-ONLY (normalized hashes equal
HEAD↔worktree), 0 LOGIC classifications, normalization idempotent on
every file. The `new-ci` substrate re-validated both independently:
pin-index M1 = 27,255 pins / 389 files / 0 unclassified oracle literals
with zero-miss incident replay, prospective plan M2 = 55/61 stale rungs
+ 6/6 surfaces on the gt4 landing, precision/recall 1.000.

## Deltas

### A. Pin-invariant generator term (kills the incident class)

Receipt schema → 2. The key's generator term becomes
`normalized_generator_sha256`: script bytes with every MANIFEST-ENUMERATED
artifact-hash pin literal replaced by a fixed placeholder, hashed; the raw
`generator_sha256` stays as an informational/provenance field but leaves
the key. Old schema-1 receipts miss as "invalid"; the first post-gt5 walk
pays one anchor re-observation, after which pin cascades never re-observe.

Mechanism (revised per external review — grammar-SUBTRACTION could
false-HIT on hash-shaped SEMANTIC constants): a **typed pin-index**
(pins.lock-lite, checked in) enumerates every classified pin site per
consumer script as {path, grammar, count}; the normalizer masks ONLY
manifest-enumerated spans (positive selection), and any grammar hit not
enumerated in the index — or an enumerated site whose current count
differs — is a REFUSAL: the receipt attempt misses, minting is skipped
with a printed refusal, and the driver preflight (`pin-index --check`)
refuses the walk until the site is classified. The index stores no hash
values and no byte offsets (those are volatile across repins); spans are
re-derived from current bytes at every use and verified against the
classification. The SAME grammar feeds repin, the topology audit, and
the prospective planner.

"Pin literal" = EXACTLY the five grammars `scripts/chain-walk-repin.py`
rewrites, extracted to `scripts/pin-grammar.py` (single source; repin
imports it and stays byte-identical in behavior) and mirrored in
`crates/oracle/pin-normalize.mjs`; a cross-check test asserts python and
mjs agree on every chain script. The path stays in the hash — retargeting
a pin is a logic change and must miss. Normalization is a pure function
of (script bytes, checked-in classification rows) — no other disk state.
Grammar evolution self-invalidates: the mjs normalizer's own bytes are a
receipt key term (`normalizer_sha256`), and the consumer's classification
rows are another (`pin_index_rows_sha256`); python-side drift is caught
by the cross-check test.

Soundness (false-HIT direction): on a receipt hit only `observeTypeScript`
is skipped; assembly, per-case guards, whole-artifact byte comparison and
every requireCondition still run with the CURRENT generator, so pin
constants (which only drive assembly-side assertions) cannot affect the
skipped computation. Residual false-MISS class (accepted, bounded): a
walk re-mint that changes a NON-hash script constant (e.g. a count const
after a manifest burn-down) still changes the normalized hash → one
anchor re-observation; self-limiting and rare. Same class, found by the
unit battery: repin's pattern-A rewrite collapses a multi-line
`"path",\n "hash"` site onto one line (pre-gt5 behavior, preserved
byte-identically), changing separator whitespace → one anchor on the
FIRST repin of such a site, none after (the collapsed form is stable).
Zero multi-line A sites exist in the corpus at landing.

Scope note: `h2-5h-qualification.mjs` and `h2-6a-qualification.mjs` carry
their own gate-tax-3 receipts with raw generator terms. Their full
re-observations are minutes-class, so they keep the raw term (the
sub-2-minute-rung non-goal); extend the normalizer there only if measured
evidence shows a real tax.

### B. Check-side per-case resume journal (bounds the anchor run)

During a full re-observation `--check`, each OBSERVING process appends
each completed case's observation record atomically to its own file under
`target/h2-5g/check-resume/<key-prefix>/` (per-shard files; the unsharded
path writes one file). Each line carries the full receipt-key identity,
the case record with its fingerprint, and its own line fingerprint (same
self-fingerprint convention and machine-local trust class as the receipt
— gate-tax 3 precedent). On re-entry with an IDENTICAL key, journaled
cases are adopted through the same per-case guards
(`storedCaseReusable`) and only the rest are observed; the journal
directory is deleted after the receipt mints, and key-divergent journal
directories are garbage-collected at full-observation start. A kill at
case N loses at most one case. Durability spec (Codex #5): single writer
per file (the observing process), append + fsync per line, torn-tail
tolerant reader (first invalid line drops the remainder of that file),
exact-set validation and full receipt-key computation happen fresh at
mint time. Because lines are per-case and adoption unions every file
under the key, the journal is shard-count independent —
`TSRS_H2_5G_CHECK_SHARDS` changes cannot invalidate it (the 2026-08-18
env-fingerprint complaint class). A journal-adopted record can never
corrupt the trusted state even under a buggy reader: adoption still
passes the per-case guards and the whole-artifact byte comparison, so
the journal library deliberately stays OUT of the receipt key.

**Keystone amendment (explicit):** gate-tax 2's keystone, as amended by
gate-tax-3.md §3, reads "observation content enters the trusted state
only through a local full re-observation". gate-tax-5 amends it again:
"…through local re-observation under a SINGLE receipt key, resumable
across process restarts — the union of partial local runs under an
identical key is one full re-observation." The write-side observation
store is untouched.

### C. Zero-observation enforcement + visibility

The 5g `--check` writes a machine-readable, self-fingerprinted OUTCOME
RECORD (`target/h2-5g/check-outcome.v1.json`: receipt attempt result,
miss term, observed/journal-adopted/receipt-adopted counts, refusals,
fresh-escape approval) — the driver never greps prose (external-review
#7; the live-confirmed stdout/stderr lines remain for humans).
chain-walk.sh reads the record after every 5g rung check, prints
`HIT` / `MISS(<term>)` per round, and tracks whether the walk itself
re-minted the 5g rung this run. Enforcement decides by MISS TERM ×
MINTED-THIS-WALK (arming on the converged-crates record alone would
false-red legitimate anchors and cold caches):

| miss term | 5g re-minted this walk | verdict |
|---|---|---|
| absent / invalid / workspace / node / platform / stored-artifact | any | cold cache or env move → observation ALLOWED, notice logged |
| normalized-generator / normalizer / pin-index / global-records / observation-content / per-case | yes | expected trust anchor → allowed ONCE, logged |
| normalized-generator / normalizer / pin-index / global-records / observation-content / per-case | no | **pin-tax regression → HARD RED naming the term** |

Additionally a per-run observation counter: the SECOND full observation
inside one walk run is a hard red regardless of term (a broken receipt
producer re-observing every round must not hide behind the cold-cache
row). The finish-train tail walk mints nothing, so any
generator/global-records miss there is red by the table.
`WALK_EXPECT_OBS=0|1` stays as a manual override (0 forces the strict
row, 1 disables enforcement for a deliberate re-anchor);
`TSRS_H2_5G_FRESH=1` (new; the packet's "--fresh" made concrete as an
env var) bypasses the receipt attempt, deletes the current-key journal,
forces the full observation, and is exempt from enforcement — both
overrides are RECORDED (outcome record + driver run summary), never
silent (external-review #7).

### D. ORDER topology (kills the third round)

- Move `h2-5h-a-comment-scope-witnesses` BEFORE `h2-5h-a-owner-graph`
  (measured: owner-graph pins comment-scope-witnesses.v1.json; witnesses
  depend only on foundation; es2015-generators-witnesses stays after
  owner-graph, which it pins). With the fix, round 1 mints every 5h-a
  rung over final inputs → round 2 is the clean round: structurally
  2 rounds. (`qualification.mjs`'s artifact list already used this
  order; nothing else pins the 5h-a sequence — verified in-slice.)
- New driver preflight audit (`scripts/walk-topology-audit.py`): build
  the artifact→producer map MECHANICALLY from each ORDER script's
  declared TARGET_RELATIVE_PATH, extract each script's referenced
  `ratchets/*` inputs, and refuse (like ORDER drift) if any referenced
  artifact's producer appears later in ORDER. Self-references exempt;
  referenced artifacts with no in-ORDER producer are reported once and
  allowed (frozen/immutable lineage).
- Repin-early mint tweak: on a rung's `--check` failure the driver now
  runs the repin BEFORE the single `--write` attempt (previously
  check→write(fail)→repin→write = up to 4 executions per stale rung).
  Repin is idempotent and only rewrites stale pin values, so running it
  unconditionally before the write is safe and trims minutes ×
  stale-rung-count from the mint round.

### E. Red-suite-first (never converge unvalidated bytes)

After a Rust fix responding to a red suite, run the failing band/fixture
on the fixed binary BEFORE the walk (the incident: local
`cargo xtask h2-5h-acceptance`, 2.5 min, validated the fix mid-walk).
Mechanism: optional `PRE_SUITE="<command>"` env the driver runs after its
fmt/clippy preflight (nonzero exit refuses the walk); plus the CLAUDE.md
walk-rule sentence landed with this slice.

### F. Pin-surface preflight + prospective plan (kills sequential tail discovery)

Measured on the gt4 landing: a Rust slice's walk discovered its pin
surfaces SEQUENTIALLY at the tail — walk1 died at the harness pin audit,
walk2 at the qualification tail (policy `rust_source_sha256`, then the
5g-profile schema promotion const), walk3 finally green: **3 walk runs
(~65 min of avoidable rounds) where one would do.** Every one of these
surfaces is checkable in SECONDS. A SIXTH pin family was found ~40 min
into that landing's gate: **fuzz manifest source references**
(`ratchets/fuzz-domain.v1.toml`, `ratchets/fuzz-preflight.v1.json`) pin
Rust source bytes and each other, validated only by the tsc_fuzz lib
tests — invisible to pin-audit, the walk, and the qualification check.

Implementation:
- `scripts/walk-preflight.py`: ONE pass reporting ALL stale pin surfaces
  at once — harness pins (pin-audit.py), hosted-policy source pins,
  schema-contract embedded {path, sha256} consts, fuzz-manifest source
  references, and the pin-index classification check. The driver runs it
  right after fmt/clippy AND at the walk tail (post-mint staleness —
  e.g. the 5g-profile schema promotion const, which is green before the
  walk and stale only after the re-mint — is then caught at walk end in
  seconds instead of a gate red 40 minutes in). The gate's
  structural-preflight WALK_DRY hook runs the same pass.
- Prospective plan (external-review #9/#11, prototype = `new-ci` M2):
  the driver preflight additionally runs the `new-ci` `plan` binary
  (base = merge-base with origin/main, head = the working tree via
  `git stash create` when dirty) and prints the predicted post-walk
  re-mint cone and every pin surface that WILL go stale after the
  re-mint. Report-only and best-effort (new-ci is a zero-dependency
  out-of-workspace crate; if it cannot build, the driver says so and
  continues) — enforcement stays with the exact current-state checks.
- One walk per converge, inside a locked transaction: a per-workspace
  lock (`target/chain-walk/lock`, PID-bound, stale-lock detected)
  refuses concurrent walks; every run gets a run-ID'd log directory
  (`target/chain-walk/runs/<run-id>/`, `latest` symlink) and the green
  tail records the run ID next to the converged-crates record — the
  certificate is that run's ID, not a fixed /tmp name.

## Slice landing cost (honest; surfaces verified 2026-08-26)

The slice edits the 5g script's logic (normalizer + journal + receipt
schema) → the 5g artifact re-mints (its `generator` field changes) and
its downstream cone (≤10 rungs) repin-cascades; upstream rungs stay
clean. The slice's own walk therefore pays a partial mint segment
(minutes) plus ONE schema-2 anchor re-observation — which lands B in the
same slice, so even that anchor is kill-resumable (the anchor run doubles
as battery item 2's live kill/resume validation). No crates/*.rs bytes
change (the h1 rungs stay clean; CI-infra slice stays mjs+scripts-only
per standing lesson).

Verified pin surfaces beyond the ladder:
- `h2-5g-qualification.schema.json`: SHAPE-only → no re-pin.
- `plans/h2-5g.v1.json`: six pins are CASE-MEMBERSHIP digests → untouched.
- `h2-5g-profile.schema.json`: the `current_exact_promotions` const
  embeds `historical_qualification` {path, hash} of the qualification
  artifact → WILL re-pin post-mint; repair recipe: assert only
  historical_qualification differs, patch the const from the artifact.
- Harness/pin-audit: no 5g-named holders; any stale Rust-side pin
  surfaces mechanically in walk-preflight/pin-audit.

## Non-goals

pins.lock extraction / pin-restamp mint mode (new-CI Phase 0 envelope
redesign — the remaining path to a Δ-linear mint round); receipts for
sub-2-minute rungs (incl. the 5h/6a receipt generator terms); hosted-side
observation caching; workspace-test receipts (gt6 candidate: package+
test-target receipts per Codex consult #2); per-case conformance receipts
(gt7 candidate); any RATCHET-ARTIFACT schema change (the receipt is
external and its own schema bumps to 2 — gate-tax 3 precedent);
concurrent-process repetitions for the 2-rep observation contract
(recorded candidate, separate decision).

## Acceptance battery

All 5g-touching runs go through the walk driver, or demoted with
`TSRS_H2_5G_CHECK_SHARDS=2` when the user is present (load-control
directive: never run the 5g check standalone at full width).

1. Key unit tests (no observation runs): normalizer equality when a
   script differs only by enumerated pin values; inequality on a 1-byte
   logic change; inequality on a pin RETARGET (path swapped, hash kept);
   refusal on an unclassified path/hash pair and on a count mismatch;
   receipt-term validation order and naming via the extracted key/term
   helpers; fresh-escape bypass.
2. Kill the anchor run mid-observation → rerun adopts journaled cases
   and observes only the remainder; the completed check reports the same
   verdict, byte-identical artifact (full case records, observation
   content included — the whole-artifact comparison is that equality),
   and mints a receipt; the journal is removed after mint; a
   key-divergent journal is ignored and GC'd. (Run live on the slice's
   own anchor observation; journal mechanics also unit-tested on
   synthetic data: torn tail, key mismatch, duplicate lines.)
3. Topology audit: synthetic inversion refuses; real ORDER passes; the
   slice walk converges with a clean round 2.
4. Enforcement table: converged tree + warm receipt → green with NO
   re-observation (receipt outcome = hit; adoption progress lines appear
   and must not trip detection); cold cache → allowed with notice;
   generator-term miss with no 5g re-mint this walk → hard red naming
   the term; anchor after a real 5g re-mint → allowed once; second
   observation in one walk → hard red. (Decision logic unit-tested via
   `walk-5g-enforce.py --self-test`; live rows exercised by the slice
   walk.)
5. Normalizer: python↔mjs agreement on all chain scripts; idempotence;
   purity (byte input + classification rows only). Grammar extraction is
   behavior-preserving for repin: a stale-pin fixture rewrites
   byte-identically before/after the pin-grammar.py extraction.
6. Full local gate at the slice head; the slice walk demonstrates the
   live post-anchor HIT.
7. Record the measured wall constants from the slice's own walk logs
   (baseline: pre-5g clean segment ≈18 min and FULL clean round ≈26 min
   at normal priority, mint round ≈45 min full-ladder demoted, 5g anchor
   89 min demoted, receipt-HIT adoption ≈3–4 min).

## Post-gt5 replay of the incident (measured constants)

probe ~3 min + build ~2 min + walk ≈60–70 min (mint round ~35–45 with
store reuse + full clean round ≈26 = pre-5g ~18 + 5g HIT adoption ~4 +
tail ~4, 2 rounds by D) + local gate ~45–60 min (gate-tax 4 receipts;
build + workspace tests dominate), hosted 35 min overlapped →
**≈1 h 50 – 2 h 10 end-to-end: meets the ≤2 h bound at the median,
hair-thin at the tail.** Incident mode = run the whole path at normal
priority. Margin levers, in order: D's repin-early tweak (in-slice), gt6
workspace-test receipts (−20–30 min on the gate — the robust-margin
lever), new-CI pin restamp (mint round → minutes). The only remaining
≥1 h single term is a REAL observation-input change — rare, deliberate,
and itself resumable per-case via B.

## Similar-cases inventory (2026-08-26 survey)

| # | case | measured cost | disposition |
|---|------|----------------|-------------|
| 1 | 5g check re-observation on pin cascade | 89 min | gt5-A (end-to-end proven on the incident) |
| 2 | third walk round from ORDER inversion (4 rungs re-minted) | +1 round ≈20–26 min | gt5-D (measured) |
| 3 | kill mid-observation loses the whole sweep | latent (2026-08-18 incident (b)) | gt5-B |
| 4 | first check after a mint re-observes | by-design trust anchor | keep; fires once, resumable via B |
| 5 | conformance ×3 + full-corpus invariants re-sweeps | ~40–90 min per gate | gate-tax 4 (landed) |
| 6 | workspace tests ~40 min regardless of change scope | 40 min/gate | partially gt4 (`cargo xtask train`); gt6 candidate |
| 7 | mint round linear in stale-cone size | ~45 min on a Rust change | inherent until new-CI pin restamp; D's repin-early trims it |
| 8 | B2 full-corpus producer single-thread regen on miss | bounded, legit | no action (post-H2.9) |
| 9 | hosted ~35 min per push, no cache, red found post-converge | 35 min + tail-chase | gt5-E prevents the tail-chase; caching = new-CI era |
| 10 | h1 rungs re-mint on any Rust byte | minutes | inherent (they scan Rust); fine |
| 11 | demoted-red perf-only rerun | ~5 min resume | already solved (journal, 6×) |
| 12 | receipt-HIT adoption pass (9,027 guarded adoptions) | ~3–4 min per clean round | inherent trust cost of gate-tax 3; incremental adoption = new-CI |
| 13 | L1 stress RSS ceiling measures the cumulative xtask process | false-red 836MB/256MB (2026-08-26) | measurement fix queued; interim = resume rerun in a lean process |
| 14 | walk-tail SEQUENTIAL pin-surface discovery | 3 walks / ~65 min avoidable | gt5-F (all-surfaces preflight + prospective plan) |

## Implementation record (landed with this slice)

- `scripts/pin-grammar.py` — the five repin grammars as a shared library
  (matches with byte spans); `scripts/chain-walk-repin.py` imports it
  (behavior byte-identical; writes now temp+rename atomic).
- `.github/ci/pin-index.v1.json` — the checked-in typed pin-index:
  per-consumer classified pin sites {path, grammar, count} (+ optional
  `semantic` exclusions, none at landing). Classification only — no
  volatile hashes/offsets. `scripts/pin-index.py --check` re-extracts
  from current bytes and refuses on any unclassified site, count drift,
  or missing enumeration; `--write` regenerates for review.
- `crates/oracle/pin-normalize.mjs` — the mjs normalizer (masks
  enumerated spans with `<extracted-pin>`; refusal on unclassified/count
  mismatch); CLI for the cross-check test.
- `crates/oracle/h2-5g-check-resume.mjs` — receipt key/term helpers +
  the per-case resume journal (append/fsync, torn-tail reader, GC).
- `crates/oracle/h2-5g-qualification.mjs` — receipt schema 2 (key terms:
  workspace, node, platform, arch, normalizer_sha256,
  pin_index_rows_sha256, normalized_generator_sha256,
  global_records_sha256, cases_observation_sha256; raw generator_sha256
  informational), outcome record, journal integration in the
  full-observation paths, `TSRS_H2_5G_FRESH` escape.
- `scripts/walk-topology-audit.py`, `scripts/walk-preflight.py`,
  `scripts/walk-5g-enforce.py` — D/F/C driver helpers, each with
  `--self-test`.
- `scripts/chain-walk.sh` — ORDER move (D), preflight additions
  (topology audit, walk-preflight, PRE_SUITE, prospective plan),
  repin-early mint loop, 5g enforcement per round, workspace lock +
  run-ID'd log directories + run-ID certificate.
- `new-ci/src/bin/plan.rs` — LADDER_ORDER kept in sync with the ORDER
  move (out-of-workspace; no ladder impact).
- `.github/ci/gate-tax-5.test.mjs` — node:test suite (registered via
  `qualification.test.mjs`): normalizer fixtures, python↔mjs cross-check
  over all chain scripts, receipt-term helpers, journal mechanics.
