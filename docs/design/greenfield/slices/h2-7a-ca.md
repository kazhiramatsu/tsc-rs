# H2.7a ca — foundation evidence + close record + the H2.7b-era transition landing

Status: design-gate rev 2 **RATIFIED** (2026-09-03; sol round 1 REVISE
incorporated, round 2 AGREE — §9); rides the `h2/7a-ca` train cut from `main`
after PR #500 (h2-7a-m-4) merges — ONE train, one full gate at its final
head. Closing this rung closes H2.7a (dormant declaration foundation)
and hands off to the H2.7b era.

## 1. Identity, purpose, and boundary

`h2-7a-ca`, kind `evidence` + `transition`, the sixth and final rung of
the [h2-7a.md](h2-7a.md) §6 ladder. The five prior rungs landed the whole
foundation set — inventory + W-H2.7A freeze (m-1, PR #496 @7e452aa8),
the resolver declaration surface (m-2, #497 @d592fcad), the NodeBuilder
(m-3, #498 @5a57795a), the printer/factory faces (m-3.5, #499
@9393b689), and the declaration transformer + selection + diagnostics
seam (m-4, #500 @424ff3e1) — every one corpus-inert, walk-converged, and fully
gated at its own head. What remains, and all this rung owns (parent §6
item 6, m-4 §2f): the era CLOSE record against [h2-7a.md](h2-7a.md) §7,
the zero-activation MACHINE proof in the h2-5h-a-foundation.mjs:1428
shape, the zero-unresolved-owner check as a machine property, the typed
declaration controls, and the ONE-commit transition landing that advances
`next_slice` to H2.7b. No production-crate byte changes: the Rust edits
are two TEST-plane files (the new integration test and its `mod` line in
`crates/compiler/tests/contracts.rs`; no `src/` byte moves), which the
walk's crate-byte definition (chain-walk.sh:274) counts for the h1
ladder re-mint as every crate byte is. No oracle observation runs (the
close generator is a pure function over frozen artifacts and the tree);
no manifest row or candidate is added, promoted, dropped, or re-owned.

## 2. Standing close evidence (already frozen; finals in the walk cert)

Values at the m-4 head `96a33a6e`; the ca walk re-pins these artifacts
(pin-only cascade) and the walk cert records the final fingerprints (the
m-2 E4 disposition-7 rule: walk-head finals live in the cert / close
record, never as harness literals).

| artifact | content | identity |
| --- | --- | --- |
| `ratchets/h2-7a-owner-inventory.v1.json` | 1,039 machine rows over 8 surfaces (declarations module 184 incl. the 68 function rows: owner 1 / nested 60 / siblings 7; selection seam 2; node builder 334 incl. 149 function rows; syntactic builder 139 incl. 54; resolver declaration subset 54 — 19 consumed members / 20 member rows / 28 call sites / 6 orchestration use sites; printer subgraph 184 over 370 function rows (58 seed / 24 type-dispatch workers); factory/parenthesizer 124 — 112 + 12 members, 477 + 14 calls; option-owner closure 18); audit 308 already-exact / 0 foundation-needed / 0 pending; `m4_anchors` 68/68/68 header-verified; partition m-3-head 7 / m-2 12; reached 386; `unresolved_candidates` 0 | inventory fingerprint `0a39b0e0…`; file `803f6ae5…` |
| `ratchets/h2-7a-witnesses.v1.json` | the frozen W-H2.7A case set: 120 cases over 14 frozen families + the S stratum (27 curated + 67 stratum + the m-2/m-3 supplements; positive 87 / adjacent-negative 2 / composition 3 / fault 2), 2 repetitions = 240 TypeScript oracle runs, deterministic 120/120; writes: declaration 202 / javascript 207 / source-map 134; reported diagnostics 132, emit diagnostics 13; `rust_runs` 0 | case-manifest fingerprint `81d5b13a…` (bound across all three m-1 artifacts); observation roll `4d2e6f6d…`; file `d71aa169…` |
| `ratchets/h2-7a-probe-traces.v1.json` | the pinned instrumented-probe traces: 120 cases, 16,925 events (fresh-process double observation reproduced), printed results 647 (schema-3) | fingerprint `d1fbac0a…`; instrumented output `e8f1be2f…`; file `a07fc424…` |
| `ratchets/h2-7a-printer-reprint.v1.json` | W-H2.7A-P: 1,350 gating rows (P1 202 / P2 993 / P3 109 / P4 46), 4 excluded, P1 fixed point 197, 4,544,016 input / 4,530,359 expected bytes, 2,700 oracle runs | file `254e85ff…` |

The Rust-side replays that consume them (all re-run on the ca head as
the walk's PRE_SUITE and again inside the gate's workspace tests; none
is re-authored here): the m-2 resolver replay (12 checker-native members),
the m-3 NodeBuilder replay `declaration_resolver_replay_decision_equal`
(the 7 NodeBuilder-backed members; createTypeOfDeclaration 320/0/0 with
the seven-root expando exclusion CLOSED at m-4), the m-3.5 reprint
contract `declaration_printer_reprint_contract` (1,350 rows byte-exact)
plus the printed-form and keyed error-name lanes, the m-4 transformer
replay `declaration_transformer_replay_decision_equal` (L1 decision-equal
over 116 eligible cases / 202 windows with full event consumption,
divergences 0; L2 byte-equal `.d.ts` over 199 unblocked + 3 blocked
windows with the 3 eligible emit diagnostics compared on every field,
mismatches 0), and the L3 controls `h2_7a_m35_controls` /
`h2_7a_m4_controls` / `h2_7a_partition_projection`.

## 3. Close accounting against h2-7a.md §7

1. **Frozen owner inventory, zero unresolved owners (§7.1):** every
   function of the §3 surfaces hashed, spanned, and dispositioned under
   the full consumer vocabulary; `summary.unresolved_candidates === 0` is
   asserted by the inventory generator's `--check` on every walk and
   re-asserted as a schema const by the §4 close artifact together with
   the final owner projections (audit 308/0/0, anchors 68/68/68,
   partition 7/12, totals — §4.1 block 3). The partition stays m-3-head
   7 / m-2 12: the three m-4 resolver additions are NOT rows of the
   frozen m-1 consumed-member subset (h2-7a-m-4-register.md "Resolver
   partition succession") — this record CLOSES the parent §9 item 7
   "7 → 10 TO-VERIFY at the P-head" marker by measurement.
2. **W-H2.7A determinism + every lane's confirm/falsify result (§7.2):**
   120/120 deterministic over 240 oracle runs; the probe artifact
   reproduced its trace-content and printed-results rolls on the E3
   re-observation (m-4 register). Per-lane record over the frozen
   coverage matrix (`coverage_matrix.lane_coverage`, 14 lanes ← 14
   families + the S stratum), each lane CONFIRMED by the named replay
   and none falsified:

   | lane (parent §5) | families | replay evidence | result |
   | --- | --- | --- | --- |
   | visibility/export graph | F1, F14 | L2 `.d.ts` bytes (m-4); reprint P1 rows (m-3.5) | confirm |
   | type serialization | F2 | L2 bytes; the m-3 NodeBuilder replay (createTypeOfDeclaration 320/320 entry/result, createReturnTypeOfSignatureDeclaration 151/151) | confirm |
   | late-bound/computed names | F3 | L2 bytes; m-3 replay (createLateBoundIndexSignatures 161/161) | confirm |
   | signatures/overloads/accessors | F4 | L2 bytes; the m-2 checker-native resolver replay | confirm |
   | JS declaration synthesis | F5 | L1 windows through `transformDeclarationsForJS` + L2 bytes | confirm |
   | directives/references | F6 | L2 bytes (`references-second` = a blocked window carrying TS2883; `references-first` is `outFile` → H2.7d, excluded by name) | confirm |
   | diagnostics channel | F7 | L1 `declBlocked` 202/202 + L2 emit diagnostics on every field (3 eligible) | confirm |
   | printer grammar/shape | F8, F14, S (67) | m-3.5 reprint 1,350 rows byte-exact; L2 bytes | confirm |
   | NodeBuilder result contracts | F9 | m-3 replay (`nodebuilder.withContext.result` 539) | confirm |
   | symbol tracking/accessibility | F10 | L1 tracker events 533 / 362 / 1 / 1 consumed in full; the m-3.5 keyed error-name lane | confirm |
   | syntactic-builder arms | F2 | the m-3 replay (syntactic builder 54 rows inside m-3) | confirm |
   | generated/global names | F11 | L1/L2 (the m-4 P4b ModuleBlock naming-scope fix: `S2/expando-4` `_a`; `hasGlobalName` 53 entries all outside windows) | confirm |
   | AST identity/provenance | F12 | L1 decision-equal + L2 bytes over the transform-heavy baseline | confirm |
   | upstream-observation controls | F13 | witness adjacents (declaration:false / JS-only): js/map bytes byte-identical under the corpus ratchet; no Rust declaration replay by design | confirm |

   The only falsified expectation in the era was m-3's forecast that the
   probe would not re-mint on a witness pin move — corrected in the m-4
   packet §1/§9; it is not a lane.
3. **Dormant modules with witness-replay evidence, zero production
   reachability, corpus ratchet byte-identical (§7.3):** byte-equal where
   the lane defines bytes (L2 `.d.ts` bytes, the reprint rows, the
   error-name strings), decision-equal against the probe traces
   (resolver / NodeBuilder / transformer decisions). Reachability: the
   per-symbol production allowlists (m-3.5 and m-4 controls) and the
   `declaration_syntax`-gated printer. Corpus: T0 = 100.0000%
   49024/49024 with FP=0 at every H2.7a train head; the 5g/6c
   qualification artifacts changed only in pin fields (5g receipt hit,
   zero observations, at every walk of the era); the candidate
   dispositions' `cases` content roll `ed0036eb…` is IDENTICAL to the
   H2.6c close state (5b4c626a) and the H2.7a first-blocker band is 0
   (chain membership 0) — verified by scan on 2026-09-03 and asserted by
   the §4 artifact.
4. **The zero-activation machine proof (§7.4):** §4.
5. **Typed declaration controls, byte-exact against the H2.6c close state
   (§7.5):** `crates/emitter/src/plan.rs`, `execute.rs`, and
   `activity.rs` are byte-identical to 5b4c626a (sha256 `ac36b2be…`,
   `e4b1e425…`, `3542317…`): the bootstrap refusal tables
   (execute.rs:99-147 — the option table refuses `declarationMap` :111,
   `emitDeclarationOnly` :112-115, and the H2.7c control `stripInternal`
   :120; the presence table refuses `declarationDir` :141 and the H2.7d
   `outFile` :142; `declaration` itself is admitted at bootstrap since
   the H2.6c m-2 flip), the plan-level `declaration` / `declarationMap`
   member refusals (plan.rs:206-216), the H2.6c allowance that admits a
   declaration-bearing unit's js/map members while the declaration
   member stays dormant (execute.rs:627-635), and the
   `observe_runtime_slice` admission panic (activity.rs:575) are the
   same bytes the H2.6c close gated. `printer.rs` grew the m-3.5 workers
   (+3,319/−192) but its `PrintRequest::Declaration` refusal span is
   asserted verbatim by the m-4 controls (printer.rs:1051-1053; the
   parent §8 citation `printer.rs:925` is that span's pre-m-3.5 line).
   The §4 artifact pins the three unchanged files against their
   H2.6c-close hashes as a HARD gate that H2.7b lifts deliberately (§4.4).
6. **Gates at every train head (§7.6):** m-1 #496 @7e452aa8, m-2 #497
   @d592fcad, m-3 #498 @5a57795a, m-3.5 #499 @9393b689 (walk certs
   20260902-175451-65160 / 20260902-200759-37076 / 20260902-230249-31021),
   m-4 #500 @424ff3e1 (walk cert 20260903-094615-69792 at 96a33a6e; gate
   green after a perf-only demoted red → normal-priority rerun, wall
   14.798/25.000s; hosted `gates` 33m47s) — each walk-converged, full local gate green,
   hosted `gates` green. This close train re-converges and re-gates at
   its own final head.

**Residues carried to ca, dispositioned here:**

- The m-3 NAMED RESIDUE (`reportLikelyUnsafeImportRequiredError`'s exact
  specifier string, h2-7a-m-3.md §2f/:426-429; "owned by ca" per the
  m-3.5 register): **CLOSED at m-4 by L2.** The single event in the
  frozen traces (`h2-7a/F6/references-second`, args `[true, 3,
  "NestedProps"]`) surfaces as TS2883 — "The inferred type of 'x' cannot
  be named without a reference to 'NestedProps' from
  'foo/node_modules/nested'. This is likely not portable. A type
  annotation is necessary." — in that case's emit diagnostics, which L2
  compares on every field over the eligible domain at mismatches 0. The
  specifier string is byte-pinned; no further lane is needed.
- The m-3.5 lone-surrogate witness stratum ("the ca rung's call",
  h2-7a-m-3.5.md §2f / F6): **DECLINED here.** Corpus reach was 0 at the
  m-3.5 E4 census; the H2.7a witness set stays frozen at 120 cases
  (manifest `81d5b13a…`). If the H2.7b activation band (count-only
  forecast: 2,456 chain rows / 1 first-blocker) needs one, H2.7b's own
  packet adds it under its own design gate — a witness-set change here
  would re-mint witnesses → probe → reprint for no evidentiary gain.
- Parent §9 item 6 ("exact ca transition values"): RESOLVED by §5.
- Parent §9 item 7 ("7 → 10 TO-VERIFY at the P-head"): CLOSED by the
  measurement in §3.1.

## 4. The zero-activation machine proof

### 4.1 The close artifact (new chain rung, LAST in ORDER)

`crates/oracle/h2-7a-close.mjs` → `ratchets/h2-7a-close.v1.json` +
`.github/ci/contracts/h2-7a-close.schema.json`, registered in the same
commit at every rung-registration point: `scripts/chain-walk.sh` ORDER
(69 → 70, after `h2-7a-printer-reprint`; the driver's coverage self-check
refuses drift), `new-ci/src/bin/plan.rs` LADDER_ORDER (70; the
walk-planner-coverage check refuses drift), `.github/ci/qualification.mjs`
ARTIFACT_SCHEMA_CONTRACTS (18 → 19) AND `.github/ci/qualification.test.mjs`
("artifact-to-schema mapping is fixed and immutable", :211 — the deep-
compared list gains the same pair), and `.github/ci/pin-index.v1.json`
(the consumer entry for the new script with its `semantic` / `unmatched`
classifications, §4.1 mechanics below). A pure function — seconds, no
TypeScript or Rust execution, no check receipt — over these inputs:

- the parent profile `ratchets/h2-5g-profile.v1.json` by CONTENT
  assertions (the h2-5h-a-foundation `loadParentProfile` idiom, not a
  hash pin, so a pin-only parent re-mint does not stale the close);
- the four H2.7a artifacts by whole-file `inputs` pins (the ladder's
  `pathHash` grammar-A convention; walk re-pins) plus their internal
  fingerprints;
- `ratchets/h2-candidate-dispositions.v1.json` (the `cases` content roll
  and the per-slice band counts);
- the tree: `crates/emitter/src/plan.rs`, `execute.rs`, `activity.rs`
  (whole-file sha256), `printer.rs` (the refusal span by text).

Blocks (schema: value consts for every dormancy field and every frozen
owner projection, `sha256` patterns for every hash — NO hash-bearing
schema const, so the `schema-const-repin.py` surface stays the h2-5g
five):

1. `runtime_contract` — the h2-5h-a-foundation.mjs:1428 shape, H2.7a
   flavored, FROZEN HISTORY at this close (consts never change):
   `foundation_slice_id "H2.7a"`, `runtime_activation_slice_id "H2.7b"`,
   `production_state "dormant"`, `transformer_registration
   "not-registered"`, `active_runtime_slices` = the 26 through `H2.6c`,
   `h2_7a_runtime_active false`, `h2_7a_activity 0`,
   `candidate_execution_state "not-run"`, `candidate_typescript_runs 0`,
   `rust_runs 0`, `parent_completed_runtime_slices 25`,
   `runtime_admissions_before/after 9196` (delta 0),
   `executed_candidates_before/after 9715` (delta 0).
2. `candidate_band` — H2.7a first-blocker rows 0 and chain-membership
   rows 0 (the zero band, parent §2); the `cases` roll asserted equal to
   the frozen H2.6c-close value `ed0036eb…`; the H2.7b FORECAST recorded
   count-only (`next_slice_forecast`: first-blocker 1, chain 2,456 at
   authoring) — the ca record distinguishing H2.7a's actual zero delta
   from any forecast for H2.7b (parent §6.6); never an admission.
3. `owner_closure` — the final owner projections as schema consts (F6):
   `unresolved_candidates 0`, `total_rows 1039`, the eight surface-row
   counts (184 / 2 / 334 / 139 / 54 / 184 / 124 / 18), declarations
   68 = 1 + 60 + 7, node builder 149, syntactic builder 54, resolver
   19 / 20 / 28 / 6, printer 370 / 58 / 24 / 184, factory 112 / 477 /
   12 / 14, audit 308 / 0 / 0, `m4_anchors` 68 / 68 / 68, partition
   7 / 12, reached 386 — a re-minted inventory that regresses any of
   them reds the close (and the H2.7b-era amendment surface, §4.4).
4. `evidence` — the four artifact pins, the witness/probe/reprint summary
   projections (cases 120, oracle runs 240, `rust_runs` 0, declaration
   writes 202, events 16,925, gating rows 1,350), and the eligible-domain
   denominators RE-DERIVED from the probe artifact by the generator
   (excluding the four named cases): eligible 116, `probe.transformSeed`
   202, `declarations.declBlocked` 202,
   `transformTopLevelDeclaration.changed` 742,
   `visitDeclarationSubtree.changed` 496, `tracker.trackSymbol` 533,
   `reportInferenceFallback` 362, `reportInaccessibleUniqueSymbolError`
   1, `reportLikelyUnsafeImportRequiredError` 1 — asserted against these
   consts (the m-4 register's E2 table becomes a machine property;
   verified reproducible by scan 2026-09-03).
5. `refusal_surfaces` — plan.rs / execute.rs / activity.rs: `sha256_now
   === sha256_at_h2_6c_close` (frozen baselines `ac36b2be…` /
   `e4b1e425…` / `3542317…`), `retained: true`; printer.rs: the
   `PrintRequest::Declaration` refusal span present, `retained: true`.
6. `transition_landing` — LIVE parent assertions (maintained per close,
   the 5h-a-foundation `loadParentProfile` idiom): `transition.next_slice
   === "H2.7b"`, `next_slice_scope`, `next_runtime_activation_slice ===
   "H2.7b"`, `completed_slice "H2.5g"`, `inactive_runtime_slice_count
   11`, and `summary` 25 / 0 / 9196 / 9715 / 0 / 0 (so the close
   artifact cannot mint before §5 lands — the close and the landing
   converge in ONE walk).
7. `summary` mirrors (`h2_7a_activity 0`, `rust_runs 0`,
   `runtime_admissions_delta 0`, `unresolved_owners 0`,
   `refusal_surfaces_retained 4`).

**Baseline mechanics (F5).** The four frozen HISTORICAL baselines — the
three source hashes and the dispositions roll — must never be
auto-repinned: chain-walk-repin.py refreshes every grammar hit whose
current file hash moved (grammar D, `const X_RELATIVE_PATH = "path"` +
`const EXPECTED_X_SHA256 = "hash"`, has an UNRESTRICTED path class), so
a plain pair for `crates/emitter/src/plan.rs` would silently follow
H2.7b's edit and stop being an H2.6c baseline. Therefore: the three
source baselines are written as grammar-D pairs AND classified as
`semantic` rows (`{path, grammar: "D"}`) under
`consumers["crates/oracle/h2-7a-close.mjs"]` in
`.github/ci/pin-index.v1.json` — semantic rows are program logic, never
masked by the receipt normalizer and skipped by the repin
(chain-walk-repin.py:41-49); the dispositions roll (a bare
`H2_6C_CLOSE_CASES_ROLL_SHA256` const with no path pair, path-adjacent
to the ratchets path) is classified as an `unmatched` row with a note,
exactly the `H2_0A_CANDIDATE_SHA256` precedent in
h2-1a-qualification.mjs. `python3 scripts/pin-index.py --write` (which
preserves hand-curated `semantic`/`unmatched` rows) then `--check`, and
`scripts/walk-preflight.py`'s pin-index surface, verify the
classification in the slice. `scripts/pin-audit.py` audits RUST-side
pin literals and is untouched: the ca test holds no hash literal. The
walk's `--check` of the close artifact recomputes every block from the
tree and refuses on any drift (exit 1 → `--write` and review).

### 4.2 The Rust control gate

`crates/compiler/tests/integration/h2_7a_ca_controls.rs`, registered by a
`#[path]` `mod` line in `crates/compiler/tests/contracts.rs` (the test
binary root; two test-plane files — F3). The new file joins
`h2-5g-profile.mjs` NON_RUNTIME_SHADOW_INPUTS after
`h2_7a_m4_controls.rs`; `contracts.rs` is already a runtime input
(NEW_RUNTIME_INPUTS :175), so the runtime-input count and the schema
min/max stay 241.

1. `h2_7a_activation_panics_on_the_production_profile` —
   `H2ActivityCanary::h2_6c_profile()` (the current production profile,
   activity.rs:479) under `catch_unwind`: `observe_runtime_slice(H2_7a)`
   panics with `unadmitted H2 runtime activity: H2.7a`; the H2_7a runtime
   counter stays 0; the same holds for every earlier constructor in the
   chain (h1 … h2_6b). H2.7a is dormant-foundation FOREVER — H2.7b's
   activation admits `H2_7b`, never `H2_7a` — so this control survives
   the era boundary unchanged.
2. `no_h2_7a_admission_exists_in_production` — source scan of
   activity.rs: no `fn h2_7a_profile`, no `H2RuntimeSlice::H2_7a.index()`
   admission line; the m-4 per-symbol allowlist scan extended with
   `observe_runtime_slice(H2RuntimeSlice::H2_7a` ⊆ ∅ over every
   non-test Rust source.
3. `declaration_family_options_remain_typed_refusals` — `tsc_emitter::
   validate_bootstrap_emit_request` (execute.rs:155, public) over a
   control host for each declaration-family refusal of execute.rs:99-147:
   `declarationMap`, `emitDeclarationOnly`, `declarationDir`, and the
   H2.7c control `stripInternal` (each `Some(true)` / `Some(path)` alone
   on default options) returns the `unsupported(name)` refusal the table
   builds today, matched by option name; `outFile` (H2.7d) rides the
   same table and is asserted alongside; `declaration: Some(true)` alone
   passes bootstrap (the H2.6c admission) while the planned declaration
   member stays refused by control 4. The expected variants are read
   from the H2.6c-close bytes at implementation (byte-identical, §3.5),
   never widened.
4. The m-4 L3 controls (`declaration_plan_execute_and_printer_refusals_
   are_retained` incl. `validate_bootstrap_shape` →
   `Unsupported(Declaration)` / `Unsupported(DeclarationMap)`, the
   bundle-root refusal, the per-symbol allowlist) and the m-3.5 controls
   (`declaration_syntax_is_constructed_only_at_the_dormant_allowlist`)
   stay in force unchanged — re-run, not re-authored.

### 4.3 Cost

No observation: the walk's stale cone is the new rung + the pin-only
cascade of §5 (5g profile transition consts → h2-5h-a-foundation → its
six artifacts → the 6a/6b/6c qualification pins → the four H2.7a
artifacts, all `receipt hit` / pin re-mints); the two test-plane crate
bytes re-mint the h1 ladder and h2-transition pins as every crate byte
does. Expected walk ≈ the m-4 walk-2 profile (~100 min, round 2 clean);
`TSRS_H2_5G_FRESH` stays unset (zero 5g re-observation is the
check-outcome expectation).

### 4.4 The H2.7b handoff — the complete amendment surface (F8)

The close artifact is a HARD gate on H2.7a's dormancy until H2.7b lifts
it on purpose. Two distinct transitions own it:

**H2.7b ACTIVATION (the rung that admits the declaration member):**
- the close artifact's `refusal_surfaces`: plan.rs / execute.rs (and
  activity.rs when `h2_7b_profile()` lands) change → the semantic
  baselines are re-frozen at the activation commit and `retained` flips
  to `lifted-at-H2.7b`; printer.rs's `PrintRequest::Declaration` refusal
  span likewise; the close schema consts + `summary.refusal_surfaces_
  retained` follow;
- the Rust refusal controls that pin the same bytes: the m-4
  `declaration_plan_execute_and_printer_refusals_are_retained`
  (h2_7a_m4_controls.rs:166 — the printer/execute literal assertions
  and `validate_bootstrap_shape`), the ca §4.2 controls 3-4, the m-3.5
  `declaration_syntax` allowlist (h2_7a_m35_controls.rs:91), and the
  emitter contract tests `output_plan_contract.rs:123` (the
  DeclarationOnlyMode / declaration-member refusals) and
  `printer_foundation_contract.rs:697` (the `PrintRequest::Declaration`
  refusal);
- NOT the ca controls 1-2 (H2_7a stays unadmitted forever) and NOT the
  `owner_closure` consts unless H2.7b re-mints the inventory with
  changed rows.

**H2.7b CLOSE (the ca-analog that lands H2.7b's transition):**
- `h2-5g-profile.mjs` transition + summary (`active_runtime_slices` +=
  `H2.7b`, `inactive_runtime_slice_count` 11 → 10, `completed_runtime_
  slices` 25 → 26, admissions/executed-candidates by H2.7b's band, an
  `h2_7b_*` adoption block, `next_slice` → H2.7c) and its schema consts;
- `h2-5h-a-foundation.mjs` parent assertions AND its schema's parent
  pins (`h2-5h-a-foundation.schema.json:520-541`, `:966-972`: 25 →
  26, 9196 / 9715 → the new values);
- the close artifact's LIVE `transition_landing` mirrors (block 6) —
  `runtime_contract` (block 1) is frozen history and stays;
- `candidate_band`'s roll IF H2.7b re-freezes the dispositions when it
  promotes its band.

## 5. The transition landing (ONE commit, LAST in the train)

Three files, strictly narrower than the H2.6c close (no numeric pin
moves, no adoption block for a dormant slice, the foundation schema's
parent pins 25 / 9,196 / 9,715 untouched):

- `crates/oracle/h2-5g-profile.mjs:634-636` (+ the comment block above
  it): `next_slice` `"H2.7a"` → `"H2.7b"`; `next_slice_scope`
  `"declaration-owner-inventory-and-dormant-foundation"` →
  `"non-bundle-declaration-output"` (post-h1-completion-slices.md §4.5
  H2.7b: non-bundle `.d.ts` emit, callback metadata, declaration-only
  routing, output paths, exact resolver/NodeBuilder results);
  `next_runtime_activation_slice` stays `"H2.7b"` — the 6c-era
  divergence between `next_slice` and the activation slice re-converges,
  exactly as h2-6c-ca.md §5 forecast. `active_runtime_slices` (26, ends
  `H2.6c`), `inactive_runtime_slice_count` 11, and `summary` (25 / 0 /
  9,196 / 9,715 / 0 / 0) are UNCHANGED: H2.7a admitted nothing. No
  `h2_7a_*` adoption block (the `h2_6a/6b/6c_*` mirror is a runtime-slice
  idiom; the dormant slice's zeros live in the §4 close artifact).
- `.github/ci/contracts/h2-5g-profile.schema.json:228-230`: the three
  transition consts to the same values.
- `crates/oracle/h2-5h-a-foundation.mjs:1296-1304`: the parent-profile
  content assertions (`next_slice === "H2.7b"`, the scope string,
  `next_runtime_activation_slice === "H2.7b"`) + the comment; the
  `canonical([...CLOSED_THROUGH_H2_5G, "H2.5h", "H2.6a", "H2.6b",
  "H2.6c"])` list and every numeric assertion stay.
- `crates/oracle/h2-transition.mjs` and every downstream pin: chain-walk
  hash re-mint only; the frozen TRANSITIONS ladder row `["H2.7a",
  "planned", …]` is H2.0a evidence and stays, as H2.6c's did. A
  repository-wide consumer search (sol round 1) confirmed no fourth live
  consumer of the three transition fields needs a semantic edit.

The §4 close artifact asserts the landed values (block 6), so the
landing commit and the close rung converge together in the ONE walk that
follows the landing.

## 6. Close markers

The slices index README gains the H2.7a-era rows it lacks (m-2 #497
@d592fcad, m-3 #498 @5a57795a, m-3.5 #499 @9393b689, m-4 #500, and this
close: **H2.7a CLOSED**, evidence = the §2 artifacts + the §4 close
artifact + the final-head gate record) and moves the m-1 row's status to
merged. `h2-7a.md` stays historical, as the 6a and 6c closes left
`h2-6a.md` / `h2-6c.md` (their landing commits b94bfde9 / 71a56e77 edited
only close/transition surfaces — and, for 6a, the then-live FCI
envelopes — with the slices index as the design-side close marker): its
§9 items 6 and 7 are resolved BY this packet's §3/§5, and — the
operational reason — the witness artifact pins the parent packet, so
editing it would re-mint witnesses → probe → reprint (the m-4 E3
cascade, ~10 min of fresh observation) for a prose change. No STAGE
change (mid-H2 slice); `ratchet.toml` untouched (no accepted-state
change). The H2.7b-era opening packet may board this train as a
design-only passenger under its own design gate (the h2-7a.md on
h2/6c-close precedent); it is not a dependency of this close.

## 7. Prohibitions

All parent §8 prohibitions remain in force (no output activation, no
candidate promotion, no hand-authored `.d.ts` expectations or
fixture-specific branches, no NodeBuilder substitution / partial-port
presentation / unknown-as-success fallback, no production wiring of
`getDeclarationDiagnostics` or custom transformers, later-owned arms
stay typed controls of their named slices). Ca-specific additions:

- No production-crate byte changes; the two test-plane Rust files
  (§4.2) are the only crate bytes. No `.d.ts`/`.d.ts.map` write, no
  refusal lift, no plan.rs/execute.rs/printer.rs seam change, no H2.7a
  profile constructor, mask bit, or `observe_runtime_slice` call, no
  manifest row touched.
- No oracle observation, band re-observation, or qualification re-run:
  the close generator reads frozen artifacts and the tree; the walk
  expects a pin-only cascade with zero 5g observations.
- No candidate promotion or re-freeze: the H2.7b forecast is count-only
  context; the dispositions roll must equal the H2.6c-close value.
- No hand-derived CURRENT pins: walk-owned hashes re-mint only through
  the walk; the close schema carries value consts only. The four frozen
  HISTORICAL baselines (§4.1 mechanics) are semantic constants
  classified in the pin-index — never repinned, never masked.
- The four H2.7a artifacts and the 120-case witness set are frozen
  inputs; this rung changes none of their content (pins move only).
- The hosted boundary stays fixed and unsplit; no acceptance change.

## 8. Acceptance

fmt/clippy green; `PRE_SUITE` = the ca controls +
`declaration_transformer_replay_decision_equal` +
`declaration_resolver_replay_decision_equal` +
`declaration_printer_reprint_contract` + the `h2_7a_` contracts, green on
the ca head BEFORE the walk (the driver's gate-tax 5-E hook); the walk's
own preflight reports ORDER / LADDER_ORDER in sync at 70/70
(walk-planner-coverage), producer-before-consumer topology clean with
the new rung last, and all pin surfaces clean incl. the pin-index
classification of §4.1; `bash scripts/chain-walk.sh` ONE-invocation
converge at the train head with the new rung minted, `node
crates/oracle/h2-7a-close.mjs --check` exit 0 twice, and the check
outcome record showing zero 5g observations; `WALK_DRY=1 bash
scripts/chain-walk.sh` green AFTER the walk (it refuses on any crate
byte not covered by the converged record, chain-walk.sh:273-282, so it
is a post-walk / gate-preflight check, never a pre-walk one — F4);
`node .github/ci/qualification.mjs check` valid with 19 registered
contracts and `.github/ci/qualification.test.mjs` green on the
19-pair list; full local gate `cargo xtask ci --baseline 424ff3e1` green at the final head; hosted `gates` green on the PR; merge
closes H2.7a and opens the H2.7b era with `next_slice ===
next_runtime_activation_slice === "H2.7b"`.

## 9. Cross-review record

Round 1 (2026-09-03): operator draft rev 1 → sol **REVISE** (7 blocking
+ 4 advisory). Dispositions: F1 (ORDER/LADDER_ORDER "68 → 69/69")
**REFUTED by measurement** — both lists hold 69 entries (mechanical
count 2026-09-03; the m-4 walk-2 log records "coverage: ORDER in sync
(69 chain scripts)" / "planner LADDER_ORDER in sync (69 rungs)"), so
69 → 70 and 70/70 stand; F2 (qualification.test.mjs immutable-mapping
list) INCORPORATED §4.1; F3 (contracts.rs `mod` registration; two
test-plane Rust files; count stays 241) INCORPORATED §1/§4.2/§4.3/§7;
F4 (`WALK_DRY=1` is post-walk only) INCORPORATED §8; F5 (semantic
baselines in the pin-index; the roll as `unmatched`; pin-audit is the
Rust-side auditor) INCORPORATED §4.1 mechanics + §7; F6 (owner
projections as schema consts) INCORPORATED §4.1 block 3 + §3.1; F7
(execute.rs:99-147 enumeration incl. `declarationDir`) INCORPORATED
§3.5/§4.2; F8 (activation-time vs close-time handoff surfaces)
INCORPORATED §4.4; F9 (per-lane confirm/falsify crosswalk)
INCORPORATED §3.2; F10 (precedent wording) INCORPORATED §6; F11
(parent §8 preamble) INCORPORATED §7. Verified without finding by
sol: every §2 count/roll/hash, the three source hashes and their empty
diff from 5b4c626a, the dispositions roll, the three residue
dispositions, the new-rung design as the simplest sound machine proof,
the public APIs of §4.2, the completeness of the three landing surfaces
and values, and the one-era hash contract (given F5 + F8).

Round 2 (2026-09-03): rev 2 → sol **AGREE**, zero new findings. F1
WITHDRAWN by the reviewer's own mechanical count (ORDER 69 /
LADDER_ORDER 69, agreeing with walk2.log:9-10); F2-F11 each marked
RESOLVED against the rev-2 text with evidence (qualification.test.mjs:211,
h2-5g-profile.mjs:175 + schema:144, chain-walk.sh:273/:282,
chain-walk-repin.py:41 + pin-index.py:110 + pin-audit.py:28, the
inventory summary, execute.rs:99/:139, the four refusal tests + the
foundation schema:520, witnesses `lane_coverage`, the b94bfde9 /
71a56e77 stats, parent §8). This revision is the h2-7a-ca design
authority.
