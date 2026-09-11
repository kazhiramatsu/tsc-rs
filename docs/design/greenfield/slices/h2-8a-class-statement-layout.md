# H2.8a A6-8 amendment: class statements and generated binding scope

The class-flag repair exposes further reached owners. Its first after is
46/50 fresh exact (61.37 seconds), all 98 builtins unit tests pass (0.13
seconds), and 5/10 originals match (33.19 seconds). The four known declaration
residues remain unchanged. ComputedNames ES5 additionally exposes two distinct
computed-key temporaries both printed as _a after moving them into the class
IIFE. All actual/expected tuples and source bytes are preserved outside the
checkout; ratchets/h2-8a-class-transform-first-after.v1.json records the run.
This is an incomplete first after, not a qualified repair.

A new immutable 64-command observer crosses JS/TS and ES5/ES2015 with sixteen
shapes: first/middle/last/only static class statements, multiline body, arrow,
function expression, method, explicit instance constructor, two instance
properties, class expression, ordinary method class, two static/instance
computed keys, two static keys, two instance keys, and computed keys with a
constructor parameter/local. Each TS command is repeated twice. At the class
flags implementation before this amendment, 32 native commands match twice
and 32 fail (70.65 seconds, exit 101): 28 layout failures and four ES5
computed-name collisions. All 64 remain mandatory after comparisons, as do
the original 50 and ten original-corpus commands with the four previously
recorded independent declaration residues. Expectations and membership freeze
at this before; failed fresh comparisons stop at their first tuple difference.

A6-8-4, printer.rs: supplement existing function-body single-line eligibility
with the upstream synthetic statement newline predicate. A synthesized
statement marked startsOnNewLine makes a PreserveLines leading, separating
or closing boundary nonzero; a first or sole statement is included. Explicit
EmitFlags::SINGLE_LINE retains precedence. This narrowly fills the missing
synthetic-metadata branch; it does not claim the unexposed preserveSourceNewlines
printer option or replace all source-range/list algorithms. Existing compact
list emission retains its normal startsOnNewLine separator for positioned
statements. No class-name-specific printer condition is permitted.

A6-8-5, builtins/class_fields/downlevel.rs: materialize_field_operation must
not mark every public field initializer statement startsOnNewLine. Upstream
transformPropertyOrClassStaticBlock leaves that statement bit absent, whereas
generateInitializedPropertyExpressionsOrClassStaticBlock explicitly marks its
EXPRESSION sequence. Keep the expression producer and constructor's own
multiline flag. This change removes only the incorrect statement bit, retaining
current source/map/comment identities, private-field branches and field order.
Both static and instance callers are covered by complete controls.

A6-8-6, builtins/target_bindings.rs: perform generateNames(body)'s declaration
name preparation before walking each function body's emission events. The
current event walk sees the instance-key reference in the constructor before
its enclosing IIFE's hoisted declaration; it allocates that identity in the
constructor scope, then independently allocates _a for the static key outside.
Upstream first names body declarations without entering nested function bodies,
then emits child scopes with cached binding identities. Preserve existing
GeneratedBindingId, reserved-in-nested-scopes policy, numbered-name phases and
name caches. A typed traversal follows generateNames' statement/container and
binding-name branches, pruning initializers and ordinary nested function bodies;
ReuseTempVariableScope function declarations retain upstream body traversal.
Reuse-flagged function expressions retain the existing naming-moment boundary.
This is declaration preparation at the existing function-body owner, not a
source-position sort or a shared counter across independent lexical scopes.

Allowed additional production files are exactly the three above. builtins.rs
retains the already observed class-flag implementation. No checker, parser,
module resolver, factory allocation-ID, source position or expected text edits.
E-NAMES-BASE is modified-requalify through the existing composed-tree event
producer. E-METADATA-BASE, E-ORDER-H and E-PROTOCOL are premise-unchanged and
rechecked: sparse metadata and independent identities, pass/hook ordering and
request borrows remain intact. No new persistent syntax fields are introduced.

Readiness binds the existing first-after record, all 64 new complete rows and
the exact working files used for their before. After edits run the 114 fresh
commands, original ten full tuples, builtins/target-binding and printer-adjacent
unit tests. Any additional failure requires actual owner review; none may be
accepted by editing expected results. Global 769 and H2.8a–e stay open.

Pinned TypeScript 6.0.3 bodies in vendor/typescript-6.0.3/lib/_tsc.js:

- shouldEmitBlockFunctionBodyOnSingleLine: 118999–119020, SHA256 `f1644748bb2314796a601992b80c26925e740f7bd10b23e23300df3614367b1a`.
- emitBlockFunctionBody/Worker: 119021–119058, SHA256 `f58d647154d3a61a8bf567fb20c623f25f12664143819599b6f660f275b47acb`.
- getLeadingLineTerminatorCount: 120268–120300, SHA256 `893244ddd50971f9938c07f3bb0b10b520dfbf70880816b69aa4b00cc1384819`.
- getSeparatingLineTerminatorCount: 120301–120329, SHA256 `78d63da04f114ae40f8ad9f012131e94a83000cf268d393c5608372aab734539`.
- getClosingLineTerminatorCount: 120330–120360, SHA256 `30b0be7e1586d76d08caeaa3f4605323147137e9c73a0bd825dd3c92881635dc`.
- synthesizedNodeStartsOnNewLine: 120398–120407, SHA256 `03f9387e93e3deb4d868a65c6cb8c1d181b323dd67ab2f80472953c0b462b427`.
- nodeIsSynthesized: 16000–16002, SHA256 `d5eb53abaa73cfcae3a8c02425faff560733177bd619f5e0db719a6f9f603f4c`.
- transformPropertyOrClassStaticBlock: 97444–97466, SHA256 `8135b6934f2b7c206c3f7558ad7e3907192380c5dc3236aeb83e1904f5252714`.
- generateInitializedPropertyExpressionsOrClassStaticBlock: 97467–97487, SHA256 `51a63f66258bcc0bd61a995b8952a78602998b82f909f47f8d12c24fc75761cb`.
- generateNames: 120515–120599, SHA256 `d8f85515528b33d4f544998d474f766975c709f643f6bdf73ca4af830e77a608`.
- generateNameIfNeeded: 120615–120623, SHA256 `de8554d7896912a89a31819d777cf68c5e4f0ffa983f290b59ced9bfbe423340`.
- generateName: 120624–120632, SHA256 `5418a174d1ef0e409435e86d45f5ce273fcf55d5a1ae8895463b7d5e56e14330`.
- pushNameGenerationScope/popNameGenerationScope: 120480–120502, SHA256 `f033f6151e7c0c4e962e58904afbff8e09ec68dfbc2a48ed78bf1e8734fa7d0c`.
- visitTypeScriptClassWrapper: 107687–107783, SHA256 `cd917e405f67b9f0859344acdb38cb908312e37cb2b1bb466bff69ffc3f3c19e`.

The completed amendment after matches all 114 fresh complete commands twice
(two tests, 124.43 seconds, exit 0), including every initial layout
and computed-name failure. All 483 emitter unit tests pass (0.65
seconds), and all 451 emitter contracts pass (1.40 seconds).
The original ten-row comparator executes both repetitions and has six exact
commands; the four previously recorded declaration-text residues remain failed
whole commands (35.53 seconds, exit 101). An explicit complete-tuple
review confirms all ten JavaScript outputs now match; all other fields match
in the four failures, and their declaration differences are identical to the
immutable before. The unconditional original comparator is retained.
The bounded after record is ratchets/h2-8a-class-layout-after.v1.json, with
source/log/tuple copies at target/h2-8a-class-layout-after-location.txt.
Global 769, remaining output work and H2.8b–e remain unqualified.
