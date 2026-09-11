# H2.8a A6-10: retain module export-name syntax and provenance

Kind: bounded runtime repair. Trusted checkpoint a94496bc85e934554c50f777bd08f66e020a9275.
Before activation, 160 complete fresh commands have 50 exact twice and 110
failures (101.95s, exit101). Forty sourceMap commands have two exact twice,
30 map-result differences and eight H2.9 recovery refusals (27.09s, exit101).
No runtime edit is allowed until check-export-name-syntax-readiness.py passes.
The parent H2.8a profile and whole H2.8a–e task stay open.

This repair owns retained name kind/text/identity across one module transform
and its publication consumers. Runtime paths are crates/emitter/src/builtins.rs
and crates/emitter/src/builtins/system.rs. Test/evidence paths are the two fresh
fixtures/observers/comparators, their contracts.rs registration, existing exact
command test projection, packet/readiness/outcome records and parent progress.
Checker, printer, factory, metadata representation, module selection, package
resolution, generated binding finalizer, profiles, original expected observations
and host/sink/command boundaries are outside the production edit set.

Required current architecture references are E-PROTOCOL and E-RESOLVER-BASE
(premise-unchanged), E-METADATA-BASE and E-STRINGS (modified-requalify at their
consumers), E-NAMES-BASE, E-ORDER-G and E-ORDER-H (premise-unchanged). The current
architecture rows are pinned by this manifest. Existing TransformNode identity,
TransformArena session side tables, NodeFactory clone/update and the borrowed
EmitResolver remain the integration boundary. Literal text-source metadata is
already available through EmitMetadata::set_string_literal_text_source. The
repair keeps original, comment, map, cooked string and generated binding identity
separate; it adds no public query or ownership transfer. E-ORDER-H's earlier
registration/hook qualification is retained; no ordering/enablement is changed.
Source/literal facts come from the existing transformed node, including synthetic
nodes produced by decorators. Scanner facts are not inferred from source text.

| Current gap | Producer and owner | Consumer / lifetime | Disposition |
| --- | --- | --- | --- |
| CommonJsModuleInfo reduces export names to Box<str> | collect, add_exported_name, add_export_mapping, add_export_specifier_mapping, add_exported_binding | prelude, declaration/import/assignment publication; one transformed source request | missing kind and existing node identity |
| hoisted_function_exports and generated binding export lists lose the same fact | hoisted_declaration_exports, CommonJsFileLevelGeneratedBindingExports | function prelude / generated assignment plan in the same request | same shared prerequisite |
| preinitializer chooses access from identifier validity | CommonJsVisitor module body | emitted prelude; synthesized name nodes | partial, must select from actual name kind |
| ordinary publication creates accesses from text | declaration/import/class/direct-variable/update plans | assignment access; cloned existing export name | partial, preserve clone and source range roles |
| external member getter chooses from text | visit export declaration | propertyName or name node on reexport | partial, use existing source member node |
| declaration value clone lacks getName flags; statement uses specifier range | create_declaration_export_statements and direct alias publication | source-map result and map write | partial, exact declaration-name flags and export name location |
| System export call always creates a literal from text | SystemVisitor::create_export_call and typed-name callers | source literal or Identifier-derived literal | partial, preserve existing literal node |
| System direct declaration wraps all aliases in initialization | System variable statement visitor | library JS and maps | independent next publication-order amendment |
| TS2484 formats a quoted declaration name without quotes | checker diagnostic producer | two CJS ES2015 diagnostics | independent next checker owner |
| ES5 extended supplementary Unicode identifier is invalid upstream | existing parser/recovery boundary | eight sourceMap commands | later H2.9, retain refusal |

A6-10-1 introduces a private ModuleExportName representation pairing unescaped
text with an exhaustive syntax origin: ExistingNode(TransformNode) or
SynthesizedIdentifier. ExistingNode includes parsed and transformed synthetic
nodes. Provide typed construction from an arena name node and construction for
existing synthesized textual identifier roles. Text remains the explicit key for
collector uniqueness; do not derive node-identity equality and silently change
first-admission order. Collection retains the actual name for export specifiers,
namespace exports, named class/variable bindings and function export names.
Default declaration/generated fallback roles retain their current text allocation.
Migrate CommonJsModuleInfo's exported_names, exports_by_local,
export_specifiers_by_local, exported_bindings and hoisted_function_exports;
CommonJsFileLevelGeneratedBindingExports; ImportReExportPlan,
DeclarationExportPlan, ExportAssignmentPlan and CommonJsDirectVariablePlan.
Update the nonempty-only variable_list_has_initialized_export_binding signature.
System consumers of common info carry or explicitly project this typed name.
Preserve generated binding IDs, resolver lookup order and each existing collector
admission/uniqueness predicate. Missing/malformed required child still returns
the existing typed error; no fallback guesses syntax kind from name text.

A6-10-2 separates name materialization by upstream role. Prelude names are newly
synthesized StringLiteral or Identifier nodes selected from retained name kind;
do not clone or attach source ranges to them. Keep existing output-name order and
CommonJS chunk size50. The observed matrices have fewer than50 exports and do
not qualify the independent native AMD/UMD chunking discrepancy. Ordinary export
assignment clones an ExistingNode and selects element access only for its actual
StringLiteral kind; synthesized identifier roles remain property accesses.
Reuse this materializer for hoisted/class/import/direct-variable publication,
assignment/update/destructuring export plans and namespace exports. Keep direct
primary declaration behavior and explicit aliases distinct. Existing A1 direct
export name materialization retains its separate positioned-name/escape behavior.
Live getter export names use a new StringLiteral with existing typed text-source
metadata, matching createStringLiteralFromNode. External reexport member access
uses the actual propertyName or name node directly, without cloning, and selects
property versus element access from kind. Preserve __importDefault receiver
wrapping, helper request/provenance, module binding identity and source/statement
ranges. Existing ImportBinding property_node and its clone owner stay unchanged.

A6-10-3 models publication map/comment roles. The local declaration value clone
retains its copied name range and receives NoSourceMap and NoComments, as
getDeclarationName/getName do with absent allowances. The explicit export
statement's map range is exportSpecifier.name, not the whole ExportSpecifier;
use the same source-name range for direct-variable alias publication. Name clones
in ordinary export access have synthesized ranges. Do not manufacture a source
range on the access itself when upstream createExportExpression has no location.
Keep original and comment identity separate from this map range. Existing direct
primary declaration ranges and expression result/temporary ordering are retained.
Changes must match complete sourceMaps payloads and map bytes, not just JS text.

A6-10-4 changes System name consumption while retaining its statement scheduling.
A literal ExistingNode is passed as the export call's name argument unchanged.
An Identifier ExistingNode creates a StringLiteral with its text-source metadata;
a synthesized identifier role creates the existing synthesized literal. Carry
names through import bindings, hoisted declarations, assignments/updates and
shared declaration name mappings. Preserve current initializer/alias publication
ordering for the separate System amendment. Do not claim its ten ordinary and two
map commands exact from this change. Apply the existing NoComments/value comment
range semantics of System createExportExpression where required by the pinned
name materializer; no printer or factory implementation changes are authorized.

All four steps preserve the live borrowed resolver and existing source session.
The source/metadata flags are not process-global, cached across programs, or
reconstructed from emitted text. Missing resolver capability and sink failures
keep their existing Result boundary. No changed sink/host/cancellation branch is
introduced, so new fault-injection witnesses are not applicable. H0 no-emit never
constructs this module info. Heavy commands use CARGO_BUILD_JOBS=2 and taskpolicy
-b nice -n15; observations and canonical mutations remain serial.

The 160 controls cross 16 shapes with JS/TS and CJS ES5/ES2015 plus AMD/UMD/System
ES5: quoted default/identifier/Unicode/nonidentifier; function/class; duplicate
spelling order; external default/named/Unicode; namespace; import publication;
postincrement; direct versus quoted declaration order. Forty map controls add
single-quoted local/member/export/update names and extended Unicode declarations.
Every TS command is observed twice and preserves complete writes, callback
metadata, diagnostics, sourceMaps/result presence, status and exit. Native exact
commands repeat twice; a failed command reports its first complete comparison
mismatch. The original fixture options, membership and expected bytes are frozen.
The first map attempt included two test artifact-kind projection failures;
its immutable harness-before record is retained. The existing comparison now
recognizes the observer's source-map spelling only for JavaScriptMap, without
changing expected values, compiler options or production behavior.

Potential bounded after is 148/160 ordinary and30/40 map commands exact, with
explicit independent/later failures still visible. These are required targets,
not measured results. A failed first field does not qualify subsequent fields.
The ordinary test remains unconditional for all160 and maps for all40; never
exclude a failing case or convert output to the expected representation. Run
both full fresh tests plus all56 A6-9 export-name commands. Require all formerly
exact witnesses to remain exact and all owned name-provenance commands to match.
Run A1's module_export_identifiers controls and System dynamic-import controls,
all emitter unit tests (current483) and contracts (current451), and the original
ten class/alias commands, retaining their two known ClassInstance2 declaration
failures. Review complete failure vectors where available. Whole769 replay,
other A owners, B–E and hosted acceptance remain pending. The current schedule
omits historical full developer CI and certificate walks; do not claim them.

Pinned upstream TypeScript6.0.3 bodies (vendor/typescript-6.0.3/lib/_tsc.js):

- collectExternalModuleInfo: 92779–92919, SHA256 `2694413ce6ea08091a03db3a313b50ee3ff526b065f199270311ac583350220e`.
- collectExportedVariableInfo: 92920–92938, SHA256 `960c2ef0d76f7361d2b9d09669cac8a7388d021592d67e4703dd91b72a92ce7e`.
- transformCommonJSModule: 110167–110204, SHA256 `a48c215e8304107fec1f4e113f74a13f720cb5901cff5287bda673a98e082749`.
- transformAsynchronousModuleBody: 110492–110534, SHA256 `8d0713be5aaf1b99c2e7e304e5e5f341884d2c0a865aefc1336c96f9cb98d9c9`.
- visitTopLevelExportDeclaration: 111366–111455, SHA256 `0ce1300a6f4496d22633196858f20948e9768ae03c4aa9a7794a7d4ef4411e29`.
- appendExportsOfImportDeclaration: 111651–111683, SHA256 `65a02d44f2100e9793cad4f3fef13ea84611f16ce6c0a660e91eabc665ff8de0`.
- appendExportsOfHoistedDeclaration: 111722–111742, SHA256 `bfbaa381d44c81a747e6de4dd148b3d0dc129a1d6e350138cee1ced097b4a3f7`.
- appendExportsOfDeclaration: 111743–111762, SHA256 `b1e3c0856abab75bf412486742c157c5a6b6a7a41fd293c039ba0936fa70cad2`.
- appendExportStatement: 111763–111772, SHA256 `b056e7b5768b85a874f11ae38b31c178a5efaef3b66df881480b462ec4e80610`.
- createExportStatement: 111791–111804, SHA256 `d533ed2215809bab955e6b206a545e71c1a8490754d2bb396fec41aca15000cc`.
- createExportExpression: 111805–111851, SHA256 `75fd880a658644ec017e38813933a1710d9f1ec7929387c8755990e3d6c9fbf8`.
- createExportStatement: 112829–112836, SHA256 `6d8d7a439b41649c648f5f37b9d3041fa8e2ca5bcf538311f55c1f470c1372a2`.
- createExportExpression: 112837–112846, SHA256 `1047eae1c9c391b34504e0e5ec4bacdb1c36d2b2d3ba04b5f672d966769b8394`.
- createStringLiteralFromNode: 21535–21543, SHA256 `a2fa6c4e9dd96af89655a0a7d44368bcdd05ad599a3ef7e898a2a64e3e5fe9ee`.
- getName: 24788–24799, SHA256 `9734f5576b1aa153598ff7ae70a2a2f994bb50d0370fbfc547c47952f72dea33`.
- getLocalName: 24803–24805, SHA256 `db85ef71236480d7de1d2e131b01d6f8fed272ef41d0f5297ce7fb3485ee7979`.
- getExportName: 24806–24808, SHA256 `c1ee333f4053e36dfbfa40b9ad6057e9f6081253c458f0fc529c931626263751`.
- getDeclarationName: 24809–24811, SHA256 `2774ac8674f5e2cbedb331ad2c3c64fa2474c0df15cd30c16f824775fdb87714`.
- cloneNode: 24436–24466, SHA256 `d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6`.
- substituteExpressionIdentifier: 111946–111989, SHA256 `972830b79228dc51aaec4b3b13ebd2a12795701304627fef3bdc5ba8b7ab3a96`.
- substituteBinaryExpression: 111990–112008, SHA256 `9196473ad503744f7c3e00054b0c615db014ebba068d3bc73a4d3ed02bad1e90`.
- visitPreOrPostfixUnaryExpression: 110905–110937, SHA256 `b8333c6cd4367aa604bc119a8ceb0bb1998ccff1484f8746490c95a48fda94e9`.

The immutable first after is recorded in
ratchets/h2-8a-export-name-syntax-first-after.v1.json: 148/160 ordinary,
20/40 map and all 56 preceding export-name commands match twice (301.74s,
exit101). All former positives stay exact. Eight remaining owned map commands
now differ only in lib.js's direct-variable alias value range; their main.js
map result is exact. Two System postincrement map commands retain missing comma
and parenthesis ranges. Subsequent fresh fields remain unqualified after the
first mismatch. Ten ordinary System library outputs, two TS2484 diagnostics,
two System library maps and eight H2.9 refusals retain their prior dispositions.

The A6-10-3 completion gives direct-variable alias publication the declaration
name clone's NoSourceMap/NoComments, then the substituted exports access's range
from that declaration name, as the already pinned getDeclarationName and
substituteExpressionIdentifier do. This is the direct storage consumer; ordinary
reference substitution and the direct primary publication remain unchanged.
The A6-10-4 completion retains the source range on the discarded postfix comma
expression and its required argument parentheses. Its current-value operand is
a clone of the original identifier. The two exact additional owners below qualify
this consumer with the immutable before/first-after map failures; the native
value-required temporary scheduling is not changed or newly qualified here.
No factory/printer edit or output normalization is introduced.
- transformSystemModule.visitPrefixOrPostfixUnaryExpression: 113142–113171, SHA256 `7cb86637e144d123b18ed01fd4d3df03058a522141ed8789259204c532b18694`.
- parenthesizeExpressionForDisallowedComma: 20483–20488, SHA256 `0a7087ac86e0e05adcb2a09a875ee73ad9e77636dc29e663dd7d03ea3e38e786`.

The immutable second after executes all 288 fresh/adjacent commands: ordinary
148/160, maps22/40, previous export names56/56, module identifiers8/8 and System
dynamic imports24/24 exact twice (271.87s, exit101). The ten owned
map-result residues are fixed. Eight external single-quote commands now reach
and fail the later main.js callback-byte field: native retains a single-quoted
live export name, while TS clones that name before createStringLiteralFromNode
and emits double quotes. Their sourceMaps results match, but complete commands
are still failures. The earlier first-field rule correctly left these later
fields unqualified. This outcome is in
ratchets/h2-8a-export-name-syntax-second-after.v1.json.

A6-10-2 completion follows the already pinned visitTopLevelExportDeclaration:
clone a literal specifier export name before live export materialization;
identifier export names use the equivalent getExportName clone/range and
NoSourceMap/NoComments/ExportName flags. The external member property remains
the existing source node. Import/declaration live bindings keep their distinct
upstream argument roles. No expected fixture or owned input changes.

The final bounded after has 178 of the 200 new complete commands exact twice:
148/160 ordinary and 30/40 source-map commands. Every owned name and map command
matches; all 52 original positives remain exact. The 12 ordinary failures are
still ten System library publication-order cases and two TS2484 diagnostic-name
cases. The ten map failures are still two System library publication-order cases
and eight H2.9 parse-recovery refusals. These unconditional comparisons keep all
failures visible; later fields of failed fresh commands remain unqualified.
All 14 retained assertion mismatches have exactly the same actual/expected fields
as before, in both repetitions; all eight H2.9 refusal boundaries are unchanged.

All 56 A6-9 name controls, eight module export identifier controls and 24 System
dynamic import controls match twice at the final production source. These six
compiler tests take 388.26s, exit101 only for the 22 named retained
failures. All 483 emitter unit tests and 451 emitter contracts pass. The original
ten class/alias commands have eight exact twice; both ClassInstance2 declaration
residues are unchanged in all four complete failure tuples. All ten JavaScript
outputs and all non-write fields match; the original replay takes
44.10s, exit101 for the two known declaration failures.

Final sources, complete logs and original failure tuples are archived outside
the checkout at target/h2-8a-export-name-syntax-after-location.txt; hashes and
counts are in ratchets/h2-8a-export-name-syntax-after.v1.json. Both intermediate
after results remain immutable, including the later callback-byte field exposed
by the second attempt. No expected observation or membership was edited. The
whole 769 replay, other A owners, B–E and hosted acceptance remain pending; no
whole-profile/global-count/full-CI qualification is claimed here.
