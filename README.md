# tsc-rs

tsc-rs is a TypeScript compiler written in Rust. It checks types and compiles
TypeScript to JavaScript, with support for source maps and declaration files.
Its compatibility target is **TypeScript 6.0.3**. The same compiler is
available to Rust programs as a library; see the
[compiler API](#compiler-api-for-rust-projects-experimental).

See the [current limitations](#current-limitations) before adopting it for
an existing project. See [Performance](#performance) for measured compile
times against `tsc` and `tsgo` on real projects, and [Run CI](#run-ci) for
the checks used to validate the compiler, how to reproduce them, and
recorded results.

## Build

Prerequisites:

- [Rust with rustup](https://www.rust-lang.org/tools/install).
- Git.
- A native linker. On macOS, install the Xcode Command Line Tools if needed.

Clone the repository:

```sh
git clone https://github.com/kazhiramatsu/tsc-rs.git
cd tsc-rs
```

Rustup selects the toolchain specified in `rust-toolchain.toml`. The first
build may download the Rust toolchain and Cargo dependencies.

Choose either [building an executable](#build-an-executable) to run by path
or [installing the command](#install-the-command) on your `PATH`. You do not
need to do both.

### Build an executable

From the cloned repository:

```sh
cargo build --release --locked --manifest-path crates/compiler/Cargo.toml
```

The executable is `target/release/tsc-rs` (`tsc-rs.exe` on Windows):

```sh
./target/release/tsc-rs --version
```

```text
Version 6.0.3
```

Node.js and npm are not required to build or run the compiler. TypeScript's
standard library declarations are included in the executable, so it can run
outside the source checkout.

### Install the command

From the cloned repository, build and install the executable into Cargo's
binary directory:

```sh
cargo install --locked --path crates/compiler
```

Make sure Cargo's binary directory, normally `~/.cargo/bin`, is on your
`PATH`. The examples below use the installed `tsc-rs` command. You can also
substitute the absolute path to the executable built above.

### Use it in place of `tsc`

`tsc-rs` reads the same `tsconfig.json` as `tsc`, and the project commands
are the same: `tsc-rs -p .` compiles a project and `tsc-rs --noEmit -p .`
type-checks it. Two differences matter when you switch:

- Compiler settings such as `outDir`, `strict`, `sourceMap` and
  `declaration` are read from `tsconfig.json` only. `tsc-rs` rejects them
  as command-line flags, so move any such flags from your scripts into
  `compilerOptions`. The accepted flags are listed under
  [common command-line options](#common-command-line-options).
- `--noEmit` cannot be combined with the emit flags `--target`, `--module`,
  `--newLine`, `--emitBOM`, `--listEmittedFiles` and `--noEmitOnError`;
  keep those settings in `tsconfig.json` as well. The type check reads the
  whole file, including `rootDir` and `declaration`, and writes nothing.

Watch mode, `--build` and project references are not supported; see the
[current limitations](#current-limitations).

To switch an npm project:

1. [Install the command](#install-the-command) on every developer machine
   and CI runner that runs the project's scripts, and make sure `tsc-rs` is
   on `PATH` there. Installing it does not change an existing `tsc`.
2. Replace `tsc` with `tsc-rs` in the `package.json` scripts, for example:

   ```json
   {
     "scripts": {
       "build": "tsc-rs -p .",
       "typecheck": "tsc-rs --noEmit -p ."
     }
   }
   ```

3. Run the scripts as before from the project directory:

   ```sh
   npm run build
   npm run typecheck
   ```

Write `tsc-rs` in the scripts, not `tsc` or `npx tsc`:
[npm scripts](https://docs.npmjs.com/cli/v11/commands/npm-run/) run with
the project's `node_modules/.bin` first on `PATH`, so `tsc` there is always
the compiler of the installed `typescript` package, and `npx tsc`
[resolves that package](https://docs.npmjs.com/cli/v11/commands/npm-exec/)
as well. Keep the `typescript` dependency if your editor or other tools use
it: `tsc-rs` replaces the compiler command and offers a
[Rust compiler API](#compiler-api-for-rust-projects-experimental), but it
does not provide TypeScript's JavaScript API or language server.

### Use the name `tsc` in your shell

This step is optional. It affects only the commands you type in a macOS or
Linux shell; npm scripts keep resolving `tsc` as described above. After
confirming that `tsc-rs --version` works, create a symlink named `tsc` in a
dedicated directory and put that directory first on `PATH`:

```sh
mkdir -p "$HOME/.local/tsc-rs/bin"
ln -sf "$(command -v tsc-rs)" "$HOME/.local/tsc-rs/bin/tsc"
export PATH="$HOME/.local/tsc-rs/bin:$PATH"
hash -r
command -v tsc
tsc --noEmit -p .
```

`command -v tsc` should show the symlink under your home directory. The
original TypeScript executable is left in place. Add the `export PATH=...`
line to your shell startup file, such as `~/.zshrc` or `~/.bashrc`, to keep
the setting in new terminals. To switch back, remove that line and open a
new terminal.

## Compile a project

From the cloned repository, create a separate example directory alongside it:

```sh
cd ..
mkdir tsc-rs-example
cd tsc-rs-example
```

Run the following project commands from `tsc-rs-example`. Create these files:

```text
tsc-rs-example/
├── tsconfig.json
├── greet.ts
└── main.ts
```

Save this as `greet.ts`:

```ts
export function greet(name: string): string {
  return `Hello, ${name}!`;
}
```

Save this as `main.ts`:

```ts
import { greet } from "./greet";

console.log(greet("TypeScript"));
```

Save this as `tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "CommonJS",
    "strict": true,
    "outDir": "dist"
  },
  "include": ["*.ts"]
}
```

From the example directory, compile the project:

```sh
tsc-rs -p .
```

This writes `dist/greet.js` and `dist/main.js`. With Node.js installed, you
can run the generated JavaScript:

```sh
node dist/main.js
```

```text
Hello, TypeScript!
```

Choose a module format that matches the runtime where you will run the
JavaScript; this example uses CommonJS.

### Select a configuration file

`-p` accepts either a project directory or a configuration file:

```sh
tsc-rs -p ./my-project
tsc-rs -p tsconfig.production.json
```

With no project or source files specified, `tsc-rs` searches the current
directory and its parents for `tsconfig.json`:

```sh
tsc-rs
```

Paths in a configuration file are resolved relative to that file. Use
`files` or `include` to select inputs, and `exclude` to narrow file discovery.
An excluded file can still be compiled if another source imports it.

## Check types without writing files

To check the example project without generating JavaScript:

```sh
tsc-rs --noEmit -p .
```

A successful check exits with status `0`. Diagnostics or command failures
produce a nonzero exit status. For plain diagnostic output in a terminal or
automation, use:

```sh
tsc-rs --noEmit -p . --pretty false
```

To try a type error, add this line to `main.ts`:

```ts
const count: number = "three";
```

Run the check again:

```sh
tsc-rs --noEmit -p . --pretty false
```

The diagnostic includes:

```text
error TS2322: Type 'string' is not assignable to type 'number'.
```

Compilation can write JavaScript even when type errors are reported. To
prevent output when diagnostics are present, use:

```sh
tsc-rs -p . --noEmitOnError
```

Remove the added line from `main.ts` before continuing with the examples.

### Configuration for type checks

The `--noEmit` project command reads the same configuration file as a
build, including output settings such as `outDir`, `rootDir`, `sourceMap`
and `declaration`. As with `tsc --noEmit`, declaration settings still
produce declaration diagnostics; no files are written.

Emit settings cannot be overridden on the command line together with
`--noEmit -p`: flags such as `--target`, `--module`, `--newLine`,
`--emitBOM`, `--listEmittedFiles` and `--noEmitOnError` are rejected with
an error on that route. Put those settings in `tsconfig.json`. The example
project can be checked at any point with `tsc-rs --noEmit -p .`, including
after adding the source map and [declaration](#declaration-files) settings
below.

## Compiler API for Rust projects (experimental)

**The Rust API is experimental and still under development. Its interfaces
may change incompatibly.**

tsc-rs is a set of Rust crates, and the `tsc-rs` command is built on them.
A Rust project can depend on the same crates to load a TypeScript project,
check it, and produce JavaScript, declaration files and source maps inside
its own process, with no Node.js and no separate compiler executable.

The API has four parts:

- `tsc-rs-host` (`tsc_host`): the read-only `CompilerHost` trait.
  `FsCompilerHost` implements it over the real filesystem and
  `MemoryCompilerHost` over files supplied in memory.
- `tsc-rs-program` (`tsc_program`): `parse_config_root_plan` parses a
  `tsconfig.json`, including `extends` chains. `load_config_program`,
  `load_config_program_with_no_emit_override` and
  `load_emitting_config_program` turn the plan into a `PreparedProgram`,
  the owned program with every source text and module resolution;
  `load_program` and `load_emitting_program` do the same for explicit root
  files without a configuration file. `LibraryCatalog` locates the standard
  library declarations and `ProgramLoadLimits` bounds the program size.
- `tsc-rs-compiler` (`tsc_compiler`): `ProgramSession` owns one prepared
  program and runs it once. `run` and `run_no_emit_command` type-check it
  and return the diagnostics, the latter including the declaration
  diagnostics that `tsc --noEmit` reports. `emit` type-checks and compiles
  it, delivering every output file to an `OutputSink`: `MemoryOutputSink`
  collects the files in memory, and `FsOutputSink` writes them through an
  `EmitFileSystem` implementation. `CheckerBudget` and `WorkerBudget` set
  the parallelism; the API defaults are serial, and the command uses
  `CheckerBudget::automatic()` and `WorkerBudget::automatic()`.
- `tsc-rs-diagnostics` (`tsc_diagnostics`): `Diagnostic`, with its code,
  category, message chain and UTF-16 location, and `TextSnapshot` for line
  and column lookup.

The API reads the standard library declarations from the directory given to
`LibraryCatalog::typescript_6_0_3`; the checked-in
`vendor/typescript-6.0.3/lib` supplies the matching files. The copy embedded
in the `tsc-rs` executable is not exposed through the API.

### Check a project

The [complete example](crates/compiler/examples/type_check.rs) reads a
`tsconfig.json`, forces `noEmit`, and returns diagnostics as JSON without
writing JavaScript or declaration files. From the repository root, run it
against the included TypeScript example:

```sh
cargo run --locked --manifest-path crates/compiler/Cargo.toml --example type_check -- \
  examples/type-check/tsconfig.json vendor/typescript-6.0.3/lib
```

The arguments are the configuration file and the directory containing
TypeScript's `lib.*.d.ts` files. Replace the first argument with your
project's configuration file to check it instead. Output settings such as
`declaration` and `rootDir` are read from the file, and nothing is written.

The included `main.ts` deliberately assigns `"three"` to a `number`, so the
example exits with status `1` and reports the following (the absolute file
path depends on your checkout):

```json
{
  "has_errors": true,
  "diagnostics": [
    {
      "code": 2322,
      "category": "error",
      "message": "Type 'string' is not assignable to type 'number'.",
      "file": "/path/to/tsc-rs/examples/type-check/main.ts",
      "start": 6,
      "length": 5,
      "line": 1,
      "column": 7
    }
  ]
}
```

`start` and `length` count UTF-16 code units; `start` is zero-based. The
example converts locations to one-based `line` and `column` values, with
columns also measured in UTF-16 code units. File and location fields can be
`null` for diagnostics without a source location. Nested diagnostic
messages are included in `message`.

Change `"three"` to `3` and rerun to get an empty diagnostic list and exit
status `0`. TypeScript and configuration errors are returned as JSON with
exit status `1`. If an unsupported project feature, I/O error, or compiler
execution failure prevents checking, the example writes the reason to
stderr and exits with status `2`.

### Compile a project

To produce output files instead, load the program through the emitting
route and pass an output sink to `emit`. This fragment reuses the `host`,
`plan`, `libraries` and `limits` values built in the example and collects
the outputs in memory:

```rust
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_program::load_emitting_config_program;

// This route requires a configuration that does not set noEmit.
let prepared = load_emitting_config_program(&host, &plan, &libraries, limits)?;
let mut sink = MemoryOutputSink::new();
let outcome = ProgramSession::new(prepared).emit(&mut sink)?;

for diagnostic in outcome.diagnostics() {
    eprintln!("TS{}: {}", diagnostic.code(), diagnostic.message.text.to_string_lossy());
}
for artifact in sink.writes() {
    println!(
        "{:?} {} ({} bytes)",
        artifact.kind(),
        artifact.path().to_string_lossy(),
        artifact.materialized_bytes().len()
    );
}
```

For the example project with `outDir`, `declaration` and `sourceMap` set,
this prints a `JavaScript`, a `JavaScriptMap` and a `Declaration` artifact
under `dist`, and nothing is written to disk. Each `EmitArtifact` carries
its output path, its kind and its bytes. To write the files, implement the
three methods of `EmitFileSystem` (`write_file`, `create_directory` and
`directory_exists`) for your filesystem and wrap it in `FsOutputSink`.

`emit` returns the emitter's outcome: its own diagnostics and, when
`listEmittedFiles` is set, the list of emitted files. The type errors found
while compiling are not part of this value; to report them, run a no-emit
session as in the example. The combined report printed by the `tsc-rs`
command comes from an internal entry that is not yet a public API.

### Add the crates to your application

For a Rust application located alongside the `tsc-rs` checkout, add these
path dependencies to its `Cargo.toml`:

```toml
[dependencies]
tsc-compiler = { package = "tsc-rs-compiler", path = "../tsc-rs/crates/compiler" }
tsc-program = { package = "tsc-rs-program", path = "../tsc-rs/crates/program" }
tsc-host = { package = "tsc-rs-host", path = "../tsc-rs/crates/host" }
tsc-diagnostics = { package = "tsc-rs-diagnostics", path = "../tsc-rs/crates/diagnostics" }
serde_json = "1.0"
```

Copy the [example](crates/compiler/examples/type_check.rs) to your
application's `src/main.rs`, then run it from that application's directory:

```sh
cargo run -- ../tsc-rs/examples/type-check/tsconfig.json ../tsc-rs/vendor/typescript-6.0.3/lib
```

To integrate it into your own code, reuse the example's `check_project`
function. It returns `Diagnostic` values and the source snapshots used for
location lookup. Read `diagnostic.code()`, `diagnostic.category()`,
`diagnostic.message`, `diagnostic.file_name`, `diagnostic.start`, and
`diagnostic.length` directly, or adapt `diagnostic_json` to your output
format. TypeScript errors are returned as diagnostics; the function's `Err`
indicates that the check could not complete.

## Compile individual files

To try a single file, save this as `hello.ts`:

```ts
const message: string = "Hello, TypeScript!";
console.log(message);
```

In a directory with no `tsconfig.json` in it or its parents, compile it with:

```sh
tsc-rs --target ES2022 --module CommonJS hello.ts
```

The output is written next to the input as `hello.js`. You can pass multiple
source files, but cannot combine explicit files with `-p`.

If you are still in `tsc-rs-example`, or another directory with a
`tsconfig.json` in it or its parents, specifying source files reports
`TS5112`. Use `--ignoreConfig` to explicitly bypass that configuration:

```sh
tsc-rs --ignoreConfig --target ES2022 --module CommonJS hello.ts
tsc-rs --ignoreConfig --noEmit hello.ts
```

`--ignoreConfig` does not apply the settings from that configuration file.
For project settings such as `strict`, `outDir`, or declaration output, use
`tsconfig.json` and `-p`.

## Source maps and declaration files

### Source maps

Add `"sourceMap": true` to the example's `compilerOptions`, then compile
the project again:

```sh
tsc-rs -p .
```

The output now includes `dist/greet.js.map` and `dist/main.js.map`. These
files let a debugger relate generated JavaScript to the TypeScript source.
Keep them alongside the corresponding JavaScript files when debugging.

### Declaration files

To generate types for a library, create `tsconfig.types.json` beside the
example's `tsconfig.json`:

```json
{
  "extends": "./tsconfig.json",
  "compilerOptions": {
    "declaration": true,
    "declarationMap": true,
    "emitDeclarationOnly": true
  }
}
```

```sh
tsc-rs -p tsconfig.types.json
```

This writes `.d.ts` and `.d.ts.map` files to `dist`. For example,
`dist/greet.d.ts` declares the exported function:

```ts
export declare function greet(name: string): string;
```

`emitDeclarationOnly` generates declarations without writing JavaScript.
To produce JavaScript and declarations together, omit that option or set it
to `false`. `tsc-rs --noEmit -p tsconfig.types.json` reports the
declaration diagnostics of this configuration without writing any files.

## Common command-line options

| Option | Use |
| --- | --- |
| `--version` | Print the TypeScript compatibility version. |
| `-p`, `--project <path>` | Select a project directory or configuration file. |
| `--noEmit` | Check the project without writing compiler output. |
| `--noEmit=false` | Enable output even when the configuration sets `noEmit`. |
| `--target <version>` | Set the JavaScript target for compilation, such as `ES2022`. |
| `--module <format>` | Set the module format, such as `CommonJS`, `ESNext`, or `Preserve`. |
| `--noEmitOnError` | Prevent output when diagnostics are present. |
| `--listEmittedFiles` | Print the paths of generated files. |
| `--pretty false` | Use plain diagnostic output. |
| `--ignoreConfig` | Compile explicit files without loading a discovered configuration. |
| `--newLine lf` | Use LF line endings in generated output; `crlf` is also accepted. |
| `--emitBOM` | Write a byte-order mark to output files. |
| `--useDefineForClassFields <boolean>` | Select define semantics for class fields. |

Boolean options accept `true` or `false`, including the equals form, such
as `--pretty=false`. To see which files a project build writes:

```sh
tsc-rs -p . --listEmittedFiles
```

Compiler settings such as `strict`, `outDir`, `sourceMap`, and `declaration`
belong in `tsconfig.json`; they are not currently accepted as CLI flags.
For `--noEmit`, see [configuration for type checks](#configuration-for-type-checks).

## Performance

tsc-rs is a native executable that parses, binds and checks a program on
several threads (up to eight checker threads by default) and writes its
output files in parallel. The standard library declarations are embedded,
so a run has no JavaScript runtime start-up. The measurements below compare
it with TypeScript 6.0.3 (`tsc`, running on Node.js) and with the native
TypeScript 7 preview compiler (`tsgo`) on real projects.

### Measured projects

Each project was checked out at the commit shown, with its dependencies
installed, and compiled with its own configuration file (`tsconfig.json`
of the directory shown, or the file named). Derived configurations switch
only the output mode (`noEmit`; JavaScript; JavaScript and declaration
files; JavaScript and source maps; all outputs with `declaration`,
`declarationMap` and `sourceMap`) and turn off `composite` and
`incremental`. VS Code was measured with `--noEmit` only.

| Project | Commit | Program files | Source lines |
| --- | --- | ---: | ---: |
| [hono](https://github.com/honojs/hono) (`tsconfig.build.json`) | `8dcd52b` | 362 | 25,947 |
| [zod](https://github.com/colinhacks/zod) | `2bf7b06` | 2,364 | 117,060 |
| [Playwright](https://github.com/microsoft/playwright) | `ec31a7b` | 1,505 | 154,645 |
| [TypeScript](https://github.com/microsoft/TypeScript) 6.0.3, `src/compiler` | `050880ce5` | 249 | 194,701 |
| [Next.js](https://github.com/vercel/next.js), `packages/next` | `1edced6f` | 2,866 | 314,268 |
| [VS Code](https://github.com/microsoft/vscode), `src` | `29b68000` | 10,272 | 3,048,452 |

Program files count every file in the program as listed by
`tsc --listFiles`, including standard library and `node_modules`
declaration files. Source lines count the program's non-declaration
TypeScript files. zod, Playwright, Next.js and VS Code report type errors
at these commits with the configurations used; all compilers report them.

### Results

Median wall-clock time in milliseconds on an Apple M5 (10 cores, 32 GiB,
macOS 26.5.1) with a warm file cache, measured on September 25, 2026.
tsc-rs and tsgo ran in seven interleaved rounds per configuration, tsc in
three (two for VS Code), each after one warm-up run. All three compilers
received the same command line: `--pretty false -p <config>`, with
`--noEmit` added for VS Code. Compilers:

- tsc-rs built from commit
  [`1be415931`](https://github.com/kazhiramatsu/tsc-rs/commit/1be415931f7ed456cba6703250b461581ecb4e30)
  with `cargo build --release --locked` (Rust 1.93.0).
- tsgo 7.1.0-dev, an unmodified build of
  [TypeScript commit `1f70213d`](https://github.com/microsoft/TypeScript/commit/1f70213d4922b434345f639b441681e470c7cfc1)
  (September 4, 2026) with Go 1.26.0, using its default checker count.
- tsc 6.0.3 on Node.js 25.2.1 with `--max-old-space-size=8192`.

Type check only (`--noEmit`):

| Project | tsc-rs | tsgo | tsc | tsc-rs ÷ tsgo | tsc ÷ tsc-rs |
| --- | ---: | ---: | ---: | ---: | ---: |
| hono | 157 | 182 | 1,110 | 0.86 | 7.1 |
| zod | 754 | 1,001 | 5,468 | 0.75 | 7.3 |
| Playwright | 558 | 609 | 4,166 | 0.92 | 7.5 |
| TypeScript `src/compiler` | 365 | 360 | 2,729 | 1.01 | 7.5 |
| Next.js `packages/next` | 945 | 1,352 | 7,864 | 0.70 | 8.3 |
| VS Code `src` | 4,524 | 4,775 | 42,606 | 0.95 | 9.4 |

Compilation with output files:

| Project | Output | tsc-rs | tsgo | tsc | tsc-rs ÷ tsgo | tsc ÷ tsc-rs |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| hono | JavaScript | 168 | 188 | 1,168 | 0.89 | 7.0 |
| hono | JavaScript + declarations | 167 | 184 | 1,127 | 0.91 | 6.7 |
| hono | JavaScript + source maps | 157 | 176 | 1,078 | 0.89 | 6.9 |
| hono | All outputs | 179 | 207 | 1,218 | 0.87 | 6.8 |
| zod | JavaScript | 799 | 1,030 | 5,779 | 0.78 | 7.2 |
| zod | JavaScript + declarations | 885 | 1,214 | 6,375 | 0.73 | 7.2 |
| zod | JavaScript + source maps | 811 | 1,087 | 5,923 | 0.75 | 7.3 |
| zod | All outputs | 979 | 1,178 | 6,103 | 0.83 | 6.2 |
| Playwright | JavaScript | 617 | 671 | 4,574 | 0.92 | 7.4 |
| Playwright | JavaScript + declarations | 636 | 751 | 4,795 | 0.85 | 7.5 |
| Playwright | JavaScript + source maps | 656 | 699 | 4,742 | 0.94 | 7.2 |
| Playwright | All outputs | 690 | 838 | 5,108 | 0.82 | 7.4 |
| TypeScript `src/compiler` | JavaScript | 510 | 560 | 3,332 | 0.91 | 6.5 |
| TypeScript `src/compiler` | JavaScript + declarations | 515 | 608 | 3,433 | 0.85 | 6.7 |
| TypeScript `src/compiler` | JavaScript + source maps | 520 | 601 | 3,512 | 0.87 | 6.8 |
| TypeScript `src/compiler` | All outputs | 552 | 666 | 3,741 | 0.83 | 6.8 |
| Next.js `packages/next` | JavaScript | 1,041 | 1,459 | 8,657 | 0.71 | 8.3 |
| Next.js `packages/next` | JavaScript + declarations | 1,155 | 1,638 | 9,114 | 0.70 | 7.9 |
| Next.js `packages/next` | JavaScript + source maps | 1,064 | 1,539 | 9,001 | 0.69 | 8.5 |
| Next.js `packages/next` | All outputs | 1,210 | 1,704 | 9,544 | 0.71 | 7.9 |

A value below 1 in the `tsc-rs ÷ tsgo` column means tsc-rs finished
first; the last column is the speed-up over tsc. tsc-rs was faster than
tsgo on 25 of the 26 configurations and 6 to 9 times faster than tsc. On
the type check of the TypeScript compiler, whose critical path is the check
of one 53,000-line file, the two native compilers were within 2 % of each
other. Differences of a few percent are within the run-to-run variation
observed on this machine.

### Output comparison

Before timing, every configuration was compiled once with tsc and once with
tsc-rs, and the diagnostics and emitted file trees were compared byte for
byte.

| Project | Diagnostics | JavaScript | JavaScript + declarations | JavaScript + source maps | All outputs |
| --- | --- | --- | --- | --- | --- |
| hono | identical | identical | identical | identical | identical |
| zod | identical | identical | 3 of 942 files | identical | 3 of 1884 files |
| Playwright | identical | identical | 9 of 1409 files | identical | 9 of 2816 files |
| TypeScript `src/compiler` | identical | identical | identical | identical | identical |
| Next.js `packages/next` | identical | identical | 12 of 3330 files | identical | 13 of 6660 files |
| VS Code `src` | identical | not measured | not measured | not measured | not measured |

Diagnostics, JavaScript files and JavaScript source maps were identical on
every configuration. The declaration files that differ contain exactly the
same declarations: only the order of union constituents and of the properties
of inferred object types differs, because tsc-rs derives that order from the
type identities of its parallel checkers. The order is stable for a given
machine and checker count, and `TSRS_CHECKERS=1` (a single checker, the exact
serial mode) reproduces tsc's order at the cost of the parallel speed-up.
Declaration maps differ only for a declaration file that itself differs.

### Reproducing

The comparison above is a quick interleaved measurement on one machine, not
the project's formal protocol. For repeatable measurements with recorded
provenance, exact output verification before timing, several sessions and
confidence intervals, see [docs/benchmarking.md](docs/benchmarking.md) and
`scripts/benchmark-cli.py`. Watch mode and incremental builds are not
supported, and cold-cache, Linux and Windows timings were not measured.

## Run CI

The compiler is checked against TypeScript 6.0.3 reference results and
targeted regression tests. You can run these checks locally or inspect their
GitHub Actions logs.

### What CI verifies

| Check | What it verifies |
| --- | --- |
| **Acceptance** (`ci`) | Runs the upstream TypeScript diagnostic corpus and the supported compilation cases. Compares diagnostics and compiler output with recorded TypeScript results, and rejects regressions in the accepted diagnostic cases. GitHub splits this suite into `early`, `wide`, and `late` groups. |
| **Witness tests** (`witnesses`) | Checks focused regressions in JavaScript and declaration output, source maps, decorators, comments, Unicode handling, configuration, and file access. Includes comparisons with TypeScript reference results and tests of the Rust APIs. |
| **Rust checks** (local only) | Checks formatting, runs Clippy, and executes the workspace's unit and integration tests and other Cargo test targets. The hosted workflows do not run these three workspace-wide commands. |

For a recorded full run on commit
[`120d91465`](https://github.com/kazhiramatsu/tsc-rs/commit/120d91465b6d641974ca664eda46d9e16c693f72),
the head of the pull request merged into `main` as
[`1be415931`](https://github.com/kazhiramatsu/tsc-rs/commit/1be415931f7ed456cba6703250b461581ecb4e30),
see the successful [acceptance run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/36075010585)
and [witness run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/36075010620)
from September 24, 2026: all 22 test jobs and both aggregate checks passed.

These results demonstrate the behavior covered by those tests. Some cases
have explicitly recorded differences or unsupported outcomes; a passing
run does not imply complete TypeScript compatibility. See the
[current limitations](#current-limitations) for the user-facing restrictions.

### Run locally

Run these commands from the repository root using a POSIX shell (for example,
Bash or Zsh). In addition to the [build prerequisites](#build), install
Python 3.11 or newer, the Node.js version pinned in
[.node-version](.node-version), and the `zstd` command for reading compressed
test data. Rustup selects the Rust version and tools from
[rust-toolchain.toml](rust-toolchain.toml). The TypeScript reference, test
inputs, and expected results are checked in; no npm install is needed.

Use the GitHub Actions build settings and acceptance worker limits:

```sh
export CARGO_BUILD_JOBS=2
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_TEST_DEBUG=0
export RUSTC_WRAPPER=
# Worker limits for acceptance tests:
export TSRS_H2_5G_WORKERS=2
export TSRS_CONFORMANCE_WORKERS=2
```

Run the complete acceptance suite:

```sh
cargo xtask acceptance
```

This executes the test sequence covered by all three hosted acceptance
groups. To run just one group, use `early`, `wide`, or `late`, for example:

```sh
python3 .github/ci/replay.py acceptance late
```

Run all registered witness suites through the same runner used by GitHub
Actions. The first command reads the suite list so new suites are included
automatically:

```sh
export WITNESS_SUITES="$(
  PYTHONPATH=scripts python3 -c 'import json, witness; print(json.dumps(witness.SUITES))'
)"
python3 .github/ci/replay.py witnesses
```

To run only a specific witness suite, provide its name instead:

```sh
WITNESS_SUITES='["declaration-maps"]' python3 .github/ci/replay.py witnesses
```

For the additional Rust checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
RUST_TEST_THREADS=2 cargo test --workspace --all-targets
```

Each command reports its own result and returns a nonzero exit status on
failure. Full acceptance and witness runs can take substantial time,
especially on the first build; local runs execute the groups sequentially
instead of using separate hosted runners. For case selection and failure
investigation, see the [witness testing guide](docs/witness-testing.md).

### GitHub Actions

CI runs automatically when you open or update a pull request. Check the
results in the pull request's **Checks** tab. The workflows select tests
based on the changed files, so a documentation-only update may skip test
jobs. The `gates` and `witness-gates` checks require every selected job to
succeed.

To run CI manually, open the repository's **Actions** tab, choose **ci** or
**witnesses**, select **Run workflow**, and choose the branch. Run both
workflows to execute all registered acceptance and witness groups. Select
a run to view its progress and logs.

Alternatively, with GitHub CLI (`gh`) installed and authenticated to an
account that can run workflows, run these commands from the cloned
repository. Replace `your-branch` with a branch already pushed to GitHub:

```sh
gh workflow run ci.yml --ref your-branch
gh workflow run witness.yml --ref your-branch
gh run list --branch your-branch
```

Use a run ID from the list to watch a workflow or rerun its failed jobs:

```sh
gh run watch RUN_ID --exit-status
gh run rerun RUN_ID --failed
```

Replace `RUN_ID` with the relevant numeric ID. A rerun uses the original
commit; push fixes to the pull request branch to validate the updated code.

## Current limitations

- Some TypeScript options and configuration combinations are unsupported
  and return an error.
- The command runs one compilation at a time. Watch mode, `--build`, and
  project-reference builds are not supported.
- `--noEmit -p` does not accept command-line emit overrides such as
  `--target` or `--module`; see the
  [configuration for type checks](#configuration-for-type-checks).
- `--help`, `--init`, and `--showConfig` are not currently implemented.
  Create the configuration file using the examples above.
- The compiler command does not provide a language server or editor service.
- In emitted declaration files, the order of union constituents and of the
  properties of inferred object types follows the parallel checkers rather
  than `tsc`. The declarations themselves are the same; set
  `TSRS_CHECKERS=1` when a byte-identical `.d.ts` matters more than the
  parallel speed-up.

If a command reports an unsupported option, first check whether the option
belongs in `tsconfig.json`.

## License

[MIT](LICENSE).
