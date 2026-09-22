# Round 154: U+2028 detached header

The owner is detached-prefix **discovery**, not the new writer or the NotEmitted path. The bug predates this work: the function has been identical since `683c1aeb2` (2026-08-09).

## Exact owner and phase

The call chain is `detached_comment_prefix_at` (frozen `printer.rs:16420`) → `detached_leading_trivia(&text[start..code_start])` (19302). That function:
- counts line breaks only for ASCII `\r`/`\n` inside an `is_ascii_whitespace` loop;
- ends a `//` comment only at `\r`/`\n`.

For `/* a */\u2028/* b */\u2028\u2028declare var x: number;`:
1. U+2028 isn't ASCII whitespace, so the generic "advance one char" fallback skips it without counting it. The next whitespace run restarts at `line_breaks = 0`.
2. The count never reaches 2 after a comment, so discovery returns `None`, and there is no `DetachedCommentPrefix`.
3. The comments then become ordinary leading comments of the first statement. `declare var` becomes a NotEmittedStatement, whose leading phase emits only recognized triple-slash comments. Both comments disappear.

The new `emit_detached_comments` writer is never reached, and it is already Unicode-correct.

**TypeScript's behaviour:** `emitDetachedComments` groups comments by `getLineOfLocalPositionFromLineMap`. `computeLineStarts` treats CR, LF, CRLF, U+2028 and U+2029 as line breaks, and `getLeadingCommentRanges` ends a line comment at any `isLineBreak`. So both comments are detached, each with `hasTrailingNewLine`, and each is written on its own line with a map.

**Same owner, same bug with non-ASCII whitespace:** a blank line containing NBSP (`/* a */\n\u00a0\n code`) splits the whitespace run and resets the count. TS still sees the code two lines after the comment, so it would detach.

**Existence:** the function bytes are identical at `640146c2a`, `4d09f3534` and `3e28cb213`. It has one caller, which is shared by file owners and node-array owners.

## Minimal safe fix: rebuild discovery from the two Unicode-correct helpers already used on the pinned path

Replace the body of `detached_leading_trivia`, or give it `(source, start, code_start)`, with the following:

```text
comments = collect_source_comment_ranges(source, start, /*trailing*/ false)
           .take_while(|c| c.end <= code_start)
group = longest prefix where !contains_two_line_breaks(source, prev.end, next.start)
if group non-empty && contains_two_line_breaks(source, last.end, code_start):
    detached_end = last.end + length of the whitespace_like run following it
    (for ASCII input this is byte-identical to the old cursor)
else None
```

**Why each piece:**
- `collect_source_comment_ranges(…, false)` is already TS `getLeadingCommentRanges`: it skips a shebang at 0 and a BOM, collects only after the first line break when `pos > 0`, and ends line comments at Unicode line breaks.
- `contains_two_line_breaks` is already the TS line-map count: CRLF counts once, and U+2028/U+2029 count as breaks. `detached_pinned_comment_end` already uses it, so the `PinnedOnly` and `All` policies agree on line semantics.
- Keep the returned `emitted_end` at the same position the old function returned ("after the blank-line whitespace run"), so that `emitted_through` and any carried-prefix logic are unchanged. `resume_next` already uses the last comment's end, which is TS's `detachedCommentEndPos`.

**Scope:** only discovery changes.
- The ordinary leading path (`emit_leading_comments`, first-statement and NotEmitted handling) isn't touched.
- ASCII inputs behave byte-for-byte as before, provided the whitespace-run end is computed the same way.
- It covers node-array owners too (same caller).

**Adjacent, separate:** the shared ordinary `emit_leading_comments` also decides newlines by ASCII `\r`/`\n` only (its `contains('\r' | '\n')` test). A non-detached `x;\n/* a */\u2028/* b */\u2028y;` may put both comments on one line where TS writes a line break. Don't bundle it. Add one control to find out, and fix it only with evidence.

## Controls (JS and d.ts, removeComments false, unless noted)

**Line-break forms of the blank line, header `/* a */<sep>/* b */<sep><sep>declare var x: number; var y = 1;`:**
1. LF (existing, must stay exact).
2. CRLF.
3. CR only.
4. U+2028 (the failing fixture).
5. U+2029.
6. Mixed: `\u2028\n` counts as two breaks and should detach. `\r\n` counts as one and should not.

**The other shapes this discovery owns:**

7. A line comment ended by U+2028: `// a\u2028\u2028code`.
8. NBSP inside the blank line: `/* a */\n\u00a0\nvar y;`.
9. A non-file owner: `namespace N {\u2028/* a */\u2028\u2028export var v = 1; }`, and the same inside a function body.
10. removeComments true with `/*! a */\u2028\u2028var y;` (pinned path, must stay exact).
11. No blank line, `/* a */\u2028var y;`. Not detached, so the comment belongs to `var y`, exactly as with LF.
12. A BOM or shebang at file start, followed by a U+2028 header.
13. The ordinary-leading probe above, as a report-only observation.
