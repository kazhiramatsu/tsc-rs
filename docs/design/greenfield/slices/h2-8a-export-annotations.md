# H2.8a A6-1: effective annotations on export assignments

The first global output comparison exposes five missing semantic diagnostics
in original checkJsdocTypeTagOnExportAssignment1/2/3/4/6.ts, while their complete
writes already agree. All eight numbered originals are in the new 809-case
oracle and all are H2.8a-only candidates. Cases5/7/8 supply positive controls.
These are actual original inputs with unchanged allowJs/checkJs, outDir,
module, target, roots and input bytes. No new later-owned disposition is added.

Owner: checkExportAssignment, _tsc.js:86391–86501, existing ledger hash
2481cb70fb33663829bfdf493fafea233783acafbd6069e1e83a2f85c5cf1a19.
Its getEffectiveTypeAnnotationNode/checkExpressionCached/getTypeFromTypeNode/
checkTypeAssignableTo branch at 86414–86417 is absent from the native method
in crates/checker/src/modules.rs. That method's old comment explicitly leaves
the JSDoc arm out; current typed syntax and effective_type_annotation_node now
support it. The existing contextual type selection already reads effective
annotations for ExportAssignment (contextual.rs:2639).

Reuse the existing typed helper, check_expression_cached(CheckMode::NORMAL),
get_type_from_type_node, and reporting check_type_assignable_to with the normal
Type_0_is_not_assignable_to_type_1 head, in upstream evaluation order. Place the
branch after the grammar/modifier checks and after obtaining the expression,
before verbatim/identifier alias processing. Preserve check-expression caching,
linked-reference collection, alias flags, diagnostic publication and all
subsequent export= logic. No new source-text recognition, parsing, type relation
algorithm, printer or module transformer belongs to this change.

Allowed production path: crates/checker/src/modules.rs, this method and its
stale comment only. Tests use a separately named projection of the exact eight
original global inputs/observations; the full769 selector remains unconditional.
Readiness must pin the existing helpers, original revision43e1107e8, the eight
input/observation row hashes and complete before failures before runtime edits.
The original full-run failure evidence remains immutable. E-PROTOCOL,
E-RESOLVER-BASE and E-CHECKER-FACTS-BASE are premise-unchanged but rechecked:
this restores a diagnostic-producing check in the existing live checker, without
moving facts across the borrowing resolver or retaining anything after its scope.
Final checker/adjacent export regressions remain required.

The full original comparison is complete: 666 exact and 103 failed cases,
each twice. All five expected annotation failures occur in both repetitions;
the three original positive controls pass. The before summary is retained as
`ratchets/h2-8a-global-before.v1.json`, with the complete log and tuples in the
external before directory. Its native failure text is diagnostic evidence,
not a new oracle or an accepted mismatch vector.

A6-1-1 revalidates the existing effective-annotation/contextual-type producers
against all eight original rows. A6-1-2 adds the missing reporting branch in
upstream evaluation order and replays the same complete rows. The readiness
script pins five upstream owners, six baseline checker files, both original
input/observation artifacts, the completed before record, the packet and the
unchanged architecture. It checks eight unique original IDs, five expected
before failures, three positive controls, and zero unresolved/undispositioned
rows before changing runtime code. The remaining global mismatches stay open.

The upstream owner closure is: getEffectiveTypeAnnotationNode 16761–16767,
getTypeFromTypeNode 63196–63198, checkTypeAssignableTo 63931–63933,
checkExpressionCached 80580–80595 and checkExportAssignment 86391–86501.
The new branch reuses all four existing helpers; no helper algorithm changes.
The applicable E-PROTOCOL, E-RESOLVER-BASE and E-CHECKER-FACTS-BASE rows remain
premise-unchanged but rechecked. Focused verification is the separately named
`original_export_assignment_annotations_match_complete_commands` test; final
full769 comparison and checker/export regressions remain required.

The A6-1 production replay passes all eight original inputs twice, including
the five repaired failures and the three positive controls. The focused test
exits 0 in 27.80 seconds; its log is
`target/h2-8a-export-annotations-after.log`. No expected observation changed.
This checkpoint fixes one causal group; it does not replace the required
final global comparison or close H2.8a.
