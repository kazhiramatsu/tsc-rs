# tsc-rs

tsc-rs is a TypeScript compiler written in Rust. It checks types and compiles
TypeScript to JavaScript, with support for source maps and declaration files.
Its compatibility target is **TypeScript 7.1** (the native compiler, at a
pinned development commit); see [Reference](#reference). The same compiler
is available to Rust programs as a library; see the
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
Version 7.1.0-dev-19dadef8
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
  `--newLine`, `--emitBOM` and `--noEmitOnError`; keep those settings in
  `tsconfig.json` as well. The type check reads the whole file, including
  `rootDir` and `declaration`, and writes nothing but the build info of an
  `incremental` project (`--listEmittedFiles` lists it).

Watch mode is not supported. `tsc-rs -b` builds a project and the
projects it references in dependency order, as `tsgo --build` does (see
[build mode](#build-mode)); `-p` compiles one project against the built
outputs of the projects it references, and an `incremental` or
`composite` project writes and reuses its `.tsbuildinfo` as `tsgo` does;
see the [current limitations](#current-limitations).

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
produce declaration diagnostics; no files are written apart from an
`incremental` project's build info.

Emit settings cannot be overridden on the command line together with
`--noEmit -p`: flags such as `--target`, `--module`, `--newLine`,
`--emitBOM` and `--noEmitOnError` are rejected with an error on that
route. Put those settings in `tsconfig.json`. The example
project can be checked at any point with `tsc-rs --noEmit -p .`, including
after adding the source map and [declaration](#declaration-files) settings
below.

### Reference

tsc-rs follows TypeScript 7.1 at the vendored native profile
(`vendor/typescript-native/7.1.0-dev-19dadef8`, the commit `--version`
names), and the TypeScript 7.1 conformance baselines verify its
diagnostics and its emitted JavaScript, declaration files and source maps
(see [Run CI](#run-ci)). In particular:

- The standard library declarations are TypeScript 7.1's (`lib.es2026.*`
  exist, `esnext.array` and the other moved aliases name the ES2026
  files), the default `target` is ES2026, and `target: "es2026"` is
  accepted.
- Union members, the properties of inferred object types and `typeof`
  facts are ordered by their content, as TypeScript 7 does
  unconditionally; see [Stable type ordering](#stable-type-ordering).
- Diagnostic messages use TypeScript 7.1's catalog.
- A relation failure is reported by the row that names it when that row
  directly follows the head for the same source and target: a missing
  property (TS2741, TS2739, TS2740), a readonly array or tuple assigned
  to a mutable one (TS4104) or excessive complexity (TS2859).

The options TypeScript 7 removed (`baseUrl`, `outFile`,
`downlevelIteration`, `target: "es5"`, the `amd`, `umd` and `system` module
kinds, the `node10` and `classic` module resolutions, `alwaysStrict: false`,
`esModuleInterop: false` and `allowSyntheticDefaultImports: false`) report
TypeScript 7.1's TS5102 or TS5108 error ("Option ... has been removed.
Please remove it from your configuration.", with the `paths` suggestion for
`baseUrl`), and the program is still checked and emitted with the option in
effect. Like `tsc`, the command line reports no semantic errors while such
an option error exists. `ignoreDeprecations` is accepted but has no effect.
The option catalog is TypeScript 7.1's: the options TypeScript 5.5 removed
(`charset`, `out`, `keyofStringsOnly`, `noImplicitUseStrict`,
`noStrictGenericChecks`, `suppressExcessPropertyErrors`,
`suppressImplicitAnyIndexErrors`, `importsNotUsedAsValues`,
`preserveValueImports`) are unknown options (TS5023), `module: "none"` and
`target: "es3"` are not accepted values (TS6046), a programmatic `module`
of 0 means unspecified, and the default `moduleResolution` is `bundler`
for every module kind except `node16`, `node18`, `node20` and `nodenext`
(so `amd`, `umd` and `system` also report TS5095). An explicit `classic`
or `node10` still selects that resolver here, whereas TypeScript 7.1 maps
it to the default one; that difference is the next step of the cutover.
The behavior of the removed options themselves is not verified against a
reference (the TypeScript 7.1 conformance skips those configurations). The
tsc 6.0.3 compatible line ended with release v0.1.0 (tag `v0.1.0`, branch
`release/6.0.3`).

### Stable type ordering

TypeScript 6.0 added the `stableTypeOrdering` compiler option, which orders
union members, object properties and other internal lists by their content
instead of by the order in which the checker created them. TypeScript 7
always uses this order, and tsgo's output follows it. tsc-rs uses it by
default. `"stableTypeOrdering": false` in `tsconfig.json`, or
`--stableTypeOrdering false` on the command line, restores the creation
order of tsc 6.0; `--stableTypeOrdering` without a value selects the default
explicitly. The setting applies to both `--noEmit` checks and builds.

The order matters most for tsc-rs because it checks files on several
parallel checkers, each with its own creation order. In the creation order
the order of union members in diagnostics and declaration files follows
the checker that produced them, and the few inferences that depend on that
order can differ between checker counts (see the
[output comparison](#output-comparison)). In the content order the result
is the same for every checker count and matches `tsc --stableTypeOrdering`
and TypeScript 7, apart from the rare union whose members compare equal up
to the final tiebreak on type identity.

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
  `CheckerBudget::automatic()` and `WorkerBudget::automatic()`. The
  compiler recurses on the native stack in proportion to the nesting depth
  of a source. The command runs its work, and every worker and checker
  thread, on a stack reservation of 1 GiB (`tsc_program::WORKER_STACK_BYTES`;
  virtual, committed only as the recursion uses it), which takes a source
  as deep as tsgo compiles. With the serial defaults the API runs on the
  calling thread, whose stack the embedding chooses; to compile sources of
  unusual depth, run the session on a thread built with
  `std::thread::Builder::new().stack_size(tsc_program::WORKER_STACK_BYTES)`.
- `tsc-rs-diagnostics` (`tsc_diagnostics`): `Diagnostic`, with its code,
  category, message chain and UTF-16 location, and `TextSnapshot` for line
  and column lookup.

The API reads the standard library declarations from the directory given to
`LibraryCatalog::typescript_7_1`; the checked-in
`vendor/typescript-native/7.1.0-dev-19dadef8/upstream/tsc/internal/bundled/libs`
supplies the matching files. The copy embedded in the `tsc-rs` executable
is not exposed through the API.

### Check a project

The [complete example](crates/compiler/examples/type_check.rs) reads a
`tsconfig.json`, forces `noEmit`, and returns diagnostics as JSON without
writing JavaScript or declaration files. From the repository root, run it
against the included TypeScript example:

```sh
cargo run --locked --manifest-path crates/compiler/Cargo.toml --example type_check -- \
  examples/type-check/tsconfig.json \
  vendor/typescript-native/7.1.0-dev-19dadef8/upstream/tsc/internal/bundled/libs
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
cargo run -- ../tsc-rs/examples/type-check/tsconfig.json \
  ../tsc-rs/vendor/typescript-native/7.1.0-dev-19dadef8/upstream/tsc/internal/bundled/libs
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

The command line is parsed as `tsgo` parses it: every compiler option of
`tsconfig.json` is accepted as `--name value` (a boolean as `--name` or
`--name false`, a list as comma-separated values, `--name null` to clear
the configuration's value; names are case-insensitive and the short names
`-p`, `-t`, `-m`, `-d`, `-i`, `-w`, `-v`, `-h` exist), and the values given
replace the configuration's. There is no `--name=value` form. An unknown
option, a bad value or a conflicting selection is reported as the
TypeScript diagnostic `tsgo` prints (TS5023, TS5025, TS6046, TS5042, TS5058,
TS5081, TS5112, …) with exit status 1. `@file` reads further arguments from
a response file.

| Option | Use |
| --- | --- |
| `--version` | Print the TypeScript compatibility version. |
| `-p`, `--project <path>` | Select a project directory or configuration file. |
| `--noEmit` | Check the project without writing compiler output. |
| `--noEmit false` | Enable output even when the configuration sets `noEmit`. |
| `--target <version>` | Set the JavaScript target for compilation, such as `ES2022`. |
| `--module <format>` | Set the module format, such as `CommonJS`, `ESNext`, or `Preserve`. |
| `--noEmitOnError` | Prevent output when diagnostics are present. |
| `--listEmittedFiles` | Print the paths of generated files. |
| `--listFiles` | Print the names of the files in the program after the diagnostics. |
| `--explainFiles` | Print why each file is in the program (its include reasons, redirects and module format), as `tsgo --explainFiles` does. |
| `--listFilesOnly` | Print the names of the files without emitting or type-checking (syntactic and option diagnostics only); not accepted with `-b`. |
| `--pretty false` | Use plain diagnostic output. |
| `--ignoreConfig` | Compile explicit files without loading a discovered configuration. |
| `--newLine lf` | Use LF line endings in generated output; `crlf` is also accepted. |
| `--emitBOM` | Write a byte-order mark to output files. |
| `--useDefineForClassFields <boolean>` | Select define semantics for class fields. |
| `-b`, `--build [projects...]` | Build the projects and their references in dependency order (must be the first argument). |
| `-v`, `--verbose` | With `-b`: print what each project needs and why. |
| `-d`, `--dry` | With `-b`: print what a build would do without doing it. |
| `-f`, `--force` | With `-b`: build every project, up to date or not. |
| `--clean` | With `-b`: delete the outputs of the projects. |
| `--stopBuildOnErrors` | With `-b`: skip the projects whose dependencies have errors. |

To see which files a project build writes:

```sh
tsc-rs -p . --listEmittedFiles
```

Compiler settings such as `strict`, `outDir`, `sourceMap`, and `declaration`
are accepted on the command line as well as in `tsconfig.json`, within the
same option support as the configuration file. `--init`, `--help`,
`--watch`, `--showConfig` and a non-English `--locale` are not supported and
are reported as a `tsc-rs:` usage error with exit status 2. For `--noEmit`,
see [configuration for type checks](#configuration-for-type-checks).

## Build mode

`tsc-rs -b` (or `--build`) is `tsgo --build`: the projects named on the
command line (the current directory when none is named) and every project
they reference are built upstream first.

```sh
tsc-rs -b tests --verbose
tsc-rs -b packages/core packages/app --dry
tsc-rs -b --clean
```

Each project is compared with its previous build before it is built: the
build info every project of a build writes (an `incremental` or
`composite` project's full one; for the other projects a small one that
records the roots and whether errors were reported), the modification
times and contents of its inputs, its outputs, its referenced projects'
declaration files, its config files and the `package.json` files it read.
A project whose inputs did not change is skipped; one whose upstream
projects changed only in ways that leave their declaration files alone has
its outputs' timestamps updated instead of being rebuilt; the others are
compiled with their old build info exactly as `-p` compiles them. The
projects are built one after the other in the order `tsgo` reports them
(`--builders` is accepted and ignored). `--verbose` prints the reasons,
`--dry` the decisions, `--force` rebuilds everything, `--clean` deletes
the outputs (`--clean --dry` lists them), and `--stopBuildOnErrors` skips
the projects whose dependencies failed. The compiler options accepted by
`-p` apply to every project of the build. The exit status is `tsgo`'s:
`0`, `1` (diagnostics, outputs skipped), `2` (diagnostics, outputs
written) or `4` (the references form a cycle). `--watch` is not
supported, and a project whose configuration file has errors is reported
without being built.

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

These comparisons were made with tsc 6.0.3's creation order on both sides
(`stableTypeOrdering: false` in tsc-rs since that order stopped being the
default; see [Reference](#reference)). Diagnostics were
identical on every configuration except Effect, where
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
checkers. The order is stable for a given machine and checker count, and
`TSRS_CHECKERS=1` (a single checker, the exact serial mode) reproduces
tsc's order at the cost of the parallel speed-up. Declaration maps differ
only for a declaration file that itself differs.

[Stable type ordering](#stable-type-ordering), tsc-rs's default, removes
the dependence on the checker partition. With the content order on both
sides, the same comparison against `tsc --stableTypeOrdering` gave identical
diagnostics on every project at every checker count tried (1, 6, 8 and 12
on Effect, where both compilers report the two Schema errors that
TypeScript 7 also reports), identical declaration files for zod,
Playwright and Next.js, and for Effect identical declaration files with a
single checker. With several checkers one Effect file,
`ai/internal/mcpProtocol/v2026_07_28.d.ts`, prints one union of three
mapped types in a different member order: the three compare equal up to
the comparison's final tiebreak, the type id, which with several checkers
is a checker-local creation id. TypeScript 7 keeps the same tiebreak.

### Reproducing

The measurements above are a quick interleaved run on one machine, not the
project's formal protocol. For repeatable measurements with recorded
provenance, exact output verification before timing, several sessions and
confidence intervals, see [docs/benchmarking.md](docs/benchmarking.md) and
`scripts/benchmark-cli.py`. Watch mode is not supported, incremental
rebuilds and `--build` were not timed, and cold-cache, Linux and Windows
timings were not measured.

## Run CI

The compiler is checked against TypeScript 7.1's conformance baselines and
the workspace's Rust tests. You can run these checks locally or inspect
their GitHub Actions logs.

### What CI verifies

| Check | What it verifies |
| --- | --- |
| **TypeScript 7.1 conformance** (`conformance (TypeScript 7.1)`) | Runs the `compiler` and `conformance` test cases vendored from TypeScript 7.1 through the native test runner's configuration expansion, on one checker, and compares the diagnostics (`.errors.txt`), the emitted JavaScript, declaration files and source maps (`.js`, `.js.map`) the type and symbol baselines (`.types`, `.symbols`: the type and the symbol at every expression and declaration name, as tsgo's test runner writes them) the source map records (`.sourcemap.txt`: every emitted line with its spans against the source text, as tsgo's recorder writes them) and the module resolution traces (`.trace.json`: the `--traceResolution` lines, sanitized as tsgo's harness does) with the vendored reference baselines byte for byte (`scripts/conformance_ts71.py --check`). The ratchet `ratchets/ts71/` records the error tier, the emit tier and the type, symbol, source map record and trace tiers each configuration has reached and rejects regressions. A sharded control (`--checkers 4`) is compared with the one-checker report by `scripts/conformance_ts71_compare.py`; the design packet records the configurations where they differ. The configurations the native runner itself skips (`target: es5`, the `umd` and `system` module kinds, the `node10` and `classic` resolutions, `baseUrl`, `esModuleInterop: false`, `alwaysStrict: false`) or never produces baselines for (`amd`, `outFile`, its skip list) are not compared. The same job then runs the `transpile` test cases through single-file transpilation (`transpileModule` and `transpileDeclaration`) as the native transpile runner does, and the command-line and tsconfig parsing tests (the arguments and configuration files the native Go tests keep in their tables), and compares each of their baselines (`transpile`, `tsoptions/commandLineParsing`, `config/tsconfigParsing`) byte for byte (`scripts/suites_ts71.py --check`, ratchet `ratchets/ts71/suites-<profile>.tsv`). |
| **Rust checks** (`rust`) | Checks formatting, runs Clippy over every target of the workspace, executes every workspace test target (`cargo test --workspace`) and verifies that the generated diagnostic catalog is current (`python3 .github/ci/replay.py rust`). |

The `gates` check requires both jobs to succeed; a change that touches only
`docs/`, this file, `CONTRIBUTING.md` or `LICENSE` selects neither.

These results demonstrate the behavior covered by those tests. Some cases
have recorded differences from the reference; a passing run does not imply
complete TypeScript compatibility. See the
[current limitations](#current-limitations) for the user-facing restrictions.

### Run locally

Run these commands from the repository root using a POSIX shell (for example,
Bash or Zsh). In addition to the [build prerequisites](#build), install
Python 3.11 or newer. Rustup selects the Rust version and tools from
[rust-toolchain.toml](rust-toolchain.toml). The TypeScript test inputs and
reference baselines are checked in; no npm install is needed.

Use the GitHub Actions build settings:

```sh
export CARGO_BUILD_JOBS=2
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_TEST_DEBUG=0
export RUSTC_WRAPPER=
```

Run the Rust checks (formatting, Clippy, the workspace tests and the
generated diagnostic catalog):

```sh
python3 .github/ci/replay.py rust
```

Run the TypeScript 7.1 conformance comparison (about twenty minutes with
four workers):

```sh
cargo build --release -p tsc-rs-conformance --bin conformance-ts71
python3 scripts/conformance_ts71.py --workers 4 --check
```

To compare one case and keep tsc-rs's rendering of the differing baselines:

```sh
./target/release/conformance-ts71 --case compiler/2dArrays.ts --no-report --dump target/conformance-dump
```

The report of a full run is `target/conformance-ts71/<profile>/report.json`.
After an intentional change, `scripts/conformance_ts71.py --workers 4
--update` records the new tiers in the ratchet without lowering any.

Run the comparison of the other suites (transpile, command-line and
tsconfig parsing; a few seconds):

```sh
cargo build --release -p tsc-rs-conformance --bin suites-ts71
python3 scripts/suites_ts71.py --check
```

Its report is `target/suites-ts71/<profile>/report.json`; `--dump
<directory>` keeps tsc-rs's rendering of the differing baselines, and
`--update` records the new tiers without lowering any.

### GitHub Actions

`.github/workflows/ci.yml` runs on pull requests, merge groups and pushes
to `main`: the `plan` job selects the jobs from the changed paths, the
`rust` and `conformance (TypeScript 7.1)` jobs run with two Cargo build
workers, and `gates` requires every selected job to succeed. The workflow's
runs are listed under the repository's Actions tab.

## Current limitations

- Some TypeScript options and configuration combinations are unsupported
  and return an error.
- Watch mode is not supported. `-p` compiles one project and reads the
  referenced projects' built outputs (it does not build them); `-b` builds
  the projects one after the other (`--builders` is accepted and ignored)
  and reports a project whose configuration file has errors without
  building it. An `incremental` or `composite` project writes the same
  `.tsbuildinfo` as `tsgo` and reuses it on the next compilation as `tsgo`
  does: files the build info still covers are not checked again, only the
  files whose output may have changed are emitted, and the build info is
  rewritten only when it changed.
- `--noEmit -p` does not accept command-line emit overrides such as
  `--target` or `--module`; see the
  [configuration for type checks](#configuration-for-type-checks).
- `--help`, `--init`, and `--showConfig` are not currently implemented.
  Create the configuration file using the examples above.
- The compiler command does not provide a language server or editor service.
- Peak memory grows with the checker count (eight by default, twelve when
  writing declaration files); see [peak memory](#peak-memory). A lower
  `TSRS_CHECKERS` count reduces it at some cost in speed.
- With `stableTypeOrdering: false` (tsc 6.0's creation order), the
  diagnostics of a program can depend on how its files are partitioned
  among the checkers: on Effect, the default eight checkers report two
  errors that `tsc` and a run with six or fewer checkers do not; see the
  [output comparison](#output-comparison). In the same setting the order
  of union constituents and of the properties of inferred object types in
  emitted declaration files follows the parallel checkers rather than
  `tsc`; the declarations themselves are the same. Keep the default
  [stable type ordering](#stable-type-ordering), or set `TSRS_CHECKERS=1`
  when a byte-identical `.d.ts` in tsc 6.0's creation order matters more
  than the parallel speed-up.
- Type checking and emit follow TypeScript 7.1 but do not yet reproduce
  every one of its conformance baselines; the `conformance (TypeScript
  7.1)` job and `ratchets/ts71/` record the current state (see
  [Run CI](#run-ci)).

If a command reports an unsupported option, first check whether the option
belongs in `tsconfig.json`.

## License

[MIT](LICENSE).
