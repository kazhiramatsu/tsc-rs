Your diagnosis is right, and gating the flag on the existing fact is the whole fix. The probes also confirm that the statement end map must stay absent when the class has static initializers. This was code reading plus Node probes against the vendored 6.0.3 compiler; nothing was built or edited.

## Owner
**Upstream** `visitClassDeclaration` (_tsc.js:94434-94510):
- `promoteToIIFE = languageVersion < ES2015 && (facts & MayNeedImmediatelyInvokedFunctionExpression /*7*/)`.
- It calls `emitFlags = getEmitFlags(node)` and ORs in `NoTrailingSourceMap` **only if** `facts & HasStaticInitializedProperties`, then `setEmitFlags(classDeclaration, emitFlags)`.
- It builds the `let` statement, calls `setOriginalNode(varStatement, node)`, then `setSourceMapRange(varStatement, moveRangePastDecorators(node))`.
- `transformClassMembers` returns the original `visitNodes` array unless parameter properties are added. So with no modifiers and unchanged members, `updateClassDeclaration` returns `node` itself. The static flag then lands on `node`, and `setOriginalNode` merges it into the statement, which loses its end map.

**Rust** `promote_class_declaration_to_iife` (builtins.rs:15469) adds `NO_TRAILING_SOURCE_MAP` to `updated_class` (15510-15515) **unconditionally**. `factory::update_node` returns `original` when the data and transform flags are unchanged (factory.rs:5657-5659). That happens for a member-decorated class whose members the TS pass didn't rewrite, so the flag lands on `original`. The `var` statement then merges it through `set_original_node`, keeps its start map and loses its end map, which is exactly the r71 symptom.

**Probe results (ES5, CommonJS)**:

| Input | Standard decorators | Legacy decorators |
|---|---|---|
| `class C { @dec m() {} }` (and getter / field variants) | statement end `}();` mapped | end `());` mapped |
| `class C { static x = 1; @dec m() {} }` | **no** statement end map | **no** statement end map |
| `class C { static x = 1; }` | no statement end map | no statement end map |

So the end map disappears exactly when `HasStaticInitializedProperties` is set, and `@await` plays no part.

## Smallest fix
- Pass `facts.has_static_initialized_properties` into `promote_class_declaration_to_iife` from the call at builtins.rs:11723-11729. The `decorated` parameter is unused (`let _ = decorated;`), so replace it with this flag.
- Wrap the `add_flags(EmitFlags::NO_TRAILING_SOURCE_MAP)` in `if has_static_initialized_properties`, mirroring `finish_typescript_class_declaration` (15409-15422).
- **This is sufficient.** Rust's reuse and merge behaviour already matches upstream: an unchanged class goes to `original` and the flag is inherited, while a rewritten class gets a new node (cloned with merged metadata) and the flag isn't inherited. That reproduces all six probe rows.
- **The static case keeps its behaviour:** when the class really has static initializers, it still gets the flag, and the statement inherits it through reuse, as upstream does.
- **The facts are equivalent.** `typescript_class_facts` (15427-15453) computes static initialized properties the same way as `getProperties(node, true, true)` (static + initializer, auto-accessors included), and member and constructor-parameter decorators through `childIsDecorated`. The promotion condition (static, member decorators, or a decorated class) covers mask 7.

## Controls
At ES5 and ES2015, script and module, with comments on and off:
- **Member-only decorated classes:** standard and legacy, with the decorator on a method, a getter, a setter, and a field.
- **The same classes plus `static x = 1`:** the statement end map must be absent at ES5.
- **Static initializer with no decorators:** ES5.
- **A class whose members the TS pass does rewrite:** e.g. a parameter property `constructor(public p) {}` alongside a decorated method, which forces a new class node, with and without a static initializer.

The r71 rows already cover `@await` and don't need a separate case.

## Static-block printer fix (round 72)
It remains local to one arm: delete only the `emit_modifiers` call and its trailing space in printer.rs's `ClassStaticBlockDeclaration` arm (around 5758-5767). That matches `emitClassStaticBlockDeclaration` (117915-117920). Hold both edits until the native 858 run finishes.