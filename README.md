# tsc-rs

tsc-rs is a TypeScript compiler written in Rust. It checks types and compiles
TypeScript to JavaScript, with support for source maps and declaration files.
Its compatibility target is **TypeScript 6.0.3**.

See the [current limitations](#current-limitations) before adopting it for
an existing project.

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
