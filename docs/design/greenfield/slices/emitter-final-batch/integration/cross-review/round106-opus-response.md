No blockers in any of the three. There is one small consistency point in (1) and one check to run before replay in (2). This comes from reading the current trees and the diffs; I made no edits and ran no builds.

## 1. No-initializer name trailing comments (e80dfc18f): correct, no blocker

- **Position.** The block now runs right after `emit_node_with_hint(name)` and before `!` and the type annotation. That is TS's `emit(name)`, then `emit(exclamationToken)`, then `emitTypeAnnotation` order, so the d.ts `x /** name */: number` order is fixed.
- **Nested suppression.** It checks `!initializer_context.nested_comments_suppressed()` as well as the `comments_disabled()` test inside the helper.
- **Two calls through `emit_deferred_expression_trailing_comments`** (`printer.rs:15804`). This matches TS `emitCommentsAfterNode` (its second call runs at `getTypeNode(node)`):
  - Owner flags and kind are the name's, even for the type-range call, via `..name_owner`. In TS both calls also pass the name's `emitFlags`, so type-node flags don't leak into the name.
  - It suppresses on `NO_TRAILING_COMMENTS` or `NotEmittedStatement`, as TS's `skipTrailingComments` does.
  - It skips synthesized or empty ranges, as TS's `(pos > 0 || end > 0) && pos !== end` does.
  - The guard is `deferred_container_scope(initializer_context).retains_end(end)`, which is TS's `end !== containerEnd && end !== declarationListContainerEnd`.
  - The JSDoc-only filter is honoured, so d.ts keeps only `/** */` comments.
- **Duplicates.**
  - For parsed `let x /*a*/: number /*t*/;` the type range's end equals the declaration's claimed end, so that call is suppressed and the list/terminator path prints `/*t*/` once.
  - For the hoisted ES5 generator case, neither end is claimed, so both print (`var x /*a*/ /*t*/`), as in TS.
- **Returned value.** Discarding the ownership/anchor is fine. The only thing that follows in d.ts is the type annotation, whose leading scan starts after `:`, so it can't overlap.

**One small consistency point (not a blocker).** The initializer branch reads the erased type only when `!self.options.declaration_syntax`. The new branch reads `metadata(name).type_node` with no such gate.
- In d.ts output the type is printed for real, and TS's declaration nodes carry no erased `typeNode`.
- If JS-transform name metadata survives into a declaration print on the shared arena, the new branch would print a `/** */` from the type's end before `:`.
- The frozen control `export declare let x …` is ambient: the TS transform elides it, so it never gets a `type_node` and can't expose this.
- Adding `!self.options.declaration_syntax` to the `if let Some(type_node)` condition would mirror the initializer branch and close the question. Otherwise, confirm that session metadata is cleared before declaration printing.

## 2. Corpus-replay selector and wrapper: correct, fail-closed

- **The context.rs edit (e2075bbb9) preserves semantics.** `!opt.is_some_and(|t| kind == Comma)` became `opt.is_none_or(|t| kind != Comma)`. For `None` both are true; for `Some` both are `kind != Comma`. The closure is a pure arena lookup, and the `||` chain evaluates in the same order.
- **Hashes match.** I verified the pinned pair: `e2075bbb9^` hashes to `780fbb2d…` and the current Recovery `context.rs` to `b7e382ff…`.
- **Selector.**
  - `SUCCESSOR_STYLE_SOURCE_PAIRS` accepts that path only when the before and after hashes match the pair exactly.
  - The new test rejects a reversed pair, removal, addition, and arbitrary bytes on either side.
  - `recovery.rs` and its test file are still admitted by path, as before.
  - `successor_source_changes` still returns every changed path, including context.rs, so it is recorded.
- **Wrapper.** `predicate_diff` now hashes `git diff ref HEAD -- <all changed paths>` (empty input when nothing changed). This covers the style edit. It also changes what `predicate_diff_sha256` means compared with earlier successor records: treat it as a new proof field and don't compare it with r94–r96 values.
- **Check before replay.** I couldn't resolve `67df86615` in this tree. Confirm that the frozen census's recorded `context.rs` hash is `780fbb2d…`. If it isn't, the selector fails closed with "outside reviewed predicates", so this only affects whether replay starts, not safety.
- **Unchanged.** Cargo.lock equality, full core and ordered recovery-fact equality, and the monotonic profile gates are all as before.

## 3. Post-integration dependency note: agreed

The note is correctly scoped. It is an inventory and design investigation, explicitly not permission to refactor. It keeps the current train to demonstrated defects, frames the checker–emitter link as a verification duty rather than a proven change, and proposes introducing one boundary at a time behind the frozen complete-command contracts. I'd add nothing that widens the current train.
