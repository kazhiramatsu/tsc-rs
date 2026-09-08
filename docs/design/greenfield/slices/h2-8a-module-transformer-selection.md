# H2.8a A6-24: module transformer selection

Runtime slice at base `74447b72aa0e51826b6bc832580aa4780d9a1eca`, on the
existing H2.8 train (trusted train base 10748f6ee19ec083ce5748224930c5dcfbbd86df).
Repair the outer module factory selected for None/AMD/UMD, including its native
activity accounting. A23 supplies two fresh mjs failures; the frozen A14 global
replay supplies original impliedNodeFormatEmit1 AMD/UMD failures. Current before
evidence, rather than those historical counts, defines this slice's membership.
No other A owner, B–E, unknown module admission, custom transforms, builder,
checker fact, binder, parser, printer, comparison adapter or expected byte edit.

The 184 complete TS commands cover all 14 known module modes, ES5/ES2015 and
ts/mts/cts/js/mjs/cjs (168), plus AMD/UMD/CommonJS/ESNext with commonjs/module
package types in ts/js (16). Standard libraries, strict checking, declarations,
source maps, CRLF and outDir remain in every case. Diagnostics: 5107 occurs 140,
5095 eight and 5071 four times. TS mint and independent --check reproduce all
commands twice; mint process metadata was lost at compaction, so only the
confirmed check has an exit-0 claim. Factory-name metadata is outside the full
command tuple and is independently compared by the native factory test.

Six original complete commands select impliedNodeFormatEmit1 in AMD, UMD,
CommonJS, ESNext, Preserve and System. They have no package files; the fresh
package controls cover that independent input dimension. Existing complete
comparators and the 809-candidate artifacts remain unchanged. Supplemental
cloned-Program write captures run only in the first before job and count as
extra commands, not complete actual tuples. Fresh failures execute once per
job; two primary jobs establish repeated first vectors. Original failures
execute twice inside their runner and capture full tuples.

The upstream call graph is getScriptTransformers -> getModuleTransformer ->
one of four factories. Preserve selects transformECMAScriptModule; System
selects transformSystemModule; CommonJS, ES2015/20/22/Next and Node16/18/20/Next
select transformImpliedNodeFormatDependentModule; the default selects
transformModule. Its getTransformModuleDelegate uses the compiler module kind
to select AMD/UMD/CommonJS. Only the implied composite selects a child using
getEmitModuleFormatOfFile -> getEmitModuleFormatOfFileWorker ->
getImpliedNodeFormatForEmitWorker. That worker considers Node modes and explicit
extension/package facts. No change to the Program-owned format computation.
The implied factory still constructs ESM then CommonJS, dispatches transforms
and hooks by source, and disposes in reverse. transformModule's existing source
eligibility, wrapper body, resolver queries, helper naming and optional host
remain the selected runtime implementation.

| TS state or transition | Current Rust owner / gap | Representation and lifetime | Action and proof |
| --- | --- | --- | --- |
| moduleKind -> factory | private builtins::get_script_transformers_with_optional_host; partial-or-stale: all non-Preserve/System modes use implied | New private four-variant ModuleTransformerKind, computed from immutable options per list; no persistent identity/state | A6-24-1;184 factory lists twice and complete commands |
| direct factory's compiler moduleKind | private transform_module_with_optional_host and CommonJsModuleTransformer.module_kind; shared-prerequisite, already implemented | Existing constructor borrows resolver/optional EmitHost for transformer lifetime; no new ownership | A6-24-1; all direct extension/package witnesses |
| implied file format | EmitHost::get_emit_module_format_of_file and private ImpliedNodeFormatDependentModuleTransformer; already-exact for admitted implied modes | Existing SourceFileId and borrowed Program facts; None remains typed runtime error | A6-24-2; host count and absent-format controls |
| native H2 activity | first list selection and observe_additional_bundle_source_activity; partial-or-stale for direct modes | Existing invocation-local H2ActivityCanary; first/list and each-source events stay separate | A6-24-2; three source identities with six host answers |
| script and hook order | existing transformer Vec/composite lifecycle; shared-prerequisite | Existing ordered boxes and TransformationContext; no hook composition change | factory lists, emitter library/contracts, adjacent87 import publication commands |
| admitted invalid options | existing target/module/outFile validation; already-exact native refusal boundary | Existing TransformError variants, before list creation | A6-24-3; invalid module and hostless outFile controls |

A6-24-1 changes only production crates/emitter/src/builtins.rs. Add private
ModuleTransformerKind { EcmaScript, ImpliedNodeFormat, System, Module } and
get_module_transformer_kind with the pinned switch. The existing upfront
validation still rejects unknown module values; the switch default mirrors TS
but does not widen admission. Match the kind at the existing module factory
construction site. EcmaScript/System/implied retain their constructors and
ordering. Module calls existing transform_module_with_optional_host(options,
resolver, host.map(|(host,_)|host)). Never call or export the cfg(test)-only
transform_module wrapper. Require a host only in the implied arm; preserve
the earlier hostless nonempty outFile refusal. Do not change earlier pass order,
module visitor logic, hook implementations or source metadata.

A6-24-2 corrects the same-file activity consumers. Factor the two existing
H2_1b/H2_1c format predicates into a private native activity helper. The direct
arm uses compiler module kind, records H2_1b and AMD/UMD H2_1c, and constructs
no ESM child or H2_1a event. Implied keeps its one H2_1a event, format query and
ESM child. Preserve/System keep existing counters. The additional-Bundle-source
consumer matches the same classification: direct uses compiler kind without
format query; implied queries that actual member; Preserve/System do nothing
there. Preserve observe_script_source_routing and its syntax/option events.
Counters are native structural controls, not upstream output observables.

A6-24-3 qualifies boundaries in emitter tests/unit/builtins/tests.rs and
compiler tests/integration/emit_session_contract.rs. Three new native tests
compare 184 lists twice, exercise 14 modes x six format answers with three
member identities, and distinguish absent host from missing format on 14 modes.
Missing format does not fail during construction: executing the implied module
transform retains MissingProgramSourceForModuleFormat. Empty TS source removes
unrelated binder queries from this fault control. Existing upfront invalid
module[-1,8,201] and hostless outFile refusals remain. The real complete commands
provide executable syntax/output coverage separately.

Eight existing compiler controls execute before changing their expectations:
deprecated_module_none_selects_transform_modules_commonjs_delegate;
h2_1c_amd_and_umd_wrappers_match_the_pinned_transform; and the generic Preserve,
H2_1a, H2_1b, H2_1c, H2_1d, H2_1e filesystem failure controls. Amend only None's
H2_1a expectation to 0, AMD/UMD wrapper H2_1a to 0, AMD/UMD filesystem H2_1a
membership, and the shared filesystem helper's ESM construction expectation
for direct modes. Preserve all expected output, diagnostics, partial-write,
retry and continuation assertions. The corrected values represent the source-
backed factory repair; retaining the obsolete counters would preserve the bug.

Architecture E-ORDER-G and E-ORDER-H are modified-requalify for this bounded
module-selection branch. Their historical validation dates do not qualify the
stale direct branch; this candidate is active-unqualified until the recorded
controls pass. E-PROTOCOL is modified-requalify for the host dependency boundary;
host, resolver, sink and outcomes retain separate ownership. E-ARENA,
E-CONTEXT and E-RESOLVER-BASE are premise-unchanged: parsed trees and metadata
remain immutable inputs, context lifecycle and borrowed semantic queries are
unchanged. The six architecture rows are re-read against current code and are
linked by the readiness manifest to whole TS owners and controls. No dormant
or planned architecture concern is activated.

After require 368 complete commands: 184 fresh, 91 A23 marker, 87 import
publication reference and six originals, plus the eight compiler activity
controls and three new native controls. Emitter library/contracts cover the
shared registration and lifecycle. These are targets until measured; save every
unexpected result before revising scope. Capture environments are disabled
after. No new global 769 total may be inferred from focused repairs. The
schedule's user-authorized lightweight workflow applies; no historical
certificate walk or full developer CI is claimed. Final whole-matrix replay,
other A owners, B–E and hosted acceptance remain open.

Heavy commands use CARGO_BUILD_JOBS=2, taskpolicy -b and nice -n 15, sequentially.
Only read-only analysis/scratch work runs during canonical mint/build/replay.
Before receipts and failed logs are copied to an immutable outside archive;
no source edits occur until each process's real exit is recorded. Readiness
checks authority hashes, whole-function spans, architecture dispositions,
every witness and before membership: unresolved=0, undispositioned=0. The
integrator is the sole writer; no delegation or separate trains are used.

Measured before: 172 fresh cases match twice per
primary job; 12 fail once per job with identical first vectors.
Durations are [367.19, 248.14] seconds (compiler contracts, first
job also contains eight passing activity controls); both jobs exit 101.
There are 356 separate supplemental artifact executions and
1080 total complete-comparison/supplemental native commands,
including 12 original executions. Four original cases match twice; AMD/UMD fail
twice with four complete tuples differing only in writes. All fresh differences
are direct-mode JavaScript plus map mappings; paths, declarations and exits
agree in the supplemental captures. These do not qualify later tuple fields.
Native factory comparison executes 368 lists: 280 match and 88 differ (44 inputs
twice). All three new native tests fail before repair. The first attempted
build had zero test executions: the activity test needed counters() before
reading getters. Its immutable first-build record precedes this test-only fix.

Whole pinned TS functions:

- transformModule: 110090–112041, SHA256 `3d54d8672774bc47f161ad1b4747b2d39a9a04f3da0a7cdab4c8b5ea125ca3eb`.
- getTransformModuleDelegate: 110091–110100, SHA256 `247c10434f894b5cba93194f5408bc7917c3e8b2b0ec89eb4ca4c8aaf6ffcb41`.
- module.transformSourceFile: 110130–110157, SHA256 `b1a1419c70b033ad4e884d96dfc78c8ee746580e502895fc68d55091b68e1752`.
- transformECMAScriptModule: 113369–113727, SHA256 `a4106ecc07d7c7b1d1caa38cb6ef962b9c244316d94f1f6acca0e3d497b28d22`.
- transformImpliedNodeFormatDependentModule: 113730–113793, SHA256 `1fa1716e96e65c34c4d0972d80814f9551d401359de602394255a5b312ebbe55`.
- getModuleTransformer: 115876–115895, SHA256 `bbb6a0f805e34703e2cf88a3a7e6fe951a390a2b6da3899aab9c02584fff9174`.
- getScriptTransformers: 115903–115949, SHA256 `69bdc65a0c428ad5819419fabd0ecd483bb661350434c5ad0ea0bdec15096fd0`.
- getEmitModuleFormatOfFile: 125476–125478, SHA256 `57ecc4aaea00e6de3e097bd930633132f5a8ce33c01b727f79432d62dd6fc846`.
- getEmitModuleFormatOfFileWorker: 125493–125495, SHA256 `ffe7b58092e4af38c9484bef12201ef7524d2e3d26ba829ea59087f1a2c0d2a1`.
- getImpliedNodeFormatForEmitWorker: 125496–125509, SHA256 `765b9d66f854668f6f9326de2a8e3659af532be224d3a2546fdec84855bbe69c`.

Reproduce TS: `node scripts/observe-module-transformer-selection.mjs --check`.
Readiness: `python3 scripts/check-module-transformer-selection-readiness.py`.
Before/after exact argv, environment, input hashes and real exit records are
retained in the outside archives; before-repeat uses only the fresh test,
while the combined before also selects eight compiler controls and three emitter
controls with --no-fail-fast. Comparators remain unconditional.

Final measured result: all 368 complete commands match TS twice: 184 fresh,
91 A23 marker, 87 import publication references and six originals. All 12 fresh
and two original factory-selection failures are repaired. All 172 fresh prior
positives and four original positives remain exact; the two A23 mjs residues
also close in their full91-case replay. There are 736 complete native command
executions, no supplemental executions, and the combined process exits 0.

The eight source-backed compiler activity controls and all three new native
controls pass in the combined job. Emitter's full 488 library tests and 451
contracts also pass in 0.81s and2.36s.
They are new executions of the unchanged binaries produced by the combined
Cargo job; binary hashes, source/build lineage, exact argv and actual exits
are pinned before execution. No historical pass or competing build is reused.
The whole library repeats the three new controls. Empty filtered targets add
no test executions. No checker suite is claimed for this unchanged checker.

The final record is ratchets/h2-8a-module-transformer-selection-after.v1.json. The runtime change is
confined to builtins.rs factory selection and its two activity consumers;
existing compiler controls change only the obsolete direct-mode counters.
Expected TS tuples, comparison adapters and the original matrix artifacts
remain byte-identical. Other A owners, B–E, final 769 replay and hosted acceptance
remain open; no new global total, full developer CI or certificate walk is claimed.
