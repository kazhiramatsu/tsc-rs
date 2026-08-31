# h2-7a-m-2 — Phase-E frozen-constant register (E4)

Companion to [h2-7a-m-2.md](h2-7a-m-2.md) §10. This file is outside
every generator input closure (see §10 "Register location"); it
records the E3/E4 evidence-derived constants. Values marked FINAL
were read from the last E3 mints; fingerprint rows are filled after
the final assembly mints on this branch.

## S2 selection (mint-reproduced; §8.4-8.6 predictions all hit)

- selector_version: m2-s2-v1; fixtures 16; cases 18; observations 36;
  trim rows: NONE.
- Case IDs: exactly the 18 §8.6 frozen IDs (expando-1..4,
  latebound-1..4, augment-1..4, entityname-1, -2, -3-c0, -3-c1,
  -4-c0, -4-c1).
- Witness totals: cases 112, oracle runs 224, strata S=67 + S2,
  lanes 14, coverage/quota projections = the m-1 94-case set
  (S2 excluded before validation).
- m-1 projection guard: H2_7A_M1_PROJECTION_SHA256 =
  44b0cca40a9ae8869ee219e6bbb6e449ce87346556084dfd622f167bd3f55b72
  (operator-rederived independently; enforced on every
  --write/--check).

## Probe schema-2 volumes (per-member entry/result pairs; FINAL)

isDeclarationVisible 2036; isLiteralConstDeclaration 612;
isExpandoFunctionDeclaration 456; isSymbolAccessible 407;
isOptionalParameter 344; isImplementationOfOverload 192;
isEntityNameVisible 195; requiresAddingImplicitUndefined 133;
isImportRequiredByAugmentation 15;
isDefinitelyReferenceToGlobalSymbolObject 10;
getPropertiesOfContainerFunction 5; isLateBound 4;
getEnumMemberValue 3. resolver.collectLinkedAliases 12;
probe.checkSeed 194; probe.transformSeed 194;
probe.fallbackSweep **0** (zero-fallback confirmed corpus-wide).
Total events 15,845 across 112 cases.

## §8.9 floors (FINAL)

- getPropertiesOfContainerFunction: 5 results, ALL NONEMPTY (floor
  "≥1 nonempty ordered result" MET).
- Every §8.1 target member has ≥1 replayed-eligible event; the sole
  named residual gap remains the
  isDefinitelyReferenceToGlobalSymbolObject `globalThis.Symbol`
  property-access arm (zero population yield, §8.4).

## v1 migration proof (FINAL; §6.6 / disposition 6)

- Decision-projection equality HOLDS. Both sides:
  b5b4516d54dae23f14d54e31abe6e5aefdbe4744809f484b6a7bedcd802fbfac
  (v1 artifact @7e452aa8 vs schema-2 re-observation).
- Excluded capture-upgrade families (both-sides sentinel): transform
  `.changed` output tuple (993 v1 events); syntactic `.result` node
  tuple (498 v1 events). All flags/inputs/hasOriginal/transformFlags
  compared and equal.
- Historical whole-field hashes for the record: v1-of-current
  d166283b… (original projection definition, pre-disposition-6);
  intermediate own-side-only b5b4516d… (= final decision hash).
- Zero-declaration symbol refs: 0. Mint-side node-ref uniqueness:
  passed (no collisions).

## E4 surprise-trigger assessment (§9.E4)

- selector drift: NO (mint reproduces the frozen lists).
- nonzero fallbackSweep: NO (0).
- migration-proof inequality: FIRED, diagnosed, resolved by
  disposition 6 (decision projection). Narrow-scope sol review:
  **AGREE** (2026-09-01, three advisories, recorded here as the
  E-phase continuation of the packet §16 record): (1) bilateral
  projection verified correct; hash and family volumes
  independently reproduced (993 = 415 visitSubtree + 578
  topLevel; 498 = 334 typeOf + 164 returnType). (2) diagnosis
  refined — the syntactic families capture the SAME
  `result = body()` on both schemas; the delta is raw-tuple vs
  provenance/sentinel ENCODING (not a different captured object);
  transform side confirmed via setOriginal+setTextRange
  (:24995-25000): 963 copied-coordinate outputs, 6 synthetic
  kind/-1/-1, 24 absent, all 969 present outputs kind-preserving.
  (3) USE CONSTRAINT: this equality is the one-time m-2 DECISION
  migration proof ONLY — it is NOT evidence of m-3/m-4
  output-node equivalence (whole-tuple normalization could
  conceal output-kind changes at those sites); the m-3/m-3.5/m-4
  design gates must not cite it for output-node claims.
- unpredicted exclusion class: NONE (zero-declaration 0; uniqueness
  clean; synthetic-without-original counts are P4-frozen per §7.5).
- floor miss: NO.

## Final fingerprints (post-final-assembly mints, 2026-09-01; both
bindings machine-verified: probe pins the witness file sha, and the
shared case-manifest fingerprint is identical in both artifacts)

- witness artifact fingerprint:
  5f669ada78346bf938eb3da23de871a4d6426dab401e5ad5274839ca65beca8d
- witness case-manifest fingerprint:
  89bb0627cee58b5d12aeb6fd5e95a92d26e1bbb54fd592750b49a34b64a89efb
- probe artifact fingerprint:
  34a0e69d990022b0a4ecc08e3415261587b66b5968b572bc7f796897de39b9df
- witness artifact file sha256 (pinned by the probe):
  e81e14e6e8de86460d569a0d3b7a8df95be94f18e596e5f79bb8d571ac5a602f
- historical m-1 values (superseded): witnesses 88722786…, manifest
  a70f750c…, probe cbae9498… (and the intermediate first-E3-mint
  values 8c602766…/267b4f65… replaced by the packet-final
  assembly).

## Harness constants for P4 transcription

Per-member replayed/excluded/shadow counts and nested-edge
diagnostic volumes are derived by the P4 harness from the FINAL
artifacts and frozen as test constants (packet §7.5-7.7); this
register records the artifact-level volumes above as their upper
envelope.
