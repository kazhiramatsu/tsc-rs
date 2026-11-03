# tsgo checker entry census

```sh
python3 scripts/tsgo_census.py build
python3 scripts/tsgo_census.py run --check-stdout
python3 scripts/tsgo_census.py compare target/tsgo-census/counts/hono.json target/tsgo-census/counts/zod.json
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
No cargo or network access is used.

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
function in `utilities.go`. Its Rust mapping is empty because `compare_types`
is imported from `tsc_types`, outside the requested checker source scope. The
checker and node builder both have `getTypeFromTypeNode`; only the checker entry
is counted. Notes identify other wrapper/body differences requiring care when
interpreting a cross-port comparison.

`compare` accepts the same JSON shape from both compilers. It maps canonical Go
names, full TSV Go locations, simple Rust names, and demangled qualified Rust
symbols through the TSV. Multiple matching Rust instantiations are summed.
An optional legacy `::h<hash>` suffix is accepted; closure child entry counts
are not folded into their parent. Raw mangled LLVM symbols should be demangled
by the profile-to-JSON producer. Explicit `re:` patterns can match other formats.
Unmapped names remain visible unchanged.

The ratio is **B / A**, sorted by descending `abs(ratio - 1)`, then name.
A missing key displays `-` / `missing`, never zero. Missing and infinite ratios
sort first. `0/0` displays `1.000000`; positive/zero displays `inf`.
