# H2.6b ca-1 — the inline/root source-map corpus-adoption band observation

Status: design-gate pass (train rung; rides the h2/6b-ca closure train
under the thick-train directive — no solo PR/gate; the full gate runs
once at the train's final head).

## 1. Identity, purpose, and boundary

`h2-6b-ca-1`, kind `evidence`, rung 3 of the h2-6b.md §8 ladder,
mjs-only (no production-crate edit is authorized by this packet). It
freezes the double-observed oracle expectations for the
dependency-closed H2.6b corpus band on the ca-1 pattern whose frozen
precedent is `crates/oracle/h2-6a-qualification.mjs`. The frozen
artifact is ca-2's acceptance input: `run_h2_6b` executes exactly the
admitted rows against these bytes.

## 2. The band (frozen denominator; asserted at every build)

From `ratchets/h2-candidate-dispositions.v1.json`: **11 rows list
`H2.6b`** (compiler 10 / transpile 1). The FROZEN dependency-closed
denominator is **6 — all compiler, all inline-source-map fixtures**
(`inlineSourceMap.ts` ×2 targets,
`jsFileCompilationWithMapFileAsJsWithInlineSourceMap.ts`,
`optionsInlineSourceMapMapRoot.ts`,
`optionsInlineSourceMapSourcemap.ts`,
`optionsInlineSourceMapSourceRoot.ts`); the 5 later-map rows stay
explicitly deferred — 4 blocked on H2.7d, 1 on H2.8c (the 2026-08-27
roadmap review; re-verified against the dispositions at authoring).
Census consts 11/6 are asserted against the pinned dispositions at
every build; a re-census moves them only by re-freezing. Zero project
and zero surviving transpile rows: the project-mount lane is dormant
and excised exactly as 6a-ca-1 §2 (fail-closed census guard,
`project_mount: null`).

Note the band's character: three of the six rows are OPTION-CONFLICT
fixtures (the TS5051/5053/5069 lattice corners the W-H2.6B
`option-conflicts` family froze) — config diagnostics PLUS the emit
shape the conflict resolves to (the inline lane wins the artifact
shape). The machine freezes both.

## 3. The machine

`crates/oracle/h2-6b-qualification.mjs`
(`--preflight|--write|--check` + the internal check-shard mode), cloned
from the frozen h2-6a machine with these deltas and NOTHING else
structural:

1. **Band selection**: rows listing `H2.6b` whose other slices are all
   closed through H2.6a; census consts 11/6/6-compiler asserted.
2. **Option floor**: every selected row must set `inlineSourceMap`
   (machine-verified at preflight over all 6); rows may additionally
   carry `sourceMap`/`mapRoot`/`sourceRoot` — the conflict corners are
   part of the floor, not violations. The 6a floor
   (`sourceMap && !inlineSourceMap`) is REPLACED, not extended.
3. **Effective option census**: machine-measured, machine-printed,
   then pinned as consts (targets/modules per the 6a-ca-1 §3.3
   convention).
4. **Write capture**: identical to 6a (callback `data`,
   `emit_result.source_maps` with one `JSON.stringify` authority);
   inline units have NO `.js.map` write — the artifact's per-case
   write lists carry that shape natively.
5. **Owner closure**: the pinned owner inventory has NO H2.6b-owned
   row (the 6b options orchestrate the H2.6a-owned surfaces) — the
   closure reuses the two `H2.6a` owner rows
   (`source-map-generator`, `source-map-output-path`) verbatim, and
   the packet records that decision here (a future re-inventory that
   adds a 6b-owned row re-freezes through the census guard).
6. **Everything retained unchanged** from the 6a machine: fixture
   loading, `effectiveCompilerOptions`, program construction,
   per-reached-file ownership analysis, double observation in fresh
   pinned processes with the determinism assert, `--write` observation
   reuse, the sharded `--check`, and the gate-tax-3 check receipt
   (`target/h2-6b/check-receipt.v1.json`, kind
   `h2-6b-qualification-check-receipt`; the raw-generator key is
   proportionate at this band size — the gate-tax-5 normalized key
   stays 5g-owned).

Contract: `.github/ci/contracts/h2-6b-qualification.schema.json`
(summary integers pinned as consts from the frozen artifact), registered
in `ARTIFACT_SCHEMA_CONTRACTS` (label "H2.6b qualification") and the
schema-boundary name list. The generator joins the chain-walk ORDER
(after `h2-6b-witnesses`) and the pin-index in the same train.

## 4. Frozen evidence (minted by this rung)

`ratchets/h2-6b-qualification.v1.json`, phase `H2.6b-inline-and-roots`:
6 cases × 2 fresh oracle runs (fingerprint-equal) + the first full
`--check` re-observation minting the receipt. Admitted/deferred split,
write/diagnostic totals, and the conflict-corner diagnostic sets are
machine-measured and pinned by the artifact + schema consts — never
restated by hand here (the falsified-transcription rule).

## 5. Consumption (ca-2 forward pointer)

`run_h2_6b` (ca-2) executes the admitted rows two-run deterministic
against these bytes — writes (paths, bytes, BOM, order), callback
`data`, `emitResult.sourceMaps` incl. inline payloads, `emittedFiles`,
diagnostics — under the COMPLETE 6b option floor (h2-6b.md §8.4
amendments). Any diverging row enters a facet-exact shrink-only
manifest ONLY if the first sweep proves divergence.

## 6. Prohibitions

No production-crate edits; no hand-authored expectations; census guards
move only by re-freezing; the deferred set may only shrink.

## 7. Acceptance (train-internal)

`--preflight` green; `--write` deterministic (12/12 fingerprint-equal
runs); full `--check` green with receipt minted; receipt-hit `--check`
green; the schema/artifact pair passes the shared subset validator.
Slice-level acceptance rides the train head (walk, full gate, hosted).
