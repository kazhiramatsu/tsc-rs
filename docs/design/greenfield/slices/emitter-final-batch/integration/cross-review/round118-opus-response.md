Your least-impact plan is right, and the class-member path does claim the property's range. Two corrections: the d.ts literal-initializer `=` must switch to source-leading or `/** n */` prints twice, and the optional-property neighbour also needs the `?` token's trailing phase. This is from source reading at aea09cbaf/883b5f15f plus TS probes (`/tmp/r80/aa.mjs`); I made no edits or builds.

## 1. What TS does (d.ts, ES5 CommonJS)

TS `emitPropertyDeclaration` runs, in order:
1. modifiers;
2. `emit(name)`: the name's own comment phase;
3. `emit(questionToken)`, then `emit(exclamationToken)`, each a token node with its own phase;
4. `emitTypeAnnotation`, where the type node gets its own `emitCommentsAfterNode`;
5. `emitInitializer`, where `=` gets leading comments only (`emitTokenWithComment`);
6. the semicolon.

The property's claimed range is `getCommentRange`.

| Source | TS d.ts | Property range (claim) | Key detail |
|---|---|---|---|
| `x /** n */: number /** t */ = 1;` | `x /** n */: number /** t */;` | [16,49] | name end 18 and type end 35 both ≠ 49, so both print |
| `y /** n */: number /** t */;` | `y /** n */: number /** t */;` | [16,45], **includes `;`** | type end 35 ≠ 45, so the type phase prints (see note below) |
| `z? /** n */: number /** t */ = 1;` | `z? /** n */: number /** t */;` | [16,50] | `/** n */` comes from the **`?` token's** trailing phase (name trailing stops at `?`) |
| `w! /** n */: number /** t */;` | `w: number /** t */;` | [16,46] | `!` is dropped; name trailing at 18 stops at `!`, so `/** n */` is lost in TS too |
| `static s …`, `"q" …` | `static s /** n */: …`, `"q" /** n */: …` | parsed | a string-literal name has its own trailing phase too |
| `readonly r /** n */ = 1 /** t */;` | `readonly r /** n */ = 1;` | [16,50] | printed by the name phase; `=` adds nothing; the new literal has no range |
| param prop `public p /** n */: number /** t */` | `p /** n */: number; /** t */` | synthesized, **comment range = parameter [29,54]** | type end 54 equals the claim, so the type phase is suppressed and the member's own trailing phase prints `/** t */` after `;` |
| param prop with `= 1` | `d /** n */: number /** t */;` | comment range [29,67] | 54 ≠ 67, so the type phase prints |
| `readonly e /** n */?: number /** t */` (param) | `readonly e /** n */?: number \| undefined; /** t */` | [29,57] | synthesized union type (pos -1), so no type phase |

**Correction to your assumption.** A parsed class property's range includes its `;`. So an uninitialized property's type phase is **not** retained, and TS prints the comment before `;`. Only the synthesized parameter property (whose comment range is the parameter's) retains the type end and prints it once after `;`.

## 2. Native (`printer.rs:5892`)

- **Name and `?`/`!` tokens have no trailing phase.** The name goes through `emit_required_identifier_name_with_context` → `emit_node_with_hint`, which passes no deferred comments. `emit_optional_declaration_token` goes through `emit_node_id_with_context`, also without deferred comments.
- **The type has no trailing phase.** `emit_type_annotation` writes `:`, leading comments and the node, but nothing after it.
- **The container is claimed.**
  - `emit_class` (`~9838`) emits each member with `emit_node_id_with_context`. The generic worker's ordinary branch calls `active_expression_comment_scope`, which claims the owner's comment range.
  - So the arm's `expression_context.comments()` holds the property's claim, taking the metadata comment range first.
  - The parameter property already prints `; /** t */` via `emit_trailing_comments_for_node(member_node)`. That shows its comment range ends at 54, so a type-end guard will be retained there, exactly once.

## 3. Narrow repair (d.ts only, inside the PropertyDeclaration arm)

Everything goes under `if self.options.declaration_syntax && !expression_context.nested_comments_suppressed()`, with `scope = expression_context.comments()`.

1. **Name.** Right after the name, call `emit_trailing_comments_for_node_in_container(name, scope)`.
2. **`?` token.** Right after `emit_optional_declaration_token(question_token)`, call the same helper on the question token, if there is one. This is needed for the optional neighbour. Leave `!` alone: TS's d.ts drops it, and the comment after it is lost in TS too.
3. **Type.** Right after `emit_type_annotation`, call the same helper on `data.r#type`, if present. A synthesized type has range -1 and does nothing.
4. **`=` for a d.ts literal initializer.** In `declaration_syntax` only, replace `emit_space_prefixed_token_with_comments(=, equal_cursor)` with `emit_source_leading_token_with_context(…, TokenLeadingSpace::Required, expression_context, …)`.
   - Otherwise the `BoundaryUnion` at the name end re-emits the `/** n */` that step 1 already printed, giving `readonly r /** n */ /** n */ = 1`.
   - The JS path keeps its current `=`, which is still the name/type trailing lane there.

**Out of scope, unchanged:**
- the shared `emit_type_annotation` and `emit_optional_declaration_token`;
- `Parameter`, the constructor signature (already exact) and the JS property path.

**Flags and filters:**
- `NO_TRAILING_COMMENTS` on the name, token or type node is honoured by `emit_trailing_comments_for_node`.
- `comments_disabled()` is checked inside that helper.
- d.ts uses `only_print_js_doc_style`, so plain `/* … */` comments must stay suppressed. The helper already applies that filter.

**Maps.** Comments record no mappings, so the d.ts map differences should only be the column shift after the inserted comment text. Confirm this with the after-fix capture rather than assuming it.

## 4. Pitfalls

- **Parameter-property name.** The same parse name node is printed twice: in the member and in the constructor parameter. TS does the same. The constructor parameter already matches and isn't touched.
- **Comment range versus text range.** The synthesized property must be guarded with its **comment range** (the parameter's). `emit_trailing_comments_for_node_in_container` goes through `comment_range_for_node`, which reads metadata first, so this is correct.
- **`!` / `?`.** Do not emit comments after an omitted `!`.

## 5. Neighbours

Your planned set is right: uninitialized, optional, definite, static, string-name, literal-type, and default/readonly parameter properties, each with `removeComments` false and true, the complete d.ts, and both maps. Add three:
1. **Plain-comment filter:** `x /* n */: number /* t */ = 1;`. TS's d.ts prints `x: number;` with no comments.
2. **Property without a semicolon:** `class D { y /** n */: number /** t */ }`. The property's end then equals the type end, so the type phase is retained and the class-member trailing phase prints it once.
3. **Literal initializer with a comment after `=`:** `readonly r /** n */ = /** v */ 1;`. This pins down the d.ts `=` switch in step 4.
