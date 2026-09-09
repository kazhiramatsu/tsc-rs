# H2.8a A6-31: export-star declaration producer

The export-star declaration producer is qualified on the H2.8 train. The
[frozen after result](../../../../ratchets/h2-8a-export-star-declaration-producer-after.v1.json) is 60/60 complete commands exact twice:
all 24 fresh and one original failure are repaired; all 32 fresh plus three
original positives are preserved. These are 120 primary executions, with no
supplemental captures. Both compiler jobs and all 28 existing checker statement,
chain and specifier units pass. Unit sources and IDs remain unchanged.

The one production change is in StatementSerializer::serialize_symbol_worker:
resolve each module and construct fresh export-star declarations through the
existing specifier and factory owners. Missing modules are omitted, later exports
remain ordered, bundled specifiers use module names, and declarations retain no
parsed comments/attributes or map positions. Typed source and named/namespace
export controls remain exact. Expected diagnostics, callback metadata, write order,
map results and exits all match; the grammar/deprecation diagnostics in the
reference cases remain intentional.

H2.8a and H2.8b–e remain open. The preceding full A6-29 checkpoint remains
733/769 exact twice with 36 failures; this focused repair does not infer a new
global count. Hosted acceptance is required before landing. The original ready
scope and frozen before evidence follow.


Kind: runtime. The complete repeated before and source dispositions are frozen;
production may start only after the machine readiness check succeeds.
Runtime base ed6ad88f6b9ed6203e5065b439f332ca7fe5d210 (qualified A6-30),
trusted train base10748f6ee19ec083ce5748224930c5dcfbbd86df. Sole prospective
production path crates/checker/src/node_builder/statements.rs. Other A owners,
H2.8b-e, profile/canary migrations and hosted acceptance remain open.

The completed current-original4 before is three exact /one failed twice, with two
complete saved failure tuples. exportNamespace_js reports the same8006 grammar
error and exit2 on native/TS; ONLYout/b.d.ts callback/materialized bytes differ:
parsed single quotes versus TSsynthetic double quotes. Its source owner is the
EXPORT_STAR arm of StatementSerializer::serialize_symbol_worker. Original adjacent
JSreexport alias,CJSreexport alias,andTSreexport-JS cases remain exact twice.
Original before frozen ratchets/h2-8a-export-star-declaration-producer-original-before.v1.json;
source dispositions in target/...-original-before-source-dispositions.json.

TS56mint+independentcheck eachobservefreshProgramstwice (224Programs total),
identical fixture/log receipts frozen. Diagnostics perfixture8006:5,2307:8,2823:2,
5101:3,5107:3;38exit0/18exit2;52commands8writes and4bundles4writes. Keep these
expected diagnostics/deprecations, including JSexporttype grammar, unmodified.
Fresh13shapes×JS/TS×CJS/ESNext +4AMD bundle type/value controls cover single/double/
escaped quotes, comments, repeated/mixed exports, missing and missing-then-valid
modules, directory index paths, named/namespace adjacent exports and attributes.
TS JSdeclaration stars are fresh syntax with empty main declaration-map mappings;
TS typed-source stars preserve sourcequotes/mappings. Bundle JSspecifiers use module
names (dep rather than ./dep). These are upstream observations, not native passes.

Current source: bind_export_declaration stores original ExportDeclaration NodeId
under EXPORT_STAR; namespace exports are separate ALIAS nodes. Native loops symbol
declarations, walks declaration_ancestor and add_cloned_parse_statement, retaining
parse literal/comments/maps and the shared parse-statement dedup. TS54133 resolves
each module in declaration order, skips None, gets its context module specifier,
adds17+UTF16length, makes fresh StringLiteral and ExportDeclaration with no modifiers,
clause or attributes, then addResult(None). The change replaces this whole producer,
not quote printing, string replacement, parser flags or a path-specific exception.

A6-31-1: freeze TS56 and native56 first/repeat plus current original4. Every failure
gets a source disposition. Preserve first actualexit/log/inputs before any amendment.
Audit11wholefunctions plus the whole resolver property-arrow. Read current architecture
and declaration/checker representation; no historical implementation assumption.

A6-31-2: only private serialize_symbol_worker EXPORT_STAR arm changes. For each actual
ExportDeclaration, take module_specifier and is_type_only from the immutable data.
An absent module reference follows TSisStringLiteralLike(undefined)==false and skips
this declaration. Reuse checker.resolve_external_module_name(declaration,module,false),
propagate CheckAbort through checker_abort_error, and skip None while continuing later
declarations. Reuse specifier_for_module_symbol(checker,context,resolved,None), add
17+specifier.encode_utf16().count(), create_string_literal and typed create_node with
ExportDeclarationData{modifiers:None,is_type_only,export_clause:None,
module_specifier:Some(literal.node()),attributes:None}. add_result(None) owns modifiers,
length contribution and result order. No parser clone/original/comment/map assignment
for this fresh node. Keep all other add_cloned_parse_statement callers/dedup unchanged.
Keep named/namespace/alias/type/class branches and all source TSsyntax emission unchanged.
Whole serializeSymbolWorker ledger188-lineidentity remains, with exact helper call graph.

State: NodeId/SymbolId belongs to current ProgramBinder; symbol_data clone snapshots
existing declarations/order. NodeBuilderContext/StatementSerializer borrow checker,
tracker and TransformArena for one serialization. getSpecifierForModuleSymbol owns
module-symbol links cache keyed by context path/mode; use existing owner, no new key,
cache or tracker/compileroptions carrier. Fresh Program/links invalidation unchanged.
Factory returns typed TransformNode or Factory error; typed create_factory_node's
ExportDeclaration arm uses the dedicated factory (generic fallback TypeScript flags
are unused). Factory owns child flags, clearing possible top-level await, synthetic
position and cooked UTF16 string provenance. Existing serialize_symbol wrapper restores
context and error-fallback stack after its captured Result, and table wrapper restores
names/remapped refs/approximate length/statement tracking on success or error. No new
lifecycle state; copied declaration flags/pointers are released before mutable resolution.

E-ENTRY/E-PROTOCOL/E-RESOLVER-BASE/E-ARENA/E-METADATA-BASE/E-POSITIONS/E-STRINGS boundaries
remain unchanged. The new source producer must qualify its exact fresh provenance;
these architecture rows are not claims of already-correct export-star output. Existing
E-DECL-SESSION/FORCE future APIs are not activated; this is ordinary command emission.
No new host/sink/cancellation capability. No JavaScript pass registration, receiver
capture/generated names/helpers/comment cursor or printer formatting edit. The
DeclarationTransformer JSgateway borrows its resolver and restores diagnostic context
in its existing Result path, while typed-source declaration syntax uses its own route.

A6-31-3: require every fresh owned repair and every previous positive; run all56 and
original4 twice, recording any explicitly outside first vectors unchanged. The measured before is 32 fresh exact and 24 fresh owned failures, identical
across two jobs; all 24 first differ at source-map result. All 24 supplemental
declaration maps differ only in mappings, and 22 declaration texts differ; no
JavaScript output difference was found in those separately counted captures.
There are zero outside, typed-runtime, unresolved or undispositioned rows.
Require 60/60 exact twice after: all 24 fresh plus one original repair, and all
32 fresh plus three original positives. This is a target until measured.
Run the unchanged checker statement, chain and specifier units (14+6+8=28),
including the existing context/factory-error restoration control.
No unit assertion changes, expected regeneration/normalization, source-name matching,
blanket fallback or opaque unknown-as-success. New dependencies require pre-edit amendment.
The schedule-authorized lightweight focused/adjacent workflow applies; historicalwalk
and full developer CI are omitted, never claimed passing. This does not closeH2.8.


The complete before is ratchets/h2-8a-export-star-declaration-producer-before.v1.json:
176 fresh primary executions, 88 supplemental artifact captures, eight original
primary executions, 272 native executions total. Fresh successes run twice per
job and failures once per job; original commands all run twice. No supplemental
capture is called a full failed tuple, and fields after a first failed comparison
are not qualified by its primary command. The original failure has two complete
actual/expected tuples. Every original and fresh membership is fixed before edits.

| Value/state | Concrete Rust owner and producer | Consumer/lifetime/invalidation | Gap and completion |
| --- | --- | --- | --- |
| export-star declarations | ProgramBinder symbol declarations: Vec<NodeId>; bind_export_declaration | serialize_symbol_worker, immutable parsed source lifetime | partial-or-stale parse cloning; A6-31-2 |
| resolved module | CheckerState::resolve_external_module_name returns CheckResult<Option<SymbolId>> | existing specifier owner, same Program; None skips and CheckAbort propagates | shared prerequisite; missing-then-valid and original controls |
| module specifier/cache | get_specifier_for_module_symbol; context enclosing file, mode and module SymbolId | per-checker symbol links; fresh Program invalidates | shared prerequisite; directory, bundle and repeated controls |
| serialization context | NodeBuilderContext and StatementSerializer borrowed lifetimes | current mutable checker/tracker/arena; outer wrappers restore context even on Err | shared prerequisite; existing restoration unit and 32 positives |
| fresh syntax/provenance | TransformArena/TransformSourceId/TransformNode through typed NodeFactory | declaration printing; per-request arena and synthetic ranges | replace cloned parse provenance; 24 map witnesses |
| string value | JavaScriptString through NodeFactory::create_string_literal; input String | cooked UTF-16 and default quote choice, per-node lifetime | fresh literal; escaped/single/double and bundle witnesses |
| order and length | existing symbol declaration order and add_approximate_length | 17+specifier UTF-16 units then add_result; serialization context restore | repeated/mixed/missing sibling witnesses |
| diagnostics and publication | existing checker, resolver, declaration transform, callback sink | complete command diagnostics, maps, write bytes/metadata/order, result and exit | all 60 complete after observations required |

Public API and visibility do not change. Only private serialize_symbol_worker
changes its export-star producer. The SourceFile resolver property validates source
identity, chooses locals or exports, resolves module symbols, and enters the same
borrowed node-builder context. Module specifier options remain with their current
checker owner; the separately open Monorepo paths carrier is not changed here.
The 56 new witnesses do not rely on that missing paths carrier. A new dependency
must be source-dispositioned in a pre-edit amendment, never silently patched.

Exact reproduction (two build jobs, one test thread; every heavy command launched
with taskpolicy -b nice -n 15):

```sh
node scripts/observe-export-star-declaration-producer.mjs --check
python3 scripts/check-export-star-declaration-producer-readiness.py
cargo test -p tsc-rs-compiler --test contracts -- export_star_declaration_producer --nocapture --test-threads=1
cargo test -p tsc-rs-compiler --test h2_8a_original_corpus -- original_export_star_declaration_producer_commands --nocapture --test-threads=1
cargo test -p tsc-rs-checker --lib -- node_builder::statements::tests node_builder::chains::tests node_builder::specifier::tests --test-threads=1
```

The two compiler commands are separate so one failure cannot skip the other.
Freeze all actual exits, inputs and binary receipts before any following edit.
The integrator is the only writer. Readiness requires 11 whole functions, the
whole resolver property, eight Rust-map rows, eight architecture premises, three
steps, 56 fresh plus four original witnesses, and zero unresolved/undispositioned.
No unit IDs/assertions or expected values change. Tests remain unconditional.

Required architecture references below retain their exact current source-row
hashes in the manifest; links here are resolved from this slice directory.

| Concern | Boundary | Rust owner and visibility | Validation | Predecessor evidence |
| --- | --- | --- | --- | --- |
| `E-ENTRY` | No-emit and emit are distinct typed entries; no-emit constructs no emitter-only component. | `tsc_compiler::ProgramSession::{run,emit}`; `tsc_emitter::emit_files_with_activity` (public) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. | frozen H2.5g profile (`0653e10d`); H0/H1 no-emit canaries |
| `E-PROTOCOL` | Read host, semantic resolver, artifact, sink, and outcome have separate ownership; planning cannot observe syntax, checked syntax exists only within the live checker/resolver scope, and sink errors become diagnostics at the write boundary. | `tsc_emitter::{EmitHost,EmitResolver,EmitArtifact,OutputSink,EmitOutcome}` (public); private `tsc_compiler::{PreparedEmitHost,CheckedEmitHost}`; `tsc_checker::emit::CheckerSession` implementation in `crates/checker/src/emit.rs` | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile; every output slice preserves this boundary |
| `E-RESOLVER-BASE` | Semantic facts cross a borrowing consumer-owned resolver; unavailable methods return typed errors. | `tsc_emitter::{EmitResolver,EmitResolverMethod,EmitResolverError}` (public); `tsc_checker::emit::CheckerSession` impl in `crates/checker/src/emit.rs` | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and resolver controls |
| `E-ARENA` | Parsed trees remain immutable; the detached arena appends synthetic nodes and tracks the mounted parsed interval. | `tsc_emitter::{TransformArena,TransformSource,TransformSourceId,TransformNode,TransformNodeArray,NodeFactory}` (public) Existing `NodeFactory::modifier_flags` is now `pub(crate)`; downlevel auto-accessor setters use fresh factory modifiers while getters retain source tokens. | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; every later transform packet maps provenance explicitly |
| `E-METADATA-BASE` | Transform flags and `emitNode`-equivalent facts are sparse session side tables; there is no standalone Rust `EmitNode` syntax object. Original/map/comment/value identities are separate. CommentRange carries independent source start/end states through CommentSourceRange; paired source slices and source-map ranges keep their existing domains. | Public `tsc_emitter::TransformArena::{transform_flags,set_transform_flags,array_transform_flags,set_array_transform_flags,metadata,metadata_mut,clear_session_metadata,get_original_node,set_original_node}` over private storage in `crates/emitter/src/factory.rs`; public `tsc_emitter::{EmitMetadata,SourceMapRange,CommentRange,CommentSourceRange,JavaScriptString}` defined in `crates/emitter/src/metadata.rs` (`EmitMetadata` storage fields remain `pub(crate)`, with its cross-crate operations exposed by public methods) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. A6-29 candidate uses existing `EmitMetadata::set_flags` to replace inherited flags on the ES5 accessor receiver clone, retaining its original/text/map ownership. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; H2.5h/H2.6 extend rather than infer from text |
| `E-CONTEXT` | One per-unit context owns lexical/block environments, hoists, helpers, diagnostics, hooks, initialization, and reverse disposal. | `tsc_emitter::{TransformationContext,Transformer,TransformationResult,transform_nodes}` (public) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. | H2.5g profile and transform lifecycle controls |
| `E-POSITIONS` | Source bytes, source/generated UTF-16, synthetic ranges, and source switches remain typed domains. | Public `tsc_emitter` position/writer/hook types; definitions in private `position`, `writer`, `metadata`, and `printer` modules | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H1/H2.5f protect predecessor behavior. | H2.5g profile and H1 Unicode/newline controls |
| `E-STRINGS` | JavaScript string values preserve UTF-16 code units; lexical spelling and synthesized cooked values have distinct provenance. | Public `tsc_emitter::JavaScriptString` and `EmitMetadata` accessors/setters; `pub(crate)` metadata fields for cooked value, quote choice, and text-source identity; private printer quote routines | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and literal/JSX/module controls |

Whole TypeScript functions, AST-verified from vendor/typescript-6.0.3/lib/_tsc.js:

| Owner/span | SHA256 | Current Rust symbol | Step/invariant |
| --- | --- | --- | --- |
| createBaseStringLiteral 21523–21528 | `357b18f0aef43dcbeef52147b7edacf70b26ce42101d0fba479a4a69694fc535` | crates/emitter/src/factory.rs::NodeFactory::create_string_literal | A6-31-1: fresh cooked value with no parsed text source |
| createStringLiteral 21529–21534 | `2bf21e80bf4e61e4e1af7273cc968a2d4423ba01535d7cedc31a7ed35ebc1c2e` | crates/checker/src/node_builder/statements.rs::create_string_literal | A6-31-1: typed factory call, default double-quote provenance |
| createExportDeclaration 23624–23635 | `9a0b5dd2decaa4205a68486280acf4778c851fb338a6c64fd464c3fba40d3174` | crates/checker/src/node_builder/type_nodes.rs::create_factory_node ExportDeclaration arm | A6-31-1: dedicated NodeFactory call owns child flags and synthetic positions |
| bindExportDeclaration 44574–44583 | `d6d358b0a0eb6ef482febcef992fb422846a31fb14ac856ff0239d0594b01f48` | crates/binder/src/bind.rs::bind_export_declaration | A6-31-1: EXPORT_STAR retains original ExportDeclaration IDs; namespace exports are ALIAS |
| resolveExternalModuleName 49465–49469 | `9d35733bd4ecb68f5802ad505327c208fc59e122e00cbc2633f50e1f3c3d4f9a` | crates/checker/src/modules.rs::resolve_external_module_name | A6-31-1: resolve original declaration module with errors enabled; preserve CheckAbort |
| resolveExternalModuleNameWorker 49470–49472 | `06af42ad3f426366f3ce8859410e119e7d429da0ed86b7094aac49115437af9a` | crates/checker/src/modules.rs::resolve_external_module_name_worker | A6-31-1: string-literal-like admission; absent module skips |
| getSpecifierForModuleSymbol 53060–53109 | `cc081ccc9162d99c71cfb5013a0786210de8d66472567a9ee1d6eab90f686463` | crates/checker/src/node_builder/specifier.rs::get_specifier_for_module_symbol | A6-31-1: reuse current module-symbol/context/mode cache and bundle specifier owner |
| serializeSymbol 53976–53991 | `89a62d997aa6147fdb69cb375b6f5a5e4a24830189c28093f5d59dfefacc0647` | crates/checker/src/node_builder/statements.rs::serialize_symbol | A6-31-1: visited symbol identity, scope membership, context/tracker cleanup on captured Result |
| serializeSymbolWorker 53992–54179 | `dc9bf6e639d95e72cefd3e99de392150843321b4b1f785340d52751bbe4bbc38` | crates/checker/src/node_builder/statements.rs::serialize_symbol_worker EXPORT_STAR branch | A6-31-2: resolve each declaration in order, skip absent module, create fresh declaration |
| addResult 54190–54210 | `1d5bbaa5bc6714a9dcb56eb0347501e0ebd1c79db08073b2b679e32987b7bee1` | crates/checker/src/node_builder/statements.rs::add_result | A6-31-1: existing modifiers/ambient/length accounting and ordered append |
| transformDeclarationsForJS 114431–114440 | `fe83d798e4e7ba53668936902ed8d8c0c15191b6138123cc72989e365c67168d` | crates/emitter/src/declarations/root.rs::transform_declarations_for_js | A6-31-1: existing JS-only declaration route and resolver/tracker diagnostic restoration |

The whole getDeclarationStatementsForSourceFile property-arrow spans 88612–88621, SHA256 `517de08538d0b91488cd2e54201e7dc44b404b08fe126ba36a1b63ce84ec70dc`. Its current Rust owner is crates/checker/src/declaration_emit.rs::emit_get_declaration_statements_for_source_file; it is a reused A6-31-1 gateway.
