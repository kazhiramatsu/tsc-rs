The typed-newline failure is exactly the conflation you describe. A `VariableDeclaration`-local split (trailing phase, then the space, then the source-leading `=`) is safe and reuses an existing helper. It also absorbs the two current special cases, container-owned and empty-type. Your round 131 correction is right: I was wrong to call binding patterns excluded, because the predicate as I wrote it only checked the next name. This is from reading the Variable tree at d27af3085 and TS probes (`/tmp/r80/gg.mjs`, `hh.mjs`, `ii.mjs`); I made no edits or builds.

## 1. The `=` lane in `VariableDeclaration` (JS emit, typed or untyped)

**What TS emits** (vendored 6.0.3, ES2022):

| Source | TS output |
|---|---|
| `export let x: number\n/*c*/ = 1;` | `export let x \n/*c*/ = 1;` |
| `export let x: number /*c*/ = 1;` | `export let x /*c*/ = 1;` |
| `export let x: number //c\n = 1;` | `export let x //c\n = 1;` |
| `export let x: number /*a*/\n/*b*/ = 1;` | `export let x /*a*/ \n/*b*/ = 1;` |
| `export let x: number /*a*/ //b\n/*c*/ = 1;` | `export let x /*a*/ //b\n \n/*c*/ = 1;` |
| `export let x\n/*c*/ = 1;` (untyped, same failure) | `export let x \n/*c*/ = 1;` |
| `export let x /*c*/ = 1;` | `export let x /*c*/ = 1;` |
| `export let x //c\n = 1;` | `export let x //c\n = 1;` |
| `export let x /*a*/\n/*b*/ = 1;` | `export let x /*a*/ \n/*b*/ = 1;` |
| `export let x /*n*/: number\n/*c*/ = 1;` | `export let x /*n*/ \n/*c*/ = 1;` |

**What TS does, in order:**
1. **Trailing phase.** The name's trailing phase at `name.end`, then the erased type's phase at `typeNode.end`. Each writes a space and the comment, plus a line break after a `//` comment. Both are guarded by the container and use the name's flags.
2. **Space.** `writeSpace()`, which is unconditional, even at the start of a line.
3. **`=`.** `emitTokenWithComment(=, typeEnd ?? nameEnd)`, which emits leading comments only after a line break.

**Native today** (`printer.rs:~4750-4790`) collapses steps 1 and 3 into one `BoundaryUnion` and ensures the space only after both. Native `write_space` is also unconditional (`writer.rs:552`), and `emit_same_line_trailing_comments` (`:~19215`) already follows TS's spacing and line-break rules. So only the ordering is wrong.

**The helper to reuse.** `emit_deferred_expression_trailing_comments(Some(&DeferredExpressionSourceComments::nested(initializer_context.comments(), LeadingAndTrailing)), owner)`. The r104/r116 no-initializer branch and System already use it. It provides:
- the owner's raw or comment range, read metadata-first;
- the active-container guard, returning `RetainedByParent`;
- the `NO_TRAILING_COMMENTS` flag and comments-disabled check, returning `Suppressed`;
- `EmptySourceRange` and `NoSourceRange` results;
- the JSDoc-only filter.

**Patch shape** (JS path, `declaration_syntax == false`):
```
// existing: if erased_type { emit_trailing_comments_at_node_position(name) }  // name phase
let owner = match erased_type {
    Some(t) => ExpressionCommentPhaseOwner { range: raw(t), ..name_owner },   // type phase, name flags
    None    => name_owner,                                                    // name phase
};
if !initializer_context.nested_comments_suppressed() {
    self.emit_deferred_expression_trailing_comments(transformation, Some(&nested(initializer_context.comments())), owner, writer)?;
}
writer.write_space(" ");
let equals = self.emit_source_leading_token_with_context(
    transformation, node, FixedToken::operator(EqualsToken),
    equal_cursor /* plain cursor, no resume */, TokenLeadingSpace::Required, initializer_context, writer)?;
```
- `container_owns_equal_boundary` and `erased_type_has_no_extent` both go away. Those cases become the helper's `RetainedByParent` and `EmptySourceRange` results, followed by the same space and source-leading `=`. That matches the r104 for-await and r130 missing-type behaviour.
- **Pass a plain cursor to `=`.**
  - The source-leading phase only collects comments after a line break, and the trailing phase only emits same-line comments, so nothing can be emitted twice.
  - A resume point would risk `CommentResumeOwnerMismatch` whenever a name's comment range doesn't end at `original_node_end_cursor`.
- **`declaration_syntax`:**
  - The r118/r119 code already emits the declared type's trailing phase after `emit_type_annotation`, so don't run the helper for a declared type.
  - For a d.ts literal initializer with no type, the helper runs with `name_owner`.
  - Use the same space and source-leading `=` in both cases.

**Evidence for transformed and non-original names.** At ES5, TS keeps both the comments and the space for:
- a block-scoped rename: `var x_1 \n    /*c*/ = 2;` and `var x_1 /*a*/ \n    /*c*/ = 2;`;
- a converted loop body: `var y \n    /*c*/ = function () {…}`.

The printer substitutes the renamed identifier, and the name node keeps its source range. Native's `original_node_end_cursor` and metadata-first comment range reproduce that. A truly synthesized name (pos -1) gives `NoSourceRange` with a synthetic cursor, so no comments in either implementation.

**Controls.**
- The ten rows above, typed and untyped.
- A missing type with a newline and with same-line comments (r130).
- A for-await binding (container-owned).
- d.ts: `export const z /** n */\n/** c */ = 1;`
- A name with `NO_TRAILING_COMMENTS`, via a harness metadata contract.
- ES5 rename and converted-loop rows.
- Multiple declarations (`let a /*x*/ = 1, b\n/*y*/ = 2`).
- Re-run: r113, r116, r117 and r119, the 164 r129 controls, the emitter tests, and the 291-case h2-8c transpile set.

## 2. Missing-comma predicate (corrected)

**Your stricter version is right:**
- **both** adjacent declaration names are non-empty Identifiers, so `let {a} b = o` and `let a {b} = o` both stay refused;
- the parent is exactly a `VariableStatement`;
- a unique adjacent retained gap;
- no skipped token anywhere within the list;
- the context profile only.

**Diagnostic check.** Use the retained diagnostic's `code() == 1005` and its message text equal to `"',' expected."`. `message_text()` is already used for diagnostic identity (`lookup_or_issue_error_js`). If the message-chain API exposes the message key and arguments, prefer comparing the `_0_expected` message with args `[","]` over string text. Either way this is a conjunct on top of the structural rule, not an allowlist.

**`using` / `await using`: one concrete risk, not a blocker.**
- A missing comma creates an **uninitialized** disposable declaration. TS ES2022 lowering emits `__addDisposableResource(env_1, void 0, false)` for `a`; ESNext keeps `using a, b = f();`.
- The native lowering's handling of a missing initializer may refuse or differ.
- Parse-valid `{ using a; }` (a checker grammar error, no recovery) exercises the same lowering path. If native already matches TS on that row, the risk is covered.

**Controls to include:**
- `using a b = f()` in a block;
- `await using a b = f()` in an async function;
- `using a = f() b = f()`;
- the parse-valid `{ using a; }` row;

each at ES2022 and ESNext.
