Of the 21 rows, 13 are now implemented or owned elsewhere and should be retired. Eight still describe a real gap, and several of those need their needle or disposition corrected. Two missing probes are absent from the inventory entirely. On the nested-paren refusals you're right: two separate guards fail, and the array-gap check fails first. This was read-only: current checker source at `f4d9ccbe1`, the pinned `_tsc.js`, the native capture `/tmp/emitter-nested-paren-r90/nested.txt`, and three small upstream `_this` probes.

## 1. The 21 checker elision rows

Upstream `checkExternalEmitHelpers` has 36 call sites; native has 25.

| # | Row | Verdict | Evidence |
|---|---|---|---|
| 1 | constructor-capture-lexical-this | **Retain, as unobservable** | Upstream constructor `captureLexicalThis` (:81579) feeds only `checkIfThisIsCapturedInEnclosingScope`. Its input list `potentialThisCollisions` is never pushed in 6.0.3 (:47436, :87010-87076), and probes of derived-class `_this` shapes give no diagnostics. Native `capture_lexical_this` exists (expr.rs:2776). Rewrite the needle as "no observable consumer in 6.0.3". |
| 2 | async-mark-linked-references | Implemented, retire | functions.rs:2291 → modules.rs:1402 (`1e3da4587`) |
| 3 | decorator-mark-linked-references | Alias part implemented, **helper part missing** | calls.rs:246 `mark_decorator_metadata_aliases`; modules.rs:1320. Missing: the `Metadata` (16) probe in `markDecoratorAliasReferenced` (:71873) and the legacy `Param` (32) probe (:82754). |
| 4 | import-equals-mark-linked-references | Implemented, retire | modules.rs:9768; the emit-time traversal is emit.rs:534 → `mark_linked_references_unspecified` (modules.rs:1220, `ddf4caf6d`) |
| 5 | export-collect-linked-aliases | Implemented, retire | modules.rs:9995, gated on declaration emit as upstream is; declaration_emit.rs:1522 |
| 6 | property-access-mark-linked-references | Implemented, retire | access.rs:2127/2174 plus the dispatcher arm at modules.rs:1291. `markPropertyAliasReferenced` only marks aliases; the "non-alias bookkeeping" comment has no upstream counterpart. |
| 7 | inaccessible-this-tracking | Owned elsewhere, retire | node_builder/tracker.rs and declaration_emit.rs:2578 (`3edc4bcd4`, `4a3f779c8`). check.rs's typeToString path has no tracker, as upstream error display uses a no-op tracker. |
| 8 | binding-object-rest-helper | Implemented, retire | statements.rs:162 |
| 9 | binding-array-downlevel-helper | Implemented, retire | statements.rs:219 |
| 10 | for-await-helper | **Missing** | `check_for_of_statement` has no probe (:83822-83829) |
| 11 | for-of-downlevel-helper | **Missing** | as row 10 (:83830) |
| 12 | signature-helper-probes | Implemented, retire | functions.rs:2057/2065/2070 (`1e3da4587`); the doc comment is stale |
| 13 | yield-helper-probes | **Missing** | `check_yield_expression` (:80456-80461). The current needle is a grammar comment, which is wrong. |
| 14 | class-extends-helper | Implemented, retire | class.rs:678 (`0382bb3a7`); the doc comment is stale |
| 15 | class-private-field-in-helper | **Missing** | `check_in_expression` (:79586-79588) |
| 16 | object-rest-helper | Implemented, retire | operators.rs:2230 |
| 17 | destructuring-helper | Implemented, retire | operators.rs:2298 |
| 18 | class-private-field-set-helper (reference-assignment tail) | **Missing; observable only at ESNext** | See the note below this table. |
| 19 | object-assign-helper | Implemented, retire | literals.rs:1256 (`599798019`) |
| 20 | template-object-helper | **Missing; reclassify** | The gate is `languageVersion < TaggedTemplates` (ES2015, :77855), so it is live at ES5. The "inactive-at-ESNext" disposition is obsolete. |
| 21 | export-assignment-collect-linked-aliases | Implemented, retire | modules.rs:10271 |

**Row 18, whether the reference-assignment tail is redundant:**
- `checkExpression(target)` runs first and reaches access.rs's Set probe, which is gated on `languageVersion < ES2022 || < ESNext || !useDefineForClassFields`.
- Helper requests are deduplicated globally, so whenever that gate is on, the tail's own request adds nothing.
- The tail is **ungated**. At target ESNext with `useDefineForClassFields` (the default there), the target probe is off and the tail alone reports, on `target.parent`.
- So the tail is necessary, but only at ESNext. Your 72 controls cover ES5/ES2015/ES2022, so add an ESNext private-destructuring control.

**Two missing probes the inventory never listed:** `Param` (32) and `Metadata` (16). Neither has a row.

## 2. Minimal probes, in upstream order

Helper requests are deduplicated per checker and the first request decides where the error lands, so order matters.

1. **`check_for_of_statement`**, after the grammar check and the container computation, and before the initializer checks:
   - If there is an `await` modifier and the container is not a static block: when the container's function flags masked with `INVALID|ASYNC` equal `ASYNC` and the target is below ES2018, probe `node` for **ForAwaitOfIncludes (16384)**.
   - Otherwise, when `downlevelIteration` is set and the target is below ES2015, probe `node` for **ForOfIncludes (256)**.
2. **`check_yield_expression`**, right after the generator check and the `isAsync` computation, only when there is an asterisk:
   - async and target below ES2018: **AsyncDelegatorIncludes (26624)**;
   - not async, target below ES2015, and `downlevelIteration`: **Values (256)**.
3. **`check_tagged_template_expression`**, after its grammar checks and before signature resolution: when the target is below ES2015, **MakeTemplateObject (262144)** on `node`.
4. **`check_in_expression`**, after the silent-never returns: when `left` is a private identifier and (the target is below ESNext or `!useDefineForClassFields`), **ClassPrivateFieldIn (2097152)** on `left`. The ES2022 clause is subsumed.
5. **Reference-assignment tail**, after the reference and assignability checks: when the target is a private-identifier property access, **ClassPrivateFieldSet (1048576)** on `parent(target)`, ungated.
6. **`check_decorators`**:
   - The **Param (32)** probe belongs **inside the legacy branch only**. Native currently merges the legacy and ES-decorator branches in one `experimental_decorators || target < ESNext` gate, so split that.
   - The **Metadata (16)** probe goes on the first decorator in the marker path (next to calls.rs:246). Keep upstream's gates:
     - `!verbatimModuleSyntax` (`canCollectSymbolAliasAccessabilityData`);
     - the ambient early return;
     - `emitDecoratorMetadata`;
     - a first decorator exists.
   - Order: after Decorate/SetFunctionName/PropKey, matching upstream `markLinkedReferences(node, Decorator)` at :82777.

## 3. The obsolete absence proof

- **The schema blocks simple deletion:** `summary.absence_proofs` is a `positive_integer`, so dropping the only proof, `linked-reference-producers`, breaks it.
- **Consumers:** nothing outside the generator refers to that ID.
- **Options:**
  1. **If the missing probes stay unimplemented in this train:** replace it with a *true* absence under the same schema. For example, a `regex-zero` proof over checker-production code with no named constant or numeric literal for the unported helper bits in any `check_external_emit_helpers` call: `EMIT_HELPER_(PARAM|METADATA|VALUES|MAKE_TEMPLATE_OBJECT|CLASS_PRIVATE_FIELD_IN|FOR_AWAIT_OF_INCLUDES|ASYNC_DELEGATOR_INCLUDES)\b`, plus a numeric-literal variant. It fails honestly once the probes land.
  2. **If the probes land in this train:** the honest result is zero absences. Change the schema from positive to non-negative (a one-line policy edit that's allowed without approval) rather than inventing a proof.

## 4. Generator repair (prep only; no mint, no pin or policy change)

- **Needles:** re-anchor the 9 protocol renames to stable struct heads. Re-anchor or rewrite the retained rows' needles to *accurate* elision statements for rows 1, 3, 10, 11, 13, 15, 18 and 20.
- **Rows:** retire the 13 implemented or owned-elsewhere rows. Add rows for the `Param` and `Metadata` probes if they stay unimplemented.
- **Errors:** collect missing, non-unique and absence-proof failures and throw once.
- **`--check-anchors`:** run everything (anchors, absence proofs, the `CompilerOptions`/catalog checks, prerequisite and elision references, `validateArtifact` on the fresh artifact) and skip only the byte comparison. Print "freshness not asserted".
- **Tests:**
  - a pure `collectAnchorFailures` export behind a main guard;
  - in-memory negative cases: one missing and one non-unique needle, aggregated into one error;
  - a positive case that spawns `--check-anchors`.
- **The artifact itself** stays stale until it is re-minted at final bytes together with its downstream cone.

## 5. Nested parens: your correction holds

Events from the native capture:

| Event | Start / full_start | Path | Result |
|---|---|---|---|
| reported 1005 | 87 / 81 | closer guard | **fails**: two parens end at 81 |
| suppressed 1005 | 87 / 81 | line-259 rule (same start as a retained event) | admitted |
| suppressed 1005 (speculative reparse, reparse start 81) | 87 / 89 | line-259 rule | admitted |
| 1434 | 90 / 96 | ExpressionStatement owner | admitted |
| 1128 | 96 / 96 | `TokenSkipped` | admitted |
| 1128 | 97 / 97 | `TokenSkipped` | admitted |

**First failure, `supports_array_gaps`:** it runs before the closer guard: at :207 for the statement profile, and inside `context_recovery_support` for the context profile.
- The skip at 96 passes, because its preceding ExpressionStatement [89,96] ends at 96.
- For the skip at 97, the preceding statement still ends at 96, which is not 97, so no owner is found and the check returns false.

**Fix for the array-gap check:** when a skip's `full_start` equals the end of an earlier skip span in the same array gap, walk back through the contiguous run and apply the `preceding.end` test to the run's first boundary. Require the same unique owner for every span in the run.

**Second failure, the closer guard:** keep the r89 chain rule. Allow more than one closer only for a direct nesting chain, with one missing-close parser event per paren at the same start and full start.

Both changes only admit more.
