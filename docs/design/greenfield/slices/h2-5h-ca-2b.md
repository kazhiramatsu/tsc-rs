# H2.5h / CA-2b — corpus-adoption seam closures, cross-cutting cluster: module declaration keywords, the `__assign` ES5 fork, checker reporting parity

Status: design-gate packet for the second H2.5h corpus-adoption
packet and the first of the two CA-2 implementation packets. This
packet edits production Rust in three well-isolated clusters
(module-lowering declaration keywords, the es2018 object-spread
helper fork, and three checker/report-parity lanes) and closes
approximately 38 of the 212 failing census rows. The ES2015/wrapper
cluster (census families A/B/C/D/G/H) is CA-2a, the next packet.

## 1. Identity, purpose, and boundary

- **Slice ID / kind:** `h2-5h-ca-2b`, kind `runtime`.
- **Purpose:** close the census families E, F, and I so their rows'
  Rust execution matches the frozen CA-1 observations exactly:
  - **E — module-transformer declaration keyword at ES5** (~21
    rows): synthesized import/require bindings must emit `var`
    below ES2015 (currently hard-coded `NodeFlags::CONST`);
  - **F — object-spread `__assign` fork** (7 rows): the es2018
    object-spread lowering must call the `__assign` helper below
    ES2015 instead of `Object.assign` (the current
    `debug_assert!(target >= ES2015)` marks the seam and aborts
    debug builds on the band);
  - **I — checker/harness report parity** (10 rows): (i) TS2396
    must be reported on the PARAMETER node, not its name identifier
    (6 rows); (ii) the three autoAccessor rows: the PRODUCTION
    blocked-emit lane is ALREADY upstream-exact at the trusted base
    (independent-review probe: `ProgramSession` with
    `noEmitOnError` reproduces the frozen observation
    byte-for-byte) — the census divergence comes from the HARNESS:
    `apply_compiler_setting` silently drops `noEmitOnError` in its
    ignore arm, so the executions were never blocked and the
    pre-emit report gate correctly excluded the semantic bucket
    behind the 5107 options row; the fix maps the flag and extends
    the blocked-row comparison contract; (iii) one missing
    `Invalid value for '--ignoreDeprecations'` row (5103) in the
    `deprecatedCompilerOptions6` case (10 expected / 9 reported) —
    the programmatic options-diagnostics producer accepts the
    invalid value `"5.1"` silently (the config-file lane already
    validates correctly against the accepted set
    `{"5.0", "6.0"}`).
- **Non-goals:** the ES2015/wrapper cluster (promoteToIIFE
  exported/namespace/decorated lanes, comment ownership, generated
  names, static placement, captured-this fold, void-0 initializers)
  — all CA-2a; the project harness (CA-3); any acceptance/gate
  wiring, activity-model or admission change (CA-4); any edit to
  the CA-1 artifact's observations (they are the frozen target).
- **Prerequisites:** CA-1 merged (`a19bc3d7`, PR #468) — the
  `h2-5h-qualification` observation store is the expectation
  authority for every fix here.
- **Trusted base:** `a19bc3d7cda87ab1c172c6a41db86891a90fcaca`
  (current `main`).
- **Activation state:** before — the three clusters diverge from
  the frozen observations (census evidence, §2); after — the E/F/I
  census rows execute byte/diagnostic-exact under the census
  harness; every existing band's corpus conformance ratchet is
  BYTE-IDENTICAL; the 32-case witness gate and every focused
  B-train projection stay green.
- **Next owner:** CA-2a (the es2015 cluster), then CA-3/CA-4 per
  the ratified ladder.
- **Authority hashes (at the trusted base):**
  - `ratchets/h2-5h-qualification.v1.json`
    (post-merge bytes; verified in-train by
    `node crates/oracle/h2-5h-qualification.mjs --check`)
  - `vendor/typescript-6.0.3/lib/_tsc.js`
    `1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`
  - CA-1 packet `docs/design/greenfield/slices/h2-5h-ca-1.md`
    (ladder authority; envelope-pinned)

## 2. Position in the ladder + the census evidence

CA-1 §2 ratified `CA-2 — seam census + closures (runtime) …
splits at its own design gate if the census demands`. The census
(2026-08-23, full 850-row run through the live pipeline on a
scratch worktree; reproduction in §9.4) demands the split:

- 806 admitted / 44 deferred (H2.9, classification exact);
- **594 admitted rows already byte-exact (73.7%)** — activity
  bookkeeping aside (CA-4's model);
- 212 failing rows in nine root-cause families; the two clusters:
  - **CA-2b (this packet):** E ≈ 21, F = 7, I = 10;
  - **CA-2a (next):** A = 63 (promoteToIIFE lanes), B ≈ 34
    (comment ownership), C ≈ 19 (generated names), D ≈ 10 (static
    placement), G = 2 (captured-this fold), H (void-0 initializers
    and the residual multi-family overlap — ≈ 46 rows by
    subtraction from 212; family counts overlap per case, so the
    per-family sizes are sizing evidence, not a partition).

CA-2b goes first because its three clusters are small, mutually
independent, and file-disjoint from the CA-2a surface, giving the
corpus-adoption campaign an early, cheaply-reviewable win while the
larger es2015 packet is authored.

## 3. Required-reference table

| Reference | Role | State |
| --- | --- | --- |
| `ratchets/h2-5h-qualification.v1.json` cases (E/F/I rows) | frozen expectation store; per-case `typescript_observation` writes+diagnostics are the acceptance target | read-only (observations never edited; pin-lines may rebind in the walk under the CA-1 projection design) |
| `crates/emitter/src/builtins.rs` module-lowering sites (§5.E) | E production surface | edited |
| `crates/emitter/src/builtins/es2018.rs` `create_object_assign_call` + spread callers | F production surface | edited |
| `crates/emitter/src/builtins/helpers.rs` + `factory.rs` `EmitHelperName` + `transform.rs` helper table | F helper registration (`__assign` text is ABSENT today; only the `import_name` row "typescript:assign" → "__assign" exists, transform.rs:314) | edited (additive) |
| `crates/checker/src/functions.rs` `check_collision_with_arguments_in_generated_code` (2174-2215) | I(2396) surface — currently reports on the name | edited (span) |
| `crates/harness/src/upstream_suites/execution.rs` `apply_compiler_setting` (~790: the ignore arm containing `"noemitonerror"`) | I(18045) surface — the flag is silently dropped, so harness executions are never emit-blocked | edited (map the flag) |
| the PRODUCTION blocked-emit lane (`crates/emitter/src/execute.rs:310-322` + `EmitDiagnosticGate::collect_with_preflight` :54-66; filled by `emit_session_diagnostics` `crates/compiler/src/lib.rs:1432-1484`; `into_reported` :84-105 appends emit diagnostics post-gate) | verified upstream-exact at the trusted base (review probe) | READ-ONLY — explicitly untouched |
| the programmatic options-diagnostics producer (`programmatic_option_diagnostics`, `crates/compiler/src/lib.rs:1497+`; the silencing block :1673 has no invalid-value branch; message anchors :1825/:1901) | I(5103) surface | edited (one validation row) |
| `crates/program/src/config.rs:2896-2903` (config-file lane already validating `{"5.0","6.0"}`; green contract test `config_program_loader_contract.rs:1156-1167`) | the accepted-set authority the programmatic lane must mirror | read-only |
| `crates/emitter/src/builtins/jsx.rs` `create_object_assign` (:959-990) | the SECOND unforked `Object.assign` producer (JSX spread attributes; corpus-inert in the band — no admitted row uses spread attributes) | edited (same fork, projection-qualified) |
| B-1 helper precedent (`h2-5h-b-b-1.md`: byte-pinned helper texts + ledger tsc-hash + parity rows) | the `__assign` helper landing pattern | rationale |
| census artifacts (report, family case lists, byte-pair dumps, scratch patches) | authoring-time evidence; frozen in the session scratchpad and reproducible via §9.4 | evidence |

## 4. Pinned upstream map (all spans = `vendor/typescript-6.0.3/lib/_tsc.js`)

### 4.E Module declaration keyword — five decision sites

The identical verbatim decision at every site:

```js
factory2.createVariableDeclarationList(
  variables,
  languageVersion >= 2 /* ES2015 */ ? 2 /* Const */ : 0 /* None */
)
```

- `module.ts` `visitTopLevelImportDeclaration` 111189-111284,
  decision at **111241** (the `import * as NS` / `import def`
  require-binding list);
- **111277** (the AMD default-import alias binding);
- `visitTopLevelImportEqualsDeclaration` 111298-111390, decision at
  **111338** (`import X = require(...)`, CommonJS/UMD);
- `module/esnextAnd2015.ts` **113555** (the
  `createRequire(import.meta.url)` helper variable) and **113591**
  (`import X = require(...)` under ESM output).

Adjacent behavior a port must preserve: an **exported**
`import X = require()` emits `exports.X = require(...)` with NO
local declaration (111302-111317); named/default member access
rides the generated module temp (`<basename>_1`); AMD passes
modules as factory parameters; System.js hoists plain `var`
bindings assigned in setters — no keyword decision anywhere in
112049-113368.

### 4.F The `__assign` fork — in the helper factory

`createAssignHelper` **25724-25740**:

```js
function createAssignHelper(attributesSegments) {
  if (getEmitScriptTarget(context.getCompilerOptions()) >= 2 /* ES2015 */) {
    return factory2.createCallExpression(
      factory2.createPropertyAccessExpression(factory2.createIdentifier("Object"), "assign"), ...);
  }
  context.requestEmitHelper(assignHelper);
  return factory2.createCallExpression(getUnscopedHelperName("__assign"), ...);
```

Callers (the complete bundle inventory): es2018
`chunkObjectLiteralElements` 101979-102001 +
`visitObjectLiteralExpression` 102002-102021 (call sites
102011/102015; alternating plain/spread chunks; `{}` unshifted only
when the first element is a spread; pairwise LEFT-FOLD so both
lanes nest: `__assign(__assign({}, s), { b: 1 })`), and
`transformJsxAttributesToExpression` **104267** (JSX spread
ATTRIBUTES — not destructuring; destructuring object-rest uses
`__rest`). The predicate lives in the helper factory, so both
upstream callers inherit the fork; below ES2015 the call also
registers the `assignHelper` for the preamble. Our side has TWO
unforked producers matching the two upstream callers: the es2018
chokepoint (`create_object_assign_call`) and the JSX
spread-attribute builder (`jsx.rs create_object_assign`) — both
are fixed here (the JSX lane is corpus-inert in the band and lands
projection-qualified). The `__assign` helper declaration is the
vendored `assignHelper` object at **26122-26139** (name
`typescript:assign`, importName `__assign`, scoped false,
**priority 1**, text = the `(this && this.__assign) || function ()
{ __assign = Object.assign || function(t) {…} … }` fallback form;
tsc-hash
`a195c84f6fbb4280f8164fde5d33f0d246cfec5b40286c16416b042d9de991f1`),
byte-pinned at landing per the B-1 helper protocol.
`createAssignHelper` tsc-hash
`fc43c441a160dbfc108e0ed2174b29b7b0cc17f8c2e74eb82ba75404d7a1d1af`.

### 4.I(2396) `checkCollisionWithArgumentsInGeneratedCode` — parameter span

Checker **83229-83238**; caller `checkSignatureDeclaration` 81289
under `addLazyDiagnostic` (81313-81316):

```js
forEach(node.parameters, (p) => {
  if (p.name && !isBindingPattern(p.name) && p.name.escapedText === argumentsSymbol.escapedName) {
    errorSkippedOn("noEmit", p, Diagnostics.Duplicate_identifier_arguments_Compiler_uses_arguments_to_initialize_rest_parameters);
  }
});
```

The reported node is **`p` — the whole Parameter**, not `p.name`.
Guard: `languageVersion < ES2015 && hasRestParameter && !Ambient &&
body present`. Adjacent: `errorSkippedOn("noEmit", ...)` marks the
diagnostic `skippedOn: "noEmit"`, and `filterSemanticDiagnostics`
(**125664-125666**) drops such diagnostics when `--noEmit` is set —
the census band has no `noEmit` rows among the six, but the port
must keep the skippedOn marking (our
`error_skipped_on_no_emit` already does).

### 4.I(18045) the `noEmitOnError` lane — harness flag drop, production already exact

Upstream `handleNoEmitOptions` **125636-125663** (tsc-hash
`a65dad0c78a6053101f588db3b409009ccc6f6a3852b098a19c09074eede5773`)
defines the blocked-emit contract the three autoAccessor rows
witness:

```js
function handleNoEmitOptions(program, sourceFile, writeFile2, cancellationToken) {
  const options = program.getCompilerOptions();
  if (options.noEmit) { ... }
  if (!options.noEmitOnError) return void 0;
  let diagnostics = [
    ...program.getOptionsDiagnostics(cancellationToken),
    ...program.getSyntacticDiagnostics(sourceFile, cancellationToken),
    ...program.getGlobalDiagnostics(cancellationToken),
    ...program.getSemanticDiagnostics(sourceFile, cancellationToken)
  ];
  if (diagnostics.length === 0 && getEmitDeclarations(...)) { diagnostics = program.getDeclarationDiagnostics(...); }
  if (!diagnostics.length) return void 0;
  ...
  return { diagnostics, sourceMaps: void 0, emittedFiles, emitSkipped: true };
}
```

Interaction with the reporting assembly
(`emitFilesAndReportErrors`, **129412-129446**): the pre-emit
report gate includes the semantic bucket ONLY when
options+global+syntactic are empty — our `into_reported` mirrors
that correctly — and `program.emit()`'s RESULT diagnostics are
appended AFTER the gate, so under `noEmitOnError` with any error
the final sorted report contains 5107 AND the four 18045s (the
frozen observation's `emit_result.diagnostics` = `[5107, 18045×4]`,
`emit_skipped: true`, `exit_code: 1`).

**Our production lane already implements this exactly** — the
independent design review proved it empirically at the trusted
base: a `ProgramSession` probe with `no_emit_on_error` set on the
byte-exact autoAccessor1 source reproduces the frozen observation
byte-for-byte (blocked-emit construction
`crates/emitter/src/execute.rs:310-322` via
`EmitDiagnosticGate::collect_with_preflight` :54-66, buckets
filled by `emit_session_diagnostics`
`crates/compiler/src/lib.rs:1432-1484`; the packet's earlier draft
claimed a production defect here and the review falsified it).
The REAL defects are harness-side, and both are in scope:

1. `apply_compiler_setting`
   (`crates/harness/src/upstream_suites/execution.rs` ~790) lists
   `"noemitonerror"` in its silent-ignore arm, so qualified-vfs
   and recorded-plan executions never set
   `compiler_options.no_emit_on_error` — the census executions
   were never blocked, the pre-emit gate correctly excluded
   semantic behind the 5107 options row, and the actual reported
   set was `[5107]`. Ripple measured by the review: the only
   noEmitOnError rows in the 5g artifact are autoAccessor1/3/4 at
   ES2015, all error-free, so mapping the flag is behavior-neutral
   for the green 5g acceptance.
2. The cloned 5g comparison
   (`execute_slice_observed_with_inputs`,
   `crates/xtask/src/h2_2c_acceptance.rs` ~866-888) hard-fails any
   case whose expected or actual `emit_result.diagnostics` is
   non-empty — blocked rows can never compare green under it. The
   committed comparison extension is CA-4's `run_h2_5h` wiring
   obligation; for THIS packet the census harness's comparison
   (scratch, §9.4) gains the blocked-row contract: exact
   `emit_result.diagnostics` (codes+spans+messages+order after the
   upstream sort), `emit_skipped`, and the exit-code derivation
   must all equal the frozen observation.

### 4.I(5103-family) the missing deprecation row

The `deprecatedCompilerOptions6` case (`/foo/tsconfig.json`,
options: module amd, target ES3, noImplicitUseStrict,
keyofStringsOnly, suppressExcessPropertyErrors,
suppressImplicitAnyIndexErrors, noStrictGenericChecks, charset,
out, `ignoreDeprecations: "5.1"`) expects 10 deprecation-family
rows (5102/5103/5107/5108); we emit 9 — the missing row is code
5103 at config offset 364/length 5 (the `Invalid value for
'--ignoreDeprecations'` diagnostic: the ACCEPTED SET in 6.0.3 is
exactly **`{"5.0", "6.0"}`** — `getIgnoreDeprecationsVersion`
**125052-125061**, tsc-hash
`b2c4d400ad76484db1b452d49227201a36429c1f7cadd2f8ecbebe8f037919bf`
— and `"5.1"` is outside it; producer
`reportInvalidIgnoreDeprecations`, **122639**, the memoized
`createOptionValueDiagnostic("ignoreDeprecations",
Diagnostics.Invalid_value_for_ignoreDeprecations)`). Our
CONFIG-FILE lane already validates exactly this set
(`crates/program/src/config.rs:2896-2903`, green contract test) —
the divergent lane is the PROGRAMMATIC options-diagnostics
producer (`programmatic_option_diagnostics`,
`crates/compiler/src/lib.rs:1497+`), which has no invalid-value
branch and accepts `"5.1"` silently. Live exposure guard: the 5g
artifact's `deprecatedCompilerOptions2` and the 5h band's
`deprecatedCompilerOptions4/5` carry `ignoreDeprecations: "5.0"`
(VALID — must stay accepted); rejecting `"5.0"` would break the
green 5g acceptance replay, so the fix mirrors the config lane's
set verbatim, review-probed against both polarities.

## 5. Rust design

### 5.E Declaration keyword threading

Our five hard-coded `NodeFlags::CONST` module-lowering sites in
`crates/emitter/src/builtins.rs` map 1:1 onto the upstream
decisions:

| Ours (builtins.rs) | Upstream | Lane |
| --- | --- | --- |
| :1364 | **113591** | ESM `transform_import_equals` (`visitImportEqualsDeclaration`; guarded to module ≥ Node16 or Preserve) |
| :1468 | **113555** | ESM `createRequire(import.meta.url)` helper var |
| :4787 | **111277** | AMD namespace/default alias |
| :4829 | **111241** | import declaration require-binding list |
| :5087 | **111338** | `visitTopLevelImportEqualsDeclaration` — the `(LocalBinding, non-AMD)` require binding |

Site-inventory completeness (review-verified): exactly five
`Const : None` ternaries exist bundle-wide; our builtins.rs has
exactly five module-lowering `NodeFlags::CONST` sites;
`builtins/system.rs` has none (matching upstream 112049-113368);
the two builtins.rs LET sites (:9348/:13124) are
TypeScript-transformer surface re-lowered downstream. Each site
takes the predicate `if target >= ES2015 { NodeFlags::CONST } else
{ NodeFlags::NONE }`. **Neither module transformer currently
carries the script target** (`EcmaScriptModuleTransformer`
builtins.rs:1065, constructed :400-406 with
module_kind/rewrite/import_helpers only;
`CommonJsModuleTransformer` :2207-2213; the
`TransformationContext` exposes no target accessor), so both
structs gain a `target: ScriptTarget` field threaded from
`options.emit_script_target()` at their construction sites — new
state, deliberately minimal. The E defect is empirically witnessed
(review probe): ES5+CommonJS namespace import emits
`const m = __importStar(require("./task"));` where upstream emits
`var`. The census rerun plus the ≥ES2015 corpus byte-identity
prove five-site completeness.

### 5.F The fork + the `__assign` helper registration

- `EmitHelperName::Assign` variant + `text() = "__assign"` +
  priority/ordering row exactly where upstream's assignHelper sits
  (priority 1, after `__extends`' 0 — read the vendored helper
  object at landing and byte-pin);
- the helper TEXT byte-pinned from the vendored declaration with a
  ledger tsc-hash header + a byte-parity test row (B-1 protocol);
  the `import_name` table row already exists ("typescript:assign",
  transform.rs:314) — unchanged;
- `create_object_assign_call` (es2018.rs:3673-3682): replace the
  `debug_assert!(self.target >= ScriptTarget::ES2015)` with the
  fork — `>= ES2015` keeps the `Object.assign` property-access
  call; `< ES2015` requests the Assign helper on the context and
  calls the unscoped helper name (the `request_emit_helper` +
  `create_unscoped_helper_identifier` machinery already exists in
  es2018.rs). Every es2018 caller funnels through this chokepoint.
- `jsx.rs create_object_assign` (:961-990, called :959): the SAME
  fork — this is our analog of upstream's second
  `createAssignHelper` caller (`transformJsxAttributesToExpression`
  104267, JSX spread attributes). Corpus-inert in the band (no
  admitted row uses spread attributes — census-measured), so it
  lands projection-qualified: a focused fresh-process oracle
  projection (JSX spread-attribute fixture at ES5 and the ≥ES2015
  polarity) is its qualification, the b2-probe pattern.

### 5.I Checker/report parity

- **2396 span:** `check_collision_with_arguments_in_generated_code`
  passes the PARAMETER node to `error_skipped_on_no_emit`
  (functions.rs:~2211-2215 today passes the name). One-line change
  plus the doc-header span note. Review-probed: our current rows
  differ from the frozen observations in span ONLY
  ((2396,24,9) vs (2396,21,12) etc.), so the span fix alone flips
  the six rows.
- **`noEmitOnError` harness mapping:** move `"noemitonerror"` out
  of `apply_compiler_setting`'s ignore arm and map it to
  `compiler_options.no_emit_on_error` (both execution routes). The
  PRODUCTION blocked-emit lane is untouched (verified
  upstream-exact, §4.I). The blocked-row comparison contract lands
  in the census harness (§9.4) and is recorded as CA-4's
  committed-acceptance obligation.
- **5103:** the programmatic options-diagnostics producer gains
  the invalid-value branch mirroring the config lane's accepted
  set **`{"5.0", "6.0"}`** verbatim
  (`crates/program/src/config.rs:2896-2903` is the in-tree
  authority; upstream `getIgnoreDeprecationsVersion`
  125052-125061), reporting 5103 with the exact upstream
  message/span. Both polarities tested: `"5.1"` → the row;
  `"5.0"`/`"6.0"` → no row (guarding the green
  `deprecatedCompilerOptions2/4/5` cases).

## 6. Gap delta and the local-gap matrix

No `h2-5h-a-gap-matrix` capability rows change (all 13 remain
`exists`; these are fidelity repairs inside existing capabilities).
The census families E/F/I flip from diverging to exact — tracked by
this packet's §9 acceptance, not by the matrix.

Per-site local-gap classification (the §1.1-mandated form):

| Site | Classification | Step | Test |
| --- | --- | --- | --- |
| builtins.rs :1364/:1468/:4787/:4829/:5087 keyword | partial-or-stale (CONST hard-coded; correct ≥ES2015) | 2 | per-lane ES5+ES2017 projections |
| module transformer structs (target field) | missing (no target state) | 2 | compile + the same projections |
| helpers `__assign` registration | missing | 1 | byte-parity row |
| es2018 `create_object_assign_call` fork | partial-or-stale (assert marks the seam) | 1 | ES5 spread byte-equal |
| jsx `create_object_assign` fork | partial-or-stale (unconditional `Object.assign`) | 3 | ES5+≥ES2015 projections |
| checker 2396 span | partial-or-stale (name vs parameter) | 4 | collision row replay |
| harness `noEmitOnError` mapping | missing (silent ignore) | 5 | autoAccessor1 end-to-end replay |
| programmatic `ignoreDeprecations` validation | missing (no invalid-value branch) | 6 | both-polarity tests |
| production blocked-emit lane | already-exact (review probe) | — | untouched; the replay test exercises it |

## 7. Implementation plan (dependency order)

1. **F helper registration**: vendored `assignHelper` slice read +
   byte-pin (helpers.rs text + factory.rs variant + priority row +
   parity-test row + ledger tsc-hash), then the
   `create_object_assign_call` fork (es2018.rs). Focused test:
   object-spread fixture at ES5 → byte-equal to the F-family
   census observation; the ES2017 polarity stays `Object.assign`
   (existing projections).
2. **E keyword threading**: the five builtins.rs sites; focused
   projections per lane (ES5 → `var`, ES2017 → `const`) against
   fresh-process oracle emits (b2-probe pattern), sources = the
   census E rows' minimal shapes.
3. **F2 — the JSX spread-attribute fork** (`jsx.rs
   create_object_assign`): same fork; focused ES5+≥ES2015
   fresh-process oracle projections (corpus-inert lane).
4. **I(2396) span fix** + focused diagnostic test replaying one
   collisionArguments census row's expected set (codes+spans
   embedded from the frozen observation).
5. **I(18045) harness mapping**: `"noemitonerror"` out of the
   ignore arm → `no_emit_on_error` (both routes) + focused test
   replaying the autoAccessor1 expected observation END-TO-END
   through the harness loader (reported [5107, 18045×4],
   `emit_result.diagnostics` equal, `emit_skipped`, exit 1). The
   production emit lane is NOT edited.
6. **I(5103) invalid-ignoreDeprecations row** in the programmatic
   producer + focused tests for both polarities
   (deprecatedCompilerOptions6's 10-row set; a "5.0" control stays
   9-row-free of 5103).
7. **Census verification sweep** (§9.3) + §8 amendments + chain
   walk + gate.

Allowed files: `crates/emitter/src/builtins.rs` (the five keyword
sites + the two transformer-struct constructions ONLY),
`crates/emitter/src/builtins/es2018.rs`,
`crates/emitter/src/builtins/jsx.rs`,
`crates/emitter/src/builtins/helpers.rs`,
`crates/emitter/src/factory.rs`,
`crates/harness/src/upstream_suites/execution.rs`,
`crates/checker/src/functions.rs`,
`crates/compiler/src/lib.rs` (the programmatic
options-diagnostics producer ONLY — the emit gate and
`into_reported` are read-only), their focused test files, and the
§8 evidence/doc surfaces. Forbidden:
`crates/emitter/src/builtins/es2015.rs`,
`generated_bindings.rs`, the builtins.rs promote/wrapper lanes
(CA-2a's surface), `crates/emitter/src/execute.rs`,
`crates/program`, `crates/xtask` (no wiring), `.github/workflows`.

## 8. Evidence, ratchet, and documentation amendments

1. `h2-5h-a.md` item 4: the CA-2 entry gains the ratified split
   note (CA-2b LANDED at the implementation sha; CA-2a next).
2. `slices/README.md`: one CA-2b row.
3. Envelope `h2-5h-ca-2b` (ready; predecessors = [h2-5h-ca-1
   receipt]); bootstrap `allowedPacketIds += h2-5h-ca-2b`;
   h2-5h-a envelope digest re-pin.
4. **Chain walk (crate-byte train — the full cascade):** h1
   ladder re-mints (crate Rust bytes changed) → transition →
   1a-qualification/profile → the executed wave → 3c/3d →
   5g-qualification (write adoption; its receipt re-mints) →
   5g-profile → **h2-5h-qualification re-mint (adoption: its
   `global_candidate_dispositions`/`owner_inventory`/
   `project_classification` inputs are projection-EXCLUDED by the
   CA-1 design, so all 850 observations adopt; pin-lines only)** →
   the h2-5h-a chain in the CA-1-corrected order (comment-scope →
   owner-graph → gap-matrix → dispositions → es2015-witnesses
   LAST) → harness pin re-pins → verify battery + registry +
   readiness chain + pin-sweep. Budget ~25-40 min demoted; walk
   BEFORE `--lane rust`; batch fmt/clippy before the first walk.
5. New checker/compiler pub fns (if any) carry disposition markers
   at authoring time (`cargo xtask ledger check` targeted).

## 9. Acceptance

1. Focused tests green (steps 1-5 above), each replaying frozen
   observation bytes/diagnostic sets — no hand-authored
   expectations.
2. Full local gate at the final head: corpus ratchet
   **BYTE-IDENTICAL** (T0=100% all bands; the census E/F/I fixture
   variants are outside the corpus universe — measured by the
   ratchet itself), escapes/relpin/ledger/fuzz/invariants green.
3. **Census verification sweep:** the scratch census harness
   (§9.4) over the E/F/I row indices reports ZERO failing rows for
   these families (the A/B/C/D/G/H rows still fail — CA-2a's
   inventory). **MEASURED (2026-08-23, full 850-row rerun at the
   implementation bytes): 212 → 185 failing** — 27 rows recovered;
   the delta from the naive 38-row sum is the documented
   multi-family overlap (an E/F row that also carries a CA-2a
   defect correctly keeps failing on that component — e.g. the
   collisionArguments rows progressed from diagnostics-differ to
   the C-family `_i` write component, and
   taggedTemplateStringsWithCurriedFunction from the `__assign`
   fork to the B-family comment-before-helper component). Family
   verdicts on the complete data: diagnostics-differ = 0 (I fully
   swept), every spot-checked pure-E/F row byte-exact,
   promoteToIIFE unchanged at 63.
4. Census reproduction (authority): worktree at the packet head +
   the frozen scratch patches (census command
   `TSRS_H2_5H_CENSUS=1 ./target/debug/xtask h2-5h-census --start
   <i> --end <j>`, cloned from `run_h2_5g_inventory` with the 5h
   header consts; the activity-bookkeeping skip is census-only —
   the accounting model is CA-4's). **Blocked-row comparison
   contract (census-side, this train):** for a case whose frozen
   observation has `emit_skipped: true` with non-empty
   `emit_result.diagnostics`, the census comparison asserts exact
   equality of the emit result's diagnostic sequence
   (code/category/file/start/length/message, upstream sort order),
   `emit_skipped`, zero writes, and the exit-code derivation —
   replacing the cloned 5g arms that hard-fail on any non-empty
   `emit_result.diagnostics`. The same contract is recorded as
   CA-4's committed `run_h2_5h` obligation. The committed
   acceptance for THIS packet remains the focused tests + the
   gate; the band-wide machine gate arrives with CA-4.
5. Hosted `gates` green at the final head; merge via PR
   (merge commit only).

## 10. Traceability

| Deliverable | Upstream pin (tsc-hash where load-bearing) | Rust surface | Test |
| --- | --- | --- | --- |
| E keyword ×5 + target threading | 111241/111277/111338/113555/113591 (decl hashes: import-decl `d2de4a9f6f71…`, import-equals `ab7eb4340bab…`) | builtins.rs 1364/1468/4787/4829/5087 + both transformer ctors | per-lane ES5+ES2017 projections |
| F fork + helper | createAssignHelper 25724-25740 `fc43c441a160…`; assignHelper 26122-26139 `a195c84f6fbb…`; callers 102011/102015 | es2018.rs 3673 + helpers/factory | ES5 spread byte-equal + helper parity |
| F2 JSX fork | caller 104267 | jsx.rs 959-990 | ES5+≥ES2015 projections (corpus-inert lane) |
| I 2396 span | 83229-83238 `2a65d820227a…` | functions.rs ~2211-2215 | collision row replay |
| I noEmitOnError mapping | handleNoEmitOptions 125636-125663 `a65dad0c78a6…` (contract; production already exact) | harness execution.rs ~790 | autoAccessor1 end-to-end replay |
| I 5103 row | getIgnoreDeprecationsVersion 125052-125061 `b2c4d400ad76…`; producer 122639 | compiler programmatic producer | both-polarity replay |

## 11. Prohibitions

No fixture/case-ID branches; no output text substitution; no
hand-authored expected bytes/diagnostics (every expectation
replays the frozen CA-1 observation or a fresh-process oracle
emit); no edit to CA-1 observations; no activity-model change; no
CA-2a-surface edit; no acceptance wiring; no raised ceiling.

## 12. Unresolved items

None. An independent design-gate review (2026-08-23,
fresh-context agent, every span quote-read and the I-cluster
claims probe-tested against the workspace crates) returned
NOT-READY on the first draft with 1 blocker + 4 fixes + 3 notes,
ALL folded into this document before the design-gate commit:

- **Blocker (falsified mechanism):** the draft claimed the
  production blocked-emit lane drops the semantic bucket; the
  review's `ProgramSession` probe reproduced the frozen
  autoAccessor observation byte-for-byte at the trusted base —
  the production lane is upstream-exact. The re-scoped defects are
  the harness `noEmitOnError` silent drop and the blocked-row
  comparison contract (§4.I, §5.I, §7 steps 5/§9.4 rewritten; the
  compiler emit gate moved to read-only). The draft's
  "instrumented proof" had shown only that the checker PRODUCES
  the 18045s — the inference about where they were lost was wrong.
- **Fixes:** the 5103 accepted set corrected to `{"5.0","6.0"}`
  (the draft's "6.0-only" would have regressed the green
  `"5.0"`-carrying cases); the E mapping table corrected
  (1364→113591, 5087→111338); the nonexistent "existing target
  accessor" claim replaced with the explicit target-field
  threading; upstream caller 104267 re-identified as JSX spread
  attributes, surfacing our second unforked producer (jsx.rs) —
  included as F2 rather than left as a silent parity gap.
- **Notes:** family-H residual sized; the `__assign` text form
  named precisely; tsc-hash identities added for the pinned
  declarations.

Remaining mechanical lookups at implementation (not design
decisions): the exact dedent/storage convention for the helper
text (mirror `EXTENDS_HELPER_TEXT`), and the harness-side blocked
exit-code derivation (read from the existing harness exit
mapping).

## 13. Citation status

Verified at authoring AND independently re-verified by the design
review (which additionally probe-tested the I-cluster claims
against the workspace crates): every upstream span in §4 read
verbatim from the vendored bundle; the five-site keyword inventory
proven bundle-complete; our Rust anchors (builtins.rs CONST sites
+ transformer ctors, es2018.rs:3673, jsx.rs:959-990,
functions.rs:2174-2215, harness execution.rs:~790, compiler
`into_reported` :84-105 / `emit_session_diagnostics` :1432-1484 /
`programmatic_option_diagnostics` :1497+,
program config.rs:2896-2903) read in-tree at the trusted base;
the production blocked-emit lane proven upstream-exact by the
review's `ProgramSession` probe (reproducing the frozen
autoAccessor1 observation byte-for-byte); the 2396 span-only
delta, the E `const`-emission defect, and the es2018 ES5
debug-assert panic each empirically witnessed; the census counts
recomputed from the full-band run log; every tsc-hash computed
over the exact line slices by `shasum`-equivalent tooling.
