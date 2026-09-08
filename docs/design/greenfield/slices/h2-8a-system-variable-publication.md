# H2.8a A6-11: System variable publication and source ownership

Kind: bounded runtime repair. Trusted checkpoint
`84bf461c3381fac483dc1ad0dbdeba5bec0ab9f6`. H2.8a and H2.8b–e stay open.
The immutable 72-command before has 28 exact twice and 44 failures: 24 first
JavaScript byte mismatches and 20 first sourceMaps-result mismatches (73.03s,
exit101). The preliminary run overlapped formatter completion and is not
qualification; its preserved log is separate. The authoritative before ran after
formatting and all earlier processes ended, with source/fixtures frozen, and
repeats its first comparisons and exact membership.

The allowed production edit paths are `crates/emitter/src/builtins/system.rs`
and, under the A6-11-5 dependency amendment below,
`crates/emitter/src/printer.rs`.
Tests, observers, records, this packet, readiness and parent progress are evidence
paths. The existing private ModuleExportName/common module-info maps are consumed
unchanged. Checker, loader, factory, shared metadata representation,
module selection, host/sink, generated-name allocation and profiles are outside
the production edit set. No runtime edit is allowed until
`scripts/check-system-publication-readiness.py` passes.

| Gap | Producer / current consumer | Repair |
| --- | --- | --- |
| Direct initializer wraps all aliases | createVariableAssignment / transform_hoisted_variable_statement | Publish only the actual declaration name around initialization |
| Aliases append only for non-direct variables | appendExportsOfVariableStatement/BindingElement/Declaration | Append explicit aliases after all initialization expressions |
| Variable append ignores exportEquals | appendExportsOf* / non-direct trailing loop | Apply the existing common guard without suppressing direct initialization |
| Default export inherits an outer source range and loses inline comments | visitExportAssignment/createExportStatement/createExportExpression | Use an unpositioned statement and the typed value/call comment owner |
| Object binding clones away receiver positions | createDestructuringPropertyAccess / create_system_binding_property_access | Reuse the existing receiver node |
| Flatten callback loses location/original identity | flattenDestructuringAssignment.emitBindingOrAssignment / Bind step | Retain binding-element location independently from declared name identity |

A6-11-1 changes SystemBindingStep::Bind from local text plus expression to actual
name identity plus expression and the optional binding-element original needed
by the callback. Evaluate steps remain distinct. Its one push_binding producer
retains the existing name; text is derived only for textual lookup. Direct
exported variables wrap each Bind assignment once with its actual declaration
name using ModuleExportName/create_export_call_with_name. Collector uniqueness
does not choose the direct name. Preserve hoist/inline-expression order, stable
temporaries, default initializer evaluation, nested-block and NoHoisting rules.

A6-11-2 implements the append phase separately. Traverse existing
binding_name_leaves in source order, exclude generated identities as
isGeneratedIdentifier requires, and look up only export_specifiers_by_local.
No initializer appends nothing when exportSelf=false. The existing
common.appends_declaration_exports guard suppresses aliases for exportEquals,
for both direct and non-direct declarations. Clone the declaration name with
its range and NoSourceMap/NoComments, matching getDeclarationName/getName.
A System createExportStatement consumer creates a typed call and unpositioned
statement, sets startsOnNewLine and NoComments unless allowed. Emit all collected
initializers first, then aliases in declaration/specifier order. Assignment/update
getExports wrapping, import setters, function/class and export-star scheduling
are unchanged.

A6-11-3 routes default ExportAssignment through that statement consumer with a
synthesized default identifier and allowComments=true. Reuse the visited value
and existing typed call's NoComments/CommentRange behavior. Do not attach an
original ExportAssignment or an outer range absent upstream. The inline value
comment and omission of the export JSDoc comment must match JS, declarations,
sourceMaps and map writes; the existing ASI trailing-comment control is required.

A6-11-4 completes object-binding provenance reached by the observed patterns.
createDestructuringPropertyAccess uses its existing value as receiver instead
of cloning away its range. Keep computed-name caching, property-name kind,
array/rest algorithms and helper requests unchanged; no complete general
flattener port is claimed. Scalar assignments keep the upstream absent location.
Binding-element callbacks retain the element range on their assignment; their
returned expression (assignment or export wrapper) retains the callback original
independently of its source-map range. Preserve local/Evaluate order and name
allocation. Use the existing non-merging set_semantic_original_node only on
the final callback return, after optional export wrapping. The wrapped inner
assignment receives only its text range; do not transfer original emit metadata.
No printer workaround or emitted-text reconstruction is allowed.

Architecture: E-PROTOCOL, E-RESOLVER-BASE, E-NAMES-BASE, E-ORDER-G and E-ORDER-H
are premise-unchanged; E-METADATA-BASE and E-STRINGS are modified-requalify at
these consumers. The manifest pins current rows. Pipeline composition and hook
registration are unchanged. TransformNode/source/comment/map facts stay in one
TransformArena session; borrowed resolver/factory ownership is retained.
Generated identity comes from existing metadata, not printable prefixes. H0
never runs this module visitor. No changed host/sink/cancellation failure edge
exists, so new fault-injection witnesses are not applicable. Required-child and
factory/resolver failures retain their typed Result boundaries.

The 72 new commands cross nine shapes with JS/TS, ES5/ES2015 and sourceMap on/off:
direct alias, direct plain, local alias, direct object-pattern alias, default
reference, default comments, direct uninitialized, multiple direct declarations
and aliases, and local alias plus export=. JS export= retains upstream TS8003
while emitting; System/ES5 deprecations remain where upstream reports them.
Frozen options are not changed to replace diagnostic streams. This matrix adds
no parse-recovery/H2.9 control. Each TS command repeats twice; observer --check
repeats its complete artifact. Native commands repeat too. A first comparison
failure does not qualify later fields; expected observations stay immutable.

The 200 A6-10 commands at the checkpoint add 178 adjacent positives, ten ordinary
System library and two System library map failures owned here, and ten retained
outside controls: two TS2484 diagnostic-name failures and eight H2.9 recovery
refusals. Their before is A6-10's final outcome, not its intermediate attempts.
Readiness joins all 272 inputs: 206 exact / 66 failed before, with 56 owned
failures. Required target is 262 commands exact twice and ten outside failures
still visible. This is a target, not a measured result. Keep all three fresh
tests unconditional; never exclude/normalize failures or count unsupported as
an exact upstream command.

Run all 72 new and 200 prior commands, the 24 System dynamic-import commands,
all 483 emitter unit tests and all 451 emitter contracts. Their System anonymous
defaults, import ordering, nested scopes, generated automatic JSX references,
destructuring temporaries and ASI comments are required regressions. Require
all former positives to remain exact and inspect retained first-failure vectors.
Whole 769 replay, other A owners, B–E and hosted acceptance remain pending.
Use the user-authorized lightweight workflow; historical full developer CI and
certificate walks are omitted, not claimed. Heavy commands use CARGO_BUILD_JOBS=2
and taskpolicy -b nice -n15. Poll every process to its real exit before canonical
source/oracle/ratchet mutation or dependent commands.

Pinned TypeScript6.0.3 bodies in vendor/typescript-6.0.3/lib/_tsc.js:

- collectExternalModuleInfo: 92779–92919, SHA256 `2694413ce6ea08091a03db3a313b50ee3ff526b065f199270311ac583350220e`.
- System.visitVariableStatement: 112634–112683, SHA256 `6571fc0551b24284b57ab11c666dce1514bf542ed6e8b30fd0ee09e6c8471a0c`.
- System.hoistBindingElement: 112684–112694, SHA256 `5cfda7e141631ac8e23cf754b12753854c8aaec768d5bb40d6f295d9a068155e`.
- System.shouldHoistVariableDeclarationList: 112695–112697, SHA256 `d85ee78c0a0f5ba08c3c02139be7fd2a4f0eeb6477e2dbed72c695330969e608`.
- System.transformInitializedVariable: 112698–112709, SHA256 `835bf789492e2590649ee51848cb96242b5e56961aac941f892a16dc06423955`.
- System.createExportedVariableAssignment: 112710–112718, SHA256 `b18a1dadd58e7336faf27f2048f5d62155f6a0daadbdbc66b8adaecded9fafc3`.
- System.createNonExportedVariableAssignment: 112719–112727, SHA256 `b1778f7e3de6005a8996ba7f953e1bb9866d4942b3fa117786dddd1ef0f37962`.
- System.createVariableAssignment: 112728–112731, SHA256 `ec0fd2758f3f60f309ba8f14012e3b91ace791b776b284e570b6245788de7d72`.
- System.appendExportsOfVariableStatement: 112764–112774, SHA256 `79af9df10be3945871925df800b3c6038e89efc15140785361af04766459d818`.
- System.appendExportsOfBindingElement: 112775–112794, SHA256 `9737ef0e58c8d69a5fa6c295ef84add4fc7fc15dc423f68500ab0f604911926c`.
- System.appendExportsOfDeclaration: 112810–112824, SHA256 `13b49063d17b505f1ac55a6cd3bff563cc556dca34c999d38c7acfc344e7cc38`.
- System.appendExportStatement: 112825–112828, SHA256 `f12c6a36ef06695d7a086b29c67d0e261ec915634e3965275a859f0ab901f1ff`.
- System.createExportStatement: 112829–112836, SHA256 `6d8d7a439b41649c648f5f37b9d3041fa8e2ca5bcf538311f55c1f470c1372a2`.
- System.createExportExpression: 112837–112846, SHA256 `1047eae1c9c391b34504e0e5ec4bacdb1c36d2b2d3ba04b5f672d966769b8394`.
- System.visitExportAssignment: 112564–112575, SHA256 `dd8adf7eef2e11ce15016a5bf672abb4193d6dcc9d4de4e837ac0e5afbeeaaed`.
- getName: 24788–24799, SHA256 `9734f5576b1aa153598ff7ae70a2a2f994bb50d0370fbfc547c47952f72dea33`.
- getDeclarationName: 24809–24811, SHA256 `2774ac8674f5e2cbedb331ad2c3c64fa2474c0df15cd30c16f824775fdb87714`.
- cloneNode: 24436–24466, SHA256 `d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6`.
- isGeneratedIdentifier: 11932–11935, SHA256 `906256d7068a095cb3ebfc51672f5d00f2dcb0dda810d29d90f975f6503f8dfd`.
- createStringLiteralFromNode: 21535–21543, SHA256 `a2fa6c4e9dd96af89655a0a7d44368bcdd05ad599a3ef7e898a2a64e3e5fe9ee`.
- flattenDestructuringAssignment: 93251–93328, SHA256 `8303d862131f74b895085ac8968b52d5d0267330e000e0e91546757aaf278ee0`.
- flattenBindingOrAssignmentElement: 93449–93485, SHA256 `63040df542279c36cc444040317fca2d769b2789c020fca8369bd3935334c11f`.
- flattenObjectBindingOrAssignmentPattern: 93486–93530, SHA256 `8fe5ff016903ce2b5f80496f1016b132a6724907bb2850c53e6bba9b677001c6`.
- createDestructuringPropertyAccess: 93630–93649, SHA256 `d9a831e921e64f0142bcf4826daf4d38068a264ebfba7b35be510fedfa1ed7eb`.

A6-11-5 dependency amendment, activated after the immutable first-after:
64 of the 72 new commands match twice; eight default-comments commands still
fail (four JavaScript bytes, four sourceMaps). Both representations lose the
inline value comment. The typed System call correctly puts NoComments on its
retained value. Printer.emitNodeListItems nevertheless emits a distinct
trailing-position comment phase at each child's getCommentRange.pos, without
consulting that child's NoLeadingComments flag. The native shared intervening
worker incorrectly applies that flag to both list and ordinary-node phases.

Introduce a private typed intervening-comment context replacing the current
opening-line boolean: ordinary node, delimited list, multiline delimited list.
The two list entry points bypass only the child's NoLeadingComments guard for
the list-owned phase. Ordinary node, ordinary leading, sibling-end and trailing
phases retain their guards. Keep the existing multiline/opening-line policy,
CommentResume merging and source-local scan boundaries. NoNestedComments remains
an enclosing suppressed extent; removeComments remains an unconditional guard.
write_source_comment already brackets source text with map positions, matching
Printer.emitTrailingCommentOfPosition. Reuse that writer, without new map facts.
This is a pinned printer-owner correction, not clearing the System value's flags.

E-PRINTER-BASE, E-COMMENTS-G and E-COMMENT-SCOPE-H are modified-requalify at these
consumers. Immutable planning, no checker dependency, pipeline hook composition,
source-local cursor/resume semantics and the immutable three-side comment scope
remain unchanged. No public metadata or factory representation is changed.
The 48 new printer observations cover call/array, inline/newline, six node/parent
flag states and both removeComments polarities, with two identical upstream
prints each. They are printer observations, not extra complete compiler commands.
The before and first-after artifacts remain immutable. Require all 38 former positives to remain exact; the three pure list-flag
failures must become exact, while the array NoLeadingComments case must retain
its independent duplicate-tail failure only. Four ParentNoNestedComments and
three array duplicate-tail cases stay explicit outside this bounded repair.
The target is 41 exact twice / seven retained failed printer observations, not
48 claimed exact. Keep the test unconditional. Also require the existing
complete 72+200+24 commands, emitter units and
contracts, the 30-case comment-scope witness gate and source-comment topology.

- Printer.emitNodeListItems: 120068–120155, SHA256 `ebeb65a71c929bbfdf5d1ebd4b2e7216f15bd37117166ef8fbee5b3a9b0a6b40`.

- Printer.emitTrailingCommentsOfPosition: 121191–121198, SHA256 `953cde198b7f8098bd7bc8d865e535cdac106e983efe35a4a067498ff239cbc0`.

- Printer.emitTrailingCommentOfPosition: 121208–121218, SHA256 `78fd8227de7e58556e3f2906ffe5aafb334a35d70f8dc0f916bed71b16cb78ea`.

The printer amendment before is 38 exact twice / 10 failed twice;
its complete raw mismatch vectors and source snapshot are archived outside the
checkout through target/h2-8a-list-comment-flags-before-location.txt.

Execution-count correction (supersedes earlier failed-repeat wording):
the shared complete-command helper catches outside its two-run loop. A failing
assertion aborts the first execution; the panic hook and aggregate assertion
render its payload twice. Successful cases execute twice. Therefore the72-case
before has28 twice-exact and44 failures observed once, the first-after has64
twice-exact and8 failures once, and the second-after has68 twice-exact and4 System
failures once. Repetitions=2 fields derived from duplicate failure payloads mean
two log renderings, not two native failed commands. The same correction applies
to prior export-name fresh failure records using this helper. Case counts,
comparison bytes, successful repeats and frozen TS observations are unchanged.
The original-corpus tuple runners and the standalone48-printer-case loop are
unaffected. The correction and exact source/log hashes are in
ratchets/h2-8a-system-publication-execution-correction.v1.json. Historical
artifacts remain immutable. New token-comment before evidence uses two separate
native jobs, so its failed cases have two actual independent executions.

A6-11-6 dependency amendment, after the complete second-after:
68/72 new System commands are exact. All JavaScript and sourceMaps comparisons
match; four TS default-comments commands first fail on their declaration bytes.
The 200 prior commands have190 exact,24 dynamic import commands match,483 units,
451 existing contracts and18 source-comment topology contracts pass. New printer
controls have41 exact and the seven previously recorded independent failures.
This intermediate result is immutable; it does not qualify all72 commands.

The declaration printer already requests onlyPrintJsDocStyle=true. Its fixed
token trailing callback and cursor-leading comment worker incorrectly ignore
that option. Printer.emitTokenWithComment delegates leading positions to
Printer.emitLeadingCommentsOfPosition and ordinary trailing positions to
Printer.emitTrailingComment, which calls Printer.shouldWriteComment. The native
predicate already recognizes JSDoc-like and pinned comments exactly. Reuse it.
The hypothesis of System metadata leaking into declarations was excluded by
separate declaration-only upstream probes; no cross-arena metadata transfer is
needed or allowed.

Add a private filtered form of emit_source_trailing_comments_of_position. The
existing unfiltered wrapper stays for list and other unchanged consumers; list
callbacks must not acquire this policy. Pass only_print_js_doc_style from the
fixed-token ordinary trailing branch and emit_comments_at_cursor_with_anchor.
Also pass that option to the latter's leading-position writer. Its other two
callers complete a final child's ordinary comment boundary and an identifier
name's ordinary comment boundary; Printer.emitTrailingCommentsOfNode and
Printer.emitIdentifierName use the same filtered ordinary pipeline. Preserve
JSX's separate callback, token/source-map positions, cursor/resume advancement,
removeComments, indentation, immutable scope and required-child errors. Do not
filter in write_source_comment, because list-owned callbacks remain unfiltered.
No declaration transform, factory, checker, option or public API change.

The new54 complete commands have42 exact twice and12 declaration-byte failures
across two independent before jobs (48.07s and the recorded repeat,exit101). The failures are TS regular block/line comments in
System/CommonJS/ESNext, with combined emit and declaration-only emit. Controls
cover JS, JSDoc, pinned comments, token-leading and token-trailing positions;
combined emit includes full sourceMaps and map-write observations. Six additional
export-list declaration-only commands preserve list-owned non-JSDoc comments.
The observer repeats every TS command twice and passes --check; native repeats
every successful command internally; two separately launched jobs provide
two actual failed executions per case. The first wrong test selector ran zero tests and is preserved as
unqualified, not a before result. Full first-mismatch vectors and all sources
are archived through target/h2-8a-declaration-token-comments-before-location.txt.

Require all54 new and all72 System commands exact twice,190/200 prior commands
and24 System dynamic import controls, with the same ten named outside failures.
This is350 complete commands, targeting340 exact twice. Rerun483 units/451
existing contracts and18 source-comment topology contracts; retain all41 current
printer positives and the seven known outside failures. E-PRINTER-BASE,
E-COMMENTS-G, E-COMMENT-SCOPE-H and the existing declaration-printer option seam
are requalified at the consumers; existing architecture dispositions are retained.
No full printer/global/full-CI/hosted/H2.8 closure claim follows from this target.

- Printer.emitTokenWithComment: 118731–118764, SHA256 `d7df39bba502705facedce379a636ae55b168b77701df5e72abf10ec444c2e50`.

- Printer.shouldWriteComment: 121145–121150, SHA256 `9585a2c5cae9ab168b146d094a846dae3f07e50b659fd70090364a9be630bf29`.

- Printer.emitTrailingComment: 121179–121190, SHA256 `14654b8999872d42159a4d2c11a27fb01fbe47c50e8131b82de8453536d60394`.

- Printer.emitLeadingCommentsOfPosition: 121166–121175, SHA256 `fa23b688b1540c772ccf513c874d47bba4a08a44e019bc430feb79cbea73d2cd`.

- Printer.emitTrailingCommentsOfNode: 121033–121046, SHA256 `e5c99d84eeab2c12d594ba56695a7a869c49720eb110f3715c0ea3f9271d1112`.

- Printer.emitIdentifierName: 117149–117157, SHA256 `847193fac9ff770a8b062033c89f8676a42ac88e0adb16b22fdafb5a33c09ae4`.

Final bounded validation: all 72 new complete commands match twice, including
JavaScript, declarations, maps, callback metadata, diagnostics, status and result
presence. The 200 preceding commands have 190 exact twice (158 ordinary, 32 map);
all 178 preceding positives remain exact. The ten retained failures are precisely
two TS2484 name diagnostics and eight H2.9 recovery refusals. Their first mismatch
vectors/refusal boundaries remain unchanged. The 24 System dynamic import
commands also match twice. These five tests take 319.15s, exit101
only for the ten retained outside failures: 340/350 complete commands exact twice.

All54 new declaration-token commands match twice, including all42 before
positives and the six unfiltered export-list controls. The54-command before
used two separate native jobs; its12 failures were independently repeated.

All 483 emitter units and 451 existing emitter contracts pass, including the
comment-scope witnesses. The separate18-test source-comment topology target passes.
The new printer controls have 41 exact twice and seven retained failures:
four inherited NoNestedComments scopes and three duplicate array trailing
comments. All 38 former positives remain exact. The overlapping array
NoLeadingComments control now retains FIRST and fails only for its independent
duplicate tail; the other six retained vectors are unchanged. The printer test
remains unconditional and exits101. No general printer suite pass is claimed.

Both intermediate after snapshots and all before snapshots remain immutable.
Failed fresh commands using the shared helper execute once; duplicate panic
renderings are not independent repetitions. The execution-count correction
remains authoritative, and successful commands still execute twice. Final source, logs,
and vectors are archived outside the checkout through
target/h2-8a-system-variable-publication-after-location.txt; the final record is
ratchets/h2-8a-system-variable-publication-after.v1.json. No expectation or
membership changed. Whole769 replay, other A owners, B–E and hosted acceptance
remain pending. This is a bounded checkpoint, not H2.8a or H2.8 completion.
