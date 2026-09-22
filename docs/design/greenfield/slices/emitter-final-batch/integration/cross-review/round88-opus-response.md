The 78 failures reduce to five emitter causes plus one parser-admission question. Your hypotheses 1 and 3 hold with corrections. Hypothesis 2 has a different cause than the r83 fix assumed, which is why that fix did nothing. Hypothesis 4 is a parenthesization defect, and 5 needs a bounded syntax-side change, not an emitter one. Native-JS statements below are inferred from map column arithmetic unless marked as captured; the TS side comes from vendor probes whose maps equal your expected maps. Read-only: I read the r82 differences file, the current printer, factory and standard-decorators code, and the pinned `_tsc.js`.

## Groups

| Group | Rows | Kind |
|---|---|---|
| standard `@(this?.x)` class/member | 24 | maps only (inferred: every column lines up) |
| heritage-optional | 8 | JS differs (inferred) |
| heritage-optional-trivia | 10 | ES5/ES2022 ×8: JS differs; ES2015 ×2: maps only (inferred) |
| heritage-trivia (original) | 4 | JS differs (**captured in r77**: the comment is dropped) |
| heritage-before-types / after-types | 4 + 4 | comment and end maps missing; comment likely dropped (inferred; capture to confirm) |
| paren-line / paren-block | 12 | JS differs (inferred: `export const z = (x);` against TS's 27-column line) |
| paren-missing-nested | 12 | typed refusal, not an emitter difference |

## 1. `@(this?.x)`: 24 rows, maps only

- **TS:** TS's output is `((this?.x).bind(this))`. The bind call's property access goes through `createPropertyAccessExpression`, which calls `parenthesizeLeftSideOfAccess`. That creates a *ParenthesizedExpression node* ranged to the chain, which maps `(` to `this` (1:2) and its end to 1:9 (member: 1:19/1:26).
- **Native:** `bind_decorator_expression` builds the `.bind` access with `create_property_access_node`, which is a raw `create_node`. The printer adds the parentheses itself with no node behind them, so those two segments are missing.
- **Your hypothesis 1:** the cause is right. The "lowering swallows `.bind`" part isn't supported: at ES2015 and ES5 the `.bind` column matches TS exactly.
- **Fix:** for the bind access only, call `factory.create_property_access_expression(source, target, bind)` (factory.rs:4063). It parenthesizes with a range and computes flags. Leave the other `create_property_access_node` callers alone, or you widen the change.

## 2. Heritage expressions: parenthesization (heritage-optional 8, plus the paren part of 8 optional-trivia rows)

**Upstream:**
- The printer (`emitExpressionWithTypeArguments`, :118536) passes `parenthesizeLeftSideOfAccess` as a *parenthesizer rule*. The printer applies rules only when **substitution changed the node**.
- Structural parentheses come from the factory: `createExpressionWithTypeArguments` (:22944) calls `parenthesizeLeftSideOfAccess(expr, false)`. That runs only when `update…` actually rebuilds the node, i.e. when the expression or the type arguments changed (:22955).

**Effect:**
- **ES2022:** `x?.y` is unchanged, so TS prints no parentheses; native prints `(x?.y)` because its printer arm parenthesizes unconditionally.
- **ES5:** es2020 lowering rebuilds the ExpressionWithTypeArguments, so TS gets a structural paren. The class transform then passes that paren as the IIFE argument: `}((x === null … x.y)));`. Native's paren exists only in the printer, so the IIFE argument loses it: `}(x === null … x.y));`.

**Fix:**
- **Factory:** in the update/reconciliation path (the `apply_parenthesizer_rules` arms), give ExpressionWithTypeArguments `parenthesize_left_side_of_access(expression)` when its expression or type arguments changed.
- **Printer:** stop the arm's unconditional left-side-of-access parenthesization.

**Neighbours:**
- `extends (x?.y)` with source parens;
- `extends a.b`, `extends f()`;
- `extends x?.y<any>`, where removing the type arguments rebuilds the node, so TS parenthesizes;
- all at ES5, ES2015 and ES2022.

## 3. Heritage comments (trivia 4, before-types 4, after-types 4, and the comment part of optional-trivia)

**TS owner of each comment** (the container rule is `end !== containerEnd`):

| Row | Owner |
|---|---|
| before-types `x /*c*/<any>` | the child `x`'s trailing phase (`x.end` ≠ the ExpressionWithTypeArguments' end) |
| heritage-trivia | the ExpressionWithTypeArguments' trailing phase (its end ≠ the heritage end) |
| after-types, optional-trivia | the heritage clause's trailing phase (heritage end ≠ class end) |

Each comes with `emitPos(commentPos)` and `emitPos(commentEnd)`.

**Why the r83 change did nothing:** this printer's generic child path doesn't run a child's trailing phase. Trailing comments are handed to the *next fixed token*, which is what `emit_trailing_comments_for_node_as_token_anchor` implements. Inside an ExpressionWithTypeArguments whose type arguments were erased, no token follows. And the class `{` token doesn't claim these boundaries. So:
- the plain child path loses the comment;
- the extent path (synthesized child, the ES2015 optional-trivia rows) prints it deferred, without comment maps.

**Fix:** port the three trailing phases explicitly, with upstream's container rule, using the existing token-anchor helper so comments get maps:
- in the ExpressionWithTypeArguments arm, emit the child's trailing comments when `child.end != wrapper.end`;
- in the heritage-clause list, emit each ExpressionWithTypeArguments' trailing comments when `wrapper.end != heritage.end`;
- in the class printer, emit the heritage clause's trailing comments when `heritage.end != class container end`, and use `SourceLeading` for `{` so nothing prints twice.

**Why ordinary heritage clauses don't regress:** for `extends B {}` all the ends are equal, so every new phase is suppressed.

## 4. Paren-line / paren-block: 12 rows, the comment is dropped

- **Cause:** it's the same printer model. f4 switched the close token to `SourceLeading`, which is correct upstream semantics. But the child's trailing comment used to be printed only by the old `BoundaryUnion` at `)`, so it now vanishes.
- **Upstream pipeline:** `pipelineEmitWithComments(paren)` sets `containerEnd = paren.end`, so the child's trailing phase prints ` /*c*/` or ` //c` whenever `child.end != paren.end`. The close token then emits only leading comments at `expression.end`.
- **Fix, in the source-paren branch:**
  - `child.end != paren.end`: use `emit_trailing_comments_for_node_as_token_anchor(child)`, then pass its anchor to the `SourceLeading` close token.
  - `child.end == paren.end` (missing `)`): skip it, which keeps the G2 fix.
  - Take both ends from the paren's own range, not from the ambient scope.
- **No global `BoundaryUnion` revert.**
- **Close cursor:** upstream uses the *current* child's `expression.end`, which is −1 for a synthesized lowered child (no token comments at all). Keep the `(object?.x\n// c\n)` witness at ES2019 and below to decide whether the synthesized case needs no cursor.

## 5. paren-missing-nested (12 refusals): extend admission in syntax, not the emitter

**TS parse of `((x //c\n as number));`:**
- both ParenthesizedExpressions are [77,81] and [79,81], ending with `x`;
- diagnostics: **one** `1005 ')' expected` at 87 (the second, at the same position, is suppressed by upstream's duplicate check), `1434` at 90, `1128` at 96 and 97;
- the output is `export const z = ((x)); //c`.

**Recovery events:** that fits your six events. The already-admitted G2 single-paren family, plus one *suppressed duplicate* missing-close event, plus one extra skipped `)`.

**Bounded change, in the statement-gap admission (syntax crate):**
- **Admit** a suppressed missing-CloseParen event only when:
  - its position equals an admitted, *reported* missing-CloseParen event;
  - and it belongs to a ParenthesizedExpression chain in which every paren's end equals its child's end, which equals that position.
- **Skipped `)` statements:** count each only under the existing statement-gap rule.
- **Precedent:** the existing "failed assertion closer + suppressed missing operand" proof in `context.rs` is the pattern.
- **Emission:** needs nothing new; section 4's paren fix suppresses both levels and the statement's trailing phase prints `//c`.
- **Witnesses:**
  - `(((x //c\n as number)))` (triple);
  - `((x) //c\n as number)` (inner paren closed);
  - a nested case with a block comment;
  - a mismatched-position control that must stay refused.
- **Keep the guard** until this source-shape proof is implemented.

## Captures that would settle the inferred items

Complete JS for:
- one heritage-before-types row;
- one heritage-after-types row;
- one ES2015 heritage-optional-trivia row;
- one paren-block row.

The `this` and heritage-optional rows are already determined by the column arithmetic.
