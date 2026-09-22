# tsc-rs

tsc-rs is a TypeScript compiler written in Rust. It checks types and compiles
TypeScript to JavaScript, with support for source maps and declaration files.
Its compatibility target is **TypeScript 6.0.3**.

See the [current limitations](#current-limitations) before adopting it for
an existing project. See [Run CI](#run-ci) for the checks used to validate
the compiler, how to reproduce them, and recorded results.

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

The `--noEmit` project command currently rejects some output settings,
including `rootDir` and `declaration`. It also rejects `--target` and
`--module` command-line overrides; put those settings in `tsconfig.json`.

The example keeps sources beside `tsconfig.json` and adds declaration
settings in a [separate configuration](#declaration-files), so its base
configuration works for both compilation and type checks. Use
`tsc-rs --noEmit -p .` to check it, including after adding source maps.

## Type-check from Rust (experimental)

**The Rust API is experimental and still under development. Its interfaces
may change incompatibly.**

The [complete Rust example](crates/compiler/examples/type_check.rs) reads a
`tsconfig.json`, forces `noEmit`, and returns diagnostics as JSON without
writing JavaScript or declaration files. It calls the Rust API directly and
does not require Node.js.

From the repository root, run it against the included TypeScript example:

```sh
cargo run --locked --manifest-path crates/compiler/Cargo.toml --example type_check -- \
  examples/type-check/tsconfig.json vendor/typescript-6.0.3/lib
```

The arguments are the configuration file and the directory containing
TypeScript's `lib.*.d.ts` files. This API requires the library directory to
be available at runtime; the checked-in `vendor/typescript-6.0.3/lib` supplies
the matching declarations. Replace the first argument with your project's
configuration file to check it instead. The same
[configuration restrictions](#configuration-for-type-checks) apply.

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

### Use the example in your application

For a Rust application located alongside the `tsc-rs` checkout, add these
dependencies to its `Cargo.toml`:

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
format. TypeScript errors are returned as diagnostics; the function's
`Err` indicates that the check could not complete.

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
to `false`. For type checks, use `tsc-rs --noEmit -p .`, following the
[configuration restrictions](#configuration-for-type-checks).

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

## Run CI

The compiler is checked against TypeScript 6.0.3 reference results and
targeted regression tests. You can run these checks locally or inspect their
GitHub Actions logs.

### What CI verifies

| Check | What it verifies |
| --- | --- |
| **Acceptance** (`ci`) | Runs the upstream TypeScript diagnostic corpus and the supported compilation cases. Compares diagnostics and compiler output with recorded TypeScript results, and rejects regressions in the accepted diagnostic cases. GitHub splits this suite into `early`, `wide`, and `late` groups. |
| **Witness tests** (`witnesses`) | Checks focused regressions in JavaScript and declaration output, source maps, decorators, comments, Unicode handling, configuration, and file access. Includes comparisons with TypeScript reference results and tests of the Rust APIs. |
| **Rust checks** (local commands below) | Checks formatting, runs Clippy, and executes the workspace's unit and integration tests and other Cargo test targets. These supplement the acceptance and witness workflows. |

For a recorded full run on commit
[`eb1442459`](https://github.com/kazhiramatsu/tsc-rs/commit/eb1442459311f383467f95de2413349de7aa58b6),
see the successful [acceptance run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35677992322)
and [witness run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35677992320)
from September 22, 2026: all 22 test jobs and both aggregate checks passed.

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

Use the same build and test settings as GitHub Actions:

```sh
export CARGO_BUILD_JOBS=2
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_TEST_DEBUG=0
export RUSTC_WRAPPER=
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
- Project type checks have [configuration restrictions](#configuration-for-type-checks).
- `--help`, `--init`, and `--showConfig` are not currently implemented.
  Create the configuration file using the examples above.
- The compiler command does not provide a language server or editor service.

If a command reports an unsupported option, first check whether the option
belongs in `tsconfig.json`.

## License

[MIT](LICENSE).
