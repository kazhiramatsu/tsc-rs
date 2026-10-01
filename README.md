# tsc-rs

tsc-rs is a TypeScript compiler written in Rust. It checks types and compiles
TypeScript to JavaScript, with support for source maps and declaration files.
Its compatibility target is **TypeScript 6.0.3**. The same compiler is
available to Rust programs as a library; see the
[compiler API](#compiler-api-for-rust-projects-experimental).

See the [current limitations](#current-limitations) before adopting it for
an existing project. See [Performance](#performance) for measured compile
times and memory use against `tsc` and `tsgo` on real projects, and
[Run CI](#run-ci) for the checks used to validate the compiler, how to
reproduce them, and recorded results.

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

### Stable type ordering

TypeScript 6.0 added the `stableTypeOrdering` compiler option, which orders
union members, object properties and other internal lists by their content
instead of by the order in which the checker created them. TypeScript 7
always uses this order, and tsgo's output follows it. tsc-rs implements the
option: set `"stableTypeOrdering": true` in `tsconfig.json`, or pass
`--stableTypeOrdering` on the command line (`--stableTypeOrdering false`
turns a configured option off). It applies to both `--noEmit` checks and
builds.

The option matters most for tsc-rs because it checks files on several
parallel checkers, each with its own creation order. Without the option
the order of union members in diagnostics and declaration files follows
the checker that produced them, and the few inferences that depend on that
order can differ between checker counts (see the
[output comparison](#output-comparison)). With the option the result is
the same for every checker count and matches `tsc --stableTypeOrdering`.
The option is off by default, as in tsc 6.0, so default runs still match
tsc 6.0.3's default output; a project that wants the TypeScript 7 behavior
enables it.

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
several threads and writes its output files in parallel. By default it runs
one checker thread per hardware thread, up to eight, and one and a half
times as many when it writes declaration files. The standard library
declarations are embedded, so a run has no JavaScript runtime start-up. The
measurements below compare its compile time and memory use with TypeScript
6.0.3 (`tsc`, running on Node.js) and with the native TypeScript 7 preview
compiler (`tsgo`) on real projects.

### Measured projects

Each project was checked out at the commit shown, with its dependencies
installed, and compiled with its own configuration file (`tsconfig.json`
of the directory shown, or the file named). Derived configurations change
only the output: the output mode (`noEmit`; JavaScript; JavaScript and
declaration files; JavaScript and source maps; all outputs with
`declaration`, `declarationMap` and `sourceMap`), a separate output
directory for each mode that is kept out of the program's inputs (with
`rootDir` where needed), and `composite` and `incremental` turned off. The
TypeScript compiler's configurations also turn off `emitDeclarationOnly`
and `isolatedDeclarations`, which the project sets for its
declaration-only build. VS Code was measured with `--noEmit` only.

| Project | Commit | Program files | Source lines |
| --- | --- | ---: | ---: |
| [hono](https://github.com/honojs/hono) (`tsconfig.build.json`) | `8dcd52b` | 362 | 25,947 |
| [zod](https://github.com/colinhacks/zod) | `2bf7b06` | 2,364 | 117,060 |
| [Playwright](https://github.com/microsoft/playwright) | `ec31a7b` | 1,505 | 154,645 |
| [TypeScript](https://github.com/microsoft/TypeScript) 6.0.3, `src/compiler` | `050880ce5` | 249 | 194,701 |
| [Next.js](https://github.com/vercel/next.js), `packages/next` | `1edced6f` | 2,866 | 314,268 |
| [Effect](https://github.com/Effect-TS/effect) 4.0.0-rc.118, `packages/effect` | `cbfc7b4` | 689 | 370,541 |
| [VS Code](https://github.com/microsoft/vscode), `src` | `29b68000` | 10,272 | 3,048,452 |

Program files count every file in the program as listed by
`tsc --listFiles`, including standard library and `node_modules`
declaration files. Source lines count the program's non-declaration
TypeScript files. zod, Playwright, Next.js and VS Code report type errors
at these commits with the configurations used; all compilers report them.
Effect, whose library code relies heavily on type-level computation, is
error-free under tsc; see the [output comparison](#output-comparison) for
what tsc-rs reports on it.

### Method

The measurements were taken on October 1, 2026 on an Apple M5 (10 cores,
32 GiB, macOS 26.5.1) running on AC power, with a warm file cache and a
desktop session in the background (load average about 3 at the start).
After one warm-up run of every configuration, tsc-rs and tsgo ran in seven
interleaved rounds per configuration and tsc in three (two for VS Code).
The same rounds also ran tsc-rs with four checkers, described under
[peak memory](#peak-memory). All compilers received the same command
line, `--pretty false -p <config>` with `--noEmit` added for VS Code, at
the same scheduling priority (`nice -n 20`). The tables show medians:
wall-clock time in milliseconds, and peak memory as the maximum resident
set size of the compiler process reported by `wait4`, in MiB.
Compilers:

- tsc-rs built from commit
  [`8761d2de6`](https://github.com/kazhiramatsu/tsc-rs/commit/8761d2de6dc483b39ed1b519f060b7eefa17616f)
  with `cargo build --release --locked` (Rust 1.93.0).
- tsgo 7.1.0-dev, an unmodified build of
  [TypeScript commit `1f70213d`](https://github.com/microsoft/TypeScript/commit/1f70213d4922b434345f639b441681e470c7cfc1)
  (September 4, 2026) with Go 1.26.0, using its default of four checkers.
- tsc 6.0.3 on Node.js 25.2.1 with `--max-old-space-size=8192`.

### Compile time

Type check only (`--noEmit`):

| Project | tsc-rs | tsgo | tsc | tsc-rs ÷ tsgo | tsc ÷ tsc-rs |
| --- | ---: | ---: | ---: | ---: | ---: |
| hono | 118 | 161 | 959 | 0.74 | 8.1 |
| zod | 512 | 946 | 5,380 | 0.54 | 10.5 |
| Playwright | 355 | 617 | 4,424 | 0.58 | 12.5 |
| TypeScript `src/compiler` | 335 | 359 | 2,732 | 0.93 | 8.2 |
| Next.js `packages/next` | 798 | 1,419 | 7,949 | 0.56 | 10.0 |
| Effect `packages/effect` | 502 | 791 | 5,417 | 0.63 | 10.8 |
| VS Code `src` | 3,427 | 4,763 | 42,891 | 0.72 | 12.5 |

Compilation with output files:

| Project | Output | tsc-rs | tsgo | tsc | tsc-rs ÷ tsgo | tsc ÷ tsc-rs |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| hono | JavaScript | 129 | 176 | 1,040 | 0.73 | 8.1 |
| hono | JavaScript + declarations | 134 | 187 | 1,118 | 0.72 | 8.3 |
| hono | JavaScript + source maps | 133 | 181 | 1,071 | 0.73 | 8.1 |
| hono | All outputs | 146 | 206 | 1,186 | 0.71 | 8.1 |
| zod | JavaScript | 568 | 978 | 5,609 | 0.58 | 9.9 |
| zod | JavaScript + declarations | 599 | 1,045 | 5,740 | 0.57 | 9.6 |
| zod | JavaScript + source maps | 564 | 1,013 | 5,793 | 0.56 | 10.3 |
| zod | All outputs | 622 | 1,075 | 5,990 | 0.58 | 9.6 |
| Playwright | JavaScript | 411 | 682 | 4,623 | 0.60 | 11.2 |
| Playwright | JavaScript + declarations | 446 | 741 | 4,858 | 0.60 | 10.9 |
| Playwright | JavaScript + source maps | 420 | 684 | 4,779 | 0.61 | 11.4 |
| Playwright | All outputs | 487 | 789 | 5,178 | 0.62 | 10.6 |
| TypeScript `src/compiler` | JavaScript | 480 | 543 | 3,420 | 0.88 | 7.1 |
| TypeScript `src/compiler` | JavaScript + declarations | 478 | 597 | 3,482 | 0.80 | 7.3 |
| TypeScript `src/compiler` | JavaScript + source maps | 511 | 609 | 3,580 | 0.84 | 7.0 |
| TypeScript `src/compiler` | All outputs | 508 | 645 | 3,683 | 0.79 | 7.2 |
| Next.js `packages/next` | JavaScript | 848 | 1,461 | 8,744 | 0.58 | 10.3 |
| Next.js `packages/next` | JavaScript + declarations | 983 | 1,681 | 9,366 | 0.58 | 9.5 |
| Next.js `packages/next` | JavaScript + source maps | 894 | 1,600 | 9,319 | 0.56 | 10.4 |
| Next.js `packages/next` | All outputs | 1,022 | 1,693 | 9,510 | 0.60 | 9.3 |
| Effect `packages/effect` | JavaScript | 560 | 826 | 5,759 | 0.68 | 10.3 |
| Effect `packages/effect` | JavaScript + declarations | 755 | 1,092 | 6,909 | 0.69 | 9.2 |
| Effect `packages/effect` | JavaScript + source maps | 583 | 891 | 6,066 | 0.65 | 10.4 |
| Effect `packages/effect` | All outputs | 786 | 1,165 | 7,252 | 0.67 | 9.2 |

A value below 1 in the `tsc-rs ÷ tsgo` column means tsc-rs finished
first; the last column is the speed-up over tsc. tsc-rs was faster than
tsgo on all 31 configurations, taking 0.54 to 0.93 of tsgo's time, and 7
to 12.5 times faster than tsc. The type check of the TypeScript compiler is
the closest case: its critical path is the check of one 54,000-line file.
Differences of a few percent are within the run-to-run variation observed
on this machine.

### Peak memory

Type check only (`--noEmit`), in MiB:

| Project | tsc-rs | tsgo | tsc | tsc-rs ÷ tsgo | tsc-rs ÷ tsc |
| --- | ---: | ---: | ---: | ---: | ---: |
| hono | 306 | 326 | 520 | 0.94 | 0.59 |
| zod | 1,310 | 1,774 | 2,069 | 0.74 | 0.63 |
| Playwright | 754 | 1,041 | 1,299 | 0.72 | 0.58 |
| TypeScript `src/compiler` | 291 | 404 | 710 | 0.72 | 0.41 |
| Next.js `packages/next` | 1,341 | 1,710 | 2,381 | 0.78 | 0.56 |
| Effect `packages/effect` | 1,034 | 1,209 | 1,657 | 0.86 | 0.62 |
| VS Code `src` | 5,475 | 6,236 | 7,386 | 0.88 | 0.74 |

Compilation with output files, in MiB:

| Project | Output | tsc-rs | tsgo | tsc | tsc-rs ÷ tsgo | tsc-rs ÷ tsc |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| hono | JavaScript | 299 | 350 | 536 | 0.86 | 0.56 |
| hono | JavaScript + declarations | 352 | 375 | 551 | 0.94 | 0.64 |
| hono | JavaScript + source maps | 310 | 356 | 541 | 0.87 | 0.57 |
| hono | All outputs | 349 | 371 | 553 | 0.94 | 0.63 |
| zod | JavaScript | 1,343 | 1,772 | 2,111 | 0.76 | 0.64 |
| zod | JavaScript + declarations | 1,530 | 1,952 | 2,130 | 0.78 | 0.72 |
| zod | JavaScript + source maps | 1,350 | 1,823 | 2,118 | 0.74 | 0.64 |
| zod | All outputs | 1,540 | 1,903 | 2,144 | 0.81 | 0.72 |
| Playwright | JavaScript | 800 | 1,153 | 1,366 | 0.69 | 0.59 |
| Playwright | JavaScript + declarations | 899 | 1,329 | 1,371 | 0.68 | 0.66 |
| Playwright | JavaScript + source maps | 811 | 1,195 | 1,359 | 0.68 | 0.60 |
| Playwright | All outputs | 903 | 1,376 | 1,370 | 0.66 | 0.66 |
| TypeScript `src/compiler` | JavaScript | 457 | 578 | 758 | 0.79 | 0.60 |
| TypeScript `src/compiler` | JavaScript + declarations | 475 | 661 | 768 | 0.72 | 0.62 |
| TypeScript `src/compiler` | JavaScript + source maps | 457 | 598 | 773 | 0.76 | 0.59 |
| TypeScript `src/compiler` | All outputs | 475 | 667 | 789 | 0.71 | 0.60 |
| Next.js `packages/next` | JavaScript | 1,376 | 1,793 | 2,497 | 0.77 | 0.55 |
| Next.js `packages/next` | JavaScript + declarations | 1,450 | 1,977 | 2,630 | 0.73 | 0.55 |
| Next.js `packages/next` | JavaScript + source maps | 1,380 | 1,838 | 2,600 | 0.75 | 0.53 |
| Next.js `packages/next` | All outputs | 1,464 | 2,048 | 2,547 | 0.71 | 0.57 |
| Effect `packages/effect` | JavaScript | 1,119 | 1,291 | 1,724 | 0.87 | 0.65 |
| Effect `packages/effect` | JavaScript + declarations | 1,322 | 1,731 | 1,891 | 0.76 | 0.70 |
| Effect `packages/effect` | JavaScript + source maps | 1,120 | 1,330 | 1,724 | 0.84 | 0.65 |
| Effect `packages/effect` | All outputs | 1,338 | 1,792 | 1,905 | 0.75 | 0.70 |

A value below 1 in the ratio columns means tsc-rs used less memory. tsc-rs
used 0.66 to 0.94 times as much memory as tsgo and 0.41 to 0.74 times as
much as tsc, less than both on every configuration, although each checker
thread keeps its own type tables and tsc-rs ran eight checkers here
(twelve when writing declaration files) against tsgo's default of four,
except for the TypeScript compiler, where one file dominates the check and
fewer checkers are used. With `TSRS_CHECKERS=4` in the environment, tsc-rs
used 0.51 to 0.82 times tsgo's memory in 0.62 to 1.06 of its time,
finishing first on every configuration but Effect's two declaration
builds. Where memory matters more than speed, set `TSRS_CHECKERS` to a
lower count.

### Output comparison

Before the timing runs, every configuration was compiled once with tsc and
once with the measured tsc-rs build, and the diagnostics, exit statuses and
emitted file trees were compared byte for byte.

| Project | Diagnostics | JavaScript | JavaScript + declarations | JavaScript + source maps | All outputs |
| --- | --- | --- | --- | --- | --- |
| hono | identical | identical | identical | identical | identical |
| zod | identical | identical | 3 of 942 files | identical | 3 of 1884 files |
| Playwright | identical | identical | 9 of 1409 files | identical | 9 of 2816 files |
| TypeScript `src/compiler` | identical | identical | identical | identical | identical |
| Next.js `packages/next` | identical | identical | 12 of 3330 files | identical | 13 of 6660 files |
| Effect `packages/effect` | differ | identical | 18 of 992 files | identical | 29 of 1984 files |
| VS Code `src` | identical | identical | identical | identical | identical |

Diagnostics were identical on every configuration except Effect, where
tsc-rs with its default eight checkers reports two errors in
`src/Stream.ts` (TS2375 at line 5009 and TS2345 at line 5059, both about
an `Effect<void, never, never>` union constituent under
`exactOptionalPropertyTypes`) that tsc does not report. The inference
behind them picks the first member of a union, and tsc orders union
members by the order in which its single checker created the types; a
parallel checker that checks `Stream.ts` without having created
`Effect<void, never, never>` before orders the union differently. tsc
itself reports the same two errors when the `void` declaration is checked
after its use. The difference depends on how the files are partitioned
among the checkers: with six or fewer checkers (`TSRS_CHECKERS=6`) the
diagnostics are identical to tsc's. (The tsgo preview also reports errors
on Effect that tsc does not, in other files.) JavaScript files and
JavaScript source maps were identical on every configuration. The
declaration files that differ contain the same declarations: the order of
union constituents and of the properties of inferred object types differs,
because tsc-rs derives that order from the type identities of its parallel
checkers, and in Effect's `Runtime.d.ts` two type parameters that tsc
prints as `A` are printed as `A_1` and `A_2`. The order is stable for a
given machine and checker count, and `TSRS_CHECKERS=1` (a single checker,
the exact serial mode) reproduces tsc's order at the cost of the parallel
speed-up. Declaration maps differ only for a declaration file that itself
differs.

[Stable type ordering](#stable-type-ordering) removes the dependence on
the checker partition. With `stableTypeOrdering` enabled on both sides,
the same comparison against `tsc --stableTypeOrdering` gave identical
diagnostics on every project at every checker count tried (1, 6, 8 and 12
on Effect, where both compilers report the two Schema errors that
TypeScript 7 also reports), identical declaration files for zod,
Playwright and Next.js, and for Effect one differing declaration file
with a single checker, `Runtime.d.ts` (the `A_1`/`A_2` naming above), plus
`ai/internal/mcpProtocol/v2026_07_28.d.ts` with several checkers. In that
file one union of three mapped types compares equal up to the comparison's
final tiebreak, the type id, which with several checkers is a checker-local
creation id; TypeScript 7 keeps the same tiebreak.

### Reproducing

The measurements above are a quick interleaved run on one machine, not the
project's formal protocol. For repeatable measurements with recorded
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
[`d8e0b2f56`](https://github.com/kazhiramatsu/tsc-rs/commit/d8e0b2f5667f881ab06b0aaae29a6016a421d7d1),
the head of the pull request merged into `main` as
[`112924652`](https://github.com/kazhiramatsu/tsc-rs/commit/1129246526e2f2985a579796b84f3c0f215d0f27)
with the same source tree, see the successful
[acceptance run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/36494313517)
and [witness run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/36494313524)
from September 28, 2026 (UTC): all 22 test jobs and both aggregate checks
passed.

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
- Peak memory grows with the checker count (eight by default, twelve when
  writing declaration files); see [peak memory](#peak-memory). A lower
  `TSRS_CHECKERS` count reduces it at some cost in speed.
- Without `stableTypeOrdering`, the diagnostics of a program can depend on
  how its files are partitioned among the checkers: on Effect, the default
  eight checkers report two errors that `tsc` and a run with six or fewer
  checkers do not; see the [output comparison](#output-comparison).
- Without `stableTypeOrdering`, the order of union constituents and of the
  properties of inferred object types in emitted declaration files follows
  the parallel checkers rather than `tsc`. The declarations themselves are
  the same. Enable [stable type ordering](#stable-type-ordering), which
  matches `tsc --stableTypeOrdering` and TypeScript 7, or set
  `TSRS_CHECKERS=1` when a byte-identical `.d.ts` in tsc 6.0's default
  order matters more than the parallel speed-up.

If a command reports an unsupported option, first check whether the option
belongs in `tsconfig.json`.

## License

[MIT](LICENSE).
