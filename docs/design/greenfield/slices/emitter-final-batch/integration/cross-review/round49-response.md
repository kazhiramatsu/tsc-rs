Findings are complete; writing the review.

**Clarification on round 48 item 1.** No source-level regression independent of the workflow: the accessor rename is internally consistent, the G1 predicate is preserved under `has_only_parameter_gap_emit_recovery`, and the gate switch is exactly what the candidate must exercise for native validation. Withdrawn as a defect; it stays a merge-time check only.

**Round 49: System creation-order hoisting. The design is implemented faithfully; two concrete deviations from upstream, one pre-existing gap, and one control note.**

**1. Regression: exports appended for uninitialized declarations.** `transform_hoisted_variable_statement` now does `trailing_exports.extend(append_variable_declaration_exports(...))` in the `else` branch for declarations without an initializer. Upstream `appendExportsOfVariableStatement` (_tsc.js:112764-112774) only appends when `decl.initializer || exportSelf`, and the System visitor passes `exportSelf = false`. Probe at ES2020/System:

```
export let x; export { x as y };
export var {w}: any; export { w as v };
→ var x, w;   execute body empty (no exports_1("y", x), no exports_1("v", w))
```

The old branch (`continue`) matched upstream. Smallest fix: keep the `hoist_binding_name` call and delete the `trailing_exports.extend` line in that branch.

**2. Deviation: BigInt counted as a simple default.** `flatten_system_binding_element` treats `BigIntLiteral` as inlineable when deciding whether a nested-pattern default needs a temp. Upstream `isSimpleCopiableExpression` (93027-93029) is string-literal-like, NumericLiteral, keyword, or Identifier; BigInt is not included. Probe:

```
export let {a: {b} = 1n, c: {d} = 1} = get();
→ _c = _b === void 0 ? 1n : _b, b = _c.b        (temp for 1n)
   d = (_d === void 0 ? 1 : _d).d               (no temp for 1)
```

Fix: remove `BigIntLiteral` from the match. The keyword set can stay as is; the only keywords that reach a default value are the four listed plus `super`, which is invalid there.

**3. Verified faithful.** Root temp reuse is `numElements != 0` for object and array patterns (93520-93525 and the array analogue), and since System only flattens declaration bindings this equals upstream's `!isDeclarationBindingElement(parent) || numElements !== 0`. Locations: root temp gets the VariableDeclaration (`flattenDestructuringAssignment` keeps `location = node` for declarations, 93251-93300), element temps get the BindingElement, computed keys get the ComputedPropertyName with `reuse = false` (93630-93649). Default temps are created after the visited initializer, matching upstream order. `push_hoisted_name` no longer dedupes, matching `hoistVariableDeclaration` (116104-116112); `var x, x;` and the double `a` for `for (var {a} of …)` now come out right. For heads: uninitialized identifiers push the raw name and hoist once, patterns flatten against `void 0` then hoist again (112932-112948). Class hoist precedes member transformation; import hoists happen after clause elision; the helper import stays the first push; import-equals hoists its own name, matching `getLocalNameForExternalImport`. The binding plan is one ordered step list, so evaluations and bindings interleave as upstream (`_a = get(), _b = _a.a, b = …, _c = _a.c, d = …`). The `default` skip removal is safe: `export default 1 + 1` and `export default function () {}` produce no hoisted var upstream, and anonymous default classes hoist `default_1`. Comments: the temp assignment's element range yields `/** a */ _a = get().a` and the statement keeps the VariableStatement range, as probed.

**4. Cross-pass note for the computed/rest control.** `export let {a = get(), b = 2, [get()]: c = 3, ...rest} = get()` gives `var _a, _b, _c, _d, _e, a, _f, b, c, rest;` because the ES2018 object-rest pass materializes its own `var _a, _b, _c;` statement first, and System hoists those through the uninitialized-declaration path. That row therefore also depends on the Rust ES2018 pass emitting its hoists as a leading `var` statement in the file rather than into System's list; if it fails, look there before the System code.

**5. Pre-existing gap, not introduced here.** With `downlevelIteration` at an ES2015+ target, upstream's array flatten routes through `__read` (`_a = __read(source, 2), a = _a[0]`); the System array path has no equivalent. Worth one control so it is recorded as known rather than discovered later.
