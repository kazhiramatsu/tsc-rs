# H2.8a A6-33: class keyword comments and header suppression

The class keyword/comment profile is qualified on the H2.8 train. The
[frozen after result](../../../../ratchets/h2-8a-class-header-token-after.v1.json) has 144/152 ordinary complete commands
exact twice: 80/88 new and 64/64 A32 predecessors, with all 64 selected repairs
and 80 prior positives. The original four retain 3/4 complete exact cases;
the remaining complete unequal tuple is byte-identical to A32 in both runs.
All 32 direct printer metadata controls now match twice, repairing 12 cases.
All 451 existing emitter contracts pass with unchanged test sources. The after
jobs contain 600 primary Program command attempts, 296 separately counted
supplemental commands and 64 direct prints. Direct prints are not Program
admissions. The global checkpoint remains 733/769; H2.8a–e remain open.

Only `Printer::emit_class` and the new private `class_keyword_cursor` change.
The helper preserves current-node single-position ownership and reads actual
modifier/decorator endpoints; class keyword/name-end comments honor the existing
immutable NoNestedComments context. No parser, checker, transform, factory,
generated-name, state or public API changes are included.

The eight independently repeated outside tuples now differ only in JS text and
map mappings: native emits `/* before class */class`, while TS has a space
between the comment and class. Source review identifies two existing composition
owners. The modifier's source trailing-comment request is missing, and the
generic fixed-token leading phase calls the trailing/leading union helper
`emit_comments_at_cursor_with_anchor`. At each last modifier endpoint TS's source
scanner finds zero leading and one trailing comment. Native's union replays the
trailing comment at the keyword and supplies no following space. The next owner
must distinguish leading-only token comments from union phases and supply the
ordinary modifier request together; adding only a modifier scan would duplicate
the comment. These eight full tuples remain failed. No text-spacing workaround
or synthetic consumed-comment state is introduced. The pre-edit prediction of
missing modifier text was refined by this complete after observation, not used
as an expected-output adjustment.

The original grammar tuple still has two extra const prefixes and one extra
PropertyAssignment export prefix. Those transformer/property-printer owners
remain separate. No new failure or unstable repeated result is accepted.

Before production, the readiness test identifier was corrected to the actual
metadata test function and the checker strengthened to verify test symbols.
The initial ready0/pre-edit archive remains preserved; the second ready0/pre-edit
archive authorized implementation. No production edit or runtime measurement
used the initial incomplete traceability check.

The frozen pre-edit scope follows. Existing hosted acceptance is required before
landing; historical certificate walk/full developer CI remain omitted.


Kind: runtime. Base `973f5988b515baa443618cc69e259b3e0ae55cd9`; trusted train base
`10748f6ee19ec083ce5748224930c5dcfbbd86df`. H2.8a–e remain open. This packet
owns the class keyword's source-comment phase and the existing class name-end
source-comment phase's use of the child-facing suppression context. A32 is
committed with 61/68 complete commands exact twice. Its six keyword-comment
failures are included here; the original grammar case's three extra modifier
prefixes remain a separate transformer/property-printer owner. The measured
global checkpoint stays 733/769; focused gains do not alter that count.

## Before evidence and admitted profile

The 88 new ordinary commands have identical complete TypeScript observations
from two independent jobs (352 Programs). They cover 16 named/anonymous/default/
expression/header shapes across JS/TS and ES2015/ESNext, plus eight removeComments,
eight ES5, four CommonJS and four declaration/map controls. Native before is
22 exact twice and 66 failed, independently repeated with identical first
vectors: 220 primary attempts and 110 separately counted supplemental executions.
All 66 first failures are exact source-map results; supplemental writes attribute
only JS text/mappings differences, with declaration text/maps unchanged.

The 32 independent direct-printer controls cover four class shapes, four CLASS
EmitFlags combinations (none, NoComments, NoNestedComments, both), and both
removeComments values. TypeScript's two jobs contain 128 direct prints and zero
Programs. Native before attempts every case twice: 20 exact, 12 failed twice,
64 complete actual prints, 24 complete mismatch captures. These are printer
observations, not Program admissions. Expected text, UTF8/base64/count and UTF16
end positions are oracle-generated. The 12 failures split into eight missing
keyword comments and four incorrectly retained name-end comments.

Before records: `ratchets/h2-8a-class-header-token-before.v1.json` and
`ratchets/h2-8a-class-header-token-metadata-before.v1.json`. Their receipts pin
inputs, logs, exits, binary identities and separate source archives. A32 after
records supply unchanged 64-command and four-original predecessor evidence.
No field after a first failing assertion is qualified by a supplemental capture.

## Exact source order and implementation

`emitClassDeclaration` directly delegates; `emitClassExpression` first generates
its required name. `emitClassDeclarationOrExpression` emits decorators/modifiers,
the ClassKeyword through `emitTokenWithComment(moveRangePastModifiers(node).pos)`,
optional IdentifierName, indentation/types/heritage, brace, member generation
scope/list, closing brace and indentation restore. Its token continuation is
unused. `getParseTreeNode` and the same-kind predicate control source-token
ownership; a copied range cannot confer it. `writeTokenText` advances by the
fixed spelling length; it performs no source-wide lexical search.

**A6-33-1:** Add private `Printer::class_keyword_cursor`, used only by emit_class
after the existing emit_modifiers call and its following space. Read the current
class record and optional current modifier list. If its last actual node has a
non-synthetic end, use that end. Otherwise find the last Decorator in reverse;
use its end only if non-synthetic. Otherwise use CURRENT class.pos. Do not scan
past a synthetic last decorator to another decorator. Ignore class.end: this is
a single position, not a paired range. `u32::MAX` maps to TokenCursor::Synthetic;
other values go through SourceBytePosition::new with the current source's
positions. Missing node/array and invalid point errors propagate as existing
PrinterError/TransformError/position errors. No mutation or source lookup by text.
Add whole-function moveRangePastModifiers and moveRangePastDecorators ledger
identities to the helper. Their property/method special arm is unreachable:
the private helper's sole caller is the class emitter, reached by exactly the
ClassDeclaration and ClassExpression NodeData arms.

Replace the raw class keyword write with the existing fixed-token helper,
FixedToken::keyword(ClassKeyword), this cursor, class_node and indentLeading=false.
Keep emit_class's whole-function ledger. The token is unmapped by this helper;
existing downstream name/node maps account for changed generated text. Preserve
the existing same-kind original ownership test and arithmetic advance.

**A6-33-2:** Use existing EmitContext::nested_comments_suppressed for the two
class header source-comment phases. Compute the anchor after modifiers even
when suppressed, preserving producer/error order. When suppressed, write the
raw keyword without comment effects; do not fabricate a Synthetic cursor as a
suppression signal. No continuation is consumed. Guard the existing name-end
emit_comments_at_cursor call by that same context; keep name lookup/spelling,
IdentifierName hint, allocator and its ordinary hook path. Class NoComments
alone does not suppress these descendants. removeComments remains a printer
option handled by the existing comment helpers. No new global state, flags,
session metadata, node generation or lifecycle is introduced.

The 88+64 ordinary fixtures and 32 direct inputs contain 202 parsed classes and
100 names; none has a name-leading comment range. Thus no missing name-leading
phase is added or qualified by this packet. CJS's unnamed class receives a fresh
synthetic name from CommonJsModuleVisitor::create_identifier; create_node sets
MAX/MAX and SYNTHESIZED, with no parsed-name range. update_generic/update_node
retain current class.pos/end after removing export modifiers. TS getName clones
a parsed identifier with its text range/flags, otherwise getGeneratedNameForNode
keeps a synthetic identifier with original=class. This explains the two CJS
default controls where TS intentionally does not print the keyword comment:
the current cursor starts at export and the fixed length advances over that
prefix. Lexically searching for `class` would regress them. Declaration controls
protect actual generated/retained name and output behavior without widening the
profile to arbitrary synthetic comments or custom transform injection.

Allowed production: only `crates/emitter/src/printer.rs`, emit_class and its new
private cursor helper. Existing generic token/modifier/name/child emitters remain
prerequisites. Allowed evidence: observers, fixtures, tests/registration, versioned
records, readiness checker and packet/index/progress. Parser, binder, checker,
transforms/factory, public APIs, expected bytes, output rewriting, new state,
fallback names, flags and admission guards are forbidden edits in this packet.

## Rust semantic map and lifecycle

| TS phase/value | Rust producer → owner/consumer | Lifetime, identity, invalidation |
| --- | --- | --- |
| current range and modifiers | parser/transform factory → TransformArena → class_keyword_cursor | immutable current node/list; retained per-source identity; no updater |
| last modifier/decorator endpoint | arena NodeData/NodeArray → private helper | source point independent of array.end and node.end; MAX only synthetic |
| source byte versus UTF16 | source positions → SourceBytePosition → TokenCursor/TextWriter | checked typed domains; no casts between source and generated coordinates |
| parse-tree token ownership | arena original chain → existing node_has_source_token_shape | same kind required; current point does not replace provenance |
| token/comment progress | emit_token_with_comments → TokenEmission/CommentResume | returned value discarded as upstream; no transfer to another owner |
| commentsDisabled / NoNestedComments | metadata flags → emit_transformed_node → immutable EmitContext | only child-facing extent; lexical restoration by scope exit; no new state |
| optional parsed/generated name | existing transformers/factory → emit_identifier_name_with_context | same IdentifierName hint, hooks and allocation; no new name or leading phase |
| member indentation/maps/scopes | existing emit_class tail → writer/member pipeline | order and receiver/binding lifetime unchanged; errors propagate |
| command/output/diagnostics | ProgramSession and MemoryOutputSink | artifacts before callbacks; complete ordered tuple unchanged except owned bytes/maps |

Eight current architecture invariant rows below remain unchanged premises. The
new class-specific phase is active-unqualified until after qualification. No
broader E-COMMENTS-G or E-COMMENT-SCOPE-H completeness is inherited from their
dated subsets. EA-GAP-FLAGS and EA-GAP-CAPTURE require no new producer; the patch
creates no syntax or binding. Broad map/output APIs and custom transforms remain
future-owned; the ordinary map/declaration controls qualify only this profile.
No host/sink/cancellation branch changes, so existing adjacent product controls
apply without inventing a new fault-injection route.

## Whole upstream owner / local-gap map

Every row is a whole pinned function read before design, with its callers and
phase relationship described above. Shared rows preserve the existing class
tail, names, parser and generic comment/token machinery. The exact Rust map,
step and test identifiers are in the readiness manifest.

| Whole TS function | `_tsc.js` span | SHA256 | Gap / step |
| --- | --- | --- | --- |
| `isParseTreeNode` | 11423–11425 | `d8a6d217a3087e6809bfb3df3a4815eefce954e8175aed4c744b515f891dbe8d` | shared-prerequisite; A6-33-1 |
| `getParseTreeNode` | 11426–11437 | `80b5c2449cb8320cf209184a8eef484f944379da161e54d74dd54ed1f0d2d592` | shared-prerequisite; A6-33-1 |
| `moveRangePos` | 17304–17306 | `11da3d6e63737439c2a5e1069044dc8474e21b2bf5a36d5c66e78ee47add4cba` | shared-prerequisite; A6-33-1 |
| `moveRangePastDecorators` | 17307–17310 | `27d3b9fba1576ed2d7269a9fe1b694ac1e16e977da92c9935f13359611222a93` | missing; A6-33-1 |
| `moveRangePastModifiers` | 17311–17317 | `9d43119a4e2ea51f3f5a151f00816f7985c1781c9dc80cfd8e44f40807d3db9d` | missing; A6-33-1 |
| `positionIsSynthesized` | 18811–18813 | `d7c8efa6a3407c62a96399f410fac2ae254372213b526c0c0c96811612cfb7b7` | shared-prerequisite; A6-33-1 |
| `getGeneratedNameForNode` | 21652–21666 | `7aeec7c8966a869665e0b8f01a41cd52e75bc957006170fd446ea819be9a6ea0` | shared-prerequisite; A6-33-1 |
| `getName` | 24788–24799 | `9734f5576b1aa153598ff7ae70a2a2f994bb50d0370fbfc547c47952f72dea33` | shared-prerequisite; A6-33-1 |
| `getLocalName` | 24803–24805 | `db85ef71236480d7de1d2e131b01d6f8fed272ef41d0f5297ce7fb3485ee7979` | shared-prerequisite; A6-33-1 |
| `parseClassDeclarationOrExpression` | 34244–34264 | `872261275a5ff0aa7d224638c4fcffe4071319832bbe900f0b3d1895fe728de1` | shared-prerequisite; A6-33-1 |
| `parseNameOfClassDeclarationOrExpression` | 34265–34267 | `2f2cea44721db1b8209b360d39ef633a8e98d3948684012062c4d127c62572c3` | shared-prerequisite; A6-33-1 |
| `emitIdentifierName` | 117149–117157 | `847193fac9ff770a8b062033c89f8676a42ac88e0adb16b22fdafb5a33c09ae4` | shared-prerequisite; A6-33-2 |
| `emitClassExpression` | 118532–118535 | `f94df8a64f3b42568611733971796c82066ac5c6d881e6cc7f1ed9eadbf89acb` | shared-prerequisite; A6-33-1 |
| `emitTokenWithComment` | 118731–118764 | `d7df39bba502705facedce379a636ae55b168b77701df5e72abf10ec444c2e50` | shared-prerequisite; A6-33-1 |
| `emitClassDeclaration` | 119059–119061 | `77f56050655c5c665c24f2eed672cf60d8ba8513416f9d34b19b1627bf1786ef` | shared-prerequisite; A6-33-1 |
| `emitClassDeclarationOrExpression` | 119062–119090 | `7f38f7abe1799deb70b1601157c52ddc1b4d9d44c83c871ecdee9dea01859c48` | partial-or-stale; A6-33-1 |
| `emitDecoratorsAndModifiers` | 119846–119902 | `8fb50c70cd68d557886307bc8242e2f79f0533fea4c44e498feb8ce7c0eba899` | shared-prerequisite; A6-33-1 |
| `emitList` | 120015–120025 | `8a0512c2af9ba16a7481b372c31ae88611a0f3f8b4daaf5919a7278927262b5c` | shared-prerequisite; A6-33-1 |
| `writeTokenText` | 120222–120226 | `ba09e58ada82fd0e2426c23037e1f0c4d854e36c0f7cc2e04700c32a71d273ae` | shared-prerequisite; A6-33-1 |
| `pushNameGenerationScope` | 120480–120492 | `75e640eff0f9e7b2d16c54e74bb57754277c93c13f82cd2954e427afaad2a1d0` | shared-prerequisite; A6-33-1 |
| `popNameGenerationScope` | 120493–120502 | `a2f730a921200a11f765aa55e960623d4e87c9c1d20b1d78fbb2d9ed6003bb37` | shared-prerequisite; A6-33-1 |
| `pipelineEmitWithComments` | 120978–120986 | `263af5299b06aaeca9c4e6397b6013e6b2c465afcab2709d8cbdd5ace688bd34` | shared-prerequisite; A6-33-2 |
| `emitCommentsBeforeNode` | 120987–120994 | `dc59a0901c0b5aaa640586703fb5f5a2420cf3f5ad344fd433501a20612154cf` | shared-prerequisite; A6-33-2 |
| `emitCommentsAfterNode` | 120995–121006 | `f0baac32a6d9fcf8f005ee8f1b923e1706f679e8a1fb88e2a7753064bb644a89` | shared-prerequisite; A6-33-2 |
| `emitLeadingCommentsOfNode` | 121007–121032 | `ce6bf342a94094cccc4bf56debcb99390c8e232705263609dfcf068589284ebb` | shared-prerequisite; A6-33-2 |
| `emitTrailingCommentsOfNode` | 121033–121046 | `e5c99d84eeab2c12d594ba56695a7a869c49720eb110f3715c0ea3f9271d1112` | shared-prerequisite; A6-33-2 |
| `emitLeadingCommentsOfPosition` | 121166–121175 | `fa23b688b1540c772ccf513c874d47bba4a08a44e019bc430feb79cbea73d2cd` | shared-prerequisite; A6-33-1 |
| `emitTrailingCommentsOfPosition` | 121191–121198 | `953cde198b7f8098bd7bc8d865e535cdac106e983efe35a4a067498ff239cbc0` | shared-prerequisite; A6-33-1 |
| `forEachLeadingCommentToEmit` | 121219–121233 | `2e1fb613c9b9bb29f94a866a92b31e77c392cd696ee93b2e61a001e4e3981a9c` | shared-prerequisite; A6-33-1 |
| `forEachTrailingCommentToEmit` | 121234–121238 | `bd6612ac9040b10e756e1b9666a34daebcc0fe66a227dfda01db103bfd3f27a7` | shared-prerequisite; A6-33-1 |

Eight after-export/after-default ordinary cases have two missing components:
keyword comments (owned here) and modifier trailing comments (outside, owned by
emitDecoratorsAndModifiers/emit_modifiers). Their latter gap is the same pending
modifier phase as four earlier getter controls. All eight remain in complete
comparison, must retain diagnostics/declarations, and must be recaptured after
the partial repair. They are not full-tuple passes, and their old first vectors
are not expected to remain byte-identical when an owned component changes.
The other 58 new failures and A32's six keyword-comment failures must fully
repair. All 22 new and 58 A32 prior positives must remain exact. Original
plainJSGrammarErrors remains outside with extra const/export prefixes; the other
three originals must stay exact. No source owner in these two implementation
steps remains unresolved or undispositioned.

## Mechanical sequence and acceptance

Run `python3 scripts/check-class-header-token-readiness.py` and freeze actual0
with the source/input preimage before editing production. Apply A6-33-1 then
A6-33-2, format, consume actual completion, and freeze final measurement inputs.
Run these serially with CARGO_BUILD_JOBS=2 and background priority:

```sh
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test contracts -- class_header_token_matches_complete_typescript_observations class_optional_name_matches_complete_typescript_observations --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-emitter --test class_header_token_metadata_contract -- --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test h2_8a_original_corpus -- original_class_optional_name_commands --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-emitter --test contracts -- --test-threads=1
```

Minimum complete exact twice: 80/88 new +64/64 prior =144/152 ordinary commands,
32/32 direct printer controls and 3/4 originals. The full emitter contracts must
pass unchanged. Repeat any ordinary failure job independently because its failed
tuples execute once per job; direct and original tests always attempt both runs.
Save complete unequal original/direct tuples and separately counted ordinary
supplemental captures; classify all residual components and fail closed on new
boundaries, regressions or unstable repetitions. Qualification requires all
owned repairs, all prior positives, identical repeated outcomes and no new
unclassified failure, not an expected-byte adjustment. Before/after inventories
stay separate. Single canonical writer; no source/input mutation during jobs.
Existing hosted acceptance precedes landing; historical walk/full developer CI
remain omitted and are not claimed. This closure does not close H2.8a–e.

Readiness: 30 whole TS owners, eight architecture premises, nine Rust map rows,
two steps, 88 new +64 prior +4 original commands and 32 direct metadata controls;
unresolved=0, undispositioned=0.

## Frozen architecture premises

| ID | Concern and invariant | Current Rust owners | Lifecycle / validation | Evidence or next owner |
| --- | --- | --- | --- | --- |
| `E-ARENA` | Parsed trees remain immutable; the detached arena appends synthetic nodes and tracks the mounted parsed interval. | `tsc_emitter::{TransformArena,TransformSource,TransformSourceId,TransformNode,TransformNodeArray,NodeFactory}` (public) Existing `NodeFactory::modifier_flags` is now `pub(crate)`; downlevel auto-accessor setters use fresh factory modifiers while getters retain source tokens. | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; every later transform packet maps provenance explicitly |
| `E-METADATA-BASE` | Transform flags and `emitNode`-equivalent facts are sparse session side tables; there is no standalone Rust `EmitNode` syntax object. Original/map/comment/value identities are separate. CommentRange carries independent source start/end states through CommentSourceRange; paired source slices and source-map ranges keep their existing domains. | Public `tsc_emitter::TransformArena::{transform_flags,set_transform_flags,array_transform_flags,set_array_transform_flags,metadata,metadata_mut,clear_session_metadata,get_original_node,set_original_node}` over private storage in `crates/emitter/src/factory.rs`; public `tsc_emitter::{EmitMetadata,SourceMapRange,CommentRange,CommentSourceRange,JavaScriptString}` defined in `crates/emitter/src/metadata.rs` (`EmitMetadata` storage fields remain `pub(crate)`, with its cross-crate operations exposed by public methods) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. A6-29 candidate uses existing `EmitMetadata::set_flags` to replace inherited flags on the ES5 accessor receiver clone, retaining its original/text/map ownership. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; H2.5h/H2.6 extend rather than infer from text |
| `E-NAMES-BASE` | Generated identity, printable name, target provenance, and allocation scope are separate; final names use the composed tree. | `crate::transform::GeneratedBindingId` (`pub(crate)`); `crate::builtins::generated_bindings::GeneratedBindingScopes` and `target_bindings::{TargetBinding,finalize_generated_binding_names}` (`pub(super)`) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and generated-binding controls |
| `E-PRINTER-BASE` | Printer has no direct checker dependency. It consumes transformed syntax/metadata, drives retained substitution and before/after notification hooks, and applies immutable structural planning. | `tsc_emitter::Printer` (public); `tsc_emitter::TransformationResult::{substitute_node,before_emit_node,after_emit_node}`; `crate::printer::EmissionPlan` and `EmitContext` (private); `crate::factory::NodeFactory::apply_parenthesizer_rules` (private) | `active-qualified`; qualified at validation ref `6acd5d43` (2026-08-21, CS-6 fixture/audit gate); previous ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. The H2.5h-a comment-scope packets reshape the private context carrier into the threaded `EmitContext` (CS-2 landed the root and core pipeline byte-identically); the row's hook-composition, no-checker-dependency, and immutable-planning invariants are preserved; requalified at `6acd5d43`. | H2.5g profile; later packets preserve hook composition and enumerate new expression contexts |
| `E-COMMENTS-G` | Parsed ownership, synthetic comments, relocated owners, token progress, and comment progress are distinct typed facts. `TokenEmission` may hand its cursor plus an optional `CommentResume` only to the immediately following token/child; the resume carries both owner start and next position, requires one source and monotone progress, and can merge only with the same owner. A retained arrow anchors trailing comments at its own comment-range end, not semantic-original provenance, so a synthetic token cannot borrow source comments. The simple parameter's list-owned trailing comments and the retained arrow token's trailing comments remain separate phases, including tsc's observable list/leading replay at a shared range start. `SourceLeadingCommentPhaseVisit` distinguishes an absent/suppressed phase from a visited exact range, so a transitional class-field comment anchor is skipped only after that same range was visited; contextless routes retain the anchor. Token/comment ownership performs no source-wide token search; the position-cursor contract keeps local source work linear. H2.5g qualifies only the direct expression and list routes enumerated by its source-comment topology contracts; it does not claim that tsc's enclosing comment-container state already reaches every nested node route. Comment ownership now distinguishes paired, synthesized, start-only and end-only positions; positional readers and ordinary node-phase extent checks remain separate. | Public `tsc_emitter::SyntheticComment`; `crate::metadata::RelocatedStatementListComments` (`pub(crate)`); `crate::comment_cursor::{CommentCursor,CommentResume,CommentResumeError}` and `crate::token_cursor::{TokenCursor,TokenEmission,TokenAnchor,TokenCommentBoundary,FixedToken,TokenWriteKind,TokenLeadingSpace}` (`pub(crate)`); private `crate::printer::SourceLeadingCommentPhaseVisit` and other comment/list workers | `active-qualified`; qualified at validation ref `6acd5d43` (2026-08-21, CS-6 fixture/audit gate); previous ref `0653e10d` (2026-08-17); candidate-only audit 2026-08-14. The H2.5h-a comment-scope packet CS-3 re-expresses the row's qualified expression/list comment projections on the per-side threaded scope (cursor/resume semantics and the token machinery unchanged); requalified at `6acd5d43`; A6-28-5 retires the obsolete initializer trailing-comment marker; [the shared packet](h2-8a-class-field-initializer-comments.md) qualifies all64 new controls and8 prior repairs in434/556 exact complete commands twice, with all490 units/451 contracts passing; subsequent [A6-28-6](h2-8a-promoted-class-export-maps.md) qualifies all100 selected map repairs plus six ES2022 cases in764/868 exact complete commands twice and all490/451 emitter tests, leaving four original A28 comment targets at that prerequisite; subsequent A6-28-7 qualifies all four; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. | Current-code audit, token/comment cursor unit contracts, `position_cursor_2727_statement_work_is_linear_and_scan_free`, source-comment topology and relocated class-field contracts, and arrow replay/resume integration contracts; H2.5g inventory/profile for observable parity |
| `E-COMMENT-SCOPE-H` | tsc scopes three independent values across nested emit routes: `containerPos`, `containerEnd`, and `declarationListContainerEnd`. The threaded immutable representation mandated by this row is LANDED end to end through the H2.5h-a comment-scope packets (CS-2..CS-6): the printer's ambient comment state is the immutable `CommentEmissionScope` triple threaded through an explicit `EmitContext` with `EmitContext::file_root` as the single zero-scope constructor, tsc's per-side flag/JsxText claim predicates on every route, the declaration-list writer (`claim_declaration_list_sides`), and the `NO_NESTED_COMMENTS` suppressed extent. The 30-case witness-driven fixture gate (byte parity against the frozen artifact, both `removeComments` polarities, all six transforms) and the permanent emitter-scoped zero-contextless workspace audit enforce it; qualified at `6acd5d43`. ES2015/Generators production (H2.5h-b) may now proceed. A single optional range or mutable ad-hoc stack is not an equivalent semantic model. Present comment endpoints claim their own sides independently; missing sides and sole-zero positions do not claim a container, and suppression flags cannot create an absent endpoint. | Landed private `crate::comment_cursor::CommentEmissionScope` and `crate::printer::EmitContext` (with `crate::printer::ExpressionSyntaxContext` as its syntax half); the 30-case artifact-driven fixture suite `crates/emitter/tests/integration/comment_scope_witness_contract.rs` | `active-qualified`; CS-2 landed 2026-08-18 ([CS-2 packet](h2-5h-a-cs-2.md)); CS-3 landed 2026-08-19 ([CS-3 packet](h2-5h-a-cs-3.md)); CS-4 landed 2026-08-20 ([CS-4 packet](h2-5h-a-cs-4.md)); CS-5 landed 2026-08-20 ([CS-5 packet](h2-5h-a-cs-5.md)); CS-6 landed 2026-08-21 ([CS-6 packet](h2-5h-a-cs-6.md)); qualified at `6acd5d43`; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. | Pinned tsc scope/save-restore graph; direct/list/wrapper/declaration-list oracle fixtures; contextless-call zero-use audit; focused, emitter, owner-control, and inventory gates |
| `E-POSITIONS` | Source bytes, source/generated UTF-16, synthetic ranges, and source switches remain typed domains. | Public `tsc_emitter` position/writer/hook types; definitions in private `position`, `writer`, `metadata`, and `printer` modules | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H1/H2.5f protect predecessor behavior. | H2.5g profile and H1 Unicode/newline controls |
| `E-OUTPUT-SCRIPT` | JavaScript artifacts are constructed before the first sink callback; callback order and `emittedFiles` stay independent. | `tsc_emitter::{emit_files_with_activity,EmitArtifact,MemoryOutputSink,FsOutputSink,EmitOutcome}` (public) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and H1 sink-failure controls |
