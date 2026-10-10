# Rust API (experimental)

**The Rust API is experimental and still under development. Its interfaces
may change incompatibly.**

tsc-rs is a set of Rust crates, and the `tsc-rs` command is built on them.
A Rust project can depend on the same crates to load a TypeScript project,
check it, and produce JavaScript, declaration files and source maps inside
its own process, with no Node.js and no separate compiler executable.

The API has five parts:

- `tsc-rs-host` (`tsc_host`): the read-only `CompilerHost` trait.
  `FsCompilerHost` implements it over the real filesystem and
  `MemoryCompilerHost` over files supplied in memory. `tsc_host::vfs` is the
  file-system layer the command line runs on: the `FileSystem` trait, `OsFs`
  for the process's file system, `MemFs` (an in-memory file system with
  case folding, symbolic links and modification times from a `Clock`) and
  `VfsCompilerHost`, a `CompilerHost` over any `FileSystem`.
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
  `LiveProgram` keeps one prepared program and its checker between calls,
  as the API server does: a file's syntactic, semantic and suggestion
  diagnostics are produced when they are asked for (each file is checked
  on its first request), the program-wide and global diagnostics follow
  tsgo's program getters, and `with_checker` runs a query over the checker.
- `tsc-rs-diagnostics` (`tsc_diagnostics`): `Diagnostic`, with its code,
  category, message chain and UTF-16 location, and `TextSnapshot` for line
  and column lookup.
- `tsc-rs-api` (`tsc_api`): the binary syntax tree format of TypeScript
  7.1's API (protocol 9), which `tsgo`'s API server sends its clients.
  `parse_source_file` parses a file as that server does,
  `references::collect_external_module_references` finds its imports,
  module augmentations and ambient modules, and
  `encoder::encode_source_file`, `encode_node` and `build_node_index_table`
  write the encoding and its node index table. `session::Session` answers
  the API's requests over the project system, `ipc::Conn` serves it over
  the MessagePack (`msgpack`) or JSON-RPC (`ipc::jsonrpc`) protocol, and
  `server::run_server` is [`tsc-rs --api`](../README.md#api-server-experimental).

The compiler recurses on the native stack in proportion to the nesting
depth of a source. The command runs its work, and every worker and checker
thread, on a stack reservation of 1 GiB (`tsc_program::WORKER_STACK_BYTES`;
virtual, committed only as the recursion uses it), which takes a source as
deep as tsgo compiles. With the serial defaults the API runs on the calling
thread, whose stack the embedding chooses; to compile sources of unusual
depth, run the session on a thread built with
`std::thread::Builder::new().stack_size(tsc_program::WORKER_STACK_BYTES)`.

The API reads the standard library declarations from the directory given to
`LibraryCatalog::typescript_7_1`; the checked-in
`vendor/typescript-native/7.1.0-dev-19dadef8/upstream/tsc/internal/bundled/libs`
supplies the matching files. The copy embedded in the `tsc-rs` executable
is not exposed through the API.

## Check a project

The [complete example](../crates/compiler/examples/type_check.rs) reads a
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

## Compile a project

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
session as in the example, or run the whole command line as described
next.

## Run the command line over your own system

`tsc_compiler::execute_command_line(system, args, testing)` runs a command
line exactly as the `tsc-rs` command does (including `-b`), over any
implementation of `tsc_compiler::system::System`: the file system it reads
and writes (a `tsc_host::vfs::FileSystem`), the current directory, the
directory of the standard library files, the clock, the environment
variables, whether the output is a terminal, and where the output goes. It
returns the exit status. `NativeSystem` is the process; a system over
`MemFs` compiles a project held entirely in memory, with no disk, as tsc-rs's
own tests of the native `tsc` and `tsc -b` baselines do and as an embedding
without a file system (such as a WebAssembly host) would. The optional
`CommandLineTesting` observes the run: the files each emit wrote, the
status lines, the resolution trace and each incremental program's state.

## Add the crates to your application

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

Copy the [example](../crates/compiler/examples/type_check.rs) to your
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
