# Phase-9 2XXX sweep — worklist and slice plan

Landing-order row 9 ([completion-convergence-plan.md](completion-convergence-plan.md)
§4): **pin 2XXX scope, then sweep** to all-corpus FP=0 and supported
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
- Branches: `p9/<slice>` (e.g. `p9/9.1a-host-adjudication`).
- **Adjudication and the A2 band pin land FIRST, before any
  implementation slice** — row 9's own order, and the point of A2:
  the exclusion set is CAPPED before implementation results exist,
  so the sweep can never quietly shrink its target. Post-pin the
  supported denominator moves in one direction only — UP, as
  resolved exclusions return via §3.2 tombstones; it is a ceiling
  on exclusions, not a frozen denominator. Under
  [measurement-integrity.md §3.1](measurement-integrity.md#31-draft-band-pins)
  a pinned band rejects in-band additions — post-hoc exclusion is
  impossible by design. An exclusion resolved later (the record
  becomes implemented anyway) leaves via a
  [§3.2 tombstone](measurement-integrity.md#32-resolution-tombstones)
  with A1 2XXX-view membership proof; a MISSED exclusion discovered
  mid-sweep cannot be added and is a stop condition (design
  review). This asymmetry is what forces the 9.1 adjudication pass
  to be rigorous.
- Gate at close: `conformance --band 2xxx` all-corpus FP=0,
  supported T0-2xxx = 100% (excluded records stay FN in the
  all-corpus view by design). M7 start requires this gate AND the
  pin green.

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
| F3 | Unported type families | 285 | `mapped types (M8-stub)` (185) + `conditional types (M8-stub)` (91) + `NoInfer intrinsic (Substitution types)` (9) | Port mapped + conditional types AND the Substitution/NoInfer machinery (type-from-node, instantiation, relations); the M8-stub escapes retire early. NoInfer is MANDATORY here: its 9 rows are ordinary 2322/2345/2353/2741 on noInfer.ts and fit no contract class — exclusion is impossible, so supported FN=0 requires the implementation |
| F4 | Elaboration engine | ~200 | `elaborateJsxComponents`/JSX attributes (105) + `elaborateObjectLiteral` (42) + `getBestMatchingType` (30) + `elaborateArrayLiteral` (19, incl. forceTuple 4×2322) + `elaborateArrowFunction` (4) | Port elaborateError chain — elaboration REPLACES the parent-position row with child-position rows, so T0 needs it; depends on F1 for message args |
| F5 | JS/checkJs band (non-JSDoc) | ~161 | `binary expando analysis` (125) + `expando-function member assignment` (19) + `getContextualType parenthesized JSDoc arms` (12) + `isJSConstructor` (5) | Implement non-JSDoc assignment-declaration semantics (contract keeps them IN scope); JSDoc-DRIVEN rows go to adjudication (9.1) |
| F6 | Rule-gap residue (no-evidence, PRESUMED in-scope) | ~673 | No boundary evidence: 2343×83 (checkExternalEmitHelpers — probe-confirmed in-program tslib fixtures), 2373×60, 2304×68, 2694×42, 2322×59, 2339×34, 2305×31, 2372×17, 2441×16, 2300×15, 2882×13, 2345×12 + tail | Rule-by-rule M8-loop mining: smallest probe → port the emitting branch. "Presumed": 9.1's full scan re-bins the host/jsdoc rows hiding here — bundlerNodeModules1.ts 2305×6 (node_modules `exports`-mediated) inside "2305×31" is the known example |
| F7 | Adjudication candidates (3-code FLOOR, not the set) | ~489 | No-evidence 2307×289 (conformance/node 228, nonjsExtensions 33) + 2834×120 + 2835×80 | A2 exact-identity exclusions under the out-of-scope contract; gray-zone rule below. The floor is where 9.1a STARTS — its scan covers all 4,733 because class membership rides the resolution path, not the code |

Family sizes are baseline attributions, not budgets: retiring a
curtain can surface latent FPs (fix as bugs) or reveal deeper
misses re-attributed to another family. The per-slice re-measure is
the truth.

## Slice plan

Order rationale: adjudication + pin first is the contract (see
Process anchors — the exclusion ceiling must predate
implementation results). Premature exclusion of implementable rows
is prevented by the contract-class test itself, not by deferring
adjudication: a record is excludable only for WHAT IT IS
(host-resolution / jsdoc-semantics / emit-dependent), never for
"unimplemented/hard", and gray-zone records that cannot select
exactly one disposition stop the slice. Among implementation
slices, F1 display goes first — largest single mass, it is the
M6-close re-owned dependency set (tuple renderer et al.), it
unmasks latent wrong verdicts EARLY (7.5 precedent: widening "{}"
unmasked 5 fabrications — better surfaced at 9.3 than at 9.9), and
F4/F6 rows cannot emit without rendering.

| Slice | Content | Exit gate (all: FP=0 all bands, `ratchet check` green — integer/set bumps only where accepted identities grew; scope-only slices leave ratchet artifacts unchanged; ci green) |
|---|---|---|
| 9.1a | Host adjudication: START from the F7 3-code mass (2307/2834/2835) but SCAN ALL 4,733 band FNs — host-resolution is a property of the record's RESOLUTION PATH, not of its code, and any code can ride a host-mediated resolution (recorded counterexample: bundlerNodeModules1.ts 2305×6 through node_modules/package.json `exports`, baseline-binned inside F6's "2305×31"). Gray-zone rule: a relative import whose TARGET FILE IS IN-PROGRAM is implementable (extension probing over program files — includes nonjsExtensions 2307s and in-program 2834/2835) and is NOT excludable; package/node_modules/exports-mediated resolution is. Entries land in the draft manifest | Every excluded record an exact schema-2 A2 identity; `scope audit` green; non-excluded remainder explicitly re-binned to F6 |
| 9.1b | JSDoc adjudication: same full-4,733 scan (reached AND no-evidence — a jsdoc-fixture row behind the display curtain is classified by the ORACLE record's nature, not by our curtain reason) for JSDoc-DRIVEN semantics; non-JSDoc assignment-declaration rows stay IN (contract line, verbatim) | Same per-record discipline; supported FN re-measured from the tool's `supported_false_negative_diagnostics` (never derived from record counts — bucket semantics, see protocol) |
| 9.2 | **Band pin**, two changes per [§1.2](measurement-integrity.md#12-reviewed-snapshot-anchor): (1) the final adjudicated content lands while the manifest is `draft`; (2) a follow-up change records that adjudication commit (full 40-hex SHA of change 1) + the complete enumerated identity set as the `2xxx` band-freeze record | `scope audit` green incl. pin verification vs trusted base; from here the pinned exclusion set is a CEILING — no in-band exclusion can be added, while the supported denominator may still GROW back toward the full band as resolved exclusions return via §3.2 tombstones |
| 9.3a | Tuple renderer (`symbol-less reference display` curtain, 190 rows; M7-re-owned escape row retires) + intersection display + contextual tuple arity + computed-key destructuring rows from the same M6-close re-owned set | Curtain rows flip matched; fabrication audit on every widened arm |
| 9.3b-x | typeToString shape ladder: mine the 1,543-row curtain by blocking type shape (debug census), then widen shape by shape (references w/ symbols, unions/intersections, anonymous object literals, signatures, indexed access, …) | Per-shape: curtained rows flip, `2xxx` band T0 monotone, fabrication audit each arm |
| 9.4 | Elaboration engine (F4): elaborateError → elaborateObjectLiteral/ArrayLiteral/ArrowFunction/JsxComponents + getBestMatchingType + reportRelationError head selection | The ~200 elaboration rows; forceTuple escape row retires |
| 9.5 | Mapped types (F3a, 185 rows): getTypeFromMappedTypeNode, instantiation, apparent members, relations | M8-stub escape narrows/retires; no new unrenderable shapes (F1 done) |
| 9.6 | Conditional types (F3b, 91 rows): resolution, distribution, infer positions (M6 infer machinery is live) + **Substitution/NoInfer (mandatory, 9 rows)** — NoInfer lands with whichever of 9.5/9.6 carries the Substitution machinery; it may not slip to M8 | M8-stub escapes narrow/retire; NoInfer rows flip matched |
| 9.7 | Parse-recovery overload band (F2): recovery-boundary parity work in the parser + narrow the functions.rs bail; fixture-driven (conformance/parser 1,181 dir rows) | 887-row 2304 mass + overload-band rows flip; syntactic T0 ≥ 99.8219% held |
| 9.8 | JS band (F5): non-JSDoc expando/assignment-declaration semantics implemented (JSDoc-driven rows were excluded at 9.1b) | F5 in-scope rows flip |
| 9.9 | Residue mining (F6 + everything the re-measures re-attributed) rule-by-rule to supported FN=0; close re-measure | Row-9 gate green: all-corpus 2XXX FP=0, supported T0-2xxx=100%, `scope audit` green |

9.1a/9.1b may interleave but both strictly precede 9.2; 9.2
strictly precedes every implementation slice. Among
implementations: 9.3 before 9.4/9.8/9.9 is a real dependency
(rendering); 9.5/9.6/9.7 are order-independent among themselves and
against 9.4.

## Adjudication protocol (9.1, binding for the whole phase)

- Contract classes only ([definition-of-done.md](definition-of-done.md)):
  `host-resolution`, `jsdoc-semantics` (+ emit-dependent, which does
  not occur in-band). No new classes without a design review.
  "Unimplemented", "hard", or "blocked before M8" are NOT classes —
  such rows stay in the supported denominator and must be
  implemented (NoInfer is the recorded example).
- Per-record: every exclusion is one exact schema-2 oracle-record
  identity; duplicate buckets need multiplicity-complete handling
  (65 of the 68 permanent duplicate canaries are in-band).
- The pass sweeps ALL 4,733 band FNs for contract-class membership,
  independent of curtain attribution AND independent of diagnostic
  code: membership is decided by the record's resolution path /
  semantic origin, never by its code (the bundlerNodeModules1.ts
  2305 rows are the recorded exemplar — member-of-module errors
  whose module arrives through node_modules/package.json `exports`
  are host-resolution even though 2305 is "in-scope" as a code).
- Exclusion-record counts never convert to FN integers: T0 is
  bucket-granular and 2XXX holds 65 duplicate buckets, so removing
  one occurrence leaves its bucket in the supported denominator and
  removing a whole bucket subtracts differently than its record
  count. What the pin fixes is the identity SET; every supported-FN
  number is read from the conformance summary
  (`supported_false_negative_diagnostics`), never derived.
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
- Pin mechanics (9.2): the two-change protocol of
  [§1.2](measurement-integrity.md#12-reviewed-snapshot-anchor) —
  content commit first, pin-record commit second referencing the
  content commit's SHA. A commit cannot reference its own hash;
  the pin is never a single commit.
- Post-pin discipline: in-band additions FAIL (§3.1). A record that
  should have been excluded but was not is a stop condition — the
  design-review outcome decides, it is never patched around. An
  excluded record later implemented resolves via §3.2 tombstone
  (A1 2XXX-view membership proof; the early 2XXX pin reads A1's
  2XXX view).

## 9.1a results (2026-07-21, host adjudication — DONE)

Full code-blind scan of all 4,733 band FNs (1,027 cases), four passes:

1. **Fixture feature scan** (node_modules/@filename, package.json files,
   host pragmas baseUrl/paths/rootDirs/typeRoots/types,
   `/// <reference path|types>`, bare-vs-relative specifiers,
   in-program `declare module` ambients): 708 cases / 2,682 FN carry
   ZERO module machinery; 319 cases / 2,051 FN queued.
2. **Green safety net**: full-chain-text vocabulary screen (all nested
   chain levels) over every green FN record — 0 hits.
3. **Queue adjudication**: strong-host cases (206 / 1,869 FN) record by
   record against fixture + golden chain text; weak cases (113 / 182 FN)
   text-screened — 17 suspicious, all relative-or-unmaterialized-bare
   failures, all IN.
4. **Import-helpers provenance audit** (every 2343/2354/2807 FN
   corpus-wide): tslib is in-program (file or ambient) everywhere except
   the two privateName fixtures (node_modules/tslib). The 2343/2354
   rule of record holds universally; 2807 splits by provenance.

Oracle probes (vendored 6.0.3, expand + driver.mjs; recipes reproducible
from the fixture shapes named here):

- **P1/P2**: the `nodeModules*TypeModeDeclarationEmitErrors` shapes with
  pkg made UNRESOLVABLE → 2353/2559/2538 still fire (arg-vs-
  `ImportCallOptions`, index-type rows) and 2339 still fires at the same
  span as `Promise<any>`; the 2694 rows VANISH (replaced by
  2307+1455/1456+2880). ⇒ 2694 needs the exports-resolved namespace
  (EXCLUDE); the siblings are resolution-independent (IN).
- **P3**: jsx fixtures' `/// <reference path="/.lib/react.d.ts" />` is
  NOT materialized by the harness expander — the oracle itself checks
  with react unresolved, so the jsx-queue records are local-type
  relations (IN). (Verify by expanding any jsx fixture and reading the
  program.json files list.)

Probe fixtures, verbatim (re-run recipe:
`cargo xtask expand <file> --out-dir <dir>` then pipe
`{"id":1,"programJsonPath":"<dir>/program.json"}` into
`node crates/oracle/driver.mjs`). P1 — the DeclarationEmitErrors
`/other.ts`+`/other3.ts`+`/other5.ts` shapes with NO node_modules
materialized:

```ts
// @target: es2022
// @strict: false
// @module: node16
// @declaration: true
// @outDir: out
// @filename: /other.ts
// missing assert:
export type LocalInterface =
    & import("pkg", {"resolution-mode": "require"}).RequireInterface
    & import("pkg", {"resolution-mode": "import"}).ImportInterface;

export const a = (null as any as import("pkg", {"resolution-mode": "require"}).RequireInterface);
export const b = (null as any as import("pkg", {"resolution-mode": "import"}).ImportInterface);
// @filename: /other3.ts
// Array instead of object-y thing
export type LocalInterface =
    & import("pkg", [ {"resolution-mode": "require"} ]).RequireInterface
    & import("pkg", [ {"resolution-mode": "import"} ]).ImportInterface;

export const a = (null as any as import("pkg", [ {"resolution-mode": "require"} ]).RequireInterface);
export const b = (null as any as import("pkg", [ {"resolution-mode": "import"} ]).ImportInterface);
// @filename: /other5.ts
export type LocalInterface =
    & import("pkg", { assert: {} }).RequireInterface
    & import("pkg", { assert: {} }).ImportInterface;

export const a = (null as any as import("pkg", { assert: {} }).RequireInterface);
export const b = (null as any as import("pkg", { assert: {} }).ImportInterface);
```

Observed (vendored 6.0.3): `/other.ts` 2307×3 + 2353@138 + 2339@168
(`Promise<any>`) + parse-band rows; `/other3.ts` 2307×3 + 2538×3 +
2559@157 + 2339@192; `/other5.ts` 2307×4 + 2880×4 + 1456×4 — **no
2694 anywhere**. P2 — the `/other2.ts` shape alone (same pragma
header, `{ assert: {"bad": "require"} }` / `{"bad": "import"}` in the
four positions): 2307×4 + 2880×4 + 1455×4, no 2694. The resolvable
originals (corpus fixtures `nodeModulesImportTypeModeDeclarationEmitErrors1.ts` /
`nodeModulesImportAttributesTypeModeDeclarationEmitErrors.ts`) emit
2694 at those positions instead.

**Exclusions landed: 303 records / 39 fixtures / 12 codes** (snapshot;
the manifest is the identity authority): 2307×214, 2694×32, 2305×31,
2877×6, 2792×6, 2807×4, 2339×3, 2748×2, 2322×2, 2665×1, 2882×1, 2688×1.
Mediation families: exports/imports-map interpretation (patterns,
conditions, typesVersions, self-reference, `#`-imports), member/shape
verdicts on node_modules-resolved modules (incl. untyped and
node_modules tslib), 2792-vs-2307 code choice (the alternate-resolver
probe SUCCEEDING against materialized node_modules), and
typeRoots/`/// <reference types>` probing (2688). All entries
`reason: host-resolution`; the two `jsdoc/importTag17.ts` rows note the
JSDoc overlap (host primary).

Boundary refinements of record (bind 9.1b and later slices):

- package.json read as RESOLUTION REDIRECTOR (main/types/exports/
  imports/typesVersions/self-name) = host; nearest in-program
  package.json `"type"` as FORMAT input for in-program files = IN scope
  (the recorded 2834/2835 gray-zone rule depends on it; format-only
  rows like the GeneratedNameCollisions 2441/2725 ride the same read).
- A failing bare specifier with nothing materialized is IN scope (the
  supported resolver reaches the same verdict over program files);
  the SAME code fails over to EXCLUDE only when a host mechanism
  produced the verdict (e.g. exports-blocked subpaths).
- Rows probe-proven to fire at the same T0 key without resolution stay
  IN even when their oracle message embeds a resolved type (message
  drift is a T1+ concern, not phase-9's).
- `/// <reference types>`/typeRoots directive-outcome rows (2688) are
  EXCLUDE; reference directives to ABSENT files materialize nothing in
  the oracle harness either, so downstream name-lookup rows (parser
  realsource 2304 mass, jsx queue) are IN.

**Supported view after the slice (from the tool, never derived):**
`supported T0 = 78.6485% (16318/20748), supported FN = 4,430`;
all-corpus view byte-identical (T0 77.5165%, FP=0, FN=4,733);
`excluded=303 unresolved=303 resolved-t0=0` — no excluded record is
currently matched, i.e. nothing implementable was excluded.

**F7 re-bin**: of the 3-code floor (2307×289 / 2834×120 / 2835×80),
214 of the 2307s are excluded; the remaining 75 2307s (nonjsExtensions
33, relative/failing-bare/format rows) re-bin to F6, and ALL 2834/2835
(the `nodeModulesAllowJs1.ts` relative-import mass, 200 rows + fixture
siblings 20) re-bin to F5/F6 as in-scope implementation work. The 89
non-2307 exclusions came OUT of F1/F6-attributed rows — the code-blind
harvest the scan existed for. jsdoc-band rows kept IN here (react
2307s, salsa JS rows) are 9.1b's question only where JSDoc-DRIVEN.

## 9.1b results (2026-07-21, jsdoc adjudication — DONE)

Full code-blind scan of all 4,733 band FNs (1,027 cases), classified by
the ORACLE record's nature (never by our curtain reason):

1. **Fixture feature scan** (sub-files split at `@filename`; JSDoc-block
   tags per JS-like vs TS-like sub-file; `@ts-check`; checkJs/allowJs):
   902 cases / 4,241 FN carry zero JSDoc tags (green); strong queue =
   tags in JS sub-files or `@ts-check` (122 cases / 323 FN); weak = tags
   only in TS-like sub-files (3 cases / 169 FN).
2. **Green safety net**: full-chain-text vocabulary screen (all nested
   chain levels) over every green FN record — 0 hits.
3. **Weak queue all-IN**: parser 112 (F2 recovery band) +
   `fixSignatureCaching.ts` 52 (pure TS fixture) + jsx 5 (.tsx) — JSDoc
   cannot drive semantics in TS files; text screen 0 hits.
4. **Strong queue per-record** against fixture + golden chain text, with
   12 neutralize-probes for every boundary call.

Probe recipe (fully reproducible, no synthetic fixtures): copy the named
corpus fixture, replace `@` with `%` INSIDE `/** … */` blocks only
(byte-length preserving — every span stays comparable; `// @filename` /
`// @ts-check` line pragmas untouched), then
`cargo xtask expand <variant> --out-dir <dir>` and pipe
`{"id":1,"programJsonPath":"<dir>/program.json"}` into
`node crates/oracle/driver.mjs`; convert `start` offsets to line/col via
the program.json `textB64`. A target FN key that SURVIVES tag
neutralization is not JSDoc-driven (IN); one that VANISHES is EXCLUDE.

Probe verdicts of record (vendored 6.0.3, 2026-07-21):

- `classCanExtendConstructorFunction`: the two 2507 rows survive
  (extends-a-TS-FILE-function — `@constructor` is inert in TS files) =
  IN; 2416/2554×2/2345×2/2417 vanish = EXCLUDE.
- `constructorFunctions` 2348×2 + C7 2554, `overloadTag2` 2394+2554,
  `jsdocTypeTagRequiredParameters` 2554×3,
  `moduleExportWithExportPropertyAssignment` 2554: ALL vanish.
- `enumTag`: the member-initializer 2322s AND the `Target.UNKNOWN` 2339
  vanish (without `@enum` the object read is JS-lenient) — all EXCLUDE.
- SURVIVORS (IN): `privateNameImplicitDeclaration` 2339 (declaration
  absence), `checkJsdocReturnTag1/2` 2872 (truthiness),
  `jsdocAugments_noExtends` 2339 (no heritage either way; contrast:
  `jsdocAugmentsMissingType` 2339 vanishes — the empty tag BREAKS a real
  base), `typeFromPrototypeAssignment/2` 2339, `thisPropertyAssignment`
  2339×2, `jsdocOuterTypeParameters1` foo-2339 (tag-position 2304s
  vanish), `thisTag3` 2339 (its 2730 vanishes),
  `jsdocTypeFromChainedAssignment` typeof-A 2339,
  `typeFromJSInitializer` null-rows ×5 (strict initializer typing; its
  `b = n` row vanishes).

Review round (PR #52, user review, all re-probed): 7 records first
authored as exclusions SURVIVE neutralization at the same T0 key and
were returned to the supported set — `checkJsdocSatisfiesTag10/6/7`
2339 `/a.js:13:10` (the object's INFERRED literal type persists; only
the `@satisfies` 2353s are tag-driven — the JS-lenient-read rule does
NOT generalize from `enumTag`, probe each), `jsdocTemplateTag` 2322
`forgot.js:22:0` (the dom-target assignment still fails on the return
type), `jsdocTemplateTag8` 2322 `a.js:18:0`/`56:0` (structural failure
persists; message drift = T1+), `jsdocTypeTagCast` 2322 `b.js:57:0`.
And one missed EXCLUDE pair was added: `jsDeclarationsReactComponents3
.jsx:2:72` 2503 — `JSX.Element` sits INSIDE the `@type` tag; vanishes
in BOTH matrix cells while the real-import 2307 rows survive.

**Exclusions landed: 240 records / 102 fixtures / 36 codes** (snapshot;
the manifest is the identity authority; total draft entries now 543):
2322×63, 2345×19, 2300×18, 2304×15, 2339×15, 2352×9, 2353×8, 2355×8,
2554×8, 2344/2564/2534/2341/2454×6, 2694/2420/2445×4, + tail. Driver
families: tag-supplied types on relation/override verdicts, tag-position
lookups (2304/2503/2694 inside `@type`/`@param`/`@template`/`@extends`),
tag-created symbols (`@typedef`/`@import` duplicate pairs), tag-created
clauses (`@implements`/`@satisfies`/`@enum`/`@this`/`@overload`,
accessibility tags), JSDoc casts, and JS ARITY (see rule below). All
entries `reason: jsdoc-semantics`. `jsdoc/importTag17.ts` ×2 stay under
their 9.1a host-primary entries — not re-added.

Boundary refinements of record (bind later slices):

- **JS parameter requiredness is JSDoc-driven** (probe-proven three
  ways): untyped JS parameters are OPTIONAL to tsc — arity verdicts
  (2554, overload-range 2554, 2769) and param-type verdicts (2345) that
  exist only via `@param`/`@type`/`@overload` signatures are
  jsdoc-semantics; the constructor-ness of a propertyless function via
  `@class`/`@constructor` (2348) rides the same rule.
- **JS-lenient object reads**: a missing-member read on a JS object
  literal errors only when a tag CLOSES the type (`@enum` exemplar) —
  probe before assuming a 2339 is tag-independent.
- Non-JSDoc assignment-declaration semantics stay IN (contract line,
  verbatim): expando/prototype/this-assignment rows,
  `Object.defineProperty`/`Object.assign` descriptor+value semantics
  (only setter-`@param`-typed property rows are excluded), CJS
  `module.exports` ordering (2565) and export= type-meaning rules
  (`moduleExportAssignment7` index.ts 2694×7).
- Real-code rows in jsdoc fixtures stay IN: real imports (react/
  prop-types 2307s, 2306/2882), heritage-expression name lookups,
  d.ts-side rows (`lovefield-ts.d.ts`), duplicate pairs where BOTH
  declarations are real (`typedefCrossModule5` 2451 Bar).
- A row whose SPAN sits inside a JSDoc comment is EXCLUDE; the same
  code at a real-code span is judged by type provenance, per record.

**Supported view after the slice (from the tool, never derived):**
`supported T0 = 79.5689% (16318/20508), supported FN = 4,190`;
all-corpus view byte-identical (T0 77.5165%, FP=0, FN=4,733);
`excluded=543 unresolved=543 resolved-t0=0` — no excluded record is
currently matched, i.e. nothing implementable was excluded.

**Family re-bins**: the 240 came out of F5's JSDoc-driven share plus
F1/F6-attributed jsdoc-band rows (the curtain-blind harvest — e.g. the
satisfies/getContextualType rows were F5-attributed, the 2564/2352
jsDeclarations rows F6). What remains of F5 is exactly the contract's
non-JSDoc assignment-declaration work (expando/defineProperty/CJS rows
kept IN above) for 9.8; the strong-queue keeps re-bin to F5/F6, the
weak queue stays F1/F2. Both adjudication passes are now complete:
9.2 pins the 543-identity set as the 2xxx band-freeze ceiling.

## Working rules

- Curtain retirement = FP-shield removal. Every widened display arm
  or narrowed bail runs the 7.5 fabrication audit: corpus diff at
  the arm, any NEW port-only row probed against the oracle before
  landing (verdict-pin technique where display still curtains the
  relation outcome). FP=0 is the gate, not a hope.
- Re-measure per slice (`conformance --band 2xxx` to a file, exit
  code checked); from 9.2 on, what is FIXED is the pinned identity
  SET — the supported-FN integer is read from the tool's
  `supported_false_negative_diagnostics` on every run and must be
  monotone non-increasing across slices. Exclusion-record counts
  and FN integers are never 1:1 (T0 is bucket-granular; 2XXX holds
  65 duplicate buckets — excluding one occurrence leaves the bucket
  in the supported denominator, and excluding a whole bucket does
  not subtract its record count from FN), so no supported-FN value
  is ever DERIVED arithmetically. Re-attribute the partition in
  this doc's slice PR when counts move materially. mismatches.json
  is regenerable — numbers in this doc are snapshots, the tool is
  the truth.
- One slice = one branch = one PR ([CLAUDE.md](../../../CLAUDE.md)
  workflow); ratchet.toml `[t0-2xxx]` + set-ratchet bumps ride the
  slice that GREW accepted identities, never the merge. Scope-only
  slices (9.1a/9.1b/9.2) land with `ratchet check` green and
  ratchet artifacts byte-unchanged — the A2 supported view is not a
  ratchet view, so an all-corpus accepted match neither appears nor
  disappears there and a `ratchet update` would be a no-op by
  construction.
- Escape hygiene: F1/F2 narrowings shrink existing curtain sites —
  after each, `escapes --write-manifest` and review the manifest
  diff; retired M7-re-owned rows are the visible progress ledger.
- Stop conditions unchanged (convergence plan §6): an exclusion that
  cannot select exactly one record, a missed-exclusion discovery
  after the pin, or three fixes hitting one model ceiling, stops
  the slice for design review.
