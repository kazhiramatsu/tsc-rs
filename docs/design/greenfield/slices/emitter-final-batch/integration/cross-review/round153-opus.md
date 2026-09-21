C has one real structural defect: early `?` returns can refuse inputs the older path admits. A and B are correct, with one regression surface to watch.

# Round 153: review of the A/B/C diff and the negative controls

I read the tracked diff from `4d09f3534` (`printer.rs`, `recovery/context.rs`) and the supporting helpers. No edits or builds.

## Fix before freezing: C's claimer must not abort the whole context solver

In `class_member_body_gap_actions` (`context.rs` +303-413), the per-action preconditions use `?`:
- `start.checked_add(length)?`
- `utf16_to_byte(start)?` and `utf16_to_byte(end)?`
- **`self.unique_skip_report(source, start, length)?`**
- `utf16_to_byte(event.full_start)?`

Any one of them returns `None` from the claimer, and so from `context_recovery_support`, for **every** ListAbort `=>` skip. That includes skips that aren't class-member gaps and that the old path handles.

The failure is concrete:
- `unique_skip_report` (219-234) needs exactly one retained Parser report with the same start **and length** that passes `is_current_token_report`.
- `supports_array_gaps`, the pre-existing path for statement gaps such as `function f(x: number) => x;`, needs only a unique retained report with the same **start**.
- So a statement-gap `=>` whose retained report has a different length, or two retained same-start reports that `supports_array_gaps` would still resolve, used to be admitted. It would now be refused. That is a context true→false regression.
- The r177-style proof would catch any corpus instance, but the code is wrong in principle.

**Fix:**
- Every per-action precondition should `continue`, i.e. leave the action unclaimed so `supports_array_gaps` still decides it.
- `owners.len() > 1` can also `continue`. Class members aren't a `supports_array_gaps` owner, so an ambiguous class gap stays refused either way, and the claimer stays monotone by construction.
- Keep `heritage.is_disjoint(&class_bodies)` as a hard `None`.

## Checked and correct in C

- **Context-only.** `supports_array_gaps` and the five earlier profiles are untouched. Claimed indices are removed from `remaining` before the shared gap check.
- **Report pairing.** The retained report comes from `unique_skip_report`, which includes the `is_current_token_report` trivia bound and `missing_node == None`. The suppressed index-less twin is admitted by the main loop's `None` arm, and the retained one by the "TokenSkipped at the same start" arm. No `recovery.rs` change is needed.
- **Boundaries.** The preceding member is function-like, `preceding.end == body.pos == body.end == full_start`, and the kind is `Block`. The following member's `pos == end_byte`, **or** `members.end == end_byte`. The latter is the close-brace case: list end = full start of `}` = the `=>` end when only trivia follows.
- **Owner uniqueness.** It reuses the ancestor-only containment check. The preceding member ends before `start` and the following member starts at `end`, so neither contains the span.

**Controls:**
- **Negatives are meaningful only if each one asserts that its triggering fact exists.** Each test should first assert that the expected `TokenSkipped` (token, span, `ListAbort`) is present, then assert refusal. Otherwise `m(x: number) = x` (the skip token is `=`), `x = 1 => 2` (property predecessor), `m() {} => x` (real body) and `=> =>` (second skip's `full_start` ≠ member end) could pass for an unrelated reason.
- **The class-expression positive doesn't prove the claimer.** Since the statement profile already admits it, add one syntax assertion that some admitted input is admitted **only** by the context profile *and* that its skip lies in class `members`. For example: `class C { m(x: number) => x; }` has statement-profile `false`, context `true`, and a skip inside members.

## A: Token canonical spelling is correct

- All Token nodes print `token_to_string`.
- `None` keeps the raw fallback when unchanged and the `UnsupportedTransformedSyntax` error when changed, exactly as before.
- Earlier arms (JSX fragments, NotEmittedTypeElement, declaration type keywords) still take precedence.
- For unescaped tokens, the raw text equals the canonical text. The output delta is confined to escaped keywords, and identifiers keep their raw escapes.

## B: detached writer matches `emitComments`, with one regression surface to watch

**What matches TS:**
- **The ported `emitComments` protocol:**
  - the intervening space is written *before* `writeComment`;
  - a filtered comment still arms the separator or writes its line;
  - the trailing separator is written at the end;
  - `write_source_comment` keeps the start/end map positions for written comments only.
- **The ported `emitNewLineBeforeLeadingComments`:** a line break between the owner start and the first comment triggers a newline, which is equivalent to TS's "different line" test.
- **Low-level helpers:**
  - `collect_source_comment_ranges(…, false)` follows `getLeadingCommentRanges`: it skips a shebang at 0, and at `pos > 0` collects only after a line break;
  - BOM is skipped as scanner single-line whitespace;
  - U+2028/U+2029 are line breaks for both the newline test and `has_trailing_new_line`;
  - native `write_space` writes even at line start, and `write_line(false)` is a no-op there, both as in TS.

**Regression surface:** `DetachedSourceCommentPolicy::All` now uses the new writer for **JavaScript** too (`only_print_js_doc_style = false`). That covers every file and node-array detached header in the corpus. The old `emit_leading_comments` model (newline if the preceding whitespace had one, not at line start) is equivalent for consecutive detached comments, but this path is hit by thousands of original inputs. Frozen qualification must include the full original JS command replay, not only the new controls.

**Keep these controls:**
1. A non-file owner: a namespace or function body starting `{\n  /* a */ /* b */\n\n  x; }`, where the block printer's line start makes the leading `write_line` a no-op.
2. A U+2028-separated header.
3. `/** a */ /* b */` + blank line in d.ts.

## Proof classifier: acceptable as classification only

The class-body-gap witness re-derives the same structural facts as production. Record it as causal classification, the third admission class. Correctness still rests on:
- the complete native command against TS for every newly selected input;
- the before-5-profiles-exact and context-monotone comparisons;
- the r177 core/raw-fact inclusion check.

Also assert in the proof that no class-gap-classified input was already context-admitted at r177, so the class really is new.
