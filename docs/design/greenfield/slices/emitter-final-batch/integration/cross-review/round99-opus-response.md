I've located the class-as drop, found no correctness blocker in the repairs (they will fail `cargo fmt --check` as written), and found no high or medium flaw in Gate-prep. Everything below comes from reading source and running Node probes against the vendored TS; I made no edits and ran no builds.

## 1. Class-as map cases: where the wrapper is lost

**The loss happens in native class-fields, not in ES2015 or the PEE factory update.** The ES5 lowering puts its own unranged paren *inside* the PEE chain. TS returns the bare comma sequence and lets the variable-declaration update add the paren *outside* the chain.

**Native code:**
- `class_fields/downlevel.rs:3101-3106` (end of `inline_class_expression`):
  ```rust
  if self.inline_sequence_placement(original)? == InlineSequencePlacement::ExistingListContext {
      Ok(expression)
  } else {
      self.create_parenthesized(expression)   // :9281 — fresh node, no range, no original
  }
  ```
- `inline_sequence_placement` (`:3109-3147`) walks up the tree:
  - a PEE continues upward;
  - a Paren, Return or Arrow parent returns `ExistingListContext`;
  - a Binary `=`/`,` parent continues upward;
  - anything else returns `RequiresParentheses`.
- For this input the walk goes PEE, PEE, then reaches the VariableDeclaration, so the result is `RequiresParentheses`.
- The native tree becomes `PEE[72,101](PEE[74,100](Paren<synthetic>(comma…)))`.
- Factory 6379's disallowed-comma check skips the PEEs, sees a Paren (primary precedence) and never fires. So the correct `set_text_range(paren, expression)` path is never reached.

**TS code:**
- `visitClassExpressionInNewClassLexicalEnvironment` returns the bare `factory2.inlineExpressions(expressions)` at `_tsc.js:97128`, with no paren.
- The two PEEs are rebuilt by `updatePartiallyEmittedExpression` (`:24365`), which keeps their ranges.
- `createVariableDeclaration` → `asInitializer` → `parenthesizeExpressionForDisallowedComma` (`:20483`) skips the PEEs, sees the comma, and wraps the **outer PEE** with `setTextRange(paren, PEE[72,101])`.

**Probe evidence** (`/tmp/r80/i.mjs`, `transformNodes` run with growing slices of the transformer list):
- After transformESDecorators the tree is still just `PEE[72,101] > PEE[74,100] > ClassExpression`.
- After transformClassFields it becomes `ParenthesizedExpression[72,101]` (no original) `> PEE[72,101] > PEE[74,100] > comma`.
- ES2015 later leaves the wrappers unchanged.

**Decoded maps match this nesting exactly:**

| Position | Expected (TS) | Actual (native) |
|---|---|---|
| col 12 | paren `(` → 1:17 | both PEE starts, last one wins → 1:18; synthetic `(` unmapped |
| col 13 | PEE starts → 1:18 | nothing |
| col 6 (line 13) | PEE end → 1:45 | synthetic `)` unmapped |
| col 7 (line 13) | paren end → 1:45 | PEE ends land here → 1:45 |

**Why only ES5 fails:**
- At ES2022 static blocks are not lowered, so line 3101 never runs.
- Without a wrapper (`const C = class {static{}}`), TS's paren takes the comma's synthetic range. That is the same bytes as native's synthetic paren.
- The two only diverge when a PEE sits between the class and the parent that adds parentheses (as, satisfies, `<T>`, `!`).

**Neighbour note:** the same inside-versus-outside ordering should affect other parents that add parentheses, not only this one. Examples are `f(class{static{}} as any)` and `x = class{static{}} as any`. Worth including in the ~288 neighbours. I have not written a patch.

## 2. Repair review (c1e699ab5, uncommitted)

**Medium, mechanical: the diff will fail `cargo fmt --check` as written.**
- `es2018.rs` import line is over 100 columns.
- `es2018.rs` `let body_location = if let …` is over 100 columns.
- `transform.rs` `generated_class_reference_name` signature is over 100 columns.
- `es2015.rs:1277-1280`: the `.and_then` / `.or_else(…) {` layout is not rustfmt style.
- Run `cargo fmt` before committing.

**(a) Yield asterisk — OK.**
- It matches TS `emitYieldExpression`, which calls `emit(node.asteriskToken)` after `emitTokenWithComment(yield)`.
- The anchor passed is the yield keyword's cursor and resume point, in the same form as the `?.` callers.
- No arrow restriction was added. Synthetic tokens stay unpositioned.

**(b) Delegated yield via `update_node(original, …)` — OK.**
- It equals `updateYieldExpression(node, node.asteriskToken, …)`.
- It shares the original asterisk child exactly as TS does.

**(c) for-await — matches `convertForOfStatementHead` (`:102241`) and `createForOfBindingStatement` (`:27296`).**
- The head statements get their source-map range from the original expression. The `from_raw` synthetic sentinel is handled, the same as in `class_fields::raw_map_range`.
- The block and array ranges are taken from the visited body only when it is a Block.
- The try is synthetic: TS sets neither range nor original on it.
- Only the first declaration is kept, and both statement and assignment are ranged to the initializer.
- Low: an `unreachable!()` and an `.expect()` are on a production path, where the crate normally returns `TransformError`.
- Neighbour note: TS first runs `transformForOfStatementWithObjectRest` (`:102196`). That pass changes `bodyLocation`/`statementsLocation` to the original statement even when the body is not a block. I found no counterpart in `es2018.rs`, so object-rest destructure neighbours may diverge. This does not block this diff.

**(d) Anonymous class reference cache — sound; no guard needed.**
- **Identity:** the writer's key comes from `generated_class_reference_owner`, which is `get_original_node` restricted to class kinds (`standard_decorators.rs:5418-5424`). The ES2015 lookup uses `get_original_node(requested)` (`es2015.rs:1260`). Both resolve to the root of the original chain, like TS `getNodeForGeneratedName`.
- **Scope:** `TransformNode` includes the source id, so bundle sources cannot collide.
- **Lifecycle:** the map lives next to `generated_binding_names` and is cleared in the same dispose block (`transform.rs` ~975). The identifiers it stores stay valid until `clear_session_metadata`.
- **Ordering:** the decorator pass runs before ES2015 for the same file, so the entry is written before it is read.
- Low: with first-writer-wins and only a `debug_assert`, a mismatched binding is silently ignored in release builds. That is acceptable.

## 3. Gate-prep (f0dd1893c pins + 9484fa718 schemas): no high or medium flaws

**Pins:**
- `FROZEN` is checked before `AUDITED`, and a failure returns before any `--fix` rewrite.
- The pair count rejects a spelling change the adjacent-pair reader would silently miss.
- `JOIN_PAIR`, `PAIR` and `PATH_CONSTANT` cannot double-count the same text.
- `h2_6c_de_promotions`' `frozen(workspace, "ratchets/…", "hex")` matches `PAIR`, 3 pairs.
- The discovery guard excludes `FROZEN` files.
- The tests cover repinning refused under `--fix`, a missing artifact, a spelling change, join and constant forms, and the frozen failure blocking fixups.

**Schemas:**
- All three are registered, and the ORDER-subsequence test passes: candidate-inputs is not a rung and is filtered out.
- `walk-preflight` `registry_surface` is now satisfied for the `h2-8a-candidates` / `h2-8a-observations` ORDER rungs.
- Top-level objects are closed.
- Constraints hold:
  - status is `candidate-inputs-only` or `typescript-reference-only`;
  - `runtime_admitted: false`;
  - disposition is `candidate-only`;
  - there are 809 cases, and `repetitions` is 2;
  - generator paths are fixed.
- The null nested `related_information` matches the frozen artifact: 88 related entries, none with a nested non-null value.

**Low only:**
- The `inputs` arrays fix the count (12 and 7) but not the paths. Provenance paths are therefore enforced only by the generators' `--check`.
- candidate-inputs has no status or generator field. It also carries no admission field, so nothing claims admission.
