## r37: comment5 ownership in the CommonJS destructuring export

**Upstream producer, from the lowered tree (tsc 6.0.3, `after` dump).** For `export let { /** … */ someMethod } = null as any;` the module transform's `flattenDestructuringAssignment(…, createAllExportExpressions)` produces `BinaryExpression[-1,-1]` (no range, `original = BindingElement[12,52]`) whose **left operand is `factory.cloneNode(name)`, an `Identifier[12,52]`**: tsc's `cloneNode` copies `pos/end`, and the parsed name's `pos` is its full start, which includes the own-line JSDoc trivia after `{`. Print-time substitution turns that identifier into `exports.someMethod` via `setTextRange(generatedAccess, node)`, so the JSDoc is emitted as the **leading comment of the exported access**, not of the assignment or statement. The `createExportExpression(…, location)` wrapper carries no range here. This is why the renamed leaf `{ /** c */ a: b }` prints no comment (the clone is `b[46,48]`, after the JSDoc) and why the defaulted leaf `{ /** c */ d = 1 }` prints it twice (`_a = o.d` gets the element range from `createDefaultValueCheck`, and `exports.d` gets the name range).

Only own-line comments count: a comment on the `{` line is a trailing comment of the erased brace and tsc drops it (my first probe used that shape and misled me for one round).

**Rust gap.** `create_export_access_from_module_name` (builtins.rs) builds the property through `factory.clone_node`, which allocates synthesized positions (`u32::MAX`), unlike tsc's `cloneNode`; the direct-export branch of `create_module_export_assignment` then applies `set_text_range(access, target)` (raw positions) but no typed `comment_range`. The printer's comment-phase owner resolves through `comment_range_for_node` first, and the CI bytes show the leading phase never fires for this synthesized access, while its source map does (maps already match: tsc's `3:0 -> 4:4` comes from `skipTrivia(name.pos)`, which the raw range also yields).

**Smallest fix.** In the direct-export branch, alongside `set_text_range(access, target)`, set the typed comment range of `access` from `target` (the parsed name identifier: full start through end), the CommonJS analogue of `set_comment_range_from` at printer-facing sites elsewhere. Do not touch source-map metadata (already exact) and do not change the non-direct branch, whose print-time substitution clone already copies the raw range into both comment and map metadata. Comment phases stay ordinary: leading at the name's full start, trailing at its end (`exports.m /* t */ = o.m` already relies on that end).

**A second divergence to expect, same producer.** Rust also `set_text_range(expression, original)` on the *wrapper* export assignments (element range), whereas tsc's wrappers have no range. With a re-export (`export let { /** c */ x } = o; export { x as y };`) tsc prints the comment once (from the inner access); Rust would print it from the wrapper as well. Include that control; if it fails, drop the wrapper `set_text_range` and keep only `set_original_node`, checking maps at the same time.

**Controls (complete commands, commonjs; ES5/ES2015/ESNext; removeComments off/on; sourceMap on).** Expected tsc bytes from probes:
1. shorthand leaf, own-line JSDoc: comment before `exports.someMethod = …` (the CI row).
2. shorthand leaf, own-line `// line` comment: same treatment.
3. same-line `{ /** c */ m }`: no comment.
4. renamed `{ /** c */ a: b }`: no comment.
5. default `{ /** c */ d = 1 }`: comment twice (`_a = o.d, \n/** c */\nexports.d = …`).
6. array `[ /** c */ x, // line\n y ]`: comment before `exports.x`, line comment before `exports.y`.
7. nested/rest `{ p: { /** c */ q }, /** c */ ...rest }`: comment before `exports.q = o.p.q, …` (the rest leaf's comment is dropped by tsc here).
8. two leaves each commented: each keeps its own.
9. `var`/`const` variants of 1.
10. non-exported `let { /** c */ z } = o`: pattern retained, comment inside the pattern.
11. re-export wrapper case above.
12. ES5 target of 1 and 5 (ES2015 destructuring lowering precedes the module pass).
13. `module: esnext` and `system` of 1 (no CommonJS flattening / System export path).
Maps must be byte-identical before and after the fix in every row; removeComments rows must be unaffected.

No native result is claimed; the fix location is the direct-export branch only, no comment-text or ID matching.
