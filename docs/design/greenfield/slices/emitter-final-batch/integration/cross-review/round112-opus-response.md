Your hypothesis is confirmed. After the child's comment phase, TS's `from`, `as` and `while` tokens print only comments that follow a line break. None of them re-emits a same-line comment at the child's original end. The scoped fix is to switch those four post-child token calls to the source-leading policy, keeping the do-while block path separate from the non-block path.

## Probe (vendored 6.0.3, `ts.createPrinter({removeComments:false}).printFile`, `/tmp/r80/v.mjs`)

**Source:**
```ts
import { foo /*p*/ as bar /*ib*/ } /*c*/ from /*s*/ "m";
export { a /*q*/ as b /*eb*/ } /*e*/ from "m";
do { x; /*db*/ } /*d*/ while (y);
```

**Metadata applied to one child at a time:**
- `setEmitFlags(child, NoTrailingComments)`
- `setCommentRange(child, { pos: child.pos, end: <inner end> })`

The inner end is `bar`'s end, `b`'s end, or `x;`'s end.

**Baseline output:**
```
import { foo /*p*/ as bar /*ib*/ } /*c*/ from /*s*/ "m";
export { a /*q*/ as b /*eb*/ } /*e*/ from "m";
do {
    x; /*db*/
} /*d*/ while (y);
```

| Child | Metadata | TS output (affected line) | What happened to the original same-line comment |
|---|---|---|---|
| import clause | NoTrailingComments | `import { foo /*p*/ as bar /*ib*/ } from /*s*/ "m";` | `/*c*/` dropped |
| import clause | commentRange end = end of `bar` | `import { foo /*p*/ as bar /*ib*/ } /*c*/ /*ib*/ from /*s*/ "m";` | `/*c*/` still printed, but by the inner `NamedImports` phase (its end equals the original end, and the override breaks the container match). The clause's phase adds `/*ib*/`. `from` adds nothing |
| named exports | NoTrailingComments | `export { a /*q*/ as b /*eb*/ } from "m";` | `/*e*/` dropped |
| named exports | commentRange end = end of `b` | `export { a /*q*/ as b } /*eb*/ from "m";` | `/*e*/` dropped. `/*eb*/` moves outside |
| renamed property `foo` | NoTrailingComments | `import { foo as bar /*ib*/ } /*c*/ from /*s*/ "m";` | `/*p*/` dropped |
| renamed property `foo` | commentRange end = end of `as` | `import { foo as bar /*ib*/ } …` | `/*p*/` dropped |
| do body | NoTrailingComments | `} while (y);` | `/*d*/` dropped |
| do body | commentRange end = end of `x;` | `x;` newline `} /*db*/ while (y);` | `/*d*/` dropped. `/*db*/` moves after `}` |

So in TS the child's own trailing phase (flags plus comment range) is the only thing that emits same-line comments at the child boundary.

## The native mismatch (from source; confirm with native fixtures)

- `emit_trailing_comments_for_node_as_token_anchor` returns a resume point **only** when the child's comment range ends exactly at the token cursor (the child's original end).
- With **NoTrailingComments**, the helper returns a plain cursor. The following `emit_space_prefixed_token_with_comments` uses `BoundaryUnion` and, having no resume point, re-emits `/*c*/`, `/*e*/`, `/*p*/` or `/*d*/` from the original end.
- With a **comment-range override**, the helper emits at the override end. Because the ends differ there is again no resume point, and the token emits the original-end comments as well.

In every row above native would print one extra comment.

## Smallest change

1. **Import `from` (`~3872`), export `from` (`~4205`), specifier `as` (`~13287`).** Replace `emit_space_prefixed_token_with_comments(…, anchor, false, writer)` with `emit_source_leading_token_with_context(…, anchor, TokenLeadingSpace::Required, expression_context /* the context in scope at that site */, writer)`.
   - Keep the preceding helper call and the explicit `write_space`. `Required` goes through `ensure_token_leading_space`, which adds no second space.
   - This is the same pattern the r104 `=` change used.
   - As a side effect it also respects nested suppression.
2. **`while` in the do-while.** `emit_while_clause` is shared by `WhileStatement` (anchor at the node start, so no comment phase runs) and both do-while branches. Pass the leading policy in as a parameter:
   - the **block** branch, which calls the helper first, uses `SourceLeading`;
   - the **non-block** branch and `WhileStatement` keep `BoundaryUnion` unchanged. I can't show from source whether the native non-block statement emits its own trailing phase. If it doesn't, `BoundaryUnion` is what currently prints `do x(); /*t*/` and removing it would drop the comment.
3. **Leave the rest unchanged:**
   - the shared spacing and comment helpers;
   - the export site's no-`from` branch;
   - `for_child(NORMAL)`.

## Controls to add (native fixture contracts)

1. **The eight metadata rows above, as expected outputs.** The source is the one shown. Put NoTrailingComments or the override on exactly one child, using the same node selection as the probe:
   - `statements[0].importClause`
   - `statements[1].exportClause`
   - `…elements[0].propertyName`
   - `statements[2].statement`
2. **Baseline and the ordinary r109 neighbours**, to show nothing changes when there is no metadata.
3. **Non-block do with a same-line trailing comment**, to decide whether step 2 can later drop `BoundaryUnion` there too:
   - `do x(); /*t*/\nwhile (y);`
   - `do x(); /*t*/ while (y);`

   Observe TS first.

One caveat for the import clause override row. The TS result depends on the inner `NamedImports` phase printing `/*c*/`, because the override breaks the container match. That part is outside the token change. If native prints it differently, record it as a separate cause rather than folding it into this fix.
