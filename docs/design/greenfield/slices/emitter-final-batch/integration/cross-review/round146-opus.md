# Round 146: function-name cause pinned, decorator table checked, and a gap in the parser-action plan

This is from reading the source plus two vendored-TypeScript probes and one parse-only corpus scan. No edits or builds.

## 1. Function-name comment: the cause at `640146c2a` is confirmed

- `git show 640146c2a:crates/emitter/src/printer.rs` shows that the FunctionDeclaration arm (5567-5575) and the FunctionExpression arm (5632-…) both printed the name with `emit_identifier_name_with_context`.
- That helper calls `emit_node_with_hint(…, None)`, which has no deferred source-comment phase. It debug-asserts the outcome is `None`.
- So the name's same-line trailing comment `/* <T> */` was never emitted. TS emits it through `emitIdentifierName` → `pipelineEmit(IdentifierName)` (`_tsc.js:117149`, called at 118966).
- My r145 read was of your new bytes, so the h1/h2 hypotheses are withdrawn.
- Your change at `000716e3b` routes the name through `emit_optional_ordinary_child(…, IdentifierName, None, …)`. That is the correct TS equivalent, and it touches only the two function arms.
- I also checked the other two `000716e3b` pieces:
  - The arrow change passes `expression_context.comments()` to the list start.
  - The detached-reference change scans from `resume.next()` with `collect_source_comment_ranges(…, false)`, so collection starts only after the first line break.

  Both match r144.

## 2. `DecoratorPolicy`: the 25 native callers against TS

Your Allow list (Parameter, Property, Method, Get/SetAccessor, Class) matches TS exactly. TS passes `allowDecorators=true` only for emitParameter, PropertyDeclaration, MethodDeclaration, AccessorDeclaration and ClassDeclarationOrExpression. Constructor is **false** in TS (117922), so Omit is correct for it.

How the other callers compare with upstream:

- **Same as TS (`allowDecorators=false`):** ImportDeclaration, ExportDeclaration, VariableStatement, FunctionDeclaration, FunctionExpression, Constructor, IndexSignature, Interface, TypeAlias, Enum, Module, ImportEquals.
- **Use `emitModifierList`, which applies no decorator policy:** ArrowFunction, TypeParameter, PropertySignature, MethodSignature, ConstructorType. The parser never puts a decorator in those lists (it parses their modifiers with decorators disallowed), so Omit and Allow give identical output. Omit is fine.
- **Emit no modifiers at all in TS:** ExportAssignment and NamespaceExportDeclaration start at `emitTokenWithComment(ExportKeyword…)`. Omit covers decorators. The remaining difference, that native prints non-decorator modifiers there, is separate and needs its own witness before anyone touches it.

**Scope, comments and source maps:**
- Filtering decorators out before indexing, spacing and `record_list_element_position` matches TS, because TS never emits the omitted decorator nodes. Their leading comments and source-map segments disappear too.
- The statement's own leading comments still come from the statement's comment phase, which runs before modifiers.
- In the mixed case, TS gives the modifiers group a `pos = -1` range when a decorator group precedes it (119874-119876). The `Modifiers` list format includes `NoInterveningComments`, so that range has no comment or map effect. Each retained modifier keeps its own ordinary pipeline.
- An all-decorator list under Omit must report "nothing written", so the caller writes no space. That matches TS returning `node.pos` without writing.
- Reverting the VariableStatement filter in the transform (`builtins.rs:11696-11724` at `000716e3b`) is right: TS keeps the decorators in the tree.

## 3. Parser action: you're right, and "skip inside `supports_array_gaps`" is still not enough

**Your hole is confirmed.**
- The early gate (`recovery.rs:203-210`) calls `supports_array_gaps(&self.actions)` for every profile except the final context profile.
- `supports_array_gaps` (447-637) does `let TokenSkipped{site: ListAbort, ..} = *action else { return false; }`.
- So the statement-gap profile, which admits report-only TS1260 events through `report_has_retained_syntax_owner`, could go from true to false. My r145 claim that only the narrow predicates were affected, and that they already failed, was wrong for that profile.

**Skipping the new variant inside the loop still flips.**
- The function ends with `!skip_spans.is_empty()` (637).
- With old actions `[]`, the gate passed via `actions.is_empty()`. With new actions `[EscapedKeywordConsumed]`, a "skip" loop leaves `skip_spans` empty and returns false.

**Minimal transparent option: partition once before the gate.**
- Add an exhaustive `ParseRecoveryAction::is_structural()`: `TokenSkipped | Reparsed => true`, `EscapedKeywordConsumed => false`. A future variant then has to be classified explicitly, which is not a code allowlist.
- In `supports_missing_nodes`, compute `structural` (the actions minus the new variant) and write the gate as `allow_context_recovery || structural.is_empty() || allow_parameter_gaps && supports_array_gaps(…, &structural)`.
- Old actions are exactly the new structural actions, so every old predicate gets byte-identical inputs.
- Add the new admission branch in the `Some(index)` arm of the report-only disjunction (253-257), only when `allow_context_recovery`, matching exact event start+length. The statement-gap profile then keeps its old decisions exactly.

**Other consumers, rechecked:**

| Consumer | Behaviour with the new variant |
|---|---|
| `recovery.rs:254`, `276`, `710` | use `matches!(TokenSkipped…)`, so unaffected |
| `context.rs:24`, `245` (context solver) | let-else `continue`, so unaffected |
| `parser.rs:1414` (incremental reuse) | the same-span event already blocks reuse |
| `parser.rs:9770` (TLA reparse) | keeps actions by `owner_start()`, so `statement_start` must be the same key `TokenSkipped` uses, or the event survives without its action |

**`actions.is_empty()` in parameter-only and missing-node profiles:** with the partition, those profiles see identical structural lists. They reject report-only events anyway (285), so nothing changes.

**Your replay plan is right as stated:**
- core, events and diagnostic origins exact;
- the actions diff is only the new `escaped-keyword-consumed` facts;
- zero true→false changes across all six predicates;
- a complete command for every newly admitted input.

## 4. Missing-body function-like forms: TS probe and corpus witnesses

**TS output on the vendored build, target ES2015:**

| Input | TS output |
|---|---|
| typed `var f = function (x: number) => x;` | `var f = ;` then `x;` (OmittedExpression) |
| untyped `var f = function (x) => x;` | `var f = function (x) { };` (the gate bypasses the visitor) |
| `class C { m(x: number) => x; }` | `class C {\n}` (method dropped) |
| `class C { constructor(x: number) => 1; }` | `class C {\n}` (constructor dropped) |
| `class C { get g(): number => 1; }` | `get g() { }` (kept, because it isn't abstract) |
| `var o = { m(x: number) => x };` | `var o = {};` then `x;` and `;` |

**Corpus scan:** I parsed every `ts-tests/tests/cases/**/*.ts(x)` file whole, without splitting `@filename` units, looking for zero-width bodies.
- **FunctionExpression:** 0 hits, so no witness and no follow-up.
- **Constructor:** 0 hits, so no follow-up.
- **FunctionDeclaration:** 21 hits (already fixed; `dottedModuleName` is among them).
- **Get/SetAccessor:** 1 each, both in `objectLiteralShorthandPropertiesErrorFromNotUsingIdentifier.ts:7-8`. TS keeps non-abstract accessors with an empty body, and native already does too. Worth confirming with a command run, but no code change is indicated.
- **MethodDeclaration:** 13 hits. These are the smallest original-input witnesses:
  1. `compiler/parseErrorIncorrectReturnToken.ts:12`: `m(n: number) => string { … }` in a class.
  2. `conformance/parser/ecmascript2018/asyncGenerators/parser.asyncGenerators.classMethods.es2018.ts:140, 146, 151`.
  3. `…/parser.asyncGenerators.objectLiteralMethods.es2018.ts:136, 142`: object-literal methods, where TS's `visitMethodDeclaration` also returns undefined.
  4. `compiler/overloadConsecutiveness.ts:10-11`, `conformance/types/objectTypeLiteral/callSignatures/callSignaturesWithParameterInitializers2.ts:22`, `…/methodSignatures/objectTypesWithOptionalProperties2.ts:14, 22`, `compiler/constructorWithIncompleteTypeAnnotation.ts:32`.

**Recommendation:** extend the same predicate to the MethodDeclaration arm (`builtins.rs:~11803`), which must return `None` as TS does, **only if** at least one of these witnesses is admitted by native today and its complete command differs.
- The method hits whose native status I found in `cross-review/r124-full-universe-results.json` are only the two async-generator files.
- Check their row status and the complete-command diff first. If none is admitted, record the method case as not reachable in the corpus rather than changing code.
