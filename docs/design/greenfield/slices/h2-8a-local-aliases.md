# H2.8a A6-5: local targets of declaration export aliases

Kind: runtime; trusted checkpoint 622850a851beff2494aed275fb3b90257d874727.
The complete 32-input before is recorded; readiness must pass before editing.
A6-2 repaired direct-require declaration runtime stops. Its original ExportForms
ES2015/ES5 commands still serialize names instead of ns as names. This packet
owns serializeMaybeAliasAssignment's target selection and same-source branch.
The independent private-inclusion ordering failures remain open in A6-2.

A6-5-1 replaces the serializer's getImmediateAliasedSymbol/custom resolved-node
fallback with the existing checker get_target_of_alias_declaration(decl, true).
Only its visibility becomes pub(crate); existing checker dispatch and lazy
resolution behavior remain unchanged. The alias declaration is selected by the
existing find-last producer. No new resolution cache or symbol identity exists.

A6-5-2 extracts the expression through getExportAssignmentExpression for export
or binary assignments, otherwise getPropertyAssignmentAliasLikeExpression.
Port getFirstNonModuleExportsIdentifier over typed identifier/qualified/property
nodes, invoking the existing binder predicate for module.exports. Only entity
name expressions use that helper. Resolve the first identifier with All,
ignoreErrors=true, dontResolveAlias=true and the serializer's enclosing node,
then include referenced-or-target before disabling tracking. This preserves
local require alias identity instead of eagerly substituting its external module.

A6-5-3 retains the export=/default symbolToExpression branch using All; a direct
identifier uses its original idText, a class expression uses the target's internal
name, and other expressions create a local non-type-only import-equals from the
existing symbolToName(target, All, expectsIdentifier=false), then export that
allocated name. Make the existing symbol_to_name helper visible only within its
parent module. Do not substitute symbolToEntityNameNode: that separate helper
walks raw parents and would skip the existing remapped symbol-chain semantics.
Preserve approximate-length updates before factories and insertion order.
Always restore disable_track_symbol before returning either success or failure.
The nonlocal widened-type/function-namespace branch remains unchanged.

Allowed production surfaces are crates/checker/src/node_builder/statements.rs
(the alias serializer and expression/root-identifier helpers),
crates/checker/src/modules.rs (visibility only on the existing alias-target
query), and crates/checker/src/node_builder/chains.rs (visibility only on
symbol_to_name). No tracker queue behavior, name allocator implementation,
resolver trait, parser, emitter, printer or expected tuple changes are included.

| State / transition | Native owner and consumer | Disposition |
| --- | --- | --- |
| Alias declaration and immediate target | checker typed NodeId and SymbolId, existing target dispatch | partial-or-stale serializer selection; reuse actual owner |
| Local target source identity | binder source root versus serializer enclosing declaration | shared-prerequisite, unchanged |
| Written alias expression and first identifier | original typed NodeData; existing binder module.exports predicate | missing extraction and root selection |
| First identifier's local alias | existing resolve_entity_name_ex with explicit enclosing location and dontResolveAlias | missing borrowed query, no new cache |
| Private inclusion | existing StatementSerializer include_private_symbol | shared-prerequisite; queue-order discrepancy stays separately open |
| Exported/written/internal names | idText or current internal name or current unused-name allocator, per branch | missing three-way branch; allocation algorithm unchanged |
| Compound entity name | existing chains::symbol_to_name and factory import-equals | missing consumer, existing chain/remapping owner reused |
| Suppressed tracking | scoped bool restored on both success and fallible exit | partial-or-stale restoration; no lifetime extension |

E-PROTOCOL, E-RESOLVER-BASE and E-METADATA-BASE are premise-unchanged but rechecked. Checker state and original syntax remain borrowed within the current
emit request, synthesized output lives only in TransformArena, and no public
API or cross-crate fact carrier changes. The existing ancestor/name mappings
and tracker ownership stay intact. This packet does not certify the separate
synchronous private-inclusion callback ordering discussed in the A6-2 follow-up.

The new complete observer covers twelve JavaScript forms (require aliases,
local identifiers, named/anonymous class expressions, local/required compound
members, module.exports member roots, object shorthand/renaming, default and
export-equals), plus four TypeScript adjacent controls, at ES5 and ES2015.
All 32 commands are independently observed twice with full output callbacks,
diagnostics, status, result presence and exit. The immutable global ExportForms
pair is checked separately; the original 22-case selector keeps its other
failures. No original membership or required_slices disposition changes.

Pinned TypeScript 6.0.3 bodies in vendor/typescript-6.0.3/lib/_tsc.js:

- getExportAssignmentExpression: 15736–15738, SHA256 `a34a30e9843eea25cb7376c14726ad20870f697d287870c83345f4a073a0babf`.
- getPropertyAssignmentAliasLikeExpression: 15739–15741, SHA256 `df19f89242c4f83ad477952e5a8fb4895b127e4394fbd4e9a7bbd731c8f4709a`.
- getFirstNonModuleExportsIdentifier: 85964–85982, SHA256 `fd0ad02c73705d11e5747df7e983ccdf1830a844a7e01de093b64c3b98eca502`.
- getDeclarationOfAliasSymbol: 48495–48497, SHA256 `2030901db3779ce4f0290f76dbd86aa7e34226a6e9d37f488d2b5449fb3f6687`.
- getTargetOfAliasDeclaration: 49071–49108, SHA256 `162af5ad124b130bc6f80f131212585721d1d34d502fc735b9eaea94780b01d0`.
- getTargetOfAliasLikeExpression: 49045–49064, SHA256 `6935da2d1759d7c4ae48909f28a7de4a2985b58a1a5dd9ba53fbdf98c9b57ebc`.
- resolveEntityName: 49292–49393, SHA256 `0c5ce0e5980d5548db101cd9240b04944dea6e35cde2b0b3416210816fdb85b9`.
- isEntityNameExpression: 17128–17130, SHA256 `2e7694f05260a41567e84db34bfbfd9ec77c27e3c37116b2a9cf88f0ddccfeee`.
- isModuleExportsAccessExpression: 15052–15054, SHA256 `a868c0d25d139d6dd4e6ea935a17e4fe03908764961fbb0ecfe786317f6b2f71`.
- serializeMaybeAliasAssignment: 54966–55082, SHA256 `76bea3c93c2aab13e158de586792f0b9e24d6840feaaf5ff7151b7d97ef249c0`.
- serializeExportSpecifier: 54947–54965, SHA256 `b3fb36e9d0e5bf03d237cacfbdc3a632a48530faab565d52fa493c0d7c88e32c`.
- symbolToName: 53315–53336, SHA256 `8000600326491063f035e6aea718ffc812fadb6de27ef8661ac40f43f7f91d26`.
- includePrivateSymbol: 54180–54186, SHA256 `47e3cfac44d1da24d0576ff68a44b9145c0b4d6d090ec814e2fb751ef8a09906`.
- getUnusedName: 55390–55412, SHA256 `2546367acf7e260487b409700963b0f5415710fa780cf2dab270d0926af91ab0`.
- getInternalSymbolName: 55429–55437, SHA256 `dabcf231a2d04cdc2cacbf91315a119bb0d9036f89eba69a2df05969003ddce2`.

The complete native before exits 101 in 34.84 seconds: 22 commands are exact
twice and ten commands fail on their first complete comparison (require-renamed,
local-renamed, class-named, compound-local and compound-require at both targets).
All eight TypeScript controls and fourteen JavaScript controls are exact. The
32 full TypeScript tuples were observed twice before any native change. The
immutable record is ratchets/h2-8a-local-alias-before.v1.json; source/log copies
are outside the checkout, located by target/h2-8a-local-alias-before-location.txt.
The readiness manifest binds these 32 rows, the unchanged two original commands,
15 pinned owner bodies, three ordered implementation steps, three architecture
rows and current baseline Rust sources. There are zero unresolved or
undispositioned rows in this local-target boundary. Other full-tuple failures
remain failures with their separate owners.

## A6-5-4: alias caller and target-name amendment

The first after command exits 101 in 34.05 seconds: 20/32 new commands are
exact and twelve fail, including two formerly exact module-exports-member
controls. The original binary does not run because Cargo stops after contracts.
Source copies and the full comparison log are retained outside the checkout,
located by target/h2-8a-local-alias-first-after-location.txt. The initial
local-target change is not qualified in isolation.

Review of the actual caller identifies two missing serializeAsAlias branches.
getSomeTargetNameFromDeclarations is eligible only when the target is a
shorthand ambient module; native invokes it for all targets, causing an
export property's name to replace its local target's name. Second, ordinary
binary/access aliases emit serializeExportSpecifier(localName, targetName);
only default/export= names call serializeMaybeAliasAssignment. Native routes
all these nodes into the latter function. The local-target repair therefore
reached two controls that upstream never dispatches to this branch.

A6-5-4 changes serialize_as_alias to query the existing actual alias-declaration
target with dontRecursivelyResolve=true before get_merged_symbol, guards the
existing declaration-name fallback with the existing typed
is_shorthand_ambient_module_symbol predicate, and separates the ExportAssignment
arm from binary/property/element arms with the upstream default/export= test.
Ordinary property aliases call the existing export-specifier helper with the
current local and target names. Preserve allocation/private-inclusion order,
all import branches, JSON handling and the untouched default-import option
policy. No name allocator, binder or module resolution implementation changes.

This amendment adds only serialize_as_alias within the already allowed
statements.rs path. The three architecture rows remain premise-unchanged but rechecked.
The original 32 observations remain immutable. Eight additional complete
shorthand-ambient commands (default, named, namespace and import-equals, each
at ES5/ES2015) check the preserved positive side of the new predicate; their
separate observer and fixture must be recorded before the caller edit.
The Class|Property special dispatch in serializeSymbolWorker is a separately
identified missing branch; it is not exercised by these Alias-symbol controls
and is not silently included in this amendment.

Additional pinned upstream bodies:

- serializeAsAlias: 54707–54946, SHA256 `60776812c24ded3bcf5a0336651b8c0726ab37d473a3a104a5495b4937227277`.
- getSomeTargetNameFromDeclarations: 54687–54706, SHA256 `99649f147e539a79f374ae318992457f809ff16febf6e0e51244acade67ec29e`.
- isShorthandAmbientModuleSymbol: 13725–13730, SHA256 `145568251fbab021f53f6cec68de6c9548409ec13d04160d2562739c46c89317`.

The additional ambient before exits 0: eight complete commands exact twice
in 10.34 seconds (target/h2-8a-ambient-alias-before.log). Its immutable record
is ratchets/h2-8a-ambient-alias-before.v1.json. The source/log copies are outside
the checkout at the location recorded by target/h2-8a-ambient-alias-before-location.txt.
The amended readiness now covers 18 owner bodies, four steps, three architecture
rows, 32 initial witnesses plus eight additional positives, and two unchanged
original commands. Zero unresolved or undispositioned rows are claimed only
within the explicitly bounded caller/target branch set.

After A6-5-4, the complete focused replay exits 0: all 32 initial and eight
ambient commands are exact twice in 42.18 seconds, followed by both unchanged
ExportForms originals exact twice in 15.98 seconds
(target/h2-8a-local-alias-caller-after.log). The ten initial differences and two
intermediate regressions are repaired. Global 769 replay remains pending; ClassExtendsVisibility's two ordering failures
retain their separate A6-2 disposition. No whole-H2.8a pass is inferred.

The actual adjacent checker selection passes all 17 statement/chain tests
in 0.03 seconds (target/h2-8a-local-alias-adjacent-correct.log, exit 0). This
covers unique require aliases, name collisions, namespace scopes, class shapes,
reexports and node provenance. The initial underscore-style filter selected
zero tests and provides no validation (h2-8a-local-alias-adjacent.log). No
production changes occurred between the two invocations.

An additional unchanged original is selected for the local compound-expression
consumer: jsDeclarationsConstsAsNamespacesWithReferences.ts#default. Its
colors.royalBlue reference is serialized within the synthetic brandColors
namespace, exercising the newly restored symbolToName/import-equals branch
that ordinary root export aliases dispatch around. Its complete TypeScript
observation and both native before failures were already captured by the
immutable global observation at 43e1107e8. A6-2's intervening changes affect
require/import-equals declarations, which this input does not contain; the
local-target body was unchanged through 622850a85. The new projection retains
the exact original input, complete tuple and original required_slices record.
The whole-case result will be recorded separately from the two ExportForms
commands; any remaining difference stays a failure.

That additional complete original comparison passes twice in 15.39 seconds
(target/h2-8a-local-namespace-alias-after.log). It supplies a direct production
witness for the compound-expression local import-equals branch. The complete
769-command selector remains unchanged and still requires a final replay.

The recorded readiness witness set now has three original commands: the initial
ExportForms pair and the additional compound namespace reference. This is an
evidence-only extension; no new runtime edit follows from it.

A subsequent replay of all 22 original require-alias commands at 7ebdee933
finds a regression in ReexportedCjsAlias ES2015: its second destructured alias
import is absent. The ES5 sibling and eighteen other commands are exact; the
two pre-existing ClassExtendsVisibility order differences remain. The complete
current before is ratchets/h2-8a-class-original-before.v1.json. This new
regression remains an A6-5 follow-up; the narrow 40+3 successes and 17 unit
controls above are not a whole-alias compatibility claim.
