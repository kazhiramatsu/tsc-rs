# H2.8a A6-27: MetaProperty names and token positions

The [System synthetic-range prerequisite](h2-8a-meta-property-system-range.md)
amends the initial scope below. Its frozen first-after record corrects16 cases
previously classified as printer-owned and adds A6-27-5 before the final repair.
The final result and corrected owner counts are recorded at the end.

Runtime slice at base `1f66c93e5c46b66278a8372899e5d498a1005942`, on the
existing H2.8 train (trusted base10748f6ee19ec083ce5748224930c5dcfbbd86df).
Repair MetaProperty's structured printer and its name context in the eager
module visitors. The source semantics, frozen complete observations and
current native controls determine this dependency-closed owner. Other A owners,
B–E, custom transform API1, builder, parser, checker, binder, general factory,
resolver API and output comparison contracts are outside the change.

Before: new120 complete commands have42 exact and78 failures; A22's108 adjacent
commands have100 exact and8 failures. Two jobs repeat all228, producing the
same79 complete source-map first vectors and7 typed transformed-MetaProperty
refusals. Four failures are the previously retained class-field-alias maps,
owned by a separate follow-up. The other82 are this slice:75 printer differences
and7 requiring both name-context preservation and the printer. Seventeen
supplemental JavaScript texts also differ; all79 map differences change only
mappings. Supplemental captures are not whole actual tuples beyond the first
comparison boundary. There are740 primary command executions and370 extra
cloned-Program captures, total1110. Positives run twice/job; failures once/job.

The120 commands are20 shapes xES5/ES2015 xCommonJS/ESNext/System. They include
JS and typed functions, constructor/arrow capture, both keywords, escaped names,
export/import binding collisions, comments on either side of '.', newline
gaps and no-map controls. Imported-binding cases include a real dep.ts.
Standard libraries, strict checking, declaration output, applicable maps, CRLF
and outDir remain. Diagnostic occurrences are5107:100 and1343:9. Both TS mint
and independent --check run each command twice and produce identical bytes.
No observer, frozen output, adapter or comparator is changed after observation.

Eight separate internal controls use TS Program.emit after-transform metadata:
new.target/import.meta xbaseline/NoTokenSourceMaps/token override/absent-name,
each twice. Native selection mirrors the existing built-in list with a Program
format host, then applies the same metadata/update. Comparison includes exact
printer text and complete map JSON. The compiler-owned sourceMappingURL is
excluded only at the oracle's explicit UTF-16 callback sourceMapUrlPos. No regex
or normalization is involved. Six query controls use the existing ES2015
imported-binding sources. The native resolver deliberately answers a meta-name
lookup if asked: names must not be queried, while real value references must
be queried during eager CJS/System transformation. ESNext retains the later
print phase. This is internal protocol coverage, not extra admitted commands.

The first native invariant setup omitted the required host. All16 attempts
failed with EmitHostRequiredForImpliedModuleFormat before transformation or
printing. Its source/log/receipts and the successful complete comparisons are
frozen in the first-before record. Only the host wiring was corrected; fixture
and assertions stayed unchanged. In the amended before,12 internal prints
match text but differ in maps, and4 absent-name prints refuse typed syntax.
All6 query scenarios execute: CJS new/import and System new wrongly query the
name; all real-value phase controls agree. The full current emitter library is
488 passed/2 new owned failures,490 tests with no ignored/filtered tests. The
before binary hash, size, timestamp and Cargo provenance are frozen; binary
bytes are not copied. A rustfmt-only include_bytes line wrap after TS mint is
retained as a pre-format source snapshot; the first preparer rejected its old
pin before any native launch, and the amended manifest records both identities.

The whole upstream function map is mechanically pinned in readiness (39 owners).
The principal graph is pipelineEmit -> getPipelinePhase -> notification,
substitution, comments, maps -> pipelineEmitWithHintWorker -> emitMetaProperty.
The last worker writes the keyword with writeToken(keyword,node.pos,
writePunctuation) WITHOUT contextNode, then plain '.', then emit(name).
emit(undefined) does nothing; present names use Unspecified, not Expression or
IdentifierName. Module/system onSubstituteNode select Identifier substitution
only for Expression. CommonJsVisitor currently eagerly visits this name as a
reference; SystemVisitor does the same for its non-import-meta branch. Their
bounded correction preserves this leaf, including its name, rather than
performing a value query before the printer can apply the correct hint.

writeToken calls emitTokenWithSourceMap unless maps are disabled. With no
context node, that worker has no owner flags/token-range override. It skips
trivia in the current source, records before, writes the keyword and advances
by its spelling length, then records after. An outer sourceMapRange cannot
change the token's source: emitSourcePos saves/restores an alternate source
for that individual record. The dot is unmapped. Name emission retains ordinary
range/comment handling, hooks and original escaped spelling. TS discards the
recorded inline keyword/dot/name gap comments; ordinary child comment handling
and the absence of a keyword comment phase produce that result.

Parser/factory create only NewKeyword or ImportKeyword MetaProperty nodes.
MetaPropertyData.keyword_token and Option<NodeId> name are already sufficient.
createMetaProperty propagates optional child flags and adds ES2015 for new or
ES2020 for import; the native initialization/recomputation already agrees.
ES2015 visitMetaProperty lowers new.target below ES2015 with existing lexical
capture; System substitutes import.meta with context.meta. No capture, helper,
name allocation, transform flag or pass order changes. Parsed/current/synthetic
identities remain in TransformArena; factory updates preserve original/range
provenance. The new internal absent-name control uses the existing update API
and updates ancestors/root in the detached arena without mutating parsed input.

| TS semantic value/phase | Rust producer, owner and lifetime | Current gap / action / proof |
| --- | --- | --- |
| keyword/name/raw pos | syntax::MetaPropertyData; mounted TransformNode in TransformArena; source identity and optional child are observable | printer missing; A6-27-2/3;120 commands and8 internal rows |
| non-expression name context | CommonJsVisitor::visit and SystemVisitor::visit_meta_property, private, per transform | partial-or-stale eager query; A6-27-1;6 query rows and7 typed failures |
| absent token context | existing SourceMapRange via Printer::token_map_range_spanning then record_map_range_side, private, per print | missing structured lane; A6-27-2; NoTokenMaps/override controls |
| trivia and keyword length | existing raw source position to SourceRange, using skip_trivia; no saved cursor | shared prerequisite, source-pinned and current map controls; A6-27-2 |
| typed map source/location | SourceRange/TransformSourceId -> SourceBytePosition/SourceUtf16Location -> TextWriter/SourceMapGenerator | existing helper, no generator change; all maps/full emitter regression |
| optional name emission | arena.node_ref + Printer::emit_node_with_hint(Unspecified), private; ordinary child pipeline | raw fallback bypasses name pipeline; A6-27-3; escaped/collision/absent-name controls |
| comment claim and resume | immutable EmitContext and existing node pipeline, private; per subtree | new structured consumer; A6-27-3; gap/newline fixtures and full comments contracts |
| lower/substitute meta nodes | es2015::visit_meta_property and system::visit_meta_property plus retained hooks | adjacent owners unchanged except System name leaf; ES5/System commands |
| source-map callback/result | existing ProgramSession/emit execution and source-map writer | unchanged exact comparison boundary; all228 complete commands |

A6-27-1 changes only production crates/emitter/src/builtins.rs and
crates/emitter/src/builtins/system.rs. Add NodeData::MetaProperty(_) => original
in CommonJsVisitor::visit before its generic child walk; the name has no value
expression to rewrite. In SystemVisitor::visit_meta_property, keep the existing
import.meta substitution and return Ok(original) for its other branch instead
of update_generic. Preserve module memoization, name identity, existing flags,
parent/original links and every actual value-reference query. No generic
identifier classifier or resolver behavior change. Prove the six query rows
and complete binding-collision commands; unexpected results stop the candidate
for evidence and packet amendment.

A6-27-2 changes only production crates/emitter/src/printer.rs. Add the typed
MetaProperty dispatch and private emit_meta_property worker with the pinned
emitMetaProperty ledger entry. Match New/Import to fixed punctuation spellings;
impossible keyword kinds use existing UnsupportedTransformedSyntax. Read raw
node.pos, compute token_map_range_spanning(source,pos,spelling.len()), record
Before when present, write_punctuation(keyword), record After when present.
Reuse record_map_range_side directly: do not supply the MetaProperty as token
owner, and do not use the brace helper's unrelated omission option. No source
text splitting, text replacement, new scan, map algorithm or generic fallback.

A6-27-3 writes plain '.', then optionally resolves name in the same source and
calls emit_node_with_hint(Unspecified, context.for_child(NORMAL)). Invalid child
ids retain existing UnknownStatement; absent name succeeds after the dot.
Keep notification/substitution order and ordinary comment/map phases. Never use
emit_node_id_with_context (Identifier becomes Expression) or the IdentifierName
helper. Do not mutate parser/factory/metadata/transform flags, generic raw
fallback or context helpers. Keyword and dot do not get comment-token phases.

A6-27-4 qualifies all228 commands after, with224 exact twice and only the four
frozen class-field-alias first vectors allowed to remain (or become exact).
Require both new native tests, all490 current emitter units and451 emitter
contracts on the newly built candidate binary. No capture clones after. The
shared generator, comment pipeline, hooks and normal reference consumers are
covered by their complete emitter regressions. Record exact binary/source/build
identity, real exits, all logs and before/after hashes before a local commit.
No new global total can be inferred by subtracting these focused repairs.

Architecture references are pinned in the manifest with current symbols,
visibility, validation refs and owner/control relations. E-PRINTER-BASE,
E-POSITIONS, E-COMMENTS-G and E-COMMENT-SCOPE-H are modified-requalify for the
bounded structured branch. E-RESOLVER-BASE is modified-requalify for query
presence, with no API change. E-ARENA, E-METADATA-BASE, E-CONTEXT, E-ENTRY,
E-PROTOCOL and E-ORDER-G are premise-unchanged. E-ORDER-H is a current audited
composition input, not an inherited qualified premise. E-MAPS' dated dormant
inventory row is research-only: the live H2.6 generator and bounded A22 maps
are audited directly and requalified by current map controls and full commands;
no dormant map compatibility is inherited. This branch starts unqualified and
its exact profile is qualified only by the after record. There is no new
architecture boundary or general API activation. Historical docs are rationale;
current source and fresh executions own these claims.

Run `node scripts/observe-meta-property-token-maps.mjs --check` and
`node scripts/observe-meta-property-token-map-invariants.mjs --check` for TS.
Run `python3 scripts/check-meta-property-token-maps-readiness.py` for readiness.
The manifests archive exact native argv, inputs and environment; focused Cargo
selects meta_property_token_maps and transformed_class_assigned_names, then
full emitter binaries run without filters. CARGO_BUILD_JOBS=2, taskpolicy -b,
nice -n15; heavy commands are serial and only read-only/scratch work runs during
them. Preserve every real exit and first failure before editing pinned sources.
The integrator is the single writer; no delegation or extra train is needed.

Readiness requires all39 whole owners, all228 witnesses, eight internal rows,
six query rows,13 architecture rows, four steps, fresh authority/baseline
hashes, unresolved=0 and undispositioned=0. Missing/partial rows map explicitly
to the steps above. The four class-field-alias residues have no MetaProperty;
their guard is exact source inspection and before vectors, with a named next
A owner. There are no unresolved semantic questions or hidden deferred runtime
branches in this bounded profile. No fixture/path/case-ID branches, handwritten
expected bytes, normalization, recursion fallback or unsupported success route.
The schedule's user-authorized lightweight workflow applies: no historical
certificate walk or full developer CI is claimed. Other A owners,B–E, final
matrix replay and hosted acceptance remain open.

Final measured result: all120 fresh commands and 104 of108
A22 adjacent commands match TypeScript twice. All82 owned differences are
repaired:59 printer-only,7 module-name/refusal and16 System synthetic-range
cases. The first candidate repaired66 and left the16 System vectors unchanged;
its208/228 result is preserved separately. The shared prerequisite corrects
the initial ownership labels without rewriting any before or first-after record. The remaining
4 class-field-alias map differences retain their exact before first
vectors. The final focused profile is 224 exact twice/4 outside once,
with 452 complete native command executions and zero supplemental
executions. The combined process exits101; that status includes the explicitly
retained outside differences and is not described as an all-green matrix.

All16 internal invariant repetitions match printer text and complete map JSON;
all6 name/value-query scenarios pass, including both CommonJS name slots and
System's new.target. All490 current emitter library tests and451 contracts pass
on the newly built binaries, with no ignored or filtered tests. The library
identity set is exactly the488 prior controls plus the two new tests. Sources,
binary hashes/size/timestamps, Cargo provenance, execution manifests and real
exits are archived in h2-8a-meta-property-token-maps-after.v1.json. The initial
host setup failure and both complete before runs remain immutable. Production
is confined to the three readiness paths. Other A owners,B–E, final global
replay and hosted acceptance remain open; no global count, certificate walk
or full developer CI is newly claimed.
