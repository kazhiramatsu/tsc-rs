No blocker. The config-precedence refinement matches upstream exactly on the emit route. The supplemental native/TS/comparator chain fails closed and never touches the census selection, pins or failure records. Three concrete additions are worth making before the run: pin and derive the roster, make the comparator return a disposition instead of crashing on a missing field, and decide on the no-emit twin of the precedence fix. This is from reading the uncommitted diff and the four new files; I made no edits or builds.

## 1. Clearing `mapRoot`/`sourceRoot` before the descriptor loop

- **Upstream:**
  - `createCompilerOptions` (`projectsRunner.ts:454-468`) always creates own keys `mapRoot` and `sourceRoot`, set to the resolved value, the raw descriptor value, or `undefined`.
  - `parseJsonSourceFileConfigFileContent(…, existingOptions)` then calls `extend(existingOptions, parsed.options)` (`commandLineParser.ts:3077`).
  - `extend` (`core.ts:1495-1507`) copies `first` last using `hasOwnProperty`, so an own `undefined` in the runner options erases a config `mapRoot`/`sourceRoot`.
- **Native:** `options.map_root = None; options.source_root = None;` before the loop, then descriptor values and resolution. This gives the same final options for explicit roots and config roots.
- **Diagnostics:**
  - Config-parse errors for those keys (for example a wrong type) are produced during config parsing, before the merge, in both implementations. They stay.
  - Checks that `createProgram` runs on the final options (such as `mapRoot` without `sourceMap`/`declarationMap`) run on the cleared value in both.
  - So no config-relative diagnostic differs.
- **Remaining gap (no-emit route):** `apply_project_runner_existing_options` doesn't clear them. Upstream has a single project path, so a config-root no-emit project with a tsconfig `mapRoot` would keep it natively and could get an extra option diagnostic.
  - No `tests/cases/projects` config sets either key, so nothing is affected today.
  - Either add the same two-line clearing to the no-emit route plus its mirror, or record it explicitly as a known no-emit precedence note. It is the same upstream rule, so applying it is the more consistent choice.
- **Resolver:**
  - It now takes the raw descriptor value.
  - A non-string with the resolve flag set is an error; so is a non-string `declarationDir`.
  - An empty string keeps the upstream fallback.
  - The domain checks are unchanged.

## 2. Native supplement (`recovery_corpus_native/project.rs`)

**Trustworthy:**
- It checks roster `schema`/`kind`, duplicate IDs, and that each ID resolves to exactly one recorded plan, which must be a project plan.
- It requires clean `HEAD`s on both workspaces before and after, the same vendor tree hash, and the same TS library and plan manifest hashes before and after.
- Output is `create_new` only.
- Refusals are loaded twice and must match, then recorded as `not-loaded; emit-not-qualified`.
- For loaded rows:
  - `plan_input` feeds the document pool;
  - the second run goes through the parent's `prepare`, which applies the byte-for-byte document and `same_input` checks;
  - the option snapshots must be equal and the two observations identical.
- `input_sha256` uses `canonical_input` (sorted keys, safe-integer guard), which matches the Python `sort_keys`, compact, `ensure_ascii=False` form.

**Gap — the roster is self-certified.** The artifact records `roster_sha256`, but nothing ties the roster to the 108 census load failures. Add both:
1. The comparator requires the roster bytes to match the committed `records/project-projection-roster-r139.json`, by recorded SHA.
2. A one-time derivation check that the roster IDs equal `{case_id for failure in census snapshot load_failures if suite == "project"}`. That's 108 IDs, and it should match the r133 descriptor file. No row can then be dropped silently.

**Note — `dependencies` is informational.** It omits the program-crate `prepared.rs`/`config.rs` and harness `execution.rs`. Provenance still holds because the clean `head` covers every file, and both the comparator and the Node side assert it.

## 3. Node observer and comparator

**Node:**
- It re-checks the native artifact's roster SHA and IDs, both `HEAD`s, the mirror pins, the native `dependencies` hashes, the library and manifest.
- It reuses `observeSelectedRow` (with its reconstruction and repetition checks) for loaded rows and marks native refusals as `native-input-unavailable`.
- It writes with `wx`.

**Comparator:**
- It enforces equal coverage in both artifacts, the input-hash triple, two identical runs per side, and option equality before output equality.
- The output comparison covers the full object: writes, diagnostics, exit code and result.
- Any row other than `complete-command-exact` makes it exit non-zero, including refusals, mismatches and unsupported rows.

**Fixes:**
1. `a["options"] != e["options"]` and `"observer_unsupported" in ec` assume keys that may be missing from TS rows that don't carry them. Use `.get(...)` and map a missing key to an explicit disposition (for example `oracle-shape-invalid`) rather than a `KeyError`.
2. Also require `len(ids)` and the roster SHA to equal the pinned values, per item 2 in section 2.

**Controls to add** (the four existing ones pass):
- a roster that doesn't match the pin;
- an oracle row missing `options`;
- a native refusal paired with a TS row that has a different disposition;
- an `input_sha256` mismatch on one side only.

Once these are in, the chain is fit for the 108-row run as a separate supplemental record. Loader success alone still isn't counted as emit qualification.
