TS confirms the ES2015 selection, and shows two further controls that matter: two constructors, and a quirk in the fields case.

# Round 155: untyped missing constructor body at ES5

## Owner, confirmed

- **TS:** `addConstructor` (`_tsc.js:105267`) calls `getFirstConstructorWithBody(node)`, which is `find(members, m => isConstructorDeclaration(m) && nodeIsPresent(m.body))` (16674-16676).
  - The parsed zero-width recovery Block counts as **missing**, so TS finds no constructor and synthesizes a default `function C() {}`.
  - `addClassMembers` skips Constructor members, so the malformed one disappears.
- **Native:** `Es2015Transformer::get_first_constructor_with_body` (`es2015.rs:8902-8919` at `3e28cb213`) returns the first Constructor with `data.body.is_some()`. It selects the malformed constructor, and `transform_constructor_parameters` then keeps `x`.
  - It has a **single caller** (`es2015.rs:8845`, `addConstructor`).
- **Why only ES5/System, untyped, fail:** the class has no ContainsTypeScript, so the TypeScriptVisitor gate keeps the constructor as TS does. The typed variants were already removed by `function_body_is_missing`, and modern targets never run ES2015 class lowering.

## Minimal fix

In `get_first_constructor_with_body`, replace `data.body.is_some()` with a `nodeIsPresent` projection. The body must exist and must not be (`pos == end && pos != u32::MAX && kind != EndOfFileToken`).
- Synthesized bodies (`u32::MAX`) stay **present**, as they are in TS.
- Use the same semantics as `TypeScriptVisitor::function_body_is_missing`. A small shared free function in `builtins` avoids a third copy, but inlining it is equally correct.
- Don't change the TypeScript-transform gate or any other transform.

## Other native `body.is_some()` constructor checks: leave them without evidence

- **TS's other `getFirstConstructorWithBody` callers:** `classOrConstructorParameterIsDecorated`, `getAllDecoratorsOfClass`, `transformClassMembers` (parameter properties), `shouldAddParamTypesMetadata`, `serializeParameterTypesOfNode`, `transformClassLike` (ES decorators), and declaration `transformTopLevelDeclaration`.
- **The native counterparts are separate sites:** `legacy_decorators.rs:2846/2865/2928/2962`, `builtins.rs:15931`, and `declarations/statements.rs:1008`.
- **Why they aren't on the failing path:**
  - every one of them is reached only by typed, decorated or declaration inputs;
  - for typed and decorated classes, the TypeScriptVisitor Constructor arm already removes a missing-body constructor, except for the classFacts computed *before* removal (`classOrConstructorParameterIsDecorated`) and declaration emit.
- **Don't bundle them.** Add two probe controls below (d.ts parameter property, decorated parameter). Fix a site only if its complete command differs.

## Controls (ES5 and System, removeComments false and true; TS expectations from the vendored probe)

| Input | TS ES5 output | What it pins |
|---|---|---|
| `class C { constructor(x) => 1; }` | `function C() {}` | the failing case |
| `class C { constructor(x) {} }` | `function C(x) {}` | a real empty body is unchanged |
| `class B {} class C extends B { constructor(x) => 1; }` | `function C() { return _super !== null && _super.apply(this, arguments) \|\| this; }` | the synthesized derived default |
| `class C { constructor(x) => 1; constructor(y) { this.y = y; } }` | `function C(y) { this.y = y; }` | TS **skips** the missing one and takes the *next* present constructor; native `is_some()` picks the first |
| `class C { y = 1; constructor(x) => 1; }` | `function C() {}` (**no** `this.y = 1`) | see below |
| `class C { constructor(public x: number) => 1; }` | `function C() {}` | typed path, already removed; also compare the d.ts, which reaches declaration `first_constructor_with_body` |

**The fields case is a real TS quirk.** Class-fields lowering rebuilds the constructor body with `setTextRange(createBlock(...), constructor.body)`, so the new Block inherits the **zero-width parsed range**. ES2015's `nodeIsPresent` then rejects it and synthesizes a bare default, and the initializer is lost.

Native matches only if its class-fields constructor rebuild also copies the original body's raw range onto the new Block. If native creates that Block synthesized (`u32::MAX`), the range-based predicate calls it present and keeps `this.y = 1`. The complete command will tell you which applies. If it differs, the owner is native class-fields body range propagation, not the new predicate.

**A decorated parameter** (`experimentalDecorators`, e.g. `constructor(@dec x) => 1`) also exercises the pre-removal classFacts site. Add it as a probe only.

## Unicode follow-up (r154): reuse is the right choice

Rebuild `detached_leading_trivia` from `collect_source_comment_ranges(…, false)` plus `contains_two_line_breaks`, with the old `emitted_end` rule (the end of the whitespace-like run after the last detached comment).
- That removes a second, ASCII-only copy of trivia semantics.
- It makes `PinnedOnly` and `All` share line semantics.
- ASCII inputs stay byte-identical.

Leave the ordinary-leading path untouched unless its complete-command probe fails.
