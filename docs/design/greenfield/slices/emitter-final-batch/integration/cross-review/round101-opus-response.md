I found no double-paren or optional-chain problem in 663da's printer and downlevel change. The r101 object-rest plan matches upstream, with three identity and range rules pinned below. Neither answer says anything about native results; those are still running. I made no edits and ran no builds.

## 1. The class-wrapper repair in 663da

- **Downlevel change (`downlevel.rs:3124-3132`).** A PartiallyEmittedExpression now returns `ExistingListContext`, as agreed in r100.
- **No double parentheses.** Once any grammar paren is written, the inner node is printed with `expression_context.for_wrapper(..)`, which resets to `NORMAL` (`printer.rs:758`). Two cases matter:
  - *Statement-level rule wraps the whole call.* This happens when the leftmost node is an object literal or function, and the call's own callee isn't a function or arrow. The callee is then printed in `NORMAL` context, not the new statement-callee context, so it can't be wrapped a second time.
  - *Callee rule wraps only the callee.* When the callee is itself a function or arrow, the statement-level rule deliberately returns false. Then:
    - a FunctionExpression callee gets its paren from the statement rule;
    - an arrow callee gets its paren from the left-side-of-access rule;
    - a factory-made ranged Paren callee is already left-hand-side, so neither rule fires.
  - I checked `function(){}()()` against TS `parenthesizeExpressionOfExpressionStatement` (`_tsc.js:20489`). TS wraps the whole expression because the outer callee is a call; native gives the same result.
- **Callee rule only fires where TS's factory would.** The statement rule can be true for the callee only when the callee's leftmost node is an object literal or function. That leftmost node is shared with the whole call. So it fires only in the case the statement level excludes, which is exactly where TS's factory wraps the callee.
- **Optional chains are unchanged.** `optional_chain` still comes from the enclosing call's flags or question-dot, as the `LeftSideOfAccess` arm did before. `a?.b()` stays bare. A synthesized non-chain call whose callee is an optional chain gets a paren, the same as TS's `parenthesizeLeftSideOfAccess(expr, false)`.
- **Propagation is unchanged.** The new variant passes through nested call callees only, exactly as `ExpressionStatement` did before. Property access, element access, tag and `new` still force their own access or new-callee grammar.
- **No stale checks.** Outside the grammar dispatch, the only `== ExpressionStatement` checks are line 7183 (updated) and 14652 (the statement arm itself).

## 2. The r101 object-rest proposal

The plan (raw preparation before planning, a shared non-visiting binding helper, the prepared body visited once) matches `transformForOfStatementWithObjectRest` (`:102196-102240`) → `transformForAwaitOfStatement` → `convertForOfStatementHead`. TS output at ES2017 (`/tmp/r80/o.mjs`) for block, single-statement, assignment-pattern, labeled, nested and inner-rest cases pins the details below.

**Trigger.** The same test as upstream:
- `initializer.transformFlags & ContainsObjectRestOrSpread`, or
- an assignment-pattern initializer that contains object rest/spread,

checked on `skipParentheses(initializer)`. The preparation runs on the for-of inside any labels, before planning.

**Pin 1: generated temp identity and naming order**
- The preparation temp comes from `createTempVariable(undefined)`. It is not hoisted: it is declared only by the new `let` list.
- Its spelling is decided in print order, not creation order, even though TS creates it first. The block case gives:
  ```
  var _a, e_1, _b, _c; …
  for (var _d = true, x_1 = __asyncValues(x), x_1_1; …; _d = true) {
      _c = x_1_1.value; _d = false;
      let _e = _c;
      const { a } = _e, r = __rest(_e, ["a"]);
  ```
- With a non-identifier expression (`x.y`), the iterator and result come first (`_e`, `_f`) and the preparation temp is `_g`.
- Nested loops give outer `_g`/`x_1`/`x_1_1`/`_h` and inner `_j`/`y_1`/`y_1_1`/`_k`, with outer hoisted temps `_a, e_1, _b, _c, _d, e_2, _e, _f`.
- The binding finalizer must therefore name the temp at first emission, after the loop-head temps. Neither the allocation order nor a pre-numbered provisional name may decide it. I couldn't confirm from source that native's finalizer is print-ordered for this local-temp kind; the nested and `x.y` cases will show it.

**Pin 2: ranges** (all from upstream)

| Node | Range |
|---|---|
| prepared `let` list and its single declaration | original initializer |
| head binding `let _e = _c` (`createForOfBindingStatement(prepared list, value)` → `updateVariableDeclarationList(prepared list)`) | original initializer; declaration keeps the prepared declaration's range |
| first body statement (`createForOfBindingStatement(original initializer, temp)`) | original initializer; declaration updated from the original first declaration |
| assignment-pattern first body statement | assignment ranged to the pattern, printed `({ a } = _e, r = __rest(_e, ["a"]));` |
| prepared block, block body | original block |
| prepared block's statement array, block body | original statements array |
| prepared block, single-statement body | original statement |
| prepared block's statement array, single-statement body | original statement |
| `updateForOfStatement` result | original for-of's range; original chain resolves to the source node |

For-await then sees a Block in every object-rest case. So the final loop body takes the prepared ranges, including the single-statement range, unlike the non-rest single-statement case where they are synthetic. The single-statement output is identical text to the block case.

**Pin 3: visit order**
1. The expression, via the existing plan.
2. The head binding (visited).
3. The prepared block, visited once. The raw first statement is flattened for rest here, not at preparation time.

The shared helper must only create nodes. The existing `create_for_await_binding` visits (`self.visit(statement)`), so the raw part has to be split out of it.

**Labels.** `L:` stays on the inner `for`, inside the synthetic try; `continue L` is unchanged. The existing `labels` path at `es2018.rs:1214` needs no change as long as preparation happens on the inner for-of.

**One adjacent point, not a blocker.** Upstream visits the body through `visitIterationBody`. That puts temps created while flattening the body (for example a computed key, `const {[k()]: a, ...r}`) inside the loop body as `var`. If the 24 cases don't include a computed-key rest, that path is unexercised; one case would cover it.

The rest of the proposal is low-impact: the ordinary for-of visitor is unchanged, and nothing changes when the initializer has no object rest.
