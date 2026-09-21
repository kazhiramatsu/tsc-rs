I found no hole that would admit a file without a proof. There is one incorrect refusal worth fixing, one refusal that holds only by accident and should be made explicit, and a set of missing controls. Frozen history is intact, apart from one changed label string in the census. This is a code read against pinned `_tsc.js` only: no Cargo, no edits, and no runs of the 240 controls.

## Checks that hold

- **Frozen accessors.** `has_only_statement_gap_emit_recovery` is `supports_missing_nodes(true,true,true,false)`, which is the pre-G3 G2 behaviour. Every G3 addition is behind `context_support`, which is `None` unless context recovery is on.
  - The new `DecoratorAwait` action cannot change any earlier predicate. Its missing head is a zero-width Identifier whose parent is a Decorator, Call or PropertyAccess, never Await or TypeAssertion. Every historical walk refuses that, and G0–G2 also reject the non-`ListAbort` action.
  - `is_literal_only` cannot see this event because it is a Parser diagnostic.
- **Census projection.** Each profile clears the later columns, and the `context_supported → statement_gaps_supported` baseline is used correctly.
  - One label changed: `statement-gaps.json`'s `predicate` string now names `has_only_statement_gap_emit_recovery` (same meaning, new name). If historical reports are compared byte-for-byte, that field will differ. Either keep the old string or record the rename.
- **Reparse runs** (context.rs:228-285). These check non-overlap, `start < end`, an external-module non-declaration file, a unique first/last root statement, `AWAIT_CONTEXT` inside the run and not beside it, and converse coverage.
  - This matches upstream `reparseTopLevelAwait`: `canReuseNode` refuses nodes whose context flags differ, so every statement in a run is fresh and flagged.
  - Rust's loop (parser.rs:9601-9669) matches upstream line for line, including the overshoot handling.
- **Actions are closed.** `ParseRecoveryAction` has only `TokenSkipped` and `Reparsed`. Every `TokenSkipped` site is either handled (`DecoratorAwait`, or `ListAbort` via heritage or the frozen G2 gap rule) or refused. Bypassing `actions.is_empty()` at recovery.rs:204 is safe because `context_recovery_support` has already returned `Some`.
- **`DecoratorAwait`.**
  - The action is recorded before `next_token` (parser.rs:4746).
  - It requires the unique retained current-token report, whose `skip_trivia(full_start) == start` check covers comments and UTF-16 before `await`.
  - It requires `missing.position == event.full_start`, a unique zero-width head at that byte position, and non-overlapping spans.
  - Upstream treats `[` in a decorator as a computed name unless it follows `?.`, so leaving ElementAccess out of the proof is correct.
- **Comma slot.** On a missing `)`, `ParenthesizedExpression.end` equals the full start of the unconsumed token, which is `Binary.end`. So `container.end > binary.end` really does prove the closer was consumed.
- **Assertion pair.** A consumed `>` would make `type.end < T.end`, so the owner test excludes it. The suppressed event must match start, length, full start and position, and each suppressed event can be claimed only once.
- **Heritage tiling** matches the 242/243/248/249/255/256 trace. The `>` skip's retained report is the 1005 `',' expected` (the `Expression expected` at the same start is suppressed), and array `end` is the full start of `{`.
- **Diagnostic bijection** (recovery.rs:211-230) still runs for G3, because context recovery implies statement gaps.

## 1. Incorrect refusal: decorator head descent is incomplete (context.rs:64-75)

Upstream `parseDecoratorExpression` passes the missing head to `parseMemberExpressionRest(…, allowOptionalChain=true)` and then `parseCallExpressionRest`. Those can wrap the head in three node kinds that the Rust loop does not descend through:
- `NonNullExpression` (`@await! class C {}`)
- `ExpressionWithTypeArguments` (`@await<T> class C {}`)
- `TaggedTemplateExpression.tag` (`@await\`x\` class C {}`)

Each of these is refused today even though it has the same single-owner proof.

**Smallest fix:** add three arms to the `child` match — `NonNullExpression(d) => d.expression`, `ExpressionWithTypeArguments(d) => d.expression`, `TaggedTemplateExpression(d) => d.tag`. `@await?.x` is already covered, because it is a PropertyAccess with a question-dot token.

The emitted shapes for these are unknown (for example, whether `!` erasure leaves an empty array element). They need TS observations under both `experimentalDecorators` settings before promotion.

## 2. Reparse overshoot: refused only by accident

Upstream and Rust share this sequence:
1. A reparsed statement overshoots the next non-await statement.
2. `pos` is moved forward only one step, to `find…WithoutAwait(pos+1)`.
3. The loop then exits at EOF.
4. The tail `addRange(statements, old, pos)` (Rust parser.rs:9661-9665) re-appends old statements that the reparsed statement already covers, and also re-adds their old diagnostics.

Example: `export {}; let a = await /1; b; c; x/;`. In await context, `/1; b; c; x/` is a regex, so A′ runs to EOF. `pos` stops at `c`, and upstream emits `c; x / ;` a second time.

G3 refuses this today only because the duplicated last statement's end also equals EOF, which makes `ends.len() == 2`. If a second await statement follows, the overlapping `Reparsed` check refuses it instead.

**Fix:** in `supports_reparse_runs`, require root statements to be monotone (`prev.end <= next.pos`). Pin this input as a refused neighbour that is recorded as upstream-emittable, rather than dropped for being malformed; admitting it later would need a native comparison of the duplicated output. Also add the version where a second await statement follows.

## 3. Missing decisive controls (unit and TS)

- **Positive, not yet covered:**
  - `export {}; class C implements await<string> {}`, which erases entirely
  - `export {}; interface I extends await<string> {}`
  - heritage with `/*😀*/` trivia before `await` and before `>`
  - `(1, /*😀*/)` (comma slot with trivia)
  - `await <number /*😀*/, string>(1)` (assertion with trivia)
  - `@await?.x class C {}`
  - the three heads from item 1
- **Refused neighbours** (record upstream output, since upstream emits these):
  - `extends await<A>, B` and `implements A, await<B>`: comma separators are deliberately not proven
  - `export {}; 1, ;`: a comma slot outside parentheses
  - `export {}; await (1,`: closer missing
  - the overshoot inputs from item 2
- **Incremental parity:** fresh-vs-incremental equality of `has_supported_emit_recovery` and the actions, for an edit inside a reparse run, just before it, and at the heritage gap. The metadata phase checked token parity; the admission result itself has no incremental test.
- **Tamper tests** alongside the forged-witness test:
  - a `Reparsed` whose `end` is off by one statement (maximality)
  - removing the heritage skip's retained report (tiling)
  - two TypeAssertions ending at one boundary (owner uniqueness)

None of these syntax results says anything about emission. Every row admitted this way still has to pass the native comparison of the 240 control commands (twice) and the five-profile census.
