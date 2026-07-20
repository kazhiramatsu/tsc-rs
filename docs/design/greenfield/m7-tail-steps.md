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

One commit per family; each with oracle-probed micro pins.

Commit(s): `m7 8.1a-f: grammar check families (+rate)`.

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

Activate T1 in the comparator and ratchet:

```sh
cargo xtask conformance --tier t1     # first measurement; record + ratchet
```

Commit: `m7 8.4: suggestion band + T1 activation`.

Gate: the suggestion-pass family rows (unused suggestion half,
infer-from-usage residue, 80XXX, deprecations, flow-derived
surfacing) reach acceptance; T1 active and ratcheted.

## Stage 8.5: options + program-level diagnostics [M]

`getOptionsDiagnostics` port (invalid combinations, 5069/5052-family
the corpus exercises), the strict-family expansion
(`getStrictOptionValue`), file-level program diagnostics
(1148/6131-family, reference directives, case-collision 1149/1261),
exit-code semantics for the CLI, and the T4 output formatter
(`formatDiagnosticsWithColorAndContext`-shape minus color for the
hash — byte parity is M8's tier work; land the structure).

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

Gate: the program/resolution family rows reach acceptance; the T4
formatter structure is live
([measurement contract A3](measurement-integrity.md#4-a3--t4-activation)).

## Final gate

```sh
cargo xtask conformance              # expect: T0 ≥ 63%; T1 recorded
cargo xtask families report          # expect: every M7-owned family complete (readiness row 10)
cargo xtask invariants --suite all
cargo xtask ledger check
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
