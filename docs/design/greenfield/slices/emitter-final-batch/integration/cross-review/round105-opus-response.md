The implemented diff matches the r104 guidance. I found no correctness blocker and no duplicate comment for typed declarations without an initializer in JS output. Two small adjustments to the new no-initializer call are worth making: move it up (fixes d.ts order) and guard it against nested-comment suppression. A third gap (comments after an erased type on hoisted names) can wait. This comes from reading the current Recovery files and TS probes (`/tmp/r80/s.mjs`); I made no edits and ran no builds.

## Checked, no issues

- **Map boundaries on source-ranged virtual parens** (`printer.rs:~14731`, `~14777`):
  - The range comes from `expression_comment_phase_owner_for_text_range(substituted)`, i.e. the substituted node's raw pos/end, with no child metadata or flags.
  - The Before map is recorded after the paren's leading comments and before `(`. The After map is recorded after `)` and before its trailing comments.
  - `record_map_range_side` skips trivia only on the Before side, returns early for a synthesized range, and records through the writer, which stops recording while `suppressed_depth > 0` (an ancestor's NoNestedSourceMaps).
  - This matches TS's `emitSourceMapsBeforeNode`/`AfterNode` on a fresh `setTextRange` paren.
- **`emit_comment_after_open_brace`:** the new `node_has_source_token_shape` gate is TS's `getParseTreeNode(context).kind === context.kind` check, which applies to every brace owner. Updated blocks still pass through their original chain. A synthesized block, or one whose original is a different kind (for example an arrow body converted from an expression), gets no brace comments, as in TS.
- **`=` token choice:**
  - The cursor's UTF-16 end is tested with `initializer_context.comments().retains_end`. That scope includes the declaration's own claim, which is the container TS's name trailing phase is checked against.
  - Resulting behaviour:
    - `const x /*c*/ = 1`, ES5 `var rest /* name side */ = []`, and `const x: number /*c*/ = 1` all keep `BoundaryUnion`, because the claimed end is after the initializer.
    - The for-await binding (list and declaration end at 102) switches to `SourceLeading`.
  - The `SourceLeading` branch also respects `nested_comments_suppressed`. That is stricter than the old helper and consistent with TS.
- **es2018 object-rest pre-pass:** it matches the r104 review.
  - `source_for_of` feeds `mark_enclosing_block_multi_line`.
  - The raw builder and the visiting wrapper are split, and type and `!` are cleared.
  - Each part is visited once, and there is no block-scope API.
  - The prepared temp declaration's name is a synthesized identifier with no comment range, so the new no-initializer path does nothing for it.

## Typed declarations without an initializer: no duplicate in JS output

TS output (ES5 and ES2017 agree) for:
- `let x /*a*/: number /*t*/;` → `let x /*a*/ /*t*/;`
- the same with `, y` → `let x /*a*/ /*t*/, y;`

How each comment is produced:
- **`/*a*/`:** TS's name trailing phase at the name end. The declaration's claimed end is the end of the type, so nothing suppresses it. The new call emits it, and before this change native lost it: `emit_type_annotation` has no colon-token comment path, and the list and terminator paths only look after the declaration.
- **`/*t*/`:** TS emits this from the list's trailing phase before `;`. Native already has this through the list and terminator path.
  - TS's second name-level call, at `getTypeNode(name).end`, is suppressed here because that end equals the declaration's claimed end.
  - The new call only looks at the name's own comment range, so it never reaches the type end.

Parsed `for (const x /*c*/ of …)` heads and `catch (e /*c*/)`: the declaration claims the name's end, so `retains_end` suppresses the new call and the list-level path still prints once. This matches the existing token-cursor controls.

## Adjustments

**1. d.ts order: move the no-initializer call up.** The new call is in the `else` branch, which runs after the `declaration_syntax` block (`!` token and type annotation).
- TS's `emitVariableDeclaration` is `emit(name)`, whose trailing phase runs, then `emit(exclamationToken)`, then `emitTypeAnnotation`.
- Declaration output keeps only JSDoc-style comments (`only_print_js_doc_style` is honoured by `emit_trailing_comments_for_node`).
- So for `export declare let x /** a */: number;` native would now print `x: number /** a */`. Before this change it dropped the comment.

Smallest faithful fix: make the same call, under `if data.initializer.is_none()`, immediately after `emit_node_with_hint(name, …)` and before `if self.options.declaration_syntax`. JS output is byte-identical, because nothing is written between the name and the old position when there is no initializer and no declaration syntax. The initializer path is unchanged.

**2. Nested-comment suppression: add one condition.** Native represents an ancestor's `NoNestedComments` as `EmitContext::nested_comments_suppressed()`, not `comments_disabled()` (`printer.rs:812` vs `:1153`). The new call only checks `comments_disabled()` and the name's `NO_TRAILING_COMMENTS`, so under a suppressed ancestor it would print comments TS doesn't. Wrap it in `!initializer_context.nested_comments_suppressed()`, the same test `emit_source_leading_token_with_context` applies. The old `BoundaryUnion` `=` path has the same gap; that predates this change and I'd leave it.

**3. Optional, not needed for the current red rows: erased-type end on hoisted names.** TS prints `function* g() { let x /*a*/: number /*t*/; yield; x; }` at ES5 as `var x /*a*/ /*t*/;`.
- The hoisted, synthesized declaration doesn't claim the type end, so TS's second call at `typeNode.end` prints `/*t*/`.
- Native will print only `/*a*/`.
- A faithful fix would be one more `retains_end`-guarded emission at `metadata(name).type_node` end in the same no-initializer branch. For parsed declarations that end equals the claimed end, so it stays suppressed and can't duplicate.
- Given the cautious scope, defer it unless you add that ES5 generator control.

## Dependencies to record for the post-integration design

1. The `=` token doubles as the name's trailing phase. Name comments are split across four places: explicit leading, `=` `BoundaryUnion`, erased-type `emit_trailing_comments_at_node_position`, and the new no-initializer call. In TS they are one `emit(name)` pipeline.
2. Container claims (`retains_end`) now decide which comment phase a token uses. Printing text therefore depends on claim scope as well as ranges.
3. Grammar parentheses are emitted in the printer, but their map and comment identity mirrors factory nodes. Their range comes from the node's range at print time, whereas a TS factory paren takes it when the factory runs.
4. `mark_enclosing_block_multi_line` relies on arena `parent` links, which `update_node` clears.
5. Nested-comment suppression lives on `EmitContext`, and removed/failure suppression on `Printer`. Every direct comment call site has to check both.
