# H2.8a A6-30: JSDoc block-scope declaration container

The current-container predicate is qualified on the H2.8 train. The
[frozen after result](../../../../ratchets/h2-8a-jsdoc-block-scope-container-after.v1.json) is 64/64 complete commands
exact twice: all 28 fresh and two original failures are repaired, and all
32 fresh plus two original positives are preserved. These are 128 primary
complete command executions, with no supplemental captures and no remaining
failure in this owner. All 71 binder library tests and 108 existing checker
JSDoc tests pass; their sources and test identities are unchanged. All four
processes exit 0. The production diff is confined to the source-root identity
guard in crates/binder/src/bind.rs and its whole upstream annotation.

The preceding [full 769-command checkpoint](../../../../ratchets/h2-8a-global-after-a6-29.v1.json) remains
733 exact / 36 failed, no regressions, all 188 projects exact. This focused
result repairs two of those failures but does not infer a new global count.
H2.8a and H2.8b–e remain open; hosted acceptance is still required before landing.
The following original ready packet retains its pre-edit scope and evidence.


Kind: runtime, on the existing H2.8 closure train. Base `259c77e6d52bb67cad4d4d382f58f2af7cdbf88e`;
trusted train base `10748f6ee19ec083ce5748224930c5dcfbbd86df`. Prerequisites are the
completed A6-29 source state and its frozen 769-command replay: 733 exact twice,
36 failed twice, no regressions and all 188 projects exact. This packet owns
the two original JSDoc scope failures and 28 fresh failures with the same cause.
Other A owners and H2.8b–e remain open. No profile or stage activation occurs.

The SourceFile branch in TS bindBlockScopedDeclaration tests
isExternalOrCommonJsModule(container), where container is the current declaration
container. Delayed JSDoc binding intentionally computes that container separately
from blockScopeContainer. A class is an ordinary container but is not a block
scope. A tag on a constructor, method, getter, property or semicolon therefore
can have a class container and a SourceFile block scope. Rust currently tests
the file's module indicators and sends the alias to the class's symbol table.
The corrected predicate tests root identity first, so these aliases reach source
locals. Existing symbol lookup and declaration serialization then consume that table.

The 60 fresh complete TS commands are ten placements × script/ESM/CommonJS ×
ES2015/ESNext. All use strict checked JavaScript, libraries, declaration maps,
JavaScript maps, CRLF and a virtual outDir. The two collision placements exercise
ordered duplicate diagnostics and noEmitOnError write suppression. TS mint and
independent check each execute every command twice; the complete reference
contains 24 diagnostic 2300 entries, 48 exit-0/6 exit-2/6 exit-1 observations,
54 commands with four writes and six with no writes.

Two independent native before jobs each find 32 exact commands (each executed
twice) and 28 failures (once per job). All first failure vectors are identical
and are exact ordered reported diagnostics. The five class placements report
2304 instead of resolving LocalValue; constructor and method callbacks also
report 7006. Collision cases report 2304 instead of the two upstream 2300s.
All 28 are owned; zero outside, typed-runtime, unresolved or undispositioned rows.
First-job supplemental captures are separately counted: 92 additional executions,
with eight declaration texts and eight declaration-map mappings differing.
They are not complete actual tuples and do not qualify fields after the first
failed comparison. The native primary total across both jobs is 184.

The original callbackOnConstructor and typedefOnSemicolonClassElement each fail
twice in the current global replay, with four saved complete actual/expected
tuples. Both have extra diagnostics and exit 2 instead of 0; semicolon declaration
text also names A instead of string. callbackTagVariadicType and typedefOnStatements
are the two original adjacent positives, already exact twice. All four are selected
unconditionally by the new original_jsdoc_block_scope_container_commands test.

The immutable before record is
ratchets/h2-8a-jsdoc-block-scope-container-before.v1.json. Its first-native archive,
independent-repeat log, original-global source record and TS receipts retain their
actual process exits and hashes. Preparation-only repairs are retained separately;
none changes a native comparison, production byte or expected tuple.

| Semantic value | Rust representation and producer | Consumer, lifetime and invalidation | Local disposition |
| --- | --- | --- | --- |
| container | BinderWorker.container: Option<NodeId>; bind_container saves/restores, delayed_bind_jsdoc_typedef_tags reconstructs | bind_block_scoped_declaration and module table lookup, one per-file worker | partial-or-stale predicate, A6-30-2 |
| blockScopeContainer | BinderWorker.block_scope_container: Option<NodeId>; distinct enclosing block walk | match dispatch and TableRef::Locals, same worker | shared-prerequisite, separate identity retained |
| module indicators | SourceFile.external_module_indicator and BinderWorker.common_js_module_indicator | module membership only when current container equals source.root | partial-or-stale consumer, A6-30-2 |
| delayed aliases | BinderWorker.delayed_type_aliases: Vec<NodeId>, bind worker pushes | std::mem::take after normal bind, insertion order, flow/container restored | shared-prerequisite, 32 exact controls and 28 classified consumers |
| parser parent identity | immutable SourceFile arena NodeId parents finalized by parser | ancestor walks; no mutation during bind or emit | shared-prerequisite |
| tables/symbols | BindData locals/exports; TableRef; SymbolArena/SymbolId; declare_symbol owns merges | published after bind, consumed by ProgramBinder and checker; fresh Program owns fresh links | selected table repaired; merge flags/order unchanged |
| flow and chain | FlowArena START node, last_container and current_flow saved/restored | delayed bind preserves enclosing state, worker dropped at file end | shared-prerequisite |
| downstream observations | existing checker diagnostics/contextual type lookup and borrowing declaration resolver | complete command diagnostics, writes, maps, result, exit; session lifetime | all 60 plus original four must compare exactly after |

A6-30-1: validate the 21 whole upstream functions below, the eight exact architecture
rows, all authority hashes, frozen before membership and complete TS fixture. The
current syntax-and-binder §3 explains the design lineage; its historical examples
are not implementation facts. Current binder source owns Rust representation.
Checker foundations supplies the fresh Program/link lifetime premise. No borrowed
source is mutated and no persisted cache or public API is introduced.

A6-30-2: the sole allowed production path is crates/binder/src/bind.rs. In private
BinderWorker::bind_block_scoped_declaration, make the SourceFile match guard
`self.container == Some(self.source.root) && (external indicator || commonjs indicator)`.
Use the actual source.root identity, not a raw arena ordinal or node-name test.
Keep the ModuleDeclaration arm, ensure_locals, TableRef::Locals arguments, declaration
flags, diagnostics, delayed traversal and flow order byte-for-byte in their existing
owners. Preserve the whole bindBlockScopedDeclaration annotation and add the whole
isExternalOrCommonJsModule annotation at the predicate adaptation. This has no new
error path: the existing missing-container invariant still panics, ordinary duplicate
symbols still produce the same ordered diagnostics, allocation and publication retain
their current owners. No checker, parser, emitter, host, sink or config edits.

A6-30-3: run the 60 fresh complete commands twice and the four originals twice in
separate test commands, so a contracts failure cannot skip the originals. Require
all 28 fresh repairs, both original repairs and all 34 prior positives. Expected
success is 64/64 exact twice; this is a target until measured. Freeze actual exits,
logs, final binary receipts and immutable inputs before any further owner change.
No test assertion, comparator or TS expected output changes are permitted.

A6-30-4: run all binder library units and the existing checker library jsdoc tests,
single threaded with two build jobs. Unit source and test IDs remain unchanged;
the new complete compiler witnesses provide the regression test for the condition.
Any actual failure is investigated against its source owner before expanding scope.
No new host/sink/cancellation capability is reached by this private table predicate,
so those fault matrices remain unchanged in their owning packets. No transform pass,
flag propagation, source/synthetic provenance, comment cursor, receiver capture,
generated binding, helper ordering, printer context or output policy changes here.

Reproduction commands (each heavy job demoted, CARGO_BUILD_JOBS=2):

```sh
node scripts/observe-jsdoc-block-scope-container.mjs --check
python3 scripts/check-jsdoc-block-scope-container-readiness.py
cargo test -p tsc-rs-compiler --test contracts -- jsdoc_block_scope_container --nocapture --test-threads=1
cargo test -p tsc-rs-compiler --test h2_8a_original_corpus -- original_jsdoc_block_scope_container_commands --nocapture --test-threads=1
cargo test -p tsc-rs-binder --lib -- --test-threads=1
cargo test -p tsc-rs-checker --lib -- jsdoc --test-threads=1
```

The integrator is the sole writer. The schedule's current lightweight workflow
applies: focused complete upstream observations plus adjacent product regressions,
then hosted acceptance before landing. Historical walks/full developer CI are
omitted and not claimed. Preserve first native evidence before any scope amendment;
new dependencies require a new explicit pre-edit readiness amendment. No case/path
special handling, text substitution, expected normalization, catch-all success,
accepted-set weakening or stale-flag inheritance. Complete this bounded owner only
after measured 64/64 and adjacent regressions; full H2.8 remains open.

Architecture premises are all premise-unchanged, active-qualified before and after.
Their exact source rows, visibility, dated validation and predecessor references
are copied and hashed in the readiness manifest; this packet reuses their boundary,
not a claim that the binder producer was already correct:

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

Whole TS6.0.3 declarations/bodies in vendor/typescript-6.0.3/lib/_tsc.js, AST-verified:

| Owner and whole span | SHA256 | Rust owner | Step and invariant |
| --- | --- | --- | --- |
| findAncestor 11411–11422 | `014da6a625f349ff90188d7f861ef81a2351d6d7ae2b118d0fa080d78a4d6bd4` | crates/binder/src/bind.rs::enclosing_container_from / enclosing_block_scope_container_from | A6-30-1: ancestor traversal retains immutable parsed NodeId parents |
| canHaveLocals 12418–12454 | `9b06bf0fd5d08bb3347bfdf755619719eb61e7885c9da2dbca6c0dc293ca7828` | crates/binder/src/containers.rs::ensure_locals / get_container_flags | A6-30-1: existing locals-capable containers |
| isBlockScope 13786–13809 | `0f16e21e5d49b19429a2c7da58f19407ff27166aa2072a90ef6837af5b6f5d05` | crates/binder/src/bind.rs::is_block_scope | A6-30-1: classes are containers but are not block scopes |
| getEnclosingContainer 13841–13843 | `bf8b0ab1aebb621d46c4e762d1c86d15c6b754c41228d5b9eedb726e7192cf6b` | crates/binder/src/bind.rs::enclosing_container_from | A6-30-1: class container is retained |
| getEnclosingBlockScopeContainer 13844–13846 | `50444054506d87acb188cbcd3ed441a6c57e41352eda843ae6f0840bbbb1cc07` | crates/binder/src/bind.rs::enclosing_block_scope_container_from | A6-30-1: class host can select source block scope |
| isExternalOrCommonJsModule 14119–14121 | `e395fd4c4d5df1373eb3cc17bc653dfcd8f2e41b9e32d949b3063633dc02c07d` | crates/binder/src/bind.rs::bind_block_scoped_declaration | A6-30-2: current container identity must own the tested module indicators |
| createFlowNode 42404–42406 | `50f4e5850330909e853e82825ef01ab9cb1bcd9bdd13b31469b77b8127094539` | crates/binder/src/flow.rs::FlowArena::create_flow_node | A6-30-1: fresh START flow per delayed tag |
| bindSourceFile2 42456–42505 | `213891d27022fad429657a32a61e019a181b1fa5a1abd1f54586b72fdc3495a1` | crates/binder/src/bind.rs::bind_source_file | A6-30-1: normal bind, delayed typedefs, JSDoc imports; per-file worker lifetime |
| declareSymbol 42602–42674 | `cb8ed21f44a66ba3e0ee2c2bbdcc066276c64ca5f4a0cd18d8c8f87883cec24e` | crates/binder/src/declare.rs::declare_symbol | A6-30-1: existing symbol merge flags and ordered duplicate diagnostics |
| declareModuleMember 42675–42721 | `971bfda716a6fae24c6b910f1af7b3f1f9349766cb220916bffbcd552b70b3b7` | crates/binder/src/containers.rs::declare_module_member | A6-30-1: module export/local table dispatch uses current container |
| jsdocTreatAsExported 42722–42733 | `1d76227aeb3d4d8a6eb3f3dac5ef9426c5805b0d3acaf8efde651d1110d25ded` | crates/binder/src/containers.rs::jsdoc_treat_as_exported | A6-30-1: fullName alias export eligibility remains unchanged |
| bindContainer 42734–42829 | `fef772b3e91e50f62414b2e5b26c7432798feb04e6372869308938583e7923b7` | crates/binder/src/containers.rs::bind_container | A6-30-1: save/restore distinct container and block-scope identities |
| bindChildren 42843–42976 | `69e5dfbb76220dae84ed056c90a443fd58a7edfda6ac555da31683f2567d41c1` | crates/binder/src/bind.rs::bind_children | A6-30-1: existing child order followed by JSDoc binding |
| bindJSDocTypeAlias 43715–43728 | `eeaf90a6f512a62877aa3ffdaece1a4a7baaf03ab994940d8132cbd0d66e7a0a` | crates/binder/src/bind.rs::bind_jsdoc_type_alias | A6-30-1: existing delayed queue producer |
| addToContainerChain 43829–43834 | `e24ee70c25ec2d285ceae0b89e0b616a61e0e4dda58077013ac7e891793f3bd3` | crates/binder/src/containers.rs::add_to_container_chain | A6-30-1: existing ordered container chain |
| bindBlockScopedDeclaration 43972–43998 | `3d334e9a90d6bdfdb3b7d79213ed700060d8357423cf382de8dede406b119b7f` | crates/binder/src/bind.rs::bind_block_scoped_declaration | A6-30-2: ModuleDeclaration branch unchanged; SourceFile module predicate corrected; other locals unchanged |
| delayedBindJSDocTypedefTag 43999–44072 | `81918f8d6ca2b8ac860ef667494925641d30f765f62fb8515aa81cb3271fe57e` | crates/binder/src/bind.rs::delayed_bind_jsdoc_typedef_tags | A6-30-1: separate enclosing containers, normal tag order, state restored |
| bind 44226–44251 | `a221c564cd1a81b185666ceebcfa20bd6f23de9fb8120aa84b7d7a95e2de759f` | crates/binder/src/bind.rs::bind | A6-30-1: existing worker/container/children dispatch; finalized parse parents |
| bindJSDoc 44252–44269 | `5ef68b6ca2f9965b685ecab0698ebce940e986b63ded2db2e9e375d2b07a71ce` | crates/binder/src/bind.rs::bind_jsdoc | A6-30-1: existing JS-only tag traversal |
| bindWorker 44287–44527 | `4b259323fa2534e8d67ea9485669ddbff8e0a5a252719533558d6d8e99181588` | crates/binder/src/bind.rs::bind_worker | A6-30-1: existing per-kind alias registration |
| getContainerFlags 45143–45201 | `762f60f62494f6fa1a80f5b3464b7c4bc552f5cda69ebddff47c8847eaed9a81` | crates/binder/src/containers.rs::get_container_flags | A6-30-1: class IS_CONTAINER differs from block-scope classification |

The call chain is bind_source_file → bind → bind_worker / bind_container /
bind_children → bind_jsdoc → delayed queue, followed by delayed alias processing.
For each delayed tag, determine the effective host, independently reconstruct both
containers, install START flow, bind the type expression, choose declaration versus
namespace handling, then restore outer state. Named aliases reach the selected
block-scoped declaration branch; module membership invokes declare_module_member
and its existing JSDoc export rule, otherwise ensure source/block locals and invoke
declare_symbol. The 19 reused owner rows are scope-specific shared prerequisites
qualified by the adjacent controls and existing units, not global binder parity.
Both partial rows map to A6-30-2 and the same unconditional complete test. Readiness
has 21 owner rows, eight Rust-map rows, eight architecture concerns, 60 fresh plus
four original witnesses, four steps, zero unresolved and zero undispositioned.
