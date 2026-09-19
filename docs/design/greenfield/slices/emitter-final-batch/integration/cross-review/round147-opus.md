# Round 147: A is one checker line, B has a bounded parser-fact admission, and C can ride with controls

This is from reading the source at `000716e3b` plus two vendored-TS probes. No edits or builds.

## A. Missing-body declarations: d.ts gets `void` where TS gives `any`

**Confirmed owner.** In TS, `getReturnTypeOfSignature` (`_tsc.js:59815`) is annotation `||` `(nodeIsMissing(declaration.body) ? anyType : getReturnTypeFromBody(declaration))`.
- Native `annotate.rs:~10776` matches only `body_of(...)`: `None` gives `any`, `Some(_)` goes to `get_return_type_from_body`.
- A parsed zero-width Block is `Some`, so the body path runs, finds no `return`, and infers `void`.
- The comment above that match already quotes the TS tail. The code simply doesn't implement it.

**Minimal fix:**
```rust
let source = state.binder.source_of_node(declaration);
if node_util::node_is_missing(source, node_util::body_of(source, declaration)) {
    Ok(state.tables.intrinsics.any)
} else {
    state.get_return_type_from_body(declaration, CheckMode::NORMAL)
}
```
- `node_util::node_is_missing` (`binder/src/node_util.rs:926`) already treats `None` as missing, so the new branch includes the old `None => any` arm.
- Annotations, `annotation` (the FunctionType/CallSignature case), composites, instantiation and caching are unchanged.

**Synthetic bodies are safe.**
- The checker only sees parse-arena nodes. The native `node_is_missing` omits TS's `pos >= 0` term, but that term only matters for synthesized nodes, which never reach this path.
- Synthetic signatures take the `declaration == None` arm first.

**Coupled sites I checked:**
- **TS7010** (`checkFunctionOrMethodDeclarationDiagnostics`, `_tsc.js:82933`) is already ported with `body.is_none() || node_is_missing(...)` (`functions.rs:2717-2719`). Nothing to change.
- **noCheck / syntactic builder:** TS `typeFromSingleReturnExpression` (134409) skips missing bodies. The native version (`syntactic_type_node_builder.rs:3063-3080`) doesn't test for missing, but an empty zero-width Block gives no single return expression, so the result is also `failed`. It then falls back to checker inference, which the fix corrects. No syntactic special case is needed.
- **A separate observed divergence, not needed for A:** that native code *rejects* synthesized (`u32::MAX`) bodies, whereas TS's `nodeIsMissing` treats a synthesized body as present.

**TS facts for focused tests (strict, declaration):**

| Input | TS diagnostics | TS d.ts |
|---|---|---|
| `export function f(x: number) => x; export const v: string = f(1);` | 7010, 1144, 2304 (**no 2322**) | `f(x: number): any` |
| the same with `noCheck` | 1144 | `f(x: number): any`, `v: any` |
| generic `g<T>(x: T) => x` | — | `g<T>(x: T): any` |
| method `m(x: number) => x;` | 7010, 1144, 7008 | `m(x: number): any` and recovered `x: any` |

**Tests beyond the full 352:**
- A checker unit test for each row above. The missing 2322 is the meaningful case: today native likely reports a false 2322, because `void` isn't assignable to `string`.
- The 2xxx diagnostic band over the 21 original FunctionDeclaration corpus hits (`compiler/dottedModuleName.ts`, `reservedWords3.ts`, `overloadConsecutiveness.ts`, …), requiring exact diagnostics.
- Negatives that must not change: `function f() {}` (a real empty body still gives `void`), an overload list, and an expression-bodied arrow.

## B. Decorator-`using` fixture with semicolons: bounded, with a parser-fact admission

**The upstream tree is identical in both spellings (probe).**
- `@dec using 1` produces a VariableStatement with an empty declaration list, then `ExpressionStatement(1)`, then `@dec using x`.
- There is one retained diagnostic, 1134 ("Variable declaration expected"), at the `1` token.
- TS output is the same in both spellings: `using ;`, `1;`, `using x;`.

**Native events.** This is inferred from the code, so confirm with a one-shot event dump: kind, start, length, `diagnostic_index`, `full_start`.
1. **Parser 1134**, span exactly the `1` token, with a diagnostic index.
2. **A suppressed ';'-expected report** at the same start with no index. TS deduplicates same-start errors, so this is the second "recovery event".

**Why only the semicolon version is refused.**
- Both events are report-only. The index-less one passes through the `None` branch (`recovery.rs:259-263`) only if the indexed one is admitted.
- The indexed one's only fitting owner is the ExpressionStatement rule in `report_has_retained_syntax_owner` (825-842). That rule requires `node.end == event_end` and `expression.end == node.end`: an **ASI-only** statement.
- Without semicolons, `1` spans [50,52], which equals the statement, so it's admitted. With `1;`, the expression ends at 49 and the statement at 50, so it fails, and `report_has_declaration_list_boundary` (TypeAssertion-specific) doesn't fit either.

**Minimal admission (a tree fact, not a diagnostic code).** In that same statements filter, also accept `expression.end == end` where the source between `end` and `node.end` is exactly trivia plus one `;` that the ExpressionStatement consumed.
- The token is still retained as that statement's whole expression.
- The only extra consumed token is the statement's own terminator.
- Keep `statements == 1` uniqueness and every other condition.

**Controls:**
1. The fixture in all 8 configs.
2. `using 1;` alone.
3. `var 1;` and `let 1;`, which may reach the same rule (let the census show it).
4. `using x, 1;`
5. `@dec using 1 ;` (a space before `;`).
6. `1 /*c*/ ;` (a comment before `;`).
7. A negative, where a second token sits between the expression and `;`, must stay refused.

Let the successor proof enumerate the newly admitted inputs; don't rewrite the fixture.

## C. A shared `nodeIsMissing` function-like predicate: exact TS semantics by arm

All of these live in `TypeScriptVisitor` (`builtins.rs`):

| Arm (native line) | TS behaviour | Native action |
|---|---|---|
| FunctionDeclaration (11586) | NotEmittedStatement | done at `000716e3b` |
| FunctionExpression (~11605) | `createOmittedExpression()` (94997-94999) | missing → `self.create_omitted_expression()`, which TypeScriptVisitor already has at 14524 |
| Constructor (~11788) | `undefined` (94794-94797) | `None` when missing |
| MethodDeclaration, class and object literal (~11803) | `undefined` (94915) | `None` when missing |
| Get/SetAccessor (~11814, ~11831) | remove only if `nodeIsMissing(body) && abstract` (94935-94937); otherwise keep, with the missing block printing as `{ }` | today's test is `body.is_none() && abstract` |

**Predicate:** one local `fn body_is_missing(&self, body: Option<NodeId>) -> Result<bool>`, meaning `None`, or parsed with `pos == end && pos != u32::MAX && kind != EndOfFileToken`. Reuse the FunctionDeclaration code from `000716e3b`.

**Gate differences to control:**
- TS reaches class members through `classElementVisitor`.
  - `visitMethodDeclaration` and the accessors re-check ContainsTypeScript themselves, so untyped members are kept.
  - `visitConstructor` has **no** flag check, so even an untyped `constructor(x) => 1` is removed.
- Verify that native reaches the Constructor arm for an untyped class the same way. One control answers it.
- **Probe facts:**
  - typed missing function expression → `var f = ;` then `x;` (the printer must print OmittedExpression as nothing);
  - untyped → `var f = function (x) { };`;
  - object-literal method → `var o = {};`.

**Should it ride this repair?** Yes, with exact controls. It is the same TS helper in the same visitor, and it only fires on zero-width parsed bodies, which exist only in admitted recovery inputs. There are 13 MethodDeclaration corpus witnesses, pending your replay. Constructor and FunctionExpression have no corpus hits, but a proven-wrong boundary in the same arm set is not speculative.

**Controls:**
1. Typed and untyped function expression.
2. A class method, and an object-literal method.
3. Typed and untyped constructor.
4. Abstract and non-abstract getter and setter.
5. A real empty body (unchanged).

## Holes to guard in the successor parser proof

1. **Every new action matches a committed TS1260 report**, one-to-one by exact span. That rules out a spurious action being passed off as "only new facts". Also assert no action appears in an input without such an event, including after top-level-await reparse retention.
2. **The five non-context predicates must be byte-identical, not merely "no true→false".** The partition promises identical inputs to them, so assert exact equality. Only the context predicate may go false→true.
3. **Event/action consistency after TLA reparse:** every retained action's `owner_start` must have a retained event with the same span.
4. **Keep the B rule separate from the parser action.** It is a recovery-rule change with no parser facts, so the proof should report its newly admitted inputs as a separate class and run complete commands for both classes.
