Found the owner. In the context profile, ListAbort actions go through `heritage_gap_actions`, and everything left must then pass `supports_array_gaps(source, true, …)`. That function only accepts parameter, declaration-list and statement arrays as owners (`recovery.rs` ~580-600). The function-declaration and object-method variants pass because their `=>` skip lands in the SourceFile statement gap. The class variants skip inside `ClassDeclaration.members`, which is not an accepted owner.

# Round 152: class-member missing-body `=>` recovery

This is from reading the source at `4d09f3534` plus Root's r179 dump. No edits or builds.

## What the dump proves

- **Class shapes (method, constructor, getter).** Each has one `TokenSkipped{EqualsGreaterThanToken, ListAbort}` and two Parser events at the `=>` span:
  - one retained (`diagnostic_index: Some`);
  - one suppressed (`None`).

  Both have `missing_node: None` and `full_start == prior member end == zero-width Block pos == end`. The next member is a `PropertyDeclaration` that begins right after the skipped token. `statement_start` is the class statement's start (0), and `reparse_start` is 0.
- **The main loop in `recovery.rs` already admits both events in the context profile.**
  - The indexed one passes through "TokenSkipped at the same start".
  - The index-less one passes through the `None` arm, which needs a retained Parser event at the same start.
  - So **no residual report handling is needed in `recovery.rs`**. The refusal comes only from `context_recovery_support`, which returns `None` because `supports_array_gaps` finds no owner.
- **Why the passing variants pass.** The function declaration and the object-literal method both abort their list back to the SourceFile statement array, so the skip sits in a statement gap. That is why they pass today, and why the shared missing-body helper is already exercised.

## Safe implementation shape: a context-only claimer, beside `heritage_gap_actions`

Do **not** add class `members` to `supports_array_gaps`. That function is shared with the statement-gap profile's early gate, so the statement profile would change.

Instead, add `class_member_body_gap_actions(source, &parents, &lists) -> Option<BTreeSet<usize>>` in `recovery/context.rs`. Call it next to `heritage_gap_actions`, and remove the actions it claims from `remaining` before `supports_array_gaps(source, true, &remaining)`.

It claims an action only if **all** of the following hold:
1. **Token and site.** The action is `TokenSkipped { token: EqualsGreaterThanToken, site: ListAbort }`.
   - Keep the token restricted. It is not what makes the rule safe, but it is the only token the evidence covers. Other tokens that can follow a missing body (`=`, `:`, …) need their own evidence.
2. **Unique report.** `self.unique_skip_report(source, start, length)` returns the single **retained** Parser event with `missing_node == None` (reuse the DecoratorAwait helper). Every other event at that span is index-less. `is_current_token_report(source, event)` holds, meaning only trivia sits between `full_start` and `start`.
3. **Unique owner.** Exactly one reachable `ClassDeclaration` or `ClassExpression` has a `members` array containing `[start_byte, end_byte)`, with no member overlapping it. Use the same "no other reachable non-ancestor node contains the span" uniqueness check as `supports_array_gaps`.
4. **Preceding member proves the gap.** The last member with `end <= start_byte`:
   - is a `MethodDeclaration`, `Constructor`, `GetAccessor` or `SetAccessor`;
   - has `body = Some(b)` with `kind(b) == Block` and `b.pos == b.end == member.end == utf16_to_byte(event.full_start)`.
5. **Following boundary.** Either the next member's `pos == end_byte`, or there is no next member and `members.end == end_byte`. The second case covers `m() => }`. Include it only if a TS fixture pins it; otherwise leave that shape refused.
6. **Claimed once.** The existing `spans` overlap check already rejects overlapping skips. Also require that no other claimer (heritage) has claimed the same index.

The AST, events, actions and the five predecessor profiles are untouched. The rule runs only inside `context_recovery_support`, which only the final profile calls.

## Coverage and controls

- **Covered naturally:**
  - constructor (same shape in the dump);
  - getter and setter;
  - `abstract get`/`abstract set` inside an abstract class (same parse shape; the TypeScriptVisitor's `missing && abstract` removes them);
  - untyped method and constructor (the parse shape doesn't depend on types; emit follows the gates already fixed in `e50719c50`);
  - class expressions.
- **Positive controls** (in addition to the recorded 64):
  1. `class C { m(x: number) => x; n() {} }`
  2. `var D = class { m(x: number) => x; };`
  3. `class C { m(x: number) /*c*/ => x; }` (trivia before the token)
  4. `class C { m(x: number) => }`, only if you adopt the close-brace variant.
- **Negative controls** (must stay refused):
  1. `class C { m(x: number) = x; }` (other token)
  2. `class C { m(x: number) => => x; }` (the second skip's `full_start` ≠ member end)
  3. `class C { x = 1 => 2 }` (preceding member has no zero-width body)
  4. `class C { m() {} => x; }` (real body)
  5. `interface I { m(): void => x }` (not a class)
  6. A class whose preceding function-like member has a zero-width body, but where another node spans the token.

## Architecture scope

- This fits inside the bounded recovery architecture. It is one more context-only structural claimer of the same kind as `heritage_gap_actions`, with no new representation, parser fact or AST change.
- **Doc update before V:** add one clause to `E-RECOVERY-FACTS`' invariant, e.g. "a skipped `=>` in a class member gap after a function-like member with a parsed zero-width body", and to its validation record entry.
- **Proof:** the fresh 16,994 replay should report this as a **third** new-admission class (class member body gap), with its own witness, alongside keyword and terminator. Every selected input still needs its complete native command against TS.
- **No CST/AST or stable-ID work is needed.** The future provenance-relation direction stays as already recorded.
