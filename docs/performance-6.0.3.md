# Performance

> **Note:** these measurements were taken on October 1, 2026 with the
> tsc 6.0.3 compatible build of tsc-rs, before the switch to TypeScript 7.1.
> They will be measured again with the TypeScript 7.1 build.

tsc-rs is a native executable that parses, binds and checks a program on
several threads and writes its output files in parallel. By default it runs
one checker thread per hardware thread, up to eight, and one and a half
times as many when it writes declaration files. The standard library
declarations are embedded, so a run has no JavaScript runtime start-up. The
measurements below compare its compile time and memory use with TypeScript
6.0.3 (`tsc`, running on Node.js) and with the native TypeScript 7 preview
compiler (`tsgo`) on real projects.

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

| Project | Commit | Program files | Source lines |
| --- | --- | ---: | ---: |
| [hono](https://github.com/honojs/hono) (`tsconfig.build.json`) | `8dcd52b` | 362 | 25,947 |
| [zod](https://github.com/colinhacks/zod) | `2bf7b06` | 2,364 | 117,060 |
| [Playwright](https://github.com/microsoft/playwright) | `ec31a7b` | 1,505 | 154,645 |
| [TypeScript](https://github.com/microsoft/TypeScript) 6.0.3, `src/compiler` | `050880ce5` | 249 | 194,701 |
| [Next.js](https://github.com/vercel/next.js), `packages/next` | `1edced6f` | 2,866 | 314,268 |
| [Effect](https://github.com/Effect-TS/effect) 4.0.0-rc.118, `packages/effect` | `cbfc7b4` | 689 | 370,541 |
| [VS Code](https://github.com/microsoft/vscode), `src` | `29b68000` | 10,272 | 3,048,452 |

Program files count every file in the program as listed by
`tsc --listFiles`, including standard library and `node_modules`
declaration files. Source lines count the program's non-declaration
TypeScript files. zod, Playwright, Next.js and VS Code report type errors
at these commits with the configurations used; all compilers report them.
Effect, whose library code relies heavily on type-level computation, is
error-free under tsc; see the [output comparison](#output-comparison) for
what tsc-rs reports on it.

## Method

The measurements were taken on October 1, 2026 on an Apple M5 (10 cores,
32 GiB, macOS 26.5.1) running on AC power, with a warm file cache and a
desktop session in the background (load average about 3 at the start).
After one warm-up run of every configuration, tsc-rs and tsgo ran in seven
interleaved rounds per configuration and tsc in three (two for VS Code).
The same rounds also ran tsc-rs with four checkers, described under
[peak memory](#peak-memory). All compilers received the same command
line, `--pretty false -p <config>` with `--noEmit` added for VS Code, at
the same scheduling priority (`nice -n 20`). The tables show medians:
wall-clock time in milliseconds, and peak memory as the maximum resident
set size of the compiler process reported by `wait4`, in MiB.
Compilers:

- tsc-rs built from commit
  [`8761d2de6`](https://github.com/kazhiramatsu/tsc-rs/commit/8761d2de6dc483b39ed1b519f060b7eefa17616f)
  with `cargo build --release --locked` (Rust 1.93.0).
- tsgo 7.1.0-dev, an unmodified build of
  [TypeScript commit `1f70213d`](https://github.com/microsoft/TypeScript/commit/1f70213d4922b434345f639b441681e470c7cfc1)
  (September 4, 2026) with Go 1.26.0, using its default of four checkers.
- tsc 6.0.3 on Node.js 25.2.1 with `--max-old-space-size=8192`.

## Compile time

Type check only (`--noEmit`):

| Project | tsc-rs | tsgo | tsc | tsc-rs ÷ tsgo | tsc ÷ tsc-rs |
| --- | ---: | ---: | ---: | ---: | ---: |
| hono | 118 | 161 | 959 | 0.74 | 8.1 |
| zod | 512 | 946 | 5,380 | 0.54 | 10.5 |
| Playwright | 355 | 617 | 4,424 | 0.58 | 12.5 |
| TypeScript `src/compiler` | 335 | 359 | 2,732 | 0.93 | 8.2 |
| Next.js `packages/next` | 798 | 1,419 | 7,949 | 0.56 | 10.0 |
| Effect `packages/effect` | 502 | 791 | 5,417 | 0.63 | 10.8 |
| VS Code `src` | 3,427 | 4,763 | 42,891 | 0.72 | 12.5 |

Compilation with output files:

| Project | Output | tsc-rs | tsgo | tsc | tsc-rs ÷ tsgo | tsc ÷ tsc-rs |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| hono | JavaScript | 129 | 176 | 1,040 | 0.73 | 8.1 |
| hono | JavaScript + declarations | 134 | 187 | 1,118 | 0.72 | 8.3 |
| hono | JavaScript + source maps | 133 | 181 | 1,071 | 0.73 | 8.1 |
| hono | All outputs | 146 | 206 | 1,186 | 0.71 | 8.1 |
| zod | JavaScript | 568 | 978 | 5,609 | 0.58 | 9.9 |
| zod | JavaScript + declarations | 599 | 1,045 | 5,740 | 0.57 | 9.6 |
| zod | JavaScript + source maps | 564 | 1,013 | 5,793 | 0.56 | 10.3 |
| zod | All outputs | 622 | 1,075 | 5,990 | 0.58 | 9.6 |
| Playwright | JavaScript | 411 | 682 | 4,623 | 0.60 | 11.2 |
| Playwright | JavaScript + declarations | 446 | 741 | 4,858 | 0.60 | 10.9 |
| Playwright | JavaScript + source maps | 420 | 684 | 4,779 | 0.61 | 11.4 |
| Playwright | All outputs | 487 | 789 | 5,178 | 0.62 | 10.6 |
| TypeScript `src/compiler` | JavaScript | 480 | 543 | 3,420 | 0.88 | 7.1 |
| TypeScript `src/compiler` | JavaScript + declarations | 478 | 597 | 3,482 | 0.80 | 7.3 |
| TypeScript `src/compiler` | JavaScript + source maps | 511 | 609 | 3,580 | 0.84 | 7.0 |
| TypeScript `src/compiler` | All outputs | 508 | 645 | 3,683 | 0.79 | 7.2 |
| Next.js `packages/next` | JavaScript | 848 | 1,461 | 8,744 | 0.58 | 10.3 |
| Next.js `packages/next` | JavaScript + declarations | 983 | 1,681 | 9,366 | 0.58 | 9.5 |
| Next.js `packages/next` | JavaScript + source maps | 894 | 1,600 | 9,319 | 0.56 | 10.4 |
| Next.js `packages/next` | All outputs | 1,022 | 1,693 | 9,510 | 0.60 | 9.3 |
| Effect `packages/effect` | JavaScript | 560 | 826 | 5,759 | 0.68 | 10.3 |
| Effect `packages/effect` | JavaScript + declarations | 755 | 1,092 | 6,909 | 0.69 | 9.2 |
| Effect `packages/effect` | JavaScript + source maps | 583 | 891 | 6,066 | 0.65 | 10.4 |
| Effect `packages/effect` | All outputs | 786 | 1,165 | 7,252 | 0.67 | 9.2 |

A value below 1 in the `tsc-rs ÷ tsgo` column means tsc-rs finished
first; the last column is the speed-up over tsc. tsc-rs was faster than
tsgo on all 31 configurations, taking 0.54 to 0.93 of tsgo's time, and 7
to 12.5 times faster than tsc. The type check of the TypeScript compiler is
the closest case: its critical path is the check of one 54,000-line file.
Differences of a few percent are within the run-to-run variation observed
on this machine.

## Peak memory

Type check only (`--noEmit`), in MiB:

| Project | tsc-rs | tsgo | tsc | tsc-rs ÷ tsgo | tsc-rs ÷ tsc |
| --- | ---: | ---: | ---: | ---: | ---: |
| hono | 306 | 326 | 520 | 0.94 | 0.59 |
| zod | 1,310 | 1,774 | 2,069 | 0.74 | 0.63 |
| Playwright | 754 | 1,041 | 1,299 | 0.72 | 0.58 |
| TypeScript `src/compiler` | 291 | 404 | 710 | 0.72 | 0.41 |
| Next.js `packages/next` | 1,341 | 1,710 | 2,381 | 0.78 | 0.56 |
| Effect `packages/effect` | 1,034 | 1,209 | 1,657 | 0.86 | 0.62 |
| VS Code `src` | 5,475 | 6,236 | 7,386 | 0.88 | 0.74 |

Compilation with output files, in MiB:

| Project | Output | tsc-rs | tsgo | tsc | tsc-rs ÷ tsgo | tsc-rs ÷ tsc |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| hono | JavaScript | 299 | 350 | 536 | 0.86 | 0.56 |
| hono | JavaScript + declarations | 352 | 375 | 551 | 0.94 | 0.64 |
| hono | JavaScript + source maps | 310 | 356 | 541 | 0.87 | 0.57 |
| hono | All outputs | 349 | 371 | 553 | 0.94 | 0.63 |
| zod | JavaScript | 1,343 | 1,772 | 2,111 | 0.76 | 0.64 |
| zod | JavaScript + declarations | 1,530 | 1,952 | 2,130 | 0.78 | 0.72 |
| zod | JavaScript + source maps | 1,350 | 1,823 | 2,118 | 0.74 | 0.64 |
| zod | All outputs | 1,540 | 1,903 | 2,144 | 0.81 | 0.72 |
| Playwright | JavaScript | 800 | 1,153 | 1,366 | 0.69 | 0.59 |
| Playwright | JavaScript + declarations | 899 | 1,329 | 1,371 | 0.68 | 0.66 |
| Playwright | JavaScript + source maps | 811 | 1,195 | 1,359 | 0.68 | 0.60 |
| Playwright | All outputs | 903 | 1,376 | 1,370 | 0.66 | 0.66 |
| TypeScript `src/compiler` | JavaScript | 457 | 578 | 758 | 0.79 | 0.60 |
| TypeScript `src/compiler` | JavaScript + declarations | 475 | 661 | 768 | 0.72 | 0.62 |
| TypeScript `src/compiler` | JavaScript + source maps | 457 | 598 | 773 | 0.76 | 0.59 |
| TypeScript `src/compiler` | All outputs | 475 | 667 | 789 | 0.71 | 0.60 |
| Next.js `packages/next` | JavaScript | 1,376 | 1,793 | 2,497 | 0.77 | 0.55 |
| Next.js `packages/next` | JavaScript + declarations | 1,450 | 1,977 | 2,630 | 0.73 | 0.55 |
| Next.js `packages/next` | JavaScript + source maps | 1,380 | 1,838 | 2,600 | 0.75 | 0.53 |
| Next.js `packages/next` | All outputs | 1,464 | 2,048 | 2,547 | 0.71 | 0.57 |
| Effect `packages/effect` | JavaScript | 1,119 | 1,291 | 1,724 | 0.87 | 0.65 |
| Effect `packages/effect` | JavaScript + declarations | 1,322 | 1,731 | 1,891 | 0.76 | 0.70 |
| Effect `packages/effect` | JavaScript + source maps | 1,120 | 1,330 | 1,724 | 0.84 | 0.65 |
| Effect `packages/effect` | All outputs | 1,338 | 1,792 | 1,905 | 0.75 | 0.70 |

A value below 1 in the ratio columns means tsc-rs used less memory. tsc-rs
used 0.66 to 0.94 times as much memory as tsgo and 0.41 to 0.74 times as
much as tsc, less than both on every configuration, although each checker
thread keeps its own type tables and tsc-rs ran eight checkers here
(twelve when writing declaration files) against tsgo's default of four,
except for the TypeScript compiler, where one file dominates the check and
fewer checkers are used. With `TSRS_CHECKERS=4` in the environment, tsc-rs
used 0.51 to 0.82 times tsgo's memory in 0.62 to 1.06 of its time,
finishing first on every configuration but Effect's two declaration
builds. Where memory matters more than speed, set `TSRS_CHECKERS` to a
lower count.

## Output comparison

Before the timing runs, every configuration was compiled once with tsc and
once with the measured tsc-rs build, and the diagnostics, exit statuses and
emitted file trees were compared byte for byte.

| Project | Diagnostics | JavaScript | JavaScript + declarations | JavaScript + source maps | All outputs |
| --- | --- | --- | --- | --- | --- |
| hono | identical | identical | identical | identical | identical |
| zod | identical | identical | 3 of 942 files | identical | 3 of 1884 files |
| Playwright | identical | identical | 9 of 1409 files | identical | 9 of 2816 files |
| TypeScript `src/compiler` | identical | identical | identical | identical | identical |
| Next.js `packages/next` | identical | identical | 12 of 3330 files | identical | 13 of 6660 files |
| Effect `packages/effect` | differ | identical | 18 of 992 files | identical | 29 of 1984 files |
| VS Code `src` | identical | identical | identical | identical | identical |

These comparisons were made with tsc 6.0.3's creation order on both sides
(`stableTypeOrdering: false` in tsc-rs since that order stopped being the
default; see the [README](../README.md#compatibility)). Diagnostics were
identical on every configuration except Effect, where
tsc-rs with its default eight checkers reports two errors in
`src/Stream.ts` (TS2375 at line 5009 and TS2345 at line 5059, both about
an `Effect<void, never, never>` union constituent under
`exactOptionalPropertyTypes`) that tsc does not report. The inference
behind them picks the first member of a union, and tsc orders union
members by the order in which its single checker created the types; a
parallel checker that checks `Stream.ts` without having created
`Effect<void, never, never>` before orders the union differently. tsc
itself reports the same two errors when the `void` declaration is checked
after its use. The difference depends on how the files are partitioned
among the checkers: with six or fewer checkers (`TSRS_CHECKERS=6`) the
diagnostics are identical to tsc's. (The tsgo preview also reports errors
on Effect that tsc does not, in other files.) JavaScript files and
JavaScript source maps were identical on every configuration. The
declaration files that differ contain the same declarations: the order of
union constituents and of the properties of inferred object types differs,
because tsc-rs derives that order from the type identities of its parallel
checkers. The order is stable for a given machine and checker count, and
`TSRS_CHECKERS=1` (a single checker, the exact serial mode) reproduces
tsc's order at the cost of the parallel speed-up. Declaration maps differ
only for a declaration file that itself differs.

Stable type ordering, tsc-rs's default, removes
the dependence on the checker partition. With the content order on both
sides, the same comparison against `tsc --stableTypeOrdering` gave identical
diagnostics on every project at every checker count tried (1, 6, 8 and 12
on Effect, where both compilers report the two Schema errors that
TypeScript 7 also reports), identical declaration files for zod,
Playwright and Next.js, and for Effect identical declaration files with a
single checker. With several checkers one Effect file,
`ai/internal/mcpProtocol/v2026_07_28.d.ts`, prints one union of three
mapped types in a different member order: the three compare equal up to
the comparison's final tiebreak, the type id, which with several checkers
is a checker-local creation id. TypeScript 7 keeps the same tiebreak.

## Reproducing

The measurements above are a quick interleaved run on one machine, not the
project's formal protocol. For repeatable measurements with recorded
provenance, exact output verification before timing, several sessions and
confidence intervals, see [benchmarking.md](benchmarking.md) and
`scripts/benchmark-cli.py`. Watch mode, incremental rebuilds and
`--build` were not timed, and cold-cache, Linux and Windows
timings were not measured.
