The 100 map/source-root rows are a bounded harness projection, not a new architecture contract. `/.src` is already an explicit invariant on both the native and TS-oracle sides, and every descriptor value is a plain relative path. Test them with the other eight. The remaining risks are specific production emit behaviours that complete-command comparison will measure, not reasons to defer. This is from reading the r133 descriptor file, the native harness, the oracle scripts, and upstream `projectsRunner.ts` and baselines; I made no edits or builds.

## The `/.src` invariant already exists

**Native:**
- `harness/src/upstream_suites.rs:32` defines `pub const VIRTUAL_SOURCE_ROOT: &str = "/.src"`.
- The project mount is `/.src/tests` (`execution.rs:1688-1697`), and each project's current directory is `/.src/tests/cases/projects/<root>`.

**Upstream:**
- `vfsUtil.ts:30` has `srcFolder = "/.src"`.
- `projectsRunner.ts:207-209` mounts `tests` under `srcFolder` and changes directory to `srcFolder/projectRoot`.
- `createCompilerOptions:462-468` resolves `mapRoot`/`sourceRoot` against it when the resolve flag is set.

**Existing TS oracle:**
- `crates/oracle/h1-project-classification.mjs:~410-420` already models this with `ts.getNormalizedAbsolutePath(testCase.mapRoot, VIRTUAL_ROOT)`.
- `h2-6c-census.mjs:45-58` and `h2-6c-qualification.mjs:~912` treat `resolveMapRoot`, `resolveSourceRoot` and `emittedFiles` as structural descriptor keys, not compiler options.
- So the observation side needs no new contract.

**Descriptor values (measured over all 108):**
- `mapRoot` and `sourceRoot` are always `tests/cases/projects/<root>/{mapFiles|src}`: relative, no `..`, no drive letter, no URL scheme, no trailing separator, never empty.
- `resolveMapRoot` and `resolveSourceRoot` are always `true` when present.

For this value set, `vpath.resolve("/.src", v)` and the oracle's `getNormalizedAbsolutePath(v, "/.src")` give the same string. Each resolve flag always appears with its own root value, so the upstream `resolve && mapRoot` fallback never triggers.

**Baselines versus raw output.** Upstream baselines show `/tests/cases/...` because `Utils.removeTestPathPrefixes` strips `/.src` when writing baselines (`projectsRunner.ts:284`). Raw TS emit contains `/.src/tests/...`. Compare native output against **raw** oracle observations, which the project lane already produces, not against sanitized baselines.

## Smallest projection (harness only, `apply_project_emit_options`)

1. **`declarationDir`:** copy the string, exactly as `outDir` is copied.
2. **`emittedFiles`:** add to the structural/metadata set with `baselineCheck`/`runTest`. Upstream never treats it as an option.
3. **`resolveMapRoot` / `resolveSourceRoot`:**
   - add them to the structural set;
   - after the descriptor loop, so descriptor key order doesn't matter, set `map_root`/`source_root` to the normalized join of `VIRTUAL_SOURCE_ROOT` and the value when the flag is truthy and the value is present;
   - keep failing closed, with an explicit refusal, if a value is absolute, a URL, has a drive letter, is empty or contains `..`. None of the current 108 hits this, and it avoids guessing semantics that haven't been observed.
4. **No-emit loader:** leave its emit-option refusal as it is.

**Loader controls:**
- resolve flag true, false, or absent;
- resolve true with no root value, which must keep the upstream fallback (`mapRoot` undefined);
- key order reversed;
- refusal of absolute, URL and `..` values.

## Production emit behaviours to measure, not assume

- **CommonJS with `outFile`.** For the `…SpecifyOutputFile/node` variants, TS reports **TS6082** ("Only 'amd' and 'system' modules are supported alongside --outFile") together with TS5101 and TS5107, **and still emits `bin/`**. The native AMD/System bundle path doesn't show whether native emits the same output for CommonJS-with-`outFile`, or refuses. This is the most likely real difference.
- **`outFile` with `outDir`** (the `…SpecifyOutputFileAndOutputDirectory` rows, 4 of them): output-path selection when both options are set.
- **Rooted map roots.**
  - The absolute `sourceMappingURL` is formed as `encodeURI(combinePaths(mapRoot, file))`. Upstream output after stripping `/.src` is `//# sourceMappingURL=/tests/cases/projects/outputdir_simple/mapFiles/test.js.map`.
  - `"sourceRoot"` gains a trailing `/`, e.g. `".../src/"`.
  - Confirm whether the existing h2-6a map-option witnesses cover rooted values as well as relative ones.
- **Declarations in bundles:** `declaration:true` combined with AMD `outFile`.

## Evidence path (no census rerun)

1. **TS side:** use the existing project-lane oracle to observe raw complete commands for the 108 rows (two module variants each), twice: every write (JS, d.ts, maps), diagnostics, `emit_result` and exit code. Pin the observer, TS library and descriptor hashes as the other lanes do.
2. **Native side:** run the patched harness loader followed by the existing project complete-command executor. Compare per row. A loader pass alone doesn't count.
3. **Records:**
   - keep the census's 110 failures as immutable historical evidence;
   - write a separate supplemental record listing, per row, exact match, mismatch with cause, or explicit refusal;
   - cover the two `noEmit` rows by the parse-tuple-equality plus no-emit command route from round 125.

Any row that fails because of a real production limit, most likely CommonJS-with-`outFile`, becomes a named known item tied to that specific cause, not to the harness projection.
