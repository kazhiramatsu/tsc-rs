# h2-7b-m-1 — Phase-E/P register (values recorded as minted; `__X__` = TO-FILL at the named step)

## Design gate
- Packet rev 3 RATIFIED 2026-09-03 (sol r1 REVISE 13+1 → r2 REVISE 5 partial → r3 AGREE). Authority file: `docs/design/greenfield/slices/h2-7b-m-1.md`.
- Frozen order: the 6c repair train (`fix/h2-6c-acceptance-wiring`) → this rung → m-2. Trusted base for the train: the repair-train merge sha `__REPAIR_MERGE__`.
- Registration base at the train cut: 21 contracts (19 + the two 6c pairs); this rung → 22.

## The band (measured at authoring 2026-09-03; asserted at every build)
GLOBAL 2,456 (compiler 1,034 / conformance 861 / project 528 / transpile 33); CANDIDATE 1,593 (921 / 476 / 196 / 0); FROZEN_NEXT_SLICE 1 (`compiler/modulePreserve4.ts#default`); dispositions `cases` roll `ed0036eb…`.
First-cut census (settings + matrix, pre virtual-config merge): compiler deferred ≥ 25 (isolatedDeclarations 21, noEmitOnError 2, stripInternal 1, standalone emitDeclarationOnly 1) + explicit-false declarationMap 10 (admitted) + removeComments 60 (4 true / 56 false, admitted facet); conformance deferred ≥ 1 (stripInternal); project: 22 config-driven rows TO-VERIFY; six `.d.ts`-only project rows = admitted `no-emit-eligible-source` controls.

## Machine (first mint) — TO-FILL
- `ADMITTED_H2_7B_ROWS` / `DEFERRED_H2_7B_ROWS` per suite: `__`; `typescript_runs` (= 2 × admitted): `__`; declaration writes: `__`; emit_refused: `__`; first_deferred_slices: `__`
- generator sha256 `__GEN__`; contract sha256 `__CONTRACT__`; `qualification_fingerprint_sha256` `__FP__`; check receipt minted: `__`; second `--check` hit: `__`
- resource measurements at `--preflight` / `--probe project:3` / `--probe compiler:3`: wall `__`, per-process RSS `__` (ceilings 4 GB / 12 GB), `--write` wall `__` (STOP 40 min)
- owner_arms amendment: h2-transition outputs regenerated (`owner_roots` 50 / `owner_arms` 4): `__`; the H2.7a inventory span pin :448-467 byte-identical + line numbers verified: `__`; h2-7a-close re-minted pin-only: `__`; 6c pass count in the walk: `__`

## Walk / gate / PR — recorded in the PR body at the final head
