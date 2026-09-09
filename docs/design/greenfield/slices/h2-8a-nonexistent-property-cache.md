# A6-26 required prerequisite: in-trial nonexistent-property de-duplication

Required shared prerequisite to the [readonly packet](h2-8a-defineproperty-readonly.md). Initial readonly production is already implemented;
its first runtime after is immutable and blocks closure. The two native controls have executed before and failed as specified below.
Readiness must pass before links.rs changes.

The first after matches all 492 fresh/adjacent commands twice, with 984 complete
native executions and 44 separate declaration-blocking emits. The original
checkingObjectDefinePropertyOnFunctionNonexistentPropertyNoCrash1 command aborts
on its first repetition with native stack overflow/SIGABRT, before tuple capture.
The second repetition and nine other original cases do not execute. The complete
current 1,718 checker units then pass on the newly built candidate binary. These
results qualify neither the original projection nor a final A6-26 close.

The sanitized OS crash stack has 145 frames. Its repeated component is
report_nonexistent_property -> type_to_string_slice_root -> property_signature_slice
-> is_readonly_symbol -> check_expression_cached(descriptor) -> object literal
property assignment -> binary expression -> property access -> report again.
The source crash report's hash and symbols are retained without unrelated process
or machine metadata. No new debug execution is counted or claimed.

TypeScript reportNonexistentProperty, _tsc.js75416–75470,
307d53095640744e1815787c33792c951a7a0c7fc169193870ec78298aca5f7c,
inserts the node/type/unchecked-JS cache key BEFORE querying/rendering the type.
A repeated key immediately returns. Native access.rs3321 has the same call but
LinksTables::insert_node_non_existent_prop_key in links.rs2965 unconditionally
returns true and records nothing whenever speculation_depth != 0. This is a
proven missing in-trial de-duplication behavior. The stack supports it as the
cycle cause, but actual speculation depth has not been dynamically traced.
The focused original replay after the bounded repair is the integration proof;
if it still aborts, preserve that new failure and investigate before widening.

The existing native policy intentionally prevents candidate-only diagnostic keys
from leaking across a transaction. Retain that policy while permitting keys to
be visible during the candidate. No deferred-diagnostic architecture, expression
cache sentinel/fallback, recursion limit, case-ID special case, or new CheckAbort
is needed. The new private journal is a native representation invariant of the
existing transaction, not a TypeScript source admission.

Allowed additional production path: crates/checker/src/links.rs only. Initial
ten readonly production paths stay byte-identical to the frozen first runtime
after while implementing this prerequisite. Tests: add two protocol controls in
checker/tests/unit/speculate/tests.rs; no existing assertion/input changes.

Step A6-26-4: add a private Vec<(NodeId, String)> journal of newly inserted
speculative diagnostic keys. Add its usize mark to SpeculativeLinksMarks and
capture its current length in speculative_marks. Keep the existing key encoding
and NodeLinks HashSet; no public query/API/type-ID shape changes. At depth zero,
assert the normal write boundary and insert permanently. At nonzero depth,
perform the real HashSet insertion; return false without journaling an existing
key, otherwise journal the new (node,key) and return true. Insertion precedes
every report query exactly as before. Compile and the first native cache control
prove repeated, global, parent, distinct-type and unchecked-JS behavior.

Step A6-26-5: a private restore helper pops entries above the checkpoint and
removes precisely those newly inserted keys. Invoke it at the end of BOTH
commit_speculative_writes and restore_speculative_writes, preserving the existing
candidate-key non-publication policy. Child commit restores child-only keys;
an outer/global key is never journaled by a repeated child lookup and survives
the child. Outer rollback or CheckAbort removes its own keys before the caller
can observe the result. No whole-set snapshot or retained-key promotion is used.
The second native control covers Commit, Rollback, Reject and BoundaryProbe;
the first covers nested commit plus outer rollback. Every boundary executes
before their final aggregate assertions, even on the failing before binary.

Native before adds two tests (full checker denominator becomes 1,720). The real
Cargo exit is 101 after 298.544921 seconds; both tests fail in 0.01 seconds.
Nested observations are [true,true,true,true,true,true,false,false,true,false,false]
instead of [false,true,false,false,true,false,true,false,true,false,false].
The four boundary observations are each (false,true) instead of (true,true):
restoration already holds, but visibility during the candidate is missing.
All Commit/Rollback/Reject/BoundaryProbe cases execute before the final assertion.
No complete compiler commands execute in this native job. The original TS observation
already runs twice with complete tuples and is immutable; source-level repeat
diagnostic identity is also anchored by the whole reportNonexistentProperty body.

Before a full replay, run only the two protocol tests and original cycle command
on the repaired binary. Require both units to pass, no stack overflow, and the
original either exact twice or the identical old name:any/name:string vectors.
No new original diagnostic/type difference is accepted as an outside failure.
If this probe passes, replay all 502 complete commands and all 1,720 checker
units, with all previous 76 readonly repairs retained. Reuse the existing 22
declaration-blocking controls. Keep the probe's two original commands separately
counted from the final 1,004 commands; preserve all failed/build attempts.

Shared readiness must pin the initial readonly readiness and first runtime
after, full checker first-after, new native before, all 26 upstream owners,
links.rs/speculate.rs baseline hashes, exact step/test traceability, seven
architecture concerns and the cache/journal lifetimes. E-RESOLVER-BASE remains
modified-requalify for the internal checker query/transaction schedule. Other
emitter concerns remain premise-unchanged; no new runtime entry, parsed mutation,
transform metadata or resolver method is activated. The new cache journal is
active-unqualified until these exact controls pass. H2.8 remains open.

Readiness command: `python3 scripts/check-nonexistent-property-cache-readiness.py`.
The first-runtime-after and checker-first-after archives retain the failed
492-command candidate and its 1,718 passing units. Fresh final validation must
use a newly built binary containing this prerequisite and both new tests.

Additional whole upstream owners:

- reportNonexistentProperty: _tsc.js:75416–75470, SHA256 `307d53095640744e1815787c33792c951a7a0c7fc169193870ec78298aca5f7c`.
- typeToString: _tsc.js:50717–50747, SHA256 `4b587962e2fb137a31ea52c35aeba733ffb4c6d97a8c54c98d5c1f1666e73dda`.

The other 24 whole owners, 240 fresh witness rows and 25 production caller sites
remain exactly those in the initial readonly readiness. Its initial ten-file
scope is retained as historical dependency scope; this amendment adds links.rs.

Architecture dispositions inherited with fresh source receipts:

| Concern | Impact |
| --- | --- |
| E-RESOLVER-BASE | modified-requalify |
| E-ENTRY | premise-unchanged |
| E-PROTOCOL | premise-unchanged |
| E-ARENA | premise-unchanged |
| E-CONTEXT | premise-unchanged |
| E-METADATA-BASE | premise-unchanged |
| E-POSITIONS | premise-unchanged |

Final measured result with the diagnostic-cache prerequisite: all 502 complete
commands match TypeScript twice. All 68 fresh readonly differences, the eight
readonly differences retained by A6-25, and the original function-expando
diagnostic are repaired. The original now reports name:string and remains
non-crashing. All other setter, binding-clone, JSDoc and original adjacent
commands remain exact. The cache repair is required for safe semantic readonly
queries; no assignment type-inference algorithm or recursion fallback was added.

The final combined job exits0 and executes 1,004 complete native commands without
supplemental artifact executions; test-target durations are [0.03, 0.0, 501.67, 51.86] seconds.
The 22 declaration-blocking controls pass with 44 separate emit-only executions.
Four selected query/cache units pass, then all 1,720 current checker units pass
with zero failures, ignored or filtered tests in 1.8 seconds on the
newly built binary. The exact identity set is the old 1,718 plus the two new cache
controls. Binary hash/size/timestamp, source and Cargo build provenance are frozen.
The prior focused cache probe separately executed the original command twice
and both new native controls successfully; those executions are not added to
the final 1,004-command count.

Initial failures remain immutable: the first after-build ran zero tests because
three existing test API callers were missed; the first runtime after completed
984 fresh commands and 44 separate emits, then aborted the first original
attempt with stack overflow before tuple capture. Its original second repetition
and nine other original cases did not run. That candidate's 1,718 checker units
passed. The two new cache controls then failed before the prerequisite, with
all nested and transaction-boundary scenarios observed. These records are not
rewritten as successful runs or counted as a complete 502-command replay.

The final ratchet is h2-8a-defineproperty-readonly-after.v1.json. Production is
bounded to the initial ten checker files plus links.rs. Existing test assertions
retain their meaning while accepting the typed Result; two additional native
tests cover the new journal's nested visibility and all exit boundaries. Public
resolver API, parser/binder, factory/printer, comparison contracts and frozen
TypeScript observations are unchanged. No new emitter-suite run, global total,
full developer CI or historical certificate walk is claimed. Other H2.8a owners,
B–E and final hosted acceptance remain open.
