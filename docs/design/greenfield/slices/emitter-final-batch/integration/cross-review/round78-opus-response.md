Replaying from `command_input` alone is feasible for all four routes, but not by reusing the existing TS builders unchanged. Every existing builder either looks rows up by case id or disagrees with the Rust loaders on some of the details you listed. The lowest-risk design: native Rust decides the program layout and the per-directive option decisions, and hands them to a thin TS executor. That executor builds options with TypeScript's own parsers and the existing observer. This review was read-only; I could not check the pinned upstream harness itself, because `ts-tests` contains only `tests/cases`.

## 1. Reuse inventory

| Helper | Exported? | Safe to reuse here? |
|---|---|---|
| Rust `load_compiler_emit` / `_no_emit` / `_with_option_floor` | yes | Yes. They are the recorded loaders and the authority for the program. |
| Rust `load_project_emit` / `_no_emit` / `_with_option_floor` | yes | Yes. |
| Rust `load_qualified_compiler_emit_with_symlinks` | yes | Yes. The census used it with the Established floor (`utf16_literal_recovery_census.rs:864/917`). |
| JS `createHermeticDirectoryOverlay` (`vfs-directory-overlay.mjs`) | yes | Yes. |
| JS `parseConfig` (`h2-8a-candidates.mjs`) | yes | Only for configs without errors: it asserts that `parseDiagnostics` and `errors` are empty. Wrap it or use a copy that returns the errors. |
| JS `directiveInput`, `projectInput` (h2-8a), `explicitRootSelection` / `createProgramCase` (h2-7b) | no | **No.** They look rows up by case id and diverge from the Rust loaders (section 2). |
| JS `observe`, `write`, `diagnostic`, `sourceMaps` (`observe-emitter-final-universe.mjs`) | no; verbatim copies | Yes, as the observer. Copy them into a new module and prove the copy by re-observing frozen universe rows (section 4). Don't edit the pinned generator: that would make its recorded identity stale. |

## 2. Where the existing JS builders disagree with the Rust loaders

1. **Missing vs empty content.** Rust skips a unit whose `content` is None (`execution.rs:459`), so no file is created, but the unit can still be named as a root. h2-8a's `content || ""` turns it into an empty file. h2-7b throws. Required rule: `content_sha256: null` means write no file; the hash of `""` means write an empty file.
2. **Harness defaults.** Rust uses `skip_default_lib_check.get_or_insert(true)` before the directives and `new_line.get_or_insert(CRLF)` after them, so a config's value wins; `no_error_truncation` is forced to true. h2-8a's `Object.assign(...)` overwrites the config's `newLine` and `skipDefaultLibCheck`. Mirror the Rust order. Also count the rows where a config sets either option, because I couldn't confirm the upstream harness behaviour.
3. **Established floor, recorded-compiler route.** Directives for the following options are skipped, not deleted:
   - `sourceMap`, `inlineSourceMap`, `inlineSources`, `sourceRoot`, `mapRoot`;
   - `emitBOM`, `emitDeclarationOnly`, `declarationMap`, `outFile`, `outDir`, `noEmitHelpers`;
   - on every floor: `declarationDir`, `rootDir`, `noCheck`, `stripInternal`, `incremental`, `tsBuildInfoFile`, `stableTypeOrdering`.

   Options coming from a virtual config are **not** floored on this route (only `SourceMapWithOptions` floors them). So a config's `sourceMap`/`outDir` survives, and a skipped directive must not clear it.
   - **Consequence:** these rows are an Established-floor command, not the upstream baseline command. Rows with a skipped directive need their own label; do not call them the upstream command.
4. **Qualified/candidate routes.** Rust forces a case-sensitive host (`execution.rs:200`) and ignores the virtual config on the Established floor. The config stays in the VFS for identity only, which is the H2.5g splice rule. The TS consumer must do both. It must also copy the order: default `skipDefaultLibCheck` first, then the settings in their recorded order, then default `newLine`.
5. **Symlinks.** Rust walks global links, then each unit's document links in **unit order** (not write order), and repeats until nothing new is added, so links into linked directories resolve. h2-7b does one pass and only adds a document link if the path is free. h2-8a matches links against files only and lets the last one win. Neither JS version matches Rust.
6. **Project module variant.** Rust applies the `module` value from the descriptor after the variant, so the descriptor wins (`project.rs` `apply_project_emit_options`). h2-8a's `projectInput` sets the variant afterwards, so the variant wins. They diverge whenever `descriptor_module_override` is not null.
   - **Project floor:** Rust applies the descriptor's `sourceMap`, `sourceRoot`, `mapRoot`, `outDir`, `outFile` and `rootDir` on every floor. It rejects `resolveMapRoot`, `resolveSourceRoot`, `declarationDir` and `emittedFiles`, which ends in a double failure with an explicit disposition. It uses `lib.es5.d.ts` as the default library, Classic resolution, `noErrorTruncation` and `skipDefaultLibCheck` false, and CRLF.
7. **NoEmit fallbacks use different option layers.** `load_compiler_no_emit` forces `noEmit: true`. `load_project_no_emit` builds a different option layer, rejecting emit options and not applying them. The TS command for such a row must copy that layer. Its result is only diagnostics, status and exit code: parse-admission only.
8. **Debug strings.** `root_selection_debug` is Rust `Debug` text, so never parse it.
   - **Compiler route:** use the structured `vfs_write_order`, `program_root_units` and `config_unit`.
   - **Project route:** recompute roots in TS (explicit `inputFiles`, or `parsed.fileNames` from the config) and assert them equal to the captured `prepared.roots`.
   - **Project module variant:** `module_variant` is also a Debug name; map it with an exhaustive table (`Commonjs` → 1, `Amd` → 2).

## 3. Required changes (harness and consumers only; census stays frozen)

**A. Factor the layout out of the loaders.**
- Extract `pub fn compiler_vfs_layout(plan) -> Layout` from `load_compiler_program`. It returns the files in write order (skipping None), the alias list in insertion order as link → physical after the fixpoint, the directory aliases, and the root paths.
- `load_compiler_program` itself then calls it, so nothing is duplicated.
- Do the same for the qualified loader, since its symlink and directory-alias building has the same shape.
- The native consumer serializes the layout into a `ts_command` for the TS executor, and the JS side never re-derives write order, links or roots.

**B. Make the directive decision a returned value.**
- Change `apply_compiler_setting` to return `Applied | FloorDropped | Ignored | Metadata` for each `(name, floor)`, and expose that decision.
- The native consumer emits the per-setting decisions in order.
- TS applies only the `Applied` entries, in that order, through `ts.optionDeclarations`, `parseListTypeOption` and `parseCustomTypeOption`.
- That keeps option parsing with TypeScript and keeps the floor decision with Rust. No JS drop list can drift from the Rust one.

**C. Config diagnostics.** Rust uses `with_program_owned_config_option_diagnostics`, so the program reports config errors. In TS:
- Keep `configFile` in the options passed to `createProgram`. `observe()` already spreads `parsed.options`; `serialOptions` must not be used for the actual command.
- Don't assert that `parsed.errors` is empty; record them.
- Compare against the native diagnostics, so a config with errors becomes a comparable row instead of a crash.
- Parse the config with `plan.use_case_sensitive_file_names`. h2-8a hardcodes `false` for compiler rows.

**D. Raw UTF-16 in the observation.**
- `write()` encodes with `Buffer.from(text, "utf8")`, which silently turns lone surrogates into U+FFFD.
- Add `callback_utf16le_sha256` and the code-unit length. Base64 as well if you want diffs you can decode.
- Compare these against the native UTF-16 code units; keep the UTF-8 bytes including the BOM.

**E. Source decoding.**
- Decode pool bytes with a strict UTF-8 decoder that keeps the BOM: `new TextDecoder("utf-8", { fatal: true, ignoreBOM: true })`. The default `TextDecoder` strips a leading U+FEFF.
- Then assert that each non-library `program.getSourceFile(p).text` hashes to the captured `prepared.source_files[].sha256`.
- Compare library files by basename plus hash, because the two sides mount them at different absolute paths.
- This check catches any BOM or decoding mismatch directly, because the Rust summary hashes the snapshot text.

## 4. Proving the copied TS executor

- **Self-check:** before the corpus run, re-observe a sample of the frozen `emitter-final-universe(-plan-base)` rows through the copied observer. `deepEqual` the existing fields; the new UTF-16 fields are additions. EF7 used the same kind of builder self-check.
- **Per row:** run each command twice with a fresh host and program, and assert the two runs are identical. Assert that no workspace path appears in any output.
- **NoEmit rows:** they go through the same observer with `noEmit` set. Keep them in a separate tally and never count them toward emit qualification.
- **Double failures:** their JSON is kept, but they are never executed.
