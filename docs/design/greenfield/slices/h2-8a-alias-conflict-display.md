# H2.8a A6-12: alias-conflict diagnostic name display

Base is the completed A6-11 checkpoint `d0f23fdcaac4fa7e7e93c2c87178d991cf528bb2`. Sole production edit path:
crates/checker/src/modules.rs, check_alias_symbol's targetFlags/excludedMeanings
conflict diagnostic branch. No runtime activation before the complete24-command
before and readiness checks. The complete24-command before is frozen and repeated in two independent jobs.

TS checkAliasSymbol calls symbolToString(symbol) when a resolved alias target's
meanings conflict with an existing declaration. Native symbol_display_name uses
only the unescaped symbol table key; it loses the declaration's written quote
syntax. Existing emit_symbol_to_string_default already implements upstream's
no enclosing declaration, absent meaning, AllowAnyNodeKind, IgnoreErrors and
single-line remove-comments printer. Call it here and propagate CheckResult with
?. Preserve the chosen merged symbol, diagnostic owner/span/message selection,
query ordering and every non-conflict branch. Do not globally change the string
helper or reconstruct names from current ExportSpecifier source bytes.

Dependencies already implemented and unchanged: declaration_emit.rs
symbol_to_string_via_node_builder / emit_symbol_to_string_default;
node_builder/serialize.rs emit_build_symbol_display_node and its per-source
mounts; state.rs emit_display_result owns one hook-less session arena. Returned
String outlives its printed node without exporting mutable checker state. The
existing node builder error conversion and temporarily disarmed replay sink
remain unchanged. No factory/printer/public resolver API edit. E-PROTOCOL,
E-RESOLVER-BASE, E-METADATA-BASE and E-PRINTER-BASE are premise-unchanged;
E-STRINGS is modified-requalify at this diagnostic consumer.

A6-12-1 changes only the conflict-branch display call described above.

24 new commands cross12 shapes with JS/TS at CJS ES2015 so semantic diagnostics
are active: single/double/Unicode/underscores, alias before/after declaration,
identifier-only names, function/class declarations, import conflicts including
quoted source names, and a no-conflict control. Two existing CJS ES2015 TS2484
commands are additional regressions. Existing ES5 TS5107 deprecation controls
retain original options and do not count as tests of the semantic branch.
Every command stores diagnostics, ordered writes/bytes/callback metadata, emit
result, status, exit, and repeated complete native/TS observations. No output
normalization or diagnostic suppression. No new sink or host-failure edge; no
new sink fault-injection witness is applicable. Parser recovery stays H2.9.

After must replay all24 and the preceding200 export-name controls (only eight
H2.9 source-map recovery refusals expected to remain), preceding56 declaration
name controls, and adjacent checker statement/chain/specifier units. The System
and token-comment consumers are unchanged from A6-11; the preceding200 already
includes System publication and map controls. Require all prior positives and
inspect every retained first mismatch; counts are targets until measured.
Whole769 replay, other A owners and B–E/hosted acceptance remain open. Follow the
schedule's user-authorized lightweight edit workflow; no historical full-CI or
certificate walk claim. Poll all jobs to real exit before canonical mutation.

Pinned TS6.0.3 bodies (vendor/typescript-6.0.3/lib/_tsc.js):

- checkAliasSymbol: 86029–86137, SHA256 `f38114c6ed2d310327581d9af646d70544ff4e4ae3434764b00d8818bf197779`.

- symbolToString: 50649–50682, SHA256 `483aaf1e4cc4280b31d8e18dab23b7c3bb1ed6966d92ff0e59a80ab3bdb157f5`.

- symbolToNode: 51122–51135, SHA256 `ba015cf97ede8e4493cf851a6464d86bfd06e225e4fed66cae72ea6a2d91ff41`.

- symbolToExpression: 53337–53387, SHA256 `f1c7de91b82f1b2f5a3b4a2e7c1b82bd8504e06172492e073464b298e0938e03`.

- getNameOfSymbolAsWritten: 55541–55588, SHA256 `3ab46f78adf8c8b4e40a35ee7661568f015913f937ce2a575104d21c3428f9f0`.

The adjacent checker unit selectors are node_builder::chains::tests,
node_builder::specifier::tests and node_builder::statements::tests (25 tests at
A6-9; target/h2-8a-export-names-checker-adjacent.log). Verify actual final count.

Execution-count discipline: the shared h2_7c helper catches outside its internal
0..2 loop, so a failure executes once and prints its panic payload twice. Never
count those two log copies as two commands. Launch two separately completed
before jobs at unchanged source for true failed-case repetition. Successful
cases execute twice within each job. The central A6-11 correction governs prior
fresh failure records. Inspect only primary anchored assertion payloads when
comparing the independent jobs; aggregated panic text is a duplicate rendering.

Read-only dependency review also confirmed merge.rs::symbol_name_as_written_slice
uses binder::node_util::declaration_name_to_string for literal declaration names;
node_builder::chains consumes entity_symbol_name_as_written_slice and preserves
its typed single-line display path. Do not replace the global merge helper or
strip quotes at this diagnostic call site.

The before has12 exact in each job (four actual executions each) and12 failures
repeated independently: ten owned diagnostic-name failures and two independent
JS import-conflict getter failures. The two runs take16.30s and17.37s,exit101.
Quoted-before-function is an adjacent positive, not a reproduced name failure.
No quote elision or expected-diagnostic rewrite is allowed. The ten failures
cover JS/TS, single/double quotes, Unicode/underscores and class declarations.

The JS import-conflict controls already retain their imports and produce the
correct diagnostics. Their getter returns lib_1.value (or a quoted property)
instead of the source local value/local. This is a distinct module getter or
semantic substitution owner. It is outside this modules.rs diagnostic edit;
keep both unconditional complete-command failures and inspect their unchanged
first byte mismatch after. Their later declaration fields remain unqualified.
No H2.9 parser recovery case is added by these24 inputs.

Final target:22/24 fresh exact twice with those two JS getter failures retained;
192/200 preceding commands exact twice with eight H2.9 recovery refusals;56/56
name controls exact twice. This is270/280 complete commands exact twice, with
ten explicit outside failures, plus the25 checker unit regressions. These are
targets until measured. Shared-helper failed after commands execute once and
render their payload twice; never claim independent failure repetition from
that duplicate output. Frozen before, first mismatches and source receipts are
in ratchets/h2-8a-alias-conflict-display-before.v1.json and the outside archive
located by target/h2-8a-alias-conflict-display-before-location.txt.

Final measured result:22/24 fresh commands,160/160 preceding ordinary names,
32/40 preceding name maps and56/56 declaration-name controls match completely
twice:270/280 complete commands. All prior positives and all ten owned diagnostic
failures are exact. The two preceding TS2484 commands also become exact.
The two retained JS getter byte vectors and eight H2.9 refusal boundaries are
unchanged. The five compiler tests finish in251.38s,exit101 only
for those ten outside cases. All25 checker statement/chain/specifier units pass
in0.03s,exit0.

This job used normal test output capture: successful test case IDs are taken
from each frozen fixture and its unconditional successful wrapper; failed
wrappers print per-case successes and complete first mismatches. Successful
commands execute twice. Failed commands execute once; duplicate panic output
is not counted as another execution. No membership or expectation changed.
The final record is ratchets/h2-8a-alias-conflict-display-after.v1.json and the
outside source/log archive is located by
target/h2-8a-alias-conflict-display-after-location.txt.
Whole769 replay, other A owners, B–E and hosted acceptance remain open.
