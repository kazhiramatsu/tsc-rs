# Non-2XXX first order — family map and scheduling skeleton

Status: adopted structure (2026-07-16); machine map landed by the A5
slice (2026-07-18). Companion to
[2xxx-first-order.md](2xxx-first-order.md), which owns the 2XXX band
and states that non-2XXX diffs are invisible to its metric. This doc
owns the decomposition of everything outside codes 2000-2999 into
implementation-owner families, their measured baselines, and the
acceptance shape each family closes on. The A5 slice turned it into
the machine map and rollup defined by the
[A5 integrity contract](measurement-integrity.md#5-a5--family-ownership-and-supported-rollup):
`diag-families.json` is the machine authority (`cargo xtask families
check`), `cargo xtask families report` the live rollup
(`target/families/report.json`). The map lands DRAFT in its
introduction PR — the first freeze cannot ride that PR (the audit
rejects it, mirroring A2's missing-base window) and follows as its
own reviewed change against the merged base. The numbers below remain
the planning baseline from the pre-oracle-correction `52c47bbb` tree,
not a ratchet — the rollup owns current numbers.

The v1 roadmap (`docs/design/non-2xxx-blockers.md`) is v1-only
(`src/`, EXECUTION-GUIDE, classifier discipline) and is not reused.
Everything below is regenerated from tsrs2 artifacts: the all-band
conformance artifact (`target/m8/conformance.json`) joined back to
the schema-2 goldens for pass provenance.

## 1. Measured baseline

Non-2XXX oracle T0 buckets: 27,803 of 48,719 (410 distinct codes).
Matched 9,131 (32.8418%), FN 18,672; T3 shadow 29.6047%.

Pass provenance is load-bearing — the same code can arrive from the
parser (syntactic), the checker (semantic, incl. `checkGrammar*`),
or the suggestion collection, and the owners differ per pass:

| Band + pass | Total | FN |
|---|---:|---:|
| 1XXX syntactic | 1,428 | 4 |
| 1XXX semantic | 2,520 | 1,088 |
| 17XXX syntactic | 389 | 0 |
| 17XXX semantic | 921 | 299 |
| 18XXX syntactic | 8 | 0 |
| 18XXX semantic | 1,244 | 698 |
| 4XXX semantic | 74 | 49 |
| 5XXX semantic | 30 | 2 |
| 6XXX syntactic | 74 | 0 |
| 6XXX semantic | 164 | 158 |
| 6XXX suggestion | 14,617 | 14,617 |
| 7XXX semantic | 1,534 | 860 |
| 7XXX suggestion | 4,164 | 591 |
| 8XXX syntactic | 330 | 0 |
| 8XXX semantic | 43 | 43 |
| 80XXX suggestion | 263 | 263 |

Facts the numeric-band view hides:

- Every syntactic row is effectively closed (4 FN total = the 1453
  pragma fixture ×4, the M1-classified parser residue). The non-2XXX
  problem is a checker/suggestion problem, not a parser problem.
- The suggestion pass is not monolithically absent: infer-from-usage
  (7043-7050) is already ~92% matched (3,571 / 3,875) through the
  `widen.rs` constants band, while the unused half (6XXX suggestion)
  is 0 / 14,617. "Implement the suggestion band" is two very
  different jobs.
- Cross-band ownership is real: 7027/7028 (unreachable/unused-label)
  are flow-graph products that surface as suggestions; 18046-18050
  are M5 strict-nullability flow; 4104 is a relations-layer
  diagnostic; codes such as 6133 appear in BOTH the suggestion pass
  and (under `noUnusedLocals`-style matrix keys) the semantic pass.
  The machine map is therefore keyed by (code, pass), never by code
  range.

## 2. Families

The A5 slice adjudicated these owners into `diag-families.json`; the
map is the authority and this table the planning record (the freeze
itself is the follow-up reviewed change described above). Anchor and
reviewed re-baseline rules are defined once in the
[A5 integrity contract](measurement-integrity.md#5-a5--family-ownership-and-supported-rollup),
so ownership cannot drift
after it becomes a gate input; "M7 8.x" refers to the stages in
[m7-tail-steps.md](m7-tail-steps.md). FN counts are the measured
baseline; ~ marks rows whose exact (code, pass) split the machine
map now pins.

| Family | Provisional owner | Anchor codes (FN) | FN |
|---|---|---|---:|
| parser pragma residue | M8 tail (M1-classified) | 1453 (4) | 4 |
| checker grammar | M7 8.1 | 1206 (129), 1340 (72), 1029 (66), 1117 (53), 1119 (36), 1361 (33); 17009 (58), 17011 (11), 17013 (3), 17019/17020 (12); 18010 (24), 18016 (34), 18033 (13), 18037 (22), 18028 (5) | ~1,277 |
| flow / strict nullability | M5 | 18046 (23), 18047 (18), 18048 (175), 18049 (4), 18050 (354) | 574 |
| flow-derived suggestions | M5 mechanism, M7 8.4 surfacing | 7027 (104), 7028 (75) | 179 |
| unused | M7 8.3 (error mode) + 8.4 (suggestion mode) | 6133 (12,932), 6196 (1,334), 6198 (187), 6138 (58), 6199 (45), 6192 (33), 6205 (20) | ~14,609 |
| suggestions: infer-from-usage residue, await/bignum, deprecations | M7 8.4 | 7044 (220), 7043 (26), 7045 (40), 7050 (18), 7032 (127); 80007 (239), 80008 (24); 6385 (4), 6387 (14) | ~700 |
| implicit-any (semantic errors) | adjudicate: M6-adjacent reporting vs M8 | 7005 (120), 7006 (80), 7053 (75), 7034 (5), 7008 (11), 7010 (20), 7031 (14) | ~370 |
| JSX mode / option | adjudicate: options + JSX checking | 7026 (431), 17004 (210), 17017 (2) | 643 |
| program / resolution / options | M7 8.5 | 6053 (142), 6263 (4), 5076 (2) | ~150 |
| checkJs / JSDoc | M8 (pulls the standing [JSDOC] M2 policy) | 8020-8039 (43), 18042 (4), 18043 (11), 7016 (30) | ~90 |
| override validation | adjudicate: class-checking owner | 4113-4127 (44) | 44 |
| relations-layer stragglers | M4/M8 relations | 4104 (5), 6234 (2) | 7 |
| module semantics tail | M8 (5.8d/5.9d-adjacent) | 18057-18061 (7), 7059/7060 (32), 17012 (1) | ~40 |

Rows are ordered by weight, not landing order. The unused family is
79% of the non-2XXX FN mass; the remaining families total ~3,900 and
are individually small but mechanically diverse — grammar walker,
suggestion surfacing, options/program plumbing, JSX/JS-check modes.
Their risk is architectural discovery, not volume, which is why each
gets an owner and an acceptance row instead of drowning in an
aggregate rate.

## 3. Acceptance grammar

Per family, in map order of strictness:

1. **Canary set complete** — the named fixtures for the family (the
   map records exact fixture + matrix key anchors) match at T0.
2. **Supported FN = 0** at T0 for the family's (code, pass) rows.
3. **Tier follow-through** — the family's buckets complete at the
   active tier (T1 once activated at M7 8.4, then T2/T3 through M8
   tiers).

A family CLOSES at level 2 for its owner milestone's gate; level 3
is tracked by the same rollup and finishes inside the M8 tier
sweeps. Closing at level 2 does not authorize a T0-only reporter: the
[slice-fidelity policy](definition-of-done.md#milestone-gates-vs-slice-fidelity)
requires the touched family to match every currently live shadow tier,
or to record exact blocked rows with their shared prerequisite and M8
retirement owner. The rollup is a derived view over the A1 accepted-match
artifact, immutable oracle-input manifest, exact A2 scope, current
full-conformance observation, and frozen family map. Scope is applied to
the exact oracle/tsrs record multisets before tier grading and family
grouping; A1 summaries alone cannot supply supported counts, especially
for partial duplicate buckets. The rollup introduces no second ratchet;
the global set ratchet already forbids regressions everywhere. Unmapped
non-2XXX (code, pass) rows appearing in the corpus fail the map check;
2XXX rows are the band family's, verified as a partition (convergence
plan A5).

## 4. Adjudication backlog — RESOLVED (A5 slice, 2026-07-18)

Decisions recorded in the map's per-family notes; summary:

- implicit-any semantic errors → one `implicit-any` family under
  **M6** (reporting rides inference results; 7034/7057 additionally
  lean on M5 evolving-array flow, caught by the later M6 gate).
  (7032, semantic) is the noImplicitAny error surface and stays here;
  its suggestion twin is infer-from-usage.
- JSX family → one `jsx-mode` family under **M8**: 17004 (options
  plumbing) and 7026 (JSX noImplicitAny) do not split — both nearly
  closed by the 5.8/5.9 JSX interop work; residue is a bounded M8
  mining family.
- 7016 → `checkjs-jsdoc` (**M8**), in BOTH its modes: the corpus
  exercises (7016, semantic) 30 and (7016, suggestion) 9, and the
  declaration-file lookup surfaces with checkJs.
- override validation → `override-validation` (**M8**; M4 closed with
  these open). 4112 belongs with 4113-4127 — the same
  checkOverrideModifier walk.
- 4111 (index-signature member access, option-gated, matched) →
  `relations-stragglers`, avoiding a one-row family.
- (7027, semantic) — unreachable-code error mode — →
  `flow-strict-nullability` (**M5**) with the 18046-18050 band; the
  suggestion-pass twin stays in `flow-derived-suggestions`
  (M7 8.4 surfacing).
- exact (code, pass) splits for every ~ row: enumerated by the
  machine map (433 non-2XXX rows on the post-epoch corpus; pinned at
  its freeze).
- suppression surfaces (M7 8.2) have no code set: the map records a
  row-less family whose acceptance is the audit artifact plus its
  canary fixtures (`conformance/directives/ts-expect-error-nocheck`,
  `ts-ignore`, `jsdoc/tsNoCheckForTypescript`).

## 5. Regeneration

`cargo xtask families report` produces the live rollup
(`target/families/report.json`). The manual recipe below remains the
independent cross-check of the same numbers:

- run `cargo xtask m8 readiness` (writes
  `target/m8/conformance.json`);
- FN by code: sum `mismatches[].false_negative[]` by `code`;
- pass attribution: join each FN key (file, code, line, col) back to
  the golden case's oracle records;
- totals: dedupe golden oracle records per case by the same key.

Any refresh that changes these totals means the corpus or comparator
changed and must be treated as a baseline event, not drift.
