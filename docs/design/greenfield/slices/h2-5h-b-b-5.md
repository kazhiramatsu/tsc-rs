# H2.5h-b / B-5 — the runtime flip: tagged-template module, joint registration, the 32-case witness gate

Design-gate packet for the FIFTH and final H2.5h-b implementation packet,
under the mandatory implementation-ready design gate
(../post-h1-completion-slices.md). Authored at the train start
(2026-08-23) on `h2/5h-b-b5` from the post-B-4 trunk; reviewed
(independent pass: both shared-module byte hashes recomputed against the
owner graph, all four §4 line-slice hashes recomputed from the vendored
file, the registration slice re-read and byte-verified against
`upstream_registration`, every §5 Rust anchor opened at the trusted
base, the witness artifact's 32 cases enumerated with their roles,
option sets, and expected codes, and the §12 blast-radius measurements
re-run — zero H2.5h deferral rows across all qualification artifacts,
no-emit conformance path confirmed, no ES5-rejection negative control
found in any suite). The design-gate pass lands with the trusted base,
envelope, bootstrap, and index in one commit before any production
edit. Machine check:
`node .github/ci/slice-readiness.mjs --check h2-5h-b-b-5`.

## 1. Identity, purpose, and boundary

- **Slice ID:** `h2-5h-b-b-5`. **Kind:** `runtime` — the FIRST and only
  runtime packet of the ratified B-ladder ([B-1 packet](h2-5h-b-b-1.md)
  §2): it flips the joint `[transformES2015, transformGenerators]`
  registration live at `languageVersion < ES2015` and closes the
  H2.5h-b implementation ladder. Three deliverables:
  1. **The tagged-template shared module**
     (`crates/emitter/src/builtins/tagged_template.rs`):
     `processTaggedTemplateExpression` + `createTemplateCooked`
     (owner-graph shared module `tagged-template`) plus the external
     `getRawLiteral` utility, consumed by the ES2015 owner at
     `ProcessLevel::All`; the `__makeTemplateObject` helper text
     (`typescript:makeTemplateObject`, priority 0) joins
     `builtins/helpers.rs` and `EmitHelperName` gains
     `MakeTemplateObject`. B-4's typed fail-closed seam in
     `visit_tagged_template_expression` (es2015.rs:2808-2816) is
     replaced by the real module call; the dormant
     `tagged_template_string_declarations` recording and source-file
     tail (es2015.rs:904, :2424-2429, landed B-4) become live.
  2. **The registration flip**
     (`crates/emitter/src/builtins.rs`): the admission floor moves
     from ES2015 to ES5 (`emit_script_target()` maps ES3 to unset →
     ES2025, so ES5 is the true 6.0.3 floor), the pipeline pushes
     `transform_es2015` then `transform_generators` after the es2016
     entry and before the module transformer — exactly the upstream
     registration order (`_tsc.js:115942-115945`, registration_sha256
     `f13bde7bd85c8fdc67b85ccc4dd8d86809cc3e7719518417e3d20b3ba7794066`
     pinned in the owner graph) — and `H2ActivityCanary` gains
     `h2_5h_profile()` with the `H2_5h` observation at
     `target < ES2015`; all four production `h2_5g_profile()` callers
     flip (builtins.rs:107, builtins.rs:122, execute.rs:271,
     compiler/src/lib.rs:765).
  3. **The 32-case witness fixture gate** (the CS-6 analog): a new
     full-pipeline integration contract in
     `crates/compiler/tests/integration/` drives every case of the
     frozen `ratchets/h2-5h-a-es2015-generators-witnesses.v1.json`
     end to end (parse → bind → check → emit with the production
     checker resolver); the frozen oracle bytes and
     `expected_reported_codes` are the ENTIRE expectation, run twice
     per case (the artifact's `repetitions: 2`), and a red case is
     fixed in production under the frozen bytes' authority — never by
     amending the witness.
- **Non-goals:** any es2018.rs edit (the upstream ES2018
  `visitTaggedTemplateExpression` → `ProcessLevel::LiftRestriction`
  consumer, `_tsc.js:102047-102056`, stays unwired: our parse records
  cannot represent the invalid-escape ES2018 transform-flag half
  (builtins.rs:13892-13896, the B-4 classifier disposition), so its
  lane remains an inherited known gap owned by the es2018
  requalification concern of the H2.5h-b closure — §12.6); any
  `ts-tests` corpus adoption for the new target band (the
  `h2-5h-qualification` corpus sweep and its hosted acceptance runner
  are the NEXT slice — the [h2-5h-a.md](h2-5h-a.md) hosted-runner
  clause is conditional and NOT triggered here, so
  `cargo xtask acceptance` keeps its current fixed suite roster);
  any printer/transform.rs/resolver.rs/generators.rs/
  flatten_destructuring.rs edit; any witness amendment; any change to
  corpus outputs for targets ≥ ES2015 (the ratchet is the
  enforcement).
- **Prerequisites:** B-1 @02f784d9 (helpers ×4 byte-pinned,
  six-query resolver surface incl. the production checker bridge,
  eager name generation, EA-GAP-FLAGS classifier, hook chaining
  order contracts), B-2 @28f04d95 (destructuring flattener,
  `FlattenHost`), B-3 @548200df (Generators state machine), B-4
  @7308f9fc merge of 366c0425+ (the complete dormant
  `Es2015Transformer`, 123 byte-equal focused projections through
  the real joint chain, the tagged-template fail-closed seam).
- **Trusted base:** `7308f9fcf4320b4e81ac4858264161d9dfadcc91` (main
  after the B-4 merge). Authority artifacts at that base:
  owner graph `ratchets/h2-5h-a-owner-graph.v1.json`
  sha256 `963277b12533b854a325dcb782e0ede6b79e5eba83baa269ca35dacce2ea5cd4`
  (fingerprint `f41c4a0b271a22769894aad67dd1f2d25858ec5024d80ab4669e3167cd94fbf5`);
  gap matrix `ratchets/h2-5h-a-gap-matrix.v1.json`
  sha256 `fcd455586592742f344dd448e8d47448988865e20457dd7e842528ba142146b1`
  (fingerprint `52de7a8310d8ad7fce9c4231e20da91c39f398d5f2e7fb8ba853e4038ab73d8b`);
  witness artifact `ratchets/h2-5h-a-es2015-generators-witnesses.v1.json`
  sha256 `6722f091217254f90ac3ce85cceef228cad07f1936048b8a592ae653f0751535`
  (fingerprint `199af14145fa9d8404b3345123e7c5dd09b687ec102af7263d14425c66f22261`);
  dispositions manifest `ratchets/h2-5h-a-dispositions.v1.json`
  sha256 `283374764772771ba9f5d615393a1421b37fd913bd662df8eafb43296c3643c5`
  (fingerprint `dd4658127e0f6f01071e732f356d5511e6fc186d5de831630bd73bd26c4d38fb`).
- **Activation state:** before — gap-matrix row 12
  `tagged-template-lowering` is `missing` (asserted absence:
  `crates/emitter/src/builtins/tagged_template.rs`), counts
  12 exists / 0 partial / 1 missing; the joint pass dormant; the
  admission boundary is the typed rejection
  "older targets belong to later target-ladder slices"
  (builtins.rs:148-153, the `pass-registration-boundary` anchor).
  After — row 12 `exists` with the module anchors and the absence
  retired, counts 13 / 0 / 0; the `pass-registration-boundary` row
  re-anchors on the live registration; the joint pass REGISTERED at
  `target < ES2015`; the corpus ratchet byte-identical
  (T0=100.0000% 49024/49024 FP=0 unchanged — §12.2 measured
  inertness argument).
- **Next owner:** H2.5h corpus adoption (the `h2-5h-qualification`
  ts-tests sweep over the target-ES5 band, its Rust acceptance suite,
  and the hosted-runner policy update per the h2-5h-a conditional
  clause), followed by the H2.5h-b closure concerns recorded in the
  B-2/B-4 packets (es2018 shared-family re-basing, the es2018
  LiftRestriction consumer).

## 2. Position in the ratified ladder

The B-1 design pass ratified the decomposition
([B-1 packet](h2-5h-b-b-1.md) §2); this packet is its final row

> **B-5** | runtime | tagged-template lowering + the joint
> registration flip (`languageVersion < ES2015` →
> `[transformES2015, transformGenerators]`) + the 32-case witness
> fixture gate (the CS-6 analog: frozen bytes are the entire
> expectation) + requalification | ALL nine families end-to-end,
> 32/32 byte-equal

and revisits neither the granularity nor the ordering. The witness
families qualify END-TO-END here: all nine families — 10 positive,
9 adjacent-negative, 7 composition, 6 fault cases — drive the real
production stack (checker resolver, not the B-4 mini-binder) through
the real registered pipeline, and the composition edges
(`pass-order`, `yield-star-synthesis`, `substitution-chain`,
`destructuring-shared-module`, `tagged-template-shared-module`) are
all live in the driven configuration. B-4's focused projections stay
green unchanged (they construct the transformers directly and are
flip-inert).

## 3. Required-reference table

| Row | Lifecycle before → after | Role here |
|---|---|---|
| `E-ORDER-H` / `EA-GAP-COMPOSITION` | `substrate-landed` (registration flip named as B-5) → `active`; disposition `activate` unchanged, rationale gains the B-5 landing | the registration flip IS this packet: the pinned order `[transformES2015, transformGenerators]` becomes the live pipeline tail before the module transformer; the hook chain (previous-first substitution, ES2015-only notification) runs in production |
| gap row `pass-registration-boundary` | anchors the typed rejection string → re-anchors the live registration | the "older targets belong to later target-ladder slices" detail string is deleted from builtins.rs; the row's anchors become the registration symbols (`transform_es2015(options, resolver)` in builtins.rs) |
| gap row 12 `tagged-template-lowering` | `missing` → `exists` | the capability this packet lands; the absence (`tagged_template.rs`) retires; counts 12/0/1 → 13/0/0 (schema summary consts change with the generator — the B-2 lesson) |
| `E-HELPERS-BASE` / `E-HELPERS-H` | four texts landed (B-1) → five | `helpers::make_template_object()` joins byte-pinned (`typescript:makeTemplateObject`, unscoped, priority 0, span §4.3); `EmitHelperName::MakeTemplateObject` joins factory.rs; the byte-parity PINNED table gains its row |
| `E-NAMES-H` | `substrate-landed`; "empirical closure rides the B-5 byte gate" → closed by this gate | the external-module branch allocates the `templateObject` unique name through the eager `allocate_numbered_binding` arm (upstream `createUniqueName("templateObject")`); the witness/projection bytes are the named verifier of the B-1 three-pillar equivalence argument |
| `E-RESOLVER-CAPTURE-H` / `E-CHECKER-FACTS-H` | `activate`; B-1 landed the checker bridge → first PRODUCTION wiring | the registered pipeline consumes the real checker `EmitResolver` (crates/checker/src/emit.rs) — the 32-case gate is the end-to-end verifier the B-4 packet named (§12.2 there) |
| `EA-GAP-CAPTURE` / `EA-GAP-FLAGS` | `activate`; B-4/B-1 landed corpus-inert → live at `target < ES2015` | the capture model and the postorder classifier run in production for the new band; no code change here beyond registration |
| `E-COMMENT-SCOPE-H` / `E-COMMENTS-H` | `active-qualified` (CS-6) → unchanged premises | no comment-scope threading change, no printer edit |
| `E-ARENA`, `E-CONTEXT` | `active-qualified` premises | unchanged |
| activity canary | `h2_5g_profile` (22 slices) → `h2_5h_profile` (23) | `H2_5h` bit admitted; observation at `target < ES2015`; the activity unit suite gains the 23-slice row mirroring the 5g test |
| runtime-input closure | 247 files → 249 | `h2-5g-profile.mjs` RUNTIME_INPUTS gains `builtins/tagged_template.rs` + the witness-contract test file; `runtimeInputSet.size` const and the schema `minItems`/`maxItems` move together (the B-4 245→247 precedent) |

Lifecycle values transcribed from the dispositions manifest and
architecture map at the trusted base; the §8 amendments re-mint the
affected artifacts through this packet's own gate.

## 4. Pinned upstream map

All spans are 1-indexed inclusive lines of
`vendor/typescript-6.0.3/lib/_tsc.js`; every hash is the ledger d2
line-slice sha256 (newlines included, final line's newline included)
and lands verbatim in the module's `tsc-hash` headers, verified by
`cargo xtask ledger check`. The owner graph additionally pins the two
module-family functions under absolute byte offsets
(`processTaggedTemplateExpression` 4585992-4587851 sha256
`5d93b32a693e1b69b0010dfd39db437e5e146b620fdde8c76b9f32240b5f5650`;
`createTemplateCooked` 4587852-4588036 sha256
`e36fe1b71887855b8bcfba7d12d620c2f87b11b0ccf86228d2290c69677e6e68`);
both recipes were re-verified against the vendored file at authoring
(zero mismatches).

### 4.1 The shared module (owner-graph `shared_modules[1]`, `tagged-template`)

| Fn | Span | d2 line-slice sha256 |
|---|---|---|
| processTaggedTemplateExpression | 93972-94018 | d318d2539195d77c458bac08f12a8adfd7b03a2c933876e9f27df4bc4782446d |
| createTemplateCooked | 94019-94021 | 1f8f38eeb9dc74ce5fa36ea4158a351d9274f829b792c388a5a955c1c8253090 |
| getRawLiteral | 94022-94033 | ed2b608e1bc5d71e6dbd771ebae0d3b917f9fe54d4c114a2520a15de62c6a854 |

Behavior pins (read from the vendored bytes at authoring):

- `processTaggedTemplateExpression(context, node, visitor,
  currentSourceFile, recordTaggedTemplateString, level)`: visits the
  tag; at `ProcessLevel.LiftRestriction` (0) with NO invalid escape it
  returns `visitEachChild` (identity — the es2018 lane, out of scope
  here); otherwise it builds the cooked and raw string arrays —
  no-substitution literal directly, else head + each span literal with
  the span expressions visited into the argument list (index 0
  reserved) — requests the helper call
  `createTemplateObjectHelper(cookedArray, rawArray)`, and for an
  EXTERNAL MODULE source allocates `createUniqueName("templateObject")`,
  records it via the owner callback, and emits
  `tempVar || (tempVar = helperCall)` as argument 0; a non-module
  source inlines the helper call. Returns
  `createCallExpression(tag, undefined, templateArguments)`.
- `createTemplateCooked`: `templateFlags & 26656 (IsInvalid)` →
  `createVoidZero()`, else `createStringLiteral(template.text)`.
- `getRawLiteral`: `node.rawText`, else (asserting a current source
  file) the source slice between the delimiters
  (`text.substring(1, text.length - (isLast ? 1 : 2))`); then
  `text.replace(/\r\n?/g, "\n")`; `createStringLiteral` +
  `setTextRange(node)`.

### 4.2 The helper and its factory row

| Item | Span | d2 line-slice sha256 |
|---|---|---|
| templateObjectHelper declaration | 26247-26257 | 95a23beee7acf99b5f61e7ee0971c9783339404894a17b279498f19421a9609f |
| createTemplateObjectHelper | 25861-25869 | 270715e6924c8655b32e871c56808bfea6e6230a85a9e66ee9a447828277a5a9 |

`templateObjectHelper`: name `typescript:makeTemplateObject`,
importName `__makeTemplateObject`, scoped false, **priority 0** (the
second priority-0 helper next to `typescript:extends`).
`createTemplateObjectHelper(cooked, raw)` requests the helper and
returns `__makeTemplateObject(cooked, raw)` via
`getUnscopedHelperName`.

### 4.3 Owner-side already-ported spans consumed here

| Fn | Span | Standing |
|---|---|---|
| visitTaggedTemplateExpression | 107927-107936 | ported B-4 as the typed fail-closed seam (ledger header already carries the span/hash); B-5 replaces the seam body with the module call at `ProcessLevel::All` — the header is UNCHANGED |
| recordTaggedTemplateString | 104759-104764 | in the owner's 171 pinned functions; the recording (append a `createVariableDeclaration(temp)` to `taggedTemplateStringDeclarations`) lands here as the owner callback feeding the B-4 vec |
| the source-file declarations tail | inside visitSourceFile 104982-105011 | landed B-4 (es2015.rs:2424-2429), dormant; becomes reachable |

### 4.4 The registration site (owner-graph `upstream_registration`)

`_tsc.js:115942-115945`, line-slice sha256
`6db9973ea5fb29630a40f11c89edc9fdef63673b6df3c3e54defe87e1512e192`,
byte-range sha256 (offsets 5446690-5446817)
`f13bde7bd85c8fdc67b85ccc4dd8d86809cc3e7719518417e3d20b3ba7794066`
(matches the frozen owner-graph pin):

```js
if (languageVersion < 2 /* ES2015 */) {
  transformers.push(transformES2015);
  transformers.push(transformGenerators);
}
```

immediately after the `languageVersion < 3 /* ES2016 */` push and
immediately before `transformers.push(getModuleTransformer(moduleKind))`.

## 5. Rust design

| Seam | Design |
|---|---|
| module | `crates/emitter/src/builtins/tagged_template.rs`: `pub(super) enum ProcessLevel { LiftRestriction, All }` (the upstream const enum; the es2018 arm is representable from day one, unwired); `pub(super) fn process_tagged_template_expression(host: &mut impl TaggedTemplateHost, node: TransformNode, level: ProcessLevel) -> Result<TransformNode, TransformError>` — the host trait carries the owner-provided operations the upstream signature threads as arguments (visit an expression with the active visitor, the current source text for raw fallback + external-module answer, `record_tagged_template_string`, the helper-call constructor); `create_template_cooked` and `get_raw_literal` are module-internal fns with their §4.1 ledger headers. Exact host shape is finalized at implementation within this fence; a plain fn set taking `&mut Es2015Visitor` is admissible if the trait indirection proves needless — the module stays the single lowering owner either way |
| invalid-escape predicate | `template_cooked_is_invalid(raw_text: &str) -> bool` — module-internal recomputation of `templateFlags & IsInvalid` for TEMPLATE literals from the raw bytes (the only invalidity source for templates is `ContainsInvalidEscape`; octal/leading-zero/separator are numeric-literal flags). The parse records deliberately do not persist templateFlags (nodes.rs is generated; the B-4 classifier disposition builtins.rs:13892-13896 already pins "B-5's tagged-template module reads rawText for it"). Equivalence: the flag is a pure function of the raw fragment bytes (scanner determinism), and the untagged parse path REJECTS invalid escapes at parse time (parser.rs:7116 re-scan branch), so only tagged-position fragments ever reach the predicate. §7 focused projections carry invalid-escape fixtures whose oracle bytes are the enforcement |
| es2015.rs consumer | `visit_tagged_template_expression` body: `tagged_template::process_tagged_template_expression(self, node, ProcessLevel::All)`; the recording callback appends `createVariableDeclaration(temp)` nodes to the landed `tagged_template_string_declarations` vec (B-4 tail es2015.rs:2424-2429 emits them); the external-module test is `syntax().external_module_indicator.is_some()` on the source record; the temp name comes from the eager `allocate_numbered_binding("templateObject")` arm (upstream `createUniqueName` non-optimistic semantics — always suffixed, `templateObject_1`) |
| helper | `helpers.rs`: `MAKE_TEMPLATE_OBJECT_HELPER_TEXT` (dedented printed form, byte-identical to the witness prelude bytes) + `pub(super) fn make_template_object() -> EmitHelper` (`"typescript:makeTemplateObject"`, unscoped, `Some(0)`, no deps); `factory.rs`: `EmitHelperName::MakeTemplateObject` → `"__makeTemplateObject"`; the byte-parity PINNED table (tests/unit/helpers/tests.rs) gains the row (span 26_247-26_257, priority Some(0)) |
| registration | builtins.rs `get_script_transformers_with_optional_host`: floor arm becomes `target > ScriptTarget::ES_NEXT` OR `target < ScriptTarget::ES5` → typed rejection with detail "H2.5h admits ES5 through ESNext" (ES3 is unreachable — `emit_script_target()` maps it to ES2025 per options.rs:428-434 — the guard is defensive); `if target < ScriptTarget::ES2015 { activity.observe_runtime_slice(H2RuntimeSlice::H2_5h); }` joins the host-block ladder after the H2_5g arm; constructors `let transform_es2015 = (target < ScriptTarget::ES2015).then(\|\| es2015::transform_es2015(options, resolver));` and `let transform_generators = (target < ScriptTarget::ES2015).then(\|\| generators::transform_generators(target, resolver));` evaluated in list order; pushes land between the es2016 push and the module transformer push (upstream order §4.4); the two `#[allow(dead_code)] // the production registration arrives with the B-5 owner` attributes on the constructors (es2015.rs:287, generators.rs:223) retire |
| activity | activity.rs: `pub const fn h2_5h_profile() -> Self` (5g profile + `H2_5h` bit) with the ladder doc comment; callers builtins.rs:107, builtins.rs:122, execute.rs:271, compiler/src/lib.rs:765 flip to `h2_5h_profile()`; tests/unit/activity/tests.rs gains `h2_5h_profile_admits_only_the_twenty_three_completed_runtime_slices` mirroring the 5g row |
| witness gate | `crates/compiler/tests/integration/es2015_generators_witness_contract.rs` (registered in the integration harness next to `h1_memory_emit_oracle_contract.rs`): includes the witness artifact bytes; for each of the 32 cases builds a `MemoryCompilerHost` at `/project` with the case files plus the COMPLETE vendored `lib.*.d.ts` inventory read from disk (`CARGO_MANIFEST_DIR/../../vendor/typescript-6.0.3/lib`, the artifact's `lib` inventory record is the census), `LibraryCatalog::typescript_6_0_3` against that directory, maps the stored `compiler_options` to typed `CompilerOptions` FAIL-CLOSED (exactly the ten stored keys — target, module, alwaysStrict, downlevelIteration, importHelpers, noEmitHelpers, newLine, useDefineForClassFields, useUnknownInCatchVariables, ignoreDeprecations — plus the per-case overrides already merged by the generator; an unexpected key panics, the CS-6 lesson), `load_program` → `PreparedProgram`, clones it, and runs TWO owned `ProgramSession::emit_with_reported_diagnostics_for_harness(&mut MemoryOutputSink)` sessions asserting: determinism across the pair; reported diagnostic codes == `expected_reported_codes` (sequence); `emit_skipped` == observation; write paths and BYTES == the frozen `observation.writes` (base64-decoded); marker occurrences re-counted on the emitted text == `observation.marker_occurrences`. A `KNOWN_DIVERGENCES: [&str; 0]` shrink-only list mirrors the CS-6 contract mechanics for first-run surfacing |
| corpus inertness | measured at authoring (§12.2): `cargo xtask conformance` runs the NO-EMIT driver path (`run_for_conformance_harness`); every acceptance-suite population pins targets ≥ ES2015; zero `required_slices: H2.5h` deferral rows exist across h2-2c/4a/4b/5a..5g qualification artifacts (grep census 0/0/0/0/0/0); no owner control or unit contract asserts the ES5 typed rejection (repo-wide grep: the detail string appears only in builtins.rs itself, the gap-matrix generator/artifact anchor, and prose). The flip therefore changes NO in-gate emit observation outside the new witness gate |

## 6. Gap-matrix delta

Row `pass-registration-boundary`: state stays `exists`; the anchor
`{path: builtins.rs, symbol: "older targets belong to later
target-ladder slices"}` is REPLACED by live-registration anchors
(`transform_es2015(options, resolver)` and
`transform_generators(target, resolver)` in builtins.rs — real
symbols, the F1 lesson); the note records the B-5 activation.
Row 12 `tagged-template-lowering`: `missing` → `exists`; anchors
gain `crates/emitter/src/builtins/tagged_template.rs`
`process_tagged_template_expression` / `create_template_cooked` and
`crates/emitter/src/builtins/helpers.rs` `make_template_object`; the
`tagged_template.rs` absence retires. Summary counts 12/0/1 →
13/0/0; the schema summary consts move in the same change.

## 7. Implementation plan (dependency order)

1. **Design-gate pass** (this document + envelope `h2-5h-b-b-5`
   status `ready` + bootstrap `allowedPacketIds += h2-5h-b-b-5` +
   index row); `slice-readiness --check` green; commit; push -u;
   PR opens EARLY (endgame-batching directive).
2. **Helper + factory row.** `MAKE_TEMPLATE_OBJECT_HELPER_TEXT`,
   `helpers::make_template_object()`, `EmitHelperName::MakeTemplateObject`,
   byte-parity PINNED row. Targeted test:
   `cargo test -p tsc-rs-emitter helpers`.
3. **The module.** `tagged_template.rs` with the three §4.1 ports +
   `template_cooked_is_invalid` + `ProcessLevel`; ledger headers.
   Targeted test: `cargo xtask ledger check` + the module's unit
   rows (cooked-invalid table, raw normalization, raw fallback).
4. **The es2015.rs consumer.** Seam body replacement + recording
   callback + external-module answer + `templateObject` allocation.
   Focused projections extend the B-4 suite style (same
   `b4-probe.mjs` host recipe, ES5, per-case options): tagged
   no-substitution / multi-span / invalid-escape script + external
   module (temp var + declarations tail + `\r\n` raw normalization)
   — oracle bytes are the entire expectation. Targeted test: the
   extended focused suite.
5. **The flip.** activity.rs profile + four callers + floor +
   observation + registration pushes + dead-code attr retirement +
   activity unit row. Targeted test:
   `cargo test -p tsc-rs-emitter activity` + the focused suite
   (still green — direct construction) + a pipeline-level unit
   asserting the ES5 list shape ([typescript, class-fields, es2021,
   es2020, es2019, es2018, es2017, es2016, ES2015, GENERATORS,
   module]) through `get_script_transformers_for_source`.
6. **The witness gate.** The §5 contract; all 32/32 green (a first
   red case is a production fix under the frozen bytes, CS-6 §6
   precedent). Targeted test: `cargo test -p tsc-rs-compiler
   es2015_generators_witness`.
7. **Train items.** §8 amendments; chain walk (`b5-walk.sh` = the
   B-4 walk with this scratchpad's path; qualification BEFORE
   profile; walk BEFORE `--lane rust`; ONE `cargo fmt` +
   `cargo clippy --tests` pass BEFORE the first walk — the B-3
   lesson; runtime-input closure 247→249 in h2-5g-profile.mjs +
   schema minItems/maxItems); pin-sweep audit; full local gate at
   the final head from the canonical repository path (detached
   launcher; demoted; perf-only-red → normal-priority resume).

## 8. Evidence, ratchet, and documentation amendments

1. **Gap matrix generator + schema** per §6 (both files together).
2. **Dispositions generator**: rationale rows for `E-ORDER-H`,
   `EA-GAP-COMPOSITION`, `E-NAMES-H`, `E-RESOLVER-CAPTURE-H`,
   `E-CHECKER-FACTS-H`, `E-HELPERS-H`, `EA-GAP-CAPTURE`,
   `EA-GAP-FLAGS` gain the B-5 landing clause (registration live,
   32/32 end-to-end, fifth helper); disposition VALUES unchanged;
   manifest re-mints with the new gap-matrix lineage.
3. **Architecture map**: `E-ORDER-H` row — registration flip landed
   (B-5, this train's commit); `E-HELPERS-H` — five texts;
   `E-NAMES-H` — empirical closure delivered by the 32-case gate;
   `EA-GAP-COMPOSITION` — tagged-template edge closed; the
   builtins.rs:147-150 dormant-seam citation updates to the live
   registration. NO heading or table-row identity changes.
4. **Handoff** `h2-5h-a.md`: the ladder's B-5 bullet gains its
   **LANDED** marker ⇒ envelope `h2-5h-a` re-pin + doc-pinning
   witness re-mints (adoption: seconds).
5. **Chain walk**: b5-walk.sh (b4-walk.sh verbatim, scratchpad path
   updated); the runtime-input closure (247→249: tagged_template.rs
   + the witness-contract file, mjs list + schema minItems/maxItems
   + `runtimeInputSet.size` const); the walk re-mints
   foundation/comment-scope witnesses (adoption), owner-graph,
   gap-matrix, dispositions, es2015-generators witnesses, 5g
   qualification/profile; pin-sweep audit before the gate.
6. **Readiness**: envelope `ratchets/fci-readiness/h2-5h-b-b-5.v1.json`
   (`ready`; predecessor `h2-5h-b-b-4` receiptSha256
   `b0a405f86712a02d86882c1b39c1952e03f46243e7d748c29abbc9407110dca0`),
   bootstrap `allowedPacketIds += h2-5h-b-b-5`, index row in
   `slices/README.md`.

## 9. Acceptance

- Module + helper landed with ledger headers;
  `cargo xtask ledger check` green (stale=0, undispositioned=0,
  todo_port=0).
- Focused suite green: B-4's 123 projections unchanged + the new
  tagged-template projections byte-equal to oracle.
- The registered ES5 pipeline constructs the §7.5 list shape; the
  activity canary admits exactly 23 slices; no unadmitted-slice
  panic anywhere in the gate.
- Witness gate 32/32: bytes, codes, emit_skipped, markers,
  determinism ×2 — all nine families end-to-end.
- `cargo test -p tsc-rs-emitter` and `-p tsc-rs-compiler` fully
  green; zero expected-string changes outside the new suites.
- Gap matrix re-minted 13/0/0; dispositions/owner-graph/witnesses
  re-minted through the walk; architecture map + handoff amended;
  h2-5h-a envelope re-pinned.
- Corpus ratchet: T0=100.0000% 49024/49024 FP=0, all bands, tiers —
  byte-identical (§12.2 inertness argument; the ratchet is the
  enforcement).
- Packet checker `slice-readiness --check h2-5h-b-b-5`; complete
  local gate green at the final head from the canonical path.

## 10. Traceability

| Invariant | Target | Test | Evidence |
|---|---|---|---|
| module fidelity (3 fns) | tagged_template.rs ports | ledger d2 headers + unit rows | §4.1 spans/hashes + owner-graph byte hashes |
| helper byte identity | make_template_object text | byte-parity PINNED row | §4.2 span; witness prelude bytes |
| cooked-invalid recomputation | template_cooked_is_invalid | invalid-escape projections (oracle bytes) | §5 predicate argument + parser.rs:7116 rejection pin |
| external-module temp protocol | recording + tail + `templateObject_1` | module-mode projections + witness bytes | §4.3 recordTaggedTemplateString pin; E-NAMES-H argument |
| registration order | builtins.rs pushes | pipeline-shape unit + witness composition cases | §4.4 registration sha (owner graph) |
| admission floor | ES5 through ESNext | floor unit contracts + witness gate (target 1) | options.rs ES3 mapping pin |
| activity admission | h2_5h_profile | 23-slice activity row + in-gate canary | activity.rs ladder |
| end-to-end families | all nine witness families | the 32-case gate ×2 | frozen witness artifact (fingerprint §1) |
| corpus inertness | zero output change ≥ ES2015 | full ratchet suite | §12.2 measured blast radius |

Resources: the walk and gate follow the standing demotion directive
(`taskpolicy -b nice -n 15`, maintenance QoS for walk re-mints), with
the perf-ceiling normal-priority resume exception.

## 11. Prohibitions

No flatten_destructuring.rs or resolver.rs edit; the generators.rs
edit is limited to the constructor's dead-code-attribute retirement;
transform.rs edits are limited to the §12.8.6 import-name table and
`requested_emit_helpers` accessor; es2018.rs/es2017.rs/es2021.rs/
es_next.rs/class_fields.rs/standard_decorators.rs edits are limited
to the initialize-floor moves of §12.8.1; the printer edits are
limited to the §12.8.4 pinned Indented arm and the §12.8.6
`importHelpers` suppression; the checker edit is limited to the
§12.8.5 Extends site; no witness amendment (a red witness case is a
production fix);
no corpus output-byte change for targets ≥ ES2015; no ts-tests
corpus adoption, hosted-runner, or qualification-policy change (the
next slice's scope); no parser/nodes schema change (the invalid
predicate recomputes from raw bytes); no generic fallback converting
an unknown branch into success (typed `TransformError` only); no
fixture-specific branches or hand-authored expected output (oracle
bytes only); the CS and B-1..B-4 prohibitions remain. This document
authorizes no production edit until its own design-gate pass and
envelope exist.

## 12. Unresolved items (all closed at authoring, 2026-08-23)

### 12.8 Mid-train amendments (first witness-gate run, 2026-08-23)

The first end-to-end run of the 32-case gate surfaced five dormant
`languageVersion < ES2015` arms the corpus-inert foundation packets had
never been able to reach; each is fixed in production under the frozen
bytes' authority (the CS-6 §6 precedent) and every edit is
witness-byte-verified:

1. **Per-transformer initialize floors** (es_next.rs:103,
   class_fields.rs:51, standard_decorators.rs:93, es2018.rs:251,
   es2017.rs:133, es2021.rs `lower_target`): the initialize-time band
   guards mirrored the pipeline floor and move ES2015 → ES5 with it.
2. **`validate_bootstrap_emit_options`** (execute.rs:73): the bootstrap
   emit validator carried its own ES2015 floor AND an `importHelpers`
   rejection row. The floor moves to ES5; `importHelpers` is accepted
   (measured corpus-inert: zero `importHelpers` occurrences across
   every qualification artifact — the option never reaches candidate
   status — and the checker/emitter lanes below carry the witness
   evidence).
3. **`transformTypeScript`'s `promoteToIIFE` arm**
   (`_tsc.js:94434-94548`, d2
   `b4f4c7bb3c8f14a7776dd0ab5337e8c11b30104d7eb70b676c5dba79a9e1ae59`):
   at `languageVersion < ES2015` a class declaration with static
   initialized properties (or decorator facts) is wrapped in the
   `TypeScriptClassWrapper` arrow IIFE consumed by the B-4-landed
   ES2015 wrapper surgery. Ported as
   `promote_class_declaration_to_iife` (builtins.rs) with the
   close-brace return protocol (`wrapping_add` reproduces the -1
   sentinel arithmetic); the exported / namespace-nested / decorated
   promote lanes are typed fail-closed seams owned by the H2.5h
   corpus-adoption slice (witness scope is the plain-script static
   lane). The same sentinel arithmetic fix lands in the ES2015 owner's
   `transformClassBody` tail (es2015.rs — synthesized members arrays
   reach it once class-fields precedes ES2015 in the live chain).
4. **The printer's function `Indented` arm**
   (`emitSignatureAndBody`, `_tsc.js:118969-118982`): the
   FunctionExpression emission brackets signature+body one level
   deeper under `EmitFlags::INDENTED`. The flag's function-node
   producer is exactly the ES2015 class lowering (inheriting
   class-fields' class-node stamp, `_tsc.js:105203`), so the arm is
   byte-inert for every target ≥ ES2015 (no function node carries the
   flag there; the ratchet is the enforcement). This is the second
   pinned Indented arm next to B-4's §12.11 object-literal arm.
5. **The checker's `Extends` external-helper site**
   (`_tsc.js:85014-85016`): `languageVersion <
   LanguageFeatureMinimumTarget.Classes` requests
   `checkExternalEmitHelpers(baseTypeNode.parent, Extends)`. The
   check machinery (tslib resolution, 2354/2343/2344 reporting,
   `__extends` name row) was already landed; only the call site and
   the `EMIT_HELPER_EXTENDS` constant were absent
   (crates/checker/src/{class.rs,modules.rs}). Measured T0-inert: the
   full-corpus conformance ratchet is the enforcement (an affected
   fixture would already be red today if one existed). The remaining
   19 unported `checkExternalEmitHelpers` sites of the ES5 band stay
   the pre-existing inherited deferral owned by the H2.5h
   corpus-adoption slice.

6. **The `importHelpers` tslib import lane**
   (`createExternalHelpersImportDeclarationIfNeeded`,
   `_tsc.js:27613-27680`, d2
   `d44b2c0d8237d7cad74d638bdaa5cd14dd4347e3a57e408de8b5fb85a6a18f77`):
   the ES-module named-import arm lands as
   `insert_external_helpers_import_declaration` (builtins.rs), called
   from the ECMAScript-module transformer for external modules under
   `importHelpers`; import names come from the transcribed unscoped
   helper table (`EmitHelper::import_name`, transform.rs — the five
   irregular `commonjs*`/`export-star` rows forbid any derived
   prefix rule), sorted case-sensitively after the prologue; the
   printer suppresses unscoped helper bodies for that configuration
   (`hasRecordedExternalHelpers` equivalence argument in the printer
   comment). The upstream aliasing arm (helper name not file-level
   unique) and the CommonJS-format import-equals arm are typed
   fail-closed seams owned by the H2.5h corpus-adoption slice.
   transform.rs additionally gains the read-only
   `requested_emit_helpers` accessor.

The envelope re-pins with this amendment (packet sha change);
`crates/checker` and `crates/emitter/src/printer.rs` join the
allowed surface accordingly (the B-1 precedent for dropping
`crates/checker` from forbiddenPrefixes).

1. ~~Trusted base + authority hashes~~ — pinned in §1 at
   `7308f9fcf4320b4e81ac4858264161d9dfadcc91`.
2. ~~Corpus blast radius of the flip~~ — MEASURED: (a) the gating
   conformance harness runs the no-emit driver
   (`run_for_conformance_harness`, compiler/src/lib.rs:904) — no
   transformer list is constructed; (b) every emit-acceptance
   population (h1, h2-2c, 4a/4b, 5a..5g) pins targets ≥ ES2015 and
   contains ZERO `required_slices: H2.5h` rows (artifact grep census
   at the trusted base: 0 across all six files); (c) the only
   repo-wide references to the ES5 rejection are builtins.rs itself,
   the gap-matrix anchor (re-anchored here), and prose; (d) B-4's
   focused suite constructs transformers directly. Therefore the
   only NEW in-gate emit observations are the witness gate's own 32
   cases, and the corpus ratchet must stay byte-identical — enforced
   by the full gate at the train head.
3. ~~Invalid-escape representation~~ — RESOLVED per §5: recomputed
   from raw fragment bytes inside the module; nodes.rs is generated
   and stays untouched; the B-4 classifier comment
   (builtins.rs:13892-13896) pre-ratified this route; the untagged
   parse path rejects invalid escapes (parser.rs:7116), so the
   predicate only ever faces tagged-position fragments; enforcement
   = invalid-escape oracle projections (§7.4).
4. ~~Witness-gate diagnostics semantics~~ — RESOLVED: the artifact's
   `reported_diagnostics` were observed through the oracle's
   `emitFilesAndReportErrors` wrapper; the Rust analog is
   `emit_with_reported_diagnostics_for_harness` (the qualification
   projection used by every H2 acceptance suite); the gate asserts
   the reported CODE sequence against `expected_reported_codes`
   (fault cases 2802/2304/2548/2349/2354/1100+2496 — emit proceeds,
   `emit_skipped: false` per the frozen observations). Default-lib
   resolution: the oracle used plain `ts.createCompilerHost` disk
   resolution at target ES5 → `lib.d.ts` (probed at authoring:
   `getDefaultLibFileName` = lib.d.ts, composition-case diagnostics
   []); the Rust gate preloads the complete vendored lib directory
   and lets `LibraryCatalog::typescript_6_0_3` resolve identically.
5. ~~es2018 LiftRestriction lane~~ — out of scope with a named
   owner (§1 non-goals, §12.6-style deferral): upstream lowers
   invalid-escape tagged templates at the ES2018 stage
   (`_tsc.js:102047-102056`); our parse records cannot carry the
   ES2018 facet (B-4 classifier disposition) and es2018.rs is
   byte-frozen `active-qualified`. At `target < ES2015` BOTH stages
   run and level All lowers every tagged template at the ES2015
   stage; the §7.4 invalid-escape projections byte-compare the
   final output against the oracle — any stage-order divergence
   surfaces there and is resolved in-packet under oracle authority.
   At `ES2015 ≤ target < ES2018` the lane remains the inherited
   pre-existing gap (unchanged by this packet), owned by the
   H2.5h-b-closure es2018 requalification concern recorded in the
   B-2 packet §12.3 lineage.
6. ~~templateObject name equivalence~~ — RESOLVED: upstream
   `createUniqueName("templateObject")` (non-optimistic, always
   suffixed) ↔ the eager `allocate_numbered_binding("templateObject")`
   arm; the B-1 E-NAMES-H three-pillar argument covers the arm and
   the witness/projection bytes are the named verifier (`templateObject_1`
   in module-mode oracle output).
7. ~~Gate placement~~ — RESOLVED: the witness gate lives in
   `crates/compiler/tests/integration/` (the h1 memory-emit oracle
   contract's home and mechanics), running under the workspace test
   phase of every full gate; the envelope's forbiddenPrefixes drop
   `crates/compiler` accordingly (the B-1 precedent for dropping
   `crates/checker`).

## 13. Citation status

Every file:line, span, hash, and count in this document was read or
recomputed from the trusted base
`7308f9fcf4320b4e81ac4858264161d9dfadcc91` at authoring (2026-08-23):
the owner-graph byte hashes (2/2 match), the four §4 line-slice
hashes, the registration slice bytes, the §5 anchors
(builtins.rs:107/122/147-153, es2015.rs:287/904/2408-2429/2808-2816,
generators.rs:223-233, activity.rs profile ladder, execute.rs:271,
compiler/src/lib.rs:765/904, options.rs:428-434, parser.rs:7116,
helpers byte-parity PINNED table, h1_memory_emit_oracle_contract.rs
mechanics, library_loader_session_contract.rs catalog mechanics), the
witness artifact's 32 case roles/options/codes, and the §12.2
blast-radius greps. Unresolved: 0.
