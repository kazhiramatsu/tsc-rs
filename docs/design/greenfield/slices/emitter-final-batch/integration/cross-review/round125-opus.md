The 108 project failures are refused before any TypeScript source is parsed or any Program is loaded, so the variable-type admission change cannot reach them. They are harness gaps in reading project descriptors, not production emitter limits. The two `noEmit` failures are an emit-only loader applied to a no-emit route. Two small, faithful harness additions exist (`declarationDir`, `emittedFiles`); `resolveMapRoot`/`resolveSourceRoot` need a separate contract. Evidence is from reading Census 67df86615, the vendored `projectsRunner.ts`, and only the `load_failures` array extracted from the snapshot; I made no edits or builds.

## 1. The 108 project failures

**Where they fail.** The census tries `load_project_emit` and then `load_project_no_emit` (`xtask/src/utf16_literal_recovery_census.rs:494-503`).

**Emit loader** (`harness/.../execution/project.rs:230-360`), in order:
1. `MountedProjectHost::new`;
2. root selection, which for config roots only parses the config JSON (`parse_project_config`);
3. `apply_project_emit_options`, which returns `Err` with `project descriptor contains unsupported property …` at `:444`;
4. only after that, `load_emitting_program_js`, the first step that parses TS sources.

**No-emit loader** (`:65-218`) has the same order: `apply_project_runner_existing_options` (`:568-616`) returns `Err` with `requires declaration=false` (`:591`) or `requests unsupported emit option` (`:606`) before `load_program_js`.

Both errors are recorded in `load_failures` with `emit_disposition: "not-loaded; emit-not-qualified"`. So no program facts exist for these 108 IDs. They are unreachable for any parser-admission predicate, and their unchanged exact refusal is the right replay disposition.

**Harness gap or production limit?** Upstream `createCompilerOptions` (`projectsRunner.ts:454-491`) copies every descriptor key that is a real compiler option, and special-cases `mapRoot`/`sourceRoot`. Keys that aren't options are ignored.

| Property | Rows | Upstream meaning | Native status |
|---|---|---|---|
| `declarationDir` | 6 (`declarationDir{,2,3}` × CommonJS/AMD) | ordinary compiler option, copied as is | production plans it (`emitter/src/plan.rs`, `execute.rs`); only the harness projection is missing |
| `emittedFiles` | 2 (`declarationsExportNamespace` × 2) | not in `optionDeclarations`; the runner ignores it; it's output resolution-info metadata | harness rejects it as unknown |
| `resolveMapRoot` / `resolveSourceRoot` | 50 + 50 | runner-only flags: `mapRoot`/`sourceRoot` become `vpath.resolve(vfs.srcFolder, …)`, with `srcFolder = "/.src"` | needs the harness's virtual-root contract |

## 2. The two `noEmit` qualified inputs

**Case IDs:**
- `typescript-6.0.3/conformance/moduleResolution/bundler/bundlerImportTsExtensions.ts#allowimportingtsextensions%3Dfalse%2Cnoemit%3Dtrue`
- `…#allowimportingtsextensions%3Dtrue%2Cnoemit%3Dtrue`

**Why they fail.** Both come from `ratchets/h2-8a-candidate-inputs.v1.json`. The census loads every qualified or candidate case with the emitting loader `load_qualified_compiler_emit_with_symlinks` (`census.rs:~580-617`). `program/src/loader.rs:989` refuses `noEmit=true`.

**Intended route.** It's a no-emit command: TS emits nothing and only diagnostics apply. The emitter preflight, and therefore any admission predicate, is never reached on that route.

**Parse coverage.** In the snapshot, each of the four variant IDs appears exactly once. The two `noemit=false` siblings are loaded rows; the two `noemit=true` IDs appear only as load failures. `noEmit` and `allowImportingTsExtensions` don't affect parsing, so the `noemit=true` parse inputs are almost certainly identical to their siblings'.

**Minimal supplemental proof, without re-running the full census:**
1. From the candidate-input artifact and the sibling rows' parse units, show that each `noemit=true` case has the same (file name, text, `ParseOptions`) tuples as its `noemit=false` sibling. That makes parse coverage explicit.
2. Cite, or run, the no-emit command witness for these two IDs: the no-emit loader route or diagnostic conformance for this file.

The replay should record them as "no-emit route; emit admission unreachable; parse inputs covered via sibling".

## 3. Which gaps are small

- **`declarationDir`: small and safe.** Add a projection mirroring the existing `outDir`/`outFile` handling in `apply_project_emit_options` (`options.declaration_dir = string value`). Upstream copies it as is, relative to the project current directory, the same way as `outDir`. It then needs those six rows loaded and their complete commands compared, not only the loader to pass.
- **`emittedFiles`: small, but only as metadata.** This isn't ignoring a field to get past the loader: upstream never treats it as an option (`optionNameMap.get(name)` is undefined). Its correct home is the metadata set with `baselineCheck`/`runTest`. The two rows still need their declaration-emitting commands compared against TS output.
- **`resolveMapRoot` / `resolveSourceRoot`: not a small projection.**
  - They change emitted `sourceMappingURL` and `sourceRoot` values to absolute paths under the runner's virtual file-system root (`/.src/…`).
  - Faithful support needs an owner contract proving that the native `MountedProjectHost`'s virtual current directory and mount paths match `vfs.srcFolder`.
  - It also needs baseline-level comparison of the absolute URLs across NoOutdir, OutputDirectory and OutputFile, including the OutputFile-plus-OutputDirectory variants.
  - Several rows also combine `outFile` with AMD, which brings bundle-emit scope.
  - Keep these 100 as explicit refusals in their own later project-harness contract, not in this train's recovery work.
