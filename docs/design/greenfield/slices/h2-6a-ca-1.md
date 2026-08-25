# H2.6a ca-1 — the source-map corpus-adoption band observation

Status: design-gate pass + frozen evidence (train rung; authored on the
`h2/6a-ca-prep` worktree branch and merged into the h2/6a closure train
under the 2026-08-25 thick-train directive — no solo PR/gate; the full
gate runs once at the train's final head).

## 1. Identity, purpose, and boundary

`h2-6a-ca-1`, kind `evidence`, rung 4 of the h2-6a.md §8 ladder,
mjs-only (no production-crate edit is authorized by this packet). It
freezes the double-observed oracle expectations for the
dependency-closed H2.6a corpus band, on the CA-1/CA-3 pattern whose
frozen precedent is `crates/oracle/h2-5h-qualification.mjs`. The frozen
artifact is ca-2's acceptance input: `run_h2_6a` executes exactly the
admitted rows against these bytes.

## 2. The band (measured at authoring, asserted at every build)

From `ratchets/h2-candidate-dispositions.v1.json` (the pinned global
census, 15,642 rows): **630 rows list `H2.6a`** in `required_slices`
(compiler 192 / conformance 32 / project 404 / transpile 2 — the
ladder's superset numbers). Dependency closure — every other required
slice ∈ {H2.1a..H2.5h} — keeps **177 rows: compiler 150 + conformance
27**, and **zero project or transpile rows** (the 404 project rows all
carry unlanded H2.7b/H2.7d/H2.8a owners; blocker census: H2.7b 434,
H2.7d 170, H2.8a 154, H2.7e 6, H2.6b 4, H2.8c 3, H2.8b 2 across the
non-closed 453). The exact row list is owned by the generator's
selection predicate over the pinned dispositions artifact — the
artifact IS the appendix (`selection_origin:
"global-h2-6a-candidate"` on all 177 case records).

Because zero project rows survive closure, **the CA-3 project-mount
lane is dormant and excised** from the machine: `projectRows.length
=== 0` is a census guard that FAILS the build if a re-census ever adds
one (fail-closed re-opening, never silent skipping), and
`project_mount` is `null` in the artifact.

## 3. The machine

`crates/oracle/h2-6a-qualification.mjs`
(`--preflight|--write|--check`, plus the internal check-shard mode),
cloned from the frozen h2-5h machine with these deltas and NOTHING
else structural:

1. **Band selection**: rows listing `H2.6a` whose other slices are all
   closed through H2.5h; census consts 630/177/150/27 asserted against
   the pinned dispositions at every build.
2. **Option floor**: every selected row must carry `sourceMap: true`
   and no `inlineSourceMap` (replaces the 5h ES5-target floor — the
   source-map machinery is target-independent). Verified over all 177
   at preflight and every build.
3. **Effective option census** (machine-measured, machine-printed,
   then pinned): targets ES2015(2)×125, ES5(1)×51, ESNext(99)×1;
   modules absent×161, CommonJS(1)×9, AMD(2)×5, ES2015(5)×2. Note: no
   `module: System` row exists — the m-3 system-splice typed refusal
   is unreachable in this band (packet h2-6a-m-3.md §3.1 adjudicated:
   the refusal stands, no line-delta shift is needed for H2.6a).
4. **Write capture extension**: `serializeWrite` records the write
   callback's `data` argument (`data_present`,
   `data_source_map_url_pos`, `data_diagnostics_count`) — the
   witness-machine convention; and `emit_result.source_maps` entries
   freeze as `{ input_source_file_names, source_map_json }` with ONE
   `JSON.stringify` authority per entry.
5. **Owner closure**: the single `source-map-generator` row of the
   pinned owner inventory (owner_slice `H2.6a`), replacing the 5h
   transform-owner pair.
6. **Everything retained unchanged**: fixture loading (@option
   parsing, multi-unit splitting, symlink documents, virtual configs),
   `effectiveCompilerOptions`, program construction, the per-reached-
   file ownership analysis (feature roots, output-kind owners, parse
   diagnostics → H2.9, AST-depth guard, import attributes, advanced
   comment placement), double observation in fresh pinned processes
   with the determinism assert, `--write` observation reuse, the
   4-shard `--check`, and the gate-tax-3 check receipt
   (`target/h2-6a/check-receipt.v1.json`, kind
   `h2-6a-qualification-check-receipt`).

Contract: `.github/ci/contracts/h2-6a-qualification.schema.json`
(summary integers all pinned as consts from the frozen artifact;
case/write/emit-result shapes tightened to the 6a forms), registered
in `ARTIFACT_SCHEMA_CONTRACTS` (label "H2.6a qualification") and the
schema-boundary name list.

## 4. Frozen evidence (minted by this rung)

`ratchets/h2-6a-qualification.v1.json`, phase `H2.6a-source-map`:

- 177 cases, 354 fresh oracle runs (each case ×2, fingerprint-equal),
  plus a full fresh sharded re-observation on the first `--check`
  (receipt then minted; the receipt-hit path re-verifies bytes and
  adopts 177 stored observations).
- **175 admitted-for-execution / 2 deferred-to-slices**, both
  deferrals `typescript-6.0.3/compiler/unicodeEscapesInNames02.ts`
  (`#target=es2015`, `#target=es5`) with first owner **H2.9** (parse
  diagnostics in the fixture — the recovery lane, consistent with the
  witness machine's `fault-parse-error` typed deferral).
- Admitted totals: 420 writes (js + `.js.map` pairs and singles per
  the oracle's own gating), 119 reported diagnostics; zero
  output-control cases; zero virtual-config and zero symlink cases in
  this band.
- **MetaProperty census: ZERO band inputs contain `new.target` or
  `import.meta`** — the m-2 §12c MetaProperty deferral does NOT land
  in this burn-down; it stays deferred until a later band carries it
  (recorded here so ca-2's manifest starts empty of MetaProperty
  expectations rather than silently absorbing them).

## 5. Consumption (ca-2 forward pointer)

`run_h2_6a` (ca-2) executes the 175 admitted rows two-run
deterministic against these bytes — writes (paths, bytes, BOM, order),
callback `data`, `emitResult.sourceMaps`, `emittedFiles`, diagnostics
— and asserts the 2 deferred rows fail typed before the first sink
write (the COUNT-ONLY deferred rule from run_h2_5h). Any diverging row
enters a facet-exact shrink-only manifest ONLY if the first sweep
proves divergence.

## 6. Prohibitions

No production-crate edits; no hand-authored expectations (every byte
from the vendored oracle, every count machine-measured); the census
guards may only move by re-freezing against a re-censused dispositions
artifact; the CA-3 lane re-opens only through the fail-closed census
guard; the deferred set may only shrink (H2.9 lands last).

## 7. Acceptance (train-internal)

`--preflight` green; `--write` deterministic (354/354 fingerprint-equal
runs); full sharded `--check` green with receipt minted; receipt-hit
`--check` green; the schema/artifact pair passes the shared subset
validator (`validateArtifactSchemaContracts` runs green at the train
head once the mid-walk 5g staleness this branch inherited is
re-converged — the pair was validated directly at authoring).
Slice-level acceptance rides the train head (walk, full gate, hosted).
