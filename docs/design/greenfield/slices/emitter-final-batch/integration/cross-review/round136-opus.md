# r136 review of the r150 native controls: three failure classes

I only read code and captures in `tsc-rs-emitter-final-variable-comma-prep`. I made no edits or builds and touched nothing frozen. Everything below comes from source inspection and needs witnesses before anyone claims qualification. Class 1 and class 3 are well localised. For class 2 I can only list candidates; the cause is not yet found.

## 1. CommonJS checked block-rename: the resolver refusal

**TS anchors**
- `getGeneratedNameForNode` sets `name.original = node` (`_tsc.js:21652-21665`, line 21664).
- In TS, es2015 renames only while printing, through `substituteIdentifier` (`_tsc.js:108011-108019`). The transform tree keeps the parse name.
- When a TS module transform reaches the renamed name, the chain back to the parse name still exists, so the parse node is available to `getParseTreeNode`.

**Native gap**
- Callers of `colliding_declaration_name_substitute` (`es2015.rs:~5894-5953`):
  - the let-list visitor at ~5775;
  - `flatten_destructuring_binding_materialized` at ~5826;
  - `visit_variable_declaration` at ~5867;
  - the for-of head at `es2015.rs:~12465`.
- These callers put the renamed name into the transform tree while transforming, not while printing.
- The new node comes from `get_generated_name_for_node` → `create_generated_identifier(&binding)`, a fresh node. It gets `set_text_range(generated, name)` and the print-order mark, but **no original link**.
- Any later resolver query on it goes through `parse_tree_node` and finds no parse anchor. The required gate, `require_parse_tree_resolver_node` (`factory.rs:422-427`), then raises `ResolverNodeNotInParseTree`. That is the error text in the capture (`transform.rs:1562`).
- Five transforms in the default chain call the required gate: `builtins.rs:11180`/`15394`, `system.rs:4413`, `es2017.rs:3095` and `jsx.rs:1628`. The printed error does not say which one fired.

**Repair (producer-local)**
Inside `colliding_declaration_name_substitute`, add one line after `set_text_range`:
```rust
self.context.arena_mut()?.set_original_node(generated, Some(original))?;
```
Here `original` is the parse name it was given (`get_generated_name_for_node(original)`). Each part has a separate job:
- **original / parse identity:** resolver metadata only. It mirrors `_tsc.js:21664`. It does not change which comments are emitted.
- **text range** (already set): the source-comment and cursor range for the name. This is what the printer's `=`-cursor and trailing-comment code read.
- **generated-binding metadata** (already set): spelling and print order.

This leaves the resolver gate, `clone_node` and the collectors untouched.

**Open points**
- It is not proven that `0:37360` is this declaration name rather than a renamed reference or its parent. The witness needs to dump node `0:37360` (kind, raw pos/end, `get_original_node`) and the transform calling the resolver.
- If it turns out to be the parent declaration, the same idea applies at `visit_variable_declaration` ~5867: build the replacement declaration with `update_node`, as `update_variable_declaration_name` already does, instead of creating it fresh.
- Once the name has a parse anchor, the module transform must still skip generated names, as TS does (`!(isGeneratedIdentifier(node) && !(autoGenerate.flags & AllowNameSubstitution))`). Otherwise a nested block rename could turn into an export rewrite. The native check for that is not verified yet. The controls below cover it.

## 2. System noCheck: `\n/*c*/` before `=` goes missing

The removeComments=true variant matches TS, and the System lookup that tolerates a missing parse node (`parse_tree_resolver_node` → `None`) avoided the class-1 error. So the tree is structurally right and only comment ownership is lost. Three candidates:

- **(a) The declaration has no source-token shape.** `visit_variable_declaration` (~5867) builds a new declaration around the renamed name instead of updating the original. The printer's initializer arm then has no raw `equal_cursor` (`node_end_cursor`, `printer.rs:17315`) and no raw type or name owner, so it writes a bare `" ="`.
- **(b) The name loses its range.** System's `should_hoist_declaration_list` (`system.rs:940`) sends nested block statements through `self.visit`, and a later producer may rebuild the name node without a text range.
- **(c) The name is renamed twice.** The print-time `substitute_identifier` → `substitution_identifier_clone` (`es2015.rs:819`) copies ranges only from an `Original` raw range. The generated node carries a source range, but no parse anchor.

**Witness:** at the VariableDeclaration initializer arm, record:
- the name node's raw pos/end;
- the declaration's `node_has_source_token_shape` and `get_original_node`;
- the computed `equal_cursor`.

Run it once for System noCheck and once for the CommonJS noCheck neighbour.

**Likely repair:** if the class-1 fix alone does not restore `/*c*/`, the likely cause is (a). The repair would then be in the same producer: keep the parse declaration's identity (update, not create) at the rename site. No printer change. That is a hypothesis until the witness says otherwise.

## 3. for-of `var x = arr_1[_i]; /*a*/` (TS: `var x = arr_1[_i] /*a*/;`)

**TS anchor:** `convertForOfStatementHead` (`_tsc.js:106574`) gives two different ranges:
- list: `moveRangePos(initializer, -1)`, which keeps the end at `initializer.end`;
- statement: `moveRangeEnd(initializer, -1)`.

In `emitCommentsBeforeNode`/`AfterNode`, the statement therefore claims `containerEnd = end-1`. The list's end is different, so the list emits its trailing comment at `initializer.end`, which puts `/*a*/` before `;`. The statement's own trailing scan starts at `end-1`, which is inside the last token, so it finds nothing.

**Native gap:** `convert_for_of_statement_head` (`es2015.rs:12465-12481`) gives both the list and the statement the full `initializer` range. It encodes the -1 only as source-map flags (`NO_LEADING_SOURCE_MAP` on the list, `NO_TRAILING_SOURCE_MAP` on the statement). For comments:
- the statement's `containerEnd` equals the list's end;
- the container rule suppresses the list's trailing comments;
- the statement emits `/*a*/` after `;`.

This matches the capture exactly.

**Repair (producer-local, uses an existing typed feature):** give the statement a `CommentRange` whose range is `CommentSourceRange::StartOnly(initializer.pos)`, set via `set_comment_range`, next to the existing `NO_TRAILING_SOURCE_MAP`. This variant already exists (`metadata.rs:126-150`). It was built for "one real endpoint and one synthesized endpoint", and `class_fields.rs`/`es2015.rs:861` already use `set_comment_range` the same way. With no end:
- the statement makes no trailing claim and emits no trailing comment;
- the list's end is compared against the enclosing container instead, so `/*a*/` goes before `;`.

This avoids a byte `end-1`, which would not equal TS's UTF-16 `end-1` and could fall inside a multi-byte character. The shared collectors don't change.

**To confirm before relying on it:**
- The printer's container logic must treat a StartOnly range as "does not set containerEnd". This is TS's `skipTrailingComments = end < 0` branch.
- Apply the same fix to the non-declaration assignment branch (`es2015.rs:12496-12504`), which has the same `moveRangeEnd(initializer, -1)`. The pattern-list branch uses TS's plain initializer range and must not change.

## Minimal neighbour controls

Pin each against vendored TS. Bounded to 8 inputs × the modes shown:

1. Block-scoped rename with no comments, checked and noCheck × CommonJS and System. This is the class-1 regression and the export-rewrite guard.
2. Rename with a same-line `/*c*/` before `=`, same four modes. This is the class-2 neighbour where the newline branch is not involved.
3. Rename where a renamed reference appears inside a nested function (`captured`). This exercises the print-time `substitute_identifier` path alongside the transform-time rename.
4. Destructuring rename `let {a: x} = o` in a colliding block. This goes through `flatten_destructuring_binding_materialized` ~5826.
5. for-of with the comment on the same line (`let x /*a*/ of`), on the next line (`let x\n/*a*/ of`), and with a leading comment (`for (/*l*/ let x of`). Covers both the converted `_loop_1` form and the unconverted form, es5 CommonJS.
6. for-of with an assignment head (`for (x /*a*/ of arr)`), for the second branch.
7. for-of with a destructuring head (`let [x] /*a*/ of`). Its behaviour must not change.
8. A non-ASCII last identifier character before `/*a*/` (BMP only, e.g. `\u{61}`-style escapes as in r114). This checks that the StartOnly encoding needs no arithmetic on positions.
