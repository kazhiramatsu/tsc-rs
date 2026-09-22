# Comparing CLI performance

Build each compiler before measuring, then run the benchmark separately from
builds, tests and profilers. `scripts/benchmark-cli.py` uses Python's standard
library on macOS or Linux; it adds no compiler dependency.

Provide a JSON plan with absolute paths to fixed binaries and input directories:

```json
{
  "variants": {
    "before": {"command": ["/absolute/baseline/tsc-rs"]},
    "after": {"command": ["/absolute/candidate/tsc-rs"]},
    "tsgo": {
      "command": ["/absolute/tsgo"],
      "env": {"GOMAXPROCS": null}
    },
    "oracle": {"command": ["node", "/absolute/typescript/lib/_tsc.js"]}
  },
  "comparison": ["before", "after", "tsgo"],
  "pair": ["before", "after"],
  "oracle": "oracle",
  "cases": [
    {
      "id": "project-noemit",
      "cwd": "/absolute/frozen-project",
      "args": ["--pretty", "false", "--noEmit", "-p", "tsconfig.json"],
      "expected_exit": 0
    }
  ],
  "extra_inputs": ["/absolute/shared-standard-libraries"],
  "sessions": 3,
  "rounds": 30,
  "warmups": 2,
  "seed": 20260922,
  "timeout_seconds": 120,
  "provenance": {
    "before_commit": "record the exact source commit",
    "after_commit": "record the exact source commit",
    "toolchains_and_build_flags": "record versions and flags for each binary"
  }
}
```

```sh
python3 scripts/benchmark-cli.py /absolute/plan.json target/benchmarks/new-run
```

The output directory must be new. For emit workloads, add `"output_dir": "out"`
to the case and configure the compiler to emit there. **That directory is
deleted before every invocation**; use a dedicated disposable project copy.
It must be a direct child of `cwd`. Include every external library, config and
dependency directory in `extra_inputs` so their hashes are recorded and checked.
Each executable in `command[0]` is hashed; scripts passed as later arguments
must also be covered by the input directories.

Use identical source files, explicit standard libraries, target/module options,
and `skipLibCheck` settings for all compilers. An explicitly supplied library
set with `noLib: true` prevents implicit library differences; it does not mean
checking without standard types. Preserve source commits, dependency lockfiles,
toolchain versions and build flags alongside the plan. `provenance` is recorded
as supplied, rather than independently verified.

Before timing, the runner obtains fresh oracle results and requires every
compiler to match the exit code, stdout, stderr and emitted file hashes exactly.
Intentional diagnostic cases can set a nonzero `expected_exit`. Mismatches and
timeouts remain in the log and stop the run; they are not silently excluded.
CLI comparison does not cover the Rust API's suggestion diagnostics, so API
regression tests remain necessary.

Each session shuffles the case order with a recorded seed. For three compilers,
all six execution orders occur equally often; 30 rounds gives five of each.
Larger comparisons use rotations in both directions, with equal position
counts. The round count must be divisible by the number of distinct orders.
Sessions are consecutive runs on one machine, not independent machines or days.
The warmup count applies to each compiler in each case and session.

`samples.jsonl` contains every invocation, including preflight and warmups.
The runner spawns each CLI directly, measures wall time through completion,
and collects that child's CPU time and peak RSS with `wait4`. Hashing, output
cleanup and log processing occur outside the timed interval. A timeout kills
the child's process group. A separate `/usr/bin/true` probe records the harness
and process-startup floor, which is never subtracted from compiler times.

`summary.json` reports each session separately: median, quartiles, MAD, range,
CPU time and peak RSS. If `pair` is set, it also reports the median paired wall
time ratio and a 95% percentile bootstrap interval over rounds. This interval
describes that session's samples; it does not account for systematic machine,
cache, temperature or workload selection effects. Keep noisy and short cases
visible, and avoid claiming general application performance from synthetic
modules alone. Incremental, watch and cold-cache performance require separate
protocols.

For a pinned tsgo build supporting `--checkers`, compare `--checkers 1`, `2`,
`4` and `8` on representative multi-file inputs. Record `GOMAXPROCS` separately:
it limits Go CPU parallelism, not the number of checkers. A
`--singleThreaded` run with `GOMAXPROCS=1` is a useful serial control. Keep exact
diagnostic/output comparisons enabled for every setting.

The experimental low-level Rust API now stores `CheckerState::diagnostics` in
`DiagnosticSink`, preserving insertion order and returned indices. Reading and
mutating it through Vec methods is supported; unrestricted mutation invalidates
the lookup index. To assign an existing `Vec<Diagnostic>`, use `.into()`; to take
out the underlying vector, use `.into_vec()`. The higher-level check results
continue to expose diagnostic vectors. This API is still being developed.
