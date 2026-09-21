One issue blocks the full selected-corpus run: the new `configFile` parity field will flag every project-config row as an input mismatch. A second issue drops JSON syntax diagnostics from the TS config errors. With those two fixed, I approve the consumers for the full run, with the limits listed at the end. This was read-only: the working diff at `8f63a3651` plus the Rust program config code.

## Blocking before the full run

**B1. `configFile` parity: every project-config row becomes a false `input-option-mismatch`.**
- **Native side:**
  - `parse_config_root_plan` always attaches the config file. The option builder (`config.rs:4976-4979`) does `ProgramOptions::default().with_config_file(config_file).with_external_config_option_diagnostics()`.
  - The project loaders keep it. `ProjectConfig` and `DiscoverConfig` only re-set the config path and the external-diagnostics flag.
  - So the native snapshot's `configFile` is `{file, sha256}`.
- **TS side:** `projectOptions` deletes `options.configFile`, which is correct for program behaviour (option diagnostics stay global). But `optionSnapshot` then reads `options.configFile` and reports `null`.
- **Fix:** keep the program behaviour, but make the snapshot describe the parsed config source. Carry `parsed.options.configFile` out of `projectOptions`, for example as `configFileIdentity: {file, sha256}` captured *before* the delete, and snapshot that instead of `options.configFile`. On the compiler route both sides keep the config file and the external flag is false (Rust calls `with_program_owned_config_option_diagnostics`, which sets it false), so nothing changes there.

**B2. The TS config errors are missing the JSON syntax diagnostics.**
- **Native side:** the program's config-parsing diagnostics are `root_parse_diagnostics` followed by `errors` (`config.rs:1790-1808`; also `:994-1005`). That is the same composition as upstream `getConfigFileParsingDiagnostics`: `[...configFile.parseDiagnostics, ...errors]`.
- **TS side:** both the compiler-route `compilerConfig` and the new project path pass only `parsed.errors`. `parseJsonSourceFileConfigFileContent` does not include the source file's `parseDiagnostics`.
  - The old project path used `parseConfigFileTextToJson` and prepended `read.error` itself; that was dropped in this change.
- **Effect:** any config with a JSON syntax error mismatches in `configParsingDiagnostics`. It is blocked rather than silent, but it is a false blocker, and the TS program also reports fewer diagnostics.
- **Fix:** on both routes, set `errors = [...source.parseDiagnostics, ...parsed.errors]` and pass it both as `configFileParsingDiagnostics` and to the snapshot.

## Medium

**M-a. Float formatting in the input hash can abort the whole comparison.** `canonicalInputText` uses `JSON.stringify`, which writes `1.0` as `1`, while serde and Python write `1.0`; their exponent forms also differ. I found no floats in the captured inputs today; every numeric field is an integer. But the comparator asserts the input hash with a bare `assert`, so a single float would stop every row.
- **Smallest fix:** have the selector (or both observers) assert that every number in `command_input` is a safe integer. Alternatively, make the comparator report a hash mismatch as a per-row blocking disposition.

**M-b. A TS repetition mismatch is labelled as an input error.** `observeSelectedRow` catches the `first`/`second` `deepEqual` failure as `input-reconstruction-mismatch`. The row still blocks correctly, but the label misattributes the cause, which matters for triage. Give it a separate disposition, e.g. `typescript-repetition-mismatch`.

## Checked and correct

- **H1 fix:**
  - native `canonical_input` sorts keys in byte order (Rust `String` ordering is UTF-8 scalar order);
  - JS sorts keys with `Buffer.compare`;
  - Python uses `sort_keys` (code-point order).
  - All three agree, and serde, Python and `JSON.stringify` escape strings the same way for this data.
- **M1:** `types` and `customConditions` are split, trimmed and emptied exactly as Rust does. `lib` is trimmed, lowercased and emptied before the element map; an unknown `lib` now fails the row instead of the whole run.
- **M3:** failures are now per row. The comparator checks the input hash first, then blocks the row without requiring two runs, and continues with the rest.
- **M6, BOM handling** is consistent with the Rust decoder:
  - compiler, qualified and candidate pool documents plus library files: one leading U+FEFF stripped;
  - project documents: left alone, because `decode_source` already stripped one;
  - double-BOM case: one U+FEFF remains, matching Rust;
  - raw bytes stay in the pool and are decoded only when referenced, so a non-UTF-8 raw descriptor no longer aborts the run.
- **M4:** the source-file parse plus deleting `configFile` gives located conversion diagnostics and global program option diagnostics, which matches the native `external_config_option_diagnostics = true` for project configs. The JS flag, `parsed.options.configFile !== undefined`, is true exactly on the two config arms. The code comment on this path is accurate as written.
- **M2, the added fields:**
  - `paths` entries and `pathsBasePath`;
  - the effective default library name, via `default_library_file_name().unwrap_or(catalog default)` against the TS host's basename;
  - the config-diagnostic tuples use the same serializer on both sides;
  - the Rust accessors all exist: `PreparedProgram::diagnostics().config()`, `PathMapping::pattern/substitutions`, `paths_base_path`, `default_file_name`, `ProgramConfigFile::diagnostic_file_name/text`;
  - `super::{diagnostics, sha256}` resolves from the child module.

## Config aux sources: no further gate needed

`config_parsing_sources` holds the snapshots of `extends` configs.
- **Effects already covered:** those sources affect only the options, which the full option parity checks; the config diagnostics, whose file name, start and length are compared; and the diagnostic file identities.
- **Bytes already covered:** their bytes come from pooled, hash-checked VFS inputs.
- **No complete-command field reads them directly:** diagnostic rendering is not compared.

A precise gate would need a non-production accessor, because the field is `pub(crate)`. Not worth it.

Stated conservatively: the snapshot now compares every `ProgramOptions` field except `config_parsing_sources`. That field is covered indirectly, and the pins on `prepared.rs` and `config.rs` catch any new private field.

## Limits of the approval

- The updated native Rust is not compiled yet.
- The 8-row EF7 self-check covers only the observer serialization.
- `paths` keys that look like integers would be reordered by JS object enumeration. That is theoretical; say so in a comment.
- No KNOWN rows are retired by any of this.