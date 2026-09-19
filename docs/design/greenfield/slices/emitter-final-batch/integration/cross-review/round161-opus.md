The cleanup itself leaves no hidden references in code. But the batch changes behaviour inside owner symbols of two more still-qualified rows, so tracking only `E-COMMENTS-G` is too narrow.

# Round 161: lifecycle scope audit for the cleanup and the Unicode/clause changes

This is read-only. I checked the working diff, a code-wide grep, and the current architecture map in `recovery-next` at `3a085de53`.

## 1. The cleanup: no hidden uses in code

- A grep over `crates/` finds no remaining `TokenCommentBoundary`, `AdjacentListItem` or `emit_list_boundary_token_with_comments`.
- Removing the enum, its wrapper and its parameter, and passing the constant `OwnerEnd` five times, is behaviour-preserving. The surviving predicate `owner_record.end != token_end_raw` is exactly what `OwnerEnd` computed.
- **References outside code; leave them alone:**
  - Historical slice packets (`h2-8a-ellipsis-comment-owners.md:293`, `h2-8a-class-optional-name.md:155`, `h2-8a-class-header-token.md:263`) contain snapshot copies of the `E-COMMENTS-G` row naming `TokenCommentBoundary`.
  - Recorded patches (`h2-8a-printer-failure/records/*.patch`, `emitter-final-batch/records/patches/*`) contain old code.
  - These are historical evidence, and readiness manifests pin exact row-line hashes of some packets. Editing them would falsify lineage.
- Only the **current** map row needs its owner list corrected.

## 2. `E-COMMENTS-G`: Root's proposal is correct

- Its owner column names `crate::token_cursor::{…,TokenCommentBoundary,…}`, which is now a stale symbol. Under §1 a stale symbol makes the row unusable as a premise.
- The Unicode collector and trailing writer, and the clause ownership change, alter its "token progress / comment progress" behaviour.
- Moving it to a delegated `active-unqualified` entry is required, as a 16th tracked concern with no new ID:
  - preserve the whole former qualified row in the audit record;
  - drop `TokenCommentBoundary` from the owner list;
  - leave invariants and evidence unchanged.
- **Linearity still holds in source, pending the test:**
  - the same-line writer scans from `rest.start` over the bounded slice and stops at the first CR/LF;
  - detached discovery and the detached writer are bounded by trivia;
  - the prefix trimmer is a single forward walk.

  `position_cursor_2727_statement_work_is_linear_and_scan_free`, which the row cites, must be in the regression run.

## 3. Two more currently `active-qualified` rows are directly invalidated

Their **named owner symbols** changed observable behaviour in this batch.

| Row | Owner symbols that changed | What changed |
|---|---|---|
| `E-COMMENT-PHASES-A36` (qualified for the A6-36 profile, 2026-09-10) | `Printer::emit_modifiers`; `emit_comments_at_cursor_with_phase` | `emit_modifiers` now filters decorators by `DecoratorPolicy` before indexing and `record_list_element_position`, so the modifier list and spacing differ for 20 callers. The row's invariant ("ordinary modifier node comments precede list spacing") is about exactly that list. `emit_comments_at_cursor_with_phase` goes through the collector, whose U+2028/U+2029 semantics changed. |
| `E-COMMENT-ELLIPSIS-A37` (qualified for the A6-37 profile, 2026-09-10) | `Printer::emit_token_with_comments_at_boundary` | Its signature changed (the boundary parameter was removed). Its trailing phase calls the collector with `trailing = true` and `emit_source_trailing_comments_of_position_with_filter`, and both now continue across U+2028. The cleanup alone would be behaviour-neutral, but the collector change is not. |

- **Recommendation:** treat both exactly like `E-COMMENTS-G`: delegated `active-unqualified` entries in `architecture-validation.md`, with the former qualified row text preserved in the audit record, and invariants and evidence unchanged. That makes **18** tracked rows.
- **Why not leave them qualified?** Their profiles were measured on the old collector and the old `emit_modifiers`. Keeping them `active-qualified` would claim historical qualification for changed behaviour, which you said you don't want.
- **H2.5h dispositions:** no change. Both IDs are already in the table as `premise-unchanged`, and that generator doesn't consume lifecycle text.

## 4. Rows checked and **not** directly invalidated

| Row | Why it stays |
|---|---|
| `E-PRINTER-BASE` (qualified `6acd5d43`) | Owners are `Printer`, the substitution/notification hooks, `EmissionPlan`/`EmitContext` and parenthesizer rules. None changed. |
| `E-OBJECT-PROPERTY-A38` | Its PropertyAssignment/ShorthandPropertyAssignment arms are not among the `emit_modifiers` callers and are unchanged. |
| `E-RETAINED-PRODUCERS-A39` | Class-fields producers are untouched. |
| `E-DECORATOR-PARAMETER-PROPERTY-G`, `E-CLASS-PENDING-G`, `E-CAPTURE-CLASS-G` | Their owner symbols are unchanged. |
| `E-POSITIONS`, `E-STRINGS`, `E-ARENA`, `E-METADATA-G`, `E-HELPERS-BASE` | No owner symbol changed. |

**The constructor selectors have no owning row by symbol:** ES2015 `get_first_constructor_with_body`, TypeScriptVisitor `prepend_parameter_property_members`, legacy `constructor_handoff`, and declaration `first_constructor_with_body`. Their concerns fall under rows that are **already tracked** as unqualified: `E-METADATA-G-CLASS` (parameter-property projection and legacy decorators), `E-CAPTURE-BASE`/`E-NAMES-CLASS-G` (ES2015 class lowering), and `E-OUTPUT-FUTURE`/`E-MAPS` (declaration output). That is consistent.

## 5. Qualification path

This remains as you stated. The new controls, the linearity contract, and the full comment/metadata/map regressions run first. Then the final V full gate, then the post-merge record-only D. Per §3, include the **A6-36 and A6-37 focused profiles** in that regression, so their final records rest on current evidence.
