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
peak memory, median of 27 runs (three windows of 9) on an Apple M5 (4
performance and 6 efficiency cores, 32 GiB), November 28, 2026. Every compiler
ran with its default settings.

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

tsc-rs printed exactly `tsgo`'s diagnostics on all eight projects.

tsrs and tsc-rs each have an opt-in mode that spreads the check over more
cores and may report a few diagnostics differently from `tsgo`: `--fast` in
tsrs, the environment variable `TSRS_FAST=1` in tsc-rs. In both, the pool
follows the machine and the checkers take files from each other as they
finish, so a program whose diagnostics depend on the order of checking can
print differently from run to run.

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
