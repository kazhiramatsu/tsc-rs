# H2.6c ca — close record + the H2.7a-era transition landing

Status: design-gate draft (2026-08-31); rides the `h2/6c-close` train
(one full gate at the train's final head). Closing this rung closes
H2.6c and hands off to the H2.7a era.

## 1. Identity, purpose, and boundary

`h2-6c-ca`, kind `runtime` (transition), the final rung of the
[h2-6c.md](h2-6c.md) §5 ladder. The 6c era compressed the ca-1/ca-2
analogs: the breadth machinery, runtime flip, first post-flip sweep,
acceptance wiring (`run_h2_6c`), local `h2-6c-oracle` freshness, and
hosted-boundary growth all landed with m-1/m-2 and the W1–W5 wave
trains (PRs #479–#494); the divergence surface was burned down under
sol-signed wave packets. What remains — and all this rung owns — is
the era CLOSE record against [h2-6c.md](h2-6c.md) §6 and the ONE-commit
transition landing that advances `next_slice` to H2.7a. No production
crate byte changes; no oracle observation runs; no manifest row is
added, promoted, or dropped.

## 2. Standing close evidence (the ca-1 analog, already frozen)

| artifact | content | sha256 |
| --- | --- | --- |
| `ratchets/h2-6c-census.v1.json` | the machine-frozen effective applicability census: 691 literal rows (48 explicit-false controls), 643 positive candidates over compiler 199 / conformance 32 / project 410 / transpile 2 | ffea234acbff0cbe5e85f3750cd18361d5ea2bea51cf43ca8bab36a2333726bf |
| `ratchets/h2-6c-qualification.v1.json` | 643/643 candidates double-observed fresh-process (1,286 runs), deterministic 643, facet agreement 643, 639 admitted-for-execution + 4 deferred-to-slices, 2,669 upstream writes, 978 source-map result entries | 1337e20b63d90d28cfa516de7427474adf123fbc8a5bb48a469c7c4e89ef5beb |
| `ratchets/h2-6c-known-divergences.v1.json` | the standing shrink-only manifest, 451 rows (owner `h2-6c-m-2-divergence-closure`), baseline 500 → W-waves → 451 | 135a3e638e37c22db2e9f5ac865aefe1e2b684ac7bad97f2aa4198892e8b911a |

The provisional 641/689 static figures in [h2-6c.md](h2-6c.md) §3 were
resolved by the m-1 census to 643/691; the packet's TO-VERIFY(m-1)
markers close against the census artifact, not the static sweep.

## 3. Close accounting against h2-6c.md §6

1. **Determinism and lane results (§6.1):** every candidate observed
   twice with byte-stable output (`deterministic_typescript_cases`
   = 643, `facet_agreement_cases` = 643); lane semantics were enforced
   per-wave by the path-joined census/detail instruments
   (h2-6c-w5.md §1) instead of a separately named W-H2.6C artifact.
   THIS close record is the authority amendment ratifying that
   artifact-shape substitution (the wave reviews validated the
   instruments but did not record the substitution itself); the
   qualification artifact's 643 repeated observations plus the
   runner's admitted-row exactness/determinism checks
   (h2_2c_acceptance.rs:3708-3845/:4049-4059) are its evidence.
2. **Frozen denominator (§6.2):** 691 literal / 643 positive
   candidates, per the census artifact above.
3. **Exact observations (§6.3):** 188 of 639 admitted rows are exact
   on every facet (writes incl. map JSON/inline payload, source-map
   records, callback metadata, emitted files, paths/bytes/order,
   diagnostics, BOM/newline, emitSkipped). The W5 census additionally
   proves the entire root-option surface (sourceRoot/mapRoot
   absolute/relative/URL forms) exact on js+map bytes inside the
   67-row exact-but-for-declarations set (h2-6c-w5.md §2).
4. **Later-dependency set (§6.4) — the authority-approved handoff:**
   recorded in §4 below; every non-exact row carries a named later
   owner. Zero rows are silently promoted, dropped, or counted exact.
5. **Standing queues (§6.5):** re-run facet-exact and shrink-only on
   the W5 train: h2-5h 92→64, h2-6a 26→4, h2-6b 2→0 (manifest FILE
   deleted per the absent-when-empty contract). No new divergence was
   admitted.
6. **Gates (§6.6):** W5 final head `7c4710f0` — walk one-invocation
   converge cert `20260831-054510-82911` (58 rungs, round-2 clean),
   full local gate green (T0 = 100.0000% 49024/49024, FP=0), hosted
   `gates` pass. This close train re-converges and re-gates at its own
   final head.

## 4. The residual-divergence handoff (the §6.4 record)

The 4 deferred-to-slices qualification rows (count-only, never
manifest entries): `unicodeEscapesInNames02.ts` ×2 targets → H2.9;
`transpile:jsWithInlineSourceMapBasic`/`jsWithSourceMapBasic` → H2.8c.

The 451 standing manifest rows decompose into two bands, every row
blocked on a NAMED later owner. Rows shrink only when their owner
closes; the manifest stays shrink-only under the existing runner
contract (the h2-5h/h2-6a standing-backlog precedent).

**Refusal band — 279 rows** (`emit_refused`; classified by the first
refusing option in the `validate_bootstrap_emit_request` gate order,
verified by static fixture/descriptor scan; `commonSourceDirectory`'s
outDir refusal probe-confirmed 2026-08-31):

| refusing option | rows | owner |
| --- | ---: | --- |
| `outFile` (project bundle descriptors) | 144 | H2.7d |
| `outDir` (128 relative-path project descriptors + 2 compiler embedded-tsconfig `commonSourceDirectory*` rows) | 130 | H2.8a |
| `rootDir` (project) | 4 | H2.8a |
| `isolatedModules` (`isolatedModulesSourceMap.ts`) | 1 | H2.8c |

(project 276 = 144 outFile + 128 outDir + 4 rootDir; compiler 3 =
2 outDir + 1 isolatedModules. Owner assignment: outFile/outDir/
isolatedModules per the option-owner map, h2-transition.mjs:462/:467;
rootDir is absent from that map — its H2.8a authority is the schedule
row, post-h1-completion-slices.md §4.5 H2.8a.)

**Non-refused band — 172 rows** (128 project / 39 compiler /
5 conformance), per the sol-signed W5 dispositions (h2-6c-w5.md §4):

| class | rows | owner |
| --- | ---: | --- |
| declaration-bearing: absent `.d.ts` writes (312 across the pool; incl. the frozen 67-row exact-but-for-declarations set); `.d.ts.map` facets → H2.7e; the compiler `outFile` subset also carries H2.7d bundle facets | 157 | H2.7b (+H2.7e/H2.7d facets) |
| non-declaration `outFile` bundle rows (inline-family ×6, `sourceMapWithCaseSensitiveFileNames`, `sourceMapWithNonCaseSensitiveFileNames`, `sourceMapWithMultipleFilesWithFileEndingWithInterface`) | 9 | H2.7d |
| non-declaration `outDir` relocation rows (SPECIAL-5(a) three + `jsFileCompilationWithMapFileAsJsWithOutDir`) | 4 | H2.8a |
| `sourceMapValidationVarInDownLevelGenerator.ts#es5` (the sole byte-diverging carry-over, 31 shifted lines) | 1 | H2.8b |
| `sourceMapValidationDestructuringForArrayBindingPattern.ts#es2015` (missing TS2318; joined to the standing h2-6a owner `h2-6a-r3-destructuring-binding-ranges` — both manifests shrink together) | 1 | h2-6a queue |

(Suite split of the 157 declaration-bearing rows, round-1 recount:
compiler 24 / conformance 5 / project 128.)

W5's census proves the js/map facets of the declaration-bearing rows
exact where matched writes exist; the H2.7d bundle facets are
COUNT-ONLY and unproven until H2.7d (no matched write paths). The
H2.7a era itself unblocks NOTHING in this table (foundation only);
the first shrink arrives with H2.7b's non-bundle `.d.ts` output.

## 5. The transition landing (ONE commit, LAST in the train)

`crates/oracle/h2-5g-profile.mjs` transition block +
`.github/ci/contracts/h2-5g-profile.schema.json` consts +
`crates/oracle/h2-5h-a-foundation.mjs` parent-profile content
assertions (the d4e7e875 surface) +
`.github/ci/contracts/h2-5h-a-foundation.schema.json`'s two numeric
parent pins (`parent_completed_runtime_slices` const/min/max 24 → 25;
the `$defs` CLOSED_THROUGH_H2_5G 22-slice list is frozen history and
stays), in one commit:

- `active_runtime_slices` += `"H2.6c"` (runtime admission landed at
  the m-2 declaration-unit flip, execute.rs:752/activity.rs:481);
  `inactive_runtime_slice_count` 12 → 11;
  `completed_runtime_slices` 24 → 25.
- New `h2_6c_*` adoption fields mirroring the `h2_6b_*` block:
  candidate 643 / admitted 639 / exact 188 / known_divergences 451 /
  source_deferred 4.
- `next_slice`: `"H2.7a"`; `next_slice_scope`:
  `"declaration-owner-inventory-and-dormant-foundation"` (schedule
  §H2.7a: owner inventory + declaration transform/printer foundation,
  no output activation).
- `next_runtime_activation_slice`: `"H2.7b"` — a deliberate first
  divergence from `next_slice`: H2.7a activates no runtime output
  (dormant-foundation, the H2.5h-b B-1..B-4 pattern); the next
  admission-profile change is H2.7b's non-bundle `.d.ts` output.
- Schema consts updated to every changed value; hash-bearing consts
  re-minted by the walk, never hand-edited.
- `crates/oracle/h2-transition.mjs` and downstream pins: chain-walk
  hash re-mint only.

## 6. Close markers

The slices index README gains the 6c-era rows (slice-opening packet,
m-2 flip, W1–W5 waves, this close: H2.6c CLOSED, evidence = the §2
artifacts + the final-head gate record). h2-6c.md stays historical
per the close precedent (b94bfde9 touched only the index): its §8
item 5 ("exact next transition value") is resolved BY this packet's
§5, not by editing the parent. No STAGE change (mid-H2 slice). The
H2.7a-era opening packet (`h2-7a.md`) boards the same train as a
design-only passenger under its own design gate.

## 7. Prohibitions

- No production-crate byte changes; no oracle observation, band
  re-observation, or qualification re-run in this rung.
- No manifest row added, dropped, promoted, or re-owned; the §4 table
  is a RECORD of standing dispositions, not a re-adjudication.
- No hand-derived schema consts; walk-owned hashes re-mint only
  through the walk.
- The hosted boundary stays fixed and unsplit; no acceptance change
  of any kind (run_h2_6c is already wired and green).
- The 6c census/qualification/manifest artifacts are frozen inputs;
  the close changes none of their bytes.

## 8. Acceptance

fmt/clippy green; `bash scripts/chain-walk.sh` ONE-invocation
converge at the train head (the mjs edits re-mint the 5g profile and
its downstream hash cone; zero 5g re-observation expected — pin-only
cascade); full local gate `cargo xtask ci --baseline 7c4710f0` green
at the final head; hosted `gates` green on the PR; merge closes
H2.6c and opens the H2.7a era.

## 9. Cross-review record

Round 1: operator draft (with two pre-verdict self-audit amendments:
the four-file landing surface and the refusal-table outDir recount)
→ sol **AGREE, zero blocking findings, 7 advisories** (2026-08-31):
close accounting independently recomputed exact (1); the corrected
refusal table re-derived exactly, with the rootDir-owner citation
moved to the schedule row (2 — incorporated); the non-refused
decomposition confirmed without remainder, incl. the 312 expected
`.d.ts` writes and the 157-row suite split compiler 24 / conformance
5 / project 128 (3 — split recorded); the transition values verified
against the 37-slice lattice and the `next_runtime_activation_slice
= "H2.7b"` divergence endorsed as sound (4); the four-file direct
pin surface confirmed complete by repository-wide consumer search
(5); the 451-row standing handoff confirmed authorized under
h2-6c.md §6.4 and the 5h/6a precedents, with W5's 481→451 shrink
independently re-verified as removal-only (6); the W-H2.6C
substitution provenance clarified — this close record itself is the
authority amendment (7 — incorporated in §3.1).
