# Follow-up to r136: checking your corrections before implementation

Your corrections 1–3 hold against the source, but one premise of the request doesn't: the producer-prep tree is **not** unedited, and the for-of repair is already in it. Your design is right in substance; I'd narrow one detail, where the printer reads the type from. I made no edits or builds.

## Tree state
- `tsc-rs-emitter-final-variable-producer-prep` is at `d5ceab6e6`, but `git diff HEAD` shows an **uncommitted 10-line patch** in `es2015.rs:12478-12488` and `12513-12514`. That patch is the for-of comment-range repair (section D).
- The comma-prep checkout is at `d891c58e7`. `d5ceab6e6` is its ancestor. That older tree is where r136 read the for-of code without the patch.

## A. Confirmed mechanisms

1. **Correction 3 is right.** `moveRangeEnd(range, end)` returns `createRange(range.pos, end)` (`_tsc.js:17301-17306`), so `-1` is the end value itself, not `end - 1`. My r136 point about UTF-16 subtraction was wrong, so I withdraw it.

2. **Correction 2 is right.**
   - `set_original_node` (`factory.rs:836-883`) calls `merge_from`, and `merge_from` copies `type_node` (`metadata.rs:926-927`).
   - `set_semantic_original_node` (`factory.rs:885-904`) only sets `original` and `original_is_semantic = true`. It merges nothing: no comment range, no source-map range, no type.
   - How the lookups treat a semantic link:
     - `get_original_node` (760) follows it.
     - `parse_tree_node` (805) follows it until it reaches a parsed node with an `Original` range.
     - `get_emit_original_node` stops at it, so lexical-environment lookups don't change.
   - TS itself only assigns `name.original = node` (`_tsc.js:21664`). The semantic setter is the closest existing adapter.

3. **The CommonJS failure is reached this way** (`builtins.rs`):
   - The dispatch at ~7633 calls `substitute_identifier` unless `is_non_reference_identifier_node` (4781) returns true.
   - The eager generated name has no original, so that check looks at the generated node itself. It gets past the check only if that node's `parent` is not the declaration; the error proves it got past. This is an inference: the parent field hasn't been read directly.
   - In `substitute_identifier` (~9990): `get_original_node(generated)` returns the generated node itself, and its `pos` is not `u32::MAX` because `set_text_range` gave it the parse name's range. It then calls the strict `resolver_node` (11180), which raises `ResolverNodeNotInParseTree`.
   - TS never gets here: `_tsc.js:111954` skips generated identifiers.
   - That node `0:37360` is this eager name is strongly supported but still a hypothesis; the witness in F confirms it.

4. **Correction 1 is right.**
   - The printer's initializer arm (`printer.rs:4736-4757`) reads `erased_type` only from `metadata(name).type_node`. The eager generated name has none, so `equal_cursor` falls back to the name's end.
   - The leading-comment scan from that point stops at `: number`, so `/*c*/` is lost.
   - In TS, `emitInitializer` reads the type end from the un-substituted `node.name`, while `emit(node.name)` puts comments on the substitute, which has no type (so no `/*a*/`).
   - `update_variable_declaration_name` uses `update_node` as you said, and I found no declaration being rebuilt. Candidate (a) from r136 is withdrawn.

5. **The print-order flag already identifies the eager rename.**
   - `mark_generated_binding_print_order` has exactly one producer: `es2015.rs:5952`, inside `colliding_declaration_name_substitute`.
   - `merge_from` spreads it with `|=`, so later clones of the generated name keep it.
   - So it already marks "stand-in for a print-time substitution" without a new synthetic flag.

6. **The ts transform sets `type_node` on the name inside the updated declaration** (`builtins.rs:11678-11690`). By the time es2015 runs, the `name` argument is that node or a clone of it. Native `merge_from` copies the type onto such clones.

## B. Smallest repair for classes 1 and 2

**es2015.rs, `colliding_declaration_name_substitute`, right after the existing `set_text_range(generated, name)`:**
```rust
self.context.arena_mut()?.set_semantic_original_node(generated, name)?;
```
- Link to **`name`**, the current node that may carry `type_node` (possibly an intermediate clone), **not** to the parse identifier `original`. `parse_tree_node` still walks on to the parse `x`.
- Keep the raw range and the binding metadata as they are. The generated name still has no `type_node` and no comment range of its own.
- What this does for CommonJS (from reading the code): `is_non_reference_identifier_node` now follows the chain to the parse `x`. Its parent is the parse VariableDeclaration with `name == x`, so it returns true, `substitute_identifier` is never called, and no resolver query happens.
- This matches TS's result, but through the declaration-name filter rather than TS's generated-identifier check. The resolver gate is not relaxed.

**printer.rs initializer arm (~4736-4757), for `equal_cursor` only:**
- When `!declaration_syntax`, `erased_type` is `None`, and `metadata(name).generated_binding_print_order()` is set:
  - walk `EmitMetadata::original()` from `name` while the node carries the print-order flag;
  - take the **first unflagged node**, which is the pre-substitution name;
  - read **only that node's** `type_node`.
- Use it as `declared_type.or(erased_type).or(pre_substitution_type).unwrap_or(name)` for the cursor.
- Do **not** feed it into `erased_type` or the trailing owner. Both trailing-helper calls keep reading the actual name, which has no type, so `/*a*/` stays suppressed as in TS.
- Why read only the first unflagged node rather than the first `type_node` anywhere on the chain: TS reads `typeNode` only from the un-substituted `node.name`. A deeper link can't add a type the pre-substitution name didn't have. Clones are still covered, because native `merge_from` already copied the type onto them.
- Nothing changes in the raw-cursor rule, the general original fallbacks, or the collectors.

**Side effects to control (hypotheses, not verified):**
- **(h1)** `printer.rs:1315`: for base-named bindings, the print-time name cache is keyed by `get_original_node`, which now resolves to the parse `x`. That matches TS keying the cache by the original node, but the spelling of the captured reference must be checked (N3).
- **(h2)** System's lenient resolver path now returns `Some(x)` for the eager name instead of `None`, so it may ask whether `x` is exported. For a nested local the expected answer is "no" (N1, N2).
- **(h3)** The printer has about 40 `get_original_node` sites. Lines 7757 and 16096 are safe because they only act when the node has a `type_node`. I did not audit the other ~38.

## C. Class 2 (System)
The same two changes should cover it: the cursor comes back through the print-order walk. Whether System also needs the semantic link is unknown, because System already tolerated the missing parse node.

## D. for-of (already patched, uncommitted)
- Declaration head: the list gets `EndOnly(initializer.end)` (matching `moveRangePos(-1)`), the statement gets `StartOnly(initializer.pos)` (matching `moveRangeEnd(-1)`), both via `set_comment_range_raw` (8784).
- Assignment head: the statement gets `StartOnly`.
- Pattern branch: correctly unchanged. TS gives the list the full initializer range and the statement no range.
- The mapping is correct. The per-side claim in the printer is designed for one-sided ranges (`deferred_container_scope`, `printer.rs:17166`; `metadata.rs:138-141`), but a StartOnly statement not claiming the container end has only been read from code, not observed.

## E. Critical negatives
Mint expected output from vendored TS. Run CommonJS and System, checked and noCheck, with removeComments false and true.

**Rename, export and spelling:**
- **N1** `export let x = 1; { let x: number = 2; (() => x)(); }`: the nested name prints `x_1`, and neither `exports.x` nor `exports_1(` appears.
- **N2** `let x = 1; export { x }; { let x = 2; (() => x)(); }`: same check.
- **N3** The captured reference `(() => x)` prints the same spelling as the declaration (h1).

**Typed and untyped rename:**
- **N4** Your exact fixture: `var x_1 \n    /*c*/ = 2;` with no `/*a*/`.
- **N5** Untyped rename `let x /*a*/\n/*c*/ = 2`.
- **N6** Typed with the comment on the same line only: `let x: number /*a*/ = 2`.
- **N7** Typed **without** a rename, in a block with no collision: output must not change (the ordinary `erased_type` path still emits `/*a*/`).
- **N8** Destructuring rename `{ let {a: x} = o; (() => x)(); }`: the pre-substitution walk must find no type.

**for-of:**
- **F1** `for (let x /*a*/ of arr)`, both the converted `_loop_1` form and the unconverted form.
- **F2** The same with `/*a*/` on the next line.
- **F3** A leading comment: `for (/*l*/ let x of arr)`.
- **F4** Assignment head: `for (x /*a*/ of arr)`.
- **F5** Pattern head: `for (let [x] /*a*/ of arr)` must not change.
- **F6** Destructuring assignment head: `for ([x] /*a*/ of arr)` must not change.

## F. Still unproven
- That node `0:37360` is the eager name. A one-shot witness dumping its kind, parent, original, pos/end and calling site before the change would confirm it.
- That the declaration-name filter is enough on its own, i.e. that no other CommonJS site queries the generated name.
- h1–h3 above.

Passing N1–N8 and F1–F6 is not full qualification; the successor replay and corpus validation are still required.
