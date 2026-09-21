No blocker in d74ca0fbc. The four new trailing-comment calls cannot suppress less than the old `BoundaryUnion` token calls did. The ordinary-token grammar reset matches TS and doesn't affect hooks, hints or maps. This is from reading the diff, the helper bodies and the vendored `_tsc.js`; I made no edits and ran no builds.

## 1. The four `emit_trailing_comments_for_node_as_token_anchor` calls

**Suppression now versus before.**
- The helper checks `comments_disabled()` (remove-comments or failure), the child's `NO_TRAILING_COMMENTS` flag, and `NotEmittedStatement`. It reads the child's comment range, metadata override first, which matches TS `getCommentRange`.
- The old path, `emit_space_prefixed_token_with_comments` → `BoundaryUnion` → `emit_comments_at_cursor_with_phase`, checked only `comments_disabled()`. It ignored both `nested_comments_suppressed` and the container state.
- So the new calls check a superset of the old conditions. With the resume anchor, the token then skips exactly the comments already written. Output differs only by the removed extra space.

**Nested-comment suppression can't reach these sites.**
- The only place upstream sets `NoNestedComments` is the TS transform's import-equals `moduleReference` (`_tsc.js:95619`).
- Natively it is set in exactly two places:
  - the same import-equals reference (`builtins.rs:14944`);
  - the post-failure root (`printer.rs:1163`), which `comments_disabled()` already covers.
- Neither is an ancestor of an import/export clause, an import specifier's property name, or a do-statement body.
- An explicit `nested_comments_suppressed` guard would be dead code. It would also be inconsistent on its own: the following token call would still emit the same comments without a resume anchor.

**Container suppression.**
- TS's trailing guard (`end !== containerEnd`) can't fire here. The child always ends before the separator token that follows it (`from`, `as` or `while`), so its end never equals a claimed container end.
- `separator_anchor_after_child` would give the same result. When the container does own the boundary, it returns a plain cursor, and the old token call would then emit the comments anyway.
- Keep the direct helper. Switching is not needed.

**The export site's `module_specifier.is_none()` branch is correct.** Without `from`, the clause is `semicolon_owner` and its trailing comment belongs to the terminator path. Emitting it here as well would duplicate it.

The non-block do branch is unchanged, matching your non-block controls.

## 2. Ordinary tokens with `for_child(NORMAL)`

- `for_child` keeps the comment scope and `nested_comments_suppressed`. It resets only the grammar and the no-ASI left edge. Neither applies to a `?.` or `*` token, since both always follow an operand or keyword.
- TS `emit(node.questionDotToken)` / `emit(node.asteriskToken)` has no parenthesizer rule.
- Hooks and hints are unaffected. The token still goes through `emit_node_with_hint[_and_source_comments]` with `EmitHint::Unspecified`, and substitution/notification are keyed on node and hint, not grammar.
- Source maps: the token's own boundary maps are unchanged. The only removals are the bogus source-ranged paren and the paren maps from b21bef2b7 that came with it, which TS doesn't produce.
- Provenance: the deferred leading/trailing anchors (`preceding`, `parent`) are unchanged.

## 3. Minimal probes

1. **Comment-range overrides on the four child kinds.** I found no `set_comment_range` in any builtin that names these kinds. If a transform ever sets one on an import clause, a specifier property name or a do body, the helper's resume anchor wouldn't match the token cursor. The token's `BoundaryUnion` could then re-emit same-line comments at the original end. A one-line debug check over the 61 new cases is enough: compare the comment range with the text range for those children.
2. **ES5 do-while with a captured loop binding.** es2015 converts the loop, and the do body can become a synthesized block that keeps the original range. Confirm one control like `do { let x; f(() => x); } /*t*/ while (c);` at ES5 matches TS. I expect it to, because the block's comment range is its own range, which is TS's rule too.

Noted on the ledger items: the kind counts will come from your computed JSON, not my summary tallies. The 391 inherited containment rows are candidates for follow-up, not proven bugs, since composite names may be valid.
