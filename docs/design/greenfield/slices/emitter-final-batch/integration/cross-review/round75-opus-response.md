Both clusters have concrete owners, and the legacy-union cluster is actually two bugs: a semantic one on distinct unions and a map-range one on repeated unions. This is from the decoded r71 differences, the frozen fixture outputs, and the pinned 6.0.3 source; nothing was built.

## B. Legacy union metadata (16 rows)

**B1. Distinct union (8 rows) prints the wrong value.**
- **Evidence:** for `@dec p: Missing.A | Missing.B`, TS prints `__metadata("design:type", Object)` (53 characters at ES5). Native has mappings at columns 66-100 on that line, so its line is longer: it prints the first constituent's `typeof (_a = …) === "function" ? _a : Object` form. The map comparison fails first, but the JS differs too.
- **Upstream:** `serializeUnionOrIntersectionConstituents` compares each constituent to the first with `equateSerializedTypeNodes`. That is a structural comparison over generated identifier (any two temps compare equal), identifier text, property access (both sides), `void 0`, string literal, `typeof`, parentheses, conditional (all three parts) and binary (operator plus both sides). `Missing.A` and `Missing.B` differ in the property name, so the result is `Object`.
- **Rust:** `serialize_type_constituents` (legacy_decorators.rs ~2395-2445) compares `serialized_type_key` strings (2479-2509). Those only cover identifiers, `void 0` and property access; every conditional fallback becomes `"other"`, so any two unknown-type constituents compare equal.
- **Fix:** replace the key comparison with a direct port of `equateSerializedTypeNodes` that recurses over the serialized nodes and treats generated identifiers as equal to each other. Keep the "`Object` identifier returns immediately" rule.
- **Temps:** keep serializing every constituent up to the mismatch, as upstream does. Upstream allocates a temp for each fallback even when the result collapses to `Object` (TS at ES2015: `var _a, _b, x, _c, v, C;`). The conditional-branch shortcut at 2536-2546 is a separate upstream rule; leave it.

**B2. Repeated union (8 rows) is missing the name's mappings.**
- **Evidence** (ES2015, line 22): TS has two extra segments, `(97→1:87)` and `(98→1:88)`. These are the start and end of `A` in `Missing.A`. The `Missing` mappings are identical.
- **Upstream:** `serializeEntityNameAsExpressionFallback` and `serializeQualifiedNameAsExpression` build `createPropertyAccessExpression(left, node.right)`. They reuse the parsed right-hand identifier, which carries its range.
- **Rust:** `checked_entity_name_parts` (~2645-2668, both the identifier-left and temp arms) and `entity_name_expression` (~2703) call `create_property_access(…, &right_text)`. That creates a fresh, unranged name.
- **Fix:** pass `clone_node(right)` plus `set_text_range(clone, right)` as the name. That reproduces the reused node's mappings without sharing one arena node between two parents; a variant of `create_property_access` that takes a name node will do.
- **Leave alone:** the existing `set_original_and_range(expression, node)` on the access in `entity_name_expression` is outside this failure.

**Controls:** all at ES5, ES2015 and ES2022, legacy decorators with `emitDecoratorMetadata`, comments on and off:
- **Qualified names:** distinct and repeated unions of unknown qualified names.
- **Mixed shapes:** unknown `A | A` identifiers, three-part names `M.N.A | M.N.A` (the temp arm), mixed `Missing.A | string`, and intersections.
- **Accessor pair:** the same type on a getter/setter pair, where one type node is serialized twice.
- **Existing rows:** the `before`/`after` System temp-order rows already cover how the dead `_b` temp interacts with the naming order.

## A. CommonJS `export-import-assignment` (4 rows): double publication
- **Evidence:** for `declare namespace N {…} export import a = N; a = N;`, TS line 4 is `exports.a = N;`. Native's line is 12 characters longer, which is almost certainly `exports.a = exports.a = N;`.
- **Upstream:** CommonJS `getExports(a)` resolves the import declaration first and returns `exportedBindings[importEquals]`. The TS transform lowers the alias to `export var a = N`, whose declaration name is the parsed, non-local name. `collectExportedVariableInfo` never records such names, so the entry is empty and nothing is published. The reference itself becomes `exports.a` through identifier substitution.
- **Rust:** the round-67 import-first branch in `export_assignment_plan` (builtins.rs ~8505-8521) returns `exported_bindings[import]` with `direct_export_storage: false`. But Rust's `exported_bindings` is a superset that also records a directly exported variable's own name (builtins.rs ~3929), so it returns `[a]` and wraps the assignment a second time.
- **Fix:** in that branch, compute `direct_export_storage` exactly as the value-declaration path does: `direct_exported_variable_names.contains(local)` and the exports include `local`. The existing filter then drops the own name.
  - A plain `import { a } …; export { a };` still publishes, because import specifiers are never in `direct_exported_variable_names`. That keeps the round-66 CommonJS controls passing.
- **Controls:**
  - internal `export import a = N; a = N; a++;`;
  - external `export import a = require("./dep"); export { a as b }; a = x;`, which should publish only `b` on top of the substituted target;
  - the named/namespace/default re-export update rows at ES5 and ES2015.