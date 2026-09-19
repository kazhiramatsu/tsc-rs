# Round 150: classification for the 13 unrecorded rows, plus two wording checks

I read the uncommitted doc diffs in `recovery-next`, `/tmp/emitter-architecture-missing-dispositions-r150.json`, `h2-5h-a-dispositions.mjs`, and the frozen owner graph and gap matrix. No edits, minting or walks.

## Facts that constrain the classification

- **Closed enum:** premise-unchanged, modified-requalify, activate, future-owned-fail-closed, proven-unreachable.
- **What the generator checks** (`dispositions.mjs:~140-190`):
  - every cited surface exists in `owner_graph.surface_row_assignments`;
  - every cited capability exists in the gap matrix;
  - `premise-unchanged` may cite only capabilities whose state is `exists`.
- **It does not check** that a surface's frozen row assignment includes the row citing it. Cite a surface only where the pinned H2.5h surface itself creates the coupling.
- **None of the 13 IDs appears** in any frozen surface's `architecture_rows`. All 13 gap-matrix capabilities are `exists`.
- **All 13 rows postdate the frozen H2.5h owner census.** The manifest records applicability to the historical H2.5h target, not current qualification. Precedent: `E-OUTPUT-FUTURE` and `E-PLAN-FUTURE` are active today but stay `future-owned-fail-closed` in this manifest.

## Ready-to-review `ROW_TABLE` entries

```js
["E-DECL-ACTIVITY", "future-owned-fail-closed", "H2.7c declaration request accounting; H2.5h JavaScript ES2015/Generators lowering never enters declaration activity counting; current behavior is qualified by its own row", {}],
["E-DECL-PATH", "future-owned-fail-closed", "declaration callback path derivation is H2.7c-owned output planning; H2.5h does not reach declaration paths", {}],
["E-DECL-SESSION", "future-owned-fail-closed", "explicit declaration sessions and cached getters are H2.7c-owned; H2.5h runs only in the ordinary JavaScript emit schedule", {}],
["E-DECL-FORCE", "future-owned-fail-closed", "forced declaration units are H2.7c-owned; no H2.5h transform or printer path selects them", {}],
["E-DECL-TRACKER-PARENT", "future-owned-fail-closed", "isolated-declaration tracker parent access belongs to the declaration transform; H2.5h owners contain no tracker callback", {}],
["E-DECL-TRACKER-ACCESSORS", "future-owned-fail-closed", "isolated accessor diagnostics belong to the declaration transform; no H2.5h reachability", {}],
["E-DECL-TRACKER-PRIVATE-TYPE", "future-owned-fail-closed", "isolated private-type diagnostics belong to the declaration transform; no H2.5h reachability", {}],
["E-RECOVERY-FACTS", "premise-unchanged", "parser-owned recovery admission precedes transformer registration and is target-independent; H2.5h adds no recovery fact or predicate and lowers only admitted trees", {}],
["E-HELPERS-IMPORT-STATE", "premise-unchanged", "ES2015/Generators add helper requests only; external helper import collection is module-transform owned and helper-name agnostic", { capabilities: ["helper-emission"] }],
["E-OBJECT-PROPERTY-A38", "premise-unchanged", "ES2015 shorthand/computed-property lowering prints through the existing PropertyAssignment/ShorthandPropertyAssignment arms; the A38 modifier-omission invariant is target-independent printer ownership", {}],
["E-RETAINED-PRODUCERS-A39", "modified-requalify", "class-fields synthetic constructors, static blocks and accessor redirectors are consumed by ES2015 class lowering (getFirstConstructorWithBody/getAllAccessorDeclarations); their separate raw/map/comment provenance must survive ES2015 wrapper placement", { surfaces: ["class-lowering-reach"] }],
["E-COMMENT-ELLIPSIS-A37", "premise-unchanged", "rest/spread comment phases ride the threaded comment scope unchanged; ES5 lowering removes ellipsis syntax rather than producing new ellipsis owners", { capabilities: ["comment-scope-threading"] }],
["E-COMMENT-PHASES-A36", "premise-unchanged", "modifier/statement/spread comment phases are target-independent refinements on the threaded comment scope; H2.5h producers add no modifier or statement-scope owner", { capabilities: ["comment-scope-threading"] }],
```

**Why each non-obvious row is classified as it is:**

- **`E-DECL-*` (7 rows) → `future-owned-fail-closed`, not `proven-unreachable`.** This follows the `E-OUTPUT-FUTURE` and `EA-GAP-MAPS-DECLS` precedent: the rows belong to a later slice, and H2.5h has no path into them. `proven-unreachable` would need a pinned proof artifact that doesn't exist.
- **`E-RETAINED-PRODUCERS-A39` is the one real H2.5h coupling.** Its synthetic constructors and static blocks are produced upstream of ES2015 class lowering, which consumes them through the pinned `class-lowering-reach` members. The same reasoning gave `E-CLASS-PENDING-G` and similar rows `modified-requalify`. This is the conservative choice and claims nothing is green.
- **`E-HELPERS-IMPORT-STATE`.** A bounded look at `collect_external_helpers_import` found no helper-name special-casing. Reclassify to `modified-requalify` if review finds any ES2015 helper-specific branch. The capability `helper-emission` is `exists`, so it is allowed with `premise-unchanged`.
- **A36/A37/A38 → `premise-unchanged`.** These are printer ownership refinements that apply to every target. Citing `comment-scope-threading` for A36/A37 is a legitimate reference to an existing capability, not a claim of H2.5h evidence. No surface is cited, because none is assigned to these rows.

**Nothing is removed or reclassified.** The 45 existing entries stay as they are, and the new table covers 58 rows (57 existing plus `E-RECOVERY-FACTS`). The manifest's counts change, and its lineage re-mints in the planned walk. Record the inherited drift (12 rows added since the last mint without dispositions) in the validation record as the reason this generator changed.

## `E-RECOVERY-FACTS` wording: accurate, one optional precision

- "Report-only consumption facts are partitioned … before the five predecessor profiles run" is correct.
- "The **new** retained escaped-keyword and whole-expression terminator rules enter only the final context profile" is correct. It doesn't claim that every report-only fact is context-only; the predecessor ASI whole-expression and other owner rules keep their profiles.
- "One retained Parser report" matches the uniqueness guard.
- **Optional:** write "whole-expression-plus-own-semicolon" instead of "whole-expression terminator", to separate it from the older ASI rule.
- The record entry keeps `E-SYNTAX-FACTS` separate, calls the terminator witness classification evidence only, and leaves the validation ref pending. All accurate.

## Checker missing-body guard: no architecture owner update needed

- `get_return_type_of_signature` is checker type inference. It isn't among `E-CHECKER-FACTS-BASE`'s listed transported facts (NodeLinks check flags, binder locals/container topology, `arguments` identity).
- It reaches declaration output through the existing resolver return-type query, and `E-RESOLVER-BASE` says that protocol is unchanged.
- The method and public API shapes are unchanged.
- **Record it in the validation record** as an upstream semantic correction (`_tsc.js:59815`), with its evidence:
  - the d.ts/map commands;
  - the strict no-false-2322 control;
  - exact diagnostics on the 21 original FunctionDeclaration corpus hits.
- Don't widen `E-CHECKER-FACTS-BASE`'s owner column or create a new row.
