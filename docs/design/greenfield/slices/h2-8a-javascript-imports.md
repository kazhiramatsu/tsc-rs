# H2.8a A6-4: retain JavaScript import aliases

Kind: runtime, complete before and initial readiness passed; literal-clone amendment below.
Trusted global before is 43e1107e8; current checkpoint 2793145a6 changes export
annotations only. A6-2/3's staged declaration-alias and root-diagnostic changes
are independent of the unchanged builtins.rs dependency. H2.8 remains open.

Seven original failures lose ordinary imports in JavaScript: elidedJSImport1,
jsDeclarationsFunctionLikeClasses ES2015/ES5, and four Node module variants of
nodeModulesAllowJsSynchronousCallErrors. Original exportNamespace_js also loses
an import, but has a separate declaration quote difference and stays an open
full-tuple failure until that owner is repaired. The complete global oracle
and before evidence remain unchanged.

Upstream shouldEmitAliasDeclaration returns compilerOptions.verbatimModuleSyntax
OR isInJSFile(node) OR resolver.isReferencedAliasDeclaration(node), in that
short-circuit order. The native TypeScript visitor skips both first branches and
queries the resolver directly for default, namespace, named and import-equals
aliases. This drops unused JS imports and JS imports whose semantic alias has no
value, although upstream preserves them. Explicit isTypeOnly remains an earlier
rejection at each relevant syntax owner. The existing local import-equals
fallback still checks external-module status and top-level value aliases only
when the shared predicate is false.

A6-4-1 copies the existing verbatimModuleSyntax option into a boolean owned by
TypeScriptTransformer, then into its per-source TypeScriptVisitor. It adds the
private should_emit_alias_declaration helper with the exact short-circuit
predicate. isInJSFile reads the input node's typed NodeFlags::JAVA_SCRIPT_FILE;
it does not infer the language from text, extension, checkJs, or binding results.
A6-4-2 replaces the four direct resolver uses (default import clause, namespace
binding, each non-type-only named specifier, import-equals declaration) with the
helper. Preserve named-array order, source identity, type-only erasure, update
factories, internal import-equals fallback and subsequent module selection. The literal-clone amendment below owns the
newly exposed ECMAScript import-equals argument branch.
A6-4-3 runs complete new controls and original commands, plus existing root
format and module/export regressions. No front-door option guard is removed;
verbatim admission and empty-import semantics remain H2.8c's separate product.

Allowed production path: crates/emitter/src/builtins.rs, the option snapshots,
TypeScriptVisitor helper, the four call sites, and the stale import-equals ledger
span corrected to the exact owner below, plus the bounded ECMAScript
import-equals literal-clone call recorded in A6-4-4. No checker fact producer, parser,
printer, module factory selection, helper import builder, name allocator or
output planner changes belong to this packet.

| Semantic state / branch | Native producer/owner and consumer | Before disposition |
| --- | --- | --- |
| verbatim option truth value | CompilerOptions -> transformer bool -> visitor bool; one transform/source lifetime | missing snapshot in alias predicate; admission remains separately guarded |
| JavaScript node identity | parsed NodeFlags in TransformArena, read at the original alias node | shared-prerequisite; producer and clone/update code unchanged |
| Referenced alias fact | live borrowing EmitResolver; queried only after both earlier branches are false | already-active query, partial-or-stale call condition |
| Explicit type-only node | typed ImportClause/ImportSpecifier/ImportEqualsDeclaration data | already-exact prior erasure, preserved |
| Retained node/array identity | existing update_generic and update_node_array factories | shared-prerequisite; retain original locations, comments and member order |
| Runtime import lowering | existing CommonJS/ECMAScript/Node module visitors | shared-prerequisite, complete callback bytes and command diagnostics rechecked |

Pinned upstream bodies in vendor/typescript-6.0.3/lib/_tsc.js:

- isInJSFile at 14886–14888: SHA256 `c5f0db66356c51537ce1e7c91692c0775f3db5764d39100f22ad85b89e0ec9a1`.
- visitImportClause at 95538–95543: SHA256 `b5034ee4461cd93181909793decb1a90aad8335d0d94483ff8fb6d354ca610d9`.
- visitNamedImportBindings at 95544–95552: SHA256 `0f7aeaf0920cb053652be2bc7d159a325d018951be96b70a8ea5dd26b6b3fe16`.
- visitImportSpecifier at 95553–95555: SHA256 `e5e75294badae2b6b365fd46d3275b40d4f1b5cd4a5dba6365b757d7b60a7abe`.
- shouldEmitImportEqualsDeclaration at 95602–95604: SHA256 `57b4513e98e044e5db682e495dffcf5e550fb2c7b0d3584ec955750cd45c0479`.
- visitImportEqualsDeclaration at 95605–95653: SHA256 `b05420512b8b9e2333d1b23ed7ec1b6a327903fcc381ed44f3fa5f46ff66c7ef`.
- shouldEmitAliasDeclaration at 95846–95848: SHA256 `768f0d459b8a033e5fbad0737593ac3b03d7f123c30ea33737cb92ce2d6604d1`.
- visitImportEqualsDeclaration at 112558–112563: SHA256 `a4085638a235566d7c7160c24c451e44f2cb394d2a497acbdb57c82a0299fcf7`.
- visitImportEqualsDeclaration at 113569–113601: SHA256 `218804820ede70d9f3cc9ab46b1244b21c98ac1c8bbaa4125eb39b9dbbc84a7f`.

E-PROTOCOL, E-RESOLVER-BASE, E-METADATA-BASE and E-ORDER-G are
premise-unchanged but rechecked. Their current architecture rows retain the
0653e10d validation lineage; this packet rechecks the current actual owners.
Semantic facts stay behind the borrowed resolver, parser flags stay typed in the
arena, and no transform is added or reordered. The new bool owns only an option
value and does not extend any checker or source lifetime. No new public type,
syntax provenance, hook registration or callback capability is introduced.

The fresh observer scripts/observe-javascript-import-retention.mjs records 42
complete commands twice. It crosses TS/JS, CommonJS/ESNext/Node-ESM/Node-CJS,
and default/namespace/named/type-only/import-equals syntax, with two checkJs=false
JS controls. All use the same virtual dependency and explicit output directory;
the complete fixture retains diagnostics, all output bytes/callback metadata,
result presence, lists, status and exit. The shared native comparator receives
an explicit checkJs option projection; its initial projection refusal is retained
separately and is not a runtime before. The corrected native before must finish
before any builtins edit. The seven unchanged original commands are a separate
full-tuple projection of the 769 candidate band.

The readiness manifest pins twelve upstream body rows, four concrete steps,
four architecture rows, all 42 fresh witnesses, seven original input/observation
rows, the baseline production dependencies, and the corrected native before.
It requires zero unresolved or undispositioned rows in this predicate boundary.
Expected results are never edited after native execution. Any newly exposed
module/declaration discrepancy remains a failed full comparison with its own
owner; it cannot become a success or expand later-reference membership.

The corrected complete native before exits 101 in 33.84 seconds: 25 exact
controls and 17 differing JavaScript imports. All twenty TS controls and the
five JS controls that erase explicitly typed or unsupported import syntax are
exact. The immutable before record is
ratchets/h2-8a-javascript-import-before.v1.json; its source copies, log and
complete reference are outside the checkout, named by
target/h2-8a-js-imports-before-location.txt. The 17 failures are retained as
failures and no builtins runtime code changed before this result.

## A6-4-4: ordinary module-literal clone amendment

The first after replay exits 101 in 94.20 seconds: 41/42 new controls are
exact; the 50 existing root diagnostic/module-format controls are exact. Cargo
stops after the failing contracts binary, so the seven-original binary has not
yet executed. The sole new failure is js/node-esm/import-equals: native emits
__require('./dep.js') where TypeScript emits __require("./dep.js"). The new
retention predicate exposes an existing later-owner omission. The immutable
42 expected rows and original seven rows remain unchanged.

A6-4-4 adds the missing factory.clone_node call on the ordinary string argument
in EcmaScriptModuleEqualsVisitor::transform_import_equals. createRequireCall
uses getExternalModuleNameLiteral, whose ordinary fallback clones the parsed
literal. The clone carries text, flags and original-node identity through the
existing factory while taking synthesized source coordinates; the printer
then applies its existing literal emission rules. No quote or text rewrite is
introduced. Factory implementation, original-node metadata and printer remain
unchanged. The preserved module name and require-helper allocation order stay
unchanged. Reuse existing 42 complete controls and seven originals as the
amendment's witness boundary.

The preceding resolved external-module-name and renamedDependencies branches
are not claimed by this amendment: this ECMAScript visitor has no host/resolver
inputs for those branches. Existing CommonJS external_module_name_literal
already implements its resolved-name/clone path. Custom renamedDependencies
remains API1; other reachable named-module paths must retain their own full
matrix disposition. Relative-extension rewriting is unchanged and its original
failures remain open. This amendment introduces no new resolver or host query.
The same four architecture rows are premise-unchanged but rechecked against
the existing arena-owned clone and original-node mapping.

Additional pinned upstream bodies:

- getExternalModuleNameLiteral at 27713–27719: SHA256 `2d6cd262d872b1b38c0094ffd8a9d77b5864e3d8f912de6e8dd07e1e7f5d5852`.
- createRequireCall at 113495–113568: SHA256 `8b039c02e0c00d4f0716746a728ab1c00772ad8f4aa97c2cba5bbf873e271cd6`.
- cloneNode at 24436–24466: SHA256 `d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6`.

The final bounded replay after the clone amendment exits 0:
all 42 fresh commands are exact twice (55.95 seconds), followed by all seven
unchanged original commands exact twice (35.53 seconds), in
target/h2-8a-js-imports-clone-after.log. This closes the predicate and ordinary
literal-clone witnesses. Global 769 replay, remaining A6 failures and broader
H2.8a–e closure are pending; no whole-slice qualification is inferred.
