# H2.8a A6-38: object property output owners

Kind: runtime. Base `d1c04df5c7c0d93b26f76539c9101bc2f4b11c69`; trusted train
base `10748f6ee19ec083ce5748224930c5dcfbbd86df`. This closes the object-property
printer cause while the retained-accessor work and all other H2.8a–e requirements
remain open. It changes no accepted profile or hosted entrypoint.

The current 48-case native before contains 16 exact and 32 failed complete
commands, each executed twice. The 96 separately captured executions also repeat
identically. All failures differ only in JavaScript writes and returned source
maps. Declaration output, diagnostics (36 TS1042 and four TS2695 reports), write
ordering and refusal outcomes remain equal. TypeScript produced the 48 frozen
expectations through 96 real commands. Four noEmitOnError controls are included.

The four original grammar commands reuse the A37 global observations after
verifying 390 unchanged runtime/oracle/input/library sources. Three were exact
twice. plainJSGrammarErrors differed only by seven JavaScript bytes:
`export cantExportProperties: 4` instead of `cantExportProperties: 4`. Both complete
failure captures are preserved, with zero new original-before executions.
The global checkpoint remains 755/769 until another full replay is measured.

## Executable edit and boundary

A6-38-1 removes the initial `emit_modifiers` call and its conditional separating
space from the PropertyAssignment and ShorthandPropertyAssignment arms of
Printer::emit_transformed_node_worker in crates/emitter/src/printer.rs. Add the
pinned whole-owner ledger beside each arm. The readiness manifest stores both
exact source replacements and the resulting source hash; no other printer byte
may change. TS emitPropertyAssignment and emitShorthandPropertyAssignment emit
no modifier list for any modifier kind. This is an unconditional node-owner
rule, not an ExportKeyword-specific predicate. The other emit_modifiers callers
retain their implementation and call order.

PropertyAssignment then emits its existing name, colon/space, initializer
comment processing and initializer in DISALLOWED_COMMA context. Shorthand emits
its existing name and optional assignment initializer, retaining equals spacing
and the same expression context. The existing outer node phase owns member
comments and maps. Comments belonging only to an ignored modifier disappear;
property/name/initializer comments retain their original owners. The frozen
comment controls distinguish those cases. No resume cursor is invented, moved,
or restored by this edit; all existing source scope and cursor operations remain.

There is no AST update, new node, flag propagation, recomputation, metadata
mutation, resolver lookup, binding allocation, lexical receiver frame, pass
routing or helper request. Parsed/current/synthetic identity remains unchanged.
The printer runs after the existing transforms and retains its normal hooks and
fallible name/initializer emission. It no longer evaluates a modifier-list lookup
that upstream never performs for these node kinds. API1 custom-transform shape
validation remains outside this profile, with its existing typed boundary.

Allowed production file: crates/emitter/src/printer.rs, exactly A6-38-1 above.
Allowed evidence: this packet, observer/fixture/test registration, readiness and
frozen records, architecture row, H2.8/index progress and their authority pins.
Forbidden: parser/checker/factory/transform changes, output substitution,
hand-authored expectations, fixture/path predicates in production, relaxed
comparators, fallback success, accepted profile or global membership changes.
Any new required owner or file requires an amended ready packet before editing.

## Semantic and current-gap map

| Row | Current producer / owner / consumer | Gap, lifetime and completion |
| --- | --- | --- |
| ordinary property modifier prefix | transformed PropertyAssignmentData.modifiers -> Printer::emit_transformed_node_worker -> emit_modifiers | obsolete; omit this consumer; A6-38-1/32 failed fresh controls and original grammar case |
| shorthand modifier prefix | transformed ShorthandPropertyAssignmentData.modifiers -> same worker -> emit_modifiers | obsolete; omit this consumer; A6-38-1/shorthand and default controls |
| property/name identity | arena node -> emit_required_identifier_name_with_context -> existing name worker | already-exact; per-node immutable identity; keywords and exported-variable controls |
| optional initializer | typed Option<NodeId> -> existing shorthand arm -> expression worker | already-exact; no default versus present default, no cached state; A6-38-1/default controls |
| ordinary initializer comments | EmitMetadata/CommentRange -> emit_intervening_comments_before_node and existing leading/trailing workers | already-exact adjacent premise; nested immutable EmitContext and CommentResume lifetime retained; comment and removeComments controls |
| expression precedence | ExpressionSyntaxContext::DISALLOWED_COMMA -> existing child emitter/parenthesizer | already-exact; parentheses remain source-owned; comma default controls |
| outer comments and maps | existing node pipeline -> PropertyAssignment/ShorthandPropertyAssignment worker -> TextWriter | existing premise with redundant modifier child removed; JS/maps must exactly match after; no ranges rewritten |
| diagnostics and refusal | PreparedProgram -> ProgramSession command -> sink and result | already-exact; same callback-scoped borrow and command lifetime; complete tuples and noEmitOnError controls |
| flags and generated bindings | existing upstream transforms -> current arena -> printer read | premise unchanged; no producer/update/invalidation is added; EA-GAP-FLAGS and EA-GAP-CAPTURE remain future-owned by their existing slices |
| other declaration modifiers | existing declaration worker -> emit_modifiers | already-exact adjacent owner; 80 unchanged Const modifier erasure controls and valid export controls |

## Acceptance and resources

Readiness: `python3 scripts/check-object-property-owners-readiness.py --before`.
Normal check omits --before and permits only the exact planned source result.
A6-38-2 runs `cargo test -p tsc-rs-compiler --test contracts --
object_property_owners_match_complete_typescript_observations --nocapture
--test-threads=1` with TSC_RS_OBJECT_PROPERTY_CASE_SET=all. Require all 128
complete commands exact twice: 32 repairs,16 fresh positives and80 adjacent
positives. Every captured complete command must repeat identically and match.
Then run `cargo test -p tsc-rs-compiler --test h2_8a_original_corpus --
original_class_optional_name_commands --nocapture --test-threads=1`; require
all four original commands exact twice, including the repaired original failure.
Run unchanged emitter unit and contract suites: `cargo test -p tsc-rs-emitter
--lib --test contracts -- --test-threads=1`, expecting 494 units/451 contracts.
The 1,350 frozen declaration reprints remain within the existing contract suite.

Every heavy job uses taskpolicy background, nice 15, CARGO_BUILD_JOBS=2, and one
heavy process at a time. Persist prelaunch source/input hashes, actual exit/log,
all complete captures and binary receipt before another build. Unknown/missing
case, extra attempt, changed repeated tuple or mismatch fails qualification.
No broad global/hosted success is inferred. Existing hosted acceptance remains
required before landing; historical full developer CI/certificate walks are
omitted under the user's lightweight workflow. Single writer: root integrator.
Unresolved semantic/ownership/oracle rows: 0. Undispositioned reachable rows: 0.

## Pinned upstream and architecture references

The nine whole owners below pin the modified output owners and their unchanged
pipeline/comment/precedence integration. The 231 reachable syntax predicates
and 216 call sites are recorded in the manifest; unchanged pipeline branches
retain A37's qualified source and native boundary. All rows map to A6-38-1 and
its complete witness test; A6-38-2 freezes the after profile. For two obsolete
native calls there is no corresponding upstream call to inherit.

| Owner | Whole span | SHA-256 | Native owner |
| --- | --- | --- | --- |
| `getEmitFlags` | 13054–13057 | `6d8c5eece414332d11f366f7ae016d6d3253477e31519ee07e143b4263b35933` | `Printer::expression_comment_phase_owner_for_node` |
| `getCommentRange` | 25358–25361 | `84e06c1d1498906aa5765dfc0bdfd9dd4ca9d1c75367b90067f24dbc57936cd2` | `Printer::comment_range_for_node` |
| `emit` | 117145–117148 | `f998a75ec5c7ebceb127e49ec7315b9e3b9aa90c15e4a5bd7d3b57cd324f5682` | `Printer::emit_node_id_with_context_and_source_comments` |
| `emitExpression` | 117158–117161 | `71793715793bfcd4946613824a562cee711318989a9b02e24d6cc8c2a7be3146` | `Printer::emit_node_with_hint_and_source_comments` |
| `pipelineEmitWithHintWorker` | 117236–117704 | `485ac452835fedcb20a856f43e6eca9b8cd6b08f424fe054dccb3a176e673371` | `Printer::emit_transformed_node_worker` |
| `emitTrailingCommentsOfPosition` | 121191–121198 | `953cde198b7f8098bd7bc8d865e535cdac106e983efe35a4a067498ff239cbc0` | `emit_source_trailing_comments_of_position_with_filter` |
| `emitPropertyAssignment` | 119516–119526 | `4b204f060207e8c2c09624fd24bee6c424610f5a3140824a700c981796dfc399` | `Printer::emit_transformed_node_worker::PropertyAssignment` |
| `emitShorthandPropertyAssignment` | 119527–119535 | `510c6242520c5f81b69c705335e5bd3384e170459911c222b0c710fb1c3b7b9c` | `Printer::emit_transformed_node_worker::ShorthandPropertyAssignment` |
| `parenthesizeExpressionForDisallowedComma` | 20483–20488 | `0a7087ac86e0e05adcb2a09a875ee73ad9e77636dc29e663dd7d03ea3e38e786` | `ExpressionSyntaxContext::DISALLOWED_COMMA` |

Architecture rows are revalidated at the current source hashes. The object
property row is active-qualified for the focused A6-38 profile after all required
complete-command and emitter checks. Other rows retain their existing scopes.

| `E-PROTOCOL` | Read host, semantic resolver, artifact, sink, and outcome have separate ownership; planning cannot observe syntax, checked syntax exists only within the live checker/resolver scope, and sink errors become diagnostics at the write boundary. | `tsc_emitter::{EmitHost,EmitResolver,EmitArtifact,OutputSink,EmitOutcome}` (public); private `tsc_compiler::{PreparedEmitHost,CheckedEmitHost}`; `tsc_checker::emit::CheckerSession` implementation in `crates/checker/src/emit.rs` | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile; every output slice preserves this boundary |
| `E-ARENA` | Parsed trees remain immutable; the detached arena appends synthetic nodes and tracks the mounted parsed interval. | `tsc_emitter::{TransformArena,TransformSource,TransformSourceId,TransformNode,TransformNodeArray,NodeFactory}` (public) Existing `NodeFactory::modifier_flags` is now `pub(crate)`; downlevel auto-accessor setters use fresh factory modifiers while getters retain source tokens. | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; every later transform packet maps provenance explicitly |
| `E-METADATA-BASE` | Transform flags and `emitNode`-equivalent facts are sparse session side tables; there is no standalone Rust `EmitNode` syntax object. Original/map/comment/value identities are separate. CommentRange carries independent source start/end states through CommentSourceRange; paired source slices and source-map ranges keep their existing domains. | Public `tsc_emitter::TransformArena::{transform_flags,set_transform_flags,array_transform_flags,set_array_transform_flags,metadata,metadata_mut,clear_session_metadata,get_original_node,set_original_node}` over private storage in `crates/emitter/src/factory.rs`; public `tsc_emitter::{EmitMetadata,SourceMapRange,CommentRange,CommentSourceRange,JavaScriptString}` defined in `crates/emitter/src/metadata.rs` (`EmitMetadata` storage fields remain `pub(crate)`, with its cross-crate operations exposed by public methods) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. A6-29 candidate uses existing `EmitMetadata::set_flags` to replace inherited flags on the ES5 accessor receiver clone, retaining its original/text/map ownership. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; H2.5h/H2.6 extend rather than infer from text |
| `E-PRINTER-BASE` | Printer has no direct checker dependency. It consumes transformed syntax/metadata, drives retained substitution and before/after notification hooks, and applies immutable structural planning. | `tsc_emitter::Printer` (public); `tsc_emitter::TransformationResult::{substitute_node,before_emit_node,after_emit_node}`; `crate::printer::EmissionPlan` and `EmitContext` (private); `crate::factory::NodeFactory::apply_parenthesizer_rules` (private) | `active-qualified`; qualified at validation ref `6acd5d43` (2026-08-21, CS-6 fixture/audit gate); previous ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. The H2.5h-a comment-scope packets reshape the private context carrier into the threaded `EmitContext` (CS-2 landed the root and core pipeline byte-identically); the row's hook-composition, no-checker-dependency, and immutable-planning invariants are preserved; requalified at `6acd5d43`. | H2.5g profile; later packets preserve hook composition and enumerate new expression contexts |
| `E-COMMENT-SCOPE-H` | tsc scopes three independent values across nested emit routes: `containerPos`, `containerEnd`, and `declarationListContainerEnd`. The threaded immutable representation mandated by this row is LANDED end to end through the H2.5h-a comment-scope packets (CS-2..CS-6): the printer's ambient comment state is the immutable `CommentEmissionScope` triple threaded through an explicit `EmitContext` with `EmitContext::file_root` as the single zero-scope constructor, tsc's per-side flag/JsxText claim predicates on every route, the declaration-list writer (`claim_declaration_list_sides`), and the `NO_NESTED_COMMENTS` suppressed extent. The 30-case witness-driven fixture gate (byte parity against the frozen artifact, both `removeComments` polarities, all six transforms) and the permanent emitter-scoped zero-contextless workspace audit enforce it; qualified at `6acd5d43`. ES2015/Generators production (H2.5h-b) may now proceed. A single optional range or mutable ad-hoc stack is not an equivalent semantic model. Present comment endpoints claim their own sides independently; missing sides and sole-zero positions do not claim a container, and suppression flags cannot create an absent endpoint. | Landed private `crate::comment_cursor::CommentEmissionScope` and `crate::printer::EmitContext` (with `crate::printer::ExpressionSyntaxContext` as its syntax half); the 30-case artifact-driven fixture suite `crates/emitter/tests/integration/comment_scope_witness_contract.rs` | `active-qualified`; CS-2 landed 2026-08-18 ([CS-2 packet](h2-5h-a-cs-2.md)); CS-3 landed 2026-08-19 ([CS-3 packet](h2-5h-a-cs-3.md)); CS-4 landed 2026-08-20 ([CS-4 packet](h2-5h-a-cs-4.md)); CS-5 landed 2026-08-20 ([CS-5 packet](h2-5h-a-cs-5.md)); CS-6 landed 2026-08-21 ([CS-6 packet](h2-5h-a-cs-6.md)); qualified at `6acd5d43`; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. | Pinned tsc scope/save-restore graph; direct/list/wrapper/declaration-list oracle fixtures; contextless-call zero-use audit; focused, emitter, owner-control, and inventory gates |
| `E-COMMENT-PHASES-A36` | Ordinary modifier node comments precede list spacing; class keyword positional leading comments do not replay the preceding modifier trailing phase; spread fixed tokens hand actual comment progress to their expression. Source-file statements establish their comment range scope before their worker so a child modifier cannot replay the statement prefix. The existing immutable three-sided scope and metadata flags retain their ownership. | Private `crate::printer::{PositionCommentPhase,Printer::{write_transformed_source_file,emit_modifiers,emit_token_with_source_leading_comments,emit_comments_at_cursor_with_phase,emit_spread_expression}}`; existing `DeferredExpressionSourceComments`, `EmitContext`, `TokenEmission`, `CommentResume` | `active-qualified` for the A6-36 focused printer profile (2026-09-10). General ordinary-node phase migration, ESNext ellipsis admission and retained-accessor/module-cursor owners remain open. | [A6-36](h2-8a-token-comment-phases.md):270/293 complete commands exact twice in each of two independent jobs (55 repairs/215 prior positives/23 outside);96 direct metadata controls and3 original commands exact twice;494 emitter units/451 contracts and32 prior direct controls pass. |
| `E-COMMENT-ELLIPSIS-A37` | Ordinary rest token/name/type children consume complete source-comment phases with explicit hints; JSX token resumes and inherited suppression preserve brace maps. The raw ellipsis source-text admission guard is retired after complete focused command parity. | Private `crate::printer::{Printer::emit_optional_ordinary_child,Printer::emit_source_leading_token_with_context,Printer::emit_token_with_comments_at_boundary,PositionCommentPhase}`; existing `EmitContext`, `DeferredExpressionSourceComments`, `TokenEmission`, `CommentResume`; `builtins::preflight_source` | `active-qualified` for the A6-37 focused printer/admission profile (2026-09-10). General node-phase migration and the separate map/accessor/module-cursor owners remain open. | [A6-37](h2-8a-ellipsis-comment-owners.md):390/398 complete commands exact twice in each of two independent jobs (97 repairs/293 prior positives/8 unchanged outside failures);144 new metadata cases and96/32 prior direct controls exact twice;494 emitter units/451 contracts with1350 frozen declaration reprints pass. |
| `E-OBJECT-PROPERTY-A38` | Object property workers omit declaration modifiers. Ordinary and shorthand properties retain their existing source name, initializer, comment and map owners. | Private `crate::printer::Printer::emit_transformed_node_worker` PropertyAssignment and ShorthandPropertyAssignment arms | `active-qualified` for the A6-38 focused object-property profile (2026-09-10); all other H2.8a-e owners and global qualification remain open | [A6-38](h2-8a-object-property-owners.md):128/128 complete commands exact twice (32 repairs/96 preserved);4/4 original grammar commands exact twice (1 repair/3 preserved);494 units/451 contracts and 1350 declaration reprints pass. |


## A6-38 qualification

The [frozen after result](../../../../ratchets/h2-8a-object-property-owners-after.v1.json) records all 128 complete
commands exact twice in one focused job: all 48 fresh controls and 80 unchanged
adjacent controls. All 32 fresh failures are repaired; all 96 prior positives are
preserved. The 256 primary commands and 256 supplemental complete commands are
accounted for separately. Every captured tuple repeats identically and matches
the frozen TypeScript expectation, including diagnostic, write and source-map
fields. No expected output, comparator or accepted profile changed.

All four original grammar commands are exact twice. The original
plainJSGrammarErrors failure is repaired and three previous positives remain
exact. The unchanged emitter suites pass 494 units and 451 contracts, including
all 1350 frozen declaration reprints. Source/input archives, actual exits and
logs are retained, with binary hashes frozen before subsequent builds.

The implementation removes only the two obsolete modifier-emission calls and
their separating spaces, with a whole-owner source ledger beside each arm.
This closes A6-38-1 and A6-38-2. The last full original replay remains A37 at
755/769; this focused repair does not establish a new global total. Retained
accessor/decorator producers and every remaining H2.8a-e requirement stay open.
