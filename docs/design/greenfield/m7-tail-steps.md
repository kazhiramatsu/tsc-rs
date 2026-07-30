# M7: grammar checks, unused band, suggestions — steps

Parent design: greenfield.md §5 (checker organization, suppression
surfaces), §6 (suggestion band, emit-free contract);
checker-foundations.md §2 (the driver slots these fill). tsc regions:
the `checkGrammar*` family, `checkUnusedIdentifiers` (82954),
`getSuggestionDiagnostics` (123761). Prerequisites: M6 gate green
AND the 2XXX completion sweep closed — phase 9 first half,
[completion-convergence-plan.md](completion-convergence-plan.md) §4 row 9: all-corpus 2XXX FP = 0,
supported-scope 2XXX FN = 0, band exclusions pinned by a verifying
[A2 `2xxx` band-freeze record](measurement-integrity.md#31-draft-band-pins). No M7 stage starts on an open 2XXX
residue.

Gate: T0 ≥ 63% (calibration only — reachable from the unused family
alone); T1 (category-aware) measured and added to ratchet.toml. Each
stage below additionally closes on its own family rows
([completion plan C4](completion-convergence-plan.md#c4-m7-tail),
[non-2xxx-first-order.md](non-2xxx-first-order.md)); the
aggregate rate is never a substitute for a stage gate.

## Tier execution policy

M7 keeps the staged corpus gates while applying the vertical-slice
policy from
[definition-of-done.md](definition-of-done.md#milestone-gates-vs-slice-fidelity):

- 8.1-8.3 remain globally gated at T0, but every touched family records
  its T1/T2/T3 shadow delta and oracle-pins the highest live tier. A
  T0 gain may not consume a previously matching upper-tier identity.
- 8.4 activates the exact-count T1 aggregate gate in `ratchet.toml`.
  Category is therefore part of each 8.4-and-later family's close,
  not an M8 cleanup item. The fixture/matrix/bucket A1 identity set was
  not yet present; M8 replaces this legacy aggregate authority when
  T1-T3 activate atomically through
  `tier1-3-input-schema-extension`.
- 8.5 lands deterministic T4 formatter and program/global assembly
  structure. Before A3, its formatter goldens and T4 conformance are
  report-only validation; they do not create accepted T4 identities.
- M8 activates T2/T3 corpus gates and then A3 T4 byte parity after the
  A2 global freeze prerequisites hold. It closes the explicit
  upper-tier blockers left by otherwise-complete M7 families.

An upper tier may be deferred only to a named shared prerequisite with
exact affected rows, an owner, and a retirement stage. "T4 in M8" by
itself is not sufficient evidence.

## Required pre-implementation survey and band strategy

M7 does not begin a stage from aggregate FN codes alone. Before each
stage's first semantic edit, follow
[m7-band-and-owner-strategy.md](m7-band-and-owner-strategy.md):
use the A5 `(code, pass)` family as the virtual band, enumerate its
exact rows, trace representative rows through D2 emitter/dependency
owners and the current Rust boundary, oracle-probe positive/negative
shapes, and freeze one producer-owned slice queue with
`slice-evidence`.

This is the phase-9 method adapted to non-2XXX. A numeric band is only a
display grouping here; A5 family ownership and pass provenance decide
implementation scope.

## Stage 8.1: grammar checks [M]

Fill the driver's grammar slot (M4 stage 5.4 hook): port the
`checkGrammar*` family in checker.ts order — modifiers
(order/placement 1029/1044-family), computed property names,
parameter lists (rest/optional ordering), accessors, heritage
clauses, statement-position rules (1105 break/continue, labeled
rules), strict-mode checks not owned by the binder, `use strict`
+ ES-target gates (private names 18028-family, static blocks 18037/
18041 await/return rules), the regex-literal re-scan worker (the
error-reporting half deferred from M1a stage 1.6 —
`checkGrammarRegularExpressionLiteral` → scanRegularExpressionWorker
port, flag-vs-target 1501 checks), meta-property placement (17013
new.target via getNewTargetContainer), super-call ordering (17009/
17011).

MODULE-BAND ROWS (recorded 2026-07-20, M4-review slice 5 — B16, plus
the A-class residue A10/A11 re-owned here: the probed FP shapes live
outside the executed corpus matrix and M6 never touches the module
band, so they ride the checker-grammar family this stage gates).
Sequenced: (1) impliedNodeFormat goes TRI-STATE (A10 — modules.rs's
unwrap-to-CommonJS fallback becomes an Option: tsc computes implied
format only under Node16-19 + node_modules resolution and leaves it
undefined elsewhere; consumers canHaveSyntheticDefault and
is_esm_cjs_ref move with it — the 1192 bundler+`"type":"module"`+
export=+default-import face, tsc probed). (2) The Node16..Node18
sync-import 1471/1479 rows land ON the tri-state (B16 —
resolve_external_module's mode arms; the resolver DOES produce those
shapes: probe_module_candidates resolves .mts/.cts, so the old
"never constructed" reduction was false — site note at the header).
(3) export= 1203 gains its decisive-extension arm (A11 — tsc
86494-86499 uses impliedFormatForEmit extension-decisively for ALL
resolution kinds; `.cts`+module=esnext FP / ambient `.d.mts` FN both
probed; the oracle-correction epoch verified the node matrix only).
Related ladder sites already annotated: statements.rs
for-await 1309/1432, functions.rs checkAwaitGrammar 2856-family.

Current 2026-07-26 state: A10, B16, and A11 are accepted. B16 closed its
exact TS1471/1479/1541/1542 queue with 213 gains at T0/T1/T2/T3. A11
then closed `checkExportAssignment` with TS1203 x4 and one each of
TS1282/1283/1284/1285/1289: nine gains at every live tier, no loss,
no target-external movement, and all-corpus FP=0. The following
`getTypeFromImportTypeNode` slice closed all 72 TS1340 identities with
the same no-loss/FP0 boundary and raised the module-format canary set
to 3/4. The following lazy `onSuccessfullyResolvedSymbol` callback
slice closed TS1361 x33 and TS1362 x31, including exact TS1376/TS1377
related origins and the checked-JavaScript export-namespace row.
T0/T1/T2/T3 each gained 64 identities with no loss, target-external
movement, or false positive. The checker-grammar family is now
2,846/3,013 with supported FN 167 and canaries 3/4. The first 8.1f
slice then closed all 31 object-literal TS18016 rows through
`checkGrammarObjectLiteralExpression`; each live tier gained 31 with
no loss or external movement. The family is now 2,877/3,013 with
supported FN 136 and canaries 4/4. Exact position review then corrected
the TS18028 reconnaissance split: both residual rows are accessors, not
one method plus one accessor. Publishing the two
`checkGrammarAccessor` rows closes that fixture at 16/16 and moves the
family to 2,879/3,013 with supported FN 134. The next
`checkJSDocTypeIsInJsFile` slice closed all 12 TS17019/TS17020
nullable/non-nullable rows with +12 at every live tier, no loss, and
FP=0. The family is now 2,891/3,013 with supported FN 122 and canaries
4/4. The final 8.1f slice then closed all 12 residual TS18010 rows
owned by `checkJSDocAccessibilityModifiers`. Its producer-local
nearest-attached-comment projection reports the oracle tag-node span
without opening the general JSDoc checking surface. The two-fixture
target moved from 23/70 to 35/70; every live tier gained 12 identities
with no loss or FP. The family is now 2,903/3,013 with supported FN 110
and canaries 4/4. All 57 planned 8.1f identities are complete; a fresh
8.1g residual survey follows. Its first slice closed all eight TS1216
rows owned by `checkESModuleMarker`: the stale caller reduction to the
global module kind now uses the existing per-file emit-format seam for
Node CommonJS package files. The TypeScript and checked-JavaScript
fixtures moved from 24/32 to 32/32; every live tier gained eight
identities with no loss, target-external movement, or FP. The family is
now 2,911/3,013 with supported FN 102 and canaries 4/4. The next direct
owner slice closed all six package-`exports` TS1543 rows in
`checkImportDeclaration`. It projects only the resolved JSON target
file name through the existing diagnostic-only package path, leaving
ordinary package resolution suppressed. The target moved from 27/81 to
33/81; every live tier gained six identities with no loss,
target-external movement, or FP. The family is now 2,917/3,013 with
supported FN 96 and canaries 4/4. The following
`checkImportMetaProperty` slice then published the four already-exact
CommonJS-format TS1470 rows in checked JavaScript across the complete
Node16-through-NodeNext matrix. The target moved from 0/4 to 4/4; every
live tier gained four identities with no loss, target-external
movement, or FP. The family is now 2,921/3,013 with supported FN 92 and
canaries 4/4. The next `checkImportDeclaration` slice then selected the
JavaScript-specific TS1473 message for a nested plain-JS import while
retaining TS1232 for the TypeScript sibling. The target moved from
98/156 to 99/156; every live tier gained one identity with no loss,
target-external movement, or FP. The family is now 2,922/3,013 with
supported FN 91 and canaries 4/4. Its separate
`checkExportDeclaration` counterpart then selected TS1474 for the
plain-JavaScript nested export while retaining TS1233 for TypeScript.
The target moved from 99/156 to 100/156; every live tier gained one
identity with no loss, target-external movement, or FP. The family is
now 2,923/3,013 with supported FN 90 and canaries 4/4. The next
`checkAliasSymbol` slice closed its complete live producer queue:
TS1205 x3, TS1288 x1, the three TS1295 import-alias rows, TS1448 x1,
TS1484 x2, and TS1485 x1. The port reads the existing
isolated/verbatim options and alias metadata, keeps exact type-only
related origins, and uses the extension-sensitive CommonJS message
helper; the helper's `.cts`/`.cjs` TS1286 arm is micro-pinned even
though it has no corpus row. The five-fixture target moved from 8/58
to 19/58, with +11 at every live tier, no loss, no target-external
movement, and FP=0. The family is now 2,934/3,013 with supported FN 79
and canaries 4/4. The following `checkExportAssignment` slice published
the three CommonJS export-default TS1295 rows using the same helper;
its decisive predicate was already live for the neighboring
type-only branches. The target moved from 6/15 to 9/15 and every live
tier gained three identities with no loss, target-external movement,
or FP. The final TS1295 slice then closed the separate dynamic-import
row at `checkGrammarImportCallExpression`: its CommonJS/verbatim
branch now runs before all ordinary import-call grammar, publishes the
whole-call diagnostic through the shared extension-sensitive helper,
and retains ES module kind as a negative control. The target moved
from 9/15 to 10/15; every live tier gained one identity with no loss,
target-external movement, or FP. TS1295 now has no checker-grammar
residue. The family is now 2,938/3,013 with supported FN 75 and
canaries 4/4. Fresh owner review then corrected the preliminary
placement of the final TS1287 namespace row: tsc excludes module
declarations from `checkGrammarModifiers` and emits from nested
`checkModuleDeclarationDiagnostics` only after
`isInstantiatedModule`. The folded Rust
`check_module_declaration` boundary now follows that split, keeping
the type-only namespace and ES module kind clean. The target moved
from 10/15 to 11/15; every live tier gained one identity with no loss,
target-external movement, or FP. The family is now 2,939/3,013 with
supported FN 74 and canaries 4/4.

The binding 8.1a-g producer clusters, current reconnaissance anchors,
and per-slice exits are in
[the M7 band strategy](m7-band-and-owner-strategy.md#5-checker-grammar-entry-reconnaissance).
One producer owner per branch/PR; an A5 family may require several
producer slices and closes only after its full rollup and canaries are
green. Each slice carries oracle-probed micro pins.

Commit(s): `m7 8.1a-g: grammar check families (+rate)`.

Gate: the checker-grammar family rows (semantic-pass 1XXX plus the
grammar rows of 17XXX/18XXX) reach their family-map acceptance.

## Stage 8.2: suppression surfaces in one module [M]

Per greenfield §5: centralize the ported dedup/suppression rules —
errorType-silences-cascade sites, once-per-node and once-per-symbol
report marks, the duplicate-diagnostic dedup in the final sort. Audit
every M4-M6 emission site against this module; ad-hoc suppression
found elsewhere moves here or is deleted.

Commit: `m7 8.2: suppression surface audit`.

Gate: audit complete over every M4-M6 emission site, and the
suppression canary fixtures named in the family map match.

## Stage 8.3: unused identifiers [M]

`registerForUnusedIdentifiersCheck` + `checkUnusedIdentifiers`
(82954) and its per-kind workers (locals-and-parameters with the
grouping rules — per-list 6199/6198 vs per-element 6133, pattern
regrouping, trailing-rest suppression; class members incl. private
`#` names and setter-pairs; type parameters; imports incl. the
single-name statement-anchor form and type-only clauses). Underscore
exemptions per position; export/global/ambient visibility rules;
`isReferenced` marking discipline across the checker (the read/write
distinction: write-only accesses do not mark).

The category rule: under `noUnusedLocals/Parameters` these are
errors; otherwise they surface as suggestions — which requires stage
8.4's band to exist for the suggestion half. Land 8.3 emitting the
error-mode half first, gate, then wire the suggestion half in 8.4.

PREREQUISITE (recorded 2026-07-19, M4-review B18 — resolves the
5.8d residue in m4-58): the markAliasReferenced (L71909) /
markExportAsReferenced (L71945) family is entirely unported and
diagnostic-inert through M6 — 8.3 ports it FIRST. The unused band's
import rows (6133/6192 family) read the referenced flags that
family writes, and its resolveName(isUse) side-effects are the
marking discipline: landing the workers without the marking family
manufactures unused-import FPs on every used-via-alias name.

Commit(s): `m7 8.3a-b: unused identifiers (+rate)`.

Gate: the unused family's error-mode rows reach their family-map
acceptance (the suggestion half closes in 8.4).

## Stage 8.4: the suggestion band, emit-free [M]

Port `getSuggestionDiagnostics` (123761) composition: the unused
suggestions from 8.3, `infer-from-usage` 7043-7050 family, 80007
await-has-no-effect, 80008 big-literals, and the rest the corpus
exercises. THE CONTRACT (greenfield §6): the oracle driver never
emits, so the checker implements the emit-side visibility rules
DIRECTLY — module/enum instance-state marking
(`getModuleInstanceState` — bound in M2 stage 3.4) determines which
container names count as referenced under the no-emit rules;
`no_emit`/`preserve_const_enums`/`emit_declaration_only` gate them
(core-interfaces §8). Category plumbing: suggestions carry
DiagnosticCategory::Suggestion and the `reportsUnnecessary` bit.

Activate the M7 exact-count T1 aggregate gate (the A1 tier identity
schema intentionally waits for M8):

```sh
cargo xtask conformance               # measure T1 shadow count; record exact aggregate
```

Commit: `m7 8.4: suggestion band + T1 activation`.

Gate: the suggestion-pass family rows (unused suggestion half,
infer-from-usage residue, 80XXX, deprecations, flow-derived
surfacing) reach acceptance; the T1 exact-count aggregate is active
and ratcheted. This is not a claim that the A1 accepted artifact
already stores T1 identities.

## Stage 8.5: options + program-level diagnostics [M]

> **M8 supersession note:** the formatter text and gates below record the
> intended M7 contract. The M8 audit found that the actual schema-2 CLI-hash
> fields and fuzzer T4 path still hashed or compared structured diagnostic
> JSON rather than genuine rendered bytes. They provide no T4 acceptance
> evidence. A3/M8 replaces those placeholders with the vendored formatter's
> rendered bytes, a deterministic Rust formatter, and schema-3 SHA-256
> evidence under the
> [measurement-integrity contract](measurement-integrity.md#4-a3--t4-activation).

`getOptionsDiagnostics` port (invalid combinations, 5069/5052-family
the corpus exercises), the strict-family expansion
(`getStrictOptionValue`), file-level program diagnostics
(1148/6131-family, reference directives, case-collision 1149/1261),
exit-code semantics for the CLI, and the T4 output formatter
(`formatDiagnosticsWithColorAndContext`-shape minus color for the
hash). Land the real deterministic sort/dedupe, path/newline,
UTF-16-span, chain, related-information, suggestion, and file-less
diagnostic structure here. Corpus-wide accepted T4 byte parity is
still A3/M8 work because A3 cannot activate before exact scope is
globally frozen; 8.5 nevertheless carries local formatter goldens and
the report-only corpus diff so M8 is closing measured rendering residue,
not replacing a placeholder formatter.

DRIVER-BAND PREREQUISITE (recorded 2026-07-20, M4-review slice 5 —
B30/B31; the two land TOGETHER, B31 first or same commit):
(1) B31 — port skipTypeCheckingWorker's remaining arms (@ts-nocheck,
checkJs-off JS files, noCheck): today those files are CHECKED and
their rows dropped at assembly, where tsc never checks them at all.
Any file-less diagnostic such a check produces becomes an FP the
moment B30 lands, and the extra checking writes shared caches in an
order tsc never runs (an M6-era order-sensitivity risk — check.rs
site note). (2) B30 — replace the assembly layer's unconditional
file-less drop (lib.rs; today only the ImportMeta
visible_global_diagnostics carve-out survives) with tsc's
getDiagnosticsWorker global-snapshot regime: each per-file pull
compares the global-diagnostic snapshot before/after checking that
file and folds new global rows into the file's result, including the
empty-previous-snapshot concatenate arm (probed). The 2317-at-no-node
shape (globals.rs get_global_type_alias_symbol) is a live example the
port currently drops.

Commit: `m7 8.5: options + program diagnostics`.

Intended gate: the program/resolution family rows reach acceptance and the T4
formatter structure is live
([measurement contract A3](measurement-integrity.md#4-a3--t4-activation)).

## Final gate

```sh
cargo xtask conformance              # expect: T0 ≥ 63%; T1 recorded
cargo xtask families report          # expect: every M7-owned family complete (readiness row 10)
cargo xtask invariants --suite all
cargo xtask ledger check
cargo xtask oracle-refresh --render-hashes --check
cargo xtask conformance --tier t4 --report-only
cargo xtask m8 evidence produce --all # current runtime/fuzz/perf artifacts; approved runner
cargo xtask m8 readiness --require-ready
```

M7 closes the build plan; M8 is the mining loop (README) and M9
hardens the differential loop that the M8-readiness gate already
requires. Write `docs/NOTES-m7.md` with the T0
residue's top-20 codes — it is M8's opening backlog.

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Unused FNs cluster in parse-errored fixtures | per-node gate consuming statements too broadly | The gate is containsParseError on the ENCLOSING node (M1 flags), not per statement/file |
| Suggestion counts differ wildly on namespace/enum fixtures | emit-marking rules missing | Stage 8.4's instance-state rules are the contract; do NOT add emit to the oracle instead |
| One member reported per overload/merged symbol set | reporting keyed per declaration instead of per symbol (or vice versa) | Each unused family has an explicit anchor rule in the tsc source — port per family |
| T1 regresses while T0 climbs | category drift (error vs suggestion) | The band is decided by options + suppression rules, never by where the check lives |
