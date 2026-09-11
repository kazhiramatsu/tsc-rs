# H2.8a A6-32: optional class declaration names

The optional-name branch is qualified on the H2.8 train. The
[frozen after result](../../../../ratchets/h2-8a-class-optional-name-after.v1.json) has 58/64 fresh complete commands
exact twice: all 18 selected repairs and all 40 prior positives.
The independently repeated remaining 6 cases preserve the explicit
class-keyword comment owner. The original four retain 3 complete exact cases;
its remaining boundary is recorded separately below. All 38 existing printer
contracts pass with unchanged test sources and IDs. There are
252 primary attempts and 122
separately counted supplemental attempts across the fresh and original jobs.

Only `Printer::emit_class` and its two call sites change: missing names are
accepted by the printer, and the obsolete expression/default distinction is
removed. The TS1211 diagnostic and noEmitOnError write suppression remain exact.
Class keyword comments and the original's next recovery boundary remain open;
this qualification does not claim those failed full tuples or infer a global
count. The last full A29 checkpoint remains 733/769. H2.8a–e are open.

The original plainJSGrammarErrors command now produces complete actual tuples twice. All fields except JavaScript callback/materialized bytes and their lengths agree. The remaining text contains two extra `const` modifiers on class members and one extra `export` modifier on an object property. TypeScript modifier erasure and the property-assignment printer own these follow-ups; they are separately bounded work, and this original remains a failed complete comparison.

The frozen pre-edit scope and readiness evidence follow. Hosted acceptance is
required before landing; historical certificate walk/full developer CI omitted.


Kind: runtime. Base `833b50bfe3f3c80289341a771d3dae7d557c4559`; trusted train base
`10748f6ee19ec083ce5748224930c5dcfbbd86df`. H2.8a–e remain open. This packet
owns the printer's absent-name branch. Parser recovery and diagnostic policy
already retain TypeScript's optional name. A31 is qualified60/60 and committed;
the full A29 checkpoint remains733/769. This packet makes no new global count.

Before production: TS64 complete observations twice plus independent repeat
(256 Programs) are identical. Native before has40 exact twice and24 runtime
failures, independently repeated with identical first vectors; 208 primary
attempts and104 separately counted supplemental attempts. Original four commands
have three exact and one runtime failure, each twice. The archive and receipts
are in the frozen before records. No later field of a failed tuple is qualified.

## Exact source boundary and lifecycle

`parseClassDeclarationOrExpression` obtains the name from
`parseNameOfClassDeclarationOrExpression`, whose absent binding-identifier arm
is undefined. Rust parser.rs uses Option<NodeId> on both ClassDeclarationData and
ClassExpressionData. `emitClassDeclaration` delegates directly; `emitClassExpression`
first performs generateNameIfNeeded. `emitClassDeclarationOrExpression` emits
modifiers, class keyword, optional name, indentation, types/heritage, opening brace,
member name scope and member list, closing brace, then restores indentation.
Its absent-name branch has no failure, modifier test, or invented identifier.

The private native `Printer::emit_class` currently rejects name=None unless
expression=true or the modifier list includes DefaultKeyword. A6-32-1 removes
that rejection and its special default-modifier scan, leaving the existing
if-let Some(name) name/comment branch intact. A6-32-2 removes the now-unused
expression boolean and its false/true arguments at the two NodeData dispatch
arms. The existing whole-function ledger for emitClassDeclarationOrExpression
and emitClassDeclaration remains unchanged. No other runtime edit is allowed.

Allowed production: `crates/emitter/src/printer.rs`, those three sites only.
Allowed evidence: new observer/fixture/integration/selector, readiness checker,
packet/index/progress and versioned records. No expected-byte edits, textual
output patch, fallback node, parser/checker/transform/factory changes, public API
or state changes. Inputs are ordinary ProgramSession commands. `noEmitOnError`
controls retain TS1211 and suppress writes; ES5 controls pass through the existing
lowering path; typed named/default controls also protect declaration/map output.

| TS value / phase | Current Rust producer, owner and consumer | Lifetime, identity and invalidation |
| --- | --- | --- |
| optional name | parser.rs parse_name_of_class_declaration_or_expression → ClassDeclarationData/ClassExpressionData.name → printer emit_class | existing arena NodeId or None; no conversion or new identity |
| current/parsed node | TransformArena metadata and get_original_node → common printer, EmitContext | one transformation; immutable source with session side tables; unchanged |
| transform flags | existing creators/transformers → current arena flags | no node is created/updated; no new full recomputation point |
| comments and source resume | existing name emission and original_node_end_cursor → emit_comments_at_cursor | lexical comment scope and cursor remain unchanged; no fabricated missing-name position |
| map locations | existing print pipeline and writer typed UTF16 positions | printing resumes through existing member pipeline; raw result/map bytes compared |
| generated names and receivers | composed-transform generated binding finalization; target/source identities | no name generation at absent printer name, no receiver or lexical scope mutation |
| member list and indentation | emit_class member loop / TextWriter, EmitContext | existing order, no new scope or cache; errors propagate through existing Result |
| output/diagnostics | ProgramSession command and MemoryOutputSink | ordered complete tuple, no write before artifact construction; checker diagnostics unchanged |

The eight architecture rows below are revalidated invariant premises, copied
with their dated qualification; the new optional-name branch is active-unqualified
until its focused after record. Existing hook order, no checker dependency,
immutable structural planning, comment endpoints and binding lifetimes are
preserved. EA-GAP-FLAGS and EA-GAP-CAPTURE require no new producer because this
patch creates no syntax or semantic state. Dormant broad map/output rows are
not compatibility premises; executable current map/command controls qualify
only this profile. General APIs and absent host capabilities remain H2.8b–e.

## Reachable owners and steps

| Whole pinned TS function | `_tsc.js` span | SHA256 | Gap / step |
| --- | --- | --- | --- |
| `moveRangePastDecorators` | 17307–17310 | `27d3b9fba1576ed2d7269a9fe1b694ac1e16e977da92c9935f13359611222a93` | shared-prerequisite; A6-32-1 |
| `moveRangePastModifiers` | 17311–17317 | `9d43119a4e2ea51f3f5a151f00816f7985c1781c9dc80cfd8e44f40807d3db9d` | shared-prerequisite; A6-32-1 |
| `parseClassDeclarationOrExpression` | 34244–34264 | `872261275a5ff0aa7d224638c4fcffe4071319832bbe900f0b3d1895fe728de1` | shared-prerequisite; A6-32-1 |
| `parseNameOfClassDeclarationOrExpression` | 34265–34267 | `2f2cea44721db1b8209b360d39ef633a8e98d3948684012062c4d127c62572c3` | shared-prerequisite; A6-32-1 |
| `emitIdentifierName` | 117149–117157 | `847193fac9ff770a8b062033c89f8676a42ac88e0adb16b22fdafb5a33c09ae4` | shared-prerequisite; A6-32-1 |
| `emitClassExpression` | 118532–118535 | `f94df8a64f3b42568611733971796c82066ac5c6d881e6cc7f1ed9eadbf89acb` | shared-prerequisite; A6-32-2 |
| `emitTokenWithComment` | 118731–118764 | `d7df39bba502705facedce379a636ae55b168b77701df5e72abf10ec444c2e50` | shared-prerequisite; A6-32-1 |
| `emitClassDeclaration` | 119059–119061 | `77f56050655c5c665c24f2eed672cf60d8ba8513416f9d34b19b1627bf1786ef` | shared-prerequisite; A6-32-2 |
| `emitClassDeclarationOrExpression` | 119062–119090 | `7f38f7abe1799deb70b1601157c52ddc1b4d9d44c83c871ecdee9dea01859c48` | partial-or-stale; A6-32-1 |
| `emitDecoratorsAndModifiers` | 119846–119902 | `8fb50c70cd68d557886307bc8242e2f79f0533fea4c44e498feb8ce7c0eba899` | shared-prerequisite; A6-32-1 |
| `emitList` | 120015–120025 | `8a0512c2af9ba16a7481b372c31ae88611a0f3f8b4daaf5919a7278927262b5c` | shared-prerequisite; A6-32-1 |
| `pushNameGenerationScope` | 120480–120492 | `75e640eff0f9e7b2d16c54e74bb57754277c93c13f82cd2954e427afaad2a1d0` | shared-prerequisite; A6-32-1 |
| `popNameGenerationScope` | 120493–120502 | `a2f730a921200a11f765aa55e960623d4e87c9c1d20b1d78fbb2d9ed6003bb37` | shared-prerequisite; A6-32-1 |

`emitIdentifierName` retains the IdentifierName hint and its existing hook pipeline;
`emitDecoratorsAndModifiers`, `emitList`, push/popNameGenerationScope retain their
current native owners unchanged. Full source bodies were read and AST-span hashes
are mechanically checked. The separate moveRange/token functions explain the
adjacent comment guard; they do not authorize edits to those native owners here.

Six anonymous-keyword-comment windows are outside this optional-name closure:
removing the refusal exposes the class keyword's existing raw writer path.
TypeScript `emitTokenWithComment(moveRangePastModifiers(node).pos)` owns that
comment. Next owner A6-33 must preserve current versus original positions,
last actual modifier/decorator and immediate child resume. It must not reuse
array-end or semantic-original cursor behavior without proof. The six exact TS
negative witnesses are frozen; their expected output is not weakened. Original
plainJSGrammarErrors has many independent recovery errors; after this guard,
its next boundary must be captured and attributed before any further edit.
Original cases2–4 must stay complete exact. These are known outside behavior,
not permission to claim a failed full tuple. No unresolved item exists in the
three-site removal; all remaining semantics have explicit owners and guards.

## Verification

Before edit: `python3 scripts/check-class-optional-name-readiness.py`, then
freeze its actual0 result and pre-edit source/input bytes. Runtime before is
immutable. Run the64 complete comparisons after edit, with minimum58 exact twice
(all18 owned plus40 prior), retaining the six comment cases in the inventory.
Run original4 and all existing printer contract modules using:

```sh
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test contracts -- class_optional_name --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test h2_8a_original_corpus -- original_class_optional_name_commands --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test -p tsc-rs-emitter --test contracts -- printer_ --test-threads=1
```

Failed fresh tuples execute once per job; independently repeat any failed after
vectors before freezing determinism. Passing tuples execute twice per job. The
original reader always attempts both repetitions. Supplemental artifact captures
are separately counted and never substitute for the full tuple. Preserve actual
exit codes and runtime binary/source identities. Existing hosted acceptance is
required before landing; historical certificate walk/full developer CI omitted.

Readiness:13 whole functions,8 invariant architecture rows,8 Rust map rows,
2 steps,64 fresh+4 original witnesses; unresolved=0,undispositioned=0.

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
