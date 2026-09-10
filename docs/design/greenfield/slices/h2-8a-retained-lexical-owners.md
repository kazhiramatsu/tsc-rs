# H2.8a A6-40: retained lexical, computed and constructor owners

Runtime slice in research/design, **not implementation-ready**. Base
`1be68d07ed42fb58ee0ccce323f726fb2cc15d4b`, 2026-09-10. The full user goal is
H2.8a-e. This slice must implement the retained class-field owner's complete
generated-binding, lexical-environment, computed-name and initializer-result
behavior. Anonymous receiver replacement alone is insufficient.

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

The [before profile](../../../../ratchets/h2-8a-retained-lexical-owners-before.v1.json)
contains 506 cases:384 exact and122 failed twice. It explicitly combines the
454 unchanged A39 captures with only52 new native controls; it is not a new
506-case execution. All production Rust and Cargo inputs match between those
jobs and the current tree. The common comparator and complete-capture code
remain byte-identical; only fixture registration and selectors changed.

| Population | Exact / failed twice | Execution provenance |
| --- | --- | --- |
| A39 retained/accessor/helper/alias/context454 | 360 / 94 | Reused frozen A39 complete captures;908 primary and908 supplemental executions, zero new executions |
| New lexical44 | 22 / 22 | New88 primary and88 supplemental native executions;88 fresh complete TypeScript executions |
| New private constructor-reference8 | 2 / 6 | New16 primary and16 supplemental native executions;16 fresh complete TypeScript executions |

All new failures differ in JavaScript and JavaScript mappings. Diagnostics,
declarations, status, exit products and typed-boundary behavior match. The four
TS1166 and four TS2695 diagnostics in computed-key controls remain in the
expected tuples. There are no new upstream exceptions; the two historical
upstream exceptions remain outside complete-command equality credit.

The current owned repair set has65 failures. Preserve all384 prior positives
and all57 complete successor negatives whose earlier decorator producer still
belongs to A6-41. The intended after minimum is449 exact twice, with all506
commands still compared strictly and every remaining failure retained. This
is an implementation target, not a qualification claim. The original769 and
class1228 populations are separate; no new global total is inferred.

Reproduction selectors on the current test are `producer` (the original454),
`lexical` (44), `constructor-references` (8), and `all` (506), using
`TSC_RS_RETAINED_ACCESSOR_CASE_SET` with
`cargo test -p tsc-rs-compiler --test contracts --
retained_accessor_owners_match_complete_typescript_observations --nocapture
--test-threads=1`. Do not repeat a frozen before job merely to combine counts.

Fresh TS inputs and observers are
[lexical inputs](../../../../crates/compiler/tests/fixtures/retained-lexical-inputs.json),
[constructor-reference inputs](../../../../crates/compiler/tests/fixtures/retained-constructor-reference-inputs.json),
[lexical observer](../../../../scripts/observe-retained-lexical-environments.mjs)
and [constructor observer](../../../../scripts/observe-retained-constructor-references.mjs).
Both use the unchanged complete TypeScript serializer, execute twice, and
preserve exceptions separately. Their frozen output paths use the corresponding
`retained-lexical-environments.json` and `retained-constructor-references.json`
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

The proposed production boundary is `crates/emitter/src/builtins/class_fields.rs`.
Do not edit it yet. Reuse the existing context and generated-binding types where
their complete semantics match; do not import downlevel extraction or its String
receiver representation. Any required additional file must be named and gated
before an edit. Standard-decorator handoff/private-static/descriptor/receiver/
naming/super remains A6-41, with the complete57 guarded negatives preserved.

## Remaining readiness work

1. Mechanically close and disposition the whole upstream owner/caller/predicate
   graph, including named constructor references, statement-list results,
   function child-table ordering, constructor's two visitation phases,
   prologue merging and generated/private name domains. Existing source136
   inventory is research input, not a ready semantic disposition ledger.
2. Finish the concrete function-level Rust edit sequence and all factory/raw/
   map/comment/flag operations. Specify lexical/block error restoration and
   every initializer-result caller. Verify context variable-declaration inputs
   against its existing consumers instead of guessing from its method name.
3. Revalidate every architecture row and frozen predecessor used as a premise,
   including the finalizer's name policy and source/parsed collision domains;
   build the exact allowed-files, row-to-step and witness mappings.
4. Freeze the staged source design and run a new mechanical readiness checker
   with unresolved=0 and undispositioned=0 before production changes. Then run
   the full506 comparator and unchanged494/451 emitter suites with1350
   declaration reprints, retain actual failures, and qualify only the complete
   measured owner scope. Hosted acceptance is still required before landing.

Use one heavy command at a time, taskpolicy background, nice15 and
CARGO_BUILD_JOBS=2. Poll a live handle; do not restart after an observation
timeout. Root is the sole writer. No fixture-specific production logic, output
substitution, handwritten expectations, permissive fallback or blanket flag
repair is allowed. H2.8a-e remains active throughout this work.
