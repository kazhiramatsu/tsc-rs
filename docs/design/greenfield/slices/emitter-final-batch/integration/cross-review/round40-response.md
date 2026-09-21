**Verdict: the binary-left change is justified. Before vs candidate compared at the exact sources; one specific uncovered risk below.**

**Before (71b1ae19b).** `BinaryExpression` emitted `data.left` via `emit_required_node_with_forwarded_source_comments` (before printer.rs:7695 → 14304). When the state was not Pending it fell to `emit_node_id_with_context` → `emit_node_with_hint` → worker with `deferred_source_comments = None`. In that worker every leading/trailing step goes through `emit_deferred_expression_leading_comments`, whose first line returns `NotVisited` on `None`, and `emit_deferred_expression_trailing_comments`, which returns `None` on `None`. So a ranged left operand printed no source comments at all unless an enclosing node had forwarded a request. The right operand, by contrast, always ran `emit_child_after_token_with_complete_source_comments` (LeadingAndTrailing). That asymmetry is the bug; my r38 trace read the already-patched file and did not see it.

**Upstream.** `onLeft` → `maybeEmitExpression(next, parent, "left")` (118468-118484) runs the full pipeline from Notification, so every non-binary operand gets `emitCommentsBeforeNode`/`emitCommentsAfterNode`; a nested BinaryExpression left is returned to the trampoline and `onEnter` (118396-118412) runs `emitCommentsBeforeNode(node)` itself, `onExit` the after-phase. No operand is ever emitted without its comments phase. The candidate's `emit_expression_child_with_source_comments` (nested LeadingAndTrailing; Pending → forward; NoNested → plain) is the same helper already used for access targets and callees, so this aligns the left operand with both upstream and the right operand.

**Guards preserved.** Pending forwarding and nested suppression are the first two branches of the helper. The outcome is always `Complete` because only `LeadingOnly` extents produce `LeadingConsumed` (producers at before 4696/13927/14269/14767), so the helper's debug assertion holds. The operator anchor now prefers the child's visited trailing anchor and falls back to the previous lookup, then to `separator_anchor_after_child` as before.

**Uncovered risk (specific).** The child phase's `VisitedHere` anchor sits at the phase owner's range end (the transformed node's comment/text range), but the lookup cursor is `original_node_end_cursor(left)` (before 16724), which follows the `get_original_node` chain. If a wrapper is `set_text_range`'d to one location but `set_original_node`'d to a node with a different end (a CommonJS export wrapper ranged to the binding name whose original is the declaration or specifier), the filter fails, the fallback `emit_trailing_comments_for_node_as_token_anchor` also uses the original end, and a same-line trailing comment can now be printed by the child phase at the name end while the token anchor still claims the original end. Before the change only the second claim existed. Controls that isolate this, CommonJS ES2015 and ES5:

```
export const x /* t */ = 1;
export const { /** a */ x /* t */ = 1 } = o;
export { x /* t */ as y };
a = b /* c */ = d;   // inner-binary left: outer operator anchor from the inner Complete outcome
```

Expected TypeScript byte counts of `/* t */` decide it; the 216-case fixture covers leading `/** leaf */` occurrences but I did not see a trailing-comment row in its shapes.

**Housekeeping.** The new test reads `export-destructuring-comments.json` (216 cases) while the worktree also holds an untracked `export-destructuring-boundaries.json` (72 cases) that no test in the diff references; both are untracked, so make sure the one the test uses is committed with it and the other is either wired or dropped.

**Wrapper ranges.** Correction accepted. The frozen `re-export` control shows `/** leaf */` twice, which is exactly what `createExportExpression`'s `setTextRange` on the wrapper produces; my deletion recommendation is withdrawn and the wrapper ranges stay intact.

The JSDoc probe record shows both JS and d.ts hashes equal at exit 0 for the focused CLI run; no claim beyond that record until the full contract comparison finishes.
