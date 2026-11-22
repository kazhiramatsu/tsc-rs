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
cores, 32 GiB), November 22, 2026. Every compiler ran with its default
settings.

| Project | tsgo | tsrs | tsc-rs |
| --- | ---: | ---: | ---: |
| VS Code `src` | 4.85 s · 6,186 MiB | 1.98 s · 2,281 MiB | 1.92 s · 2,330 MiB |
| Next.js `packages/next` | 1.32 s · 1,518 MiB | 0.49 s · 575 MiB | 0.50 s · 510 MiB |
| zod | 0.89 s · 1,593 MiB | 0.32 s · 581 MiB | 0.30 s · 529 MiB |
| Effect `packages/effect` | 0.79 s · 1,087 MiB | 0.32 s · 432 MiB | 0.30 s · 415 MiB |
| Playwright | 0.58 s · 905 MiB | 0.21 s · 352 MiB | 0.22 s · 373 MiB |
| Vue | 0.54 s · 667 MiB | 0.20 s · 231 MiB | 0.20 s · 246 MiB |
| TypeScript `src/compiler` | 0.37 s · 367 MiB | 0.15 s · 139 MiB | 0.16 s · 170 MiB |
| hono | 0.17 s · 260 MiB | 0.06 s · 109 MiB | 0.06 s · 112 MiB |

tsc-rs printed exactly `tsgo`'s diagnostics on all eight projects.

tsrs and tsc-rs each have an opt-in mode that spreads the check over more
cores and may report a few diagnostics differently from `tsgo`: `--fast` in
tsrs, the environment variable `TSRS_FAST=1` in tsc-rs (its result is the
same on every machine and in every run).

| Project | tsrs `--fast` | tsc-rs `TSRS_FAST=1` |
| --- | ---: | ---: |
| VS Code `src` | 1.17 s · 1,828 MiB | 1.33 s · 2,446 MiB |
| Next.js `packages/next` | 0.27 s · 561 MiB | 0.31 s · 636 MiB |
| zod | 0.21 s · 502 MiB | 0.24 s · 557 MiB |
| Effect `packages/effect` | 0.21 s · 387 MiB | 0.26 s · 431 MiB |
| Playwright | 0.14 s · 345 MiB | 0.17 s · 447 MiB |
| Vue | 0.13 s · 254 MiB | 0.15 s · 312 MiB |
| TypeScript `src/compiler` | 0.13 s · 137 MiB | 0.13 s · 164 MiB |
| hono | 0.05 s · 95 MiB | 0.05 s · 119 MiB |

- `tsgo` is TypeScript 7.1 built from the commit tsc-rs follows
  (`aa814927`). [tsrs](https://github.com/oxc-project/tsrs) is version
  7.1.0-beta.tsrs3.
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
