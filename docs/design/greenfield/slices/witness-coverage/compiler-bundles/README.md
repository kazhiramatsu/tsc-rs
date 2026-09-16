# OPS-COVER-3J/3K: bundle Program and declaration/map witnesses

Status: combined candidate validated in PR #546; all seven hosted replay jobs and both gates passed. Base: `618ed97b5ae25d6763f19a6c9d357c6b3a279a5f`.
This evidence/CI batch adds direct entries to existing tests; it changes no
compiler behavior, frozen expectation, runtime admission or acceptance denominator.
It follows the [coverage inventory](../README.md) and the user instruction to
combine compatible slices before hosted CI.

| Slice / suite | Selected target/tests | Frozen input and observation boundary |
| --- | --- | --- |
| OPS-COVER-3J / `bundle-program` | `h2_7d_bundle_program`, all four tests | 25 main sequences: ordinary commands, fresh forced declarations, and same-session commands/getters/forces. Two adjacent rows compare the noEmit command plus its fresh force/getters, and preserve ordinary-target non-admission. |
| OPS-COVER-3K / `bundle-declarations` | `h2_7d_declaration_bundles`, first three exact tests | 25 declaration rows (19 visitor comparisons, three map-owner rows and three prior Program gates); 12 map rows plus eight JSON controls; six metadata, two constant-value and three runtime-comment controls, in ordinary/fresh-forced modes. |

The fourth declaration test joins four original JavaScript corpus IDs; it is
excluded from this batch and remains in the filtered-target coverage queue.
Acceptance already executes original command comparisons; this does not prove
its internal visitor/map-recording checks run. The two new suites share 25
inputs, and maps/metadata also reuse IDs across observation modes. Memberships
are namespaced by fixture section and never summed as new corpus successes.

Pinned expectations and owners: [bundle declaration observations](../../h2-7d-declaration-observations.md),
[bundle map recording](../../h2-7d-map-recording.md),
`scripts/observe-bundle-declarations.mjs`, `scripts/observe-bundle-maps.mjs`,
`crates/emitter/tests/fixtures/bundle-declarations.json`, `bundle-maps.json`.
The existing observers run twice internally and compare the entire frozen
artifact, including input/source hashes. Use the repository's `.node-version`.

Implementation boundary: the bounded test-debt repair below, `scripts/witness.py` catalog/section membership,
`.github/workflows/witness.yml` pinned Node selection, `.github/ci/test_replay.py`
selection/failure controls, the new inventory snapshot and these records.
Fixture/observer changes select all owning suites. Shared upstream/corpus/overlay
inputs retain broad coverage. Missing/duplicate/empty section rows fail before
Cargo; observer failures stop replay; nonzero exits and unexpected test counts
fail the runner. No runtime packet is activated.

Local validation uses the two target commands at the base, then the final
combined runner, with `taskpolicy -b nice -n 15`, two Cargo workers, debug info
and incremental disabled. Logs retain actual exits, durations and hashes.
Heavy local processes run one at a time. Hosted validation follows the complete
batch and retains the existing two-worker/60-minute limits and 45-minute split
review threshold. Hosted results are recorded below.

## OPS-DEBT-BUNDLE-NOEMIT: stale command refusal

The unchanged base ran three Program tests successfully and failed
`bundle_later_owner_references_remain_separate` at its historical
`InvalidProgramMode` assertion (exit 101). All three declaration/map tests passed.
`ProgramSession::emit_command_for_harness` already handles NoEmit since
`d364a056a`; `c66ef5248` completed its declaration-diagnostic scheduling.
The frozen adjacent row has a normal command result, no writes, diagnostics
5101/5107 and exit 2. `_tsc.js:125636` (`handleNoEmitOptions`) and
`129412-129485` (command reporting/status) own this observation. Vendor hash:
`1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`.

Before changing the test, the original failures and source manifest were saved.
The sole Rust edit replaces the stale refusal with the frozen complete public
command tuple and zero H2 activity checks. The fresh forced and getter checks
remain; ordinary-target emit is still reference-only. No expected bytes,
production source or comparison normalization changes. This bounded test-debt
repair joins 3J/3K; it does not promote H2.9 or claim same-session noEmit reuse.

## Final local validation

- Base: Program 3 passed / 1 stale refusal failure, declaration/map 3 passed /
  1 original wrapper filtered. Actual exits 101 and 0, saved in [baseline](baseline.v1.json).
- [Final combined entry](final.v1.json): exit 0, all 7 tests passed. Program
  4/0 filtered; declaration/map 3/1 filtered. All applicable comparisons twice.
  Both frozen observers passed: 69.392s; Cargo build/replay 129.343s; complete
  invocation 198.959s. These are local shared-machine observations, not hosted
  runtime estimates or isolated performance comparisons.
- [Checks](checks.v1.json): planner/failure boundaries 54 tests; policy boundaries
  2 tests; policy validation, inventory regeneration and formatting passed.
- [Source parity](source-parity-final.v1.json): 1,336 crate/vendor/build files
  unchanged from the captured base, with the one test-only edit listed explicitly.
  Existing fixtures and all production sources are unchanged. Both final Rust
  binary hashes and actual commands/environment are retained in the final receipt.
- [Inventory v15](../inventory.v15.json): 69 standalone targets, 30 unfiltered /
  13 filtered / 26 without a direct entry (compiler6 / other20).

Hosted CI validated the combined candidate; the recorded source identity is below. The unchanged
controls job last took 35m45s; the new local entry fits its available margin,
and the measured hosted total is recorded below against the 45m review threshold.
Seven replay jobs and both gates remain required for this shared-runner change.
The ignored/unselected original wrapper and other uncovered targets remain open.

## Hosted validation: PR #546

Candidate `160161683d002a18f939f8ee6fcce5e33f6e6faf` passed both workflows and both aggregate gates.
[The receipt](hosted.v1.json) retains full run/job identities and compressed log hashes.

| Job | Duration | Result |
| --- | --- | --- |
| [acceptance (wide)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35098305041/job/104801258638) | 28m44s | success |
| [acceptance (early)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35098305041/job/104801258664) | 11m52s | success |
| [acceptance (late)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35098305041/job/104801258697) | 17m21s | success |
| [witnesses (retained)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35098304984/job/104801223896) | 9m20s | success |
| [witnesses (controls)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35098304984/job/104801223906) | 37m51s | success |
| [witnesses (primary)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35098304984/job/104801223994) | 10m37s | success |
| [witnesses (printer)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35098304984/job/104801224053) | 2m51s | success |

Controls executed all 18 compiler-direct suites / 64 tests, including the new
4 Program and 3 declaration/map tests. Both frozen bundle observers passed;
comparison repetitions and membership overlap remain as documented above.
The stale noEmit assertion repair is validated on hosted as well as locally.

Controls took 37m51s; the seven replay jobs total
118m36s. These totals exclude plans, aggregate gates
and any main-push run. Compiler-direct oracle time was
440.889s and combined Cargo build/replay time
1273.943s. They measure the entire
selected group, not just the two new suites.

Controls remained below the 45-minute split-review threshold. The 60-minute
hard limit and two-worker setting are unchanged.

The follow-up evidence commit changes documentation and receipts only. Hosted
Rust success belongs to the candidate above; a documentation-only CI plan is
not counted as a new Rust replay.
