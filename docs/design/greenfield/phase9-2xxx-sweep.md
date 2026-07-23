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
| 9.1c | **Host chain-grade re-audit** (added at the 9.1b review — the chain-grade criterion is a change to 9.1a's judging basis, not a spot fix): enumerate ALL 9.1a same-T0-key keeps (every record kept via an unresolvable-variant probe) and re-verify each at T3-equivalent identity — **category + start + length + full chain + related** — against the variant; DRIFT rows (resolved-content embeds) become `host-resolution` exclusions. The DeclarationEmitErrors 2339s (`Promise<resolved-import>` vs `Promise<any>`, scouted at the 9.1b review) are MANDATORY targets. This is the FINAL adjudication commit — 9.2 pins its full SHA and settled set | Same per-record discipline; counts + supported metrics re-measured from the tool; `scope audit` green |
| 9.2 | **Band pin**, two changes per [§1.2](measurement-integrity.md#12-reviewed-snapshot-anchor): (1) the final adjudicated content (= 9.1c) lands while the manifest is `draft`; (2) a follow-up change records that adjudication commit (full 40-hex SHA of change 1) + the complete enumerated identity set as the `2xxx` band-freeze record. **BLOCKED until 9.1c lands** | `scope audit` green incl. pin verification vs trusted base; from here the pinned exclusion set is a CEILING — no in-band exclusion can be added, while the supported denominator may still GROW back toward the full band as resolved exclusions return via §3.2 tombstones |
| 9.3a | Tuple renderer (`symbol-less reference display` curtain, 190 rows; M7-re-owned escape row retires) + intersection display + contextual tuple arity + computed-key destructuring rows from the same M6-close re-owned set | Curtain rows flip matched; fabrication audit on every widened arm |
| 9.3b-x | typeToString shape ladder: mine the 1,543-row curtain by blocking type shape (debug census), then widen shape by shape (references w/ symbols, unions/intersections, anonymous object literals, signatures, indexed access, …) | Per-shape: curtained rows flip, `2xxx` band T0 monotone, fabrication audit each arm |
| 9.4 | Elaboration engine (F4): elaborateError → elaborateObjectLiteral/ArrayLiteral/ArrowFunction/JsxComponents + getBestMatchingType + reportRelationError head selection | The ~200 elaboration rows; forceTuple escape row retires |
| 9.5 | Mapped types (F3a, 185 rows): getTypeFromMappedTypeNode, instantiation, apparent members, relations | M8-stub escape narrows/retires; no new unrenderable shapes (F1 done) |
| 9.6 | Conditional types (F3b, 91 rows): resolution, distribution, infer positions (M6 infer machinery is live) + **Substitution/NoInfer (mandatory, 9 rows)** — NoInfer lands with whichever of 9.5/9.6 carries the Substitution machinery; it may not slip to M8 | M8-stub escapes narrow/retire; NoInfer rows flip matched |
| 9.7 | Parse-recovery overload band (F2): recovery-boundary parity work in the parser + narrow the functions.rs bail; fixture-driven (conformance/parser 1,181 dir rows) | 887-row 2304 mass + overload-band rows flip; syntactic T0 ≥ 99.8219% held |
| 9.8 | JS band (F5): non-JSDoc expando/assignment-declaration semantics implemented (JSDoc-driven rows were excluded at 9.1b) | F5 in-scope rows flip |
| 9.9 | Residue mining (F6 + everything the re-measures re-attributed) rule-by-rule to supported FN=0; close re-measure | Row-9 gate green: all-corpus 2XXX FP=0, supported T0-2xxx=100%, `scope audit` green |

9.1a/9.1b may interleave; 9.1c follows both and strictly precedes
9.2 (the pin is blocked until the re-audit lands); 9.2 strictly
precedes every implementation slice. Among implementations: 9.3
before 9.4/9.8/9.9 is a real dependency (rendering); 9.5/9.6/9.7
are order-independent among themselves and against 9.4.

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
  IN even when their oracle message embeds a resolved type — **rule
  SUPERSEDED at the 9.1b review round** (message drift is a T2+
  concern under the current tier definitions, and the supported scope
  is the M8 T1-T4 basis): key-only survival no longer un-excludes.
  Every keep made under this rule is re-audited in slice 9.1c at
  T3-equivalent identity (category + start + length + full chain +
  related); known mandatory targets: the DeclarationEmitErrors 2339s
  (golden embeds `Promise<{ default: typeof
  import("/node_modules/pkg/import"); }>`, variant shows
  `Promise<any>` — scouted DRIFT), while their 2353/2559/2538
  siblings scouted chain-identical.
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

Review rounds (PR #52, user review ×2, all re-probed):

- Round 1 (T0 keys): 7 records first authored as exclusions survive
  neutralization at the same T0 key; one missed EXCLUDE pair was
  found — `jsDeclarationsReactComponents3.jsx:2:72` 2503,
  `JSX.Element` INSIDE the `@type` tag; vanishes in BOTH matrix cells
  while the real-import 2307 rows survive (added ×2).
- Round 2 (tier correction): un-exclusion by T0-key survival alone is
  WRONG — the supported scope is the M8 T1-T4 measurement basis and
  the 9.2 pin is a ceiling (no in-band additions ever after), so a
  record stays IN only if its FULL chain (and related) is also
  identical under neutralization; "message drift" is a **T2+**
  concern under the current tier definitions (T2 = message, T3 =
  full chain), and drift whose content is tag-derived makes the
  record UNIMPLEMENTABLE at T2/T3 without out-of-scope JSDoc
  machinery ⇒ EXCLUDE. Chain-grade re-verification of ALL 25
  same-key survivors: `checkJsdocSatisfiesTag10/6/7` 2339
  `/a.js:13:10` chain+related IDENTICAL ⇒ stay IN (the object's
  inferred literal type persists; only the `@satisfies` 2353s are
  tag-driven), and the other 18 probe survivors (PD/PG/PH/PI/PJ2/
  PK/PL/PM/PN/PA-2507s) all chain+related IDENTICAL ⇒ stay IN;
  but `jsdocTemplateTag` 2322 (`(keyframes: Array<any>) => void`
  source display is @param-derived), `jsdocTemplateTag8` 2322
  `18:0`/`56:0` (`Covariant<unknown>`/`Invariant<unknown>` typedef
  displays + altered nested arms), `jsdocTypeTagCast` 2322 `57:0`
  (identical head, nested leaf `string | number` is the cast-var
  type) DRIFT ⇒ returned to EXCLUDE. The criterion change is a change
  to 9.1a's judging basis too, so its same-T0-key keeps are re-audited
  as their own slice **9.1c** (see the slice table) at T3-equivalent
  identity — category + start + length + full chain + related (the
  9.1b-review scout compared chain+related only) — before 9.2 pins
  anything.

**Exclusions landed: 244 records / 103 fixtures / 36 codes** (snapshot;
the manifest is the identity authority; total draft entries now 547):
2322×67, 2345×19, 2300×18, 2304×15, 2339×15, 2352×9, 2353×8, 2355×8,
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
`supported T0 = 79.5845% (16318/20504), supported FN = 4,186`;
all-corpus view byte-identical (T0 77.5165%, FP=0, FN=4,733);
`excluded=547 unresolved=547 resolved-t0=0` — no excluded record is
currently matched, i.e. nothing implementable was excluded.

**Family re-bins**: the 244 came out of F5's JSDoc-driven share plus
F1/F6-attributed jsdoc-band rows (the curtain-blind harvest — e.g. the
satisfies/getContextualType rows were F5-attributed, the 2564/2352
jsDeclarations rows F6). What remains of F5 is exactly the contract's
non-JSDoc assignment-declaration work (expando/defineProperty/CJS rows
kept IN above) for 9.8; the strong-queue keeps re-bin to F5/F6, the
weak queue stays F1/F2.

**Status after 9.1b: the JSDoc adjudication is COMPLETE; the 9.1a
chain-grade re-audit is NOT — 9.2 is BLOCKED on slice 9.1c.** The
547-entry draft manifest is PROVISIONAL, not the pin set: 9.1c
re-audits every 9.1a same-T0-key keep at T3-equivalent identity
(category + start + length + full chain + related), adds DRIFT rows
as `host-resolution`, re-measures counts and supported metrics from
the tool, and becomes the final adjudication commit whose full SHA
and settled identity set 9.2 then pins. *(Landed — see 9.1c results
below.)*

## 9.1c results (2026-07-21, host chain-grade re-audit — DONE)

Universe. The 9.1a keeps decided by unresolvable-variant probes are
exactly the non-2694 band FNs on the two P1/P2 fixtures — **56
records** = `nodeModulesImportTypeModeDeclarationEmitErrors1.ts` +
`nodeModulesImportAttributesTypeModeDeclarationEmitErrors.ts` × 4
module matrix cases (node16/node18/node20/nodenext) × 7 keys per case
(2339 `/other.ts 3:51` + `/other3.ts 3:55`; 2353 `/other.ts 3:21`;
2538 `/other3.ts 2:22, 5:49, 6:49`; 2559 `/other3.ts 3:20`). No other
9.1a-excluded fixture retains kept band FNs except
`verbatimModuleSyntaxAmbientConstEnum.ts` (its F.A row, kept on
in-program grounds, no probe involved).

Probe recipe (fully reproducible): variant = the corpus fixture
VERBATIM minus its `/node_modules/*` `@filename` sections,
**newline-preserving** (the fixtures are CRLF; a text-mode read
LF-converts silently and shifts every byte `start` by exactly the
line number while line/col survive — the 9.1a embedded shapes were LF
re-typings comparing at T0 line/col grade, the 9.1c variant is
byte-faithful so `start`/`length` compare exactly). Then
`cargo xtask expand <variant> --out-dir <dir>` (matrix fixtures emit
`program-<matrixKey>.json` per case, NOT `program.json`), pipe each
into `node crates/oracle/driver.mjs`, and compare each kept record
against the variant record at its T0 key: category + start + length +
full chain (deep, all nested levels) + related (deep).

Verdicts of record (vendored 6.0.3, 2026-07-21):

- **40 records IDENTICAL** at T3-equivalent identity (2353×8,
  2538×24, 2559×8): byte-exact category/start/length, deep-equal
  chain+related. The 9.1b-review scout's chain+related-only verdict
  upgrades to full identity — these keeps are FINAL.
- **16 records DRIFT (2339×8 per fixture)** — chain-ONLY drift:
  golden embeds the exports-RESOLVED module shape
  `Promise<{ default: typeof import("/node_modules/pkg/import"); }>`,
  variant shows `Promise<any>`; category/start/length/related all
  identical. The drifted content exists only via node_modules exports
  resolution ⇒ unimplementable at T2/T3 without host machinery ⇒
  EXCLUDE `host-resolution` (the scouted mandatory targets, all
  landed).
- **2694 regression check**: all 32 excluded 2694 keys still VANISH
  in the variant (9.1a verdict re-confirmed).
- **Safety net**: full-chain screen (`node_modules` in chain-tree
  text, related chain text, and record/related FILE NAMES) over all
  4,186 kept band FN records → 17 hits = the 16 targets + 1:
  `references/library-reference-5.ts` 2403. Adjudicated KEEP: the
  record file and related file are node_modules PATH SPELLINGS of
  `@filename`'d in-program files; the expander roots ALL five
  sub-files (probe: expansion `program.json` files list), so the
  duplicate-`alpha` collision (`any` vs `{}`, both in-program
  declarations) is in-program — nothing in the chain-grade identity
  is resolution-derived.
- **P3 keeps hold by construction**: jsx expansion `program.json` =
  `['file.tsx']` (re-verified) — `/.lib/react.d.ts` never
  materializes, the golden IS the unresolved outcome, so no resolved
  variant exists to drift against.

**Exclusions landed: 16 records / 2 fixtures / 1 code** (2339×16),
authored via `identity.mjs`; manifest now **563 entries =
host-resolution 319 + jsdoc-semantics 244**, still schema 2 /
status draft (the pin itself is 9.2's two-step protocol).

**Supported view after the slice (from the tool, never derived):**
`supported T0 = 79.6466% (16318/20488), supported FN = 4,170`
(monotone: 4,186 → 4,170); all-corpus view byte-identical
(T0 77.5165%, 16318/21051, FP=0, FN=4,733);
`excluded=563 unresolved=563 resolved-t0=0` — nothing implementable
excluded. Ratchet artifacts byte-unchanged (`ratchet check` green vs
`origin/main`); `scope audit` ok (dup-canaries 68/65,
cross-checked=68); families check frozen/433 rows baseline ok;
escapes 223/0/0/116.

**Status after 9.1c: the phase-9 scope adjudication is COMPLETE and
FINAL. This slice's merge is the adjudication commit of record — 9.2
is UNBLOCKED and pins this content SHA plus the settled 563-identity
set via the §1.2 two-step protocol.**

## 9.2 results (2026-07-21, 2xxx band pin — DONE)

The [§1.2](measurement-integrity.md#12-reviewed-snapshot-anchor)
two-step freeze is complete:

- **change 1** = the 9.1c slice commit
  `3ed4e2fb0ca911c863399d880c8be497d250d620` — the final adjudicated
  content (draft manifest, 563 exclusions), user-reviewed on PR #53
  with 0 findings and merged @a529ee7f (tree unchanged);
- **change 2** = this slice: `band_pins[0] = { band: "2xxx",
  adjudication_commit: <that full SHA>, identities: <all 563> }` —
  the complete enumerated identity set (count/hash are derived, never
  stored). Anchor-side precedent followed: the A5 freeze record cites
  the content-side slice commit (`ba1c7ef3`, "map lands DRAFT"), not
  its merge — the band pin does the same.

Verification (all re-derived by `scope audit`, identity compare, not
self-hash): the anchor must name a full commit SHA, be an ancestor of
HEAD, hold a DRAFT manifest, and the pinned set must equal the
2xxx-band subset of the exclusions at that commit. Trusted-base rule:
new reviewed pins may land; an existing pin is byte-stable forever —
changing one is an explicit reviewed re-baseline event that never
rides a slice.

Ceiling semantics now live (§3.1/§3.2, plan of record):

- No in-band exclusion can be ADDED — `scope audit` fails on any 2xxx
  exclusion outside the pinned set; a missed-exclusion discovery
  post-pin is a STOP CONDITION (convergence plan §6), not a manifest
  edit.
- Resolved exclusions leave via §3.2 tombstones (standing proof = A1
  membership under the 2xxx fixed view) and RETURN to the supported
  denominator — the denominator moves UP only, toward the full band.
- Supported-FN integers keep coming from the tool on every run,
  monotone non-increasing across implementation slices.

Gates at the pin: all-corpus 2xxx byte-identical (T0=77.5165%,
16318/21051, FP=0, FN=4,733); supported T0=79.6466% (16318/20488)
FN=4,170 unchanged from 9.1c (the pin adds no exclusions);
`excluded=563 unresolved=563 resolved-t0=0`; scope audit ok
(band-pins=1, dup-canaries 68/65, cross-checked=68, baseline
origin/main); ratchet artifacts byte-unchanged; families check
frozen/433 baseline ok; escapes 223/0/0/116.

**Status after 9.2: the 2xxx band is PINNED. Scope work is done —
implementation begins at 9.3 (display ladder, first slice 9.3a tuple
renderer).**

## 9.3a results (2026-07-21, tuple renderer — DONE)

All four M6-close re-owned M7 escape rows retired (manifest 225→221
sites, stale=0, untagged=0). Band movement: all-corpus T0 54.8813% →
**55.2831%** (26905 → 27102, +197, FP=0); 2xxx T0 77.5165% →
**78.4143%** (16318 → 16507, +189); supported-2xxx T0 79.6466% →
**80.5691%** (16507/20488), supported FN 4,170 → **3,981** (tool
integers, monotone ✓). Shadow tiers: T2 +304 / T3 +210 on
already-matched rows (formatUnionTypes text fidelity). Baseline
curtain attribution re-measured live: `symbol-less reference display`
212 → 0; computed-key 12 → 0 (rows now plain M7 suggestion-family
FNs, no boundary evidence).

Decisions of record:

1. **Tuple arm placement**: typeReferenceToTypeNode's tuple arm
   (51948-51978) transcribed into `type_to_string_slice_structured`
   BEFORE the symbol head — tuple targets are the symbol-less
   references; the swap against tsc's dispatch order (global-Array
   identity first) is unobservable because the two tests are
   disjoint. Empty/arity-0 tuples print `[]` unconditionally:
   typeToString always runs under IgnoreErrors ⊇ AllowEmptyTuple
   (50722), so the encounteredError leg is dead in the error-display
   slice. The residual non-tuple symbol-less reference re-curtains
   under the M8 nodeBuilder-tail reason (no fresh panic claim).
2. **Parenthesizer as kind-tags**: the string slice now returns
   `(text, SliceTypeNodeKind)` and transcribes the factory
   parenthesizer rules (20540-20606) at every join — union/
   intersection constituents, keyof/readonly operands, array-element
   and optional-element postfix wraps. Oracle-pinned faces:
   `[(string | undefined)?]` (optional union parenthesizes),
   `[...(string | boolean)[]]` (rest union through the ArrayTypeNode
   wrap), `a?: number | undefined` (NamedTupleMember types NEVER
   parenthesize — factory 22247 applies no rule). The pre-existing
   array sugar gained the same element wrap (was a latent
   `string | number[]`-shape T2 infidelity, T0-invisible).
3. **formatUnionTypes (55474-55498) ported** — required, not
   optional: the port's interned union order shows `undefined`
   FIRST, tsc re-appends nullables at the tail (null before
   undefined; the eOPT missing marker re-appends as plain
   undefined) and collapses enum-like runs. The interned
   `true | false` pair carries TypeFlags::BOOLEAN (tables stamp it
   like getUnionType) and must print as the KEYWORD before any union
   walk — transcribed as a pre-union arm; without it the collapse
   would re-enter the union walk unboundedly. getBaseTypeOfEnumLike-
   Type was ALREADY ported (engine.rs) — reused, not duplicated.
4. **Fabrication audit hit both barrels** (7.5 precedent confirmed
   again): first full re-measure tripped NEW_FP=20. Family A (12
   rows, 2739/2741): the missing-property pre-head override fired
   where tsc's propertiesRelatedTo tuple arm (66771-66774) takes the
   ARITY walk — a tuple TARGET with an array-or-tuple SOURCE never
   reaches reportUnmatchedProperty; guard transcribed into
   `report_unmatched_property_head` (the non-array source half keeps
   its 2741 face — arityAndOrderCompatibility01's 'StrNum' rows pin
   it). Family B (8 rows, 2322): three head-only
   checkTypeAssignableToAndOptionallyElaborate sites emitted outer
   heads where tsc's elaborateError reports inner rows and
   suppresses the head — binding-pattern initializer
   (statements.rs), return position (functions.rs), assignment
   (operators.rs) now run the Step-12 elaborate-first idiom over
   `elaborate_literal_assignment`. Both fixes are SOURCE-level (tsc
   shape), not display shields; the yield-position site stays
   head-only (no failing evidence — wire it when a row appears).
5. **Tuple-intersection special block DELETED whole** (the 9.1-era
   syntax bridge + `is_tuple_arity_only_constraint`): tsc has no
   counterpart — with tuples renderable the standard head path
   covers the pair. Proof it was corpus-dead: band=all matched
   integers byte-identical before/after the deletion; zero corpus
   rows rode the bridge (its only consumer was a unit pin, rewritten
   to containment-until-9.3b — the intersection's `{ p: string }`
   member is an anonymous object WITH members, a 9.3b shape).
6. **Computed-key destructuring containment retired with a clean
   probe**: the port's `get_type_of_destructured_property` was
   already the full tsc shape; the 6.6f-era fear was evaluation-order
   narrowing divergence. Live re-measure of
   controlFlowAssignmentPatternOrder (the PR-#41094 fixture, 12
   computed-key faces designed to fabricate 2322s on any order bug):
   ZERO false positives — the M5/M6 flow machinery already orders
   key/default evaluation correctly. The 12×6133 rows are plain
   unused-suggestion-family FNs (M7), no longer escape-blocked.
7. **Label resolution**: getTupleElementLabel's declaration arm only
   (51958 gates on a present label); `Debug.assert(isIdentifier)`
   transcribed as containment (M8 tail reason) — a pattern-named
   label would throw in shipped tsc, so no fixture can pin it.
   unescapeLeadingUnderscores (51961) folded into the helper.

Remaining F1 curtain after 9.3a: `typeToString beyond the 5.4 display
slice` attributes 1,891 FNs (grew from 1,543 — rows formerly
attributed to the tuple reason now unwind at their non-tuple CHILDREN:
anonymous objects with members, signatures, enum members …) — that is
the 9.3b-x shape ladder's worklist, mined by blocking type shape.
`elaborateArrayLiteral` spread-tupleization (forceTuple) now shows 6
attributed FNs (elaborate runs at three more sites) — its escape row
stays M7-owned and retires at 9.4 per plan.

Verification pins: 5 new oracle-probed display tests in check.rs
(labeled members, optional-union parens, labeled-optional no-parens,
rest/variadic incl. rest-union parens, empty + readonly-4104) +
3 frontier pins FLIPPED live (access 2493 tuple-index, calls 2345
tuple-arity, operators destructuring 2493+2322 pair) + 1 rewritten to
containment-until-9.3b. All heads byte-match the vendored oracle
(scratchpad probe-93a, noLib strict).

**Review round (PR #55, 2 P1 findings — both source-verified against
_tsc.js and fixed, +1 matched each band):**

8. **Return elaboration takes the EFFECTIVE check node** — tsc
   checkReturnStatement computes `getEffectiveCheckNode(expr)` and
   passes THAT into checkTypeAssignableToAndOptionallyElaborate
   (84585-84587): outer parens AND satisfies strip BEFORE
   elaborateError, so `return ([1] satisfies [number])` against
   `[string]` elaborates the array literal and the element row
   replaces the head. The 9.3a first cut passed the RAW expression
   (the elaborate entry arms strip parens/as-const but deliberately
   NOT satisfies — the member-initializer rule), which stopped
   elaboration and emitted the outer head where tsc reports the
   element. Oracle-pinned: (2322, 36, 1) 'number'→'string'.
9. **EnumLike display arm (51367-51399) ported** — enum-member
   literal types fell into the Literal arm and printed their BASE
   VALUES (`[0]` for `[E.A]`): the tuple renderer made the outer
   shape renderable, surfacing the pre-existing child infidelity as
   emitted text. The arm precedes the literal arms AND the union
   walk: member face `E.A` (parent name + identifier member),
   single-member collapse (51371: the member type IS the declared
   type → bare `E` — probes: `[S.Only]` prints `[S]`, string-valued
   single member prints `[E, E]`), the EnumLiteral-stamped declared
   union prints `E` (also the loop-breaker for formatUnionTypes'
   enum-run collapse, which hands that union back), const-enum same,
   and the bare-literal source composes with reportRelationError's
   literal-source generalization (`E.A` source head prints `E`).
   Non-identifier member names (tsc renders `typeof E["..."]`
   indexed access) + the unconstructible symbol-less/parentless
   faces stay behind the M8 tail (3 containment sites, count=3 in
   the manifest). `is_identifier_text` promoted pub from the syntax
   parser for the member-name gate. 6 oracle-probed pins
   (probe-93a-review). T2 +67 / T3 +58 on already-matched enum rows.

## 9.3b results (2026-07-22, anonymous-object display + the
## relation-reporting unlock — DONE)

Shape census first (the 9.3b-x mining step): a THROWAWAY patch tagged
every display-curtain escape reason with the blocking TypeData shape
and one `--band 2xxx` run aggregated the tags through the built-in
`fn_partial_boundary_audit` reasons census — no new tooling. Ranking
over the 1,891-row curtain: anonymous objects from TYPE LITERALS
665 (+35 instantiated), OBJECT LITERALS 304, module/namespace value
faces 198, function/method shapes 165 (+24), template-literal 80,
string-mapping 26, indexed-access 22, keyof 10, tail ~25. First rung
= the anonymous-object literal face; signatures are next.

Band movement: all-corpus T0 55.2831% → **56.1766%** (27102 → 27540,
+437 with the review commit's +1 absorbed, FP=0); 2xxx T0 78.4191% →
**80.3525%** (16508 → 16915, +407); supported-2xxx T0 80.5740% →
**82.4824%** (16899/20488), supported FN 3,980 → **3,589** (tool
integers, monotone ✓). Shadow tiers (2xxx): T2 76.9797% → 79.0271%,
T3 71.7211% → 73.0607%. Curtain attribution re-measured live:
`typeToString beyond the 5.4 display slice` 1,891 → **1,142** (−749).
Syntactic band unchanged (2242/2246).

Decisions of record:

1. **The arm is createAnonymousTypeNode's structural tail**
   (51750-51812 → createTypeNodeFromObjectType 51894-51938 →
   createTypeNodesFromResolvedType 52137-52240 →
   addPropertyToElementList 52241-52400): gated to
   TYPE_LITERAL/OBJECT_LITERAL symbols and symbol-less anonymous
   types. Every symbol special ahead of the tail (instantiation-
   expression TypeQuery reuse, JS constructors, typeof-function),
   the visitedTypes revisit faces (alias-for-literal / `...`
   elision), single call/construct shorthands, abstract-construct
   re-derivation, method/accessor member faces, reverse-mapped
   placeholders, private/unique-symbol names, and non-plain string
   names stay behind the same M8-tail curtain — the signature rung
   and later. A state-level `slice_visited_types` set guards
   recursion on BOTH the symbol-carrying and symbol-less paths (tsc
   guards only the former; divergence needs a symbol-less
   self-containing type, which cannot be constructed).
2. **The 7.5 empty-resolution FP shield carries over**: a
   symbol-CARRYING shape that resolves to zero members stays
   curtained (JSON imports M7, checkJs object literals M8 — their
   member machinery is unported, so an emitted `{}` would fabricate);
   the symbol-less empty face keeps printing `{}`.
3. **Member types render STRUCTURALLY** — probed, not assumed: the
   error-display path never takes the syntacticNodeBuilder
   annotation-reuse arms (`{ y?: number }` prints
   `y?: number | undefined`; parenthesized annotations drop their
   parens; `undefined | string` reorders to `string | undefined`;
   alias spellings resolve). approximateLength/checkTruncationLength
   is likewise unmodeled — over-long literals print whole where tsc
   elides `... N more ...`. Both are text-only tails (row keys are
   position+code): recorded T2 residue on the M8 nodeBuilder tail,
   not new escapes.
4. **Name machinery** (getPropertyNameNodeForSymbol 53411-53442 +
   createPropertyNameNodeForIdentifierOrLiteral 19208-19212):
   identifier-able names print bare, numeric canonical names print
   through js_number_to_string, declared quote styles survive
   (isSingleQuotedStringNamed reads the literal's closing quote —
   trivia-immune), computed/element-access names classify through
   the late-bound nameType's STRING_LIKE flags where tsc re-enters
   checkExpression (recorded deviation — identical for the
   literal-typed keys late binding produces), and the negative
   numeric face prints computed `[-N]`.
5. **The accessor fall-through is load-bearing**: same read/write
   type with a non-class parent prints the PLAIN property row
   (oracle-pinned `{ get p(): string; set p(v: string) }` →
   `{ p: string; }`); diverging write types and the class-parent
   arms stay curtained (signature rung).
6. **typeof faces landed with the named-object arm's
   instance-side split**: the 5.4-era arm printed class STATICS and
   enum objects as the bare symbol name; the did-you-mean row
   surfaced the infidelity as emitted text ('A' where tsc prints
   'typeof A' — the 9.3a-review lesson pattern again: widening a
   renderer surfaces child infidelities). isClassInstanceSide keys
   the split via declared-type comparison; TypeQuery joined the
   parenthesizer kinds (postfix positions wrap `(typeof C)[]`).
7. **Fabrication audit, seven families** (NEW_FP=24 first 2xxx
   re-measure, +18 band=all — every fix at the SOURCE, none display
   shields):
   - **hasExcessProperties reporting face** (65347-65410): the
     engine's verdict-only twin became a shared verdict/report
     worker (`excess_properties_worker`, tsc's reportErrors2 split —
     verdict and report CANNOT drift), and the head sites
     (check_type_assignable_to AND check_type_comparable_to — the
     comparable case-clause face) run it excess-FIRST: the
     parent-skipped 2353/2561 IS the top-level code, anchored at the
     excess property's name, with the spelling suggestion probing
     getSuggestionForNonexistentProperty. The discriminant-
     incompatibility half keeps the head (its
     Types_of_property_0_are_incompatible row is elided-chain
     content). JSX-attribute sources keep the JSX band's
     containment.
   - **elaborateError's entry did-you-mean probe** (63959-63966 +
     elaborateDidYouMeanToCallOrConstruct 64063-64091): every
     elaboration entry re-probes a failed callable/constructable
     source (construct signatures first, Any/Never return guard) and
     re-reports AT THE EXPRESSION with the 6212/6213 related row —
     the stringIndexer 2741s anchor at the VALUE, not the member
     name. `probe_head` threads elaborateError's headMessage;
     satisfies passes None and keeps its recorded containment.
   - **Shorthand members elaborate** (generateObjectLiteralElements
     64443: innerExpression UNDEFINED, errorNode = the name; the
     shorthand name IS the value reference, so its cached type is
     the member type). Method/accessor members still skip — the
     get/set double-yield needs a dedupe decision at the signature
     rung.
   - **elaborateElementwise's member lookup is an indexed access**
     (64131): a property miss falls through to the APPLICABLE INDEX
     SIGNATURE's value type in both the object walk and the array
     walk (tuple-like targets keep the limited-elements skip,
     64393-64401) — numeric-indexer fixtures report their member
     rows instead of fabricating heads.
   - **Two more head-only sites got the Step-12 elaborate-first
     idiom**: the Step-13 merged-declaration initializer row and the
     constructor-return arm (84560 — headless cTATAOE over the RAW
     expression; the 2409 lands on EVERY failed relation, elaborated
     or not, matching the oracle's two-row face).
   - **reportUnmatchedProperty pre-walk fidelity**: the walk
     apparent-izes nonPrimitive `object` IN PLACE (renders '{}',
     the oracle 2741 face), PRIMITIVE sources never take the
     missing-property faces (reportStructuralErrors = reportErrors
     && !sourceIsPrimitive), TYPE VARIABLES keep the generic head
     (their constraint re-enters through a NESTED isRelatedTo and
     the outer level re-heads with the type-parameter face), and
     the single-property 2741 renders through
     getTypeNamesForErrorDisplay's equality re-render.
     isKnownProperty's index probe switched from the M3-era
     STRING/NUMBER flag shortcut to the faithful
     isApplicableIndexType chain — template-literal keys
     (`[k: \`s${string}\`]`) and symbol keys admit their members
     (the shortcut FABRICATED excess verdicts there, previously
     masked by the curtained heads), with the late-bound-name
     esSymbolType probe and the string-index back-compat disjunct
     (74828).
   - **The scanner cooks numbers as ECMA Number#toString**: the
     local formatter used Rust's f64 Display (never switches to the
     >=1e21 exponent form) and non-bigint 0B/0O/0X literals kept
     their EXACT decimal expansion where tsc rounds ONCE through
     parseInt — member names like `9.671406556917009e+24:` now
     canonicalize identically, un-fabricating the 7053 element-
     access family (which the object display had been masking
     corpus-wide).
8. **resolved-t0 = 16 after the slice**: the 9.1c chain-drift 2339
   rows (nodeModulesImport*DeclarationEmitErrors ×8 fixture-cells ×2
   keys) now T0-match — their exclusions were CHAIN-drift verdicts,
   and T0 membership is the §3.2 singleton proof. Tombstoned in this
   slice's follow-up commit (resolving commit 41b1eabb3da8680d, the
   implementation commit): exclusions 563 → 547 + 16 standing
   tombstones, the pin's 563-identity record untouched. Post-
   tombstone re-measure: resolved-t0=0, supported denominator
   20488 → 20504, supported T0 = **82.4961%** (16915/20504, all 16
   returned occurrences matched), supported FN 3,589 unchanged.

Verification pins: 21 new in check.rs — 9 display faces (basic /
optional+readonly / name faces incl. quote styles / index-before-
properties / nested+union / accessor collapse / method containment /
class-static + enum typeof) and 12 machinery rows (2353, 2561,
did-you-mean 2741 at the value, shorthand missing-prop + member row,
index-fallback member row, constructor 2322+2409 pair, merged-decl
member row, nonPrimitive '{}' face, template-key clean verdict,
case-clause 2353, non-finite canonical-name resolution ×3 shapes) —
all byte-exact against the strict-mode oracle probes (scratchpad
probe-93b*, probe-93b-pins-final). 2 containment pins flipped live
(object-literal 2352 assertion faces, tuple-intersection head).
Escape rows: the new arm's curtain sites join the standing M8
nodeBuilder-tail reason (manifest diff is the review surface);
`escapes --stale M6` green, untagged 0, recovery 116 unchanged.

## 9.3b2 results (2026-07-22, signature rung + the annotation-reuse
## channel — DONE)

Band movement: all-corpus T0 56.1766% → **57.3923%** (27540 → 28136,
+596, FP=0 across every band); 2xxx T0 80.3525% → **83.1219%**
(16915 → 17498, +583); supported-2xxx T0 82.4961% → **85.3394%**
(17498/20504), supported FN 3,589 → **3,006** (tool integers,
monotone ✓). Shadow tiers (2xxx): T2 79.0271% → 81.7158%, T3
73.0607% → 74.3385%. Curtain attribution re-measured live:
`typeToString beyond the 5.4 display slice` 1,142 → **553** (−589).
Syntactic band unchanged.

Decisions of record:

1. **The arm is signatureToSignatureDeclarationHelper (52504-52631)
   as a string renderer** over the seven producible kinds: the
   single call/construct shorthands (51907-51916, `(...) => R` /
   `[abstract ]new (...) => R` — the abstract modifier reads the
   signature flag), call/construct signature members
   (createTypeNodesFromResolvedType order: calls, constructs, index
   signatures, properties), method members (one MethodSignature per
   call signature of the undefined-filtered type, the optional token
   on each), and the get/set faces for DIVERGING non-class accessor
   pairs (the same-type non-class collapse stays the 9.3b plain
   row). Class-parented accessor arms and the abstract-construct
   intersection re-derivation (51918-51928) stay curtained with
   reachability notes (spreads drop prototype accessors — probed;
   `abstract new` mixes need M8-band synthesis). FunctionType /
   ConstructorType joined SliceTypeNodeKind — the union-constituent
   parenthesizer wrap covers every producible position through the
   existing fall-through chain.
2. **Parameter faces**: getExpandedParameters' tuple-rest expansion
   runs as display-transient faces (no symbols minted — enterNewScope,
   the only other consumer, is dead without an enclosingDeclaration);
   labels ride the 4-arg getTupleElementLabel (labeled declarations,
   pattern-element recursion incl. nested rests — `rest_0`-style
   synthesis — and the `_N` duplicate counters keyed on the rewritten
   name, 57956); a mid-list REST face falls back to the declared
   parameter list (52519-52523). Binding-pattern names print with
   initializers elided (`{ a, b }` padded, `[a, b]` unpadded, omitted
   elements empty). isOptionalParameter's initializer arm reads
   getMinArgumentCount under (StrongArityForUntypedJS |
   VoidIsNonOptional), which reduces to the min-argument integer
   without the void-trimming loop (structural.rs variant);
   requiresAddingImplicitUndefined adds the strict `| undefined` to
   required-initialized parameters.
3. **The annotation-reuse channel is LIVE and enclosing-gated**
   (oracle-probed 5 rounds, ~90 cases): canReuseTypeNodeAnnotation
   (50932-50955) returns false without a context
   enclosingDeclaration, and error-path typeToString carries one
   ONLY through getTypeNamesForErrorDisplay's context-sensitive
   probe (50748: the symbol's value declaration, when it is an
   expression and NOT context-sensitive). So declare-let sources
   render structurally (parens drop, aliases resolve, `x?: number`
   prints `x?: number | undefined`) while fn-expression and
   object-literal sources REUSE annotation spellings (`(x?: number)`,
   `(x: (string))`, alias names — through member faces too). The
   port parks the enclosing on CheckerState
   (slice_display_enclosing), sets it at the three
   getTypeNamesForErrorDisplay sites (reportRelationError pass-1,
   the single-missing-property 2741 face, operator errors) and
   restores across the Err unwind. Type-parameter CONSTRAINTS reuse
   through typeToTypeNodeHelperWithPossibleReusableTypeNode
   (52832-52834) WITHOUT the enclosing gate — the declared
   constraint annotation prints whenever its unmapped type equals
   the current constraint (`<T extends AB>` keeps the alias);
   defaults never reuse (52829 — `= (A)` prints `= string`,
   probed). The reused-node printer emits CLONES: string literals
   re-quote double, numeric literals print their cooked text
   (`0x10` → `16`), type-literal members re-join `{ ...; }`;
   unsupported node kinds Err (the row stays curtained — never
   divergent text). The 9.3b "annotation reuse probed inert" note
   was declare-let-shaped — this slice's probes falsified it for
   expression-valued sources and the stale-justification comment on
   getTypeNamesForErrorDisplay came out.
4. **Return faces**: serializeReturnTypeForSignature's syntactic arm
   rides the same reuse gate; the inferred arm renders the type
   predicate first (`asserts x is T` / `this is T` faces via the
   7.5-B7 model — getTypePredicateOfSignature already resolves
   through signature.target/mapper, so the context.mapper
   re-instantiation is identity and elided with a comment).
   SetAccessor faces drop the return; GetAccessor/SetAccessor take
   no type parameters (factory shape).
5. **Member elaboration**: generateObjectLiteralElements' method /
   accessor / get+set yields landed in the object walk —
   innerExpression undefined, the row at the NAME node, the source
   member type read from the source type's own property (the indexed
   access), and a get/set pair yields TWICE (one row per accessor
   name — probed, no dedupe). Computed method names keep the plain
   2322 (the 2418 swap is PropertyAssignment-only, 64449).
6. **The anonymous-symbol gate widened to FUNCTION|METHOD symbols**:
   shouldWriteTypeOfFunctionSymbol (51789-51795) requires
   UseTypeOfFunction or a revisit, and error-path typeToString sets
   neither — so top-level, local, namespace-parented declarations
   and expressions ALL render structurally on first visit (probed);
   the revisit `typeof f` face stays behind the slice_visited_types
   curtain, and class/enum/value-module symbols keep their heads
   (value-module faces are the next rung). JS-file fn symbols stay
   behind the checkJs band.
7. **Fabrication audit, three families** (NEW_FP=160 first 2xxx
   re-measure + 2 band=all, every fix at the source):
   - **shouldReportUnmatchedPropertyError (67043-67054)**: a
     signature-shaped property-less source against a
     non-callable-shaped target keeps the HEADLESS relation head —
     no 2741/2739 face (`t = () => 1` vs `{ f(): void }` is a plain
     2322; the port's 9.3b pre-walk was missing the gate, masked by
     the fn-display curtain). Both-callable pairs keep reporting.
   - **elaborateArrowFunction (63997-64046)**: expression-body
     arrows with no annotated parameters elaborate the RETURN
     position — the row lands on the body expression (`var aLambda:
     (x: string) => number = (x) => 'a string'` rows at `'a string'`,
     golden-verified), recursing through paren/comma bodies; block
     bodies, annotated params, non-single-signature sources and
     related returns decline to the caller's head. Ported into
     elaborate_literal_assignment as the kind-220 arm — the missing
     arm had been anchoring heads at declaration/member names.
   - **TS expando functions (bindSpecialPropertyAssignment 44821)**:
     `function foo() {}; foo.x = 1` declares members tsc-side even
     in .ts files; the port's binder records the parent symbols
     (expando_assignment_targets — the stage-3.4c unreliability
     flag) and the member-miss reporters SUPPRESS (errorType
     continues) for flagged parents — property access, element
     access (7053 faces), and the failed-declaration relation
     (propertyAssignmentUseParentType). NOT an Err containment: the
     reporter sits inside symbol type resolution, and the first cut's
     Unsupported unwound through `var n` redeclarations into
     NEIGHBORING statements' real class-side 2339s — the set-ratchet
     caught the 8-identity regression live (the third live catch).
     Classes/class expressions are not expando parents and keep
     their rows (control-pinned).
8. **Flipped pins**: 4 containment pins whose documented oracle rows
   now render (method-member display, instantiation-expression 2635
   — the old comment's span guess was wrong, the live span is the
   type-argument list — construct-only 2348, decorator 1270) and 3
   7.5d-era relation-frame fail-faces (parked-frame, clamp
   re-enter, callback control) — all flipped to oracle-probed live
   rows.

Verification pins: 33 new — 19 display faces (structural vs reuse
optional-parameter TWIN pair, generic constraint+default, abstract
ctor, member order, diverging accessors, overloaded optional
methods, tuple-rest expansion ×3, binding pattern+reuse, asserts
predicate, union/tuple parens, this-param, constraint-alias reuse,
context-sensitive structural control, setter union, expansion-beats-
reuse, return-annotation parens), 5 member-elaboration rows (method
row, get/set double-yield, computed-name 2322, accessor-vs-index,
method-vs-index), 8 fabrication-audit pins (headless-head +
2741-control, arrow return-position ×2 + block-body/annotated-param
controls, expando-clean + class control), plus 1 renamed decorator
pin — all byte-exact against strict-mode oracle probes (scratchpad
probe-sig*, probe-93b2-pins). Escapes 251/0/0/116 (the new curtain
sites are the signature machinery's Err arms; manifest diff
reviewed); ledger 17 new tsc-port headers hashed; ratchet +596/+583
with comment lines.

**Review round (user review, 4 findings — all fixed at the source;
+1 matched both bands, FP=0 both, escapes 250/0/0/116, 894 tests):**

1. **Expando suppression made NAME-PRECISE** (high): the binder
   records (parent symbol → assigned member names) —
   getElementOrPropertyAccessName spellings, escaped — instead of a
   bare parent flag, and every member-miss consult
   (report_nonexistent_property, the element-access 7053 ladder)
   suppresses ONLY names an assignment would have bound: `foo.y`,
   `alias.q`, `foo["z"]` keep their real 2339/7053 rows. The
   failed-declaration-relation containment stays symbol-level
   (relation verdicts cannot be name-precise). The expando'd
   DECLARATION symbol also gained its display face: tsc's binding
   namespaces the symbol, so the ValueModule disjunct prints
   `typeof foo` (oracle-probed) — the fn-EXPRESSION flavor flags
   the variable, not the type's symbol, and keeps the structural
   face minus the unbound members (recorded stage-3.4c T2 residue).
2. **Union-target member elaboration through getBestMatchingType**
   (high): getBestMatchingType (67256) landed as a CheckerState
   port — findMatchingDiscriminantType /
   findMatchingTypeReferenceOrTypeAliasReference /
   findBestTypeForObjectLiteral / findBestTypeForInvokable /
   findMostOverlappyType over the already-ported discriminant kit
   (the RelationChecker twin from 9.3b keeps the in-walk
   comparator; this one carries getBestMatchingType's default
   assignable probe, which is what the elementwise caller passes) —
   and the member walk's target lookup re-probes the best-matching
   constituent for union targets
   (getBestMatchIndexedAccessTypeOrUndefined 64103-64114, both the
   object and array walks; the 9.3a-era calls.rs union containment
   retired with it). `{ m: () => string } | { x: number }` sources
   now row at `m`, head suppressed.
3. **isOptionalParameter's IIFE arm counts EFFECTIVE arguments**
   (medium): getEffectiveCallArguments expands spread tuples, so
   `(function f(a, b) {...})(...[1, ""] as const)` displays
   `(a: 1, b: "") => void` — no phantom `?`.
4. **The optional-member face rides the 65185 nullable-candidate
   substitution, not removeMissingType** (medium): probing showed
   removeMissingType is exactOptionalPropertyTypes-gated identity
   under default options, while isRelatedTo's entry substitutes a
   [nullable, X] / [nullable, nullable, X] union target with X for
   a DefinitelyNonNullable source — THAT is why `{ m?: () => string }`
   members (and `let v: string | undefined = n` heads) report
   against `() => string` alone while two-real-member unions keep
   the union face (oracle-probed U1-U5). Ported as
   nullable_stripped_report_target at both report entries
   (assignable + comparable), where tsc's in-engine reporting sees
   the substituted pair; the elementwise report tail ALSO carries
   the faithful removeMissingType pair for the exactOptional
   corpus. Review overreach note: the fresh-literal discriminated-
   union control (`{kind:"a",v:1}` into a two-object union) exposed
   a PRE-EXISTING verdict FN (the port relates it) — outside this
   slice, pinned via the declared-source twin instead.

**Review round 2 (user review, 1 finding — fixed at the source;
corpus integers unchanged (`ratchet update`: no additions), FP=0
both bands, 895 checker tests):**

1. **No-substitution template keys record like string literals**
   (high): round 1's name recorder was a hand-rolled near-duplicate
   of getElementOrPropertyAccessName whose element-access arm
   matched only StringLiteral/NumericLiteral — and whose span
   citation (45190-45201) actually lands in getContainerFlags'
   tail; the real definition is _tsc.js 15134, whose literal arm is
   isStringLiteralLike || isNumericLiteral, so `` foo[`x`] = 1 ``
   recorded nothing: the assignment kept a 7053 and every read of
   the assigned name kept its 2339/7053 row (port-only rows). The
   duplicate is deleted; the recording site calls the pre-existing
   faithful port get_element_or_property_access_name
   (is_string_or_numeric_literal_like + literal_text_of — both
   already template-inclusive). Oracle-probed pin: `` foo[`x`] = 1
   `` then `` foo.x / foo[`x`] / foo["x"] `` all clean, `foo.y`
   keeps (2339, `typeof foo`) — the recorded name is the TEXT, so
   suppression is spelling-independent in both directions.

## 9.3b3 results (2026-07-23, symbol/value/module heads — DONE)

Numbers (tool-read, band 2xxx): T0 84.1385% (17712/21051, +213)
FP=0; supported T0 86.3831% (17712/20504) FN=2,792 (from 3,005);
T1 84.1338% / T2 82.7372% (+206) / T3 75.3076% (+204) — the new rows
carry chain fidelity through T3 nearly 1:1 with T0, so the module
faces land T2/T3-clean where the chain model is live. Band all:
57.8594% (28365/49024, +228) FP=0. Generic display curtain 553→340.
Syntactic unchanged. 904 checker tests (9 new oracle-probed pins).

1. **Throwaway re-census** (evidence in the PR; method = the 9.3b
   pattern: 30 curtain sites tagged with unique `[cen ...]` reasons,
   4 of them dynamic — structured-tail TypeData discriminant,
   anon-symbol flag split, anon-other flags, empty-resolution symbol
   flags — one band-2xxx run, integers byte-identical, then the
   instrumentation reverted). All 553 generic-curtain FN rows carried
   exactly one tag: module-ns 200 / empty-sym ObjectLiteral 107 /
   template-literal 83 / empty-sym Transient|TypeLiteral 61 /
   string-mapping 26 / indexed-access 23 / tail-object 18 /
   module-ext 17 / keyof-index 10 / JSON-import transient 4 /
   unique-symbol 3 / instexpr 1. Slice target = module-ns +
   module-ext = 217; the 9.3b4 operator ranking (template-literal /
   string-mapping / indexed-access / keyof) and the 9.3b5 tail
   (tail-object incl. importAttributes6, unique/private names,
   instexpr revisit) are re-confirmed by the same census.

2. **The arm** — `symbolToTypeNode` error-path Value slice
   (53114-53198): lookupSymbolChainWorker (52943-52958) builds
   `[symbol]` without an enclosingDeclaration, so faces render
   UNQUALIFIED (probed: nested `namespace A { namespace B }` member
   misses print `typeof Inner`, never `typeof A.B`) and the
   accessibility/type-parameter chain machinery is out of scope by
   construction. Import face (hasNonGlobalAugmentationExternalModuleSymbol
   50541-50543 over the symbol's declarations): `typeof
   import("<specifier>")`, where the error-path specifier
   (getSpecifierForModuleSymbol 53060-53081) is the SECOND
   ambientModuleSymbolRegex unquote — it matches every admitted
   symbol because bindSourceFileAsExternalModule names source-file
   modules `"<fileName minus extension>"` (which is also why corpus
   faces print extension-free `import("/b")`); the AMD moduleName arm
   (pragma unparsed, zero conformance uses) and the fileName fallback
   are unreachable on the error path under that naming invariant —
   the quoted-name test still GATES (new named escape site) instead
   of asserting. The node16/nodenext resolution-mode attributes and
   /node_modules/ specifier-swap legs read impliedNodeFormat (not
   modeled): recorded T2 residue, row keys unaffected. Entity face:
   `typeof <name>` TypeQuery via the symbol_display_name posture
   (getNameOfSymbolAsWritten's module names are identifier text);
   globalThis rides it (SymbolFlags::MODULE, symbolName fallback).
   ImportType joined SliceTypeNodeKind: NO parenthesizer rule lists
   the kind (20540-20606), probed (`typeof import("/b") | null`
   renders bare).

3. **Anon-gate split** — CLASS/REGULAR_ENUM/CONST_ENUM stay curtained
   (intercepted upstream by the named-object arm; the gate is now a
   constructibility guard), VALUE_MODULE routes to the new face
   BEFORE the FUNCTION admission (tsc's 51779 disjunct order —
   pinned via the function+namespace merge printing `typeof f`).
   Upstream named-object arm fix: the typeof split adds VALUE_MODULE
   — a merged interface+namespace VALUE side printed the plain `X`
   reference (pre-existing 5.4-era infidelity, probe: tsc prints
   `typeof X`); class+ns/enum+ns splits pinned as controls.

4. **Fabrication audit: NEW_FP=10 → 0, both families fixed at
   source** (the 7.5 protocol, fourth consecutive slice it fired):
   - *Alias-blind value filters* (2×2353, namespaceImportTypeQuery2/3):
     `export { A }` over a local that merges a type-only import alias
     with a `const` is a VALUE property of the module face; tsc's
     symbolIsValue (50092-50094) FOLLOWS aliases via getSymbolFlags.
     `is_known_property` had hand-rolled an inline members.get +
     VALUE-flags probe (the 9.3b2 "second copy" pattern again — the
     faithful `get_property_of_object_type` existed one file over)
     and `get_named_members` carried a stale "alias resolution is M4"
     justification note (watch-pattern hit) — both now gate through
     the faithful `symbol_is_value` (get_symbol_flags_full was
     already fully ported in modules.rs).
   - *Merge-blind expando consults* (8×2339,
     typeFromPropertyAssignment32/33): the amalgamated-duplicates
     flush CLONES per-file symbols into fresh program symbols
     (cloneSymbol), and the stage-3.4c expando-record consults keyed
     the per-file binder symbol — a cross-file function+namespace
     merge lost its suppression and fabricated member-miss rows. New
     reverse index (merged_symbol_sources) +
     symbol_has_expando_assignment_merged /
     symbol_expando_covers_merged thread all four consult sites
     (report_nonexistent_property, the element-access 7053 ladder,
     the typeof-face admission, the declaration-relation
     containment) through merge sources transitively. tsc needs no
     equivalent — it binds expando members into the merged table
     itself; the indirection is the recorded 3.4c stand-in shape.

5. **Pins** (all oracle-probed byte-exact, incl. spans): unqualified
   nested-namespace faces; merged interface+ns `typeof X` with the
   type-position `X` control; class+ns/enum+ns controls; `typeof
   globalThis`; fn+ns disjunct order; ambient-module import face;
   source-file import face (extension-free specifier); the
   export-alias known-property program (2353 absent + properties
   introspection); expando+ns cross-file merge (suppression through
   the clone + the unassigned-name `typeof EM` row + tsc's 2433).
   Multi-file pins ride a new `program_diags` helper; the unit env's
   un-rooted fileNames print `import("b")` where corpus goldens show
   `import("/b")` (same naming rule, different fileName input —
   noted in the pins).

6. Residual FN attribution: fixture-32/33's p8/p9 assignment-face
   2322s stay contained (operators.rs, recorded 3.4c residual);
   remaining curtain 340 = the 9.3b4 operator shapes + the preserved
   empty-resolution shield (ObjectLiteral 107 / Transient|TypeLiteral
   61 — members unreal until their producers land) + the 9.3b5 tail.

## 9.3b4 results (2026-07-23, type operators — DONE)

Numbers (tool-read, band 2xxx): T0 84.7988% (17851/21051, +139)
FP=0; supported T0 87.0611% (17851/20504) FN=2,653 (from 2,792);
T2 83.5162% (+0.78pt) / T3 75.8491% (+0.54pt). Band all: 58.1450%
(28505/49024, +140) FP=0; syntactic unchanged. Generic display
curtain 340→200/201 (the ±1 is evidence-chooser drift between runs
on identical integers). 933 checker tests (9 new: 7 oracle-probed
display-pin batteries + the printer escape-table twin + the
fabrication-audit coercion pin). Escapes 252/0/0/116 (+1: the
StringMapping symbol-less constructibility gate shares the standard
curtain message).

1. **The arms** — typeToTypeNodeHelper's four operator faces
   (51569-51597) land in the structured slice in tsc order after the
   anonymous dispatch: `Index` → `keyof <operand>` (operand takes
   parenthesizeOperandOfTypeOperator; the union-origin substitution
   51536-51538 re-routes through the same helper, so keyof origins
   and direct deferred Index types share one renderer);
   `TemplateLiteral` → head/span/tail concatenation (span types join
   bare — createTemplateLiteralTypeSpan applies no parenthesizer);
   `StringMapping` → the intrinsic alias reference `Uppercase<T>`
   (symbolToTypeNode Type-meaning with one argument; Type::symbol is
   set at creation, the gate is a constructibility guard);
   `IndexedAccess` → `obj[index]` (OBJECT side takes
   parenthesizeNonArrayTypeOfPostfixType; index side bare). Two new
   SliceTypeNodeKind variants (TemplateLiteral, IndexedAccess) are
   parenthesizer-inert — no factory rule lists either kind, so they
   join every position bare; TypeOperator/Reference reuse covers the
   other two. All positions oracle-probed: union/intersection bare
   for all four, `(keyof T)[]`/`[(keyof T)?]`/`(keyof T)[K]` wraps
   vs `T[K][]`/`[T[K]?]`/`` `a${T}`[] ``/`[`a${T}`?]`/`T[K][K2]`/
   `T[keyof T]` bare, `readonly (keyof T)[]` composition.

2. **Template texts print through the printer's real re-escape** —
   getLiteralText's synthesized branch (13660-13677):
   escapeTemplateSubstitution(escapeNonAsciiString(text, backtick)),
   ported as template_text_raw + escape_string_backtick (the
   backtick regex/escapedCharsMap/getReplacement fold, 16275-16314)
   + escape_non_ascii_backtick (per-UTF-16-unit, astral = two
   surrogate escapes) + escape_template_substitution. Probe-pinned:
   `\r\n` pair, `\0` vs digit-lookahead `\x001`, unmapped control
   U+0001 → `\u0001`, `あ` → `\u3042`, `😀` → `\uD83D\uDE00`, lone
   `\r`, literal LF stays raw, `$`/`{` identity outside `${`. This
   is a FULL escape (no identity-admission curtain) — the 9.3b3
   "escapeString stays behind the curtain" posture was a
   specifier-face decision and stands there; template texts are
   the printer's own literal path.

3. **Fabrication audit: NEW_FP=2 → 0, fixed at source** (fifth
   consecutive slice the protocol fired; the display arm unmasked a
   pre-existing verdict infidelity whose reporting had been contained
   by the curtain Err): templateLiteralTypesPatterns `numbers("0b1")`
   / `numbers("0o1")` drew 2345 because structural.rs carried an
   M4-era local `js_string_to_number` slice missing the 0b/0o radix
   forms (and admitting Rust-parser "inf" spellings JS rejects) —
   the 9.3b2 second-copy pattern again: the faithful full ToNumber
   had landed in evaluate.rs at M6. The local slice now delegates
   (None-encodes-NaN face kept for its relation/inference callers),
   and is_numeric_literal_name_js runs tsc's raw
   `(+name).toString() === name` formula (so "NaN"/"Infinity" names
   count exactly like tsc). Oracle-probed pin: the ToNumber battery
   (`1`/`-1`/`0`/`0b1`/`0o1`/`0x1`/`1e21` admit, `other`/`inf`
   refuse, byte-exact spans).

4. **Producer facts the probes settled** (no port work needed — all
   pinned green first run): `keyof keyof T` and `keyof Uppercase<T>`
   resolve through the apparent type (never under noLib) rather than
   defer; `keyof (T & U)` distributes to `keyof T | keyof U`;
   `` `a${T | U}b` `` distributes at construction into a union of
   templates; a literal index over a generic template resolves
   through the apparent type (2339 on `{}` under noLib); the 65185
   nullable-candidate substitution strips `keyof T | null` report
   targets to `keyof T`. Source-position operator types generalize
   to their constraints in reportRelationError — display pins ride
   TARGET-position annotations.

5. Residual: remaining curtain 200 = the preserved empty-resolution
   shield (168) + the 9.3b5 tail (26) + a handful of rows whose
   operator face renders but whose FN has other owners; a throwaway
   inner-recursion census (tags on all five arm recursion sites, one
   band run, integers byte-identical, reverted) found zero curtain
   rows dying inside the new arms.

## Remaining implementation sequence after 9.3b2

The table in §Slice plan remains the phase contract. The following is
the concrete PR order from the 9.3b2 landing point. Names are minimum
reviewable slices, not permission to combine unrelated tsc branches;
split a row further when its fabrication audit finds more than one
semantic owner. One row/sub-row = one `p9/<slice>` branch and PR. The
serial spine is below; the explicitly parallel D2a/escape-infrastructure
lane follows the table.

| Order | Slice | Implementation boundary | Slice exit |
|---:|---|---|---|
| 1 | 9.3b3 symbol/value/module heads | Re-run the throwaway blocking-shape census over the current generic display curtain; port namespace/module value faces, `typeof` value-side heads, qualified symbol names, import-alias heads, `globalThis`, and external-module symbol references. Preserve the symbol-carrying empty-resolution shield until that symbol's members are real. | The selected shapes leave the generic curtain; target rows are T2/T3-pinned where the diagnostic path is live; all-band FP=0. |
| 2 | 9.3b4 type operators | Render the already-constructible `TemplateLiteral`, `StringMapping`, `Index` (`keyof`), and `IndexedAccess` shapes, including every parenthesizer position used by union/intersection/array/tuple/signature displays. | No newly constructible type is display-inert; target rows and local render pins match the oracle. |
| 3 | 9.3b5 display special tail | Close identically-named operator display, 2507 display, non-keyof origin-union, unique/private names, recursive aliases, and the re-censused tail. A non-keyof origin-union shield is removed only with proof that its flow/relation verdict is already faithful, or with that producer fix in the same slice. | Generic display curtain is zero for supported 2XXX rows over currently constructible types. Later-created mapped/conditional shapes are owned by 9.5/9.6 and must land with their renderer. |
| 4 | 9.3c shadow-tier identity diff | Serialize exact T1/T2/T3 matched/lost/gained identities or stable hashes in `conformance --out-json`, and add `cargo xtask conformance-diff <old> <new>`. Do not touch A1/ratchet artifacts or activate a corpus-wide tier. | Adversarial identity-swap tests pass; existing 9.3 evidence replays; the report is mandatory PR evidence from 9.4b onward but is not yet a hard no-loss gate. |
| 5 | 9.4a elaboration core | Extract the existing `operators.rs` reporting implementation and `calls.rs` disposition-only walk into one tsc-declaration-preserving elaboration module. Use an explicit reported/declined outcome; `Unsupported` is not the normal "declined to elaborate" result. This is behavior-preserving before the callers are widened. | Direct oracle pins preserve current rows; no accepted-set movement is required; duplicated elaboration decisions no longer have separate owners. |
| 6 | 9.4b object/array/arrow | Route call/assignment sites through the common engine; close object members, forced-tuple array/spread elaboration, arrow return-position recursion, and `getBestMatchingType`. | The object/array/arrow curtain reasons retire; moved inner rows match T0-T3 when chain prerequisites are live; exact T1-T3 diff is reviewed and each loss is fixed or carries an exact-row shared-prerequisite debt record. |
| 7 | 9.4c JSX/report heads | Add JSX component/attribute elementwise reporting and the remaining `reportRelationError` head selection to the common engine. The result representation must carry chain/related data rather than baking in a head-only API. | F4 supported rows have no elaboration curtain; exact T1-T3 diff is reviewed and each loss is fixed or carries an exact-row shared-prerequisite debt record. |
| 8 | 9.5a mapped model | Add the mapped-type immutable semantic payload, constructor/accessors, `getTypeFromMappedTypeNode`, and mapped display. Mutable resolved members/caches stay in `TypeLinks`; semantic identity does not become flags plus side-table convention. Update the core-interface contract in the same PR if the representation changes it. | D2a and the dormant-assumption census are landed; model/constructibility canaries pass and trigger the owned dormant audit; the producer reaches only named downstream escapes; creating a mapped type never creates an unrenderable type. |
| 9 | 9.5b mapped members/instantiation | Port mapped constraint/template/name types, key remapping, `+/-readonly`, `+/-?`, index infos, apparent members, and instantiation. | Member and instantiation canaries match; no broad `errorType` or empty-object fallback is introduced. |
| 10 | 9.5c mapped relations/context/inference | Activate `mappedTypeRelatedTo`, mapped indexed access, contextual substitution, homomorphic inference, and corpus-required reverse-mapped paths. Audit every pre-M8 constant-false/unreachable mapped assumption in `instantiate.rs`, `indexed.rs`, `structural.rs`, `contextual.rs`, `literals.rs`, `access.rs`, and `inference.rs`. | F3a supported rows close and every canary-triggered dormant assumption has a direct pin or a narrower named escape. |
| 11 | 9.6a conditional/substitution model | Add conditional root/type and substitution immutable payloads, interning/accessors, constraint hooks, and display. | Model/constructibility canaries pass and trigger the owned dormant audit; no accepted movement required; every producer has a renderer and unwind-safe cache protocol. |
| 12 | 9.6b NoInfer | Mint the NoInfer substitution type and activate all `isNoInferType` guards in inference/indexed/expression paths. | The mandatory NoInfer supported rows close; no NoInfer-specific escape remains. |
| 13 | 9.6c conditional resolution | Port check/extends/true/false resolution, distributivity, infer positions, constraints/default constraints, simplification, tail recursion, and 2589. | Conditional evaluation pins match and every newly triggered dormant assumption is retired or narrowed to an exact owned escape. |
| 14 | 9.6d conditional relation/inference | Port instantiation, relation, and dormant `inferToConditionalType` paths. Before widening candidate/inference trials, adopt the existing speculation transaction at a real production candidate boundary and prove commit/rollback behavior. | F3b supported rows close; every triggered conditional/substitution assumption has a direct pin or narrower exact escape; permanent cache writes remain forbidden during rollback-capable trials. |
| 15 | 9.7a recovery census | Partition the overload curtain by recovery shape and compare declaration kind/name/body presence, symbol-declaration order, and parse-error boundary against the oracle tree. Do not narrow the checker bail in this slice. | Reproducible fingerprints and minimal parser fixtures exist for every selected shape; semantic accepted sets are unchanged. |
| 16 | 9.7b recovery tree parity | Fix parser/binder recovery one shape family at a time, holding the syntactic gate and AST/schema audits. | Each fixed shape has tree and syntactic-diagnostic pins; no checker-side guessed boundary. |
| 17 | 9.7c overload bail retirement | Narrow, then remove, `checkFunctionOrConstructorSymbol`'s blanket recovery bail only for tree shapes proven equivalent. | F2 supported rows close and syntactic T0 remains at or above the pinned baseline. |
| 18 | 9.8a assignment-declaration binding | Centralize the non-JSDoc assignment-declaration classifier and bind function/property/prototype/module assignment forms into real symbols. | Assignment-kind and symbol-diff canaries match; JSDoc-driven scope records are untouched. |
| 19 | 9.8b expando members | Resolve expando members through the normal symbol/type paths and retire name-level diagnostic suppression and the stage-3.4c symbol-diff allowlist as their real producers land. | Binary/expando curtain reasons retire without suppressing neighboring unknown members. |
| 20 | 9.8c JS constructors/tail | Port JS-constructor, prototype/static, and remaining in-scope non-JSDoc checkJs paths. | F5 supported rows close; excluded JSDoc rows remain visible only in the all-corpus view. |
| 21+ | 9.9x owner-mined residue | Re-snapshot after 9.8 and rank supported FN buckets, but branch by tsc emitter/dependency owner, never by diagnostic code alone. Start with the heaviest shared prerequisites (expected candidates: in-program relative resolution 2834/2835/2307, `checkExternalEmitHelpers` 2343, parameter-initializer scope 2372/2373, namespace/member resolution 2694), then re-rank after every slice. | Each branch removes a fixed family snapshot or a measured prerequisite; phase closes only at supported T0-2xxx=100%, all-corpus FP=0. |

The model slices (9.4a, 9.5a, 9.6a, and 9.7a) are allowed to be
accepted-set neutral under the prerequisite-only rule in
[definition-of-done.md](definition-of-done.md#milestone-gates-vs-slice-fidelity).
They are not rate progress and must be followed by their named
consumer. The displayed sequence is deliberately serial at the model
activation seams: mapped and conditional construction turn previously
dead branches live across several modules, and recovery bail retirement
changes which semantic paths execute.

Parallel infrastructure lane after 9.3b5: D2a may run beside any of
9.4a-9.4c and has a hard landing deadline immediately before 9.5a.
It upgrades the existing inventory to schema 2, adds exact static
closure/SCC + ledger joins, and exposes the non-gating `port-plan`
display; automated probes/traces stay unavailable. The
`dormant-assumption` escape-class/canary extension and the initial
annotation census may ride D2a or a separate small infra PR, but share
the same 9.5a deadline. The writing-time grep upper bound is 32; the
transition adjudicates the sites and records the landed census count as
`[escapes].max_dormant`. The existing ceiling machinery then admits only
non-increasing updates. Neither artifact counts as a parity gain or
satisfies D2b/D3/M8 readiness.

## Per-slice tier policy

Phase 9's corpus-wide close gate remains supported T0-2xxx=100% and
absolute all-corpus FP=0. That staged gate does not authorize T0-only
implementations:

- A slice that makes a diagnostic family observable matches the target
  rows through the highest live tier. Display and elaboration slices
  normally require T2 top-message fidelity and T3 chain/related
  fidelity wherever the shared chain model is live.
- Before A3, T4 evidence is local formatter goldens or report-only
  output. It is not written into the accepted T4 state and cannot
  activate corpus-wide T4 early.
- Every slice records target-family T1/T2/T3 shadow before/after. A
  newly regressed previously-matching upper-tier identity is fixed in
  the slice; it is not traded for a T0 gain. Today the review evidence
  is the band-level shadow-rate delta plus the target-family rows in
  `mismatches.json`; through 9.3b5 the identity-level shadow diff is not
  yet automated. Slice 9.3c then adds exact per-tier identities. From
  9.4b onward, each touched family attaches the exact diff to PR
  evidence; each lost previously-matching T1/T2/T3 identity is fixed or
  gets the existing exact-row shared-prerequisite debt record. This
  remains a review requirement, not an automated no-loss gate, until
  that debt vocabulary is machine-readable. Gained identities remain
  report-only until formal tier activation; A1 artifacts do not change.
- When an upper tier is blocked by shared infrastructure, record exact
  rows, dependency owner, and retirement slice. Do not add a broad
  "nodeBuilder/T2/M8" reason for a shape whose producer is now known.

## Working rules

- Until D2a exact inventory/static call graph lands—no later than
  9.5a—use the manual equivalent to select a semantic slice boundary:
  inspect the target tsc call chain, run minimal emitting/non-emitting
  sibling oracle probes where constructible, and record the dependency
  boundary in the PR. This does not block 9.3b4/9.3b5 or 9.4. After
  D2a, every semantic slice carries its exact static cluster and
  `port-plan` report. B2 may later add the
  [trace-assisted cluster procedure](measurement-integrity.md#62-trace-assisted-implementation-clusters);
  D2a itself does not synthesize probes or runtime coverage.
- Dormant constructibility assumptions use `escapes.toml`
  `class = "dormant-assumption"` plus a named canary, never a second
  manifest and never `STAGE` expiry. Existing M8-stub stage rows are
  reclassified, not duplicated; one site has one class. The scanner also
  matches legacy `M8-stub`/`constant-false` markers; after the census
  transition it rejects a remaining marker without exactly one dormant
  manifest row. When the canary test exists and passes, the old
  annotation must disappear or narrow in the same PR. The 9.5a/9.6a
  exits require their owned constructibility canaries to exist, pass,
  and trigger a full owned-row audit.
- The owning PRs for elaboration, mapped, conditional/substitution,
  recovery, and JS assignment carry the convergence-plan subsystem
  matrix. Only one new type-model family is constructible at a time;
  finish mapped activation before conditional/substitution activation.
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
- Apply the vertical-slice policy above: active T0/set gates remain
  monotone, target-family shadow-tier deltas are part of the PR
  evidence, and pre-A3 T4 checks remain report-only/local. A
  prerequisite-only slice names its consuming family and is not
  described as a parity increase.
- One slice = one branch = one PR ([CLAUDE.md](../../../CLAUDE.md)
  workflow); ratchet.toml `[t0-2xxx]` + set-ratchet bumps ride the
  slice that GREW accepted identities, never the merge. Scope-only
  slices (9.1a/9.1b/9.1c/9.2) land with `ratchet check` green and
  ratchet artifacts byte-unchanged — the A2 supported view is not a
  ratchet view, so an all-corpus accepted match neither appears nor
  disappears there and a `ratchet update` would be a no-op by
  construction (9.1c changes only the A2 manifest, exactly like
  9.1a/9.1b).
- Escape hygiene: F1/F2 narrowings shrink existing curtain sites —
  after each, `escapes --write-manifest` and review the manifest
  diff; retired M7-re-owned rows are the visible progress ledger.
- Stop conditions unchanged (convergence plan §6): an exclusion that
  cannot select exactly one record, a missed-exclusion discovery
  after the pin, or three fixes hitting one model ceiling, stops
  the slice for design review.
