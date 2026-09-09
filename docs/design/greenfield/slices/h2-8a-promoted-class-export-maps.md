# H2.8a A6-28-6: promoted class and export source-map ownership

The four producer corrections and their ES5 decoration-name amendment are
qualified on the current train candidate. The
[frozen amended comparison](../../../../ratchets/h2-8a-promoted-class-export-maps-amended-after.v1.json)
has 764/868 complete commands exact twice. All 100 original map targets
(84 new and 16 prior) are repaired, all 658 prior positives and every first
candidate positive are preserved, and six additional ES2022 cases are exact.
The [emitter qualification](../../../../ratchets/h2-8a-promoted-class-export-maps-shared-regressions.v1.json)
has all 490 units and451 contracts passing with identical test IDs.
There are 1632 primary executions and no
supplemental captures in the amended comparison. The 104 remaining cases
retain explicit source owners; this is not a new global matrix count.
At this prerequisite original A28 was 392/492 exact, with four owned
wrapper-comment cases and 96 outside cases open. The subsequent
[independent-comment endpoint qualification](h2-8a-one-sided-class-comments.md)
repairs those final four targets and passes all 494/451 emitter tests. Current
A28 is 396/492 exact twice, with all original owned targets qualified and
96 outside cases open. The global H2.8 matrix remains open.

The first full candidate is immutable at 750/868 exact, 86 original target
repairs, 14 owned failures, six additional ES2022 exact cases and no prior loss.
Its 110 first vectors and 8 typed boundaries were frozen before the source
amendment. The missing ES5 getInternalName policy on both decoration names
was corrected within the same legacy producer and scope; the original target
was never reduced. The following design and before records retain their
original baselines and intended steps. H2.8a and B–E remain open.

Readiness covers four producer files on the qualified initializer-comment
candidate, with no promoted/export runtime edits yet. HEAD is
`dc868373612af16ad299ed08bdec4ece437325dc`; trusted train base is
`10748f6ee19ec083ce5748224930c5dcfbbd86df`. The semantic base is the
[comment qualification](../../../../ratchets/h2-8a-class-field-initializer-comments-shared-regressions.v1.json),
434/556 complete commands exact twice, all490 emitter units/451 contracts.
The [repeated before](../../../../ratchets/h2-8a-promoted-class-export-maps-before.v1.json)
adds312 commands:224 exact,84 new owned map failures and4 retained A19 escaped
ES5 name failures. Both before jobs retain identical first failure vectors.
H2.8a and B–E remain open; no global matrix count is inferred here.

144 fresh commands cross12 source shapes, ES5/ES2015, CommonJS/AMD/ESNext,
and set/define fields. Named, named default, anonymous default, later local
export, namespace, decorators before/after export, legacy default, static
block, member decorator, compound assignment and multiple/quoted aliases
all retain strict checking, ordinary libraries, JS/DTS maps, CRLF, outDir,
field/initializer comments and an export tail. TS mint and independent check
run144 commands twice each with identical logs;120 diagnostic5107 occurrences,
96 exit2/48 exit0 cases, four writes and two maps each. Native jobs also run
all168 unchanged A19 hoisted-export commands. New144 have60 exact/84 failed;
A19 has164 exact/4 existing escaped ES5 failures. There are1072 primary native
executions and204 separately counted supplemental write captures, total1276.
Only new144 have supplemental captures. They establish that all84 new map
pairs differ only in mappings, with JS/DTS callback bytes unchanged; they do
not establish full actual tuples after the first failed assertion. A19 keeps
its existing comparator and no supplemental capture claim.

The complete record pins four current production files. legacy_decorators.rs
was omitted from the first prelaunch input list; the complete freezer verifies
and archives its exact immutable HEAD bytes, explicitly recording that this
is a later source archive rather than claiming an earlier prelaunch pin.
All other original prelaunch inputs and actual exits are preserved.

Allowed production paths are `crates/emitter/src/builtins.rs`,
`crates/emitter/src/builtins/es2015.rs`,
`crates/emitter/src/builtins/legacy_decorators.rs` and
`crates/emitter/src/builtins/class_fields/downlevel.rs`. The fourth path is an
explicit amendment of the preliminary three-path draft: six new ES2015 named
default controls prove that the class-fields split-export producer drops the
same explicit name provenance. Its TS owner was already in the98 whole-source
closure. The qualified earlier downlevel changes remain the semantic base.
There is no printer, range representation, generic factory, parser, checker,
allocator, transform-flag computation, resolver interface, host or comparator
change in this packet.

| Value and lifetime | Exact producer and consumer | Bounded correction |
| --- | --- | --- |
| promoted statement map range, per transformed tree | TypeScriptVisitor::promote_class_declaration_to_iife -> ES2015 wrapper traversal -> Printer | skip last decorator only; preserve through ES2015 |
| moved explicit export name, per emitted export | TypeScriptVisitor::visit_top_level_class_declaration -> module export relation/Printer | clone actual identifier and raw range; NoComments; LocalName for default |
| legacy named export name, same lifetime | LegacyDecoratorVisitor::create_named_export -> collectExternalModuleInfo and Printer | retain semantic original/raw range, apply NoComments and NoSourceMap |
| legacy default export name, same lifetime | legacy create_export_default -> module assignment/Printer | preserve input explicit name, NoComments and LocalName; ES5 InternalName where applicable |
| class-fields default export name, same lifetime | DownlevelClassVisitor::visit_class_declaration/create_export_default -> module/Printer | carry actual explicit input name to cloned export name; generated fallback stays synthetic |
| module outer assignment raw range, per expression | CommonJsVisitor::visit_binary_expression -> ordinary emitted assignment | setTextRange only, keeping visited inner metadata on inner expression |

A6-28-6a follows TypeScript.visitClassDeclaration94434–94548:
setCommentRange(varStatement,node) and
setSourceMapRange(varStatement,moveRangePastDecorators(node)).
moveRangePastDecorators17307–17310 selects the last decorator's valid end,
otherwise node.pos, retaining export/default modifiers after that decorator.
Remove Es2015Visitor::visit_variable_statement's later override past all
original modifiers and its sole-use helper. The whole TS visitVariableStatement
106385–106418 only visits children outside converted-loop hoisting. Keep its
hierarchy enter/exit and hoisting intact. This repairs48 fresh range cases;
the six decorator-after-export cases already have the correct first point
and protect the last-decorator rule from regression.

A6-28-6b follows getName24788–24799/getLocalName24803/getDeclarationName24809
at TypeScript's moved-export branches94528–94543. Explicit names clone the
input Identifier, setTextRange to that identifier, preserve original metadata,
and add NoComments; default also adds LocalName. Generated/anonymous default
names retain their existing allocation and synthetic fallback without
borrowing a class or other source range. NodeFactory::clone_node copies the
identifier spelling and transform flags, creates a synthetic node and merges
original metadata; set_text_range supplies raw positions separately. Rust
resolver queries already follow original identity, so no parent mutation or
new identity flag is needed. This repairs42 fresh ES5 moved-name cases,
overlapping the range repairs. The ES2015 shared get_name raw spelling/dot
line-gap owner remains separate with its four A19 failures.

A6-28-6c follows transformClassDeclarationWithClassDecorators98566–98647.
Named export uses getDeclarationName with omitted allow flags, hence
NoComments+NoSourceMap. Preserve its original name identity for resolver
queries; six fresh ESNext legacy named cases prove this branch. Default
export reuses declName from getLocalName/getInternalName(false,true), so
preserve the explicit name range and apply LocalName/NoComments (InternalName
at ES5) instead; six fresh ES2015 default cases prove that branch. Do not
apply the named-export no-map policy to default, or mutate a shared parsed
name to supply export flags. Clone the producer's name when explicit and
keep an anonymous/generated fallback synthetic.

A6-28-6d follows substituteBinaryExpression111990–112008 and whole
createExportExpression111805–111850: each new export assignment receives
setTextRange(location), with no setOriginalNode/metadata merge. The native
CommonJS visitor's current set_original_and_range erroneously duplicates the
inner legacy assignment's sourceMapRange on the outer expression. Set only
its text range. The visited inner expression keeps the decoration producer's
correct moveRangePastModifiers sourceMapRange and NoComments. Twelve fresh
CommonJS/AMD legacy cases exercise this gap. All24 ordinary compound/multiple
alias shapes and the adjacent A19 commands protect shared export behavior.
Retained substitution remains the existing eager owner projection; no new
query, temporary, traversal, helper or export-binding allocation is introduced.

A6-28-6e follows class-fields
visitClassDeclarationInNewClassLexicalEnvironment96971–97045, whose split
default export97011–97024 uses getLocalName(false,true). Carry the explicit
input class name through create_export_default instead of replacing it with
a text-only synthetic identifier. Preserve original/range and LocalName plus
NoComments on the clone. The existing generated default allocation is
unchanged and supplies the anonymous fallback. Six fresh named controls
repair this branch, and six already-exact anonymous controls protect it.

A6-28-6f compares all868 complete commands (556 previous +144 new +168 A19),
without supplemental captures, requiring all658 current positives preserved
and all100 owned failures repaired:84 new and16 old A28 map cases. The target
is at least758 exact twice. The110 other failures retain their owners:
102 previous outside cases, four one-sided wrapper comments, four escaped
A19 ES5 names. Original A28 reaches386/492 if these16 repairs pass; its390
minimum remains OPEN for the following four comment cases. Run all490 current
emitter units and451 contracts with identical test ID sets, no new rename.
Freeze actual exits, first vectors, input bytes and binary identity before
any further runtime edits. No global count, whole H2.8 close, certificate
walk or full developer CI result is claimed. Hosted acceptance remains
required at the final train candidate before landing.

The98 whole TS owners are pinned by the readiness manifest, including factory,
original/clone transport, comments, class coordination, names, module queries
and map workers. Current architecture inventory uses19 exact table rows.
E-METADATA-BASE/E-POSITIONS/E-RESOLVER-BASE/E-NAMES-BASE/E-NAMES-CLASS-G are
modified-requalify for these producer uses; no failed branch is inherited as
qualified. E-MAPS remains the dated dormant compatibility row and supplies
no inherited parity claim; fresh full maps are the evidence. E-ORDER-H and
E-NAMES-H retain their active source premises with no broad qualification
claim. Context, arena, protocol, entry, helpers, printer and comment scope
remain validated current premises with the existing full emitter regressions.
No architecture type inventory needs a production redefinition here.

Before production, run the initial, comment and promoted/export readiness
checks. Refresh only the144-case test registration, this indexed packet and
its parent readiness references, archiving every old authority byte from the
qualified baseline or a dedicated pre-readiness snapshot. All fixtures,
observers and complete assertions remain unchanged. The original three-path
draft, first failure archive and old authority history remain immutable.

Exact architecture references: `E-PRINTER-BASE`, `E-POSITIONS`, `E-COMMENTS-G`, `E-COMMENT-SCOPE-H`, `E-RESOLVER-BASE`, `E-ARENA`, `E-METADATA-BASE`, `E-CONTEXT`, `E-ENTRY`, `E-PROTOCOL`, `E-ORDER-G`, `E-ORDER-H`, `E-MAPS`, `E-NAMES-BASE`, `E-NAMES-CLASS-G`, `E-NAMES-H`, `E-HELPERS-BASE`, `E-HELPERS-PROVENANCE-G`, `E-METADATA-G`.


A6-28-6c amendment after the first full868 candidate: the restored moved-export
identity exposes an additional required ES5 decoration-name branch. The first
candidate and every vector are frozen in the shared-after record; its remaining
owned targets are still in scope and the minimum758 target is unchanged.
Whole generateConstructorDecorationExpression98827 chooses
getInternalName(false,true) belowES2015, otherwise
getDeclarationName(false,true), for both helper argument and assignment LHS.
Native visit_class_declaration omitted those flags on both decoration names.
For explicit source names supply NoComments and, belowES2015, LocalName plus
InternalName; retain ES2015 declaration-name publication. Preserve both actual
identifier ranges/originals, generated fallbacks, and the assignment's mandated
moveRangePastModifiers range/NoComments. This amendment touches only
legacy_decorators.rs on the frozen first candidate; it adds no producer path,
source owner, architecture boundary, test exception or target reduction.
All868 complete commands and all490/451 emitter tests must qualify the amended
candidate. The first candidate remains an immutable failed observation.
