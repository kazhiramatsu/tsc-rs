# Round 163: layout repair review and the adjacent ASCII-only predicates

This is from reading the working diff on `ed4a41983`, the printer and `_tsc.js`, plus one TS probe. No edits or builds.

## 1. The working repair (`SourceLineStart`): no blocking defect

- **JSX:** `FullStart` with the raw `pos` matches `emitJsxExpression` (119451).
- **Function body:** `TokenStart` with `skip_trivia(pos)` matches `getStartPositionOfRange(…, includeComments = false)`. Native `skip_trivia` skips comments, as `stopAtComments = false` does.
- **Comparison:** both endpoints are looked up with `positions().line_and_character_byte`, so CR, LF, CRLF, U+2028 and U+2029 all count as lines, as TS's line map does.
- **The `.expect`s are safe:**
  - `start > end || end > len` is checked first;
  - `skip_trivia` returns a character boundary;
  - `range.end()` comes from a validated `SourceRange`.
- **Unchanged:** the original-node resolution and the synthesized `Ok(true)` fallback.

## 2. Two different contracts; keep them separate

- **Comment-collection contract (CR/LF only). Intentional, already TS-exact:**
  - TS `iterateCommentRanges` starts collecting and ends trailing scans only on CR/LF; U+2028/U+2029 only set `pendingHasTrailingNewLine`.
  - Native owners: `collect_source_comment_ranges` (the `'\r' | '\n'` test at ~19093, which my grep mislabels as `synthetic_comment_will_emit_new_line`) and `strip_same_line_comment_prefix` (19237). **Don't change them.**
- **Layout contract (line map).** TS `getLinesBetweenNodes`, `rangeEndIsOnSameLineAsRangeStart`, `rangeStartPositionsAreOnSameLine`, `rangeIsOnSingleLine` and `getLinesBetweenPositions` all compare `computeLineStarts` lines, where U+2028 and U+2029 are line breaks.
  - Any native layout predicate that byte-scans for `\r`/`\n` is a real gap.
- **Deferred, as agreed:** `emit_leading_comments` (5 hits) is the ordinary delimited-leading comment writer. It is comment-layout, not a line-map predicate. Leave it until a control proves it wrong.

## 3. Five genuine layout gaps

The TS expectations come from the vendored-TS probe (ESNext). Native currently keeps each of these on one line.

| Native helper | Callers / route | TS counterpart | TS output with U+2028 |
|---|---|---|---|
| `source_nodes_start_on_same_line` (8814) | `clause_single_statement_same_line` → `emitCaseOrDefaultClauseRest` | `rangeStartPositionsAreOnSameLine` (both starts trivia-skipped) | `case 1:\u2028x++;` → `case 1:` / `x++;` on its own line |
| `source_node_end_and_node_start_same_line_comparable` (8953), via `…_are_on_same_line` | BinaryExpression before and after the operator (7968, 7982) | `getLinesBetweenNodes` → `rangeEndIsOnSameLineAsRangeStart` | `a\u2028+ b` → `a` / `+ b`; `a +\u2028b` → `a +` / `b` |
| `source_gap_has_line_break` (12893) | PropertyAccess before the dot, including `?.` (7706) | `getLinesBetweenNodes(node, expression, token)` | `a\u2028.b` → `a` / `.b`; `a\u2028?.b` → `a` / `?.b` |
| `source_node_leading_trivia_has_line_break` (12942) | PropertyAccess after the dot (7767) | `getLinesBetweenNodes(node, token, name)` | `a.\u2028b` → `a.` / `b` |
| `lines_between_optional_nodes` (9157) | ConditionalExpression `?` and `:` (5145-5163) | `getLinesBetweenNodes` | `x\u2028? 1 : 2` → `x` / `? 1 : 2`; `x ? 1\u2028: 2` → `x ? 1` / `: 2` |

**Exact local repair, the same in all five:**
- Keep each helper's endpoint selection exactly as it is. That covers the full-start versus trivia-skipped start, the `QuestionDotToken` skip in `source_gap_has_line_break`, the original-node resolution and the synthesized/incomparable fallbacks.
- Replace only the final CR/LF byte scan with `line(start) != line(end)` from `source.positions().line_and_character_byte`.
- For every gap without U+2028/U+2029 the result is identical, because a CR/LF inside `[start, end)` is exactly a line change.
- `lines_between_optional_nodes` still returns 0/1. TS returns `rangeEndIsOnSameLineAsRangeStart ? 0 : 1` because `preserveSourceNewlines` is never set.

**Not affected:** `preserved_list_siblings_need_line_break`, which did not appear among the ASCII-scan hits.

## 4. Controls

Use the probe inputs above, U+2028 and U+2029 each, JS with `sourceMap: true`, removeComments false (layout doesn't depend on comments), ES2015 and ES5:
1. **Clause:** `case 1:\u2028x++;`, plus `\n` and `\r\n` twins (TS: multi-line for all).
2. **Binary:** `a\u2028+ b` and `a +\u2028b`.
3. **Property access:** `a\u2028.b`, `a.\u2028b` and `a\u2028?.b`.
4. **Conditional:** `x\u2028? 1 : 2` and `x ? 1\u2028: 2`.
5. **Negatives:** one same-line version of each (`a + b`, `a.b`, `x ? 1 : 2`, `case 1: x++;`), which must stay unchanged.

That is about 12 shapes × the option matrix, in addition to Root's 20 layout shapes.

## 5. Architecture

- None of the five helpers is a named owner symbol of any row. The rows these routes belong to are already among the 18 tracked `active-unqualified` rows, and the H2.5h dispositions are unaffected.
- **No larger algorithm mismatch showed up.** Every gap is the same single-line predicate using the wrong line-break set, with TS's endpoint semantics otherwise already ported.
