# H2.7d bundle module-identity references

Status: repeated TypeScript 6.0.3 observations, 2026-09-07. This packet
changes no Rust behavior or admission. The accepted source is commit
`050880ce59e30b356b686bd3144efe24f875ebc8`.

`scripts/observe-bundle-module-identities.mjs` records 24 complete
`Program.emit` tuples, each on two fresh Programs: 20 ordinary source/option
controls and four separate `renamedDependencies` host-API references. The
same artifact records 14 direct path-helper controls, each twice. These
focused denominators do not enlarge the original 315-row bundle census.
The output is `crates/emitter/tests/fixtures/bundle-module-identities.json`.

Every Program tuple retains all pre-emit diagnostics, source order, common
source directory, internal module-name/resolved-file observations, callback
bytes/BOM/source metadata/order and the complete EmitResult. The reference
deliberately keeps deprecation diagnostics 5101/5107 and unresolved-module
2307. No options are removed to suppress them. It does not execute a CLI or
claim command status/exit equivalence. The library mount is restricted to
the pinned `/lib` files. The compiler, observer, directory overlay and Node
version identities are recorded.

The source seams in `vendor/typescript-6.0.3/lib/_tsc.js` are
`getResolvedExternalModuleName` / `getExternalModuleNameFromPath`
(16535–16566), `getExternalModuleNameLiteral` / `tryGetModuleNameFromFile`
(27713–27738), AMD `collectAsynchronousDependencies` (110442 onward), and
System `collectDependencyGroups` (112159 onward). The checked observations
show:

| Input | Required result |
| --- | --- |
| Relative import resolving to `src/dep.ts`, common directory `src/` | Both wrapper name and dependency literal use `dep`. |
| Nested reexport from `src/app/main.ts` to `src/lib/dep.ts` | Wrapper names are `app/main` and `lib/dep`; dependency is `lib/dep`. |
| Nonempty `amd-module` directive | Its name overrides the derived path for AMD and System. |
| Empty `amd-module` name | The empty string is retained as a source fact but falls back to the derived name. |
| Resolved `.d.ts` package without explicit name | Dependency retains its original package literal; no emitted module name. |
| Resolved `.d.ts` package with explicit name | JavaScript dependency uses that explicit name; the declaration helper still returns absent. |
| Unresolved import | Original literal remains, with TS2307. |
| Resolved source plus `renamedDependencies` entry | Resolved source identity wins over the rename. |
| Unresolved source plus rename entry | The rename becomes the JavaScript dependency; TS2307 remains. |
| Two specifiers resolving to the same source | AMD keeps two dependency entries; System groups them after rewriting and assigns both locals in one setter. |

Import-equals and path-alias resolution use the same resolved-file helper.
The path controls preserve the file's spelling while comparing canonical
paths. `.ts`, `.tsx`, `.mts`, `.cts`, declaration extensions, `.js` and `.json`
are removed by the pinned helper; uppercase `.TS` remains. A reference file
adds `./` when needed, while the bundle's own common-directory name has no
such prefix. The case-insensitive control is a direct helper reference, not
admission of the later host-casing owner.

The candidate implementation uses the existing checked
`get_external_module_file_from_declaration` projection. AMD/UMD and System
share the explicit-name/path-name worker for wrappers and dependencies. System
rewrites dependency names before grouping; AMD retains each dependency entry.
Required outFile resolver answers propagate typed errors, and a detached
transformer request without an emit host refuses outFile. The declaration
visitor can use `get_resolved_external_module_name(host, file, reference_file)`
while retaining its separate declaration-file and bare-import decisions.

The 14 path helper observations and source identities from all 24 reference
cases match twice, including empty explicit names and declaration files. These
checks compare the source-identity facets only; the four renamedDependencies
API references do not acquire a Rust API implementation. The hostless boundary
control also passes (three tests), and all 95 existing builtin transformer unit
controls pass. Emitter all-target Clippy also passes with warnings denied;
the complete TypeScript observer recheck, formatting and diff checks pass.
Full bundle JavaScript bytes, merged System setters, declaration
wrappers and complete original-corpus tuples still require the shared-writer and
declaration visitor integration. The production outFile guard remains in place.

Reproduction: `node scripts/observe-bundle-module-identities.mjs --check`.
Both fresh-Program repetitions and a complete second observer run match the
fixture. The whole-Program tuple expectations remain unchanged; helper equality and
existing transform regressions do not replace their eventual Rust comparison.
