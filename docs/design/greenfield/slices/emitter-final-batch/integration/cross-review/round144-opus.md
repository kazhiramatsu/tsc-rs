# Round 144: runtime residuals (a), (b) and (c)

This is from reading the source plus one vendored-TypeScript Node probe. No edits or builds.

- **(a)** and **(b)** each have a confirmed, local cause with a small fix that follows TS.
- **(c)** is an inherited refusal, not a regression, and it can be admitted with one small parser fact.

## (a) `fatarrowfunctionsOptionalArgs`: the comment before `arg => 2` is emitted twice

**Confirmed mechanism:**
- The source has `// one param, no type\r\narg => 2;`. The ExpressionStatement, the ArrowFunction and the simple parameter `arg` all start at the same position.
- **TS** emits the list's first item in two phases:
  - an intervening phase, `emitTrailingCommentsOfPosition(child.pos)`, which only picks up comments on the same line;
  - the parameter's own leading phase, which `forEachLeadingCommentToEmit` skips because `pos === containerPos` (`_tsc.js:121219-121220`). The statement and the arrow claimed that position.
  - Result: one copy of the comment.
- **Native** routes the index-0 case in `emit_parameter_list_with_parentheses` (`printer.rs:~12745`) to `emit_leading_comments_for_delimited_list_start`. That helper passes `CommentEmissionScope::empty()` (15691-15703).
  - The first half, the intervening phase (`list_owned`), is correct.
  - The second half, `parent_comment_container_owned_prefix_for_owner(empty.container_pos(), …)` (15940-15976), never matches with an empty scope. The parameter's leading phase then re-emits the statement's comment.
- **Why direct fixtures pass:** they print the arrow without an enclosing statement claim, so there is no duplicate to suppress. Only a full SourceFile shows it.
- **The "intentional replay" comment is about the other half.** It covers the same-line intervening phase. Example: `x; /*c*/ arg => 2` really does print `/*c*/` twice in TS, because the previous statement's trailing phase and the list's trailing-of-position phase both emit it. The fix leaves that phase alone.

**Minimal fix:** for index 0 only, call the existing `emit_leading_comments_for_delimited_list_start_in_container(transformation, parameter, expression_context.comments(), writer)` (15722).
- There is precedent: the VariableDeclarationList head at 4598 already passes its real scope the same way.
- It changes behaviour only when the parameter's start equals the claimed container position, which is only simple arrow heads. `(arg) =>`, `async arg =>` and function parameter lists start after a token, so they are unaffected.
- **Verify first:** that `expression_context.comments()` at the ArrowFunction arm (5694-5719) actually carries the arrow's or the statement's claimed `container_pos`. A one-shot witness at the call site is enough.

**Controls** (plus the existing 147 list-flag and direct arrow controls):
1. `x;\n// c\narg => 2;` (the failing shape)
2. `x; /*c*/ arg => 2;` (the replay stays)
3. `/*c*/ arg => 2;` at position 0
4. `f(/*c*/ arg => 2);`
5. `(/*c*/ arg) => 2;`
6. `async /*c*/ arg => 2;`
7. `var f = /*c*/ arg => 2;`

Run each with removeComments false and true.

## (b) `parserharness`: the four `///<reference>` lines after the license header are dropped

**TS path:**
- The first statement, `declare var assert…`, becomes a NotEmittedStatement at position 0.
- `emitLeadingComments(0, isEmittedNode=false)` goes through `forEachLeadingCommentToEmit(0, emitTripleSlashLeadingComment)` (`_tsc.js:121123-121137`).
- The SourceFile doesn't claim `containerPos`, and there are detached comments here, so `forEachLeadingCommentWithoutDetachedComments` resumes at `detachedCommentEndPos` (121242-121250). It emits every recognized triple-slash comment after the license header.

**Native path:**
- `emit_leading_comments_for_comment_phase_owner` (`printer.rs:16248-16260`) emits triple-slash comments only if `start == 0 && resume.is_none()`.
- For the first statement, the detached-header resume makes `resume` `Some`, so it emits nothing.

**Minimal fix (no "always emit references" shortcut):** in the NotEmitted branch, when `start == 0` and `resume` is `Some`:
- apply the same owner check as the ordinary branch (16264-16278: `resume.owner_start().position() == range_start`, and the same source);
- scan `text[resume.next()..code_start]` instead of `text[start..code_start]`.

This mirrors the detached-comment skip. For a statement at position 0, TS never has `pos === containerPos`, because the SourceFile doesn't claim and wrappers are synthesized. So a resume there can only mean the detached block.

**Edge case:** TS's `forEachLeadingCommentRange` from a non-zero position doesn't collect until after the first line break. A triple-slash comment on the same line as a detached *block* comment (`/* hdr */ ///<reference …/>`) is therefore not emitted. Mirror that when scanning from the resume position.

**Controls:**
1. The parserharness shape.
2. The same without the blank line (not detached, so the current path handles it).
3. A detached block header followed by references.
4. A same-line `/* hdr */ ///<reference path="a"/>`.
5. An unrecognized `/// plain` after the header (must still be dropped).
6. An emitted first statement after the header (ordinary path, unchanged).
7. removeComments=true.

## (c) `scannerUnicodeEscapeInKeyword2`: inherited refusal, precise proof

**What TS does (probe on the vendored build):**
- file1 has 3 × TS1260 (`\u0061wait 12`, `\u0079ield 12`, `typ\u0065 notok`).
- file2 has 4 × TS1260.
- Both still emit, and the escaped keywords print in canonical spelling (`await`, `yield`, `var`; the type alias is erased).
- The native refusal matches file1's three.

**Why native refuses:**
- `Parser::next_token` (`parser.rs:1033-1045`) reports TS1260 through `parse_error_at`, with origin `Parser`, no missing node and no recovery action. It is a report-only event.
- `supports_missing_nodes` (`recovery.rs:248-271`) admits report-only Parser events only when one of these holds:
  - a `TokenSkipped` action starts at the event;
  - `report_has_retained_syntax_owner` (752-843) applies. That covers parenthesized closers, an ExpressionStatement ending flush, or a declaration-list boundary. None fits an escaped keyword.
  - a variable delimiter or context assertion applies.
- So all three events fail, and preflight (`builtins.rs:16288`) raises `ParseDiagnosticsDeferred`.

**Why this is inherited, not caused by the candidate:**
- No commit in `d891c58e7..HEAD` touches `parser.rs`, `scanner.rs` or `recovery.rs`.
- The TS1260 emission dates from the workspace promotion (`c866b04f3`).
- `ratchets/h2-candidate-dispositions.v1.json` still lists the case as `pending-source-analysis`.
- It surfaced only because the projection moved to a new merge base. Record it that way, not as hidden.

**Minimal correct admission, if you want it now:**
- **Parser side:** in `next_token`, when the TS1260 report fires, also record a parser-owned action such as `ParseRecoveryAction::EscapedKeywordConsumed { start, end }`. It must use the same commit/rollback path as `TokenSkipped`, so reports from abandoned speculative parses don't survive.
- **Recovery side:** in `recovery.rs:252-257`, admit a report-only Parser event whose `[start, start+length)` exactly equals one such action.
- **Why it's safe:** TS1260 never changes the tree in TS, because the token is consumed as the keyword it spells. The admission therefore rests on a parser fact, not on the diagnostic code.
- **Printer precondition:** keyword tokens must print by kind rather than from source text. Identifiers such as `var \u0061wait` must keep their escapes, as TS output shows.

**Controls:**
1. Both files of this case, as a complete command.
2. `\u0069f (x) {}` (a statement keyword).
3. `(\u0061sync x => x)` (speculative lookahead; the report must be rolled back or committed exactly as TS does).
4. `let \u0061sync = 1` (identifier consumption, no diagnostic).
5. `\u{0076}ar x;` (extended escape).
6. The declaration-emit counterpart of the escaped `type` alias.
