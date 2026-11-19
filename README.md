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
cores, 32 GiB), November 19, 2026. Every compiler ran with its default
settings.

| Project | tsgo | tsrs | tsc-rs |
| --- | ---: | ---: | ---: |
| VS Code `src` | 4.71 s · 6,259 MiB | 2.07 s · 2,286 MiB | 2.05 s · 2,388 MiB |
| Next.js `packages/next` | 1.37 s · 1,500 MiB | 0.51 s · 575 MiB | 0.54 s · 515 MiB |
| zod | 0.90 s · 1,578 MiB | 0.32 s · 581 MiB | 0.32 s · 532 MiB |
| Effect `packages/effect` | 0.79 s · 1,074 MiB | 0.34 s · 432 MiB | 0.31 s · 413 MiB |
| Playwright | 0.58 s · 938 MiB | 0.21 s · 351 MiB | 0.24 s · 374 MiB |
| Vue | 0.54 s · 669 MiB | 0.21 s · 231 MiB | 0.23 s · 236 MiB |
| TypeScript `src/compiler` | 0.36 s · 374 MiB | 0.15 s · 139 MiB | 0.16 s · 166 MiB |
| hono | 0.17 s · 258 MiB | 0.06 s · 109 MiB | 0.06 s · 110 MiB |

tsc-rs printed exactly `tsgo`'s diagnostics on all eight projects.

tsrs and tsc-rs each have an opt-in mode that spreads the check over more
cores and may report a few diagnostics differently from `tsgo`: `--fast` in
tsrs, the environment variable `TSRS_FAST=1` in tsc-rs (its result is the
same on every machine and in every run).

| Project | tsrs `--fast` | tsc-rs `TSRS_FAST=1` |
| --- | ---: | ---: |
| VS Code `src` | 1.19 s · 1,832 MiB | 1.51 s · 2,503 MiB |
| Next.js `packages/next` | 0.28 s · 558 MiB | 0.33 s · 615 MiB |
| zod | 0.21 s · 503 MiB | 0.27 s · 554 MiB |
| Effect `packages/effect` | 0.22 s · 388 MiB | 0.27 s · 425 MiB |
| Playwright | 0.15 s · 345 MiB | 0.19 s · 425 MiB |
| Vue | 0.13 s · 253 MiB | 0.16 s · 290 MiB |
| TypeScript `src/compiler` | 0.13 s · 138 MiB | 0.14 s · 162 MiB |
| hono | 0.05 s · 95 MiB | 0.05 s · 121 MiB |

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
