# H2.8a A6-9: separate declaration export names from internal bindings

Kind: bounded runtime repair at checkpoint 8ed4ee178e9a4837c124b66b4e2d606fa6b4610b.
Readiness must pass before editing production. Allowed runtime path is
crates/checker/src/node_builder/statements.rs, only serialize_as_alias's
ExportSpecifier arm. Existing tsc-port/span/hash annotations cover this body.
The versioned manifest pins all 56 fresh commands and twelve original commands.

The first 48 native commands have 26 exact twice and 22 failures (36.61 seconds,
exit 101). Fourteen are declaration export-name differences. Eight quoted-name
commands fail first on JavaScript bytes: exports.default versus exports["default"]
or __importDefault(lib).default versus __importDefault(lib)["default"]. Their
whole-command comparators remain unconditional. Their later declaration writes
are not claimed exact from a comparison that stopped on JavaScript. That syntax
provenance defect belongs to the following emitter repair; no emitter changes
are allowed here. New CommonJS class/function default reexports supply eight
additional commands with explicit allowSyntheticDefaultImports=true and
esModuleInterop=false; all eight are exact twice before (10.24 seconds, exit 0).
The first attempt for these eight stopped in test option projection before
compiler execution. Both option fields are now copied directly to the existing
CompilerOptions. That infrastructure failure is retained in the harness-before
log and is not counted as a product failure.

All twelve original commands fail twice (35.83 seconds, exit 101). DefaultsErr
and ExportSpecifierNonlocal (two targets each) differ only in one declaration
write. Eight Node ImportHelpersCollisions3 commands each differ in two declaration
writes and one JavaScript write. Their missing tslib import/qualification is
independent and remains a mandatory full-command failure until its emitter owner
is repaired. Both repetitions are identical; all non-write fields match upstream.

A6-9-1 preserves semantic name roles in the existing typed ExportSpecifier arm.
Resolve the actual alias declaration and its fresh target as before. Preserve
get_internal_symbol_name and include_private_symbol before the dispatch. Obtain
the external module literal with the existing alias_module_specifier. When it
exists and the actual ExportSpecifierData.property_name satisfies the existing
checker.module_export_name_is_default predicate, set verbatim_target_name to
InternalSymbolName::DEFAULT. Call serialize_export_specifier with the unescaped
exported symbol.escaped_name instead of local_name. Its target remains the
verbatim target for external exports, and the internal target for local exports.
The existing factory handles equality by omitting propertyName. Keep the raw
allowSyntheticDefaultImports check and target/internal-name allocation order.

Local keywords and default must retain their original exported names, even when
the same symbol needs an internal _class or _default binding. Upstream's extra
keyword post-export declaration is preserved, including the observed _class as
class entry. Leading underscores are unescaped by the existing binder helper.
The explicit default-source override follows the pinned upstream arm. The
CommonJS controls remain adjacent positives: the hypothesis that they would
isolate a mismatched target name was not reproduced. They do not independently
prove a previously failing override because the existing target path already
produces default in these cases.

E-PROTOCOL, E-RESOLVER-BASE and E-METADATA-BASE are premise-unchanged and rechecked:
the same live CheckerSession owns the alias query, existing parsed NodeIds supply
name kind and text, and the same declaration factory creates output nodes in the
session arena. No public resolver query, provider filter, cache key, callback,
metadata identity or arena lifetime changes. E-NAMES-BASE's emitter generated
binding finalizer is distinct from the checker's remappedSymbolNames and is not
modified by this repair. No source-text scanning or output replacement is used.

The 48 controls cross JS/TS, ES5/ES2015, local/external exports, default and renamed
names, class/type keywords, double underscores, merged typedef/default names,
quoted default source/export names and multiple sibling reexports. The additional
eight cross JS/TS and ES5/ES2015 with class/function CommonJS targets. Every TS
command executes twice and stores complete output bytes, paths/order, callback
metadata, reported/emit diagnostics, result presence, status and exit. Native
exact fresh commands repeat twice; a failed fresh command reports its first full
comparison mismatch. The original twelve always collect two complete tuples.
No expectations, options or membership are rewritten to turn a difference green.
These valid module literals do not qualify malformed non-string module syntax.
No sink-failure branch is changed; sink fault injection is not applicable here.

Before records: ratchets/h2-8a-export-names-before.v1.json,
ratchets/h2-8a-cjs-default-reexport-before.v1.json and
ratchets/h2-8a-export-names-original-before.v1.json. Source/log snapshots are
outside the checkout at the corresponding target/h2-8a-*-before-location.txt.

After checks are both fresh tests selected by their full names
cjs_default_reexport_names_match_complete_typescript_observations and
export_specifier_names_match_complete_typescript_observations, and the
original_export_specifier_names_match_complete_commands projection. Required
adjacent checks are 40 local/ambient alias commands, 48 repeated-target alias
commands, 22 original require aliases and the checker statement/chain/specifier
unit tests. All owned nonquoted commands must become exact, every formerly exact
control stays exact, original declaration writes match and independent JavaScript
failures remain visible. The quoted cases stay open for the emitter repair; do
not claim all 56 exact. Whole769 replay, remaining A causes, B–E and final hosted
acceptance remain open. Historical full developer CI/walk are omitted under the
schedule's current user-authorized lightweight workflow.

Pinned TypeScript 6.0.3 bodies in vendor/typescript-6.0.3/lib/_tsc.js:

- serializeSymbolWorker: 53992–54179, SHA256 `dc9bf6e639d95e72cefd3e99de392150843321b4b1f785340d52751bbe4bbc38`.
- serializeAsAlias: 54707–54946, SHA256 `60776812c24ded3bcf5a0336651b8c0726ab37d473a3a104a5495b4937227277`.
- serializeExportSpecifier: 54947–54965, SHA256 `b3fb36e9d0e5bf03d237cacfbdc3a632a48530faab565d52fa493c0d7c88e32c`.
- moduleExportNameIsDefault: 13032–13034, SHA256 `a5be65407a7f18b94596f3fec078ba710f3c23f8594057edb6e1550afa87cdf0`.
- unescapeLeadingUnderscores: 11441–11444, SHA256 `e8294a1e4ef10b8ca2bcce06045e22adab6689e46b655acf51bacc3810ef5271`.
- getDeclarationOfAliasSymbol: 48495–48497, SHA256 `2030901db3779ce4f0290f76dbd86aa7e34226a6e9d37f488d2b5449fb3f6687`.
- isAliasSymbolDeclaration: 48498–48500, SHA256 `56e340e6315f9514b9b10ee2da3e5255c009cf09db1260c3198ac6088db29832`.
- getTargetOfExportSpecifier: 49003–49031, SHA256 `7d710915d1c15e2eff24a3f23fffdcac5bd92f5b92d1ce16c411a297346265d6`.
- getTargetOfAliasDeclaration: 49071–49109, SHA256 `d7fb459cc5e3f09b7e9d4378284e84f19895c29a94f6bf97d9b170afbb1a6d0a`.
- getUnusedName: 55390–55412, SHA256 `2546367acf7e260487b409700963b0f5415710fa780cf2dab270d0926af91ab0`.
- getNameCandidateWorker: 55413–55428, SHA256 `b496a6d0abb70eff10421196e949b96fc10a935837e7a12b28b5094ccc2e3b92`.
- getInternalSymbolName: 55429–55437, SHA256 `dabcf231a2d04cdc2cacbf91315a119bb0d9036f89eba69a2df05969003ddce2`.

The A6-9 after executes all 56 fresh commands: 48 exact twice and eight
independent quoted-name JavaScript failures (60.66 seconds, exit 101,
two tests). All fourteen formerly failing nonquoted declaration-name commands
now match; all 34 former positives remain exact. Each of the eight first byte
mismatches is byte-identical to the immutable before mismatch. Later fields of
these failed fresh commands are not claimed exact. The unconditional comparisons
remain red until the following emitter repair; no expectation was edited.

The original twelve now have four whole commands exact twice and eight retained
ImportHelpersCollisions3 failures. Every declaration write matches upstream.
Each remaining failure has exactly one unequal JavaScript write,
out/subfolder/index.js, byte-identical to its previous missing-tslib output.
All other writes, callback metadata and non-write fields match, in both complete
repetitions. The same original binary also passes all 22 adjacent require-alias
commands twice (two tests overall, 90.57 seconds, exit 101 only
for the eight named independent failures). All 88 local/ambient/repeated-target
alias commands pass twice (104.05 seconds, three tests, exit 0), and
all 25 checker statement/chain/specifier tests pass (0.02 seconds,
exit 0). Complete log hashes, final production bytes and all sixteen remaining
original failure tuples are retained outside the checkout at the location in
target/h2-8a-export-names-after-location.txt, with the outcome record in
ratchets/h2-8a-export-names-after.v1.json. No full769/hosted/full-CI result is
claimed by this bounded checkpoint.
