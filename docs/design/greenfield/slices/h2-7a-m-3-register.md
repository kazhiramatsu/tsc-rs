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

## Freeze guards (§7.7 — values frozen at E2, exercised at E3)

- H2_7A_M2_S2_PROJECTION_SHA256 (witness) =
  d9cb88a8100cf481c1221dfe986bfc6036857cb875cc7dd667eb246cea90a4d3
  — PASSED on fresh observation and artifact paths.
- H2_7A_M2_SCHEMA2_PROJECTION_SHA256 (probe, permanent 112-case) =
  6b43e5a1fa6596e36db867b18442f71c080c11b688217c95cabb73368c76c9b9
  (id-order pin 581d0b4ba3e584b8…) — PASSED inside the mint's
  validateArtifact: the frozen schema-2 traces survived
  re-observation byte-identically.
- v1 migration projection: S3 prefix excluded; frozen 94-id
  denominator unchanged.

## S3 per-member volumes and floors (§7.9 — ALL MET, E3 FINAL)

- createTypeOfExpression S3 root pairs: 4 (one per typeofexpr case).
- createLiteralConstValue S3 root pairs: 8 (2/1/2/3 across
  literalconst-1..4).
- Non-entity-name heritage serialization observed: YES (every
  typeofexpr case).
- Literal-const initializer synthesis observed: YES (every
  literalconst case).
- Per-case S3 root map (entry counts): typeofexpr-1 {lateBound 2,
  typeOfExpression 1}; typeofexpr-2 {returnType 2, typeOfDecl 2,
  lateBound 2, typeOfExpression 1}; typeofexpr-3 {lateBound 2,
  typeOfExpression 1}; typeofexpr-4 {lateBound 2, returnType 1,
  typeOfExpression 1}; literalconst-1 {literalConst 2, typeOfDecl 1};
  literalconst-2 {literalConst 1}; literalconst-3 {literalConst 2,
  lateBound 1}; literalconst-4 {literalConst 3, typeOfDecl 3}.

## Expected-zero confirmations across 120 cases (§6.4 — E3 FINAL)

- ALL EIGHT zero lanes confirmed zero across the 120-case corpus:
  nodebuilder.withContext.decision 0,
  nodebuilder.moduleSpecifierOverride.* 0,
  tracker.reportTruncationError 0,
  tracker.reportPrivateInBaseOfClassExpression 0,
  tracker.reportInaccessibleThisError 0,
  tracker.reportCyclicStructureError 0,
  tracker.reportNonlocalAugmentation 0,
  tracker.reportNonSerializableProperty 0.
- probe.fallbackSweep: 0 (corpus-wide, unchanged).
- Singleton sites unchanged at 120 cases:
  reportInaccessibleUniqueSymbolError 1,
  reportLikelyUnsafeImportRequiredError 1.

## Serialization-member volumes (120-case corpus — E3 FINAL)

- createTypeOfDeclaration 320; createReturnTypeOfSignatureDeclaration
  151; createLateBoundIndexSignatures 161;
  getDeclarationStatementsForSourceFile 3; createTypeOfExpression 4;
  createLiteralConstValue 8 (m-1+S2 baseline was 314/148/152/3/0/0).
- Total trace events 16,925 (was 15,845); withContext.result 539;
  trackSymbol 538; reportInferenceFallback 373.
- Exclusion-class per-member counts: P5-frozen from the FINAL
  artifacts (packet §6.7); artifact-level volumes above are the
  upper envelope.

## Successor fingerprints (E3-time values; walk-head finals land in
## the walk cert / close record per the m-2 disposition-7 rule)

- witness artifact fingerprint (E3):
  6439353e417fd7a5… (full value in the artifact).
- witness observation-content roll (E3): 4d2e6f6dc52cf5e4….
- witness case-manifest fingerprint (E3): 81d5b13a639f5cc8….
- probe artifact fingerprint (E3): 73627055e0e12ee7….
- probe trace-content roll (E3): ca24d47c54b14df3….
- witness raw file sha pinned by the probe (E3): fb521718d04894f2….
- Trusted-base predecessors (recorded, h2-7a-m-3.md §1): manifest
  89bb0627…, witness roll 091cea9c…, probe roll dcf1243f…, raw shas
  ec2823c890d7…/d459898a9d46…, embedded fingerprints
  84e478a1…/abbb443a….

## E4 surprise-trigger assessment (§8.E4 — operator, 2026-09-01)

- selector drift: NO (mint reproduced the frozen lists exactly).
- zero-volume S3 target: NO (4 and 8).
- guard failure: NO (all three §7.7 guards passed live).
- nonzero expected-zero lane: NO (all eight lanes zero at 120).
- unpredicted exclusion class: NONE at artifact level (per-member
  replay exclusions freeze at P5 per packet §6.7).
- floor miss: NO.
- VERDICT: no trigger fired — Phase P proceeds on this operator
  verification record (packet §8.E4).

## Harness constants for P5 transcription

Per-member replayed/excluded counts and per-root decision-lane
volumes are derived by the P5 harness from the FINAL artifacts and
frozen as test constants (packet §6/§10); this register records the
artifact-level volumes above as their upper envelope.

- 2026-09-04 (h2-7b m-2, fence amendment #5): the W5 stratum pool — the 172 non-refused H2.6c divergence rows this roster was drawn from — is now frozen provenance in `ratchets/h2-7a-w5-stratum-pool.v1.json`; `h2-7a-witnesses.mjs` no longer reads the live 6c manifest (which the H2.7b activation flip regenerated to 318 rows). Witness identity reproduced byte-for-byte (see the h2-7b-m-2 register).
