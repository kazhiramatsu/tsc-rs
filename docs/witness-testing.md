# Focused witnesses and hosted replay

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
lists all 66 standalone Cargo test targets and 16 lib/bin test harnesses. It
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
