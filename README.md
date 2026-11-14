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
cores, 32 GiB), November 15, 2026. Every compiler ran with its default
settings.

| Project | tsgo | tsrs | tsc-rs |
| --- | ---: | ---: | ---: |
| VS Code `src` | 4.51 s · 6,330 MiB | 1.91 s · 2,282 MiB | 2.09 s · 2,620 MiB |
| Next.js `packages/next` | 1.32 s · 1,490 MiB | 0.51 s · 577 MiB | 0.56 s · 646 MiB |
| zod | 0.87 s · 1,532 MiB | 0.31 s · 581 MiB | 0.33 s · 652 MiB |
| Effect `packages/effect` | 0.76 s · 1,086 MiB | 0.30 s · 432 MiB | 0.32 s · 501 MiB |
| Playwright | 0.56 s · 905 MiB | 0.20 s · 352 MiB | 0.24 s · 425 MiB |
| Vue | 0.52 s · 656 MiB | 0.20 s · 231 MiB | 0.26 s · 275 MiB |
| TypeScript `src/compiler` | 0.36 s · 367 MiB | 0.14 s · 139 MiB | 0.17 s · 186 MiB |
| hono | 0.16 s · 261 MiB | 0.06 s · 109 MiB | 0.07 s · 131 MiB |

tsc-rs printed exactly `tsgo`'s diagnostics on all eight projects.

tsrs and tsc-rs each have an opt-in mode that spreads the check over more
cores and may report a few diagnostics differently from `tsgo`: `--fast` in
tsrs, the environment variable `TSRS_FAST=1` in tsc-rs (its result is the
same on every machine and in every run).

| Project | tsrs `--fast` | tsc-rs `TSRS_FAST=1` |
| --- | ---: | ---: |
| VS Code `src` | 1.11 s · 1,826 MiB | 1.46 s · 2,802 MiB |
| Next.js `packages/next` | 0.27 s · 560 MiB | 0.34 s · 747 MiB |
| zod | 0.19 s · 504 MiB | 0.28 s · 739 MiB |
| Effect `packages/effect` | 0.20 s · 387 MiB | 0.27 s · 522 MiB |
| Playwright | 0.14 s · 341 MiB | 0.19 s · 502 MiB |
| Vue | 0.12 s · 251 MiB | 0.18 s · 344 MiB |
| TypeScript `src/compiler` | 0.12 s · 137 MiB | 0.15 s · 184 MiB |
| hono | 0.04 s · 95 MiB | 0.06 s · 145 MiB |

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
