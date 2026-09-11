# H2.8a A6-28: class-field substitution and sequence positions

All original A28 source owners remain qualified on the train. The subsequent
[helper and accessor producer qualification](h2-8a-class-helper-accessor-producers.md)
brings the original inventory to 412/492 exact twice, repairing 16 formerly
outside helper/setter-map cases. All 192 original owned repairs and 198 original
positives remain exact, along with six previously repaired ES2022 cases.
The other 80 original outside cases, H2.8a global and B–E remain open.
All 494 emitter units and 451 contracts pass with unchanged test IDs.

Historical milestones remain immutable: first candidate 362/492, initializer
comments 370/492, promoted/export maps 392/492 and independent comment endpoints
396/492. A6-29 has its own three-file scope and before/after evidence; it does
not rewrite the original A28 scope or its minimum390 requirement. The following
design and before record retain their historical counts and source ownership.

Runtime slice at base `dc868373612af16ad299ed08bdec4ece437325dc`, on the H2.8
train with trusted base10748f6ee19ec083ce5748224930c5dcfbbd86df. A27 leaves four
A22 class-field-alias map differences. This slice repairs their producer ranges
and the corresponding field, block and expression-context branches. Production
is limited to crates/emitter/src/builtins/class_fields/downlevel.rs. Other A
owners, B–E, ES5 static-this phase composition, helper request order, retained
ES2022 fields/accessors, nested computed-name environments and legacy default
class naming remain separate owners. No public API or metadata format changes.

Fresh witnesses are384 complete commands:32 shapes xES5/ES2015/ES2022
xCommonJS/ESNext xassignment/define fields. Add all108 A22 assigned-name
commands, including A27's repaired new.target controls. Strict checking, ordinary
libraries, declaration output, applicable JS/DTS maps, CRLF and outDir remain.
Every command compares diagnostics, exact bytes and paths, write order and
metadata, result presence and status, emitSkipped and exit. Mint and independent
--check each run every fresh TS command twice and produce identical log hashes.
Diagnostic occurrences are5107:128,4094:12,2695:8,2465:8,2339:8,2816:16.

The initial native job rejects36 legacy cases at the adapter's unrecognized
experimentalDecorators option before Program construction. Those cases execute
zero commands; its old adapter, source, captures and real exit101 are immutable
in the first-before record. The adapter then accepts the existing bool option
without changing assertions, fixtures or production. All492 run in each of two
amended jobs:198 exact twice/job and294 fail once/job. Both jobs have identical
286 complete first map vectors and8 typed Block generated-name errors. Every
map differs only in mappings. The first complete job additionally captures
artifacts through690 cloned Programs; those are not complete actual tuples
beyond the first failed comparison. There are1380 primary commands plus690
captures=2070 for the two complete jobs. The earlier partial job has652 primary
and652 captures, giving3374 total native attempts across all three jobs.

The complete-before record initially proposes208 owned/86 outside. Subsequent
source inspection, still before any production edit, disproves the scratch
flag-preservation proposal: Es2015Transformer::transform_root invokes
initialize_transform_flags, which recomputes flags from the eager AST and
overwrites a replacement Identifier's ContainsLexicalThis. Preserve that
initial proposal and the explicit correction artifact in readiness. Move the
eight typed ES5 field-arrow/static-block-arrow cases, four ES5 concise-arrow
cases and four ES5 legacy-static-block cases to the next static-this phase
owner. That follow-up must combine actual late-field/early-bound-block phase
semantics with the generated-name Identifier-kind guard. A28 does neither.

The final gate has192 owned failures,198 adjacent exact and102 outside. Outside
membership is58 retained ES2022,16 private/auto-accessor helper order,8 nested
computed-name environment,4 ES5 legacy default naming,16 ES5 static-this phase.
Require at least390/492 complete commands exact twice after, with every owned
case and every prior exact control included. All comparisons remain
unconditional. Mixed-owner outside cases can change their first vectors when
owned ranges change; preserve their complete new vectors and named causes.
No partial tuple or first-vector improvement counts as an exact command.

The readiness manifest pins70 whole TS declarations/bodies by lines and SHA256.
The classFields graph is visitClassExpression -> visitInNewClassLexicalEnvironment
-> getClassFacts -> visitClassExpressionInNewClassLexicalEnvironment ->
transformClassMembers and generateInitializedPropertyExpressionsOrClassStaticBlock.
The last class-expression worker returns factory.inlineExpressions without
assigning the class original or range to that outer expression. Only the
updated class child keeps the parsed class range, gets Indented, and each
operand starts on a new line. inlineExpressions reduces comma nodes at up to
ten expressions and creates a CommaList above ten. Factory parenthesization
copies a synthetic child's -1/-1 range to synthetic parentheses. Native
inline_class_expression instead attaches the whole class's original and range
to its comma or wrapper, producing extra maps and tail comments.

For a field, TS visitThisExpression leaves ThisKeyword unchanged. Later
substituteThisExpression selects classThis/classConstructor, clones it and
assigns original and text range from that actual ThisKeyword. For a bound
static block, visitThisExpression substitutes the existing synthetic binding
earlier; that replacement has no parsed this range. An invalid legacy receiver
stays ThisKeyword during the early visit, then becomes a synthetic parenthesized
void-zero during printing. Its replacement has no original/range. The native
eager projection can correct the final ranges and parentheses independently
of the separately unqualified ES5 flag phase. Preserve generated identity,
receiver selection, binding allocation, static-super policy and frame guards.

Arrows inherit StaticBindingFrames, ordinary functions/classes enter their
existing boundaries, and computed names retain their existing frame handling.
Do not change the computed-name environment rule here: the eight source-proven
differences remain under that owner. Generated static auto-accessor redirectors
use a synthetic binding directly in TS transformAutoAccessor and keep the
early synthetic policy. Named-evaluation transport blocks use the block policy.

For ordinary static blocks, transformClassStaticBlockDeclaration creates an
IIFE via createImmediatelyInvokedArrowFunction. It assigns only original
identity and AdviseOnEmitNode to the callee arrow, then original AND raw range
to the CallExpression. The sequence caller later sets expression sourceMapRange
past modifiers and commentRange to the member. Native materialization currently
assigns original/range to the outer statement but omits the call's raw range.
Keep all existing expression/statement source-map and comment-range overrides;
attach the missing raw call range at its producer. No printer-map or comment
deduplication change is required by this slice.

| Semantic value | Native owner, lifetime and observable consumer | Gap and action |
| --- | --- | --- |
| synthetic comma and parentheses | DownlevelClassVisitor::inline_class_expression -> TransformArena -> ordinary Printer; per transformed tree | stale original/raw range; A6-28-1 |
| class child identity and indentation | existing class update and metadata; per tree; assigned_name and Printer consume it | unchanged; A22 names and expression contexts |
| bound static receiver and replacement stage | private StaticBindings inside RAII StaticBindingFrames; push/pop per lexical evaluation | add private Early/Emit stage for final range policy; A6-28-2 |
| actual ThisKeyword provenance | existing set_original_and_range -> arena original + raw source range; persists with replacement | copy to bound Emit-stage identifier only; A6-28-2 |
| invalid legacy recovery | StaticReceiver::InvalidLegacyDecorated; per active frame | synthetic Parenthesized(Void0), no parsed range; A6-28-2 |
| block IIFE raw call position | StaticOperation::Block -> materialize_static_operations; tree lifetime | missing producer location; A6-28-3 |
| comments and map overrides | current member materialization + immutable Printer contexts and SourceMapGenerator | unchanged algorithms; full bytes/maps and490/451 regression |
| generated-name and ES5 flag consumers | target_bindings collector and ES2015 flag initialization; per composed tree | named outside owner; no inherited compatibility for16 cases |

A6-28-1: in inline_class_expression, retain existing Indented and operand
starts_on_new_line metadata and placement selection. Return Ok(expression)
for ExistingListContext; return create_parenthesized(expression) otherwise.
Remove only the two assignments of original/range to the outer sequence or
wrapper. Do not remove the original argument: it still selects placement.
Preserve class alias-assigned flags and every allocation/precedence decision.
The original four A22 cases, all expression contexts, explicit parentheses,
comments and >10 operands are the observable completion checks.

A6-28-2: add private StaticThisSubstitution::{Early,Emit} and carry it in
StaticBindings. static_bindings takes the stage; visit_optional_static_node
selects Emit for public/private static field initializers. visit_static_node
takes the stage explicitly; ordinary and named-evaluation static block callers
select Early. static_auto_accessor_bindings selects Early. In the ThisKeyword
visitor, a Bound receiver still creates the same binding Identifier; only Emit
copies the actual ThisKeyword's original and raw range. Early stays synthetic.
InvalidLegacyDecorated creates Void0 then ParenthesizedExpression regardless
of stage, both synthetic. Unbound this and every function/class guard are
unchanged. Do not add flags or change flag initialization. No new failure
boundary: existing factory/arena errors propagate through Result.

A6-28-3: in materialize_static_operations' Block arm, after creating the arrow
assign its original identity to the static member and add AdviseOnEmitNode,
without assigning a raw range to the arrow. Keep existing body multiline state
and parenthesization. After create_call, set_original_and_range(call,original)
before making the existing expression statement. Leave its current statement
metadata and downstream expression map/comment projection intact. Pin the
whole upstream transformClassStaticBlockDeclaration as the ledger owner.

A6-28-4: run all492 complete commands on the final candidate with no capture
clones. Every new owned failure or prior exact loss stops qualification: freeze
source, log and actual exit before amending readiness or production. Preserve
all outside first vectors and typed boundaries. Run all490 current emitter
library tests and451 contracts on freshly built candidate binaries, no ignored
or filtered tests. Record binary hash/size/timestamp and Cargo provenance; binary
bytes need not be copied. Archive before/after receipts and verify allowed
production paths before the local commit. No global total is inferred.

The18 architecture rows are mechanically pinned with current Rust symbols,
visibility, validation refs, evidence and lifecycle. E-POSITIONS,
E-METADATA-BASE, E-COMMENTS-G, E-COMMENT-SCOPE-H and E-ORDER-H require this
candidate's producer/consumer qualification. E-MAPS' dated dormant row is
research-only; current H2.6 map source and fresh observations supply evidence.
E-PRINTER-BASE, E-RESOLVER-BASE, E-ARENA, E-CONTEXT, E-ENTRY, E-PROTOCOL,
E-ORDER-G, E-NAMES-BASE, E-NAMES-CLASS-G, E-NAMES-H, E-HELPERS-BASE and
E-HELPERS-PROVENANCE-G keep their current implementation. Active-unqualified
composition and named outside name/helper branches are not inherited qualified
premises. The metadata/arena representation already carries all required
values; this introduces no architecture boundary. Candidate positions remain
unqualified until the after record; outside profiles remain unqualified.

Run node scripts/observe-class-field-alias-map-positions.mjs --check for TS,
then python3 scripts/check-class-field-alias-map-positions-readiness.py before
production. The manifests record exact Cargo argv/environment; compiler filters
are class_field_alias_map_positions and transformed_class_assigned_names.
CARGO_BUILD_JOBS=2, taskpolicy -b, nice -n15; no competing heavy builds or edits
to pinned inputs during execution. Read real exits before dependent actions.
No new unit test is needed: the complete fixtures directly expose all owned
branches, and the current full emitter suites protect shared consumers.
No source/path/case-ID branches, expected-output edits, normalization, broad
flag patch or unsupported-success fallback. All reachable rows have an action
or explicit outside owner: unresolved=0, undispositioned=0 within this bound.
The schedule's user-authorized lightweight workflow applies; final matrix
replay and hosted acceptance remain required. H2.8 A and B–E remain open.
