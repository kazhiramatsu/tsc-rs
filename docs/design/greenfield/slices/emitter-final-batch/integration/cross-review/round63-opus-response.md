I found no correctness error in the per-kind overrides, and no performance or provenance problem that blocks the census. Several guards are implemented correctly but no case in the 249 actually exercises them, because the existing cases would give the same result without the guard. Concrete TS inputs for those are listed below. This is a code read only; nothing was built.

## Overrides checked against upstream factories

| Kind | Rust fields | Upstream | Verdict |
|---|---|---|---|
| Identifier | source when `escaped_text == "await"` | `createIdentifier` 21618 | ✓ |
| Type nodes (`FirstTypeNode..LastTypeNode`), TypeParameter, the signatures, Interface, TypeAlias | skipped | TypeExcludes, and own flags assigned ContainsTypeScript | ✓ |
| Enum, Module, the import/export family, ExternalModuleReference | skipped | bit cleared (23395-23679) | ✓ |
| MissingDeclaration, NamespaceExportDeclaration | skipped | factory has no children or only a stripped name; modifiers are assigned after the factory call (33723, 34229, 34421) | ✓ |
| Function/FunctionExpression/Arrow/Constructor as a child | skipped when not the root | Function/Arrow/Constructor excludes contain the bit | ✓ |
| FunctionDeclaration | no body or `declare` → nothing; otherwise modifiers, asterisk, name, type parameters, parameters, type | 23303-23318 | ✓ |
| FunctionExpression / Arrow | same fields without the body (Arrow also has `=>`) | 22676-22710 | ✓ |
| Method / Get / Set | no body → nothing; Get includes `type`, Set has none; accessor type parameters left out | 21924-22052; accessor type parameters are assigned after the factory (34002) | ✓ |
| Constructor | no body → nothing; modifiers and parameters only | 21988; type parameters and type assigned after the factory (33928) | ✓ |
| Class declaration / expression | `declare` class → nothing; modifiers, name, type parameters, heritage clauses, members | 23339 / 22927 | ✓ |
| VariableStatement | `declare` → nothing; modifiers and the declaration list | 23058-23064 | ✓ |
| Parameter | `this` → nothing; modifiers, `...`, name, `?`, initializer (type not passed up) | 21838-21847 | ✓ |
| VariableDeclaration, PropertyDeclaration, BindingElement, PropertyAssignment | name kept only when not an Identifier; initializer; property modifiers | 23274, 21890, 22428, 24138 | ✓ (tokens are harmless) |
| ShorthandPropertyAssignment | initializer only | 24160; modifiers, `?`, `!` and `=` assigned after the factory (32981-32992) | ✓ |
| PropertyAccess | expression and `?.` | 22464 | ✓ |
| QualifiedName | left only | 21804 | ✓ |
| ClassStaticBlock | body only | 21961 | ✓ |
| Everything else | `for_each_child` | propagates every child; type arguments, `as`/`satisfies` types and assertion types are removed by the type-range check | ✓ |

**Gate and run selection:**
- The gate skips a top-level FunctionDeclaration, and none of the other excluded kinds contribute anything themselves. This matches `sourceFile.transformFlags` (29342).
- A run uses the root statement's own bit together with `!AWAIT_CONTEXT` (29303).
- Class members pass up under their own member-kind exclusions: Constructor is removed by the non-root check, while Method/Get/Set/Property/StaticBlock pass up.

**Performance:** the walk is iterative, returns on the first hit, and skips function bodies. Each statement is walked at most by the gate plus the two `find_*` scans. The one inefficiency is a fresh `children` Vec per popped node; allocating one Vec outside the loop and clearing it each iteration removes that. It is not a correctness problem.

**Provenance:** the observer covers both repetitions, pins the compiler hash, and uses `--write`/`--check`. The Rust test checks the 6.0.3 version, the repetition count and the 249-case total. It does not check `observer_sha256` against `scripts/observe-await-flag-boundary.mjs`. If your other syntax suites bind the fixture to its observer, add the same assertion here; otherwise the fixture could drift from the script without the test noticing.

## Guards the current 249 cases don't exercise
In each of these cases the child is already excluded by the type-range check or by being bodyless, so removing the guard would not change any verdict. Add them as direct oracle controls in all three contexts (script, module, run):

1. **`declare` VariableStatement:** `declare let x = await;`. The existing `ambient-variable` case only has an `await` inside a type.
2. **`declare` ClassDeclaration:** `declare class C { p = await; }`. `ambient-class` only has a bodyless computed method.
3. **`declare` FunctionDeclaration with a body:** `declare function f(a = await) {}`. This only shows in the run context, because the gate excludes functions.
4. **`this` parameter:** `class C { m(@await this) {} }` and `class C { m(this = await) {} }`.
   - `this-parameter` only has a type annotation.
   - These also check how Rust represents the name. If Rust parses the `this` parameter name as something other than an Identifier with escaped text `"this"`, the guard never fires and the decorator and initializer would wrongly pass up.
5. **Bodyless accessors:** `abstract class A { abstract get [await](): number; abstract set [await](v); }`.
6. **Parameter identifier name** (no case covers it): `class C { m(await) {} }` should not reparse; `class C { m({ [await]: a }) {} }` should.
7. **Escaped identifier as a reference:** `const x = aw\u0061it;`. This one actually sources the bit, because upstream matches on escaped text. `unicode-property` only tests a stripped property name.
8. **Property forms:**
   - `class C { declare [await]: number; }`: `declare` on a property still passes up (21890 ORs it in rather than assigning).
   - `class C { accessor [await] = 1; }`.
   - `const x = source?.await;`: PropertyAccessChain name.

None of these changes the implementation. They turn the guards into observed behaviour before the five-profile census.
