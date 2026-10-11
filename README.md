# tsc-rs

A TypeScript compiler written in Rust.

**Status:** compatibility with tsc 6.0.3 is complete, with some
limitations (release [`v0.1.0`](https://github.com/kazhiramatsu/tsc-rs/releases/tag/v0.1.0)).
Compatibility with **TypeScript 7.1** (the native compiler, `tsgo`) is in
progress on `main`; see [Compatibility](#compatibility).

- Same `tsconfig.json` and command line as `tsc`: `-p`, `--noEmit`,
  `--watch`, `-b`.
- Fast and memory-efficient ([Performance](#performance)).
- No Node.js required.

Also experimental: an [API server](#api-server-experimental) and a
[Rust API](#rust-api-experimental). There is no language server.

## Install

Requires [Rust (rustup)](https://www.rust-lang.org/tools/install), Git and a
native linker (on macOS, the Xcode Command Line Tools).

```sh
git clone https://github.com/kazhiramatsu/tsc-rs.git
cd tsc-rs
cargo install --locked --path crates/cli   # installs tsc-rs into ~/.cargo/bin
tsc-rs --version                           # Version 7.1.0-dev
```

## Usage

Use it like `tsc`:

```sh
tsc-rs -p .                 # compile the project of ./tsconfig.json
tsc-rs --noEmit -p .        # type-check only
tsc-rs --watch -p .         # compile again on every change
tsc-rs -b                   # build the project and the projects it references
tsc-rs --ignoreConfig a.ts  # compile single files, ignoring tsconfig.json
```

In `package.json`, replace `tsc` with `tsc-rs`. Write `tsc-rs`, not `tsc`
or `npx tsc`, which run the installed `typescript` package.

```json
{ "scripts": { "build": "tsc-rs -p .", "typecheck": "tsc-rs --noEmit -p ." } }
```

Every `tsconfig.json` option can also be given on the command line
(`--strict`, `--outDir dist`, `--sourceMap`, …). Other options:

| Option | Use |
| --- | --- |
| `--noEmitOnError` | Write nothing when there are errors |
| `--pretty false` | Plain diagnostics |
| `-b --verbose`, `--dry`, `--force`, `--clean` | Build mode: explain, preview, rebuild all, delete outputs |
| `--listFiles`, `--explainFiles`, `--listEmittedFiles` | List the program's files, why each is included, the files written |
| `--showConfig`, `--init` | Print the configuration in effect, create a `tsconfig.json` |
| `--diagnostics`, `--generateTrace <dir>` | Statistics, performance trace |
| `--locale <tag>` | Translated messages (`ja`, `de`, `zh-CN`, …) |
| `--checkers <n>` | Checker threads (default: one per core, up to 8; also `TSRS_CHECKERS`) |

Exit status: `0` no errors, `1` errors and nothing written, `2` errors and
output written, `4` (`-b` only) a cycle of project references.

## Compatibility

`main` follows TypeScript 7.1 at commit `19dadef8`, vendored under
`vendor/typescript-native/`. This work is in progress; so far:

- **Conformance:** in TypeScript 7.1's own test suite, the diagnostics match
  in all 13,451 compared configurations and the emitted files in 13,443.
- **Real projects:** `tsc-rs --noEmit` printed the same output as `tsgo` on
  all 9,067 DefinitelyTyped packages, all 2,756 azure-sdk-for-js
  configurations and all 38 material-ui projects (October 2026, one
  checker).
- **TypeScript 7 rules:** the default `target` is ES2026, and the options
  TypeScript 7 removed (`baseUrl`, `outFile`, `target: "es5"`, the `amd`,
  `umd` and `system` modules, …) are errors.

tsc 6.0.3 compatibility is complete in release
[`v0.1.0`](https://github.com/kazhiramatsu/tsc-rs/releases/tag/v0.1.0)
(branch `release/6.0.3`), with the limitations listed in
[its README](https://github.com/kazhiramatsu/tsc-rs/blob/v0.1.0/README.md#current-limitations)
(for example, no watch mode or `--build`).

## Performance

> **Note:** these numbers were measured with the tsc 6.0.3 compatible build
> (October 1, 2026), before the switch to TypeScript 7.1. They will be
> measured again.

Type check (`--noEmit`), median wall-clock time and peak memory on an Apple
M5 (10 cores, 32 GiB), against `tsgo` 7.1.0-dev and `tsc` 6.0.3:

| Project | tsc-rs | tsgo | tsc |
| --- | ---: | ---: | ---: |
| hono | 118 ms, 306 MiB | 161 ms, 326 MiB | 959 ms, 520 MiB |
| zod | 512 ms, 1,310 MiB | 946 ms, 1,774 MiB | 5,380 ms, 2,069 MiB |
| Playwright | 355 ms, 754 MiB | 617 ms, 1,041 MiB | 4,424 ms, 1,299 MiB |
| TypeScript `src/compiler` | 335 ms, 291 MiB | 359 ms, 404 MiB | 2,732 ms, 710 MiB |
| Next.js `packages/next` | 798 ms, 1,341 MiB | 1,419 ms, 1,710 MiB | 7,949 ms, 2,381 MiB |
| Effect `packages/effect` | 502 ms, 1,034 MiB | 791 ms, 1,209 MiB | 5,417 ms, 1,657 MiB |
| VS Code `src` | 3,427 ms, 5,475 MiB | 4,763 ms, 6,236 MiB | 42,891 ms, 7,386 MiB |

Over all 31 configurations measured (type checks and builds with outputs),
tsc-rs took 0.54–0.93 of `tsgo`'s time and 0.66–0.94 of its memory. To use
less memory, lower `--checkers`. Full results and method:
[docs/performance.md](docs/performance.md).

## API server (experimental)

`tsc-rs --api` serves TypeScript 7.1's JavaScript API client, like
`tsgo --api`:

```sh
tsc-rs --api --cwd /path/to/workspace   # MessagePack over stdio (--async: JSON-RPC, --pipe <path>: socket)
```

Implemented: snapshots and projects, config parsing, source files,
transpilation, module resolution, symbol, type and signature queries, scope
lookups, type nodes, diagnostics and emit. Not yet: `printNode`, the build
orchestrator and language-service requests. Details: [design notes](docs/design/greenfield/slices/ts71-api-server/README.md).

## Rust API (experimental)

The compiler is a set of crates (`tsc-rs-compiler`, `tsc-rs-program`,
`tsc-rs-host`, `tsc-rs-diagnostics`, `tsc-rs-api`) that Rust programs can
use to check and compile projects in-process. Interfaces may change. Try
the [type-check example](crates/compiler/examples/type_check.rs):

```sh
cargo run --locked --manifest-path crates/compiler/Cargo.toml --example type_check -- \
  examples/type-check/tsconfig.json \
  vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs
```

Guide: [docs/rust-api.md](docs/rust-api.md).

## Limitations

- No language server or editor integration.
- `-b` builds projects one at a time.
- `--diagnostics` prints fewer statistics than `tsgo`.
- Memory grows with the number of checkers.
- `tsc-rs --api --pipe` does not support Windows named pipes.
- Some of TypeScript 7.1's `.types` and `.symbols` test baselines still
  differ.

## Development

Building from source, tests and CI: [docs/setup.md](docs/setup.md).
Design documents: [docs/design/README.md](docs/design/README.md).

## License

[MIT](LICENSE).
