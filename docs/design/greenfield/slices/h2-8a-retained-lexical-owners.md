# H2.8a A6-40: retained lexical, computed and constructor owners

Runtime slice in research/design, **not implementation-ready**. Base
`1be68d07ed42fb58ee0ccce323f726fb2cc15d4b`, 2026-09-10. The full user goal is
H2.8a-e. This slice must implement the retained class-field owner's complete
generated-binding, lexical-environment, computed-name and initializer-result
behavior. Anonymous receiver replacement alone is insufficient.

The whole-file [Rust candidate v2](h2-8a-retained-lexical-owners.candidate-v2.rs.txt)
now compiles in an isolated workspace with `cargo check -p tsc-rs-emitter --lib
--offline`. Its SHA-256 is
`ab3363962f79229d4a10433f0c88126912efea062f61347e3a94eb38ccc39573`.
The [design-check receipt](../../../../ratchets/h2-8a-retained-lexical-design-check.v2.json)
freezes the exact candidate, all678 Rust/Cargo input pins, successful actual
exit0 and prior failed attempts. This checks types and API composition only:
production source is unchanged, that typecheck executed no runtime tests, and
no compatibility/admission/hosted claim follows from it. A subsequent isolated
runtime design experiment is described below.
The immutable [first candidate](h2-8a-retained-lexical-owners.candidate.rs.txt)
remains the preimage for earlier source-review evidence. Candidate v2 adds only
the51-line comma factory amendment described below; isolated attempt4 passed
in5.956 seconds without diagnostics.

Pinned TypeScript 6.0.3 defines semantics. The freshly qualified
[A6-39 producer packet](h2-8a-retained-field-producers.md) and its
[after profile](../../../../ratchets/h2-8a-retained-field-producers-after.v1.json)
provide the existing receiver-node, backing/accessor/static-block metadata,
factory-time flag, WillHoist and constructor/list raw-position boundaries.
Those boundaries have source SHA-256
`26de511b53ea232b073235a4507f703ae78cd4881c12bfea720f71f932dc03e7` in
`crates/emitter/src/builtins/class_fields.rs`. They do not qualify lexical or
generated-name behavior that this packet still needs to implement.

## Frozen current evidence

The [expanded before profile](../../../../ratchets/h2-8a-retained-lexical-owners-before.v3.json)
contains530 cases:393 exact and137 failed twice. SHA-256:
`b16b64964c0e8cd78d652d38efd9b303906a35147df73e32a67fc35c1ed0bed8`.
It preserves the immutable [522-case predecessor](../../../../ratchets/h2-8a-retained-lexical-owners-before.v2.json)
and adds only8 new comma factory controls; no522/530 baseline replay occurred.
Earlier observations combine454 A39 captures with52 lexical/constructor and16
edge controls. All277
predecessor source/Cargo pins match current bytes, and275 match the new job's
1038-pin manifest. The remaining two historical pins belong to the independent
`new-ci` workspace, which the compiler test does not build. The common comparator
and complete-capture code remain byte-identical; only registration changed.

| Population | Exact / failed twice | Execution provenance |
| --- | --- | --- |
| A39 retained/accessor/helper/alias/context454 | 360 / 94 | Reused frozen A39 complete captures;908 primary and908 supplemental executions, zero new executions |
| New lexical44 | 22 / 22 | New88 primary and88 supplemental native executions;88 fresh complete TypeScript executions |
| New private constructor-reference8 | 2 / 6 | New16 primary and16 supplemental native executions;16 fresh complete TypeScript executions |
| New lexical edges16 | 5 / 11 | New32 primary and32 supplemental native executions;32 fresh complete TypeScript executions; native binary bytes archived before any rebuild |
| New comma factory8 | 4 / 4 | New16 primary and16 supplemental native executions;16 fresh complete TypeScript executions; native binary bytes archived before any rebuild |

All new failures differ only in JavaScript and, where applicable, JavaScript
mappings, including the map JSON inside `emit_result`. Diagnostics,
declarations, status, exit products and typed-boundary behavior match. The four
TS1166 and four TS2695 diagnostics in computed-key controls remain in the
expected tuples. The16 edge controls also preserve108 TS1166,12 TS2464,48
TS2465 and48 TS2339 diagnostics across the unique observations. The8 comma
factory controls retain100 TS1166 and four each of TS2464/TS2465/TS2339. There are no
new upstream exceptions; the two historical
upstream exceptions remain outside complete-command equality credit.

The current owned repair set has80 failures, including every prior65 repair,
all11 edge failures and all4 comma factory failures. Preserve all393 prior positives
and all57 complete successor negatives whose earlier decorator producer still
belongs to A6-41. The intended after minimum is473 exact twice, with all530
commands still compared strictly and every remaining failure retained. This
is an implementation target, not a qualification claim. The original769 and
class1228 populations are separate; no new global total is inferred.

Reproduction selectors on the current test are `producer` (the original454),
`lexical` (44), `constructor-references` (8), `edges` (16), `comma-factory` (8),
and `all` (530), using
`TSC_RS_RETAINED_ACCESSOR_CASE_SET` with
`cargo test -p tsc-rs-compiler --test contracts --
retained_accessor_owners_match_complete_typescript_observations --nocapture
--test-threads=1`. Do not repeat a frozen before job merely to combine counts.

Fresh TS inputs and observers are
[lexical inputs](../../../../crates/compiler/tests/fixtures/retained-lexical-inputs.json),
[constructor-reference inputs](../../../../crates/compiler/tests/fixtures/retained-constructor-reference-inputs.json),
[edge inputs](../../../../crates/compiler/tests/fixtures/retained-lexical-edge-inputs.json),
[comma factory inputs](../../../../crates/compiler/tests/fixtures/retained-comma-factory-inputs.json),
[lexical observer](../../../../scripts/observe-retained-lexical-environments.mjs)
and [constructor observer](../../../../scripts/observe-retained-constructor-references.mjs),
plus the [edge observer](../../../../scripts/observe-retained-lexical-edges.mjs)
and [comma factory observer](../../../../scripts/observe-retained-comma-factory.mjs).
All use the unchanged complete TypeScript serializer, execute twice, and
preserve exceptions separately. Their frozen output paths use the corresponding
`retained-lexical-environments.json`, `retained-constructor-references.json`
and `retained-lexical-edges.json`, `retained-comma-factory.json`
fixtures. Do not hand-author or regenerate expected output to fit native code.

## Resolved semantic requirements

`getClassFacts` (_tsc.js96844-96898) requests a retained constructor reference
for an unnamed static auto-accessor at ES2022 without `classThis`. It also
requests one when a non-static private element has the resolver's
ContainsConstructorReference flag. The latter branch is reachable on named
classes and is not guarded by downlevel-private lowering. The new eight
controls prove that omitting it is an actual output discrepancy.

The class-declaration owner (96971-97045) allocates its reference before heritage/member visitation,
registers constructor-reference substitution, and emits an assignment after
the class. The observed declaration has `#self = () => _a` and `_a = Box`
after the class. The class-expression owner (97049-97129) can instead emit an
unassigned reference temp: its static redirectors use `_a`, while the private
self-reference remains `Box`. Preserve this pinned output; do not invent a
class-expression assignment to make the output appear more useful.

Receiver selection is `classThis`, then generated class constructor, then the
actual current class-name node. Existing names stay `TransformNode` identities.
Generated names use `TargetBinding` and `write_generated_metadata`; the final
name pass owns ordinary spelling. Never turn a parsed receiver into a String,
recognize a generated identifier from an underscore prefix, or encode fixture
IDs/paths into production behavior.

`visitParameterList`91168-91181 lowers defaults after parameter visitation and
before body visitation. Computed method/accessor names are evaluated in the
outer environment, before entering the parameter/body environment. Retained
function dispatch must preserve this phase boundary for all seven function
kinds. The existing downlevel wrapper visits the whole function before lowering
defaults and is not an exact premise for this retained implementation.

`addDefaultValueAssignmentForInitializer`91239-91276 clones identifier nodes,
puts NoSourceMap on the assignment target and initializer, preserves the
initializer's existing flags, and assigns the parameter's raw positions to the
assignment and guard block. The assignment has NoComments, not whole-assignment
NoSourceMap. Binding-pattern defaults use one generated parameter identity and
an initialization statement. `convertToFunctionBlock`20665-20672 gives both the
return statement and block the concise expression's raw range.

`mergeLexicalEnvironment`24889-24932 orders standard directives, hoisted
functions, hoisted variables and custom initialization statements; it preserves
statement-array ranges and trailing-comma state. A source-root prepend helper
cannot replace it. The new binding-parameter control observes parameter `_a`
followed by body declarations `_b, _c`, even though the first body reference was
allocated while transforming the parameter initializer.

`visitIterationBody`91291-91305 owns a block scope around each iteration body,
not its header. The resolver's BlockScopedBindingInLoop bit chooses `let`
there; an ordinary anonymous static accessor inside a loop still uses a
function-level `var`. Existing computed-instance-plus-static-accessor controls
exercise the positive branch. The native `TransformationContext` already has
typed lexical/block start/end and hoist/add APIs; their implementation must be
used or mapped explicitly before introducing another scope stack.

`transformAutoAccessor`96256-96293 allocates a non-inlineable computed-key temp
before visiting the key expression. An existing cache is reused only through
the source-owned generated-assignment predicate. Cache temp and assignment
receive the expression's source-map range. Backing/getter/setter results then
pass through `accessorFieldResultVisitor`96091-96102. The backing initializer
is handled by the private-field owner; redirectors use function environments.
The current eager generic property walk and direct expansion bypass both
ordering requirements.

Constructor initialization uses the source properties in their source-defined
parameter-property/non-parameter order. `transformConstructor`97253-97289 first
visits the existing constructor, then, when WillHoist applies, opens its second
parameter/body environment and produces initializers in that environment.
`transformConstructorBody`97329-97431 preserves the raw positions already fixed
by A39. Moving field visitation out of that constructor environment would put
its generated declarations in the wrong function.

## Rust design decisions to carry into the executable packet

| Semantic row | Current gap | Required representation and consumer |
| --- | --- | --- |
| Visit result | Class declaration prologue currently refuses; postfix constructor assignment has no array result | Private one-or-many statement outcome; statement arrays splice it, embedded statement positions lift it to a block, expression positions reject a statement list |
| Stateful visitation | Global NodeId/NodeArrayId memo tables ignore lexical/constructor phase | Remove those obsolete caches; preserve only source-owned generated-binding identity, not arbitrary visited subtrees |
| Generated identity | `hoisted_names` and `next_temp_name` are source-global Strings | `TargetBinding` plus existing scope-aware allocation/finalization; actual declaration placement remains separate |
| Lexical products | Only source-root hoisted declaration installation exists | Existing `TransformationContext` lexical environment, typed phase restoration on Result errors, source/function environment materialization |
| Loop products | No retained block-scope placement | Existing context block-scope protocol around each body and resolver-selected ownership; typed error outside a legal iteration scope |
| Class facts/receiver | Retained visitor lacks resolver/reference allocation and named-private branch | Per-class facts and actual receiver nodes; generated constructor aliases use the existing print-time alias registry with generated identity |
| Computed cache | Eager name visit, unconditional second cache and initializer-style metadata | Source predicate, allocation-before-visit, explicit expression map range and getter/setter original-name updates |
| Expanded accessor | Backing and redirectors bypass their result visitors | Exhaustive backing/getter/setter result processing; ordinary field and function owners consume each once at their semantic phase |
| Function header/default/body | Generic walk ignores phase-specific hoisting | Header/name outside the body scope; visit/lower parameters, then body; cloned provenance and explicit prologue merge |
| Constructor fields | Initializers are materialized after generic constructor visitation in the surrounding scope | Materialize source field plans inside the constructor's source-defined lexical phase; preserve the A39 raw-position producers |

The production boundary now also needs `crates/emitter/src/printer.rs`: the
isolated experiment below found a missing `CommaListExpression` worker. Its
direct controls additionally require `crates/emitter/src/factory.rs` for the
source-owned parenthesizing of comma-delimited expression arrays. The
class-field candidate remains confined to
`crates/emitter/src/builtins/class_fields.rs`. Do not edit these production
file before the amended whole-slice gate passes. Reuse the existing context and generated-binding types where
their complete semantics match; do not import downlevel extraction or its String
receiver representation. Any required additional file must be named and gated
before an edit. Standard-decorator handoff/private-static/descriptor/receiver/
naming/super remains A6-41, with the complete57 guarded negatives preserved.

## Concrete staged implementation and checks

The candidate is a complete replacement for the one allowed production file,
not fragments requiring an implementer to invent missing call sites. Its
3904 lines include the existing transformer/downlevel dispatch and substitution
consumer, explicit retained state,32 reused helper bodies, and the following
new composed owners. All earlier frozen observations remain unchanged.

| Order | Candidate owner | Input, output and completion boundary |
| --- | --- | --- |
| 1 | `ClassFieldsVisitor::new`, `RetainedClassFrame`, `RetainedClassFacts` | Supply resolver and alias-registry borrows; replace arbitrary node/array caches and source-global temporary strings with generated identities, source-name caches and per-class state. Parsed private names reserve the source collision domain. |
| 2 | `allocate_binding`, `materialize_lexical_environment`, `merge_lexical_environment` | Keep generated identity separate from declaration placement; consume context Identifier hoists into source-ordered prologue products. Every source/function owner restores its outer environment on Result propagation. |
| 3 | `visit_function_with_name_visitor`, `visit_retained_parameters`, `visit_iteration_body` | Evaluate headers outside function scopes; visit and lower parameters before bodies; handle all seven function and five iteration kinds. Input static blocks own a separate lexical environment. Generated initializer blocks are not revisited. |
| 4 | `retained_class_facts`, `visit_class_declaration`, `visit_class_expression` | Allocate constructor references before heritage/member visitation, preserve receiver nodes, register declaration aliases, and return declaration prologue/class/postfix statements. The retained expression branch preserves the source's potentially unassigned constructor temp. |
| 5 | `property_name_expression_if_needed`, `retained_auto_accessor_names`, `visit_class_member_name`, `flatten_retained_comma_elements` | Recognize generated cache assignments before expression visitation; separate source-name identity from hoist timing; preserve raw expression maps and class-only pending-expression injection. For more than ten operands, flatten eligible comma elements exactly one level before NodeArray creation. Ordinary object-literal method names use the ordinary visitor. |
| 6 | `prepare_retained_private_storage`, `retained_private_storage` | Preallocate generated private member names after heritage and before member bodies; share their source-name identity with constructor initialization and redirectors. Preserve escaped source spelling, collision separator behavior and private `_i`/`_n` skipping. |
| 7 | `transform_retained_members`, `transform_retained_auto_accessor`, `transform_retained_field` | Process backing/getter/setter results through their owners. Insert synthetic constructors/static blocks after structurally verified classThis and assigned-name blocks, preserving the source member-array positions. |
| 8 | `transform_retained_constructor`, `visit_constructor_super_path`, `retained_property_initializer` | Revisit the existing constructor first, then materialize source fields in its second lexical environment, parameter properties first. Preserve prologue/super-path order and exact raw/comment/map/flag products. `extends null` is not derived. |
| 9 | `RetainedVisitOutcome`, `visit_statement_lifted`, `NodeDataChildVisitor` | Splice declaration result lists in statement arrays, preserve a single embedded statement, lift longer lists to a Block, and reject a statement list in an expression position. Source declaration files bypass this visitor. |

The design check found six compile errors: the helper-name enum's module path,
raw NodeFlags representation, missing EmitFlags BitAnd, and two calls to a
nonexistent TransformFlags method. All were corrected using existing APIs;
the subsequent check passed without diagnostics. The first launcher attempt
failed before starting Cargo because taskpolicy was addressed at the wrong
absolute path; that failure is retained separately from the two Cargo checks.
No shared production API or dependency was changed to make the draft compile.

Further source review also resolved two previously imprecise producer details:
`moveRangePastModifiers` uses **property.name.pos** for properties/methods,
not the last modifier's end; and `liftToBlock` preserves a one-statement result.
Both are reflected in the typechecked candidate. The ordinary generated-name
finalizer remains the spelling authority; temporary provisional strings never
become the generated identity.

The16 edge controls freeze four themes at ES2022/ESNext and set/define. BigInt
computed names require caches because `isSimpleInlineableExpression` excludes
BigIntLiteral; the same predicate includes keyword kinds. File-wide parsed
private-name collisions force `#value_2_accessor_storage`, while escaped source
spelling produces `#\u0076alue_accessor_storage`. Sixteen computed accessors
after an unrelated `#_a_accessor_storage` reserve `_b` onward and skip `_i` and
`_n`. Twelve pending computed field names exercise the greater-than-ten
CommaList branch, including a function-local captured lexical receiver and
the return comma expression's unparenthesized printer context. Their fresh
before tuples confirm that all11 new failures belong to these retained owners;
the other five complete tuples are adjacent preservation controls.

## Source review checkpoint and comma factory amendment

The reproducible [source inventory](../../../../ratchets/h2-8a-retained-lexical-source-inventory.v1.json)
contains203 selected seeds and their nested callbacks:220 whole owners,
960 predicates and1436 call sites. Its
[observer](../../../../scripts/inventory-retained-lexical-owners.mjs) derives
the nearest registered parent from AST ancestry after registration, eliminating
the earlier seed-order dependency. Call order is explicitly AST syntax order;
it is not used as proof of execution order or callee resolution.

The historical [source review](../../../../ratchets/h2-8a-retained-lexical-source-review.v1.json)
maps five reviewed requirements to exact candidate function spans, source
owners and frozen witness IDs: function/default phases, lexical products,
iteration phases, computed caches and private-name domains. These are reviewed
semantic invariants, not blanket qualification of all branches of each general
source owner. Verify its frozen candidate and mappings with
`python3 scripts/check-retained-lexical-source-review.py`; the check explicitly
reports **NOT implementation-ready**.

Review found `A40-F-COMMA-FACTORY`: `inlineExpressions` calls
`createCommaListExpression`24382-24387 for more than ten operands. That factory
first applies `flattenCommaElements`24371-24381 once per supplied element.
It expands only a synthetic, non-parse node without `original`, `emitNode`, or
a materialized source `node.id`; a comma binary expands to its two operands,
and a comma list expands to its elements. This differs from recursive
`flattenCommaListWorker`, used when collecting pending erased-field effects.
The first candidate omitted this operation. The
[comma design amendment](../../../../ratchets/h2-8a-retained-comma-design.v1.json)
closes that design finding with `flatten_retained_comma_elements` in candidate
v2. All other candidate bytes are unchanged, as checked by the exact source edit.

Eight fresh direct TypeScript factory observations are embedded in the review.
Plain binary, nested binary and comma-list inputs expand one level; an original
node, emit metadata, materialized ID, parsed provenance or a source range each
preserves the operand. These are factory research observations, with zero
Program/native commands and no equality credit. Rust's always-present arena
`NodeId` cannot stand in for TypeScript's lazy `node.id` guard.

The [source supplement](../../../../ratchets/h2-8a-retained-comma-sources.v1.json)
adds15 whole helper owners, enumerates all32 direct `getNodeId` sites and maps
40 pre-retained identity requests. The admitted built-in producers request IDs
for declaration/binding/member/ComputedPropertyName keys, not the comma
expression inside a computed name. Checker queries project parsed/anchored
nodes; module and printer consumers run later. Thus the source lazy-ID guard
is true for eligible operands at this private retained boundary. Custom AST
injection remains API1.2; the manually assigned ID factory control retains its
source result without a native API equality claim. No shared API or additional
state was introduced. Native raw ranges, Synthesized flags and the absence of
the original/emit metadata entry implement the other source predicates.

Eight fresh complete Program witnesses now cover a generated comma-valued
computed member after12 pending field operands and the adjacent parsed-comma
case, each at ES2022/ESNext and set/define. Four define tuples match before;
four set tuples differ only in JS/maps. The generated case also observes the
inner class's lexical receiver helper hoisted inside the enclosing function.
Their diagnostics and declaration writes are preserved. Check this amendment
with `python3 scripts/check-retained-comma-design.py`; this verifies the design,
exact candidate edit, isolated typecheck and frozen before, not A40 readiness.

## Remaining readiness work

Before closing the readiness ledger, execute an isolated **design experiment**
over the frozen530 complete commands using the exact candidate v2 and unchanged
comparator. This extends the isolated typecheck with observable evidence for
the candidate's convergence; it does not authorize a production edit or confer
runtime qualification. Use the copied design workspace, separate Cargo target
directory, two build workers and one background job. Freeze all copied input
hashes, the candidate, actual exit, binary bytes, and both complete captures
per case. Compare the393 prior positives,80 required repairs and57 successor
negatives without changing any expectations. Any discrepancy becomes an
explicit design finding before readiness can close. Production still requires
the whole owner/architecture mapping and a fresh mechanical ready gate below.

Reproduce with `python3 scripts/run-retained-lexical-design-experiment.py N`,
using a fresh positive attempt number. Poll the existing process while it is
running. After a complete test execution, freeze its comparison with
`python3 scripts/analyze-retained-lexical-design-experiment.py N`.
Attempt1 ended with actual exit101 after426.003 seconds during compilation:
18 include sites could not find the sibling tests' root-relative witness
files in the copied workspace. It executed zero candidate commands and
produced no test binary. Its prelaunch, actual exit, full log and source archive
remain immutable. The runner now copies and hashes the statically referenced
root-relative include inputs. Attempt2 uses the same candidate and comparator
and completed all530 comparisons. Its frozen result is described below.

### Isolated experiment result and printer amendment

The [design experiment receipt](../../../../ratchets/h2-8a-retained-lexical-design-experiment.v1.json)
freezes attempt2 at candidate SHA `ab3363962f79229d4a10433f0c88126912efea062f61347e3a94eb38ccc39573`:
**471 exact /59 failed twice**, all393 prior positives preserved,74 of80
required repairs exact, and four additional complete repairs from the earlier
successor set. It retained1060 primary and1060 supplemental executions, both
complete tuples per case, the actual exit101, the executed binary bytes,
985 copied inputs and109 vendor inputs. The command took892.951 seconds.
Root production and copied compiler inputs remained unchanged during the run.
This is measured design evidence, not a qualified after profile or activation.

`A40-F-COMMA-PRINTER` remains open. The six owned failures are the ES2022/ESNext
set variants of `pending-comma-list`, `generated-comma-member` and
`parsed-comma-member`. Each now produces the source-owned greater-than-ten
`CommaListExpression`, then returns the same typed missing-printer-worker
error. The printer already recognizes comma-list precedence and grammar
parentheses; `Printer::emit_transformed_node_worker` lacks its emission arm.
Keep the v2 comma factory correction. Converting the list back to a binary tree
would avoid the missing owner and would not implement the source behavior.

The printer amendment must stage an exact dispatch arm and private expression
list worker in `crates/emitter/src/printer.rs`. Its source owner is
`emitCommaList`119780-119788, calling `emitExpressionList`120026-120028 and
`emitNodeList`/`emitNodeListItems`120029-120155 with format528. Map these inputs
before applying the patch: explicit Expression hint per element, comma then
space separators, no brackets or trailing comma, raw sibling/parent end
comparisons, child comment-range starts, inherited comment suppression, and
the optional one-line break/temporary indent for `startsOnNewLine` on the next
element. `getLeadingLineTerminatorCount`120268-120300,
`getSeparatingLineTerminatorCount`120301-120329 and
`getClosingLineTerminatorCount`120330-120360 define line behavior. The current
printer's `preserveSourceNewlines` state is unavailable and false; format528
does not inherit a parent's MultiLine flag through `emitList`. Reuse typed
`EmitContext`, `CommentCursor`/position phases and ordinary expression emission;
do not substitute the existing declaration-list writer's Unspecified hints
and comment phases. Freeze focused source-produced controls and the exact
staged printer patch, then compare the six failures and adjacent cases again.

The additional four exact commands are `legacy-bound-this` and
`legacy-invalid-this`, ES2022 set, each in CommonJS and ESNext module form.
Their JavaScript is unchanged; the complete source maps now match.
`getClassFacts` retains ClassWasDecorated, and `transformProperty`97488-97500
sets original/AdviseOnEmitNode/name map metadata for static expressions when
class facts exist. `retained_class_was_decorated`, `RetainedClassFacts::any`
and `retained_property_initializer` now compose those owners. Keep their old
before dispositions immutable and record the four measured repairs separately.

The other53 failures remain in the predeclared successor population. Twenty
of the57 successor tuples changed from before, including the four now exact.
The receipt retains all changed fields so remaining mismatches can be reviewed
against both TypeScript and their complete prior tuples; a failed case's
unchanged status is not proof that all its output bytes were preserved.
No candidate adjacent emitter-suite run or hosted acceptance has occurred yet.

#### Staged comma printer worker

The [exact printer patch](h2-8a-comma-printer.candidate.patch) adds one dispatch
arm and three private methods. It applies only to printer base SHA
`8951dc94e07df2cfdca2f41a7b82e4c9ceeae75d6d5ff60ae5dabb752db53398`;
its SHA is `f8a30aca24079b013b1396af6ffb769eabe8509af700e4f63f1fdf4f2ce8b90c`
and the resulting printer SHA is
`9fa6f4729211ac6a1fc180a1c86c7bcbe132c13a565a89dbc3782d689c96ed09`.
Class candidate v2 is unchanged. This remains an isolated design candidate;
the whole A40 readiness gate below is still open.

The [printer source supplement](../../../../ratchets/h2-8a-comma-printer-sources.v1.json)
pins26 whole owners, their source bodies, calls, and predicates. Reproduce it
with `node scripts/observe-comma-printer-sources.mjs --check`. This supplements
the earlier class owner inventories without claiming that their remaining
architecture and semantic dispositions have been completed.

| Source input / transition | Concrete candidate owner and behavior | Focused witness |
| --- | --- | --- |
| `emitCommaList` -> `emitExpressionList`, format528 | Worker dispatch -> `emit_comma_expression_list`; explicit `EmitHint::Expression`, `ExpressionSyntaxContext::NORMAL`, existing hook/substitution/source-map pipeline | All50 direct controls; six frozen complete Program failures |
| Absent or empty children | `Option<NodeArrayId>` returns empty output for None; `TransformNodeArray::new` and arena validation distinguish an invalid present array from an empty one | `empty`, `single` |
| CommaDelimited / SpaceBetweenSiblings; all other format bits absent | Snapshot child IDs in order; no brackets, inherited MultiLine, element parentheses, trailing comma, leading or closing line break | `trailing-comma`, `parent-multiline-ignored`, `nested-comma`, `grammar` |
| `previous.end != parent.end`, NoTrailingComments | `emit_comma_element_end_comments` reads raw ends and flags, then invokes only `PositionCommentPhase::SourceLeading` with the ambient containerPos guard | `end-leading`, `parent-range`, child flag controls |
| Child comment range before child emission | `emit_comma_element_start_comments` reads `CommentRange` independently from raw/original/map range; inherited `CommentEmissionScope::retains_end` guards trailing readers | `parsed`, `synthetic-comment-range`, parent flag controls |
| `getStartsOnNewLine(next)` | Per-next-child `EmitMetadata::starts_on_new_line`; increase indent, optional positional comment phase, line break, ordinary expression, decrease indent; first child has no separator | `next-line`, `first-line-ignored`, `synthetic-next-line` |
| Raw child.pos synthesized guard on the pre-newline phase | Test raw position before consulting a possibly retained comment range; normal no-newline phase still consults that range | `synthetic-next-line` vs `synthetic-comment-range` |
| prefixSpace / forceNoNewline callback selection | For528, prefixSpace wins: filtered trailing comments receive a prefix whenever not at line start; otherwise use the existing non-prefixing intervening-comment callback | `next-line`, `line-comments` |
| commentsDisabled / NoNestedComments | Inherited immutable `EmitContext`; both list-position helpers stop under nested suppression, ordinary child phases retain their existing owner | All retain/remove pairs and parent/child flag controls |
| Failure and temporary state | Arena/range/hook/printer `Result` propagates; temporary indent restores before propagating a child-phase error; no new mutable global state or public API | Typed arena operations; full six-error comparison pending |

The format528 specialization does not allocate transform nodes, modify flags,
or change generated identities. Ordinary child emission owns nested comments,
source maps, hook ordering and parenthesization. The enclosing statement-list
owner retains detached-prefix consumption; before production readiness, its
non-reentry proof must be included with the existing comment-scope premise.
`preserveSourceNewlines` remains unavailable/false in this admitted printer
configuration. Direct metadata observations exercise the private design and
do not activate the separately owned custom-transform Program API.

The [source-produced fixture](../../../../crates/emitter/tests/fixtures/comma-list-printer.json)
contains50 cases, each printed twice by pinned TypeScript, with complete text,
UTF-8 bytes and ending UTF-16 writer location. Its SHA is
`5fc4aaa95b65aa1d2135aaeb9e4d5a36c6ddae7ea18fe38533b3f8f654a5c33b`.
Reproduce with `node scripts/observe-comma-list-printer.mjs --check`.
The native direct contract executes the same recipes twice and compares every
recorded field; expectations are never generated from native output.

`python3 scripts/run-comma-printer-design-experiment.py N direct` executes the
direct contract in the existing isolated workspace; selection `all` executes
the unchanged complete530 comparator. `edges` and `comma-factory` select its
existing16/8-case bands. Each fresh attempt pins copied and root inputs,
archives the applied patch and source bytes before execution, and retains the
executed test binary before any later Cargo command. Attempts are never
overwritten. One background heavy job, two Cargo workers, niceness15.

The [direct attempt1 receipt](../../../../ratchets/h2-8a-comma-printer-direct.v1.json)
freezes exit101 in182.698 seconds: **47/50 exact
twice**, three repeated differences. The failed controls are
`parent-no-trailing/retain`, `parent-no-own/retain`, and `parent-no-all/retain`.
Each loses `/* tail c */` outside the call argument's generated parentheses.
Keep all50 frozen expectations. The binary and both repeated differences are
retained in the attempt archive; this is a design finding, not a passing suite.

`A40-F-COMMA-ARGUMENT-COMMENTS` records the shared boundary involved:
`emit_call_arguments` invokes `emit_node_id_with_context` without deferred
outer trailing comments, then its explicit end-comment phase consults the
unparenthesized argument's NoTrailingComments. The virtual SourceRanged
parenthesis already has the correct `EmitFlags::NONE` owner, but no deferred
outer trailing phase reaches it. Upstream
`parenthesizeExpressionForDisallowedComma`20483-20488 creates a parenthesis and
copies only the text range, leaving child flags inside. This finding needs a
source-derived shared argument/parenthesizer disposition before qualification;
do not add a comma-case special condition or change expectations. Full
attempt2 uses the same frozen printer patch to measure the original six
complete-command failures and preserve all471 prior candidate positives.
After termination, freeze it with
`python3 scripts/analyze-comma-printer-design-experiment.py 2`.

The next isolated amendment fixes the factory boundary, where upstream creates
the parentheses before the printer sees a call argument. Add one private
normalizer to `NodeFactory::apply_parenthesizer_rules` in `factory.rs`; its
only consumers are `CallExpression` (including optional chains),
`NewExpression`, and `ArrayLiteralExpression`. These are all four callers of
`parenthesizeExpressionsOfCommaDelimitedList` in pinned tsc: array22441-22453,
call22579-22601, chain22602-22620, and new22621-22634. Their existing callee,
type-argument, optional-chain, and node update owners remain separate.

The exact helper sequence is:

1. Select the typed argument/element field. An absent new-expression argument
   array stays absent; absent call/array-literal lists become empty arrays.
   Validate every present source-scoped array and node through the arena.
2. For an array literal ending in OmittedExpression, apply the upstream
   `createNodeArray(elements, true)` step before mapping: keep an already
   trailing-comma array, otherwise clone its nodes/text range and cached
   transform flags with hasTrailingComma true. Do not change the caller's
   input array.
3. Visit each child once, left to right, using the existing private
   `parenthesize_expression_for_disallowed_comma`. It skips partial wrappers
   for precedence, preserves the original child when precedence is greater
   than comma, or creates a real ParenthesizedExpression with propagated
   child transform flags and the child's raw text range. Emit/comment/map
   metadata and original links stay with the inner expression.
4. Preserve the array identity if all children are identical. Otherwise create
   the mapped array with the input trailing-comma bit, recompute its child
   transform flags, and copy only the source text range. This is `sameMap` ->
   `createNodeArray` -> `setTextRange`, not mutation of the input array.
5. Install the resulting typed array ID in the selected field. Creation and
   structurally changed updates reach this normalizer; unchanged updates
   retain the existing identity fast path. Propagate all typed errors.

This makes the wrapper's existing printer/comment owner observable directly.
Do not patch the call printer to copy or clear the original argument's flags,
or special-case CommaListExpression output. Stage and typecheck the factory
patch outside production, then repeat the unchanged50 direct controls and add
source-produced call/optional/new/array and plain-binary-comma controls. The
full readiness gate must requalify the changed shared factory/parenthesizer
premise before any production activation.

Full comma-printer attempt2 has now completed and its
[immutable receipt](../../../../ratchets/h2-8a-comma-printer-design-experiment.v1.json)
records **477/530 exact twice**, all80 required repairs, all393 original
positives and all471 predecessor-candidate positives preserved. Exactly the
six formerly missing-worker tuples changed; each is now completely equal to
TypeScript. The other524 tuples are identical to the preceding candidate.
There are53 predeclared successor failures and zero typed printer failures.
Actual exit101,899.058 seconds;1060 primary and1060 supplemental executions,
987 copied inputs,109 vendor inputs, and the executed128177152-byte binary
are retained. This closes the six-case missing-worker finding as measured
design evidence, while the direct three-case factory finding and whole A40
readiness remain open.

The [factory patch](h2-8a-comma-argument-factory.candidate.patch) is65 added
lines against factory base SHA
`4c0ade2cd1a17a83bb9af5c0c53628a4f88017ef241ed6bb3e27a42aff1094f0`.
Patch SHA `4c4ba5f1406ce9b36ae076423a4422a72e91904f6f35edc86b34aa375e9c8039`;
applied factory SHA
`05f3fbd51cf98e3a3ec2e43bab4a5463b1d90a69d568234fc14f5681efe261c0`.
The [factory source supplement](../../../../ratchets/h2-8a-comma-argument-factory-sources.v1.json)
pins16 whole declarations including all four callers, sameMap, array creation,
precedence and parenthesis construction. Its reproduction command is
`node scripts/observe-comma-argument-factory-sources.mjs --check`.

Factory attempt4 (`4 direct --factory`) completed exit0 in7.681 seconds:
**all50 original direct controls now exact twice**. This resolves the original
three argument-wrapper comment differences with real source-owned factory
nodes, without editing the old call printer's flags or expectations.
The [factory direct receipt](../../../../ratchets/h2-8a-comma-argument-factory-direct.v1.json)
retains attempts3 and4, both executed binaries,989 copied inputs and109 vendor
inputs, the188 direct print attempts, and the separate state/output results.

The [additional44 source controls](../../../../crates/emitter/tests/fixtures/comma-argument-factory.json)
cover call, optional call, new and array construction with both binary and
CommaList expressions; source-ranged/synthetic operands, NoTrailingComments,
unchanged arrays, absent arrays and final omitted elements. The observer
`node scripts/observe-comma-argument-factory.mjs --check` compares two TS
observations, including array identity/text range/trailing comma, every
element's kind/raw range/flags/original presence, wrapper-child identity, and
the complete printed text/UTF-8 bytes/ending UTF-16 location.

Factory attempt3 (`3 factory-direct --factory`) completed exit101 in62.258
seconds:40/44 complete controls exact twice. The **factory-state fields match
for all44**, while the four `hole` controls print an extra `/* between */`.
Cargo stopped after that test target failed, so attempt3 did not execute the
original50; attempt4 supplies that separate observation. Keep the four source
expectations unchanged. `emit_delimited_trailing_comments_for_node`16115-16157
finds a comma after the previous element's end and emits its following source
comments even when the actual next child is a synthetic omitted element.
Upstream `emitNodeListItems`120068-120155 consults the **actual next child's
comment-range start**. Record this as open finding
`A40-F-LIST-INTERVENING-OWNER`; resolve its shared call/array-list owner or prove
an explicit scope disposition before qualification. Do not special-case holes.

Factory attempt5 (`5 all --factory`) completed exit101 in897.169 seconds.
Its [immutable receipt](../../../../ratchets/h2-8a-comma-argument-factory-design-experiment.v1.json)
freezes **477/530 exact twice**, all80 required repairs and all477 predecessor
positives preserved. **All530 complete tuples are unchanged** from the
printer-only candidate, including the53 successor failures. The128177440-byte
executed binary,989 copied inputs,109 vendor inputs, and1060 primary plus1060
supplemental executions are retained. No qualified-after or production
activation follows from these experiments.

Attempt6 (`6 emitter --factory`) ran the existing emitter library units and
`contracts` target against the same three staged sources. The runner now
archives both library and integration binaries and uses `--no-fail-fast` for
multiple emitter targets. The separate additional44 direct controls retain
their four known output failures; no test or expectation is skipped or
reclassified as passing.

The [adjacent emitter receipt](../../../../ratchets/h2-8a-comma-argument-factory-emitter-checks.v1.json)
now freezes attempt8 at **494 units /451 contracts /1350 declaration reprints
passing**, exit0 in19.170 seconds, with no warnings and both binaries retained.
Attempt6 had494/494 units and450/451 contracts: the retained-field contract
provided UnavailableEmitResolver, but source `getClassFacts`96844-96888 queries
ContainsConstructorReference on its retained private `#native` field. The
[fresh checker observation](../../../../ratchets/h2-8a-retained-accessor-contract-resolver.v1.json)
records that exact fixture's real TypeScript result as false, twice, together
with its emitted JavaScript. Reproduce with
`node scripts/observe-retained-accessor-contract-resolver.mjs --check`.
The contract now supplies the existing NoConstantValueResolver and retains
its complete expected output. Production resolver lookup and its typed
unavailable error remain intact. An unused import in that same test file was
removed. Attempt7 retained the same failed result because an edit guard failed
before writing the file; its repeated result and identical binaries are
preserved, not counted as repair credit. Attempt8 verifies the applied fixture
correction. Whole A40 readiness and hosted acceptance are still required.

### Actual next-item comment ownership design amendment

The new96 controls in `list-intervening-owners.json`, produced twice by
`node scripts/observe-list-intervening-owners.mjs --check`, vary four
containers, three comment layouts, four retained/reordered/synthetic child
recipes, and source-ranged versus synthetic arrays. Attempt9 freezes the
unmodified candidate:26/96 additional controls exact, the original40/44
factory controls exact, and50/50 comma controls exact. These are direct
factory/printer observations, not Program/custom-transform admission.

The next staged printer amendment replaces the implicit previous-node
argument of `emit_delimited_trailing_comments_for_node` with a private
`DelimitedCommentBoundary` enum. `BeforeItem(TransformNode)` reads that
actual item's `CommentRange` start, including explicit metadata overrides;
an absent synthetic start emits nothing. `AfterTrailingComma(TransformNode)`
keeps the existing final-token lane separate. The enum owns no mutable state
or cache and lives only for the immediate list callback. The returned
`CommentResume` identifies the same source/start and the emitted prefix end;
the next ordinary leading phase consumes it once. Arena, range and cursor
validation remain typed `PrinterError` boundaries. Comment suppression uses
the inherited expression context and the positional callback's container-end
guard, independently of the next child's NoLeadingComments flag.

Update all six shared callers: both ordinary expression-list branches,
JSON expression lists, parameter lists, named import/export lists, and call
arguments. For siblings, pass the actual next node, never a source-comma scan
from the previous node. Only a written final trailing comma uses the separate
token boundary. Synthesized call and named import/export arrays must consume
the returned resume instead of replaying the item's intervening phase. Keep
the containing call's ordinary leading-prefix ownership. Do not modify the
factory or class candidates, original fixtures, or root production sources.

Source authority is `emitNodeListItems`120068-120155, especially next-child
`getCommentRange(child).pos`120107-120125, and the distinct final-token call
120133-120141 to `emitTokenWithComment`118731-118764. The existing26-owner
printer inventory pins the list, range and comment callbacks. Array/object
callers118203-118222, call/new118275-118297, named imports119254 and named
exports119350, parameters119976-119990 determine the format and consumers.
This amendment resolves the choice of sibling comment owner; the whole A40
gate, full format/newline and final-token ownership audit still remain open.
Freeze the patch and run original50+44 and new96 together, then adjacent
emitter suites and the full530 comparator against the frozen477 predecessor.
Any new differences retain their exact observations as open design findings.

Attempt10 of that amendment is188/190 exact twice: original94/94 and new94/96.
The two remaining reverse-array/line-comment rows expose a distinct separating
line predicate. `getSeparatingLineTerminatorCount`120299-120327, with
preserveSourceNewlines=false and PreserveLines, compares the current raw
positions when the original nodes have the same parent; reversed positions
are valid. Array range synthesis does not disable this branch. Rust's older
comparable-gap helper rejects reversed ranges and its caller skips synthesized
arrays, so both rows omit the required line after the comment.

The v2 staged patch adds a private preserved-list sibling-line predicate for
both ordinary expression-list branches. It validates current source positions,
tests raw endpoint synthesis, compares original-parent identity only for the
upstream parent predicate, and uses `PositionIndex::line_and_character_byte`
for each endpoint independently. This preserves Unicode/CRLF line semantics
without an ordered gap scan or repeated whole-file scans. JsxText, missing
original parents, and synthesized startsOnNewLine/PreferNewLine follow the
source branches. Existing generic gap helpers remain with their other owners.
Additional source bodies read: nodeIsSynthesized16000-16002,
rangeEndIsOnSameLineAsRangeStart17352-17359, getStartPositionOfRange17367-17375,
synthesizedNodeStartsOnNewLine120398-120407,
originalNodesHaveSameParent121105-121108. Root sources still remain unchanged.

Attempt11 (v2) is190/190 direct controls exact twice, with all140 factory
states exact. The [direct receipt](../../../../ratchets/h2-8a-list-intervening-direct.v1.json)
retains attempts9/10/11, their immutable binaries, complete input pins, and
every failed tuple before repair. However, adjacent attempt12 is494 units
passing and450/451 contracts: the declaration reprint's existing
`P4/unfiltered-intervening-comments` row detects a regression. The proposed
prefix-space spelling of the ordinary sibling callback accidentally applied
onlyPrintJsDocStyle filtering. Source emitTrailingCommentOfPosition is
unfiltered; only the prefix callback selected before a separating newline
uses emitTrailingComment's filter. Preserve this failure and expectation.

The v3 patch passes the caller's separating-line decision explicitly into
the boundary callback. Ordinary same-line sibling comments retain the
unfiltered positional lane; before-newline and final-token callbacks keep
their source-defined filtering. Compute literal sibling line decisions once
and reuse them for both comments and the following newline. This is an
additional callback-policy distinction, not a case-specific exception.

The [v3 patch](h2-8a-list-intervening-printer.candidate-v3.patch), SHA-256
`e0b8c978b40d1dc19e834f25403681a35ebd8d0de11bea4d1f2caca119d1446d`,
applies after the original comma-printer patch and is the current list-owner
candidate. V1 and v2 patches remain immutable historical experiments.
Attempt13 (`13 factory-direct --factory --list-owner`) is **190/190 exact
twice**, including all140 factory-state observations, exit0 in30.498 seconds.
The [updated direct receipt](../../../../ratchets/h2-8a-list-intervening-direct.v2.json)
freezes the actual sources and both binaries. Attempt14
(`14 emitter --factory --list-owner`) is **494 units /451 contracts /1350
declaration reprint rows passing**, exit0 in53.575 seconds. Its
[emitter receipt](../../../../ratchets/h2-8a-list-intervening-emitter-checks.v1.json)
also retains the exact failed attempt12. The
[fresh P4 observation](../../../../ratchets/h2-8a-list-intervening-jsdoc.v1.json)
reprints the unchanged declaration input twice and matches its old expected
bytes; reproduce with `node scripts/observe-list-intervening-jsdoc.mjs --check`.
The [caller/token supplement](../../../../ratchets/h2-8a-list-intervening-sources.v1.json)
pins13 whole owners and the
[line-predicate supplement](../../../../ratchets/h2-8a-list-separating-sources.v1.json)
pins9 whole owners. Their corresponding observer scripts support `--check`.

`A40-F-LIST-INTERVENING-OWNER` and the measured reverse-array separating
positions are resolved for these direct controls. Full530 comparison of v3,
the remaining format/final-token/detached-prefix audit, whole A40 readiness
and hosted acceptance remain required. The full comparator's `--list-owner`
route now uses the stronger factory/printer477 predecessor receipt, verifies
all1060 old captures, and reports every complete tuple change or regression.

1. Mechanically close and disposition the whole upstream owner/caller/predicate
   graph, including named constructor references, statement-list results,
   function child-table ordering, constructor's two visitation phases,
   prologue merging and generated/private name domains. The source220 inventory
   and15-owner helper supplement are research inputs, not a ready semantic
   disposition ledger. The comma factory amendment resolves its concrete
   design finding; complete the remaining per-owner/callee dispositions.
2. Audit the now-complete, typechecked candidate against each whole upstream
   row and map the newly frozen name/comma witnesses to their exact predicates.
   The concrete source already contains the factory/raw/map/comment/flag
   operations, lexical/block restoration and initializer-result callers, but
   typing alone does not prove their semantic equality.
3. Revalidate every architecture row and frozen predecessor used as a premise,
   including the finalizer's name policy and source/parsed collision domains;
   build the exact allowed-files, row-to-step and witness mappings.
4. Freeze the staged source design and run a new mechanical readiness checker
   with unresolved=0 and undispositioned=0 before production changes. Then run
   the full530 comparator and unchanged494/451 emitter suites with1350
   declaration reprints, retain actual failures, and qualify only the complete
   measured owner scope. Hosted acceptance is still required before landing.

Use one heavy command at a time, taskpolicy background, nice15 and
CARGO_BUILD_JOBS=2. Poll a live handle; do not restart after an observation
timeout. Root is the sole writer. No fixture-specific production logic, output
substitution, handwritten expectations, permissive fallback or blanket flag
repair is allowed. H2.8a-e remains active throughout this work.
