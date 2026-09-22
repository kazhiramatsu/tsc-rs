The missing-comma pair has a sound, narrow structural admission rule, and I found no semantic blocker for it. The unicode/scanner row is not a bounded repair: closing it means admitting general scanner-level recovery. The `target: 100` row is an option/source-kind question, not recovery work. Two things remain unverified and need controls: native d.ts serialization of the uninitialized `const`, and comment placement between declarations. This is from reading the Variable tree at d27af3085, the vendored `_tsc.js` and the fixtures; I made no edits or builds.

## 1. How TS produces this shape

**`parseDelimitedList` (`_tsc.js`).** After an element, if `parseOptional(Comma)` fails:
- If `isListTerminator(kind)`, the list ends. For variable declarations, `isVariableDeclaratorListTerminator` is true when:
  - `canParseSemicolon()` holds: the token is `;`, `}` or EOF, **or there is a preceding line break**;
  - or the token is `in`/`of`, or `=>`.
- Otherwise it calls `parseExpected(Comma)` (`getExpectedCommaDiagnostic` is `undefined` for variable declarations), which reports `',' expected` (1005) **at the current token without consuming it**.
- It skips a token only if no progress was made (`startPos === getTokenFullStart()`).
- The loop then parses the next element (`number = "missing colon"`).

**Native mirror.** `parser.rs:~1630-1670` does the same:
- `parse_expected(CommaToken, …)` at `:1655`;
- a skip is recorded only on the semicolon and no-progress paths, via `record_token_skip(DelimitedSemicolon | DelimitedNoProgress)` (`:1663`, `:1667`).

**Resulting facts for `export const a number = "…"`:**
- one `Parser` event with a retained `diagnostic_index`, `missing_node: None` and length 6;
- the `full_start` is the end of `a`;
- no `TokenSkipped` action;
- a tree in which `VariableDeclarationList` contains two retained declarations.

**Why it refuses today.** It takes the report-only path in `supports_missing_nodes` (`recovery.rs:~250`). No existing owner applies:
- no skip action;
- no closing-paren chain;
- no `ExpressionStatement` ending at the report;
- `report_has_declaration_list_boundary` needs the list to *end* at the full start, but this list continues.

## 2. The admission rule

Add a predicate `report_is_missing_variable_delimiter(source, parents, event, actions)` in `recovery.rs`, next to `report_has_retained_syntax_owner` (`:~654`). OR it into the report-only `admitted` expression under `allow_context_recovery` only, so the lower profiles stay unchanged and profile monotonicity holds. Leave the parser, event producers and positions untouched.

**All of the following must hold:**
1. **The event.**
   - `kind == Diagnostic(Parser)`, `missing_node == None`, `length > 0`;
   - `diagnostic_index == Some(i)` pointing at a retained diagnostic (a deduplicated report doesn't qualify);
   - `is_current_token_report` (`:639`);
   - `parse_diagnostics[i]` is code 1005 with argument `","`.
2. **A unique adjacent pair.** In a reachable `VariableDeclarationList` L whose parent is a `VariableStatement` (via `declaration_list`), with no `USING`/`AWAIT_USING` flag, there is exactly one pair of consecutive declarations D_i, D_{i+1} such that:
   - `D_i.end == byte(event.full_start) == D_{i+1}.pos`;
   - `D_{i+1}.name` is an `Identifier`;
   - its trivia-skipped start is `event.start`, and `event.start + event.length == name.end`.
3. **No lost tokens.** No `TokenSkipped` or reparse action overlaps `[D_i.pos, D_{i+1}.end)`.
4. **Independent pairing.** Each such event pairs with its own gap. Multiple missing separators are positives because each event owns a distinct gap, not because of a count rule. Duplicate or tampered events fail pairing.

**Why this is not a TS1005 allowlist.** 1005 is used across the grammar: `')'`, `';'`, and commas in arguments, parameters, arrays and objects. The code is only an extra conjunct here. What actually admits the report is the geometry:
- the report sits exactly on the retained next declaration's name token;
- only trivia separates the two declarations;
- no token was skipped.

That shows the tree equals what TS kept, with nothing lost or reassigned.

## 3. Risks and how the rule handles them

| Area | Handling |
|---|---|
| `for`/`for-in`/`for-of` heads | Excluded: the list's parent must be a `VariableStatement`. `in`/`of` are list terminators, and for-in/for-of allow one declaration with their own lowering. |
| Binding patterns (`let a {b} = o`, `let {a} b`, `let a [b]`) | Excluded by the Identifier-name conjunct. Pattern emission and lowering are a separate question. |
| Multiple missing separators (`let a b c = 1`) | Positive, each event paired separately. `let a, b c` is also positive. |
| Newline (`let a\nb = 1`) | ASI ends the list, so there's no recovery at all. Include as a both-exact control. |
| Comments (`let a /*c*/ b = 1`) | `full_start` is before the comment, and `is_current_token_report` skips trivia. Structurally admitted, but **where TS prints `/*c*/` must be observed**. |
| `using` / `await using` | Excluded by the list flags: initializer requirements and the disposal lowering are different consumers. |
| Ambient `declare let a b;` | Structurally admitted. JS is elided; d.ts must be observed. |
| Nested initializer syntax (`let a = f(x y)`, `{x: 1 y: 2}`, `[1 2]`) | Rejected: those reports belong to argument, object or array lists, not to an adjacent declaration pair. |
| Missing type plus missing comma (`let a: = 1 b = 2`, `let a: T b`) | Each predicate consumes its own event, so the combination is positive. Needs a control. |
| Consumer: uninitialized `const` | TS JS gives `export const a, number = "…"` in ESM and `exports.number = exports.a = void 0; exports.number = "…";` in CommonJS. TS d.ts gives `export declare const a: any, number = "missing colon";`. A checked program adds checker diagnostics (1155); transpileDeclaration adds 9010. **The native d.ts `a: any` and the checked diagnostics are unverified**, so treat them as controls, not as a blocker. |

## 4. Controls

**Positive** (JS, d.ts and both maps where applicable; ESM, CommonJS, System and AMD; ES5 and ES2022; checked and noCheck; transpile and ordinary routes):
- the two originals, with `reportDiagnostics` on and off;
- `let a b = 1;`
- `let a = 1 b = 2;`
- `let a b c = 1;`
- `let a, b c;`
- `let a /*c*/ b = 1;`
- `var` and `const` variants, including `export const a number = "x";`
- namespace-internal declarations;
- `declare let a b;`
- the mixed rows above.

**Negative** (must still refuse):
- `for (let a b = 0;;)`, `for (let a b of xs)`, `for (let a b in o)`;
- `let a {b} = o`, `let {a} b = o`, `let a [b] = o`;
- `using a b = f()`, and `await using` in an async function;
- `let a = f(x y);`, `let a = {x: 1 y: 2};`, `let a = [1 2];`;
- `function f(a b) {}`;
- `let a 1;` (element abort with a skip);
- an enum member missing a comma (different message);
- tampered events: wrong start, suppressed index, duplicated event.

**Control that must match without recovery:** `let a\nb = 1`.

**Proof path:** fresh successor replay against the unchanged original snapshot and the current, projection and merge-base trees, then complete-command comparison of the selected rows.

## 5. The two other known rows

- **`text/unicode` (the `\u{1F600}` / emoji case): not a bounded emitter repair.**
  - It has 9 diagnostics and 19 events.
  - Several are scanner-origin, non-literal (1127 invalid character, 1351).
  - The recovered tree mixes a binding pattern with a numeric-literal property, a missing `TypeReference` under `AsExpression`, stray blocks and an `EmptyStatement`.
  - Admitting it means admitting scanner-level recovery plus general statement recovery. That is wider work on the scanner and malformed-input admission.
- **`target: 100` (`ScriptTarget.JSON`): not recovery work.** It is refused at emit option validation (`unsupported emit compiler option: target`). Whether a numeric JSON target maps to the latest JS target, or is a source-kind signal, is a decision about the option domain and source kind. It needs its own interface decision, as you already classified it, and doesn't belong in parser or emitter recovery.
