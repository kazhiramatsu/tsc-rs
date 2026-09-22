The consumers are close, but one high issue blocks the whole comparison and six medium issues cause rows to be blocked or failures to be blamed on the emitter wrongly. I read the code at replay `8f63a3651` and compared it with the harness loaders and the census capture; nothing was built or run. `crates/syntax`, `harness`, `program` and `types` are byte-identical between census `67df86615` and `8f63a3651`, so the pins currently hold.

## High

**H1. Native `input_sha256` is not canonical, so the comparator's assertion fails on every row.**
- **Trigger:** `serde_json` has `preserve_order` enabled in the harness, program and checker crates. Cargo features unify, so xtask's `serde_json::Value` keeps insertion order.
- **Effect:** `sha256(serde_json::to_vec(&row["command_input"]))` hashes the keys in the order the census wrote them (the `json!` order in `plan_input`, then `limits` and `prepared` appended). The selector preserves that order because it dumps without `sort_keys`. The TS observer hashes a key-sorted form, and so does the comparator (`sort_keys=True`). So `actual["input_sha256"] == expected == input_sha` fails for every row.
- **Fix:** in the native consumer, hash a recursively key-sorted copy using the same compact separators. Escaping already matches: serde, Python `ensure_ascii=False` and JS `JSON.stringify` escape the same characters here. Add a unit test that feeds keys out of order.
- **Unaffected:** `same_input` is fine; both sides come from the same `json!` code, so their order matches.

## Medium

**M1. List options: the JS mirror doesn't trim the way Rust does.**
- **Rust:** `types` and `customConditions` trim each element and drop empty ones. `lib` also lowercases.
- **JS:** `ts.parseListTypeOption` does neither. Checked: it returns `"a, b"` → `['a',' b']`.
- **Result:**
  - `@types: a, b` becomes a false `input-option-mismatch`;
  - `@lib: es5,` puts an error in `errors`, and the assertion then aborts the whole TS run.
- **Fix:** for `types` and `customConditions`, split on commas, trim, and drop empties, exactly as Rust does. For `lib`, drop empty elements before mapping each through the option's element map.

**M2. The ProgramOptions parity check misses fields that change diagnostics or module resolution.**
- **Gap:** `ProgramOptions` also holds `paths`, `config_parsing_diagnostics`, `config_parsing_sources`, `external_config_option_diagnostics`, `default_library_file_name` and `config_file`. The snapshot covers only 6 fields.
- **Consequence:** differences in config-parse diagnostics or option-diagnostic ownership surface as `complete-command-mismatch`, which blames the emitter for what is really an input difference.
  - Paths: the source-set check only catches `paths` changes that alter the loaded file set; unresolved-import diagnostics that differ slip through as command mismatches.
- **Fix:** add these to both snapshots:
  - `paths` entries;
  - the default library name (the host's name in TS);
  - config-parse diagnostics (code, file, start, length), taken from `input.errors` on the TS side;
  - whether an options config file is present (`!!options.configFile`);
  - the external-option-diagnostics flag.

**M3. A TS input-reconstruction failure aborts the entire observer run.**
- **Trigger:** any failure of the assertions in `prepare` or `observe` (root order, source set, unsupported option, a missing document, and the M1 and M6 cases) throws out of `main`. The other rows then get no evidence at all.
- **Fix:** catch the error per row and record `input-reconstruction-mismatch; emit-not-qualified` with the message. The comparator should count it as blocking. Native should stay fail-fast, because its input is already verified against the census.

**M4. Project config parse diagnostics may differ in form.**
- **JS:** uses the object-based `parseConfigFileTextToJson` + `parseJsonConfigFileContent`, whose errors carry no file or position.
- **Rust:** `parse_project_config` uses `parse_config_root_plan`, which parses the source text; only *option* diagnostics are made global via `with_external_config_option_diagnostics`.
- **Trigger:** a project config with a syntax or value error; the diagnostics differ in location.
- **Fix:** check which form Rust gives the program's config-parsing diagnostics and mirror it, e.g. `parseJsonText` + `parseJsonSourceFileConfigFileContent`, then delete `options.configFile` so option diagnostics stay global. Include these diagnostics in the M2 check.

**M5. The pins miss code the mirror depends on.**
- **Current pins:** `execution.rs`, `project.rs`, the directory overlay and `options.rs`.
- **Missing:**
  - `crates/program/src/prepared.rs`: the `ProgramOptions` field set, which decides snapshot completeness;
  - the program crate's config parse module, i.e. the `parse_config_root_plan` semantics;
  - `crates/xtask/src/utf16_literal_recovery_census.rs`: `qualified_input` and `candidate_input`, which the JS artifact layout mirrors;
  - `crates/harness/.../observable_input.rs`.
- **Fix:** add them so a change in any of them forces the mirror to be reviewed again.

**M6. BOM handling in source text.**
- **JS:** `decode` keeps U+FEFF, and the host passes it to `createSourceFile`.
- **Rust:** if its source decoding strips a leading BOM (as `sys.readFile` does), the prepared SHA differs. The source check then fails, and because of M3 the whole run aborts.
- **Fix:** confirm what the Rust decoder does and mirror it in JS `readFile`. Add a control fixture that starts with a BOM.

## Low

- **L1. NoEmit fallback rows may never be exact.** `load_compiler_no_emit` builds its program with `load_program`, which is not the emitting loader. If `emit_command_for_harness` refuses such a program, every fallback row ends up as `production-refusal` and never reaches `no-emit-command-exact`. Check this in the first native unit run.
- **L2. Source order is not checked.** Source sets are compared after sorting, so a difference in load order goes unrecorded; emit and diagnostic order derive from it. Record an order-equality fact without making it block.
- **L3. The comparator doesn't check both observers' code pins.** It doesn't cross-check native `head` or `observer_sha256` against the oracle's `dependencies`. It records them, so this is only evidence hygiene.
- **L4. Possible compile issue in `options.rs`.** `catalog.option_file_name(value).unwrap_or(value)` mixes `&'static str` with `&String`. If it doesn't compile, use `.unwrap_or(value.as_str())`. The other APIs used exist: `ProgramPath::display() -> JsStr`, `ModuleSuffix::value_js`, `option_file_name`.

## Checked and correct

- **Recorded-compiler route:**
  - write order, and skipping units whose content is null;
  - the symlink fixpoint, which matches `execution.rs:503-541` including aliases published by earlier links;
  - the config host: case-insensitive raw-name `fileExists`/`readFile`, and `readDirectory` under `/.src` (your correction holds);
  - config options are not floored on Established, while directive options are;
  - option defaults applied in Rust's order: `skipDefaultLibCheck` first, then settings, then `allowJs` materialized, then `newLine`, then forced `noErrorTruncation`.
- **Qualified and candidate routes:**
  - a forced case-sensitive host;
  - the virtual config kept in the VFS but not applied;
  - the candidate config added only when its path is absent;
  - links whose targets are original files only.
- **Recorded-project route:**
  - root selection matches the three Rust arms, with `project + "/tsconfig.json"` and the base path at the current directory;
  - explicit roots keep duplicates and missing entries;
  - runner defaults override config options in the same order;
  - `lib.es5.d.ts` as the default library;
  - the module variant followed by the descriptor's own `module`.
- **Encodings:**
  - diagnostic category numbering matches: Rust's enum is Warning, Error, Suggestion, Message;
  - message-chain flattening matches;
  - callback UTF-16 plus BOM and data metadata keys;
  - build-info is reported as `observer_unsupported`;
  - `emittedFiles`/`sourceMaps` absent is kept distinct from empty.
- **Workspaces:** the split between code and data workspaces, census ancestry, cleanliness of the data paths, and the library-root realpath all match the round-79 advice.

With H1 fixed, and M1–M3 and M5 addressed, I'd approve the pipeline as a gate that blocks input mismatches correctly. M4 and M6 need checking against the Rust decoder and config parser before the full run, because either one can currently stop the TS run. The 8-row EF7 self-check only confirms the observer serialization; it says nothing about the recorded-compiler, project, or qualified/candidate reconstruction.