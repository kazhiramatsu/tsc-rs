# OPS-COVER-3N / 3O: module identities and original JavaScript bundle maps

2026-09-17. Owner: integration. Kind: evidence. Base:
`84da0c0278c296fd295f15e48177ada87810f841` (PR #549).

## Boundary and implementation design

OPS-COVER-3N registers `h2_7d_module_identities`: three existing tests comparing
38 ordinary JavaScript transform/print inputs twice with a real checked resolver.
The frozen collections contain 24 module inputs (20 ordinary, four API-only
references), 12 System generated-name inputs, six underscore-alias inputs, and
14 path-helper references. The API/path references are not additional Rust
executions. The observer checks each frozen collection before native replay.

OPS-COVER-3O registers the previously unselected fourth test in
`h2_7d_declaration_bundles`. It compares four original JavaScript declaration
bundle inputs, each twice, through the existing internal recorder. Existing
ordinary/forced bundle controls keep their separate selection. The new observer
repeats the exact four original TypeScript Program commands and compares all
fields with the unchanged H2.7d/e ratchet. A four-ID manifest records hashes of
the original inputs, observations, oracle implementation and compiler pin.

Neither native comparator is a new public Program.emit or CLI qualification.
3N compares source/library order, module paths/source associations, JavaScript
bytes and BOM. 3O compares the existing transform/print/declaration/map recorder
observations; its public-command oracle contains more fields than that internal
Rust path executes. Reused corpus IDs and reference memberships are not new
accepted corpus cases. No runtime activation or production change is planned.
Claude's T1 bundle-metadata repair and the R9/R12 residuals stay separate.

Required references are the document precedence in `docs/design/README.md`,
the current emitter architecture's arena/metadata/transform/printer boundaries,
the post-H1 schedule §1.1, the coverage inventory v18, and the unchanged test
sources below. Upstream behavior is TypeScript 6.0.3; existing fixtures pin body
spans, compiler hashes and observer dependencies. `baseline.v1.json` records
the exact source hashes and fresh native results before the runner changes.

| Slice | Current gap | Existing authority and observer | Implementation |
| --- | --- | --- | --- |
| 3N | standalone target has no PR entry | `h2_7d_module_identities.rs`; `bundle-module-identities.json`, `system-generated-names.json`, `module-alias-underscores.json`; their three `observe-*.mjs` scripts | Add `module-identities` to the compiler-direct registry with three unfiltered tests, fixture membership validation, observer checks and explicit ownership |
| 3O | name-filtered target omits original JavaScript test | `h2_7d_declaration_bundles.rs::original_javascript_declaration_bundles_match_typescript_twice`; `h2-7de-candidate-inputs.v1.json` and `h2-7de-observations.v1.json`; `h2-7de-observations.mjs` | Add `bundle-original-javascript` with one exact test / three filtered, a fixed original-ID manifest and a fresh two-pass oracle check |

Allowed changes: the witness registry, replay planner/tests, workflow Node
selection, execution hashes, new bounded observer/manifest, inventory and these
records. Test repairs require reproducing a failure and preserving its original
observations. Production changes require a separate owning packet.

Dedicated test inputs select their owner; the shared declaration test source
selects both registered suites. The module-identity fixture is also consumed by
the declaration observer and emitter unit tests, so it preserves full replay.
Ratchets, compiler/library bytes, VFS overlay, shared support and unknown inputs
also retain full selection. These registrations never narrow shared production
coverage. All selected observers must succeed before Cargo; missing, zero,
ignored, partial or unexpectedly filtered native results must fail the runner.

The new suites join `controls` and reuse its compiler build. Their measured
local entry takes 81.466 seconds (29.875 seconds for oracles and 51.461 seconds
for Cargo build/replay). Added to the previous 38m11s controls job, this gives
about 39m32s as a planning estimate, below the 45-minute review threshold. The
estimate is not a hosted measurement. Both suites install pinned Node even
when selected alone; two Cargo workers and the 60-minute limit are retained.
Inventory v19 records the initial separate-job candidate; v20 records the final
shared-build plan after measurement. This matrix change does not change the
witness commands, observer/native source or frozen inputs used in the local run.

## Validation and delivery

First run the existing three module tests and the exact original-JavaScript
test at the base, preserving commands, exits, logs and source hashes. Then
validate the final registered entries together once locally. The Rust binaries
can be compared with the baseline when their sources remain unchanged.
Planner tests cover shared/private dependencies, matrix membership, observer
failure, missing test results, native failures and frozen manifest drift.
Run qualification policy, inventory regeneration and relevant format checks.
The combined final candidate goes through hosted CI in one PR.

Completion requires observed native success, frozen upstream checks, verified
selection/failure behavior, inventory update and all selected hosted gates.
Until recorded below, hosted execution is pending. Existing success records
from #549 are not evidence for this candidate.

## Local results

- [Baseline](baseline.v1.json): three module tests passed (54.70s native replay;
  482.287s including a new worktree build). The exact original-JavaScript test
  passed in 3.62s (6.039s including its target build).
- [Registered entry](final.v1.json): all four tests passed in 81.466s through
  `replay.py witnesses`. Module38 and original-JavaScript4 inputs are each
  compared twice. Four observer commands passed; the new observer compared
  eight fresh TypeScript Programs with every original field unchanged.
- [Source parity](source-parity.v1.json): all existing crate/vendor source and
  fixture bytes are unchanged; 1,640 inventoried files are byte-identical. Both
  Rust test binaries are identical before and after registration.
- Planner/runner tests cover 62 selection/failure checks. Qualification policy
  and two policy-boundary tests pass. Final matrix checks and v20 regeneration
  are recorded in `checks-final.v1.json`. No second native replay is claimed
  for the subsequent matrix-only adjustment.

The final registry has 71 standalone targets: 34 unfiltered, 14 name-filtered,
23 without a direct entry (compiler3 / other20). The previously omitted fourth
declaration-bundle test now has its own exact selection; the other three keep
their existing scoped suite. Hosted results are recorded below. PR #550 subsequently merged at
`eb6dc2c7872b18442657f8eefde5efc9e8fb4cf7` after the T1 addition passed
[its final-head validation](../../h2-8a-bundle-metadata-t1/integration/README.md).

## Hosted validation: PR #550

[PR #550](https://github.com/kazhiramatsu/tsc-rs/pull/550) passed all nine replay
jobs and both aggregate gates at `059519e997c5f81fd313c8166b9e28fd8fefee79`.
Both planning checks also passed: 13 successful checks in total.
[The receipt](hosted.v1.json) retains run/job identities, timing and the hash of
[the controls log](hosted-controls.log.gz).

Controls executed all 20 compiler-direct suite selections / 68 tests, including
the three module tests and the original JavaScript recorder test. The original
JavaScript observer checked eight fresh TypeScript Programs against the unchanged
ratchet. All four newly registered Rust tests passed. The whole compiler-direct
batch took 460.852s in observers and 1299.146s in Cargo build/replay. These are
whole-batch measurements, not the added suites' isolated cost.

| Job | Duration | Result |
| --- | --- | --- |
| [acceptance (early)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35166123773/job/105027603408) | 12m23s | success |
| [acceptance (late)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35166123773/job/105027603416) | 17m28s | success |
| [acceptance (wide)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35166123773/job/105027603464) | 22m35s | success |
| [witnesses (printer)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35166123746/job/105027601260) | 2m33s | success |
| [witnesses (controls)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35166123746/job/105027601280) | 38m49s | success |
| [witnesses (retained)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35166123746/job/105027601289) | 9m05s | success |
| [witnesses (declaration-maps)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35166123746/job/105027601320) | 6m15s | success |
| [witnesses (primary)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35166123746/job/105027601357) | 10m30s | success |
| [witnesses (decorator-binding-pipeline)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35166123746/job/105027601358) | 17m59s | success |

Controls took **38m49s**, below the 45-minute split-review threshold and the
60-minute hard limit. The nine replay jobs total **137m37s**, excluding planning,
aggregate gates and any later main-push validation. No extra compiler job was
added. The implementation and hosted validation of 3N/3O are complete; this
record does not claim the PR has merged or expand runtime admission.
