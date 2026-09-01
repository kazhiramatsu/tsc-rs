# h2-7a-m-3 — Phase-E frozen-constant register (E4)

Companion to [h2-7a-m-3.md](h2-7a-m-3.md) §10. This file is outside
every generator input closure (the m-2 register-location precedent);
it records the E3/E4 evidence-derived constants. Values marked FINAL
are read from the E3 mints; fingerprint rows are filled after the
final assembly mints on this branch.

## S3 selection (E3 mint 2026-09-01 — every prediction REPRODUCED)

- selector_version: m3-s3-v1; fixtures 8; cases 8; observations 16;
  trim rows: NONE (FINAL — as predicted).
- Case IDs: FINAL exactly typeofexpr-1..4 + literalconst-1..4, no
  `-c<index>` (as predicted).
- Witness totals: FINAL cases 120, oracle runs 240,
  deterministic_cases 120.
- Raw predicate populations 31/50; post-exclusion eligible yields
  31/49.
- Fresh-observation S2 projection guard: PASSED during the mint
  (the frozen 18-row S2 stratum reproduced byte-identically on
  fresh observation before assembly).

## Freeze guards (§7.7; values recorded at E2 authoring)

- H2_7A_M2_S2_PROJECTION_SHA256 (witness): PENDING(E2).
- H2_7A_M2_SCHEMA2_PROJECTION_SHA256 (probe, permanent 112-case):
  PENDING(E2).
- v1 migration projection: S3 prefix excluded; frozen 94-id
  denominator unchanged.

## S3 per-member volumes and floors (§7.9)

- createTypeOfExpression S3 root pairs: PENDING(E3) — must be ≥1.
- createLiteralConstValue S3 root pairs: PENDING(E3) — must be ≥1.
- Non-entity-name heritage serialization observed: PENDING(E3).
- Literal-const initializer synthesis observed: PENDING(E3).

## Expected-zero confirmations across 120 cases (§6.4)

- nodebuilder.withContext.decision, nodebuilder.
  moduleSpecifierOverride.*, tracker.reportTruncationError,
  tracker.reportPrivateInBaseOfClassExpression,
  tracker.reportInaccessibleThisError,
  tracker.reportCyclicStructureError,
  tracker.reportNonlocalAugmentation,
  tracker.reportNonSerializableProperty: PENDING(E3).

## Serialization-member volumes (120-case corpus)

- createTypeOfDeclaration / createReturnTypeOfSignatureDeclaration /
  createLateBoundIndexSignatures /
  getDeclarationStatementsForSourceFile / createTypeOfExpression /
  createLiteralConstValue entry-result pairs: PENDING(E3)
  (m-1+S2 baseline 314/148/152/3/0/0).
- Exclusion-class counts per member: PENDING(E3).

## Successor fingerprints (E3-time values; walk-head finals land in
## the walk cert / close record per the m-2 disposition-7 rule)

- witness artifact fingerprint (E3):
  6439353e417fd7a5… (full value in the artifact).
- witness observation-content roll (E3): 4d2e6f6dc52cf5e4….
- witness case-manifest fingerprint (E3): 81d5b13a639f5cc8….
- probe artifact fingerprint: PENDING(E3 probe mint).
- probe trace-content roll: PENDING(E3 probe mint).
- witness raw file sha pinned by the probe: PENDING(E3 probe mint).
- Trusted-base predecessors (recorded, h2-7a-m-3.md §1): manifest
  89bb0627…, witness roll 091cea9c…, probe roll dcf1243f…, raw shas
  ec2823c890d7…/d459898a9d46…, embedded fingerprints
  84e478a1…/abbb443a….

## E4 surprise-trigger assessment (§8.E4)

- selector drift / zero-volume S3 target / guard failure / nonzero
  expected-zero lane / unpredicted exclusion class / floor miss:
  PENDING(E4).

## Harness constants for P5 transcription

Per-member replayed/excluded counts and per-root decision-lane
volumes are derived by the P5 harness from the FINAL artifacts and
frozen as test constants (packet §6/§10); this register records the
artifact-level volumes above as their upper envelope.
