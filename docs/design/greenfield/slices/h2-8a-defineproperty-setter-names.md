# H2.8a A6-25: setter declaration parameter names

The initial one-file design below was amended by the
[binding-name prerequisite](h2-8a-binding-name-map-ranges.md) after its first
validation exposed declaration-map regressions. The final measured result is
recorded at the end; the initial observations and gates remain historical.

Base d12abe61cd90a45d45935bb057be65bf3dfd5abd, on the existing H2.8 train;
trusted train base 10748f6ee19ec083ce5748224930c5dcfbbd86df. Correct the checker
node builder's setter name source, query order and omitted-type branch.
Production is bounded to crates/checker/src/node_builder/statements.rs.
The other A owners, H2.8b–e, parser, binder, type inference, printer and comparison
semantics remain separately owned. This is a focused repair, not a global close.

The immutable main before has 88 complete TS commands: 22 source shapes across
ES5/ES2015 and CommonJS/ESNext. Every input has standard libraries, strict checking,
declarations, source maps, declaration maps, CRLF and outDir. Both native jobs
match 56 cases twice and fail 32 once with identical first vectors. The 24
setter-name differences are owned here. Eight quoted/computed descriptor methods
produce an extra readonly modifier through a different property branch; their
complete failures stay visible and require a later isReadonlySymbol/descriptor
producer repair. No name-predicate widening is allowed to conceal those eight.

Inline JSDoc in some main witnesses does not establish typed/private branches:
TS emits public any setters there. Four anonymous private TS class inputs report
4094 and suppress declarations, retaining their full command observations.
Rather than rewriting these inputs, a separate 20-case annotation supplement
uses proper multiline JSDoc: private/public actual JS setters, descriptor methods,
private TS accessors, and an exported alias of a private-setter JS class. Both
native jobs match 16 twice and fail four once, only in descriptor parameter names.
Its private branches emit real private setters with omitted parameter types.

The combined fresh profile is 108 cases: 72 adjacent-exact, 28 owned setter names,
eight outside readonly failures. The original jsDeclarationsGetterSetter command
fails twice; its full actual tuples differ only in one declaration write's three
parameter names. First fresh failure boundaries are callback bytes for main.d.ts;
earlier declaration-map writes match, but later fields are not qualified by
component agreement. First-job cloned-Program captures are supplemental artifact
executions, not complete actual tuples: 144 main plus 36 supplement. Across both
primary jobs, those captures and the two original commands, 542 native complete
comparison/capture commands execute. The 22 existing declaration-blocking cases
pass with 44 additional emit executions and command_reporting=false; these are
reported separately. All expectations and before archives remain immutable.

| TS owner/state | Current Rust gap or premise | Representation and action | Proof |
| --- | --- | --- | --- |
| makeSerializePropertySymbol setter selection | Only actual SetAccessor is searched | A6-25-1: ordered find_map over symbol declarations; actual accessor or bindable defineProperty descriptor member named by Identifier set | Repeated calls, duplicate descriptor members, function-valued properties, nonliteral descriptor arguments, quoted/computed names |
| isFunctionLikeDeclaration and getSignatureFromDeclaration | Descriptor method signature is never queried | Apply function-like guard after selecting the first member; no initializer following; first signature parameter or existing value fallback | Method/rest/destructuring/noarg/property controls and original command |
| setterDeclaration | Actual setter owns the type seam and range independently of name selection | A6-25-2: a separate first actual SetAccessor; no descriptor range substitution | All complete JS and declaration map writes; actual setter controls |
| symbolName, getEffectiveParameterDeclaration, parameterToParameterDeclarationName and nested cloneBindingName | Existing helpers are shared prerequisites | Borrow the same checker and arena; retain effective binding names and provenance | Existing node-builder tests, object/array/rest names |
| getWriteTypeOfSymbol and serializeTypeForDeclaration | Write type is queried before name construction, even when omit_type | A6-25-2: signature, length accounting, parameter name, then only when type is emitted query/serialize write type | Multiline typed/private controls and existing write_type cache unit |
| Debug.assert setter | Missing selected declaration must not silently become value | A6-25-3: expect at the source invariant; no new resolver method, CheckAbort class or recovery fallback | Internal TS invariant observation, native deliberate symbol corruption |
| cloneSymbol, mergeSymbol, instantiateSymbol, createUnionOrIntersectionProperty | Normal accessor declaration identity survives | Read-only prerequisite audit; existing clones copy declarations, merge extends in order, instantiation copies target, synthetic intersection/union collects constituent declarations | Whole source pins, existing generic/class tests and repeated-call witness |

A6-25-1 uses existing tsc_binder::assignment::is_bindable_object_define_property_call
and the local declaration_name helper, which delegates to node_util::get_name_of_declaration.
The call predicate proves arity and static names but not an object literal third
argument: match NodeData::ObjectLiteralExpression before reading its properties.
An absent property list is empty. Select the first declaration whose actual
Identifier text is set, then apply node_util::is_function_like_declaration_kind.
A PropertyAssignment with a function initializer remains a non-function-like
selected member and produces value; do not skip it to find a later method.

A6-25-2 keeps the actual setterDeclaration separately for
serialize_type_for_declaration_seam and range_member. The descriptor method is
only the signature/name source. After the signature query, account approximate
length, build the existing effective parameter name, and conditionally request
the write type. Use existing typed checker-abort/factory errors with their
propagation. Parsed trees stay immutable and generated parameter/statement nodes
remain arena-owned. The getter branch has a different schedule and is outside
this repair; its successful adjacent commands must remain identical.

A6-25-3 uses an ordinary invariant assertion for the missing setter, matching
TS Debug.assert rather than treating absence as the no-argument case. Binder
bind.rs binds actual SetAccessor nodes and gives descriptor symbols SET_ACCESSOR
only with an object literal containing an identifier-named set member. Clones
and instantiated symbols copy declarations, merged symbols extend them, and
union/intersection synthetic properties concatenate the original declarations
when retaining matching accessor flags. No legitimate missing-setter producer
was found. The TS internal observer deliberately clears both the property view
and the original class member's declarations: its normal control serializes a
setter, the corrupted control throws Debug Failure. This is an internal fault
witness, not a newly admitted source command. Clearing only a transient property
view was an ineffective research mutation and is not claimed as this fault.
The corresponding native unit corrupts a checker-owned clone because file-binder
symbols are immutable, and catches the specified invariant panic; production
factory/resolver failures still use their existing typed Result channels.

The other new native unit primes the actual setter signature and proves that
doing so leaves the property write_type cache empty. It then calls the real
property serializer for public/private multiline JS setters. Public serialization
must populate the write type and emit it; private serialization must preserve the
empty cache and omit the type. Both preserve the supplied name. No production
trace state or special test entry is introduced. This qualifies query omission;
the source sequence and complete command comparisons qualify ordering without
claiming a new runtime query trace.

Architecture E-RESOLVER-BASE is modified-requalify only for this checker-owned
internal serialization schedule, active-unqualified until focused validation.
The existing public resolver identities and methods do not change. E-PROTOCOL,
E-ARENA, E-CONTEXT, E-METADATA-BASE and E-POSITIONS are premise-unchanged: the
same borrow boundary, context lifetime, detached nodes, original/map ranges and
UTF-16 length conventions remain. Current Rust symbols are re-read before
readiness; historical lifecycle dates alone provide no qualification for the
missing setter branch. No planned/dormant concern is activated.

After qualification requires 213 complete commands: all 108 fresh cases, 96
existing JSDoc implements cases, the original getter/setter command and eight
original JSDoc implements commands. The target is 205 exact twice, with only the
same eight readonly failures. Keep those failures in the unchanged comparator.
Run the 22 declaration-blocking controls and all node_builder:: checker units,
including both new units. The new binary lists 62 tests for the substring filter
node_builder:: (51 node_builder and 11 syntactic_type_node_builder tests); run
all 62. This replaces the unminted target of 33, without reusing historical
31-test selections or the unrelated emit::tests:: namespace. Disable
supplemental captures after repair. Preserve unexpected failures before revising
anything; no focused successes may be subtracted from historical global totals.

Use two Cargo jobs and background priority. Record actual process exits before
source changes or dependent jobs, retain log/input hashes in outside archives,
and check readiness with unresolved=0 and undispositioned=0 before production.
The user-authorized lightweight schedule applies: focused comparisons while
editing and hosted acceptance before landing. No historical certificate walk or
full developer CI is claimed. All remaining H2.8 work continues on this train.

Whole pinned TS functions:

- symbolName: 11452–11457, SHA256 `201131264fe4f6f45c2da6248df265ae411ebc24acc96f4b77c6b676a48ade0a`.
- getNonAssignedNameOfDeclaration: 11517–11561, SHA256 `382ebe3aca3c5b65c264f1177b6f9ed47454cdc918c228f8469456fb504d617b`.
- getNameOfDeclaration: 11562–11565, SHA256 `5d3aafbdab871f0fe6f088a4904cd11e6b44e467e0cca8ad0c215b3f899b570b`.
- isFunctionLikeDeclaration: 11998–12000, SHA256 `404ab157d8b8bff3e824ad430826c813dabff35889efea1abc649dc35ad41b68`.
- isFunctionLikeDeclarationKind: 12004–12017, SHA256 `98d32c450062ebe6e646b6d040bb2eca00c5125e30f03ee890d3d24000678a58`.
- isBindableObjectDefinePropertyCall: 15059–15065, SHA256 `11b59e7edd8f66683d81b05c7488aa75a2404cf5328472fffb75d1a94f2cb2a9`.
- isBindableStaticNameExpression: 15086–15088, SHA256 `53acbb025bc0548bdb246e8d1665df9cada206e07d887c1612ef1486dfe2b6c0`.
- isStringOrNumericLiteralLike: 15844–15846, SHA256 `c4b0aff81eab867a2d799872ff7a55d26daeae2697511c247d141e352be2c42c`.
- cloneSymbol: 47696–47706, SHA256 `1a41af611deac3728405e13030e086a1fbdd56ccbeb7e8aea75c5489897c283c`.
- mergeSymbol: 47707–47783, SHA256 `1b2782c87ef6132c3927aeaa879b58dfaff7fcb1e192463b4d52ab115f868242`.
- getEffectiveParameterDeclaration: 52845–52853, SHA256 `2588afb7d3b8e6e07a54d982d06c2f7c2b2858fc29e3477e095fde223ae1dafe`.
- parameterToParameterDeclarationName: 52876–52909, SHA256 `f8c988288813b2b174b4e49718f6864d442cbfe82334ec499d89013ec3df06a4`.
- parameterToParameterDeclarationName.cloneBindingName: 52878–52908, SHA256 `d14170c43f6b359e6b695faf5357071724cd4133986d5621d68cf83b06867014`.
- serializeTypeForDeclaration: 53487–53508, SHA256 `61ebc9bf5f2f88bf1e2a94886d4878fb12a562ca515d046a7d782c34c54ce979`.
- makeSerializePropertySymbol: 55099–55229, SHA256 `fab03df95bee26e871b1bc817932eb99b58a9c262f37009266cc1a5291f817a7`.
- getWriteTypeOfAccessors: 56786–56803, SHA256 `5a40e99bfe94666a2fd8034dfe6e5f6823070a2e3c7990a72b56f3da8f16dc61`.
- getWriteTypeOfSymbol: 56929–56944, SHA256 `2c1e85a0c9e90a6b8acad4ad180acfd12a0afc3d8cc35e3216a643a6ab8f4380`.
- createUnionOrIntersectionProperty: 59101–59245, SHA256 `21791f74b0558b599db3de8950d26bd93152bbb62c3e250727503334167bf713`.
- getSignatureFromDeclaration: 59569–59651, SHA256 `f74d65ac24febb2f8dc4dfe2c0cc3fc26cfd01a4f004385457085decf95922cd`.
- instantiateSymbol: 63436–63462, SHA256 `3151a0198339a37ffdc3a8585ade0d5da549d926ba9a7fe013bd08c3919ba27a`.

Measured before native controls: both tests fail in 0.01 seconds;
the public cache control passes, the private cache is populated incorrectly,
and a corrupted accessor symbol silently emits instead of asserting. The full
launch, real exit and log are retained in the native-before archive.

Reproduce observers with `node scripts/observe-defineproperty-setter-names.mjs --check`,
`node scripts/observe-defineproperty-setter-annotations.mjs --check`, and
`node scripts/observe-setter-symbol-invariant.mjs --check`. Readiness is
`python3 scripts/check-defineproperty-setter-names-readiness.py`. Exact native
argv, environment and input receipts are frozen with every actual exit.

Final measured result with the binding-name prerequisite: 253 of 261 complete
commands match TS twice. All 28 fresh setter-name failures, 36 additional binding
clone failures and the original getter/setter command are repaired. The first
setter repair's eight new binding-map regressions are also closed. All previous
positive commands, 96 JSDoc-implements commands and eight original implements
commands remain exact. Eight separately owned readonly failures execute once
with exactly their before first vectors. Their unconditional comparisons retain
the contracts target's expected exit 101; this is a focused owner close.

The final run executes 514 complete native commands with no supplemental captures,
plus 44 separate declaration-blocking emit executions. All 65 tests selected by
node_builder:: pass: 54 node_builder and 11 syntactic_type_node_builder tests.
They include omitted private write-type queries, the expected missing-setter
assertion, recursive binding ranges/flags/origins, initially unmounted source
remapping and the typed unknown-target error. The 22 declaration-blocking cases
pass. The real combined exit is 101 for the retained readonly differences;
test-target durations in order are [0.05, 0.0, 275.35, 24.2] seconds. No new emitter-suite run
is claimed, because emitter production surfaces are unchanged.

The final record is ratchets/h2-8a-defineproperty-setter-names-after.v1.json. Production changes are bounded
to statements.rs and signatures.rs. The input adapter projects declarationMap;
complete comparators and frozen TS expectations are unchanged. Original
one-file readiness and its failed first-after observation, plus the expanded
prerequisite readiness, remain in immutable archives. Other A owners, B–E, the
final global replay and hosted acceptance remain open. Historical global totals
are unchanged; no full developer CI or certificate walk is claimed.
