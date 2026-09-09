# H2.8a A6-37 — ordinary ellipsis comments and admission

Kind: qualified focused runtime prerequisite. The base is `dde032d995ec708e89196301a4d0d6ae72fc606f`.
H2.8a-e remain open. This packet restores the source-comment calls owned by rest
syntax and JSX before removing the raw ellipsis comment guard. It qualifies no
complete corpus band, API1 custom-transform route, or hosted acceptance by itself.

Before runtime edits, `python3 scripts/check-ellipsis-comment-owners-readiness.py --before`
checks the frozen398 complete commands,144 direct controls, exact upstream bodies,
all branch dispositions, current architecture/authority rows, trusted-base Rust
hashes and required after coverage. Runtime edits are limited to printer.rs and
builtins.rs; the latter changes only the ellipsis guard and its dead target
field/preflight argument. No checker/parser/host mutation is authorized here.

| Step | Concrete change | State, lifetime and check |
| --- | --- | --- |
| A6-37-1 | Private ordinary-child source adapter with explicit EmitHint; optional positional comment phase in fixed-token worker | Borrow the actual node/parent, immutable EmitContext and actual TokenEmission. Keep notification/substitution/map order, restore scopes by value, and preserve old token caller phases. New144 and prior96/32 direct controls; adjacent emitter units/contracts. |
| A6-37-2 | Complete ordinary children of Parameter, BindingElement, NamedTupleMember, RestType and JSDocVariadicType; matching SourceLeading equals/colon boundaries | Use actual source/comment ranges and parent/list claims, remove the redundant manual Parameter name trailing call, retain initializer cursor producer boundaries. All parameter/binding/type Program and direct witnesses, including newline comments and declaration reprints. |
| A6-37-3 | JSX spread attribute/expression child phases and open-token resume; suppression with brace maps retained | Empty-expression guard, multiline indentation and expression.end/open-return close cursor stay source owned. Child and positional source phases honor inherited NoNested without synthesizing flags/cursors. JSX preserve/React complete commands and direct flags. |
| A6-37-4 | Retire has_comment_after_ellipsis and the sole downlevels_es2018 field/argument path | Preserve independent private-name/in, optional assertion, parse/depth/decorator/JSX gates. All80 guard candidates must match complete commands; no admission-only success or replacement source scans. |
| A6-37-5 | Qualify97 repairs and293 prior positives, all144 direct controls, adjacent regressions | Two independent focused after jobs, emitter494 units/451 contracts with1350 reprints, prior96 token and32 class-header direct controls. Review all8 remaining failed tuple components. Existing hosted acceptance still required before landing. |

The versioned readiness manifest records124 complete TypeScript bodies, their
exact source hashes, and552 branch predicates/dispositions. The five new name/writer helpers were read
in full. In particular, `emitParameter` calls `emitNodeWithWriter(name,
writeParameter)`, whose callback changes the writer but still calls ordinary
`emit(node)` with **Unspecified**, not IdentifierName or Expression. Native
`TextWriter::write_parameter`, `write_symbol` and `write` have identical text
semantics; spelling/source range reuse remains owned by the identifier worker.

## Source comment requests and hint

Add one private adapter for an optional ordinary child with explicit EmitHint.
It validates the real child reference, uses a complete deferred source-comment
extent with the actual parent/current CommentEmissionScope, and calls the existing
notification/substitution/map/worker pipeline. Ordinary dot tokens and binding
names use Unspecified. JSX expressions use Expression. No substitution result or
original node is converted to a spelling and rebuilt. Missing optional dot emits
nothing; required names/expressions keep existing typed child errors.

The caller's inherited NoNestedComments prevents requesting child source phases;
the child's own NoNested flag must still leave its own leading/trailing phases
live. Do not convert NoNested to NoLeading/NoTrailing flags on the child or mutate
metadata. Existing pipeline remains responsible for synthetic metadata; this
packet does not qualify arbitrary custom-transform metadata (API1). Any real
ordinary-command regression in that pipeline still requires repair, not exclusion.

Migrate the ordinary child phases of Parameter (dot/name/question/type/initializer),
BindingElement (dot/propertyName/name/initializer), NamedTupleMember
(dot/name/question/type), and RestType/JSDocVariadicType's type child. Preserve surrounding
decorators/modifiers, optional fields, type syntax gates, initializer cursor
selection and parent/list claims. BindingElement's propertyName remains an
ordinary child before its raw colon. A newline comment
after `...` belongs to the following name's leading phase: a complete dot phase
alone is insufficient. See `h2-8a-ellipsis-comment-next-name-leading-review.json`.

Parameter currently manually calls `emit_trailing_comments_at_node_position(name)`
when an erased type exists. That repeats the same comment-range end that the new
complete name request visits. Remove that redundant source-comment call when the
complete ordinary name owns it. Keep erased-type metadata/cursor lookup used by
initializer emission separate; do not silently alter initializer position rules.
The native Parameter transform's additional type_node annotation is not present
in upstream visitParameter (95029-95052); it is an existing
producer discrepancy to record, not evidence that upstream adds a second type
trailing phase on Parameter names. Upstream variable declaration type_node and
its `emitCommentsAfterNode` second phase remain a distinct unchanged owner.

Fixed equals boundaries after the newly complete name/type phases must select
SourceLeading instead of replaying their same-line tails through BoundaryUnion.
Their initializer receives the real equals TokenEmission and Expression hint.
The selected fixed-token and ordinary-child adapters honor inherited NoNested;
raw type annotation colon/space remain raw prefix writers followed by ordinary
type emission. Existing general declaration helpers are not globally migrated.

NamedTupleMember's colon uses `emitTokenWithComment` at **name.end**, not
question.end, and must select SourceLeading after the complete ordinary phases.
Do not introduce a source-token range on raw RestType ellipses. RestType direct
controls are already12/12 exact and are mandatory preservation controls.

## JSX brace and child handoff

JsxSpreadAttribute retains raw `{...`, ordinary complete expression, raw `}`.
Its current inherited NoNested branch must suppress the expression source phase;
direct NoNested and NoOwnOrNested expose the unwanted `/* tail */` (2 failures).

JsxExpression's optional-expression/empty-comment guard must include inherited
NoNested. Keep multiline indentation, source-shape predicates, fixed opening brace
position, and close position at expression.end or the actual opening-token return.
Use the real opening TokenEmission to hand its consumed comment resume to the
first child (dot if present, expression otherwise). Emit the dot's complete
ordinary phase and then the expression's complete ordinary phase. Replace the
manual first-child leading phase, rather than retaining it alongside the new one.
The closing brace uses **SourceLeading** positional comments after the ordinary
expression trailing phase so same-line tails are not replayed. No guessed resume
offset and no global visited-comment set.

Extend the private fixed-token worker's phase argument to
`Option<PositionCommentPhase>`: None suppresses both positional leading and
trailing comments but preserves all source validation, skipTrivia arithmetic,
token text, SourceBytePosition checks, and brace-map before/after events. Existing
callers pass Some(their current phase). JSX passes None only under inherited
NoNested, otherwise Some(SourceLeading). A raw-brace shortcut would lose maps.
The returned comment resume must remain None when no comment phase ran.

## Admission and preserved independent owners

Retire `has_comment_after_ellipsis` and its sole `downlevels_es2018` field/value/
constructor forwarding/preflight parameter together. Keep the private-name/in
comment and optional-chain/assertion predicates and their typed guard. Preserve
the independent parse-diagnostic/depth/decorator/JSX admission rules. Do not add
source-wide replacement scans, target exceptions, fixture IDs or spellings.

The105 fresh Programs retain original93 rows and12 additional global-JSX positives.
All24 JSX windows have actual guard errors in both corrected before jobs;
the initial before-1 rejected jsx in the adapter before command execution instead.
Keep that historical distinction and keep original JSX diagnostic cases.

Three ES2015 object-rest binding commands have equal JavaScript text and different
source maps in before-1. This is a separate ES2018 lowering map producer, not a dot
printer comment repair. Retained auto-accessor4 and recovery module cursor1 remain
explicit existing failures. Other currently guarded commands must be compared in
full after admission; a newly visible checker/transform/map failure requires its
actual owner and packet amendment, never an invented success or dropped control.

## Required observations

Direct-before-3:105/144 exact twice,39 failed twice,288 prints,78 identical repeated
failure captures. All removeComments72 are exact. RestType12 are exact.
The previous two direct attempts failed to compile and produced zero observations.
Before-2 was prepared but never run. Corrected before-3 ACTUAL101 has293 exact
twice,105 failures once,691 primary commands and691 supplemental commands; all270
A36 positives and all374 captured before-1 complete tuples are preserved.
Its owner partition has80 guard candidates and17 comment-phase candidates,
requiring390 exact commands after97 repairs, with8 explicit outside failures.
Independent before-4 session60848 ACTUAL101 confirms the same293/105 partition,
all270 A36 positives, and691 primary/supplemental commands each. The freezer
checks every complete tuple against before-3 before recording this evidence.

After production edits, require the complete intended398 repair/preservation
partition and all144 direct controls, plus adjacent emitter unit/contracts,
all1350 frozen declaration reprints inside the existing contract suite, and prior
96 modifier/spread and32 class-header direct controls if their token/map owners
are modified. Existing hosted acceptance is still required before landing; neither
these focused results nor source-owner counts prove H2.8a-e completion.

## Whole upstream and native ownership

All124 complete upstream bodies were read. The machine manifest retains all552
branch expressions and explicit owned/unchanged/outside dispositions. Existing
initializer metadata/cursor and general declaration/API1 boundaries are recorded
as separate owners, not silently promoted by this focused repair.

| TS owner | `_tsc.js` span | Body SHA256 | Native owner | Step |
| --- | --- | --- | --- | --- |
| `getOriginalNode` | 11400–11410 | `e6e639e966314faf444b9b68796893745ffb06eb0adcf1180d6935332d8797a3` | `TransformArena::get_original_node` | A6-37-1 |
| `isParseTreeNode` | 11423–11425 | `d8a6d217a3087e6809bfb3df3a4815eefce954e8175aed4c744b515f891dbe8d` | `TransformArena::parse_tree_node` | A6-37-1 |
| `getParseTreeNode` | 11426–11437 | `80b5c2449cb8320cf209184a8eef484f944379da161e54d74dd54ed1f0d2d592` | `Printer::node_has_source_token_shape` | A6-37-1 |
| `getEmitFlags` | 13054–13057 | `6d8c5eece414332d11f366f7ae016d6d3253477e31519ee07e143b4263b35933` | `Printer::expression_comment_phase_owner_for_node` | A6-37-1 |
| `moveRangePastDecorators` | 17307–17310 | `27d3b9fba1576ed2d7269a9fe1b694ac1e16e977da92c9935f13359611222a93` | `Printer::class_keyword_cursor` | A6-37-1 |
| `moveRangePastModifiers` | 17311–17317 | `9d43119a4e2ea51f3f5a151f00816f7985c1781c9dc80cfd8e44f40807d3db9d` | `Printer::class_keyword_cursor` | A6-37-1 |
| `positionIsSynthesized` | 18811–18813 | `d7c8efa6a3407c62a96399f410fac2ae254372213b526c0c0c96811612cfb7b7` | `SourceRange` | A6-37-1 |
| `getCommentRange` | 25358–25361 | `84e06c1d1498906aa5765dfc0bdfd9dd4ca9d1c75367b90067f24dbc57936cd2` | `Printer::comment_range_for_node` | A6-37-1 |
| `emit` | 117145–117148 | `f998a75ec5c7ebceb127e49ec7315b9e3b9aa90c15e4a5bd7d3b57cd324f5682` | `Printer::emit_node_id_with_context_and_source_comments` | A6-37-1 |
| `emitIdentifierName` | 117149–117157 | `847193fac9ff770a8b062033c89f8676a42ac88e0adb16b22fdafb5a33c09ae4` | `Printer::emit_identifier_name_with_context` | A6-37-1 |
| `emitExpression` | 117158–117161 | `71793715793bfcd4946613824a562cee711318989a9b02e24d6cc8c2a7be3146` | `Printer::emit_node_with_hint_and_source_comments` | A6-37-1 |
| `emitJsxAttributeValue` | 117162–117164 | `4fced51ac1c35c59ef9d3fcee3191a4ee4c399b9ec76d68035514b4966787df7` | `Printer::emit_transformed_node_worker (JsxAttribute/StringLiteral)` | A6-37-3 |
| `pipelineEmit` | 117173–117178 | `6a43b2cfcfd4e20228aa474d96673af90c71efe2c44ad31612af62fe44208d7b` | `Printer::emit_node_with_hint_and_source_comments` | A6-37-1 |
| `shouldEmitComments` | 117179–117181 | `ddce9678b9409ae56b2cf5dc583f707688f123bc4bc11d1913e461b77304677d` | `EmitContext::nested_comments_suppressed` | A6-37-1 |
| `shouldEmitSourceMaps` | 117182–117184 | `b4c9c3e6a103e3addb0aec0d55ea2d4ad230854588385eaddc06330f08c20b36` | `Printer::emit_transformed_node` | A6-37-1 |
| `getPipelinePhase` | 117185–117215 | `0e29bfcca5db712b3747d892f8c4743919a8330c96c1bda5c4931dbadd009220` | `Printer::emit_node_with_hint_and_source_comments` | A6-37-1 |
| `getNextPipelinePhase` | 117216–117218 | `141386a932a53fd36fe32d5d519b027b61a2b995a5ff65cab3cdd8326db9f7f1` | `Printer::emit_node_with_hint_and_source_comments` | A6-37-1 |
| `pipelineEmitWithNotification` | 117219–117222 | `3e319f7fb4078d10414fdd4842780ba31e5903d0e812e66351b6f2163ab2b98f` | `TransformationResult::before_emit_node` | A6-37-1 |
| `pipelineEmitWithHint` | 117223–117235 | `78e89abc1577a8d034033e940d91fbc6c699501552a003b28e8c73f3009b1b1f` | `Printer::emit_transformed_node` | A6-37-1 |
| `pipelineEmitWithHintWorker` | 117236–117704 | `485ac452835fedcb20a856f43e6eca9b8cd6b08f424fe054dccb3a176e673371` | `Printer::emit_transformed_node_worker` | A6-37-1 |
| `pipelineEmitWithSubstitution` | 117712–117718 | `143322c35d63fd5ef4c65079067c52a939e82aa22301bb507b486e3b3031ba20` | `TransformationResult::substitute_node` | A6-37-1 |
| `emitTypeParameter` | 117839–117854 | `b4b47c664d9316e4783ee0627731ef922859682ef4501ffa1877f9b327281b16` | `Printer::emit_modifiers` | A6-37-1 |
| `emitParameter` | 117855–117871 | `c8a71c70afb1914a0fbbd0f27249f82412abaaa22edb5556dd0eaa008edbc216` | `Printer::emit_transformed_node_worker (Parameter)` | A6-37-2 |
| `emitPropertySignature` | 117876–117882 | `d00ad651bf20c3f2d211a7170fb80a3a3c1797061618e054a6e27777af77321b` | `Printer::emit_modifiers` | A6-37-1 |
| `emitPropertyDeclaration` | 117883–117896 | `3a8a77a1520e0f2514117112ee985ff76c5f002894a8e362c15b7c3d1d3f229e` | `Printer::emit_modifiers` | A6-37-1 |
| `emitMethodSignature` | 117897–117902 | `c453a10e9122fd15e599e716d22d84334b123491fb8115c53aaf5681466f3bd2` | `Printer::emit_modifiers` | A6-37-1 |
| `emitMethodDeclaration` | 117903–117914 | `89a778e7fe25aecb9601b99b118ef6a16b7f9083b80365708c49788fdc91a9ff` | `Printer::emit_modifiers` | A6-37-1 |
| `emitConstructor` | 117921–117930 | `2e020b18d76deff748d37e85149cf9e12a99ed95368bf1ef0e74cedbf7c16685` | `Printer::emit_modifiers` | A6-37-1 |
| `emitAccessorDeclaration` | 117931–117943 | `b78bdd3026fb4a0a93d8226592ecf9a0b8167eb27c0747cac1a32ade5b8e643e` | `Printer::emit_modifiers` | A6-37-1 |
| `emitCallSignature` | 117944–117946 | `ad8f666751fee27890100b6ff7dd45406df263bd93b068ccb8329c37f619e087` | `Printer::emit_call_signature` | A6-37-1 |
| `emitConstructSignature` | 117947–117951 | `9a6d030c141f03f6a5704263ebc9908418993c72321d28e7bd16c9ff987bb7f2` | `Printer::emit_construct_signature` | A6-37-1 |
| `emitIndexSignature` | 117952–117962 | `44c42588c8ee7a43adc413cf88ce3429e9385bced611a037399d3a21f503e698` | `Printer::emit_modifiers` | A6-37-1 |
| `emitFunctionType` | 117987–117989 | `e821a7f2097f595a2652d37be70554d7d62de730f1029c7df673d280a27ce28f` | `Printer::emit_function_type` | A6-37-1 |
| `emitFunctionTypeHead` | 117990–117995 | `ddb7d8dad42cc356afba13a1c1d86c3872d219bf1017c9e339222fc7ae0d413e` | `Printer::emit_function_type_head` | A6-37-1 |
| `emitFunctionTypeBody` | 117996–117999 | `1c493fdc5b06bb3b4bd47832322f3b57b2cfd441c58956e233f5bdb7c6e83711` | `Printer::emit_function_type_body` | A6-37-1 |
| `emitConstructorType` | 118018–118023 | `be259ca6036db88662a7435626fd40fe64b6e96bb36f5fb381cfd3e9f5ed294e` | `Printer::emit_modifiers` | A6-37-1 |
| `emitRestOrJSDocVariadicType` | 118044–118047 | `0c537e5af9b680d3e92913404528d5f4430baeb1d421fee15b0d115158402910` | `Printer::emit_rest_type; Printer::emit_transformed_node_worker (JSDocVariadicType)` | A6-37-2 |
| `emitNamedTupleMember` | 118054–118061 | `85b9ebf0be566066920a0c5a0392fd6e765591f21884b507c67036cfd1d821b6` | `Printer::emit_named_tuple_member` | A6-37-2 |
| `emitObjectBindingPattern` | 118183–118187 | `7d48e55e79417fd4749c89f9adde9bdc03ead12308ad1576c438ddc91a6f696e` | `Printer::emit_delimited_expression_list (ObjectBindingPattern)` | A6-37-1 |
| `emitArrayBindingPattern` | 118188–118192 | `dd8a3ac8a614faa4e0d16448bc7338e203d645153266cd5783e3ffb809bf184e` | `Printer::emit_delimited_expression_list (ArrayBindingPattern)` | A6-37-1 |
| `emitBindingElement` | 118193–118202 | `6e9fbf5ef42983bc2276648844edfd783f023aa063832a53bfe851edfd8f66e1` | `Printer::emit_transformed_node_worker (BindingElement)` | A6-37-2 |
| `emitArrowFunction` | 118336–118339 | `a1e285863bb96cefa83d22615ec2100e76dc155df1bcaf359344299c2ad8d2f7` | `Printer::emit_modifiers` | A6-37-1 |
| `emitSpreadElement` | 118528–118531 | `555db665aa4db3c4793a1da4804ec52af741a2a647f150bb76ab55a053a39d9d` | `Printer::emit_spread_expression` | A6-37-1 |
| `emitVariableStatement` | 118606–118615 | `144128a85a4abb196ccd7f8dbb273c22bfd095d678ea5bc3dae84ea4946474d3` | `Printer::emit_modifiers` | A6-37-1 |
| `emitTokenWithComment` | 118731–118764 | `d7df39bba502705facedce379a636ae55b168b77701df5e72abf10ec444c2e50` | `Printer::emit_token_with_comments_at_boundary` | A6-37-1 |
| `emitVariableDeclaration` | 118934–118940 | `f7e60332c5befaae55cf6e30cd52807fe00353958f1a4721c717abebf268a57f` | `Printer::emit_transformed_node_worker (VariableDeclaration)` | A6-37-1 |
| `emitFunctionDeclarationOrExpression` | 118956–118968 | `6d2c34fd59ec1e7c8c3ffa2e58652d6717fc7200756562032ca22a17ab18dd72` | `Printer::emit_modifiers` | A6-37-1 |
| `emitSignatureAndBody` | 118969–118982 | `89b92f0b4759c58fe92caf1d0f3d783f883659010450f9f40cf948d5453a0847` | `Printer::emit_transformed_node_worker (function-like owners)` | A6-37-1 |
| `emitSignatureHead` | 118994–118998 | `1051bc6f6d403e11ae463222deba4cc157d1615716c2c426b26dea7e6804defb` | `Printer::emit_signature_head` | A6-37-1 |
| `emitClassDeclarationOrExpression` | 119062–119090 | `7f38f7abe1799deb70b1601157c52ddc1b4d9d44c83c871ecdee9dea01859c48` | `Printer::emit_class` | A6-37-1 |
| `emitInterfaceDeclaration` | 119091–119110 | `a887761fc0ac81b4b98c2b77710cc09bf25d3ac5fa7463c1617716c2d5cef1fe` | `Printer::emit_modifiers` | A6-37-1 |
| `emitTypeAliasDeclaration` | 119111–119127 | `5d8d7d0554fee809348cf6483cee70c6b8907ff9f96a0d910364c40c9a9c6797` | `Printer::emit_modifiers` | A6-37-1 |
| `emitEnumDeclaration` | 119128–119142 | `69ecc6800787365590d79116bd96bf15095f6af95adc7b04aba062f2e0c37513` | `Printer::emit_modifiers` | A6-37-1 |
| `emitModuleDeclaration` | 119143–119164 | `c1481129cf6fab009c9bc1dd9fbd483e583386e4e100dc9521bc8df8dd2355cf` | `Printer::emit_modifiers` | A6-37-1 |
| `emitImportEqualsDeclaration` | 119187–119206 | `5d3d7449427b95cc8c112f097efbd64c805dc4f998240807ea2550fbfa2bdcc7` | `Printer::emit_modifiers` | A6-37-1 |
| `emitImportDeclaration` | 119214–119234 | `cf5d69cbe9d54b9f69bc87acc30453f8ac1449d620fd00e278cc71d5fc7a00c6` | `Printer::emit_modifiers` | A6-37-1 |
| `emitExportDeclaration` | 119275–119304 | `d0cf37e456f845641fbc51e1b9686078bd09068ac049e013ad7f247207fc42ed` | `Printer::emit_modifiers` | A6-37-1 |
| `emitJsxElement` | 119380–119384 | `5b1d5fe90200fd41b6edb6a55760302591919ee4786e9ae1d536ece167ca2941` | `Printer::emit_transformed_node_worker (JsxElement)` | A6-37-3 |
| `emitJsxSelfClosingElement` | 119385–119392 | `4592183136bf3b135a16e71300dfe131fdead3c3926cc71eada7798b4df2f399` | `Printer::emit_transformed_node_worker (JsxSelfClosingElement)` | A6-37-3 |
| `emitJsxFragment` | 119393–119397 | `8b938ac55ea023e0cf05619801a560918203b997eb0f9d412e6d8fafd32e22be` | `Printer::emit_transformed_node_worker (JsxFragment)` | A6-37-3 |
| `emitJsxOpeningElementOrFragment` | 119398–119412 | `a8a7e24f504b79b4cbeeaad203d17053240f9a059dc05ee24fbfce11cd56ab6a` | `Printer::emit_transformed_node_worker (JsxOpeningElement/JsxOpeningFragment)` | A6-37-3 |
| `emitJsxClosingElementOrFragment` | 119416–119422 | `22d8acdf6f22e267e5602001501e29c4ef9a00a6496aa705edfa1d2017948315` | `Printer::emit_transformed_node_worker (JsxClosingElement/JsxClosingFragment)` | A6-37-3 |
| `emitJsxAttributes` | 119423–119425 | `fcaf24f360e9df2ecc0df320e2031762660cef08b234f8a69d5449388586e3d1` | `Printer::emit_jsx_attributes` | A6-37-3 |
| `emitJsxAttribute` | 119426–119429 | `bd1d8ff0155179e2f1bb6ff948166a493bb80f690e7a501c45e0b1c7f791266f` | `Printer::emit_transformed_node_worker (JsxAttribute)` | A6-37-3 |
| `emitJsxSpreadAttribute` | 119430–119434 | `421c0a5e8b120d58ebdcc3372af28058a18b7c00c8729215840200e9165dc8b0` | `Printer::emit_transformed_node_worker (JsxSpreadAttribute)` | A6-37-3 |
| `hasTrailingCommentsAtPosition` | 119435–119439 | `1f76c889fc7f6325907f182ae0c373cb840c9a16d414d60a02cf218a1f7c5843` | `Printer::original_jsx_has_comments_at_open` | A6-37-3 |
| `hasLeadingCommentsAtPosition` | 119440–119444 | `3ebbcef5ed893e1d499214be1463c1b22ee3313f084fcf6a8edd6dd93e4d50f8` | `Printer::original_jsx_has_comments_at_open` | A6-37-3 |
| `hasCommentsAtPosition` | 119445–119447 | `1f7f3bcd5f8966fc3f59c3841891c054b9eda9800032f6835074b99d9a6d6db1` | `Printer::original_jsx_has_comments_at_open` | A6-37-3 |
| `emitJsxExpression` | 119448–119463 | `79028c94779cb389cea543b200cf72de1ff41170c2160432de1121a22e2a0ef1` | `Printer::emit_transformed_node_worker (JsxExpression)` | A6-37-3 |
| `emitJsxNamespacedName` | 119464–119468 | `d20c8db075517380a721f357d96fc7eb0983b3358aceb987e0847faa3ffb7596` | `Printer::emit_transformed_node_worker (JsxNamespacedName)` | A6-37-3 |
| `emitJsxTagName` | 119469–119475 | `ccfb711b5b88cdca03af28c671c9d0a40699f53dec23a65421ffa94f988effaf` | `Printer::emit_required_jsx_tag_name` | A6-37-3 |
| `emitSpreadAssignment` | 119536–119541 | `7059018a63098d18b24ef30a55641f81bde3b327d84c9e118f04d35fd9f3dc06` | `Printer::emit_spread_expression` | A6-37-1 |
| `emitSourceFile` | 119710–119719 | `cea241c6f593d9352d30faf866f13f9ef158c779c560bdea880391d7acd8bd42` | `Printer::write_transformed_source_file` | A6-37-1 |
| `emitSourceFileWorker` | 119753–119769 | `8dfb3b4d8372581bce1739cccac27e886d9260d86756c67ae36c9f462578ef65` | `Printer::write_transformed_source_file` | A6-37-1 |
| `emitPrologueDirectives` | 119789–119811 | `e59768a46c32e263ca0db916fefb348a165bfd2f21990a07899dccbcecbecba9` | `Printer::write_transformed_source_file` | A6-37-1 |
| `emitDecoratorsAndModifiers` | 119846–119902 | `8fb50c70cd68d557886307bc8242e2f79f0533fea4c44e498feb8ce7c0eba899` | `Printer::emit_modifiers` | A6-37-1 |
| `emitModifierList` | 119903–119907 | `4638028a82dbefee35d3c54bc7285084b413c4b8f5fc4a82e1a1f5bcef53440f` | `Printer::emit_modifiers` | A6-37-1 |
| `emitTypeAnnotation` | 119908–119914 | `bd4ed80de73c0256110b8beadf4b9ec0bcf6eeb71669e4b3f98dd515a7325513` | `Printer::emit_type_annotation; selected ordinary type child adapter` | A6-37-2 |
| `emitInitializer` | 119915–119922 | `ae5d88a7431fb9468d6238393cda0221d118e7b046bcebef829caaad3bda20f7` | `Printer::emit_transformed_node_worker (Parameter/BindingElement initializer)` | A6-37-2 |
| `emitNodeWithPrefix` | 119923–119928 | `d7d6e7020feefa909c4afa0723025de3e86df7af3b296b09bab4401e646e7084` | `Printer::emit_type_annotation; TextWriter::write_punctuation` | A6-37-1 |
| `emitTypeArguments` | 119967–119969 | `095bb2e591a182100c1586f39db20b74f024c9ca9e743692af8894fde3317d60` | `Printer::emit_type_arguments` | A6-37-1 |
| `emitTypeParameters` | 119970–119975 | `67edb593d7741ab6b3273370e553eda19470127765c51d0346e8c30c93e71bf6` | `Printer::emit_type_parameters` | A6-37-1 |
| `emitParameters` | 119976–119978 | `29cbd34e00c1a1d702c41218432f77de96dc45d96af69b0d81fb605181c0828c` | `Printer::emit_parameter_list` | A6-37-1 |
| `canEmitSimpleArrowHead` | 119979–119982 | `1bbd66ea7cd5ef900ab62b7fb05f75cbcfbf4996baa29f1a64c0aafcf45c68d0` | `Printer::can_emit_simple_arrow_head` | A6-37-1 |
| `emitParametersForArrow` | 119983–119989 | `a10b4e848b2b723985d53b1b0379275490fcf1a88240ffeb3a62b7a36351baef` | `Printer::emit_arrow_parameter_list` | A6-37-1 |
| `emitParametersForIndexSignature` | 119990–119992 | `f8d6242cc9ba28683f5b66fafe283cc94112bd81dce6ad0f776a6a56fbb56b82` | `Printer::emit_parameter_list` | A6-37-1 |
| `emitList` | 120015–120025 | `8a0512c2af9ba16a7481b372c31ae88611a0f3f8b4daaf5919a7278927262b5c` | `Printer::emit_modifiers` | A6-37-1 |
| `emitNodeList` | 120029–120067 | `9286227d388af8c22b8ceb9909204c1b0e7338f14fd54f346c09bd1deabe987d` | `Printer::emit_modifiers` | A6-37-1 |
| `emitNodeListItems` | 120068–120155 | `ebeb65a71c929bbfdf5d1ebd4b2e7216f15bd37117166ef8fbee5b3a9b0a6b40` | `Printer::emit_modifiers` | A6-37-1 |
| `writeToken` | 120210–120212 | `d1b2567202b4cf08f596b25fd907df3782572df2839cad23bd174b9e46f4aa84` | `Printer::record_token_map_side` | A6-37-1 |
| `writeTokenNode` | 120213–120221 | `04ee5d9812e94e045643f0f4fadc7327cf8c883a3261f0bded2aa14fb847ec80` | `Printer::emit_transformed_node_worker` | A6-37-1 |
| `writeTokenText` | 120222–120226 | `ba09e58ada82fd0e2426c23037e1f0c4d854e36c0f7cc2e04700c32a71d273ae` | `Printer::write_fixed_token` | A6-37-1 |
| `pipelineEmitWithComments` | 120978–120986 | `263af5299b06aaeca9c4e6397b6013e6b2c465afcab2709d8cbdd5ace688bd34` | `Printer::emit_node_with_hint_and_source_comments` | A6-37-1 |
| `emitCommentsBeforeNode` | 120987–120994 | `dc59a0901c0b5aaa640586703fb5f5a2420cf3f5ad344fd433501a20612154cf` | `Printer::emit_deferred_expression_leading_comments` | A6-37-1 |
| `emitCommentsAfterNode` | 120995–121006 | `f0baac32a6d9fcf8f005ee8f1b923e1706f679e8a1fb88e2a7753064bb644a89` | `Printer::emit_deferred_expression_trailing_comments` | A6-37-1 |
| `emitLeadingCommentsOfNode` | 121007–121032 | `ce6bf342a94094cccc4bf56debcb99390c8e232705263609dfcf068589284ebb` | `Printer::established_container_sides` | A6-37-1 |
| `emitTrailingCommentsOfNode` | 121033–121046 | `e5c99d84eeab2c12d594ba56695a7a869c49720eb110f3715c0ea3f9271d1112` | `Printer::emit_deferred_expression_trailing_comments` | A6-37-1 |
| `emitLeadingSynthesizedComment` | 121047–121057 | `dd71f6d75be7af5c6d9efc7399a0b5617c1265b12b01469746dffbabe301e5c0` | `Printer::emit_synthetic_leading_comments_for_node` | A6-37-1 |
| `emitTrailingSynthesizedComment` | 121058–121066 | `869809433732254558868ddc3fb16b2f77d79b73d033f153ec72f7a8b54e9432` | `Printer::emit_synthetic_trailing_comments_for_node` | A6-37-1 |
| `emitBodyWithDetachedComments` | 121075–121104 | `b07b0634586c6da5ba8ad7422074544deef1969168e79789b065a964b86b6ac7` | `Printer::write_transformed_source_file` | A6-37-1 |
| `emitLeadingComments` | 121123–121134 | `365543958b2ff53ab2f1731d2c6e2cba39658cc427517b6cce17442adad56cd4` | `Printer::emit_leading_comments_for_comment_phase_owner` | A6-37-1 |
| `shouldWriteComment` | 121145–121150 | `9585a2c5cae9ab168b146d094a846dae3f07e50b659fd70090364a9be630bf29` | `should_write_js_doc_style_comment` | A6-37-1 |
| `emitLeadingComment` | 121151–121165 | `35d2197a2d7a2b1904ebcf44899bf3a97b8f3ff986d060c7b6b75e2b0ec157ce` | `emit_source_leading_comments_of_position` | A6-37-1 |
| `emitLeadingCommentsOfPosition` | 121166–121175 | `fa23b688b1540c772ccf513c874d47bba4a08a44e019bc430feb79cbea73d2cd` | `Printer::emit_comments_at_cursor_with_phase` | A6-37-1 |
| `emitTrailingComments` | 121176–121178 | `de3cf762696470a95f312618ffc50f62fe76b80a96420d707543b4b257729fb5` | `Printer::emit_deferred_expression_trailing_comments` | A6-37-1 |
| `emitTrailingComment` | 121179–121190 | `14654b8999872d42159a4d2c11a27fb01fbe47c50e8131b82de8453536d60394` | `emit_same_line_trailing_comments` | A6-37-1 |
| `emitTrailingCommentsOfPosition` | 121191–121198 | `953cde198b7f8098bd7bc8d865e535cdac106e983efe35a4a067498ff239cbc0` | `emit_source_trailing_comments_of_position_with_filter` | A6-37-1 |
| `emitTrailingCommentOfPositionNoNewline` | 121199–121207 | `36e4838b752b5c85052462eef98deb3651bdb4515eb2ba9452097536142f8bc9` | `emit_source_jsx_trailing_comments_of_position` | A6-37-1 |
| `emitTrailingCommentOfPosition` | 121208–121218 | `78fd8227de7e58556e3f2906ffe5aafb334a35d70f8dc0f916bed71b16cb78ea` | `emit_source_intervening_comments_of_position` | A6-37-1 |
| `forEachLeadingCommentToEmit` | 121219–121233 | `2e1fb613c9b9bb29f94a866a92b31e77c392cd696ee93b2e61a001e4e3981a9c` | `Printer::parent_comment_container_owned_prefix_for_owner` | A6-37-1 |
| `forEachTrailingCommentToEmit` | 121234–121238 | `bd6612ac9040b10e756e1b9666a34daebcc0fe66a227dfda01db103bfd3f27a7` | `CommentEmissionScope::retains_end` | A6-37-1 |
| `pipelineEmitWithSourceMaps` | 121277–121282 | `0c57cdceae760b1c5e0c2fca17706537034621460cc030cb1800d55c8c1e3efe` | `Printer::emit_transformed_node` | A6-37-1 |
| `emitSourceMapsBeforeNode` | 121283–121293 | `ac346b41706c68dd97ca01be4df11b5db57df063dfb4a1b84ee9b5ac1efa2517` | `Printer::record_node_map_boundary` | A6-37-1 |
| `emitSourceMapsAfterNode` | 121294–121303 | `6ca767d42995b2ea08ceccbb1261345452b6f976a0665d60a80c68fa14e0aeb6` | `Printer::record_node_map_boundary` | A6-37-1 |
| `emitTokenWithSourceMap` | 121333–121351 | `1f4c5a048470151a92b7a92ff32a976744e5222fb62cfa7c9b3e3964bde39732` | `Printer::record_token_map_side` | A6-37-1 |
| `emitListItemNoParenthesizer` | 121393–121395 | `67cbfa522b4cf25269d17dbf5e84287d3b750b0669d53505c5a97e9258c725c0` | `Printer::emit_node_with_hint_and_source_comments` | A6-37-1 |
| `emitListItemWithParenthesizerRuleSelector` | 121396–121398 | `d79c2b4f42d8c5dc08bb181cc25a1a3533e5818d5fa184d40656d86e198efa43` | `EmitContext::for_child; Printer::context_parentheses` | A6-37-1 |
| `emitListItemWithParenthesizerRule` | 121399–121401 | `590d4028ab14eb7b519699733113bfaaa8150af4cd08c79a593d938f32d30036` | `EmitContext::for_child; Printer::context_parentheses` | A6-37-1 |
| `getEmitListItem` | 121402–121404 | `7f1c03120629151133ef66fc2e4f98648f6bdfdab9417a83c9fc13243d9177c0` | `EmitContext::for_child; Printer::context_parentheses` | A6-37-1 |
| `emitIdentifier` | 117806–117814 | `8f07175bbf11451c8df83e4d29e195bd70c71c3963d4ef806fddae7c720ccfec` | `Printer::emit_transformed_node_worker (Identifier)` | A6-37-1 |
| `emitNodeWithWriter` | 119839–119845 | `ac88db15d31f74f81c507aec882699f3d2d9a483d3977861283f50cf58a1ea73` | `Printer::emit_node_with_hint_and_source_comments; TextWriter::write_parameter` | A6-37-1 |
| `writeBase` | 120162–120164 | `6e8bfa0c4681bf581398ca338d944524471d6e4107487a8f3424c81efbede165` | `TextWriter::write` | A6-37-1 |
| `writeSymbol` | 120165–120167 | `a6f94848eec1f662a52c4f31f4f2f3c6569abaa94710ad20a5769662c6327f70` | `TextWriter::write_symbol` | A6-37-1 |
| `writeParameter` | 120180–120182 | `5689e2b164c766b0213f62cc5527d80bcd862b2e284bf6501d5c4d18fd349471` | `TextWriter::write_parameter` | A6-37-1 |

The native admission path has seven trusted-base references at lines 448, 524, 590, 15704, 15713, 15751, 15752. Its raw-byte ellipsis helper has no upstream emission counterpart; ordinary AST owners handle the admitted sources.

## Current architecture references

Predecessor rows retain their own validation scope. The A37 row is qualified
only for the focused complete commands and direct controls recorded below.

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
| `E-COMMENT-ELLIPSIS-A37` | Ordinary rest token/name/type children consume complete source-comment phases with explicit hints; JSX token resumes and inherited suppression preserve brace maps. The raw ellipsis source-text admission guard is retired after complete focused command parity. | Private `crate::printer::{Printer::emit_optional_ordinary_child,Printer::emit_source_leading_token_with_context,Printer::emit_token_with_comments_at_boundary,PositionCommentPhase}`; existing `EmitContext`, `DeferredExpressionSourceComments`, `TokenEmission`, `CommentResume`; `builtins::preflight_source` | `active-qualified` for the A6-37 focused printer/admission profile (2026-09-10). General node-phase migration and the separate map/accessor/module-cursor owners remain open. | [A6-37](h2-8a-ellipsis-comment-owners.md):390/398 complete commands exact twice in each of two independent jobs (97 repairs/293 prior positives/8 unchanged outside failures);144 new metadata cases and96/32 prior direct controls exact twice;494 emitter units/451 contracts with1350 frozen declaration reprints pass. |

`EA-GAP-FLAGS` and `EA-GAP-CAPTURE` retain their separate emitter-wide lifecycle
and capture owners. A37 consumes existing local source scopes; it does not grant
arbitrary factory metadata or callback admission.

The A37 fixture adapter adds only the existing numeric `jsx` option mapping after
`module`. The A34 readiness checker retains its original preserveConstEnums delta
and validates this later single insertion with separate hashes. This updates the
fixture input bridge without changing production options or historical outputs.


## Subsequent JSX kind comparison repair

The first runtime candidate command job reached all398 inputs. It has378 primary
cases exact twice,12 primary assertion failures caused by the test's JSX kind
label, and8 complete command divergences in the predeclared outside group. All293
before positives remain exact. Each of the12 JSX preserve failures has only one
supplemental command; that whole command equals the unchanged TypeScript fixture.
These single observations do not qualify the required independent after jobs.

The original observer labels `.jsx` output `javascript`, while the older shared
primary comparator labels it `jsx`. The shared comparator now accepts the original
label only when the actual artifact kind is JavaScript and its exact path has the
`.jsx` extension. The existing exact path, callback/materialized bytes, callback
metadata, diagnostics, maps, status and exit comparisons remain in force; old
fixtures expecting `jsx` still take the original path. No production code or
fixture expectation changes for this repair. The after record archives the completed
candidate log/captures; the JSX kind amendment receipt preserves exact comparator
and authority preimages. The two fresh complete398 jobs below fulfill the
independent after requirement.


## Qualified after observations

The [after record](../../../../ratchets/h2-8a-ellipsis-comment-owners-after.v1.json)
has390/398 complete commands exact twice in each of two independent processes:
all97 intended repairs and all293 previous positives. All80 raw-guard candidates
now match complete commands, including output text, maps, callback metadata,
diagnostics, status and exit. There are1576 primary executions and1576 separately
counted supplemental complete commands across the two qualifying processes; all
supplemental tuples, including the8 failures, agree between processes.

All144 direct metadata controls match twice, repairing39 prior failures and
preserving105 positives. The96 prior modifier/spread and32 class-header metadata
controls also match twice. The unchanged494 emitter unit IDs and451 contract IDs
all pass, including all1350 frozen declaration reprints. These direct reprints
and metadata prints have separate denominators from complete Program commands.

The12 original JSX preserve primary assertions in after-1 failed because of the
test kind label described above. That candidate and its singleton supplemental
observations remain archived and do not count as either qualifying process.
Both qualifying processes use the corrected shared comparison adapter. The
original TypeScript fixtures, their bytes and diagnostic controls are unchanged.

The8 outside commands remain red:3 ES2015 object-rest source-map cases,4 retained
auto-accessor producer/map cases and1 recovery module-head cursor case. Every
complete failed tuple is unchanged from the frozen before runs; there are no new
component regressions. These8 failures explain each focused process's exit101.
Their continued comparison is not admission credit and does not close their owners.

No global769 or class1228 total is inferred. This qualifies the focused printer
and ellipsis admission prerequisite only. General source-comment migration,
remaining transform producers, H2.8a-e completion and hosted acceptance before
landing remain open; the historical full developer-CI/certificate walk is omitted.
