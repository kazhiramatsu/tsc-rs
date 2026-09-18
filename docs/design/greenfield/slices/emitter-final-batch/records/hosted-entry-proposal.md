# Hosted entry / job-split proposal (for the integrator; nothing here is registered)

Scope: the two new local entries of this batch and the EF8 minimum inputs. The integrator owns
`.github/workflows`, `cargo xtask acceptance`, profile admission and `ratchets/`; this file only
proposes what to register and how to split it.

## 1. New local entries (this batch)

| entry | command | inputs | wall (local, demoted) | proposed hosted job |
| --- | --- | --- | --- | --- |
| EF2/EF3 rows | `cargo test -p tsc-rs-compiler --test emitter_final_rows` | 21 profile memberships (20 IDs) replayed twice each against the frozen H2.5h / H2.6a / H2.6c observations; `KNOWN` list = attributed open rows (retire assertion refuses stale known rows); at the r8 candidate `KNOWN` holds only the two case-insensitive host rows (oracle contract, `records/oracle/case-canonical-proposal.md`) | ~20 s after build | `gates` sibling job `emitter-final-rows` (same runner class as `ts-tests`; no Node) |
| EF4/EF5 class 40 + EF6 global 14 | `cargo test -p tsc-rs-compiler --test emitter_final_batch` | fixtures `class-field-alias-map-positions.json` / `promoted-class-export-maps.json` (28 + 12 commands) and `ratchets/h2-8a-*` (14 IDs) | ~70 s + ~50 s | same job, second step |

| EF7 universe (r9) | `cargo test -p tsc-rs-compiler --test emitter_final_universe` | `fixtures/emitter-final-universe.json` (217 previously unobserved PLAN-BASE IDs, fresh TS observations ×2, `scripts/observe-emitter-final-universe.mjs`) and `emitter-final-universe-plan-base.json` (1,809 never-observed compiler/conformance IDs); `KNOWN` = attributed open rows with the retire assertion | ~100 s (217) after build; plan-base measured in REPORT §9 | same job, third step (or its own `emitter-final-universe` step if the plan-base replay exceeds the 45-min review margin) |
| EF8 P1–P8 (r9) | `cargo test -p tsc-rs-compiler --test contracts h2_8a_output_matrix` | `fixtures/output-matrix.json` (22 rows, P1–P7) + `output-matrix-filesystem.json` (4 rows, P8), `scripts/observe-output-matrix.mjs` | ~55 s after the contracts build | the EF8 §5 `output-matrix` controls entry: `--test contracts h2_8a_output_ h2_8a_package_output_inputs h2_8a_import_helpers` now includes this module by prefix |

All entries are hermetic (fixture-driven, no network, no Node). They belong in the hosted
acceptance boundary as a separate job so the fixed `ts-tests` entrypoint stays unsplit
(`cargo xtask acceptance` policy).

## 2. Job split for the heavy replays this batch could not run locally

| replay | size | why hosted | proposed job |
| --- | --- | --- | --- |
| `decorator-binding-pipeline --all` | 758 exact + 9 known | ~40 min local | `witness-heavy` matrix job, one suite per matrix entry |
| `post-t1-residuals --all`, `bundle-metadata-t1 --all` | 101 / 18 | ran locally at final bytes (chain8) | fold into `witness-heavy` |
| EF8 P1–P8 (26 rows: 22 + 4) | minted at r9 (`scripts/observe-output-matrix.mjs`) | replayed locally (`contracts-output-matrix-r9a`); hosted job still missing | `output-matrix` controls entry (§1) |
| global 769 / class 1228 full replays (EF8 A-CLOSE denominators) | — | no hosted entry exists today (EF8 §hosting gap) | new `output-matrix` job reading `ratchets/h2-8a-*` |

## 3. Retire / admission proposals (evidence-only; see REPORT §8 for the both-pass results)

- `ratchets/h2-5h-known-divergences.v1.json`: retire the H2.5h rows that replay exact ×2 in
  `emitter_final_rows` (listed in REPORT §8 with input hashes).
- `ratchets/h2-6c-known-divergences.v1.json` / `h2-6a`: retire the map-family rows that replay
  exact ×2 (same table).
- `sourceMapWithNonCaseSensitiveFileNames` / `…AndOutDir` (h2-6c): NOT retired by this batch.
  Apply the oracle host patch (`records/oracle/h2-6c-qualification.case-canonical.patch`),
  re-mint the four cases of the proposal §3, replay `emitter_final_rows` against the minted
  bytes, then retire the two rows from `KNOWN` and from `h2-6c-known-divergences` (proposal §5–§6).
- Known-native fixture rows retired in this batch keep their original projection under
  `records/ef1/post-t1-residuals-known-native.before-retire.json`.
