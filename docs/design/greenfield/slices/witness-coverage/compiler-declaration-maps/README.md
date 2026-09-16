# OPS-COVER-3L / 3M: declaration map commands and stateful APIs

Base: main `3e8f582cb` (PR #546). Codex owns these two compatible CI-entry
slices; Claude's C02 generated-name/binding work remains separate. This batch
registers the existing declaration map output target and the declaration map
API target in a dedicated `declaration-maps` job, then validates their combined
source once on hosted CI.

## Scope and frozen inputs

| Slice / suite | Cargo target | Rust tests | Frozen fixture memberships |
| --- | --- | ---: | ---: |
| 3L / `declaration-maps` | `h2_7e_declaration_maps` | 8 | 84 |
| 3M / `declaration-map-apis` | `h2_7e_declaration_map_apis` | 3 | 75 |

Both targets run without a name filter, ignored tests or internal case selectors.
The catalog counts input memberships, including overlapping and reference rows;
it does not add them together as new corpus admissions.

3L compares 24 original inputs through the direct declaration printer, ordinary
Program, scoped Program and CLI; three runtime map inputs and two disabled
`declaration` inputs through ordinary/scoped Program and CLI; and eleven
ordinary sink/gate commands selected from the 54-row API fixture. The original
`declarationMapsWithoutDeclaration.ts#default` is also compared through the
existing ratchet, overlapping the disabled-declaration fixture. CLI config inputs retain their existing relocation into an owned temporary
directory. Each CLI command now compares stdout, stderr, exit and the complete
recursive file set/bytes against the pinned `_tsc.js`, twice. The directory is
rebuilt from pristine inputs at the identical path before the reference runs;
there is no output/path normalization and no previous output can satisfy a write. Raw declaration/map
bytes, diagnostics, write order, BOM, metadata and Program map structures retain
their existing comparisons. The bundle-boundary test now compares its original input against a new
frozen Program observation, twice.

3M replays all 54 API sequences plus 19 reference-path sequences (8 base,
3 supplementary target families and 8 outDir controls). Calls within a sequence
share a checker; each sequence starts from a fresh Program twice. It compares
getters, forced emit, ordinary commands, final diagnostics, partial writes,
callback errors, sink feedback and resolver borrow counts. The one scoped
ordinary noEmit command remains a typed refusal; the two ordinary target
sequences remain oracle references. Neither is promoted by registering CI.

The six observer commands run in check mode against immutable fixtures:
API, reference paths, ordinary maps, runtime maps, disabled declaration maps and the former bundle-boundary input.
All observations repeat twice. The API observer is deduplicated when both suites
run together. The pinned Node version is installed even for a single-suite job.
Shared `declaration-reference-paths.json` preserves full acceptance/witness
selection because the forced-declaration acceptance helper also imports it.
Shared ratchet, vendor and VFS overlay changes keep full coverage. The common API
fixture/observer selects both suites; individual target and other dedicated
fixture changes select their owner only.

## Validation

The baseline and final receipts record actual commands, exits, source manifests,
compiler versions, compressed logs and binary hashes. Timing includes local
compilation/replay on a shared machine and is not an isolated benchmark.
Planner checks exercise every observer mode, cross-target/shared ownership,
missing/zero/ignored/filtered Rust results and failure propagation before Cargo.

The previous controls job took 37m51s. The additional local run took 6m57s,
which would bring their simple sum to 44m48s with little margin for hosted
variation. OPS-BUDGET therefore gives these two suites one dedicated
`declaration-maps` job before the first hosted run. Its extra compiler build
adds runner time, while controls retains its original 18 compiler-direct suites
and 64 tests. All eight replay jobs and both gates remain required. The
60-minute hard limit and two-worker setting remain in force.

### Baseline and bounded test repairs

[Baseline](baseline.v1.json): the API target passed 3/3 tests (68.35s replay;
468.284s including its initial build). The output target passed 6/8 (85.28s
replay; 87.869s including its build), exit 101. The immutable logs and source
manifest retain both failures:

- `h2_7e_bundle_maps_keep_typed_boundary` expected an outFile refusal although
  main emitted the admitted bundle. The new fixture preserves that exact
  source/options and records the complete pinned Program result. The replacement
  test checks the input identity and compares the original public observation,
  without adding a new CLI admission.
- `h2_7e_cli_status_and_exit_match_every_typescript_observation` compared a
  config-based CLI with bare Program expectations. The absolute and relative
  declarationDir cases have different TS6 config defaults: TS5011 and `types/src`
  output paths. The actual pinned CLI reproduces that behavior. The comparator
  now checks the full actual CLI result for every existing case, retaining
  diagnostics rather than filtering only TSFILE lines. Original Program
  observations and checks remain unchanged.

No production source or existing fixture is changed. CLI comparison also now
repeats twice; the former wrapper ran its CLI once. There are 35 CLI memberships
(34 unique inputs, with the original disabled-declaration ID compared by two
wrappers), 70 fresh Rust/reference pairs. These include five file-system failure
inputs, each repeated twice; their complete diagnostics and file bytes also match. The extra bundle input is the former
refusal input, not a new runtime admission.

An optional new config-based outFile CLI probe found a separate macOS host
refusal (`useCaseSensitiveFileNames`), while TypeScript reports TS5101 and emits
the bundle. [Both raw observations](out-file-cli-reference.v1.json) and the
[exploratory test failure](exploratory-local.log.gz) are retained. The original
boundary assertion exercised only Program emit. Its replacement keeps that
scope; H2.8b-HOST1 / H2.8e-ARGS1 retain the extra CLI route. No new failure is
hidden by a normalization, conditional skip or successful admission claim.
The same focused run passed the repaired existing 24-case CLI test.

### Final local validation

[Combined replay](final.v1.json): exit 0, all 11 tests passed without filtering
or ignored tests. API replay took 103.77s and map output replay 224.87s. The six
frozen observers passed in 83.309s; Cargo build/replay took 333.243s; the complete
entry took 416.755s. All 70 fresh CLI pairs matched stdout/stderr/exit and files.
The API binary hash is identical to its baseline. The output binary hash and
both source manifests are retained in the receipt.

[Source parity](source-parity.v1.json): 410 production/vendor/build inputs and
298 existing fixtures remain byte-identical. The new former-boundary fixture
has its own hash. Only the output test, map observer and witness runner changed
within the existing crate/script validation files.

[Initial checks](checks.v1.json) validated the entry before job splitting.
[Final checks](checks-final.v2.json) validate the final planner/policy/inventory,
formatting and diff. The planner has 56 selection/failure tests; policy boundary
checks have two tests. [Inventory v16](../inventory.v16.json) retains the initial
controls projection; [v17](../inventory.v17.json) records the final dedicated job.
Both have 69 standalone targets: 32 unfiltered, 13 filtered, 24 with no direct
entry (compiler4 / other20). Job splitting changes no runtime test/observer bytes
or selected witness command; no duplicate local Rust replay is claimed.

Pending: hosted results and merge.
