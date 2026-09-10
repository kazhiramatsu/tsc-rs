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

Attempt15 of v3 completed in1019.422 seconds, exit101, at **477/530 exact
twice and53 failures**. Its
[full receipt](../../../../ratchets/h2-8a-list-intervening-design-experiment.v1.json)
proves all530 tuples unchanged from the stronger predecessor, all80 required
repairs and all477 positives preserved, zero typed failures and zero
regressions. The actual128194112-byte binary,990 copied inputs,109 vendor
inputs and all primary/supplemental observations are retained.

### Ordinary item comments and the final comma

The new104 source controls in `list-trailing-token-owners.json` cover parsed,
cloned, range-only and comment-only final identifiers; NoTrailingComments,
source-ranged/synthetic arrays, present/absent trailing commas, inline/line/
file-prefix comments, and adjacent onlyPrintJsDocStyle controls. The
`observe-list-trailing-token-owners.mjs --check` command observes TS twice.
Attempt16 against unchanged v3 is260/294 complete controls exact twice:
all prior190 preserved,70/104 new rows exact, **34 new output differences**,
and all244 applicable factory-state observations exact. No expectation is
changed. The native fixture uses update_node_array on two parsed children
to produce one fresh child array with the source trailing comma, then copies
only the requested array range; it never mutates a parsed array for setup.

These failures establish `A40-F-LIST-TRAILING-TOKEN-OWNER`: source
emitNodeListItems120129-120150 checks the final child's own NoTrailingComments
and raw end, applies emitTokenWithComment only when allowed, and guards the
closing leading-comment phase by parent/raw-child end equality. A CommentRange
override affects ordinary node comments, not those raw token/list predicates.
The old array-end helper ignores those guards and JSDoc filtering, while the
old item-end helper substitutes raw endpoints for ordinary CommentRange ends.
writeTokenText120222-120226 preserves a negative position rather than adding
one; the cloned file-prefix controls confirm that the synthetic token cursor
must remain synthetic.

The next isolated printer patch (v4) changes the five consumers of
emit_delimited_expression_list: array/object literals, array/object bindings,
and import attributes. Its explicit item hint is Expression for array literal
elements and Unspecified for the other four source emitList callers. Add a
trailing-comma policy to DelimitedListFormat: literals and bindings allow it;
ImportAttributes526226 does not. These are source format fields, not token or
fixture spelling tests.

Replace manual item-leading/item-end comment reconstruction in that worker
with separate positional intervening comments and the existing complete
emit_optional_ordinary_child pipeline. That pipeline retains substitution,
parenthesizer, hook and source-map order, with the inherited comment scope;
ordinary child comments use CommentRange and list phases use raw endpoints.
No synthetic parent-container claim or trailing-only shortcut is introduced.
Before each sibling comma use the source-leading raw-end phase. At the final
comma, reuse emit_source_leading_token_with_context with a cursor built from
the raw final end independently; absent synthetic end stays TokenCursor::Synthetic.
The final leading phase chooses the raw array end only when the trailing comma
is emitted and that raw end is nonzero, otherwise the final child's raw end.
Parent/raw-child equality, NoTrailingComments, inherited commentsDisabled and
containerPos suppression all guard it. Do not follow original for positions;
the existing fixed-token helper owns getParseTreeNode/similar-kind checks.

Preserve the current first/closing-line controller pending its separate full
source disposition. Restore list and temporary sibling indentation on every
Result path. Freeze the v4 patch, rerun all294 direct controls, then existing
emitter suites and full530 against the frozen v3 predecessor. Other list
workers and detached-prefix/format/fault obligations remain open; this is
still an isolated design experiment, not whole A40 readiness or activation.

The v4 candidate also removes the old multiline-only leading helper and its
private context branch: its sole former caller was this replaced expression
list body, as verified across `crates/emitter/src`. The complete ordinary
pipeline now owns that phase. An initial unexecuted render is retained only
in target; the actual v4 patch is
[candidate-v4.patch](h2-8a-list-intervening-printer.candidate-v4.patch), SHA-256
`37d3637aa327c87a3ed7529d88a0c63ad9c374342a49af554917a80bb4fb78a3`.
The [19-owner source supplement](../../../../ratchets/h2-8a-list-tail-sources.v1.json)
pins the exact ordinary-comment and final-token owners, including printer
emit117145 and emitExpression117158; other same-named compiler/generator
functions are excluded by those pinned declaration positions.

The [direct before](../../../../ratchets/h2-8a-list-trailing-token-direct-before.v1.json)
retains attempt16 and all34 failed tuples. Attempt17 of v4 is **294/294 exact
twice**, with all244 factory states exact, exit0 in24.331 seconds. Its
[direct after](../../../../ratchets/h2-8a-list-trailing-token-direct-after.v1.json)
retains both executed binaries and complete source pins. Attempt18 is
**494 units /451 contracts /1350 declaration reprint rows passing**, exit0
in39.533 seconds without warnings; see the
[adjacent emitter receipt](../../../../ratchets/h2-8a-list-trailing-token-emitter-checks.v1.json).
The unchanged11 declaration exclusions remain excluded. All root production
bytes remain unchanged. The full comparator now pins the complete v3
477-positive receipt as the strongest predecessor for v4, rejects unrecognized
candidate patch identities, and checks all1060 predecessor captures.

Attempt19 of v4 completed in1001.541 seconds, exit101, at **477/530 exact
twice and53 failures**. Its
[full receipt](../../../../ratchets/h2-8a-list-trailing-token-design-experiment.v1.json)
checks all530 complete tuples against v3, including errors and partial
writes: all are identical. All80 required repairs and477 positives remain
exact, with zero typed failures, regressions or changed failing tuples.
The128193632-byte executed binary is archived with SHA-256
`abd77c7ae97f822c1e5535c66b1bcf7f793afc8dad0028bac78d88259cad1aa5`.
This evidence qualifies neither the whole emitter nor the still-open A40 gate.

### First and closing list lines: raw positions and parent identity

The next source witnesses are generated by
`node scripts/observe-list-boundary-lines.mjs --check`. The264 controls
combine four line layouts, four parent provenance states, four child
provenance states, ranged/synthetic arrays, PreferNewLine, and explicit
startsOnNewLine overrides. The ordinary array factory receives children from
a parsed call; the generated array may independently retain that call's raw
range or original link. Both are legal observable metadata, and neither
implies a parsed array parent. Native controls check the complete factory
array state, parent range/original/multiLine state, output bytes and UTF16
coordinates twice. Existing294 controls and expectations remain unchanged.

Source getLeadingLineTerminatorCount120268-120298 reads current raw parent
start and both child endpoints, compares the original identities of the
child's parent and current parent, then compares raw token-start lines.
getClosingLineTerminatorCount120328-120358 requires the child's actual parent
identity instead, and compares raw ends. Neither checks whether the NodeArray
has source ranges. Both fall back through synthesizedNodeStartsOnNewLine;
PreferNewLine precedes those predicates. The current first/closing controllers
follow original ranges, impose source order, ignore parent identity and gate
on NodeArray synthesis; all four choices require correction.

The leading predicate also reads nextListElementPos. Whole-source search
finds its declaration116946, assignment120125 and read120276. Its initial
value is undefined, not zero; reset117117-117140 does not reset it. It is
printer-instance state that survives nested lists and repeated print calls,
not a scoped parent-context value. Every reachable list updater and failure
path must be mapped before introducing that state. These new boundary
controls do not establish the complete cursor lifecycle. First reproduce the
raw/parent differences against v4; then stage concrete source-owned boundary
helpers in an isolated candidate, retain cursor findings explicitly, and
verify all558 controls plus adjacent suites. Root production remains unchanged.

Attempt20 against v4 completed in7.982 seconds, exit101: **517/558 exact
twice,41 boundary-line differences**, all508 factory states and all264 new
parent states exact. The
[before receipt](../../../../ratchets/h2-8a-list-boundary-lines-direct-before.v1.json)
retains every complete failed tuple and both executed binaries. All294 old
controls remain exact. This establishes `A40-F-LIST-BOUNDARY-LINES` without
changing expected output. The
[source supplement](../../../../ratchets/h2-8a-list-boundary-sources.v1.json)
pins11 whole functions, all three cursor references, and the whole reset body;
reproduce with `node scripts/observe-list-boundary-sources.mjs --check`.

The isolated v5 candidate adds a private DelimitedListBoundary enum and
preserved_list_boundary_needs_line_break. The caller passes Leading or Closing
explicitly; raw parent start and both raw child endpoints select applicability.
Leading compares get_original_node(child.parent) with get_original_node(parent),
while Closing compares TransformNode identity directly. Absent child.parent
passes both predicates. Compare independently validated token starts for
Leading and raw ends for Closing, without an ordering requirement. The
current source PositionIndex supplies line numbers; originals never supply
positions. PreferNewLine returns first; JsxText suppresses Leading; synthesized
child startsOnNewLine supplies the fallback. The worker removes only its
NodeArray-synthesis guard and retains all comment phases and Result cleanup.
No public type or root production implementation is changed.

The [v5 patch](h2-8a-list-intervening-printer.candidate-v5.patch), SHA-256
`dc9df77bbe90b4a588decd33de9ac9c0ddfa2561929212aa34b175838342ba5c`,
applies after the original comma-printer patch. This is the raw-position and
parent-predicate repair; nextListElementPos remains unresolved and explicitly
blocks whole leading-line qualification. Its separate state model must cover
every updater, nested reentry, repeated print calls and observable failure
order before production readiness. The v5 full comparator is pinned to the
completed v4 receipt, rather than relying on pass counts alone.

Attempt21 of v5 passes **558/558 controls twice**, with all508 factory
states exact, exit0 in65.507 seconds. All41 observed differences are repaired.
Compilation identified the old source_node_starts_are_on_same_line and
source_node_ends_are_on_same_line as unused. Whole-printer search confirms
their only occurrences are now their declarations; their sole former callers
were replaced by the new boundary worker. Remove these obsolete60 lines in
[v6](h2-8a-list-intervening-printer.candidate-v6.patch), SHA-256
`805a14b709ad3ec858181ed94b1399ab6e4f24c9a24c33207fd8d237ebfd97d2`,
then rerun direct and adjacent checks on that exact patch. Preserve v5 and its
executed evidence; no expectation changes or warning suppression are allowed.
The applied v6 printer SHA-256 is
`86ac43281bc32f73301813154f1410b6bfab0bf814ce3a9df9ef40ee6b83b748`.

Attempt22 of v6 passes **558/558 controls twice**, with508 factory states and
264 parent states exact, exit0 in22.122 seconds without warnings. Its
[after receipt](../../../../ratchets/h2-8a-list-boundary-lines-direct-after.v1.json)
retains the before20 comparison and executed intermediate21 evidence.
Attempt23 passes **494 units /451 contracts /1350 declaration reprint rows**,
exit0 in105.706 seconds without warnings; the11 preexisting exclusions remain
excluded. The
[emitter receipt](../../../../ratchets/h2-8a-list-boundary-lines-emitter-checks.v1.json)
validates all992 copied inputs,109 vendor inputs and both archived binaries.
The next full530 comparison must retain the complete v4 predecessor's477
positives and report every tuple change. No full-v6 result is claimed yet.

The [fresh cursor sequences](../../../../ratchets/h2-8a-list-cursor-lifecycle.v1.json)
are reproduced by `node scripts/observe-list-cursor-lifecycle.mjs --check`.
Eleven sequences execute `[target, seed, target, target]` with one TypeScript
printer, each twice. First-item, nested-first, comma, call and object-contained
list seeds suppress the next target's leading newline; last-item, empty,
nested-then-last and scalar-after-list seeds do not. A subsequent target
restores the original observable output by updating the cursor again.
This proves that nested list updates survive their return and ordinary scalar
emission does not perform a list update. A different source's seed at UTF16
position7 but byte position9 still matches target position7: the semantic
state compares UTF16 position numbers across sources, without node or source
identity. These are source observations only, with zero native executions.
They prohibit resetting the state per print/source, restoring it per nested
list, storing only byte offsets, or updating it for every ordinary child.
The next Rust design must also audit list-bypassing source-text paths and
preserve the cursor at each observable failure boundary.

Attempt24 of v6 completed in951.707 seconds, exit101: **477/530 exact twice,
53 failures**, all530 complete tuples identical to v4. Its
[full receipt](../../../../ratchets/h2-8a-list-boundary-lines-design-experiment.v1.json)
checks all1060 current captures and every baseline/predecessor capture, retains
all80 required repairs and477 positives, and reports zero typed failures,
regressions or changed failing tuples. The executed128193744-byte binary is
archived with SHA-256
`b61f341dad19df03fb3de27a546be49e1a68f18d70ce80a73117dd4d8c89880c`.

### Printer-owned list position state

The [cursor source census](../../../../ratchets/h2-8a-list-cursor-sources.v1.json)
records all51 direct list-entry calls within createPrinter and60 whole owners,
including the shared getEmitListItem/emitListItem callbacks outside the printer
closure. It also records the separate prologue path and identifier-type-argument
metadata producer/consumer. Reproduce with
`node scripts/observe-list-cursor-sources.mjs --check`. This census is not a
completed readiness disposition. MappedType.members, identifierTypeArguments,
JSDoc-list workers, list-bypassing source-text paths and the remaining comment
failure ordering still require explicit semantic resolution.

The11 cursor sequences now have a direct Rust contract that uses a single
Printer for four StandaloneNode requests. It mounts both source files in one
arena for the UTF16-versus-byte case and retains the target's parsed argument
NodeArray exactly as TypeScript does. No expected output or prior558 control
is changed. The factory-direct selection now executes569 cases twice, with
the original508 applicable factory-state checks. First run this contract on
unchanged v6 and retain every complete failed observation before applying the
new state candidate.

The concrete Rust state design is a private ListElementPosition enum with
Synthesized and Source(SourceUtf16Position) variants, held by Printer as
Option<ListElementPosition>. None is the initial undefined state. The producer
reads the current node's raw pos independently of end or original; a sourced
position converts through the mounted source's PositionIndex. The value has
no source identity and survives successful print calls, source changes and
nested-list returns. No reset or scoped restoration is added. The existing
public print entry points already require &mut Printer; propagate that
exclusive borrow through their private emission call graph. Keep read-only
helpers and public signatures unchanged. This preserves ordinary owned state
and Printer's Clone/Eq/Send/Sync properties without interior mutability.

The staged design updates the first-line reader after PreferNewLine and before
JsxText/source-line predicates. List item entry records its position before
ordinary item emission and never records names or initializers merely because
they are children. Empty lists do not update it. Source and function prologues
are not list entries; source statements after the prologue and block statements
after function prologues are. Integrate explicit updates in literal/binding/
attribute, comma, call, parameter, named import/export, declaration/type,
modifier/decorator, JSX, class, case and statement list workers. The draft has
22 update sites across19 native workers (the case-clause worker has two arms).
Its100 private mutable receivers are
derived from the emission call graph, not a public API rewrite.

Extend the isolated candidate's allowed files to printer/bundle.rs only for
the transitive &mut self receiver of write_bundle_prologue: a prologue itself
does not update the cursor, but its ordinary emission may reach list workers
after substitution. No writer ownership or cross-thread shared state is added.
Raw-position validation retains typed errors; a later item error must not roll
back a completed cursor update. Existing manual comment workers require their
own failure-order controls before whole-cursor qualification; normal-output
success alone will not close that obligation. Run569 direct controls, adjacent
emitter suites and the full530 comparator on the final frozen candidate, and
keep A40 production readiness open until every listed gap is dispositioned.

Attempt25 on unchanged v6 is **562/569 exact twice**, exit101 in9.362 seconds.
All7 failures change only the after_seed observation; fresh output, seed output
and the following target output are individually equal. All558 previous cases
and508 factory states remain exact. The
[before receipt](../../../../ratchets/h2-8a-list-cursor-direct-before.v1.json)
establishes `A40-F-LIST-CURSOR-LIFETIME` and retains the exact failed tuples and
both executed binaries. The first frozen state candidate is
[v7](h2-8a-list-intervening-printer.candidate-v7.patch), SHA-256
`e22b80f928d7d307aad96a3c3f58fdb8f161c3ede7079c00c6b3e1a0cb1f602c`.
It includes the single bundle receiver change and the owned state design above.
The runner now explicitly pins the unchanged root bundle source before applying
this patch; its full-comparison predecessor is the completed v6 receipt.
Initial draft-generation assertions and rustfmt delimiter checks failed before
any native execution; corrected, formatted draft bytes alone are frozen here.

Attempt26 of v7 passes **569/569 direct controls twice**, with all508 factory
states exact, exit0 in92.541 seconds without warnings. Its
[after receipt](../../../../ratchets/h2-8a-list-cursor-direct-after.v1.json)
retains all562 previously exact controls and repairs every observed after_seed
difference. Attempt27 passes **494 units /451 contracts /1350 declaration
reprint rows**, exit0 in108.791 seconds without warnings. The
[emitter receipt](../../../../ratchets/h2-8a-list-cursor-emitter-checks.v1.json)
validates993 copied inputs,109 vendor inputs and both executed binaries; the11
preexisting declaration exclusions remain excluded. These are isolated candidate
results. The remaining first-line consumers, missing list fields and comment/
failure-order obligations listed above still block whole A40 readiness. The
full530 v7 comparison (attempt28) is now complete: **477/530 exact twice**,
53 failures, exit101 in1098.796 seconds. The
[full receipt](../../../../ratchets/h2-8a-list-cursor-design-experiment.v1.json)
retains993 copied inputs,109 vendor inputs and the executed binary. All530
complete tuples, including errors and partial writes, are identical to the
completed v6 predecessor; all80 required repairs and477 prior successes remain
exact. This supplies regression evidence, not additional repairs of the53
successor failures or whole-emitter convergence.

### Mapped type member-list amendment (source controls frozen)

`emitMappedType` in pinned `_tsc.js:118112-118155` emits the optional `members`
list after the mapped signature and before the closing brace. The format is2
(`PreserveLines`): no brackets, no delimiter, no sibling space and no list
indent. `emitList` adds `PreferNewLine` from parent `EmitFlags.MultiLine`;
the signature's `SingleLine` choice is independent. Ordinary child emission,
positional comments and the printer-owned cursor still run. Empty/absent
unbracketed lists do not emit bracket-boundary comments. Native
`MappedTypeData.members` exists, but `emit_mapped_type` currently ignores it.
This is a missing reachable owner, not a deferred syntax category.

The fresh observer `node scripts/observe-mapped-type-members.mjs --check`
fixes328 direct controls in
`crates/emitter/tests/fixtures/mapped-type-members.json` (SHA-256
`1aad1af30be5d18a4816ac8bb0b994124459b6829f8414b50c289593a8aed2d3`).
These vary four source layouts, parsed/updated/created/ranged parents,
kept/reversed/cloned children, array ranges, empty/absent arrays, all four
SingleLine/MultiLine combinations and nested/JSDoc/remove-comment policies.
Full tree provenance and raw positions, output bytes and final UTF16 writer
coordinates are observed twice. The native test must reproduce the recipes
with one mounted `main.ts`, declaration syntax admitted, an empty transform
pipeline and StandaloneNode printing; raw Rust byte positions are converted
to UTF16 before comparison. No expected observation is hand-authored.

The candidate implementation step is to generalize the existing delimited
list worker with explicit optional brackets, delimiter and sibling-space
policies, then route mapped members through it with Unspecified item hint.
No empty punctuation writes or duplicated mapped-only comment engine are
allowed. Preserve the common raw-position line controllers and cursor state;
run ordinary child comments at their existing pipeline phase. Delimiter-end
callbacks and final-comma handling are absent when the format has no delimiter.
Freeze native-before evidence on v7, then the cumulative candidate patch and
native-after controls; compare adjacent emitter suites and the full530 tuples
to the completed v7 receipt. Production files remain unchanged while the whole
A40 readiness gate is open.

Native-before attempt29 executes all897 controls twice on unchanged v7:
585 exact,312 failed, exit101 in15.083 seconds. Every nonempty mapped-member
control fails; every328 tree state, all16 empty/absent controls and the569
previous controls are exact. The
[before receipt](../../../../ratchets/h2-8a-mapped-members-direct-before.v1.json)
establishes `A40-F-MAPPED-MEMBERS` and retains all three executed binaries.
The cumulative
[v8 candidate](h2-8a-list-intervening-printer.candidate-v8.patch) is frozen with
SHA-256 `271c5668f7de149c8d14f9b71173e4e744551b7fa9a094f47950485e38a7e697`;
its rendered printer SHA-256 is
`98936a543b3683e0dac33f35f8ac26ee6b1a217b62fb110985f3ec2c23007296`.
`ListBrackets` selects optional square/curly punctuation; `ListDelimiter`
selects comma/none, independently of sibling spacing and indentation. The
renamed common `emit_formatted_node_list` handles the new member caller and
all five previous callers. Only mapped members derive the newly needed
PreferNewLine from parent emit flags here; the existing callers' flag-wrapper
and Compact line-policy gaps remain separate open obligations. No broader
list-format readiness is claimed. Draft-generator count assertions exposed
unrelated string-token sites; edits were bounded to the five call sites and
the common worker before any v8 native execution. Native-after is next.

Attempt30 on v8 is **873/897 exact twice**, with24 failures, exit101 in85.664
seconds. It repairs288 of312 nonempty failures and preserves every585 prior
success and all328 tree states. Every remaining failure is a cloned member
with source comments. The upstream caller expansion identifies two missing
ordinary name phases: `emitPropertySignature` (`_tsc.js:117876-117882`) calls
`emitNodeWithWriter(name,writeProperty)` (`119839-119845`), which calls ordinary
`emit`; `emitMethodSignature` (`117897-117902`) directly calls `emit(name)`.
Both names therefore enter Unspecified notification/substitution and ordinary
leading/trailing comments. Cloning the member leaves its raw range synthetic,
but its reused parsed name still owns source comments. Native
`emit_required_identifier_name_with_context` currently enters IdentifierName
without deferred source comments, so it cannot emit that name-owned comment.

The next cumulative candidate routes these two required names through
`emit_optional_ordinary_child` with Unspecified hint, retaining explicit
MissingTransformedChild validation and full source-comment extent. This is
a general signature-name fix, with no mapped-parent/provenance branch. The
parent comment scope suppresses comments already consumed by a parsed member;
NoNestedComments and removeComments continue through the existing pipeline.
The24 frozen clone failures, the304 other mapped controls and569 previous
controls are the immediate witnesses, followed by the full declaration/core
and530 regression comparisons. Writer callback/category reachability and the
remaining signature-token phases still require their whole-owner disposition;
this amendment does not qualify those unmeasured observables.

The frozen [v9 candidate](h2-8a-list-intervening-printer.candidate-v9.patch)
has SHA-256 `b72e45e6317307a04d0576ba15cc000c50296cdd07c1c8e0476a67ae4d54083a`
and applied printer SHA-256
`dfd02c0ca77b41b559184f4f27103b56e3aecd0afddddb5add59878173056f32`.
It changes the two ordinary signature-name calls on top of v8. Its strongest
completed full530 predecessor is still v7, not the failed direct-only v8 run.

Attempt31 on v9 passes **897/897 direct controls twice**, exit0 in24.000 seconds,
with508 factory states and328 mapped tree states exact. The
[after receipt](../../../../ratchets/h2-8a-mapped-members-direct-after.v1.json)
retains the intermediate v8 failures and all three v9 executed binaries. It
repairs every312 measured mapped-member difference and preserves585 prior
successes. Adjacent and full530 checks will run on the final cumulative
candidate after the immediately shared format-policy obligation below.

### List wrapper and parent flag amendment

Fresh source inspection distinguishes `_tsc.js:120015-120028` emitList (adds
PreferNewLine from EmitFlags.MultiLine) from emitExpressionList (no such
addition). The whole callers are binding patterns (`118183-118192`), array
literal (`118203-118207`), object literal (`118208-118222`) and import attributes
(`119316-119321`). Array and object literals add PreferNewLine from `multiLine`;
import attributes do not read `multiLine`. Binding formats have no LinesMask
and do not read `multiLine`: getLeading/getClosing return0, while separating
lines follow only the next child's startsOnNewLine. A PreferNewLine bit alone
does not enable that PreserveLines branch. The current common worker conflates
these inputs and its callers omit the emitList parent flag addition.

`node scripts/observe-list-format-flags.mjs --check` freezes160 controls in
`crates/emitter/tests/fixtures/list-format-flags.json`, SHA-256
`3d7b7be6069f460f0a585893b280b843ef0a2dceb14d14203214ebd8203a90a8`.
Five owner kinds vary source sibling lines, factory/public-setter multiLine,
all four SingleLine/MultiLine emit-flag combinations and last-child
startsOnNewLine. Both tree state and complete writer outputs are observed
twice. Binding-node multiLine is assigned through the same open field accepted
by native public set_multi_line; it is an intentionally ignored input in tsc.
Native controls use the corresponding factory recipes and ordinary standalone
printing with no transforms. The expanded direct command executes1057 controls
in four binaries; previous897 expectations are unchanged.

The design step is to derive PreferNewLine at each caller from exactly its
source-owned inputs, then separate PreserveSource from Compact line behavior
inside the shared worker. Compact first/closing lines are false; compact
sibling breaks use child startsOnNewLine and temporary indentation. Array
emit flags are not consulted; object literals OR multiLine with parent
EmitFlags.MultiLine; import attributes use only parent EmitFlags.MultiLine;
bindings ignore both parent inputs. Capture native-before on v9, freeze a new
cumulative candidate, then execute the complete expanded direct set followed
by494/451/1350 adjacent checks and full530 versus v7. Existing writer/cursor/
comment ordering stays owned by the common worker. Whole A40 remains open.

The first expanded native run32 has985/1057 exact controls and72 failures:
56 real output differences plus32 tree spelling differences (16 overlap).
The latter come from the observation adapter: TypeScript's enum reverse map
overwrites kinds301/302 with AssertClause/AssertEntry
(`typescript.js:5828-5831`), while native Debug uses ImportAttributes/
ImportAttribute for the same301/302 (`syntax/src/kind.rs:611-613`). The native
test adapter now asserts these numeric identities and emits the source's
final alias names. Oracle fixtures, syntax trees and production remain
unchanged. Run32 remains retained evidence of the adapter mismatch, not72
semantic failures. A corrected native-before run on the same v9 follows
before executing the staged format-policy candidate.

Corrected before33 on unchanged v9 has **1001/1057 exact twice**,56 output
failures, exit101 in9.702 seconds; all897 previous controls and all488 observed
tree states are exact. The
[before receipt](../../../../ratchets/h2-8a-list-format-flags-direct-before.v1.json)
retains run32's initial adapter evidence separately. The56 measured differences
are8 object literals,16 import attributes and16 each binding-pattern kind.
The array-literal negative controls remain exact. Candidate
[v10](h2-8a-list-intervening-printer.candidate-v10.patch), SHA-256
`fea19f9b360900530d5f6f29847d2ae9f005b7ae6715b4b2ac9cdae9b7e02552`,
implements the exact caller and Compact policies above. Applied printer
SHA-256 is `7a8bfc47a8b08ef3e2da6af2f978ea77841d4f9205e6de1dc5b6041bb569a505`.
The shared parameter now explicitly names PreferNewLine; it is not a generic
request to force multiline output. Native-after on these bytes is next.

After34 on v10 passes **1057/1057 controls twice**, exit0 in27.475 seconds,
with508 earlier factory states and488 mapped/format tree states exact. The
[after receipt](../../../../ratchets/h2-8a-list-format-flags-direct-after.v1.json)
retains all four executed binaries and repairs every56 measured format
difference while preserving1001 previously exact controls. The cumulative
candidate also preserves all328 mapped-member controls. Complete adjacent
emitter and full530 comparisons on these bytes are next; none is claimed from
the direct-control result alone.

Adjacent35 on final v10 passes **494 units /451 contracts /1350 declaration
reprint rows**, exit0 in172.865 seconds without warnings. The
[emitter receipt](../../../../ratchets/h2-8a-list-format-flags-emitter-checks.v1.json)
verifies997 copied inputs,109 vendor inputs and both executed binaries. The11
preexisting declaration exclusions remain unchanged. All eight direct source
observers were freshly checked against their immutable artifacts. Full530 on
the same v10 candidate is next, comparing all complete tuples to v7; these
focused and adjacent successes do not establish whole A40 readiness or global
emitter completion.

Full36 on v10 is now complete: **477/530 exact twice**,53 failures, exit101
in1239.712 seconds. Its
[full receipt](../../../../ratchets/h2-8a-list-format-flags-design-experiment.v1.json)
retains997 copied inputs,109 vendor inputs and the executed binary. All530
full tuples, including errors and partial writes, are identical to v7; all80
required repairs and477 prior successes remain preserved, with no typed
failure or regression. The mapped-member/list-format repairs improve their
direct witnesses but do not reduce the53 successor failures in this separate
population. No full-emitter convergence claim follows from this result.

### Import-type attribute hint and pipeline phase amendment

Pinned TypeScript6.0.3 `_tsc.js:118163-118182` emits import-type attributes
through pipelineEmit(ImportTypeNodeAttributes), not the ordinary attribute
worker. `pipelineEmitWithHintWorker:117236-117259` selects the specialized
worker after notification, substitution, comments and maps. That worker
(`119305-119315`, hashc2028e6703acc1bf61520b1e7d35939630903c14182894f429a798dc396a8486)
writes the outer `{ with: ... }`/`{ assert: ... }` wrapper and calls emitList
with format526226. Its list uses exactly the common ImportAttributes policies;
parent EmitFlags.MultiLine adds PreferNewLine and multiLine is ignored.

Native EmitHint::ImportTypeNodeAttributes exists in transform.rs but was not
used by the printer. The import-type caller directly invokes its private
worker, bypassing the attribute's notification, substitution, ordinary
comments, NoNestedComments extent and node-map boundaries. Its private
emit_import_attribute_elements also routes through a compact comma helper,
omitting the common preserved-line/cursor/positional-comment phases.

Fresh source observer `observe-import-type-attributes.mjs --check` fixes84
controls in `fixtures/import-type-attributes.json` (SHA-256
56dbdad5fed18af927e304830d7238e593de9b99f673cf49b5541471d985b528):
four layouts (including Unicode), with/assert, parsed/clone/created/ranged
attribute provenance, parent flags0/2, comment suppression/JSDoc controls and
four actual attribute substitutions. The full parent/attribute/list/child
tree state, complete output bytes/final UTF16 coordinates and enabled
attribute hook events are recorded twice. Replacements use a fresh reversed
member array and fresh attributes; both original and replacement provenance
are observed. Native mirrors the same factory recipes with declaration
syntax and StandaloneNode printing, and an internal Transformer enabling
only the ImportAttributes hooks. No external custom-transformer API is
activated by these internal seam controls.

The source hook events are **substitute, before, after**. This is established
by `getPipelinePhase:117185-117216` and
`pipelineEmitWithNotification:117219-117222`: selecting the next phase invokes
substituteNode (and the parenthesizer for a changed node) before entering the
notification callback. All seven native selection sites currently call
before_emit_node first: source root/statement pairs in original-text, JSON
and canonical routes, plus the common node pipeline. The independent source
observer `observe-emit-pipeline-phases.mjs --check` fixes8 ASCII/Unicode
controls in `fixtures/emit-pipeline-phases.json`, SHA-256
ee930248d3000c051fd4dc146f338c2c0242fb6b3f8ec2337086d4b6be519ba9.
Those controls enable SourceFile/ExpressionStatement hooks and, for the
standalone route, Identifier hooks, checking complete ordered traces and
writer outputs. Existing fields and expected artifacts are unchanged.

The cumulative candidate remains confined to printer.rs plus v7's existing
bundle receiver hunk, applied in an isolated workspace while A40 is open:

1. Carry the existing EmitHint by value through the common substituted-node
   comment adapter and transformed-node map/notification adapter. Manual
   source statements supply Unspecified. The virtual no-ASI whole-node
   wrapper forwards its incoming hint; nested children keep their own hints.
   This is node-local call data, never a printer-global field or an inherited
   child context. No reset/invalidation protocol is added.
2. At the node worker selection point, ImportTypeNodeAttributes dispatches
   to the specialized worker, retaining kind validation there. All other
   hints keep their current ordinary worker path; unimplemented mapped-type
   parameter/identifier-specific validation remains an explicit open owner.
   The attribute caller enters emit_optional_ordinary_child with its special
   hint and complete source-comment extent, so substitution changes the
   actual comment/map/worker owner before emission.
3. Pass the actual attribute handle to its worker, validate/clone its data,
   and replace its sole manual list helper with emit_formatted_node_list,
   Curly/IMPORT_ATTRIBUTES/Unspecified and parent EmitFlags.MultiLine.
   Remove the now-unused private compact attribute helper. Preserve the
   wrapper's before/after spaces and tokens.
4. Reorder substitution selection before before_emit_node at all seven
   current entry sites. In the common node path, select grammar parentheses
   before entering notification, then retain the existing scoped restoration
   and primary-error precedence. A substitution failure has not entered any
   notification; it must not invoke after. No transform trait or public ABI
   changes are needed. Full callback-failure, reentrant replacement and
   parenthesizer failure traces remain unresolved obligations; successful
   trace controls alone cannot qualify those branches.
5. Freeze native-before on unchanged v10 after full36 has terminated and its
   evidence is archived. Execute all previous1057 plus the92 new controls,
   retaining complete failures and tree states. Then freeze/execute the
   candidate, resolve measured differences and run adjacent494/451/1350 plus
   full530 against the strongest completed predecessor. No fixtures may be
   rewritten to hide new output/trace regressions, and all80 required fixes
   and previously exact full530 tuples remain individually gated.

The source-only fixtures are new files created while full36 runs; they were
not copied or executed in36 and supply no credit to that run. Production,
whole A40 readiness, H2.8a-e and hosted activation remain incomplete.

Native-before37 on unchanged v10 completed with exit101 in14.700 seconds:
**1057/1149 exact twice**,92 failures, all1057 previous controls preserved.
The [before receipt](../../../../ratchets/h2-8a-import-type-attributes-direct-before.v1.json)
retains all2298 attempts across six binaries, all572 tree states and508 factory
states exact. All84 attribute controls lack their expected events;76 also
differ in text. All8 phase controls have an event-order difference; one also
differs in text. Those distinct observations remain recorded, including the
text difference not explained by event order alone.

The cumulative candidate v11 is frozen in
`h2-8a-list-intervening-printer.candidate-v11.patch`, SHA-256
c9f84f82ccf3ba8eadcb50bb585342693c7a8128e91a8c4190581e55298e6805.
Applied printer SHA-256 is
2f8be33e6f35b9e512c49b813dd06f245be9fa492924c86e9f99f240498d8d3d;
the existing v7 bundle hunk remains unchanged. The next execution is
`python3 scripts/run-comma-printer-design-experiment.py 38 factory-direct --factory --list-owner`,
followed by `python3 scripts/analyze-list-intervening-design-experiment.py 38`.
The full comparator pins v11 to completed full36, the strongest predecessor.
No v11 execution success is claimed by this candidate freeze.

Attempt38 is a compilation failure (exit101,5.857 seconds, no executed
binaries): `bundle.rs:263` still called emit_transformed_node without its
new hint. Its complete compiler output and frozen inputs are retained in
`ratchets/h2-8a-import-type-attributes-compile-attempt.v1.json`; it earns no
runtime credit. The bundle caller census therefore adds an eighth selection
site, write_bundle_prologue, to the seven in printer.rs. Upstream
emitPrologueDirectives (`_tsc.js:119789-119811`) calls ordinary emit(statement)
after writeLine; it shares the same substitution-before-notification order.
The Rust change is to select substitution before before_emit_node here too,
and pass EmitHint::Unspecified through the common worker adapter. The existing
bundle file is already an allowed candidate file; no new production file is
required. Failure/reentry behavior remains open as above.

Fresh `node scripts/observe-emit-pipeline-bundle.mjs --check` freezes two
ASCII/Unicode one-file bundles with a prologue and a following statement in
`fixtures/emit-pipeline-bundle.json`, SHA-256
f9f5d2f37f7060ba36ab119991f86d9939af94c59fc6e9db7fc2ebf3d69e7c98.
Native uses TransformBundle and PrintRequest::Bundle, observing all enabled
statement/root events and complete text twice. Attempt39 repeats native-before
on v10 with these added controls (1151 total), preserving the unchanged1149
observations. A new cumulative v12 will repair the omitted bundle caller;
published/executed v11 bytes remain immutable.

Attempt39 completed on v10:1057/1151 exact twice,94 failures, exit101 in12.044
seconds. Both added bundle controls differ only in events; every1149 previous
observation is unchanged. The expanded native-before is retained in
`ratchets/h2-8a-import-type-attributes-bundle-before.v1.json`.
Cumulative v12 is frozen as `h2-8a-list-intervening-printer.candidate-v12.patch`
(SHA-256 c787c9e6ab327bd355b2ae9d8a9ac47e761324c93d1032569053c5bc8464f50a).
Its printer bytes equal v11; its applied bundle SHA-256 is
2cf673e2048236f7aae0ce2aef2d7ea655546e4c327748b688158c88051aad61.
Attempt40 will run factory-direct with --factory --list-owner on these bytes.

Attempt40 is now complete: **1150/1151 exact twice**, one JSON Unicode text
difference, exit101 in26.831 seconds. All84 import-type attribute controls and
all94 ordered event traces are exact; all1057 previously exact controls remain
preserved, as do572 tree states and508 factory states. The
[after receipt](../../../../ratchets/h2-8a-import-type-attributes-direct-after.v1.json)
retains every result and all six executed binaries. The remaining JSON case
prints a literal emoji while TypeScript prints escaped UTF16 surrogate units.
A source probe locates canUseOriginalText (`_tsc.js:13689-13702`): absent
node.parent prevents reuse of original literal spelling. parseJsonText does
not populate parent links; calling setParentRecursive on the same TS tree
changes its output to the literal emoji. Rust NodeArena finalization always
populates structural parents. The representation of optional upstream parent
provenance, and the literal worker's broader original-text eligibility, are
open findings; no JSON/path-specific escaping workaround is authorized.

Adjacent41 on the same v12 candidate passes494 units,451 contracts and all1350
declaration reprint rows, exit0 in38.493 seconds with no warnings. The
[adjacent receipt](../../../../ratchets/h2-8a-import-type-attributes-emitter-checks.v1.json)
retains1002 copied inputs,109 vendor inputs and both executed binaries.
Full530 comparison to v10/full36 is next; the focused and adjacent results
do not establish whole-emitter completion or cover callback-failure/reentry.
All eleven direct source observers were freshly checked against their frozen
artifacts after adjacent41. No expected output was changed.

Full42 on v12 completed with477/530 exact twice and53 failures, exit101 in
999.269 seconds. The [full receipt](../../../../ratchets/h2-8a-import-type-attributes-design-experiment.v1.json)
retains1002 copied inputs,109 vendor inputs and the executed binary. Every530
complete tuple is unchanged from v10/full36, including errors and partial
writes. All80 required repairs and all477 previous successes remain preserved;
there are no new typed failures or regressions. The direct repairs do not
reduce the53 failures in this separate full comparison population.

### Literal source eligibility and UTF16 observation amendment

Full42 must terminate and its complete comparison be frozen before installing
the following direct tests or changing the current runner inputs. This is an
isolated candidate amendment; A40 readiness and H2.8a-e remain open.

The JSONUnicode discrepancy is partly an input-state mismatch: TS parseJsonText
leaves parent links absent, while Rust parsing finalizes structural parents.
The existing Node.parent:Option<NodeId> can represent either input. The direct
test adapter can project setParentNodes=false by clearing parents in its owned
SourceFile before TransformArena::add_source mounts the immutable emit copy.
Children, node IDs, flags and raw ranges are retained. This requires no new
parser flag, source-mode field or public emitter API. It does not qualify a
new production parser entrypoint or mutate an already mounted parse authority.

The literal worker also has a separate semantic error: !changed is not
canUseOriginalText. Pinned `_tsc.js:13647-13688` getLiteralText
(356597156c0c174eae1c33df6b6a6f93615b875d1212e6d41d391323a4dba00c)
delegates to canUseOriginalText:13689-13702
(bf211667ca154c343bba9e29c26d71723a30cc41f76c857458ed4bf552ae76d6).
nodeIsSynthesized:16000-16002
(d5eb53abaa73cfcae3a8c02425faff560733177bd619f5e0db719a6f9f603f4c)
tests raw pos/end, independently of NodeFlags.Synthesized or node.original.
String literals additionally need an actual parent to retain source spelling.
getLiteralTextOfNode:120467-120479
(43989b908107b6f48eae6547835a82a24937f43ba2ddd8673c48b83018d8201e)
selects textSourceNode before that test, and ORs printer neverAsciiEscape with
the node's NoAsciiEscaping flag when choosing the escaping algorithm.
The current native StringLiteral path omits the printer option.

Fresh `observe-literal-parent-provenance.mjs --check` freezes128 source rows:
TS/JSON, parented/unparented, four tokens (escaped ASCII, literal Unicode,
escaped Unicode, unpaired surrogate), parsed/parsed-with-Synthesized-flag/
clone/ranged-clone, and neverAsciiEscape false/true. Each row retains both
parsed and emitted node states, raw UTF16 ranges, actual parent state,
original identity, cooked UTF16 values, output and final writer coordinates.
The original artifact SHA-256 is
1f667aa43870ac0e813ebc9a071e3ead997665386d7f6897ccd4bccfd92098a4.
Twelve source outputs contain an unpaired UTF16 unit, which serde_json String
cannot represent. `observe-literal-parent-provenance-utf16.mjs --check` first
rechecks the original source observer, then stores exactly those output units
as text_utf16 arrays, retaining the same bytes and coordinates. Its artifact
SHA-256 is267371271768c6497d3076bcdeb91202fcd71221f94acfe0a589b4547681012f.
Both artifacts are immutable. This projection preserves all units; it does
not replace them with U+FFFD, escaped spelling or a weaker equality check.

The initial observer called setEmitFlags on an unparented parsed node and
failed before printing. The four actual TS/JSON x flags0/NoAsciiEscaping setup
failures are retained separately in both artifacts. getOrCreateEmitNode
(`_tsc.js:25287-25301`) requires a parse SourceFile for a newly annotated parse
node. These are source-only failure findings and earn no native print credit.
Their native failure/ownership contract remains unresolved. The128 print
recipes use the independent printer option and perform no setEmitFlags call.

Native recipes use existing public syntax/factory representations. Before
mounting, project requested parent absence and the explicit Synthesized bit.
Represent TS Node.text's UTF16 value with existing JavaScriptString metadata,
decoded from the input token using template_text_utf16 (these witnesses use
only its supported string escapes). Clone and set_text_range use real factory
methods; the complete128 tree states must match before claiming output parity.
No hand-authored expected value, output substitution or filename branch is
permitted. The JSON phase adapter receives the same pre-mount parent fix.
Before43 on v12 must retain all old1151 observations and measure all128 new
ones:1279 controls,2558 repetitions,700 tree states,508 factory states and94
event traces across seven binaries/eight test functions.

Candidate v13 is printer-only relative to v12: raw pos/end != u32::MAX and
record.parent.is_some() replace !changed for string spelling; textSourceNode
remains earlier in the selection; printer neverAsciiEscape participates in
the existing cooked-string quoting path. The existing source-range helper
validates actual raw offsets and writes source bytes verbatim. No new storage,
reset, inherited flag or public ABI is introduced. Private bundle bytes are
unchanged. Patch SHA-256
f391fb4dade7a634aa92c998fdb6530c979d1a34debef984a8d75a0f522b9b7a;
applied printer SHA-256
40c39fa01a47d47f7585c682f081bddb8003f2f55d7c41a347051efa1cbbcd94.
Only target drafts exist; no native v13 result is claimed.

E-ARENA and E-METADATA-BASE are unchanged representation premises for these
input adapters; E-STRINGS is modified-requalify. Their dated architecture rows
still require fresh qualification before production use. UTF16 output storage
is a new open finding: TextWriter.output and PrintedText.text are String;
quote_javascript_string escapes even an unpaired unit when neverAsciiEscape
is true. The12 source outputs cannot be represented losslessly by the current
return type. Preserve their failures and design a typed lossless writer/output
boundary; do not silently substitute UTF8 replacement bytes or omit controls.
General textSourceNode recursion/parent eligibility, raw range failure order,
termination options, metadata setter failure and whole A40 readiness stay open.

After before43, freeze its actual complete outcomes, publish the v13 patch and
pin the strongest completed full42 predecessor before after44. Run the existing
72 string_literal_identifier_source_contract controls as an adjacent owner
check, then emitter494/451/1350 and the required final full530 comparison on
the actual final candidate. Retain failures rather than changing their source
observations. Do not claim whole-emitter or H2.8 completion from this scope.

The initial native attempt43 failed to compile its new test: NodeFlags was
imported from tsc_syntax instead of tsc_types. Exit101 in7.027 seconds, no
executed binaries; the complete observation is retained in
`ratchets/h2-8a-literal-parent-compile-attempt.v1.json`. Correct only that test
import and use fresh attempt44 for v12 native-before; the planned v13 after
therefore moves to45. Attempt43 supplies no semantic or runtime evidence.

Native-before44 on v12 completed with1224/1279 exact twice and55 failures,
exit101 in14.634 seconds. All1151 preceding observations are unchanged after
the JSON input-parent projection; all700 tree states,508 factory states and94
event traces are exact. Of128 new literal rows,54 differ in output; the old
JSONUnicode row still differs. The
[before receipt](../../../../ratchets/h2-8a-literal-parent-direct-before.v1.json)
retains all2558 attempts and seven executed binaries. v13 is now published at
the frozen patch path, with the same hashes recorded above. Attempt45 runs
the expanded direct comparison; no v13 runtime success is claimed yet.

After45 on v13 completed with1267/1279 exact twice and12 failures, exit101 in
29.420 seconds. It repairs43 of the55 measured differences, including the
original JSONUnicode case; all1151 controls predating this amendment are now
exact. All700 tree states,508 factory states and94 event traces remain exact.
The [after receipt](../../../../ratchets/h2-8a-literal-parent-direct-after.v1.json)
retains every output, UTF16 unit sequence and executed binary. The remaining12
cases are exactly the unpaired-unit outputs under neverAsciiEscape: native
still writes the escape's eight units including quotes, while TypeScript
writes three units including quotes. These failures require the separate
lossless generated-text design; no global or whole-owner completion follows.

Adjacent46 passes all72 existing string-literal identifier/text-source controls
twice, exit0 in9.379 seconds. Its
[receipt](../../../../ratchets/h2-8a-literal-parent-neighbor-checks.v1.json)
verifies the exact case set, fixture, complete archive and executed binary.
Its source observer was freshly checked before the native run. General emitter
checks on v13 are next; the lossless-output failures remain open.

Adjacent47 on v13 passes494 units,451 contracts and1350 declaration reprint
rows, exit0 in41.969 seconds without warnings. The
[emitter receipt](../../../../ratchets/h2-8a-literal-parent-emitter-checks.v1.json)
retains1005 copied inputs,109 vendor inputs and both executed binaries.
The latest completed full530 comparison remains v12/full42; a final full
comparison is still required after the remaining generated-text work. No
v13 full-corpus, activation or qualification claim is made here.
All fourteen source-observer commands, including the lossless JSON projection
and72 adjacent text-source observations, were freshly checked after47.

### Lossless generated text and literal escaping amendment

The12 direct failures require preserving generated UTF16, not just cooked
metadata. Fresh `observe-utf16-writer.mjs --check` freezes48 programs with all
intermediate writer states (LF/CRLF/single-line; paired/split/reversed/unpaired
units, empty raw/ordinary/comment writes, all ordinary writer aliases,
indentation, comment flags, forced lines and clear/reuse). Artifact SHA-256:
787d8e76f56ec71aca18fb5b35da8362fafc922fb831ae2633ac53bc10cc770f.
The initial source probe found createSingleLineStringWriter is private, so the
observer exposes that exact lexical function by adding one export property in
an in-memory copy of pinned typescript.js. Both original and instrumented
compiler hashes are retained; no writer worker is changed. This48 population
is source-only until the new typed UTF16 write surface exists in the candidate.

`observe-utf16-literal-escaping.mjs --check` additionally freezes288 factory
print rows:12 inputs x double/single/four template token kinds x both printer
neverAsciiEscape and node NoAsciiEscaping flags. It covers every ASCII control,
U+0085/2028/2029, NUL followed by a digit, literal quotes, backslash, template
substitution, CRLF and bare LF, pairs and unpaired units. Full cooked tree
values, generated UTF16, UTF8 projection and final positions are compared.
Artifact SHA-256:cbd9f34c0889a4c47ecf03b6a3717858895d9d45fc95c9315c4f11e8b5e41fed.
Native-before48 runs v13 plus these288 controls:1567 total,3134 executions,
988 tree states,508 factory states and94 event traces in eight binaries.
All1279 previous observations, including their12 failures, are retained.

Upstream authority is createTextWriter (`_tsc.js:16365-16461`),
createSingleLineStringWriter (`12672-12703`), getIndentString/getIndentSize
(`16355-16364`), and the previously pinned literal owners. The writer owns
output, indentation, pending line-start state, line count/position and trailing
comment state until clear/reset. updateLineCountAndPosFor measures each
appended chunk; splitting CRLF across writes therefore remains observable.
String concatenation can pair a high and low surrogate from separate writes.
Escaping (`16274-16319`) preserves unpaired units when ASCII escaping is off;
vertical tab uses \\v and U+0085 always uses \\u0085. Backtick escaping retains a
bare LF but escapes a CRLF pair together; escapeTemplateSubstitution follows
character escaping. Source rawText remains a separate lexical owner.

Isolated candidate files are writer.rs, printer.rs and its existing bundle.rs
file; root production stays unchanged. The writer file's unchanged root hash
must join the runner's base pins before candidate execution. Function steps:

1. Add private GeneratedText storage with a normal String and an optional
   complete UTF16 representation, promoted when a write contains unpaired
   units. The UTF8 projection is derived, never the source of promoted units.
   Append handles a low unit after a prior high by updating the projection's
   final replacement character while retaining both original units. Empty
   writes preserve that possibility. clear removes the logical text and resets
   the representation; clone owns independent buffers. Equality compares the
   logical UTF16 sequence, independently of promotion history.
2. Existing UTF8 writer methods retain their signatures and fast path. Add
   explicit UTF16 counterparts for ordinary/raw/literal/comment writes and
   the string-literal alias. They share the existing indentation, empty-input,
   comment and line/position transitions. A shared measurement routine consumes
   UTF16 units (encode_utf16 for UTF8 callers, copied units for UTF16 callers)
   using computeLineStarts (`_tsc.js:8250-8277`): CRLF coalesces within a chunk,
   and CR/LF/U+2028/U+2029 end lines. It retains only the count and final start
   consumed by updateLineCountAndPosFor, avoiding a temporary line-start array
   or a replacement-string allocation.48 exact transition traces and adjacent
   writer checks must verify both input routes. text() is the UTF8 projection, and
   text_utf16() exposes the actual units without normalization.
3. PrintedText owns GeneratedText and exposes the same two views. All four
   writer-copy constructors in printer.rs/bundle.rs and the canonical file
   return path retain the generated value. System helper insertion preserves
   prefix/suffix units while using its existing ASCII/UTF8 delimiter search;
   convert the validated insertion byte boundary to a UTF16 offset before
   insertion and remeasure the actual units. Existing map refusal is retained.
4. String and cooked-template quoting return UTF16 units through the existing
   JavaScriptString type. One unit-based escaping worker implements the pinned
   quote/control/non-ASCII rules, including NUL lookahead, CRLF and template
   substitution. Literal writers consume those units directly. Existing raw
   UTF8 token spelling follows its current route; no value is reconstructed
   from final emitted text. Native literal observations then read text_utf16(),
   leaving their immutable expected arrays and byte/position checks unchanged.
5. Freeze actual before48, stage and pin the cumulative candidate, install48
   writer controls and execute all1615 controls twice in nine binaries. Verify
   all old positives and complete failed observations, then72 literal-source
   neighbors and494/451/1350 adjacent checks. Final full530 must compare all
   tuples to completed v12/full42. No previous failed run is discarded.

The completed before48 receipt is
`ratchets/h2-8a-utf16-literal-escaping-direct-before.v1.json`, SHA-256
5d7fa1d17d2d19ba25488f7498169fcc5f88d9890add2d59534c5dba20cd9050:
1461/1567 exact twice,106 failures; every1279 prior row individually unchanged,
including its12 failures. The added288 produced194 exact and94 failing rows.

Candidate v14 is frozen in
`h2-8a-list-intervening-printer.candidate-v14.patch`, SHA-256
42eb23af16878692f1b99802e3318520a9c5f99196b231cbcbeb0e05f3409863.
Applied printer SHA-256:
5cb51c0d750cc8ef68f55749169be7adf6e2caa6c8e105745cacf2f9077768c7;
bundle:f0070263f5748d2fe7bdda43b59eeba48fd58e545b53e0cffcb0a2d4dab58c0a;
writer:1fdc8e5da9b839d8f45897cf042115d73b7817e8496671f8ef2eb4b01ec2fb2a.
The shared per-chunk measurement consumes the complete source algorithm in
`_tsc.js:8250-8277`, SHA-256
147e073a0b43a88a9f85025067b10b6b4ce1f11b28549970fca1e71eee8d93e8.

`python3 scripts/run-comma-printer-design-experiment.py 49 factory-direct
--factory --list-owner` completed with actual exit0 in102.20566350000445s.
All1615 controls matched twice (3230 native attempts):988 input trees,
508 factory states and94 event traces also exact. All1461 prior positives
were preserved and all106 prior failures repaired; prior fixture hashes are
unchanged. The48 writer rows include every intermediate state and compare
both all-UTF16 and mixed UTF8/UTF16 input routes. A separate native property
test proves chunk-independent equality, clone/clear independence, and unequal
JavaScript values with equal UTF8 projections; it earns no additional source
case credit. Nine binaries ran eleven test functions.
`ratchets/h2-8a-utf16-writer-direct-after.v1.json`, SHA-256
84b679b1be9607d7957117468063caa825c3aa477b9708a5080a83241ac19522,
retains the actual terminal result, executed binaries, input archive and the
exact analyzer. Before48 and after49 analyses are immutable. The runner now
pins the unchanged production writer before applying the isolated patch.

Adjacent50 passed all72 literal-source controls twice, exit0 in8.505378416972235s;
receipt `ratchets/h2-8a-utf16-writer-neighbor-checks.v1.json`, SHA-256
fd81d893849426820c09d57e16f9ae6021f13ca324372ba02ceb85d101082a26.
Adjacent51 passed494 unit tests and450/451 contracts, including all1350
declaration reprints, but exited101 in104.13500695803668s. Its one failure is
retained in `ratchets/h2-8a-utf16-writer-emitter-failure.v1.json`, SHA-256
ec418f0f9a695341b419f702b00a9ebbe668128937890fbf04f8dfbeb9128eaf.

Fresh `node scripts/observe-template-fragment-provenance.mjs --check` freezes
the exact eight combinations used by that legacy contract: four template
token kinds, the same cooked units D83D/DE00/D800, and both printer escape
settings. It observes synthesized tokens and their separately parsed lexical
comparators, including actual units, bytes and positions. Fixture
`template-fragment-provenance.json` SHA-256:
ff3d21c4bc12bb9b3ff39d2ec9e02015d712082550039185670705237a8300a3.
The four NoAscii outputs differ in TypeScript: a synthesized token retains
D800, while the parsed raw token retains its backslash-uD800 spelling. The
old native-to-native equality assertion was therefore incorrect; the revised
contract compares both independent outputs to these source observations.
It retains all eight old inputs and repeats each comparison twice. This
test correction changes no candidate production code or source expectation.

Tests using the new writer APIs are staged in
`h2-8a-utf16-writer-tests.candidate.patch`, SHA-256
549a3817b6ac55d62c40b9faf302c904e15aef1cfd30f36f6e929a1d9139c46b.
The runner applies this only in the isolated workspace, after the production
candidate: it creates utf16_writer_contract.rs, changes the two direct literal
observers to use text_utf16(), and replaces the legacy declaration-template
equality assertion with the source-derived contract. Root tests continue to
compile against root APIs. The two direct test bodies and new writer body
must remain byte-identical to their actually executed after49 copies; the
declaration contract is the only changed executed test. Root base hashes and
the new test's root absence are checked explicitly and retained in prelaunch.

Adjacent52 re-executed the corrected contracts and completed with actual exit0
in26.445854375022464s:494 unit tests,451 contracts,1350 declaration reprints,
and all8 source-derived template provenance rows twice. Receipt
`ratchets/h2-8a-utf16-writer-emitter-checks.v1.json`, SHA-256
5da692f2df41acbfc10740f43333795b7b5b1a4fc5a2436293699d36b323f0e5.
Comparing its complete copied input table to51 proves that only the revised
declaration contract changed and only its new fixture was added. All
candidate production bytes are unchanged; the library binary is identical.
Comparing52 to49 separately proves all five candidate production files and
the three relocated direct test bodies are byte-identical. Root's two direct
literal test bodies match the API-compatible before48 bytes exactly. The
sixteen previous source-observer checks and new template-provenance check
also passed. Full530 still requires a fresh execution against full42; direct,
adjacent, and full populations are distinct and cannot be added together.

The new state is owned by a writer/PrintedText, never global or stored in
node metadata. E-STRINGS and writer/printed-output boundaries are
modified-requalify; mounted arenas and cooked JavaScriptString are unchanged
representations. The executor and declaration sinks already request a UTF8
projection; their broader callback-value qualification remains open. Raw
template text from arbitrary UTF16 factory input, full textSourceNode
recursion, writer/map fault order, metadata annotation failures, performance
qualification and whole A40 readiness also remain unresolved. These are not
unproved API1 deferrals, and this candidate cannot activate production.

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
