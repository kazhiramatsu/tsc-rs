# H2.8a G5c: JSDoc return annotation ownership in the syntactic declaration builder

Document role: **slice design and readiness record** for the G5c request in
[h2-8a-jsdoc-return.md](h2-8a-jsdoc-return.md). Precedence follows
[docs/design/README.md](../../README.md): pinned TypeScript 6.0.3 bodies own
the semantics, the [current emitter architecture](../emitter-architecture.md)
owns the Rust seams, this packet owns only the bounded change below. The
selection inventory is [h2-8a-jsdoc-return-selection.v1.json](h2-8a-jsdoc-return-selection.v1.json);
the frozen native before is [h2-8a-jsdoc-return-before.v1.json](h2-8a-jsdoc-return-before.v1.json).
Runtime base `7d6bc9848e97c26b0438da5ec73bd08c9a175c9a`; the branch carries
two documentation commits on top of it and is never reset.

Status: **design gate satisfied; implemented and verified at the final
source (§9, [h2-8a-jsdoc-return-report.md](h2-8a-jsdoc-return-report.md))**.
This document does not claim the global H2.8a total; it claims one cause
closure with its positive and negative controls.

## 1. Purpose, scope and predecessors

The original `typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsFunctionsCjs.ts#default`
prints `d`/`e` with `: any` in `index.d.ts` where TypeScript prints `: string`
and `: T & U`. Both functions are `module.exports.<name> = function <name>(a, b)
{ return /** @type {*} */(null); }` with the `@return` tag on the JSDoc block
that precedes the assignment statement. Every other byte of both writes,
every diagnostic, the JS product and the exit code already match
(before receipt: four difference paths, all under `/writes/1`).

Predecessors that this slice must keep intact: the parameter-tag lookup
repair (`e6035fd0f`, [h2-8a-declaration-comment-ranges.md §12](h2-8a-declaration-comment-ranges.md#12-native-before-evidence-and-parameter-lookup-dependency-amendment)),
which introduced the binder AST utility `get_jsdoc_parameter_tags` and
replaced the syntactic builder's local parameter fallback; the G4a/G4b comment
location repairs; the UTF-16 `JsString`/`JsStr` ownership, raw callback
units and printer escaping policy (PR #521 lineage); the noEmit declaration
diagnostics gate (§33.3 of the UTF-16 adjacent repair). None of those
surfaces is edited here.

Out of scope, recorded as other owners when reached: the parser's JSDoc
attachment rules (whether a comment attaches at all), the semantic checker's
return inference, the printer, program/config, and every H2.5h residual.

## 2. Pinned upstream owners

All spans are `vendor/typescript-6.0.3/lib/_tsc.js`, whole lines including the
final newline (the repository ledger algorithm, verified against four existing
`tsc-hash` headers). The selection inventory's `getStart..end` hashes cover the
same functions under the other contract and are not repeated here.

| Function | Lines | SHA-256 | Branch followed |
| --- | --- | --- | --- |
| `getJSDocReturnTag` | 11708–11710 | `9b02cc14a58da114906e21bbb106c7cf8c0bf85e3555915f43e295c59776dd20` | first owned `JSDocReturnTag` |
| `getJSDocTypeTag` | 11714–11720 | `0c07d7f33cca41752e2a1714164de9d4912d78de25145a44694d688c4ceb48a9` | first owned `JSDocTypeTag` with `typeExpression.type` |
| `getJSDocReturnType` | 11728–11744 | `7bcd67792fdeaceeecc2c6ff990b94e0065646e3068932012592c00efafe9eea` | return tag with a type expression, else callable `@type` (type literal call signature, function type, JSDoc function type) |
| `getJSDocTagsWorker` | 11745–11759 | `57325aecd61d6df7de8277e221e63b1ecb7a7f6a3501d999255bc88fb82d81f2` | owned comments flattened to tags, cached |
| `getFirstJSDocTag` | 11767–11769 | `692bf3520107ebc11067c254d2e883b0e4767a5dccd1d9dbdd795d8e7dd5e679` | `find` over the worker's order |
| `getJSDocCommentsAndTags` | 15429–15450 | `06bd3326770ddb5efcb52a26bc02e694410ca4cfc4fdd1b0a5da80215773703d` | initializer attachment, then the host and each next location |
| `filterOwnedJSDocTags` | 15451–15461 | `4287e545ec38802a2766922acf834c9bd3408679eaeb047278c09528ed924c94` | last block owns its tags, earlier blocks contribute `@overload` only |
| `ownsJSDocTag` | 15462–15464 | `b2fe170504879c65b19c45cf47f347e85cc00cfb334570284df207ea27b3a651` | `@type`/`@satisfies` inside a parenthesized expression belong to it alone |
| `getNextJSDocCommentLocation` | 15465–15474 | `12679e1abba9e5aa883b2cae53161fead29e07485c4853f7e507a7fe5ff433c2` | FunctionExpression → BinaryExpression (assignment) → ExpressionStatement; stops at a ParenthesizedExpression parent |
| `getEffectiveReturnTypeNode` | 16768–16770 | `48ca97d514d3167e7f6ad934bb144fdf907c4bca4f09b790587d13a4fc2faeeb` | JSDocSignature, direct `.type`, JS `getJSDocReturnType` |
| `isJSDocTypeAssertion` / `getJSDocTypeAssertionType` | 27553–27555 / 27556–27560 | `62e3b328401615642ade63804d7b852af9ff6cd6f4ed3ff1c159e9409f492193` / `cfd55501eb32ceeddde6324f1995a8762d73652e63928d8787c23d389fe0c9d1` | the body shortcut's `*` source |
| `canReuseTypeNodeAnnotation` | 50932–50955 | `edfd54626c63d3d1645a16cfcad8561dab1388e09a7278579ada789709becc6d` | value signature → `getReturnTypeOfSignature`; annotation type equivalence; error types reuse |
| semantic `serializeReturnTypeForSignature` | 53524–53546 | `31fc902e4dc5253fc144eb471e4f27423714c36d86bf4af777b4186cabb4b123` | declaration present and not expandable → syntactic builder first |
| `serializeTypeAnnotationOfDeclaration` | 133706–133729 | `e23bbbb5fd3312ca518de15faf39bf09573dbe3a1e071fdc6640ff91ff3525e9` | reuse check, `serializeExistingTypeNode`, no fallback without `requiresAddingUndefined` |
| syntactic `serializeReturnTypeForSignature` | 133807–133829 | `392dd1cdbbe89fcce6955d0c3d92b5f8f79cb32596078ca3d747b39114d144f8` | kind dispatch to `createReturnFromSignature` |
| `inferReturnTypeOfSignatureSignature` | 133962–133970 | `0dc7d6a529bd4c9f52dbbd183a425c0b83ce8e4662657e1ff99f9d8c7d78f5e9` | semantic fallback |
| `typeFromTypeAssertion` | 133988–133999 | `35cfd4c12775e6d02a0ffe014b8eddb48abf9a7974d39eb816ec11e89a553af2` | serializes the asserted type node |
| `typeFromExpression` | 134000–134082 | `618dccd876b1fdcf9053f5bfabc74f50521767b59a4b2b3bb3e2f5b441562ca7` | ParenthesizedExpression + JSDoc assertion branch |
| `createReturnFromSignature` | 134397–134406 | `1e237223cdce33e60b17473553ec75a4a4a36d23e0147d6c3c33037a16063359` | annotation → single return expression → resolver |
| `typeFromSingleReturnExpression` | 134407–134441 | `300d9bc8493ff2d3f460dc696531a2ebf20651741256b7a044ab0d41e8c55138` | one top-level `return`, contextual typing gate |
| `isContextuallyTyped` | 134442–134446 | `045888c7689997501d48c7fecb06b6beefc553a072825502ad1019cc0ba70468` | call/annotated/JSX ancestors |
| `getTypeNamesForErrorDisplay` | 50748–50756 | `40a0bc0eba39778afa87e7a1acdc421c12ecc5e0d0c117c72327676c61922597` | error display: enclosing declaration only for context-sensitive expression symbols |
| `checkAndAggregateReturnExpressionTypes` | 78959–79008 | `69b5d219762f77c14a66f98a7981ba6bfa0ee5411becccc95ddf59efef3e609e` | `skipParentheses(expr, /*excludeJSDocTypeAssertions*/ true)` for the return expression and the `await` operand; the checked expression keeps its JSDoc assertion |
| `skipParentheses` | 15661–15664 | `57477e009374b3ffadffee5b4db7695a3c33fc1710ed92ce3c06ab51d147e7f3` | `ExcludeJSDocTypeAssertion` flag |
| `isOuterExpression` / `skipOuterExpressions` | 27561–27581 / 27582–27587 | `5516dd616d83f3a2e9d8caaf560d9d12c8a8718fd446d453702b1a81ab8da298` / `8b1eff7c004dde6bbe6b5940ba064195f1aea6668ca5d8b1f4a69bf9cec4dec1` | a parenthesized JSDoc type assertion is not an outer expression under that flag |

Reading of the upstream order for a value signature declaration `fn`:

1. `returnTypeNode = getEffectiveReturnTypeNode(fn)`; in a JS file this is
   `getJSDocReturnType(fn)`, whose tag lookup walks the *owned* chain. For
   `module.exports.d = function d(...)` the chain is FunctionExpression (no
   attachment) → `getNextJSDocCommentLocation` returns the ExpressionStatement
   because the parent BinaryExpression is an assignment → the statement's last
   block owns `@param`, `@param`, `@return`.
2. A found node is serialized through `serializeTypeAnnotationOfDeclaration`
   (reuse check against the semantic return type, then existing-node reuse).
   When reuse is refused the result is `undefined` and step 3 is skipped
   because `returnTypeNode` exists: the semantic resolver serializes the
   return type without reporting a fallback.
3. Only when no annotation node exists does `typeFromSingleReturnExpression`
   run; a JSDoc type assertion `/** @type {*} */(null)` that is not
   contextually typed serializes `*`, that is `any`.

## 3. Rust state at the start source

Start hashes (selection inventory): `crates/binder/src/node_util.rs`
`7bb0a612b1307952f6ef4995394b6f618a27447f02c053e29ff681a421db0f04`,
`crates/checker/src/syntactic_type_node_builder.rs`
`655accdcf99fc6a636f56dc2c379ca4cff754978f4b5873dfd217379a7fe731e`,
`crates/checker/src/jsdoc.rs` `6c6b28c7…`, `functions.rs` `7e38449b…`,
`node_builder/serialize.rs` `b3374f68…` (full values in the inventory).

| Path | Rust owner at the start source | Fact |
| --- | --- | --- |
| Binder ownership traversal | `node_util::visit_owned_jsdoc_tags` (initializer attachment, then host and `get_next_jsdoc_comment_location`), `visit_owned_jsdoc_tags_from_attachment` (last-block ownership, `@overload` from earlier blocks), `owns_jsdoc_tag` | AST-only port of `getJSDocCommentsAndTags` / `filterOwnedJSDocTags` / `ownsJSDocTag` / `getNextJSDocCommentLocation`; already exercised by `get_jsdoc_type_tag` and `get_jsdoc_parameter_tags` |
| Semantic JSDoc | `CheckerState::get_jsdoc_tags` (cached owned chain), `first_jsdoc_tag`, `get_jsdoc_return_type` (`jsdoc.rs`) | Mirrors the upstream worker; `functions.rs::effective_return_type_node` consults it, so the semantic return annotation route is already owned-chain based |
| Syntactic return worker | `SyntacticBuildSession::effective_return_type_node` → `get_jsdoc_return_type` → **`direct_jsdoc_tags`** | `direct_jsdoc_tags` reads only `node.js_doc` of the host: no next-location walk, no initializer attachment, no last-block ownership filter |
| Syntactic `@type` lookup | `SyntacticBuildSession::get_jsdoc_type` → `node_util::get_jsdoc_type_tag` / `get_jsdoc_parameter_tags` | Already uses the binder traversal (parameter repair) |
| Return dispatch | `create_return_from_signature`, `type_from_single_return_expression`, `is_contextually_typed`, `type_from_expression` (JSDoc assertion branch), `serialize_type_annotation_of_declaration`, `infer_return_type_of_signature_signature` | Faithful to the upstream order; they are not the cause |
| Semantic → syntactic seam | `node_builder/serialize.rs::serialize_return_type_for_signature_in_context` (port of 53524–53546) and `ProductionSyntacticBuilderResolver::can_reuse_type_node_annotation` (port of 50932–50955) | Unchanged; the reuse check reads the semantic return type of the signature |

Two divergences live in the start `get_jsdoc_return_type` of the syntactic
builder, both closed by delegating to the binder port, and one adjacent
semantic divergence (C2, §3.1) was proven by the before controls:

- **D1 ownership**: tags attached to the owning statement, a variable's
  initializer attachment, a property assignment or a return statement are
  invisible; only tags attached directly to the function node are read.
  Reached by every `module.exports.x = function` / `exports.x = function` /
  `const v = function` / `o.m = function` / arrow host.
- **D2 first-tag selection**: the start code returns the first return tag
  *that has a type*, while upstream reads only the first return tag and falls
  to the callable `@type` route when it has no type expression. Reachable
  only through a direct attachment (a function declaration or class member
  with `@return` followed by `@return {T}`): control
  `r3-untyped-then-typed-return-tags`.

### 3.1 C2: return aggregation drops JSDoc type assertions (semantic)

Before control `r3-type-assertion-parenthesized-owner` reports TS2352
"Conversion of type '() => any' to type '() => string' may be a mistake …
Type 'null' is not comparable to type 'string'" (exit 2) where TypeScript
reports nothing (exit 0); `r3-untyped-then-typed-return-tags` prints
`z(): null` through the semantic fallback where TypeScript's semantic return
type is `any`. Upstream probes on the pinned compiler: `function a() { return
/** @type {*} */(null); }` has return type `any`; a bare `return null` stays
`null`. `CheckerState::check_and_aggregate_return_expression_types`
(`functions.rs`, port of 78959–79008) skips the return expression's
parentheses with the binder's plain `node_util::skip_parentheses_pub`, both
for the expression and for the `await` operand, so the parenthesized JSDoc
assertion is removed before `check_expression_cached` and the bare `null` is
checked. Upstream passes `excludeJSDocTypeAssertions = true`, which keeps a
JSDoc type assertion as the checked expression. The checker already owns the
faithful helper `skip_parentheses_excluding_jsdoc_type_assertions`
(`evaluate.rs`, same ledger span). C2 is reached only on the semantic route
(multiple returns, generators, contextual comparisons and diagnostics
display); the original G5c takes the annotation route after D1 and does not
depend on it. Witnesses: `r3-type-assertion-parenthesized-owner`,
`r6-asserted-returns-semantic-fallback`,
`r6-asserted-await-returns-semantic-fallback`. The other sixteen checker
call sites of `skip_parentheses_pub` are not audited here (§7).

### 3.2 D3: diagnostic signature display keeps its own direct-only return worker

After D1/D2/C2 one control stayed red: `r3-return-tag-over-callable-type-tag`
renders TS2322 as "Type '() => any' is not assignable to type '() => number'"
where upstream renders `'() => string'`; the `.d.ts` and the elaboration
line agree with upstream. Error display does not use the node builder:
`CheckerState::get_type_name_for_error_display` → the text-slice renderer
(`check.rs`), whose `serialize_return_type_for_signature_slice` is a second
port of `createReturnFromSignature`. It reads only a direct `.type`
annotation (no `getEffectiveReturnTypeNode` JS route, 16768–16770) and then
runs the single-return shortcut even when an annotation exists, while
upstream runs `typeFromSingleReturnExpression` only when no annotation node
exists (134397–134406). The renderer's reuse step already models the
enclosing-declaration gate of `canReuseTypeNodeAnnotation`
(`getTypeNamesForErrorDisplay`, 50748–50756: the value declaration is the
enclosing declaration only for context-sensitive expression symbols) and
lowers JSDoc type-node shapes like `visitExistingNodeTreeSymbolsWorker`, so
the closure is the lookup and the order, nothing in the reuse channel.

## 4. Cause proof on the same nodes

TypeScript side (public API, `scratch trace-g5c.mjs` over the frozen original
input; recorded again per control as `return_trace` in the fixture):
`d` = `FunctionExpression` pos 259 end 311, `direct_jsdoc` 0, ownership chain
FunctionExpression → ExpressionStatement (one block), owned tags
`JSDocParameterTag`×2 + `JSDocReturnTag` (all attached to the
ExpressionStatement), `getJSDocReturnType` = `StringKeyword` 228..234,
`getEffectiveReturnTypeNode` = the same node; single return candidate
`ParenthesizedExpression` 285..308, a JSDoc type assertion whose type is
`JSDocAllType` 297..298. `e` = `FunctionExpression` 408..460, owned tags
`JSDocTemplateTag` + `JSDocParameterTag`×2 + `JSDocReturnTag`,
return node `IntersectionType` 378..383, type parameters `T` 330..331 and
`U` 332..333 declared by the template tag, candidate 434..457 asserting
`JSDocAllType` 446..447.

Rust side (temporary `eprintln!` instrumentation of
`create_return_from_signature`, `type_from_single_return_expression`; patch
retained as `instrumentation.patch` in the before run directory and removed
before the fix commit): for the same hosts
`FunctionExpression@259..311` and `@408..460` of `/.src/index.js`,
`direct_jsdoc_tags=0`, `return_type_node=None`, `value_signature=true`; the
single-return candidate is `ParenthesizedExpression@285..308` /
`@434..457`, `jsdoc_type_assertion=true`, `contextually_typed=false`, and
`type_from_single_return_expression` produces `AnyKeyword@297..298` /
`@446..447`, the serialized `*`. Function declarations `g`/`hh` show
`direct_jsdoc_tags=2` (direct attachment works) and take the semantic
fallback because their bodies are not single-return. The instrumented run's
two complete captures are identical to the frozen before receipt's captures
(same four `/writes/1` difference paths, exit 101), so the instrumentation
observed without changing behaviour and is not used as an oracle.

Conclusion: the mis-selection is at the **annotation step** (D1); the
shortcut and the semantic fallback behave as upstream once the annotation is
found or absent. The semantic checker's own return type is not consulted by
the failing route at all before the fix; after the fix it is consulted by the
reuse check, which the R4/R5 controls observe end to end.

## 5. Change design

### 5.1 Binder: AST-only return lookup (`crates/binder/src/node_util.rs`)

```rust
/// tsc-port: getJSDocReturnTag @6.0.3
/// tsc-hash: 9b02cc14a58da114906e21bbb106c7cf8c0bf85e3555915f43e295c59776dd20
/// tsc-span: _tsc.js:11708-11710
pub fn get_jsdoc_return_tag(source: &SourceFile, host: NodeId) -> Option<NodeId>

/// tsc-port: getJSDocReturnType @6.0.3
/// tsc-hash: 7bcd67792fdeaceeecc2c6ff990b94e0065646e3068932012592c00efafe9eea
/// tsc-span: _tsc.js:11728-11744
pub fn get_jsdoc_return_type(source: &SourceFile, host: NodeId) -> Option<NodeId>
```

- `get_jsdoc_return_tag` visits `visit_owned_jsdoc_tags(source, host, …)` and
  stops at the first `JSDocReturnTag` (the `find` of `getFirstJSDocTag` over
  the worker's order).
- `get_jsdoc_return_type` reads that tag: when it carries a
  `JSDocTypeExpression` the expression's `type` is the answer, whether or not
  it is present (upstream returns `typeExpression.type` without falling
  through). Without a type expression it consults `get_jsdoc_type_tag` (the
  existing `getJSDocTypeTag` port, which already requires a typed
  expression) and projects `TypeLiteral` → first `CallSignature.type`,
  `FunctionType.type`, `JSDocFunctionType.type`, otherwise `None`.
- Lifetimes: both borrow `&SourceFile` for the call only and return `NodeId`s
  of that same source; nothing is cached or retained. No scanner, parser,
  binder assignment classifier or symbol change.

### 5.2 Checker syntactic builder (`crates/checker/src/syntactic_type_node_builder.rs`)

- `SyntacticBuildSession::get_jsdoc_return_type(node)` becomes the mirror of
  `get_jsdoc_type`: resolve `self.arena.source(node.source())` to the parsed
  `SourceFile`, call `node_util::get_jsdoc_return_type(source, node.node())`,
  and wrap the result as `TransformNode::new(node.source(), id)`. The result
  is always a node of the host's own `TransformSourceId`; no node of another
  source is produced, so the #4e attribution invariant of
  `type_from_expression` is untouched.
- `direct_jsdoc_tags` is deleted (its only caller was the return worker).
  `jsdoc_type_from_tag` stays for `get_jsdoc_type` and the JSDocSignature
  branch of `effective_return_type_node`.
- `effective_return_type_node`, `create_return_from_signature`,
  `type_from_single_return_expression`, `serialize_type_annotation_of_declaration`
  and the resolver callbacks are not edited.

### 5.3 Checker semantic return aggregation (`crates/checker/src/functions.rs`)

- In `check_and_aggregate_return_expression_types`, the two
  `node_util::skip_parentheses_pub(...)` calls (the return expression and the
  `await` operand) become
  `self.skip_parentheses_excluding_jsdoc_type_assertions(...)`, the existing
  port of `skipParentheses(node, /*excludeJSDocTypeAssertions*/ true)`. The
  checked expression is then the assertion itself, as upstream. Nothing else
  in the function changes; no widening rule is touched (a bare `null` return
  stays `null` in both compilers).

### 5.4 Checker diagnostic signature display (`crates/checker/src/check.rs`)

- In `serialize_return_type_for_signature_slice`, the direct `.type` match is
  replaced by `self.effective_return_type_node(declaration)` (the semantic
  port of `getEffectiveReturnTypeNode`: JSDocSignature, direct type, JS JSDoc
  route through the owned chain) and the single-return shortcut moves into
  the `else` branch, so it runs only when no annotation node exists. The
  reuse step (`annotation_reuse_text_slice`) and the semantic rendering are
  unchanged. The `isJSDocConstructSignature` arm of `createReturnFromSignature`
  stays unmodeled in this renderer, as before (a construct signature has no
  function-like declaration kind and renders semantically).

### 5.5 What is deliberately not changed

- Semantic `jsdoc.rs`/`functions.rs`/`contextual.rs`/`annotate.rs`: the
  semantic route already walks the owned chain; the R4/R5/R6 controls verify
  that the reuse check agrees with the annotation for `T & U`, constrained /
  defaulted template parameters, typedef and import returns, literal, union,
  callable and JSDoc function types.
- `node_builder/signatures.rs` / `serialize.rs`: the seam that hands the
  declaration and symbol to the syntactic builder is unchanged; symbol,
  location and type-parameter identity keep flowing from the semantic
  signature.
- No production branch depends on names, tag text or source text.
- The generated `nodes.rs`, the parser, printer, program paths, harness,
  xtask, shared comparators, `.github/`, global manifests and
  `crates/oracle/` are untouched.

### 5.6 Edit sequence and commits

1. `fix(checker)` first cause commit: the two binder functions of §5.1 and
   the delegation of §5.2 (D1 and D2 close together because the fix is the
   same delegation; the design records both).
2. `fix(checker)` second cause commit: the C2 closure of §5.3.
3. `fix(checker)` third cause commit: the D3 closure of §5.4.
4. `test(compiler)`: freeze the controls (observer, fixture, compiler
   consumer, checker node-identity control); their red state at the start
   source is recorded by the before run, and every commit on the branch
   keeps the suites green.
5. `docs`: this design, its readiness manifest and check, the report, the
   packet index row.

Any further cause discovered by the controls gets its own commit and its
own row in §9; nothing is fixed by editing expectations, filters or
comparison depth.

## 6. Controls and witnesses

Observer `scripts/observe-h2-8a-jsdoc-return.mjs` → fixture
`crates/compiler/tests/fixtures/h2-8a-jsdoc-return.json`
(SHA-256 `687d28b13459dff3b472515c430f66165c4bdaaec6f07bbbb893ca2997d54ba3`,
58 cases, 116 complete command executions, 70 traced function-like nodes).
Each case stores the two identical complete observations (writes as UTF-16
units plus UTF-8 base64, diagnostics, status writes, emit result, exit) and
the per-function upstream `return_trace`. Consumers:

- `crates/compiler/tests/h2_8a_jsdoc_return.rs`
  `jsdoc_return_controls_match_complete_commands_twice` (all 58, each twice,
  capture directory `TSC_RS_JSDOC_RETURN_CAPTURE_DIR`, optional
  `TSC_RS_JSDOC_RETURN_FILTER` for targeted reruns that never replace the full
  run) and `original_g5c_complete_command` (the unchanged original tuple
  through the shared original-corpus comparator).
- `crates/checker/tests/unit/syntactic_type_node_builder/tests.rs`
  `jsdoc_return_lookup_matches_upstream_node_identity`: for every traced
  host, `get_jsdoc_return_type` and `effective_return_type_node` of the
  syntactic session must return the upstream node identity (kind, pos, end,
  file) or `null`, twice.
- The existing strict `original_shared_g5c_complete_command` in
  `h2_8a_declaration_comment_ranges.rs` stays unchanged and must pass.

Options mirror the original tuple (`target` es2015, `allowJs`, `checkJs`,
`declaration`, `outDir`, no `module`) unless the row says otherwise. Expected
`.d.ts` return rendering per row (from the frozen upstream observations):

| Group | Case | Expected upstream result | Route it isolates |
| --- | --- | --- | --- |
| R1 | `r1-original-d-string` | `d(a: number, b: number): string` | D1, outer `@return` over body `*` |
| R1 | `r1-original-e-template-intersection` | `e<T, U>(a: T, b: U): T & U` | D1 + template identity reuse |
| R1 | `r1-returns-alias` | `d(): string` | `@returns` spelling |
| R1 | `r1-body-star-without-annotation` | `d(): any` | shortcut kept without annotation |
| R1 | `r1-plain-body-without-annotation` | `p(): number` | shortcut kept, literal widening |
| R2 | `r2-exports-property` | `f(a: any): string` (TS7006) | `exports.x` assignment statement |
| R2 | `r2-variable-initializer` | comment + `v(): string` | variable statement chain |
| R2 | `r2-function-declaration` | comment + `g(): string` | direct attachment positive |
| R2 | `r2-arrow-expression-body` | `arrow(): string` | expression body candidate |
| R2 | `r2-parenthesized-function` | `paren(): any` | chain stops at ParenthesizedExpression (negative) |
| R2 | `r2-prototype-method` | class `C` with `m(): string` | prototype assignment |
| R2 | `r2-ordinary-property` | namespace `o` with `m(): string` | expando property assignment |
| R2 | `r2-object-literal-method` | namespace `obj` with `m(): string` | method direct attachment |
| R2 | `r2-property-assignment-function` | namespace `obj` with `n(): string` | PropertyAssignment parent |
| R2 | `r2-class-method` | class `K` with `m(): string` | class member direct attachment |
| R3 | `r3-inner-tag-before-outer` | `x(): number` | host's own block precedes the statement's |
| R3 | `r3-last-block-owns-tags` | `y(): string` | last block owns |
| R3 | `r3-last-block-owns-tags-declaration` | two comments + `y(): string` | last block owns under direct attachment (D1 ownership filter) |
| R3 | `r3-untyped-return-tag` | `z(): any` | untyped tag → no node |
| R3 | `r3-untyped-then-typed-return-tags` | `z(): any` (TS1223) | D2 first-tag selection |
| R3 | `r3-return-tag-over-callable-type-tag` | `const t: () => number` (TS2322 naming `'() => string'`) | declared `@type` owns the symbol type; D3 in the diagnostic display |
| R3 | `r3-callable-type-tag-only` | `const t: () => number` | callable `@type` route |
| R3 | `r3-sibling-jsdoc-not-inherited` | `s1(): string`, `s2(): any` | no sibling inheritance |
| R3 | `r3-type-assertion-parenthesized-owner` | `const q: () => string` | `ownsJSDocTag` parenthesized owner |
| R4 | `r4-template-constraint-default` | `cd<T extends string, U = number>(a: T): T \| U` | constraint/default identity |
| R4 | `r4-template-shadows-typedef` | `s<T>(a: T): T`, `n(): T`, `type T = number` | template shadows typedef |
| R4 | `r4-typedef-return` | `mk(): Point` + `type Point` | typedef return reuse |
| R4 | `r4-import-type-return` | `mk(): import("./other").Thing` | import type reuse |
| R4 | `r4-same-name-template-two-sources` | `id<T>(a: T): T`, `wrap<T>(a: T): T[]` | per-source identity |
| R5 | `r5-literal-annotation` | `l(): "lit"` | literal annotation |
| R5 | `r5-union-annotation` | `u(): string \| number` | union |
| R5 | `r5-callable-type-literal` | `c(): { (): void; x: number; }` | type literal |
| R5 | `r5-jsdoc-function-type` | `jf(): (arg0: number) => string` | JSDoc function type |
| R5 | `r5-async-promise` | `as(): Promise<string>` | async |
| R5 | `r5-generator` | `gen(): Generator<number>` | generator |
| R5 | `r5-async-generator-without-annotation` | `ag(): AsyncIterableIterator<number, void, unknown>` | semantic fallback |
| R5 | `r5-single-return-without-annotation` | `one(): string` | shortcut |
| R5 | `r5-multiple-returns-without-annotation` | `two(b: any): 1 \| 2` (TS7006) | semantic fallback |
| R6 | `r6-ts-direct-type-ignores-jsdoc` | `.ts`: `t(): number` with the comment kept | direct type wins |
| R6 | `r6-accessor-pair` | namespace `acc` with `let g: string` | accessor route |
| R6 | `r6-jsdoc-construct-signature` | `const ctor: new (arg1: number) => Point` (TS2322, TS7006) | construct signature first parameter |
| R6 | `r6-annotation-body-mismatch` | `mis(b: any): number` (TS7006, TS2322) | annotation over body, diagnostics kept |
| R6 | `r6-unresolved-annotation` | `bad(): NotDefined` (TS2552) | error type reuse |
| R6 | `r6-asserted-returns-semantic-fallback` | `c(x: any): any` (TS7006) | C2: two asserted returns through the semantic fallback |
| R6 | `r6-asserted-await-returns-semantic-fallback` | `aw(p: Promise<number>): Promise<any>` | C2: asserted `await` operand and asserted awaited value |
| R7 | `r7-allowjs-without-checkjs` | d/e exact, no diagnostics | `checkJs` off |
| R7 | `r7-emit-declaration-only` | only `main.d.ts` | `emitDeclarationOnly` |
| R7 | `r7-noemitonerror-blocked` | no writes, exit 1 | `noEmitOnError` |
| R7 | `r7-noemit-with-declaration` | no writes, exit 0 | `noEmit` + declaration diagnostics gate |
| R7 | `r7-declaration-map` | `main.d.ts.map` + `sourceMappingURL` | `declarationMap` |
| R7 | `r7-remove-comments` | d/e exact | `removeComments` |
| R8 | `r8-lone-surrogate-escape-literal` | `lone(): "\uD800"` | escaped lone unit literal |
| R8 | `r8-astral-pair-literal` | `pair(): "😀"` | astral pair escaping |
| R8 | `r8-replacement-character-literal` | `rep(): "�"` | U+FFFD literal |
| R8 | `r8-g4a-function-parameter-and-return` | typedef + comment + `g(a: Num): string` | G4a location kept |
| R8 | `r8-g4b-class-method-owned-comment` | class comment + `m(a: number): string` | G4b kept |
| R8 | `r8-detached-prefix-with-return` | `d(): string` | detached prefix kept |
| R8 | `r8-parameter-tag-binding-with-return` | `f({ x }: { x: number; }): string` | parameter-tag lookup kept |

## 7. Boundaries, other owners and regressions

Production edits: `crates/binder/src/node_util.rs`,
`crates/checker/src/syntactic_type_node_builder.rs`, the two skip calls of
`check_and_aggregate_return_expression_types` in
`crates/checker/src/functions.rs` (§5.3) and the annotation lookup/order of
`serialize_return_type_for_signature_slice` in `crates/checker/src/check.rs`
(§5.4) only. Tests and evidence:
the two consumers above, the observer, the fixture, this document, the
report and the packet index row.

Regression set (all on the final source, each real exit recorded):
`h2_8a_declaration_comment_ranges` (all six tests, G5c included and not
skipped), `h2_8a_jsdoc_return` (both tests), the binder crate tests, the
checker library tests (syntactic builder, node builder, JSDoc and the rest of
the checker unit suite), `h2_8a_declaration_specifiers`,
`h2_8a_utf16_review_fix_controls`, `h2_8a_utf16_identity_recovery_controls`,
`cargo fmt --all -- --check`, all-targets check of the changed crates.
Known inherited failures outside this slice stay as listed in
[h2-8a-utf16-adjacent-repair.md §33](h2-8a-utf16-adjacent-repair.md#33-implementation-review-fix-round-2026-09-14);
G5c is not one of them.

Other owners recorded, not fixed here: the remaining sixteen checker call
sites of `node_util::skip_parentheses_pub` (contextual, expression, literal
and operator paths) whose upstream counterparts may or may not pass
`excludeJSDocTypeAssertions`; none is reached by a G5c control, so they are
listed for a separate audit, not changed; the parser's attachment of an inline
JSDoc block inside a one-line object literal or class body (upstream does not
attach it; every control keeps its block on its own line so parser
attachment is not exercised), and the semantic `get_jsdoc_return_type`'s
fall-through when a return tag has a type expression without a type (a
parser-unreachable shape, left as is).

## 8. Readiness

| Gate row | State |
| --- | --- |
| Upstream owners pinned with whole-line hashes | §2, checked by `scripts/check-h2-8a-jsdoc-return-readiness.mjs` |
| Rust types / functions / lifetimes fixed | §5.1–§5.2 |
| Edit steps and commit plan | §5.4 |
| Witnesses: fixture hash, counts, consumers | §6, checked by the readiness script |
| Positive and negative controls per reached branch | §6 table |
| Boundaries and other owners | §7 |
| Architecture rows reached | `E-DECL-SESSION` (one borrowed checker per session, unchanged), `E-DECL-TRACKER-PARENT` (`report_inference_fallback` path, unchanged); no row governs the syntactic JSDoc lookup itself, and the change adds no seam, API or cache, so no row is rewritten; the packet index gains one row |
| Unresolved items | 0 |

Readiness manifest: [h2-8a-jsdoc-return-readiness.v1.json](h2-8a-jsdoc-return-readiness.v1.json).

## 9. Execution record

Run directories live under `target/jsdoc-return-runs/` (dedicated Cargo
target `target/jsdoc-return`, `CARGO_BUILD_JOBS=2`, `taskpolicy -b nice -n 15`,
`--test-threads=1`, one heavy native command at a time; every step's
command, environment, exit code and log hashes are in that directory's
`receipt.json`, written by `run-step.py`).

### 9.1 Before (start source plus the temporary trace)

Run directory `target/jsdoc-return-runs/before-20260914-155056`. Steps and real exits:
`g5c-original-before-trace` 101 (two complete captures identical to the
frozen receipt, four `/writes/1` difference paths; the trace lines of §4),
`controls-before` 101 (55-case fixture: 21 exact, 34 failures),
`checker-lookup-before` 101 (12 existing syntactic-builder tests pass, the
node-identity control fails on 39 hosts × 2 with `Null` for every D1 host),
`controls-before-58` 101 (final fixture: 22 exact, 36 failures; the three
added controls: `r3-last-block-owns-tags-declaration` exact through the
semantic fallback, both C2 witnesses fail with `null` / `Promise<number>`).
Full classification in the report §2.

### 9.2 After (final source)

Run directory `target/jsdoc-return-runs/after2-final`; the full step table,
counts and the inherited clippy note are in the report §4.2. Summary: the
original G5c tuple exact twice; 58/58 controls exact twice; all six
comment-range tests, 13 syntactic-builder tests (the node-identity control
included), 73 binder tests, 238 node-builder/JSDoc/functions tests, the
three adjacent compiler suites (11 + 2 + 1) and the full checker library
(1738) pass; `cargo fmt --all -- --check` passes; clippy fails only in the
untouched `tsc-rs-program` crate (145 inherited lint errors, identical when
that crate is linted alone). The intermediate battery at the D1/D2/C2 head
(`after-20260914-160710`, report §4.1) already had the original exact and
57/58 controls; D3 closed the last one.
