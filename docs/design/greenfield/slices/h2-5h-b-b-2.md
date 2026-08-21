# H2.5h-b / B-2 — the destructuring flattener: the 18-function shared family at FlattenLevel All

Design-gate packet for the SECOND H2.5h-b implementation packet, under
the mandatory implementation-ready design gate
(../post-h1-completion-slices.md). Authored at the train start
(2026-08-22) on `h2/5h-b-b2` from the post-B-1 trunk; reviewed
(independent full-dimension pass: 28 citation checks, 8 hash
recomputations, live oracle edge probes — verdict READY-WITH-FIXES,
all ten findings folded in: the initializer-rebind row, the six
converter pins, the frozen crash edge, `isPropertyNameLiteral`, the
embedded probe, the ObjectRest driver gate, the trailing-temp arm
split, exact `nodeIsSynthesized`, citation/wording corrections). The
design-gate pass lands with the trusted base, envelope, bootstrap, and
index in one commit before any production edit. Machine check:
`node .github/ci/slice-readiness.mjs --check h2-5h-b-b-2`.

## 1. Identity, purpose, and boundary

- **Slice ID:** `h2-5h-b-b-2`. **Kind:** `foundation` — a corpus-inert
  substrate packet, the second rung of the ratified B-ladder
  ([B-1 packet](h2-5h-b-b-1.md) §2). It lands the
  `destructuring-flattener` shared module frozen in the owner graph:
  the 18-function family (`flattenDestructuringAssignment` /
  `flattenDestructuringBinding` and their sixteen callees,
  `_tsc.js:93251-93697`) as a new shared Rust module with BOTH
  `FlattenLevel` arms implemented and the FlattenLevel-All arms
  qualified by focused projections. The module is the B-4 consumer
  surface (`transformES2015` reaches the family through exactly these
  two entries — owner-graph `shared_modules[0]`, edge
  `destructuring-shared-module`).
- **Non-goals:** transformer registration or activation (the dormant
  seam `crates/emitter/src/builtins.rs:145-148` "older targets belong
  to later target-ladder slices" is preserved verbatim); any edit to
  the active ObjectRestSpread lowering in
  `crates/emitter/src/builtins/es2018.rs` (its plan-based flattener
  stays the production ES2018 path; re-basing it onto the shared
  family is a byte-identity-gated concern for the H2.5h-b closure, not
  a foundation-packet edit); ES2015 visitors and loop conversion
  (B-4); the Generators state machine (B-3); tagged-template lowering
  (B-5); witness-set amendment; any corpus output-byte change.
- **Prerequisites:** B-1 merged with envelope `h2-5h-b-b-1` ready
  (helpers ×4, six-query resolver surface, eager name generation,
  EA-GAP-FLAGS classifier, hook chaining) — merged 2026-08-21
  @02f784d9. The B-1 substrate this packet consumes directly:
  `GeneratedBindingScopes` temp allocation + `TargetBinding` identity
  (E-NAMES-H eager model), `helpers::read()`/`helpers::object_rest()`
  texts and `EmitHelperName::{Read,Rest}`, and factory-owned
  transform-flag facets on created nodes.
- **Trusted base:** `02f784d920a8eaf0318f0958f33fcaf5d5106c53` (main
  after the B-1 merge). Authority artifacts at that base:
  owner graph `ratchets/h2-5h-a-owner-graph.v1.json`
  sha256 `b0deb0ea0e3fe238ca536f95eee9e340b3ad6c6a488dca0a193105bb61bcf386`
  (fingerprint `7bd5a77227d2f6e1599d6d72fa6ff257f218218d08048a1085396b99f5ccfc65`);
  gap matrix `ratchets/h2-5h-a-gap-matrix.v1.json`
  sha256 `47ffa610f964d181658d75bf51a212050cbe5405671e26e4c88a455baa52a9e1`
  (fingerprint `28693af8d8b783abd7c899d13fa1572d9e40ee1339f0263fb83dcb154747b55b`);
  witness artifact `ratchets/h2-5h-a-es2015-generators-witnesses.v1.json`
  sha256 `428163554d046539bf2ec50575b7894d148fe59d414636b0dd41fbbb082f0895`
  (fingerprint `4dd2e4f7f5e2077cba2617a51096915976e001d5b5855dbd6b417e38e7786ec2`);
  dispositions manifest `ratchets/h2-5h-a-dispositions.v1.json`
  sha256 `71bab6c3b675c3c3339998650a4619b202c63e0afd98ffc34e7dcb9e8436da08`
  (fingerprint `abbd33a7cb7c70162ba76098ff0a519b6ab2fe858e2799f356679e2f4dcec24b`).
- **Activation state:** before — gap-matrix row 11
  `destructuring-flattener-es2015` is the matrix's ONLY `partial`
  (anchors: the es2018 plan flattener; asserted absence: the substring
  `flatten_destructuring` in `crates/emitter/src/builtins.rs`), counts
  9 exists / 1 partial / 3 missing. After — row 11 `exists` with the
  shared-module anchors and the absence retired, counts
  10 / 0 / 3; the joint pass STILL dormant; the corpus ratchet
  byte-identical (T0=100.0000% 49024/49024 FP=0 unchanged).
- **Next owner:** B-3 (Generators state machine, per the ratified
  ladder).

## 2. Position in the ratified ladder

The B-1 design pass ratified the decomposition
([B-1 packet](h2-5h-b-b-1.md) §2); this packet is its row

> **B-2** | foundation | destructuring flattener at FlattenLevel All
> (the 18-function shared family; ObjectRestSpread level already lives
> in `es2018.rs`) | `destructuring-flattener` family focused
> projections

and revisits neither the granularity nor the ordering. The
`destructuring-flattener` witness family (4 cases) qualifies
END-TO-END at B-5 (its composition case routes the flattener through
the Generators machine); B-2's qualification surface is the focused
projections of §7, which exercise the family's own arms directly.

## 3. Required-reference table

| Row | Lifecycle before → after | Role here |
|---|---|---|
| `EA-GAP-COMPOSITION` | `activate` (dispositions row 43, cites capability `destructuring-flattener-es2015`) → unchanged disposition; rationale records the B-2 landing | the architecture gap owning the flattener capability; its §6-map section gains the substrate-landed preamble (§8.3) |
| `E-CAPTURE-BASE` | `modified-requalify` (dispositions row 15, cites surface `destructuring-module`) → unchanged | premise-only: B-2 edits none of its owners (`es2017.rs` capture plans untouched); the surface citation is satisfied by the shared module landing without owner-graph change |
| `E-NAMES-BASE` / `E-NAMES-H` | `substrate-landed` (B-1) → unchanged | temp allocation: `GeneratedBindingScopes::{allocate_temp,allocate_local_temp}` + `TargetBinding::allocate` under the reviewed eager model |
| `E-HELPERS-BASE` / `E-HELPERS-H` | `active-*`/`substrate-landed` → unchanged | `helpers::read()` (`typescript:read`, helpers.rs:178) and `helpers::object_rest()` (`typescript:rest`, helpers.rs:174) are the two helper texts the family requests |
| `EA-GAP-FLAGS` | `activate` (B-1 landed the classifier) → unchanged | created nodes take factory-owned facet flags; the module's constructors use the `propagate_child_flags`/`child_flags` idiom (es2018 precedent) |
| `E-ARENA`, `E-CONTEXT` | `active-qualified` premises | `TransformArena`/`NodeFactory` construction; `TransformationContext::hoist_variable_declaration` and the lexical environment |
| gap row 11 | `partial` → `exists` | §6 matrix |

Lifecycle values transcribed from the dispositions manifest at the
trusted base; the §8 amendments re-mint the affected artifacts through
this packet's own gate.

## 4. Pinned upstream map

The upstream IS the frozen artifact chain plus the vendored slices
below. All spans are 1-indexed inclusive lines of
`vendor/typescript-6.0.3/lib/_tsc.js`; every hash is the ledger d2
line-slice sha256 (newlines included, final line's newline included)
and lands verbatim in the module's `tsc-hash` headers, verified by
`cargo xtask ledger check`.

### 4.1 The family (owner-graph `shared_modules[0]`, offsets 4555185-4573251)

| # | function | lines | d2 line-slice sha256 |
|---|---|---|---|
| 1 | `flattenDestructuringAssignment` | 93251-93328 | `8303d862131f74b895085ac8968b52d5d0267330e000e0e91546757aaf278ee0` |
| 2 | `bindingOrAssignmentElementAssignsToName` | 93329-93337 | `4a6499b3dfa091b10245389ca3dc6d5a09d821a95fa0d49d5832eb942fededaf` |
| 3 | `bindingOrAssignmentPatternAssignsToName` | 93338-93346 | `f44ebdc8793a4c61c9949833b6369a47c3aba4fc2cf40aa6a03f459a51dc0bd8` |
| 4 | `bindingOrAssignmentElementContainsNonLiteralComputedName` | 93347-93354 | `908c2e74f1688ce04c71afa624f20261f36251a8ed7b2c40ae8d1cb096ef3ef8` |
| 5 | `bindingOrAssignmentPatternContainsNonLiteralComputedName` | 93355-93357 | `06f126b6b428017c5b579a14f4c44f61e560ee7bf325e987c928732973f8b6fd` |
| 6 | `flattenDestructuringBinding` | 93358-93448 | `ab53debce805e94e3a0f018f2bdf6d7724e4439fdd1ee40898b06059b8d6681e` |
| 7 | `flattenBindingOrAssignmentElement` | 93449-93485 | `63040df542279c36cc444040317fca2d769b2789c020fca8369bd3935334c11f` |
| 8 | `flattenObjectBindingOrAssignmentPattern` | 93486-93530 | `8fe5ff016903ce2b5f80496f1016b132a6724907bb2850c53e6bba9b677001c6` |
| 9 | `flattenArrayBindingOrAssignmentPattern` | 93531-93601 | `7c9aa1d2d4dcc64cfcc543b819b65310d59487c805946c9e90ffba19ed9fc4b8` |
| 10 | `isSimpleBindingOrAssignmentElement` | 93602-93611 | `c32008e16aff4c697941b5aca185f394e97533ad3be405710d2638ae4233b495` |
| 11 | `createDefaultValueCheck` | 93612-93629 | `c3af049d742326be9c4a9847bb0cc2cb5945ed96f166bd20432eed847dc07b18` |
| 12 | `createDestructuringPropertyAccess` | 93630-93649 | `d9a831e921e64f0142bcf4826daf4d38068a264ebfba7b35be510fedfa1ed7eb` |
| 13 | `ensureIdentifier` | 93650-93672 | `0930118fbbb030e6f038a1c1ad35661bb7f4656a7801d78014b5f480702c14bb` |
| 14 | `makeArrayBindingPattern` | 93673-93676 | `0d388aba458e4ce89f66e014d433801c2f9b336ca5745c2a439e04a498973fef` |
| 15 | `makeArrayAssignmentPattern` | 93677-93680 | `f1bd32dfcb0316067e434c2091cb8d51614bf8f81baa20e7e753a53bb91a297d` |
| 16 | `makeObjectBindingPattern` | 93681-93684 | `37da6610c94c9910dc0ac79b03c2dc47bedc7385da2575e7b71061d18e669ad0` |
| 17 | `makeObjectAssignmentPattern` | 93685-93688 | `b4b3a87526d8fe5aa891460fdb6d316503a622c7ddbab26cff50af715c91fe87` |
| 18 | `makeBindingElement` | 93689-93697 | `98a1fdd8c589b9944fdae0d7d8a520d5d7105197de1b23f9983e92bbdae3e651` |

The owner graph's `declaration_sha256` values pin the same slices under
the byte-offset recipe (`sha256(bytes[start_offset:end_offset))`,
exclusive end); both recipes were re-verified against the vendored file
at authoring.

### 4.2 Family-adjacent addenda (ported with their own headers)

| function | lines | d2 sha256 | role |
|---|---|---|---|
| `makeAssignmentElement` | 93698-93700 | `7851d63da1a407a05b43e09ea5678957490184527f12e4e1e4ed33c5c847ac99` | the identity `createArrayBindingOrAssignmentElement` of the assignment closure (`:93275`); outside the frozen 18 but a direct callee |
| `createRestHelper` | 25784-25823 | `9f8d5a4c75d0f742d506008b65adc3929c78b7f8e683adeec95c82a6c3d44106` | ObjectRest-arm helper-call constructor: excluded-name array over `elements[0..len-1]` (absent property names skipped; computed names consume `computedTempVariables` in order as `typeof _t === "symbol" ? _t : _t + ""`), `setTextRange(array, location)`, `__rest` call |
| `createReadHelper` | 25906-25914 | `f0baad214d517818636a8a2f1391a0f4521fc9b216a40bec37fb32ac179f82f3` | level-All downlevelIteration constructor: `__read(record)` or `__read(record, count)` with the count as a numeric literal |
| `getInitializerOfBindingOrAssignmentElement` | 27739-27764 | `aa9009ea650c27b73622759579cf755ef09b53bf651ceb7242d9e7ad2c4b8f86` | accessor: declaration elements → `.initializer`; PropertyAssignment → simple-assignment initializer's `.right`; Shorthand → `.objectAssignmentInitializer`; assignment → `.right`; SpreadElement → recurse |
| `getTargetOfBindingOrAssignmentElement` | 27765-27791 | `7852fe130a4fdc160138ce8deb2b6fb1fe8ef0c352df36406b65ad6370db4445` | accessor: declaration → `.name`; PropertyAssignment → recurse initializer; Shorthand → `.name`; SpreadAssignment/SpreadElement → recurse expression; assignment → recurse `.left`; else the element itself |
| `getRestIndicatorOfBindingOrAssignmentElement` | 27792-27802 | `b66844eeeecde2db166db5291416739d7de0975983bd9caacca6e6eb484d52d1` | Parameter/BindingElement → `.dotDotDotToken`; SpreadElement/SpreadAssignment → the element |
| `getPropertyNameOfBindingOrAssignmentElement` | 27803-27807 | `b90bf9d010fe009f8cd6dda3d152946fd09b012e16a586234212eb4d16aa917e` | tryGet + must-exist assert (SpreadAssignment exempt) |
| `tryGetPropertyNameOfBindingOrAssignmentElement` | 27808-27838 | `5d9d3b218899be017e5efa4340acc66602553cb3211f87d7983d953426ff42fb` | **the literal-computed-name unwrap**: a `ComputedPropertyName` whose expression is `isStringOrNumericLiteral` (String 11 \| Numeric 9 ONLY) unwraps to the expression — this is why `{ ["s"]: c }` reads as direct element access with no temp; PrivateIdentifier is a fail |
| `isStringOrNumericLiteral` | 27839-27842 | `173553973c2864a63a86abb15bcdd0703c19c559dc4dbc8359ea50aea2e99438` | the unwrap predicate (NOT the `-Like` variant) |
| `getElementsOfBindingOrAssignmentPattern` | 27843-27852 | `38a18465bc091e8a8d849a5bed5263a39e737a8908329c4a805c00dee7fc1d62` | Object/ArrayBindingPattern + ArrayLiteral → `.elements`; ObjectLiteral → `.properties` |
| `isDeclarationBindingElement` | 12106-12114 | `9b9e7fd00908376088df1064569a8fe280be9cc0a55052e0b898c7b0fcf2a105` | VariableDeclaration \| Parameter \| BindingElement |
| `isBindingOrAssignmentPattern` | 12118-12120 | `fb8c2a2f89f30c8744af2ecb3a77c9ba5e179666f8060c662b579f607bde5713` | object-or-array pattern union |
| `isObjectBindingOrAssignmentPattern` | 12121-12128 | `4f8f3dfc4d16547ea88fd11c1e14389ff91a382338093400e9bba25fb1d12763` | ObjectBindingPattern \| ObjectLiteralExpression |
| `isArrayBindingOrAssignmentPattern` | 12141-12148 | `f14f2b1eef9741c8ac3dd3d3fc063c4567b278b8de75a6801a97f00835afb6db` | ArrayBindingPattern \| ArrayLiteralExpression |
| `isSimpleCopiableExpression` | 93027-93029 | `388e8823ae5507fbcabb38b5bbd06c28c38b7381ce8b0eb987dbad1150ed52f1` | string-literal-like \| numeric \| keyword \| identifier |
| `isSimpleInlineableExpression` | 93030-93032 | `75411b5859a6888595a6e090ab2d42fe4f904d7becfe81a855f1b6111fa27cee` | `!identifier && simpleCopiable` |
| `isAssignmentExpression` | 17111-17113 | `cf13ad3dbebb98bf2a29a53d70992d7a24259be2616df4e4298e9f7ea8bf76fd` | binary `=` (excl. compound) with LHS-expression left |
| `isDestructuringAssignment` | 17114-17124 | `57f11978bed7f73705f836f943b584fbe39823ae01178fff5a5b6b046b44268b` | simple `=` whose left is Object(211)/Array(210) literal |
| `isEmptyObjectLiteral` | 17189-17191 | `330be2296e95c47508474b91c1c9678fff171d14291250f0519d81602b234c51` | the unwrap-loop guard |
| `isEmptyArrayLiteral` | 17192-17194 | `6cd2789388794ede983ceb3f9dcb4b362bfce3d582b9a63b71c65e7c181a7dcd` | the unwrap-loop guard |
| `isStringOrNumericLiteralLike` | 15844-15846 | `c4b0aff81eab867a2d799872ff7a55d26daeae2697511c247d141e352be2c42c` | `createDestructuringPropertyAccess` clone arm (includes NoSubstitutionTemplateLiteral via `isStringLiteralLike` 12583-12585) |
| `isPropertyNameLiteral` | 15888-15898 | `daae3011f849f003859aaa1373c2cc6c65b1fd0ea3fe264741b6eb3318030f6d` | the `isSimpleBindingOrAssignmentElement` property-name guard (census external edge): Identifier \| StringLiteral \| NoSubstitutionTemplateLiteral \| NumericLiteral |
| `convertToArrayAssignmentElement` | 20716-20732 | `a53d9e38c4ae559cf9c2cd29ef7b8bb1f189b05f53f47ca88e0ca5228c1c23bb` | `makeArrayAssignmentPattern`'s element converter (`factory.converters`, property-access-reached so absent from the census `external_edges`): BindingElement → SpreadElement/assignment/target conversion; non-binding inputs pass through `cast(element, isExpression)` |
| `convertToObjectAssignmentElement` | 20733-20747 | `ceda6ad456fc04784db6d09c68f51944a485356b200901d4289907922e469b09` | `makeObjectAssignmentPattern`'s element converter: BindingElement → SpreadAssignment/PropertyAssignment/ShorthandPropertyAssignment; non-binding inputs `cast(element, isObjectLiteralElementLike)` |
| `convertToAssignmentPattern` | 20748-20757 | `63b77454be5b3cb32c2723a1990da4cb9ac84663c7182dcb8c955302aba5b6a9` | the converters' recursion dispatcher |
| `convertToObjectAssignmentPattern` | 20758-20769 | `e6f6e3aa9e2039d2937eea5b5032475643399a980d61a32b504b0a33fd4b9f0d` | ObjectBindingPattern → ObjectLiteralExpression (+original/range) |
| `convertToArrayAssignmentPattern` | 20770-20781 | `395755484303c41fbecc2ff8af6fc8509f4b4480fe1bb957772acec107f002ed` | ArrayBindingPattern → ArrayLiteralExpression (+original/range) |
| `convertToAssignmentElementTarget` | 20782-20787 | `d2b8e01bb66232d9ecebbf2b7b8874357bfaf509cc0423b7ed4655c545ab2e0c` | binding-pattern targets recurse; expressions pass through |

### 4.3 Frozen behavior pins

- **FlattenLevel** is bundler-inlined: `All=0`, `ObjectRest=1`; the
  family compares `level >= 1 /* ObjectRest */` at :93499/:93556 and
  `level < 1` at :93534/:93548. ObjectRest arms additionally consult
  `transformFlags & (32768 ContainsRestOrSpread | 65536
  ContainsObjectRestOrSpread)` and `hasTransformedPriorElement`.
- **`createTypeCheck` semantics** (`:24548-24550`): tag `"undefined"`
  → strict equality against `void 0` (NOT typeof); tag `"symbol"` →
  `typeof x === "symbol"`. The Rust idioms exist
  (`es2018.rs:4118` `create_strict_undefined_check`;
  `es2018.rs:3818` typeof-symbol conditional).
- **Callers (B-4 consumer surface, all level All):**
  `flattenDestructuringBinding` at :105754/:105906 (parameters, rval =
  generated/expression name), :106477 (visitVariableDeclaration,
  `hoistTempVariables` = exported), :106583 (for-of head, rval =
  boundValue), :107575 (catch clause, rval = temp);
  `flattenDestructuringAssignment` at :106347
  (needsValue = `!expressionResultIsUnused`), :106395 (converted-loop
  initializers). ES2018-level callers at
  :102059/:102105/:102164/:102715/:102761; module-transform callers
  (`createAssignmentCallback` consumers) at :110718 (system,
  `createAllExportExpressions`), :111628, :112700, :113108, :95073.
- **Observable failure order:** `Debug.checkDefined` on every
  `visitNode(..., isExpression)` (absent/mistyped visit result is a
  defect, not a recoverable state); `Debug.assertNode(target,
  createAssignmentCallback ? isIdentifier : isExpression)` /
  `(target, isBindingName)` at the emit sinks; `Debug.assert` inside
  `getPropertyNameOfBindingOrAssignmentElement`;
  `Debug.assertEachNode` inside the four make* constructors;
  `Debug.assertIsDefined(computedTempVariables)` inside
  `createRestHelper`. All become typed `TransformError` fail-closed
  arms (§5), never silent fallbacks.
- **Oracle behavior corpus** (probe recipe in §7): single-element
  patterns bind without a source temp; multi-element declaration
  patterns reuse an identifier source; defaults print
  `_x === void 0 ? d : _x`; non-literal computed keys force the
  fresh-value entry arm (the entry REBINDS the variable declaration to
  a copy whose initializer is the temp — `updateVariableDeclaration`
  at `_tsc.js:93384-93393` inside pin #6 — so the effectful
  initializer is evaluated exactly once); literal
  computed/string/numeric keys are direct element access; empty
  patterns force a temp (`var _a = init();`); array holes skip; array
  rest slices (`.slice(i)`); downlevelIteration arrays read through
  `__read` (count omitted iff trailing rest); assignment mode returns
  the bare value when `needsValue` and no expressions were emitted,
  else appends the value iff `needsValue`; the
  pattern-assigns-to-source collision hoists a fresh temp; the
  empty-literal unwrap loop reduces `({} = {} = obj)` to the visited
  innermost value.
- **Upstream crash edge, frozen:** an all-omitted assignment pattern
  with an unused result (`[,,] = x;`) reaches
  `inlineExpressions(undefined)` at `_tsc.js:93326` and CRASHES on
  vendored 6.0.3 (reproduced at review); the
  `|| createOmittedExpression()` fallback is dead code. The Rust arm
  is a typed `TransformError` (fail-closed per §11), asserted by a
  fault-shaped focused contract — this expectation cannot be
  oracle-minted because the oracle throws.

## 5. Rust semantic map

New shared module `crates/emitter/src/builtins/flatten_destructuring.rs`
(registered as `mod flatten_destructuring;` in
`crates/emitter/src/builtins.rs` — this deliberately retires the gap
matrix's asserted absence). Function-per-function port; every ported
fn carries the `tsc-port`/`tsc-hash`/`tsc-span` header from §4 and
`#[allow(dead_code)] // production consumers arrive with the B-4/B-5 owners`
where caller-less (the B-1/CS-4 precedent).

| Item | Target |
|---|---|
| level | `pub(super) enum FlattenLevel { All, ObjectRest }` with `is_object_rest(self) -> bool` (`level >= 1` compare sites) |
| consumer seam | `pub(super) trait FlattenHost`: `fn context(&mut self) -> &mut TransformationContext`; `fn source(&self) -> TransformSourceId`; `fn downlevel_iteration(&self) -> bool` (per §3.3 of the infrastructure survey, options live on the owning visitor, never on the context); `fn generated_bindings(&mut self) -> &mut GeneratedBindingScopes`; `fn visit_expression(&mut self, node: TransformNode) -> Result<TransformNode, TransformError>` (= `Debug.checkDefined(visitNode(..., isExpression))`); `fn visit_binding_or_assignment_element(&mut self, node: TransformNode) -> Result<TransformNode, TransformError>` (ObjectRest chunk arm only). Implementors: the B-4 `Es2015Visitor` (future) and the §7 focused-suite driver (now) |
| flatten state | `struct FlattenContext` mirroring the upstream closure record: `level: FlattenLevel`, `downlevel_iteration: bool`, `hoist_temp_variables: bool`, `has_transformed_prior_element: bool`, `kind: FlattenPatternKind` (`Binding` \| `Assignment` — selects the make* constructor set exactly as the two upstream closures do), plus the emit sinks: `pending_expressions: Vec<TransformNode>` (upstream `pendingExpressions`, empty ⇔ `undefined`), `pending_declarations: Vec<PendingFlattenDeclaration>` (binding mode), `expressions: Vec<TransformNode>` (assignment mode) |
| pending declaration | `struct PendingFlattenDeclaration { pending_expressions: Vec<TransformNode>, name: TransformNode, value: TransformNode, location: Option<TransformNode>, original: Option<TransformNode> }` — the exact upstream record; the trailing-temp arm appends assignments onto the LAST pending declaration and rebinds its value (`_tsc.js:93412-93421`) |
| assignment callback | `createAssignmentCallback` is a `FlattenHost` method `fn create_assignment_completion(&mut self, target, value, location) -> Result<Option<TransformNode>, TransformError>` defaulting to `Ok(None)` (standard arm: `set_text_range(create_assignment(visit_expression(target), value), location)`); the module-transform consumers (`createAllExportExpressions`) arrive with their owners |
| entries | `pub(super) fn flatten_destructuring_assignment<H: FlattenHost>(host, node, level, needs_value) -> Result<TransformNode, TransformError>` and `pub(super) fn flatten_destructuring_binding<H: FlattenHost>(host, node, level, rval: Option<TransformNode>, hoist_temp_variables: bool, skip_initializer: bool) -> Result<Vec<TransformNode>, TransformError>` — upstream signatures with `visitor`/`context` folded into `host` |
| temp variables | a temp that is HOISTED (`ensureIdentifier` with `hoist_temp_variables == true`; the array ObjectRest deferral with hoisting; the trailing-temp `hoistTempVariables == false` ELSE-arm `_tsc.js:93412-93421`): `allocate_temp()` + `TargetBinding::allocate` + `context.hoist_variable_declaration(identifier)`. A temp DECLARED BY ITS OWN EMITTED BINDING (the `hoist_temp_variables == false` `ensureIdentifier` sink; the array ObjectRest deferral without hoisting; **the trailing-temp `hoistTempVariables == true` ARM `_tsc.js:93397-93411`, which emits the fold as a fresh pending declaration and does NOT hoist**): `allocate_local_temp()`, no hoist. This maps upstream `createTempVariable(/*recordTempVariable*/ void 0)` + each site's explicit `hoistVariableDeclaration` under the E-NAMES-H eager model (`es2018.rs:3531-3548` precedent; `allocate_temp`/`allocate_local_temp` share one per-scope ordinal counter, `generated_bindings.rs:130/:170`, so spelling order is stable); every use site mints a fresh identifier via `TargetBinding` metadata (`create_generated_identifier` precedent `es2018.rs:3966-3973`) |
| initializer rebind | the collision/computed entry arm of `flattenDestructuringBinding` REBINDS the input (`factory.updateVariableDeclaration` `_tsc.js:93384-93393`): the Rust port re-creates the `VariableDeclaration` with the ensured-temp initializer via `factory().update_node` (with `set_original_node(new, old)` + `set_text_range(new, old)` if `update_node` does not itself record them — verified at implementation), so the effectful initializer is evaluated exactly once |
| node converters | the six `factory.converters` fns (§4.2) port as module-internal fns reached only from the two `make*AssignmentPattern` constructors (assignment-mode ObjectRest chunk arms); the BindingElement arms port faithfully (spread/assignment/property/shorthand conversion with original/range threading) even though assignment-mode inputs are already assignment-shaped — non-binding inputs pass through with a fail-closed binding-shape rejection replacing upstream's `cast` |
| node construction | module-internal `create_*` wrappers over `self.context().factory()?.create_node(source, NodeData::…, flags)` with flags from `TransformArena::propagate_child_flags` / an es2018-style `child_flags` fold / `array_transform_flags` (the EA-GAP-FLAGS discipline: synthesized output takes factory-computed facets, never stale inherited ES2015/Generators facets); constructors needed: assignment (`=` binary), conditional, element access, property access, string/numeric literal, array/object literal, array/object binding pattern, binding element, variable declaration, void 0, `.slice(i)` call, comma fold (`inline_expressions`), typeof-symbol conditional, `__read`/`__rest` calls (`request_emit_helper(helpers::read()/object_rest())` + `create_unscoped_helper_identifier(source, EmitHelperName::Read/Rest)` — `transform.rs:645-662`, `factory.rs:1130`) |
| provenance & ranges | every synthesized node is born `SYNTHESIZED` from `create_node`; `expression.original = original` / `variable.original = original` map to `arena_mut().set_original_node(node, Some(original))`; `setTextRange(node, location)` maps to `factory().set_text_range(node, location)` with `Option` locations handled at call sites (es2018 `set_original_and_range` precedent `es2018.rs:5055`) |
| element accessors | the §4.2 accessor/predicate set (incl. `isPropertyNameLiteral`, a census external edge) as module-internal fns over `NodeData` (`BindingElement`/`PropertyAssignment`/`ShorthandPropertyAssignment`/`SpreadAssignment`/`SpreadElement`/`BinaryExpression`/`OmittedExpression` arms exactly as pinned); trivial single-kind predicates (`isIdentifier`, `isComputedPropertyName`, `isOmittedExpression`, `isVariableDeclaration`, `isLiteralExpression` token-range check, `isBigIntLiteral`) stay inline `matches!` idioms (es2018 precedent); `nodeIsSynthesized` ports its EXACT position-based semantics (`pos < 0 \|\| end < 0`, `_tsc.js:16000-16002`) over the arena's `SourceRange::from_raw` classification (`SourceRange::Synthesized` ⇔ both raw bounds are the `u32::MAX` sentinel; a mixed range is a typed error) — NOT the `!is_parsed_node` approximation, which diverges on synthesized nodes carrying copied ranges; `inline_expressions` is the comma fold whose empty input is the §4.3 crash edge (typed error); list helpers (`append`/`addRange`/`some`/`every`/`forEach`/`map`/`last`) are `Vec`/iterator idioms |
| comment/printer surfaces | untouched: the module creates synthesized nodes only (no comment ownership, no resume-cursor transitions, no printer expression-context change); lexical receiver/captured-binding ownership unchanged (no receiver nodes minted); pass ordering/composition unchanged (no registration edit) |

Producer/owner/consumer per row: the module is the sole producer of
its synthesized subtrees; the consuming visitor (B-4; the §7 driver
now) owns visitation and statement placement; the
`TransformationContext` owns hoisted declarations and helper requests;
invalidation follows the arena (nodes are immutable once created; the
one in-family update is the §5 initializer-rebind row).

## 6. Current local-gap matrix (B-2 row, from the frozen artifact)

| Capability | State | Anchor evidence | Absence evidence |
|---|---|---|---|
| `destructuring-flattener-es2015` | `partial` | `flatten_destructuring_assignment` / `flatten_destructuring_binding` in `crates/emitter/src/builtins/es2018.rs` (the ObjectRestSpread plan flattener) | the substring `flatten_destructuring` asserted absent from `crates/emitter/src/builtins.rs`; note: "the shared ES5-level family (FlattenLevel All…) and its extraction as a shared module remain outstanding" |

Registering `mod flatten_destructuring;` trips the absence exactly as
the generator intends ("absence violated … re-disposition this
matrix"); the §8.1 amendment is that reviewed re-disposition.

## 7. Implementation sequence (dependency order; every step corpus-inert)

Fence: `crates/emitter/src/builtins.rs` (the `mod` registration line
only; the target rejection at :145-148 is read-only),
`crates/emitter/src/builtins/flatten_destructuring.rs` (new),
`crates/emitter/tests/unit/flatten_destructuring/tests.rs` (new,
attached with the `#[cfg(test)] #[path]` idiom), and the §8 evidence
set. `es2018.rs` and every other production file are out of fence.

1. **Family leaves.** The §4.2 accessor/predicate ports, the four
   make* constructors (+ `make_assignment_element` identity), the
   in-family predicates (#2-#5, #10), `create_default_value_check`
   (#11), `create_destructuring_property_access` (#12),
   `ensure_identifier` (#13), and the two helper-call constructors
   (`create_rest_helper_call`, `create_read_helper_call`), each with
   its ledger header; `FlattenLevel`, `FlattenHost`,
   `FlattenContext`, `PendingFlattenDeclaration`.
   Check: leaf unit contracts green (accessor arms incl. the
   literal-computed unwrap; default-check `=== void 0` shape;
   property-access arms literal/computed/identifier;
   ensure-identifier reuse×hoist matrix); `cargo xtask ledger check`
   stale=0 undispositioned=0.
2. **Family core.** The three walkers (#7, #8, #9) and the two
   entries (#1, #6) over the `FlattenContext` sinks: the
   pendingExpressions folding protocol, the trailing-temp arm, the
   materialization loop, the empty-literal unwrap loop, the
   collision/computed entry ensures, needsValue, skipInitializer,
   both `FlattenLevel` arms (ObjectRest chunking, transform-flag
   consultation, rest-helper path, `has_transformed_prior_element`
   temps). Register `mod flatten_destructuring;`.
   Check: `cargo build -p tsc-rs-emitter` + clippy clean; protocol
   unit contracts green (pending-declaration folding, trailing-temp
   last-declaration append, assignment expression-list assembly).
3. **Focused projections.** A test-only driver in the unit suite:
   parse fixture → `TransformArena::add_source` → a minimal
   `Transformer` whose visitor implements `FlattenHost`, flattens
   every binding-pattern `VariableDeclaration` (level per fixture) and
   every destructuring-assignment `BinaryExpression`
   (needsValue = value-used position, upstream
   `!expressionResultIsUnused` `_tsc.js:106352`; needs-value
   propagates through parentheses per `visitParenthesizedExpression`),
   merges the file-level lexical environment (es2018
   `merge_source_lexical_environment` precedent), prints via
   `create_printer` (LF) — byte-compare against oracle-minted
   expectations (full output bytes INCLUDING emitted helper texts;
   the `active_transform_contract.rs:4891` precedent embeds the
   complete `__rest` prelude the same way).
   **Fixture-language constraint:** every fixture contains no
   other-pass-lowerable syntax anywhere (`var` only, no
   `let`/`const`/arrows/templates/classes), so full-emit equality is a
   pure flattener projection. **ObjectRest driver gate:** at
   `FlattenLevel::ObjectRest` the driver flattens a site only when it
   contains object rest (the upstream ES2018 trigger,
   `_tsc.js:102058`); every ObjectRest fixture's flatten site
   contains a rest element, so driver and oracle route identically.
   The 27 oracle fixtures (exact sources; expected bytes frozen in the
   suite from the probe output):
   level All, target ES5 — `var { a } = obj;` · `var { a, b } = obj;`
   · `var { b = 1 } = obj;` · `var { [k]: c } = obj;` ·
   `var { ["s"]: c } = obj;` · `var { "s p": c } = obj;` ·
   `var { 1: c } = obj;` · `var { a: { b } } = obj;` ·
   `var { a: { b } = { b: 1 } } = obj;` · `var {} = init();` ·
   `var [x] = arr;` · `var [x, , y = 2, ...zs] = arr;` ·
   `var [] = init();` · `var { a: [b, { c = 3 }] } = obj;` ·
   `({ a, b } = obj);` · `r = ({ a } = obj);` · `({ x } = x);` ·
   `({} = {} = obj);` · `[x, y = 1] = arr;` · `y = ([,,] = x);` (the
   crash edge's needsValue sibling: the bare-value return);
   level All + downlevelIteration — `var [x, y] = pair;` ·
   `var [x, ...r] = arr;` · `var [] = init();`;
   level ObjectRest, target ES2017 — `var { a, ...rest } = obj;` ·
   `var { [k]: c, ...rest } = obj;` · `({ a, ...r } = o);` ·
   `var { [k()]: c } = o;` (the effectful-initializer rebind
   witness, level All at ES5).
   Fault-shaped typed-error contracts (not oracle-mintable — the oracle
   crashes): `[,,] = x;` (the §4.3 crash edge) and
   `({ m() { } } = o);` (a method element has no
   binding-or-assignment target, so the property-name must-exist
   assert is the upstream failure point).
   The exact probe (frozen; also recorded in the PR):

   ```js
   // node b2-probe.mjs — vendored oracle, single virtual file,
   // alwaysStrict:false (no prologue), LF; full emitted bytes are
   // the expectation.
   import { createRequire } from "node:module";
   const require = createRequire(import.meta.url);
   const ts = require("<repo>/vendor/typescript-6.0.3/lib/typescript.js");
   function emitCase(source, extra) {
     const options = {
       target: extra.target ?? ts.ScriptTarget.ES5,
       alwaysStrict: false, newLine: ts.NewLineKind.LineFeed,
       downlevelIteration: !!extra.downlevelIteration,
       types: [], lib: ["lib.es5.d.ts"],
     };
     const fileName = "/project/input.ts";
     const sf = ts.createSourceFile(fileName, source, options.target, true);
     const outputs = {};
     const host = {
       getSourceFile: (f) => (f === fileName ? sf : undefined),
       getDefaultLibFileName: () => "lib.d.ts",
       writeFile: (f, text) => { outputs[f] = text; },
       getCurrentDirectory: () => "/project",
       getCanonicalFileName: (f) => f,
       useCaseSensitiveFileNames: () => true,
       getNewLine: () => "\n",
       fileExists: (f) => f === fileName,
       readFile: () => undefined,
       directoryExists: () => true, getDirectories: () => [],
     };
     ts.createProgram([fileName], options, host).emit();
     return outputs["/project/input.js"];
   }
   ```

   Oracle bytes are the entire expectation (no hand-authored output).
   Check: focused suite green (27 byte-equal projections + the typed
   fault contracts); `cargo test -p
   tsc-rs-emitter` fully green with zero expected-string changes
   outside the new suite.
4. **Train items.** §8 amendments, chain walk (b2-walk.sh = the
   B-1 walk with this scratchpad's path; qualification BEFORE
   profile; pin-sweep audit), envelope `h2-5h-b-b-2` (`ready`,
   predecessor `h2-5h-b-b-1` receipt
   `bd35691af3776aab1e8c1f0f01df5f0a5e47af1c0b495cb4c905b7a3e6402e7a`),
   bootstrap `allowedPacketIds += h2-5h-b-b-2`, index row, static
   `--lane rust` BEFORE the walk, full local gate at the final head
   from the canonical repository path (detached launcher;
   perf-only-red → normal-priority resume per protocol).

## 8. Evidence, ratchet, and documentation amendments

1. **Gap matrix generator** (`crates/oracle/h2-5h-a-gap-matrix.mjs`
   row 11): `state: "partial"` → `"exists"`; anchors gain
   `crates/emitter/src/builtins/flatten_destructuring.rs`
   `flatten_destructuring_binding` / `flatten_destructuring_assignment`
   (the es2018 anchors remain as the ObjectRestSpread siblings); the
   `builtins.rs` absence retires (empty absence list is legal — nine
   rows carry `absences: []` today); note records the landing and the
   deliberate es2018 non-migration; summary counts 9/1/3 → 10/0/3.
2. **Dispositions generator** (`crates/oracle/h2-5h-a-dispositions.mjs`):
   `EA-GAP-COMPOSITION` rationale gains the B-2 landing clause
   (disposition stays `activate`); no other row changes; the manifest
   re-mints with the new gap-matrix lineage.
3. **Architecture map** (`docs/design/greenfield/emitter-architecture.md`):
   the `EA-GAP-COMPOSITION` section gains a
   "**Substrate landed (B-2, …)**" preamble naming the module, the
   trait seam, and the remaining scope (B-3/B-4/B-5) — the
   EA-GAP-FLAGS/B-1 pattern; NO heading or table-row identity changes
   (the dispositions generator derives the row inventory from
   `^\| \`E-…\`` and `^### \`EA-GAP-…\``).
4. **Handoff** `h2-5h-a.md`: the ladder's B-2 bullet gains its
   **LANDED** marker at the final implementation-step commit ⇒
   envelope `h2-5h-a` re-pin + doc-pinning witness re-mints
   (adoption: seconds).
5. **Chain walk**: b2-walk.sh (the B-1 walk verbatim, scratchpad path
   updated); qualification BEFORE profile; step 7 re-mints
   foundation/comment-scope witnesses (adoption), owner-graph,
   gap-matrix, dispositions, es2015-generators witnesses; pin-sweep
   audit before the gate after any multi-attempt walk.
6. **Readiness**: envelope `ratchets/fci-readiness/h2-5h-b-b-2.v1.json`
   (`ready`; fence = §7 + the walk set), bootstrap
   `allowedPacketIds += h2-5h-b-b-2`, index row in `slices/README.md`.

## 9. Acceptance

- All 18 family functions + the §4.2 addenda landed with ledger
  headers; `cargo xtask ledger check` green (stale=0,
  undispositioned=0, todo_port=0).
- Focused projection suite green: every fixture byte-equal to its
  oracle-minted expectation (level-All object/array/assignment arms,
  downlevelIteration read-helper arms, ObjectRest rest-helper arms),
  typed-error contracts green, provenance contracts
  (original/range/SYNTHESIZED) green.
- `cargo test -p tsc-rs-emitter` fully green; zero expected-string
  changes outside the new focused suite.
- Gap matrix re-minted: row 11 `exists`, counts 10/0/3; dispositions +
  owner-graph + witnesses re-minted through the walk; architecture map
  and handoff amended per §8.
- Corpus ratchet: T0=100.0000% 49024/49024 FP=0, all bands, tiers —
  byte-identical (foundation packet; zero output change).
- Packet checker `slice-readiness --check h2-5h-b-b-2`; complete local
  gate green at the final head from the canonical path.

## 10. Traceability

| Invariant | Target | Test | Evidence |
|---|---|---|---|
| family fidelity (18 fns) | `flatten_destructuring.rs` ports | ledger d2 headers + protocol contracts | §4.1 spans/hashes + owner-graph declaration hashes |
| accessor semantics (incl. literal-computed unwrap) | module accessor fns | leaf unit contracts | §4.2 spans/hashes |
| level-All output shape | entries + walkers | focused projections byte-equal | oracle probe corpus (§7.3 command) |
| ObjectRest arms | walkers + `create_rest_helper_call` | rest-helper contracts | §4.2 `createRestHelper` pin + ES2017-target probes |
| temp model equivalence | `FlattenHost` + `TargetBinding` idiom | ensure-identifier matrix + projection temps (`_a`, `_b`, …) | E-NAMES-H argument (B-1 §12.3) + es2018 precedent |
| synthesized-flag correctness | module `create_*` wrappers | facet assertions in the suite | EA-GAP-FLAGS tables (B-1) |
| dormancy | no registration edit; `builtins.rs` mod line only | untouched-file assert + ratchet | corpus byte-identity |
| consumer-surface completeness | entry signatures | compile-time (B-4 call shapes typed) | §4.3 caller pins |

Resources: the focused suite is plain `cargo test` (no worker
ceilings); the walk and gate follow the standing demotion directive
(`taskpolicy` maintenance/`nice`), with the perf-ceiling
normal-priority resume exception.

## 11. Prohibitions

No transformer registration or activation change; no corpus
output-byte change (the ratchet is the enforcement); no `es2018.rs`
edit; no ES2015/Generators visitor or state-machine code (B-3/B-4
scope); no witness amendment; no new `EmitContext` constructors; no
generic fallback that converts an unknown branch into success (every
unexpected shape is a typed `TransformError`); no fixture-specific
branches or hand-authored expected output (oracle bytes only); the
CS-3/4/5/6 and B-1 prohibitions remain. This document authorizes no
production edit until its own design-gate pass and envelope exist.

## 12. Unresolved items (all closed at authoring, 2026-08-22)

1. ~~Trusted base + authority hashes~~ — pinned in §1 at
   `02f784d920a8eaf0318f0958f33fcaf5d5106c53`; the §8 amendments
   re-mint the artifacts through this packet's own gate.
2. ~~Module seam~~ — RESOLVED: `FlattenHost` trait (context, source,
   `downlevel_iteration`, generated bindings, the two visit methods,
   the assignment-callback default) — chosen over free functions with
   a context bundle because the B-4 visitor and the focused driver
   both already own exactly these capabilities, and the trait keeps
   the upstream `(visitor, context)` split typed without borrow
   gymnastics.
3. ~~ObjectRest-arm scope~~ — RESOLVED: the full family ports (the
   level branches are inside the frozen declaration hashes); the
   ObjectRest arms are exercised by focused contracts against
   ES2017-target oracle probes; `es2018.rs` keeps its independent
   plan-based production path (out of fence); re-basing es2018 onto
   the shared family is deliberately deferred to the H2.5h-b closure
   under a byte-identity gate — the duplication is recorded in the §8
   architecture-map preamble, and the family's ObjectRest arms carry
   the focused contracts as their reachability control until then.
4. ~~Visitor equivalence for the focused driver~~ — RESOLVED at design
   level: the family invokes the visitor only at (a) initializer/value
   expressions, (b) assignment-target leaves, (c) computed key
   expressions, (d) whole elements in the ObjectRest chunk arm. The
   §7.3 fixtures contain no other loweringable syntax at those
   positions, so the production visitor is extensionally the identity
   there and the driver's identity visitor projects the family
   exactly. Named verifier for the composition boundary: the frozen
   `destructuring-flattener--composition-inside-generator` witness
   case (B-5 byte gate) plus the B-4 integration itself.
5. ~~Expectation minting~~ — RESOLVED: fresh-process
   `ts.createProgram` probes against the vendored 6.0.3 bundle with
   `alwaysStrict: false` (no prologue framing), LF newlines, per-§7.3
   options; full emitted bytes are the expectation; the command is
   frozen in the suite header and §7.3.
6. ~~Helper-call constructors~~ — RESOLVED: `createRestHelper` /
   `createReadHelper` port as module addenda (§4.2) over the landed
   helper texts (`typescript:rest`/`typescript:read`) and
   `EmitHelperName::{Rest,Read}`; no helper-text change.

## 13. Readiness summary

Upstream: the frozen owner-graph/gap-matrix/witness/dispositions chain
(§1) plus the §4 vendored pins (18 family + 28 addenda slices, all
hashed). Rust-map rows: 12 (§5), targets measured present or new
within fence. Gap rows: 1 (§6). Witness families cited: 1 of 9
(qualifies at B-5; focused projections are this packet's surface — 27
oracle fixtures + typed fault contracts).
Architecture impact: `EA-GAP-COMPOSITION` substrate progress recorded
(disposition unchanged `activate`), `E-CAPTURE-BASE`
premise-unchanged, dormancy preserved. Undispositioned: 0.
Unresolved: 0 — items 2-6 resolved with measured pins or design-level
arguments at authoring (2026-08-22); the §12.4 identity-visitor
argument carries its named verifier into the B-5 byte gate.
