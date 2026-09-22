# add70fc29 review: no implementation defect found, and no extra existing test is worth running

I read the producer diff `d891c58e7..e868087f5` for `es2015.rs` and `printer.rs` only, and made no edits or builds.

## Implementation: no ownership error found

**es2015.rs:5947-5953.** `set_semantic_original_node(generated, name)` sets only `original` and the semantic flag. It merges no metadata (`factory.rs:885-904`), so the generated name gets no `type_node`, comment range or source-map range.

- **No self-link.** A repeated call can't link the generated name to itself: `parse_tree_identifier` returns `None` for any node that already has a `generated_binding_id` (`es2015.rs:671-677`).
- **No second substitution at print time.** The same check guards `substitute_identifier`.
- **CommonJS aliases and export rewrites.** `is_non_reference_identifier_node` (`builtins.rs:4781`) now follows the link to the parse `x`, sees it as the name of its parse VariableDeclaration, and skips `substitute_identifier`. The strict resolver query never runs.
- **Publication references unchanged.** `create_declaration_publication_reference` still returns early for generated bindings.

**printer.rs:17294-17311, `variable_initializer_pre_substitution_type`:**

| Case | Result | Why |
|---|---|---|
| Name is not a flagged stand-in | `None` | `current != name`; the existing `erased_type` path already handles it |
| First unflagged node has no metadata | `None` | loop exits; that node has no type |
| Clone of the stand-in | walk continues | `merge_from` carries the print-order flag with `|=`, so the walk steps over the clone |
| Cycle | stops | `visited` set |

The recovered type feeds only `equal_cursor`. `erased_type` and both trailing owners are unchanged, so `/*a*/` stays with the actual name, which has no type.

**es2015.rs:12485-12494 and 12519-12520 (for-of).**

- **Declaration branch:** list = `EndOnly(end)`, statement = `StartOnly(pos)`. This matches `moveRangePos(-1)` / `moveRangeEnd(-1)`.
- **Assignment branch:** statement = `StartOnly`.
- **Pattern branch:** untouched, which is correct.
- **Synthesized initializer:** if the initializer has no real position, both ranges become `Synthesized`, the same as before.

## Generated-name cache: the only naming contract that reads the new link

- **Where the link is read.** In the name-generation code, `get_original_node` appears in exactly one place: the lookup of names carried over from a failed print, `carried_node_name` (`target_bindings.rs:803-817`).
- **How it gets populated.** `printer.rs:1313-1322` records `node_names` under the original node when that differs from the node itself.
- **What changes.** For a stand-in name, the key is now the parse `x`; before, nothing was recorded. This matches TS, whose `nodeIdToGeneratedName` is keyed by the original node.
- **Why it's consistent.** es2015 keeps one binding per parse node, so any later lookup under that key finds the same binding's own spelling (`x_1`).
- **When it matters.** Only when a print fails and is re-entered.
- **Test coverage.** No existing fixture puts an ES5 rename through that path. The printer-failure and comment-carry fixtures contain no ES5 target, so no existing test is sensitive to it.

## Answer: no additional bounded test is justified

I checked the two compiler contracts whose names suggest this risk. Neither creates the link:

- **`h2_8a_system_variable_publication`:** its fixture has 36 ES5 cases but no numbered renames, so the eager rename never runs.
- **`h2_8a_decorator_next_witnesses`** (`decorator-name-owners`): its `var class_1` is a user-written identifier, not a colliding rename.

No other compiler contract or fixture I found contains a colliding ES5 rename. The one path where the added link changes behaviour is failed-print carry, analysed above as consistent, and nothing in the 485-test contracts binary exercises it. The 404 command controls, 1015 emitter tests, and selected44/project108 remain the relevant evidence. This is from reading the source, not a qualification claim.
