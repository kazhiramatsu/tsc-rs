# H2.8a A6-23: CommonJS esModule marker

Base `6acce5e014d8c572769e59281ef11fdc987a3f5c`. Two independent fresh
before jobs produce 61 exact and 30 failed cases each, with identical first
vectors and membership. Their durations are 159.13s and 106.54s, both exit 101.
The first combined job also executes seven original commands twice, all failed
in 39.39s; its 14 full tuples differ only in write bytes containing extra markers.
The immutable before record retains outside archive receipts for all inputs,
logs, full original tuples and supplemental fresh captures.

The 91 fresh complete TS 6.0.3 commands cover 13 shapes in CommonJS, AMD,
UMD, Node16, Node18, Node20 and NodeNext: auto/force/legacy module detection,
exports/module.exports, require-only/no-CommonJS, real ESM and mixed syntax,
js/jsx/cjs/mjs/cts/ts. All use standard libraries, ES2015, strict checking,
declarations, source maps and CRLF output. Diagnostic 5107 occurs 26 times and
2304 five times. Mint and independent confirmed check each execute 91 commands
twice. An earlier complete check log has lost process exit metadata; its
confirmed replacement is retained explicitly, with no fabricated exit claim.
Seven original commands select nodeModulesAllowJsExportAssignment in the four
Node modes and nodeModulesCJSEmit1 in Node18/20/Next. Node16 CJSEmit1 is absent
from the current frozen candidate set; fresh Node16 controls provide that mode.
The existing complete comparators, original 809-candidate artifacts and adapters
are unchanged. Fresh first-before supplemental cloned-program byte captures
are extra executions, not complete actual command tuples. Original failures
capture full actual tuples twice within one job. Fresh failures run once per
job, so a second independent fresh job is required; disable captures there.

A6-23-1 adds an exact binder-fact projection in
crates/checker/src/program.rs::ProgramBinder next to
is_external_or_common_js_module_of_node. Select the owning file with the
existing file_index_of_node and read file_entries[file].data()
.common_js_module_indicator.is_some(). This is a native adapter for TypeScript's
direct SourceFile property; do not claim an upstream isCommonJsModule function.
Add consumer-owned EmitResolver::is_common_js_module and
EmitResolverMethod::IsCommonJsModule/name in crates/emitter/src/resolver.rs.
The default returns the existing typed Unavailable error. Implement the method
in crates/checker/src/emit.rs::CheckerSession through with_resolver_node,
including existing source identity validation and CheckerAborted mapping.
Leave the combined external-or-CommonJS projection and all its callers intact.
Binder set_common_js_module_indicator already rejects real ESM while allowing
forced boolean indicators; no binder, parser, checking, or source mutation.

A6-23-2 changes only the marker decision inside the existing visit_result
closure in CommonJsVisitor::transform_source_file (builtins.rs). Add private
should_emit_es_module_marker(root). Existing no-export-equals and external
presence checks already decide false without querying. For a filename satisfying
hasJSFileExtension, inspect the retained external indicator node kind: SourceFile
is the parser's boolean-true sentinel, whereas imports/exports/JSX are real
indicators. Query is_common_js_module only for that forced JS branch, passing
existing resolver_node(root) so the binder receives the parsed Program root.
Return false for true; false permits the marker. Propagate typed query errors.
The upstream predicate reads infallible SourceFile fields; the native query is
fallible, so independent false/real-ESM/non-JS routes must not demand a provider.
The local extension helper exactly matches js/jsx/mjs/cjs case-sensitively and
retains fileExtensionIs's path-length-greater-than-extension requirement.
No compatible shared emitter helper exists: current declaration helper also
uses NodeFlags and lowercase, while output-extension selection lowercases.
Do not import checker into emitter or expand those other helper contracts.

TransformArena::replace_root updates syntax.root after earlier transforms and
CommonJsModuleTransformer's strict-prologue insertion. Comparing the indicator
with current syntax.root would lose the sentinel. Use its retained SourceFile
kind, preserve the existing parse-tree resolver identity, and add no synthetic
identity fields or mutable parsed parents. Keep the predicate inside visit_result;
end_lexical_environment must run before visit_result? propagates errors. Existing
marker construction, wrapper selection, write ordering and maps stay unchanged.
The marker predicate is shared by CommonJS and the AMD/UMD body. Separate mjs
transformer-selection failures, if measured, remain an explicit later owner.

A6-23-3 adds controls to emitter tests/unit/builtins/tests.rs and checker
tests/unit/emit/tests.rs. Exercise provider true/false, typed Unavailable and
CheckerAborted; verify exact Program source/parsed root and call count after
strict root replacement in CommonJS/AMD/UMD. No-query controls cover a real ESM
indicator, TS extension, uppercase filename, extension-only filename, absent
external indicator and export-equals. Wrap the real module transformer in a
sentinel outer lexical environment; verify flags are restored on success and
failure before propagating the original Result. The trait default/name are
checked independently. Checker controls bind auto/force exports, force-empty,
real ESM mixed CJS, module.exports and require-only documents in one identity
domain; query the exact binder fact and unchanged combined fact. Wrong source,
source-node mismatch and unknown node retain precise typed method errors.

Architecture E-RESOLVER-BASE and E-CHECKER-FACTS-BASE are modified-requalify:
the new borrowing query projects an existing binder fact without transferring
ownership. E-ARENA and E-METADATA-BASE are premise-unchanged: parsed provenance,
current roots and mutable detached syntax remain separate. E-ORDER-H is
premise-unchanged: transform selection/order and CommonJS wrapper ownership
are untouched. E-PROTOCOL is premise-unchanged: EmitHost, resolver, output sink
and command result retain their ownership and failure boundaries.

After require 397 complete commands: fresh 91, original seven, 87 import
publication references, 84 synthetic default aliases, 32 CommonJS class-instance,
72 System variable publication and 24 System dynamic imports. Target 395 exact
twice: 89 fresh and all 306 adjacent/original commands. The two AMD/UMD mjs
module-selection failures must preserve their exact first source-map vectors.
Require emitter library/contracts and checker emit units. These remain
predictions until measured; preserve any unexpected result before revising scope. Keep comparisons
unconditional; capture environments disabled after. No new global 769 total is
inferred. The schedule's lightweight workflow applies; historical certificate
walk and full developer CI are omitted and not claimed. Other A owners, B–E,
final complete replay and hosted acceptance remain open.

All 30 fresh first failures are exact source-map results. All 152 supplemental
captures complete without errors, their exit codes match TS, and repeated
positive captures agree. Twenty-eight commands (four per module) differ only
by extra markers in JavaScript and mappings in source-map JSON. All declaration
bytes match. The two AMD/UMD mjs controls select ESM instead of TS wrappers.
Write paths and exit codes match across every capture. These supplemental
components do not qualify later complete-command fields after a first failure.
Across two fresh jobs there are 304 primary and 152 supplemental native commands,
plus 14 original commands: 470 total. Each fresh positive executes six times and
each fresh failure three times; the extra capture executions are kept separate.

Whole pinned TS functions:

- fileExtensionIs: 5323–5325, SHA256 `a9383f1765a1e059f2173ec87c046e8a2fb25b04c3208ee854ce729ec9333d04`.
- isExternalOrCommonJsModule: 14119–14121, SHA256 `e395fd4c4d5df1373eb3cc17bc653dfcd8f2e41b9e32d949b3063633dc02c07d`.
- isFileModuleFromUsingJSXTag: 17967–17969, SHA256 `03cf7895028d987d8656f4dac648629eff3460560667e0f1aee1c638a5da612f`.
- isFileForcedToBeModuleByFormat: 17970–17972, SHA256 `899bdc1b5bf64d169184d5ba41a67bd50a876f22b6bba9d6e0511e392e48bd93`.
- getSetExternalModuleIndicator: 17973–17993, SHA256 `79ad6849558d6a2335dc93ffba43de342602ab327ad1b767895a52516a19b0a5`.
- hasJSFileExtension: 18654–18656, SHA256 `26f2de10186fd7377e0fc90d254165421f27320a1b95dca68e43ee8f2f71128d`.
- isExternalModule: 28910–28912, SHA256 `5effe04fdce706cc75f238b5c4efbb1f317b3f6bd665389fb71a79a119e7ceaa`.
- setCommonJsModuleIndicator: 44589–44600, SHA256 `294f2a8b52accab297f5d966aa37276fb9f4b9bb850ed593d2b15c49a6531118`.
- collectExternalModuleInfo: 92779–92919, SHA256 `2694413ce6ea08091a03db3a313b50ee3ff526b065f199270311ac583350220e`.
- getTransformModuleDelegate: 110091–110100, SHA256 `247c10434f894b5cba93194f5408bc7917c3e8b2b0ec89eb4ca4c8aaf6ffcb41`.
- module.transformSourceFile: 110130–110157, SHA256 `b1a1419c70b033ad4e884d96dfc78c8ee746580e502895fc68d55091b68e1752`.
- shouldEmitUnderscoreUnderscoreESModule: 110158–110166, SHA256 `947ac8347f6e2848ce70717b634bf7d184204c966f3b9fe16fc0a59cc96beb98`.
- transformCommonJSModule: 110167–110204, SHA256 `a48c215e8304107fec1f4e113f74a13f720cb5901cff5287bda673a98e082749`.
- transformAsynchronousModuleBody: 110492–110534, SHA256 `8d0713be5aaf1b99c2e7e304e5e5f341884d2c0a865aefc1336c96f9cb98d9c9`.
- createUnderscoreUnderscoreESModule: 111773–111790, SHA256 `fce473b099c9bd36bb2f57fe48889e51b049c8489f48dbf33e9c44e912129b0d`.

The first direct-control build failed before executing any tests (E0505): the
new checker control moved its source Arc into ParsedDocument while BinderWorker
still borrowed it. The frozen direct-first-build record preserves all 11 launch
receipts and the failure log. Clone the Arc into ParsedDocument, as the existing
checker harness does; this is a test ownership correction within A6-23-3 and
changes no production source. No test pass or command execution is claimed for
that failed build.

Final measured result: 395/397 complete commands match TS twice. All 28 fresh
owned failures and all seven original marker failures are repaired. Fresh exact
coverage is 89/91, retaining all 61 prior positives; all 299 adjacent commands
match twice. The two AMD/UMD mjs transformer-selection failures retain their exact
before first vectors and execute once each. The compiler contracts take
549.36s, five passing and one failing, followed by the passing
seven-original projection in 31.12s. The combined exit remains
101 because comparisons are unconditional. There are 792 native command
executions and no supplemental capture executions in this after job.

All 485 emitter library tests and 451 contracts pass in
0.69s and 2.18s. All 17 checker
emit units pass in 0.09s. The library and checker suites reuse the
unchanged test binaries built by the passing direct-control job; both binaries
were hashed before these independent read-only executions. No competing Cargo
build or historical test result is reused. The three new direct controls
also passed before the complete compiler replay, including 120 module-transform
provider/filename scenarios, exact parsed-root identity, typed failures and
lexical-scope restoration. Exact launch packet/readiness and all input snapshots
are preserved outside the checkout before this measured-result append.

The initial readiness count of 32 checker emit units mistakenly combined A21's
31 node_builder tests with the new emit test. The complete selected emit suite
contains 17 tests: all 16 existing tests and the new binder-fact control passed.
The checker-selection-correction record preserves the original launch manifest,
actual selected names and the prior different suite. No test source or production
source changes accompany this count correction.

The final record is ratchets/h2-8a-commonjs-esmodule-marker-after.v1.json. This closes
the marker predicate and its binder-fact projection. No expected observation,
shared comparator, parser, binder producer or host/sink contract changed.
Other A owners, B–E, the final 769 replay and hosted acceptance remain open.
No new global total, historical full developer CI or certificate walk is claimed.
