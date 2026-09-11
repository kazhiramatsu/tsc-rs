# A6-41 decorator source follow-up: literal names, double hoists, undecorated owners

Design and execution record for the four remaining standard-decorator source
paths prepared in
[the follow-up handoff](h2-8a-decorator-followup-handoff.md) and pinned by
[the start manifest](h2-8a-decorator-followup-start.v1.json). Passing the 48
prepared commands is the claim of this record. It is not a claim that every
`transformESDecorators` source path or all of H2.8 is complete.

## Start point

- Worktree `/Users/hiramatsu/dev/tsc-rs-dec-followup`, branch
  `prep/h2-8a-decorator-followup`, HEAD `0fda49509` (the start manifest's
  `2953ecb8a` plus the preparation docs commit). The nine production inputs,
  the proposed inputs, the upstream observations and `_tsc.js` were
  re-hashed at the start and match the manifest byte for byte.
- [PR #512](https://github.com/kazhiramatsu/tsc-rs/pull/512) at the start:
  draft, head `2953ecb8a`, base `main`, mergeable, hosted `gates`
  (run 34565157958) still in progress. Not merged; the merge target and the
  63 prerequisite commits are the PR's own question. This work neither
  decides that nor touches the root production or the previous worktrees.
- Observations: `crates/compiler/tests/fixtures/decorator-source-followup.json`
  (48 commands, TypeScript 6.0.3, twice each, diagnostic-free). They are
  TypeScript facts only.

## Rust baseline (before any production change)

Test registration only: the `source-followup` group (48) was added to
`crates/compiler/tests/integration/h2_8a_decorator_next_witnesses.rs` and the
group run once on the unchanged production
(`standard_decorators.rs` `c0486540d…`). Receipt:
[`ratchets/h2-8a-decorator-followup-baseline.v1.json`](../../../../ratchets/h2-8a-decorator-followup-baseline.v1.json);
run directory `target/decorator-followup-runs/baseline-r1/` (log, 96
captures, archived binary `fc6f3768b…`, classification).

| family | result | cause |
| --- | --- | --- |
| `literal-computed-decorated-field` | 5 failed, ESNext/define exact | Rust routes `@dec ["x"]` through the `__propKey` temp (`var _a`, `_a = __propKey("x")`, context `name: _a`, `obj[_a]`); tsc keeps `"x"` everywhere and the pending decorator assignment flows to the static block |
| `identifier-decorated-field-control` | 6 exact | — |
| `object-computed-pending-absorption` | 6 exact | already matching: tsc's `visitor` never enters the inner object literal (`shouldVisitNode`: no `ContainsDecorators`); Rust's generic child visit injects nothing inside it either, so both consume the queue at the outer class computed name |
| `class-computed-pending-control` | 6 exact | — |
| `anonymous-in-decorated-computed-field` | 5 failed, ESNext/define exact | Rust hoists once, wraps once and names the inner class `""`; tsc hoists the same generated name twice (`var _a, _a`), wraps twice (`_a = __propKey(_a = __propKey(fieldKey))`) and names the class `_a` |
| `named-in-decorated-computed-field-control` | 6 exact | — |
| `undecorated-outer-property-hoist` | 5 failed, ESNext/define exact | Rust performs no named evaluation in an undecorated class: no `__propKey`, class-fields hoists its own `_a = fieldKey`, the inner class is named `""`; tsc hoists `var _a` into `make()`, rewrites the name to `[_a = __propKey(fieldKey)]` and names the class `_a` |
| `decorated-outer-property-hoist-control` | 6 exact | — |

The ESNext/define rows keep native decorators; their exactness is not a
lowering result. Full diffs: `baseline-r1/classification.txt`
(`976bc67b0…`).

## Upstream callees and predicates (pinned `_tsc.js`, sha `1c59e77a…`)

- `transformESDecorators.visitor` 99110–99195 gates every visit with
  `shouldVisitNode` (`ContainsDecorators`, or lexical this/super under a
  `classThis`); `ComputedPropertyName` routes to `visitComputedPropertyName`
  100369 only when reached.
- `partialTransformClassElement` 99831: a decorated member whose name is a
  `ComputedPropertyName` with `isPropertyNameLiteral(expression) &&
  !isIdentifier(expression)` gets `propertyName = { computed: true, name:
  createStringLiteralFromNode(expression) }` — no `visitReferencedPropertyName`,
  no temp, no hoist, no `__propKey`. Every other computed name enters
  `enterName(); visitReferencedPropertyName; exitName()`. The element name is
  then visited by `visitPropertyName` → `visitComputedPropertyName`, whose
  `injectPendingExpressions` only fires when the visited expression is not
  simply inlineable, so a literal leaves the queue for the next consumer
  (the leading static block of `transformClassLike` 99379–99400).
- `createESDecorateClassElementAccess{Get,Set,Has}Method` 25586–25677:
  `computed` selects `obj[name]` element access and `name in obj` with the
  same string literal. `getHelperVariableName` 99219 yields `member` for a
  `ComputedPropertyName` name.
- `visitPropertyDeclaration` 100041: `isNamedEvaluation(node,
  isAnonymousClassNeedingAssignedName)` runs before `enterClassElement`.
  `transformNamedEvaluationOfPropertyDeclaration` 93916 →
  `getAssignedNameOfPropertyName` 93770: a non-literal computed name takes
  `assignedName = getGeneratedNameForNode(name)`,
  `context.hoistVariableDeclaration(assignedName)` (innermost lexical
  environment), and `updateComputedPropertyName(name, assignedName =
  __propKey(name.expression))` with the expression unvisited.
  `finishTransformNamedEvaluation` injects `__setFunctionName(this,
  assignedName)` into the anonymous class.
- `visitReferencedPropertyName` 100345 on that updated name: the expression
  is an assignment, not a literal, so `getGeneratedNameForNode(node)` is
  requested again. `getNodeForGeneratedName` walks `.original` to the parsed
  name and `generateNameCached` keys on that node: the second generated
  identifier prints the same `_a`. `hoistVariableDeclaration` never
  de-duplicates, hence `var _a, _a`. The visited assignment is wrapped again:
  `_a = __propKey(_a = __propKey(fieldKey))`, then `injectPendingExpressions`.
- `visitClassDeclaration` 99628 undecorated branch: `enterClass(undefined)`
  and `classElementVisitor` per member, no `startLexicalEnvironment`; the
  named-evaluation hoist therefore lands in the enclosing environment —
  `visitFunctionBody` for a function body, `visitLexicalEnvironment` for the
  `SourceFile` (`visitEachChild`), both merged by `mergeLexicalEnvironment`
  after prologue directives. `partialTransformClassElement(member,
  undefined)` visits the rewritten name; the queue is empty in an
  undecorated class.
- Class-fields consumer (unchanged owner): `getPropertyNameExpressionIfNeeded`
  97620 treats an assignment to a generated identifier as
  `alreadyTransformed` and emits it after the class (ES2015) or in a static
  block (ESNext set); `findComputedPropertyNameCacheAssignment` 28193 reads
  the outer assignment's left for the field operation.

## Source → Rust owner → witness map

| `_tsc.js` (6.0.3) | Rust (`crates/emitter/src/builtins/standard_decorators.rs`) | witness | change |
| --- | --- | --- | --- |
| `partialTransformClassElement` literal-computed branch, `createStringLiteralFromNode`, `createESDecorateClassElementAccess*` computed form, `getHelperVariableName` | `decorator_property_name`, `PropertyPlan`/`MethodPlan` (`computed_expression`), `partial_transform_property_plan`, `partial_transform_method_plan`, `create_decorator_context`, `create_method_decorator_context`, `create_access_object` | `literal-computed-decorated-field` (fields); the method/accessor branch of the same callee has no witness | cause 1 |
| `visitPropertyDeclaration` named evaluation before `enterClassElement`, `getAssignedNameOfPropertyName`, `visitReferencedPropertyName` second `getGeneratedNameForNode`/`hoistVariableDeclaration`, `finishTransformNamedEvaluation` | `prepare_property_named_evaluation` (decorated early return), `transform_class_member`, `visit_referenced_property_name`, `hoist_temp_variable`, `inferred_class_name_references` → `class_expression_runtime_name` → `create_set_function_name_block` | `anonymous-in-decorated-computed-field` | cause 2 |
| undecorated `visitClassDeclaration`/`visitClassExpression` + `classElementVisitor` → `visitPropertyDeclaration` named evaluation hoisting into the enclosing `visitFunctionBody` environment | `visit_class_members_generic` (literal-only recording), `prepare_property_named_evaluation` (undecorated path), `visit_function_like_body`, `merge_block_environment` | `undecorated-outer-property-hoist` (inside a function) | cause 3 |
| `visitEachChild(SourceFile)` → `visitLexicalEnvironment` + `mergeLexicalEnvironment` (top-level owner of the same hoist) | `transform_root` (visits the source file with no lexical environment) | none in the 48; a top-level class needs its own witness before a claim | reachability extension, witnessed separately below |
| `visitor`/`shouldVisitNode` skipping decorator-free subtrees; `visitComputedPropertyName` at the member | generic `visit_with_value_use` child visit (no injection inside object literals); `previsit_property_name` injection at the member name | `object-computed-pending-absorption`, `class-computed-pending-control` | none (already exact) |

## Ownership decisions

- **Scope of temporaries.** Cause 1 allocates no temporary. Cause 2 keeps
  both hoists in the decorated class's own IIFE environment
  (`transform_class_like` → `start_lexical_environment`), in tsc's order:
  named-evaluation hoist, member decorator transforms, referenced-name hoist.
  Cause 3 hoists into the innermost enclosing environment on the visitor's
  stack, which for the witnesses is the `make()` body
  (`visit_function_like_body`), declared at the block start by
  `merge_block_environment`.
- **Generated-name identity.** The double hoist is one `TargetBinding`
  declared twice: `hoist_existing_temp_variable` pushes the same binding
  into the current environment's temporaries without advancing the temp
  ordinal, so the finalizer prints one spelling for both declarators and
  every reference (`getGeneratedNameForNode` caching per parsed node). No
  string comparison creates the identity.
- **Visit order and frames.** The named-evaluation rewrite uses the
  unvisited key expression (tsc passes `name.expression` to
  `createPropKeyHelper` before any visit). For a decorated member the
  rewritten name is not previsited; `visit_referenced_property_name` visits
  the assignment under the name frame, exactly where tsc visits it. For an
  undecorated member the rewritten name is previsited under the name frame
  (`visitPropertyName` in `partialTransformClassElement(member, undefined)`).
- **Ranges and flags.** Rewritten computed names update the parsed name node
  (`updateComputedPropertyName`), keeping its text range and original chain
  through both updates, and carry `GENERATED_COMPUTED_PROPERTY_NAME` so the
  class-fields owners (`find_computed_property_name_cache`, downlevel
  6547) read the cached key; the literal branch sets nothing and the
  parsed `["x"]` is visited generically. The literal context name is a
  synthesized string literal whose `string_literal_text_source` is the
  parsed string literal (the `textSourceNode` source-string branch), so
  the source spelling (quotes, escapes) is reproduced; numeric and
  no-substitution-template sources use their literal text, as
  `getTextOfIdentifierOrLiteral` does.
- **Class-fields boundary.** No change to `class_fields.rs`/`downlevel.rs`:
  the shapes handed over (`[temp = __propKey(…)]` with the flag; plain
  `["x"]`) are the ones the decorated controls already exercise.
- **Helper request.** `__propKey` is requested only where a temp exists
  (causes 2 and 3), never for the literal branch.

## Execution record

Per-cause commits follow this record; each is verified with the focused
witness filter on the fixed binary before the next cause. Results, receipts
and the final scope are appended below as they are produced.
