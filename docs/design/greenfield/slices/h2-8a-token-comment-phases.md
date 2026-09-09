# H2.8a ordinary modifier and spread comment phases

Status: focused printer prerequisite qualified on the H2.8 train. Runtime packet A6-36, base
`b5f2c8e88785a1fdc4b012538494a8a77365c54c`, on the existing H2.8 train.
All H2.8a–e remain required. This packet restores the ordinary modifier node
comments phase, separates the class keyword's positional leading phase,
and sends spread expressions through their fixed-token comments phase.

The shared typed comment scope, source positions, original-node projection,
token handoff, parenthesization, source-map recording, notification and
substitution are existing prerequisites. Their types and lifetime invariants
remain unchanged. New call-site behavior belongs to the separate candidate
architecture row below; predecessor qualifications are not broadened.

## Before observations and required outcome

The [before record](../../../../ratchets/h2-8a-token-comment-phases-before.v1.json)
contains 293 complete ordinary Program commands: 129 fresh modifier/declaration/
spread cases, all 88 class-header controls, all 64 optional-class-name controls,
and all 12 static-modifier accessor controls (ES5/ES2015/ES2022, CJS/ESNext,
set/define). Each native job executes 215 positives twice and 78 failures once;
two independent jobs must agree on all 508 complete supplemental command
tuples per job. Primary and supplemental executions are separately counted:
1016 each. A failing primary comparison never implies its unvisited repetition
ran. TypeScript's new observer executes 129 cases twice (258 Programs).

Fifty-five failures belong to the selected printer owners. After implementation
these and all 215 positives must be exact, giving 270/293, in two independent
jobs. Twenty-three complete comparisons remain explicit adjacent negatives:

- Eighteen ESNext spread cases stop at the existing
  `builtins::has_advanced_comment_placement` preflight. Its raw
  `has_comment_after_ellipsis` also covers parameter/binding/type ellipses and
  strings/comments. Removing that whole guard requires those owners' fresh
  observations; this printer prerequisite alone is not sufficient. The next
  ellipsis admission packet must retire it, and H2.8a cannot close with it.
- Four ES2022 static-accessor cases also have the retained auto-accessor
  producer/map defect. Modifier text may improve while the complete tuple
  remains unequal. Record every such delta without counting it as exact.
- Recovery `export import` has a typed CommentResumeOwnerMismatch before
  output. Module keyword cursors and their preceding node ownership need a
  separate source audit. Keep the error and any after delta visible.

The separate direct printer observation has 96 cases (eight source shapes,
six comment-flag combinations, both removeComments settings). Before is
64 exact twice and 32 failed twice. All 96 must be exact afterwards, including
NoTrailingComments on the export modifier and NoNestedComments on the parent
class/spread. This denominator is never added to Program commands. The original
three ES2015 allowJs/JSDoc spread failures use the full unchanged output-matrix
comparison twice, including JavaScript, declarations and diagnostics. All three
must become exact. The frozen original inputs contain `.../** @type ... */(...)`.

## Source-owned implementation steps

A6-36-1: `Printer::emit_modifiers` must give each ordinary modifier the existing
`emit_node_id_with_context_and_source_comments` request with
`DeferredExpressionSourceComments::nested` and LeadingAndTrailing extent.
Capture the enclosing `CommentEmissionScope` before entering that modifier;
the modifier's own flags/range establish its nested scope, and its trailing
phase restores the enclosing end ownership. Consume the Complete outcome.
The source order is emitNodeListItems -> emit -> notification -> substitution
-> comments-before -> source maps/worker -> comments-after -> list spacing.
Do not append comments after writing the next modifier or keyword.
An inherited NoNestedComments extent suppresses this new request. A modifier's
own NoNestedComments still permits its own leading/trailing phases.

The 21 TS modifier-calling node owners below are reviewed in full. Native
function/accessor splitting adds two call sites, while native ClassStaticBlock,
PropertyAssignment, ShorthandPropertyAssignment, ExportAssignment and
NamespaceExportDeclaration have five extra modifier calls absent upstream.
Those are separate producer defects, not evidence of TS modifier ownership;
their removal stays in the next grouped local-producer work. This packet does
not certify those malformed/synthetic productions. Existing recovery export
and assignment commands are adjacent positives and must remain exact.
Decorator grouping, decorator admission and list formatting are unchanged;
the legacy-decorator commands execute their normal transform before printing.
Retained/new decorator APIs are not admitted by this packet.

The first candidate exposed 27 regressions in the unchanged 1,350-row
declaration printer reprint contract: leading JSDoc or library header comments
were repeated by the first modifier. The initial contracts job exited 101
(450 passing tests, one failing contract); it is retained as failed evidence,
not qualification. The earlier focused/direct/unit results describe that
initial source hash and must be rerun after the correction.

A6-36-1 also owns the statement scope producer in
`Printer::write_transformed_source_file`. Upstream `emitSourceFile` calls
`emitSourceFileWorker`, whose MultiLine `emitList` reaches each statement's
ordinary `pipelineEmitWithComments`. That statement claims its current
comment-range start/end before emitting its modifiers. Native source-file
leading/trailing phases were already explicit, but the intervening direct
worker received an empty file-root context. Derive its owner with
`expression_comment_phase_owner_for_node`, establish the existing
`active_expression_comment_scope` over `EmitContext::file_root()`, and pass
that scope into `emit_transformed_node`. Scope values expire after this
statement, preserving sibling restoration. Do not emit its leading comments
again or invoke notification/substitution twice. Explicit metadata ranges,
one-sided claims, suppression flags, detached comment resume, helper/prologue
ordering and statement map brackets keep their existing producers. No JSDoc
special case, original-range borrowing, visited set or expectation edit.

The four additional whole source-file/detached/prologue owners and their
18 predicates are audited below. Their selection, filtering, helper, name
scope and detached-list branches are unchanged prerequisites; only the
statement scope handoff joins the existing comments pipeline. Re-run all
451 contracts including all 1,350 frozen declaration reprints before the
remaining final-source observations. Their denominator stays separate from
the 293 Program commands and 96 direct metadata controls.

A6-36-2: `emitClassDeclarationOrExpression` calls emitDecoratorsAndModifiers
before `emitTokenWithComment(ClassKeyword, pos, ...)`. The latter requests
only `emitLeadingCommentsOfPosition(startPos)` before its token; the preceding
modifier already owns same-line trailing comments. Add private immutable
`PositionCommentPhase::{BoundaryUnion,SourceLeading}` and route the class
keyword through `emit_token_with_source_leading_comments`.
`emit_comments_at_cursor_with_phase` shares source/resume validation, indentation,
filtering and actual cursor advancement. SourceLeading emits only source-leading
ranges; it does not scan/emit trailing ranges and does not fabricate a consumed
resume. Preserve all other fixed-token wrappers and explicit child/name/semicolon
boundaries with their existing BoundaryUnion behavior. The class-name end
boundary remains unchanged. Complete migration of other positional callers
requires their ordinary preceding-node phases; it is not implied here.

The fixed token retains source-shape similarity, original source identity,
skipTrivia then fixed-spelling arithmetic, brace/conditional map routing,
token-end owner suppression and JSDoc filtering. No token-kind search, global
visited set, guessed byte span, new ambient state, or whitespace repair.
The token result carries only actually observed comment progress.

A6-36-3: both spread workers use `emitTokenWithComment(DotDotDotToken,node.pos)`
then `emitExpression(..., parenthesizeExpressionForDisallowedComma)`.
`Printer::emit_spread_expression` uses the transformed node's current start
cursor and the shared fixed-token producer, then passes its actual TokenEmission
to `emit_child_after_token_with_complete_source_comments`. Preserve the child's
substitution/parenthesization and enclosing end ownership. For inherited
NoNestedComments, emit the punctuation and the ordinary child without requesting
source comments. SpreadAssignment's missing-expression branch emits nothing
as upstream; valid ordinary syntax always has the expression. SpreadElement
retains the native typed missing-required-child error for malformed factory
inputs. No target/option/case-specific printer branch.

A6-36-4: run all complete focused/original/direct observations and the adjacent
emitter suites. Preserve the independent second focused run and all captured
outside differences. Qualified means all 55 intended repairs, all 215 prior
positives, all three original commands and all 96 direct controls pass, with
494 emitter units, 451 emitter contracts and the previous 32 direct class-header
controls passing. The 23 outside commands keep their unchanged TS comparisons;
the aggregate focused test is intentionally red until their owners close.
Neither its red exit nor a focused green is a global emitter result.

Only production path `crates/emitter/src/printer.rs` may change. No existing
test expectations, shared loader, option adapter, guard, accepted membership,
STAGE, profile, factory/metadata carrier, resolver or output sink change.
Registration, new observer/test, packet and evidence surfaces belong to the
integrator. Full historical CI/certificate walks remain omitted under the
current schedule; existing hosted acceptance is required before runtime landing.

## Rust ownership and local-gap map

| TS fact/transition | Rust producer, carrier and consumer | Current gap and verification |
| --- | --- | --- |
| ordinary modifier emit | emit_modifiers -> nested DeferredExpressionSourceComments -> emit_node_with_hint_and_source_comments | missing trailing request; step 1, modifier/declaration/decorator commands and flag controls |
| containerPos/containerEnd/declarationListContainerEnd | immutable CommentEmissionScope in EmitContext; active_expression_comment_scope and established_container_sides | unchanged per-side claims; inherited parent lives across each child and returns by value; prior class controls plus all emitter units/contracts |
| node flags and comment range | expression_comment_phase_owner_for_node -> existing EmitMetadata/CommentRange | unchanged independent original/range/flags identity, no copies; direct96 exercises per-side flags |
| keyword positional phase | emit_class -> PositionCommentPhase -> emit_comments_at_cursor_with_phase | partial/stale union; step 2, export/default/Unicode/class-name controls |
| token source shape and continuation | TransformArena::parse_tree_node -> node_has_source_token_shape -> TokenCursor/TokenEmission | unchanged same-source validation and arithmetic; typed failures retained; class/header/token cursor contracts |
| spread fixed punctuation | spread worker -> emit_spread_expression -> fixed token | missing token comments; step 3, ES2015/original spread commands and direct array/object controls |
| trailing-token/child-leading handoff | actual TokenEmission + CommentResume -> emit_child_after_token_with_complete_source_comments | shared prerequisite, no fabricated progress or source-wide scan; block/line/newline/parenthesis/list controls |
| comment filtering/writer/maps | emit_source_leading_comments_of_position, emit_source_trailing_comments_of_position_with_filter, emit_same_line_trailing_comments, existing TextWriter | unchanged filters, exact UTF-8/UTF-16 and complete map results |
| emit notification/substitution | emit_node_with_hint_and_source_comments, emit_substituted_node_with_comments, TransformationResult hooks | unchanged call order and after-notification cleanup on errors; emitter contract regression |
| artifact/diagnostic callbacks | existing ProgramSession, MemoryOutputSink and full comparator | unchanged callback construction/order, output refusal boundary and diagnostics; no new sink/host/cancellation branch |

All new phase choices are call-local values. No persistent cache is introduced;
invalidation and cleanup follow the existing call stack and session metadata
disposal. EA-GAP-FLAGS and EA-GAP-CAPTURE remain open: no new transform lattice,
receiver capture, synthetic node category or cross-source metadata inheritance.
Other worker dispatch cases in the full printer pipeline inventory are audited
prerequisites/outside owners, not new compatibility claims.

## Reproduction and acceptance

All heavy commands use `CARGO_BUILD_JOBS=2`, `taskpolicy -b nice -n 15`, one
test thread and one active heavy job. Save fresh input archives, manifests,
logs, actual process exits and complete captures. Consume an actual terminal
process result before editing its inputs; an observation timeout is not a stop.

```sh
node scripts/observe-token-comment-phases.mjs --check
node scripts/observe-token-comment-phase-printer-metadata.mjs --check
python3 scripts/check-token-comment-phases-readiness.py --before
cargo test -p tsc-rs-compiler --test contracts token_comment_phases_match_complete_typescript_observations -- --nocapture --test-threads=1
cargo test -p tsc-rs-compiler --test h2_8a_original_corpus original_spread_token_comments_match_complete_commands -- --nocapture --test-threads=1
cargo test -p tsc-rs-emitter --test token_comment_phase_metadata_contract -- --nocapture --test-threads=1
cargo test -p tsc-rs-emitter --test class_header_token_metadata_contract -- --nocapture --test-threads=1
cargo test -p tsc-rs-emitter --lib -- --test-threads=1
cargo test -p tsc-rs-emitter --test contracts -- --test-threads=1
```

Each complete test retains its oracle data without normalization or success
allowlists. Readiness validates 78 whole TS owners, 501 explicit branch
dispositions, 293 command witnesses and 96 separate direct witnesses, all
authority/predecessor hashes and the before evidence. Unresolved=0 and
undispositioned=0 apply to the named printer prerequisite only. H2.8a admission,
the ESNext ellipsis guard, retained-accessor producer, module cursors, remaining
ordinary node phases, and all H2.8b–e retain their required completion owners.

## Whole upstream and native owner inventory

All 78 whole functions were read before their applicable readiness edit. Exact body hashes and all
501 branch expressions/dispositions are retained in the machine manifest.

| TS owner | `_tsc.js` span | SHA256 | Native owner | Step |
| --- | --- | --- | --- | --- |
| `getOriginalNode` | 11400–11410 | `e6e639e966314faf444b9b68796893745ffb06eb0adcf1180d6935332d8797a3` | `TransformArena::get_original_node` | A6-36-1 |
| `isParseTreeNode` | 11423–11425 | `d8a6d217a3087e6809bfb3df3a4815eefce954e8175aed4c744b515f891dbe8d` | `TransformArena::parse_tree_node` | A6-36-1 |
| `getParseTreeNode` | 11426–11437 | `80b5c2449cb8320cf209184a8eef484f944379da161e54d74dd54ed1f0d2d592` | `Printer::node_has_source_token_shape` | A6-36-1 |
| `getEmitFlags` | 13054–13057 | `6d8c5eece414332d11f366f7ae016d6d3253477e31519ee07e143b4263b35933` | `Printer::expression_comment_phase_owner_for_node` | A6-36-1 |
| `moveRangePastDecorators` | 17307–17310 | `27d3b9fba1576ed2d7269a9fe1b694ac1e16e977da92c9935f13359611222a93` | `Printer::class_keyword_cursor` | A6-36-1 |
| `moveRangePastModifiers` | 17311–17317 | `9d43119a4e2ea51f3f5a151f00816f7985c1781c9dc80cfd8e44f40807d3db9d` | `Printer::class_keyword_cursor` | A6-36-1 |
| `positionIsSynthesized` | 18811–18813 | `d7c8efa6a3407c62a96399f410fac2ae254372213b526c0c0c96811612cfb7b7` | `SourceRange` | A6-36-1 |
| `getCommentRange` | 25358–25361 | `84e06c1d1498906aa5765dfc0bdfd9dd4ca9d1c75367b90067f24dbc57936cd2` | `Printer::comment_range_for_node` | A6-36-1 |
| `emit` | 117145–117148 | `f998a75ec5c7ebceb127e49ec7315b9e3b9aa90c15e4a5bd7d3b57cd324f5682` | `Printer::emit_node_id_with_context_and_source_comments` | A6-36-1 |
| `emitIdentifierName` | 117149–117157 | `847193fac9ff770a8b062033c89f8676a42ac88e0adb16b22fdafb5a33c09ae4` | `Printer::emit_identifier_name_with_context` | A6-36-1 |
| `emitExpression` | 117158–117161 | `71793715793bfcd4946613824a562cee711318989a9b02e24d6cc8c2a7be3146` | `Printer::emit_node_with_hint_and_source_comments` | A6-36-1 |
| `pipelineEmit` | 117173–117178 | `6a43b2cfcfd4e20228aa474d96673af90c71efe2c44ad31612af62fe44208d7b` | `Printer::emit_node_with_hint_and_source_comments` | A6-36-1 |
| `shouldEmitComments` | 117179–117181 | `ddce9678b9409ae56b2cf5dc583f707688f123bc4bc11d1913e461b77304677d` | `EmitContext::nested_comments_suppressed` | A6-36-1 |
| `shouldEmitSourceMaps` | 117182–117184 | `b4c9c3e6a103e3addb0aec0d55ea2d4ad230854588385eaddc06330f08c20b36` | `Printer::emit_transformed_node` | A6-36-1 |
| `getPipelinePhase` | 117185–117215 | `0e29bfcca5db712b3747d892f8c4743919a8330c96c1bda5c4931dbadd009220` | `Printer::emit_node_with_hint_and_source_comments` | A6-36-1 |
| `getNextPipelinePhase` | 117216–117218 | `141386a932a53fd36fe32d5d519b027b61a2b995a5ff65cab3cdd8326db9f7f1` | `Printer::emit_node_with_hint_and_source_comments` | A6-36-1 |
| `pipelineEmitWithNotification` | 117219–117222 | `3e319f7fb4078d10414fdd4842780ba31e5903d0e812e66351b6f2163ab2b98f` | `TransformationResult::before_emit_node` | A6-36-1 |
| `pipelineEmitWithHint` | 117223–117235 | `78e89abc1577a8d034033e940d91fbc6c699501552a003b28e8c73f3009b1b1f` | `Printer::emit_transformed_node` | A6-36-1 |
| `pipelineEmitWithHintWorker` | 117236–117704 | `485ac452835fedcb20a856f43e6eca9b8cd6b08f424fe054dccb3a176e673371` | `Printer::emit_transformed_node_worker` | A6-36-1 |
| `pipelineEmitWithSubstitution` | 117712–117718 | `143322c35d63fd5ef4c65079067c52a939e82aa22301bb507b486e3b3031ba20` | `TransformationResult::substitute_node` | A6-36-1 |
| `emitTypeParameter` | 117839–117854 | `b4b47c664d9316e4783ee0627731ef922859682ef4501ffa1877f9b327281b16` | `Printer::emit_modifiers` | A6-36-1 |
| `emitParameter` | 117855–117871 | `c8a71c70afb1914a0fbbd0f27249f82412abaaa22edb5556dd0eaa008edbc216` | `Printer::emit_modifiers` | A6-36-1 |
| `emitPropertySignature` | 117876–117882 | `d00ad651bf20c3f2d211a7170fb80a3a3c1797061618e054a6e27777af77321b` | `Printer::emit_modifiers` | A6-36-1 |
| `emitPropertyDeclaration` | 117883–117896 | `3a8a77a1520e0f2514117112ee985ff76c5f002894a8e362c15b7c3d1d3f229e` | `Printer::emit_modifiers` | A6-36-1 |
| `emitMethodSignature` | 117897–117902 | `c453a10e9122fd15e599e716d22d84334b123491fb8115c53aaf5681466f3bd2` | `Printer::emit_modifiers` | A6-36-1 |
| `emitMethodDeclaration` | 117903–117914 | `89a778e7fe25aecb9601b99b118ef6a16b7f9083b80365708c49788fdc91a9ff` | `Printer::emit_modifiers` | A6-36-1 |
| `emitConstructor` | 117921–117930 | `2e020b18d76deff748d37e85149cf9e12a99ed95368bf1ef0e74cedbf7c16685` | `Printer::emit_modifiers` | A6-36-1 |
| `emitAccessorDeclaration` | 117931–117943 | `b78bdd3026fb4a0a93d8226592ecf9a0b8167eb27c0747cac1a32ade5b8e643e` | `Printer::emit_modifiers` | A6-36-1 |
| `emitIndexSignature` | 117952–117962 | `44c42588c8ee7a43adc413cf88ce3429e9385bced611a037399d3a21f503e698` | `Printer::emit_modifiers` | A6-36-1 |
| `emitConstructorType` | 118018–118023 | `be259ca6036db88662a7435626fd40fe64b6e96bb36f5fb381cfd3e9f5ed294e` | `Printer::emit_modifiers` | A6-36-1 |
| `emitArrowFunction` | 118336–118339 | `a1e285863bb96cefa83d22615ec2100e76dc155df1bcaf359344299c2ad8d2f7` | `Printer::emit_modifiers` | A6-36-1 |
| `emitSpreadElement` | 118528–118531 | `555db665aa4db3c4793a1da4804ec52af741a2a647f150bb76ab55a053a39d9d` | `Printer::emit_spread_expression` | A6-36-3 |
| `emitVariableStatement` | 118606–118615 | `144128a85a4abb196ccd7f8dbb273c22bfd095d678ea5bc3dae84ea4946474d3` | `Printer::emit_modifiers` | A6-36-1 |
| `emitTokenWithComment` | 118731–118764 | `d7df39bba502705facedce379a636ae55b168b77701df5e72abf10ec444c2e50` | `Printer::emit_token_with_comments_at_boundary` | A6-36-2 |
| `emitFunctionDeclarationOrExpression` | 118956–118968 | `6d2c34fd59ec1e7c8c3ffa2e58652d6717fc7200756562032ca22a17ab18dd72` | `Printer::emit_modifiers` | A6-36-1 |
| `emitClassDeclarationOrExpression` | 119062–119090 | `7f38f7abe1799deb70b1601157c52ddc1b4d9d44c83c871ecdee9dea01859c48` | `Printer::emit_class` | A6-36-2 |
| `emitInterfaceDeclaration` | 119091–119110 | `a887761fc0ac81b4b98c2b77710cc09bf25d3ac5fa7463c1617716c2d5cef1fe` | `Printer::emit_modifiers` | A6-36-1 |
| `emitTypeAliasDeclaration` | 119111–119127 | `5d8d7d0554fee809348cf6483cee70c6b8907ff9f96a0d910364c40c9a9c6797` | `Printer::emit_modifiers` | A6-36-1 |
| `emitEnumDeclaration` | 119128–119142 | `69ecc6800787365590d79116bd96bf15095f6af95adc7b04aba062f2e0c37513` | `Printer::emit_modifiers` | A6-36-1 |
| `emitModuleDeclaration` | 119143–119164 | `c1481129cf6fab009c9bc1dd9fbd483e583386e4e100dc9521bc8df8dd2355cf` | `Printer::emit_modifiers` | A6-36-1 |
| `emitImportEqualsDeclaration` | 119187–119206 | `5d3d7449427b95cc8c112f097efbd64c805dc4f998240807ea2550fbfa2bdcc7` | `Printer::emit_modifiers` | A6-36-1 |
| `emitImportDeclaration` | 119214–119234 | `cf5d69cbe9d54b9f69bc87acc30453f8ac1449d620fd00e278cc71d5fc7a00c6` | `Printer::emit_modifiers` | A6-36-1 |
| `emitExportDeclaration` | 119275–119304 | `d0cf37e456f845641fbc51e1b9686078bd09068ac049e013ad7f247207fc42ed` | `Printer::emit_modifiers` | A6-36-1 |
| `emitSpreadAssignment` | 119536–119541 | `7059018a63098d18b24ef30a55641f81bde3b327d84c9e118f04d35fd9f3dc06` | `Printer::emit_spread_expression` | A6-36-3 |
| `emitDecoratorsAndModifiers` | 119846–119902 | `8fb50c70cd68d557886307bc8242e2f79f0533fea4c44e498feb8ce7c0eba899` | `Printer::emit_modifiers` | A6-36-1 |
| `emitModifierList` | 119903–119907 | `4638028a82dbefee35d3c54bc7285084b413c4b8f5fc4a82e1a1f5bcef53440f` | `Printer::emit_modifiers` | A6-36-1 |
| `emitList` | 120015–120025 | `8a0512c2af9ba16a7481b372c31ae88611a0f3f8b4daaf5919a7278927262b5c` | `Printer::emit_modifiers` | A6-36-1 |
| `emitNodeList` | 120029–120067 | `9286227d388af8c22b8ceb9909204c1b0e7338f14fd54f346c09bd1deabe987d` | `Printer::emit_modifiers` | A6-36-1 |
| `emitNodeListItems` | 120068–120155 | `ebeb65a71c929bbfdf5d1ebd4b2e7216f15bd37117166ef8fbee5b3a9b0a6b40` | `Printer::emit_modifiers` | A6-36-1 |
| `writeToken` | 120210–120212 | `d1b2567202b4cf08f596b25fd907df3782572df2839cad23bd174b9e46f4aa84` | `Printer::record_token_map_side` | A6-36-1 |
| `writeTokenNode` | 120213–120221 | `04ee5d9812e94e045643f0f4fadc7327cf8c883a3261f0bded2aa14fb847ec80` | `Printer::emit_transformed_node_worker` | A6-36-1 |
| `writeTokenText` | 120222–120226 | `ba09e58ada82fd0e2426c23037e1f0c4d854e36c0f7cc2e04700c32a71d273ae` | `Printer::write_fixed_token` | A6-36-1 |
| `pipelineEmitWithComments` | 120978–120986 | `263af5299b06aaeca9c4e6397b6013e6b2c465afcab2709d8cbdd5ace688bd34` | `Printer::emit_node_with_hint_and_source_comments` | A6-36-1 |
| `emitCommentsBeforeNode` | 120987–120994 | `dc59a0901c0b5aaa640586703fb5f5a2420cf3f5ad344fd433501a20612154cf` | `Printer::emit_deferred_expression_leading_comments` | A6-36-1 |
| `emitCommentsAfterNode` | 120995–121006 | `f0baac32a6d9fcf8f005ee8f1b923e1706f679e8a1fb88e2a7753064bb644a89` | `Printer::emit_deferred_expression_trailing_comments` | A6-36-1 |
| `emitLeadingCommentsOfNode` | 121007–121032 | `ce6bf342a94094cccc4bf56debcb99390c8e232705263609dfcf068589284ebb` | `Printer::established_container_sides` | A6-36-1 |
| `emitTrailingCommentsOfNode` | 121033–121046 | `e5c99d84eeab2c12d594ba56695a7a869c49720eb110f3715c0ea3f9271d1112` | `Printer::emit_deferred_expression_trailing_comments` | A6-36-1 |
| `emitLeadingSynthesizedComment` | 121047–121057 | `dd71f6d75be7af5c6d9efc7399a0b5617c1265b12b01469746dffbabe301e5c0` | `Printer::emit_synthetic_leading_comments_for_node` | A6-36-1 |
| `emitTrailingSynthesizedComment` | 121058–121066 | `869809433732254558868ddc3fb16b2f77d79b73d033f153ec72f7a8b54e9432` | `Printer::emit_synthetic_trailing_comments_for_node` | A6-36-1 |
| `emitLeadingComments` | 121123–121134 | `365543958b2ff53ab2f1731d2c6e2cba39658cc427517b6cce17442adad56cd4` | `Printer::emit_leading_comments_for_comment_phase_owner` | A6-36-1 |
| `shouldWriteComment` | 121145–121150 | `9585a2c5cae9ab168b146d094a846dae3f07e50b659fd70090364a9be630bf29` | `should_write_js_doc_style_comment` | A6-36-1 |
| `emitLeadingComment` | 121151–121165 | `35d2197a2d7a2b1904ebcf44899bf3a97b8f3ff986d060c7b6b75e2b0ec157ce` | `emit_source_leading_comments_of_position` | A6-36-1 |
| `emitLeadingCommentsOfPosition` | 121166–121175 | `fa23b688b1540c772ccf513c874d47bba4a08a44e019bc430feb79cbea73d2cd` | `Printer::emit_comments_at_cursor_with_phase` | A6-36-2 |
| `emitTrailingComments` | 121176–121178 | `de3cf762696470a95f312618ffc50f62fe76b80a96420d707543b4b257729fb5` | `Printer::emit_deferred_expression_trailing_comments` | A6-36-1 |
| `emitTrailingComment` | 121179–121190 | `14654b8999872d42159a4d2c11a27fb01fbe47c50e8131b82de8453536d60394` | `emit_same_line_trailing_comments` | A6-36-1 |
| `emitTrailingCommentsOfPosition` | 121191–121198 | `953cde198b7f8098bd7bc8d865e535cdac106e983efe35a4a067498ff239cbc0` | `emit_source_trailing_comments_of_position_with_filter` | A6-36-1 |
| `emitTrailingCommentOfPositionNoNewline` | 121199–121207 | `36e4838b752b5c85052462eef98deb3651bdb4515eb2ba9452097536142f8bc9` | `emit_source_jsx_trailing_comments_of_position` | A6-36-1 |
| `emitTrailingCommentOfPosition` | 121208–121218 | `78fd8227de7e58556e3f2906ffe5aafb334a35d70f8dc0f916bed71b16cb78ea` | `emit_source_intervening_comments_of_position` | A6-36-1 |
| `forEachLeadingCommentToEmit` | 121219–121233 | `2e1fb613c9b9bb29f94a866a92b31e77c392cd696ee93b2e61a001e4e3981a9c` | `Printer::parent_comment_container_owned_prefix_for_owner` | A6-36-1 |
| `forEachTrailingCommentToEmit` | 121234–121238 | `bd6612ac9040b10e756e1b9666a34daebcc0fe66a227dfda01db103bfd3f27a7` | `CommentEmissionScope::retains_end` | A6-36-1 |
| `pipelineEmitWithSourceMaps` | 121277–121282 | `0c57cdceae760b1c5e0c2fca17706537034621460cc030cb1800d55c8c1e3efe` | `Printer::emit_transformed_node` | A6-36-1 |
| `emitSourceMapsBeforeNode` | 121283–121293 | `ac346b41706c68dd97ca01be4df11b5db57df063dfb4a1b84ee9b5ac1efa2517` | `Printer::record_node_map_boundary` | A6-36-1 |
| `emitSourceMapsAfterNode` | 121294–121303 | `6ca767d42995b2ea08ceccbb1261345452b6f976a0665d60a80c68fa14e0aeb6` | `Printer::record_node_map_boundary` | A6-36-1 |
| `emitTokenWithSourceMap` | 121333–121351 | `1f4c5a048470151a92b7a92ff32a976744e5222fb62cfa7c9b3e3964bde39732` | `Printer::record_token_map_side` | A6-36-1 |
| `emitSourceFile` | 119710–119719 | `cea241c6f593d9352d30faf866f13f9ef158c779c560bdea880391d7acd8bd42` | `Printer::write_transformed_source_file` | A6-36-1 |
| `emitSourceFileWorker` | 119753–119769 | `8dfb3b4d8372581bce1739cccac27e886d9260d86756c67ae36c9f462578ef65` | `Printer::write_transformed_source_file` | A6-36-1 |
| `emitPrologueDirectives` | 119789–119811 | `e59768a46c32e263ca0db916fefb348a165bfd2f21990a07899dccbcecbecba9` | `Printer::write_transformed_source_file` | A6-36-1 |
| `emitBodyWithDetachedComments` | 121075–121104 | `b07b0634586c6da5ba8ad7422074544deef1969168e79789b065a964b86b6ac7` | `Printer::write_transformed_source_file` | A6-36-1 |

Native caller inventory is tied to the trusted-base printer file hash.
The five extra caller phases are named separate defects, never TS owners.

| Native caller | Base line | Source role |
| --- | --- | --- |
| ImportDeclaration | 3090 | ordinary TS modifier-calling node owner |
| ExportDeclaration | 3396 | ordinary TS modifier-calling node owner |
| ExportAssignment | 3663 | extra native phase; later local-producer owner |
| VariableStatement | 3730 | ordinary TS modifier-calling node owner |
| PropertyAssignment | 4393 | extra native phase; later local-producer owner |
| ShorthandPropertyAssignment | 4453 | extra native phase; later local-producer owner |
| FunctionDeclaration | 4631 | ordinary TS modifier-calling node owner |
| FunctionExpression | 4696 | ordinary TS modifier-calling node owner |
| ArrowFunction | 4778 | ordinary TS modifier-calling node owner |
| Parameter | 4871 | ordinary TS modifier-calling node owner |
| ClassStaticBlock | 4986 | extra native phase; later local-producer owner |
| PropertyDeclaration | 5015 | ordinary TS modifier-calling node owner |
| Constructor | 5099 | ordinary TS modifier-calling node owner |
| MethodDeclaration | 5133 | ordinary TS modifier-calling node owner |
| GetAccessor | 5193 | ordinary TS modifier-calling node owner |
| SetAccessor | 5237 | ordinary TS modifier-calling node owner |
| ClassDeclarationOrExpression | 8539 | ordinary TS modifier-calling node owner |
| TypeParameter | 8929 | ordinary TS modifier-calling node owner |
| PropertySignature | 8986 | ordinary TS modifier-calling node owner |
| MethodSignature | 9053 | ordinary TS modifier-calling node owner |
| IndexSignature | 9155 | ordinary TS modifier-calling node owner |
| ConstructorType | 9388 | ordinary TS modifier-calling node owner |
| InterfaceDeclaration | 10206 | ordinary TS modifier-calling node owner |
| TypeAliasDeclaration | 10267 | ordinary TS modifier-calling node owner |
| EnumDeclaration | 10320 | ordinary TS modifier-calling node owner |
| ModuleDeclaration | 10399 | ordinary TS modifier-calling node owner |
| ImportEqualsDeclaration | 10523 | ordinary TS modifier-calling node owner |
| NamespaceExportDeclaration | 10573 | extra native phase; later local-producer owner |

## Current architecture references

The following exact current rows retain their validation refs, visibility,
lifetime and frozen predecessor coverage. The separate A36 route row
is qualified only for the complete focused observations below.

| ID | Invariant | Symbols | Current validation | Checks |
| --- | --- | --- | --- | --- |
| `E-ARENA` | Parsed trees remain immutable; the detached arena appends synthetic nodes and tracks the mounted parsed interval. | `tsc_emitter::{TransformArena,TransformSource,TransformSourceId,TransformNode,TransformNodeArray,NodeFactory}` (public) Existing `NodeFactory::modifier_flags` is now `pub(crate)`; downlevel auto-accessor setters use fresh factory modifiers while getters retain source tokens. | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; every later transform packet maps provenance explicitly |
| `E-METADATA-BASE` | Transform flags and `emitNode`-equivalent facts are sparse session side tables; there is no standalone Rust `EmitNode` syntax object. Original/map/comment/value identities are separate. CommentRange carries independent source start/end states through CommentSourceRange; paired source slices and source-map ranges keep their existing domains. | Public `tsc_emitter::TransformArena::{transform_flags,set_transform_flags,array_transform_flags,set_array_transform_flags,metadata,metadata_mut,clear_session_metadata,get_original_node,set_original_node}` over private storage in `crates/emitter/src/factory.rs`; public `tsc_emitter::{EmitMetadata,SourceMapRange,CommentRange,CommentSourceRange,JavaScriptString}` defined in `crates/emitter/src/metadata.rs` (`EmitMetadata` storage fields remain `pub(crate)`, with its cross-crate operations exposed by public methods) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. A6-29 candidate uses existing `EmitMetadata::set_flags` to replace inherited flags on the ES5 accessor receiver clone, retaining its original/text/map ownership. [A6-29](h2-8a-class-helper-accessor-producers.md) qualifies these producers on the train: 1020/1140 complete commands exact twice, all92 repairs and928 prior positives, and unchanged494/451 emitter tests;120 outside cases remain. Prior dated qualifications remain frozen. | H2.5g profile; H2.5h/H2.6 extend rather than infer from text |
| `E-NAMES-BASE` | Generated identity, printable name, target provenance, and allocation scope are separate; final names use the composed tree. | `crate::transform::GeneratedBindingId` (`pub(crate)`); `crate::builtins::generated_bindings::GeneratedBindingScopes` and `target_bindings::{TargetBinding,finalize_generated_binding_names}` (`pub(super)`) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and generated-binding controls |
| `E-PRINTER-BASE` | Printer has no direct checker dependency. It consumes transformed syntax/metadata, drives retained substitution and before/after notification hooks, and applies immutable structural planning. | `tsc_emitter::Printer` (public); `tsc_emitter::TransformationResult::{substitute_node,before_emit_node,after_emit_node}`; `crate::printer::EmissionPlan` and `EmitContext` (private); `crate::factory::NodeFactory::apply_parenthesizer_rules` (private) | `active-qualified`; qualified at validation ref `6acd5d43` (2026-08-21, CS-6 fixture/audit gate); previous ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H2.5f protects the predecessor subset. The H2.5h-a comment-scope packets reshape the private context carrier into the threaded `EmitContext` (CS-2 landed the root and core pipeline byte-identically); the row's hook-composition, no-checker-dependency, and immutable-planning invariants are preserved; requalified at `6acd5d43`. | H2.5g profile; later packets preserve hook composition and enumerate new expression contexts |
| `E-COMMENTS-G` | Parsed ownership, synthetic comments, relocated owners, token progress, and comment progress are distinct typed facts. `TokenEmission` may hand its cursor plus an optional `CommentResume` only to the immediately following token/child; the resume carries both owner start and next position, requires one source and monotone progress, and can merge only with the same owner. A retained arrow anchors trailing comments at its own comment-range end, not semantic-original provenance, so a synthetic token cannot borrow source comments. The simple parameter's list-owned trailing comments and the retained arrow token's trailing comments remain separate phases, including tsc's observable list/leading replay at a shared range start. `SourceLeadingCommentPhaseVisit` distinguishes an absent/suppressed phase from a visited exact range, so a transitional class-field comment anchor is skipped only after that same range was visited; contextless routes retain the anchor. Token/comment ownership performs no source-wide token search; the position-cursor contract keeps local source work linear. H2.5g qualifies only the direct expression and list routes enumerated by its source-comment topology contracts; it does not claim that tsc's enclosing comment-container state already reaches every nested node route. Comment ownership now distinguishes paired, synthesized, start-only and end-only positions; positional readers and ordinary node-phase extent checks remain separate. | Public `tsc_emitter::SyntheticComment`; `crate::metadata::RelocatedStatementListComments` (`pub(crate)`); `crate::comment_cursor::{CommentCursor,CommentResume,CommentResumeError}` and `crate::token_cursor::{TokenCursor,TokenEmission,TokenAnchor,TokenCommentBoundary,FixedToken,TokenWriteKind,TokenLeadingSpace}` (`pub(crate)`); private `crate::printer::SourceLeadingCommentPhaseVisit` and other comment/list workers | `active-qualified`; qualified at validation ref `6acd5d43` (2026-08-21, CS-6 fixture/audit gate); previous ref `0653e10d` (2026-08-17); candidate-only audit 2026-08-14. The H2.5h-a comment-scope packet CS-3 re-expresses the row's qualified expression/list comment projections on the per-side threaded scope (cursor/resume semantics and the token machinery unchanged); requalified at `6acd5d43`; A6-28-5 retires the obsolete initializer trailing-comment marker; [the shared packet](h2-8a-class-field-initializer-comments.md) qualifies all64 new controls and8 prior repairs in434/556 exact complete commands twice, with all490 units/451 contracts passing; subsequent [A6-28-6](h2-8a-promoted-class-export-maps.md) qualifies all100 selected map repairs plus six ES2022 cases in764/868 exact complete commands twice and all490/451 emitter tests, leaving four original A28 comment targets at that prerequisite; subsequent A6-28-7 qualifies all four; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. | Current-code audit, token/comment cursor unit contracts, `position_cursor_2727_statement_work_is_linear_and_scan_free`, source-comment topology and relocated class-field contracts, and arrow replay/resume integration contracts; H2.5g inventory/profile for observable parity |
| `E-COMMENT-SCOPE-H` | tsc scopes three independent values across nested emit routes: `containerPos`, `containerEnd`, and `declarationListContainerEnd`. The threaded immutable representation mandated by this row is LANDED end to end through the H2.5h-a comment-scope packets (CS-2..CS-6): the printer's ambient comment state is the immutable `CommentEmissionScope` triple threaded through an explicit `EmitContext` with `EmitContext::file_root` as the single zero-scope constructor, tsc's per-side flag/JsxText claim predicates on every route, the declaration-list writer (`claim_declaration_list_sides`), and the `NO_NESTED_COMMENTS` suppressed extent. The 30-case witness-driven fixture gate (byte parity against the frozen artifact, both `removeComments` polarities, all six transforms) and the permanent emitter-scoped zero-contextless workspace audit enforce it; qualified at `6acd5d43`. ES2015/Generators production (H2.5h-b) may now proceed. A single optional range or mutable ad-hoc stack is not an equivalent semantic model. Present comment endpoints claim their own sides independently; missing sides and sole-zero positions do not claim a container, and suppression flags cannot create an absent endpoint. | Landed private `crate::comment_cursor::CommentEmissionScope` and `crate::printer::EmitContext` (with `crate::printer::ExpressionSyntaxContext` as its syntax half); the 30-case artifact-driven fixture suite `crates/emitter/tests/integration/comment_scope_witness_contract.rs` | `active-qualified`; CS-2 landed 2026-08-18 ([CS-2 packet](h2-5h-a-cs-2.md)); CS-3 landed 2026-08-19 ([CS-3 packet](h2-5h-a-cs-3.md)); CS-4 landed 2026-08-20 ([CS-4 packet](h2-5h-a-cs-4.md)); CS-5 landed 2026-08-20 ([CS-5 packet](h2-5h-a-cs-5.md)); CS-6 landed 2026-08-21 ([CS-6 packet](h2-5h-a-cs-6.md)); qualified at `6acd5d43`; [A6-28-7](h2-8a-one-sided-class-comments.md) qualifies independent comment endpoints on the train candidate: 896/996 complete commands exact twice, all 16 selected repairs and all 880 prior positives, with 494 units/451 contracts passing; historical paired-range qualification remains retained. | Pinned tsc scope/save-restore graph; direct/list/wrapper/declaration-list oracle fixtures; contextless-call zero-use audit; focused, emitter, owner-control, and inventory gates |
| `E-POSITIONS` | Source bytes, source/generated UTF-16, synthetic ranges, and source switches remain typed domains. | Public `tsc_emitter` position/writer/hook types; definitions in private `position`, `writer`, `metadata`, and `printer` modules | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14. H1/H2.5f protect predecessor behavior. | H2.5g profile and H1 Unicode/newline controls |
| `E-OUTPUT-SCRIPT` | JavaScript artifacts are constructed before the first sink callback; callback order and `emittedFiles` stay independent. | `tsc_emitter::{emit_files_with_activity,EmitArtifact,MemoryOutputSink,FsOutputSink,EmitOutcome}` (public) | `active-qualified`; validation ref `0653e10d` (2026-08-17); candidate audit 2026-08-14 | H2.5g profile and H1 sink-failure controls |
| `E-COMMENT-PHASES-A36` | Ordinary modifier node comments precede list spacing; class keyword positional leading comments do not replay the preceding modifier trailing phase; spread fixed tokens hand actual comment progress to their expression. Source-file statements establish their comment range scope before their worker so a child modifier cannot replay the statement prefix. The existing immutable three-sided scope and metadata flags retain their ownership. | Private `crate::printer::{PositionCommentPhase,Printer::{write_transformed_source_file,emit_modifiers,emit_token_with_source_leading_comments,emit_comments_at_cursor_with_phase,emit_spread_expression}}`; existing `DeferredExpressionSourceComments`, `EmitContext`, `TokenEmission`, `CommentResume` | `active-qualified` for the A6-36 focused printer profile (2026-09-10). General ordinary-node phase migration, ESNext ellipsis admission and retained-accessor/module-cursor owners remain open. | [A6-36](h2-8a-token-comment-phases.md):270/293 complete commands exact twice in each of two independent jobs (55 repairs/215 prior positives/23 outside);96 direct metadata controls and3 original commands exact twice;494 emitter units/451 contracts and32 prior direct controls pass. |

## Qualified after observations

The [after record](../../../../ratchets/h2-8a-token-comment-phases-after.v1.json)
contains 270/293 exact complete commands twice in each of two independent jobs:
all 55 intended repairs and all 215 prior positives. The 1126 supplemental
complete command tuples agree across the two jobs; primary commands contribute
another 1126 executions and are counted separately. Three original ES2015
allowJs/JSDoc commands match twice, including declarations and diagnostics.
All 96 direct metadata cases match twice, repairing all 32 prior failures;
the prior 32 direct class-header cases also remain exact twice. The 494 emitter
unit IDs and 451 contract IDs are unchanged and all pass.

The initial candidate caused 27 JSDoc/header duplication regressions in the
unchanged declaration reprint contract. Its failed process and source inputs
remain archived. The source-file statement worker now receives its existing
per-statement comment scope, preserving prefix ownership in nested modifiers.
All 1,350 frozen declaration reprints pass on the corrected source. Every
qualifying job above ran on those final source bytes; the initial candidate's
green jobs were not reused as final qualification. The 1,350 declaration
reprints are separate from the Program and metadata denominators.

The 23 outside commands continue to compare against unchanged TypeScript
expectations. The complete before/after tuples are preserved in outside_review:
18 ESNext ellipsis preflights, four retained ES2022 accessor producer/map
defects, and one recovery module cursor. Nineteen complete failed tuples are
unchanged. In the four accessor cases, backing-field/getter comments improve
while a reused source modifier now adds an unwanted comment to the synthetic
setter. This component regression is explicitly recorded under the retained
accessor producer; no previously exact complete case regressed. Their maps also
remain unequal. Both focused processes exit101 because these comparisons remain red.

No shared loader, option adapter, expected byte, guard, profile or accepted
membership was changed. The global769 and class1228 checkpoints remain atA34;
these focused results do not update either count. H2.8a–e and hosted acceptance
before runtime landing remain required. Continue with ellipsis admission and
the remaining source-owned local producers; this packet does not close H2.8a.
