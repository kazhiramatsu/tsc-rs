# Performance

tsc-rs is a native executable that parses, binds and checks a program on
several threads and writes its output files in parallel. By default it runs
four checker threads, matching tsgo, with deterministic weighted import-graph
association. The historical measurements below used the checker counts of
the recorded binaries. The standard library
declarations are embedded, so a run has no JavaScript runtime start-up. The
measurements below compare its compile time, CPU time and memory use with
`tsgo`, the native TypeScript 7.1 compiler that tsc-rs follows, on real
projects. The earlier measurement of the tsc 6.0.3 compatible build, which
also compares TypeScript 6.0.3 (`tsc` on Node.js), is in
[performance-6.0.3.md](performance-6.0.3.md).

The newest numbers come first. The detailed tables after them (more output
modes, CPU time, output comparison) are from October 12, 2026 and an older,
slower build.

## Latest numbers

Type check (`--noEmit`) of eight open-source projects, November 28, 2026:
wall-clock time and peak memory (the peak memory footprint reported by
`/usr/bin/time -l`), median of 27 interleaved runs in three windows of 9 (the
order of the compilers rotated every round, one untimed run before each
project), on an Apple M5
(4 performance and 6 efficiency cores, 32 GiB, macOS 26.5.1) with no other
heavy job running. Every compiler ran `-p <config> --pretty false` with its
default settings.

| Project | tsgo | tsrs | tsc-rs |
| --- | ---: | ---: | ---: |
| VS Code `src` | 4.43 s · 6,225 MiB | 1.86 s · 2,281 MiB | 1.67 s · 2,181 MiB |
| Next.js `packages/next` | 1.28 s · 1,510 MiB | 0.48 s · 576 MiB | 0.44 s · 478 MiB |
| zod | 0.86 s · 1,600 MiB | 0.31 s · 580 MiB | 0.25 s · 463 MiB |
| Effect `packages/effect` | 0.75 s · 1,080 MiB | 0.30 s · 432 MiB | 0.27 s · 382 MiB |
| Playwright | 0.55 s · 905 MiB | 0.20 s · 352 MiB | 0.19 s · 346 MiB |
| Vue | 0.52 s · 661 MiB | 0.19 s · 231 MiB | 0.18 s · 228 MiB |
| TypeScript `src/compiler` | 0.35 s · 367 MiB | 0.14 s · 139 MiB | 0.14 s · 154 MiB |
| hono | 0.16 s · 261 MiB | 0.06 s · 109 MiB | 0.05 s · 103 MiB |

tsc-rs printed exactly `tsgo`'s output (diagnostics and exit status) on all
eight projects.

The opt-in modes that use more cores — `--fast` in tsrs, `TSRS_FAST=1` in
tsc-rs ([the fast preset](#the-fast-preset)):

| Project | tsrs `--fast` | tsc-rs `TSRS_FAST=1` |
| --- | ---: | ---: |
| VS Code `src` | 1.07 s · 1,836 MiB | 1.12 s · 2,149 MiB |
| Next.js `packages/next` | 0.26 s · 560 MiB | 0.25 s · 623 MiB |
| zod | 0.19 s · 503 MiB | 0.19 s · 559 MiB |
| Effect `packages/effect` | 0.20 s · 388 MiB | 0.17 s · 420 MiB |
| Playwright | 0.14 s · 350 MiB | 0.13 s · 419 MiB |
| Vue | 0.12 s · 252 MiB | 0.11 s · 295 MiB |
| TypeScript `src/compiler` | 0.12 s · 137 MiB | 0.12 s · 158 MiB |
| hono | 0.04 s · 95 MiB | 0.05 s · 103 MiB |

Compilers:

- tsc-rs: main at
  [`941cc28d5`](https://github.com/kazhiramatsu/tsc-rs/commit/941cc28d5),
  `cargo build --release`.
- `tsgo`: TypeScript 7.1 built from the commit tsc-rs follows (`aa814927`).
- [tsrs](https://github.com/oxc-project/tsrs) 7.1.0-beta.tsrs3, built from
  source with its release profile.
- No profile-guided optimization (PGO) on either Rust compiler: both are
  plain release builds. The binaries tsrs publishes are PGO builds (and
  BOLT-optimized on Linux); a PGO build of tsc-rs is not part of this
  comparison either.

The projects are those of [Measured projects](#measured-projects) below at
the same commits, plus [Playwright](https://github.com/microsoft/playwright)
(`ec31a7bf7`) and [Vue](https://github.com/vuejs/core) (`4ab865a84`), each
with its own `tsconfig.json` and `noEmit`.

## Measured projects

Each project was checked out at the commit shown, with its dependencies
installed, and compiled with its own configuration file (`tsconfig.json`
of the directory shown, or the file named). Derived configurations change
only the output: the output mode (`noEmit`; JavaScript; JavaScript and
declaration files; JavaScript and source maps; all outputs with
`declaration`, `declarationMap` and `sourceMap`), a separate output
directory for each mode that is kept out of the program's inputs (with
`rootDir` where needed), and `composite` and `incremental` turned off. The
TypeScript compiler's configurations also turn off `emitDeclarationOnly`
and `isolatedDeclarations`, which the project sets for its
declaration-only build. VS Code was measured with `--noEmit` only.

| Project | Commit | Source lines |
| --- | --- | ---: |
| [hono](https://github.com/honojs/hono) (`tsconfig.build.json`) | `8dcd52b` | 25,947 |
| [zod](https://github.com/colinhacks/zod) | `2bf7b06` | 117,060 |
| [TypeScript](https://github.com/microsoft/TypeScript) 6.0.3, `src/compiler` | `050880ce5` | 194,701 |
| [Next.js](https://github.com/vercel/next.js), `packages/next` | `1edced6f` | 314,268 |
| [Effect](https://github.com/Effect-TS/effect) 4.0.0-rc.118, `packages/effect` | `cbfc7b4` | 370,541 |
| [VS Code](https://github.com/microsoft/vscode), `src` | `29b68000` | 3,048,452 |

Source lines count the program's non-declaration TypeScript files. These
are the projects and configurations of the earlier measurement, without
Playwright.

## Method

The measurements were taken on October 12, 2026 on an Apple M5 (10 cores,
32 GiB, macOS 26.5.1), one project at a time with no other heavy job
running. Before timing, each compiler compiled every configuration once and
the results were compared (see [Output comparison](#output-comparison));
these runs also warmed the file cache. Then tsc-rs and tsgo ran in
interleaved rounds per configuration: seven for hono and zod, five for the
TypeScript compiler, Next.js and Effect, three for VS Code. Both received
the same command line, `-p <config> --pretty false`, at the default
scheduling priority, each under `/usr/bin/time -l`. The tables show
medians: wall-clock time in milliseconds, CPU time (user plus system) in
seconds, and peak memory, the maximum resident set size of the compiler
process, in MiB. Compilers:

- tsc-rs built from commit
  [`68625b636`](https://github.com/kazhiramatsu/tsc-rs/commit/68625b6360dace0d793ef5df6069e074fd9e46f1)
  (October 12, 2026) with `cargo build --release` (Rust 1.93.0), with its
  default checker count.
- tsgo 7.1.0-dev, an unmodified build of
  [TypeScript commit `aa814927`](https://github.com/microsoft/TypeScript/commit/aa8149273b23401f0a79a5f0384c42de51888693),
  the commit tsc-rs follows, with Go 1.27.1, using its default of four
  checkers.

Later changes made tsc-rs's program loading faster (pull requests #760
and #761); these numbers do not include them.

## Compile time

Type check only (`--noEmit`):

| Project | tsc-rs | tsgo | tsc-rs ÷ tsgo | tsc-rs CPU | tsgo CPU |
| --- | ---: | ---: | ---: | ---: | ---: |
| hono | 124 | 150 | 0.83 | 0.57 | 0.62 |
| zod | 512 | 814 | 0.63 | 3.39 | 4.26 |
| TypeScript `src/compiler` | 317 | 343 | 0.93 | 0.94 | 1.14 |
| Next.js `packages/next` | 715 | 1,240 | 0.58 | 4.22 | 4.97 |
| Effect `packages/effect` | 469 | 721 | 0.65 | 3.09 | 3.39 |
| VS Code `src` | 3,529 | 4,518 | 0.78 | 24.52 | 22.20 |

Compilation with output files:

| Project | Output | tsc-rs | tsgo | tsc-rs ÷ tsgo | tsc-rs CPU | tsgo CPU |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| hono | JavaScript | 135 | 161 | 0.83 | 0.64 | 0.69 |
| hono | JavaScript + declarations | 140 | 183 | 0.77 | 0.73 | 0.85 |
| hono | JavaScript + source maps | 136 | 166 | 0.82 | 0.66 | 0.73 |
| hono | All outputs | 148 | 191 | 0.77 | 0.76 | 0.94 |
| zod | JavaScript | 569 | 896 | 0.64 | 3.90 | 5.02 |
| zod | JavaScript + declarations | 607 | 952 | 0.64 | 4.32 | 5.44 |
| zod | JavaScript + source maps | 577 | 928 | 0.62 | 3.96 | 5.33 |
| zod | All outputs | 620 | 995 | 0.62 | 4.46 | 5.63 |
| TypeScript `src/compiler` | JavaScript | 459 | 503 | 0.91 | 1.37 | 1.80 |
| TypeScript `src/compiler` | JavaScript + declarations | 465 | 547 | 0.85 | 1.43 | 2.09 |
| TypeScript `src/compiler` | JavaScript + source maps | 473 | 561 | 0.84 | 1.41 | 2.02 |
| TypeScript `src/compiler` | All outputs | 506 | 610 | 0.83 | 1.54 | 2.30 |
| Next.js `packages/next` | JavaScript | 821 | 1,370 | 0.60 | 5.25 | 5.98 |
| Next.js `packages/next` | JavaScript + declarations | 950 | 1,523 | 0.62 | 6.46 | 7.29 |
| Next.js `packages/next` | JavaScript + source maps | 879 | 1,458 | 0.60 | 5.71 | 6.50 |
| Next.js `packages/next` | All outputs | 1,010 | 1,650 | 0.61 | 7.04 | 8.22 |
| Effect `packages/effect` | JavaScript | 583 | 789 | 0.74 | 3.75 | 4.04 |
| Effect `packages/effect` | JavaScript + declarations | 712 | 1,016 | 0.70 | 4.78 | 5.56 |
| Effect `packages/effect` | JavaScript + source maps | 545 | 863 | 0.63 | 3.90 | 4.42 |
| Effect `packages/effect` | All outputs | 758 | 1,097 | 0.69 | 4.93 | 6.04 |

A value below 1 in the `tsc-rs ÷ tsgo` column means tsc-rs finished
first. tsc-rs was faster than tsgo on all 26 configurations, taking 0.58
to 0.93 of tsgo's time. The type check of the TypeScript compiler is the
closest case: its critical path is the check of one 54,000-line file.
tsc-rs used 0.67 to 0.93 of tsgo's CPU time, except on VS Code, where it
used 1.10 times as much while finishing in 0.78 of the time. Differences
of a few percent are within the run-to-run variation observed on this
machine.

## Peak memory

Type check only (`--noEmit`), in MiB:

| Project | tsc-rs | tsgo | tsc-rs ÷ tsgo |
| --- | ---: | ---: | ---: |
| hono | 304 | 276 | 1.10 |
| zod | 1,323 | 1,606 | 0.82 |
| TypeScript `src/compiler` | 290 | 379 | 0.77 |
| Next.js `packages/next` | 1,290 | 1,535 | 0.84 |
| Effect `packages/effect` | 1,016 | 1,091 | 0.93 |
| VS Code `src` | 5,499 | 6,369 | 0.86 |

Compilation with output files, in MiB:

| Project | Output | tsc-rs | tsgo | tsc-rs ÷ tsgo |
| --- | --- | ---: | ---: | ---: |
| hono | JavaScript | 318 | 297 | 1.07 |
| hono | JavaScript + declarations | 333 | 325 | 1.03 |
| hono | JavaScript + source maps | 322 | 303 | 1.06 |
| hono | All outputs | 338 | 334 | 1.01 |
| zod | JavaScript | 1,357 | 1,580 | 0.86 |
| zod | JavaScript + declarations | 1,513 | 1,702 | 0.89 |
| zod | JavaScript + source maps | 1,361 | 1,682 | 0.81 |
| zod | All outputs | 1,532 | 1,834 | 0.84 |
| TypeScript `src/compiler` | JavaScript | 451 | 553 | 0.82 |
| TypeScript `src/compiler` | JavaScript + declarations | 473 | 639 | 0.74 |
| TypeScript `src/compiler` | JavaScript + source maps | 444 | 597 | 0.74 |
| TypeScript `src/compiler` | All outputs | 475 | 640 | 0.74 |
| Next.js `packages/next` | JavaScript | 1,316 | 1,590 | 0.83 |
| Next.js `packages/next` | JavaScript + declarations | 1,436 | 1,911 | 0.75 |
| Next.js `packages/next` | JavaScript + source maps | 1,325 | 1,652 | 0.80 |
| Next.js `packages/next` | All outputs | 1,443 | 1,994 | 0.72 |
| Effect `packages/effect` | JavaScript | 1,116 | 1,182 | 0.94 |
| Effect `packages/effect` | JavaScript + declarations | 1,161 | 1,602 | 0.72 |
| Effect `packages/effect` | JavaScript + source maps | 1,116 | 1,232 | 0.91 |
| Effect `packages/effect` | All outputs | 1,168 | 1,640 | 0.71 |

tsc-rs used 0.71 to 1.10 times as much memory as tsgo: less on every
project but hono, where it used 1.01 to 1.10 times as much. Each checker
thread keeps its own type tables, and the recorded tsc-rs binary ran up to
eight checkers by default (twelve when writing declaration files) against
tsgo's four. Where
memory matters more than speed, lower `--checkers`.

## Output comparison

Before the timing runs, every configuration was compiled once with each
compiler, and the exit statuses, diagnostics and emitted file trees were
compared byte for byte.

| Project | Diagnostics | Emitted files |
| --- | --- | --- |
| hono | identical | identical |
| zod | depend on the checker partition (below) | identical |
| TypeScript `src/compiler` | identical | identical |
| Next.js `packages/next` | identical | identical |
| Effect `packages/effect` | identical | 5 of 992 declaration files, 7 of 1,984 with all outputs (fixed since) |
| VS Code `src` | identical | — |

Exit statuses were identical everywhere. zod's diagnostics depend on how
the files are partitioned among the checkers, in tsgo too: its default
run and its single-threaded run (`--singleThreaded`) report different
TS5115 and TS2344 errors. With one checker on both sides (`tsgo
--singleThreaded` and `TSRS_CHECKERS=1`), the diagnostics were identical.
Effect's declaration files differed in two ways, both fixed after the
measurement (pull request #758): the order of a conditional type's `infer`
type parameters, and the reuse of a type serialized for an earlier
declaration.

## The fast preset

By default tsc-rs checks with four checkers and associates files with them
as tsgo does, and its diagnostics are tsgo's byte for byte (for emitted
files see the [known differences](compatibility.md#limitations)). More
checkers under that association repeat much of each other's work, so more
cores buy little.
Setting `TSRS_FAST=1` in the environment (it is not a command-line option)
selects a preset for machines with more cores:

- the number of checkers follows the machine and the program: one per core
  up to sixteen cores (beyond that the larger of sixteen and half the cores,
  at most 32), never more than one per 40 type-checked files, and at least
  four (`--checkers` and `TSRS_CHECKERS` still set it);
- files are associated with checkers by directory and by the modules they
  import, so that the files of one checker resolve largely the same
  declarations;
- every checker starts with its heaviest files, and a checker that has run
  out of files takes unstarted ones from the checker with the most work left.
  Heavy counts what a file would cost another checker: the modules it imports
  that no other checker holds. A small file of a large family is then checked
  early by the checker that holds the family, and what is left at a
  checker's end, where the others take, costs a taker least. A taker builds
  the types of the region it takes from for itself, so with eight to fifteen
  checkers one checker at a time takes from another, and it stays with that
  checker while there is a file left to take;
- a declaration file heavy enough to hold its checker back is checked in
  ranges of statements on several checkers;
- when the command only checks (`--noEmit`, one run), a TypeScript module
  that no other file refers to — nothing imports or references it, and it
  re-exports nothing and declares no namespace, global or module
  augmentation — gives its syntax tree and symbol tables back to the system
  soon after it is checked, in a few large batches while the other files are
  still being checked. In a project with many such files (tests, entry
  points) the run needs less memory at its peak.

Every file is checked exactly once. The pool's size follows the machine, and
which checker takes a file is decided by time, so the preset does not promise
the same output on every machine or in every run. A file whose diagnostics do
not depend on what its checker checked before reports what the default
reports; a program whose diagnostics depend on the checkers' history can
print differently from run to run (tsgo itself reports such programs
differently at different checker counts).
Four rules make the commonest such answers a matter of the program alone
(when a type argument's constraint is checked, where the count of
instantiations of one check ends, which declaration a widened object
literal's filled-in property carries, and at which declaration a merged
interface is checked). Of the eight projects below seven printed the
default's output in every one of twelve runs (VS Code: four); zod printed
two different outputs in twelve runs (38 errors eleven times, 37 once; the
default reports 35, tsrs `--fast` 36).

Its numbers beside the default and tsrs: [Latest numbers](#latest-numbers).

The preset uses more memory: every checker keeps its own type tables, and
because the checkers level their work all of them are alive at the end of
the run (zod: 522 MiB by default, 630 with the preset). VS Code, a fifth
of whose files nothing refers to, peaks at 2,322 MiB by default and at
2,317 MiB with the preset when the command only checks (2,494 before those
files' records were given back).

## The lane profile

The default pool deals the files to its four checkers by their sizes, as
tsgo does, and a file's size says little about what its check costs: on the
projects above the longest-working checker works 1.1 to 1.6 times the mean,
and the run waits for it. Setting `TSRS_LANE_PROFILE` in the environment (it
is not a command-line option) to a file name makes a command record what the
check of every file cost its checker — counts of the checker's work, not
times — and a later run of the same program levels its checkers by that
record: the files are dealt as before, and files then leave the costliest
checker while it stands out.

```sh
TSRS_LANE_PROFILE=.tsc-rs-lanes tsc-rs --noEmit -p .   # the first run records
TSRS_LANE_PROFILE=.tsc-rs-lanes tsc-rs --noEmit -p .   # later runs are levelled
```

- The variable names the record's file. If it names an existing directory,
  the record is one file per project in it, named after the configuration
  file's path.
- The record is text and the same on every machine and in every run of a
  program. An entry is used while its file's text keeps its length and is
  recorded anew when it changes; a file without an entry weighs its size.
- Only a one-shot command keeps a record: not `--watch`, not `-b`. The
  variable has no effect together with `TSRS_FAST=1`.
- Without the variable nothing is read or written, and the dealing is the
  default's.

The levelling stays inside a budget for the pool's peak memory: a checker
that ends returns its tables while the others still check, so a pool whose
checkers all end together holds more at once, and a file is moved only where
that does not raise the default dealing's own budget. The budget is counted
in the record's cost units, not in bytes: measured, the peak rose by 33 MiB
on Next.js and by less elsewhere.

Type check (`--noEmit`) with a record against the default, November 19,
2026, on the machine of [Latest numbers](#latest-numbers): wall-clock time
(median of the per-round ratios of 9 interleaved runs) and the change of
the median peak memory.

| Project | Wall-clock time | Peak memory |
| --- | ---: | ---: |
| VS Code `src` | 0.0 % | +1 MiB |
| Next.js `packages/next` | −11.9 % | +33 MiB |
| zod | +1.9 % | −8 MiB |
| Effect `packages/effect` | −7.3 % | −7 MiB |
| Playwright | −6.0 % | +2 MiB |
| Vue | −8.6 % | +11 MiB |
| TypeScript `src/compiler` | −13.8 % | −1 MiB |
| hono | −17.0 % | +5 MiB |

VS Code and zod keep the default dealing with their records (levelling them
would raise the pool's peak), so their rows show the measurement's own
variation: about 2 % in time and 10 MiB in memory between two runs of the
same compiler.

A program and its record give the same output in every run. That output is
the default's except where a diagnostic depends on which checker saw a file
first; tsgo itself reports such programs differently at different checker
counts (zod is one: see [Output comparison](#output-comparison)). On these
eight projects the diagnostics were the default's with and without a record.

## Content mappers

On the four projects of the [content mapper](usage.md#content-mappers)
comparison, `-p <config> --runExternalCode --noEmit --pretty false`, median
wall-clock time of seven runs after one warm-up, measured for pull request
#766 on October 12, 2026 (the same machine and tsgo build):

| Project | Mapped files | tsc-rs | tsgo | tsc-rs ÷ tsgo |
| --- | ---: | ---: | ---: | ---: |
| [vinhphm/site](https://github.com/vinhphm/site) (Astro) | 35 | 175 | 199 | 0.88 |
| [withastro/starlight](https://github.com/withastro/starlight) (Astro) | 99 | 427 | 450 | 0.95 |
| [antfu-collective/vitesse](https://github.com/antfu-collective/vitesse) (Vue) | 10 | 128 | 98 | 1.31 |
| [soybeanjs/soybean-admin](https://github.com/soybeanjs/soybean-admin) (Vue) | 90 | 249 | 234 | 1.06 |

The times include the mappers' own work. vitesse shows a small project's
fixed cost: without its mapper, tsc-rs takes 92 ms where tsgo takes 71.

## Reproducing

The harness of the main measurement, `bench.py`, and its raw samples,
`results.jsonl`, are kept with the
[performance notes](design/greenfield/slices/ts71-perf-2026-10/README.md)
(the content mapper timing script with the
[content mapper notes](design/greenfield/slices/ts71-content-mapper/README.md));
their paths are those of the measuring machine. These are quick
interleaved runs on one machine, not the project's formal protocol. For
repeatable measurements with recorded provenance, exact output
verification before timing, several sessions and confidence intervals,
see [benchmarking.md](benchmarking.md) and `scripts/benchmark-cli.py`.
Watch mode, incremental rebuilds and `--build` were not timed, and
cold-cache, Linux and Windows timings were not measured.
