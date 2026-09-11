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

Per-cause commits follow this record; each was verified with the focused
witness filter (`TSC_RS_DECORATOR_NEXT_CASE_FILTER`) on the binary built from
that commit before the next cause was applied. Every run directory under
`target/decorator-followup-runs/<run>/` holds `prelaunch.json` (head, git
status, production/fixture/test/runner/Cargo.lock/`_tsc.js` SHA-256),
`production-and-witness-inputs.tar.gz`, `run.log`, `captures/` (two complete
captures per case), `binaries/` (the executed test binary) and `receipt.json`;
the receipts are copied into `ratchets/`. Runner tooling:
`target/decorator-followup-runs/tools/` (`run-followup-witnesses.py`,
`run-full530.py`, `run-emitter-suites.py`, `diff-captures.py`,
`compare-with-full62.py` (SHA-256 `23507016ab…`, the previous work's script),
`final-chain.sh`).

### Cause 1 — literal computed names of decorated members (commit `8b2660f19`)

`decorator_property_name` now returns the helper stem, the computed
expression needing a cache temp, and the literal expression of a computed
name that `partialTransformClassElement` keeps (`isPropertyNameLiteral(expression)
&& !isIdentifier(expression)`: string, numeric, no-substitution template).
`PropertyPlan`/`MethodPlan` carry `computed_literal`; the plan's partial
transform takes the `previsit_class_element_name` path (name frame, no
temp, no hoist, no `__propKey`; the literal is simply inlineable so the
pending decorator assignment is left for the class static block, as for an
identifier name). `create_decorator_context`/`create_method_decorator_context`
name the context by `create_string_literal_from_property_literal`
(`createStringLiteralFromNode`: literal text; a string source also sets
`string_literal_text_source`, the `textSourceNode` branch, so the source
spelling prints). `create_access_object` takes `ComputedAccessName`
(`Temp` | `Literal`): `has` uses the key itself, `get`/`set` use element
access, matching `createESDecorateClassElementAccess{Has,Get,Set}Method`.

Focused run `target/decorator-followup-runs/cause1-r1/`
([receipt](../../../../ratchets/h2-8a-decorator-followup-cause1-focused.v1.json)):
filter `-decorated-field` = `literal-computed-decorated-field` ×6 +
`identifier-decorated-field-control` ×6, 12 exact twice, exit 0, sources and
witness inputs unchanged after the run.

Unwitnessed branches of the same callee, implemented without a claim: decorated
methods/accessors with literal computed names; numeric and template literal
keys (tsc prints a template source's own backticked text through
`getLiteralTextOfNode`, which the Rust printer's text-source branch reproduces
only for string sources — a printer-owner row if a witness ever needs it).

### Cause 2 — anonymous class in a decorated computed field (commit `707704835`)

`prepare_property_named_evaluation` no longer returns early for decorated
members. It hoists `getGeneratedNameForNode(name)` (`hoist_temp_variable`,
reserved in nested scopes) in the class IIFE environment, rewrites the parsed
name to `[temp = __propKey(expression)]` with the key expression unvisited,
registers the class under `inferred_class_name_references` (so
`class_expression_runtime_name` yields `AssignedReference(temp)` and the inner
class gets `__setFunctionName(_classThis, _a)`), and returns
`PropertyNamedEvaluation::Decorated { name, assignment, temporary }`.
`transform_class_member` stores those on the plan (`data.name`,
`computed_expression`, `named_evaluation_temp`); `partial_transform_property_plan`
passes the binding to `visit_referenced_property_name`, which declares it again
through `hoist_existing_temp_variable` (same binding pushed into the same
environment's temporaries, no ordinal advance: `var _a, _a`), visits the
assignment under the name frame, wraps it (`_a = __propKey(_a =
__propKey(fieldKey))`), injects the pending decorator assignment and updates
the rewritten name (range and original chain through both updates,
`GENERATED_COMPUTED_PROPERTY_NAME` for the class-fields owners, whose
`find_computed_property_name_cache` reads the outer assignment unchanged).
The print finalizer merges repeated identifier events of one binding, so both
declarators and every reference print one spelling.

Focused run `target/decorator-followup-runs/cause2-r1/`
([receipt](../../../../ratchets/h2-8a-decorator-followup-cause2-focused.v1.json)):
filter `-in-decorated-computed-field` = `anonymous-in-decorated-computed-field`
×6 + `named-in-decorated-computed-field-control` ×6, 12 exact twice, exit 0,
sources and witness inputs unchanged after the run.

### Top-level witnesses — reachability extension for cause 3 (commit `59bc7252e`)

The prepared `undecorated-outer-property-hoist` family reaches the hoist only
inside `make()`. The same callee's other environment owner,
`visitEachChild(SourceFile)` → `visitLexicalEnvironment` +
`mergeLexicalEnvironment`, had no witness, and the Rust source-file visit
owned no lexical environment (a top-level hoist would have failed with
"lexical environment for a hoisted temporary" once cause 3 hoisted). A new
group `source-followup-top-level` (inputs
`crates/compiler/tests/fixtures/decorator-source-followup-top-level-inputs.json`,
observations `…-top-level.json`, 18 commands: the top-level class, the same
file behind a `"use strict"` prologue, and the decorated control; six
target/define configurations each) was observed with the same observer
(`scripts/observe-decorator-next-witnesses.mjs`, group added; the four
existing groups are untouched): 36 Program executions, 0 diagnostics, 0
exceptions, exit 0 (`target/decorator-followup-prep/top-level-r1/`). TypeScript
places `var _a;` after the prologue directive and before the first statement
(the printer emits the helpers between them), exactly `mergeLexicalEnvironment`'s
`leftHoistedFunctionsEnd` splice. The 48 prepared inputs and observations are
unchanged (SHA-256 as in the start manifest).

Baselines on the cause-2 production (`target/decorator-followup-runs/cause3-baseline-r1/`,
`cause3-toplevel-baseline-r1/`; receipts
[`…-cause3-baseline.v1.json`](../../../../ratchets/h2-8a-decorator-followup-cause3-baseline.v1.json),
[`…-top-level-baseline.v1.json`](../../../../ratchets/h2-8a-decorator-followup-top-level-baseline.v1.json)):
in-function family 1 exact / 5 failed (unchanged from the first baseline);
top-level group 8 exact / 10 failed — both undecorated sources on the five
lowered configurations, with the same shape (no `__propKey`, class named `""`,
class-fields' own `_a = fieldKey`); the six decorated controls and the two
native ESNext/define rows exact.

### Cause 3 — undecorated classes' named-evaluation keys (commit `a4c089c7b`)

`visit_class_members_generic` (shared by the undecorated class declaration and
class expression routes) runs `prepare_property_named_evaluation(member,
&data, false)` inside the class-element frame before the element name
previsit: the hoist lands in the innermost environment on the visitor's stack
(`visit_function_like_body` for `make()`, declared at the block start by
`merge_block_environment`), the name is rewritten to `[_a = __propKey(fieldKey)]`
and previsited under the name frame (`partialTransformClassElement(member,
undefined)` → `visitPropertyName`; the queue is empty in an undecorated class),
and the initializer's class receives `__setFunctionName(_classThis, _a)`.
`transform_root` now starts the source-file lexical environment around the
root visit and `merge_source_file_environment` splices the hoisted `var`
statement after the leading prologue directives and any custom-prologue
hoisted function declarations (`is_prologue_directive`, `is_hoisted_function`
ported from `mergeLexicalEnvironment`'s left-side scan). The class-fields owners
consume the flagged rewritten name unchanged (cache read for the field
operation; `_a = __propKey(fieldKey)` after the class at ES2015, in a static
block at ESNext/set, untouched at ES2022/define).

Focused runs `target/decorator-followup-runs/cause3-r1/` and `cause3-toplevel-r1/`
([receipt](../../../../ratchets/h2-8a-decorator-followup-cause3-focused.v1.json),
[receipt](../../../../ratchets/h2-8a-decorator-followup-top-level-focused.v1.json)):
filter `outer-property-hoist` = `undecorated-outer-property-hoist` ×6 +
`decorated-outer-property-hoist-control` ×6, 12 exact twice; top-level group
18 exact twice; both exit 0, sources and witness inputs unchanged after the
runs.

Not changed, by design: the hoisted `var` statements carry no
`EmitFlags::CUSTOM_PROLOGUE` (tsc's `endLexicalEnvironment` marks them); the
existing function-body declarations never carried it either, and no witness
distinguishes the two, so this stays a recorded difference rather than a
speculative change.

## Candidate patches

`scripts/export-decorator-followup-candidates.sh` exports, with both endpoints
fixed by default (start `0fda49509`, end `a4c089c7b`; explicit arguments only
reproduce another range), one `.candidate.patch` per commit that changes
`crates/` or `scripts/` plus the cumulative production diff, under
`docs/design/greenfield/slices/h2-8a-decorator-followup-NN-<cause>.candidate.patch`
and `h2-8a-decorator-followup-production.cumulative.patch`. The decorator-next
exporter and its default endpoint `306930ab7` are untouched.

## Final measurements

All runs below execute the production of the last production commit
`a4c089c7b` (`standard_decorators.rs` SHA-256 `47ec5c4afcc7…`; `class_fields.rs`,
`downlevel.rs`, `execute.rs`, `printer.rs`, `jsx.rs`, `es2018.rs`,
`generated_bindings.rs`, `target_bindings.rs` unchanged since the start
manifest) through one contracts test binary built once by
`target/decorator-followup-runs/tools/final-chain.sh` (build log
`target/decorator-followup-runs/final-build/build.log`). Every receipt records
the head, the production/fixture/test SHA-256, the environment, the actual
exit, the log SHA-256, the archived binary and every capture's SHA-256, and
attests that sources and witness inputs were unchanged after the run.

| suite | run dir | cases | exact ×2 | failed | exit | binary | log | receipt |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 48 prepared commands (`source-followup`) | `final-source-followup` | 48 | 48 | 0 | 0 | `081b095f4948…` | `b5907e27cbb8…` | [`h2-8a-decorator-followup-final48.v1.json`](../../../../ratchets/h2-8a-decorator-followup-final48.v1.json) |
| 18 top-level commands (`source-followup-top-level`) | `final-top-level` | 18 | 18 | 0 | 0 | `081b095f4948…` | `0e54ad203547…` | [`h2-8a-decorator-followup-final-top-level.v1.json`](../../../../ratchets/h2-8a-decorator-followup-final-top-level.v1.json) |
| existing `transform-order` | `final-transform-order` | 60 | 60 | 0 | 0 | `081b095f4948…` | `ca937e7dc5e9…` | [`h2-8a-decorator-followup-final-transform-order.v1.json`](../../../../ratchets/h2-8a-decorator-followup-final-transform-order.v1.json) |
| existing `super-paths` | `final-super-paths` | 42 | 42 | 0 | 0 | `081b095f4948…` | `5adc64d60871…` | [`h2-8a-decorator-followup-final-super-paths.v1.json`](../../../../ratchets/h2-8a-decorator-followup-final-super-paths.v1.json) |
| existing `name-owners` | `final-name-owners` | 24 | 24 | 0 | 0 | `081b095f4948…` | `97cc7a1c239b…` | [`h2-8a-decorator-followup-final-name-owners.v1.json`](../../../../ratchets/h2-8a-decorator-followup-final-name-owners.v1.json) |
| 530 existing commands (`h2_8a_retained_accessor_owners`) | `final-full530` | 530 | 530 | 0 | 0 | `081b095f4948…` | `1a169b0c6c54…` | [`h2-8a-decorator-followup-final530.v1.json`](../../../../ratchets/h2-8a-decorator-followup-final530.v1.json) |

- The three existing groups are the previous work's 126 witnesses, unchanged
  expectations, all exact twice (60 + 42 + 24).
- full62 comparison of the 530 current captures against the previous work's
  reference captures (archive SHA-256 `48fbbfbaa153…`, manifest
  `badaa9c02d9e…`, comparison script `23507016ab00…`, all verified before the
  comparison): 530 exact twice, changed 0, missing 0, extra 0, exit 0
  ([receipt](../../../../ratchets/h2-8a-decorator-followup-final530-full62.v1.json)).
- Emitter suites `cargo test -p tsc-rs-emitter --lib --test contracts --
  --test-threads=1`: lib 495 passed, contracts 452 passed, exit 0
  ([receipt](../../../../ratchets/h2-8a-decorator-followup-final-emitter-suites.v1.json)).
- Acceptance (`cargo xtask acceptance`, the handoff's command and
  environment): see the subsection below.

Baseline → final, per family (five lowered configurations each; ESNext/define
keeps native decorators): `literal-computed-decorated-field` 1 → 6 exact,
`anonymous-in-decorated-computed-field` 1 → 6, `undecorated-outer-property-hoist`
1 → 6, top-level undecorated (plain, prologue) 2 → 12; everything else was
already exact and stayed exact.

## Claim boundary and remaining rows

- Claimed: the 48 prepared commands and the 18 top-level commands are exact
  twice on the final production, the existing 126 and 530 commands are
  unchanged (full62 comparison zero), and the emitter suites pass. That is
  the A6-41 source follow-up for the four hypotheses.
- Not claimed: every `transformESDecorators` source path, or H2.8 as a whole.
  Unwitnessed branches implemented through the same callees are named in the
  cause 1 and cause 3 sections (decorated methods/accessors with literal
  computed names; numeric/template literal keys; `CUSTOM_PROLOGUE` on hoisted
  `var` statements). They are not counted.
- Pre-existing at the start head, outside this work: the hosted `gates` run of
  PR #512's head `2953ecb8a`
  ([run 34565157958](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34565157958))
  finished after this work started and failed at H2.5h only —
  `typescript-6.0.3/compiler/blockScopedVariablesUseBeforeDef.ts#target=es5` is
  exact now and `ratchets/h2-5h-known-divergences.v1.json` must shrink. H2.5g
  and every earlier band passed there (H2.5g candidates 9027, exact 8511,
  h2_8a_deferred 6, h2_9_deferred 510). That manifest shrink belongs to the
  repairs that made the row exact (PR #512's own admission), so this branch
  does not edit it; the local acceptance run below is read against it.
