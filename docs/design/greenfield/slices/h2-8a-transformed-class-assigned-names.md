# H2.8a A6-22: assigned names after class and function transforms

Base `ea345da10f18cbb660cb51ea5d7b889f06831dd0`. The 108 fresh complete
TypeScript 6.0.3 commands cover four class shapes (plain, implements, generic,
typed member) in five parents (variable, property, assignment, binding,
parenthesized), plus seven function, arrow, named-class and class-fields alias
controls, at ES5/ES2015 in CommonJS/ESNext. All commands use standard libraries,
strict checking, declaration and source-map output. TS observation and --check
each execute every command twice. Diagnostic 5107 occurs in 54 commands; the
remaining 54 provide semantic controls without that diagnostic.

Two independent native before jobs each produce 74 exact and 34 failed primary
comparisons, with identical first vectors and membership. Successes execute
twice per job; failures execute once per job. The first job also emits cloned
prepared programs through the existing inspection hook, capturing 182 additional
artifact traces. Those are supplemental callback text/path/kind and exit-code
observations, not complete actual command tuples. Across both jobs there are
364 primary comparisons and 182 supplemental executions: six native executions
per positive case and three per failing case. All captures complete without
errors; their exit codes match TS and repeated positive captures agree.
The immutable before record preserves this distinction and the outside archive.

All 34 first failures are source-map results. Supplemental bytes identify 26
owned failures: 24 ES5 typed class expressions in unparenthesized parents and
two ES5 typed anonymous functions using new.target. Their JS differs only by
the local identifier Foo where TS uses class_1 or _a. The eight remaining
failures have identical JS: four class-fields alias controls across both targets
and modules, plus four ES2015 plain/typed function new.target controls. All
captured declarations match TS, and only mappings differ within map JSON.
These component observations do not qualify later complete-command fields
beyond the first failure. All comparisons remain unconditional.

A6-22-1 changes only crates/emitter/src/builtins/es2015.rs::assigned_name.
TS getAssignedName reads the current node.parent. Native currently follows
parse_tree_node to the original parsed parent, incorrectly recovering Foo after
TypeScript erasure detaches a changed class or function. Read arena.node(node)
parent instead, return None if absent, and scope parent and child TransformNode
handles to node.source(). The binary right-child identity must use node.node().
Preserve the expression-kind and class_expression_alias_assigned metadata gates,
all parent-kind arms and their error propagation. Move the misplaced
getInternalName ledger comment to get_internal_name; clarify get_name's current
parent requirement. The existing alias exclusion and generated-identifier guard
remain unchanged; no claim that TS assigns a generated binary parent is needed.

The native factory already clears parent in clone_node while retaining original
metadata. update_node preserves identity when data and flags are unchanged and
clones otherwise. TypeScriptVisitor::update_class_expression removes type
parameters and visits heritage/members before that factory update; corresponding
function visitors remove typed syntax. This distinction explains plain versus
typed controls. Do not change factory parent links, parsed trees, provenance,
type erasure, map/comment/raw ranges, name allocation or finalization. In
particular, the separate get_name raw-range/printer dot-layout work remains open.

The sole assigned_name caller is get_name. get_internal_name serves class body,
constructor/member prefixes and returns; get_local_name serves class declaration
and function new.target capture/visitors. Preserve hierarchy facts, converted
loop state, flag inheritance and all generated-name allocation phases. Existing
class-fields alias metadata is produced by DownlevelClassVisitor and remains a
separate premise. No new checker query, sink, host, public API or mutable state.

Architecture: E-NAMES-BASE is modified-requalify for this private name-selection
consumer; GeneratedBindingId, printable names, target provenance and allocation
scope remain distinct, with finalize_generated_binding_names using the composed
tree. E-NAMES-CLASS-G is premise-unchanged: ClassBinding and ClassTempPlan keep
their allocation phases. E-ARENA and E-METADATA-BASE are premise-unchanged:
TransformArena/NodeFactory preserve immutable parsed syntax and separate current
parent from original/map/comment/value identities. E-ORDER-H is premise-unchanged:
get_script_transformers retains ES2015/generator and module ordering.
E-PROTOCOL is premise-unchanged: host, resolver, artifacts and sink keep their
separate ownership. E-PRINTER-BASE is premise-unchanged: Printer consumes the
transformed syntax and metadata through its existing hook pipeline.

Required after: 434 complete commands, comprising these 108, 96 A6-21 JSDoc
implements, 50 class-transform flags, 64 class-statement layout, 84 synthetic
default aliases and 32 CommonJS class-instance commands. Target 426 exact twice:
100 fresh and all 326 adjacent, including A6-21's two ES5 class-expression map
residues. The eight fresh outside map failures must retain their exact first
vectors, once each in this job. Disable supplemental captures for this run.
Require the existing emitter library and contracts suites (483 and 451 tests).
These are predictions until measured; preserve unexpected results before any
scope revision. No new global total follows from focused fixes. The prior
eight original implements passes and 31 checker units remain prior evidence.

The schedule's current lightweight workflow applies. Historical full developer
CI and certificate walks are omitted and not claimed. Other A owners, B–E,
the final 769-command replay and hosted acceptance remain open.

Whole TS function bounds and hashes, verified against the pinned AST, are listed
in the readiness manifest beside the immutable input and architecture hashes.

- getNonAssignedNameOfDeclaration: 11517–11561, SHA256 `382ebe3aca3c5b65c264f1177b6f9ed47454cdc918c228f8469456fb504d617b`.
- getNameOfDeclaration: 11562–11565, SHA256 `5d3aafbdab871f0fe6f088a4904cd11e6b44e467e0cca8ad0c215b3f899b570b`.
- getAssignedName: 11566–11580, SHA256 `16b3160d83ab91d6d9c08811b7d3ef66bce1a84ccdde30ee5cac09babbc2bab7`.
- isGeneratedIdentifier: 11932–11935, SHA256 `906256d7068a095cb3ebfc51672f5d00f2dcb0dda810d29d90f975f6503f8dfd`.
- getGeneratedNameForNode: 21652–21666, SHA256 `7aeec7c8966a869665e0b8f01a41cd52e75bc957006170fd446ea819be9a6ea0`.
- updateFunctionExpression: 22698–22700, SHA256 `6a79c936cf190f04229e70e497fcd9a6ee0432824bb40c0bc14c0960e12535a2`.
- updateClassExpression: 22938–22940, SHA256 `b0a96dc25891132cbab5d16e0aab9ff6e8d83d92a2d24008e9ccd0bc918b2c3a`.
- cloneNode: 24436–24466, SHA256 `d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6`.
- getName: 24788–24799, SHA256 `9734f5576b1aa153598ff7ae70a2a2f994bb50d0370fbfc547c47952f72dea33`.
- getInternalName: 24800–24802, SHA256 `cf6484de86856ef03a019d04862c731662b2aa5b215b22cb036feb31593f7778`.
- getLocalName: 24803–24805, SHA256 `db85ef71236480d7de1d2e131b01d6f8fed272ef41d0f5297ce7fb3485ee7979`.
- update: 24995–25001, SHA256 `384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707`.
- typescript.visitClassExpression: 94549–94563, SHA256 `4dae4f7a40f66795c79f1667200c7b9d9d63898d21391eeda09415cd761b4605`.
- transformClassMembers: 94564–94598, SHA256 `306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8`.
- typescript.visitFunctionDeclaration: 94974–94996, SHA256 `4ff87d0368d9f37dbae1efd7006696081b804867772366e90e13844fb32c0d6c`.
- typescript.visitFunctionExpression: 94997–95014, SHA256 `11df95836f617bf0b12a6119707b8e84be13150c8c6a5e49f2996b1f002a68ec`.
- typescript.visitArrowFunction: 95015–95028, SHA256 `0ec3d3afcda9f15a5765f40426b8411b0bae9822e6764901ebe7a8fa337207ce`.
- classFields.visitClassExpression: 97046–97048, SHA256 `6397bc711e73da9e2ec7d561e753eef95e5967bc08c063ba2f19678aac743ac1`.
- visitClassExpressionInNewClassLexicalEnvironment: 97049–97129, SHA256 `5885e805a286e1451a1c60771127ff84a6c108f88522eb2f90901c2703763319`.
- es2015.visitClassDeclaration: 105144–105174, SHA256 `b5eee1e707db4b5a7f0bd97240c5e9614cfd341992a4c5cddf52223940b0626e`.
- es2015.visitClassExpression: 105175–105177, SHA256 `290a1ce981403ab42d9557d7235156214cfe66e5c69b388cf2f064038fdfc52d`.
- transformClassLikeDeclarationToExpression: 105178–105220, SHA256 `73df944ac57dd513d2f9038860b0430ee225c3058023329eb4563f96d4fb7595`.
- transformClassBody: 105221–105249, SHA256 `9850847f0d39924ad08a7fd966626be67f88fa17b9f4d723c340dace16e67454`.
- insertCaptureNewTargetIfNeeded: 105945–106005, SHA256 `820984ac3bd8d30b44abadfe06e1257f60177a6f672c5acaa9a89549796dc117`.
- es2015.visitArrowFunction: 106151–106178, SHA256 `5aa1e3a6520e9abf96f800b0657391f09b4616fe55362903ac71d6a676f2de0e`.
- es2015.visitFunctionExpression: 106179–106201, SHA256 `d282358038f6b8e9a21488951ec8505ed30e36de35066fb763dfec2e78283220`.
- es2015.visitFunctionDeclaration: 106202–106223, SHA256 `fbb5a9f062f4bbf2352ecd3c56154c6b08b2b33a08a163bac32a03dd4bd90ce3`.
- transformFunctionBody: 106255–106329, SHA256 `3a3d99baf53b7ade96d462610aefdbf6855671e31375e93c5e5078e71d80d750`.
- getClassMemberPrefix: 108071–108073, SHA256 `cd87f6646a467f778a7b7900a34066d49198e9e5823a370c845f780b71cc689a`.

First after amendment: the initial current-parent consumer alone produces
415/434 exact twice and 19 failed once (348.21s, exit 101). All 26 owned fresh
failures and both A6-21 typed class-expression residues are fixed, but eight
plain class controls and three qualified JSDoc implements commands regress.
The other eight failures retain their exact before vectors. The complete log,
first vectors, source and launch inputs are frozen outside the checkout and in
ratchets/h2-8a-transformed-class-assigned-names-first-after.v1.json. This failed
first after is retained; it does not qualify the owner.

A6-22-2 adds crates/emitter/src/builtins/class_fields/downlevel.rs to production
scope. rebuild_class_member_array deliberately replaces an unchanged member
array with a synthetic one, forcing a class clone and dropping its current
parent. Its old justification was to force canonical compiler printing.
Current execute.rs already selects SourceFileTextMode::Canonical at both
compiler printer construction sites; Printer's class arms are structural.
Remove that obsolete identity change: when an original member array exists,
use the existing NodeFactory::update_node_array for both unchanged and changed
members. It returns the original array for identical child identities; changed
arrays retain range, trailing-comma and missing-list metadata. Absent arrays
still use create_node_array. No class parent mutation or additional side-table
fact is needed. TS classFields.transformClassMembers uses visitNodes2 and
visitArrayWorker, which likewise preserve arrays with unchanged children;
updateClassExpression then preserves the class identity.

Both class declaration and expression callers remain covered by the existing
434 complete commands. E-ARENA is now modified-requalify for this producer's
identity-preserving use of the existing factory, while factory types and code
remain unchanged. E-PRINTER-BASE stays premise-unchanged: canonical compiler
printing is explicit in execute.rs, and standalone PreserveUnchanged retains
its documented identity behavior. E-NAMES-BASE remains modified-requalify.
Re-run all 434 commands and the 934 emitter tests after this amendment. Expected
426 exact twice and eight unchanged map failures remain predictions; any new
result is preserved and reviewed. First-after evidence is never overwritten.

Additional whole TypeScript owners for the identity-preserving member producer:
- visitNodes2: 91087–91115, SHA256 `211c41370d3526081e4f4c696ed20f63ade07e0407cab76facb848ceb8a9eff9`.
- visitArrayWorker: 91129–91161, SHA256 `5a8ae21ce714ea10b9c62ccba2469fe1bc0c36dd14db542dd190db4330252da9`.
- classFields.transformClassMembers: 97143–97237, SHA256 `8f02dc71f423a197caae79451edbed69e643ef5b909248bf13a649c2c2491071`.

A6-22-3 corrects only the existing empty-function-body comment contract in
crates/emitter/tests/integration/active_transform_contract.rs. The first emitter
run passes all 483 library tests and 450/451 contracts; the lone failure is
empty_function_body_does_not_reown_the_open_brace_trailing_comment. Its helper
requests PreserveUnchanged, so the now unchanged class and root correctly retain
the source comment through H1's identity path. That test intends compiler
canonical output. A fresh TypeScript program recheck repeats its existing
expected bytes twice (with diagnostic 5107 for alwaysStrict=false); the expected
comment-free output is unchanged. This recheck is supplemental unit evidence,
not a new complete-command oracle. The failed emitter run and launch inputs are
frozen in the emitter-first-after record and its outside archive.

Change this one call to the existing transform_and_print_canonical_at_target
helper. Keep every expected byte, other test helper and production path unchanged.
The passing 434-command final compiler comparison remains valid: its production
and compiler inputs are unchanged by this test-only correction. Preserve its
exact prelaunch packet/readiness separately. Re-run the full 451 contracts;
retain the already passing 483 library results at identical production bytes.
No new compiler run or second library run is implied or claimed. E-PROTOCOL and
E-PRINTER-BASE remain premise-unchanged: compiler Canonical and standalone
PreserveUnchanged are distinct existing requests, and the contract must select
its intended request explicitly.

Final measured result: 100/108 fresh complete commands match TS twice, including
all 26 owned failures and all 74 prior positives. All 326 adjacent commands
match twice, including the two previously retained A6-21 ES5 class-expression
map failures. Overall 426/434 complete commands are exact twice. The eight
outside map failures retain their exact before first vectors and execute once
each in this after job. Supplemental capture is disabled. The six compiler
contract functions finish in 624.24s, with five passing and one failing;
unconditional complete comparisons keep exit 101. The emitter's 483 library
tests pass in 0.79s at these production bytes. After selecting Canonical
in the one comment contract, all 451 contracts pass in 2.41s, exit 0.
The first combined emitter command's exit 101 and its 450/451 contracts result
remain frozen. No expected bytes changed, and no second library or compiler
execution is claimed for this test-only correction.

The final record is ratchets/h2-8a-transformed-class-assigned-names-after.v1.json. Exact launch
packet/readiness and all other launch inputs are preserved in separate outside
compiler-prelaunch and contracts prelaunch trees before this measured-result
append. Production changes are confined to es2015.rs and the class-fields member
array producer; both first-after failures are preserved in their frozen records.
No expected observation or shared comparator changed. This closes
the current-parent assigned-name owner; other A work, B–E, the final 769 replay
and hosted acceptance remain open. No new global total or fresh checker run,
historical full developer CI or certificate walk is claimed.
