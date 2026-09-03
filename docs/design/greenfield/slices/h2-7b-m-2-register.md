# h2-7b-m-2 — Phase-E/P register (values recorded as minted; `__X__` = TO-FILL at the named step)

## Design gate
- Packet rev 10 RATIFIED 2026-09-04 (sol r1 REVISE 15+1 → r2 REVISE 7+1 → r3 REVISE 4+2 → r4 REVISE 3 → r5 REVISE 4+1 → r6 REVISE 1+1 → r7 REVISE 2+2 → r8 REVISE 2 → r9 REVISE 1 → r10 AGREE). Authority file: `docs/design/greenfield/slices/h2-7b-m-2.md`.
- Frozen order: repair (PR #502 @a67fdd37) → m-1 (PR #503 @`440e6073`) → this rung → the closure waves → ca. Trusted base for the train: `440e6073`.
- Parent restatement landed with the packet (h2-7b.md §6.2 / §6.4 / §8: the hosted boundary grows by exactly one call at m-2).

## Opening commit (pre-flip) — TO-FILL
- `--pre-flip` census: typed refusals `__` (expect 111) / executed `__` (expect 1,450) / zero-member controls `__` (expect 6) / deferred 26; activity 0 on every successful outcome; the structural canary test: `__`
- the 6c pre-flip population (§6.4c1): 451 rows, coarse facets byte-identical `__`; `refused_option` map totals outFile 171 / declarationMap 6 / outDir 130 / rootDir 4 / isolatedModules 1: `__`
- guard test `hosted_acceptance_and_oracle_phases_cover_h2_2c_rungs` green with `run_h2_7b` inside `fn acceptance`: `__`; policy pins (main.rs, h2_2c_acceptance.rs): `__`

## Flip commit — TO-FILL
- LIFTED controls re-pinned (the §5.11 list): `__`; RETAINED controls untouched: `__`
- focused tests (the §9.2 list incl. the pre-pass branches, the length-1 contract, the collision matrix, the printer activation, the unavailable-oracle fault injection, the mixed listing control, the failing-sink control, the fallback order, the single-admission control): `__`
- L2 production replay 0 / 0: `__`; ledger check: `__`
- the four printer-field dispositions (`module`, `moduleResolution`, `inlineSourceMap`, `extendedDiagnostics`) with grep evidence: `__`

## First sweep / cross-slice — TO-FILL
- exact `__` / diverging `__` (manifest rows `__`; owner `h2-7b-m-2-divergence-closure`); refusals 0: `__`; the 29 transform-blocked + 5 collision rows exact: `__`; activity 2,445 per repetition: `__`
- 6c regeneration: in-band shrink `__` (≤ 133), the 133-row activation refinement `__`, the 33 registered refusal transitions (`case_id` list): `__`; totals 639 / 4 / 643: `__`; 293 / 0 activity: `__`
- 5h / 6a / 6b shrink-only: `__`; corpus ratchet T0 100% FP=0: `__`
- inventory re-mint (seven anchored H2.7b rows; the two `markLinkedReferences` rows → H2.8c): `__`; close artifact `lifted-at-H2.7b` ×4 with `retained_arms`: `__`
- resources: local sweep wall `__` (STOP 15 min), per-worker RSS `__` (4 GB), aggregate `__` (8 GB); the first hosted `run_h2_7b` step wall `__`

## Walk / gate / PR — recorded in the PR body at the final head

## Implementation-time amendments
- 2026-09-04 lane B STOP 1: `h1_emit_qualification_contract.rs` (:449-465 the declaration-control write truncation; :488 the H2.6c activity mapping) is a LIFTED control missed by the G11 survey — packet §5.11 addendum; the lift = both frozen writes exact incl. the `.d.ts` bytes + the H2.7b member observation.
- 2026-09-04 lane A STOP 1: the harness project loader `crates/harness/src/upstream_suites/execution/project.rs` joins the opening-commit file roster (the emitted-files listing must be enabled for project rows through `load_project_emit`; packet §4.6 unchanged in substance).
