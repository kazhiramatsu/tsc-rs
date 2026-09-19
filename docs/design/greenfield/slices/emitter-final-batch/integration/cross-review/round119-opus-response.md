I found no remaining code issue in the uncommitted property diff on d70df944e. This is from reading the diff and the paths it touches; I made no edits or builds.

**Nested-comment suppression**
- All three new trailing calls check `!expression_context.nested_comments_suppressed()`.
- The d.ts `=` goes through `emit_source_leading_token_with_context`, which applies the same check.
- `comments_disabled()`, `NO_TRAILING_COMMENTS` and the d.ts JSDoc-only filter are all enforced inside `emit_trailing_comments_for_node[_in_container]`. So plain `/* */` comments stay out of d.ts, which your filter control checks.

**Container scope and provenance**
- **The scope passed is the property's own claim.** `expression_context.comments()` in this arm comes from `emit_class`'s member loop → `emit_node_id_with_context` → the generic worker's ordinary branch, which claims the owner's range via `active_expression_comment_scope`.
- **Ranges are read metadata-first.** `comment_range_for_node` checks metadata before the node's own range.
- **Parameter property:** the synthesized member's comment range is the parameter, ending at 54.
  - A reused type node ending at 54 is retained, so only the member-level `emit_trailing_comments_for_node` prints `; /** t */`, once.
  - A type rebuilt from the checker has no range, so its phase does nothing.
  - With a default initializer the parameter ends at 67, so the type phase prints, as in TS.
- **Parsed property:** the range includes `;`, so the name end and type end aren't retained and both phases print. Without a semicolon, the type end equals the property end, so the type phase is retained and the member phase prints the comment after the synthesized `;`. Your no-semicolon control covers this.

**Token duplication**
- **Name → `?`.**
  - The name phase stops at a non-comment character. For `z? /** n */` it prints nothing, then the `?` token's phase prints `/** n */`.
  - For `z /** n */?` the name phase prints the comment, and the `?` phase finds nothing after `?`.
- **Synthesized `?`** (e.g. a readonly optional parameter property): range -1, so the phase does nothing.
- **`!`:** nothing is emitted after it. The name phase stops at `!`, which matches TS dropping the comment after an omitted `!`.
- **`=` (d.ts only):**
  - `SourceLeading` at the name end or declared-type end emits only comments that follow a newline.
  - Same-line comments were already printed by the name or type phase, and `emit_same_line_trailing_comments` stops at a line break, so there's no overlap. Your `= /** v */ 1` control covers the initializer's own leading comments.
  - The JS `=` path still uses `BoundaryUnion`, which is still the name/type trailing lane there.

**Unchanged:** `emit_type_annotation`, `emit_optional_declaration_token`, the `Parameter` arm, the JS property path, and the committed System candidate.

**Two non-blocking checks for the test run**
1. `declaration_syntax` isn't only the d.ts pipeline. It's also enabled by the checker's declaration printers (`checker/src/declaration_emit.rs:971, 2026`) and by printer reprint/metadata contracts, for example `declaration_printer_reprint_contract.rs` and `ellipsis_comment_metadata_contract.rs`. Checker node-builder output is synthesized and has no ranges, so it's unaffected. A reprinted parsed class property with comments would now follow TS's name/type/`?` phases. If an existing contract expectation changes, confirm it with pinned TS `createPrinter` before accepting the new output.
2. In the definite-assignment control, confirm native's d.ts drops the `!` as TS does (`w: number /** t */;`). If native keeps it, that's a separate existing issue, not this diff.
