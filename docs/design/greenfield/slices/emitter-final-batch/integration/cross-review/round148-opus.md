# Round 148: one real admission loophole, mirrored in the proof; everything else checks out

I reviewed `000716e3b..b002fbdac`, then `e50719c50` (Root committed the follow-ups during the review), and the three proof scripts. No edits or builds.

## 1. The loophole: the parser records the action even when TS1260 is deduplicated

**Mini input:** `var x = 1 \u0069f (x) {}`

**TypeScript** reports only `1005@10+7` (';' expected). The TS1260 for the escaped `if` has the same start, so TS deduplicates it. TS then emits `var x = 1;\nif (x) { }`.

**Native** follows the same steps:
- `push_parse_diagnostic_with_event` deduplicates on `last.start == start` (`parser.rs:~10170-10173`), so TS1260 is suppressed there too.
- `next_token` (`b002`, `parser.rs:1040-1055`) still pushes `EscapedKeywordConsumed{start:10, length:7}`.
- The escaped `if` token and the 1005 report have the same span. So the new context-profile branch (`recovery.rs:~290-292`, exact start and length) admits the **1005** report as though it were an escaped-keyword report.

The output may happen to be right here, but the admission rests on the wrong fact. Any report-only Parser error at the current escaped-keyword token that is reported just before consumption is admitted the same way.

**Owner:** `Parser::next_token`.

**Minimal fix (no diagnostic code involved):**
- Record the action only if the report was actually retained. Take `let before = self.parse_diagnostics.len();` before `parse_error_at` and push the action only if the length increased. Alternatively, use the index-returning `push_parse_diagnostic_with_index` path.
- A suppressed attempt then leaves only its index-less event. That event goes through the existing `None` branch, which depends on the admission of whichever report was retained first.
- **Control:** `var x = 1 \u0069f (x) {}` must stay exactly as admitted or refused as it was before the action existed.

**The proof mirrors the hole.** `keyword_extension` asserts `matching > 0`, where `matching` counts any Parser event with the same start and length. In the input above, both the 1005 event and the suppressed attempt match. Strengthen the proof:
- **(a)** Each action matches exactly one event that has `diagnostic_index = Some(i)` and `parse_diagnostics[i].code == 1260`. A verifier checking the code is fine; admission must not depend on it.
- **(b)** Per input, the number of actions equals the number of retained TS1260 diagnostics, in both directions. That catches spurious actions and silently lost admissions, for example through the `recovery_statement_start.unwrap_or(token_start)` fallback plus the top-level-await retention filter.
- **(c)** Replace `seen[key] <= matching_report_events` with "at most one per span".

## 2. What is correct as implemented

- **Partition before the gate** (`recovery.rs:~228-241`). The non-context profiles get exactly the old structural action list, including the `is_empty()` fast path. `is_structural` is exhaustive.
- **`intersects`/`owner_start` for the new variant.** Incremental reuse only gets more conservative, which affects performance, not results.
- **Census serialization** adds an explicit `escaped-keyword-consumed` kind.
- **Rule B** (`e50719c50`, `recovery.rs:866-883`):
  - The terminator case is enabled only through `allow_context_recovery`, and 289 is the only call site, so the five older profiles are unchanged.
  - The ASI case is equivalent to before, because `expression.end == end` together with `node.end == end`.
  - `skip_trivia` means `1 /*c*/ ;` is accepted, with the comment retained. A token between the expression and `;` fails, as intended.
- **Checker** (`annotate.rs:10774-10779`): exactly the r147 fix. `node_is_missing(None)` keeps the old `None` answer.
- **`function_body_is_missing` and its six arms:** these match the TS semantics (FunctionDeclaration → NotEmitted, FunctionExpression → `create_omitted_expression`, Constructor/Method → `None`, accessors only when also abstract). The flag gates are unchanged.
  - Still to verify with controls: the untyped constructor case (TS's `visitConstructor` has no flag check) and the OmittedExpression printer output (`var f = ;`).
- **DecoratorPolicy.**
  - The Allow set matches TS: Parameter, Property, Method, both accessors, Class.
  - Items are filtered before indexing, `record_list_element_position` and newline writing.
  - PropertySignature and MethodSignature are left as `Allow`. No production path differs: a TS probe shows the parser turns `@dec` in an interface or type literal into a `MissingDeclaration`, never a decorated signature, and the declaration transform strips decorators. The only mismatch would be a synthetic fixture: TS `emitModifierList` prints an inline `@dec`, while native `Allow` prints a newline. That isn't a production coupling, so leave it.
- **Triple-slash `write_source_comment`**: correct; it restores the mapped writer.

## 3. The proof-script extension otherwise

- The legacy digest is rebuilt from the exact `Debug` fields. `assert_eq!(legacy, format!("{recovery:?}"))` on inputs without the new facts self-validates the reconstruction format, which is good.
- The allowed probe-file changes are limited to the three replay scripts, and `digest_code_sha256` stays pinned.
- The raw-versus-legacy accounting check `(raw != legacy) == bool(actions)` is sound.

**Still to add, as you planned:**
- A comparison against the previous r151 replay requiring the five non-context profiles to be **exactly equal** for every input. The partition promises identical inputs, so "no true→false" is too weak.
- The context profile may only go false→true.
- Classify every new admission into one of three classes and run a complete native command against TS for each:
  - has a keyword action;
  - has no action but uses the Rule-B terminator tie;
  - other (must be empty).
