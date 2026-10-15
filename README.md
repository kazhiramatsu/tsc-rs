# tsc-rs

A TypeScript compiler written in Rust.

**Status:** compatibility with tsc 6.0.3 is complete, with some
limitations (release [`v0.1.0`](https://github.com/kazhiramatsu/tsc-rs/releases/tag/v0.1.0)).
Compatibility with **TypeScript 7.1** (the native compiler, `tsgo`) is in
progress on `main`; see [Compatibility](#compatibility).

- Same `tsconfig.json` and command line as `tsc`: `-p`, `--noEmit`,
  `--watch`, `-b`.
- Checks `.vue` and `.astro` files through content mappers, as `tsgo` does
  ([Content mappers](#content-mappers)).
- Fast and memory-efficient ([Performance](#performance)).
- No Node.js required.

Also experimental: a [language server](#language-server-experimental), an
[API server](#api-server-experimental) and a [Rust API](#rust-api-experimental).

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
| `--runExternalCode` | Run the [content mappers](#content-mappers) of `tsconfig.json` |

Exit status: `0` no errors, `1` errors and nothing written, `2` errors and
output written, `4` (`-b` only) a cycle of project references.

## Content mappers

TypeScript 7.1 checks other languages' files, such as `.vue` and `.astro`,
through content mappers: programs from npm packages that turn each file
into TypeScript. List them in `tsconfig.json` and run with
`--runExternalCode`, which allows tsc-rs to start them:

```json
{ "contentMappers": [{ "package": "@astrojs/ts-content-mapper", "extensions": [".astro"] }] }
```

As with `tsgo`, errors point into the original files, and with
`declaration` a mapped file gets a declaration file (`Card.d.astro.ts`) but
no JavaScript. On two Astro and two Vue projects, the output matched
`tsgo`'s. Content mappers also run in `-b` builds, under `--watch` and in
the API server (`tsc-rs --api --runExternalCode`): `--incremental` and
`-b` build again when a mapper's version or options change, and `--watch`
restarts a mapper whose package changes.

## Compatibility

`main` follows TypeScript 7.1 at commit `aa814927`, vendored under
`vendor/typescript-native/`. This work is in progress; so far:

- **Conformance:** in TypeScript 7.1's own test suite, every compared
  configuration (13,577 of 15,339; `tsgo`'s runner skips the others)
  matches `tsgo`'s baselines: diagnostics, emitted files, source maps,
  module resolution traces, `.types` and `.symbols`, and content mapper
  output.
- **Real projects:** `tsc-rs --noEmit` printed the same output as `tsgo` on
  all 9,067 DefinitelyTyped packages, all 2,756 azure-sdk-for-js
  configurations and all 38 material-ui projects (October 2026, one
  checker).
- **Language server:** in 4,583 of the 4,584 sessions of TypeScript 7.1's
  language server tests, tsc-rs answers exactly as `tsgo` does; `tsgo`'s
  own answer to the last one varies from run to run (October 15, 2026).
- **TypeScript 7 rules:** the default `target` is ES2026, and the options
  TypeScript 7 removed (`baseUrl`, `outFile`, `target: "es5"`, the `amd`,
  `umd` and `system` modules, …) are errors.

tsc 6.0.3 compatibility is complete in release
[`v0.1.0`](https://github.com/kazhiramatsu/tsc-rs/releases/tag/v0.1.0)
(branch `release/6.0.3`), with the limitations listed in
[its README](https://github.com/kazhiramatsu/tsc-rs/blob/v0.1.0/README.md#current-limitations)
(for example, no watch mode or `--build`).

## Performance

Type check (`--noEmit`), median wall-clock time and peak memory on an Apple
M5 (10 cores, 32 GiB), against `tsgo` built from the TypeScript commit
tsc-rs follows (October 12, 2026):

| Project | tsc-rs | tsgo |
| --- | ---: | ---: |
| hono | 124 ms, 304 MiB | 150 ms, 276 MiB |
| zod | 512 ms, 1,323 MiB | 814 ms, 1,606 MiB |
| TypeScript `src/compiler` | 317 ms, 290 MiB | 343 ms, 379 MiB |
| Next.js `packages/next` | 715 ms, 1,290 MiB | 1,240 ms, 1,535 MiB |
| Effect `packages/effect` | 469 ms, 1,016 MiB | 721 ms, 1,091 MiB |
| VS Code `src` | 3,529 ms, 5,499 MiB | 4,518 ms, 6,369 MiB |

Over all 26 configurations measured (type checks and builds with outputs),
tsc-rs took 0.58–0.93 of `tsgo`'s time and 0.71–1.10 of its memory. To use
less memory, lower `--checkers`. Full results, CPU time, content mapper
projects and method: [docs/performance.md](docs/performance.md). These
numbers predate the faster program loading of pull requests #760 and #761,
and will be measured again.

## Language server (experimental)

`tsc-rs --lsp --stdio` serves the Language Server Protocol over stdio, like
`tsgo --lsp --stdio`. Implemented: diagnostics, hover, completion (with
auto-imports and snippets), signature help, go to definition (also type definition,
implementation and source definition), references, rename (also of files),
document highlights, call hierarchy, code lens, document and workspace
symbols, folding and selection ranges, semantic tokens, inlay hints, linked
editing of JSX tags, formatting, quick fixes and organize imports, also in
content mapper files (`.vue`, `.astro`), with the mappers of a project's
config or (for files outside projects) of editor extensions. JavaScript
projects get their typings from npm (automatic type acquisition). It has not
been tried with editors yet.

## API server (experimental)

`tsc-rs --api` serves TypeScript 7.1's JavaScript API client, like
`tsgo --api`:

```sh
tsc-rs --api --cwd /path/to/workspace   # MessagePack over stdio (--async: JSON-RPC, --pipe <path>: socket)
```

With `--runExternalCode`, projects run their [content mappers](#content-mappers).
The language server also serves the client over a pipe
(`custom/initializeAPISession`), with the editor's projects and open files
(`getCurrentLanguageServerSnapshot`).

Implemented: snapshots and projects, config parsing, source files,
transpilation, module resolution, symbol, type and signature queries, scope
lookups, type nodes, `printNode`, `formatNodeForInsertion`, diagnostics,
emit, the build orchestrator and the language-service requests
(completions, references, signature usages, JSDoc, import edits); the
client's tests all pass. Not yet: `printNode`'s `preserveSourceNewlines`.
Details: [design notes](docs/design/greenfield/slices/ts71-api-server/README.md).

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

- The [language server](#language-server-experimental) is incomplete and
  untested in editors.
- `-b` builds projects one at a time.
- `--diagnostics` prints fewer statistics than `tsgo`.
- Memory grows with the number of checkers.
- `tsc-rs --api --pipe` does not support Windows named pipes.
- `--pprofDir` (and the language server's profile commands) writes CPU
  profiles only, on Unix; heap and allocation profiles are not supported.

## Development

Building from source, tests and CI: [docs/setup.md](docs/setup.md).
Design documents: [docs/design/README.md](docs/design/README.md).

## License

[MIT](LICENSE).
