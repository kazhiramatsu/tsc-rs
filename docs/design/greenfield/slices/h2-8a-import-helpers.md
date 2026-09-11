# H2.8a CommonJS-format external helper imports

Status: focused CJS helper profile qualified; the executable readiness check
continues to verify its source-owned premises. Base `14f5332125f7a61b8b8a9f06ac108d8ef62e7a04`. H2.8a–e remain open. This packet repairs the CommonJS,
AMD and UMD producer and its shared recorded-helper consumer; it does not
close ESM alias substitution, System setters, bundled import admission or
configuration diagnostic ordering.

## Observations and required outcome

The [complete before](../../../../ratchets/h2-8a-import-helpers-before.v1.json)
contains 111 ordinary configuration/Program commands: 26 exact twice and 85
failed twice across two independent jobs. All 274 supplemental complete
commands agree, including fields after the primary comparator's first failure.
TypeScript was observed twice per case (222 Programs). The extra 12-case
identity investigation executed 24 TS Programs and is not added to the 111.

Eighty new failures belong to the CJS-format helper producer. The other five
remain visible: ESM helper-name aliasing and bundled-import option refusals,
one System helper/setter omission, and AMD/UMD noEmitOnError diagnostic order.
The latter two correctly emit nothing. No expected diagnostics are reordered.
The 16 original allowJs helper-collision commands reuse A34's 32 saved complete
before tuples after production, source, library and driver equivalence checks;
this adds zero new original-before executions. Fresh and original denominators
stay separate.

After this packet, all 80 owned new cases and 26 existing positives must match
the full command twice, independently repeated, and all 16 original commands
must match twice. The five remaining controls still compare against unchanged
TS expectations; retain and explain every changed complete tuple. System may
expose its missing producer differently after helper suppression follows real
metadata. Such a delta is not a repair and cannot be counted as exact.

## Source-owned steps

A6-35-1 establishes source-owned helper facts in existing `EmitMetadata`.
`getExternalHelpersModuleName` and `hasRecordedExternalHelpers` follow the
original SourceFile. `TransformArena::replace_root` changes `syntax.root`, so
read the full original chain and validate the SourceFile kind. Store the
namespace as a `TransformNode` and the ESM named-import fact as a boolean;
never store a provisional spelling as its identity. These two fields are
absent from TS `mergeEmitNode` and must not be copied by the native merge.
Existing session metadata clearing disposes them. Correct the E-HELPERS-H
architecture claim to its actual existing ESM-only, no-alias-collision scope.

A6-35-2 completes the CJS collection and import producer. Effective external
status is external-module status OR the CommonJS binder indicator for module
CommonJS/Node16..NodeNext. The existing typed resolver query supplies the
indicator, and errors propagate. Non-JavaScript sources cannot acquire that
indicator (getAssignmentDeclarationKind's isInJSFile guard and bindWorker's
call-expression guard); preserve that source fact when avoiding an unnecessary
query. Source identity uses `require_parse_tree_resolver_node`, not a raw id.
Raw NodeNext=199 is not the emitted CJS format: use the existing host format
projection for the `< System` demand decision.

Collect unscoped pre-module helper demand plus export-star/value,
import-star and import-default demand with the full TS predicates. A module
transform can be the first helper producer, as all original 16 demonstrate.
Reuse an existing source namespace or create one `createUniqueName("tslib")`.
Construct the import-equals/external-module-reference with existing factory
methods; mark the import `NeverApplyImportHelper`. Add it first to the external
dependency list and visit it after the marker/export prelude, before AMD
interop initializers and source statements. The synthetic import has no
checker-owned declaration and cannot trigger resolver queries.

A6-35-3 preserves generated identities through declarations, dependencies and
substitution. The import-equals variable declaration clones its actual name.
AMD parameters carry an actual identifier node and share the helper namespace
identity with all references; unaliased UMD dependencies still require in the
body. Ordinary generated module aliases use the existing `TargetBinding`
metadata in CJS and UMD as they already do in AMD. Lookup spellings remain
provisional keys; the composed tree owns final names. The measured collision
witness requires helper `tslib_1`, user import `tslib_2`, or `tslib_2/tslib_3`
when parsed `tslib_1` is occupied. Do not hardcode ordinals or add an allocator.

Restore SourceFile before/after notification extent on the module transformer.
The new callback arm is Expression hint + Identifier kind + HELPER_NAME only;
previous callbacks already run first. Existing ordinary import/export
substitution stays eager and the call/tag helper receiver guard remains.
Create a fresh property access with no added original/map/text range. Its
property child uses IdentifierName hint, preventing recursive qualification.
The original namespace node can be absent from the emitted tree because its
declaration was cloned: obtain its spelling from the existing finalized
generated-binding record, retaining the same identity, before emitting the
new reference. Do not print an unfinalized `tslib` name.

A6-35-4 makes `Printer::sorted_source_emit_helpers` use actual recorded helper
metadata. The existing ESM named-import producer records its source flag too.
Retain helper dependency ordering, priorities and callback behavior. The
separate scoped-helper/noEmitHelpers branch is not expanded in this packet;
the existing production async-super captures are AST statements, and this
packet's noEmitHelpers commands exercise unscoped helpers. The broader scoped
helper policy remains explicitly unqualified, not inherited from this change.

A6-35-5 validates full observations, repeats them, runs the original selector
and emitter unit/contract suites, then records actual exits and cause closure.
Existing marker-query unit expectations must distinguish the newly implemented
enclosing effective-module query from the marker query itself; retain both
false answers and typed unavailable/aborted error cleanup. No other unit
expectation changes are authorized without a source-owned amended packet.

Allowed production paths: `crates/emitter/src/builtins.rs`,
`crates/emitter/src/metadata.rs`, `crates/emitter/src/printer.rs`.
The only planned existing-unit edit is the module-marker query control in
`crates/emitter/tests/unit/builtins/tests.rs`; new focused controls may use
that same module. Registration, observation, packet and evidence paths are
integrator-owned. No accepted/profile membership, shared loader, expected
byte, configuration diagnostic, System runtime, noEmit or bundle guard edit.

## Representation and impact

| Producer | Carrier/updater | Consumer and lifetime |
| --- | --- | --- |
| Complete module/effective-format predicates | source flags + existing borrowing resolver/host | one module transform; typed errors preserved |
| prior helpers + import/export demand | CommonJsModuleInfo demand facts | helper import creation before source visitation |
| namespace/named-helper import | original SourceFile EmitMetadata | helper suppression and emit-time substitution; session disposal |
| synthetic import-equals | actual generated name + NeverApplyImportHelper | CJS/UMD cloned variable binding or AMD parameter |
| user module aliases | existing TargetBinding/GeneratedBindingId | final composed-tree spelling, including same-base imports |
| SourceFile notification | module transformer current-source extent | expression helper callback, cleared after emit/disposal |
| helper name | HELPER_NAME + IdentifierName child hint | property-access substitution without receiver erasure |
| complete command | unchanged ProgramSession + MemoryOutputSink | diagnostics, ordered writes, maps, result, status and exit |

EA-GAP-FLAGS remains open: constructors and existing postorder updates own
flags here; no new lattice, mask or synthetic token kind. EA-GAP-CAPTURE
remains open: lexical/function capture producers are unchanged. Per-source
helper metadata and notification state do not become a receiver-capture cache.
Output paths, artifact construction-before-callback, host/system faults and
diagnostic production remain unchanged premises. Current H2.8b–e are not waived.

The source inventory enumerates 367 branch expressions as an audit aid.
Each receives an explicit owner/step/prerequisite/outside disposition in the
manifest; enumeration and hashes are not proof of full compiler coverage.
The CJS helper creation, effective-module predicate, declaration-name carrier,
notification extent and recorded suppression are newly implemented. Other
branches remain mapped prerequisites or named open owners, not silent claims.

## Whole pinned TypeScript owners

All functions below were read in full; their helper/caller ownership and the
specific newly ported predicates are mapped above and in the machine manifest.

| Owner | `_tsc.js` span | SHA256 |
| --- | --- | --- |
| `getOriginalNode` | 11400–11410 | `e6e639e966314faf444b9b68796893745ffb06eb0adcf1180d6935332d8797a3` |
| `isCommonJSContainingModuleKind` | 13753–13755 | `3b2d3a59852fc51f6ed3b6725471f3387a820fbfb352322ce10a8319ce02dfed` |
| `isEffectiveExternalModule` | 13756–13758 | `faeb969f19783953861ac4d20410d7c78928ef45e6ec39f2f27231d2d0acfc33` |
| `getAssignmentDeclarationKind` | 15055–15058 | `86ed418c050973f93d14271122ba9f948961e1e038e6c338eb6df19543402bd2` |
| `getNamespaceDeclarationNode` | 15271–15282 | `4e696f2f442304a0c41dfd5bf7d6e8b19b5191acf8288c2d75581f3d53fc4854` |
| `createUniqueName` | 21647–21651 | `63ce34b71e831906ac09351627344eeb9bf76d71b5685d4715fb1873a61ccec5` |
| `createPropertyAccessExpression` | 22474–22489 | `3a8dc9211cb8df73437d1f7063798bbb942069a342c1e637992d943ff7e7c935` |
| `createImportEqualsDeclaration` | 23460–23473 | `cd9f16baa6064edf0ddd264a39e0a8e83e7c70d6d9dc77e0765829dc80b493b9` |
| `createExternalModuleReference` | 23675–23681 | `a4e717aee97b01162966ea47ab18b01145643373b8854219378ee413446f1ca8` |
| `cloneNode` | 24436–24466 | `d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6` |
| `mergeEmitNode` | 25218–25277 | `6d9f4af1f1fa79b494c5ef7b570972925000f7939cd16ffe520855a67583f375` |
| `getOrCreateEmitNode` | 25287–25301 | `973debb573fcc3974cba5fd8c4bf89f1c5ffacc8ba297ec47a83f1534846ab1a` |
| `disposeEmitNodes` | 25302–25310 | `0f82231ff0268dcb94304e298c18c37ff2504cd1877a006042e1c1a4b2898599` |
| `addInternalEmitFlags` | 25331–25335 | `1a4cfcfaac8d89770a3da89096d551a101e9158efae74894c740e09fca80fbe1` |
| `getUnscopedHelperName` | 25526–25528 | `4eccb820e726db854c379fb20072e2506d22a8caa82b367dcd88168334c0936e` |
| `getExternalHelpersModuleName` | 27603–27607 | `0ca5e12b63beaf46f3b5090835cdabe97e837d78e5990b7965f86934519e446f` |
| `hasRecordedExternalHelpers` | 27608–27612 | `ed1779440c89c10d2a9c3801a8d608a05bb703f53772b2354102ebf477443d76` |
| `createExternalHelpersImportDeclarationIfNeeded` | 27613–27680 | `d44b2c0d8237d7cad74d638bdaa5cd14dd4347e3a57e408de8b5fb85a6a18f77` |
| `getImportedHelpers` | 27681–27683 | `e5dfcad527bd993cbc068c7f236365e64ea6e1fa81f0f229780979f4e1d7bb19` |
| `getOrCreateExternalHelpersModuleNameIfNeeded` | 27684–27695 | `6c3961c1f7fb4a4078962c1e353d7d8d26edcdfdbd1ca04d299c27e3d8ab4e61` |
| `getLocalNameForExternalImport` | 27696–27712 | `5d7ff3b8b36f70137de51c9fb8ccc549d73d83098f572dcd451c4c9dd4a2119e` |
| `getExternalModuleNameLiteral` | 27713–27719 | `2d6cd262d872b1b38c0094ffd8a9d77b5864e3d8f912de6e8dd07e1e7f5d5852` |
| `isExternalModule` | 28910–28912 | `5effe04fdce706cc75f238b5c4efbb1f317b3f6bd665389fb71a79a119e7ceaa` |
| `bindWorker` | 44287–44527 | `4b259323fa2534e8d67ea9485669ddbff8e0a5a252719533558d6d8e99181588` |
| `setCommonJsModuleIndicator` | 44589–44600 | `294f2a8b52accab297f5d966aa37276fb9f4b9bb850ed593d2b15c49a6531118` |
| `bindCallExpression` | 44970–44978 | `b113d393e5aa91b6f92421ebab0f71f3497dfe630bab99bf0e8fbf4b8c62a1bb` |
| `containsDefaultReference` | 92739–92743 | `d7a086084d56e7a7fd53e46f22aa19598845433d9c8046732d93960cf40cb313` |
| `isNamedDefaultReference` | 92744–92746 | `e74f805fcc700641a618844b6738b011f7582b995e5b776ea9d30eeda6373549` |
| `getImportNeedsImportStarHelper` | 92759–92775 | `33ce3417da043e9997aa0f2cc5438f657d22a47fd965c11c011db825cfa62b9a` |
| `getImportNeedsImportDefaultHelper` | 92776–92778 | `8a8f2e44755061bef27db923c1a10b9ea0a2ad3f1f88afe6064aefe86f7f041c` |
| `collectExternalModuleInfo` | 92779–92919 | `2694413ce6ea08091a03db3a313b50ee3ff526b065f199270311ac583350220e` |
| `transformSourceFile` | 110130–110157 | `b1a1419c70b033ad4e884d96dfc78c8ee746580e502895fc68d55091b68e1752` |
| `transformCommonJSModule` | 110167–110204 | `a48c215e8304107fec1f4e113f74a13f720cb5901cff5287bda673a98e082749` |
| `collectAsynchronousDependencies` | 110442–110480 | `41cebfe62805c877b285fd853d3392735f8d0c08458d69182234964dc1c31116` |
| `getAMDImportExpressionForImport` | 110481–110491 | `81403d2adfe66352c70ab014f15430f55fd97a9f1011586e444cbe98ae9dcf0b` |
| `transformAsynchronousModuleBody` | 110492–110534 | `8d0713be5aaf1b99c2e7e304e5e5f341884d2c0a865aefc1336c96f9cb98d9c9` |
| `getHelperExpressionForImport` | 111177–111188 | `e754d1298bee277a2e52c72a32a17dd3dc7bd9b1a572fe56600be79e1d065cde` |
| `visitTopLevelImportDeclaration` | 111189–111284 | `d2de4a9f6f71d14b85841e2f6f97a06817e3fee3d04272b233ab11be434dd34a` |
| `createRequireCall` | 111285–111297 | `8cd1834665238e4961170a0e1990870e41b9fc54045ec4eb0a7d97c66956c439` |
| `visitTopLevelImportEqualsDeclaration` | 111298–111365 | `8577823442eb4668d4144ba8be82838ed14b0c7ebd76f51e2b82380cd29d4406` |
| `onEmitNode` | 111860–111870 | `4ccea9f68d01aa463c69190bcfbfa7f5ac0c80d62f40d54e87dbeffcdb4102a9` |
| `onSubstituteNode` | 111871–111882 | `4275c47c81e1ec247934ac9d79029304a6b07f259ccde7a9036f1bdc3d160a3c` |
| `substituteShorthandPropertyAssignment` | 111883–111894 | `327c0cba28a7c4c775395f63ee22c58baeea3805a208f666da5344f3f906dc37` |
| `substituteExpression` | 111895–111907 | `110766e2e010527fe3aa8d766d8c02db2a437cc1f4b92977fb53e3e8cf77d338` |
| `substituteCallExpression` | 111908–111926 | `b7d37e8b58aa358394f808214077759c803c92d04fd68440e2963acb00318140` |
| `substituteTaggedTemplateExpression` | 111927–111945 | `7ae75d422344e617c1fff45272b14d44abe2582c0f4922f6d9e9078b090f038b` |
| `substituteExpressionIdentifier` | 111946–111989 | `972830b79228dc51aaec4b3b13ebd2a12795701304627fef3bdc5ba8b7ab3a96` |
| `emitHelpers` | 117719–117755 | `27b8a8efc6d94c599181ff19bc36f9702eca61b9e9a24d729ff3f5c989bcc53c` |
| `getEmitModuleFormatOfFileWorker` | 125493–125495 | `ffe7b58092e4af38c9484bef12201ef7524d2e3d26ba829ea59087f1a2c0d2a1` |
| `getImpliedNodeFormatForEmitWorker` | 125496–125509 | `765b9d66f854668f6f9326de2a8e3659af532be224d3a2546fdec84855bbe69c` |

## Architecture premises

Existing invariants retain qualification. The source-helper concern is now
qualified for the focused profile recorded below; its named outside owners remain open.

| Architecture ID | Invariant | Rust symbols | Lifecycle | Evidence |
| --- | --- | --- | --- | --- |
| `E-ARENA` | Parsed trees remain immutable; the detached arena appends synthetic nodes and tracks the mounted parsed interval. | `tsc_emitter::{TransformArena,TransformSource,TransformSourceId,TransformNode,TransformNodeArray,NodeFactory}` (public) Existing `NodeFactory::modifier_flags` is now `pub(crate)`; downlevel auto-accessor setters use fresh factory modifiers while getters retain source tokens. | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; every later transform packet maps provenance explicitly |
| `E-METADATA-BASE` | Transform flags and `emitNode`-equivalent facts are sparse session side tables; there is no standalone Rust `EmitNode` syntax object. Original/map/comment/value identities are separate. CommentRange carries independent source start/end states through CommentSourceRange; paired source slices and source-map ranges keep their existing domains. | Public `tsc_emitter::TransformArena::{transform_flags,set_transform_flags,array_transform_flags,set_array_transform_flags,metadata,metadata_mut,clear_session_metadata,get_original_node,set_original_node}` over private storage in `crates/emitter/src/factory.rs`; public `tsc_emitter::{EmitMetadata,SourceMapRange,CommentRange,CommentSourceRange,JavaScriptString}` defined in `crates/emitter/src/metadata.rs` (`EmitMetadata` storage fields remain `pub(crate)`, with its cross-crate operations exposed by public methods) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. A6-29 candidate uses existing `EmitMetadata::set_flags` to replace inherited flags on the ES5 accessor receiver clone, retaining its original/text/map ownership. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; H2.5h/H2.6 extend rather than infer from text |
| `E-CONTEXT` | One per-unit context owns lexical/block environments, hoists, helpers, diagnostics, hooks, initialization, and reverse disposal. | `tsc_emitter::{TransformationContext,Transformer,TransformationResult,transform_nodes}` (public) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. | H2.5g profile and transform lifecycle controls |
| `E-NAMES-BASE` | Generated identity, printable name, target provenance, and allocation scope are separate; final names use the composed tree. | `crate::transform::GeneratedBindingId` (`pub(crate)`); `crate::builtins::generated_bindings::GeneratedBindingScopes` and `target_bindings::{TargetBinding,finalize_generated_binding_names}` (`pub(super)`) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and generated-binding controls |
| `E-PRINTER-BASE` | Printer has no direct checker dependency. It consumes transformed syntax/metadata, drives retained substitution and before/after notification hooks, and applies immutable structural planning. | `tsc_emitter::Printer` (public); `tsc_emitter::TransformationResult::{substitute_node,before_emit_node,after_emit_node}`; `crate::printer::EmissionPlan` and `EmitContext` (private); `crate::factory::NodeFactory::apply_parenthesizer_rules` (private) | `active-qualified`; qualified at validation ref `6acd5d43` (2026-08-21, CS-6 fixture/audit gate); previous ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. The H2.5h-a comment-scope packets reshape the private context carrier into the threaded `EmitContext` (CS-2 landed the root and core pipeline byte-identically); the row's hook-composition, no-checker-dependency, and immutable-planning invariants are preserved; requalified at `6acd5d43`. | H2.5g profile; later packets preserve hook composition and enumerate new expression contexts |
| `E-RESOLVER-IDENTITY-G` | A resolver query accepts only an identity anchored in the mounted immutable parse interval. Projection follows typed original-node provenance, rejects synthesized/range-incompatible identities, and carries the owning Program source so an appended raw `NodeId` cannot alias a parsed node in another source. | `tsc_emitter::TransformSource::{contains_parsed_node,program_source}` and `TransformArena::{is_parsed_node,parse_tree_resolver_node,require_parse_tree_resolver_node}` (public); `tsc_emitter::EmitResolverNode` | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate-only audit 2026-08-14 | Current-code audit and resolver-identity contracts; H2.5g inventory/profile for observable parity; every later resolver consumer revalidates the projection |
| `E-PROTOCOL` | Read host, semantic resolver, artifact, sink, and outcome have separate ownership; planning cannot observe syntax, checked syntax exists only within the live checker/resolver scope, and sink errors become diagnostics at the write boundary. | `tsc_emitter::{EmitHost,EmitResolver,EmitArtifact,OutputSink,EmitOutcome}` (public); private `tsc_compiler::{PreparedEmitHost,CheckedEmitHost}`; `tsc_checker::emit::CheckerSession` implementation in `crates/checker/src/emit.rs` | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile; every output slice preserves this boundary |
| `E-OUTPUT-SCRIPT` | JavaScript artifacts are constructed before the first sink callback; callback order and `emittedFiles` stay independent. | `tsc_emitter::{emit_files_with_activity,EmitArtifact,MemoryOutputSink,FsOutputSink,EmitOutcome}` (public) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and H1 sink-failure controls |
| `E-HELPERS-H` | The five owner helper texts (`extends`, `values`, `spreadArray`, `generator`, `makeTemplateObject`) are landed byte-pinned to the vendored declarations by the proven `typescript:read` dedent recipe, with upstream metadata (extends and makeTemplateObject priority 0, generator priority 6) and the d2 ledger hashes; the B-5 §12.8.6 importHelpers lane currently inserts ESM named imports without alias collisions. CJS/AMD/UMD namespace imports now have the separately qualified [A6-35](h2-8a-import-helpers.md) source-state producer. System helper setters, ESM helper alias collisions and bundled import admission remain open. The historical B-5 wording did not qualify these branches. | `crate::builtins::helpers::{extends,values,spread_array,generator,make_template_object}`; byte-parity suite `crates/emitter/tests/unit/helpers/tests.rs`; `crate::builtins::insert_external_helpers_import_declaration` | `active`; B-1 landed the four 2026-08-21 ([B-1 packet](h2-5h-b-b-1.md)) at `ad62e4a5`; B-5 landed the fifth text and the tslib import lane 2026-08-23 ([B-5 packet](h2-5h-b-b-5.md)) | vendored declaration slices; ledger d2; byte-parity contracts; the witness fixture gate |
| `E-HELPERS-IMPORT-STATE` | External helper namespace/named-import facts live on the original SourceFile and are not copied by emit metadata merging. CJS/AMD/UMD imports, generated declarations/dependencies and expression helper references share a generated identity finalized from the composed tree; source notifications bound substitution, and the printer suppresses unscoped helpers only for recorded imports. | `EmitMetadata::{external_helpers_module_name,external_helpers}`; `CommonJsModuleTransformer::collect_external_helpers_import`; `CommonJsModuleInfo`; `CommonJsVisitor`; `get_external_helpers_module_name`; `has_recorded_external_helpers`; existing `NodeFactory`, `TargetBinding` and `Printer::sorted_source_emit_helpers`. Fields and getters remain internal. | `active-qualified` for the A6-35 focused train profile (2026-09-10); broader System/ESM alias/bundle/scoped-helper branches remain open. | [A6-35](h2-8a-import-helpers.md):106/111 complete commands exact twice in each of two independent jobs (80 repairs,26 prior positives);16 original commands exact twice;494 units/451 contracts with unchanged IDs. Five controls remain failed, including the explicitly recorded System map-boundary change. |

## Commands and checks

```sh
python3 scripts/check-import-helpers-readiness.py
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test contracts -- import_helpers_matches_complete_typescript_observations --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test h2_8a_original_corpus -- original_import_helper_collisions_match_complete_commands --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-emitter --lib -- --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-emitter --test contracts -- --test-threads=1
```

Keep one heavy job at a time and consume actual process exits before changing
pinned inputs. Capture complete supplemental tuples for both focused runs;
record their extra executions separately. The whole 111-case test remains red
while the five named outside controls fail; qualification requires 106 exact,
80 repairs, 26 preserved positives and no new failure, plus all original 16.
The scoped public helper policy, System, ESM aliases and bundling stay open.
Run existing hosted acceptance before runtime landing. Historical full CI and
certificate walks remain omitted under the current authorized workflow.

Native ownership anchors: `CommonJsModuleTransformer`, `CommonJsModuleInfo`,
`CommonJsVisitor`, `EmitMetadata`, `NodeFactory`,
`EmitHost::get_emit_module_format_of_file`, `EmitResolver::is_common_js_module`,
`Printer::sorted_source_emit_helpers`.

## A6-35 qualification (2026-09-10)

The [frozen after](../../../../ratchets/h2-8a-import-helpers-after.v1.json)
records 106/111 complete commands exact twice in each of two independent jobs:
all 80 owned repairs and all 26 prior positives. All 434 supplemental complete
commands agree across those jobs, including typed errors and partial writes.
Primary commands add another 434 executions; supplemental captures are counted
separately. Original16 all match the full command twice (32 executions).
Emitter494 unit IDs and451 contract IDs are unchanged and all pass. The sole
existing unit amendment distinguishes the enclosing JavaScript effective-module
query from the historical marker query and retains typed-error cleanup checks.

All five outside controls remain failed against unchanged TypeScript expectations.
Four complete captures are unchanged from before. System helper demand now reaches
the existing `printer.rs` System helper-splice/source-map refusal with zero partial
writes: actual recorded state exposes its missing namespace/setter producer and
map phase. This changed failure boundary is retained explicitly; it is neither a
repair nor a previously exact case regression. Both full111 test processes therefore
exit101. No expected diagnostic order, option guard or accepted membership changed.

The first unit attempt exited101 at compilation (Node.flags is an integer mask),
and executed no tests. The corrected candidate is shared by all successful suites
and both focused observations. No full769 or class1228 total is inferred here.
H2.8a–e, the remaining helper branches and hosted acceptance before runtime landing
remain open. The next reviewed priority is the shared comment-emission phase.
