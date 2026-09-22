# Round 149: the retained-only fix and the proof are correct; one admission-side gap remains; add one new architecture row

I read the uncommitted diff on top of `e50719c50` and the proof worktree. No edits or builds.

## 1. Retained-only action (`parser.rs:1034-1062`): correct and minimal

- Native deduplicates only against the **last** retained diagnostic's start (`push_parse_diagnostic_with_event`, `parser.rs:~10170-10173`). "`parse_diagnostics.len()` increased" is therefore exactly "this TS1260 report was retained".
- The span and statement start are computed after `parse_error_at` from unchanged scanner state.
- The index-less suppressed event is unchanged, so the `None` branch still depends on whichever report was retained first.
- The new negative `var x = 1 \u0069f (x) {}` asserts three things: a single 1005, no action, and continued refusal. That is the right proof of the preserved boundary. Keeping those 8 commands out of the positive fixture is honest.

**Remaining production gap: admission matches by span, not by report identity.**

The context branch (`recovery.rs:~290-292`) admits any retained Parser event whose start and length equal an action. TS deduplicates only against the last error, so this sequence is possible in principle:
1. error A at the escaped-keyword token S;
2. an error at a different position;
3. the TS1260 at S, retained because the last error's start differs.

That leaves two retained events with the same span, and A would be admitted under the keyword fact. I have **not** built a concrete input for this, so treat it as theoretical. It can be closed cheaply either way:

| Option | Change | Effect |
|---|---|---|
| (a) | Require exactly **one** retained Parser event (`diagnostic_index.is_some()`) with that span before the branch admits | No new field |
| (b) | Record the retained diagnostic index in the action and require `event.diagnostic_index == Some(action_index)` | Explicit report identity; adds one serialized field to the not-yet-frozen variant |

Either keeps the admission tied to the parser fact, not to a diagnostic code. Add the same check to the proof: no *other* retained Parser event may share an action's span.

## 2. Rule B's new admission of `let a 1;`: legitimate, and handled honestly

- TS's tree is `let a` followed by `ExpressionStatement(1;)`. The 1005 report covers exactly the `1` token, which is the whole expression, followed only by its own `;`.
- That is exactly the class Rule B admits.
- Moving it out of the missing-variable-delimiter negative list (`recovery.rs` test 144-148) into the Rule B positives, with 8 new complete commands, is correct. It is a new admission with proof, not a weakened assertion.
- The other entries left in that negative list aren't statement-expression reports: object/array element gaps, a parameter, an enum member. Rule B can't reach them.

## 3. Proof worktree: correct, with one addition

**Correct as written:**
- `keyword_extension` requires exactly one matching event with `diagnostic_index = Some(i)` and `parse_diagnostics[i].code == 1260`. The code check sits in the verifier, not the admission.
- It also requires the action count to equal the retained TS1260 count, plus the self-validating legacy `Debug` reconstruction.
- Together these close the r148 proof hole.

**Addition:** assert that no second retained Parser event has the same span as any action. This is the proof-side mirror of the §1 gap.

**Note on `retained_statement_terminators`:** it re-implements the production predicate byte for byte (`skip_trivia` plus a single `;`). So it is *classification* evidence, not an independent proof of correctness. That is acceptable, because every class-B admission still requires its complete native command against TS. Label it that way in the record.

## 4. Architecture map: add one bounded row, `E-RECOVERY-FACTS`

**Why a new row is the minimum accurate choice:**
- **`E-SYNTAX-FACTS`** (row 238) is explicitly planned persistent scanner **token flags** for ES2015. Putting ParseRecovery under it would imply qualifying that planned representation.
- **`E-ENTRY`** is the typed no-emit/emit entries.
- **`E-PLAN-SCRIPT`** is selection, root, mode and path planning.
- **`E-METADATA-BASE`** is emitter side tables.
- None of those rows owns the fact that parser-produced recovery facts decide the emit preflight's parse-diagnostic refusal (`preflight_source` → `has_supported_emit_recovery`). This work changed that boundary materially: report-only versus structural actions, the partition, retained-only facts, profile monotonicity, and Rule B.

**Precedent:** `E-CHECKER-FACTS-BASE` is already a row for facts that the emitter consumes but another crate owns. `E-RECOVERY-FACTS` is its parser analogue.

**Row content:**
- **Owners:** `tsc_syntax::{ParseRecovery, ParseRecoveryAction::{…, EscapedKeywordConsumed}, is_structural, supports_missing_nodes}` and emitter `preflight_source`.
- **Invariants:**
  - The five non-context profiles see only structural actions.
  - The context profile alone admits report-only facts: the retained escaped-keyword report, and the terminator-owned whole-expression report.
  - Every fact is rolled back with the same checkpoint as its event.
- **Lifecycle:** delegated to the linked validation record like the other 14, so 15 rows in total. It is `active-unqualified` until V.

**Required coupling:**
- `h2-5h-a-dispositions.mjs` derives the full row inventory from the doc and refuses drift (`deriveArchitectureRows`, lines 122-139). The new row therefore needs **one reviewed table entry** in that generator, in the same pre-walk commit as the doc edit.
- Likely disposition: `premise-unchanged`, since parse-recovery admission is target-independent and outside the H2.5h owner graph. Root should confirm that choice.
- The walk re-mints the trio in any case, so this adds no extra walk. Don't instead stretch an existing row's text to cover a concern it doesn't own.
