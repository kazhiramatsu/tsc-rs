The heritage restriction is sound: it only removes admissions, so it cannot add unproved ones. For the await flag, the nine drifts all come from one design flaw. The walker descends into every child, while upstream only propagates the bit through specific fields, and some child kinds (functions, arrows, constructors, namespaces, type nodes) strip it on the way up. The fix is a field-aware one-bit model. A small per-node, per-field rule is enough, as long as it knows which field each child comes from; a plain `for_each_child` walk cannot express that. This is a read of `_tsc.js` and the frozen probes only; nothing was compiled.

## 1. Heritage restriction (`array.nodes.len() != 1`, context.rs:312-318)
The only change is a stricter guard, so every file admitted now was admitted before.
- **Why `extends await<A>, B` was admitted before:** every token outside the two children was a recorded skip (`await`, `<`, `>`, and the comma, which was aborted rather than consumed as a separator). So the tiling check passed.
- **What can still pass with one child:** a comma can only appear untracked if the list consumed it as a separator. With a single element, that would be a trailing comma after the element, and then `cursor(child.end) ≠ array.end`, so tiling fails and the file is refused (e.g. `extends await A, {`).
- The originals, which have one child, are unaffected.
- **Add a control:** `export {}; class C extends await<A>, {}`. Here the comma is a recorded skip, so the file is admitted. Observe TS's output for it rather than assuming either verdict.

## 2. What upstream actually computes
Upstream sets and passes up one bit, `ContainsPossibleTopLevelAwait` (1<<26):
- **Source:** `createIdentifier` with escaped text `"await"` (21618). Nothing else sets it: not the `await` keyword token, not AwaitExpression, not PrivateIdentifier.
- **Passing a child up:** `propagateChildFlags(child)` = `child.flags & ~subtreeExclusions(child.kind)` (25110). Only these exclusions contain the bit (from 25125-25190 and the constants at 3791-3806):
  - **ArrowFunction, FunctionExpression, FunctionDeclaration, Constructor, ModuleDeclaration;**
  - **every type node (kinds 183-206), the keyword types, TypeParameter, Property/Method/Call/Construct/IndexSignature, InterfaceDeclaration, TypeAliasDeclaration.**
  - It is **not** in the Method/Get/Set, Property, Class, ObjectLiteral, Call/New, VariableDeclarationList, Parameter, BindingPattern, CatchClause or default-node exclusions.
  - `PropertyNamePropagatingFlags` doesn't contain the bit, so replaying a named declaration's name flags never brings it back.
- **Two levels:**
  - **Entry gate:** `sourceFile.transformFlags`, which uses the propagated flags of each statement (29342). The existing Rust gate already excludes FunctionDeclaration here; keep it.
  - **Run selection:** a statement's **own** flags (`containsPossibleTopLevelAwait`, 29303), with no exclusion for the statement's own kind.

Per-factory deviations from "every child passes up":

| Rule | Kinds / fields (upstream line) |
|---|---|
| Own flags **assigned** as ContainsTypeScript (nothing passes up) | Interface 23367, TypeAlias 23380, TypeParameter 21830, all type nodes including TypeQuery/ImportType, signatures; **only in these cases:** a Parameter named `this` 21847, a FunctionDeclaration with no body or `declare` 23313, a ClassDeclaration with `declare` 23347, a VariableStatement with `declare` 23064, a Method/Constructor/Get/Set with no body 21936/21988/22020/22050, a Module with `declare` 23409 |
| Bit **cleared** on the whole node | Enum, Module, ImportEquals, Import, ImportClause, NamespaceImport, NamespaceExport, NamedImports, ImportSpecifier, ExportAssignment, ExportDeclaration, NamedExports, ExportSpecifier, ExternalModuleReference (23395-23679; this matches the current Rust list) |
| Body stripped | FunctionDeclaration, FunctionExpression, Arrow, Method, Constructor, Get, Set (`body & ~bit`). **ClassStaticBlock is not stripped:** `propagateChildFlags(body)`, 21961 |
| `propagateNameFlags` (an Identifier name is stripped; a computed name or binding pattern passes up) | `name` on FunctionDeclaration, FunctionExpression, Method, Get, Set, PropertyDeclaration, ClassDeclaration, ClassExpression, VariableDeclaration, Parameter and PropertyAssignment; both `propertyName` and `name` on BindingElement |
| `propagateIdentifierNameFlags` (always stripped) | PropertyAccess `name` (22464), QualifiedName `right` (21804), **ShorthandPropertyAssignment `name`** (24160), NamespaceExportDeclaration `name` |
| Name passed up **plainly** (not stripped) | Labeled/Break/Continue `label`, JsxAttribute `name`, JsxNamespacedName, MetaProperty `name` |
| Fields the parser assigns **after** the factory call (never passed up) | PropertyAssignment/ShorthandPropertyAssignment `modifiers`/`questionToken`/`exclamationToken`/`equalsToken` (32981-32992); **MissingDeclaration `modifiers`** (33723, 34229), so a MissingDeclaration contributes nothing; ClassStaticBlock `modifiers` (34055); NamespaceExportDeclaration `modifiers` (34421); Constructor `typeParameters`/`type` (33928); accessor `typeParameters` (34002) |
| Not passed up at all | the `type` field of VariableDeclaration, Parameter and PropertyDeclaration. Every such field holds a type node, so the child-kind exclusion already covers it. |

## 3. Proposed one-bit model (iterative, same stack shape as now)
```
blocked_root(n)   = assigned_ts(n) || whole_strip(n)          // the statement's own flags
blocked_child(c)  = blocked_root(c) || excl_await(c.kind)     // what passes outward
contributing(n)   = children of n minus: the stripped bodies,
                    Identifier-valued names per the two name rules,
                    and the post-assigned fields in the table above
has(root): stack=[root]; pop n:
    if n != root && blocked_child(n) → skip
    if n == root && blocked_root(n)  → skip
    if Identifier && escaped_text=="await" → true
    push contributing(n)
```
- Keep the gate: `statement.kind != FunctionDeclaration && has(statement)`. Run selection uses `!AWAIT_CONTEXT && has(statement)`.
- **Implementation shape:** add `for_each_contributing_child(node, f)`, an explicit `match` over the kinds in the table that names each field, falling back to `for_each_child` for all other kinds. For all other expression and statement kinds, upstream really does pass up every child, and the child-kind exclusions (for example type arguments, and `as`/`satisfies`/assertion types) are handled by `blocked_child`.
  - Plain `for_each_child` loses the field a child came from, so it cannot separate a name, body or modifiers field from an operand.
  - Add a unit test that fails if a NodeData variant with a `name`, `body` or `modifiers` field is missing from the explicit match.

This accounts for all 17 probes. Of the nine drifts:
- the type alias reference, the variable annotation, the property type and the generic arrow constraint are type nodes, stripped by the child-kind exclusion;
- the class type parameter is a TypeParameter child, stripped the same way;
- the type alias name is covered because TypeAlias's own flags are assigned;
- the method name and object property name are stripped by `propagateNameFlags`;
- `source.await` is stripped by `propagateIdentifierNameFlags`.

The positives stay positive: a computed name or element argument passes up, and so do the ExpressionWithTypeArguments children of `implements`/`extends` heritage clauses (kind 234, default exclusion).

## 4. Controls
For each: TS twice for each statement's `await_context` and the diagnostics, then full commands for the rows where emit changes. The comments give the expected run behaviour from the model above.

- **The nine observed drifts:** these should become non-reparsed with no diagnostics.
- **Reparse expected:**
  - `class C { [await]() {} }`
  - `class C { m(a = await) {} }`: method parameters pass up, only the body is stripped
  - `class C { p = await; }`
  - `class C { static { await; } }`: static block body not stripped
  - `@await class C {}`
  - `const { [await]: a } = o;`
  - `({ a = await } = o);`: shorthand initializer passes up
  - `await: ;`: label passes up
  - `.tsx`: `<a await={1} />`
- **No reparse expected:**
  - `const f = (a = await) => 1;`: arrow as a child is stripped
  - `class C { constructor(a = await) {} }`: ConstructorExcludes
  - `const x = { await };`: shorthand name stripped
  - `const { await: a } = o;`
  - `function f(a = await) {}` alone: gate excluded
  - `declare let x: typeof await;` and `declare function f(a?: await): void;`
  - an overload without a body: `function f(a = await): void; function f(a) {}`
  - `function f(this: await) {}`
  - `@await;`: MissingDeclaration contributes nothing
  - `class C { @await static {} }`: post-assigned modifiers
  - `import x = await.y;`
  - `export as namespace await;`
  - `namespace N { await; }`
- **Run membership:** `await; function f(a = await) {}` puts `f` in the run, because the run test uses the statement's own flags.

Then repeat the original 36 and provenance checks, the enlarged syntax and complete-command corpus, and the five-profile census. None of these rows counts as qualified until that census has run.
