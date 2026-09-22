Native's shared `emit_leading_comments` (`printer.rs:19503-19536`) drops a filtered comment together with all of its separator bookkeeping (`continue`). TS keeps the separators on the detached path only; `emitLeadingComment` returns before any separator. So the fix must be limited to the detached writer.

# Round 151: A and B diagnosed, with owners and minimal fixes

This is from reading the source at `4d09f3534` plus vendored-TS probes. No edits or builds. I left C alone, pending Root's native event dump.

## A. Escaped `async` modifier printed raw (`\u0061sync`)

**TS phase:**
- A Token node (keyword or punctuation) goes through `pipelineEmitWithHintWorker` to `writeTokenNode(node, writeKeyword|writePunctuation)`.
- `writeTokenNode` always writes `tokenToString(node.kind)` and never the source text.

**Native path:**
- The `NodeData::Token if changed` arm (`printer.rs:2890-2898`) canonicalizes only transformed nodes. A node is changed if it has an original, is synthesized, or is structured.
- An unchanged parsed token falls through to the generic `_ if !changed => write_original_without_leading_trivia` (8458). That writes the raw spelling `\u0061sync`.

**Minimal fix:** drop the `changed` guard on that arm and print `token_to_string(kind)` for every Token. Keep the raw fallback only where `token_to_string` returns `None` and the token is unchanged, so no new error path appears. The earlier arms (JSX fragments, NotEmittedTypeElement, declaration type keywords) stay first.

**Why it's safe:**
- For every unescaped token, the raw text equals the canonical text, so output only changes for escaped keywords. That is exactly TS1260's domain.
- Comments and maps use node ranges, not text length. The column difference in the map goes away with the text fix.
- Identifiers with escapes stay raw, because TS `getTextOfNode` uses the source text for identifiers.
- `yield`, `await`, `function` and similar are already written as fixed keywords by their own arms.

**Controls** (in addition to the 176 already passing):
1. `(\u0061sync x => x);` in all configs.
2. `\u0061sync function f() {}` with an ESNext target (modifier on a declaration).
3. `class C { \u0073tatic x = 1 }`
4. `\u0065xport const x = 1;` with ESM output.
5. `\u0064eclare var y: number;` (erased; must stay NotEmitted).
6. Identifier negatives: `var \u0061sync = 1; x.\u0069f;` must keep their escapes.

## B. d.ts leading `" \n"` for `/* license */ ///<reference …/>` plus blank line

**TS phase:** `emitDetachedComments` (`_tsc.js:16817-16862`) groups the leading comments (the license and the reference, same line, then a blank line). It calls `emitComments(…, leadingSeparator=false, trailingSeparator=true, …, writeComment)` (16794-16816).

In `emitComments`, the **separator bookkeeping is independent of `writeComment`**:
- a pending separator writes `writeSpace(" ")` before the next comment;
- `hasTrailingNewLine` triggers `writeLine()`, otherwise the separator is armed;
- if one is still armed at the end, it writes a trailing space.

The declaration printer's `writeComment` is `emitComment` (121268). `shouldWriteComment` there returns early for non-JSDoc comments, before `emitPos` and before any text, but the separators have already been decided.

**Trace for this input:**
1. `/* license */` is filtered and has no trailing newline, so the separator is armed.
2. The reference writes the separator `" "`, is itself filtered, and has a trailing newline, so `writeLine` produces `" \n"`.
3. Neither comment gets a map segment, which is why the map starts with `;`.

**Probe confirmation (vendored TS):**

| Source shape | d.ts start |
|---|---|
| `/** license */ ///<ref…>` | `/** license */ \n` |
| license on its own line, then the reference | nothing (`writeLine` at line start is a no-op) |
| `removeComments: true` (only pinned comments are considered) | nothing |

**Native gap:**
- `emit_detached_comment_prefix` (`printer.rs:16509-16546`, `DetachedSourceCommentPolicy::All`) calls the shared `emit_leading_comments` (19503).
- That function skips a filtered comment with `continue` **before** any separator logic.
- Its newline model is also "newline in the preceding whitespace", not `emitComments`' `hasTrailingNewLine` and separator-flag protocol.

**Do not change the shared `emit_leading_comments`.** TS's per-comment `emitLeadingComment` path returns before any separator for filtered comments, so the shared helper is already right for ordinary leading phases.

**Minimal fix:** add a private detached-only port of `emitComments`, and use it only in `emit_detached_comment_prefix`'s `All` policy:
- Iterate the same collected comment ranges (`collect_source_comment_ranges`, which provides `has_trailing_newline`) between `owner_start` and `emitted_through`.
- For each comment, in order:
  1. if a separator is pending, `write_space(" ")`;
  2. if `!only_print_js_doc_style || should_write_js_doc_style_comment(...)`, write it with `write_source_comment`, which keeps the start/end map positions;
  3. if it has a trailing newline, `write_line(false)`; otherwise arm the separator.
- After the last comment, if a separator is pending, `write_space(" ")`.
- Keep `emit_pinned_leading_comments` for the removeComments policy unchanged.
- Check `emitNewLineBeforeLeadingComments` (called at 16845). It is a no-op at `pos == 0`, but for non-file detached owners (node arrays via `detached_comment_prefix_for_node_array`) it writes a newline when the node is preceded by a line break. The port must include it if those callers use the same prefix path.

**Pitfalls:**
- **Writer behaviour.** Confirm that native `write_space` writes even at line start, as TS `writeSpace` does, and that `write_line(false)` is a no-op at line start, as TS's is. Otherwise `" \n"` will come out as `"\n"` or `""`.
- **JS output is unchanged:** `only_print_js_doc_style` is false there, so every comment is written, and separators then behave as before. JS is already exact.
- **Filtered comments must emit no map positions.** Only the new line creates the leading `;`.

**Controls (d.ts, removeComments false):**
1. The fixture shape.
2. `/* a */ /* b */` + blank line + declaration → expect `" \n"`.
3. `/** a */ /* b */` + blank → expect `"/** a */ \n"`.
4. `/* a */` alone + blank → expect `""`.
5. `// a` then `// b` + blank → expect `""`.
6. The license and the reference on separate lines → expect `""`.
7. A non-file detached owner, e.g. a namespace body starting with `/* a */ /* b */` then a blank line.
8. The same shapes with removeComments true, and the JS outputs (must be unchanged).
