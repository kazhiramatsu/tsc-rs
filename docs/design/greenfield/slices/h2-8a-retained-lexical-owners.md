# H2.8a A6-40: retained lexical, computed and constructor owners

Runtime slice in research/design, **not implementation-ready**. Base
`1be68d07ed42fb58ee0ccce323f726fb2cc15d4b`, 2026-09-10. The full user goal is
H2.8a-e. This slice must implement the retained class-field owner's complete
generated-binding, lexical-environment, computed-name and initializer-result
behavior. Anonymous receiver replacement alone is insufficient.

The whole-file [Rust candidate](h2-8a-retained-lexical-owners.candidate.rs.txt)
now compiles in an isolated workspace with `cargo check -p tsc-rs-emitter --lib
--offline`. Its SHA-256 is
`15d6aa40ca6d6bda2f4083b79068fa7ce23492bff72d66c059d9384efda62b13`.
The [design-check receipt](../../../../ratchets/h2-8a-retained-lexical-design-check.v1.json)
freezes the exact candidate, all678 Rust/Cargo input pins, successful actual
exit0 and prior failed attempts. This checks types and API composition only:
production source is unchanged, no candidate runtime test was executed, and
no compatibility/admission/hosted claim follows from this check.

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

The [expanded before profile](../../../../ratchets/h2-8a-retained-lexical-owners-before.v2.json)
contains522 cases:389 exact and133 failed twice. SHA-256:
`9eb5b661a5dfce450a5e3006e798342970b71d20bea59a00525f275859331461`.
It preserves the immutable [506-case predecessor](../../../../ratchets/h2-8a-retained-lexical-owners-before.v1.json)
and adds only16 new edge controls; no506/522 baseline replay occurred. The
predecessor itself combines454 A39 captures with52 new controls. All277
predecessor source/Cargo pins match current bytes, and275 match the new job's
1035-pin manifest. The remaining two historical pins belong to the independent
`new-ci` workspace, which the compiler test does not build. The common comparator
and complete-capture code remain byte-identical; only registration changed.

| Population | Exact / failed twice | Execution provenance |
| --- | --- | --- |
| A39 retained/accessor/helper/alias/context454 | 360 / 94 | Reused frozen A39 complete captures;908 primary and908 supplemental executions, zero new executions |
| New lexical44 | 22 / 22 | New88 primary and88 supplemental native executions;88 fresh complete TypeScript executions |
| New private constructor-reference8 | 2 / 6 | New16 primary and16 supplemental native executions;16 fresh complete TypeScript executions |
| New lexical edges16 | 5 / 11 | New32 primary and32 supplemental native executions;32 fresh complete TypeScript executions; native binary bytes archived before any rebuild |

All new failures differ only in JavaScript and, where applicable, JavaScript
mappings, including the map JSON inside `emit_result`. Diagnostics,
declarations, status, exit products and typed-boundary behavior match. The four
TS1166 and four TS2695 diagnostics in computed-key controls remain in the
expected tuples. The16 edge controls also preserve108 TS1166,12 TS2464,48
TS2465 and48 TS2339 diagnostics across the unique observations. There are no
new upstream exceptions; the two historical
upstream exceptions remain outside complete-command equality credit.

The current owned repair set has76 failures, including every prior65 repair
and all11 new edge failures. Preserve all389 prior positives
and all57 complete successor negatives whose earlier decorator producer still
belongs to A6-41. The intended after minimum is465 exact twice, with all522
commands still compared strictly and every remaining failure retained. This
is an implementation target, not a qualification claim. The original769 and
class1228 populations are separate; no new global total is inferred.

Reproduction selectors on the current test are `producer` (the original454),
`lexical` (44), `constructor-references` (8), `edges` (16), and `all` (522), using
`TSC_RS_RETAINED_ACCESSOR_CASE_SET` with
`cargo test -p tsc-rs-compiler --test contracts --
retained_accessor_owners_match_complete_typescript_observations --nocapture
--test-threads=1`. Do not repeat a frozen before job merely to combine counts.

Fresh TS inputs and observers are
[lexical inputs](../../../../crates/compiler/tests/fixtures/retained-lexical-inputs.json),
[constructor-reference inputs](../../../../crates/compiler/tests/fixtures/retained-constructor-reference-inputs.json),
[edge inputs](../../../../crates/compiler/tests/fixtures/retained-lexical-edge-inputs.json),
[lexical observer](../../../../scripts/observe-retained-lexical-environments.mjs)
and [constructor observer](../../../../scripts/observe-retained-constructor-references.mjs),
plus the [edge observer](../../../../scripts/observe-retained-lexical-edges.mjs).
All use the unchanged complete TypeScript serializer, execute twice, and
preserve exceptions separately. Their frozen output paths use the corresponding
`retained-lexical-environments.json`, `retained-constructor-references.json`
and `retained-lexical-edges.json`
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

## Concrete staged implementation and checks

The candidate is a complete replacement for the one allowed production file,
not fragments requiring an implementer to invent missing call sites. Its
3853 lines include the existing transformer/downlevel dispatch and substitution
consumer, explicit retained state,32 reused helper bodies, and the following
new composed owners. The existing before506 observations remain unchanged.

| Order | Candidate owner | Input, output and completion boundary |
| --- | --- | --- |
| 1 | `ClassFieldsVisitor::new`, `RetainedClassFrame`, `RetainedClassFacts` | Supply resolver and alias-registry borrows; replace arbitrary node/array caches and source-global temporary strings with generated identities, source-name caches and per-class state. Parsed private names reserve the source collision domain. |
| 2 | `allocate_binding`, `materialize_lexical_environment`, `merge_lexical_environment` | Keep generated identity separate from declaration placement; consume context Identifier hoists into source-ordered prologue products. Every source/function owner restores its outer environment on Result propagation. |
| 3 | `visit_function_with_name_visitor`, `visit_retained_parameters`, `visit_iteration_body` | Evaluate headers outside function scopes; visit and lower parameters before bodies; handle all seven function and five iteration kinds. Input static blocks own a separate lexical environment. Generated initializer blocks are not revisited. |
| 4 | `retained_class_facts`, `visit_class_declaration`, `visit_class_expression` | Allocate constructor references before heritage/member visitation, preserve receiver nodes, register declaration aliases, and return declaration prologue/class/postfix statements. The retained expression branch preserves the source's potentially unassigned constructor temp. |
| 5 | `property_name_expression_if_needed`, `retained_auto_accessor_names`, `visit_class_member_name` | Recognize generated cache assignments before expression visitation; separate source-name identity from hoist timing; preserve raw expression maps and class-only pending-expression injection. Ordinary object-literal method names use the ordinary visitor. |
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

## Source review checkpoint and concrete candidate gap

The reproducible [source inventory](../../../../ratchets/h2-8a-retained-lexical-source-inventory.v1.json)
contains203 selected seeds and their nested callbacks:220 whole owners,
960 predicates and1436 call sites. Its
[observer](../../../../scripts/inventory-retained-lexical-owners.mjs) derives
the nearest registered parent from AST ancestry after registration, eliminating
the earlier seed-order dependency. Call order is explicitly AST syntax order;
it is not used as proof of execution order or callee resolution.

The [source review](../../../../ratchets/h2-8a-retained-lexical-source-review.v1.json)
maps five reviewed requirements to exact candidate function spans, source
owners and frozen witness IDs: function/default phases, lexical products,
iteration phases, computed caches and private-name domains. These are reviewed
semantic invariants, not blanket qualification of all branches of each general
source owner. Verify the current inputs and mappings with
`python3 scripts/check-retained-lexical-source-review.py`; the check explicitly
reports **NOT implementation-ready**.

Review found `A40-F-COMMA-FACTORY`: `inlineExpressions` calls
`createCommaListExpression`24382-24387 for more than ten operands. That factory
first applies `flattenCommaElements`24371-24381 once per supplied element.
It expands only a synthetic, non-parse node without `original`, `emitNode`, or
a materialized source `node.id`; a comma binary expands to its two operands,
and a comma list expands to its elements. This differs from recursive
`flattenCommaListWorker`, used when collecting pending erased-field effects.
The current staged `inline_expressions` creates the array directly and omits
this factory operation. The candidate must be amended before readiness.

Eight fresh direct TypeScript factory observations are embedded in the review.
Plain binary, nested binary and comma-list inputs expand one level; an original
node, emit metadata, materialized ID, parsed provenance or a source range each
preserves the operand. These are factory research observations, with zero
Program/native commands and no equality credit. Rust's always-present arena
`NodeId` cannot stand in for TypeScript's lazy `node.id` guard: first establish
the reachable provenance or a concrete typed representation. Complete Program
witnesses must cover a generated comma-valued computed member after more than
ten pending operands, plus adjacent preservation controls. Do not modify the
already frozen16 edge outputs or reuse the recursive flattener as a shortcut.

## Remaining readiness work

1. Mechanically close and disposition the whole upstream owner/caller/predicate
   graph, including named constructor references, statement-list results,
   function child-table ordering, constructor's two visitation phases,
   prologue merging and generated/private name domains. The source220 inventory
   is research input, not a ready semantic disposition ledger. Add the two
   comma factory owners and close their provenance predicates, as recorded in
   `A40-F-COMMA-FACTORY`; also close the named helper omissions in the review.
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
   the full522 comparator and unchanged494/451 emitter suites with1350
   declaration reprints, retain actual failures, and qualify only the complete
   measured owner scope. Hosted acceptance is still required before landing.

Use one heavy command at a time, taskpolicy background, nice15 and
CARGO_BUILD_JOBS=2. Poll a live handle; do not restart after an observation
timeout. Root is the sole writer. No fixture-specific production logic, output
substitution, handwritten expectations, permissive fallback or blanket flag
repair is allowed. H2.8a-e remains active throughout this work.
