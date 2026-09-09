# H2.8a A6-28-5: initializer trailing-comment ownership

The shared prerequisite is qualified on the current train candidate. The
[frozen full comparison](../../../../ratchets/h2-8a-class-field-initializer-comments-shared-after.v1.json)
has434/556 exact twice, including all64 new controls and8 prior field-comment
repairs; all362 earlier positives remain exact. The122 remaining failures retain
114 unchanged first comparison vectors and8 unchanged typed boundaries. The
[emitter qualification](../../../../ratchets/h2-8a-class-field-initializer-comments-shared-regressions.v1.json)
has all490 units and451 contracts passing, with exactly the documented unit
rename. The first emitter test build failed on two stale private-field
initializers in comment_scope_predicate/tests.rs; the failure was frozen and
only those two initializers were deleted, preserving every assertion/test ID.
There were990 primary command executions and no supplemental captures.
At this prerequisite A28 retained20 owned failures and102 outside cases.
The subsequent [promoted/export-map correction](h2-8a-promoted-class-export-maps.md)
repairs16 of those targets and six additional ES2022 cases, with all490/451
emitter tests passing. The subsequent
[independent-comment endpoint qualification](h2-8a-one-sided-class-comments.md)
repairs the final four original wrapper-comment targets and passes all 494/451
emitter tests. A28 is 396/492 exact twice with all original owned targets
qualified and 96 outside cases open. These focused records make no new global
matrix claim; the following design retains its original pre-edit counts.

This shared prerequisite amends [A28](h2-8a-class-field-alias-map-positions.md).
Its first candidate is frozen at362/492 exact twice,164 repairs,28 owned
failures and no prior exact loss. All490 emitter units and451 contracts pass.
This is a partial A28 result; its original390 target is still open. The shared
change repairs32 new semicolon controls and8 of those28 field-comment cases.
The other20 need their own following source gates; all102 earlier outside
cases retain their owners. H2.8 A and B–E remain open.

The semantic base is A28's archived first candidate, not unchanged HEAD.
HEAD remains dc868373612af16ad299ed08bdec4ece437325dc, trusted train base
10748f6ee19ec083ce5748224930c5dcfbbd86df. The before manifest records exact
current hashes for downlevel.rs,metadata.rs,printer.rs and
factory/parsed_metadata.rs. Only downlevel.rs differs from HEAD, exactly as
archived by A28. All four are allowed production paths for this prerequisite.
No other transform, flag recomputation, name allocation, resolver, parser,
checker, output contract or public API changes.

New64 commands cross declaration/expression, static/instance, semicolon/ASI,
ES5/ES2015, CommonJS/ESNext and set/define fields. They retain leading field
comments, a trailing initializer comment, typed number values, strict checking,
ordinary libraries, JS and DTS maps, CRLF, outDir and an export tail. Both TS
mint and independent --check run64 commands twice with identical output hashes.
Each job has32 diagnostic5107 occurrences;32 exit2 and32 exit0 cases.

Both native before jobs have32 ASI cases exact twice and32 semicolon cases
failing once, with identical first source-map vectors. All32 maps differ only
in mappings. The first job's separately counted captures show that each JS
difference is solely the missing space plus initializer-end comment; no path,
kind or extra output differences. These captures do not qualify full actual
tuples beyond the first comparison. There are192 primary executions and96
captures across the two jobs, total288. All source, logs, binary identity,
real exits and first vectors are immutable in the two before records.

The source map pins76 whole TS owners, including A28's70 and the six comment
workers. transformPropertyWorker97501 creates an assignment/defineProperty
with the visited initializer. transformPropertyOrClassStaticBlock97444 and
generateInitializedPropertyExpressionsOrClassStaticBlock97467 attach the
field's comment/map ranges to the operation; they clear synthetic comments
only on the outer expression. They never mark the initializer's trailing
phase as relocated. getCommentRange25358 uses the explicit comment range or
the node's raw range. pipelineEmitWithComments120978 saves the enclosing
container, establishes the current node's sides, emits the node, restores
the saved sides, then emits its trailing phase. forEachTrailingCommentToEmit
121234 suppresses a boundary equal to the restored container or declaration
list end. Semicolon fields have a later field.end, so their initializer
comments emit on the initializer; ASI fields share that end and leave the
comment to the operation. Synthetic/empty ranges claim neither side.

Native materialize_field_operation currently stamps every runtime initializer
with RelocatedTrailingCommentOwner::ClassFieldOperation. That is the only
production constructor of this private one-variant type. Its printer consumers
unconditionally suppress the initializer's trailing phase before the existing
correct range/container checks can apply. The parsed_metadata.rs occurrence
is a unit setup, not another production producer. The marker originated before
the current threaded comment scopes. Its private merge transport and the
JavaScript-to-declaration snapshot exception exist only to carry/discard that
same obsolete marker; retaining them would leave an unowned suppression path.

| State | Producer/owner and lifetime | Action/consumer proof |
| --- | --- | --- |
| initializer and field raw/comment ranges | existing FieldOperation + arena nodes, per transformed tree | unchanged;64 complete commands and8 prior field-comment cases |
| container position/end/declaration-list end | existing CommentEmissionScope/EmitContext, per nested emission | unchanged checks decide semicolon vs ASI ownership |
| relocated initializer marker | materialize_field_operation -> private EmitMetadata -> Printer, per tree | obsolete; retire producer, field, enum and consumers together |
| marker merge | EmitMetadata::merge_from, clone/update lifetime | delete only this retired field's transport |
| parsed metadata exception | snapshot_parsed_emit_metadata, JS-to-declaration boundary | remove marker-only exception; preserve the exact whitelist for live fields |
| numeric/string constants | ParsedNodeMetadata/EmitConstantValue, source-identity snapshot lifetime | preserve exact bits/code units and the existing assertions |

A6-28-5a removes the marker assignment in materialize_field_operation and its
unused import. Retire RelocatedTrailingCommentOwner, EmitMetadata's optional
field and its merge_from arm. In printer.rs remove only the corresponding
three metadata suppression terms and ExpressionCommentPhaseOwner's
relocated_trailing boolean, its initializers and its trailing-phase guard.
Keep NO_TRAILING_COMMENTS, all source ranges, empty-range guards, container
ownership, token/comment resumes, synthetic comments and every other relocated
owner. Do not replace the marker with a semantic-kind, name or text predicate.

A6-28-5b removes snapshot_parsed_emit_metadata's marker-only exception and
the obsolete marker setup/assertion in its constant-value unit. Preserve the
negative-zero number-bit and UTF-16 string assertions verbatim; rename the
unit to parsed_constants_preserve_number_bits_and_string_code_units. That is
one test-id replacement, with the library count still490. No new whitelist
entry and no change to source-identity validation or typed failure boundaries.
Ordinary factory/arena errors still propagate through existing Result paths.

A6-28-5c runs all556 complete commands (initial492 plus new64), without capture
clones. Require all64 new cases exact twice, all362 first-candidate positives
preserved and the8 initial field-comment cases repaired: at least434 exact
twice. The remaining20 initial targets stay open, rather than being silently
reclassified as a completed A28 profile. Preserve every remaining first vector
and typed boundary. Run all490 current emitter units and451 contracts on new
candidate binaries; require exactly the documented one unit-id replacement
and all other identities unchanged. Freeze actual exits and source before
every further runtime edit. No new global count is inferred.

The19 architecture rows are A28's18 plus E-METADATA-G. E-METADATA-G,
E-METADATA-BASE, E-PRINTER-BASE, E-COMMENTS-G and E-COMMENT-SCOPE-H are
modified-requalify for this retirement; no failed marker branch is inherited
as qualified. E-POSITIONS and E-MAPS retain their existing representations and
algorithms and require the fresh map evidence. The other entry, arena,
protocol, context, resolver, name, helper and order references retain their
current source, with the initial active-unqualified/dormant disclaimers.
Update the current E-METADATA-G/E-COMMENTS-G type inventories when the marker
is retired, preserving their earlier row bytes in the immutable snapshots.
No new architecture boundary or portable metadata channel is introduced.

Before production, run the initial and shared readiness checks. The shared
manifest explicitly refreshes A28's test-registration authority after adding
the64-case module; the old bytes are archived in its first-regressions record.
Fixture, observer and complete assertions stay unchanged after the TS mint.
The source finding, baseline runtime hashes,76 whole owners,19 architecture
rows,64 witnesses,8 prior repair targets and expected test-id change must all
validate. Each missing/stale semantic row has its explicit retirement step;
there are no unresolved or undispositioned branches inside this prerequisite.
The one-sided partial-expression comment range, promoted class/name maps and
decorator/export-name maps are named following owners with frozen failures.
The user-authorized lightweight workflow applies: focused full observations
and adjacent product regressions; final matrix replay and hosted acceptance
remain required before landing the H2.8 train.

Exact architecture references: `E-PRINTER-BASE`, `E-POSITIONS`, `E-COMMENTS-G`, `E-COMMENT-SCOPE-H`, `E-RESOLVER-BASE`, `E-ARENA`, `E-METADATA-BASE`, `E-CONTEXT`, `E-ENTRY`, `E-PROTOCOL`, `E-ORDER-G`, `E-ORDER-H`, `E-MAPS`, `E-NAMES-BASE`, `E-NAMES-CLASS-G`, `E-NAMES-H`, `E-HELPERS-BASE`, `E-HELPERS-PROVENANCE-G`, `E-METADATA-G`.
