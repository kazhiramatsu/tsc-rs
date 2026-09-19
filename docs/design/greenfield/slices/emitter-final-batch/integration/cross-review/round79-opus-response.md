The TS-only mirror is acceptable. I found no case where a mistake in the mirror could produce a false "exact" result, so pins plus adversarial tests are enough and loader extraction isn't required. The input-workspace scheme is also sound, but only after five changes: the census HEAD check, cleanliness of the files actually read, the canonical library path, the TS library location, and splitting code vs data workspaces in `recovery_corpus_native.rs`. The native file has a few further issues, listed in section 3. This review was read-only: no builds, tests or edits.

## 1. TS-only mirror vs extracting the loader logic

A mirror error on the TS side can only show up as a native-vs-TS difference; it can't make both sides agree wrongly:
- A layout mistake (write order, missing vs empty content, links, directory aliases, case handling, roots) changes which files are loaded or their realpaths, or changes module resolution. Each row's source-path + hash + roots check against the captured `prepared` catches that directly.
- An option mistake changes the output or the diagnostics, so it appears as a visible difference.
- Where Rust's Established projection itself differs from upstream, the mirror copies that on purpose, and your "Established projection, not the upstream baseline" label covers it.

**The real remaining risk is misattribution, not unsoundness.** If the mirror mis-projects an option, the result looks like an emitter bug, and someone could "fix" the emitter to match a wrong TS command. Two ways to close this without touching the loaders:
- **Recommended:** an option-parity check inside the consumers. The native consumer writes an explicit table of emit- and check-relevant fields from `program.compiler_options()`, using TS option names. The TS side writes the same fields from `program.getCompilerOptions()`. Any mismatch becomes an `input-mismatch` disposition instead of an emitter row. Fields to include:
  - target, module, moduleResolution, newLine
  - the source-map family, outDir/outFile/rootDir
  - declaration, declarationMap, emitDeclarationOnly, emitBOM
  - noEmit, noEmitOnError, noEmitHelpers, importHelpers, removeComments
  - the jsx options, esModuleInterop, useDefineForClassFields
  - experimentalDecorators, emitDecoratorMetadata
  - preserveConstEnums, isolatedModules, verbatimModuleSyntax, downlevelIteration
  - skipDefaultLibCheck, noErrorTruncation, allowJs, checkJs, the strict family, lib
- **Required:** tests for these adversarial branches of the mirror:
  - a None-content unit that is a root, vs `""` content;
  - a config that sets `newLine` or `skipDefaultLibCheck` (Rust fills defaults only when absent);
  - config `sourceMap: true` plus directive `sourceMap: false` (the directive is skipped, so the config value stays);
  - a directory link into a linked directory (Rust repeats until nothing new is added);
  - a document link to a path that is already taken;
  - a project descriptor `module` that overrides the variant;
  - a qualified row with a virtual config that must not be applied, and its forced case-sensitive host;
  - a case-insensitive compiler row;
  - a NoEmit-loader row, where `noEmit` is forced and the project NoEmit option layer is different.

Pin `execution.rs`, `project.rs` and `vfs-directory-overlay.mjs` by SHA so that any change to them forces the mirror to be reviewed again.

## 2. Input-workspace scheme

Sound in principle, with these fixes:
1. **HEAD equality will break.** Once the census commits its results, the census tree's HEAD is no longer `census_head`. Instead:
   - require `git merge-base --is-ancestor census_head HEAD`;
   - require `git diff --quiet census_head HEAD -- <data paths>`, where the data paths are `vendor/typescript-6.0.3`, `ts-tests`, the recorded manifest, and the `input_manifest` artifacts;
   - require working-tree cleanliness for those same paths: no diff against HEAD and no untracked files there.

   `rev-parse HEAD:vendor/...` checks only the committed tree, not the files actually being read. Run the check at start and again at end.
2. **Canonical library path.** `CompilerSuiteHost` canonicalizes the library directory (`fs::canonicalize`), and the project host uses `normalize_existing_directory`. The TS side must use `fs.realpathSync(<input>/vendor/typescript-6.0.3/lib)` for its library-path check and library root. Otherwise the paths differ even though the bytes are the same.
3. **TS library location.** Load `typescript.js` from either tree, but assert its bytes equal the input tree's copy. Then override both `getDefaultLibLocation` and `getDefaultLibFileName` to point at the input library root. The EF7 observer overrides only `getDefaultLibFileName`, and only for projects; resolving the `lib` option would still fall back to the loaded script's directory.
4. **Local paths in the output.** Outputs will contain the census tree's absolute path, so they are local evidence only. Don't reuse the `persist()` / "no workspace path" assertions unchanged. Assert instead that the replay tree's own path never appears.
5. **Split `workspace` in `recovery_corpus_native.rs`.**
   - **Code workspace:** `clean`, the observer's `include_bytes` self-check, `syntax_tree_hash` and the digest-code check.
   - **Input workspace:** the plan manifest, `input_manifest`, `load_recorded_execution_plans`, all `load_*` calls and `qualified_input`/`candidate_input`. The last two derive the library directory from the workspace they are given.
   - **Both:** check `vendor_tree_hash` in both trees and require them equal.

**Parser identity.** `syntax_tree_hash` must stay equal to the replay binary's `crates/syntax`. "Latest native binary" is fine only if it doesn't change the syntax crate. If the syntax crate differs, the selection no longer describes that parser; that needs a per-row re-digest on the replay parser, with a "parse drift" disposition when the digest differs.

## 3. Review of `recovery_corpus_native.rs`

**Correct as written:**
- **Plan index:** keys use no case id and include the configuration index and the scenario. Duplicate keys fail. The case id is checked only after the lookup.
- **Document pool:** pool hashes are verified, references are verified on reload, and null content is kept distinct from empty content.
- **Input equality:** the whole serialized input is compared byte for byte after the Debug-hash check, so the error message points at the right cause.
- **Qualified/candidate routes:** the loader, route/universe and case sensitivity are all checked. The embedded input is used directly, and candidates without a shared mount are handled.
- **Repeat runs:** each run gets a fresh reload.
- **Callbacks:** `callback_units()` gives raw UTF-16, alongside the callback bytes and the materialized bytes. `string_value` keeps UTF-16, so the lossy UTF-8 copy next to it is harmless.
- **Disposition guards:** checked in both directions. Production errors are counted as refusals that don't qualify.
- **Output:** created exclusively, and the HEAD is re-checked at the end.

**Issues:**
- **NoEmit fallback rows are loaded and verified but never executed.** That is consistent with "parse-admission only". But recovery changes affect parse diagnostics, so run the command twice anyway: the loader has already forced `noEmit`. Record diagnostics, status and exit code, still labelled `parse-admission-only; emit-not-qualified`. The TS side must then also run with `noEmit: true`, using the same NoEmit option layer.
- **Diagnostic category.** `category` is written as a `u8`, while the EF7 TS observer writes the `DiagnosticCategory` name. Pick one encoding and state it in the consumer contract.
- **Message chain flattening** (newline + two-space indent per level) matches `flattenDiagnosticMessageText`. Diagnostic file names can carry the absolute library path, which is consistent under the input-workspace scheme. Do **not** apply EF7's `/lib/` projection on the TS side.
- **Build-info metadata** is an error for the whole run, not a per-row disposition. One row with build-info would abort the corpus. Make it a per-row `observer-unsupported` disposition.
- **`validate_selection`:** besides the known pending whitelist:
  - Also check each input's `input_manifest` pin against the input workspace.
  - Require that every qualified or candidate row's `universe` appears in `input_manifest`, so a row can't cite an artifact that was never pinned.
