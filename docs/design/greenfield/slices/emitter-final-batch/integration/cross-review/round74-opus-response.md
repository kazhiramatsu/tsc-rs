Both clusters have a one-site cause. This is from the r71 log decoded against the frozen fixtures and a read of the code and the vendored 6.0.3; nothing was built or edited.

## Cluster 1: 28 System `helpers-true` rows, `tslib = tslib_1_1;`
**Evidence** (`iteration-pair`, ES5): native prints `var _a, tslib_1, a, b;`, `function (tslib_1_1) {`, then `tslib = tslib_1_1;`. The expected setter line is `tslib_1 = tslib_1_1;`. Every other line is identical, including the uses of `tslib_1.__read`.

**Cause:**
- The external-helpers import-equals name is a typed numbered binding. Its provisional text is `tslib`, and the finalizer names it `tslib_1`.
- Before round 69, `hoist_name_node` also registered that binding in the text-keyed `generated_bindings` map, so `create_identifier("tslib")` got the binding's metadata back.
- Round 69 correctly stopped that registration. But the import-equals branch of `create_setters_array` (system.rs, about 3663-3670) still builds the setter target from text: `let target = self.create_identifier(&name)?;`. That yields a plain `tslib`.
- The setter parameter (`tslib_1_1`) and the wrapper `var` (`tslib_1`) are already correct.

**Fix:** keep the name *node* in that branch and build the target with the existing identity-preserving helper:
```rust
let name_node = data.name.and_then(|id| self.context.arena().node_ref(self.source, id));
let name = name_node.and_then(|n| identifier_text_owned(self.context.arena(), n).ok()).ok_or(...)?;
let target = self.create_local_name_reference(name_node, &name)?;
```
- `create_local_name_reference` recovers the binding through `generated_binding_of_identifier` (now including `derived_from`) for a generated name, and falls back to text for a parsed `import x = require()` name. So user import-equals output doesn't change.
- Leave the exported import-equals publication (`create_export_call(&name, …)`) alone: the export key is text, and the external helpers import is never exported.
- No reservation change is needed.

## Cluster 2: 4 `pattern-after-using` rows (ES5/ES2015, remove false/true)
The generated line is identical in TS and native: `(exports_1("x", x = source.x), exports_1("y", y = source.y));`. Decoding the maps on that line gives exactly two differences:

| Generated column | TS | Native |
|---|---|---|
| 16 (`(`) | `1:25`, the statement (`export let …`) | `1:36`, the pattern `{ x, y } = source` |
| 76 (after `)`) | no segment | `1:53` |

The rest of the line is identical.

**Cause:**
- Upstream `hoistInitializedVariable` (103664-103677) creates the assignment with `setOriginalNode`, `setCommentRange` and `setSourceMapRange(assignment, node)`, and **no `setTextRange`**. The assignment's pos/end stay -1.
- `createExpressionStatement` then wraps it in a ParenthesizedExpression through `setTextRange(paren, assignment)`, so the paren is unranged and emits no source maps. Column 16 therefore keeps the statement's mapping, `1:25`.
- Rust es_next.rs:1104 calls `self.set_original_and_range(assignment, declaration)?`. That helper (es_next.rs:2327-2336) sets a **text range**, so the round-67 factory paren takes range `36…53`. Its start mapping replaces the statement's mapping at the same column, and its end adds the extra `(76 → 1:53)`.
- System flattening replaces the assignment itself, so only the paren's inherited range is visible. Native output goes through the correct owner (the ESNext `using` hoist); only the range on this one node is wrong.

**Fix (es_next.rs:1104 only):** replace `set_original_and_range(assignment, declaration)` with the upstream triple:
- `set_original_node(assignment, Some(declaration))`;
- a comment range and a source-map range, both equal to the declaration's range. Build them the way other passes do, with `SourceRange::from_raw(pos, end, positions)` wrapped in `CommentRange::new` / `SourceMapRange::new(source, range)`, or add a small `set_comment_and_source_map_range_from` helper in es_next.
- Leave out `set_text_range`. When the assignment isn't flattened (identifier targets, CommonJS, ESNext without System), it is still mapped through its source-map range exactly as upstream, and comments are unchanged through the comment range.
- Leave the statement at 1114 unchanged. Upstream likewise gives it only comment and source-map ranges, but its text range produces identical maps here, and it isn't implicated.

**Likely related:** the 18c `using-object-pattern` and `nested-rest-pattern` rows in System 858 go through the same line and should now match; check them in that run.

## Targeted controls
- **Cluster 1:**
  - the 28 rows;
  - a user `import x = require("m")` under System with `importHelpers: true`, to confirm the text fallback still gives `x = m_1;`;
  - the export-star plus helpers combination.
- **Cluster 2:**
  - the 4 rows;
  - identifier after `using` at ES5, ES2015 and ESNext, under both System and CommonJS: `using r = source; export let x = source; x = 1;`;
  - the object/array/nested-rest patterns;
  - a commented form: `using r = source; /*c*/ export let /*d*/ { x } = source;`, to check comment ownership through the comment range.

Apply both fixes only after the r71 run finishes.