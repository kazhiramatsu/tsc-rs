# H2.8a A6-13: one element-end comment phase per delimiter

Base `ba2375f8d69b7887a32be2982d80395da99c4735` is the completed A6-12 checkpoint. Its printer bytes are unchanged
from A6-11. Sole production path is crates/emitter/src/printer.rs, only the
duplicate element-end comment call in the compact emit_delimited_expression_list
branch. The corrected complete52-command before is frozen in two independent
jobs before production activation.

A6-13-1 removes the second identical call to
emit_list_element_end_comments_in_container, currently inside emit_delimiter.
The unconditional call immediately above already emits the element-end phase.
TS emitNodeListItems performs one appropriate phase before the delimiter.
Retain the first call, final-child and multiline behavior, trailing comma,
CommentResume handoff, indentation and source line decisions. Do not alter
comment flags, source-map encoding, metadata, factory, names or checker.

The source writer already brackets each comment with its start/end map records.
All14 fresh before differences are map mappings only; all other map fields are
identical. The extra segment pair returns to the same comment start and writes
its end again. Removing the redundant caller removes both duplicate bytes and
duplicate mapping positions. This is a comment-consumer repair, with no source-map
recorder or public API change.

E-COMMENTS-G is modified-requalify. E-PRINTER-BASE, E-COMMENT-SCOPE-H and
E-METADATA-BASE are premise-unchanged: immutable structural plan and per-side
comment scope, node hook ordering, typed source cursors and sparse source/name
metadata stay in place. Parent NoNestedComments remains a separate known gap;
no mutable ambient comment state or source-wide deduplication is introduced.
There is no new host/sink/cancellation boundary, so no new sink fault witness
is applicable.

52 complete commands cross JS/TS and both removeComments settings, all with
JavaScript maps and declaration output. They cover every direct caller kind:
array/object literal, array/object binding pattern and import attributes, with
unchanged call controls, multiline, trailing-comma, final/single/empty-element
controls. The TypeScript commands report zero diagnostics; each was observed
twice and the full observer check also passes. Native before is38 exact in each
job and14 independently repeated first-map failures, taking52.90s and
40.74s,exit101. Successful cases execute four times across two jobs;
failed cases execute twice. Target52 exact after remains a target until measured.
Later comparison fields of failed before commands are not claimed qualified.

The original two preliminary jobs stopped four import-attribute controls in the
test helper, which did not accept moduleResolution. They are retained outside
the checkout and are not the complete52-command before. The only shared harness
change transports moduleResolution to the existing CompilerOptions field,
matching the original-corpus runner. Inputs and TypeScript expectations did not
change. A test-only formatting wrap was applied after both preliminary jobs
exited; no canonical mutation overlapped a job. The qualified two before jobs
use the corrected, formatted adapter at unchanged production source.

The preceding48-case standalone printer fixture is immutable A6-11 evidence:
41 exact twice and seven actual twice-executed failures. Require all41 positives
plus the three array-inline duplicate-tail controls (None, NoLeadingComments,
NoNestedComments, retained) to become exact:target44/48. Four parent
NoNestedComments controls stay outside. The overlapping array-inline parent
control loses its duplicate tail but still fails the suppressed-scope owner;
the other three retained vectors must remain byte-identical. The standalone
runner captures both failed repetitions independently, unlike the shared
complete-command helper. Never infer failed repetitions from duplicate panic
renderings; the A6-11 execution correction remains authoritative.

After the edit run all52 complete commands, all48 standalone printer controls,
483 emitter units,451 existing emitter contracts (including30 scope witnesses)
and18 source-comment topology contracts. Inspect any newly exposed first
mismatch and freeze an intermediate after before expanding scope. No fixture,
expectation or test membership shrink. The test remains unconditional with
four known parent-scope controls; do not claim a general printer suite pass.

Whole769 replay, other A owners, B–E and hosted acceptance remain open. Follow
the schedule's user-authorized focused edit workflow. Historical full developer
CI and the certificate walk are omitted, not claimed as passing. Poll all jobs
to real exit before canonical mutation.

Pinned complete TS6.0.3 bodies in vendor/typescript-6.0.3/lib/_tsc.js:

- emitNodeListItems: 120068–120155, SHA256 `ebeb65a71c929bbfdf5d1ebd4b2e7216f15bd37117166ef8fbee5b3a9b0a6b40`.

- emitLeadingCommentsOfPosition: 121166–121175, SHA256 `fa23b688b1540c772ccf513c874d47bba4a08a44e019bc430feb79cbea73d2cd`.

- emitTrailingCommentsOfPosition: 121191–121198, SHA256 `953cde198b7f8098bd7bc8d865e535cdac106e983efe35a4a067498ff239cbc0`.

Final measured validation:all52 complete commands match exactly twice, including
all38 before positives and all14 previously failing maps. Diagnostics, ordered
write bytes/metadata, declarations, map results, status and exit all match.
The compiler test finishes in46.83s,exit0.

All483 emitter units,451 existing contracts (including30 scope witnesses) and
18 source-comment topology contracts pass. The standalone printer fixture has
44 exact twice:all41 previous positives and all three duplicate-tail controls.
Four independently repeated parent NoNestedComments failures remain. The
overlapping array-inline case loses only its duplicate AFTER comment, and the
other three first vectors remain byte-identical. This emitter invocation exits101
only for that unconditional printer test; no general printer pass is claimed.
Cargo stopped at that failed target, so the18 topology contracts were run in a
separate invocation at the same source and pass with exit0.

No further production change, fixture normalization or membership update was
needed. The final record is ratchets/h2-8a-array-comment-publication-after.v1.json;
the outside source/log archive is located by
target/h2-8a-array-comment-publication-after-location.txt. Whole769 replay,
remaining A owners, B–E and hosted acceptance remain open.
