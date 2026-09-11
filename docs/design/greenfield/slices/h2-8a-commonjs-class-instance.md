# H2.8a A6-17: CommonJS class instance clone identity

The complete before and owner dispositions are frozen. Readiness must pass
before production edits. Base is A6-16
`83d06cfbb21340e13ba6f43bd8c7cae0617661f2`. No production edit until two
independent native before jobs, complete TS controls, owner dispositions and
readiness have been frozen. The production paths are checker annotate.rs,
node_builder/statements.rs and node_builder/type_nodes.rs.

The two original ExportAssignedClassInstance2 commands emit a namespace-like
member export instead of a declaration referencing Foo. TS CommonJS assignment
combination preserves the class symbol only when the member count is unchanged,
and marks IsClassInstanceClone only if the input type is the class's declared
instance type. Native combination preserves the symbol but omits the marker.
The statement namespace-representability predicate also tests the raw CLASS
object bit instead of isClassInstanceSide; an anonymous clone does not have CLASS.
Both producer and consumer are required. Type-node serialization already has an
inline equivalent of the proper class-side predicate.

A6-17-1 adds the exact shared is_class_instance_side CheckResult<bool> predicate
next to the existing declared-class type queries in annotate.rs: CLASS symbol
and (declared-type identity or OBJECT with IS_CLASS_INSTANCE_CLONE). Switch the
existing type_nodes calculation to this helper with unchanged abort conversion,
symbol flags, value declaration, meaning, name/accessibility and following queries.
A6-17-2 adds the missing producer marker after existing aliases/type arguments
and propagating flags in combine_common_js_export_members. Its condition is
STRICT input identity with the result symbol's declared class type. Do not call
the broader class-side predicate here or mark every generic/class-symbol clone.
Preserve member merging, initial-size comparison, signatures, aliases, order and
existing CheckResult propagation. The source type is not mutated.
A6-17-3 replaces the namespace predicate's raw CLASS shortcut with the shared
class-side query AFTER the unchanged type-node provenance and index-info checks,
and BEFORE the existing declaration annotation check. A class instance then
uses the existing reference/anonymous-class-literal declaration path. No direct
output-text construction or special-case namespace statements.

Current unrelated predicate order (annotation/context before properties and
signatures) and its accessor type-equivalence check differ from TS's broader
body. This slice does not claim those unobserved edges are fixed or silently
change them. If the fresh controls reproduce another cause, retain/disposition
it before expanding the runtime edit. The added class query has its exact TS
position relative to the provenance/index/annotation dependencies.

All data stay in the current checker TypeTables/binder/NodeBuilderContext and
existing detached declaration arena. The clone is a new anonymous TypeId; the
original class type, source AST and module symbol are not relabeled. Existing
get_declared_type_of_class_or_interface caches and CheckResult errors are reused
without a new cache, host callback, lifetime or public resolver API. Propagate
private checker aborts through the current node-builder error conversion.
E-PROTOCOL, E-RESOLVER-BASE, E-METADATA-BASE and E-PRINTER-BASE are
premise-unchanged; E-CHECKER-FACTS-BASE is modified-requalify for the private
semantic class-instance fact. No new output sink or host fault edge is added.

Research evidence at this base: the temporary native query produced an anonymous
export type with CLASS symbol Foo, alias Foo, distinct TypeId from declared Foo,
and objectFlags16 (the clone marker is missing), twice. This probe is not complete
command qualification. Its extra-member raw-symbol query was EARLY: it omitted
declaration-entry CommonJS merged-export preparation and still showed only the
member property. Do not treat that result as the final emitted type. The original
Instance3 commands are complete positives in A6-14. Temporary test bytes were
archived and restored exactly; research source/log/results are outside the tree.

32 fresh complete TS commands cover two targets, JS ordinary/additional/same
member exports, constructor/object/local/factory/generic/derived/anonymous/private/
accessor instances, and TS instance/constructor/object/generic counterparts.
Expectations include maps and declarations, full ordered diagnostic messages,
paths, all bytes/callback fields, emit result, status and exit. Preserve the16
ES5 TS5107 output controls and the ES2015 JS generic control's TS7006 from its
inline constructor parameter JSDoc. Do not rewrite that source or suppress the
error; distinguish this input from a fully annotated JS constructor.
Before successes run twice per job, failures once per job; require two completed
independent jobs and never count duplicate panic rendering as execution.

After should replay all32, retaining independently proven outside failures, the existing ten original static/class declarations
(including the two target failures and additional-member positives), and the
prior local/ambient/class-order/repeated-target controls. All prior positives
must remain exact. Final counts and retained failures must be actual measurements.
The producer-only and consumer-only hypotheses do not authorize partial success.
Other A owners, four standalone comment residues, B–E and hosted acceptance stay
open. No new whole769 count is inferred from focused repairs. Follow the schedule
header's user-authorized lightweight workflow; no historical full CI or walk
claim. Poll every job to real exit before canonical mutation.

Initial complete before identifies12 exact,10 declaration-byte failures and10
source-map-result failures. The latter occur in both JS/TS static-field shapes:
JS instance/added-member/constructor and TS instance/constructor, both targets.
The mapping string lacks the static initializer's additional zero-generated-
column source positions (for example TS suffix EAAE,AAAL,CAAM vs native EAAE,CAAC).
All names/source files and other source-map fields in the inspected pair agree.
This also fails for constructor exports where the clone marker cannot apply;
retain as a separate emitter static-field map owner. Do not drop source maps or
claim later declaration fields from a failed complete command. Both independent before jobs now confirm these exact first boundaries.

The ten declaration failures are same-member/factory/derived/anonymous/accessor
JS instances, both targets. Existing local-instance/private-instance, object and
generic controls (including explicit TS7006) are exact. Require all ten declaration
failures and all12 positives to become/remain fully exact; target22/32 exact with
all10 unchanged static-field source-map boundaries retained until the following
emitter owner. Both original ClassInstance2 commands have no source maps and
must become complete positives, alongside original Instance3. These targets
remain unqualified until measured and do not establish a new global count.

Pinned TS6.0.3 owners (vendor/typescript-6.0.3/lib/_tsc.js):

- isClassInstanceSide: 50771–50773, SHA256 `8e21fadcbc33deb3417c74d765f9cb21010fabad5d134be360a3918935e4f1de`.
- getDeclaredTypeOfClassOrInterface: 57375–57403, SHA256 `b159a970fade450a929f147df283c2d536e3a3459c66ac6b6e9b9675173ef57c`.
- createAnonymousTypeNode: 51750–51810, SHA256 `0c4bd387aaaa40e88a957f74d475f7dc797d65b59c06df516af17e933713bf98`.
- isTypeRepresentableAsFunctionNamespaceMerge: 55083–55098, SHA256 `59d447d2e6b4f724aecd7cea606cb5a4f3c66955eb99ec2305be8b639de4afc4`.
- getInitializerTypeFromAssignmentDeclaration: 56348–56446, SHA256 `9e23e3cbf38ef08a8fa25529eaf36a4c7796bfa243d6f2ad834907e8cf5c5f5b`.

Frozen measured before:12 exact per job (four actual executions each),20 failed
per independent pair (two actual executions each),27.13s and
40.36s,exit101. All first failure vectors match across the two jobs.
The record is ratchets/h2-8a-commonjs-class-instance-before.v1.json; research query provenance is ratchets/h2-8a-commonjs-class-instance-native-query.v1.json and explicitly
excludes final-type claims for its extra-member rows. The32 fresh inputs retain
all fields unconditionally; ten map failures remain part of the test denominator.
After targets:22/32 fresh plus116 preceding local/ambient/class-order/repeated-
target commands and10 originals exact twice (148/158 complete commands with10
outside map failures), plus25 existing checker statement/chain/specifier units.
These are targets until measured. Failed fresh after commands run once per job;
their doubled panic rendering is not another execution. Compare each retained
source-map vector to its immutable before and make no claim for its later fields.

Final measured result:22/32 fresh commands are exact twice, fixing all ten
declaration-byte failures and preserving all12 previous positives. Each of the
ten retained static-field source-map failures executes once in the after job;
every first boundary and actual/expected vector is byte-identical to both
frozen before jobs. Later fields of those failed commands remain unqualified.
The prior116 local/ambient/class-order/repeated-target commands are all exact
twice. This contracts job takes149.75s and retains its expected failure
status; it is not reported as a passing suite. The ten original static/class
commands are all exact twice (33.49s,exit0), including both original
ClassInstance2 failures and both Instance3 positives. The combined compiler
invocation exits101 because of the ten retained map failures. In total148/158
complete commands are exact twice, with ten outside commands retained rather
than excluded. All25 adjacent checker statement/chain/specifier units pass
(0.03s,exit0). The after record is
ratchets/h2-8a-commonjs-class-instance-after.v1.json.

This closes only the class-instance clone identity owner. The static initializer
map owner, other A work, four standalone parent NoNestedComments controls,
B–E and hosted acceptance remain open. The A6-14 whole769 result stays immutable;
this focused result is not a new whole-matrix count.
