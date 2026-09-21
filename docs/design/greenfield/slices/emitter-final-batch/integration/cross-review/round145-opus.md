# Round 145: review of items 1–4

This is from reading the source plus two vendored-TS probes. No edits or builds. Note that HEAD has moved to `d0de86c0d`.

- **Items 1 and 4:** confirmed; the fixes are bounded.
- **Item 2:** confirmed, but the correct place to fix it is the **printer**, not a modifier filter in the transform.
- **Item 3:** the premise doesn't match current source. The function-name helper Root named isn't the one in use, so the cause is still open.

## 1. Missing function body (`nodeIsMissing`)

**Confirmed:**
- TS `visitFunctionDeclaration` (`_tsc.js:94974-94996`) returns a NotEmittedStatement when `!shouldEmitFunctionLikeDeclaration(node)`, which is `!nodeIsMissing(node.body)` (94760).
- `nodeIsMissing` (12910) means undefined, or `pos === end && pos >= 0 && kind !== EndOfFileToken`.
- The native arm (`builtins.rs:11586-11601`) tests only `data.body.is_none()`, so the parser's zero-width recovery Block gets through.

**Namespace coupling is already correct:**
- `visit_namespace_exported_declaration` (`builtins.rs:12803-12833`) returns early when the visited result is NotEmitted, so no `N.f = f` is added. Fixing the arm fixes the extra assignment too.
- This entry point deliberately bypasses the ContainsTypeScript gate (`visit_typescript`, force=true). That matches TS's `namespaceElementVisitorWorker`, which sends exported namespace members to `visitTypeScript` without the gate.
- So even an *untyped* `export function f(p) => p` inside a namespace must become NotEmitted. Put the guard in the arm, not at the gate.

**`record_class_or_function_declaration` is correct as it is.** TS's `onBeforeVisitNode` (94106-94116) also records missing-body functions.

**Fix:** add a local predicate, parsed body with `pos == end && pos != u32::MAX && kind != EndOfFileToken`, OR-ed with `body.is_none()` in the FunctionDeclaration arm. Keep the fast gate.

**Same predicate elsewhere (same TS helper, same visitor):**
- Constructor (11788) and Method (11803): TS returns `undefined` when `nodeIsMissing(body)`.
- Accessors (11814 and 11831): TS uses `!(nodeIsMissing(body) && abstract)`.
- FunctionExpression: TS returns `createOmittedExpression`, which native doesn't implement.

Bundle the constructor and method only if you add controls for them. Leave FunctionExpression separate, since native would need an OmittedExpression replacement.

**Controls:**
1. Typed `function f(x: number) => x` at top level.
2. The same inside `namespace N { export ... }`: no `N.f = f`.
3. Untyped exported inside a namespace: must also become NotEmitted, because of the gate bypass.
4. Untyped `function f(p) => p` at top level: still emits `function f(p) { }`, because the gate stays.
5. An overload signature, `function f(): void;` (the `body.is_none()` path is unchanged).

## 2. `@dec` leaking from variable statements

**The TS mechanism, confirmed by probe:**
- The decorators **survive every transform**. The probe counted 2 decorators both before and after the `before`/`after` custom transformers.
- They disappear in the **printer**: `emitVariableStatement` → `emitDecoratorsAndModifiers(node, modifiers, allowDecorators=false)` (`_tsc.js:118607`, function at 119846-119890).
  - When the list is all decorators and `allowDecorators` is false, it emits nothing and returns `node.pos`.
  - When decorators and modifiers are mixed, it emits only the modifier groups.
- The `allowDecorators` table I extracted from all 16 callers:
  - **true:** Parameter, PropertyDeclaration, MethodDeclaration, AccessorDeclaration, ClassDeclaration/Expression.
  - **false:** Constructor, IndexSignature, VariableStatement, FunctionDeclaration/Expression, Interface, TypeAlias, Enum, Module, ImportEquals, Import, Export.

**The native gap:** `emit_modifiers` (`printer.rs:13454`) always prints `ModifierListItemKind::Decorator` items (13491), whatever the node kind.

**Smallest correct fix:**
- Add an `allow_decorators` input to `emit_modifiers`, or a small `DecoratorPolicy` enum.
- Set it from the table above at each caller.
- When it is false, skip decorator items entirely: no `write_line`, no `record_list_element_position`, and no node emission for them. Modifier items are emitted as today.
- The return value ("wrote something") must be false for an all-decorator list, so callers don't write the trailing space.

This is the TS protocol itself. It never touches valid class, member or parameter decorators, whose policy is true. A transform-side strip would diverge from TS, because TS keeps the nodes in the tree.

**Controls:**
1. `@dec using x` (this case).
2. `@dec let x = 1;`
3. `@dec export const x = 1;` (mixed list: expect `export const`).
4. `@dec function f() {}`
5. `@dec enum E {}`

Valid neighbours that must not change:
- a class decorator, under both experimental and ES decorators;
- a method/property decorator;
- a parameter decorator with `experimentalDecorators`.

## 3. Function-name comment `clone/* <T> */(`: the premise doesn't match current source

- In current source, **both** the FunctionDeclaration arm (`printer.rs:5566-5575`) and the FunctionExpression arm (5637-5646) already emit the name through `emit_optional_ordinary_child(…, EmitHint::IdentifierName, None, …)` (14378-14428).
- That helper runs the deferred LeadingAndTrailing source-comment phases and asserts `Complete`. It is the TS equivalent of `emitIdentifierName` → `pipelineEmit(IdentifierName)` (`_tsc.js:117149-117155`, called at 118966).
- `emit_identifier_name_with_context` is used for break/continue labels, class names and specifiers, not function names. Switching helpers would therefore change nothing.

TS itself emits this comment as the name's **same-line trailing** comment, with a prefix space: `clone /* <T> */(`.

**Open hypotheses; pin them with one witness first:**
- **(h1)** At print time, `data.name` is not the parse identifier but a clone with `NO_COMMENTS` or a synthesized range, set by a pass that rebuilt the inner function inside the exported namespace function `compileUnit`.
- **(h2)** The deferred trailing phase returns `RetainedByParent`, `NoSourceRange` or `Suppressed` for this owner.

**Witness:** at 5566, record the name's node id, `get_original_node`, raw pos/end, metadata flags and comment range, plus the trailing-ownership value from `emit_deferred_expression_trailing_comments`.

**Minimal controls:**
1. `function f/* c */(a: any) {}` at top level.
2. The same nested inside `namespace M { export function g(x: string) { function f/* c */(a: any) {} } }`: the parserharness shape.

If (1) passes and (2) fails, the owner is a transform, not the printer. Don't broaden the IdentifierName behaviour.

## 4. Parser fact: new `ParseRecoveryAction` variant vs new `ParseDiagnosticOrigin`

**What both share (confirmed):**
- **Rollback:** `ParseRecovery::restore` (`recovery.rs:938-942`) truncates diagnostic origins, events and actions together, so either design discards reports from abandoned speculation.
- **AST core:** the core digest excludes `parse_recovery` (`recovery_parse_snapshot.rs:324-326`), so neither design changes the 16,994-input AST or diagnostic core snapshot.
- **Recovery facts:** both change the serialized recovery facts in `utf16_literal_recovery_census.rs:221-233`.
  - With a new origin, the event `kind` string changes (it is `Debug`-formatted).
  - With a new action, there is a new `actions` entry. The exhaustive `match` at 227-231 won't compile until it has an arm, which makes the change visible on purpose.

**Why the action variant is safer:**
- **An origin change is not monotone.**
  - Seven predicates test `event.kind == Diagnostic(Parser)`: `recovery.rs` 250, 261, 274, 289, 492, 664, 811. So does the `diagnostic_origins[index]` equality check at 219-220.
  - Re-labelling TS1260 events silently removes them from every one of those rules. Some TS1260 events may be admitted today through `report_has_retained_syntax_owner`'s flush ExpressionStatement rule. For example, an ASI `\u0074his` statement ends exactly at the token.
- **An action is monotone by construction.**
  - Events keep the `Parser` origin, so every existing rule behaves exactly as now. You add one more OR branch at `recovery.rs:253-257`: a report-only Parser event whose `[start, start+length)` equals an `EscapedKeywordConsumed` action.
  - The other action consumers skip unknown variants: `context.rs:24` and `245` and `recovery.rs:710` use let-else or `matches!`.
  - `recovery.rs:205` (`actions.is_empty()`) matters only for the narrower predicates. Those already reject any file with a report-only event (the `return false` at 285), so nothing flips.
  - In incremental reuse (`parser.rs:1399-1415`), the event at the same span already blocks reuse.
- **One real coupling: top-level-await reparse.**
  - `parser.rs:9765-9770` keeps actions by `owner_start()`, matched against the reparsed statement positions.
  - The new variant must therefore carry the same statement start that `TokenSkipped.statement_start` uses. Otherwise a retained event could lose its action (a refusal, which is safe but loses the admission), or keep a stale one.
  - Implement `owner_start` and `intersects` for it. Record it in `next_token` (`parser.rs:1033-1045`), immediately after `parse_error_at`, so they sit under the same checkpoint.
  - This is TS1260's only production site. The admission rests on "keyword token consumed while it has an escape", not on a diagnostic code.

**Expected newly admitted scope:** exactly the inputs whose only unadmitted events are committed TS1260 reports.

**Guards for the 16,994 census comparison against the retained old snapshot:**
1. The AST core and diagnostics digests are byte-identical for every input.
2. The recovery facts differ **only** by added `escaped-keyword` actions, each with exactly the span of a TS1260 event, and one per committed report.
3. Each of the six admission predicates changes only from false to true, and only on inputs that contain such an action.
4. Every newly admitted input goes through a complete-command comparison against TS. Admission is not treated as proof.

**Controls, in addition to the ones from r144:**
- top-level `\u0061wait x` in a module (the TLA reparse path);
- a speculative arrow case, `(\u0061sync x => x)`.
