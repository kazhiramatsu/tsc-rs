Approve the loader projection and the Node mirror for the 108-row supplemental run, with two small fail-closed additions and a few extra controls. For the probe, reuse the native runner's existing `prepare`/`complete` steps and `plan_input`, but give it its own entry point and selection kind rather than loosening the census selection. This is from reading only the four files in your diff, plus upstream `core.ts`/`commandLineParser.ts` and the xtask native runner; I made no edits or builds.

## `project.rs`

**Descriptor versus config precedence**
- **Where descriptor values win:**
  - Config roots load `compiler_options` first, then `apply_project_emit_options` writes descriptor values over them.
  - Upstream agrees: `extend(existingOptions, parsed.options)` (`commandLineParser.ts:3077`) copies `first` (the runner and descriptor options) last, so they win.
  - Resolution runs after the loop and only for roots that are present in the descriptor, so it doesn't depend on key order.
  - A root that exists only in the config is never resolved by a descriptor flag. That's right, because upstream only reads `testCase.mapRoot`.
- **Latent difference, outside these 108.** Upstream's `createCompilerOptions` always includes `mapRoot` and `sourceRoot` keys, which may be `undefined`. `extend` uses `hasOwnProperty`, so on config-root projects a descriptor with no `mapRoot` still overrides a tsconfig `mapRoot`/`sourceRoot` with `undefined`. Native keeps the config value.
  - No `tests/cases/projects` config contains `mapRoot` or `sourceRoot` (grep found none).
  - All 108 use explicit `inputFiles` roots.
  - So there's no effect now. Record it as a precedence note for config-root projects rather than changing it in this candidate.

**Scalar paths (two concrete additions)**
1. **Non-string root with the resolve flag set.**
   - `property.value.as_str().map(...)` turns a non-string `mapRoot`/`sourceRoot` into `None`.
   - The resolver then silently skips it, even though the resolve flag is `true`.
   - Upstream would try to resolve any truthy value.
   - Fail closed instead: when `resolve && descriptor_*_root` holds and the value is not a string, return an error.
2. **Non-string `declarationDir`.** This is new code using the same silent-`None` pattern. Return an error for non-string values instead of dropping them.

**Resolve flags.** They use `property_bool`, so a non-boolean flag is an error. That's stricter than upstream's truthiness test, but it fails closed, which is fine.

**The bounded resolve domain is a trustworthy refusal.**
- It rejects `:`, `\`, NUL, and empty, `.` or `..` segments. That covers absolute paths, trailing separators, `//`, URL schemes and drive letters.
- A value outside the domain produces a loader error, which is recorded as a refusal, not a silent misemit.
- In-domain values give `normalize_virtual_path("/.src", p)`, the same result as upstream `vpath.resolve` for plain relative paths.
- All 108 descriptor values are in the domain.

**No-emit loader.** It still refuses a truthy `resolve*`, `declarationDir` or `emittedFiles`. It's untouched, and the test confirms this.

## Node mirror (`recovery-command-input.mjs`)

- It follows upstream truthiness and resolves with `absolute(v, "/.src")` after the loop, with the same fallbacks: resolve true with no root gives `undefined`, and resolve true with an empty root gives `""`.
- Its domain is wider than native's, which is correct on the TS side. Native refusals will appear as exact refusal-versus-emit differences, never as false matches.
- `emittedFiles` and the resolve flags are skipped only on the emit path. The no-emit path keeps its asserts.

## Controls to add (small)

- **Native refusals:**
  - `"./maps"` and `"a//b"`, i.e. the dot and empty-segment branches;
  - a non-string root with resolve `true`, which should error after addition 1;
  - a non-boolean resolve flag;
  - a non-string `declarationDir`, which should error after addition 2.
- **Config-root precedence:** one config-root plan where the descriptor's `mapRoot` plus resolve flag overrides a config value, if any existing fixture can be adapted that way.
- **Mirror:** a case where resolve is set but the root is supplied only by the config, showing it isn't resolved.

## Reusing the native probe without weakening census pins

`crates/xtask/src/recovery_corpus_native.rs` couples its `run()` to `validate_selection` and `validate_data_workspace`, which check the census head, syntax tree, digest code and input manifest. Don't loosen those. Instead:

1. **New entry point** (for example `run_project_supplement`) with its own artifact `kind`, such as `emitter-project-projection-selection`, and its own pins:
   - the r133 descriptor file SHA;
   - the fixed list of 108 case IDs;
   - the plan manifest SHA;
   - the harness `project.rs` SHA;
   - the TS library SHA.
2. **Reuse, not copy,** the existing `prepare`, `complete`/`observe`, `captured_write` and `diagnostics` functions, plus `observable_input::plan_input` with the same document pool. Load each row through `load_recorded_execution_plans`, select it by ID, then call `load_project_emit`.
3. **Fingerprints on both sides.** Record the exact prepared roots, document hashes and `compiler_options_debug_sha256`/`program_options_debug_sha256`. Build the TS side from the same `plan_input` through `recovery-command-input.mjs`, and require the existing `same_input` equality before comparing outputs.
4. **Comparison.** Compare every write, diagnostic, `emit_result` and exit code, twice on each side.
5. **Records.** Keep the census's 110 failures and the selection artifact unchanged. The result is a separate supplemental record with per-row exact match, mismatch plus cause, or refusal. Loader success alone never counts as emit qualification.
