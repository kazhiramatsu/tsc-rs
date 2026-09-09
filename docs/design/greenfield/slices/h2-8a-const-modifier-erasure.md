# H2.8a A6-34: erase ConstKeyword modifiers

The ConstKeyword erasure branch is qualified on the H2.8 train. The
[frozen after result](../../../../ratchets/h2-8a-const-modifier-erasure-after.v1.json) has all 144 ordinary complete commands
exact twice (80 new and 64 adjacent), independently repeated: 28 repaired and
116 preserved. All 494 existing emitter units and 451 contracts pass with
unchanged test sources. The five after jobs contain 584 primary Program command
attempts and 288 separately counted supplemental attempts.

The original grammar selector retains three exact commands and one complete
unequal command, each twice. The latter's two extra const prefixes are now
removed: native JavaScript is 4291 bytes versus TypeScript's 4284. All fields
except callback/materialized bytes and their lengths agree; its sole remaining
difference is `export cantExportProperties: 4` in a PropertyAssignment. The exact
full-tuple delta from A33 is only those two const removals. The property printer
owns that remaining failure. It is not counted as a newly exact original case.

Production changes only the private TypeScript modifier predicate and its
upstream ledger in builtins.rs. The test adapter separately maps the already
existing preserveConstEnums option, with a retained one-line preimage proof.
Both the initial partial preparation result and complete corrected before jobs
remain frozen. Expected bytes, original selectors and existing tests are
unchanged. Five current predecessor pins receive this exact adapter amendment;
older already-stale historical manifests are not restamped or claimed passing.

The global A29 checkpoint stays 733/769. A33 modifier-comment/token-leading
composition, other output families and H2.8b–e remain open. Existing hosted
acceptance is still required before landing; the historical certificate walk
and full developer CI remain omitted. The frozen pre-edit scope follows.


Kind: runtime. Base `6aa294728610c1478942bf7dffbf3e1fb8f2f171`; trusted train base
`10748f6ee19ec083ce5748224930c5dcfbbd86df`. H2.8a–e remain open.
This packet owns the existing TypeScript modifier filter's missing ConstKeyword
arm. A33 is committed; the last full A29 result remains 733/769. No focused
repair is subtracted from that global denominator.

The complete 80-case before has 52 exact and 28 failed commands, independently
repeated with identical first vectors. Both qualified jobs exited 101. There
are 264 primary and 132 separately counted supplemental executions. Every
failure first differs at the exact source-map result; complete supplemental
outputs attribute all 28 to extra `const ` prefixes and map `mappings` only.
Declaration bytes/maps are unchanged. This attribution does not normalize any
expected output or qualify later fields of a failed comparison.

The initial attempt is separately archived: 50 exact, 28 unequal and two
test-adapter preparation rejections at `preserveConstEnums`, before those two
Programs were loaded. Its 128 primary/128 supplemental executions are not the
complete-80 baseline. The sole test adapter correction maps that option to the
already existing `CompilerOptions::preserve_const_enums`; it changes no product
behavior. Complete before observations were made after that correction.

## Source boundary and executable change

TS `modifierToFlag` maps ConstKeyword to 4096, included in TypeScriptModifier
28895. `createToken` already produces ContainsTypeScript for this token.
`visitor` enters `saveStateAndInvoke`, saves current scope, calls
`onBeforeVisitNode`, then `visitorWorker`; the worker gates on ContainsTypeScript
and calls `visitTypeScript`. Source-element and class-element entry points keep
their separate dispatch. `visitTypeScript` drops ConstKeyword alongside the
other type-only modifiers. `modifierVisitor` uses the same TypeScriptModifier
classification; decoratorElidingVisitor and modifierElidingVisitor preserve
their distinct predicates. The saved state is restored by the existing owner.

Class fact calculation selects class update/lowering and class-element visits.
Properties remove ambient/type-only data; constructors already clear modifiers;
methods/accessors visit their modifier lists before updating the node. Enum
emission first applies shouldEmitEnumDeclaration, retaining const enums only
under the existing computed preservation option; runtime enum modifier filtering
already explicitly excludes ConstKeyword. Module emission keeps its declaration
eligibility and namespace state. Variable statements/declarations keep NodeFlags
for `const` variables: the keyword is not a ConstKeyword modifier child.

The private `is_typescript_modifier` in `crates/emitter/src/builtins.rs` has four
consumers. A6-34-1 adds only `SyntaxKind::ConstKeyword` to its matches expression
and a whole-function upstream ledger. `TypeScriptVisitor::visit_with_typescript_gate`
then returns None through its existing token-erasure branch and memoizes that
result for this visitor. `enum_runtime_modifiers` already removes ConstKeyword;
`module_runtime_modifiers` gains the same source predicate. The existing
`local_transform_flags` token arm already ORs ContainsTypeScript for ConstKeyword,
so the added predicate is idempotent for every existing flag value. Do not
remove that token arm or change a flag mask.

Allowed production: that one private predicate and its ledger in builtins.rs.
Allowed evidence: observer, fixture, integration registration, the exact option
adapter correction above, packet/checker/records, index and H2.8 progress pins.
Forbidden: parser/checker/plan/factory/printer edits, new public APIs/state,
InKeyword/OutKeyword expansion, expected-byte edits, output substitution,
case/path branches, fallback success and changes to accepted-state baselines.

A6-34-2 runs the complete 80 new and 64 class-optional-name controls, each twice,
and repeats that job independently. All 144 must be exact. Reuse of A33's four
original before tuples requires identical production/source-selector hashes;
it adds zero new original-before executions. Run the unchanged original selector
afterward: three prior exact tuples must stay exact. The fourth must produce
complete tuples twice with only its independently owned extra property `export`
remaining; it is still a failed complete comparison. Run unchanged emitter unit
and contract suites (494 and 451 respectively). Any newly reached owner requires
an amended packet and fresh readiness before further production changes.

## Rust semantic map and current gaps

| Source fact / gap | Producer → current Rust owner/updater → consumer | Lifetime, identity, completion |
| --- | --- | --- |
| Const modifier / partial-or-stale | parsed SyntaxKind → private is_typescript_modifier → visit_with_typescript_gate | per-token predicate; A6-34-1, 28 full-command repairs |
| absent visited child / shared-prerequisite | TypeScriptVisitor token None → existing try_visit_each_child/array visitor → update_generic | per-visitor nodes memo retains None; arrays drop only absent children |
| current parent / shared-prerequisite | update_generic → flags_after_update → NodeFactory::update_node | existing detached arena update, original identity and ranges retained; parsed arena immutable |
| token flags / already-exact | local_transform_flags Const token arm → TransformArena flags → ContainsTypeScript gate | added predicate OR is idempotent; no new bits or recomputation mask |
| class metadata / shared-prerequisite | existing class visitors → session EmitMetadata → later class-fields/decorator passes | source/current/class definition identities remain separate; no metadata mutation added |
| const variables and keyword names / already-exact | parser NodeFlags or Identifier → unchanged visitors/printer | ordinary variables, enum members and property names remain; 52 prior positives |
| enum preservation / already-exact | existing CompilerOptions computed method → enum eligibility/runtime filter → existing enum transform | no duplicate option cache; default/preserved enum witnesses exact before |
| namespace state / shared-prerequisite | TypeScriptVisitor namespace_stack → module_runtime_modifiers and module visitors | existing push/restore and export predicate; namespace const-variable controls |
| receiver and generated bindings / shared-prerequisite | existing transform frames/generated binding scopes → later lowering/finalization | no allocation or capture rule added; ES5/ES2015/ESNext controls |
| comments, maps and results / shared-prerequisite | existing parent ranges/EmitContext → printer and MemoryOutputSink → complete command comparator | typed positions, source comment ownership/resume and callbacks retained; raw bytes/order/status checked |

The invariant architecture premises below retain their existing qualification.
The newly repaired token branch is active-unqualified until the immutable after
record qualifies this profile. Dormant broad map/declaration/API rows are not
inherited compatibility: declaration/map controls are executed here, while
general config/host/transpile/targeted/CLI owners remain H2.8b–e.

EA-GAP-FLAGS remains open for full-lattice recomputation. This change updates
parents through the existing postorder flags_after_update and node factory;
it does not claim no AST updates, inherit a new partial mask, or synthesize
new token kinds. Const already contributes exactly ContainsTypeScript.
EA-GAP-CAPTURE remains open outside qualified scopes. No receiver, function/loop
frame, generated binding, hook, expression context or comment-resume lifetime
changes. Earlier TypeScript erasure feeds the existing ordered decorator, target,
class-field and module passes; the fresh target/module witnesses exercise this
composition. Existing Result errors propagate unchanged; no new effect or
sink/cancellation boundary is introduced. noEmitOnError controls protect blocked
output. Broader host/sink fault behavior is an unchanged architecture premise.

## Pinned upstream owners and architecture

Every whole function below was read, including its branch predicates, callers
and callees. The map above and source-order discussion assign their retained
dependencies. All rows map to A6-34-1 or A6-34-2 and the frozen complete-command
test in the machine manifest. The option property is pinned separately.

| Whole TS owner | `_tsc.js` span | SHA256 | Gap / step |
| --- | --- | --- | --- |
| `modifierToFlag` | 17035–17071 | `3d5bdf349d9c880e96706263e2ac1043c579f0ce98e4c0f98b746030f334145b` | shared-prerequisite; A6-34-1 |
| `createToken` | 21710–21766 | `c78e317d4226871f44d628bcae0862e3c7c7a3b4d67c798042153e85455dc041` | shared-prerequisite; A6-34-1 |
| `saveStateAndInvoke` | 94086–94096 | `9c9631e0317b2eb51171d28afcdd8b57673a25cda1ce6c8f8dc0c5f7807e6203` | shared-prerequisite; A6-34-1 |
| `onBeforeVisitNode` | 94097–94118 | `9e6cdf9efd5f0019b1e19673fcfe5a4ffed84265eba5385f57404bf833bc8858` | shared-prerequisite; A6-34-1 |
| `visitor` | 94119–94121 | `7510e745d336993739b10a48656dc94d6323f34154830d12e0e23b4197f81042` | shared-prerequisite; A6-34-1 |
| `visitorWorker` | 94122–94127 | `3e070025ada6ea8db623872da91537a2ee373fdb66224c5ece5786511d972198` | shared-prerequisite; A6-34-1 |
| `sourceElementVisitor` | 94128–94130 | `8a80dcce22611c1d511b9f7b30eaf315e138763de3ec196c7809cbfaa27b0e37` | shared-prerequisite; A6-34-1 |
| `sourceElementVisitorWorker` | 94131–94141 | `9dc0fe5c09e83bc372f867d5d303ed166b7870c6ab2b21e01ff530d2a8c35800` | shared-prerequisite; A6-34-1 |
| `getClassElementVisitor` | 94215–94217 | `1e1752c8ee0ea3321e0b645a22af471cf081a4e2def3b0fce2d5fa5212ecf12e` | shared-prerequisite; A6-34-1 |
| `classElementVisitorWorker` | 94218–94239 | `ab9945158502af2ca9cb98b00ef4c94ec5cbfa70475ff7d150b0ab5e74cb6902` | shared-prerequisite; A6-34-1 |
| `decoratorElidingVisitor` | 94259–94261 | `1a4771049b79c8701523315e211841fbb619946e260af61630b1a91d6280ae41` | shared-prerequisite; A6-34-1 |
| `modifierElidingVisitor` | 94262–94264 | `66a5524cc89e8d323c948eaa1ae083e77725f037422d59e0e90b5c28bcb9d8ee` | shared-prerequisite; A6-34-1 |
| `modifierVisitor` | 94265–94273 | `1b99b2cf530cfc7d5ec216d5825eb98cd57ff653eb98d741415001177ec35978` | partial-or-stale; A6-34-1 |
| `visitTypeScript` | 94274–94389 | `bce515282739cfd7721a4be04b7f91e3b05015cc09fbc1ef5a1cb58d578cc510` | partial-or-stale; A6-34-1 |
| `getClassFacts` | 94410–94427 | `81b81924250121c13e7318fa6fe7748c25e8772b034bd7b78c11b5226d4b6651` | shared-prerequisite; A6-34-1 |
| `hasTypeScriptClassSyntax` | 94428–94430 | `2cc078fafd2c2ed3cbe7e1a48d8fea5fb75a6f220c96862ab9a99d800da0663b` | shared-prerequisite; A6-34-1 |
| `isClassLikeDeclarationWithTypeScriptSyntax` | 94431–94433 | `f99b70774885874532bc1947c7b742b2f3257b1a97de87676502becfef31dab8` | shared-prerequisite; A6-34-1 |
| `visitClassDeclaration` | 94434–94548 | `b4f4c7bb3c8f14a7776dd0ab5337e8c11b30104d7eb70b676c5dba79a9e1ae59` | shared-prerequisite; A6-34-1 |
| `visitClassExpression` | 94549–94563 | `4dae4f7a40f66795c79f1667200c7b9d9d63898d21391eeda09415cd761b4605` | shared-prerequisite; A6-34-1 |
| `transformClassMembers` | 94564–94598 | `306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8` | shared-prerequisite; A6-34-1 |
| `injectClassElementTypeMetadata` | 94611–94623 | `78c344acb0156f75208813131956a124255f130cc7cd28bf4ff47a3558bc8632` | shared-prerequisite; A6-34-1 |
| `visitPropertyNameOfClassElement` | 94732–94745 | `91f63521057be7cf65255a3f737b8a2b466dd48633708c74a268d9dccae88d43` | shared-prerequisite; A6-34-1 |
| `shouldEmitFunctionLikeDeclaration` | 94760–94762 | `c687bd932c50cfb720df2b84445ec71dd21842b8731e7558437e8735d353f0bf` | shared-prerequisite; A6-34-1 |
| `visitPropertyDeclaration` | 94763–94793 | `dbfc13d8681393f9bd7e687ca0d329cac727a02f7ce2c42db15a752c6916b959` | shared-prerequisite; A6-34-1 |
| `visitConstructor` | 94794–94805 | `99c584007ae74e3c29b119097c6f383f3844ba0a5e4f0cfc259c3f1cce2c942a` | shared-prerequisite; A6-34-1 |
| `visitMethodDeclaration` | 94911–94934 | `857d014d380ea47881c963c1802d22c2de735dc427e69784559ba582c2056176` | shared-prerequisite; A6-34-1 |
| `shouldEmitAccessorDeclaration` | 94935–94937 | `f4553053fcdec2af760a047f4367fb7ca84360fd0e5a8ed8b8bc15f2e3fe113d` | shared-prerequisite; A6-34-1 |
| `visitGetAccessor` | 94938–94956 | `9fcdbcc02cd3bcb547e013863ce96c78f4c5d5242262ebf861700a0d0d944085` | shared-prerequisite; A6-34-1 |
| `visitSetAccessor` | 94957–94973 | `a6550a8556985252c95f7ed5e497f100d3414eeeca28e5c5e0c2588c552352d6` | shared-prerequisite; A6-34-1 |
| `visitVariableStatement` | 95052–95069 | `b2161e99280cedae5103140325ab71391f5d025348ebcacf7e1166ac86d67a94` | shared-prerequisite; A6-34-1 |
| `visitVariableDeclaration` | 95093–95107 | `e02eafc6bada5a766350df029997d839f990c36a609ccc2a731cccc3e2b94b62` | shared-prerequisite; A6-34-1 |
| `shouldEmitEnumDeclaration` | 95177–95179 | `8f652cbb3ecccf44d02da3cd07a7d116ac6285e3074ea8b4b5b2f26f66c5d6a7` | shared-prerequisite; A6-34-1 |
| `visitEnumDeclaration` | 95180–95261 | `d9c6144e575b7cf631814b05cb26fd6e25fcc76ab872424509de7ce1ce36f96b` | shared-prerequisite; A6-34-1 |
| `shouldEmitModuleDeclaration` | 95325–95331 | `57d2d69f36aa92389324be8744e8b861c036a61e68bafbcce2782455a0d4e36b` | shared-prerequisite; A6-34-1 |
| `addVarForEnumOrModuleDeclaration` | 95352–95382 | `c5d0e3bfef96bcdca32833d26e9711b7f3c262bf2e1bae97a2cbe1530eafd5f0` | shared-prerequisite; A6-34-1 |
| `visitModuleDeclaration` | 95383–95466 | `c51a760030f209b2382ad74105c99301ddee96eab9cb4a897ed7e8a5ecfb3dcf` | shared-prerequisite; A6-34-1 |
| `_computedOptions.preserveConstEnums` | 18157–18162 | `edc68fd84cd2acbcb4278efd5d9341624e7e889e109ae3b37b1eb471c33d90b1` | already-exact; A6-34-2 |

| Architecture ID | Invariant | Current Rust / visibility | Validation / lifecycle | Qualification |
| --- | --- | --- | --- | --- |
| `E-ARENA` | Parsed trees remain immutable; the detached arena appends synthetic nodes and tracks the mounted parsed interval. | `tsc_emitter::{TransformArena,TransformSource,TransformSourceId,TransformNode,TransformNodeArray,NodeFactory}` (public) Existing `NodeFactory::modifier_flags` is now `pub(crate)`; downlevel auto-accessor setters use fresh factory modifiers while getters retain source tokens. | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; every later transform packet maps provenance explicitly |
| `E-METADATA-BASE` | Transform flags and `emitNode`-equivalent facts are sparse session side tables; there is no standalone Rust `EmitNode` syntax object. Original/map/comment/value identities are separate. CommentRange carries independent source start/end states through CommentSourceRange; paired source slices and source-map ranges keep their existing domains. | Public `tsc_emitter::TransformArena::{transform_flags,set_transform_flags,array_transform_flags,set_array_transform_flags,metadata,metadata_mut,clear_session_metadata,get_original_node,set_original_node}` over private storage in `crates/emitter/src/factory.rs`; public `tsc_emitter::{EmitMetadata,SourceMapRange,CommentRange,CommentSourceRange,JavaScriptString}` defined in `crates/emitter/src/metadata.rs` (`EmitMetadata` storage fields remain `pub(crate)`, with its cross-crate operations exposed by public methods) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. A6-29 candidate uses existing `EmitMetadata::set_flags` to replace inherited flags on the ES5 accessor receiver clone, retaining its original/text/map ownership. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; H2.5h/H2.6 extend rather than infer from text |
| `E-METADATA-G-CLASS` | Cross-pass class facts retain typed source/declaration identity: a synthesized legacy-decorated class expression records its declaration owner, standard/ESNext transforms can transport `class_this` and `assigned_name`, generated constructor reads point at their class owner, and generated computed names retain cache provenance. The TypeScript pass projects a parameter property as a `PropertyDeclaration` whose original is its source `Parameter`; later standard-decorator updates preserve that chain. Class-field lowering follows the full original-node chain when it needs either source-language declaration, and never derives a runtime name, constructor local, or resolver identity from a publication/generated spelling. | `crate::metadata::ClassExpressionDeclarationOrigin` (`pub(crate)`); public re-export `tsc_emitter::InternalEmitFlags::GENERATED_COMPUTED_PROPERTY_NAME`; public `tsc_emitter::EmitMetadata` with `pub(crate)` fields `class_this`, `assigned_name`, `class_constructor_reference`, and `class_expression_declaration_origin`; producers in private `builtins.rs`, `legacy_decorators.rs`, `es_next.rs`, and `standard_decorators.rs`; consumers in private `class_fields.rs` and `class_fields/downlevel.rs` | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate-only audit 2026-08-14 | Current-code audit and focused named-evaluation/decorated-class/parameter-property contracts, including `legacy_decorated_anonymous_default_reserves_definition_before_computed_key`; H2.5g inventory/profile for observable parity |
| `E-CONTEXT` | One per-unit context owns lexical/block environments, hoists, helpers, diagnostics, hooks, initialization, and reverse disposal. | `tsc_emitter::{TransformationContext,Transformer,TransformationResult,transform_nodes}` (public) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. | H2.5g profile and transform lifecycle controls |
| `E-NAMES-BASE` | Generated identity, printable name, target provenance, and allocation scope are separate; final names use the composed tree. | `crate::transform::GeneratedBindingId` (`pub(crate)`); `crate::builtins::generated_bindings::GeneratedBindingScopes` and `target_bindings::{TargetBinding,finalize_generated_binding_names}` (`pub(super)`) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and generated-binding controls |
| `E-PRINTER-BASE` | Printer has no direct checker dependency. It consumes transformed syntax/metadata, drives retained substitution and before/after notification hooks, and applies immutable structural planning. | `tsc_emitter::Printer` (public); `tsc_emitter::TransformationResult::{substitute_node,before_emit_node,after_emit_node}`; `crate::printer::EmissionPlan` and `EmitContext` (private); `crate::factory::NodeFactory::apply_parenthesizer_rules` (private) | `active-qualified`; qualified at validation ref `6acd5d43` (2026-08-21, CS-6 fixture/audit gate); previous ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. The H2.5h-a comment-scope packets reshape the private context carrier into the threaded `EmitContext` (CS-2 landed the root and core pipeline byte-identically); the row's hook-composition, no-checker-dependency, and immutable-planning invariants are preserved; requalified at `6acd5d43`. | H2.5g profile; later packets preserve hook composition and enumerate new expression contexts |
| `E-COMMENTS-G` | Parsed ownership, synthetic comments, relocated owners, token progress, and comment progress are distinct typed facts. `TokenEmission` may hand its cursor plus an optional `CommentResume` only to the immediately following token/child; the resume carries both owner start and next position, requires one source and monotone progress, and can merge only with the same owner. A retained arrow anchors trailing comments at its own comment-range end, not semantic-original provenance, so a synthetic token cannot borrow source comments. The simple parameter's list-owned trailing comments and the retained arrow token's trailing comments remain separate phases, including tsc's observable list/leading replay at a shared range start. `SourceLeadingCommentPhaseVisit` distinguishes an absent/suppressed phase from a visited exact range, so a transitional class-field comment anchor is skipped only after that same range was visited; contextless routes retain the anchor. Token/comment ownership performs no source-wide token search; the position-cursor contract keeps local source work linear. H2.5g qualifies only the direct expression and list routes enumerated by its source-comment topology contracts; it does not claim that tsc's enclosing comment-container state already reaches every nested node route. Comment ownership now distinguishes paired, synthesized, start-only and end-only positions; positional readers and ordinary node-phase extent checks remain separate. | Public `tsc_emitter::SyntheticComment`; `crate::metadata::RelocatedStatementListComments` (`pub(crate)`); `crate::comment_cursor::{CommentCursor,CommentResume,CommentResumeError}` and `crate::token_cursor::{TokenCursor,TokenEmission,TokenAnchor,TokenCommentBoundary,FixedToken,TokenWriteKind,TokenLeadingSpace}` (`pub(crate)`); private `crate::printer::SourceLeadingCommentPhaseVisit` and other comment/list workers | `active-qualified`; qualified at validation ref `6acd5d43` (2026-08-21, CS-6 fixture/audit gate); previous ref `0653e10d` (2026-08-17); candidate-only audit 2026-08-14. The H2.5h-a comment-scope packet CS-3 re-expresses the row's qualified expression/list comment projections on the per-side threaded scope (cursor/resume semantics and the token machinery unchanged); requalified at `6acd5d43`; A6-28-5 retires the obsolete initializer trailing-comment marker; [the shared packet](h2-8a-class-field-initializer-comments.md) qualifies all64 new controls and8 prior repairs in434/556 exact complete commands twice, with all490 units/451 contracts passing; subsequent [A6-28-6](h2-8a-promoted-class-export-maps.md) qualifies all100 selected map repairs plus six ES2022 cases in764/868 exact complete commands twice and all490/451 emitter tests, leaving four original A28 comment targets at that prerequisite; subsequent A6-28-7 qualifies all four; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. | Current-code audit, token/comment cursor unit contracts, `position_cursor_2727_statement_work_is_linear_and_scan_free`, source-comment topology and relocated class-field contracts, and arrow replay/resume integration contracts; H2.5g inventory/profile for observable parity |
| `E-COMMENT-SCOPE-H` | tsc scopes three independent values across nested emit routes: `containerPos`, `containerEnd`, and `declarationListContainerEnd`. The threaded immutable representation mandated by this row is LANDED end to end through the H2.5h-a comment-scope packets (CS-2..CS-6): the printer's ambient comment state is the immutable `CommentEmissionScope` triple threaded through an explicit `EmitContext` with `EmitContext::file_root` as the single zero-scope constructor, tsc's per-side flag/JsxText claim predicates on every route, the declaration-list writer (`claim_declaration_list_sides`), and the `NO_NESTED_COMMENTS` suppressed extent. The 30-case witness-driven fixture gate (byte parity against the frozen artifact, both `removeComments` polarities, all six transforms) and the permanent emitter-scoped zero-contextless workspace audit enforce it; qualified at `6acd5d43`. ES2015/Generators production (H2.5h-b) may now proceed. A single optional range or mutable ad-hoc stack is not an equivalent semantic model. Present comment endpoints claim their own sides independently; missing sides and sole-zero positions do not claim a container, and suppression flags cannot create an absent endpoint. | Landed private `crate::comment_cursor::CommentEmissionScope` and `crate::printer::EmitContext` (with `crate::printer::ExpressionSyntaxContext` as its syntax half); the 30-case artifact-driven fixture suite `crates/emitter/tests/integration/comment_scope_witness_contract.rs` | `active-qualified`; CS-2 landed 2026-08-18 ([CS-2 packet](h2-5h-a-cs-2.md)); CS-3 landed 2026-08-19 ([CS-3 packet](h2-5h-a-cs-3.md)); CS-4 landed 2026-08-20 ([CS-4 packet](h2-5h-a-cs-4.md)); CS-5 landed 2026-08-20 ([CS-5 packet](h2-5h-a-cs-5.md)); CS-6 landed 2026-08-21 ([CS-6 packet](h2-5h-a-cs-6.md)); qualified at `6acd5d43`; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. | Pinned tsc scope/save-restore graph; direct/list/wrapper/declaration-list oracle fixtures; contextless-call zero-use audit; focused, emitter, owner-control, and inventory gates |
| `E-POSITIONS` | Source bytes, source/generated UTF-16, synthetic ranges, and source switches remain typed domains. | Public `tsc_emitter` position/writer/hook types; definitions in private `position`, `writer`, `metadata`, and `printer` modules | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H1/H2.5f protect predecessor behavior. | H2.5g profile and H1 Unicode/newline controls |
| `E-OUTPUT-SCRIPT` | JavaScript artifacts are constructed before the first sink callback; callback order and `emittedFiles` stay independent. | `tsc_emitter::{emit_files_with_activity,EmitArtifact,MemoryOutputSink,FsOutputSink,EmitOutcome}` (public) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and H1 sink-failure controls |
| `E-PLAN-SCRIPT` | JavaScript selection/root/mode/path planning stays typed and fail-closed. | `tsc_emitter::{EmitSelection,EmitRoot,EmitMode,EmitOutputPaths,EmitOutputUnit,EmitOutputPlan,EmitPreflight}` (public re-exports; definitions in private `plan` module) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and output-plan controls |
| `E-RESOLVER-IDENTITY-G` | A resolver query accepts only an identity anchored in the mounted immutable parse interval. Projection follows typed original-node provenance, rejects synthesized/range-incompatible identities, and carries the owning Program source so an appended raw `NodeId` cannot alias a parsed node in another source. | `tsc_emitter::TransformSource::{contains_parsed_node,program_source}` and `TransformArena::{is_parsed_node,parse_tree_resolver_node,require_parse_tree_resolver_node}` (public); `tsc_emitter::EmitResolverNode` | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate-only audit 2026-08-14 | Current-code audit and resolver-identity contracts; H2.5g inventory/profile for observable parity; every later resolver consumer revalidates the projection |

## Frozen witnesses, commands and acceptance

The fixture contains 80 ordinary complete Programs: 12 core shapes × JS/TS ×
ES2015/ESNext = 48, with 32 removeComments, blocked, ES5, CommonJS,
declaration/maps, enum-preservation, enum-name, const-type-parameter and namespace
controls. The observer produces expected output without hand editing. Two TS
jobs each run every case twice: 320 Programs. Their identical log SHA is
`0322b14cb3084130c45370596e97471ce307921d73c30336d9b891d92580e434`.
Proposed malformed const-object-method and const-namespace shapes were excluded
before observations: the parsed AST has no ConstKeyword modifier for those
shapes. The shape audit and failed preparatory audit attempts remain archived.

```sh
node scripts/observe-const-modifier-erasure.mjs --check
python3 scripts/check-const-modifier-erasure-readiness.py
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test contracts -- const_modifier_erasure_matches_complete_typescript_observations class_optional_name_matches_complete_typescript_observations --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test h2_8a_original_corpus -- original_class_optional_name_commands --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-emitter --lib -- --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-emitter --test contracts -- --test-threads=1
```

The manifest pins all authorities, fixture rows, source owners, unchanged tests,
base Rust bytes, option adapter preimage and before observations. Readiness must
exit zero before production. One writer, one heavy job, two build workers;
consume actual process exits before dependent launch or pinned edits. Prelaunch
archives and logs retain exact commands, bytes, exits and times. Read-only and
new scratch work may continue while a job runs. Readiness: 36 whole functions,
one computed property, 10 Rust rows, 80 witnesses, 12 architecture premises,
two open-gap dispositions, two steps, unresolved=0, undispositioned=0.

Hosted acceptance is required before landing. The user-authorized schedule
omits the historical certificate walk/full developer CI and does not claim
them passing. This owner may close only after all 28 repairs, 52 prior positives,
64 adjacent commands, original dispositions, unchanged 494/451 emitter tests
and final bytes are verified. Next independent owners: property-assignment
modifier emission and A33's modifier-comment/token-leading composition. H2.8
completion additionally requires the remaining output families and all B–E work.
