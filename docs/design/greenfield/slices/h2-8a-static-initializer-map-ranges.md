# H2.8a A6-18: static initializer expression source-map ranges

The complete before is frozen at A6-17 `d7863caaad331660aee8d3bf3704273cab10e190`. No production edit is authorized until
104 fresh complete TS commands, two independent native before jobs, exact
upstream owner pins and readiness dispositions are frozen. The proposed sole
production path is crates/emitter/src/builtins/class_fields/downlevel.rs.

A6-17 retains ten JS/TS source-map first failures. Decoding an existing full
comparison shows the missing source position at generated column17, just after
`Foo.stat = 10`: TS maps back to source column23 (the property name's end), then
the statement maps the semicolon to column29. The native output lacks the
intermediate entry. This is a transform metadata producer issue; never edit
VLQ strings, discard maps, or qualify fields following the first failed boundary.

Upstream transformProperty ranges a static property's transformed expression to
the property name whenever the current class lexical environment has ANY
getClassFacts bit. Native FieldOperation currently requests that metadata only
for a legacy-decorated class. Existing ClassFactsPlan and constructor-reference
plans preserve several facts, but do not retain WillHoistInitializersToConstructor
for this consumer. The condition must include the source member categories and
target/mode predicates, not infer facts from a generated temporary allocated
later for expression sequencing. Class facts belong to the original class's
member scan before auto-accessor expansion. Nested classes must restore their
parent's environment. No global flag or printer-side inference is allowed.

The planned source scan preserves public initialized versus uninitialized fields,
private fields/methods/accessors, auto accessors, original abstract-member
exclusion and the exact target/mode hoist predicate. Carry a class-facts presence
value through the existing scoped PrivateEnvironment for declaration and
expression visits, using the already selected constructor-reference plan and
legacy-decoration fact. The metadata consumer retains existing name range
resolution, adds the actual property original-node provenance and AdviseOnEmitNode
at the same condition, and uses existing TransformArena methods with typed errors.
No new resolver query, host callback, cache, output sink or public API is needed.

Initial104 fresh complete commands cover20 TS shapes under ES5/ES2015 and both
assignment/define modes (80), four of those shapes under ESNext module in both
targets (8), and four boundary shapes under ES2022/ESNext with both field modes
(16). They include static-only and instance-only negatives, initialized and
uninitialized public fields, abstract members, parameter properties, private
members, auto accessors, static this/super, named/anonymous class expressions,
nested scope reset/restoration, comments, multiline/parenthesized expressions,
computed/quoted names and absent static initializers. Full ordered diagnostics,
all output bytes/paths/callback fields, source-map result, status and exit remain
unconditional. Preserve compiler-option output-control diagnostics.

Transform metadata research uses actual TS built-in output but transpileModule;
it is not complete-command qualification. It confirms name ranges/original
PropertyDeclaration/AdviseOnEmitNode for initialized/private/auto-accessor and
static-this shapes, the absence for static-only/set-mode uninitialized shapes,
and the define-mode uninitialized distinction. Research records remain under
target until the before is archived. Fresh/native observations determine the
actual owned and outside failure sets; no after success count is assumed.

After must replay all104 fresh commands, the complete32 A6-17 class-instance
commands including all ten retained map failures, original static/class ten,
and applicable prior class-flag/layout and import-name-map adjacent controls.
Emitter unit/contracts and existing source-map/printer boundary tests cover the
shared metadata consumer. Exact selection and counts will be recorded before
implementation. Other A owners, four standalone NoNestedComments controls,
B–E and hosted acceptance remain open. The A6-14 whole769 checkpoint is immutable;
focused results do not establish a new global count. Follow the schedule header's
user-authorized lightweight workflow and claim no historical full CI/walk.
Poll every job to real exit before canonical mutation.

A6-18-1 completes the existing getClassFacts member scan with its constructor-
initializer hoist bit. Read original abstract modifiers, classify each original
member before auto-accessor expansion, retain existing private constructor-
reference query order, and combine the exact set/define/private/auto target
conditions. Do not derive this bit from a planned runtime value: a parameter
property's local assignment does not make its synthetic field initialized in
getClassFacts's set-mode scan.

A6-18-2 retains the target conditions on static lexical facts: this only below
ES2022, super only from ES2015 up to ES2021. This is reached by the two ES5
static-super controls: native reserves/substitutes a class super reference
although upstream's class-field facts do not request one. The later ES2015
transform owns that lower target's super lowering. Preserve all other reference
allocation and static binding decisions, including selective private transforms.
This correction is part of the same getClassFacts producer, not a separate
static-super lowering algorithm.

A6-18-3 carries facts presence through each existing PrivateEnvironment, using
the selected constructor-reference plan, the legacy-decorated fact and the new
hoist bit. Never use a class-expression sequencing fallback temporary as proof
of class facts. For static public field operations this controls the property
original node, AdviseOnEmitNode and existing name source-map range. After the
statement inherits its own property metadata, clear synthetic comments on the
expression exactly as transformPropertyOrClassStaticBlock does. No new context
stack, cache, resolver callback or mutable printer state is added.

A6-18-4 also installs the missing ordinary-property statement map range.
moveRangePastModifiers explicitly uses node.name.pos for a property (not the
last modifier token), ending at the property end. Reuse and rename the existing
private property-range helper without changing its old private-field behavior;
the property producer uses original source identity and position. Preserve the
separate Parameter-original branch exactly. Existing typed range errors are
propagated. This fixes the define-mode leading map even when class facts are
empty, as witnessed by ES5/define/static-only. The observed component is in the
same transformPropertyOrClassStaticBlock body; no factory/printer edit is added.

E-METADATA-BASE and E-CAPTURE-CLASS-G are modified-requalify at this private fact
and metadata producer. E-ARENA, E-PROTOCOL, E-RESOLVER-BASE, E-METADATA-G-CLASS,
E-NAMES-BASE and E-PRINTER-BASE are premise-unchanged. The existing immutable
parsed arena, scoped source-member/expanded-private plans, typed constructor
identity, and metadata merge/range/comment channels remain authoritative. New
facts do not cross a public seam. Existing normal/error/panic lifecycle contracts
remain required; no new host/sink failure boundary is introduced. E-SYNTAX-FACTS
is not applicable: no scanner storage or persistent syntax fact is created.

Before execution first exposed a TEST ADAPTER gap: useDefineForClassFields was
not mapped into the existing CompilerOptions field. The initial scout executed
zero native commands (104 preparation panics), took0.66s and exited101. It also
started before a yielded formatter had returned its real exit, and is excluded
from qualification on both grounds. Its log, old adapter/source bytes, actual
process sample and disposition are archived outside the checkout. The adapter
now has exactly one added option mapping; assertions and TS expectations are
unchanged. Both qualification jobs ran only after the formatter/scout completed
and the adapter bytes were fixed. Do not count this scout as a native before.

The95 complete first failures are all source-map-result boundaries. The initial
scope review classified21 owned-only,49 mixed,25 outside-only and9 complete
positives. The final decoded review below corrects six conservative mixed
dispositions without changing any frozen before observation. These are
component dispositions, not a claim that later fields match. The map-difference
coordinates are recorded in the readiness witnesses; the native JS callback
bytes occur later in the comparator and remain unqualified for failed cases.
The separate outside map components are:

- local-class-export-publication-range: CommonJS ClassDeclaration publication
  in builtins.rs5238–5282 maps the assignment to original_declaration, while
  TS appendExportsOfDeclaration111743–111762 uses the export specifier name.
  The complete static-only ES2015 case isolates this difference on exports.Foo.
- es2015-abstract-class-leading-range: the ES5 class IIFE's leading position,
  produced before the static property operation; keep that class-lowering range
  owner outside this property-range change.
- private-method-helper-call-range: source positions inside the emitted private
  method call, separate from the static property's expression/statement ranges.
- class-expression-sequence-range: leading class-expression sequence/IIFE ranges
  and nested sequence return positions; never relabel those as a static-property
  name endpoint merely because both appear in the same source-map string.
- static-this-substitution-range: child receiver source positions in _a.stat,
  distinct from the enclosing static assignment's newly added name endpoint.
- native-class-static-block-and-constructor-range: ES2022/ESNext set-mode native
  class/block/constructor maps produced by class_fields.rs, outside the proposed
  downlevel.rs production path.
- native-auto-accessor-setter-range: ES2022 setter child/boundary maps, retained
  separately from the public static initializer and local-export publication.

These outside phases remain unqualified H2.8a work; no root-cause repair of them
is claimed here. Their complete cases stay in all before/after denominators.
Readiness means every observed component has an explicit scoped disposition,
not that all those outside mechanisms or map bytes have been fixed. After,
all9 fresh positives and all21 owned-only cases must be fully exact. Review
remaining mixed-case maps against the frozen before component coordinates;
never infer whole-command success by removing a mapping segment.

Required after projection:104 fresh +32 A6-17 CommonJS class-instance commands
+50 prior class flags +64 prior class layout +87 import publication references
+10 original static/class commands =347. Target30/104 fresh, all32 class-instance
(including ten formerly retained map failures), and all211 remaining prior
controls exact twice:273/347 total, with74 outside fresh cases still compared.
This is a target until measured; newly exposed later boundaries must be recorded
and resolved or explicitly retained before any closure claim. Run the emitter
lib and contracts suites, including map flags, metadata merge/original identity,
class scope, synthetic comments, standard/legacy decorators and private selective
ES2022 controls. They are existing product regressions, not generated unit tests
that mirror this implementation. Whole769, other A owners, B–E and hosted
acceptance remain open.

Frozen measured before:9 exact per job (four actual executions each),95 failed
per independent pair (two actual executions each),56.38s and50.99s,
exit101 for both jobs. All95 first vectors agree across jobs. TS104 were minted
twice and completely repeated by --check;44 TS5107 output controls are retained.

Pinned TS6.0.3 source spans in vendor/typescript-6.0.3/lib/_tsc.js:

- getClassFacts: 96844–96898, SHA256 `18ea59522a3e87f378c8b5682c5eb2172be55cba02380fc3b240acbf0f4dd388`.
- visitInNewClassLexicalEnvironment: 96921–96967, SHA256 `64423c7c89a2026a572faea7911cf786beb6725c7245d013d484fc7bbc4a7de2`.
- visitClassDeclarationInNewClassLexicalEnvironment: 96971–97045, SHA256 `07a4943badefc9b5d6d774a2d04dac4f3803e24852f8410d2bb735feef6fd6d7`.
- visitClassExpressionInNewClassLexicalEnvironment: 97049–97129, SHA256 `5885e805a286e1451a1c60771127ff84a6c108f88522eb2f90901c2703763319`.
- transformProperty: 97488–97500, SHA256 `c4e9fbf0eb6953a64ba8257f83a5a79f3f8d904f06c12336d30b94ad5cdfd847`.
- transformPropertyOrClassStaticBlock: 97444–97466, SHA256 `8135b6934f2b7c206c3f7558ad7e3907192380c5dc3236aeb83e1904f5252714`.
- getSourceMapRange: 25336–25339, SHA256 `7b45f9797ce3582eccac7e1a3469e63d81994d1aa38d604e7e28f671ece9329e`.
- setSourceMapRange: 25340–25343, SHA256 `23552aa8ff2afcb814f9ee33741f7bfb206dc595468594b4c4ef156f282bb5dc`.
- setOriginalNode: 25208–25217, SHA256 `8ef5d40b9635be7af9ec133e0cb89a40498944062d5e9570facb5c3468121129`.
- moveRangePastModifiers: 17311–17317, SHA256 `9d43119a4e2ea51f3f5a151f00816f7985c1781c9dc80cfd8e44f40807d3db9d`.
- transformClassFields option bindings: 95864–95874, SHA256 `4cfce5e944ab637781314b938ced635a606303732015ff3365593f6889f4700d` (binding fragment; not the complete transformClassFields body).

A6-18-5 repairs literal keys in the reached transformPropertyWorker define-mode
branch. The first complete A6-18 after is frozen separately:272/347 exact twice,
75 fresh map failures (29/104 exact), no losses among previous positives, and
all32 A6-17 class-instance commands exact. The owned quoted-name define case
still lacks source-map entries inside the defineProperty argument list. The
first-after record is ratchets/h2-8a-static-initializer-map-ranges-first-after.v1.json;
no273 success or final A6-18 closure was claimed from it.

Native property_key_expression has one caller, create_define_property. It clones
StringLiteral/NumericLiteral names. Existing NodeFactory::clone_node correctly
creates synthesized raw pos/end and retains original metadata provenance; that
API is not a range-preserving identity operation. TS transformPropertyWorker
97501–97575 returns propertyName itself for those literal names. Keep the existing
mounted literal TransformNode instead of cloning it. Parsed syntax remains
immutable, and no range/quote/value is reconstructed. Identifier conversion,
computed-expression selection, private-name and recovery branches retain their
existing behavior. No factory, printer, resolver API or output comparison edit.

Twenty additional complete TS commands isolate identifier, double/single-quoted,
numeric and computed-string keys under set/define and ES5/ES2015. They use a
global class plus void Foo, so CommonJS local-export statement maps are absent.
Declarations/maps/diagnostics/all bytes/callbacks/status and exits remain fully
compared. Both TS observations and the complete --check replay are identical;
ten TS5107 output controls remain. Two independent native before jobs at the
FROZEN FIRST-AFTER PRODUCTION BYTES are complete before this extra edit. This
is a distinct before checkpoint from the original104 at d7863caaa, never a
combined before count. The source hash is pinned to the first-after record.

Both literal-key native jobs have14 exact and6 source-map failures
(19.15s and18.29s,exit101). Every first vector and membership
agrees. Positives execute four times across the two jobs; failed cases execute
twice. The frozen record is ratchets/h2-8a-property-literal-key-ranges-before.v1.json. The six are define-mode double/single-quoted/numeric names,
both targets. Set-mode literal and define identifier/computed controls are
positives. Final targets:all20 literal controls exact twice; original104 should
reach30 exact and74 explicitly retained outside maps; all32+114+87+10 previous
controls exact twice. Combined367 complete commands with target293 exact twice,
then emitter lib/contracts. Actual measurements own final counts; preserve any
newly exposed later boundary and do not remove a failed complete command.

The existing eight architecture dispositions still apply. Literal reuse adds no
new context or storage; E-ARENA and E-METADATA-G-CLASS premises remain unchanged.
The two additional pinned TS functions are transformPropertyWorker97501–97575
hash fb5e7b8fdfc4fab54f8fdd4ea6f48902c80207af52647e23cb47491f0ce46edd
and cloneNode24436–24466 hash d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6.
The latter documents the unchanged clone semantics reached by the failed before;
this amendment changes the call-site decision, not cloneNode behavior.

A6-18-6 preserves the downstream expression placement override. The second
complete after reaches293/367 exact twice, including all20 literal-key controls.
However, decoded map comparison catches12 newly different property endpoints
inside already failing class-expression/nested cases. This candidate is not
closed: its source, complete logs and map-point comparison are frozen separately
in ratchets/h2-8a-static-initializer-map-ranges-second-after.v1.json. Existing
483 emitter units and451 contracts pass at that checkpoint, but do not excuse
those new differences. The original104 before already contains exact mapping
points for these endpoints; these witnesses remain unconditional regressions.

TS generateInitializedPropertyExpressionsOrClassStaticBlock97467–97487 calls
transformProperty, then deliberately replaces its property-name source-map range
with moveRangePastModifiers(property). Native class-expression extraction at
visit_class_expression unwraps the static expression statement, sets the original
and text range, and installs the existing comment-source anchor. Text range alone
does not override the explicit source-map range now produced by A6-18-3. After
set_original_and_range at this already typed PropertyDeclaration branch, set the
expression map from property_source_map_range(original). Preserve AdviseOnEmitNode,
comment ownership, private/static operation ordering, expression placement and
all non-property branches. No map-byte rewriting or printer/factory change.

The complete upstream function's SHA256 is
51a63f66258bcc0bd61a995b8952a78602998b82f909f47f8d12c24fc75761cb.
The existing helper ledger's97460–97487 span includes the preceding function tail;
correct that ledger to this complete body when adding the consumer. Architecture
and target counts remain unchanged. Final evaluation must repeat367 complete
commands and emitter lib/contracts, and prove that every retained map's extra
and missing decoded points are subsets of the original frozen before. The
second-after subset-check failure is retained; do not relabel its new points
as unrelated outside work. There are no new fixture inputs or expectation edits.

Final before-disposition correction: the six set-mode class-expression,
anonymous-expression and nested-resume controls already have exact property
endpoints in the immutable original before. Their reached class-facts metadata
is overwritten by inline placement in TypeScript; they are negative controls for
that decision, not evidence of a missing observable property endpoint. Initial
reachability-based mixed labels are retained as history but corrected to
outside-only in current readiness. Actual before components are therefore9
complete positives,21 owned-only,43 mixed and31 outside-only. All six corrected
first vectors match final output exactly, while still failing their retained
outside map boundary. The six define-mode inline controls do have an owned
property-inline-range-past-modifiers difference in the before, now explicitly
named. No before membership, bytes, failure vectors, fixture or runtime code is
changed by this classification review. Earlier intermediate archives retain the
initial disposition record and their actual failed subset comparison.

Final measured result:30/104 original fresh commands and all20 literal-key
controls are exact twice. All32 A6-17 class-instance commands, including their
ten previously retained maps, all50 class-flag,64 class-layout,87 import-name
reference controls and all ten original static/class commands are exact twice.
This is293/367 complete commands exact twice. Each of the74 remaining fresh
source-map failures executes once and retains its outside disposition; later
complete-command fields remain unqualified. All31 outside-only first vectors
remain identical to both original before jobs. Decoded comparison proves all
74 remaining maps have no extra or missing difference points beyond those in
the original before; all non-mapping source-map fields also match. In particular,
the12 new class-expression property endpoints from the second candidate are gone.
The unmodified subset-check script now exits0; its earlier failing result stays
in the immutable second-after archive. No failed command was excluded or its
expected output normalized to establish this result.

The contracts projection reports5 passed/1 failed in486.16s; the original
projection passes in43.84s. The combined compiler invocation retains
exit101 because of74 complete map comparisons. Emitter lib483 tests and451
contracts all pass (0.8s and2.2s,exit0), covering lifecycle,
metadata, class/private/decorator and printer behavior. The final record is
ratchets/h2-8a-static-initializer-map-ranges-after.v1.json. Frozen original before,
literal-key before and both intermediate candidates retain their own source
identities and measured results. This closes the static-property metadata and
literal reuse owner only. Remaining A source-map/declaration/import work, B–E,
whole-matrix final replay and hosted acceptance remain open. No new global769
count or historical full developer CI/certificate walk result is claimed.
