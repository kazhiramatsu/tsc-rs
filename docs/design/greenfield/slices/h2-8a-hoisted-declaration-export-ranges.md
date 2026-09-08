# H2.8a A6-19: hoisted declaration export source-map ranges

Before frozen at2f7f297c13046fb569bbb199b39393b4a235a3e5; readiness owns implementation activation.
Do not edit production until168 complete TS commands repeat, two independent
native before jobs finish, exact source pins and actual failure dispositions
are recorded and the readiness checker passes. Initial production path:
crates/emitter/src/builtins.rs; the System consumer amendment below adds system.rs. No printer, checker, factory or arena API edit.

A6-18 preserves74 fresh outside map failures. One isolated component is local
class export publication: class Foo followed by export { Foo } emits exports.Foo
mapped to the entire original class rather than exportSpecifier.name. The same
native producer is also used for prologue function publications. Native
CommonJsModuleInfo::hoisted_declaration_exports returns only exported names and
therefore discards direct-declaration versus explicit-specifier location.
The collector stores each function publication with original_declaration;
the class visitor also assigns original_declaration to every publication.

TS appendExportsOfHoistedDeclaration first suppresses publication for exportEquals,
then publishes the direct export using getLocalName and the declaration location.
It appends explicit aliases using getDeclarationName and each exportSpecifier.name
as location. createExportStatement applies setTextRange to the statement only,
sets startsOnNewLine and NoComments, and leaves the expression location absent.
Retain source identities through collection/materialization; never repair maps
or statement text after printing, or recover provenance by matching rendered text.
Direct-first dedup must keep its direct location when an explicit name duplicates
it. Collector uniqueExports already filters explicit names; do not widen or alter
that producer solely from appendExportStatement's StringLiteral exception.

A6-19-1: replace the name-only private hoisted-publication result with a typed
plan retaining direct declaration versus explicit export-name source identity.
Carry this same plan through both the function collector and class visitor.
Preserve exportEquals, collection order, direct/default naming, dedup and
function-prologue versus class-after-declaration placement. Generated anonymous
default names retain the module collector's existing allocation; no second
getGeneratedNameForNode request or text-based alias allocation is allowed.

A6-19-2: materialize each publication statement using its plan's exact location,
startsOnNewLine and NoComments. Do not copy a declaration's unrelated metadata
onto an assignment or overwrite the exported name's syntax provenance. Keep
variable/binding declaration plans and import/external re-exports unchanged.
They remain unconditional adjacent comparisons.

A6-19-3 (the complete fresh before demonstrates this reached value dependency):
preserve source Identifier spelling and factory declaration/local-name semantics
for publication values. Ordinary named references use the existing cloned,
ranged NoSourceMap|NoComments identity. Direct values additionally carry LocalName;
explicit aliases follow existing declaration-reference substitution. Existing
create_import_publication_reference owns the borrowed resolver order and may be
reused/refactored privately, but new direct names must not introduce unnecessary
queries, and synthesized default names must not be passed as parsed resolver
nodes. A generated/fallback local is distinct from a parsed source identifier.
Keep the existing allocation, and preserve prior import references' exact behavior.
No resolver API or general factory/printer name consumer change is proposed.

The168 commands cover14 shapes, classes/functions, CJS/AMD, ES5/ES2015/ES2022:
direct/local/renamed/quoted/default named+anonymous/multiple/direct+explicit,
escaped alias/direct names, export-before-declaration, comments, duplicate quoted
aliases and exportEquals controls. All diagnostics, declarations, raw maps,
output paths/order/callback fields, result/status/exit are unconditional. The
112 commands with TS5107 retain140 output-control diagnostic rows; the remaining
56 are active semantic controls. TS2323/2484/2300/2309 in the intentional duplicate
and exportEquals cases are retained, not normalized or discarded. No native
success count is presumed from those source shapes.

The source-map recorder/printer protocol is unchanged. E-METADATA-BASE and
E-NAMES-BASE are modified-requalify at the private range/reference consumer.
E-ARENA, E-PROTOCOL, E-RESOLVER-BASE, E-PRINTER-BASE remain premise-unchanged, with
actual adjacent output and lifecycle tests required. Do not inherit the dormant
historical E-MAPS row as a compatibility claim. Existing source identities,
metadata channels, typed errors and scoped resolver borrows remain authoritative.
No new host or sink failure mode is introduced; current resolver errors propagate.

Before:observe168twice, --check168twice, native two independent jobs. Reuse the
existing complete compiler comparator unchanged. A failed case executes once in
a job even though its panic is rendered twice; two independent before jobs are
required for failed-case repetition. Every job must reach real exit before
canonical mutation or a dependent build. Complete cases stay in the denominator
with explicit outside owners if a first failure is independent of this producer.
After selection/counts must be set from the measured before. Required adjacency
includes all104 A6-18 static-map cases,87 import publication references, relevant
module-name/default/variable export controls, original public class/function
commands, emitter lib/contracts and existing resolver queries if reached. Do not
infer any new global769 total. Remaining A work, B–E and hosted acceptance remain
open; the schedule header's lightweight workflow applies and no historical full
CI/certificate walk is claimed.

Both measured before jobs are64 exact/104 source-map failures (178.25s and163.14s,
exit101), with identical first vectors. All24 escaped/direct-escaped controls
fail. Twenty are hoisted publication values; the four ES5 class controls also
lose escape spelling at the earlier prototype receiver and IIFE return. They
have been transformed into variable declarations before this producer runs and
remain outside as es2015-declaration-name-raw-range. The corresponding helper
es2015.rs:get_name5105+ deliberately retains only map/comment range on a clone,
leaving raw positions synthesized; its prior property-access line-break premise
needs a separate owner and fresh multiline controls. A19 does not edit that path
or the variable publication consumer. Those four first vectors must remain
identical after the hoisted repair. Target fresh164/168, with the four outside
commands retained and later fields unqualified; this is unmeasured until after.

A6-18's104 controls have35 retained failures whose only remaining component is
local class export publication. Thus the planned A19 projection targets65/104
for that existing complete fixture, retaining39 other maps. All remaining map
point differences must be subsets of A6-18's frozen final after. No whole-command
success is inferred from this component target; the complete comparator still
runs on all104 and every newly exposed later boundary must be handled explicitly.

The final Rust representation uses private HoistedDeclarationExports groups:
original declaration, a HoistedDeclarationName (Source node or existing allocated
spelling), and ordered HoistedDeclarationExport entries. Each entry retains its
ModuleExportName and HoistedExportOrigin (DirectDeclaration or ExplicitSpecifier).
Explicit location is the existing exported name's syntax node, consumed by the
already qualified set_explicit_export_statement_location. Direct location is the
original declaration and receives the same startsOnNewLine/NoComments policy.
The producer returns no group for exportEquals or an empty publication list,
before inspecting a publication value. Preserve the existing dedup order.

Capture the class's existing name before inserting an anonymous default fallback;
otherwise that synthesized identifier could be mistaken for a source name. An
ordinary nongenerated identifier whose original belongs to the mounted parse
interval becomes Source; absent/generated/unanchored names retain the module
collector's existing allocated spelling. This native ownership distinction does
not request a second generated name or fabricate a checker identity. Source direct
values clone/range with NoSourceMap|NoComments|LocalName. Source explicit values
reuse the existing declaration-name reference/substitution path. Allocated values
preserve the previous generated spelling and bypass parsed resolver queries.

Extract the ordinary clone/range/flag block from create_import_publication_reference
into one private clone_declaration_publication_name helper, with explicit additional
flags. Rename that shared reference consumer create_declaration_publication_reference
now that it serves hoisted aliases too. Its generated fallback, NoSubstitution /
LocalName bypass, export-container-first then import-binding query order, access
range placement and typed errors remain identical for existing imports. Direct
values add LocalName via the pure clone helper and do not query the resolver.
A shared materialize_hoisted_declaration_exports consumes each group; the source
transform flattens function groups in prologue order, while the class visitor
appends its group's statements after the updated class. No public type or API.

Complete decoded review of all104 before maps verifies that all100 owned cases
have differences exclusively on export publication lines. The four ES5 class
controls have earlier method/return differences and retain the separate owner.
All non-mapping map fields agree. Later whole-command fields are still unqualified
for each failed before; publication-line localization is not whole-command success.
The full first vectors are immutable in the before archive; current readiness
stores each first decoded block plus every affected generated line for navigation.

Required after:168 fresh +104 static initializer maps +20 literal keys +87 import
publication references +40 export-name syntax maps +48 export specifier names
+8 CommonJS default re-exports +10 original static/class commands =485 complete
commands. Target434 exact twice:164+65+20+87+32+48+8+10. Retain4 fresh ES5 name
maps,39 static maps and8 preexisting H2.9 recovery refusals (51 failures) in the
complete comparisons. The combined compiler job is expected to remain exit101;
never describe it as a passing suite. Repeat emitter483 units/451 contracts and
the existing18 resolver query cases plus25 adjacent checker units. Counts remain
targets until measured; every positive must survive and new first/later failures
must be resolved or explicitly retained. No fixture or comparator normalization.

Pinned complete TS6.0.3 functions (AST-verified declaration boundaries):

- getOriginalNode: 11400–11410, SHA256 `e6e639e966314faf444b9b68796893745ffb06eb0adcf1180d6935332d8797a3`.
- isParseTreeNode: 11423–11425, SHA256 `d8a6d217a3087e6809bfb3df3a4815eefce954e8175aed4c744b515f891dbe8d`.
- getParseTreeNode: 11426–11437, SHA256 `80b5c2449cb8320cf209184a8eef484f944379da161e54d74dd54ed1f0d2d592`.
- cloneNode: 24436–24466, SHA256 `d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6`.
- getName: 24788–24799, SHA256 `9734f5576b1aa153598ff7ae70a2a2f994bb50d0370fbfc547c47952f72dea33`.
- getLocalName: 24803–24805, SHA256 `db85ef71236480d7de1d2e131b01d6f8fed272ef41d0f5297ce7fb3485ee7979`.
- getDeclarationName: 24809–24811, SHA256 `2774ac8674f5e2cbedb331ad2c3c64fa2474c0df15cd30c16f824775fdb87714`.
- setTextRange: 28256–28258, SHA256 `b4484231223d27d5a3bd94103656b0f7ee336d397d3f1af742f67f893bd1cc08`.
- getReferencedExportContainer: 87870–87899, SHA256 `64fe010400264ccd927bf7b73511da7c480cf87f60b40ed5c8f43c8090aea8eb`.
- getReferencedImportDeclaration: 87900–87917, SHA256 `32efa07ac60055a482669428125107b75b8241d4362602ff91d352d6d9405b11`.
- collectExternalModuleInfo: 92779–92919, SHA256 `2694413ce6ea08091a03db3a313b50ee3ff526b065f199270311ac583350220e`.
- appendExportsOfHoistedDeclaration: 111722–111742, SHA256 `bfbaa381d44c81a747e6de4dd148b3d0dc129a1d6e350138cee1ced097b4a3f7`.
- appendExportsOfDeclaration: 111743–111762, SHA256 `b1e3c0856abab75bf412486742c157c5a6b6a7a41fd293c039ba0936fa70cad2`.
- appendExportStatement: 111763–111772, SHA256 `b056e7b5768b85a874f11ae38b31c178a5efaef3b66df881480b462ec4e80610`.
- createExportStatement: 111791–111804, SHA256 `d533ed2215809bab955e6b206a545e71c1a8490754d2bb396fec41aca15000cc`.
- createExportExpression: 111805–111851, SHA256 `75fd880a658644ec017e38813933a1710d9f1ec7929387c8755990e3d6c9fbf8`.
- substituteExpressionIdentifier: 111946–111989, SHA256 `972830b79228dc51aaec4b3b13ebd2a12795701304627fef3bdc5ba8b7ab3a96`.

The first candidate failed to compile (E0061/E0308), with zero native commands
executed. Its source, original packet/readiness and build log are frozen in
ratchets/h2-8a-hoisted-declaration-export-ranges-compile-failure.v1.json. The initial
caller inventory missed System's transform_hoisted_function, transform_hoisted_class
and create_export_star_prelude, which also consume CommonJsModuleInfo. Correct the
dependency closure before editing those consumers; no complete comparison is
claimed for the failed build. The immutable two-job fresh before is unchanged.

The runtime path set now includes crates/emitter/src/builtins/system.rs. Adapt
both System function/class calls to pass original declaration identity, flatten
the optional group, and consume each publication.name in existing order. System
continues to construct its own local value and export call with unchanged flags,
locations, generated allocation and resolver behavior. It does not invoke the CJS
materializer or adopt CJS statement locations. The export-star exclusion prelude
flattens the ordered function groups and consumes only publication.name. This
retains the prior sequence and push_unique predicate, without duplicated name
storage, a second collector or a parallel compatibility representation.

Add all72 existing System variable-publication and24 dynamic-import complete
commands to the required after. This makes581 total, target530 exact twice and
51 retained failures. The original485 selection remains unconditional. The System
controls are existing frozen expectations, and the emitter lib/contracts also
exercise System declaration hoisting, default names and export-star exclusions.
No fresh System compatibility expansion is claimed by this representation change.
The full source closure check must scan all emitter source files for both the
method and field consumers so a private cross-module use cannot be omitted again.

Additional complete TS System consumer functions (AST-verified):
- transformSystemModule.addExportStarIfNeeded: 112245–112316, SHA256 `c0d7318ab3996c48f9b0868cf4b7755b202767fd2a4720fac382253fd83e897a`.
- transformSystemModule.visitFunctionDeclaration: 112576–112604, SHA256 `c9849240c0fe34698d20c78bc1e0e89f5a6b06d37b7e9133450057840c271667`.
- transformSystemModule.visitClassDeclaration: 112605–112633, SHA256 `de6afa6b3825a73326606b89f43e7620cddea99d7de289df7990f87fdfd3ad69`.
- transformSystemModule.appendExportsOfHoistedDeclaration: 112795–112809, SHA256 `99fef8ae7429a78f3b3e5c8edb7d4033beac0eb6c5f9253a4910352e50dba94e`.
- transformSystemModule.appendExportsOfDeclaration: 112810–112824, SHA256 `13b49063d17b505f1ac55a6cd3bff563cc556dca34c999d38c7acfc344e7cc38`.
- transformSystemModule.appendExportStatement: 112825–112828, SHA256 `f12c6a36ef06695d7a086b29c67d0e261ec915634e3965275a859f0ab901f1ff`.
- transformSystemModule.createExportStatement: 112829–112836, SHA256 `6d8d7a439b41649c648f5f37b9d3041fa8e2ca5bcf538311f55c1f470c1372a2`.
- transformSystemModule.createExportExpression: 112837–112846, SHA256 `1047eae1c9c391b34504e0e5ec4bacdb1c36d2b2d3ba04b5f672d966769b8394`.

Final measured result:164/168 fresh complete commands are exact twice. The four
ES5 escaped class controls retain byte-identical first map vectors from both
before jobs. All100 owned fresh failures and all64 prior positives now complete
exactly. A6-18's static-map fixture improves from30 to63/104 exact twice; all41
remaining maps have difference-point multisets contained in A6-18's frozen final
after, and non-mapping map fields agree. No new point differences are introduced.

The original target was530/581 (65/104 static maps), but the complete result is
528/581 (63/104). Two ES2015 set/define static-super cases had been classified
in the prior component inventory as local-export-only. Decoded full-map review
shows that each already also contained a Reflect.get receiver endpoint mapped
to source column83 instead of78. The export-statement differences are now gone,
while those exact earlier endpoint differences remain. The correction record
ratchets/h2-8a-hoisted-declaration-export-ranges-static-super-disposition.v1.json
preserves both sets of decoded points and log identities. Neither the A6-18
frozen record nor any expected output is rewritten. The target shortfall stays
explicit in the final analysis, and these two commands remain failed with a
separate static-super-reference-endpoint-range owner.

All20 literal-key,87 import-reference,48 export-specifier,8 CommonJS default-name,
72 System variable-publication,24 System dynamic-import and10 original static/class
commands are exact twice. Export-name syntax maps retain32/40 exact twice with the
same eight H2.9 recovery refusals as A6-14. Overall528/581 complete commands are
exact twice. Each of the45 map failures and eight refusals executes once in this
job, and later complete-command fields remain unqualified. No failed commands
were removed, retried as if successful, or normalized to reach these counts.

Compiler contracts report6 passed/3 failed in799.78s and the original test
passes in39.12s; the combined invocation retains exit101. Emitter483 units
and451 contracts pass (0.61s/1.6s,exit0). Checker26 units, including
18 resolver-query cases repeated twice and25 adjacent statement/chain/specifier
units, pass (0.03s,exit0). The shared import reference and System consumers
are requalified by these final source bytes. The initial compilation failure
remains immutable and contributes zero command executions.

This closes the hoisted declaration publication range/reference owner. It does
not close the four earlier ES5 class-name failures, the41 other static-map owners,
H2.9 recovery, remaining A work, B–E, final whole-matrix replay or hosted acceptance.
The global769 A6-14 checkpoint remains unchanged; focused improvements do not
establish a new whole-matrix count. No historical full developer CI or certificate
walk result is claimed. The final record is
ratchets/h2-8a-hoisted-declaration-export-ranges-after.v1.json.
