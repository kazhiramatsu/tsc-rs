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
cores, 32 GiB), November 25, 2026. Every compiler ran with its default
settings.

| Project | tsgo | tsrs | tsc-rs |
| --- | ---: | ---: | ---: |
| VS Code `src` | 5.11 s · 6,200 MiB | 2.12 s · 2,283 MiB | 2.01 s · 2,237 MiB |
| Next.js `packages/next` | 1.40 s · 1,506 MiB | 0.52 s · 576 MiB | 0.50 s · 475 MiB |
| zod | 0.98 s · 1,589 MiB | 0.34 s · 581 MiB | 0.30 s · 478 MiB |
| Effect `packages/effect` | 0.86 s · 1,101 MiB | 0.33 s · 432 MiB | 0.31 s · 385 MiB |
| Playwright | 0.59 s · 946 MiB | 0.22 s · 351 MiB | 0.22 s · 351 MiB |
| Vue | 0.56 s · 663 MiB | 0.21 s · 231 MiB | 0.21 s · 237 MiB |
| TypeScript `src/compiler` | 0.38 s · 363 MiB | 0.16 s · 139 MiB | 0.16 s · 158 MiB |
| hono | 0.17 s · 260 MiB | 0.06 s · 109 MiB | 0.06 s · 103 MiB |

tsc-rs printed exactly `tsgo`'s diagnostics on all eight projects.

tsrs and tsc-rs each have an opt-in mode that spreads the check over more
cores and may report a few diagnostics differently from `tsgo`: `--fast` in
tsrs, the environment variable `TSRS_FAST=1` in tsc-rs. In both, the pool
follows the machine and the checkers take files from each other as they
finish, so a program whose diagnostics depend on the order of checking can
print differently from run to run.

| Project | tsrs `--fast` | tsc-rs `TSRS_FAST=1` |
| --- | ---: | ---: |
| VS Code `src` | 1.28 s · 1,816 MiB | 1.38 s · 2,205 MiB |
| Next.js `packages/next` | 0.29 s · 559 MiB | 0.30 s · 638 MiB |
| zod | 0.21 s · 505 MiB | 0.24 s · 583 MiB |
| Effect `packages/effect` | 0.23 s · 388 MiB | 0.20 s · 429 MiB |
| Playwright | 0.16 s · 350 MiB | 0.16 s · 432 MiB |
| Vue | 0.14 s · 252 MiB | 0.14 s · 303 MiB |
| TypeScript `src/compiler` | 0.13 s · 137 MiB | 0.14 s · 163 MiB |
| hono | 0.04 s · 95 MiB | 0.05 s · 104 MiB |

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
