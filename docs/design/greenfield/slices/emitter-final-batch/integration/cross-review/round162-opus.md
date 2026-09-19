The probe shows TS keeps `function f()\n{ return 1; }` on **one** line, and the same with a comment or U+2028 before `{`. Native's raw scan counts that pre-`{` newline, so it has an ASCII divergence too, not only a Unicode one.

# Round 162: `source_node_range_is_on_single_line`

This is read-only (source at `ed4a41983`, vendored `_tsc.js`, and one TS probe). No edits or builds.

## TS contracts: the two callers differ

**Function-body caller** (`printer.rs:8217`, used only when `function_body`):
- TS `shouldEmitBlockFunctionBodyOnSingleLine` (`_tsc.js:118999-119019`) calls `rangeIsOnSingleLine(body)`. That is `rangeStartIsOnSameLineAsRangeEnd(body, body)` (17321, 17344).
- **Start:** `getStartPositionOfRange(range, sf, includeComments = false)`, which is `skipTrivia(text, pos, stopAfterLineBreak = false, stopAtComments = false)`. It skips whitespace, line breaks **and comments** to the `{` (17367-17374).
- **End:** `range.end`, the full end after `}`, not `end - 1`.
- **Comparison:** `positionsAreOnSameLine` → `getLinesBetweenPositions` over the **line map** (8318). The line map counts CR, LF, CRLF (once), U+2028 and U+2029.

**JSX caller** (`printer.rs:3655`):
- TS `emitJsxExpression` (119451) uses `getLineAndCharacterOfPosition(node.pos).line !== …(node.end).line`.
- That is **raw `pos`, no trivia skip**, compared on the same line map.

**Native** (8885-8906) scans raw `text[pos..end]` for ASCII `\r`/`\n` only. That gives two defects:
1. **U+2028/U+2029 are not counted.** These are the 16 measured failures.
2. **For the function-body caller, leading trivia is counted.** TS starts at the `{` after `skipTrivia`.

   Probe (vendored TS, ES2015): `function f()\n{ return 1; }`, `function f() /*c*/\n{ return 1; }` and `function f()\u2028{ return 1; }` all print **`function f() { return 1; }`**. The native raw scan sees the newline before `{` and chooses multi-line.

   So Root's proposal (line map on the raw endpoints) would fix the Unicode shapes but leave this case wrong, and it would make it wrong for U+2028 too.

## Smallest correct implementation

Keep the original-node resolution and the synthesized fallback (`Ok(true)`) exactly as they are. Give the helper the start rule as an explicit choice at its two call sites:
- **JSX (3655): raw start.** `start = range.start()`.
- **Function body (8217): trivia-skipped start.** `start = skip_trivia(source.text(), range.start())`. Native `skip_trivia` skips comments, matching `stopAtComments = false`.
- **Both:** compare lines with the existing position index, `line_and_character_byte(start).line == line_and_character_byte(range.end()).line`, using the validated byte endpoints. No text scan.

**How to pass the choice:** a two-variant private enum, or two tiny wrappers around one body. Don't add a boolean parameter at the call sites. That's the whole change: no collector or other predicate change, and no coordinate compensation.

## Controls (JS; removeComments false and true; ES2015 and ES5; with maps)

**Function-body caller, Unicode inside the body (the failing class):**
1. `function f() { return 1;\u2028}` (TS: multi-line)
2. The same with U+2029.
3. `function f() { x; /* a */\u2028/* b */\u2028}` (the measured shapes).

**Function-body caller, leading-trivia boundary (TS: single line):**

4. `function f()\n{ return 1; }`
5. `function f() /*c*/\n{ return 1; }`
6. `function f()\u2028{ return 1; }`

**Function-body caller, ASCII line breaks and a single-line control:**

7. `function f() { return 1;\r\n}` (CRLF counts once; TS: multi-line)
8. `function f() { return 1;\r}` (CR only)
9. `function f() { return 1; }` (single-line control)

**Other function-body routes:**

10. An arrow block: `var g = () => { return 1;\u2028};` (TS: multi-line)
11. A method: `class C { m() { return 1;\u2028} }` (TS: multi-line)
12. A getter body with U+2028.

**JSX caller** (`.tsx`, `jsx: preserve`):

13. `<div>{/* c */}</div>`
14. `<div>{\n/* c */}</div>`
15. `<div>{\u2028/* c */}</div>`
16. One with a multi-line block comment inside the braces, e.g. `<div>{\u2028/* a\n   b */}</div>`, so the multi-line indentation decision becomes visible in the output.

## Architecture and lifecycle

- **No blocking issue.** `source_node_range_is_on_single_line` isn't a named owner symbol of any current architecture row.
- **Function-body layout** belongs to the printer concerns already tracked as unqualified (`E-PRINTER-G`, `E-COMMENTS-G`).
- **JSX expression layout** has no qualified owner row that names this symbol.
- **Rows:** this adds nothing beyond the 18 already `active-unqualified`, and the H2.5h dispositions are unaffected. Qualification still comes only from the full control run and the regressions.
