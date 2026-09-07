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

The existing Rust checker already projects
`get_external_module_file_from_declaration` through `EmitResolver`. Current
AMD `ModuleTransformer::external_module_name_literal` consults that method
only when some source has an explicit module name, then selects only that
explicit name. Its wrapper-name branch also selects only `module_name`.
System similarly selects only an explicit wrapper name, while dependency
groups currently use the original literal text. Thus outFile support needs
the shared resolved-file/path-name decision in both transforms, and System
grouping must occur after the decision. An unavailable required resolver
answer must retain a typed failure instead of silently selecting a fallback.
The declaration-bundle visitor must keep its distinct `.d.ts` exclusion;
the JavaScript helper cannot be reused as an identical decision.

Reproduction: `node scripts/observe-bundle-module-identities.mjs --check`.
Both fresh-Program repetitions and a complete second observer run match the
fixture. These observations supply the next implementation packet; bundle
transform, printer, resolver and complete Rust byte comparisons remain owned
by the implementation and original-corpus acceptance work.
