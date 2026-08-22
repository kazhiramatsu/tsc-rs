# H2.5h-b / B-3 — the Generators state machine: transformGenerators as a dormant foundation module

Design-gate packet for the THIRD H2.5h-b implementation packet, under
the mandatory implementation-ready design gate
(../post-h1-completion-slices.md). Authored at the train start
(2026-08-22) on `h2/5h-b-b3` from the post-B-2 trunk; reviewed
(independent full-dimension pass: 161/161 §4 hashes recomputed, all
129 owner-graph byte-offset hashes re-verified, 40+ file:line
citations opened, 16 live oracle probes — verdict READY-WITH-FIXES,
all eleven findings folded in: the visitation-phase label-literal
blocker resolved as the §12.4 finalize-write, the ledger-protocol and
argument-presence pins, the fixture-language no-shadowing constraint,
the `createMemberAccessForPropertyName` direct-reuse correction, the
transformer-lifetime rename-map ownership, the `writeEndfinally`
768-scope, the embedded 72-fixture list, and the three bookkeeping
notes). The design-gate
pass lands with the trusted base, envelope, bootstrap, and index in
one commit before any production edit. Machine check:
`node .github/ci/slice-readiness.mjs --check h2-5h-b-b-3`.

## 1. Identity, purpose, and boundary

- **Slice ID:** `h2-5h-b-b-3`. **Kind:** `foundation` — a corpus-inert
  substrate packet, the third rung of the ratified B-ladder
  ([B-1 packet](h2-5h-b-b-1.md) §2). It lands the `transformGenerators`
  owner frozen in the owner graph — the complete state machine
  (labels, try/catch protocol, instruction encoding via
  `createGeneratorHelper`; `_tsc.js:108119-110087`, 129 pinned local
  functions) — as the new dormant module
  `crates/emitter/src/builtins/generators.rs` exposing a real
  `Transformer` (`GeneratorsTransformer`), NOT registered in any
  pipeline. Consumer-first per the pinned `yield-star-synthesis`
  composition edge: the machine (consumer) lands before B-4's ES2015
  loop conversion (producer), so the two pinned `yield*` synthesis
  sites have a complete consumer when they land.
- **Non-goals:** transformer registration or activation (the dormant
  seam `crates/emitter/src/builtins.rs:144-150` "older targets belong
  to later target-ladder slices" is preserved verbatim); ES2015
  visitors, class lowering, loop conversion, captured
  this/arguments/new.target (B-4); tagged-template lowering and the
  joint registration flip (B-5); any edit to the active es2017/es2018
  lowerings; witness-set amendment; any corpus output-byte change.
- **Prerequisites:** B-1 (helper texts incl. `typescript:generator`
  priority 6 and `typescript:values`, the six-query resolver surface
  incl. `get_referenced_value_declaration`, eager name generation,
  EA-GAP-FLAGS classifier, hook chaining) merged @02f784d9; B-2
  (destructuring flattener) merged @28f04d95. The B-1/B-2 substrate
  this packet consumes directly: `helpers::generator()` /
  `helpers::values()` texts, `GeneratedBindingScopes` +
  `TargetBinding` (E-NAMES-H eager model incl. `allocate_numbered`
  and the dormant `allocate_loop_variable`), factory-owned
  transform-flag facets (`propagate_child_flags` +
  `function_facets`), the chained substitution surface
  (`Transformer::substitute_node` + `enable_substitution`), and the
  `EmitResolver::get_referenced_value_declaration` trait row with its
  checker production bridge (`crates/checker/src/emit.rs:196`).
- **Trusted base:** `28f04d95ac8e413cf63b95146dda940143b791d6` (main
  after the B-2 merge). Authority artifacts at that base:
  owner graph `ratchets/h2-5h-a-owner-graph.v1.json`
  sha256 `f28d51770b0ea476a51053e438d186b66939f7b658f1455ad0c0f8729eed69f8`
  (fingerprint `e43871cfedeb4a049f93a5f3a03f0083af7dc037f58e3b51b22cb0d51e4b3294`);
  gap matrix `ratchets/h2-5h-a-gap-matrix.v1.json`
  sha256 `8717fe0c59b7bd0a992b92ee55cb1cc00122df0f422639f5118b1707576227e8`
  (fingerprint `f3fb764a35faa9ea93a2e65cd36a4500a618b38f0be464ea6e4d3f5c534a5ab8`);
  witness artifact `ratchets/h2-5h-a-es2015-generators-witnesses.v1.json`
  sha256 `85e9d767625331ec81cbe15a08af8ffac7a709ca6b0556896495a8e73621b8f4`
  (fingerprint `a97ac664628dab8efed1c543ce317cc3f6d67fdc9e77cbab5d80590eb7860209`);
  dispositions manifest `ratchets/h2-5h-a-dispositions.v1.json`
  sha256 `951a68ad6234752ff3836cc8d773545b15779076ba865e2f52917b1c9a19460a`
  (fingerprint `5177e753ff4c6e13ae19c477d45a7bb2edb927aa7921d29d4cffa30f03b9ac45`).
- **Activation state:** before — gap-matrix row 10
  `generator-state-machine` is `missing` (asserted absence: module
  `generators.rs` in `crates/emitter/src/builtins`), counts
  10 exists / 0 partial / 3 missing. After — row 10 `exists` with the
  module anchors and the absence retired, counts 11 / 0 / 2; the
  joint pass STILL dormant; the corpus ratchet byte-identical
  (T0=100.0000% 49024/49024 FP=0 unchanged).
- **Next owner:** B-4 (ES2015 visitors with the two pinned `yield*`
  synthesis sites, per the ratified ladder).

## 2. Position in the ratified ladder

The B-1 design pass ratified the decomposition
([B-1 packet](h2-5h-b-b-1.md) §2); this packet is its row

> **B-3** | foundation | the Generators state machine (labels,
> try/catch protocol, instruction encoding via
> `createGeneratorHelper`) — dormant, driven by focused fixtures only
> | `loop-conversion-capture` yield-star consumers deferred to B-4;
> state-machine focused contracts

and revisits neither the granularity nor the ordering. The witness
families that exercise the machine (`loop-conversion-capture` incl.
both `yield*` sites, `hook-chains` generator-var-hoisting cases,
`helper-graph`, `name-generation`, `enum-pair-guards`) qualify
END-TO-END at B-5; B-3's qualification surface is the focused
projections of §7, which drive the real `GeneratorsTransformer` on
parsed fixtures directly. The `yield-star-synthesis` edge's consumer
obligations (the `EmitFlags::ITERATOR` skip in `visitYieldExpression`
and the delegation opcode) land here and are exercised by focused
yield* projections; the producer sites arrive with B-4.

## 3. Required-reference table

| Row | Lifecycle before → after | Role here |
|---|---|---|
| `EA-GAP-COMPOSITION` | `activate` (dispositions row cites capability `generator-state-machine`) → unchanged disposition; rationale records the B-3 landing | the architecture gap owning the machine; its §6-map section gains the substrate-landed B-3 preamble (§8.3) |
| `E-ORDER-H` | `activate` (hooks half landed B-1) → unchanged | the Generators `onSubstituteNode` (catch-variable rename) lands DORMANT as `GeneratorsTransformer::substitute_node`, exactly the chain position the B-1 order contracts pinned (previous-first delegation; Generators registers substitution only, no notification) |
| `E-HELPERS-BASE` / `E-HELPERS-H` | `active-*`/`substrate-landed` → unchanged | `helpers::generator()` (`typescript:generator`, priority 6, helpers.rs:236) and `helpers::values()` (helpers.rs:220) are the two helper texts the machine requests; B-3 adds the missing `EmitHelperName::{Generator, Values}` identifier variants (factory.rs:19/:45) — no helper-text change |
| `E-NAMES-BASE` / `E-NAMES-H` | `substrate-landed` (B-1) → E-NAMES-H gains the reuse-temp-variable-scope finalize arm | temp allocation (`allocate_temp`), named catch renames (`allocate_numbered` → `e_1`), loop variables (`allocate_loop_variable` → `_i`, first production caller), and the state-temp naming arm: `finalize_generated_binding_names` must honor `EmitFlags::REUSE_TEMP_VARIABLE_SCOPE` (§5, §12.3) |
| `EA-GAP-FLAGS` / `E-METADATA-BASE` | `activate` (B-1 classifier) → unchanged disposition; the parsed-tree generator facet completes | created nodes take factory-owned facets (`function_facets`, factory.rs:695-705 already ports the `isAsyncGenerator ? ES2018 : isAsync ? ES2017 : isGenerator ? ContainsGenerator : None` arm); the parsed-tree initializer `local_transform_flags` gains the SAME arm for FunctionDeclaration/FunctionExpression/MethodDeclaration (§5; corpus-inert: zero active readers of `CONTAINS_GENERATOR`, measured §12.2) |
| `E-RESOLVER-CAPTURE-H` / `E-CHECKER-FACTS-H` | query trio landed (B-1) → unchanged | the machine consumes exactly ONE resolver query, `get_referenced_value_declaration` (owner-graph `resolver_methods`), inside catch-rename substitution; production bridge exists (checker emit.rs:196); the §7 suite supplies a fixture resolver (§12.5) |
| `E-COMMENT-SCOPE-H` / `E-COMMENTS-H` | `active-qualified` (CS-6) → unchanged premises | the machine mints synthetic trailing comments on instruction literals (`add_trailing_comment`, metadata.rs:570 — the enum-substitution precedent prints `case 2 /* … */:` byte-correctly, active_transform_contract.rs:2812); no comment-scope threading change, no printer edit |
| `E-ARENA`, `E-CONTEXT` | `active-qualified` premises | `TransformArena`/`NodeFactory` construction; `TransformationContext::{hoist_variable_declaration, hoist_function_declaration, resume_lexical_environment, end_lexical_environment, suspend_lexical_environment, start_lexical_environment, enable_substitution, request_emit_helper, read_emit_helpers}` (transform.rs:472-664) |
| gap row 10 | `missing` → `exists` | §6 matrix |

Lifecycle values transcribed from the dispositions manifest at the
trusted base; the §8 amendments re-mint the affected artifacts through
this packet's own gate.

## 4. Pinned upstream map

The upstream IS the frozen artifact chain plus the vendored slices
below. All spans are 1-indexed inclusive lines of
`vendor/typescript-6.0.3/lib/_tsc.js`; every hash is the ledger d2
line-slice sha256 (newlines included, final line's newline included)
and lands verbatim in the module's `tsc-hash` headers, verified by
`cargo xtask ledger check`. The owner graph pins the same 129 local
functions under the byte-offset recipe
(`sha256(bytes[start:end))` relative to the owner declaration offset
5142146); both recipes were re-verified against the vendored file at
authoring (all 129 byte hashes match the artifact).

### 4.1 The owner (owner-graph `owners[1]`, `transformGenerators`)

Declaration `_tsc.js:108119-110087`, declaration_sha256
`32e4d6d16a155d726d64ecb62721900c6c32c06d7265b6b8849e17a67191849c`,
body_sha256
`19ec3038b4854b4a1ba89b8a804ee21091d9c18e4918cc07cc82d55b574b1d8d`.
Hooks: `onSubstituteNode` only. Factory methods consumed: 41;
resolver methods: 1 (`getReferencedValueDeclaration`); context APIs:
9; helper calls: 2 (`createGeneratorHelper`, `createValuesHelper`);
external utilities: 41; enum references: 75.

| # | function | lines | d2 line-slice sha256 |
|---|---|---|---|
| 1 | transformSourceFile | 108159-108166 | 73ead7f02f3e34dd8ea4ebd6677fe6ad02975986387bf7e3070c8038afbf4866 |
| 2 | visitor | 108167-108180 | 308e1dfaad03d12139fe34a12a955137b22ba52a29451647a48a09280b11b0de |
| 3 | visitJavaScriptInStatementContainingYield | 108181-108194 | bad26cf40a6fce206348a2af61015cd15a85bde8d05429d1d73e91765a98ffc6 |
| 4 | visitJavaScriptInGeneratorFunctionBody | 108195-108225 | 63e11dc87d7f1e0449c3c5c8c41a4108253cc6b27f0acd7850ee53f9c8997981 |
| 5 | visitJavaScriptContainingYield | 108226-108249 | 34bf6f04ea57f28683c14c543b122a4bb14d82235224140b1c96c1a6972929c1 |
| 6 | visitGenerator | 108250-108259 | 5e9a4a42b8122ef4c7b815998e6cb62b74c0f91ba32d00e3bf68ad644784d07b |
| 7 | visitFunctionDeclaration | 108260-108296 | 40d0dbd048205fe90ac87a5e4c2f2662f28a9079b78fe3ade306ed9118a2f6e0 |
| 8 | visitFunctionExpression | 108297-108329 | 2b3f71007b119b5c39c87413cb1212f275c3391dbeff9611596dcd3923215792 |
| 9 | visitAccessorDeclaration | 108330-108339 | 894de52c8603a49d3cf839366ec506ec76a5b9e065923cc73214029af55e2aa2 |
| 10 | transformGeneratorFunctionBody | 108340-108397 | b5c1b42385ee0a311c638406213c2f61ddc245bf21ba1c206259ad02dab08b26 |
| 11 | visitVariableStatement | 108398-108422 | 2cd0a6aa7d6d020abc07018c73d8ba5b7d190557d42f5e87e723e0dba75eef4c |
| 12 | visitBinaryExpression | 108423-108433 | c07fedbd5eac4aac0a1e65b84df6ded6a84c33f9938babb756fbc3ff48e8ab74 |
| 13 | visitRightAssociativeBinaryExpression | 108434-108474 | a6fe47778d107b985e7b45adfe168f1fa53d16edab04ec23949c33a74fd1d10e |
| 14 | visitLeftAssociativeBinaryExpression | 108475-108485 | 739d608f180f4418856ec640d69f0d7b02483307789afb0be8a0708c51345dc5 |
| 15 | visitCommaExpression | 108486-108503 | 350c6a0c857f3227c98cdc140c7a34e7e66ddd9cdf6c9b98300f9c9c65da52b5 |
| 16 | visit | 108491-108502 | 39e7520089a7dfe7989ff77cd85034462b81fbee2115d2200eda7f55e3bb6976 |
| 17 | visitCommaListExpression | 108504-108518 | 0ba115d81a4a89f86fd5f0380efb42437af378330fbd3b4dd7880c104d5eec38 |
| 18 | visitLogicalBinaryExpression | 108519-108551 | 25f76339ccad88ae3361cc39c35f639e208a24e491a9ca16bbb70dafa2f6583f |
| 19 | visitConditionalExpression | 108552-108581 | c305eb643e4f7e0d98ab80cbb0760bae2ebde1fa1b884f48630eac21821d26ff |
| 20 | visitYieldExpression | 108582-108604 | 7f650448505e98a8735389dd31382932f8518c3c3d6a75f68d0e399a5b4ada46 |
| 21 | visitArrayLiteralExpression | 108605-108614 | 373615dbd7adce68e4d93ed3d8527fb73eb668fbdbcac638b56c6520c918adbc |
| 22 | visitElements | 108615-108656 | 27c5033bced1782855dddb2f3df36908ae590e07619b65375f521837df5aae69 |
| 23 | reduceElement | 108634-108655 | 141664afe0e73989abfeed5ca406229b61f93956e08969989911c65215827fbd |
| 24 | visitObjectLiteralExpression | 108657-108687 | c314e8e8c0217868cc4d8b5c2e1cd7d5033ea20b4939cf04a2f90d421255d1be |
| 25 | reduceProperty | 108672-108686 | e95a0bc9862f52d34e861168144c502309325213cb564e2f9a06ec0ad02e7f73 |
| 26 | visitElementAccessExpression | 108688-108693 | 6b0e546d4bf1d61c58b3b64f90744a25b37179a049ec7eed301238251af73841 |
| 27 | visitCallExpression | 108694-108716 | 7c0d96a6a0e1226a6233687b1582904727947f358ea003f5a43d4e922c0971b9 |
| 28 | visitNewExpression | 108717-108742 | c28eff415054d25b4582e5ce0ca9d0f82fa529a5ff31da715d2a303186d0d580 |
| 29 | transformAndEmitStatements | 108743-108748 | 9f018585be7cc8c2cd3c7b87d0ecc0b1ba4ef42ac117a658e14ea2a19b76b4da |
| 30 | transformAndEmitEmbeddedStatement | 108749-108755 | 2881aa8b507d5888781fa634ab97b8f18f0072bc343e7f6156a2cfe17483c458 |
| 31 | transformAndEmitStatement | 108756-108763 | 7c8230a8640cc176775bf9b14164fa985eb9040cfe9c2394d21b6c8a25ef471a |
| 32 | transformAndEmitStatementWorker | 108764-108799 | 8aff072cd4d9b5d5e4f25b2a269fc128f8f9e72501d6fef327c56da4c422f482 |
| 33 | transformAndEmitBlock | 108800-108806 | b1f5978ac132ca11e0e8cca03631b341ba82720ebf621fc075c045fb95303021 |
| 34 | transformAndEmitExpressionStatement | 108807-108809 | a61dd0047076cf7eff8560d2fbf2f9fbf5ac76d958b53e49004b56e69e98a3e9 |
| 35 | transformAndEmitVariableDeclarationList | 108810-108835 | 6cdc756131383449ef11bc1c638880dd14f387e6c7b22df16f440273339dcea3 |
| 36 | transformInitializedVariable | 108836-108844 | 8f41ee5a89a8c5a5f518518266db37752edb2ea60fa14b87deb56e9fac029b3e |
| 37 | transformAndEmitIfStatement | 108845-108869 | 500b8cc6c6b336e94155a5f3f94294a351cbc31c121d8ca9f35bec60c0d2dd76 |
| 38 | transformAndEmitDoStatement | 108870-108886 | faad717ba42b42cd664ed72d71b49931ee8310f711a2c55522e4ccd7564a116c |
| 39 | visitDoStatement | 108887-108896 | 1e34e04e121b22424121927ab9f109afb9b73292547f43f32e8a0d3e40f5e41d |
| 40 | transformAndEmitWhileStatement | 108897-108909 | 2ff85356121b544b13e6a615b543609df19aff5380bfc9d6772548848434764c |
| 41 | visitWhileStatement | 108910-108919 | 55f1ec91156d3be0ab32fcd25eae5b5bac82b2a38844784eb8cf39883b879f4b |
| 42 | transformAndEmitForStatement | 108920-108961 | e26d4be3d37b50edc9dcff728ed4108dff5bdafe2a8578bd4a2f61f008b6610e |
| 43 | visitForStatement | 108962-108986 | f4d66caca57c2ffe08fa156e288cc79302a4638826425eba583127a3338ac90e |
| 44 | transformAndEmitForInStatement | 108987-109038 | ccd539ed18f0a978a441c4b1c43bf5068d0c6f2e94ff6a32002deee5823551dd |
| 45 | visitForInStatement | 109039-109056 | 1479db3ac09b3fab28933d5d5fd8c11db834e9dc0136ca405bed91fa894e0efe |
| 46 | transformAndEmitContinueStatement | 109057-109068 | a37f3803e0ae7fae191f4a9dd6230e0368b370eac895ac0fb4a7357450549830 |
| 47 | visitContinueStatement | 109069-109081 | 08ed3cdd08519b9fe7bcf61335d67c4d736d2e329f89198e73142291d382ceb9 |
| 48 | transformAndEmitBreakStatement | 109082-109093 | 848f1199536d426641932d064fe89be3442363d768c198ffcc563f1edc4b32d8 |
| 49 | visitBreakStatement | 109094-109106 | d2510c23505af99ff9401d8a429d0aed0c4570f7fab823f5e743a668f102bd31 |
| 50 | transformAndEmitReturnStatement | 109107-109113 | fc7ad6b2114c68f0a963399e2bbf4f7f8d8a762049675cd96e0f9be1df88d586 |
| 51 | visitReturnStatement | 109114-109120 | 384a635a720c3012aa363d354920f9cb7595555e152520f5d4a0e28442392291 |
| 52 | transformAndEmitWithStatement | 109121-109129 | 9e589d24c92ff9f629eb94f3d070fb3f09f2bb9f95479281944a672e4965bc84 |
| 53 | transformAndEmitSwitchStatement | 109130-109194 | bb8675f09bac7919d9a7793188a3b212b16916e8222f7222861b0004f034b7ee |
| 54 | visitSwitchStatement | 109195-109204 | 401fa818af05e2c447fe19cadef81439924f359d29df80c633965abda4fa5602 |
| 55 | transformAndEmitLabeledStatement | 109205-109213 | ecea8dc0da489771425915e05b371b2699aa4be555d2aa3ed09cc7fbdac7b7b9 |
| 56 | visitLabeledStatement | 109214-109223 | c41c046ab364ef445626c9b8c54bb9e79fd0a71bc493db66a9ebef5633039369 |
| 57 | transformAndEmitThrowStatement | 109224-109230 | e0892ff3c04ca7bc06b6ea744d40cf831d84b1ccf68d9d6e7ddd4894a46f5a76 |
| 58 | transformAndEmitTryStatement | 109231-109247 | d99e7fe5df70434d2ef8aff32395698a9b0625efa8f3457b5a08b377b6798d33 |
| 59 | containsYield | 109248-109250 | bfd65ca7e124e65266ddc461c31883f9c271350dc67e9176a6ae91ba980b4ab3 |
| 60 | countInitialNodesWithoutYield | 109251-109259 | 37a6f6f3e7e7d5aea92d146079cdd358976c3cf0cdd6913dd8a64b5fec07d35f |
| 61 | onSubstituteNode | 109260-109266 | 95db4fbcaba10e96d12184b62ac54dd102ddb3c2a854723902e633ab92aa8693 |
| 62 | substituteExpression | 109267-109272 | ab512e435fe64c869361c8729c3d87b75cb965d3e702b6de4f82cc1b74ceb921 |
| 63 | substituteExpressionIdentifier | 109273-109290 | 9ad55adb46c4f1e904041b50939e84e422ac31d9ea5483001ca44050e80611d4 |
| 64 | cacheExpression | 109291-109303 | 9bf7b66845e2481b073404546d59dd3bb2d9271febd15d2974e5f6f45353d056 |
| 65 | declareLocal | 109304-109311 | 38e3b180101f2fd947737aa526b2ea6940e3530542c7a86da49ff279b32bab77 |
| 66 | defineLabel | 109312-109320 | 9c866533854e082d08103b062fef1cbb31edaacc749271a5fb72a7cf717ef017 |
| 67 | markLabel | 109321-109324 | f562499ccf6af36ef341feb29f86d4b13fd33dff8d77f4d7062450e42e9389cf |
| 68 | beginBlock | 109325-109338 | a006f625b082280688e18ab9d688070b1065d21eb01d7664409d22f456425333 |
| 69 | endBlock | 109339-109348 | 33882e2938dcc1cae49666e4176befd58f599eebca3f71d50caec2212f0f009f |
| 70 | peekBlock | 109349-109351 | ae3fc972654d70a55d0c051ce3184be5a410c97bd1ff55689b8d3317a4c0b828 |
| 71 | peekBlockKind | 109352-109355 | eafb8d00e5962cb51348f56e0384cdd1c2e507692625cca91de14f26ff7ca911 |
| 72 | beginWithBlock | 109356-109366 | e9175b0ba2e70e441a513881f02cde3aff768e8c72593ce26845c94a21033e6c |
| 73 | endWithBlock | 109367-109371 | 8fc9b1a49d04fe9aaf31d43eaee0e3bde59e5e7cd1f77991f3385abf2eadcf54 |
| 74 | beginExceptionBlock | 109372-109384 | 42ade19da0a0a40b47d6cb42308c83e5a775c41dc28e22014adf4b66b4316b28 |
| 75 | beginCatchBlock | 109385-109418 | d2fc338ccbc6cdacf53c9afe096110573e9e77b2a14a48e612409a37bc82ea1c |
| 76 | beginFinallyBlock | 109419-109429 | 5f19e31e2dd90f9a92496b6778e1a641ee5651e417fbdfe822104a812787cf01 |
| 77 | endExceptionBlock | 109430-109442 | f8f26b14bea22ca58ec2eb938dac6634d932384bc59e0cdf094d474b58fece76 |
| 78 | beginScriptLoopBlock | 109443-109450 | d02cc5869859e3004627943929fc5100c8817dbefd77a3e721cab8147ee66520 |
| 79 | beginLoopBlock | 109451-109460 | 3cd2a0e840c91099ebbc55813f48e1bc409642d12d9d7eecdb754ee99e29c7bf |
| 80 | endLoopBlock | 109461-109468 | 5c1060e4cb0a8df47bca2186f14e4a8acd1d6cb48cc77ed43e33c9feff0c0aaa |
| 81 | beginScriptSwitchBlock | 109469-109475 | 93817ddadcac9da1a07ec4356650e8e4f7cf928ee9f0f346023bb9dbe68ee94d |
| 82 | beginSwitchBlock | 109476-109484 | b879557ff0cc26b2bb70e8938b43d691cf07ee67914f1c4ea41c5c7a5af75882 |
| 83 | endSwitchBlock | 109485-109492 | 74dd506129e3c02e9f5c97144be2dfb411ea6d28580638dba55fcbf92a0f3026 |
| 84 | beginScriptLabeledBlock | 109493-109500 | 2e5d8f46ca701d04a451ebe163ebf3b8f7a875dbd5048dc03bb1268018498d84 |
| 85 | beginLabeledBlock | 109501-109509 | 8251d37e91f50aaad36de6848190322c18fc66509ae033b99b2c01330cde80e2 |
| 86 | endLabeledBlock | 109510-109516 | 7fbc09cf024cc2aaabe489db29a65fecdc2b3fb12be129daae24a644eccffce7 |
| 87 | supportsUnlabeledBreak | 109517-109519 | 338a47b0a2efcc800983f5f92fbdb44d5206e4983a6a0226e7619a2b87746b36 |
| 88 | supportsLabeledBreakOrContinue | 109520-109522 | 8843ad57c6e6a83f6335c4b8a78e2e45948af398c3bc9e97b285a1a3e638cfe8 |
| 89 | supportsUnlabeledContinue | 109523-109525 | 81ce98f906a0701e222a278d50cc44fc88e120879a2ca0202cf69ebf1e5a7f31 |
| 90 | hasImmediateContainingLabeledBlock | 109526-109538 | ef67f25d295c8597261e63f8f9959939f21702d276450c6f7f88ec9818a170ec |
| 91 | findBreakTarget | 109539-109560 | 7ff3d1fddb7e4f8f73cecfdda28e3bdbbc19f1e569e0ed7ece3f308188d44752 |
| 92 | findContinueTarget | 109561-109580 | bc87cdf4e300b2991fa38baf61f54dee8c89b6d8881fb7da2005222fca0f15a2 |
| 93 | createLabel | 109581-109595 | 4e2a8ecb553028df68197008ed04343c91f28d511d2a9bc1debfa5043d12ee7b |
| 94 | createInstruction | 109596-109600 | 803684eeae5ab66d21ce6f2344ac372b526c4752b809158e9aba9fca37946c4c |
| 95 | createInlineBreak | 109601-109612 | 26170574408df556d6fecbf09f577cd386ec89aa0d2d899cb07a022179dc468c |
| 96 | createInlineReturn | 109613-109622 | a3e0c29e7977e4e0ccfd57a3a75b4aaab55975fae52eb5729d784c30feb7f9d6 |
| 97 | createGeneratorResume | 109623-109633 | 7950ba978df30ed6bd49d800d02e87091cf61ab64c44c7eac5735f3b61204f96 |
| 98 | emitNop | 109634-109636 | 171f3eb6a5435d8b0845a21f28e2ad2d3046c52fe7f85ebce64ccf0d59a4b227 |
| 99 | emitStatement | 109637-109643 | bd8f18daaa16c88d26fb7e7623fa5bafef0f41414db9b3f29730e7c6cf7b66e4 |
| 100 | emitAssignment | 109644-109646 | c9dce9f7dbeae13b7a4c6b03da80d3d231758bfac5f1656f13287ce415080bf5 |
| 101 | emitBreak | 109647-109649 | a57648523065117b399b057715a831fb56b18710ba005c10abe35bfa77250260 |
| 102 | emitBreakWhenTrue | 109650-109652 | 5e5f430ecde1fd9bf82e3d7e405868ca2693e7bfe3cd3b558b2b0ac2ae113e01 |
| 103 | emitBreakWhenFalse | 109653-109655 | 813e987a2d88b2051bfa86aee01389128ae6d6e68622866f457f80e1ca05a217 |
| 104 | emitYieldStar | 109656-109658 | c43daa1c4465d56d8813c657e69c73134bdde91b849ca839aba29c71e129eca6 |
| 105 | emitYield | 109659-109661 | 644e0ba4063953c1fef3f2bbd2e14abac5d7fc5c3a29ba4a3f5a953fe16fb05a |
| 106 | emitReturn | 109662-109664 | 38d1338d596867038cae20b4a4e4c146c99d5256a3d1f26232b9f84a35c6e239 |
| 107 | emitThrow | 109665-109667 | ae8741de8b7365f889a3a9c8a4335bd1727b8f60a5fbe0d6b97e65e394fa55a5 |
| 108 | emitEndfinally | 109668-109670 | 6a5fa6863e7a958825293e5e29a78a5d3e26562128bbd249e71d57f1b7ed138d |
| 109 | emitWorker | 109671-109684 | 54fb879d6878e8bfabb06122b2dc0d12c95e6254834f754ef93e711325c8cb26 |
| 110 | build2 | 109685-109726 | 9310d5f9a876a3bbb7502359d284f69f3c97a566e4895ce12c875bec2d466fa2 |
| 111 | buildStatements | 109727-109745 | 55db5a17f4d457720e5ea7b84d5a2373215c8826a347db363623a1292ac9998e |
| 112 | flushLabel | 109746-109757 | bbb88d6572897b9910bc6e387164a9495898592f7665282fbb282816bb5df9e8 |
| 113 | flushFinalLabel | 109758-109776 | 9ebbd4e75c178bedef4db085fa4e878e53c9bab1d38c600e29becfa20605990d |
| 114 | isFinalLabelReachable | 109777-109790 | bea697744daa1ce6e02578d978bd492f5cd0af3b0d663453ecb5282ce10477c6 |
| 115 | appendLabel | 109791-109841 | fc91b657fa18527229fca3a40b031dc97de4dd82e668d7a10d45fe7430e0d8c7 |
| 116 | tryEnterLabel | 109842-109859 | f127b6a29348b36ca6f5bd91f78f2962d8ffdc9dccee44bf89c2c29dc4fc3fa8 |
| 117 | updateLabelExpressions | 109860-109876 | 5b44b108fb2fceda2c2ac85a3b522df22fc3c93bf937700aabe3b8d32a993e4d |
| 118 | tryEnterOrLeaveBlock | 109877-109910 | 5807b6973c63cd20ef5b5b6bae0c1d9284c05046b6172376e1f26f6078473c7c |
| 119 | writeOperation | 109911-109948 | b7e584811b23bbac18f9c0d087934d1a5665370f5bd1f9b902e1c53714967dfa |
| 120 | writeStatement | 109949-109957 | 92fac0cb7f8243b3daa39c7346d6307d3874fa4ff31e2210689a7e9b4984a77e |
| 121 | writeAssign | 109958-109960 | 253ed241d024a65f599ad717c22ca620b00bb1cf775816c4b0ba129ca62dac94 |
| 122 | writeThrow | 109961-109965 | 459416d429d3a071b50839b6dfc40232f75e837563a517b3b63ea10c2ce24cbb |
| 123 | writeReturn | 109966-109982 | eec0a59991adf9a21b0ffd04c86e234a61eb678ccf8c98fcb61cc004e4b79a8c |
| 124 | writeBreak | 109983-109999 | d2d8b5e612e7562ac6d269966e1923f2c4858b9b1e190d09c2e7ab3a7cbbc988 |
| 125 | writeBreakWhenTrue | 110000-110021 | 68c2d1f827d300062d24ea6dbe5ed243416cd8dbff556ac97a327af8b5f103e9 |
| 126 | writeBreakWhenFalse | 110022-110043 | 4868ca165f53461bc3fbfa6f5ef241b4fcc194f6accacc30869445aecc78938d |
| 127 | writeYield | 110044-110059 | ea2181b9bfd87b8deeb48ed7d593551f3edcca4aaabe1b30187fee1c42c011e1 |
| 128 | writeYieldStar | 110060-110076 | 413b341e49c5a376b92142b6272ca376f8e1f9e25e4fe7f8d50d3599dc7f5270 |
| 129 | writeEndfinally | 110077-110086 | ed16caf003c7e373dc4a655a284831c4cf3b8ca78843174ad4a5c34d76245efa |

### 4.2 Owner-adjacent addenda (ported with their own headers)

| function | lines | d2 sha256 | role |
|---|---|---|---|
| `getInstructionName` | 108103-108118 | `eb0ca6bd5545a7d3e3a5122748e1ddaa3d2fe6b95d183300c01411904e967e10` | instruction → comment text: 2→`return`, 3→`break`, 4→`yield`, 5→`yield*`, 7→`endfinally`; all other codes carry NO comment |
| `createCallBinding` | 24691-24753 | `445f6a3542132e1adf49e01683e039e6fa034bd127cd15ab5447db84951b41bc` | the `{target, thisArg}` computation for apply-decomposed calls; `cacheIdentifiers=true` from `visitCallExpression`; super arms are post-ES2015-unreachable and port fail-closed (§5) |
| `shouldBeCapturedInTempVariable` | 24669-24690 | `930638d4e30da0491d0c7e2612bf2920f6280413e771880f72c3c18f6712baf0` | the receiver-caching predicate: identifiers per `cacheIdentifiers`; this/numeric/bigint/string literals never; empty array/object literals never; everything else always |
| `skipOuterExpressions` | 27582-27587 | `8b1eff7c004dde6bbe6b5940ba064195f1aea6668ca5d8b1f4a69bf9cec4dec1` | callee unwrap (parens, type assertions, non-null, expr-with-type-args, partially-emitted) at `OuterExpressionKinds.All=63` |
| `isOuterExpression` | 27561-27581 | `5516dd616d83f3a2e9d8caaf560d9d12c8a8718fd446d453702b1a81ab8da298` | the unwrap predicate (kind mask arms) |
| `createMemberAccessForPropertyName` | 27206-27217 | `88b490bf2cd47503f62314d8fc5fb1c7bca83df86aae8890df643915162ce392` | receiver.name / receiver["s"] / receiver[1] / receiver[computed]; non-computed names are REUSED DIRECTLY (no clone) — the resulting ACCESS takes `setTextRange(…, memberName)` + `NoNestedSourceMaps` (128); the computed arm passes `.expression` into an element access ranged to `location` |
| `getAllAccessorDeclarations` | 16719-16760 | `8e23b58d85c286c6344992bac81b90a2c92285508dcf40a9c80d316dca13286a` | accessor pair discovery over the ORIGINAL object literal's properties (first/second/get/set selection) |
| `createExpressionForPropertyName` | 27339-27347 | `fc486b593b709b18b266695eed3d95c48147033188cb4fc1c3b0f2a658b8a51d` | defineProperty name: Identifier → `createStringLiteralFromNode` (text-source threading); computed → expression clone + range; literal → clone + range |
| `hasDynamicName` | 15850-15853 | `d126787bc1b36621098ed5255c26d1e27abe5bf6dbc55570657aa03f95a588bb` | the accessor-pair fast path (computed non-literal names stand alone) |
| `getPropertyNameForPropertyNameNode` | 15861-15887 | `5770eff9fe2f071f83fce9a7aaff9c54fa6f09141154c33c0f7f3e5dc86ee117` | accessor-pair name identity (identifier escaped text; literal text escaped; literal-computed unwrap; else dynamic) |
| `createObjectDefinePropertyCall` | 24595-24597 | `82dbc40a8f28d6f589084723a9c4a47b5e6288b2516946eb49b56fbf27d03f38` | `Object.defineProperty(receiver, name, descriptor)` global-method call |
| `createPropertyDescriptor` | 24614-24624 | `f71c9293c5c9f0af27225fb5ceea721ab74257384648f0b2b52b5997c8a28b0d` | descriptor object literal in enumerable/configurable/writable/value/get/set order; `multiLine = !singleLine` (the accessor arm passes `!node.multiLine`) |
| `tryAddPropertyAssignment` | 24607-24613 | `eda864c7c631ed434d62e5c792cc35d2b5195e9b56665b5510885fba827cde9b` | descriptor row constructor (absent attributes contribute no row) |
| `createExpressionForAccessorDeclaration` | 27348-27404 | `f7a4fc78ae9810764bc7643a09f2468c93573afa4573451dceda8b4adbce250b` | `Object.defineProperty(receiver, name, { enumerable: false, configurable: true, get/set })` — fires ONLY at the firstAccessor position, returns undefined otherwise (the reduceProperty `visited` gate) |
| `createExpressionForPropertyAssignment` | 27405-27422 | `d875847e6dfe5a88cdd180d4c2d247f9e3065648496c2ee42cdb8d2ec08e27db` | `receiver.name = initializer` with original/range threading |
| `createExpressionForShorthandPropertyAssignment` | 27423-27442 | `177d276035c1a4c120f6d4ca82554cfc7af50a06c72125186664c9bf0ead0c4a` | `receiver.name = name-clone` (post-ES2015-dormant: ES2015 lowers shorthand first; ported faithfully, §7 unit-driven) |
| `createExpressionForMethodDeclaration` | 27443-27482 | `bd3aa684f7597a8b6f7df3c0671f980501e141759c75a5a4c79dcc9ccb281fb5` | `receiver.name = function (…) {…}` (post-ES2015-dormant: ES2015 lowers object methods first; ported faithfully, §7 unit-driven) |
| `createExpressionForObjectLiteralElementLike` | 27483-27498 | `fa28bb1dbba197796435533109e6e363d16c2e025051702a2212f763301e34b3` | the reduceProperty dispatcher (accessor/property/shorthand/method arms; PrivateIdentifier is a typed fault) |
| `getExpressionAssociativity` | 16003-16007 | `305a13c1344f1bf932c36db1bd830f5c27b1a81b610181add2f7b327303cb386` | binary dispatch: Left → cache-left arm, Right → target-decomposition arm |
| `getOperatorAssociativity` | 16008-16043 | `eb5fcb3da6d283ff2bb685355345d612ced69c8e299c81ab48ead1fa8691cf51` | the associativity table (assignment/exponent right; new-without-args right; else left) |
| `getOperator` | 16049-16057 | `6b59816eb86d900c9d66f39023f5e7d05def156ca74830bbf09eafdace4a8918` | operator extraction (binary token / unary operator / node kind) |
| `isLogicalOperator` | 17075-17077 | `b27722cefafa158e12d3a292e3145161aa29ba491af543906ab8ee77c924c7bb` | `\|\|`/`&&` (+`!`, unreachable as a binary token) — routes to `visitLogicalBinaryExpression` |
| `isBinaryLogicalOperator` | 17072-17074 | `459efae65b3f553b94abb065c588bb781640f5a21b1b8afa1207b17c9cb7a986` | the `\|\|`/`&&` half of the predicate |
| `getInitializedVariables` | 17421-17423 | `c8fe6eddb970f82b98bea9d71039c1b05f0ed2f3d794db496b853a12e77a7498` | `filter(declarations, isInitializedVariable)` |
| `isInitializedVariable` | 17424-17426 | `eb2bd20260ee3a6193c2d34e623393ed30c82bb8d5905c3c9fe588ba1e938849` | initializer presence |
| `isCompoundAssignment` | 93033-93035 | `cf363727b517ac8079c5b9f484d3874e50114346987a6065d811ac34416fc940` | token range 65..=79 |
| `getNonAssignmentOperatorForCompoundAssignment` | 93036-93069 | `92244f9073469f47d35d385e7aac910055f3863bb6192feacb69a8c31d6272d7` | `+=`→`+` … `??=`→`??` map |
| `getOriginalNodeId` | 92735-92738 | `469bb8aeb5a7c852e56997cea7c4513459ec2c30ae6c4d59ca93a5ec14c6c3b3` | rename-map key: the ORIGINAL node's identity (Rust: `TransformArena::get_original_node` + `TransformNode` as the key, factory.rs:414) |
| `isImportCall` | 14150-14154 | `74cfad37d8ed5b905210a6398b89c2c9f89f42600024f1749a0c92ab8c6c11f1` | the `visitCallExpression` guard (dynamic import is never apply-decomposed) |
| `copyPrologue` | 24827-24830 | `9c233c2771a89af4d6e7767d7a30253c9b999d5c25ac9ed62be08aba4149dfdb` | standard + custom prologue copy (the generator-body entry, `ensureUseStrict=false`, statements VISITED) |
| `copyStandardPrologue` | 24837-24857 | `7a83f5b2d0bfada432bb729b16e41de52a8cb69e13f5bdb19f627d23e06607f4` | leading string-literal directives (dedup via `use strict` detection is the ensure arm, OFF here) |
| `copyCustomPrologue` | 24858-24870 | `31ebe86b6ab3451c7d9470915ce9e462a96be87c7b4aad217af7b41d2a2df664` | `EmitFlags.CustomPrologue` statements after the directive prologue, visited |
| `visitParameterList` | 91168-91181 | `75f4e96e0f53dac4523f71d86dc9a4216465c88b670afeb6202b7853fb27d8fa` | start env → visit parameters → suspend env; the `addDefaultValueAssignmentsIfNeeded` arm is gated `target >= ES2015` and is DORMANT at the machine's ES5-input position (post-ES2015 inputs carry no parameter initializers; §12.6) |
| `visitIterationBody` | 91291-91305 | `b03d1c5c697121a89f1eb971763c4207e9dedffd27c6ac9545ac74a32d82f9bc` | visit + liftToBlock; the block-scope collection arms are inert on post-ES2015 input (no uncaptured block-scoped declarations; §12.6) |
| `startOnNewLine` | 27596-27602 | `c7324efd98cac9b986305d5d803c688160d42b9793ae3179178284ae6caa2321` | maps to `EmitMetadata::set_starts_on_new_line(true)` (metadata.rs:574; printer honors at printer.rs:5731/6366) |
| `addSyntheticTrailingComment` | 25385-25387 | `0d737de4db916ab368f7a0e39b7cf834acc42ce7d982268cd857e0b760da3716` | maps to `EmitMetadata::add_trailing_comment(SyntheticComment::new(MultiLine, text, false, false))` (metadata.rs:570); comment text is the UNPADDED instruction name (prints `[4 /*yield*/, 1]`) |
| `createValuesHelper` | 25897-25905 | `032848f776556d9246f0c0edb4f976921c507f9b47ad5fdcee88c17dc1b06688` | request `typescript:values` + `__values(expression)` call |
| `createGeneratorHelper` | 25915-25923 | `08b82812765a67b6725d6639fd1251deeff5667d74dbfbf5debbcdb1a509454f` | request `typescript:generator` + `__generator(this, body)` call |

List helpers (`reduceLeft` with `start<0 → 0`, `lastOrUndefined`,
`map`, `some`, `forEach`, `idText`) are `Vec`/iterator idioms exactly
as in B-2; trivial single-kind predicates
(`isFunctionLikeDeclaration` limited to the three generator-capable
kinds, `isBlock`, `isVariableDeclarationList`, `isIdentifier`,
`isLeftHandSideExpression` as the existing expression classification,
`isObjectLiteralElementLike`, `isStatement` tests) stay inline
`matches!`/classification idioms (es2018 precedent).

### 4.3 Frozen behavior pins

- **The two instruction alphabets.** OpCode (the recording alphabet,
  bundler-inlined): Nop=0, Statement=1, Assign=2, Break=3,
  BreakWhenTrue=4, BreakWhenFalse=5, Yield=6, YieldStar=7, Return=8,
  Throw=9, Endfinally=10. Instruction (the EMITTED alphabet inside
  `return [n, …]` arrays): Next=0, Throw=1, Return=2, Break=3,
  Yield=4, YieldStar=5, Catch=6, Endfinally=7; only 2/3/4/5/7 carry
  the synthetic `/*name*/` trailing comment (`getInstructionName`).
- **Code blocks.** kinds Exception=0, With=1, Switch=2, Loop=3,
  Labeled=4; block actions Open=0, Close=1; exception states Try=0 <
  Catch=1 < Finally=2 < Done=3. `beginBlock` records
  (action, offset=operations.length, block) and pushes the SAME block
  identity that `endBlock` pops (state mutates across begin/end —
  Rust: one arena `Vec<CodeBlock>` indexed by block id, §5).
- **Label protocol.** `defineLabel` allocates `nextLabelId++` with
  offset −1; `markLabel` sets `labelOffsets[label] =
  operations.length`; `createLabel(label>0)` emits a NumericLiteral
  placeholder `Number.MAX_SAFE_INTEGER` recorded in
  `labelExpressions[label]` and later REBOUND to the case number via
  `expression.text = String(labelNumber)` (`updateLabelExpressions`);
  `createLabel(undefined \| 0)` emits `createOmittedExpression()` (the
  `[0, 2, , 3]` trys-array holes). The Rust equivalent is the
  label-literal finalize-write (§5, §12.4).
- **Build protocol.** `build2` resets build state, runs
  `buildStatements` (writeOperation per op + `flushFinalLabel`), and
  wraps in
  `__generator(this, function (state) { … })` via
  `createGeneratorHelper(setEmitFlags(functionExpression,
  ReuseTempVariableScope))`; the callback block is
  `createBlock(buildResult, multiLine: buildResult.length > 0)`.
  With NO clauses the raw statements return (the switch-less
  empty-generator arm — the callback block prints MULTI-line
  `function (_a) {` / `return [2 /*return*/];` / `}`; the OUTER
  declaration body takes the single-line rule);
  with clauses the single statement is
  `startOnNewLine(createSwitchStatement(state.label,
  createCaseBlock(clauses)))`.
- **writeOperation dead-op rule.** `tryEnterLabel` →
  `tryEnterOrLeaveBlock` → `if (lastOperationWasAbrupt) return;` —
  operations after an abrupt writer in the SAME label are DROPPED
  (probe `return_bare`: the `[4 /*yield*/, 1]` after `return;` never
  emits, but the resume label still opens `case 1: _a.sent()`).
  Abrupt writers: Break/Return/Throw/Yield/YieldStar/Endfinally;
  completion writers (Return/Throw) additionally set
  `lastOperationWasCompletion` consulted by `isFinalLabelReachable`
  (the final `return [2 /*return*/]` is suppressed only when the last
  operation completed AND no label expression targets the final
  offset).
- **appendLabel wrapping.** with-block statements wrap
  `[createWithStatement(expr, createBlock(statements))]` innermost
  last; an OPEN `currentExceptionBlock` unshifts
  `state.trys.push([startLabel, catchLabel, finallyLabel, endLabel])`
  (createLabel per slot — absent slots print as omitted-expression
  holes) exactly ONCE (cleared after unshift); `markLabelEnd` appends
  `state.label = labelNumber + 1` when the label falls through
  (probes `if_else` case 3→4, `do_loop` case 1→2).
- **Label merging.** `tryEnterLabel` flushes once per operation index
  with pending statements; ALL labels at that offset map to the same
  case number (`labelNumbers[labelNumber].push(label)`).
- **Exception protocol.** `beginExceptionBlock` marks start, emits
  Nop; `beginCatchBlock`: generated catch names hoist as-is,
  source-named catch variables allocate `declareLocal(idText(name))`
  (unique name `e_1`), record
  `renamedCatchVariables.set(text)` +
  `renamedCatchVariableDeclarations[originalNodeId(variable)] = name`
  + `context.enableSubstitution(Identifier)` on FIRST rename, then
  `emitBreak(endLabel)`, mark catchLabel, state→Catch,
  `emitAssignment(name, state.sent())`, `emitNop`;
  `beginFinallyBlock`: `emitBreak(endLabel)`, mark finallyLabel,
  state→Finally; `endExceptionBlock`: state<Finally → break endLabel,
  else `emitEndfinally()`; mark endLabel, `emitNop`, state→Done.
- **Catch-rename substitution** (`onSubstituteNode`, Expression hint
  only): a NON-generated identifier whose text is in
  `renamedCatchVariables` resolves through
  `getOriginalNode` → (original is identifier with parent) →
  `resolver.getReferencedValueDeclaration(original)` →
  `renamedCatchVariableDeclarations[originalNodeId(declaration)]`;
  hit → CLONE of the replacement name with source-map/comment range
  of the substituted node. The rename applies at PRINT time (the
  transform does not descend into yield-free catch statements —
  probe `try_catch`: `handle(e)` → `handle(e_1)` via substitution
  only).
- **cacheExpression skip rule.** generated identifiers and
  `EmitFlags.HelperName` carriers pass through; EVERYTHING else
  (literals included — probe `yield_star_in_expr`: `_a = 1`) caches
  into `createTempVariable(hoistVariableDeclaration)` via an emitted
  assignment.
- **declareLocal.** named → `createUniqueName(text)` (`e_1` family);
  unnamed → `createTempVariable(void 0)`; BOTH hoist via
  `hoistVariableDeclaration`.
- **EmitFlags stamps.** write-side `return [...]` statements and the
  yield/break family take `NoTokenSourceMaps` (768) — EXCEPT
  `writeEndfinally`, which stamps nothing;
  `writeBreakWhenTrue`/`writeBreakWhenFalse` ifs take `SingleLine`
  (prints `if (!c) return [3 /*break*/, 2];` on one line — the
  statement-route plain `if (a)\n return […];` from
  `createInlineBreak` inside a visited if is NOT single-line, probe
  `continue_break`); `visitYieldExpression` consults
  `getEmitFlags(node.expression) & Iterator` (8388608) to SKIP the
  `__values` wrap — the consumer half of the `yield-star-synthesis`
  edge (B-4's loop conversion stamps Iterator on the synthesized
  iterator); `visitVariableStatement` returns `CustomPrologue`
  (2097152) statements unchanged (B-4-synthesized hoist carriers;
  dormant arm here).
- **State references.** `state.label` (read + assigned),
  `state.sent()` (resume value: `createGeneratorResume`, also the
  catch-arrival value), `state.trys.push([...])`; the state temp is
  created ONCE per generator body
  (`createTempVariable(void 0)` at `transformGeneratorFunctionBody`
  entry) and is the `__generator` callback's only parameter.
- **Naming order (probe-pinned).** Upstream defers generated-name
  spelling to first EMIT; with `ReuseTempVariableScope` the callback
  shares the outer function's temp scope. Net effect (probe
  `for_in`): hoisted temps `_a, _b, _c`, loop variable `_i`, THEN the
  state parameter `_d` — document order, one shared counter, `_i`
  excluded from the temp alphabet (the Rust allocator already skips
  ordinals 8/13, generated_bindings.rs:581). The Rust equivalence is
  the doc-order `finalize_generated_binding_names` walk plus the
  reuse-scope arm (§5, §12.3).
- **Hoisting placement.** `hoistFunctionDeclaration`ed inner
  functions and the hoisted `var` set merge AFTER the copied
  prologue and BEFORE `return __generator(…)`
  (`insertStatementsAfterStandardPrologue(statements2,
  endLexicalEnvironment())` — probe `hoisted_fn`:
  `function g() { function inner() { … } return __generator(…` and
  `use_strict_directive`: the directive stays first and the body
  prints multi-line).
- **Observable failure order.** `Debug.checkDefined` on every
  visited-expression read (absent/mistyped visit result is a
  defect); `Debug.failBadSyntaxKind` in `visitGenerator`'s default
  arm (a generator METHOD reaching the machine un-lowered — ES2015
  converts object/class generator methods first; typed
  `TransformError` + §7 fault contract); `beginCatchBlock` reads
  `variable.name` unconditionally (ES2019 optional catch binding is
  lowered before the machine; absent declaration is a typed error +
  §7 fault contract); `Debug.assert(labelOffsets !== undefined)` in
  `markLabel`; `Debug.assertLessThan(0, label)` in
  `createInlineBreak`; `peekBlock === undefined → Debug.fail` in
  `endBlock`; exception-state monotonicity asserts in
  `beginCatchBlock`/`beginFinallyBlock` (state ordinal) and the
  block-kind asserts in `endExceptionBlock`/`endWithBlock`/the loop,
  switch, and labeled enders. All
  become typed `TransformError` fail-closed arms, never silent
  fallbacks.
- **Implicit array trailing comma.** `createArrayLiteralExpression`
  adds `hasTrailingComma` when the LAST element is an omitted
  expression (`_tsc.js:22441-22449`, the `[,]` print shape) — the
  machine's chunked array rebuilds thread the rule.
- **Oracle behavior corpus** (probe recipe §7.3; 72 fixtures): the
  empty-generator switch-less body; single/bare/starred yields;
  resume values through `sent()`; the dead-op rule; if/do/while/for
  (incl. bare `for(;;)`)/for-in label protocols; plain and labeled
  continue/break against state-machine loops vs SCRIPT
  loops/switches/labeled blocks inside yield-containing statements
  (real `break`/`continue` survive); try/catch/finally incl. nesting
  (`trys` nesting, `endfinally`), catch renames `e_1`/`e_2` with
  print-time substitution, throw-in-try; switch clause chunking with
  `case _b.sent():` inline breaks and default routing; with-block
  re-wrapping per label; logical &&/`\|\|`, conditional, comma,
  compound-assign target decomposition, element/property-access
  target caching; array chunking through `temp.concat([...])` (holes
  ride the initial chunk `[,]`); object chunking through
  `temp.y = …` assignments incl. accessor
  `Object.defineProperty(…, { enumerable: false, configurable:
  true, get/set })` and string/numeric member access; call/new
  apply-decomposition (`_a.apply(void 0, [..])`,
  `_b.apply(_a, _c.concat([…]))`, `new (_a.apply(C, _b.concat([…])))()`);
  hoisted inner functions/generators; prologue directives; parameter
  pass-through; multi-line object folding with `startOnNewLine`
  continuation layout; generator function expressions (anonymous +
  named); nested generators (fresh inner temp alphabet); a
  non-generator sibling function passing through byte-identical.

## 5. Rust semantic map

New dormant module `crates/emitter/src/builtins/generators.rs`
(registered as `mod generators;` in `crates/emitter/src/builtins.rs`
— this deliberately retires the gap matrix's asserted absence).
Function-per-function port; every ported fn carries the
`tsc-port`/`tsc-hash`/`tsc-span` header from §4 and
`#[allow(dead_code)] // production consumers arrive with the B-5 owner`
where caller-less.

| Item | Target |
|---|---|
| rename state | `RenameState` on `GeneratorsTransformer` (NOT the visitor): `renamed_catch_variables: BTreeMap<String, ()>` + `renamed_catch_variable_declarations` keyed by the parse-tree resolver identity — TRANSFORMER-lifetime because `onSubstituteNode` consults the maps at PRINT time, after `transform_root` returns, and the enable-substitution latch fires once at the first rename (`_tsc.js:109394-109398`); the maps are NOT in the upstream per-body save/restore set |
| module seam | `pub(super) struct GeneratorsTransformer<'resolver>` implementing `Transformer` (`name = "transformGenerators"`, `transform_root`, `substitute_node`), built by `pub(super) fn transform_generators<'resolver>(language_version: ScriptTarget, resolver: &'resolver dyn EmitResolver) -> Box<dyn Transformer + 'resolver>` — the es2017.rs:110 seam verbatim; B-5 registers it; the §7 suite constructs it directly and runs the REAL transformer through `transform_nodes` (no projection driver — the Rust side under test is the production code path) |
| source-file gate | `transform_root`: `initialize_transform_flags`, then the `transformSourceFile` gate (`is_declaration_file \|\| (flags & CONTAINS_GENERATOR) == 0 → identity`), `visit_each_child` via the visitor, then helper attachment exactly as the es2017/es2018 `transform_root` tails |
| visitor state | `struct GeneratorsVisitor<'context, 'resolver>` holding the upstream closure state 1:1: `in_generator_function_body: bool`, `in_statement_containing_yield: bool`, the recording arrays (`operations: Vec<OpCode>`, `operation_arguments: Vec<Option<Vec<TransformNode>>>`, `operation_locations: Vec<Option<TransformNode>>`), label state (`label_offsets: Vec<Option<usize>>` indexed by label id, `label_expressions` as the §12.4 patch ledger, `next_label_id: usize` starting 1), block state (`blocks: Vec<CodeBlock>` arena + `block_actions: Vec<(BlockAction, usize /*offset*/, BlockId)>` + `block_stack: Vec<BlockId>`), the state binding (`state: Option<TargetBinding>`), the `label_expressions` ledger, and the build-phase fields (`block_index`, `label_number`, `label_numbers: Vec<Option<Vec<usize>>>`, `last_operation_was_abrupt/_completion`, `clauses: Option<Vec<TransformNode>>`, `statements: Option<Vec<TransformNode>>`, `exception_block_stack: Vec<Option<BlockId>>`, `current_exception_block: Option<BlockId>`, `with_block_stack: Vec<BlockId>`); NO visit memoization (the visitor is stateful — the es2018 memo map is deliberately not replicated) |
| code blocks | `enum CodeBlock { Exception { state: ExceptionBlockState, start_label, end_label, catch_variable: Option<TransformNode>, catch_label: Option<usize>, finally_label: Option<usize> }, With { expression: TransformNode }, Switch { is_script: bool, break_label: usize /* -1 ⇔ 0-sentinel via Option */ }, Loop { is_script: bool, break_label: usize, continue_label: usize }, Labeled { is_script: bool, label_text: String, break_label: usize } }` — one `Vec<CodeBlock>` arena; `block_stack` holds indices so `beginCatchBlock`'s in-place state transitions mutate the SAME record `endBlock` later pops (upstream object identity); script sentinels (`breakLabel: -1`) become label 0 (the shared "no target" value `findBreakTarget`/`findContinueTarget` return) |
| label back-patching | the LABEL-LITERAL FINALIZE-WRITE (§12.4): `create_label` mints the upstream `Number.MAX_SAFE_INTEGER` placeholder and records it in the `label_expressions` ledger — from BOTH phases (visit-time `createInlineBreak` sites inside recorded operation arguments AND build-time writers); after the single build pass, `update_label_expressions` assigns every recorded literal its final case number through the new typed arena finalize-write `TransformArena::set_numeric_literal_text` (factory.rs — the exact `set_generated_identifier_text`/E-NAMES finalize precedent at factory.rs:287, NumericLiteral arm; the sole caller is `update_label_expressions`). The ledger is per-body state (saved/restored by `transform_generator_function_body` exactly as upstream saves `labelExpressions`), accumulates across recording and build within one body, is never reset in between, and `is_final_label_reachable` consults its per-label presence |
| opcode/instruction | `enum OpCode` (11 variants, §4.3 values) and `enum Instruction` (8 variants) with `Instruction::comment_text() -> Option<&'static str>` (the `getInstructionName` port); `create_instruction` = numeric literal + `metadata_mut(node).add_trailing_comment(SyntheticComment::new(MultiLine, name, false, false))` |
| state temp | allocated as `TargetBinding::allocate` at `transform_generator_function_body` entry (upstream `createTempVariable(void 0)` — NOT hoisted); every `state` reference mints a fresh identifier via `create_generated_identifier` (es2018.rs:3966 precedent); the `__generator` callback `FunctionExpression` carries `EmitFlags::REUSE_TEMP_VARIABLE_SCOPE` and the finalize walk (below) keeps the shared temp alphabet |
| reuse-scope finalize arm | `crates/emitter/src/builtins/target_bindings.rs`: the finalize/collect walk (`is_function_scope_kind` gate at :573) additionally consults the node's `EmitFlags::REUSE_TEMP_VARIABLE_SCOPE` — a flagged function-scope node does NOT open a fresh generated-name scope. Corpus-inert by construction: the flag's only current producer is es2017's awaiter body (es2017.rs:1777), and T0=100.0000% at the trusted base proves no corpus fixture distinguishes the arms (a distinguishing fixture would already mismatch tsc today, §12.3); the arm gains its own unit contracts in the target-bindings suite |
| temp/name allocation | hoisted temps `allocate_temp` (+ `context.hoist_variable_declaration`), `declareLocal(text)` → `TargetBinding::allocate_numbered` (`e_1` family), `createLoopVariable` → the dormant `GeneratedBindingScopes::allocate_loop_variable` (first production caller; `_i` family), all under the E-NAMES-H eager model with doc-order finalization |
| parsed-tree generator facet | `crates/emitter/src/builtins.rs` `local_transform_flags`: the `FunctionDeclaration` (:13711), `FunctionExpression` (:13721), and `MethodDeclaration` arms gain the exact factory conditional (`isAsyncGenerator ? CONTAINS_ES_2018 : isAsync ? CONTAINS_ES_2017 : isGenerator ? CONTAINS_GENERATOR : NONE` — the vendored `createFunctionExpression` flag row `_tsc.js:22685-22688`, already ported as `function_facets` factory.rs:695-705); async-ness from the modifier list, generator-ness from `asterisk_token.is_some()`; existing bits in those arms are NOT removed. The SAME completion applies to the completion statements: `createContinueStatement`/`createBreakStatement` stamp `ContainsHoistedDeclarationOrCompletion` (`_tsc.js:23177`/`:23188`) and the parsed-tree initializer lacked the arm — without it the machine's generator-body dispatch never descends to `if (a) continue;` shapes (measured live: the visit-phase inline-break projections). Corpus-inert both ways: zero readers of `CONTAINS_GENERATOR` outside the factory classifier (factory.rs:701, write side) and ZERO readers of `CONTAINS_HOISTED_DECLARATION_OR_COMPLETION` in any active transform (measured — every non-generators hit is a write site), and the corpus ratchet is the enforcement |
| helper identifiers | `EmitHelperName::{Generator, Values}` variants + `text()` arms `"__generator"`/`"__values"` (`crates/emitter/src/factory.rs:19/:45`); calls built as `request_emit_helper(helpers::generator()/values())` + `create_unscoped_helper_identifier` (es2017.rs:1738-1793 `create_awaiter_call` template) |
| call binding | module-internal `create_call_binding(host, expression, language_version, cache_identifiers) -> (target, this_arg)` porting §4.2 (`skipOuterExpressions` at All + `shouldBeCapturedInTempVariable`); the two super arms are post-ES2015-unreachable at the machine's pipeline position and port as typed fail-closed errors with the pinned justification (upstream reaches them only from pre-ES2015 super positions that transformES2015 has already rewritten) |
| apply/concat/bind shapes | module-internal constructors over `create_node`: `createFunctionApplyCall` (`target.apply(thisArg, args)`), `createArrayConcatCall` (`temp.concat([…])`), the `new (target.bind.apply(C, [void 0, …]))()` new-decomposition, `createPostfixIncrement`, `createLessThan`, `createLogicalNot` — the es2017/es2018 `create_property_access` + `create_call` idiom (es2017.rs:677 precedent); parenthesization is factory-automatic (`apply_parenthesizer_rules`, factory.rs:1113) |
| node construction | module-internal `create_*` wrappers over `self.context().factory()?.create_node(source, NodeData::…, flags)` with flags from `TransformArena::propagate_child_flags`/`child_flags` folds (EA-GAP-FLAGS discipline); constructors needed beyond B-2's set: switch/case-block/case-clause/default-clause, with, for-in, throw, return, labeled, conditional, postfix/prefix unary, omitted expression, parameter, function expression/declaration, object/array literal, property/element access, call/new, block, numeric/void-0 literals |
| provenance & ranges | `set_text_range` / `set_original_node` exactly as B-2; `setSourceMapRange` → `EmitMetadata::set_source_map_range` (metadata.rs:543; byte-inert without a map recorder but ported faithfully); `setCommentRange` → `set_comment_range` (metadata.rs:551 — the hoisted clone in `transformAndEmitVariableDeclarationList` carries the source name's comment range) |
| substitution | `GeneratorsTransformer::substitute_node` (Expression hint → identifier → the §4.3 rename walk; previous-first chaining is the harness's job per the B-1 order contracts); `context.enable_substitution(SyntaxKind::Identifier)` at the FIRST catch rename (transform.rs:472; class_fields.rs:58 precedent); `is_generated_identifier` = `metadata.generated_binding_id().is_some()` (builtins.rs:6756 idiom); `EmitFlags::HELPER_NAME` read for the cacheExpression skip (factory.rs:400 idiom) |
| prologue | module-internal `copy_prologue` port (§4.2 spans; standard directives + visited custom-prologue carriers), the es2017/es2018 `prologue_end` cursor precedent (es2017.rs:2132, es2018.rs:4886) for `insertStatementsAfterStandardPrologue` |
| generic descent | `visit_each_child` fallbacks via `try_visit_each_child` + `NodeDataChildVisitor` (for_each_child.rs:3036/3045; es2018.rs:5087 impl precedent); `visitParameterList`/`visitIterationBody` as module-internal fns per §4.2 with the pinned-dormant arms |
| comment/printer surfaces | untouched: synthesized output only, no comment-scope threading change, no printer edit (the case-clause single-statement inline rule, block single-line rule, `starts_on_new_line`, `SINGLE_LINE`, and synthetic trailing comments are measured present — printer.rs:5624/5731/6915/11507 + the active es2017 corpus shapes and active_transform_contract.rs:2682/2812 contracts; §12.7) |

Producer/owner/consumer per row: the module is the sole producer of
its synthesized subtrees; `TransformationContext` owns hoisted
declarations and helper requests; the state/rename/label/block
records live in the visitor for exactly one generator body
(`transformGeneratorFunctionBody` saves and restores ALL of them on
entry/exit — nested generators get fresh machines); invalidation
follows the arena (nodes immutable once created, with ONE typed
exception: the deferred label literals complete their text through
the §12.4 finalize-write before the transform returns).

## 6. Current local-gap matrix (B-3 row, from the frozen artifact)

| Capability | State | Anchor evidence | Absence evidence |
|---|---|---|---|
| `generator-state-machine` | `missing` | — | module `generators.rs` asserted absent from `crates/emitter/src/builtins`; requirement: "the transformGenerators state machine (labels, try/catch protocol, instruction encoding via createGeneratorHelper)"; surfaces `yield-star-synthesis`, `helper-factory`; architecture rows `EA-GAP-COMPOSITION`, `E-ORDER-H` |

Creating `crates/emitter/src/builtins/generators.rs` trips the
absence exactly as the generator intends; the §8.1 amendment is that
reviewed re-disposition (row 10 `missing` → `exists`, counts
10/0/3 → 11/0/2).

## 7. Implementation sequence (dependency order; every step corpus-inert)

Fence: `crates/emitter/src/builtins/generators.rs` (new),
`crates/emitter/src/builtins.rs` (the `mod generators;` line + the
function-like facet arms and the completion-statement facet arm of
the parsed-tree flag initializer ONLY; the target rejection at
:144-150 is read-only), `crates/emitter/src/factory.rs` (the two
`EmitHelperName` variants + `text()` arms + the
`TransformArena::set_numeric_literal_text` finalize-write ONLY),
`crates/emitter/src/builtins/target_bindings.rs` (the
reuse-temp-variable-scope finalize arm ONLY),
`crates/emitter/tests/unit/generators/tests.rs` (new, attached with
the `#[cfg(test)] #[path]` idiom),
`crates/emitter/tests/unit/target_bindings_tests.rs` (reuse-arm
contracts), and the §8 evidence set. `es2017.rs`, `es2018.rs`,
`helpers.rs`, `printer.rs`, and every other production file are out
of fence.

1. **Machine leaves.** `OpCode`/`Instruction`/`CodeBlock`/
   `ExceptionBlockState`/`BlockAction` enums; the label/block
   primitives (`define_label`, `mark_label`, `begin_block`,
   `end_block`, `peek_block`, the twelve begin/end block fns, the
   find-target walkers, `hasImmediateContainingLabeledBlock`);
   `create_label`/`create_instruction`/`create_inline_break`/
   `create_inline_return`/`create_generator_resume`; the emit
   recorders (`emit_worker` + the eleven wrappers); the §4.2
   accessor/predicate/prologue/call-binding addenda;
   `EmitHelperName::{Generator, Values}`; the `local_transform_flags`
   facet arms; the target_bindings reuse-scope arm.
   Check: leaf unit contracts green (label offsets, block
   begin/end pairing, find-target walkers incl. the
   labeled-immediate-containment rule, call-binding arms,
   instruction comment texts); reuse-arm contracts in the
   target-bindings suite; `cargo xtask ledger check` stale=0
   undispositioned=0 (headers at authoring time — the B-1 lesson).
2. **Visitors and emitters.** The three-mode visitor dispatch
   (`visitor`, `visitJavaScriptInStatementContainingYield`,
   `visitJavaScriptInGeneratorFunctionBody`,
   `visitJavaScriptContainingYield`, `visitGenerator` incl. the
   typed-fault default), function/accessor save-restore arms,
   `transformGeneratorFunctionBody` (state save/restore + prologue
   copy + hoist merge), the transformAndEmit* statement family
   (block/expression/variable-list/if/do/while/for/for-in/
   continue/break/return/with/switch/labeled/throw/try), the
   script-block visit* family, the expression visitors
   (binary right/left-associative, logical, comma incl.
   `visitCommaListExpression`, conditional, yield/yield*, array via
   `visitElements`, object via reduce-property, element access,
   call, new), `cacheExpression`/`declareLocal`, catch-rename
   bookkeeping + `substitute_node`. Register `mod generators;`.
   Check: `cargo build -p tsc-rs-emitter` + clippy clean; protocol
   unit contracts green (dead-op rule, label merge, trys hole
   shapes, dispatch fall-through matrix).
3. **Build pipeline + focused projections.** `build`/
   `build_statements`/`flush_label`/`flush_final_label`/
   `is_final_label_reachable`/`append_label`/`try_enter_label`/
   `try_enter_or_leave_block`/`write_operation` + the nine writers,
   in the single-pass protocol with the §12.4 label finalize-write;
   the `__generator` wrap. Then the suite:
   `crates/emitter/tests/unit/generators/tests.rs` drives the REAL
   `GeneratorsTransformer` per fixture — parse →
   `transform_nodes(arena, [SourceFile], [transform_generators(ES5,
   &FixtureResolver)], …)` → `create_printer(PrinterOptions::new(LF)
   .with_target(ES5))` → byte-compare against the frozen oracle
   emits (full output bytes INCLUDING the `__generator`/`__values`
   preludes; the B-2/active_transform_contract.rs:4877 precedent).
   `FixtureResolver` implements `get_referenced_value_declaration`
   by lexical catch-clause resolution over the parse tree (§12.5)
   and inherits every other typed fail-closed default.
   **Fixture-language constraint (§12.6):** every fixture is
   ES5-plus-generator-syntax ONLY — `var`-only bindings, no
   let/const/arrow/template/class/destructuring/default/rest/spread/
   for-of, no object-literal computed keys/shorthand/methods (in
   generator-relevant positions), no `super`, no dynamic import; and NO inner binder (parameter,
   function name, or `var`) reuses an outer catch-variable name
   (nested catch clauses may — the rename maps key per declaration) —
   so the upstream pipeline's passes before Generators are
   extensionally the identity, full-emit equality is a pure machine
   projection, and the §12.5 fixture resolver is exact (all 72
   frozen fixtures verified against the constraint at authoring).
   The 72 oracle fixtures (exact sources; expected bytes frozen in
   the suite from the probe output; multi-line sources notate
   newlines as `⏎`):
   - `empty` — `function* g() { }`
   - `yield_one` — `function* g() { yield 1; }`
   - `yield_bare` — `function* g() { yield; }`
   - `yield_value_use` — `function* g() { var a = yield 1; use(a); }`
   - `yield_star` — `function* g() { yield* h(); }`
   - `two_yields` — `function* g() { yield 1; yield 2; }`
   - `return_value` — `function* g() { yield 1; return 2; }`
   - `return_bare` — `function* g() { return; yield 1; }`
   - `throw_stmt` — `function* g() { yield 1; throw new Error("e"); }`
   - `if_else` — `function* g() { if (c) { yield 1; } else { yield 2; } done(); }`
   - `do_loop` — `function* g() { do { yield 1; } while (c); }`
   - `while_loop` — `function* g() { while (c) { yield 1; } }`
   - `for_loop` — `function* g() { for (var i = 0; i < n; i++) { yield i; } }`
   - `for_in` — `function* g() { for (var k in o) { yield k; } }`
   - `continue_break` — `function* g() { while (c) { if (a) continue; if (b) break; yield 1; } }`
   - `labeled_break` — `function* g() { outer: while (c) { while (d) { yield 1; break outer; } } }`
   - `try_catch` — `function* g() { try { yield 1; } catch (e) { handle(e); } }`
   - `try_catch_use` — `function* g() { try { yield 1; } catch (e) { yield e; } tail(e2); }`
   - `try_finally` — `function* g() { try { yield 1; } finally { cleanup(); } }`
   - `try_catch_finally` — `function* g() { try { yield 1; } catch (e) { handle(e); } finally { cleanup(); } }`
   - `switch_yield` — `function* g() { switch (t) { case a: yield 1; break; case yield 2: done(); break; default: other(); } }`
   - `with_stmt` — `function* g() { with (o) { yield m; } }`
   - `logical_and` — `function* g() { var r = a() && (yield 1); use(r); }`
   - `conditional` — `function* g() { var r = c ? (yield 1) : (yield 2); use(r); }`
   - `comma_expr` — `function* g() { var r = (a(), yield 1, b()); use(r); }`
   - `binary_cache` — `function* g() { var r = a() + (yield 1); use(r); }`
   - `compound_assign` — `function* g() { o.p += yield 1; }`
   - `elem_access` — `function* g() { var r = o[yield 1]; use(r); }`
   - `array_literal` — `function* g() { var r = [a(), yield 1, b()]; use(r); }`
   - `object_literal` — `function* g() { var r = { x: a(), y: yield 1, z: b() }; use(r); }`
   - `call_args` — `function* g() { f(a(), yield 1, b()); }`
   - `method_call_args` — `function* g() { o.m(a(), yield 1); }`
   - `new_args` — `function* g() { var r = new C(a(), yield 1); use(r); }`
   - `hoisted_fn` — `function* g() { yield 1; function inner() { return 2; } use(inner); }`
   - `plain_vars` — `function* g() { var a = 1, b = f(); yield a + b; }`
   - `nested_fn` — `function* g() { var h = function () { return 1; }; yield h(); }`
   - `nested_generator` — `function* g() { function* inner() { yield 1; } yield* inner(); }`
   - `script_loop_in_yield_stmt` — `function* g() { while (yield 1) { for (var i = 0; i < n; i++) { if (i) break; } } }`
   - `non_generator_untouched` — `function f() { return 1; }⏎function* g() { yield f(); }`
   - `use_strict_directive` — `function* g() { "use strict"; yield 1; }`
   - `params` — `function* g(a, b) { yield a + b; }`
   - `fn_expr_generator` — `var h = function* () { yield 1; };⏎use(h);`
   - `named_fn_expr_generator` — `var h = function* gen() { yield 1; };⏎use(h);`
   - `accessor_nested_generator` — `function* g() { yield 1; var o = { get p() { function* h() { yield 2; } return h; } }; use(o); }`
   - `for_bare` — `function* g() { for (;;) { yield 1; if (c) break; } }`
   - `script_do_in_yield_stmt` — `function* g() { while (yield 1) { do { x(); } while (d); } }`
   - `script_switch_in_yield_stmt` — `function* g() { while (yield 1) { switch (x) { case 1: break; default: other(); } } }`
   - `script_labeled_in_yield_stmt` — `function* g() { while (yield 1) { inner: while (d) { break inner; } } }`
   - `labeled_block_break` — `function* g() { lbl: { yield 1; break lbl; done(); } tail(); }`
   - `if_no_else` — `function* g() { if (c) { yield 1; } done(); }`
   - `nested_try` — `function* g() { try { try { yield 1; } finally { inner(); } } catch (e) { handle(e); } }`
   - `catch_generated_reuse` — `function* g() { try { yield 1; } catch (e) { use(e); } try { yield 2; } catch (e) { use2(e); } }`
   - `throw_in_try` — `function* g() { try { yield 1; throw bad(); } catch (e) { handle(e); } }`
   - `continue_in_for_yield_incr` — `function* g() { for (var i = 0; i < n; i++) { if (skip(i)) continue; yield i; } }`
   - `logical_or` — `function* g() { var r = a() || (yield 1); use(r); }`
   - `yield_in_condition_only` — `function* g() { var r = (yield 1) ? a() : b(); use(r); }`
   - `elem_access_target_assign` — `function* g() { o[k()] = yield 1; }`
   - `prop_access_target_assign` — `function* g() { o.p = yield 1; }`
   - `yield_star_in_expr` — `function* g() { var r = 1 + (yield* h()); use(r); }`
   - `var_multi_split` — `function* g() { var a = p(), b = yield 1, c = q(); use(a, b, c); }`
   - `obj_accessor_initial_chunk` — `function* g() { var r = { get p() { return 1; }, y: yield 1 }; use(r); }`
   - `obj_accessor_post_yield` — `function* g() { var r = { y: yield 1, get p() { return 1; } }; use(r); }`
   - `obj_accessor_pair_post_yield` — `function* g() { var r = { y: yield 1, get p() { return q; }, set p(v) { s(v); } }; use(r); }`
   - `obj_method_post_yield` — `function* g() { var r = { y: yield 1, m: function () { return 2; } }; use(r); }`
   - `obj_string_numeric_after_yield` — `function* g() { var r = { y: yield 1, "s p": a(), 1: b() }; use(r); }`
   - `multiline_obj` — `function* g() {⏎  var r = {⏎    x: a(),⏎    y: yield 1,⏎    z: b()⏎  };⏎  use(r);⏎}`
   - `multiline_body` — `function* g() {⏎  yield 1;⏎}`
   - `array_leading_hole` — `function* g() { var r = [, yield 1]; use(r); }`
   - `array_yield_first` — `function* g() { var r = [yield 1, a()]; use(r); }`
   - `call_yield_first_arg` — `function* g() { f(yield 1, a()); }`
   - `elem_call_args` — `function* g() { o[m()](a(), yield 1); }`
   - `paren_call_target` — `function* g() { (0, o.m)(a(), yield 1); }`
   Fault-shaped typed-error contracts (not oracle-mintable — the
   composition pipeline shields them upstream): a generator
   MethodDeclaration reaching `visit_generator`
   (`Debug.failBadSyntaxKind`) and a binding-less catch clause
   reaching `begin_catch_block` (upstream undefined-deref on
   `variable.name`).
   The exact probe (frozen; §7.3-command):

   ```js
   // node b3-probe.mjs — vendored oracle, single virtual file,
   // target ES5, alwaysStrict:false (no prologue), LF; full emitted
   // bytes are the expectation.
   // (identical emitCase host to the B-2 §7.3 probe; only CASES
   // and the fixed ES5 target differ)
   ```

   Oracle bytes are the entire expectation (no hand-authored
   output). Check: focused suite green (72 byte-equal projections +
   the typed fault contracts); `cargo test -p tsc-rs-emitter` fully
   green with zero expected-string changes outside the new suite.
4. **Train items.** §8 amendments, chain walk (b3-walk.sh = the
   B-2 walk with this scratchpad's path; qualification BEFORE
   profile; walk BEFORE `--lane rust` — the crate-byte train rule;
   NEW_RUNTIME_INPUTS closure for the two new crates files
   (243→245) + the generator-internal size consts + schema
   minItems/maxItems + gap-matrix schema summary consts), envelope
   `h2-5h-b-b-3` (`ready`, predecessor `h2-5h-b-b-2` receipt
   `08ca53f568b3527609d6fda8eed2a47072d10979fb76331dea29ef92fbe8b72f`),
   bootstrap `allowedPacketIds += h2-5h-b-b-3`, index row, full
   local gate at the final head from the canonical repository path
   (detached launcher; demoted; perf-only-red → normal-priority
   resume per protocol).

## 8. Evidence, ratchet, and documentation amendments

1. **Gap matrix generator** (`crates/oracle/h2-5h-a-gap-matrix.mjs`
   row 10): `state: "missing"` → `"exists"`; anchors gain
   `crates/emitter/src/builtins/generators.rs`
   `transform_generators` / `GeneratorsTransformer` /
   `transform_generator_function_body` (real symbols — the F1
   lesson); the module absence retires (`absences: []`); note
   records the landing, dormancy, and the B-4/B-5 remaining scope;
   summary counts 10/0/3 → 11/0/2 (the SCHEMA pins the summary
   counts as consts — both files change together, the B-2 lesson).
2. **Dispositions generator**
   (`crates/oracle/h2-5h-a-dispositions.mjs`): `EA-GAP-COMPOSITION`
   rationale gains the B-3 landing clause (disposition stays
   `activate`); the `E-ORDER-H` rationale notes the dormant
   substitution owner landing if its row cites the capability; no
   other row changes; the manifest re-mints with the new gap-matrix
   lineage.
3. **Architecture map**
   (`docs/design/greenfield/emitter-architecture.md`): the
   `EA-GAP-COMPOSITION` section's B-2 substrate preamble extends
   with the B-3 machine landing (module, transformer seam, dormancy,
   remaining B-4/B-5 scope); NO heading or table-row identity
   changes.
4. **Handoff** `h2-5h-a.md`: the ladder's B-3 bullet gains its
   **LANDED** marker at the final implementation-step commit ⇒
   envelope `h2-5h-a` re-pin + doc-pinning witness re-mints
   (adoption: seconds).
5. **Chain walk**: b3-walk.sh (the B-2 walk verbatim, scratchpad
   path updated); qualification BEFORE profile; the new-crates-file
   closure (NEW_RUNTIME_INPUTS mjs list + schema minItems/maxItems
   + the generator-internal runtimeInputSet.size consts, 243→245);
   step 7 re-mints foundation/comment-scope witnesses (adoption),
   owner-graph, gap-matrix, dispositions, es2015-generators
   witnesses; pin-sweep audit before the gate after any
   multi-attempt walk.
6. **Readiness**: envelope
   `ratchets/fci-readiness/h2-5h-b-b-3.v1.json` (`ready`; fence =
   §7 + the walk set), bootstrap `allowedPacketIds += h2-5h-b-b-3`,
   index row in `slices/README.md`.

## 9. Acceptance

- All 129 owner functions + the §4.2 addenda landed with ledger
  headers; `cargo xtask ledger check` green (stale=0,
  undispositioned=0, todo_port=0).
- Focused projection suite green: all 72 fixtures byte-equal to
  their oracle-minted expectations (yield/resume/labels, loops,
  try/catch/finally + rename substitution, switch/with, expression
  decomposition, array/object chunking, call/new apply forms,
  prologue/hoisting, nested generators), typed fault contracts
  green, provenance contracts (original/range/SYNTHESIZED) green.
- `cargo test -p tsc-rs-emitter` fully green; zero expected-string
  changes outside the new focused suite and the target-bindings
  reuse-arm contracts.
- Gap matrix re-minted: row 10 `exists`, counts 11/0/2;
  dispositions + owner-graph + witnesses re-minted through the
  walk; architecture map and handoff amended per §8.
- Corpus ratchet: T0=100.0000% 49024/49024 FP=0, all bands, tiers —
  byte-identical (foundation packet; zero output change; the
  parsed-tree facet arms and the reuse-scope finalize arm are
  corpus-inert by the §5 measured arguments and the ratchet is the
  enforcement).
- Packet checker `slice-readiness --check h2-5h-b-b-3`; complete
  local gate green at the final head from the canonical path.

## 10. Traceability

| Invariant | Target | Test | Evidence |
|---|---|---|---|
| owner fidelity (129 fns) | `generators.rs` ports | ledger d2 headers + protocol contracts | §4.1 spans/hashes + owner-graph byte hashes |
| instruction/label encoding | `create_instruction`/`create_label` + the §12.4 finalize-write | byte projections (`[4 /*yield*/, …]`, trys holes, label rebinds) | §4.3 alphabets + probe corpus |
| try/catch protocol | exception block family + writers | try/catch/finally/nested projections + fault contract | §4.3 exception pins |
| catch rename + substitution | `substitute_node` + rename maps + resolver query | rename projections (`e_1`/`e_2`, print-time rename) | §4.3 substitution pin + B-1 order contracts |
| yield-star consumer edge | `visitYieldExpression` Iterator skip + YieldStar writer | yield* projections (wrapped) + the Iterator-skip unit contract (synthesized-flag driven) | owner-graph `yield-star-synthesis` edge |
| temp/naming equivalence | state binding + reuse-scope finalize arm | for_in/with/switch naming projections (`_a…_i…state`) + reuse-arm unit contracts | E-NAMES-H argument (B-1 §12.3) + §12.3 |
| parsed-tree generator facet | `local_transform_flags` arms | facet unit contracts + the source-file gate projection | vendored factory row `_tsc.js:22685-22688` + factory.rs:695 |
| synthesized-flag correctness | module `create_*` wrappers | facet assertions in the suite | EA-GAP-FLAGS tables (B-1) |
| dormancy | no registration edit; `builtins.rs` mod line + facet arms only | untouched-file assert + ratchet | corpus byte-identity |
| consumer-surface completeness | `Transformer` impl (transform_root + substitute_node) | compile-time (B-5 registration shape typed) | §5 module seam |

Resources: the focused suite is plain `cargo test` (no worker
ceilings); the walk and gate follow the standing demotion directive
(`taskpolicy` maintenance/`nice`), with the perf-ceiling
normal-priority resume exception.

## 11. Prohibitions

No transformer registration or activation change; no corpus
output-byte change (the ratchet is the enforcement); no printer,
helpers.rs, es2017.rs, or es2018.rs edit; no ES2015 visitor, loop
conversion, or tagged-template code (B-4/B-5 scope); no witness
amendment; no ad-hoc in-place node mutation — label literals
finalize EXCLUSIVELY through the typed
`TransformArena::set_numeric_literal_text` finalize-write (the
`set_generated_identifier_text`/E-NAMES precedent, factory.rs:287
NumericLiteral arm), applied only by `update_label_expressions`; no
generic fallback that converts an
unknown branch into success (every unexpected shape is a typed
`TransformError`); no fixture-specific branches or hand-authored
expected output (oracle bytes only); the CS and B-1/B-2 prohibitions
remain. This document authorizes no production edit until its own
design-gate pass and envelope exist.

## 12. Unresolved items (all closed at authoring, 2026-08-22)

1. ~~Trusted base + authority hashes~~ — pinned in §1 at
   `28f04d95ac8e413cf63b95146dda940143b791d6`; the §8 amendments
   re-mint the artifacts through this packet's own gate.
2. ~~Parsed-tree `CONTAINS_GENERATOR`~~ — RESOLVED: the parsed-tree
   initializer (`local_transform_flags`) never sets the generator
   facet (measured: no `asterisk_token` consultation; the token arm
   has no AsteriskToken row), so the machine's source-file gate and
   dispatch would never fire on parsed input. The fix mirrors the
   vendored factory conditional (`_tsc.js:22685-22688`) exactly and
   is corpus-inert: the only non-test reader of
   `CONTAINS_GENERATOR` is the factory classifier itself
   (factory.rs:701, write side), no active transform consults the
   bit, and the full-corpus ratchet enforces byte identity.
3. ~~State-temp naming across the `__generator` boundary~~ —
   RESOLVED: upstream names temps at first emit under
   `ReuseTempVariableScope` (probe `for_in`: hoisted `_a.._c`, loop
   `_i`, state `_d` — document order, one scope). The Rust
   equivalence: `finalize_generated_binding_names` already assigns
   final spellings in document order over the finished tree; the
   missing half is the reuse arm — a function-scope node whose
   metadata carries `EmitFlags::REUSE_TEMP_VARIABLE_SCOPE` must not
   open a fresh scope (target_bindings.rs:573 gate). Corpus-inert:
   es2017 (the flag's only current producer) is corpus-active at
   T0=100.0000%, so no existing fixture distinguishes the arms —
   one that did would already mismatch tsc under the current
   fresh-scope behavior. The arm lands with its own unit contracts;
   the B-1 E-NAMES-H per-arm equivalence argument extends with this
   row.
4. ~~Upstream label-expression mutation
   (`expression.text = String(n)`)~~ — RESOLVED (review r1, the one
   blocker): the LABEL-LITERAL FINALIZE-WRITE. Placeholder label
   literals are minted in TWO phases — build-phase writers
   (`writeBreak`/`writeBreakWhenTrue`/`writeBreakWhenFalse`, the
   `appendLabel` trys array) AND visitation-phase `createInlineBreak`
   sites (switch clause chunking `_tsc.js:109155-109166`,
   script-route break/continue `:109069-109106`) whose literals ride
   inside recorded operation arguments that the build re-emits
   verbatim — so no build-only scheme can reach them all. The port
   mirrors upstream exactly: `create_label` records every
   placeholder in the `label_expressions` ledger (both phases; the
   ledger is per-body saved/restored state, accumulates across
   recording and build within one body, and is never reset in
   between), `try_enter_label` records the label → case-number map,
   and `update_label_expressions` (the `flushFinalLabel` tail)
   assigns each recorded literal its final text through the new
   typed arena finalize-write `TransformArena::set_numeric_literal_text`
   — the exact mechanism the ratified E-NAMES finalizer already uses
   for generated identifiers (`set_generated_identifier_text`,
   factory.rs:287): a text-only completion of a
   deliberately-deferred node, sanctioned by §11's amended wording,
   with `update_label_expressions` as the sole caller. The build
   runs ONCE; `is_final_label_reachable` consults the ledger's
   per-label presence (including visitation-phase-only entries)
   exactly as upstream. Argument-presence branches
   (`writeStatement`'s guard, bare `return;`/`yield;` element
   counts) are recording facts fixed before the build reads them.
5. ~~Focused-suite resolver~~ — RESOLVED: the machine consumes
   exactly one resolver query (`get_referenced_value_declaration`)
   inside catch-rename substitution. The suite's `FixtureResolver`
   answers it by lexical catch-clause resolution over the single
   parse tree (identifier text → nearest enclosing catch clause
   declaring it — exact for the §7 fixture language, which contains
   no shadowing of catch names); all other queries inherit the
   typed fail-closed defaults (resolver.rs:302). The production
   resolver is the landed checker bridge
   (`crates/checker/src/emit.rs:196`); its wiring is B-5's
   registration concern with the 32-case byte gate as the named
   verifier.
6. ~~Oracle equivalence for full-emit expectations~~ — RESOLVED at
   design level: the §7 fixture language contains no construct any
   pass before Generators lowers. TypeScript-syntax passes are
   identity on type-free sources; ES2016-ESNext passes lower
   nothing in the language; transformES2015 visits generator
   functions but preserves them extensionally — `shouldVisitNode`
   gates on `ContainsES2015` (`_tsc.js:104800-104806`),
   `visitFunctionDeclaration`/`transformFunctionLikeToExpression`
   thread `node.asteriskToken` verbatim
   (`:106202-106223`/`:106224-106254`), `transformFunctionBody`
   returns the ORIGINAL body when statements are unchanged
   (`arrayIsEqualTo` early-return `:106316-106318`), and the
   `update*` family returns original nodes on unchanged children —
   so the tree reaching transformGenerators in the oracle pipeline
   IS the parse tree the Rust suite feeds the machine. Verified
   empirically across all 72 probe emits (§7.3), incl. a
   non-generator sibling function passing through byte-identical.
   The dormant upstream arms this shields (`CustomPrologue`
   variable statements, generator methods, optional catch binding,
   super call bindings, `CommaListExpression`, shorthand/method
   object elements) port faithfully and carry direct unit contracts
   or typed-fault contracts instead of oracle projections; their
   end-to-end verifier is B-5's 32-case witness gate.
7. ~~Printer sufficiency (no printer edits)~~ — RESOLVED by
   measurement: synthesized case clauses take the single-statement
   inline arm (`source_nodes_start_on_same_line` returns true for
   synthesized ranges — printer.rs case-clause emission + the
   active enum-substitution contract printing
   `case 2 /* … */:`); synthetic trailing comments on numeric
   literals print via `emit_synthetic_trailing_comments_for_node`
   (printer.rs:11507) with the space-before-`/*` layout the probes
   show; `SINGLE_LINE` on synthesized ifs and `starts_on_new_line`
   are honored (printer.rs:5624/:5731/:6915/:6366); the
   single-line-source function body wrapping a synthesized
   `return __generator(…)` is the ACTIVE es2017 awaiter shape whose
   current evidence is the corpus at T0=100.0000% (not a focused
   contract test); the §7.3 suite pins it 72 times over. Any residual layout
   divergence surfaces as a §7.3 byte mismatch BEFORE any
   production wiring exists — the suite is the tripwire, and a
   printer gap would pause the train for its own reviewed slice
   rather than an in-fence patch.
8. ~~Module seam~~ — RESOLVED: a real `Transformer`
   (`GeneratorsTransformer` via `transform_generators(language_version,
   resolver)`, the es2017 seam) rather than a B-2-style host trait —
   chosen because the machine IS a whole pass (B-5 registers exactly
   this object), the focused suite then tests the production code
   path itself, and the upstream `(visitor, context)` split maps to
   the established transformer/visitor structure with no borrow
   gymnastics.

## 13. Readiness summary

Upstream: the frozen owner-graph/gap-matrix/witness/dispositions
chain (§1) plus the §4 vendored pins (129 owner functions + 38
addenda slices, all hashed; all 129 re-verified against the owner
graph's byte-offset hashes at authoring). Rust-map rows: 20 (§5),
targets measured present or new within fence. Gap rows: 1 (§6).
Witness families cited: 5 of 9 (qualify at B-5; focused projections
are this packet's surface — 72 oracle fixtures + typed fault
contracts). Architecture impact: `EA-GAP-COMPOSITION` substrate
progress recorded (disposition unchanged `activate`), `E-ORDER-H`
substitution owner landed dormant, `E-NAMES-H` gains the reuse-scope
arm, `EA-GAP-FLAGS` parsed-tree facet completes, comment/printer
rows premise-unchanged. Undispositioned: 0. Unresolved: 0 — items
2-8 resolved with measured pins or design-level arguments at
authoring (2026-08-22); the §12.6 composition-shielded arms carry
their named verifier into the B-5 byte gate.
