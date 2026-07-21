# Phase-9 2XXX sweep — worklist and slice plan

Landing-order row 9 ([completion-convergence-plan.md](completion-convergence-plan.md)
§4): pin 2XXX scope, then sweep to all-corpus FP=0 and supported
FN=0, BEFORE M7. Content charter:
[2xxx-first-order.md](2xxx-first-order.md) phase 9 (first half —
band expansion is M7/M8). Working style: the README M8 mining loop
(snapshot → top one-sided codes → owner → smallest probe → port →
gate) restricted to the 2XXX band. This doc owns the phase's mined
worklist, slice order, and adjudication protocol; steps-doc
semantics stay with the m*-steps docs each slice cites.

Process anchors:

- STAGE stays `M6` for the whole phase (phase 9 is a row between
  milestones, not a milestone; `escapes --stale M6` must stay green
  throughout — new escapes take owner M7/M8).
- Branches: `p9/<slice>` (e.g. `p9/9.1a-tuple-renderer`).
- The A2 band pin lands LAST (slice 9.9). Under
  [measurement-integrity.md §3.1](measurement-integrity.md#31-draft-band-pins)
  a pinned band rejects in-band additions and the pinned set must
  equal the band subset at its adjudication commit — so every
  exclusion must be adjudicated before the pin, and the pin commit
  is the sweep's closing artifact. M7 start requires the row-9 gate
  AND this pin green.
- Gate at close: `conformance --band 2xxx` all-corpus FP=0,
  supported T0-2xxx = 100% (excluded records stay FN in the
  all-corpus view by design).

## Baseline (2026-07-21, main @5277ae79, measured)

```text
conformance band=2xxx fixtures=5908 cases=7691 T0=77.5165%
  matched=16318/21051 FP=0 FN=4733 mismatches=1027
FN partial-boundary audit: reached=3571 no-evidence=1162
M8 scope: draft, entries=0 (nothing adjudicated yet)
```

Top FN codes: 2322×1006, 2304×968, 2339×363, 2307×289, 2345×236,
2365×157, 2834×120, 2367×114, 2741×100, 2694×90, 2343×83, 2835×80,
2411×70, 2353×62, 2373×60, 2403×57, 2493×39, 2503×39, 2300×35,
2769×35 (top-20 = 4,003; tail = 135 more codes, 730 rows).

## The partition (attack families)

Every FN carries either a curtain reason (reached=3,571) or no
evidence (1,162). The mass partitions into seven families; counts
are the baseline measurement, re-mined per slice:

| # | Family | FN | Evidence / curtain string | Fix axis |
|---|---|---:|---|---|
| F1 | Display curtain | ~1,808 | `typeToString beyond the 5.4 display slice` (1,543) + tuple renderer (190) + operator-display identically-named (38) + 2507 display (31) + origin-union (6) | Port the nodeBuilder/typeToString slice to T0-emission grade, shape by shape (`check.rs` `type_to_string_slice_ex`); every widening needs the 7.5 fabrication audit |
| F2 | Parse-recovery overload band | 1,102 | `overload band over a parse-recovery tree` (887 of 2304, plus 2389/2391/2392/2393-family) | Parser-recovery exactness: make recovery-tree declaration/body boundaries tsc-exact, then narrow the `functions.rs` `checkFunctionOrConstructorSymbol` bail. tsc emits these bands in errored files — the divergence is our tree (2xxx-first-order.md deviation 1) |
| F3 | Unported type families | 276 | `mapped types (M8-stub)` (185) + `conditional types (M8-stub)` (91) | Port mapped + conditional types (type-from-node, instantiation, relations); the M8-stub escapes retire early |
| F4 | Elaboration engine | ~200 | `elaborateJsxComponents`/JSX attributes (105) + `elaborateObjectLiteral` (42) + `getBestMatchingType` (30) + `elaborateArrayLiteral` (19, incl. forceTuple 4×2322) + `elaborateArrowFunction` (4) | Port elaborateError chain — elaboration REPLACES the parent-position row with child-position rows, so T0 needs it; depends on F1 for message args |
| F5 | JS/checkJs band (non-JSDoc) | ~161 | `binary expando analysis` (125) + `expando-function member assignment` (19) + `getContextualType parenthesized JSDoc arms` (12) + `isJSConstructor` (5) | Implement non-JSDoc assignment-declaration semantics (contract keeps them IN scope); JSDoc-DRIVEN rows go to adjudication (F7) |
| F6 | Rule-gap residue (no-evidence, in-scope) | ~673 | No boundary evidence: 2343×83 (checkExternalEmitHelpers — probe-confirmed in-program tslib fixtures), 2373×60, 2304×68, 2694×42, 2322×59, 2339×34, 2305×31, 2372×17, 2441×16, 2300×15, 2882×13, 2345×12 + tail | Rule-by-rule M8-loop mining: smallest probe → port the emitting branch |
| F7 | Adjudication candidates | ~489 | No-evidence 2307×289 (conformance/node 228, nonjsExtensions 33) + 2834×120 + 2835×80 | A2 exact-identity exclusions under the out-of-scope contract; gray-zone rule below |
| — | NoInfer intrinsic | 9 | `NoInfer (Substitution types, M8)` | Rides F3 (Substitution machinery) or stays M8 with the rows adjudicated impossible-before-M8 — decide at 9.3 |

Family sizes are baseline attributions, not budgets: retiring a
curtain can surface latent FPs (fix as bugs) or reveal deeper
misses re-attributed to another family. The per-slice re-measure is
the truth.

## Slice plan

Order rationale: F1 first — largest single mass, it is the M6-close
re-owned dependency set (tuple renderer et al.), it unmasks latent
wrong verdicts EARLY (7.5 precedent: widening "{}" unmasked 5
fabrications — better surfaced at 9.1 than at 9.9), and F4/F6 rows
cannot emit without rendering. F2 is an independent parser axis and
can interleave. Adjudication (9.6/9.7) runs late so implementable
rows are not prematurely excluded, but BEFORE the residue slice so
9.8 mines a purely in-scope remainder.

| Slice | Content | Exit gate (all: FP=0 all bands, ratchets bumped, ci green) |
|---|---|---|
| 9.1a | Tuple renderer (`symbol-less reference display` curtain, 190 rows; M7-re-owned escape row retires) + intersection display + contextual tuple arity + computed-key destructuring rows from the same M6-close re-owned set | Curtain rows flip matched; fabrication audit on every widened arm |
| 9.1b-9.1x | typeToString shape ladder: mine the 1,543-row curtain by blocking type shape (debug census), then widen shape by shape (references w/ symbols, unions/intersections, anonymous object literals, signatures, indexed access, …) | Per-shape: curtained rows flip, `2xxx` band T0 monotone, fabrication audit each arm |
| 9.2 | Elaboration engine (F4): elaborateError → elaborateObjectLiteral/ArrayLiteral/ArrowFunction/JsxComponents + getBestMatchingType + reportRelationError head selection | The ~200 elaboration rows; forceTuple escape row retires |
| 9.3 | Mapped types (F3a, 185 rows): getTypeFromMappedTypeNode, instantiation, apparent members, relations; NoInfer decision | M8-stub escape narrows/retires; no new unrenderable shapes (F1 done) |
| 9.4 | Conditional types (F3b, 91 rows): resolution, distribution, infer positions (M6 infer machinery is live) | M8-stub escape narrows/retires |
| 9.5 | Parse-recovery overload band (F2): recovery-boundary parity work in the parser + narrow the functions.rs bail; fixture-driven (conformance/parser 1,181 dir rows) | 887-row 2304 mass + overload-band rows flip; syntactic T0 ≥ 99.8219% held |
| 9.6 | JS band (F5): non-JSDoc expando/assignment-declaration semantics implemented; JSDoc-driven rows adjudicated `jsdoc-semantics` with exact identities | F5 rows flip or carry exclusion records |
| 9.7 | Host adjudication (F7): per-record 2307/2834/2835 under `host-resolution`; gray-zone rule: a relative import whose TARGET FILE IS IN-PROGRAM is implementable (extension probing over program files — includes nonjsExtensions 2307s and in-program 2834/2835) and is NOT excludable; package/node_modules/exports-mediated resolution is | Every excluded record an exact A2 identity; remainder implemented or moved to F6 |
| 9.8 | Residue mining (F6 + everything the re-measures re-attributed): rule-by-rule to supported FN=0 | `conformance --band 2xxx` supported FN=0 |
| 9.9 | Band pin: A2 `2xxx` band-freeze record (band, adjudication commit = the slice's full 40-hex SHA, complete enumerated identity set) + close re-measure | Row-9 gate green: all-corpus 2XXX FP=0, supported T0-2xxx=100%, `scope audit` green, pin verified vs trusted base |

Slices 9.3/9.4/9.5 are order-independent among themselves; 9.1
before 9.2/9.6/9.8 is a real dependency (rendering); 9.7 before 9.8
keeps the residue in-scope-pure; 9.9 strictly last.

## Adjudication protocol (9.6/9.7 and any stragglers)

- Contract classes only ([definition-of-done.md](definition-of-done.md)):
  `host-resolution`, `jsdoc-semantics` (+ emit-dependent, which does
  not occur in-band). No new classes without a design review.
- Per-record: every exclusion is one exact schema-2 oracle-record
  identity; duplicate buckets need multiplicity-complete handling
  (65 of the 68 permanent duplicate canaries are in-band).
- Gray-zone rules of record:
  - 2343/2354 import-helpers rows are IN scope (probe 2026-07-21:
    the esDecorators fixtures define their own in-program
    `tslib.d.ts` — checkExternalEmitHelpers is an ordinary rule
    gap, F6).
  - Relative-path 2834/2835/2307 with an in-program target: IN
    scope (the resolver sees program files; no host probing
    involved). Excludable only when resolution is mediated by
    node_modules/package.json/exports/`/// <reference>` redirects.
  - JSDoc boundary: JSDoc-DRIVEN semantics (types read from JSDoc
    tags) excludable; non-JSDoc assignment-declaration semantics
    never excludable (contract line, verbatim).
- Later-resolved exclusions follow §3.2 tombstones (A1 2XXX-view
  membership proof; the early 2XXX pin reads A1's 2XXX view).

## Working rules

- Curtain retirement = FP-shield removal. Every widened display arm
  or narrowed bail runs the 7.5 fabrication audit: corpus diff at
  the arm, any NEW port-only row probed against the oracle before
  landing (verdict-pin technique where display still curtains the
  relation outcome). FP=0 is the gate, not a hope.
- Re-measure per slice (`conformance --band 2xxx` to a file, exit
  code checked); re-attribute the partition in this doc's slice PR
  when counts move materially. mismatches.json is regenerable —
  numbers in this doc are snapshots, the tool is the truth.
- One slice = one branch = one PR ([CLAUDE.md](../../../CLAUDE.md)
  workflow); ratchet.toml `[t0-2xxx]` + set-ratchet bumps ride the
  slice, never the merge.
- Escape hygiene: F1/F2 narrowings shrink existing curtain sites —
  after each, `escapes --write-manifest` and review the manifest
  diff; retired M7-re-owned rows are the visible progress ledger.
- Stop conditions unchanged (convergence plan §6): an exclusion that
  cannot select exactly one record, or three fixes hitting one
  model ceiling, stops the slice for design review.
