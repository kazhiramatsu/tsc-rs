# Usage

tsc-rs is used like `tsc`: the same `tsconfig.json` and the same command line. This page has the installation, the options
worth knowing, and the parts that go beyond the command line (content mappers, the language server, the API server,
the Rust API). What differs from `tsc`: [compatibility.md](compatibility.md).

## Install

Requires [Rust (rustup)](https://www.rust-lang.org/tools/install), Git and a
native linker (on macOS, the Xcode Command Line Tools).

```sh
git clone https://github.com/kazhiramatsu/tsc-rs.git
cd tsc-rs
cargo install --locked --path crates/cli   # installs tsc-rs into ~/.cargo/bin
tsc-rs --version                           # Version 7.1.0-dev
```

## Command line

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
| `--checkers <n>` | Checker threads (default: 4; clamped to 1–256 and the file count; also `TSRS_CHECKERS`) |
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
projects get their typings from npm (automatic type acquisition). For an
editor that cannot watch files itself, the server watches them on macOS and
Windows, as `tsgo` does. It has not been tried with editors yet.

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
client's tests all pass.
Details: [design notes](design/greenfield/slices/ts71-api-server/README.md).

## Rust API (experimental)

The compiler is a set of crates (`tsc-rs-compiler`, `tsc-rs-program`,
`tsc-rs-host`, `tsc-rs-diagnostics`, `tsc-rs-api`) that Rust programs can
use to check and compile projects in-process. Interfaces may change. Try
the [type-check example](../crates/compiler/examples/type_check.rs):

```sh
cargo run --locked --manifest-path crates/compiler/Cargo.toml --example type_check -- \
  examples/type-check/tsconfig.json \
  vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs
```

Guide: [docs/rust-api.md](rust-api.md).
