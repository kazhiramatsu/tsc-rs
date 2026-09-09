# H2.8a A6-27 prerequisite: synthetic System import.meta range

This amends the initial [A27 packet](h2-8a-meta-property-token-maps.md)
before changing its System substitution. The first candidate completed with
exit101:208/228 commands exact twice,20 failed once,436 complete command
executions, no supplemental clones. Both selected native tests pass, including
all16 invariant repetitions and six name-query cases. Full emitter suites were
not run on that intermediate candidate. Its source, receipts, first vectors
and real exit are frozen in
ratchets/h2-8a-meta-property-token-maps-first-after.v1.json.

All20 remaining first vectors are identical to before. Sixteen System
import.meta cases were misclassified as printer-owned in the initial75;
System substitutes them before MetaProperty printing. Preserve those original
labels and add the explicit correction in the first-after record. The corrected
owner split is59 printer-only,7 module-name plus printer,16 System synthetic
range,4 outside class-field-alias maps and142 adjacent exact. Initial repairs
are66; the total owned target remains82, with224/228 exact twice required.

The already pinned substituteMetaProperty (_tsc.js:113312–113317,
e84500cf33cc66757fb10aaaf3d10bc88a7a222cce65c52c3d1c24c79ed6419d)
returns factory.createPropertyAccessExpression(contextObject,
factory.createIdentifier("meta")), with no original, text, source-map or comment
range assignment. The five additional whole factory owners establish the
range provenance: baseFactory.createBaseNode20259–20267 initializes pos/end
to-1; nodeFactory.createBaseNode21493–21495 delegates; createBaseDeclaration
21496–21501 adds symbols only; createBasePropertyAccessExpression22464–22473
sets children and flags; createPropertyAccessExpression22474–22489 applies
ordinary access parenthesization and returns that synthetic node. The fixed
context identifier takes the existing ordinary access path. All44 whole source
owners, including the initial39, are pinned in the amended readiness manifest.

Native SystemVisitor::create_property_access already produces the corresponding
synthetic PropertyAccessExpression and Identifier in TransformArena. The extra
set_original_and_range in visit_meta_property then attaches the parsed meta
node's source range and original identity to that outer access. Its position
reaches the normal printer map pipeline and introduces the16 observed map
differences. No factory, range representation, map generator or shared helper
needs a change.

| Semantic value | Rust producer/consumer and lifetime | Disposition and evidence |
| --- | --- | --- |
| wholly synthetic outer access | SystemVisitor::visit_meta_property -> create_property_access -> TransformArena; per transformation | remove the extra original/range assignment;16 complete commands |
| context and meta identifiers | existing create_identifier/create_property_access, private; per transformation | unchanged name identity and allocation; mapped and no-map commands |
| absent source location | existing synthetic SourceRange and metadata -> ordinary Printer map pipeline | existing representation, changed consumer; full maps and emitter regression |
| retained new.target name | initial A6-27-1 negative branch and Unspecified printer hint | retain Ok(original); six query cases and full228 commands |

A6-27-5 changes only the positive import.meta branch in production
crates/emitter/src/builtins/system.rs: after the unchanged predicate and context
identifier creation, return self.create_property_access(context,"meta")
directly. Remove that branch's set_original_and_range call. Retain the initial
negative-branch repair. CommonJS and printer source must stay byte-identical to
the archived first candidate while this prerequisite is implemented. No new
public interface, metadata field, general helper or transform ordering change.

The existing16 complete System map commands cover value, property, element,
typeof, local binding collision, both comment gaps and imported binding, each
underES5/ES2015. Two System import-meta-no-map commands are adjacent controls.
All228 compiler observations, two native tests, eight internal oracle rows,
six query rows, adapters, comparison assertions and frozen expectations remain
unchanged. Their initial source hashes remain authoritative. No new fixture
or handwritten expected output is necessary.

The amended manifest retains all13 architecture rows and their initial
dispositions except E-METADATA-BASE: its System consumer is now
modified-requalify with no inherited qualified premise. E-POSITIONS and E-MAPS
also require the new System evidence; their shared implementations remain
unchanged. E-MAPS' historical dormant row remains research-only. The synthetic
range is an existing representation, so this introduces no architecture
boundary or generic metadata activation.

Run the initial and shared readiness checks before the runtime edit. Freeze a
fresh shared-after prelaunch, then run all228 commands and both native tests.
Require224 exact twice and only the four frozen class-field-alias first vectors
to remain unchanged (or become exact). Run all490 emitter library tests and451
contracts on the newly built candidate binaries, with no ignored or filtered
tests. Preserve the first candidate separately and archive final source, log,
binary identity, real exits and authority receipts. The extra step has no
unresolved or undispositioned branch. The user-authorized lightweight workflow
continues; other A owners,B–E, final matrix replay and hosted acceptance remain
open. No new global total is inferred from this focused repair.

Final shared result: all16 System range cases now match twice. All120 fresh
commands and 104/108 A22 commands match twice: 224/228 exact
with 4 unchanged class-field-alias first vectors. The final combined process
exits101, with 452 complete executions and no capture clones. Both native
tests pass (16 internal exact repetitions,6 query cases), and the newly built
binaries pass all490 library tests and451 contracts, no ignored or filtered
tests. The first208/228 candidate and corrected16-case ownership remain frozen
separately. The final after record archives both prelaunch and final authority
identities. This closes A6-27-5 and the bounded A27 owner; other A owners,B–E,
final global replay and hosted acceptance remain open.
