# H2.8a A6-2: CommonJS aliases in declaration statements

Kind: runtime. The original 769-command comparison at 43e1107e8 stops before
writing in thirteen JavaScript declaration cases because serialize_as_alias
requires a PropertyAccessExpression initializer even for a direct require call.
The immutable global before contains both repetitions. The current checkpoint
2793145a6 changes only export-assignment annotation checking; this packet's
statement, factory, parser and specifier dependencies remain identical to the
before revision. H2.8a stays open and no new profile admission follows this fix.

The pinned serializeAsAlias owner first resolves/merges the alias target,
chooses its internal name and includes its private symbol. A variable with a
property-access initializer emits two import-equals declarations: a unique
module namespace, then its member. Every other variable falls through to the
import-equals arm. That arm first handles a JSON export-equals target with
serializeMaybeAliasAssignment; otherwise it chooses an entity name only for a
non-ValueModule target of an actual ImportEqualsDeclaration. A variable always
uses an external module reference. External imports receive no export modifier;
local aliases retain the caller's modifiers. Generated imports are not type-only.

Allowed production path: crates/checker/src/node_builder/statements.rs, the
VariableDeclaration/ImportEqualsDeclaration arms of serialize_as_alias and a
private shared helper spelling their common arm. Other alias branches, relation
algorithms, module resolution, parser, printer, and public APIs remain unchanged.
A6-2-1 retains the guarded property-access arm, corrects its module-symbol input
to target.parent.unwrap_or(target), and routes other variable initializers into
the shared arm. A6-2-2 restores that arm's JSON special case, variable exclusion
from local imports, false is_type_only, modifier choice and approximate-length
updates. Reuse the existing factories, alias serializer, specifier and name
helpers in upstream order; never derive node kinds or module identity from text.

| Semantic fact / transition | Native owner and lifetime | Before gap / disposition |
| --- | --- | --- |
| Alias declaration and merged target | CheckerState's SymbolId/NodeId tables, borrowed within the live resolver | shared-prerequisite, unchanged; target resolution precedes all factories |
| Variable initializer branch | NodeData::VariableDeclaration and typed initializer NodeData | missing fallthrough; preserve the property branch's required name error |
| Target module identity | target_data.parent.unwrap_or(target), existing specifier_for_module_symbol | partial-or-stale property arm passes target alone |
| JSON source identity | SourceFile node plus binder NodeFlags::JSON_FILE, produced by parse_json_text | shared-prerequisite; flag is the existing parser-owned JSON discriminator, not an extension guess |
| Import kind | target SymbolFlags::VALUE_MODULE and alias declaration SyntaxKind | missing variable exclusion and JSON branch |
| Synthetic names/references | TransformArena factory nodes and GeneratedIdentifierFlags::NONE, one declaration request | shared-prerequisite; preserve unique namespace identity in the member reference |
| Results and modifiers | serializer's ordered results, add_result, modifier flags and approximate length | preserve insertion order and external/local modifier distinction |

The dependency closure below pins exact bodies in vendor/typescript-6.0.3/lib/_tsc.js.
Existing includePrivateSymbol/getInternalSymbolName stay before the switch;
getSpecifierForModuleSymbol and createImportEqualsDeclaration are revalidated
inputs. serializeMaybeAliasAssignment owns JSON type serialization. Its deeper
existing type builder remains unchanged; any independent output difference is
an open failure, never a new expected tuple.

- isJsonSourceFile: 14122–14124, SHA256 `916f4ee9aa201d6a1f517bb01eff1ab541c3e5d61580eb92b2c7b5c392d58c7a`.
- createImportEqualsDeclaration: 23460–23473, SHA256 `cd9f16baa6064edf0ddd264a39e0a8e83e7c70d6d9dc77e0765829dc80b493b9`.
- getSpecifierForModuleSymbol: 53060–53109, SHA256 `cc081ccc9162d99c71cfb5013a0786210de8d66472567a9ee1d6eab90f686463`.
- includePrivateSymbol: 54180–54186, SHA256 `47e3cfac44d1da24d0576ff68a44b9145c0b4d6d090ec814e2fb751ef8a09906`.
- serializeAsAlias: 54707–54946, SHA256 `60776812c24ded3bcf5a0336651b8c0726ab37d473a3a104a5495b4937227277`.
- serializeMaybeAliasAssignment: 54966–55082, SHA256 `76bea3c93c2aab13e158de586792f0b9e24d6840feaaf5ff7151b7d97ef249c0`.
- getInternalSymbolName: 55429–55437, SHA256 `dabcf231a2d04cdc2cacbf91315a119bb0d9036f89eba69a2df05969003ddce2`.

E-PROTOCOL and E-RESOLVER-BASE are premise-unchanged but rechecked: private
checker state stays in the existing borrowing resolver; the printer receives
only arena nodes. E-METADATA-BASE is premise-unchanged but rechecked: synthetic
unique-name identity and parsed source flags keep their separate owners. Their
architecture rows retain their recorded 0653e10d validation lineage; that older
lineage is not substituted for this packet's current exact comparisons.

The manifest pins this packet, current architecture, original census/input/
observation/before artifacts, and baseline statements/specifier/factory/parser
and binder dependencies. It records seven upstream owners, two steps, three
architecture rows, twenty-two complete original witnesses and zero unresolved
or undispositioned rows within this bounded arm. Thirteen before failures and
nine already-exact controls cover direct requires, JSON, property requires,
destructuring aliases, TS import-equals and local namespace imports. Every
original complete TS tuple remains unchanged and is compared twice. The test
is original_require_alias_declarations_match_complete_commands; the complete
769-command selector remains unconditional. Existing node_builder_statements
and module/export unit tests provide adjacent lifecycle/factory regressions.
A new mismatch after removing the runtime stop requires its own owner review.

The first complete after run exits 101 in 98.99 seconds for the combined
36-command projection: all eight export-annotation controls and all six root
diagnostic controls are exact twice; 18 of these 22 alias commands are exact
twice. All thirteen runtime stops disappear and nine originally failing
commands become exact. ClassExtendsVisibility ES2015/ES5 still orders its
private require after the merged namespace, and ExportForms ES2015/ES5 still
uses names where upstream uses ns as names. These four complete unequal
results stay failures. New source copies, comparison log and full failing
tuples are retained outside the checkout in the directory recorded by
target/h2-8a-a6-2-3-after-location.txt. Their callback/private-inclusion and
local-target serialization owners require separate follow-up readiness.
