# tsgo / tsc-rs checker entry census

```sh
python3 scripts/tsgo_census.py build
python3 scripts/tsgo_census.py run --check-stdout
python3 scripts/tsgo_census.py run-rs --projects hono zod ts
python3 scripts/tsgo_census.py run-rs --skip-build --projects vscode effect next playwright vue
python3 scripts/tsgo_census.py compare target/tsgo-census/counts/hono.json target/tsc-census/counts/hono.json
python3 scripts/tsgo_census.py compare --summary
python3 -B -m unittest discover -s scripts/tsgo_census -p test_census.py -v
```

`build` verifies the clean `aa8149273` source checkout, copies its working tree
without `.git`, and copies the offline module/toolchain cache on the first build.
It regenerates the private upstream tree and starts with an empty Go build cache
on every build. Go runs with `-p=2`, `GOMAXPROCS=2`, and the reference build's
environment, plus `GOPROXY=off`. The copied checksum-database cache authenticates
the Go toolchain locally (`GOSUMDB=sum.golang.org`; Go rejects `GOSUMDB=off` for
downloaded toolchains, even when cached). The two Go commands and all
compiler invocations run serially under `nice -n 20`, with a direct fallback
if the sandbox refuses nice. A checkout-local lock prevents overlapping invocations.
The tsgo build uses no cargo or network access.

Everything generated is under `target/tsgo-census/`: `upstream/`, `tsc`, isolated
Go caches, `build.json`, `rewrite.json`, `run.json`, compiler output in `runs/`,
and one `{function: entry_count}` JSON file per project in `counts/`.
Build metadata includes the command lines, actual exit codes, elapsed seconds,
environment, source pin, and manifest digest. `run.json` retains compiler exit
codes even when benchmark diagnostics make them nonzero; `run` collects all
projects and returns the first nonzero compiler status. Missing or invalid
counter output fails the runner. `--projects hono` selects a smaller run.
Source/cache locations, bench root, and the reference binary have CLI overrides.

The counters are **non-atomic plain additions**. Use this binary only with
`--singleThreaded`, with one compilation per process. Counters still increment
when `TSGO_CENSUS` is unset, but nothing is written. This is an entry-count tool,
not an instrumented wall-time performance baseline. The ordinary compiler exit
code, stdout, and stderr are retained even if writing the census fails; the runner
checks that a fresh census file actually exists. Watch/API/LSP modes are outside
the measurement scope.

Single-threaded execution does not guarantee identical counts across processes:
the upstream checker also sorts slices collected from Go maps. In the initial
hono validation, 47 counters matched exactly on a repeat; `compareTypes` changed
from 314,344 to 314,345 (about 0.00032%). The exact cause of that one-call change
was not traced. Retain raw measurements and investigate small differences before
treating them as port divergences; the rewriter does not change upstream ordering.

`--check-stdout` runs hono with census enabled, with the same instrumented binary
and census disabled, and with the pinned uninstrumented reference. It records
byte identity for both stdout and stderr and compares all three exit statuses.
The check runs serially as part of the hono measurement.

`functions.tsv` has three tab-separated columns: exact Go source location
relative to upstream's `tsc/`, Rust function name or `re:`-prefixed regex, and
notes. Add rows there to grow the list. `rewrite.go` uses Go's AST to find the
exact receiver and method, inserts `census.Enter(census.X)` immediately after the
opening brace, adds imports, and generates the enum and JSON names. JSON names
are the Go function names with their first letter lowercased (`CompareTypes`
becomes `compareTypes`). Missing declarations are listed in `rewrite.json` and
excluded from the enum and counts, so absence cannot be confused with zero.
The exit hook dumps explicitly between `runMain()` and `os.Exit`, which skips
deferred functions.

At this pin `resolveName` is a function field, not a declaration in
`internal/checker`; it is recorded as missing. `CompareTypes` is the exported
function in `utilities.go`; its Rust counterpart is `compare_types` in
`crates/types/src/type_order.rs`, imported by the checker. The
checker and node builder both have `getTypeFromTypeNode`; only the checker entry
is counted. Notes identify other wrapper/body differences requiring care when
interpreting a cross-port comparison.

## Rust PGO entry counts

`run-rs` reuses step 1 of `scripts/pgo_build.py`: an instrumented release build
in `target/tsc-census/instrumented` with `-Cprofile-generate` and
`-Cllvm-args=-pgo-instrument-entry=true`, `cargo build
--release -j 1 -p tsc-rs-cli --offline --locked`, and `CARGO_NET_OFFLINE=true`.
It stops if a dependency is unavailable locally. The build and measured runs
try `nice -n 20`, falling back directly if nice is refused. The same lock as the
tsgo runner prevents concurrent census builds/runs. Projects run serially with
the same roots, benchmark configs, and `--pretty false --singleThreaded` flags
as `run`; VS Code uses `tsconfig.bench-stable.json`, the others `bench-noemit`.
`--projects` selects projects in the specified order; the default is all eight.
On macOS the shared PGO helper appends `-fno-profile-generate -fno-profile-use`
to `CFLAGS`/`CXXFLAGS`: cc-rs otherwise instruments native dependencies with
Apple clang's incompatible profile ABI, which can crash mimalloc at startup.
Rust instrumentation remains enabled; the effective environment is recorded.

`--skip-build` reuses the binary after checking its commit and SHA-256 against
`build.json`. Compiler sources must be clean for both building and reusing it. The binary's commit,
build flags, exact commands, exits, and elapsed seconds are recorded. A compiler
diagnostic exit is retained and collection continues; `run-rs` returns zero
when all selected profiles were collected. A missing, empty, or extra training
profile or a failed profile conversion makes the runner fail.

The PGO helper supplies `LLVM_PROFILE_FILE=.../profiles/<project>/train-%p.profraw`;
the CLI already uses the C runtime exit chain when this variable is present.
Only that project's fresh `train-*.profraw` file is merged; build-script and
proc-macro profiles are excluded. `llvm-profdata` is discovered in the pinned
Rust toolchain (override with `--llvm-profdata PATH`), then `merge` and
`show --all-functions --counts` run once per project. The **first block count**
is the function entry count; the displayed `Function count` is not used.
The extra LLVM flag is essential: ordinary IR PGO can place another block's
counter first. The parser rejects profiles reporting `entry_first = 0`.
All symbols are demangled in one `c++filt` process per project.

Plain Rust TSV names match the function's own final path segment exactly,
after removing generic arguments and before any `::{{closure}}`. Counts sum
over monomorphizations **and closure children**, as requested for this census.
Consequently the aggregate can exceed the parent function's own entry count;
inspect `mapping.txt` before interpreting a large ratio as extra checker work.
`re:` rows search the full demangled path. Ambiguous matches fail collection.
Blank mappings and names absent from the profile are unmapped, even if the
function might simply be unused in that project. A matched zero counter is
retained as a real zero.

PGO observes optimized emitted bodies: early/MIR inlining can erase a source
boundary or leave an unused outlined copy with zero counters. Thin wrappers
and direct worker calls therefore need inspection even when a symbol matched.
A zero outlined entry count does not establish that the source operation was
skipped. The mapping audit exposes these boundaries instead of guessing counts.

Rust artifacts live under `target/tsc-census/`: `build.json`, invocation history
in `run.json`, stdout/stderr and raw conversion/demangling output in `runs/`,
and `counts/<project>.json`. Count keys use the Go JSON names. `_unmapped` lists
canonical TSV names with no matching Rust symbol; these keys are omitted from
the count object. `_meta` records the binary commit and digest, manifest digest,
command, working directory, profile environment, compiler exit, and elapsed
seconds. `mapping.txt` lists every TSV row and each matched demangled symbol
with its count, grouped by project (including matched zero counts).

## Comparing counts

`compare` accepts count JSONs from both compilers. It maps canonical Go
names, full TSV Go locations, simple Rust names, and demangled qualified Rust
symbols through the TSV. Multiple matching Rust instantiations are summed.
An optional legacy `::h<hash>` suffix is accepted; closure child entry counts
are folded into their parent. Raw mangled LLVM symbols should be demangled
by the profile-to-JSON producer. Explicit `re:` patterns can match other formats.
Unmapped names remain visible unchanged.

The ratio is **B / A**, sorted by descending `abs(ratio - 1)`, then name.
Pass tsgo as A and tsc-rs as B for a port comparison. Metadata is ignored;
unmapped rows are listed separately and excluded from ratios.
A missing key displays `-` / `missing`, never zero. Missing and infinite ratios
sort first. `0/0` displays `1.000000`; positive/zero displays `inf`.

`compare --summary [GO_COUNTS_DIR RS_COUNTS_DIR]` prints a function-by-project
ratio table, sorted by each row's largest distance from 1. The default paths
are the two census count directories. `--projects hono zod ts` selects columns.
A missing project file fails rather than silently hiding an incomplete run.
The default selects projects with a file on either side. Missing functions and
unmapped functions keep their distinct labels in this table as well.
