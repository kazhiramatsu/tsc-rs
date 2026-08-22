# H2.5h-b / B-4 — the ES2015 visitors: transformES2015 as a dormant foundation module

Design-gate packet for the FOURTH H2.5h-b implementation packet, under
the mandatory implementation-ready design gate
(../post-h1-completion-slices.md). Authored at the train start
(2026-08-22) on `h2/5h-b-b4` from the post-B-3 trunk; reviewed
(independent full-dimension pass: all 171/171 §4.1 d2 hashes and
44/44 §4.2 table hashes recomputed, the owner read in full against
every §4.3 behavior pin, both yield* witness offsets recomputed, all
§5 Rust anchors opened, all 12 named facts pairs verified at their
call sites, the 116-at-review fixture sources mechanically scanned
for pipeline-identity violations — verdict READY-WITH-FIXES, all
eleven findings folded in: the Identifier extended-unicode facet arm,
the `createMethodCall` span, the catch-destructured-element and
assignment-target checker pins, the full `getName` protocol with its
generated-name fallback, the extends-clause `CapturesThis` predicate,
the context-API count, seven review-driven fixtures closing the
uncovered projection lanes (123 total), the three ad-hoc facts pairs,
the lib-global resolver boundary, and the attribution/source-file-gate
wording). The design-gate
pass lands with the trusted base, envelope, bootstrap, and index in
one commit before any production edit. Machine check:
`node .github/ci/slice-readiness.mjs --check h2-5h-b-b-4`.

## 1. Identity, purpose, and boundary

- **Slice ID:** `h2-5h-b-b-4`. **Kind:** `foundation` — a corpus-inert
  substrate packet, the fourth rung of the ratified B-ladder
  ([B-1 packet](h2-5h-b-b-1.md) §2). It lands the `transformES2015`
  owner frozen in the owner graph — the complete ES2015 visitor set
  (class lowering lanes, captured `this`/`arguments`/`new.target`,
  parameters, block-scoped bindings, iteration/loop conversion WITH
  the two pinned `yield*` synthesis sites, spread, template
  expressions, object-literal chunking, for-of in both array and
  iterable modes; `_tsc.js:104740-108100`, 171 pinned local
  functions) — as the new dormant module
  `crates/emitter/src/builtins/es2015.rs` exposing a real
  `Transformer` (`Es2015Transformer`), NOT registered in any
  pipeline. Producer side of the pinned `yield-star-synthesis`
  composition edge: B-3's machine (consumer) is already landed, so
  this packet's focused suite drives the REAL
  `[transform_es2015, transform_generators]` joint projection — the
  exact upstream registration order — and the two synthesis sites are
  exercised end-to-end against oracle bytes inside this packet.
- **Non-goals:** transformer registration or activation (the dormant
  seam `crates/emitter/src/builtins.rs:147-150` "older targets belong
  to later target-ladder slices" is preserved verbatim);
  tagged-template lowering (the `tagged-template` shared module —
  `processTaggedTemplateExpression`/`createTemplateCooked` — is B-5
  scope; the owner's `visitTaggedTemplateExpression` lands as a typed
  fail-closed seam and gap row 12 stays `missing`); the
  `__makeTemplateObject` helper text (B-5, with the module); any edit
  to the active es2017/es2018 lowerings or to `generators.rs`;
  witness-set amendment; any corpus output-byte change.
- **Prerequisites:** B-1 (helper texts incl. `typescript:extends`
  priority 0, `typescript:values`, `typescript:spreadArray`; the
  six-query resolver surface; eager name generation incl. the dormant
  `allocate_loop_variable` and `allocate_source_numbered_for_node`;
  EA-GAP-FLAGS classifier; hook chaining with the pinned
  "ES2015 chains BOTH hooks / Generators substitution-only" order
  contracts) merged @02f784d9; B-2 (destructuring flattener with the
  `FlattenHost` trait whose doc names `Es2015Visitor` as the intended
  production host) merged @28f04d95; B-3 (Generators state machine,
  the `EmitFlags::ITERATOR` consumer skip at generators.rs:2338-2352,
  the `REUSE_TEMP_VARIABLE_SCOPE` finalize arm) merged @548200df.
- **Trusted base:** `548200dfb52a9ebc31c5d4f26085ec82a151658b` (main
  after the B-3 merge). Authority artifacts at that base:
  owner graph `ratchets/h2-5h-a-owner-graph.v1.json`
  sha256 `7b325f9102bef8c388b2cdcfc0b7d24c7e9b0538587be8149a29db451896b8a0`
  (fingerprint `7ebb950510680f10e0a3d7c6e19a726324944f6f56c8dbd534890fb58d029265`);
  gap matrix `ratchets/h2-5h-a-gap-matrix.v1.json`
  sha256 `8304ccb5d3d780f10192a3413876839a8b34d94aedd744a5d46d9d8c53cec2b6`
  (fingerprint `2e066e7c3beb8103a63b9d54bde55b269b52517fcd025f264dc0923b3e19e1d4`);
  witness artifact `ratchets/h2-5h-a-es2015-generators-witnesses.v1.json`
  sha256 `10886d16008f42b103f8053e8ba02801185fd5c1ec9e10d1f2e66863e368dad1`
  (fingerprint `00ddc32c4cce1f1bcdbc108fdfd6a002cd1478fedf1be19fe06393863163eb1a`);
  dispositions manifest `ratchets/h2-5h-a-dispositions.v1.json`
  sha256 `bdbd2e01e2abe6ea92c4a2a6a8d95318ea8a474d16f90d61ba6ec7a855b9be5f`
  (fingerprint `674d31efef0e4af728b19d27c0aa47427423b54943dd879c945333d1f1144316`).
- **Activation state:** before — gap-matrix row 9
  `loop-conversion-capture` is `missing` (asserted absence: symbol
  `converted_loop` in `crates/emitter/src/builtins.rs`), counts
  11 exists / 0 partial / 2 missing. After — row 9 `exists` with the
  module anchors and the absence retired, counts 12 / 0 / 1 (row 12
  `tagged-template-lowering` stays the sole `missing`); the joint
  pass STILL dormant; the corpus ratchet byte-identical
  (T0=100.0000% 49024/49024 FP=0 unchanged).
- **Next owner:** B-5 (tagged-template shared module, the joint
  `[transformES2015, transformGenerators]` registration at
  `languageVersion < ES2015`, the 32-case witness fixture gate,
  requalification — per the ratified ladder).

## 2. Position in the ratified ladder

The B-1 design pass ratified the decomposition
([B-1 packet](h2-5h-b-b-1.md) §2); this packet is its row

> **B-4** | foundation | the ES2015 visitor set (class lowering
> lanes, captured this/arguments/new.target, parameters, loop
> conversion WITH the two pinned `yield*` synthesis sites feeding
> B-3's machine) — dormant, driven by focused fixtures only

and revisits neither the granularity nor the ordering. The witness
families that exercise the owner (`loop-conversion-capture` incl.
both `yield*` sites, `class-lowering`, `destructuring`,
`name-generation`, `resolver-foundation`, `hook-chains`,
`helper-graph`) qualify END-TO-END at B-5; B-4's qualification
surface is the focused projections of §7, which drive the real
`Es2015Transformer` (jointly with the real landed
`GeneratorsTransformer` — upstream registration order) on parsed
fixtures directly. The `yield-star-synthesis` edge's producer
obligations (the `EmitFlags::ITERATOR` stamp on both synthesized
delegation calls, owner-relative offsets 101107/101796) land here
and are exercised by oracle byte projections through B-3's consumer.

## 3. Required-reference table

| Row | Lifecycle before → after | Role here |
|---|---|---|
| `EA-GAP-CAPTURE` / `E-CAPTURE-BASE` | gap row 9 `missing` → `exists` | the capability this packet retires: converted-loop extraction with captured block-scoped bindings, out-parameters, this/arguments/new.target capture, and `yield*` re-emission |
| `EA-GAP-COMPOSITION` | `activate` → unchanged disposition; rationale records the B-4 landing | ES2015 becomes the FIRST production `FlattenHost` (B-2's trait doc names `Es2015Visitor`); the tagged-template edge stays open for B-5 |
| `E-ORDER-H` | hooks half landed B-1; Generators dormant substitution landed B-3 → unchanged | the ES2015 side of the pinned chain lands DORMANT: `substitute_node` (identifier/this) AND the emit-notification pair (`before_emit_node`/`after_emit_node` — the upstream `onEmitNode` enter/exit decomposition, exactly the hook_chaining/tests.rs order contracts' "es2015" slot) |
| `E-HELPERS-BASE` / `E-HELPERS-H` | `active-*`/`substrate-landed` → unchanged texts | `helpers::extends()` (priority 0, helpers.rs:188), `helpers::values()` (:220), `helpers::spread_array()` (:204), `helpers::read()` (:178) are the four helper texts the owner requests; B-4 adds the missing `EmitHelperName::{Extends, SpreadArray}` identifier variants (factory.rs:19-45) — no helper-text change |
| `E-NAMES-BASE` / `E-NAMES-H` | `substrate-landed` → first production callers of the dormant arms | `allocate_loop_variable` (`_i`), `allocate_source_numbered_for_node` (`getGeneratedNameForNode`/`getLocalName`/`getInternalName` backing), file-level-optimistic `_this`/`_newTarget`/`_super`, numbered `_loop`/`_loop_init`/`out_*`/`state`/`inc`/`this`/`arguments`/`e`, temps; loop body functions carry `EmitFlags::REUSE_TEMP_VARIABLE_SCOPE` honored by the B-3 finalize arm (target_bindings.rs:581-586) |
| `EA-GAP-FLAGS` / `E-METADATA-BASE` | `activate` → unchanged disposition; the parsed-tree ES2015 facet completes | created nodes take factory-owned facets (`classify_created_node_flags`); the parsed-tree initializer gains the missing ES2015 facet arms (§5, §12.4 — corpus-inert: zero active readers of `CONTAINS_ES_2015`, measured; the ratchet is the enforcement) |
| `E-RESOLVER-CAPTURE-H` / `E-CHECKER-FACTS-H` | six-query surface landed (B-1) → first production consumer of five | the owner consumes `get_referenced_declaration_with_colliding_name`, `is_declaration_with_colliding_name`, `is_arguments_local_binding`, `is_binding_captured_by_node`, `has_node_check_flag` (resolver.rs:316-404); the §7 suite supplies the mini-binder `FixtureResolver` (§12.2) whose answers are verified by the oracle bytes themselves |
| `E-COMMENT-SCOPE-H` / `E-COMMENTS-H` | `active-qualified` (CS-6) → unchanged premises | synthetic leading `@class` comments (`add_leading_comment`), comment-range threading, `NoComments` families; no comment-scope threading change, no printer edit |
| `E-ARENA`, `E-CONTEXT` | `active-qualified` premises | `TransformArena`/`NodeFactory` construction; `TransformationContext::{hoist_variable_declaration, start/resume/suspend/end_lexical_environment, set_lexical_environment_flags, enable_substitution, enable_emit_notification, request_emit_helper, read_emit_helpers}` (transform.rs:472-667) |
| gap row 9 | `missing` → `exists` | §6 matrix |
| gap row 12 | `missing` → UNCHANGED | tagged-template stays B-5's flip; B-4 must not create `crates/emitter/src/builtins/tagged_template.rs` |

Lifecycle values transcribed from the dispositions manifest at the
trusted base; the §8 amendments re-mint the affected artifacts through
this packet's own gate.

## 4. Pinned upstream map

The upstream IS the frozen artifact chain plus the vendored slices
below. All spans are 1-indexed inclusive lines of
`vendor/typescript-6.0.3/lib/_tsc.js`; every hash is the ledger d2
line-slice sha256 (newlines included, final line's newline included)
and lands verbatim in the module's `tsc-hash` headers, verified by
`cargo xtask ledger check`. The owner graph pins the same 171 local
functions under the byte-offset recipe
(`sha256(bytes[start:end))` relative to the owner declaration offset
5010274); both recipes were re-verified against the vendored file at
authoring (all 171 byte hashes match the artifact; zero mismatches).

### 4.1 The owner (owner-graph `owners[0]`, `transformES2015`)

Declaration `_tsc.js:104740-108100`, declaration_sha256
`b59cbbc6c204139cf10d353626d1e1042545e96e3262e0d057bbf3b6efeb0097`,
body_sha256
`c7471e876573c7438920271f0f13943f4dc193fb465808a97be37e60402e0ea8`.
Hooks: `onEmitNode` AND `onSubstituteNode` (both chained
previous-first; the emit-notification hook is the enter/exit facts
wrapper). Factory methods consumed: 92; resolver methods: 5;
context APIs: 10 (+ the six-name context destructure); helper calls: 4
(`createExtendsHelper`, `createReadHelper`, `createSpreadArrayHelper`,
`createValuesHelper`); external utilities: 115; enum references: 167.

| # | function | lines | d2 line-slice sha256 |
|---|---|---|---|
| 1 | recordTaggedTemplateString | 104759-104764 | e44a8b5f8a01f5174faa2e9ba2d0e629b9175d698d9290e9a78ab4ca9e8126fc |
| 2 | transformSourceFile | 104768-104781 | 5259f401bfa4a730f0520aae96d9c7cdf636ce79f6ac7e515a4495d555c16e72 |
| 3 | enterSubtree | 104782-104786 | cf4c612dbd59e62b72d2c4c597760cb9fa8f73023a77cb2d466673d064852ee8 |
| 4 | exitSubtree | 104787-104789 | 4792b8f9d72b075de5dd753a51fda08df77e9fcec865c9ae5a06869ad6b1b5c6 |
| 5 | isReturnVoidStatementInConstructorWithCapturedSuper | 104790-104792 | f6c8c6170bc4f560ac839a8f039fb2171d7795a3b3f550150f68a0faabe0d3ce |
| 6 | isOrMayContainReturnCompletion | 104793-104799 | 512f73e1544830b532901dc633c1b58620ebe5934e65c05a4e9aa2a0f3fefa29 |
| 7 | shouldVisitNode | 104800-104806 | daaa7226e6ca8115617cc655f69f95e1523fc50596bb4cfef2f5e1c7c0e9c174 |
| 8 | visitor | 104807-104813 | 6c81599cbeb4c7d8a540cdad1ae6e36165af188b04160c0211a0385bf404702d |
| 9 | visitorWithUnusedExpressionResult | 104814-104820 | d96bb20713207bddd4bf48a0bd4d91aaa27a8c1fa596beaabbe7460b17ab4cc8 |
| 10 | classWrapperStatementVisitor | 104821-104844 | a194668e571877263b786ff647eee87cc430cebcd54f37ea5c72eec9c5819572 |
| 11 | callExpressionVisitor | 104845-104854 | 1cd164c791adeb5549edcc6ac319f12b1e66e87123ebf9a9ee2fc0213437807b |
| 12 | visitorWorker | 104855-104981 | 6c45f345b7b1911b12899874ce7b2816c9459e0e8bac37d9c1b44dd2176e19e0 |
| 13 | visitSourceFile | 104982-105011 | edf8f67d8fff1b6f3f66819bfeae6f9748e2f96c81c55763ef24917630ca5917 |
| 14 | visitSwitchStatement | 105012-105021 | 190046ed895022aa398d84951c951655ea5e774dec6b84c5ada2a4858b31bd35 |
| 15 | visitCaseBlock | 105022-105027 | c2af15bfefb2b24dfa120095ad3328610bd9de65e1eb3fd9d7c02048102e26a0 |
| 16 | returnCapturedThis | 105028-105030 | a63cb5a00ee2252a88554fd93bd82cf3c590e67baedfe430bf9ac9580820b681 |
| 17 | createCapturedThis | 105031-105033 | b34b092a136f191e4afe1cadca97a3d20b422f35611287f7fa7b53ffbd81c3fd |
| 18 | visitReturnStatement | 105034-105054 | 1a1668d953062e8ab8d142a5c8c81e784c5d24bc6084257524e4ea141e6f8cde |
| 19 | visitThisKeyword | 105055-105068 | 04811031550ee1ad94085e3b8fa9e441793913e58aeb63e40ed07a21739908a8 |
| 20 | visitVoidExpression | 105069-105071 | 54c16b398fbb5ab4b9a9ce27217d3ca320b10e59515e9f089288b3388d511605 |
| 21 | visitIdentifier | 105072-105088 | 3f9c650aa2a11cdb26e226e22fbc8255a36e911c4c9e4f62f02f5e338cafbfd8 |
| 22 | visitBreakOrContinueStatement | 105089-105143 | ef8efd157dd0f849a8cfed9fda4d5f6b4dc464c2134c2337740f188ef6749590 |
| 23 | visitClassDeclaration | 105144-105174 | b5eee1e707db4b5a7f0bd97240c5e9614cfd341992a4c5cddf52223940b0626e |
| 24 | visitClassExpression | 105175-105177 | 290a1ce981403ab42d9557d7235156214cfe66e5c69b388cf2f064038fdfc52d |
| 25 | transformClassLikeDeclarationToExpression | 105178-105220 | 73df944ac57dd513d2f9038860b0430ee225c3058023329eb4563f96d4fb7595 |
| 26 | transformClassBody | 105221-105249 | 9850847f0d39924ad08a7fd966626be67f88fa17b9f4d723c340dace16e67454 |
| 27 | addExtendsHelperIfNeeded | 105250-105262 | 8fd26c32abea947587ce196fdb828e453bf78c605355140b8f434934bc4add42 |
| 28 | addConstructor | 105263-105289 | 223c0cfd1cd8d9322b25846e9f09b62c35213006b109ad85ea6c48b9033bfb27 |
| 29 | transformConstructorParameters | 105290-105292 | 2a339b376b75500624eecbdcadc2462084b3812ff3ddfc1c09625b3734314dca |
| 30 | createDefaultConstructorBody | 105293-105310 | 184565cacd8247fee8f36a43308d1f2b8788a35b84a2b520ca592d0cf3bf8626 |
| 31 | isUninitializedVariableStatement | 105311-105313 | a6b59363464694f71b6e9d8a151f669545db7305f989027fc998cb662ca77cbb |
| 32 | containsSuperCall | 105314-105342 | b10611b4dfa5fff36d21c92b8a16a226fe46dab99869419e8955c2509a2054bc |
| 33 | transformConstructorBody | 105343-105388 | a6060ea9732eea48bca4d8e6f6fd30ad01335823010c924c6e4628b96bb40eb4 |
| 34 | isCapturedThis | 105389-105391 | 446a26fdc1a32915d56e5919798d2bf6cd2fd78b2a5f3a9c217c2f3270678e38 |
| 35 | isSyntheticSuper | 105392-105394 | 92959d1dd77f16b0e02b4b04f4b2b6790df457198d37b03354ee41f61e59369a |
| 36 | isThisCapturingVariableStatement | 105395-105397 | 1926c34da63ad10eaf11b3a8b8e2d7285ae469849f1e23cbd916d36e0c85fe59 |
| 37 | isThisCapturingVariableDeclaration | 105398-105400 | ecf7d7dd98a74760a84b2490f9e746a1d8b19dc1cb5c205f870f6d8be4a55f67 |
| 38 | isThisCapturingAssignment | 105401-105407 | 8388b581c5b25521a0aae4814000b34640a9585f39a2e35323133c1cba8af4af |
| 39 | isTransformedSuperCall | 105408-105410 | 4adfc61b1ef07a1452ad1b953204442d7ee858deab597ce22d989dbdeb59ad9b |
| 40 | isTransformedSuperCallWithFallback | 105411-105413 | 28e352d09edb6ccc4082e27215912f4301389e2c48b52f63a43e7f91ffef8e30 |
| 41 | isImplicitSuperCall | 105414-105416 | 7165b5d34503c679d3cc1fa518c253aaf72bbbd71e66ca77291f7428f96097bf |
| 42 | isImplicitSuperCallWithFallback | 105417-105419 | 839350266d2cdb9cae06bbfa934bffcd26c4e23e1dd791e68ea4ab3015a74237 |
| 43 | isThisCapturingTransformedSuperCallWithFallback | 105420-105422 | 30556f05d7ac691c850641ae7478c32c8352b76660804171630bd1c9b882e27b |
| 44 | isThisCapturingImplicitSuperCallWithFallback | 105423-105425 | fb16cec1a46b2a3c7b6c5232b6a3c2c6e4f9245fcd824d5f00b6d16574b32f61 |
| 45 | isTransformedSuperCallLike | 105426-105428 | 38ce63e4fe9b06319096dadf49028f44784bf4f86c20be6903b50bb2c10a2743 |
| 46 | simplifyConstructorInlineSuperInThisCaptureVariable | 105429-105485 | a415c9084e8e7e85649b875f311bb8b2c10e6b289d887898905d5d3822fc4cfc |
| 47 | simplifyConstructorInlineSuperReturn | 105486-105527 | 38195570d984762a2d12b39d3f6f6d6651f2326dbb2c0885af51b93a1d8816aa |
| 48 | elideUnusedThisCaptureWorker | 105528-105568 | 5e307dab74d11def5cc21f2825b3ec361acac6fea26c4a78eb9d5ed2a130f985 |
| 49 | simplifyConstructorElideUnusedThisCapture | 105569-105579 | ff8939432c9380ee9e1975ff915e7894e04f4ba09d55ad225ea5f06f08c613a6 |
| 50 | injectSuperPresenceCheckWorker | 105580-105621 | 1849d41cad4b43c942701794958dd9c116035005c5ca68b1ef6178102e793ab4 |
| 51 | complicateConstructorInjectSuperPresenceCheck | 105622-105624 | 82da36a56a8e525f23cbc73c6461c8eed9db102570bdb2b9e02587c66e2e0bee |
| 52 | simplifyConstructor | 105625-105636 | 1d6ecfb11a44aa571b4b00f6fe11fefcbc63a00fa817fa583120ef4405f8083e |
| 53 | isSufficientlyCoveredByReturnStatements | 105637-105652 | c9017db587b06db7a1da6ff4950bd80c58ab0aacd66accb25115affeffe7c942 |
| 54 | createActualThis | 105653-105655 | 2ffecdfd9c44035332056d9a3cee157f04a8ce5f3c032648d1086674044006e5 |
| 55 | createDefaultSuperCallOrThis | 105656-105671 | 72158ce104853a17c9cbca2ecd902d90ed1bce7609c1d1f028106e494c2dc7d2 |
| 56 | visitParameter | 105672-105722 | a1848862a06c3fe5ac65f77b69d47da37b30ebaa5a076ca0bb5d75f7c90d4e93 |
| 57 | hasDefaultValueOrBindingPattern | 105723-105725 | 04e54a9ade403c9673531a0a4216d80ec619cdf99caeecd6da49eb921076c8c9 |
| 58 | addDefaultValueAssignmentsIfNeeded2 | 105726-105744 | 482860970a15e88cd23c88baabd2ecc2ec15cb13fe81c1d3920405baba547550 |
| 59 | insertDefaultValueAssignmentForBindingPattern | 105745-105783 | 9ca1cca3cd788d1d5e68559aa6271b66235a7a203f811032168e86838c6f623f |
| 60 | insertDefaultValueAssignmentForInitializer | 105784-105814 | 79c81a915aedc84b09ad6cb2155bbb5c3b4c7a2c76c325cdcf18e913cd3756c7 |
| 61 | shouldAddRestParameter | 105815-105817 | d8c077d222c6225fda5df9eff5adb0b2b0ed455c3791678edc570a2799f9f1f2 |
| 62 | addRestParameterIfNeeded | 105818-105917 | 7d6931b3bcf385971c3f1bbb7034f9596b31941dd1c5ee4496a4dea642340e4a |
| 63 | insertCaptureThisForNodeIfNeeded | 105918-105924 | 45639599b45b90b5d3d35c4ec5549e1867983f1535774671f0a08c0231f219e4 |
| 64 | insertCaptureThisForNode | 105925-105944 | 97a38bc8c3789af8530cfa22114dea94aea9c5c38df6c66977819cdc509a1c1e |
| 65 | insertCaptureNewTargetIfNeeded | 105945-106005 | 820984ac3bd8d30b44abadfe06e1257f60177a6f672c5acaa9a89549796dc117 |
| 66 | addClassMembers | 106006-106030 | 2c33fd973cf6cbe9089b25235ce4fafb3bf13741dc3a2a97bfd39b7a617b4374 |
| 67 | transformSemicolonClassElementToStatement | 106031-106033 | cda54eaa61e7acee6423576d92b8c77586ce02f2dcf4dcff25a4f8d3338c800e |
| 68 | transformClassMethodDeclarationToStatement | 106034-106072 | 5180ce812b5ee59c2bc0a4366caf06bb31d0a563d5dd6469e6a5bc76ee21e1d7 |
| 69 | transformAccessorsToStatement | 106073-106084 | 9fc3590ab0768f12cd93f556a2838fc0a2254e765f2b882d4397cbbd13e536c6 |
| 70 | transformAccessorsToExpression | 106085-106150 | 8230f8e121121efdb568b49d825d54980c8b0299156d5fead6690f1658cf6cb1 |
| 71 | visitArrowFunction | 106151-106178 | 5aa1e3a6520e9abf96f800b0657391f09b4616fe55362903ac71d6a676f2de0e |
| 72 | visitFunctionExpression | 106179-106201 | d282358038f6b8e9a21488951ec8505ed30e36de35066fb763dfec2e78283220 |
| 73 | visitFunctionDeclaration | 106202-106223 | fbb5a9f062f4bbf2352ecd3c56154c6b08b2b33a08a163bac32a03dd4bd90ce3 |
| 74 | transformFunctionLikeToExpression | 106224-106254 | a387fa1b4621bc39a9907377af4b532274faaaf20a565ddbbbb2e8f8dbeb33dc |
| 75 | transformFunctionBody | 106255-106329 | 3a3d99baf53b7ade96d462610aefdbf6855671e31375e93c5e5078e71d80d750 |
| 76 | visitBlock | 106330-106338 | f92d9420e434cf356bc214f53100e12d8623c1dcf9eba976a9e0c7c049968ddc |
| 77 | visitExpressionStatement | 106339-106341 | f18e77397af1d0ea9eba2f5bb4a868fd41b30dd61489f9fdf22573fbcd36d5d9 |
| 78 | visitParenthesizedExpression | 106342-106344 | 94058b819191804cd2cc9b65a6d3d0dceaff730f38adcbfeb381ef6b989713c0 |
| 79 | visitBinaryExpression | 106345-106364 | e0f6bfb2444d83cca54f4b8284d4e96b0e44fd648efd77c86788396ac456a65a |
| 80 | visitCommaListExpression | 106365-106381 | 228ee52cee142abb49c94501f55f0f27bdc556ce15530e2dd3d29291c919cbde |
| 81 | isVariableStatementOfTypeScriptClassWrapper | 106382-106384 | 31a18db369045519f75b5175bc55db48232f9ed7b1b1ce746ef638b72a0459da |
| 82 | visitVariableStatement | 106385-106418 | d4ad2ab4e75c439dfd94062c1cc87cb9085fd84128230174ffd73126ebc697f2 |
| 83 | visitVariableDeclarationList | 106419-106439 | 4d6889b5ad683119291304054db2694d3d191227211d32ab8abd88a3265dedfc |
| 84 | getRangeUnion | 106440-106447 | dc50f7cf51de23e7e3c3ec0dbeaa884f2e6b0d3cfbbb7fef1323d07a98064a88 |
| 85 | shouldEmitExplicitInitializerForLetDeclaration | 106448-106454 | 57764e4e2c565e7ea31404315d66ecd6fe9692c724433b66922d4140f7b86309 |
| 86 | visitVariableDeclarationInLetDeclarationList | 106455-106472 | dd4a618cbe337593fb6db97a7f3a56c60b2378256e02f079570580a92fa74d18 |
| 87 | visitVariableDeclaration | 106473-106491 | e60df465c1dafe75387852d60a455f4b45ac1e8dbf10429cfd0a0b461dce0de2 |
| 88 | recordLabel | 106492-106494 | 7766cbffd08668b0700407f95c14a9d112c7349b37a520b5f624d40fede08d26 |
| 89 | resetLabel | 106495-106497 | 2917d74d5fa128ee63b3c7a562a397c29a2d8336accc3985b953437b694df144 |
| 90 | visitLabeledStatement | 106498-106512 | 4d2f491656597174aa3adcd6665ce0d5ed8d6bd2d010cef26a6da0b9fccd5b72 |
| 91 | visitIterationStatement | 106513-106525 | 422645a8d8919dd57a850ad214ad5a905a80156245ff7d3663d71e5d9269994c |
| 92 | visitIterationStatementWithFacts | 106526-106531 | 5f0815db9d20f7a9bbdf228511f13008cf6c9026941386bf36fe76bfae8996d1 |
| 93 | visitDoOrWhileStatement | 106532-106539 | 736b53bebd94da898f5c5a9412b5b8051155812d1c03af7a9ccfbe371e0981e3 |
| 94 | visitForStatement | 106540-106547 | f040e1f84b67a1ed94230ef30d1762f6557bd9a13e7b49ec1d4e3eae85f9b00e |
| 95 | visitEachChildOfForStatement2 | 106548-106556 | d27328859478fc4bf59e3ac4e80c09f3b46719c7b592ba2a64cd561e2fcdd10b |
| 96 | visitForInStatement | 106557-106564 | 0828972826e4ba4aa77bc16c291feed80966d6e38f2b396613400e29a1293861 |
| 97 | visitForOfStatement | 106565-106573 | 93f7ac8c9b196b64c10c4f30ec9f10714beb4f5218a7e2d6ce9603dabe482875 |
| 98 | convertForOfStatementHead | 106574-106655 | b36ba10e482e3c2d129b9ddb1b17835aae94fb81371dc6c16850f0205a51eee6 |
| 99 | createSyntheticBlockForConvertedStatements | 106656-106665 | 1bb56192da2351cc83b39761bedbe1437ec3cb21317e0986bac41d5c21f8b942 |
| 100 | convertForOfStatementForArray | 106666-106725 | a31a359ace6132e5eb162c28a9a6899fdd8e750503e48c1a6ee4369762136a81 |
| 101 | convertForOfStatementForIterable | 106726-106866 | f82ef9afe08d89403b8995a26cb5ebeda48ce9fcca7236238fd35c689265f0c2 |
| 102 | visitObjectLiteralExpression | 106867-106899 | b459cf3d073d23b749e01b2e2ea12872454374ebd9d40138fad657bba005fd63 |
| 103 | shouldConvertPartOfIterationStatement | 106900-106902 | ae1d4df2b0e2feee943c00bc03955266d0c0c24cb4b6781b4977befa81da8874 |
| 104 | shouldConvertInitializerOfForStatement | 106903-106905 | 34f7fb1bc69073490b78befae72bc14ff5d1d1d87a6794f3322f57e17bd9893e |
| 105 | shouldConvertConditionOfForStatement | 106906-106908 | 1d5d8c978af2409b66085555b18223a0245a30e5141ee284962f61c3afc34aec |
| 106 | shouldConvertIncrementorOfForStatement | 106909-106911 | 4ac4fd109916bbeb753cd35a801b50a546c37e3b4df1fe97471fb30e375b4bff |
| 107 | shouldConvertIterationStatement | 106912-106914 | 09fd782f527c4038cc63193bded9b82ac9ca5c995517728c83c99318d2a6c91a |
| 108 | shouldConvertBodyOfIterationStatement | 106915-106917 | 6339bb663ae636e10c16f85310a24951d206da87d1bf1506209ee28eb8251073 |
| 109 | hoistVariableDeclarationDeclaredInConvertedLoop | 106918-106934 | 63349b45803aa21b02603a402e922910139de7bbe9857b9a0ed53f64edd1b506 |
| 110 | visit | 106923-106933 | 10f64666aecf1edf3c8d54b056f1596a4423c0cbf95fd81f2b9f5d709b1581f8 |
| 111 | convertIterationStatementBodyIfNecessary | 106935-106989 | 91611cb45f3877b4283ae47aa6b98a7ddf7044a6053537474e6a9cbb3ec709e3 |
| 112 | convertIterationStatementCore | 106990-107005 | f90fb8585825172623599e05fc7e4355890ff1a58d8b04bdfef969378aecc8ad |
| 113 | convertForStatement | 107006-107016 | d1ec9787f729191e41a6ac2d575f6c2ac009d24b30f42a8ddd71db34dad66105 |
| 114 | convertForOfStatement | 107017-107026 | 274f8709c95acd37440d505d323b00cc2387f3288cd7eb16ee17bbeddfa32922 |
| 115 | convertForInStatement | 107027-107034 | 47dc93245170b45b5460e57e46947715743257cd428221d1bcbd832f81c77e0d |
| 116 | convertDoStatement | 107035-107041 | 26d0afc9d37d0038fce2be9eef460ed0b1b86c542e6dde422eaf3369960720a2 |
| 117 | convertWhileStatement | 107042-107048 | 92d42279b892c99ee5908d5053cb72c4f6a36b764cd59a77635124872d2bd7e0 |
| 118 | createConvertedLoopState | 107049-107082 | 669273fa3a9362caa338addc7f71db0cc202d64692e08e9e3797c22994b73f13 |
| 119 | addExtraDeclarationsForConvertedLoop | 107083-107157 | 5cf33cd652664d05c078f8516837e513444eabcdb3734e585a99b4dd87553a34 |
| 120 | createOutVariable | 107158-107167 | 509144252ca51bbed068e9bb8f00d0fc059cd07da3329dd47fc9d1a5353d0836 |
| 121 | createFunctionForInitializerOfForStatement | 107168-107224 | bc8e778206246a551c531ac4e89eb4b4ebf6b64cf23182053a8497394babe1bb |
| 122 | createFunctionForBodyOfIterationStatement | 107225-107306 | 16d0864da1c018f8ee227fe9e941165f10e8aab9804485bb4ae3deb610d1db9c |
| 123 | copyOutParameter | 107307-107311 | 2682ac234923738698251f1c7266033d54aa0ce45df18508b8f4c064e77e27b3 |
| 124 | copyOutParameters | 107312-107318 | a34a0cbd980c8aeb4f4be1e7d5e0aa85ca5b5abcfea6b1b0e0737154c042d318 |
| 125 | generateCallToConvertedLoopInitializer | 107319-107331 | 4a0b400a40b21ed872ee8dd8ca175d345558c50113df092cebd1849bdcc7cfcb |
| 126 | generateCallToConvertedLoop | 107332-107419 | c5ca870e65bed1b57dabee006e28542b2ce95047c0bde442fddc525cd16a57e2 |
| 127 | setLabeledJump | 107420-107432 | 6fe2c8c909ed47acea83f4d4a959a2c812131fb756c1d928696f9f82c46f22a1 |
| 128 | processLabeledJumps | 107433-107448 | e00b53461f6a95a971d911b5595e0d7e04a1ef3918d73bbeee9d8d65b5a24339 |
| 129 | processLoopVariableDeclaration | 107449-107483 | 36518bd98c58010b529ec039cd838439bdb3e5fb8dcb475498547dbd2492abe0 |
| 130 | addObjectLiteralMembers | 107484-107511 | a1cdb8896ce8298383f56fe3daec7ff246c2b38042292b0db0c4cc6a804f4df7 |
| 131 | transformPropertyAssignmentToExpression | 107512-107526 | 5b2000cdb58228a9f8836d1e0c0864293f953fb46e31e155f5451a410b79e18e |
| 132 | transformShorthandPropertyAssignmentToExpression | 107527-107541 | 7a7be981ba5757884fb490e01196a675412feee29e9ea8128f44db7d18706fc3 |
| 133 | transformObjectLiteralMethodDeclarationToExpression | 107542-107563 | 1dd122082b022ec70935a58b9f3b93a3d664d0d00ccd08829398649a2226d158 |
| 134 | visitCatchClause | 107564-107595 | a07efc99cc6405347314776be164ca516462bcede9e0c73b579d1c11057f7b93 |
| 135 | addStatementToStartOfBlock | 107596-107599 | ff982635f12dfb32fc1e44767af664eac088f99a07aa8ee4c0533ea9fe3d9cc1 |
| 136 | visitMethodDeclaration | 107600-107620 | d56b4681c7eceb5147d21466386bbe1363effc5ed2af0aed1e5f11b02c2db3a3 |
| 137 | visitAccessorDeclaration | 107621-107637 | 74e578370a593301971ed3b86923d55e1bf4321bf3e2a65152f1b617043ed7fa |
| 138 | visitShorthandPropertyAssignment | 107638-107647 | 8c948a1b8b24003593c33efc4fbdcd9b65a0a2042e5c1fbc3e3250b84fdb0796 |
| 139 | visitComputedPropertyName | 107648-107650 | 0e8ce1579d4516671fdca0e0a617143696f43dd041b6bc04b56d10b967905938 |
| 140 | visitYieldExpression | 107651-107653 | 0cbeb41776ccdeeb9f17e80b64a30c6ae7f8e8f7c2bea8708036f8419518029d |
| 141 | visitArrayLiteralExpression | 107654-107666 | 73164d757cad43b2e92f90479df18e4733f74299d6d835b4ea4eff9043c14b32 |
| 142 | visitCallExpression | 107667-107686 | 45cabc1dc62c5d5e4f84ecbcdeb1b38b19ede361647ea40e357745f945c7f160 |
| 143 | visitTypeScriptClassWrapper | 107687-107783 | cd917e405f67b9f0859344acdb38cb908312e37cb2b1bb466bff69ffc3f3c19e |
| 144 | visitCallExpressionWithPotentialCapturedThisAssignment | 107784-107828 | 5e6fc555b510aa03d52a0fdffbfd4d9c4b05375f5a5bb5224b5993e84e5db08e |
| 145 | visitNewExpression | 107829-107852 | f600744e13d11f657ef35a9e79ac71fe932c76773fd6bcf5e4c0299256cdb518 |
| 146 | transformAndSpreadElements | 107853-107879 | e8c7f17449f433c1dbed7938b6de257d7a724f594fbddf4c7c9c1284dbf3c994 |
| 147 | partitionSpread | 107880-107882 | ecee966e972615cff23821fd56e7ad52e89b601287f42cd0d993d24da397eda8 |
| 148 | visitSpanOfSpreads | 107883-107885 | 216584b6acc9497681b024052f842da9d7f8d1eadbb2f6a269e2e8cc907b5647 |
| 149 | visitExpressionOfSpread | 107886-107901 | 3a184d6548d64722a1e1a352a831c0bfa6d360bb1718f25a82b8d41d4f88d138 |
| 150 | visitSpanOfNonSpreads | 107902-107908 | 36b1da1779ffb146910da0130ca7c5908e731fa807701bf42f83140de1cc7225 |
| 151 | visitSpreadElement | 107909-107911 | 4739a46ea49b2710d8998664d780ed2343a4b97cb404cef87c4a779d2ff8761f |
| 152 | visitTemplateLiteral | 107912-107914 | fde517b62bb80b6d1d22dd2d9dcf537b52c27350c8865d8037e4e9b105c88fe0 |
| 153 | visitStringLiteral | 107915-107920 | 681f6439ed00ed909b262b464c9792b22130db1231779524d766d3414928d92e |
| 154 | visitNumericLiteral | 107921-107926 | 4caf4d932461774f5450b679677ebf901915403b0c7650ac0fe3e010d027996b |
| 155 | visitTaggedTemplateExpression | 107927-107936 | 311c98a5b65cc44b160bbf1a221a3950679691e0fca83a65b8449b4ef4a8a919 |
| 156 | visitTemplateExpression | 107937-107952 | f5dfead2be93a40dc2a498913d2945edfa7d29c222bb2e642422251792d07c1d |
| 157 | createSyntheticSuper | 107953-107955 | 32fcaf9fa900d7c23e9a318e0917e585cf489bd790e4d5e63760e11e2a5fd652 |
| 158 | visitSuperKeyword | 107956-107962 | 6d19da8b75d9f4d7234044542bdbfff72f391935c4dc1f7273270c6484192a19 |
| 159 | visitMetaProperty | 107963-107969 | d21802d0b4b5f3394247c5b071c3a3a78e4c847d6b72d70e1b950ed1de9f090b |
| 160 | onEmitNode | 107970-107981 | d40d38f5984c152e785e53bf787aecc395b7447a7406a1aa44a67d00cf0b33c0 |
| 161 | enableSubstitutionsForBlockScopedBindings | 107982-107987 | 370d3cfeda365a23d8182f0904f69f14e9a1da986f804b19645022e3dc587366 |
| 162 | enableSubstitutionsForCapturedThis | 107988-108000 | 1a495f7143adcc0d9d17a756942d319e3e45ea692c9c31a5e2923ffc643f02ea |
| 163 | onSubstituteNode | 108001-108010 | b394c52c1aa81fe0824beb6fe6e136215fca7c67804c0c4dcf30d9ae352f2147 |
| 164 | substituteIdentifier | 108011-108019 | 013c4617e93fc50e00b9b10b58a0e71fe12e2279482711828178120972f71291 |
| 165 | isNameOfDeclarationWithCollidingName | 108020-108029 | 99833a0d93e796be19158f565367ca8bd1977344d1d60615bd0b339d86a26e6b |
| 166 | substituteExpression | 108030-108038 | 24124bc8ea7bd7cd3d994f6ba1b134eb70d59d2831dce4e6fdd3baf10dc8508d |
| 167 | substituteExpressionIdentifier | 108039-108047 | 46f51cc58b34c3da1fc518e5eda36f005120dfb5bdc34bbf29499ecfd2777deb |
| 168 | isPartOfClassBody | 108048-108064 | e74c699f696ee9a55e13a0d193a41e8a34e1314678776a5801a0a58fb3a2c22f |
| 169 | substituteThisKeyword | 108065-108070 | 5b11bd13c32beab2d6c24f4fa50ba8c8872804d6bcd1b5781594a0504153bac7 |
| 170 | getClassMemberPrefix | 108071-108073 | cd87f6646a467f778a7b7900a34066d49198e9e5823a370c845f780b71cc689a |
| 171 | hasSynthesizedDefaultSuperCall | 108074-108099 | 3759ff43da6111de19fca9fcccff9beb9b9fd63d75954b1e950cce4e58deaaa7 |

### 4.2 Owner-adjacent addenda (ported with their own headers)

New pins (not previously ported by any packet):

| function | lines | d2 sha256 |
|---|---|---|
| `spanMap` | 324-356 | `b81f70c665fc5bbbb916ce4cd80f4ff26706146062aaedd9a179b861a0191cc1` |
| `arrayIsEqualTo` | 457-470 | `dbabd399703753a41ee061112da610ed985667c1ba1797bd4c371924ebe47395` |
| `singleOrMany` | 604-606 | `889a4b67e5fe6f0fd05b6c0b7a2a997959eb77ffe7e0e16de8f125b625a82cee` |
| `getCombinedNodeFlags` | 11342-11344 | `5d698610c67a2e73fafa1a1472315acb0ae677c2b67f4bf46e0d5d970f792061` |
| `getParseTreeNode` | 11426-11437 | `80b5c2449cb8320cf209184a8eef484f944379da161e54d74dd54ed1f0d2d592` |
| `unescapeLeadingUnderscores` | 11441-11444 | `e8294a1e4ef10b8ca2bcce06045e22adab6689e46b655acf51bacc3810ef5271` |
| `getNameOfDeclaration` | 11562-11565 | `5d3aafbdab871f0fe6f088a4904cd11e6b44e467e0cca8ad0c215b3f899b570b` |
| `insertStatementsAfterCustomPrologue` | 12947-12949 | `d761e394849a886073c226027fd159cc65bbef99618c3957487240f411612c90` |
| `insertStatementAfterCustomPrologue` | 12950-12952 | `7b1a417fc2da425e75dac5bedf61ad6e0021e0b6e88543114a64e6942fc300b6` |
| `getEnclosingBlockScopeContainer` | 13844-13846 | `50444054506d87acb188cbcd3ed441a6c57e41352eda843ae6f0840bbbb1cc07` |
| `isSuperCall` | 14147-14149 | `ed46d3b633bea556783f24654454e51a494901f4fde4c70b6c39bac09b32f806` |
| `isHoistedFunction` | 14167-14169 | `a43dffc56712a0f0a13148f4eca8cd05064849784894e44d65835c84e84b880a` |
| `isHoistedVariableStatement` | 14173-14175 | `be4121319d7decd5d3087cc7fd9d2eb5510b17ae67b08bc236fb082f77b141d8` |
| `unwrapInnermostStatementOfLabel` | 14393-14403 | `b2ed1607745b0f49fd60c8231a9a8d0c223f8eacaba7ceabbce4f801d009bf91` |
| `isSuperProperty` | 14608-14611 | `d71f4915c785ca5e6a0642e8c3e85529c28ea19447923c8a337ab6ffa5c4f262` |
| `getClassExtendsHeritageElement` | 15752-15755 | `7101b7d0f1e607daa5a4ec5b194f7d3cfe15c24c30ebbdbb41a845dceaae5c7d` |
| `isIdentifierANonContextualKeyword` | 15806-15809 | `b6d4aae387d7c92d3e2fcd53d07d177cad21e661e6e3e8a9fd838e457b1146cd` |
| `getFirstConstructorWithBody` | 16674-16676 | `9a7337f235fb939299cfc0513bfd74f5a61039c196919e9dde7af622e2557370` |
| `isDestructuringAssignment` | 17114-17124 | `57f11978bed7f73705f836f943b584fbe39823ae01178fff5a5b6b046b44268b` |
| `createRange` | 17297-17300 | `520b69615170f74d3a3927feddbf50e2061870b64af199c54c6404f46e046f17` |
| `moveRangeEnd` | 17301-17303 | `8a135c6e337b82d65993e5115e1b1acdd0761c053c02ba2df5946efcea1ab184` |
| `moveRangePos` | 17304-17306 | `11da3d6e63737439c2a5e1069044dc8474e21b2bf5a36d5c66e78ee47add4cba` |
| `createTokenRange` | 17318-17320 | `d49903d6e91ae703df414c06af8caa9c04179a99e4c627f83be1d54eef5d1f14` |
| `rangeEndIsOnSameLineAsRangeStart` | 17352-17359 | `dd99f6cad4d8b4ddb82e6dce98f287b16aa413c040a022706f3488b89ce7983a` |
| `isPackedElement` | 19085-19087 | `54c84a760177f650beba6c58055d20b350882ea15749fa4842f25a9ccef3f9b6` |
| `isPackedArrayLiteral` | 19088-19090 | `76b37f17f9a548219da8522d4a2533bbff2f4a6173852a73012cdfffed4cda42` |
| `createExportDefault` | 24522-24530 | `e531c9901b0419c3d02970c67a28f7e130d9d8701ac5cc6523f5568f8fd6172b` |
| `createExternalModuleExport` | 24531-24547 | `6290f43d065740c822616b58b195cee6622f8f717bdae7037b39e1fc2648495f` |
| `createTypeCheck` | 24548-24550 | `545917e4fff60f1d07b445c7b41156183e8da6c608d5723705fe1edc1bf1f553` |
| `createFunctionCallCall` | 24580-24582 | `b63730e192802f36eae8706de4ffa67ce7722aadcfd695eeea01f77f1b7a3f00` |
| `restoreOuterExpressions` | 24646-24654 | `954f25c47999754f47c599c6955c83aa60e378fc74ef0ba8fd54289bbd65abd8` |
| `restoreEnclosingLabel` | 24655-24668 | `fe151529af3462a6c56359506563a0b4173bcd3ea5c0605e9beb4ac6a2a8d298` |
| `inlineExpressions` | 24785-24787 | `0b804e265fda3151c49457cd1f8ca94580b01c04d161eed6103baadbec28db8a` |
| `liftToBlock` | 24878-24881 | `c96ac6375abe99aeb4b2779fc5d1a4b28d835df33d5198647cd888d1abd36a48` |
| `mergeLexicalEnvironment` | 24889-24932 | `ac1f665ea3f8a127f7cb6dbd55b79a8e307e38359a9aef18a2f5dada71bcd2c2` |
| `replacePropertyName` | 24945-24962 | `ce59174f1f7c2ec07e7c3b0a12a8b0428892682975d39f417a4895d23bc0d5ee` |
| `getSourceMapRange` | 25336-25339 | `7b45f9797ce3582eccac7e1a3469e63d81994d1aa38d604e7e28f671ece9329e` |
| `setTokenSourceMapRange` | 25344-25349 | `d6e7ca6d4906720533d07640c7ccf2445ff5b79109f0470afba3c66a3f1a5614` |
| `getCommentRange` | 25358-25361 | `84e06c1d1498906aa5765dfc0bdfd9dd4ca9d1c75367b90067f24dbc57936cd2` |
| `addSyntheticLeadingComment` | 25374-25376 | `0aa536dc24cbad2ca0fef6dd5db63126f69c3dfb6e8e86ab23f18c092572d580` |
| `moveSyntheticComments` | 25388-25395 | `dbec5c77db1209731faea7ecc4bbe067a09abe111ed885ca1c4dfb7b7b90677a` |
| `isCallToHelper` | 26566-26568 | `65c471809533a93e4ad2d44931471cb8a169cf9c93c9b291bc7a7dbdeede8fef` |
| `getSuperCallFromStatement` | 93070-93076 | `f777d5cf25bf07fb7171f609cfc81def662cbf6df522af76a81dc77e3f355287` |
| `createSpreadSegment` | 104737-104739 | `a04413ca6e352516d65787da81e6c23deca95098fefaa252b3d37f77865480c8` |

Additional new pins folded with the table above:
`createMethodCall` (24554-24576, the shared worker behind the three
call-shape wrappers; its call-chain arm is unreachable from synthesized
ES2015 receivers and ports as the plain-call arm with the guard
asserted), `createFunctionBindCall` (24577-24579),
`createFunctionApplyCall` (24583-24585),
`createGlobalMethodCall` (24586-24588), `isHoistedVariable`
(14170-14172, the identifier-name-and-no-initializer element predicate
of `isHoistedVariableStatement`), `getName` (24788-24799 — the
`getLocalName`/`getInternalName` worker: clone the declaration's
identifier name with range+parent threading, INHERIT the name's emit
flags, add `NoSourceMap` when source maps are not allowed and
`NoComments` when comments are not allowed, stamp the requested
LocalName/InternalName flags; FALL BACK to `getGeneratedNameForNode(node)`
when the declaration name is missing, generated, or not an identifier —
the anonymous-class-expression arm `class_expr_anon` exercises), with
its wrappers `getInternalName` (24800-24802,
LocalName|InternalName), `getLocalName` (24803-24805, LocalName;
`allowComments=true` at the `visitClassDeclaration` call `:105146`),
`getExportName` (24806-24808, dormant here), `getDeclarationName`
(24809-24811, dormant here), `asExpression` (24969-24971,
the string/number/boolean literal coercion the descriptor/type-check
constructors consult), `updateOuterExpression`
(24625-24642) + `isIgnorableParen` (24643-24645) (the
`restoreOuterExpressions` internals), and `escapeLeadingUnderscores`
(11438-11440, the `createIdentifier(unescapeLeadingUnderscores(...))`
inverse used by identifier construction) — exact d2 hashes land in the
module headers and the ledger gate verifies them (`cargo xtask ledger
check` recomputes every span hash; the table hashes above were
generated programmatically from the vendored file at authoring).

Re-pinned spans this module ports AGAIN as its own module-internal
functions (first ported by B-2/B-3; the spans and d2 hashes are
IDENTICAL to the B-3 packet §4.2 rows (several spans also carry
landed headers in `generators.rs`/`legacy_decorators.rs`) and are not
repeated here): `createCallBinding` 24691-24753 (B-4 ports the FULL
function including the two super arms, which are LIVE at the ES2015
pipeline position — `isSuperProperty` → `thisArg = this`,
`isSuperKeyword` → `thisArg = this`, `target = callee` because the
ES2015 call sites pass `languageVersion = undefined`; the
`languageVersion < ES2015 → "_super" identifier` arm is dormant and
ports faithfully), `shouldBeCapturedInTempVariable` 24669-24690,
`skipOuterExpressions` 27582-27587, `isOuterExpression` 27561-27581,
`createMemberAccessForPropertyName` 27206-27217,
`getAllAccessorDeclarations` 16719-16760,
`createExpressionForPropertyName` 27339-27347, `hasDynamicName`
15850-15853, `getPropertyNameForPropertyNameNode` 15861-15887,
`createObjectDefinePropertyCall` 24595-24597,
`createPropertyDescriptor` 24614-24624, `tryAddPropertyAssignment`
24607-24613, `copyPrologue` 24827-24830, `copyStandardPrologue`
24837-24857, `copyCustomPrologue` 24858-24870, `visitParameterList`
91168-91181 (the `addDefaultValueAssignmentsIfNeeded` arm is gated
`getEmitScriptTarget(...) >= ES2015` and stays DORMANT at the ES5
construction this packet makes — ES2015's OWN default handling is
`addDefaultValueAssignmentsIfNeeded2` in `transformFunctionBody`),
`startOnNewLine` 27596-27602. (`createFunctionApplyCall`
24583-24585 rides the new-pin block above with its `createMethodCall`
siblings — B-3 built the apply shape as an unpinned constructor
idiom; B-4 pins the factory family.)
Duplication over reuse is deliberate: the B-3 copies are
module-internal to `generators.rs` and thread its visitor state;
re-porting keeps this packet's fence away from every landed
production file (§7 fence). The ledger accepts multiple headers
pinning the same span (each header's hash verifies independently).

List helpers (`map`, `some`, `every`, `filter`, `forEach`, `append`,
`addRange`, `concatenate`, `flatten`, `first`, `firstOrUndefined`,
`last`, `lastOrUndefined`, `elementAt` incl. its negative-offset
arm, `singleOrUndefined`, `cast`/`tryCast`, `idText`) are
`Vec`/iterator idioms exactly as in B-2/B-3; trivial single-kind
predicates (`isBlock`, `isIdentifier`, `isVariableDeclarationList`,
`isVariableStatement`, `isVariableDeclaration`, `isExpressionStatement`,
`isBinaryExpression`, `isCallExpression`, `isPropertyAccessExpression`,
`isArrayLiteralExpression`, `isSpreadElement`, `isOmittedExpression`,
`isCatchClause`, `isCaseBlock`, `isCaseClause`, `isDefaultClause`,
`isReturnStatement`, `isIfStatement`, `isWithStatement`,
`isSwitchStatement`, `isTryStatement`, `isLabeledStatement`,
`isForStatement`, `isIterationStatement`, `isBindingPattern`,
`isComputedPropertyName`, `isPrivateIdentifier`, `isClassLike`,
`isClassElement`, `isPropertyDeclaration`, `isFunctionLike`,
`isGeneratedIdentifier` = `metadata.generated_binding_id().is_some()`,
`isInternalName` = `EmitFlags::INTERNAL_NAME` read, `isStatic`/
`hasStaticModifier`/`hasSyntacticModifier` = modifier-list scans,
`isAssignmentExpression`, `isDestructuringAssignment` re-pinned
17114-17124, `isStatementWithLocals`, `nodeIsSynthesized` =
`SourceRange::Synthesized` position test as B-2 pinned it,
`isPrologueDirective`, `isCustomPrologue` = `EmitFlags::CUSTOM_PROLOGUE`
read, `findSpanEnd`, `findAncestor`, `isBlockScope`,
`getHeritageClause`, `skipParentheses`, `skipTrivia` = the existing
scanner-side helper, `getEmitFlags`/`getInternalEmitFlags` = metadata
reads, `getCombinedNodeFlags` re-pinned 11342-11344) stay inline
`matches!`/classification idioms or tiny pinned fns per the table
(es2018/B-3 precedent).

### 4.3 Frozen behavior pins

- **HierarchyFacts alphabet** (bundler-inlined; port as typed
  consts): Function=1, ArrowFunction=2, AsyncFunctionBody=4,
  NonStaticClassElement=8, CapturesThis=16,
  ExportedVariableStatement=32, TopLevel=64, Block=128,
  IterationStatement=256, IterationStatementBlock=512,
  IterationContainer=1024, ForStatement=2048,
  ForInOrForOfStatement=4096, ConstructorWithSuperCall=8192,
  StaticInitializer=16384; AncestorFactsMask=32767; subtree facts
  NewTarget=32768, LexicalThis=65536, CapturedLexicalThis=131072;
  FunctionSubtreeExcludes=229376. Enter/exclude pairs as they appear
  at the call sites: SourceFile=(8064,64), Constructor=(32662,73),
  AsyncFunctionBody=(32662,69), Function=(32670,65),
  StaticInitializer=(32670,16449), ArrowFunction=(15232,66) with
  subtree-exclude 0, BlockScope=(7104,0),
  IterationStatementBlock=(7104,512), Block=(6976,128),
  DoOrWhile=(0,1280), For=(5056,3328), ForInOrForOf=(3008,5376);
  three further ad-hoc pairs ride their fn ports verbatim —
  `transformFunctionLikeToExpression`'s non-static class-member variant
  (32670, 73=65|8) at `:106227`, `visitVariableStatement`'s
  (0, 32-when-exported) at `:106386`, and `visitVariableDeclaration`'s
  (32, 0) at `:106474`.
  `enterSubtree` masks to AncestorFactsMask; `exitSubtree` keeps
  subtree bits and restores ancestor bits (`:104782-104789`).
- **Substitution/jump/out-param alphabets**: ES2015SubstitutionFlags
  CapturedThis=1, BlockScopedBindings=2; Jump Break=2, Continue=4,
  Return=8; LoopOutParameterFlags Body=1, Initializer=2;
  CopyDirection ToOriginal=0, ToOutParameter=1; SpreadSegmentKind
  None=0, UnpackedSpread=1, PackedSpread=2.
- **Resolver flag values** (NodeCheckFlags, tsc_types
  flags.rs:1521-1619 matches upstream):
  LoopWithCapturedBlockScopedBinding=4096,
  ContainsCapturedBlockScopeBinding=8192,
  CapturedBlockScopedBinding=16384, BlockScopedBindingInLoop=32768,
  NeedsLoopOutParameter=65536. The checker rules that mint them are
  pinned as the FixtureResolver specification (§12.2):
  `checkNestedBlockScopedBinding` `_tsc.js:72250-72290`,
  `isBindingCapturedByNode` `:72291-72294`,
  `isAssignedInBodyOfForStatement` `:72295-72311`,
  `isSymbolOfDeclarationWithCollidingName` `:87921-87958`,
  `getReferencedDeclarationWithCollidingName` `:87959-87970`,
  `isDeclarationWithCollidingName` `:87971-87979`,
  `isArgumentsLocalBinding` `:87858-87866`; the helper predicates the
  rules consult: `isInsideFunctionOrInstancePropertyInitializer`
  `:72237-72239`, `getPartOfForStatementContainingNode`
  `:72240-72242`, `getEnclosingIterationStatement` `:72243-72249`
  (findAncestor, quitting at a new lexical environment),
  `isStatementWithLocals` `:12879-12889`
  (Block/CaseBlock/For/ForIn/ForOf), `isBlockScopedContainerTopLevel`
  `:13731-13733` (SourceFile/ModuleDeclaration/function-like),
  `isSymbolOfDestructuredElementOfCatchBinding` `:87918-87920` (the
  catch-destructured-element arm of the collision rule; its
  `walkUpBindingElementsAndPatterns` walk `:11315-11321` — note
  `checkNestedBlockScopedBinding`'s catch-skip at `:72251` skips only
  declarations whose PARENT is the catch clause, so destructured
  elements, parented by the pattern, DO take the nested-binding rules),
  and `isAssignmentTarget` `:15597-15599` over `getAssignmentTarget`
  `:15536-15579` (the out-param assignment test consulted by
  `isAssignedInBodyOfForStatement` at `:72301`; the fixture language
  reaches the direct-assignment, compound-assignment, and ++/-- arms).
  The two additional
  `LoopWithCapturedBlockScopedBinding` setters (`:74072`, `:81546`)
  are class-fields/private-name arms outside the §7 fixture language.
- **The two `yield*` synthesis sites** (the producer half of the
  pinned edge): `generateCallToConvertedLoopInitializer`
  `:107326-107329` and `generateCallToConvertedLoop` `:107341-107344`
  both mint `createYieldExpression(AsteriskToken,
  setEmitFlags(call, EmitFlags.Iterator))`; the Iterator stamp is
  exactly what B-3's `visitYieldExpression` consumer-skips
  (generators.rs:2338-2352 — no `__values` wrap on the delegation).
- **Name-generation spellings** (verified live against the §7.3 probe
  emits): `_this`/`_newTarget`/`_super` = `createUniqueName(text,
  Optimistic|FileLevel)`; `_loop_1`/`_loop_init_1`/`out_<x>_1`/
  `state_1`/`inc_1`/`this_1`/`arguments_1`/`e_1` = plain
  `createUniqueName` (numbered family); `_i` = `createLoopVariable`;
  `_a`… = `createTempVariable`; `getGeneratedNameForNode` = the
  node-keyed numbered cache (`x_1` for renamed collisions and
  pattern-named parameters/for-of rhs references).
- **Class IIFE shape**: `var C = /** @class */ (function (_super?) {
  __extends(C, _super);? function C(...) {...} <members> return C;
  }(Base?));` — the `@class` synthetic leading comment rides the
  PARENTHESIZED result (`addSyntheticLeadingComment(...,
  MultiLineCommentTrivia, "* @class ")`); the double
  `PartiallyEmittedExpression` wrap pins comment-suppressed end
  positions (inner end = node.end; outer end = skipTrivia(pos)); the
  class function takes `(getEmitFlags(node) & Indented) |
  ReuseTempVariableScope`; the `return C` statement is ranged to the
  close-brace token range and carries `NoComments|NoTokenSourceMaps`;
  the body block is multiLine, ranged to members, `NoComments`;
  `__extends(C, _super)` is an expression statement ranged to the
  heritage element; the constructor function is ranged to
  `constructor || node` and carries `CapturesThis` whenever an extends
  clause is present (INCLUDING `extends null`, where `isDerivedClass`
  is false — `_tsc.js:105283-105285`).
- **Constructor protocol**: default derived body `return _super !==
  null && _super.apply(this, arguments) || this;`
  (`createDefaultSuperCallOrThis` with `NoSubstitution` actual-this);
  `hasSynthesizedDefaultSuperCall` `:108074-108099` detects the
  TS-transformer-synthesized `super(...arguments)` statement
  (parameterless constructor, every node synthesized);
  `mayReplaceThis = isDerivedClass || ConstructorWithSuperCall` gates
  `_this` capture with `createActualThis()`; the trailing
  `return _this;` is suppressed when
  `isSufficientlyCoveredByReturnStatements`; then the
  `simplifyConstructor` pipeline runs in order — inline super into
  the `var _this = this;` capture variable, inline the super-call +
  `return _this;` pair, elide the unused this-capture ONLY when one
  of the first two changed the body, and (synthesized-super only)
  inject the `_super !== null &&` presence check.
- **Converted-loop protocol**: `createConvertedLoopState` collects
  loopParameters from for-head block-scoped declarations (binding
  patterns recurse per element) and out-params per
  `NeedsLoopOutParameter`/head-capture; the body function is
  `var _loop_1 = function (params) {...};` with `NoHoisting` on the
  declaration list and `ReuseTempVariableScope` (+`CapturesThis`
  when containsLexicalThis; asterisk + `AsyncFunctionBody` when
  containsYield in the respective context) on the function; the
  initializer function `_loop_init_1` does NOT take
  `ReuseTempVariableScope`; simple loops call bare
  (`_loop_1(i);` + ToOriginal copy-backs), non-simple loops bind
  `var state_1 = _loop_1(i);` and dispatch `typeof state_1 ===
  "object" → return` / `state_1 === "break" → break` / labeled
  switch via `processLabeledJumps` (inner labels resolve to direct
  break/continue, outer labels re-return state through the outer
  loop's state); condition/incrementor conversion moves both into
  the body function under the `inc_1` conditionVariable protocol
  (first-iteration skip, `if (!cond) break` via a VISITED synthetic
  break); `convertForStatement` elides the converted
  condition/incrementor from the loop head and replaces the
  initializer with the out-variable declaration list part;
  `visitVariableStatement` inside a converted loop hoists
  non-block-scoped `var` declarations into `hoistedLocalVariables`
  and rewrites initializers as assignments (binding patterns via
  `flattenDestructuringAssignment` ON THE DECLARATION);
  break/continue become `return "break"/"continue"/"break-<label>"/
  "continue-<label>"` string markers (out-param comma-prefixed) when
  not locally allowed; `visitReturnStatement` wraps
  `return { value: expr }`; `this` → `this_1` (arrows instead set
  containsLexicalThis), `arguments` → `arguments_1` via
  `isArgumentsLocalBinding`; `addExtraDeclarationsForConvertedLoop`
  emits the merged extra `var` statement at the OUTERMOST converted
  loop (inner states propagate names to outerState instead).
- **for-of shapes**: array mode `for (var _i = 0, _a = expr; _i <
  _a.length; _i++) { var v = _a[_i]; ... }` (counter =
  createLoopVariable; rhs = getGeneratedNameForNode(identifier
  expression) or temp; `NoHoisting` on the head declaration list;
  `NoTokenTrailingSourceMaps` on the for statement; head statement
  ranges built with moveRangePos/End(initializer, -1)); iterable
  mode (downlevelIteration) wraps `try { for (var x_1 =
  __values(expr), x_1_1 = x_1.next(); !x_1_1.done; x_1_1 =
  x_1.next()) {...} } catch (e_1_1) { e_1 = { error: e_1_1 }; }
  finally { try { if (x_1_1 && !x_1_1.done && (_a = x_1.return))
  _a.call(x_1); } finally { if (e_1) throw e_1.error; } }` with
  `e_1`/returnMethod hoisted, SingleLine on the protocol ifs/blocks,
  and the `IterationContainer` ancestor fact selecting the inline
  `(e_1 = void 0, __values(expr))` error-record reset.
- **Spread protocol**: spanMap partitions spread/non-spread runs;
  single-segment shortcuts (argument list w/o downlevelIteration,
  packed array literal, existing `__spreadArray` call) return the
  segment expression; otherwise `__spreadArray` folds left starting
  from `[]` when the first segment is a spread; `visitExpressionOfSpread`
  wraps non-array-literal unpacked segments in `__read` under
  downlevelIteration (PackedSpread); the pack flag passed to
  `__spreadArray` is `segment.kind === UnpackedSpread &&
  !isArgumentList`. Calls decompose via `createCallBinding`
  (cacheIdentifiers=false) into `.apply(thisArg, spread)`; `new`
  decomposes into `new (target.bind.apply(C, [void 0,
  ...spread]))()`; super spread-calls thread `NoSubstitution` on the
  this-arg and wrap `_super.apply(this, spread) || this` with
  `_this` assignment per `assignToCapturedThis`.
- **Object-literal chunking**: gate = first computed-name property
  (or a ContainsYield property inside an async body — dormant here);
  the initial chunk keeps VISITED properties in a literal that takes
  `Indented` when computed names exist; remaining members become
  `temp.name = ...` assignments (accessor pairs fold into ONE
  `Object.defineProperty(temp, name, {get/set/enumerable:...,
  configurable: true})` expression at the firstAccessor position via
  `transformAccessorsToExpression`; methods via
  `transformFunctionLikeToExpression`); `startOnNewLine` per
  node.multiLine; the trailing temp reference is a range-threaded
  clone (multiLine arm) or the temp itself.
- **transformFunctionBody identity keystone**: standard prologue →
  hoisted-function custom prologue → hoisted-variable custom
  prologue → default/rest insertions → remaining custom prologue →
  visited statements; when `arrayIsEqualTo(statements,
  body.statements)` and no prologue was added, the ORIGINAL body
  node returns — this is the arm that makes generator-only fixtures
  reach transformGenerators as the parse tree (B-3 §12.6's argument,
  now consumed from the producer side).
- **Arrow body layout**: single-line vs multi-line decided from the
  PARSE positions of `=>` and the body
  (`rangeEndIsOnSameLineAsRangeStart`), `SingleLine` emit flag on
  the synthesized block when single-line and nothing forced
  multi-line; the return statement takes the body range,
  `moveSyntheticComments(returnStatement, body)`, and
  `NoTokenSourceMaps|NoTrailingSourceMap|NoTrailingComments`; the
  block's close-brace token source-map range = the body.
- **Print-time facts**: `onEmitNode` re-enters Function facts
  (+CapturesThis from the node's emit flags) around the emit
  callback for the seven enabled function kinds once CapturedThis
  substitution is enabled — `hierarchyFacts` is therefore
  TRANSFORMER-lifetime state consulted by `substituteThisKeyword`
  (`hierarchyFacts & CapturesThis → _this`) at PRINT time; identifier
  substitution consults the resolver
  (`getReferencedDeclarationWithCollidingName`, declaration-name
  positions via parent-kind dispatch BindingElement/ClassDeclaration/
  EnumDeclaration/VariableDeclaration) and skips generated/internal
  names and class-body-internal references (`isPartOfClassBody`).
- **Literal normalizations**: template head/middle/tail and
  no-substitution templates → string literals ranged to the node;
  extended-unicode strings re-minted plain; binary/octal numerics
  re-minted decimal (`numericLiteralFlags & 384`); identifiers with
  extended unicode escapes re-minted via
  `unescapeLeadingUnderscores(escapedText)` with original+range
  threading.

## 5. Rust semantic map

New dormant module `crates/emitter/src/builtins/es2015.rs`
(registered as `mod es2015;` in `crates/emitter/src/builtins.rs`).
Function-per-function port; every ported fn carries the
`tsc-port`/`tsc-hash`/`tsc-span` header from §4 and
`#[allow(dead_code)] // production consumers arrive with the B-5 owner`
where caller-less.

| Item | Target |
|---|---|
| module seam | `pub(super) struct Es2015Transformer<'resolver>` implementing `Transformer` (`name = "transformES2015"`, `transform_root`, `substitute_node`, `before_emit_node`, `after_emit_node`), built by `pub(super) fn transform_es2015<'resolver>(options: &CompilerOptions, resolver: &'resolver dyn EmitResolver) -> Box<dyn Transformer + 'resolver>` — the es2017.rs:110 seam with the constructor capturing `downlevel_iteration` and `use_define_for_class_fields` snapshots; B-5 registers it; the §7 suite constructs it directly and runs the REAL transformer chain `[transform_es2015, transform_generators]` through `transform_nodes` |
| transformer-lifetime state | `hierarchy_facts: HierarchyFacts` + `enabled_substitutions: Es2015SubstitutionFlags` + `emit_facts_stack: Vec<HierarchyFacts>` live on `Es2015Transformer` (NOT the visitor): `onEmitNode`/`onSubstituteNode` consult and mutate them at PRINT time, after `transform_root` returns (the B-3 rename-map precedent). `before_emit_node` pushes the saved ancestor facts and enters Function(+CapturesThis-from-emit-flags) facts for the seven enabled function kinds under `enabled_substitutions & CapturedThis`; `after_emit_node` pops and exits — the printer's error-preserving before/after pairing (printer.rs:8884-9128) guarantees stack discipline |
| visitor state | `struct Es2015Visitor<'a, 'resolver>` holding the per-root state: `context`, `source`, `resolver`, `current_text`/line map (for `rangeEndIsOnSameLineAsRangeStart` and `skipTrivia`), `converted_loop_state: Option<Box<ConvertedLoopState>>`, `tagged_template_string_declarations: Vec<TransformNode>` (dormant — the B-5 seam records into it), a `&mut` borrow of the transformer's `hierarchy_facts`/`enabled_substitutions`, `generated_bindings: GeneratedBindingScopes`, and the shared file-level `_this`/`_newTarget`/`_super` bindings; NO visit memoization — the visitor is STATEFUL (hierarchy facts, converted-loop state, and the unused-expression-result flag select dispatch), so the es2017 per-node memo map is deliberately not replicated (the B-3 generators precedent) |
| ConvertedLoopState | `struct ConvertedLoopState { labels: BTreeMap<String, bool>, labeled_non_local_breaks/continues: Vec<(String, String)> (insertion-ordered maps), non_local_jumps: Jump, allowed_non_labeled_jumps: Jump, arguments_name/this_name: Option<TargetBinding>, hoisted_local_variables: Vec<TransformNode>, condition_variable: Option<TargetBinding>, loop_parameters: Vec<TransformNode>, loop_out_parameters: Vec<LoopOutParameter { flags, original_name, out_param_name }>, contains_lexical_this: bool }` — outer/inner propagation exactly as `:107049-107156` |
| hierarchy facts | `HierarchyFacts` bitflags with the §4.3 alphabet; `enter_subtree`/`exit_subtree` as inherent fns over the borrowed facts |
| resolver bridge | the five queries through `EmitResolver` (resolver.rs:316-404) with `NodeCheckFlags::*.bits() as u32` for `has_node_check_flag` (es2017.rs:475 idiom); every query projects parse-tree nodes via `TransformArena::parse_tree_resolver_node` — synthesized nodes never reach the resolver (upstream `getParseTreeNode` guards, ported) |
| FlattenHost | `impl FlattenHost for Es2015Visitor` — `context`/`context_ref`/`flatten_source`/`downlevel_iteration` (the captured option)/`generated_bindings`/`visit_expression` → its own `visitor` entry/`visit_binding_or_assignment_element` → same; `create_assignment_completion` keeps the default-None arm (upstream ES2015 passes no createAssignmentCallback); call sites: visitBinaryExpression (destructuring assignments, FlattenLevel::All, needs-value = !expressionResultIsUnused), visitVariableStatement converted-loop arm (flatten ON the declaration), visitVariableDeclaration (+exported-variable last-value arm parameter), convertForOfStatementHead pattern arm, visitCatchClause pattern arm, insertDefaultValueAssignmentForBindingPattern, addRestParameterIfNeeded pattern arm |
| name generation | `_this`/`_newTarget`/`_super` via `TargetBinding::allocate_file_level_optimistic_reserved_in_nested_scopes`; `_loop`/`_loop_init`/`out_<x>`/`state`/`inc`/`this`/`arguments`/`e` via `TargetBinding::allocate_numbered`; `_i` via the dormant `GeneratedBindingScopes::allocate_loop_variable` (first production caller); temps via `TargetBinding::allocate` (+`context.hoist_variable_declaration` when recorded); `getGeneratedNameForNode` via `allocate_source_numbered_for_node` keyed by the parse-tree node identity (first production caller); every generated identifier is minted eagerly per E-NAMES-H and finalized in document order — loop-body functions carry `EmitFlags::REUSE_TEMP_VARIABLE_SCOPE` so the B-3 finalize arm (target_bindings.rs:581-586) keeps the shared temp alphabet, matching the upstream one-scope emission. The `_this`/`_newTarget`/`_super` mints are ONE shared `TargetBinding` per source file (lazily allocated at the first mint site; the transformer keeps them for print-time substitution clones): upstream mints a fresh optimistic instance per site and the printer's name generator converges same-text file-level-optimistic instances to one spelling — measured live incl. the collision spellings (`captured_this_collision` → every site `_this_1`; `synthetic_super_collision` → every site `_super_1`; `captured_this_two_fns` → `_this` in both functions), which the probe fixtures freeze as oracle bytes |
| getLocalName/getInternalName | module-internal ports of the pinned `getName` worker + wrappers (§4.2: 24788-24811): clone the identifier name with range threading, inherit its emit flags, add `NoSourceMap`/`NoComments` per the allow params, stamp `EmitFlags::LOCAL_NAME`(&#124;`INTERNAL_NAME`) (metadata.rs:34-36; the module-transformer idiom builtins.rs:777/8085), and FALL BACK to `get_generated_name_for_node(node)` for missing/generated/non-identifier names (the anonymous class expression); `isInternalName` (substitution skip) reads the INTERNAL_NAME flag; the keyword-guard arm (`isIdentifierANonContextualKeyword(name) → getGeneratedNameForNode(name)`) uses the numbered node-keyed cache |
| helper identifiers | `EmitHelperName::{Extends, SpreadArray}` variants + `text()` arms `"__extends"`/`"__spreadArray"` (factory.rs:19-45); calls built as `request_emit_helper(helpers::extends()/spread_array()/values()/read())` + `create_unscoped_helper_identifier` (es2017.rs create_awaiter_call template; B-3 generators.rs precedent) |
| node construction | module-internal `create_*` wrappers over `self.context().factory()?.create_node(source, NodeData::…, flags)` with flags from `TransformArena::propagate_child_flags`/child-fold (EA-GAP-FLAGS discipline); `this`/`super` are TOKENS (`create_token(source, ThisKeyword/SuperKeyword, …)`, es2017.rs:679 precedent); constructors beyond B-3's set: class expression/declaration output shapes (function expressions, parameter lists, property/element access, partially-emitted expressions), for-of protocol shapes, defineProperty descriptors, template concat chains, meta-property replacement identifiers |
| provenance & ranges | `set_text_range`/`set_original_node` exactly as B-2/B-3; `setSourceMapRange` → `EmitMetadata::set_source_map_range`; `setTokenSourceMapRange` → `set_token_source_map_range` (metadata.rs:547 — the close-brace arm of arrow blocks and class bodies); `setCommentRange` → `set_comment_range`; `addSyntheticLeadingComment("* @class ")` → `add_leading_comment(SyntheticComment::new(MultiLine, "* @class ", …))`; `moveSyntheticComments` → the NEW `TransformArena::move_synthetic_comments` finalize-surface (§12.6) |
| substitution | `Es2015Transformer::substitute_node` — Expression hint → `substituteExpression` (identifier/this arms); otherwise identifier-typed declaration-name substitution (`substituteIdentifier`); previous-first chaining is the harness's job (hook_chaining contracts); `context.enable_substitution(Identifier)` at `enableSubstitutionsForBlockScopedBindings`, `enable_substitution(ThisKeyword)` + `enable_emit_notification(×7 function kinds)` at `enableSubstitutionsForCapturedThis`; substitution clones minted via `context.substitution_factory()` + range threading (generators.rs:6222 precedent) |
| prologue machinery | re-ported `copy_prologue`/`copy_standard_prologue`/`copy_custom_prologue` (WITH the filter arms: hoisted functions, hoisted variable statements — the two-phase `transformFunctionBody` copies) + `insert_statement(s)_after_custom_prologue` + `insert_statements_after_standard_prologue` (es2017 prologue_end cursor precedent) + the FULL `merge_lexical_environment` port (24889-24932: three-span splice + directive dedup) — module-internal; es2017's private copy untouched |
| parsed-tree ES2015 facets | `crates/emitter/src/builtins.rs` `local_transform_flags`/`local_contextual_target_flags` gain the missing ES2015 facet arms: TemplateExpression + NoSubstitutionTemplateLiteral + TaggedTemplateExpression (ES2015), ComputedPropertyName (ES2015\|COMPUTED_PROPERTY_NAME), ShorthandPropertyAssignment (ES2015), destructuring-assignment ES2015 bits for BOTH pattern kinds (the array arm is entirely missing today; the object arm lacks the ES2015 bit), MetaProperty `new.target` (ES2015), binary/octal NumericLiteral + extended-unicode StringLiteral (ES2015), extended-unicode-escape Identifier (ES2015 — the vendored `createIdentifier` row `_tsc.js:21621-21623` over `NodeFlags::IDENTIFIER_HAS_EXTENDED_UNICODE_ESCAPE = 256`; the classifier's Identifier arm today handles only `"await"`), StaticKeyword token (ES2015) — each mirroring the vendored factory row already ported in `classify_created_node_flags` (factory.rs:747-1024); the classifier itself gains the missing MetaProperty arm AND the Identifier extended-unicode arm with their vendored citations. Corpus-inert: zero ACTIVE readers of `CONTAINS_ES_2015` (§12.4 measurement); the ratchet is the enforcement |
| tagged-template seam | `visit_tagged_template_expression` ports the owner fn faithfully as a call into module-internal `process_tagged_template_expression_seam(…) -> Err(TransformError::…)` — a typed fail-closed seam with the pinned B-5 justification; `record_tagged_template_string` and the source-file tail that emits the declarations list port faithfully (dormant until B-5 replaces the seam with the shared-module call) |
| TS class wrapper | `visit_type_script_class_wrapper` full port (`:107687-107783` IIFE surgery incl. `classWrapperStatementVisitor` static-initializer facts arm and the `restoreOuterExpressions` rebuild); reachable only from TS-transformer-marked input (`InternalEmitFlags::TYPE_SCRIPT_CLASS_WRAPPER`), driven by hand-built wrapper trees in unit contracts (§12.5) |
| comment/printer surfaces | untouched: synthesized output only, no comment-scope threading change, no printer edit (synthetic leading comments on parenthesized expressions, SingleLine/starts-on-new-line/multi-line node records, and token source-map ranges are measured present — CS-6 contracts + printer.rs:1023-1405/8884-9128; §12.7) |

Producer/owner/consumer per row: the module is the sole producer of
its synthesized subtrees; `TransformationContext` owns hoisted
declarations and helper requests; the converted-loop, hierarchy-fact,
and substitution records live as §5 rows above (per-root visitor
state vs transformer-lifetime print state); invalidation follows the
arena (nodes immutable once created — B-4 needs NO finalize-write
beyond the E-NAMES machinery and the §12.6 synthetic-comment move,
which relocates metadata, not node records).

## 6. Current local-gap matrix (B-4 row, from the frozen artifact)

| Capability | State | Anchor evidence | Absence evidence |
|---|---|---|---|
| `loop-conversion-capture` | `missing` | — | symbol `converted_loop` asserted absent from `crates/emitter/src/builtins.rs` (owner-relative absence, file sha re-measured at mint); requirement: "converted-loop extraction with captured block-scoped bindings, out-parameters, this/arguments/new.target capture, and yield* re-emission for generator-containing bodies"; surfaces `loop-partition-machinery`, `yield-star-synthesis`; architecture rows `EA-GAP-CAPTURE`, `E-CAPTURE-BASE` |

Landing the loop-conversion machinery (the `converted_loop` symbol
family lives in the new `es2015.rs`, reached from `builtins.rs` via
the `mod es2015;` line) trips the absence exactly as the generator
intends; the §8.1 amendment is that reviewed re-disposition (row 9
`missing` → `exists`, counts 11/0/2 → 12/0/1). Row 12
(`tagged-template-lowering`) is deliberately untouched.

## 7. Implementation sequence (dependency order; every step corpus-inert)

Fence: `crates/emitter/src/builtins/es2015.rs` (new),
`crates/emitter/src/builtins.rs` (the `mod es2015;` line + the §5
parsed-tree ES2015 facet arms ONLY; the target rejection at :147-150
is read-only), `crates/emitter/src/factory.rs` (the two
`EmitHelperName` variants + `text()` arms + the classifier
MetaProperty arm + the §12.6 `move_synthetic_comments` arena surface
ONLY), `crates/emitter/src/metadata.rs` (the §12.6 synthetic-comment
take/append accessors ONLY, if the move cannot be expressed through
existing surfaces — implementation measured: NOT needed, the move
rides the existing pub(crate) comment lists through the new factory
surface), `crates/emitter/src/printer.rs` (the §12.11
`emitObjectLiteralExpression` Indented arm ONLY — a mid-train
amendment under the design-gate amend rule),
`crates/emitter/tests/unit/es2015/tests.rs` (new,
attached with the `#[cfg(test)] #[path]` idiom), and the §8 evidence
set. `generators.rs`, `flatten_destructuring.rs`, `es2017.rs`,
`es2018.rs`, `helpers.rs`, `printer.rs`, `transform.rs`,
`resolver.rs`, and every other production file are out of fence.

1. **Leaves.** `HierarchyFacts` + the §4.3 alphabets;
   enter/exit-subtree; the §4.2 addenda (new pins + re-pins);
   `EmitHelperName::{Extends, SpreadArray}`; the parsed-tree facet
   arms + classifier MetaProperty arm; the resolver wrapper fns; the
   synthetic-comment move surface.
   Check: leaf unit contracts green (facts mask algebra, addenda
   behavior rows: spanMap runs, mergeLexicalEnvironment splice +
   directive dedup, restoreOuterExpressions/isIgnorableParen,
   insert-after-prologue cursors, packed-array predicates,
   rangeEndIsOnSameLineAsRangeStart over the parse line map);
   `cargo xtask ledger check` stale=0 undispositioned=0 (headers +
   disposition markers at authoring time — the B-1 lesson).
2. **Visitor core.** Dispatch (`visitorWorker` 50-kind switch +
   `shouldVisitNode` gate + `visitor`/`visitorWithUnusedExpressionResult`/
   `callExpressionVisitor`/`classWrapperStatementVisitor`),
   `transform_root` (source-file gate: DECLARATION FILES ONLY —
   upstream `transformSourceFile` `:104768-104781` has no transform-flag
   gate; the per-node `shouldVisitNode` owns every flag/forced arm, and
   `visitSourceFile` always runs), `visitSourceFile`,
   functions/arrows (`visitArrowFunction`, `visitFunctionExpression`,
   `visitFunctionDeclaration`, `transformFunctionLikeToExpression`,
   `transformFunctionBody`), parameters (`visitParameter`,
   `addDefaultValueAssignmentsIfNeeded2`,
   `insertDefaultValueAssignmentForBindingPattern`/`ForInitializer`,
   `addRestParameterIfNeeded`), this/new.target capture
   (`insertCaptureThisForNode(IfNeeded)`,
   `insertCaptureNewTargetIfNeeded`, `visitThisKeyword`,
   `visitMetaProperty`, `visitIdentifier`), block/statement family
   (`visitBlock`, `visitExpressionStatement`,
   `visitParenthesizedExpression`, `visitBinaryExpression`,
   `visitCommaListExpression`, `visitVariableStatement`,
   `visitVariableDeclarationList` + let-initializer rule,
   `visitVariableDeclaration`, `visitSwitchStatement`,
   `visitCaseBlock`, `visitReturnStatement`, `visitVoidExpression`,
   `visitBreakOrContinueStatement`, `visitLabeledStatement`,
   `visitCatchClause`, `addStatementToStartOfBlock`), literal family
   (template/string/numeric normalizations), super/meta
   (`visitSuperKeyword`, `createSyntheticSuper`), the FlattenHost
   impl + destructuring call sites, spread family
   (`visitArrayLiteralExpression`, `visitCallExpression`,
   `visitCallExpressionWithPotentialCapturedThisAssignment`,
   `visitNewExpression`, `transformAndSpreadElements` + span
   walkers, `visitSpreadElement`), object-literal family
   (`visitObjectLiteralExpression`, `addObjectLiteralMembers`,
   `transformPropertyAssignmentToExpression`,
   `transformShorthandPropertyAssignmentToExpression`,
   `transformObjectLiteralMethodDeclarationToExpression`,
   `visitMethodDeclaration`, `visitAccessorDeclaration`,
   `visitShorthandPropertyAssignment`, `visitComputedPropertyName`,
   `visitYieldExpression`), the tagged-template fail-closed seam.
   Register `mod es2015;`. Check: `cargo build -p tsc-rs-emitter` +
   clippy clean; dispatch/facts protocol unit contracts green.
3. **Class lanes.** `visitClassDeclaration`/`visitClassExpression`,
   `transformClassLikeDeclarationToExpression`, `transformClassBody`,
   `addExtendsHelperIfNeeded`, `addConstructor`,
   `transformConstructorParameters`, `createDefaultConstructorBody`,
   `transformConstructorBody`, `containsSuperCall`,
   `isSufficientlyCoveredByReturnStatements`, the
   `simplifyConstructor` family (all 13 predicate/rewrite fns),
   `createActualThis`, `createDefaultSuperCallOrThis`,
   `addClassMembers` + member transforms + accessors,
   `getClassMemberPrefix`, `hasSynthesizedDefaultSuperCall`,
   `visitTypeScriptClassWrapper` + wrapper statement visitor.
   Check: class unit contracts (wrapper surgery on hand-built
   trees, keyword-name guard, member fault arm).
4. **Loop conversion.** The §4.3 converted-loop protocol:
   `visitIterationStatement(WithFacts)`, `visitDoOrWhileStatement`,
   `visitForStatement`, `visitEachChildOfForStatement2`,
   `visitForInStatement`, `visitForOfStatement`,
   `convertForOfStatementHead`,
   `createSyntheticBlockForConvertedStatements`,
   `convertForOfStatementForArray`,
   `convertForOfStatementForIterable`, `shouldConvert*` (5),
   `hoistVariableDeclarationDeclaredInConvertedLoop`,
   `convertIterationStatementBodyIfNecessary`,
   `convertIterationStatementCore` + the five per-kind converts,
   `createConvertedLoopState`, `addExtraDeclarationsForConvertedLoop`,
   `createOutVariable`, `createFunctionForInitializerOfForStatement`,
   `createFunctionForBodyOfIterationStatement`, `copyOutParameter(s)`,
   `generateCallToConvertedLoopInitializer`,
   `generateCallToConvertedLoop` (BOTH `yield*` synthesis sites with
   the `EmitFlags::ITERATOR` stamp), `setLabeledJump`,
   `processLabeledJumps`, `processLoopVariableDeclaration`,
   `recordLabel`/`resetLabel`. Hooks: `onEmitNode` decomposition +
   `enableSubstitutionsFor*` + the three substitution fns +
   `isNameOfDeclarationWithCollidingName` + `isPartOfClassBody`.
   Check: protocol unit contracts (jump-marker table, out-param
   flag algebra, label propagation).
5. **Focused projections.** The suite
   `crates/emitter/tests/unit/es2015/tests.rs` drives the REAL
   transformer chain per fixture — parse →
   `transform_nodes(arena, [SourceFile], [transform_es2015(&options,
   &FixtureResolver), transform_generators(ES5, &FixtureResolver)],
   …)` → `create_printer(PrinterOptions::new(LF).with_target(ES5))`
   → byte-compare against the frozen oracle emits (full output bytes
   INCLUDING helper preludes; the B-3 precedent). `FixtureResolver`
   is the §12.2 mini lexical binder; per-case options
   `downlevel_iteration`/`use_define_for_class_fields` mirror the
   probe. **Fixture-language constraint (§12.3):** script files only
   (no import/export), full ES2015 minus everything a pass BEFORE
   transformES2015 lowers and minus tagged templates — no `**`,
   async, object rest/spread, optional catch binding, optional
   chaining, nullish coalescing, logical assignment, class fields /
   static blocks / private names / decorators, BigInt, `with`, no
   user binding named `arguments`, no `eval`. Generators ARE allowed
   (the joint chain is the projection). The 123 oracle fixtures
   (exact sources; expected bytes frozen in the suite from the probe
   output; multi-line sources notate newlines as `⏎`; cases marked
   `[DI]` run with downlevelIteration:true, `[UDCF]` with
   useDefineForClassFields:true):
   - `let_basic` — `let a = 1;⏎use(a);`
   - `let_no_init_toplevel` — `let a;⏎use(a);`
   - `let_explicit_init_block` — `function f() { { let a; use(a); } }`
   - `let_block_collision` — `{ let x = 1; use(x); }⏎var x = 2;`
   - `let_two_blocks` — `{ let x = 1; a(x); }⏎{ let x = 2; b(x); }`
   - `let_shadow_fn` — `let x = 1;⏎function f() { let x = 2; use(x); }⏎use(x, f);`
   - `const_basic` — `const c = compute();⏎use(c);`
   - `let_in_switch_case` — `switch (t) { case 1: let v = one(); use(v); break; default: other(); }`
   - `let_for_in` — `for (let k in o) { use(k); }`
   - `let_loop_no_capture` — `for (let i = 0; i < n; i++) { use(i); }`
   - `loop_capture_basic` — `for (let i = 0; i < n; i++) { sink(function () { return i; }); }`
   - `loop_capture_out_param` — `for (let i = 0; i < n; i++) { sink(function () { return i; }); i = step(i); }`
   - `loop_capture_break` — `for (let i = 0; i < n; i++) { sink(function () { return i; }); if (c) break; }`
   - `loop_capture_continue` — `for (let i = 0; i < n; i++) { sink(function () { return i; }); if (c) continue; tail(i); }`
   - `loop_capture_return` — `function f() { for (let i = 0; i < n; i++) { sink(function () { return i; }); if (c) return i; } return fallback(); }`
   - `loop_capture_return_void` — `function f() { for (let i = 0; i < n; i++) { sink(function () { return i; }); if (c) return; } tail(); }`
   - `loop_capture_labeled_break` — `outer: for (let i = 0; i < n; i++) { for (let j = 0; j < m; j++) { sink(function () { return i + j; }); if (c) break outer; } }`
   - `loop_capture_labeled_continue` — `outer: for (let i = 0; i < n; i++) { inner: for (let j = 0; j < m; j++) { sink(function () { return j; }); if (c) continue outer; } }`
   - `loop_capture_this` — `function f() { for (let i = 0; i < n; i++) { sink(function () { return i; }); use(this); } }`
   - `loop_capture_this_arrow` — `function f() { for (let i = 0; i < n; i++) { sink(() => i + this.x); } }`
   - `loop_capture_arguments` — `function f() { for (let i = 0; i < n; i++) { sink(function () { return i; }); use(arguments[0]); } }`
   - `loop_capture_while` — `while (c()) { let v = p(); sink(function () { return v; }); }`
   - `loop_capture_do` — `do { let v = p(); sink(function () { return v; }); } while (c());`
   - `loop_capture_for_in` — `for (var k in o) { let v = o[k]; sink(function () { return v; }); }`
   - `loop_capture_for_of` — `for (var e of xs) { let v = e; sink(function () { return v; }); }`
   - `loop_capture_var_hoist` — `for (let i = 0; i < n; i++) { var t = q(i); sink(function () { return i; }); } use(t);`
   - `loop_capture_var_destr` — `for (let i = 0; i < n; i++) { var [p, q2] = pair(i); sink(function () { return i; }); } use(p, q2);`
   - `loop_capture_nested_return` — `function f() { for (let i = 0; i < n; i++) { for (let j = 0; j < m; j++) { sink(function () { return i + j; }); if (c) return j; } } }`
   - `loop_init_conversion` — `for (let seed = (sink(function () { return seed; }), 0), index = 0; index < n; index++) { body(seed, index); }`
   - `loop_cond_conversion` — `for (let i = 0; check(function () { return i; }); i++) { body(i); }`
   - `loop_incr_conversion` — `for (let i = 0; i < n; i = bump(function () { return i; })) { body(i); }`
   - `yield_star_body_site` — `function* g() { for (let i = 0; i < n; i++) { sink(function () { return i; }); yield i; } }`
   - `yield_star_init_site` — `function* g() { for (let seed = (sink(function () { return seed; }), yield 1), index = 0; index < n; index++) { body(seed, index); } }`
   - `loop_capture_switch_break` — `for (let i = 0; i < n; i++) { sink(function () { return i; }); switch (t(i)) { case 1: break; default: other(); } }`
   - `class_basic` — `class C {⏎  m() { return 1; }⏎}⏎use(C);`
   - `class_ctor` — `class C {⏎  constructor(a, b) { this.a = a; this.b = b; }⏎}⏎use(C);`
   - `class_extends` — `class D extends B {⏎  constructor(x) { super(x); this.x = x; }⏎  m() { return this.x; }⏎}⏎use(D);`
   - `class_extends_default_ctor` — `class D extends B {⏎  m() { return 2; }⏎}⏎use(D);`
   - `class_extends_null` — `class D extends null {⏎}⏎use(D);`
   - `class_methods_static` — `class C {⏎  m() { return 1; }⏎  static s() { return 2; }⏎}⏎use(C);`
   - `class_accessors` — `class C {⏎  get p() { return this._p; }⏎  set p(v) { this._p = v; }⏎  static get sp() { return 1; }⏎}⏎use(C);`
   - `class_expr_named` — `var E = class Name {⏎  m() { return Name; }⏎};⏎use(E);`
   - `class_expr_anon` — `use(class {⏎  m() { return 3; }⏎});`
   - `class_semicolon_element` — `class C {⏎  ;⏎  m() { return 1; }⏎}⏎use(C);`
   - `class_super_property` — `class D extends B {⏎  m() { return super.m() + super.p; }⏎}⏎use(D);`
   - `class_static_super_property` — `class D extends B {⏎  static s() { return super.s(); }⏎}⏎use(D);`
   - `class_new_target_ctor` — `class C {⏎  constructor() { use(new.target); }⏎}⏎use(C);`
   - `class_derived_return_this` — `class D extends B {⏎  constructor() { super(); if (c) { return; } tail(); }⏎}⏎use(D);`
   - `class_derived_super_tail` — `class D extends B {⏎  constructor() { effect(); super(a1()); }⏎}⏎use(D);`
   - `class_generator_method` — `class C {⏎  *m() { yield 1; }⏎}⏎use(C);`
   - `class_method_default_param` — `class C {⏎  m(a = 1) { return a; }⏎}⏎use(C);`
   - `class_udcf_method` [UDCF] — `class C {⏎  m() { return 1; }⏎}⏎use(C);`
   - `fn_new_target` — `function f() { return new.target; }⏎use(f);`
   - `fn_expr_named_new_target` — `var f = function named() { return new.target; };⏎use(f);`
   - `arrow_basic` — `var f = (a) => a + 1;⏎use(f);`
   - `arrow_multiline` — `var f = (a) =>⏎  a + 1;⏎use(f);`
   - `arrow_block_body` — `var f = (a) => { const b = a + 1; return b; };⏎use(f);`
   - `arrow_this_fn` — `function f() { var g = () => this.x; return g; }⏎use(f);`
   - `arrow_this_toplevel` — `var g = () => this;⏎use(g);`
   - `arrow_nested_this` — `function f() { var g = () => () => this.x; return g; }⏎use(f);`
   - `arrow_default_param` — `var f = (a = seed()) => a;⏎use(f);`
   - `arrow_in_method_super` — `class D extends B {⏎  m() { return () => super.m(); }⏎}⏎use(D);`
   - `param_default` — `function f(a, b = a + 1) { return b; }⏎use(f);`
   - `param_default_two` — `function f(a = one(), b = two(a)) { return a + b; }⏎use(f);`
   - `param_rest` — `function f(a, ...rest) { return rest.length + a; }⏎use(f);`
   - `param_rest_only` — `function f(...xs) { return xs; }⏎use(f);`
   - `param_pattern_array` — `function f([a, b]) { return a + b; }⏎use(f);`
   - `param_pattern_object_default` — `function f({ x = 1 } = {}) { return x; }⏎use(f);`
   - `param_pattern_rest_mix` — `function f(a = 0, [b] = [1], ...rest) { return a + b + rest.length; }⏎use(f);`
   - `destr_var_array` — `var [a, b] = pair();⏎use(a, b);`
   - `destr_var_object` — `var { x, y: z } = o;⏎use(x, z);`
   - `destr_assignment` — `var a, b;⏎[a, b] = pair();⏎use(a, b);`
   - `destr_assignment_expr_value` — `var a, b;⏎use([a, b] = pair());`
   - `destr_let_block` — `{ let [a, b] = pair(); use(a, b); }⏎var a = 1;⏎use(a);`
   - `destr_for_of_pattern` — `for (var [k, v] of pairs) { use(k, v); }`
   - `destr_catch` — `try { t(); } catch ({ message }) { use(message); }`
   - `destr_nested_defaults` — `var { a: { b = one() } = {} } = o;⏎use(b);`
   - `spread_call` — `f(a, ...xs, b);`
   - `spread_call_only` — `f(...xs);`
   - `spread_new` — `var r = new C(...xs);⏎use(r);`
   - `spread_array` — `var r = [1, ...xs, 2];⏎use(r);`
   - `spread_array_literal_seg` — `var r = [...[1, 2], ...xs];⏎use(r);`
   - `spread_array_downlevel` [DI] — `var r = [1, ...xs, 2];⏎use(r);`
   - `spread_call_downlevel` [DI] — `f(a, ...xs);`
   - `spread_method_call` — `o.m(a, ...xs);`
   - `spread_elem_call` — `o[k()](...xs);`
   - `spread_super_call` — `class D extends B {⏎  constructor() { super(...args()); }⏎}⏎use(D);`
   - `template_basic` — `var s = `a${x}b`;⏎use(s);`
   - `template_expr_only` — `var s = `${x}`;⏎use(s);`
   - `template_no_subst` — `var s = `plain`;⏎use(s);`
   - `template_multi_span` — `var s = `a${x}${y}b${z}`;⏎use(s);`
   - `forof_array_basic` — `for (var v of xs) { use(v); }`
   - `forof_array_expr` — `for (var v of make()) { use(v); }`
   - `forof_let_head` — `for (let v of xs) { use(v); }`
   - `forof_assign_head` — `var v;⏎for (v of xs) { use(v); }`
   - `forof_iterable` [DI] — `for (var v of xs) { use(v); }`
   - `forof_iterable_nested` [DI] — `for (var a of xs) { for (var b of ys) { use(a, b); } }`
   - `forof_labeled_break` — `outer: for (var v of xs) { if (c(v)) break outer; use(v); }`
   - `forof_capture_body` [DI] — `for (var e of xs) { let v = e; sink(function () { return v; }); }`
   - `obj_computed_first` — `var o = { [k()]: 1, b: 2 };⏎use(o);`
   - `obj_computed_middle` — `var o = { a: 1, [k()]: 2, c: 3 };⏎use(o);`
   - `obj_computed_multiline` — `var o = {⏎  a: 1,⏎  [k()]: 2,⏎  c: 3⏎};⏎use(o);`
   - `obj_shorthand` — `var o = { a, b };⏎use(o);`
   - `obj_method` — `var o = { m() { return 1; } };⏎use(o);`
   - `obj_generator_method` — `var o = { *m() { yield 1; } };⏎use(o);`
   - `obj_accessor_computed_mix` — `var o = { get p() { return 1; }, [k()]: 2, set p(v) { s(v); } };⏎use(o);`
   - `obj_computed_shorthand_method_mix` — `var o = { [k()]: 1, s: a, m() { return 2; } };⏎use(o);`
   - `string_extended_unicode` — `var s = "\u{1F600}";⏎use(s);`
   - `numeric_binary_octal` — `var n = 0b101 + 0o17;⏎use(n);`
   - `ident_unicode_escape` — `var \u{61}b = 1;⏎use(ab);`
   - `switch_in_converted_loop` — `for (let i = 0; i < n; i++) { sink(function () { return i; }); switch (i) { case 1: break; } }`
   - `labeled_nonloop` — `lbl: { work(); break lbl; }⏎tail();`
   - `comma_unused_result` — `a(), b();`
   - `captured_this_collision` — `var _this = 1;⏎function f() { var g = () => this.x; return [g, _this]; }⏎use(f);`
   - `captured_this_two_fns` — `function f() { var g = () => this.x; return g; }⏎function h() { var g = () => this.y; return g; }⏎use(f, h);`
   - `synthetic_super_collision` — `var _super = 1;⏎class D extends B {⏎  m() { return super.m() + _super; }⏎}⏎use(D);`
   - `loop_capture_labeled_break_mid` — `outer: for (let i = 0; i < n; i++) { mid: for (let j = 0; j < m; j++) { sink(function () { return i + j; }); if (c) break mid; } }`
   - `let_uninit_captured_in_loop` — `for (let i = 0; i < n; i++) { let v; sink(function () { return v; }); v = q(i); }`
   - `let_uninit_colliding_block` — `{ let x; use(x); }⏎var x = 1;⏎use(x);`
   - `let_uninit_colliding_in_loop` — `for (let i = 0; i < n; i++) { let x; use(x); x = i; }⏎var x = 9;⏎use(x);`
   - `forof_assign_destr_head` — `var a, b;⏎for ([a, b] of pairs) { use(a, b); }`
   - `spread_array_trailing_comma` — `var r = [1, ...xs, 2, ];⏎use(r);`
   - `spread_new_prop_callee` — `var r = new o.C(...xs);⏎use(r);`
   Fault-shaped typed-error contracts (not oracle-mintable): a
   tagged-template expression reaching the B-5 seam (typed
   `TransformError`, not a crash); a class PropertyDeclaration
   reaching `addClassMembers` (upstream `Debug.failBadSyntaxKind`);
   a private-identifier accessor name reaching
   `transformAccessorsToExpression` (upstream `failBadSyntaxKind`).
   Composition-shielded arms carry direct unit contracts instead of
   oracle projections (§12.5): TS class wrapper surgery, export
   lanes (`createExportDefault`/`createExternalModuleExport`
   shapes), the AsyncFunctionBody-gated loop arms, the
   StaticInitializer facts arm. The exact probe (frozen; §7.3
   command): `b4-probe.mjs` — the identical emitCase host to the
   B-3 §7.3 probe (single virtual file, fresh-process
   `ts.createProgram`, ES5, alwaysStrict:false, LF) extended with
   the per-case `downlevelIteration`/`useDefineForClassFields`
   options. Oracle bytes are the entire expectation (no
   hand-authored output). Check: focused suite green (123 byte-equal
   projections + the typed fault contracts + unit contracts);
   `cargo test -p tsc-rs-emitter` fully green with zero
   expected-string changes outside the new suite.
6. **Train items.** §8 amendments, chain walk (b4-walk.sh = the B-3
   walk with this scratchpad's path; qualification BEFORE profile;
   walk BEFORE `--lane rust`; ONE `cargo fmt` + `cargo clippy
   --tests` formatting pass lands BEFORE the first walk — the B-3
   three-walk lesson; NEW_RUNTIME_INPUTS closure for the two new
   crates files (245→247) + the generator-internal size consts +
   schema minItems/maxItems + gap-matrix schema summary consts),
   envelope `h2-5h-b-b-4` (`ready`, predecessor `h2-5h-b-b-3`),
   bootstrap `allowedPacketIds += h2-5h-b-b-4`, index row, full
   local gate at the final head from the canonical repository path
   (detached launcher; demoted; perf-only-red → normal-priority
   resume per protocol).

## 8. Evidence, ratchet, and documentation amendments

1. **Gap matrix generator** (`crates/oracle/h2-5h-a-gap-matrix.mjs`
   row 9): `state: "missing"` → `"exists"`; anchors gain
   `crates/emitter/src/builtins/es2015.rs` `transform_es2015` /
   `Es2015Transformer` / `convert_iteration_statement_body_if_necessary`
   (real symbols — the F1 lesson); the `converted_loop` absence
   retires (`absences: []`); note records the landing, dormancy, and
   the B-5 remaining scope (tagged template + registration); summary
   counts 11/0/2 → 12/0/1 (the SCHEMA pins the summary counts as
   consts — both files change together, the B-2 lesson). Row 12
   untouched.
2. **Dispositions generator**
   (`crates/oracle/h2-5h-a-dispositions.mjs`): rationale rows citing
   the `loop-conversion-capture` capability gain the B-4 landing
   clause (dispositions themselves unchanged); the manifest re-mints
   with the new gap-matrix lineage.
3. **Architecture map**
   (`docs/design/greenfield/emitter-architecture.md`): the
   `EA-GAP-CAPTURE` (and, where it cites the composition ladder,
   `EA-GAP-COMPOSITION`) section text extends with the B-4 owner
   landing (module, transformer seam, dormancy, remaining B-5
   scope); NO heading or table-row identity changes.
4. **Handoff** `h2-5h-a.md`: the ladder's B-4 bullet gains its
   **LANDED** marker at the final implementation-step commit ⇒
   envelope `h2-5h-a` re-pin + doc-pinning witness re-mints
   (adoption: seconds).
5. **Chain walk**: b4-walk.sh (the B-3 walk verbatim, scratchpad
   path updated); qualification BEFORE profile; the new-crates-file
   closure (NEW_RUNTIME_INPUTS mjs list + schema minItems/maxItems
   + the generator-internal runtimeInputSet.size consts, 245→247);
   the walk re-mints foundation/comment-scope witnesses (adoption),
   owner-graph, gap-matrix, dispositions, es2015-generators
   witnesses; pin-sweep audit before the gate after any
   multi-attempt walk; ONE formatting pass (`cargo fmt` + clippy
   incl. `--tests`) BEFORE the first walk (the B-3 lesson — every
   fmt/clippy repair re-touches crate bytes and re-mints the h1
   ladder).
6. **Readiness**: envelope
   `ratchets/fci-readiness/h2-5h-b-b-4.v1.json` (`ready`; fence =
   §7 + the walk set; predecessor `h2-5h-b-b-3` with its receipt
   sha), bootstrap `allowedPacketIds += h2-5h-b-b-4`, index row in
   `slices/README.md`.

## 9. Acceptance

- All 171 owner functions + the §4.2 addenda landed with ledger
  headers; `cargo xtask ledger check` green (stale=0,
  undispositioned=0, todo_port=0).
- Focused projection suite green: all 123 fixtures byte-equal to
  their oracle-minted expectations through the REAL
  `[transform_es2015, transform_generators]` chain (let/const +
  collision renames, loop conversion incl. BOTH `yield*` synthesis
  sites and out-param/label/return protocols, class lanes incl.
  extends/super/accessors/simplify pipeline, arrows + this capture,
  parameters (default/rest/pattern), destructuring composition
  through the B-2 flattener, spread in both iteration modes,
  template expressions, for-of array + iterable, object-literal
  chunking, literal normalizations), typed fault contracts green,
  composition-shielded unit contracts green, provenance contracts
  (original/range/SYNTHESIZED) green.
- `cargo test -p tsc-rs-emitter` fully green; zero expected-string
  changes outside the new focused suite.
- Gap matrix re-minted: row 9 `exists`, counts 12/0/1; dispositions
  + owner-graph + witnesses re-minted through the walk; architecture
  map and handoff amended per §8.
- Corpus ratchet: T0=100.0000% 49024/49024 FP=0, all bands, tiers —
  byte-identical (foundation packet; zero output change; the
  parsed-tree facet arms, the classifier MetaProperty arm, the two
  EmitHelperName variants, and the synthetic-comment move surface
  are corpus-inert by the §12.4/§12.6 measured arguments and the
  ratchet is the enforcement).
- Packet checker `slice-readiness --check h2-5h-b-b-4`; complete
  local gate green at the final head from the canonical path.

## 10. Traceability

| Invariant | Target | Test | Evidence |
|---|---|---|---|
| owner fidelity (171 fns) | `es2015.rs` ports | ledger d2 headers + protocol contracts | §4.1 spans/hashes + owner-graph byte hashes |
| yield-star producer edge | both synthesis sites + `EmitFlags::ITERATOR` stamp | `yield_star_body_site`/`yield_star_init_site` byte projections through B-3's machine | owner-graph `yield-star-synthesis` edge (offsets 101107/101796) |
| loop-conversion protocol | converted-loop state machine + resolver gates | loop_capture_* / loop_init/cond/incr projections | §4.3 converted-loop pins + checker-rule pins |
| resolver-driven renames | substitution fns + mini-binder answers | collision/rename projections (bytes carry the names) | §12.2 self-checking argument |
| class lowering | IIFE + constructor pipeline + members | class_* projections + wrapper/keyword unit contracts | §4.3 class pins |
| this/arguments/new.target capture | capture insertion + print-time facts | arrow_this_* / loop_capture_this / *_new_target projections + onEmitNode facts contract | §4.3 print-time pin |
| parameters | default/rest/pattern prologue protocol | param_* projections | §4.3 parameter pins |
| destructuring composition | FlattenHost impl | destr_* projections | B-2 flattener contract + §5 row |
| spread/apply shapes | segment protocol + call binding | spread_* projections (both DI modes) | §4.3 spread pin |
| helper graph | 4 helper requests + 2 new identifier variants | projection preludes byte-compared | E-HELPERS rows |
| parsed-tree ES2015 facets | initializer/classifier arms | facet unit contracts + the source-file gate projections | vendored factory rows + §12.4 measurement |
| dormancy | no registration edit; `builtins.rs` mod line + facet arms only | untouched-file assert + ratchet | corpus byte-identity |
| consumer-surface completeness | `Transformer` impl (all four hooks) | hook_chaining order contracts (frozen B-1) + compile-time | §5 module seam |

Resources: the focused suite is plain `cargo test` (no worker
ceilings); the walk and gate follow the standing demotion directive
(`taskpolicy` maintenance/`nice`), with the perf-ceiling
normal-priority resume exception.

## 11. Prohibitions

No transformer registration or activation change; no corpus
output-byte change (the ratchet is the enforcement); no printer edit
beyond the single pinned §12.11 Indented arm (a mid-train amendment;
every other printer surface untouched); no
helpers.rs, transform.rs, resolver.rs, generators.rs,
flatten_destructuring.rs, es2017.rs, or es2018.rs edit; no
tagged-template module (`tagged_template.rs` stays absent — gap row
12 is B-5's flip); no `__makeTemplateObject` helper text; no witness
amendment; no ad-hoc in-place node mutation (B-4 needs no
finalize-write beyond E-NAMES; the synthetic-comment move relocates
METADATA through the typed arena surface only); no generic fallback
that converts an unknown branch into success (every unexpected shape
is a typed `TransformError`); no fixture-specific branches or
hand-authored expected output (oracle bytes only); the CS and
B-1/B-2/B-3 prohibitions remain. This document authorizes no
production edit until its own design-gate pass and envelope exist.

## 12. Unresolved items (all closed at authoring, 2026-08-22)

1. ~~Trusted base + authority hashes~~ — pinned in §1 at
   `548200dfb52a9ebc31c5d4f26085ec82a151658b`; the §8 amendments
   re-mint the artifacts through this packet's own gate.
2. ~~Focused-suite resolver~~ — RESOLVED: the owner consumes five
   resolver queries whose checker-side minting rules are pinned in
   §4.3. The suite's `FixtureResolver` implements them as a mini
   lexical binder over the single parse tree: a scope walk
   (function/block/for-head/catch scopes; `var`/function hoisting to
   the function scope; `let`/`const`/`class`/parameter/catch
   declarations in their block scopes) resolves every identifier
   reference, then the pinned rules run verbatim —
   `checkNestedBlockScopedBinding` per reference (the four loop
   flags + `capturedBlockScopeBindings` per for-part),
   `isAssignedInBodyOfForStatement` for out-params,
   `isSymbolOfDeclarationWithCollidingName` (outer `resolveName`
   hit, else the captured/loop-position rule) for the two collision
   queries, and the arguments-symbol rule for
   `is_arguments_local_binding`. Exactness argument: the fixture
   language has no `eval`/`with`/TS namespaces/enums, so lexical
   resolution IS name resolution; and the resolver's answers
   determine EMITTED BYTES (renames, `_1` suffixes, out-params,
   loop-function extraction), so every projection byte-compare
   verifies the resolver against the real checker's behavior — a
   wrong answer cannot hide. One boundary: upstream `resolveName` can
   hit LIB GLOBALS the single-tree binder cannot see; the fixture
   language therefore forbids block-scoped declarations whose names
   collide with `lib.es5` globals (measured: the block-scoped names
   across all fixtures are project-local identifiers, none
   lib-colliding), keeping the collision probe exact. The production resolver is the landed
   checker bridge (B-1); its wiring is B-5's registration concern
   with the 32-case byte gate as the named verifier.
3. ~~Oracle equivalence for full-emit expectations~~ — RESOLVED at
   design level, extending B-3 §12.6: the §7 fixture language
   contains no construct any pass BEFORE transformES2015 lowers
   (TypeScript-syntax passes are extensionally identity on type-free
   sources via the `update*` unchanged-children arms; ES2016-ESNext
   passes lower nothing in the language — no `**`, async, rest/
   spread properties, optional catch, `?.`/`??`/logical assignment,
   class fields/static blocks/private names), and the passes AFTER
   the pair (module transformer at script/None position) are
   identity on script files. The Rust projection therefore runs the
   same two-transformer suffix the upstream pipeline runs. Verified
   empirically across all 123 probe emits at authoring (fresh
   createProgram per case; zero syntactic diagnostics; the
   generator-composition cases print through B-3's landed machine).
4. ~~Parsed-tree ES2015 facet gaps~~ — RESOLVED: the parsed-tree
   initializer (`local_transform_flags`/`local_contextual_target_flags`)
   lacks the §5 ES2015 facet arms (measured at the trusted base —
   templates, computed property names, shorthand assignments, array
   destructuring assignments entirely, the object arm's ES2015 bit,
   `new.target`, binary/octal numerics, extended-unicode strings,
   extended-unicode-escape identifiers (`ident_unicode_escape` has NO
   other ES2015 content — without this arm `shouldVisitNode` never
   fires on it; the vendored row is `createIdentifier`
   `_tsc.js:21621-21623`), the static-keyword token); without them the dormant owner's
   `shouldVisitNode` gate never fires on parsed input. The
   completion mirrors the vendored factory rows already ported in
   `classify_created_node_flags` (factory.rs:747-1024), adds the
   missing classifier `MetaProperty` arm with its vendored citation,
   and is corpus-inert: `CONTAINS_ES_2015` has ZERO active readers
   (the only consult sites are the dormant ES2015 gate itself and
   exclusion masks; measured by grep at authoring — every other hit
   is a write site or a mask constant), and the full-corpus ratchet
   enforces byte identity.
5. ~~Composition-shielded arms~~ — RESOLVED (B-3 §12.6 treatment):
   arms only reachable through passes outside the fixture pipeline
   port faithfully and carry direct unit contracts — the
   TypeScript-class-wrapper surgery (input carries
   `InternalEmitFlags::TYPE_SCRIPT_CLASS_WRAPPER`, minted only by
   the TS transformer; unit contracts drive hand-built wrapper
   trees), the export lanes (`createExportDefault`/
   `createExternalModuleExport` — module files leave the script
   fixture language), the AsyncFunctionBody-gated loop/object arms
   (async lowers in es2017 BEFORE this position), and the
   StaticInitializer facts arm (class static fields are TS-wrapper
   input). Their end-to-end verifier is B-5's 32-case witness gate.
6. ~~`moveSyntheticComments`~~ — RESOLVED: the arrow expression-body
   return statement moves the body's synthetic comments
   (`:106304`). No generic move surface exists (measured); the
   packet adds the typed arena surface
   `TransformArena::move_synthetic_comments(from, to)` (factory.rs;
   metadata take+append accessors if required) with the vendored pin
   25388-25395 — a METADATA relocation, not a node-record mutation,
   sanctioned exactly as the CS-6 comment carriers; corpus-inert
   (new surface, sole caller is the dormant module; ratchet
   enforces).
7. ~~Printer sufficiency (no printer edits)~~ — RESOLVED by
   measurement + the probe corpus: synthetic leading comments on
   parenthesized expressions (`/** @class */`) print through the
   CS-6-landed statement/expression comment phases; `SingleLine`,
   `starts_on_new_line`, node-record `multi_line`, and token
   source-map ranges are honored (printer.rs:5624/:5731/:6915 +
   metadata.rs:547); helper preludes print in priority order
   (printer.rs:1235-1244 — `__extends` priority 0 FIRST, then
   unprioritized in request order, `__generator` priority 6 among
   prioritized). Any residual layout divergence surfaces as a §7
   byte mismatch BEFORE any production wiring exists — the suite is
   the tripwire, and a printer gap would pause the train for its
   own reviewed slice rather than an in-fence patch.
8. ~~onEmitNode decomposition~~ — RESOLVED: upstream's
   `onEmitNode(hint, node, emitCallback)` wrapper maps to the
   landed `before_emit_node`/`after_emit_node` pair (transform.rs:
   749-765) with the enter/exit facts split across the pair and the
   saved ancestor facts on an explicit transformer stack; the
   printer's pairing discipline (before at printer.rs:8893, after
   at :9115 with error-preserving restore) guarantees LIFO nesting;
   the B-1 hook_chaining order contracts already pin this exact
   topology with the literal "es2015" slot (notification enabled,
   first-registered outermost).
9. ~~createCallBinding at the ES2015 position~~ — RESOLVED: the
   super arms are LIVE here (unlike B-3's generators position) —
   ports faithfully per §4.2 with `languageVersion = undefined` at
   both ES2015 call sites (the `< ES2015 → "_super"` arm stays
   dormant and ports faithfully); `cacheIdentifiers` defaults false.
10a. ~~Printer object-literal `Indented` arm (mid-train amendment)~~ —
    RESOLVED during implementation: upstream
    `emitObjectLiteralExpression` honors `EmitFlags.Indented`
    (`_tsc.js:118208-118222`); this printer implemented the protocol
    only in its class arm, and the §7 suite's computed-name chunking
    fixtures tripped exactly the §12.7 tripwire. The gap is
    UNREACHABLE-today outside this packet (measured: every active
    `INDENTED` producer stamps classes — class_fields.rs:556,
    class_fields/downlevel.rs:2633 — and the printer's class arm
    already ports the identical protocol), so the one-arm completion
    landed as a mid-train fence amendment (the CS-2 amend-rule
    precedent) with the vendored pin in the arm header; the corpus
    ratchet is the enforcement, and the byte tripwire that found it
    (`obj_computed_multiline`) is now a permanent projection.
10b. ~~Implementation-measured adaptations (all byte-verified by the
    §7 projections)~~ — (a) the harness hint model passes
    `Unspecified` for non-identifier expression children (printer
    `emit_node_id_with_context`), so the upstream `hint === Expression`
    routing for `this` maps to the ThisKeyword-token arm of
    `substitute_node`; (b) parsed-spelling channels: the printer
    prints position-ranged identifiers from SOURCE text and keys the
    member-access line break on the receiver's node positions (the
    dot token is unrepresentable), so the `getName` clone and the
    `visitIdentifier` re-mint thread their ranges through the
    map/comment channels instead of node positions — upstream
    suppresses both effects through parent-less clones and synthesized
    dot tokens, and position-threading here would open channels
    upstream never opens (maps are byte-inert at this dormant
    position); (c) tsc numbers `createUniqueName` families in the
    printer's per-scope name-generation pass (a parent scope's names
    fully before its nested function scopes), while the eager model
    allocates post-order for the converted-loop `state` family — the
    module re-plans that family in scope-pass order before the
    finalize walk (`renumber_state_bindings`, writing through the
    sanctioned `set_generated_identifier_text` surface); (d) the
    parse records carry COOKED literal text, so binary/octal
    detection reads the record's `numeric_literal_flags` word and
    extended-unicode identifier detection reads the source slice
    (`\u{` — no parse-side NodeFlags-256 writer exists), in both the
    §12.4 facet arms and the visitor lanes; (e) §12.4's list gains
    the `createBaseCallExpression` super-property row
    (`_tsc.js:22574-22576` — a call whose callee is a super property
    carries `ContainsLexicalThis`; zero active readers, measured);
    (f) the parsed-tree MetaProperty arm stamps ONLY the `new.target`
    ES2015 half — the `import.meta` ES2020 half has ACTIVE readers
    and stays outside this packet's corpus-inert scope (the dormant
    classifier carries the full vendored row); (g)
    `createExtendsHelper` passes the file-level-optimistic `_super`
    as the second `__extends` argument (`_tsc.js:25852-25860`, now
    pinned in the module).
10. ~~Reuse vs re-port of B-2/B-3 addenda~~ — RESOLVED: re-port
    into `es2015.rs` (§4.2 rationale — fence tightness over landed
    production files, visitor-state coupling of the generators
    copies, es2018/B-2 flatten-level precedent for duplicated
    upstream spans; the ledger verifies every duplicate header
    independently).

## 13. Readiness summary

Upstream: the frozen owner-graph/gap-matrix/witness/dispositions
chain (§1) plus the §4 vendored pins (171 owner functions + ~50
addenda slices new or re-pinned, all hashed; all 171 re-verified
against the owner graph's byte-offset hashes at authoring). Rust-map
rows: 18 (§5), targets measured present or new within fence. Gap
rows: 1 flipped, 1 deliberately untouched (§6). Witness families
cited: 7 of 9 (qualify at B-5; focused projections are this packet's
surface — 123 oracle fixtures + typed fault contracts + unit
contracts). Architecture impact: `EA-GAP-CAPTURE` retired to
`exists`, `EA-GAP-COMPOSITION` gains the first production
FlattenHost, `E-ORDER-H` gains the dormant ES2015 hook pair,
`E-NAMES-H` gains its first production callers of the dormant
loop-variable/node-keyed arms, `EA-GAP-FLAGS` parsed-tree ES2015
facet completes, comment/printer rows premise-unchanged.
Undispositioned: 0. Unresolved: 0 — items 2-10 resolved with
measured pins or design-level arguments at authoring (2026-08-22);
the §12.5 composition-shielded arms carry their named verifier into
the B-5 byte gate.
