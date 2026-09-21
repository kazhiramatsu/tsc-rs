Your causal path is right. `PartiallyEmittedExpression → ExistingListContext` is the right narrow repair, but it has one missing boundary that must land with it: **a call's callee when the call is the whole expression statement.** Without that, `(class { static {} } as any)();` would print as `_a = …, _a();`, which changes runtime behaviour. Everything here comes from source reading and Node probes against the vendored TS; I made no edits and ran no builds.

## 1. The causal path

- **Native pre-wraps inside the chain.** `class_fields/downlevel.rs:3101-3106` returns `create_parenthesized(expression)` (`:9281`), which has no range and no original.
  - `inline_sequence_placement` (`:3109-3147`) walks up through the PEE parents, reaches the VariableDeclaration and returns `RequiresParentheses`.
  - The PEEs are then rebuilt around that synthetic paren, and the declaration update reaches `apply_parenthesizer_rules` (`factory.rs:5664`) → `parenthesize_initializer_for_disallowed_comma`.
  - `parenthesize_expression_for_disallowed_comma` (`:6379`) skips the PEEs, finds a Paren with primary precedence, and adds nothing.
- **TS leaves the sequence bare.** `_tsc.js:97128` returns plain `inlineExpressions`. The parent's `createX` then puts the paren outside the PEE chain.
- **The TS tree confirms this for every position.** I dumped it after transforms for 14 positions. The paren is always outside the chain:

| Position | TS wrapper order |
|---|---|
| call argument, array element, object property | `Paren[range of outer PEE] > PEE > comma` |
| `.x`, `[0]`, callee, `new` callee, tag, `void`, spread | `Paren[range of outer PEE] > PEE > PEE > comma` |
| assignment, conditional | `Paren[-1,-1] > PEE > comma` (unranged) |
| template span, return | bare `PEE > comma` |

Native currently always puts the paren inside the PEEs, so maps diverge at every one of these positions, not only the variable declaration.

## 2. Does something outside the chain parenthesize, once the PEE stops the walk?

| PEE parent | Where native parenthesizes | Matches TS? |
|---|---|---|
| variable / property / parameter / binding initializer, for-of, switch/case | factory initializer rule | yes, paren ranged to the PEE |
| call / new arguments, array elements | factory comma-list rule | yes, ranged |
| `void` / `typeof` / `delete` / `await` / prefix unary | factory `parenthesize_operand_of_prefix_unary` (skips PEE, ranged) | yes, `[4,33]` |
| postfix unary, `yield` | factory rules | yes |
| binary operands (`=`, `\|\|`, …) | factory `parenthesize_binary_operand` | yes, unranged |
| conditional branches | factory `parenthesize_conditional_operands` | yes, unranged |
| `.x`, `[i]`, tag | printer `LeftSideOfAccess` context (`printer.rs:7432/7576/7268`), paren ranged to the PEE | yes |
| `new` callee | printer `NEW_CALLEE` | yes |
| spread element / spread assignment | printer `emit_spread_expression` with `DISALLOWED_COMMA` | yes, ranged |
| return, throw, if, template span, ExpressionStatement | none needed; TS leaves them bare | yes |
| **callee of a call that is the whole statement** | **none** | **no, see below** |

**The callee gap:**
- The printer's CallExpression arm (`printer.rs:~7176-7186`) replaces `LeftSideOfAccess` with `ExpressionStatement` grammar for the callee when the call is the statement.
- The grammar dispatch (`printer.rs:~14636`) then evaluates only `expression_statement_requires_parentheses`. That rule looks at the leftmost node, which is `_a`, and says no paren.
- The factory has no callee rule: `update_generic` (`downlevel.rs:9432`) → `apply_parenthesizer_rules` covers only arguments.
- TS applies `parenthesizeLeftSideOfAccess` to every callee through `createCallExpression` (`_tsc.js:20466`), separately from the statement rule (`:20489`). The probe gives `(_a = class { }, (() => { })(), _a)();`.
- Today this case is correct only because of the pre-wrap.

**What the callee boundary needs:** evaluate both rules in statement position — the statement left-edge rule and `LeftSideOfAccess`. That would give a paren ranged to the PEE, matching TS `Paren[0,28] > PEE[0,28]`. `(function(){})()` is unaffected: its callee is already a ranged Paren from `parenthesize_statement_expression`, so `LeftSideOfAccess` sees a left-hand-side expression and adds nothing.

**Why this is the right semantics:** `ExistingListContext` stops meaning "a list already holds this sequence" and starts meaning "a PEE's owner handles the grammar". That is exactly TS's ownership, and it is not tied to the class-as case.

**Why not remove the pre-wrap everywhere:** the same callee gap applies, and it would also change rows without a PEE — the ExpressionStatement, if/throw/template-span over-wraps described below. That widens this repair beyond the rows in question, so defer it.

## 3. Output changes beyond the 4 map rows

Under the proposal these positions move from over-wrapped to bare. That matches TS in my ES2015 probe, but it changes JS text, not only maps:
- **Statement `(K as any);`**: TS prints `_a = class { }, (() => { })(), _a;`. Native today pre-wraps, which should print `(_a = …);` (I read this from code; I didn't run native).
- **`if (K as any)`, `throw K as any`, template span `${K as any}`** (ES2015+): TS prints them bare; native pre-wraps today.
- Nested chains stop at the first PEE; `((K as any))` keeps its source paren; return is unchanged.
- Arrow concise body is moot: TS turns the body into a block (`{ var _a; return … }`) because of the hoisted `_a`. Your arrow control will confirm native does the same.

The same over-wrap happens today without a PEE (for example `${class{static{}}}`, which TS prints bare). That is outside this repair.

## 4. Controls to add (bounded)

Your +64 covers call argument, assignment, unary, array, return, arrow, object property and conditional. Add only these; I didn't specify target/module variants:
1. **`(K as any)();` as a statement** — required to show the callee boundary.
2. `(K as any).x;` — covers the access left side.
3. `(K as any);` as a statement — text change.
4. `` `${K as any}` `` at ES2015+ — text change.
5. `[...(K as any)]` or `{...(K as any)}` — covers spread.

The probe scripts are `/tmp/r80/k.mjs`–`n.mjs`.
