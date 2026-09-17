# Focused witnesses and hosted replay

## Batch implementation before hosted CI

User instruction, 2026-09-16: for Codex/integration work, complete multiple
compatible assigned implementation slices before starting hosted CI. Sequence
related changes on one integration branch, with focused local checks during
implementation, then submit the combined final source for hosted validation.

Retain each slice's readiness conditions, frozen inputs, before/after evidence,
and ownership boundaries. Validate the combined source and keep the existing
job coverage, worker limits and time budgets. Further hosted runs are for failed
checks or necessary fixes and new changes. Choose the next group of implementation
slices before beginning the next hosted CI cycle.

## Focused local checks

Use a case selection while editing. List IDs before selecting; repeated `--case`
arguments select a union. A missing or empty selection fails before Cargo starts.

```sh
python3 scripts/witness.py followup3 --list
python3 scripts/witness.py followup3 --case es2015/set/ --dry-run
python3 scripts/witness.py followup3 --case es2015/set/
python3 scripts/witness.py retained --case retained-constructor-references/
python3 scripts/witness.py direct --all
python3 scripts/witness.py printer --all
python3 scripts/witness.py literal-value-provenance --all
python3 scripts/witness.py comma-argument-factory --all
python3 scripts/witness.py declaration-map-cli --all
python3 scripts/witness.py utf16-review-fix --all
python3 scripts/witness.py utf16-literal-witnesses --all
python3 scripts/witness.py utf16-original-commands --all
```

Suites: `primary`, `extra`, `followup`, `followup2`, `followup3`, `retained`, `direct`,
`printer`, `bundle-sinks`, `declaration-map-cli`, plus the eleven small
[emitter direct suites](design/greenfield/slices/witness-coverage/emitter-direct/README.md)
and three [compiler UTF-16 suites](design/greenfield/slices/witness-coverage/compiler-utf16/README.md):
`utf16-identity-recovery`, `utf16-review-fix`, `utf16-tagged-template`, plus
[compiler literal suites](design/greenfield/slices/witness-coverage/compiler-literals/README.md)
`utf16-literal-witnesses` and `utf16-original-commands`.
The [transpile research contract](design/greenfield/slices/h2-8c-transpile/INTEGRATION.md)
is registered as `transpile-routes`: 287 original + 14 review inputs, nine tests.
Its observer checks both frozen collections twice before Cargo runs.
`--all` explicitly requests the whole suite, normally on hosted CI. `--list` and
`--dry-run` neither build Rust nor run tests. Commands use a manifest path and an
exact test name for the large shared comparator targets. Small emitter direct
suites and unfiltered compiler direct suites run their whole target, with `--all`;
`--case` is rejected for those suites.
Their entry checks only the selected fixture observers and rejects missing,
zero-test, ignored or unexpected filtered target results.
`utf16-literal-witnesses` selects its one dedicated test and requires exactly nine
shared helper tests to be filtered out. Its internal environment selectors are
cleared so `--all` always covers all 64 promoted inputs.
They also avoid building the dev-profile xtask executable before the test profile.
The `printer` target runs together: 142 small direct rows (141 exact, one documented
generated-name gap), plus the same target's probe/safety/negative controls. It takes milliseconds
after compilation and does not run the Program/oracle chain. `bundle-sinks` runs
10 complete commands together; normally leave that replay to hosted CI.
`declaration-map-cli` runs eight existing original CLI cases twice, with output
bytes/set, diagnostics and exit checks. Its exact test omits the shared Program
comparison already covered by acceptance; the immutable oracle checks remain.
See [OPS-COVER-3A](design/greenfield/slices/witness-coverage/declaration-map-cli/README.md).
On macOS, use `taskpolicy -b nice -n 15 python3 scripts/witness.py ...` to lower
priority. Do not run several heavy local replays simultaneously.

The primary input list includes two recorded upstream exceptions. The Rust
comparator reports those separately from complete native observations; a selection
with no native case fails. Direct controls run together (28 exact, 4 documented
divergences). Capture-enabled observers can emit extra commands: enable captures
only for cases needing diagnosis and keep both measurement sides equivalent.

The common compiler comparator and SUPER test load immutable `lib.*.d.ts` bytes
once per process. Each case still receives a fresh memory host; the two fresh
Program constructions, comparisons, callback inspection and failure reporting
remain. No AST, checker state, Program, or observation result is cached.

## Hosted jobs

`.github/workflows/ci.yml` retains the required `gates` check and splits the complete
31-slice `cargo xtask acceptance` sequence into independently rerunnable jobs:

| Group | Slices |
| --- | --- |
| early | conformance, H1, H2.1 through H2.5f |
| wide | H2.5g |
| late | H2.5h, H2.6a/b/c, H2.7b/c/d/e |

All jobs retain the 60-minute timeout and two-worker limits. The canonical full
command remains available. A structural check compares the grouped slice registry
and dispatch calls against that full command, so omitting or duplicating a slice
fails before replay.

`.github/workflows/witness.yml` owns primary, short controls and retained jobs.
A separate `printer` job shares one emitter build between the printer failure
suite (four observers, seven targets and one exact noEmitOnError owner control)
and eleven individually selectable literal/factory/metadata targets. It retains a
20-minute limit and two workers. Selected direct targets run in one Cargo call;
a literal fixture change runs only its owning target and observers, without the
printer failure suite or a compiler/acceptance replay.
The controls job also runs `bundle-sinks` and `declaration-map-cli`, sharing its
existing compiler build. The CLI test wrapper selects only its eight CLI cases;
its shared Program helper selects late acceptance and the CLI suite.
The three compiler UTF-16 targets also share the controls build. Their selected
observers run first, followed by one Cargo call for only the selected targets.
The 120 fixture rows contain 111 complete-command comparisons and nine typed
refusal controls, each repeated twice; refusals are not reported as exact matches.
OPS-COVER-3C adds 64 literal witnesses and complete-command fields for four existing
corpus IDs. The original-command target joins the unfiltered Cargo batch; the
literal target uses a separate exact-test invocation sharing the same build.
Its three observer groups are checked separately, including on fixture-only changes.
Printer fixtures/targets/observers select only the printer job; the bundle sink
fixture/target selects only its ten commands in controls. Common printer source
changes retain all related acceptance and witness coverage. The printer runner
rejects zero-test successes as well as missing targets and nonzero exits.
A change to one SUPER fixture selects only its collection in the controls job.
Manual dispatch and common compiler/vendor/manifest or unknown changes select
full coverage. Documentation-only changes require no Rust build or replay.
The planner uses the complete Git diff, including both paths of a rename and
deletions, rather than GitHub's truncated path-filter list. Missing change ranges
select everything. Compiler tests imported by acceptance through `#[path]` remain
acceptance inputs: changing the shared declaration comparator selects late
acceptance and retained witnesses.

The aggregate checks reject planning failures, failed/cancelled replay jobs and
unexpectedly skipped selected jobs. A legitimate empty documentation selection
passes explicitly. Local full legacy CI is opt-in; focused checks and the relevant
hosted jobs are the ordinary edit/review path.

To rerun a failed group locally only when needed:

```sh
python3 .github/ci/replay.py acceptance late
# Or just the failing slice:
cargo xtask acceptance-slice h2-7c
```

A new witness target needs an explicit CI entry and ownership rule. Until that is
added, do not report it as covered by `cargo xtask acceptance` or these decorator
jobs. Unknown paths keep existing broad coverage but do not invent execution of a
new target.

## Hosted measurements and remaining coverage work

[PR #524](https://github.com/kazhiramatsu/tsc-rs/pull/524) landed as `bb2d51c89`.
Both aggregate checks passed. The previous run was
[PR #523 acceptance](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34962781518)
and its [witness workflow](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34962781338).

| Job | Before | After |
| --- | --- | --- |
| Acceptance | One serial job: 46m23s | early 11m50s, wide 28m03s, late 16m47s |
| Primary witnesses | 15m15s | 10m31s |
| Controls witnesses | 11m43s | 8m23s |
| Retained witnesses | 14m12s | 6m42s |

The [new acceptance run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34967807268)
retained all 31 slices. H2.5g alone still took about 22m40s to execute its 9,027
candidates; the longest complete job fell to 28m03s by separating other work.
The [new witness run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34967807283)
retained primary 670 exact × 2 plus two upstream exceptions, controls 408 exact × 2
plus direct 28 exact / 4 known, and retained 530 exact × 2.

These are observed job times including builds, job overhead and
hosted runner variation, not an isolated cache benchmark. Separate acceptance
builds increased aggregate runner time (the three jobs total 56m40s); the benefit
is timeout margin and independent retries. Collection selection and documentation
skipping reduce unnecessary execution on narrower changes.

For future additions, use 45 minutes per job as a split-review threshold while
keeping the 60-minute hard limit. Record build/oracle/replay time and total runner
minutes. Do not increase workers without measuring memory. New standalone
compiler witnesses and future build/watch/LSP suites still require their own
coverage inventory and explicit jobs; see OPS-COVER / OPS-BUDGET in the
[completion plan](design/greenfield/remaining-completion-slices.md).

[OPS-COVER-2 / PR #530](https://github.com/kazhiramatsu/tsc-rs/pull/530) added ten
emitter direct targets within the existing printer job: 2m17s for that job,
including 7.336s of new observer checks and 12.019s of new Cargo build/replay.
All seven replay jobs and both PR gates passed; the longest job was 27m36s.
The seven replay jobs total 87m04s, excluding plans/gates and main-push replay.

## Test entry coverage

The [PR-gate entry inventory](design/greenfield/slices/witness-coverage/README.md)
lists all 69 standalone Cargo test targets and 16 lib/bin test harnesses. It
separates unfiltered commands, named-test filters, and shared acceptance helpers.
A broad replay selected for an unknown test source does not automatically run
that source's standalone target. OPS-COVER-2 through 4 pair new owner commands
with target/fixture selection and a measured job budget. OPS-COVER-2 adds the ten
emitter direct targets: the remaining count is 42 standalone targets without a
direct entry at that step (compiler22 / other20). OPS-COVER-3A then registers the
eight CLI cases through a named-test filter: 41 targets still lack a direct entry
(compiler21 / other20). OPS-COVER-3B registers three compiler UTF-16 targets,
leaving 38 without a direct entry (compiler18 / other20). OPS-COVER-3C adds two
more, leaving 36 (compiler16 / other20). Earlier inventory
snapshots remain as history; entry configuration and successful hosted execution
are recorded separately in each slice report.


[OPS-COVER-3B / PR #532](https://github.com/kazhiramatsu/tsc-rs/pull/532) integrated
three compiler UTF-16 targets. All seven replay jobs and both gates passed.
Controls took 11m17s, including 52.206s of new observer checks and 88.968s of new
Cargo build/replay; all four new Rust tests passed (111 complete commands and
nine typed refusals, each twice). The longest job took 28m21s. PR replay jobs
total 90m32s, excluding plan/gates and main-push replay. The detailed
[receipt](design/greenfield/slices/witness-coverage/compiler-utf16/hosted.v1.json)
keeps the exact run/head identities and observed timing boundaries.

The merge's [main-push acceptance](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35040745901)
also passed. Its three replay jobs total 54m12s separately; PR plus main push
total 144m44s, still excluding planning/gates. The receipt retains both runs.

C04 adds the new `transpile_routes_contract` to controls (nine tests / 301 inputs).
Inventory v6 therefore has 65 standalone targets: 22 unfiltered, seven filtered,
and 36 without a direct entry. Existing acceptance membership is unchanged.

The C04 prototype landed in [PR #535](https://github.com/kazhiramatsu/tsc-rs/pull/535).
All seven replay jobs and both gates passed at `e86ed768f`; controls took 15m45s.
Inventory v7 records the pinned Node 25.2.1 setup for selections containing `transpile-routes`;
the original fixture bytes and the 22 known native differences are retained.

A-PC1 registers `compact-body-comments` (240 direct printer cases, one test)
and `parameter-temporaries` (12 original + 56 focused complete commands, two
tests). The printer and controls jobs own these targets respectively. Both
observers replay frozen expectations twice; the parameter entry clears inherited
case selectors and captures so `--all` cannot silently select a subset. Its
frozen selection under `docs/` is an executable input, explicitly matched before
the ordinary documentation skip. Inventory v8 records 66 standalone targets:
24 unfiltered, seven filtered, 35 without a direct entry (compiler15 / other20).

[A-PC1 / PR #538](design/greenfield/slices/h2-8a-compact-body-comments.md) passed
all seven PR replay jobs and both gates. New240 printer cases and all68 parameter
commands compare exactly twice. Printer took 1m58s, controls 16m45s, and the longest
job 27m32s; the seven jobs total 89m00s, excluding plans/gates and main-push runs.
The linked hosted receipt records the candidate/merge identities and timings.


C05 adds `resolution-cache` to controls: 26 change families / 112 generations /
197 requests, eleven contract tests and the complete 56-test Program library
(including eight cache unit tests). Its pinned-Node observer compares two fresh
observations with immutable expected bytes before Rust starts. The entry requires
both targets, exact nonzero counts, zero ignored and zero filtered tests.
Dedicated test/fixture/observer changes select this entry; shared Program source
still selects full related coverage. Inventory v10 has 67 standalone targets:
25 unfiltered, seven filtered, 35 without a direct entry, plus one of the 16
lib/bin harnesses directly registered. See the [integration receipt](design/greenfield/slices/l2-3-resolution-cache/integration/README.md).

[C05 / PR #539](design/greenfield/slices/l2-3-resolution-cache/integration/README.md)
passed all seven PR replay jobs and both gates. Controls took 831s; the new entry
took 0.363s for the observer and 6.128s for Cargo build/replay. The seven final jobs
total 5638s, excluding plans/gates/main push. The earlier cancelled candidate adds
2486s separately; it is retained as superseded evidence.

## Declaration command witnesses

OPS-COVER-3D registers `declaration-specifiers` (30 inputs),
`declaration-comments` (41) and `jsdoc-return` (58) in the controls job.
Use `scripts/witness.py <suite> --list` or `--all --dry-run` to inspect them;
`--all` runs the complete selected suite. Multiple exact test names form one
libtest selection per target, keeping imported helper tests out of these runs.
The runner clears internal filters and checks both passed and filtered counts.
See the [scope and validation record](design/greenfield/slices/witness-coverage/compiler-declarations/README.md)
for original-wrapper exclusions, observer boundaries and hosted results.

## Literal updates and require rewriting

`literal-update` checks the factory, transform and lifetime groups (1,396 IDs)
before running the emitter target in the printer job. `literal-update-pipeline`
checks 22 complete commands in controls. Both use the pinned Node version;
a shared observer change selects both suites. Generic inexpressible rows and
the five synthetic disposal differences remain separately recorded controls.

`require-rewrite` checks 74 dedicated complete commands with four exact test
names, keeping ten imported/original tests out of that invocation. Each
observer group is checked before Cargo; inherited internal filters and capture
settings are cleared. See the [combined integration record](design/greenfield/slices/h2-8a-literal-update/integration/README.md).

## Config/library and prologue witnesses

`config-library` checks twelve frozen observers before selecting exactly 24 tests
inside the compiler `contracts` binary. Its dedicated module and fixture changes
select this suite; the shared `contracts.rs` registration still selects broad
coverage. The shared declaration comparator and library snapshot retain late
acceptance and their previous witnesses, plus this suite. The 96 inputs contain
94 complete commands and two typed refusal controls; ordered Program facts are
checked for all 96, independently of command success.

`prologue-comments` runs the eight existing prologue-only comment controls,
comparing JS text, write count, exit and diagnostic codes twice. It does not
claim complete command-tuple coverage. Both suites share the controls build;
use `--list` or `--all --dry-run` before replay. See the
[combined scope and validation record](design/greenfield/slices/witness-coverage/compiler-config-prologue/README.md).


## Recovery census and map option witnesses

OPS-COVER-3H/3I adds two compiler suites to the controls job:

```sh
python3 scripts/witness.py utf16-recovery-corpus --all
python3 scripts/witness.py map-option-projection --all
```

The first replays 50 complete command comparisons twice. The runner verifies the
archived census hash and stages it at the historical path only while the frozen
observer runs. It preserves an identical pre-existing file, rejects a conflicting
file, and removes only its own staged file even when the observer fails.
The archive is historical selection evidence; no new parser census is implied.

The map suite selects three exact tests. Its 31 dedicated inputs use both qualified
and recorded adapters twice; five original IDs overlap existing acceptance.
Three originals also compare the old option floor with the map-family floor.
The ignored historical census remains unselected. The Rust test derives status
and exit fields from emit results, so this is not an additional CLI execution.
See the [slice record](design/greenfield/slices/witness-coverage/compiler-recovery-map/README.md)
for observations, input hashes, coverage and measured time.


## Bundle Program and declaration/map witnesses

OPS-COVER-3J/3K adds `bundle-program` and `bundle-declarations` to controls.
The first runs all four tests over 25 main sequences and two adjacent references.
The second selects three exact tests: 19 visitor inputs from 25 declaration rows,
20 map inputs, and 11 metadata/lifetime inputs in two modes. Its fourth original
JavaScript wrapper remains outside this selection. All comparisons repeat twice;
the ordinary-target reference and reused IDs remain separately classified. The
noEmit row compares its existing command result rather than an obsolete refusal.

Use `scripts/witness.py <suite> --list` or `--all --dry-run` to inspect the
27 and 56 fixture memberships. Supplemental sections have their own namespaces
and nonempty/unique/count checks. These totals include shared and reference rows,
not new corpus admissions. The frozen Node observers run before Cargo; selecting
both suites checks their shared declaration observer once. Changes to its fixture
or observer select both suites; map-only changes select the declaration suite.
Both selections install the pinned Node version even when selected alone.
See the [scope and validation record](design/greenfield/slices/witness-coverage/compiler-bundles/README.md).


[OPS-COVER-3J/3K / PR #546](design/greenfield/slices/witness-coverage/compiler-bundles/README.md)
passed all seven hosted replay jobs and both gates at `160161683`. Controls ran
18 compiler-direct suites / 64 tests, including all seven new bundle tests, in
37m51s. Its compiler-direct oracle took 440.889s and Cargo build/replay 1273.943s;
these are whole-group measurements. The seven jobs total 118m36s, excluding
plans, aggregate gates and main-push runs. Controls remains below the 45-minute
split-review threshold.


## Declaration map output and stateful API witnesses

OPS-COVER-3L/3M registers `declaration-maps` and `declaration-map-apis` in a
dedicated `declaration-maps` job. Their 6m57s local entry plus the previous
37m51s controls job left little margin below the 45-minute review threshold,
so the split adds one compiler build while retaining every suite. All eight
replay jobs and both gates are required.
Use `scripts/witness.py <suite> --list` or `--all --dry-run` to inspect the
84 and 75 fixture memberships, including shared and reference rows. `--all`
runs the selected complete target harness, or both in the selected compiler batch:
eight output tests and three stateful API tests. The shared API observer runs
once per batch; all four map observer modes and reference-path checks run
before Cargo. Both suites select the frozen Node version even on their own.
The shared reference-path fixture retains full acceptance/witness selection.
See the [scope and validation record](design/greenfield/slices/witness-coverage/compiler-declaration-maps/README.md).


[OPS-COVER-3L/3M / PR #547](design/greenfield/slices/witness-coverage/compiler-declaration-maps/README.md)
merged at `0e3852509` after all eight replay jobs and both gates passed at
`6cfd82ca2`; both Git trees are identical. The new declaration-maps job took
5m38s, including all 11 tests and 70 pristine CLI comparison pairs. Its six
observers took 35.504s and Cargo build/replay 267.979s. Controls retained all
18 compiler-direct suites / 64 tests and took 38m11s. The eight jobs total
121m55s, excluding plans, aggregate gates and main-push runs. The receipt records
whole-job timing and the additional build; it is not an isolated performance
comparison. Inventory v17 leaves 24 standalone targets without a direct entry
(compiler4 / other20).


## Generated binding integration

`decorator-binding` shares the printer job and checks 146 submitted direct inputs
plus ten failure-carry controls (204 route comparisons: 202 exact,
two typed `InvalidLifecycle` known divergences). Both direct observers run before
Cargo. `decorator-binding-pipeline` has its own 60-minute job and runs the shared
pipeline observer's `--check` before replaying 767 complete commands; one upstream
exception stays recorded separately. The frozen nine native divergences must
match on both passes and must be retired when exact. The runner checks requested
membership, `exact + known == selected`, and the selected known count, including
758 exact + 9 known for `--all`.

The shared observer selects both suites; dedicated fixtures select their owner.
Both jobs use `.node-version`. Capture/report/dump and inherited selection
variables are cleared. Local focused checks and admission status are recorded in
[the integration review](design/greenfield/slices/h2-8a-generated-binding/integration/revised/README.md).


## Module identities and original JavaScript bundle recorder

OPS-COVER-3N/3O adds two suites to the existing `controls` job. `module-identities` runs
three existing tests over 38 ordinary JavaScript inputs twice. Its catalog also
retains four API references and 14 path-helper references; the resulting 56
fixture memberships are not 56 native executions. All three frozen observers
run first. `bundle-original-javascript` runs the previously unselected exact
test over four original JavaScript declaration bundle inputs twice. Its new
observer repeats the four complete TypeScript commands against the unchanged
H2.7d/e ratchet; Rust keeps the existing internal recorder's narrower scope.

```sh
python3 scripts/witness.py module-identities --all --dry-run
python3 scripts/witness.py bundle-original-javascript --all --dry-run
```

The two suites reuse the controls compiler build, pinned Node and two workers.
Their measured 81-second local entry leaves planning margin below the
45-minute review threshold when added to the previous 38m11s controls run.
Shared declaration target changes select both its old three-test suite and the
new exact test. Shared module-identity fixtures and ratchets keep full replay.
See the [scope and validation record](design/greenfield/slices/witness-coverage/compiler-module-facets/README.md).

[PR #550](https://github.com/kazhiramatsu/tsc-rs/pull/550) passed all nine replay
jobs and both gates at `059519e99`. Controls executed all 20 compiler-direct
suite selections / 68 tests in 38m49s. Its oracle phase was 460.852s and Cargo
build/replay was 1299.146s; these cover the full compiler-direct batch. The
remaining 23 standalone targets without direct entries retain their explicit
owners. The receipt records hosted results; merge remains separate.


[T1 bundle metadata integration](design/greenfield/slices/h2-8a-bundle-metadata-t1/integration/README.md)
adds `bundle-metadata-t1` to controls with pinned Node even when selected alone.
Its 18 complete commands contain 15 exact and three frozen native divergences;
15 packet probes contain 12 exact and three frozen flag divergences. The runner
selects two exact test names, requires eight shared-helper tests to be filtered,
and clears inherited case/dump selectors. Its generator, observer and fixtures
select the same owner. Shared production changes retain full replay. T1 removes
five known rows from the unchanged 767-command binding collection: the expected
full summary is 763 exact / four known, with its one upstream exception separate.


## Post-T1 residual controls

[H2.8a-A-RES-POST-T1](design/greenfield/slices/h2-8a-post-t1-residuals/REPORT.md)
registers `post-t1-residuals` in the local runner: 101 complete commands
(anonymous default class names, System hoisted class end maps, private
compound-assignment receiver temps, private assignment right-operand
comments, decorator expression comment suppression) with the T1 parse-node
packet probe for its 79 bundle rows. Its generator, observer and frozen
observations are dedicated files; five printer-owned rows are frozen as
known native divergences. The suite needs the pinned Node version and is
not yet listed in `.github/workflows/witness.yml`; the integrator adds the
hosted entry (about 250 s of observer checks and 200 s of Cargo build and
replay locally) before treating it as covered.

```sh
python3 scripts/witness.py post-t1-residuals --all --dry-run
python3 scripts/witness.py post-t1-residuals --all
```
