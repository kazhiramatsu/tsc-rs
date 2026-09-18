# Per-cause patch scripts (applied in this order on top of `c35e00ccb`)

Each script asserts its anchors (`assert s.count(old) == 1`) and rewrites the listed files; together
they reproduce the uncommitted tree byte-for-byte (after `cargo fmt --all`). `COMMIT-PLAN.md` maps
the scripts to per-cause commits; shared files need hunk-level staging.

Order: ef1-patch → ef2-class-name-patch → ef6-umd-patch → ef4-default-name-patch (v1, superseded
by r6) → ef2-promise-ctor-patch → ef2-arrow-parens-patch → ef2-async-super-patch → r5-patches →
ef5-escaped-patch → ef2-read-comment-patch → r6-patches → ef6-jsdoc-link-patch →
ef2-async-alias-mark-patch → r7-patches-1 … r7-patches-6 → r8-patches-1 (EF2-AWAIT-USING-MISSING-NAME,
es_next) → r8-patches-2 (EF2-DETACHED-COMMENT, printer) → r8-patches-3 (EF6-IMPORT-TYPE-SELF, checker)
→ r8-patches-5 (EF2-ALIAS-NUMBERING: print-time substitution spelling) → r8-patches-6
(EF2-LOOP-VARIABLE-POLICY) → r8-patches-7 (EF2-DETACHED-COMMENT follow-up: top-level carry consumption) → r8-patches-8
(EF2-ALIAS-NUMBERING follow-up: the visit-time colliding-name query is gated like tsc's substituteIdentifier). Late manual edits recorded in DESIGN §7.1
(the `enclosing_class_evaluation` removal, the `current_function_is_async` removal, the
`static_this_substitute_flags` shape widening, the for-await plan fallback; r8: the ES2015
`colliding_declaration_name_substitute` + `generated_binding_print_order` metadata + the finalize
walk's naming-moment regrouping and derived-of-temp deferral in `target_bindings.rs` (r8 "part 4",
re-applied by hand after `cargo fmt` moved its anchors), the `get_global_iterable_type` publish in
`checker/src/globals.rs`, the outFile refusal-arm removal in `emitter/src/execute.rs`) are in the
tree diff `candidate-tracked.diff` (regenerated at the r8 final bytes).

## r9 / r10 (2026-09-18)

- `r9-printer-comment-boundary.diff`: the EF2-COMMENT-BOUNDARY printer change (unified diff of `crates/emitter/src/printer.rs`
  against the r8 bytes); the other r9 edits (member-name leading phase, accessor keyword token phase, decorator parse parens,
  option gates, checker fixes, EF7/EF8 entries) are in the tree diff.
- `r10-patches-1.py` (checker: EF7-ENUM-ISOLATED, EF7-ENUM-18056, EF7-JSDOC-CHECK-NODE, EF7-SYMBOL-WRITTEN-FACE; emitter:
  EF7-ASYNC-ARROW-LEXICAL-THIS (arena-aware facet arm), EF7-VERBATIM-EXPORT-ASSIGNMENT, EF7-HELPERS-IMPORT-PROLOGUE,
  EF7-YIELD-PARENS, EF7-GENERATOR-LOOP-VARIABLE, EF7-OBJECT-LITERAL-INDENTED),
  `r10-patches-2.py` (EF7-EXPORTED-REST-HOIST, EF7-SYSTEM-IMPORT-HELPERS incl. the shared collector, EF7-CJS-FLATTENED-EXPORT-NAME),
  `r10-patches-3.py` (EF7-CJS-EXPORT-INLINE, EF7-ASYNC-ACCESSOR-BODY, EF7-ASYNC-SHORTHAND-ASSIGNMENT, EF7-ASYNC-HELPER-CHECKS,
  EF7-NOCHECK-ROUTE, EF7-SYSTEM-IMPORT-EQUALS-EXPORTS, EF7-SYSTEM-NAMESPACE-ALIAS, the System helpers identity follow-up):
  each script asserts its anchors, is idempotent, and is applied in order on the r9 bytes (`python3 r10-patches-N.py <tree>`),
  then `cargo fmt --all`.

## r11 (2026-09-18, after the complete PLAN-BASE census)

- `r11-patches-1.py` (on the r10 bytes, BEFORE `cargo fmt`; not re-runnable after fmt): EF7-USING-HOISTED-CLASS-NAME
  (standard_decorators.rs), EF7-SYSTEM-EXECUTE-ASYNC + EF7-SYSTEM-HOISTED-DEFAULT-EXPORT (system.rs),
  EF7-VERBATIM-IMPORT-EXPORT-ELISION + EF7-ES5-ANONYMOUS-DEFAULT-CLASS-NAME (builtins.rs),
  EF7-ASYNC-ARROW-SUPER-CAPTURE + EF7-YIELD-PARENS follow-up + `arguments_N` file-wide numbering (es2017.rs),
  EF7-STATIC-ACCESSOR-RECEIVER + EF7-DECORATED-STATIC-FIELD-COMMENT (class_fields/downlevel.rs, standard_decorators.rs),
  EF7-SCOPED-NUMBERED-NAMES (generated_bindings.rs), touched-crate clippy hygiene (downlevel.rs, class_fields.rs,
  es2015.rs, system.rs, builtins.rs, declaration_map.rs, execute.rs).
- `r11-patches-2.py`: EF7-PRESERVE-CJS-HELPERS (builtins.rs: ESM transformer host, import-equals helpers form,
  helper-name substitution).
- `r11-patches-3.py` (harness): `noEmitOnError` / `preserveConstEnums` option projections
  (h2_7d_original_corpus_shared.rs).
- Order: `r10-patches-{1,2,3}.py` → `r11-patches-{1,2,3}.py` → `cargo fmt --all`. Probes: scratchpad `r11probes/`
  (32 inputs minted with the vendored `_tsc.js`; 30/32 identical at the r11 bytes — the two left are
  `tslibReExportHelpers2` (checker TS2343 through an ESM re-export, KNOWN) and the ES2015 accessor comment closed by the
  last part-1 hunk).

