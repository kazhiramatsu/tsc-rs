I found one real defect this round. It was not introduced by the diff, but the new postfix controls will expose it. The two round-54 corrections themselves match upstream. Like last round, this is a code read only: no Cargo, no edits.

## `should_hoist_declaration_list` (system.rs:916-923)

This is correct. It matches `shouldHoistVariableDeclarationList` (_tsc.js:112695-112697): NoHoisting is read from the current node, `BlockScoped` from `getOriginalNode`. The change affects every ES5 `var` list that was created by an earlier pass. Some of those have no original upstream, so they must still hoist. The controls need to cover both kinds (listed below).

## `exports_for_identifier` (system.rs:2742-2805) compared with `getExports`

The logic matches `getExports` (113318-113330) and `getReferencedDeclaration` (113336-113348) in every category you listed:

| Case | Upstream result | Diff |
|---|---|---|
| Direct `export let x`, then `x = 2` | `[x]` (container name; `exportedBindings` has no own name for non-LocalName declarations) | `[x]`: the duplicate from Rust's module info is removed, then `x` is put first ✓ |
| Alias only (`let x; export {x as y}`) | container `None` → `[y]` | `[y]` ✓ |
| Direct + alias, in either order | `[x, y]` | `[x, y]`. Moving `x` first is what fixes alias-before-direct ✓ |
| Merged declarations (`var x; export var x = 1`) | picks the declaration that has bindings, or else `valueDeclaration`; same name text either way | Same: `value_declaration` is re-selected in the loop ✓ |
| Namespace / enum member | container is a Module/Enum declaration, not the SourceFile, so no own name | The new `SourceFile` kind check. The old `.is_some()` would have taken these ✓ |
| Exported function/class/enum/namespace, including defaults | Reference mode returns `None` (`ExportHasLocal` && !Variable, 87880-87883), so only `exportedBindings` | Unchanged; the added arms for Class/Enum/Function are effectively unreachable ✓ |
| Generated names and `LOCAL_NAME` | handled before this code | The early returns at 2704-2731 are unchanged ✓ |

**Metadata / printing.** `getDeclarationName` → `getName` (24788-24797) returns `setParent(setTextRange(cloneNode(name), name), name.parent)`. That clone has a position and a parent, so `getTextOfNode` returns the raw source spelling. Rust's `ExistingNode(parsed name)` gets the same raw spelling through printer.rs:3313-3342 (same source, parent present). `create_module_export_name_literal` produces an unranged literal whose only link is the text source. The comment range still comes from `value` (system.rs:3216-3231), as before. So the change adds no source-map or comment donor.

The one visible metadata effect: for direct exports, the text source used to be the post-lowering `leaf.name`; it is now the parsed name. That only shows for escaped identifiers, and the new behaviour is the one upstream uses. The escaped-identifier control below checks it.

**Minor items, no action needed:**
- The container kind is read through the transform arena, `self.node(container.node())`. The checker already restricts this to the same root, so reading the kind from the resolver side would be the safer lookup. It is not a correctness problem.
- There is no `ImportEqualsDeclaration` arm. Upstream would publish `export import x = …; x = 1`, but assigning to an import is a compile error anyway.

**Removing the `exports_by_local` fallback is correct.** Beyond `using`, the only case I can find where the result changes is an ambient export: `export declare let x: any; x = 1;`. The TS transform removes that statement, so Rust's module info never had `x`, and the old fallback returned nothing. Upstream reaches the SourceFile container with the ambient `valueDeclaration` and emits `exports_1("x", x = 1)`. The new code does the same. This is the decisive control for dropping the fallback.

## Real defect: postfix update used as a value (system.rs:2587-2599)

This was already there in a0; the diff does not touch it. Upstream `visitPrefixOrPostfixUnaryExpression` (113142-113171) builds `(_a = x++, x)`, wraps it in each export call, and only then adds `, _a`:

```
upstream: (exports_1("y", exports_1("x", (_a = x++, x))), _a)
Rust:     (_a = x++, exports_1("x", x), _a)       // plus the y wrap on `x` only
```

The Rust branch also builds the current value with `create_identifier(&operand_text)`, a spelling-based name, where upstream uses `cloneNode(node.operand)`.

**Fix:** build the result the same way as the discarded branch (2572-2585):
1. `assign = create_assignment(temp, update)` with `set_text_range(assign, original)`.
2. `comma = create_binary(assign, Comma, clone_node(original_operand))` with `set_text_range(comma, original)`.
3. Wrap `comma` in each export call, in order.
4. Finish with `create_binary(wrapped, Comma, temp)`, text range `original`.

Check whether the discarded branch's extra `create_parenthesized` step (the 20483 parenthesizer) also applies here once the result is an argument. The printer decides that parenthesization, so compare it against the observation rather than adding it by hand.

## Missing decisive controls

Add these to the 206 set (System ES5, ES2015 and ESNext; remove false/true):

1. `export declare let x: any; x = 1; x++;` — the removed fallback
2. `export let \u0061 = 1; \u0061 = 2; export { \u0061 as b };` — raw spelling of the name used as the export key
3. `export let x = 0; const v = x++;`, plus the same with `export { x as y }` before and after — the postfix defect above; your alias × postfix rows only catch it if the value is used
4. `export let x = 0; x += 1; [x] = [2];` — compound and destructuring assignments go through the same new ordering
5. Nested inside a block at ES5: `{ enum E { A } }`, `{ namespace N { export const k = 1; } }`, `{ class C {} }`. These use list shapes created by the TS and ES2015 passes, where the list's original is either missing or Rust-only, which the new predicate now reads. Upstream: the enum/namespace `let` stays in the block; the class `var C` hoists.
6. `for (const [a] of xs) {}` at ES5. The `_i`/`xs_1` initializer list has no original and hoists; the body's `var a` list has the `const` as original and must not hoist. Your plain for-of rows may not create the second kind of list.

Everything else in the diff (range removals, `__read`/`__rest` handling, the BigInt predicate, the uninitialized publication removal) is round 49–53 work already reviewed. I found nothing new there.
