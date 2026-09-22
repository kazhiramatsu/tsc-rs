The r89 patch matches upstream on all three fronts: the bind parenthesization, factory-owned parentheses on ExpressionWithTypeArguments, and the ordinary comment pipeline for parens and the three heritage levels. I found no high issues, one medium item to witness, and three low items. You're right about the comment maps, and I withdraw my r88 claim. For the nested-paren refusals, the exact failing rule is the closer count in `report_has_retained_syntax_owner`, not the handling of the suppressed event. A predicate-only admission change leaves the running census valid; its delta can be derived honestly by a successor replay. This was read-only: `r89-emitter-candidate.diff`, the current printer/factory/recovery code, the census tree's replay and selector, and `_tsc.js`.

## 1. The r89 patch

**Correct as written:**
- **Bind access:** `create_property_access_expression(target, "bind")` produces the ranged ParenthesizedExpression exactly for an uncached chain target, and computes transform flags so later lowering visits it. The cached branch's target is a fresh left-hand-side access, so it gets no parentheses.
- **Factory rule:**
  - `parenthesize_heritage_expression` sits inside `apply_parenthesizer_rules`, and the flags-only skip mirrors `updateExpressionWithTypeArguments`, which rebuilds only when `expression` or `typeArguments` changed (:22955).
  - When the ts transform drops type arguments, `data` does change, so the rule runs, as upstream does.
  - I checked the other ExpressionWithTypeArguments producers:
    - downlevel class fields wrap `(_a = B)` with `create_parenthesized` before `update_node`; that's already a left-hand side, so there's no double paren;
    - standard decorators and the ts-transform generic update also go through `update_node`.
  - So every rebuilt ExpressionWithTypeArguments is covered, and dropping the printer's unconditional rule can't expose an invalid `extends _a = B`.
- **Printer tag:** `LeftSideOfAccessAfterSubstitution` applies the rule only to a substituted replacement, which is `pipelineEmit`'s behaviour. CommonJS substitution `B` → `mod_1.B` is a left-hand side, so no parentheses.
- **Parenthesized child:**
  - For source parens, `emit_optional_ordinary_child` with the open-token prefix gives the child a full leading+trailing phase against the paren's own container.
  - The trailing phase is suppressed exactly when `child.end == paren.end`, i.e. a missing `)`, which keeps the G2 fix.
  - Synthetic parens go through the same deferred-extent construction as before (`without_preceding_token(parent, …, ambient)`), so their behaviour is unchanged.
  - The close cursor now uses the *current* child's end: a synthesized child gives no token comments, matching upstream's `expression.end == -1`.
- **Three heritage levels:** each level is an ordinary child of its parent, so the owners are exactly upstream's:
  - child vs the ExpressionWithTypeArguments;
  - the ExpressionWithTypeArguments vs the clause;
  - the clause vs the class.
  - `{` is written directly, as upstream's `writePunctuation("{")` does, so nothing prints twice.
- **Maps:** as you say, `emit_same_line_trailing_comments` → `write_source_comment` records both comment maps. My r88 statement that the deferred path "omits maps" was wrong; the losses came from ownership (which phase ran), not from missing map emission.

**Medium: witness declaration output.** Interface and class heritage lists now run full comment phases in `.d.ts` output too. Upstream's declaration printer prints only JSDoc-style comments, and I couldn't confirm that the ordinary deferred path applies `only_print_js_doc_style` on the declaration route. Add these witnesses with `declaration: true`:
- `class C extends B /*c*/ {}` and `class C extends B /** j */ {}`;
- `interface I extends A /*c*/, B {}`;
- `class C implements I /*c*/ {}`.

**Low:**
- **List delimiter/end comments:** `emit_heritage_list`, like the old `emit_node_array` (so this is not a regression), doesn't port `emitNodeListItems`' rule: `emitLeadingCommentsOfPosition(previousSibling.end)` before a delimiter or at the list end, when `previousSibling.end != parent.end`. It only matters for multi-type lists (`.d.ts` interfaces, `implements`) with a newline before the comment. Record it, or add `interface I extends A\n/*c*/, B {}` as a known gap.
- **Duplicate-comment controls:** `class C extends B /*c*/ { m() {} }` and `class C extends B // c\n{}`. The clause-level trailing phase now owns these; confirm no first-member path also emits them.
- **Hints:** `EmitHint::Unspecified` for the clause list and `Expression` for the ExpressionWithTypeArguments child are consistent with upstream.

## 2. Nested missing parens: the exact guard

**Upstream parse** of `((x //c\n as number));`:
- both parens are [77,81] and [79,81];
- the inner paren's `parseExpected(CloseParen)` reports `1005` at `as`;
- the outer paren's identical report at the same start is suppressed by `parseErrorAtPosition`'s duplicate check;
- then `1434` at `number`, and `1128` at each `)`.

**Native path through `supports_missing_nodes` (statement-gaps profile):**
- The suppressed duplicate (`diagnostic_index: None`) is **already admitted** at recovery.rs:259-263, because a retained Parser event shares its start. It is not the blocker.
- The reported `1005` goes to `report_has_retained_syntax_owner` (:580). `is_current_token_report` holds (`skip_trivia(full_start) == start`, the start being `as`). The code then counts "closers": ParenthesizedExpressions with `node.end == full_start` and `expression.end == node.end`.
  - Both parens qualify, so `closers == 2`, and `return closers == 1` (:612-613) returns **false**.
  - The event is not admitted, and the refusal follows.
- On positions, you're right: the tie is `paren.end == full_start`, where `full_start` is `x.end` (before the trivia), while the diagnostic starts at `as`. The existing code already uses `full_start` correctly.

**A bounded change that only admits more:**
- In that branch, accept `closers > 1` only when all of these hold:
  1. The closers form one *direct* nesting chain: each outer closer's `expression` is exactly the next closer, and the innermost closer's expression is not a paren. This rules out `(<T>(x`, `((x) y`, and similar.
  2. The number of Parser `Diagnostic` events with this `start` and this `full_start` and no `missing_node` equals `closers.len()`. That's one reported event plus `closers.len() - 1` suppressed ones: one failed `parseExpected` per paren.
- Everything else is unchanged:
  - the statement-gap loop still requires every *reported* diagnostic to map one-to-one to an event;
  - the `1128` skipped-token reports still admit only through `TokenSkipped`;
  - the `1434` report still goes through the ExpressionStatement owner.
- **Emission:** the r89 paren change already yields upstream's `((x)); //c`. Both close tokens use `SourceLeading` at 81, and the statement's trailing phase prints `//c`.
- **Witnesses:**
  - a triple nesting `(((x //c\n as number)))`;
  - `((x) //c\n as number)` (inner paren closed: one closer, already handled);
  - `(<any>(x //c\n as number))`, which must stay refused;
  - a nested case with a block comment;
  - a control with a duplicated start but a mismatched event count, which must stay refused.

## 3. Census validity and deriving the delta

- **Nothing invalidates the running census.** Its per-input `profiles` (`literal` … `context_recovery`) are recorded from the 67df syntax build, and the selector asserts that the "current" replay equals that snapshot (`select-recovery-parse-corpus.py:30`). Those records stay true for the 67df predicates.
- **An honest successor delta:**
  1. Build `scripts/replay-recovery-parse.rs` (feature `current-recovery-profiles`) at the fixed syntax head.
  2. Run it over the *same* snapshot artifact. It replays every captured input with its captured parse options.
  3. Assert, for every input, that `core` digests equal the snapshot's. That proves the parse didn't change, only admission did.
  4. Report per-input `profiles` changes against the snapshot, and recompute row-level newly admitted/refused sets with the census's own aggregation rule (shared code, not re-implemented).
- **Don't overwrite the snapshot's "current".** Emit this as a separate successor record, and feed its newly admitted rows into a supplemental selection. Those rows then need the normal complete-command comparison before any emit claim.
- **Why this works:** row-level profile deltas come from separate census reports, so reuse the same aggregation path. Any core-digest difference means it wasn't a predicate-only change, and the successor replay must refuse.

## 4. H1 note

Understood: `mark_linked_references_unspecified` (`ddf4caf6d`) and the live helper/extends probes (`1e3da4587`, `0382bb3a7`) mean my r86/r87 advice (retire six, harden the absence regex) is invalid. I'll take the full semantic audit when you send it.
