Start native validation. One small repair to the emitter lane is worth making first, and I found no over-admission in the predicate. A few controls should be added or recorded as baselines. This is from reading the uncommitted `recovery.rs` and `printer.rs` diffs on base d27af3085; I made no edits, builds or Cargo runs.

## 1. `report_has_retained_variable_delimiter` (`recovery.rs`)

**What it checks, which is enough:**
- **Event:** a `Parser` event with no missing node and `is_current_token_report`, so the length is non-zero and trivia is skipped exactly.
- **Diagnostic:** a retained diagnostic (a suppressed or deduplicated one is refused) with `code() == 1005` and `message_text() == "',' expected."`.
- **List:** a reachable `VariableDeclarationList` whose parent is exactly a `VariableStatement` via `declaration_list`.
- **Skipped tokens:** no `TokenSkipped` action overlaps the list's range.
- **Pair:** consecutive declarations where:
  - `previous.end == boundary == next.pos == next_name.pos`;
  - both names are non-empty Identifiers;
  - `next_name.end` is the report end, and the trivia-skipped start of the name is the report start.
- **Uniqueness:** exactly one such pair.
- **Profile:** it's reached only when `allow_context_recovery` is set.

**What it excludes:**
- for heads (the parent must be a statement);
- patterns on either side;
- nested argument, array and object reports (no adjacent declaration pair);
- tokens skipped inside the list, including element aborts and the no-progress skip.

Each missing separator in a multi-gap list pairs independently.

**Not checked, and doesn't need to be:** non-skip actions such as reparse. The context profile's existing `supports_reparse_runs` still governs those.

**Record current admission as a baseline** instead of tightening unrelated predicates:
- `let a 1;` gives 1134 with a list abort but no skip. By my reading, `report_has_declaration_list_boundary` rejects it because the next statement isn't a `TypeAssertionExpression`, but assert its current profile values explicitly.
- `let a = f(x y)`, `[1 2]`, `{x: 1 y: 2}` and `function f(a b) {}`: assert their current profiles too, so any later change shows up as a baseline difference rather than a silent one.

## 2. The initializer lane (`printer.rs`)

**Correct as written:**
- It runs one trailing phase through `emit_deferred_expression_trailing_comments` with `DeferredExpressionSourceComments::nested(initializer_context.comments(), LeadingAndTrailing)`. That applies:
  - the container guard against the declaration's claim;
  - `NO_TRAILING_COMMENTS` and comments-disabled, taken from the name's flags via `..name_owner`;
  - the empty-range and synthesized-range skips;
  - the JSDoc filter.
- It then does an unconditional `write_space(" ")`, then `=` through `emit_source_leading_token_with_context(…, Required, …)` at the original cursor, with no resume.

This matches TS's order: the name/type trailing phase, then `emitInitializer`'s `writeSpace`, then `=` with leading comments only. It also absorbs both former special cases:
- **For-await:** the name end equals the declaration's claimed end, so the helper returns `RetainedByParent`.
- **Missing type:** the type's range is empty, so it returns `EmptySourceRange`.

**`declaration_syntax`:**
- **Declared type:** the helper is correctly skipped, because the r118/r119 branch already emitted the type's trailing phase.
- **Literal initializer with no type:** the helper runs on `name_owner` with the JSDoc filter.

Nothing is emitted twice.

**The repair.** The erased-type branch still calls `emit_trailing_comments_at_node_position(name)` for the name's own phase. That call checks comments-disabled and `NO_TRAILING_COMMENTS`, but **not** `nested_comments_suppressed` or the container. With an erased type under an ancestor that has `NO_NESTED_COMMENTS`, native would print the name's comment and TS would not; your printer-flag set's `ParentNoNested` rows can hit this.
- Replace it, inside the same `!initializer_context.nested_comments_suppressed()` guard, with the same helper using `name_owner`, then the erased-type owner, in that order.
- The container result can't change: with an initializer present, the name end never equals the declaration's claimed end. So the only effect is correct suppression.

**Transformed names.**
- For renamed and converted-loop names, the metadata-first comment range and `original_node_end_cursor` agree. Earlier TS ES5 probes print `var x_1 \n    /*c*/ = 2`.
- One divergence remains, and it predates this change. For a clone that has an original link but a synthesized raw range, TS's `emitInitializer` uses `node.name.end == -1`, so it prints no `=` leading comments. Native uses the original chain's end.
- After this change native only prints comments that follow a line break there, so the gap is narrower than before. It is not a blocker.

## 3. Controls to add

1. Erased type, `ParentNoNested` on the list, with name and type comments. This covers the repair above.
2. ES5 for-of lowering with comments after the name, on the same line and on the next line: `for (const x /*a*/\n/*b*/ of arr) {}`. The binding is rebuilt from the original declaration name.
3. The baseline-profile assertions for the negative neighbours in section 1.

The 20 `using`/`await using` controls, including parse-valid `{ using a; }`, cover the specific lowering risk. Proceed with native validation of the 260, 145 and 120 sets.
