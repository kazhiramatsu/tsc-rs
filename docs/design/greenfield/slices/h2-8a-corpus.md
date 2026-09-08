# H2.8a A4: original output corpus and System import identity

Kind: runtime; status: 24 focused and 23 original cases exact twice,
2026-09-08. Base `56e666af51a12ddd3339bf08b17680148d2eeeba`; this packet runs
on the same A1–A3 train. Its boundary is the 23 unchanged original H2.7d/e
inputs whose later owner is H2.8a, plus focused System dynamic-import controls.
The historical 291 qualified rows and all original inputs/observations remain
immutable. The fresh supplemental artifact is a reference observation, not a
qualification claim. H2.8b–e, H2.9, API1 and BLD1 retain their other requests.

`node scripts/observe-output-directory-corpus.mjs --write` runs every selected
original input on fresh TS6 Programs twice and compares every observation to
the frozen original tuple. All 23 agree. The unchanged Rust command comparator
in `h2_7d_original_corpus_shared.rs` repeats complete commands twice. Before A4,
22 agree and `outFilerootDirModuleNamesSystem.ts#default` differs only in the
dynamic import argument: TS emits `"a"`, Rust emits `"./a"`. The evidence is
`target/h2-8a-output-corpus-before.log` and the complete actual/expected records
under `target/h2-8a-output-corpus-failures/`. No output substitution is allowed.

| Step / gap | Upstream owner and branches | Native producer, consumer and lifecycle | Completion witness |
| --- | --- | --- | --- |
| A4-1 / partial | `getExternalModuleNameLiteral`, `_tsc.js:27713–27719`, enters resolution only for a StringLiteral first argument. `tryGetModuleNameFromDeclaration`, 27736–27738, obtains the resolved SourceFile; `tryGetModuleNameFromFile`, 27724–27735, selects its explicit name, otherwise its bundle path for a non-declaration file. | Private `SystemVisitor::visit_call_expression` uses the existing `external_module_names::resolved_external_module_name_literal` on the original call, only when the original first argument has StringLiteral kind. The helper projects typed original-node provenance and borrows `EmitResolver::get_external_module_file_from_declaration`; only the owned optional name survives the query. `SourceFile.module_name`, declaration status, EmitHost output options and common directory remain the existing owners. No text-based resolution or new identity cache. | Original failing bundle; relative and explicit-root bundles; named source; declaration and unresolved targets; expression, parenthesized literal and template arguments. |
| A4-2 / partial | `visitImportCallExpression`, 113092–113106, resolves before visiting the first argument. It selects a new literal only when the resolved name differs from the visited StringLiteral (or the visited node is not one), preserves the visited node otherwise, then emits zero or one argument. | Within the existing System visitor, visit only the first argument and build `context.import`. Retain equal literal spelling/ranges through the visited node, synthesize a string only for a changed name, and retain the existing original/range association of the resulting call. Additional import options are omitted as in this pinned System transform. Factory errors and unavailable required bundle resolver answers propagate unchanged; absent resolution preserves the visited expression. | Equal-name and quote controls; import options; no-argument malformed call; ES2015 and ES5 transform composition; complete callback bytes and diagnostic/exit tuples. |
| A4-3 / shared prerequisite | `getExternalModuleNameFromPath`, 16552–16566, and `getResolvedExternalModuleName`, 16535–16537, already supply static System dependency and registration identities. | Reuse the private external-module-name helper without edits. It borrows EmitHost/SourceFile and returns owned text per request; no persistent state or invalidation is introduced. Native common directories remain A2-owned. | Fresh 23 original cases, focused bundled/direct controls, original D283/E-only8 regressions. |

Caller order is the existing module pass after target transforms. The first
argument inspected by System is that current transformed argument; the resolver
separately projects the call back to its immutable parsed identity. In ES5 a
template literal has already become a StringLiteral and can therefore acquire
a bundle name; ES2015 keeps the template and its original module text. A literal
input's unchanged fallback is already its visited first argument. The
`SourceFile.renamedDependencies` host API has no active native input and remains
API1-owned; no public capability is added. A NoSubstitutionTemplateLiteral is
not a StringLiteral at this branch. Static import dependency collection,
CommonJS/AMD transforms, module resolution, factory/printer and Program APIs
are read-only dependencies for A4.

Allowed production path: `crates/emitter/src/builtins/system.rs`, restricted to
the dynamic-import visitor above. The integrator is the sole writer. Evidence
may add the focused observer/fixture/test, reuse the complete compiler test
comparator (adding only outFile option parsing), and wire the unchanged 23-row
supplement into the current hosted runner after it passes. Baseline body and
dependency hashes are recorded in the readiness manifest. An additional
behavioral gap outside this map requires an amendment and fresh observations.

Architecture dispositions, revalidated on current source rather than inherited
from their `0653e10d` historical qualification: `E-RESOLVER-IDENTITY-G` and
`E-RESOLVER-BASE` are premise-unchanged (the existing resolver projection and
typed error boundary are reused); `E-PROTOCOL` is premise-unchanged (a live
checker borrow supplies semantics, the sink still owns only output);
`E-ORDER-G` is modified-requalify for this System consumer, with the existing
transform registration and order unchanged. All four remain active; there is
no new public lifecycle or dormant activation. Final qualification of affected
rows is recorded at the full H2.8a close.

Before the production edit, freeze the focused oracle, register its tests and
run `python3 scripts/check-output-corpus-readiness.py`. The gate checks this
packet, five upstream spans, the unchanged original source join, new witnesses,
baseline Rust/dependencies, four architecture dispositions, three mapped
steps and zero unresolved/undispositioned rows. Then run focused compiler
commands, original 23, A1–A3 controls and System/emitter regressions. Final
Clippy/fmt and the train's existing hosted acceptance precede landing. Heavy
commands use at most two build jobs and background priority. H2.8a is not
closed by these 23 matches: its remaining path intersections, older-input
promotions, global profile and architectural close still require evidence.

## A4 amendment: literal template module requests

The first A4 comparison passes all 116 A1–A3 cases and 22 of the 24 new controls.
Both template controls fail before output because the Program request collector
omits their authoritative resolution key. The unchanged TS6 observations show
ES2015 `context.import` preserving the template argument and ES5 replacing its
lowered literal with the resolved bundle name. The error records are in
`target/h2-8a-a4-focused-after.log` (six passing tests, one failing grouped test,
148.79 seconds). No fixture bytes or original comparisons change.

| Step / gap | Upstream | Native action and lifecycle | Exact witness |
| --- | --- | --- | --- |
| A4-4 / missing | `collectExternalModuleReferences`, 124075–124120, invokes `forEachDynamicImportOrRequireCall`, 20016–20038, with the string-literal-like predicate; `isStringLiteralLike`, 12583–12585, accepts StringLiteral and NoSubstitutionTemplateLiteral. | In `program::module_requests::plan_source_requests`, use the existing private `string_literal_like_text` helper for the first dynamic-import argument. Preserve ordered occurrences, original node span, source-loading flag and computed dynamic resolution mode. The loader remains the sole host-resolution producer; the checker consumes its immutable authoritative key, with no fallback lookup or retained syntax. | Both frozen template commands, their ordinary-string/expression/parenthesized controls, and existing dynamic-import request/attribute/deferred-import contracts. |

This adds only `crates/program/src/module_requests.rs` to allowed production
files. Static imports and the already-correct require branch remain unchanged.
The existing Program contract gains literal-template coverage with an
interpolated-template negative control; all Program tests are rerun. The gate
now has eight owner spans and four steps; architecture dispositions remain the
same because the authoritative Program-to-resolver boundary is preserved.
Run amended readiness before editing the collector.

The amended implementation passes all 140 focused cases twice (seven compiler
tests, 131.97 seconds), and all 23 unchanged original cases twice (17.36 seconds).
The System observer `--check` reproduces the 24-case fixture. Program validation
passes 468 contracts with five existing ignores, 24 unit tests, the original
bundle source-fact comparison and native Memory/Fs host smoke test. The current
hosted runner executes the same 23-case assertion after the independent original
D283/E-only8 assertions, requiring disjoint sets and reporting 314 current exact
IDs with 11 later references. Its frozen 291-row qualification and original
325-row join are untouched. Hosted execution and adjacent emitter/declaration
regressions remain pending; this is not a full H2.8a close. The subsequent full
H2.7c run passes 14 of 15 tests, with its newly admitted original rootDir case
reporting one TS5055 difference. The native package resolver lacks the upstream
output-to-input mapping for local package exports/imports; the next packet owns
that shared prerequisite. The original 31 qualified H2.7c rows remain exact.
