# Performance

tsc-rs is a native executable that parses, binds and checks a program on
several threads and writes its output files in parallel. By default it runs
one checker thread per hardware thread, up to eight, and one and a half
times as many when it writes declaration files. The standard library
declarations are embedded, so a run has no JavaScript runtime start-up. The
measurements below compare its compile time, CPU time and memory use with
`tsgo`, the native TypeScript 7.1 compiler that tsc-rs follows, on real
projects. The earlier measurement of the tsc 6.0.3 compatible build, which
also compares TypeScript 6.0.3 (`tsc` on Node.js), is in
[performance-6.0.3.md](performance-6.0.3.md).

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
thread keeps its own type tables, and tsc-rs runs up to eight checkers by
default (twelve when writing declaration files) against tsgo's four. Where
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

## Content mappers

On the four projects of the [content mapper](../README.md#content-mappers)
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
