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
```

Suites: `primary`, `extra`, `followup`, `followup2`, `followup3`, `retained`, `direct`,
`printer`, `bundle-sinks`.
`--all` explicitly requests the whole suite, normally on hosted CI. `--list` and
`--dry-run` neither build Rust nor run tests. Commands use a manifest path and an
exact test name, avoiding the unrelated comparator tests compiled into that target.
They also avoid building the dev-profile xtask executable before the test profile.
The `printer` target runs together: 70 small direct rows (65 exact, 5 documented
gaps), plus the same target's probe/safety/negative controls. It takes milliseconds
after compilation and does not run the Program/oracle chain. `bundle-sinks` runs
10 complete commands together; normally leave that replay to hosted CI.
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
A separate `printer` job runs its two observers, seven focused emitter targets
and one exact noEmitOnError owner control with a 20-minute limit and two workers.
The controls job also runs `bundle-sinks`, sharing its existing compiler build.
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

## Test entry coverage

The [PR-gate entry inventory](design/greenfield/slices/witness-coverage/README.md)
lists all 64 standalone Cargo test targets and 16 lib/bin test harnesses. It
separates unfiltered commands, named-test filters, and shared acceptance helpers.
A broad replay selected for an unknown test source does not automatically run
that source's standalone target. OPS-COVER-2 through 4 pair new owner commands
with target/fixture selection and a measured job budget.
