Every closure has an explicit `Result<_, String>` annotation that fixes inference before `.map_err(Into::into)` or `.map(...)`. So removing the wrapper naively can break or change typing. The `panic!` one can't take a method call at all.

# Round 166: `duplicate_mod` and the routine clippy fixes

This is read-only (HEAD `34dea8e69`, `git show`, and the r238 clippy log). **I approve your module fix, with the exact scope below and one exclusion.** The routine fixes are fine too, with two typing cautions.

## 1. Every helper declaration, mapped to its roots

I built the root map from every `#[path = "…integration/…"]` edge in each compiler and harness test root and in compiler examples.

**Compiler, loaded only by `crates/compiler/tests/contracts.rs`** (29 modules; declaring `scalar_json` (J), `scalar_path` (P) or both):
- `automatic_type_directive_session_contract` (P)
- `declaration_transformer_replay_decision_equal` (P)
- `emit_session_contract` (P)
- `filesystem_loader_contract` (P)
- `h1_emit_qualification_contract` (P)
- `h1_memory_emit_oracle_contract` (J+P)
- `h2_5h_static_this_super_rows` (J)
- `h2_5h_static_this_super_witnesses` (J)
- `h2_7b_m2_controls`, `h2_7b_w1_controls` (P)
- `h2_7b_w2_write_census` (J+P)
- `h2_7b_w2a_controls`, `h2_7b_w2b_controls`, `h2_7b_w3a_controls`, `h2_7b_w3b_controls`, `h2_7b_w4b_controls`, `h2_7b_w4i_controls` (P)
- `h2_8a_decorator_next_witnesses`, `h2_8a_ellipsis_comment_owners`, `h2_8a_import_helpers`, `h2_8a_object_property_owners`, `h2_8a_retained_accessor_owners`, `h2_8a_token_comment_phases` (J)
- `h2_8a_package_output_inputs` (J+P)
- `library_loader_session_contract`, `original_path_session_contract`, `preserve_symlinks_session_contract`, `source_map_emit_witness_contract` (P)
- `upstream_no_emit_harness_contract` (J+P)

**The shared case:** `source_map_band_probe.rs` (P, line 1046) is loaded by **both** `contracts.rs` and `h2_6a_map_option_projection.rs`. The latter already declares both helpers at its crate root (lines 469-474), which is why it reports `duplicate_mod`.

**Harness:** `module_suffixes_oracle_contract.rs` (388-393) and `upstream_execution_plan.rs` (1408-1413) are loaded only by `crates/harness/tests/contracts.rs`, and each declares both helpers.

**Exclude `h2_8a_decorator_super.rs`.** Its only root is the standalone `decorator_super_contract.rs` (line 9), which does **not** declare the helper. It has a single declaration and no duplicate. Leave it byte-identical.

**Other roots that stay as they are:** every other declaration is in a standalone root or a different crate, each with one copy per crate:
- the compiler standalone tests (`bundle_metadata_t1_contract`, `h2_7d_*`, `h2_7e_*`, `post_t1_residuals_contract`, `transpile_routes_contract`, …);
- the example `h2_baseline_qualification.rs:243`;
- `conformance/tests/unit/h0_memory/tests.rs`;
- `emitter/tests/contracts.rs`;
- `program/tests/*`.

The only other files loaded by several roots (`h2_7b_w4a_controls`, `h2_7c_declaration_blocking`, `h2_7d_original_corpus_shared`) declare neither helper. No xtask code includes any of these files.

## 2. Why one root copy is semantically identical

- **Stateless.** `scalar_json.rs` exports only `pub trait ScalarJson` and `pub fn observe`. `scalar_path.rs` exports only `pub trait ScalarTestPath`. There is no `static`, thread-local or `OnceLock`, so separate per-module copies had no separate state.
- **Consumers use two names.** They use only `use utf16_scalar_json::observe as scalar_json;` and `use utf16_scalar_path::ScalarTestPath as _;`.
- **Precedent:** `crates/program/tests/contracts.rs:1-4` already declares both once at the root.

**The patch:**
1. In `compiler/tests/contracts.rs` and `harness/tests/contracts.rs`, add at the root:
   `#[path = "../../program/tests/support/scalar_json.rs"] mod utf16_scalar_json;` and
   `#[path = "../../host/tests/support/scalar_path.rs"] mod utf16_scalar_path;`
   A private root module is visible to every descendant, so no `pub` is needed.
2. In each listed consumer, replace its `#[path] mod …;` line and keep its existing `use`, retargeted: `use crate::utf16_scalar_json::observe as scalar_json;` / `use crate::utf16_scalar_path::ScalarTestPath as _;`.
3. In `source_map_band_probe.rs`, `use crate::utf16_scalar_path::ScalarTestPath as _;` resolves in both roots.
4. Leave the helper implementations untouched. No `include!`, no `allow` attributes, and no production module moves.

## 3. Routine lint fixes: safe, with two cautions

| Lint | Where | Assessment |
|---|---|---|
| `needless_borrow` `&out` | `xtask/src/utf16_literal_recovery_census.rs:1211` | Safe. Clippy suggests it only when `out` isn't used afterwards. |
| `nonminimal_bool`, 6 sites | e.g. `h2_5h_static_this_super_rows.rs:342/347`, `emitter_final_rows.rs:434/439`, `h2_5h_utf16_literal_rows` | For booleans `a != !b` is exactly `a == b`. Take clippy's suggested rewrite verbatim. |
| `useless_conversion` JsString, 3 sites | `declaration_transformer_replay_decision_equal.rs:2877/2932/3020` | Safe to drop `.map(Into::into)`: the lint proves it is the identity on `Option<JsString>`. |
| The remaining needless-borrow site | declaration replay | Safe, same class. |
| `redundant_closure_call`, 5 sites | below | **Caution** |

**The five closure sites:**
- `module_suffixes_oracle_contract.rs:73`
- `emit_session_contract.rs:78`
- `h2_7e_declaration_maps.rs:760`
- `h2_7e_declaration_map_apis.rs:192`
- `examples/h2_baseline_qualification.rs:119`

**Why care is needed:** each closure has an explicit `-> Result<_, String>` annotation. That annotation pins the error type, and at `module_suffixes:73` it also pins `collect()` to `Vec<String>`, before `.map_err(Into::into)` / `.map(|paths| …)`.
- If you just unwrap the closure, the error type or the `collect()` target can become ambiguous or be inferred differently.
- **Preserve typing** with a typed binding: `let result: Result<Vec<String>, ConfigHostError> = Ok(…collect());` then `result.map(…)`. Do the same for the `String`-error sites.
- **The `emit_session_contract.rs:78` body is `panic!(…)`.** A method call on `!` doesn't work, so replace the whole expression with just `panic!(…)` as the function's tail. `!` coerces to the return type, and the panic message and effect stay identical.
- None of the five closures uses `?`, so no early-return semantics change.

## 4. Verification and pin classification

**Focused verification before the walk, all on the final bytes:**
1. The workspace clippy (`--all-targets`, `-D warnings`) must be clean. The walk refuses to start otherwise.
2. Build and run the affected targets:
   - compiler `--test contracts` (full target);
   - harness `--test contracts`;
   - `h2_6a_map_option_projection`, `emitter_final_rows`, `h2_5h_utf16_literal_rows`, `h2_7e_declaration_maps` and `h2_7e_declaration_map_apis`;
   - `--example h2_baseline_qualification` (build);
   - xtask bin tests.
3. Confirm the test counts match the r238 pass counts, so no module silently drops out.

**What changes and what doesn't:**
- These are test and tooling bytes only. Production compiler/emitter/parser sources stay as runtime-qualified at `0336c566`.
- The H2.5g 921-path **membership doesn't change**: all edited files are already listed, because they changed since `TRUSTED_BASE`. But their **hashes** do, so the edits must land before the walk, as planned.
- **Pin-only metadata** refreshed by the walk and your planned steps:
  - the 5g profile `runtime_inputs` hashes;
  - policy/fuzz source hashes (r226);
  - witness-inventory v36 (minted after the walk).

  None of these is a runtime failure.
