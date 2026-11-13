# tsc-rs

A TypeScript compiler written in Rust.

It follows TypeScript 7.1's `tsc` (the native compiler, `tsgo`): the same
`tsconfig.json`, the same command line, the same diagnostics and the same
output files. This page is about how fast it is; for everything else, what
you know about `tsc` applies.

```sh
git clone https://github.com/kazhiramatsu/tsc-rs.git && cd tsc-rs
cargo install --locked --path crates/cli   # installs tsc-rs into ~/.cargo/bin
tsc-rs -p .                                # use it like tsc
```

## Performance

Type check (`--noEmit`) of eight open-source projects: wall-clock time and
peak memory, median of 9 runs on an Apple M5 (4 performance and 6 efficiency
cores, 32 GiB), November 13, 2026. Every compiler ran with its default
settings.

| Project | tsgo | tsrs | tsc-rs |
| --- | ---: | ---: | ---: |
| VS Code `src` | 4.44 s · 6,371 MiB | 1.87 s · 2,281 MiB | 2.13 s · 2,726 MiB |
| Next.js `packages/next` | 1.29 s · 1,510 MiB | 0.48 s · 575 MiB | 0.59 s · 706 MiB |
| zod | 0.85 s · 1,595 MiB | 0.31 s · 582 MiB | 0.35 s · 685 MiB |
| Effect `packages/effect` | 0.77 s · 1,081 MiB | 0.31 s · 432 MiB | 0.33 s · 535 MiB |
| Playwright | 0.55 s · 904 MiB | 0.20 s · 352 MiB | 0.25 s · 447 MiB |
| Vue | 0.52 s · 678 MiB | 0.19 s · 232 MiB | 0.27 s · 300 MiB |
| TypeScript `src/compiler` | 0.35 s · 370 MiB | 0.14 s · 139 MiB | 0.17 s · 191 MiB |
| hono | 0.17 s · 262 MiB | 0.06 s · 109 MiB | 0.07 s · 135 MiB |

tsc-rs printed exactly `tsgo`'s diagnostics on all eight projects.

tsrs and tsc-rs each have an opt-in mode that spreads the check over more
cores and may report a few diagnostics differently from `tsgo`: `--fast` in
tsrs, the environment variable `TSRS_FAST=1` in tsc-rs (its result is the
same on every machine and in every run).

| Project | tsrs `--fast` | tsc-rs `TSRS_FAST=1` |
| --- | ---: | ---: |
| VS Code `src` | 1.09 s · 1,825 MiB | 1.51 s · 2,917 MiB |
| Next.js `packages/next` | 0.27 s · 559 MiB | 0.37 s · 805 MiB |
| zod | 0.20 s · 503 MiB | 0.29 s · 812 MiB |
| Effect `packages/effect` | 0.20 s · 388 MiB | 0.27 s · 605 MiB |
| Playwright | 0.14 s · 345 MiB | 0.20 s · 534 MiB |
| Vue | 0.12 s · 251 MiB | 0.19 s · 367 MiB |
| TypeScript `src/compiler` | 0.12 s · 137 MiB | 0.17 s · 191 MiB |
| hono | 0.04 s · 96 MiB | 0.06 s · 148 MiB |

- `tsgo` is TypeScript 7.1 built from the commit tsc-rs follows
  (`aa814927`). [tsrs](https://github.com/oxc-project/tsrs) is version
  0.23.0.
- tsrs and tsc-rs were both built from source as plain release builds,
  without profile-guided optimization (PGO). The binaries tsrs publishes are
  PGO builds.
- Method, more measurements and the fast preset:
  [docs/performance.md](docs/performance.md).

## Everything else

The same as `tsc`. [Usage](docs/usage.md) (install, options, content
mappers, language server, API server, Rust API) ·
[Compatibility](docs/compatibility.md) (how it is checked against `tsgo`,
and the differences that remain) · [Development](docs/setup.md) ·
[MIT license](LICENSE)
