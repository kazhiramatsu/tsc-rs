# Native TypeScript reference workflow

This is the first implementation step of the user-approved
[TypeScript 7 direction](typescript-7-direction.md). It establishes a local
upstream investigation loop; Rust compatibility and the accepted 6.0.3
profile remain separate claims.

## Pinned environment

- Repository: `microsoft/TypeScript`, commit
  `1f70213d4922b434345f639b441681e470c7cfc1`.
- Built compiler version: `7.1.0-dev` (native 7-series development source).
- Go: `go1.26.0`; verified on macOS arm64. The helper uses POSIX file locking.
- Checkout, toolchain, dependencies, build cache, binaries and logs:
  `target/typescript7/`. The system Go installation is unchanged.
- Resource limits: `GOMAXPROCS=2`, build parallelism 2, test parallelism 1;
  macOS commands run with `taskpolicy -b nice -n 15`.

From the tsc-rs repository root:

```sh
python3 scripts/typescript7.py setup
python3 scripts/typescript7.py build
target/typescript7/bin/tsc --version
python3 scripts/typescript7.py compiler checkingObjectWithThisInNamePositionNoCrash.ts
python3 scripts/typescript7.py compiler deleteExpressionMustBeOptional.ts
python3 scripts/typescript7.py fourslash TestGoToDefinitionImport3
python3 scripts/typescript7.py fourslash TestBasicEdit
```

The helper fetches the pinned source and Go toolchain on first setup. It
refuses a different existing HEAD. Source edits at the same HEAD are allowed
for debugging and captured in each run's `source.patch` and `source_status`.
Untracked files appear in status but are not included in the patch; save any
such probe files with the investigation. Concurrent helper runs in this
checkout are refused. Run raw upstream commands serially too: the compiler
runner clears its local compiler/conformance baseline directories.

The build uses the upstream Go module directly, with embedded standard
libraries. The pinned checkout already contains the generated Go sources
and libraries; npm installation/generation is unnecessary for this path.
Upstream `npx hereby build` instead uses `noembed` and copies libraries next
to the binary. Do not pass `noembed` to compiler reference tests: `TestLocal`
skips when libraries are not embedded. Packaging, code generation and API
workspace builds still follow the upstream CONTRIBUTING instructions.

## Test format and result identity

All native paths below are relative to the upstream checkout.

| Surface | Native format and execution | Integration rule for tsc-rs |
| --- | --- | --- |
| Compiler / conformance | `tsc/testdata/tests/cases/{compiler,conformance}/**/*.ts[x]`; `TestLocal` in `tsc/internal/testrunner` | Preserve `@Filename`, virtual files, symlinks, embedded tsconfig and option combinations. A fixture is not necessarily one compilation. |
| Compiler expectations | `tsc/testdata/baselines/reference/{compiler,conformance}/`; configuration appears in the basename | Use the selected version's diagnostics, JS/declarations, types/symbols and maps. Existing 6.0.3 baseline paths are not interchangeable. |
| Language Service / FourSlash | `tsc/internal/fourslash/tests/*_test.go`; a Go `Test...` function contains source text, markers/ranges and Go assertions | Preserve edits and request order as well as source text. Extracting the raw string alone loses the test. Native assertions use LS/LSP types. |
| FourSlash baselines | `tsc/testdata/baselines/reference/fourslash/` | Keep the corresponding upstream Go test and baseline together when specifying a Rust LSP test. Some tests use direct assertions instead. |
| Build/watch and other subsystem tests | Go tests in the corresponding native packages | Follow the package's harness and state transitions; do not force these into a single-file compiler fixture. These are not yet covered by the helper. |

The compiler helper selects an exact fixture basename and lets the official
parser expand **all** its configurations. For example,
`deleteExpressionMustBeOptional.ts` varies `strict: true, false`. Go names
the cases `TestLocal/<basename>_<configuration>`; the runner's diagnostic,
output, type and other checks are children of those cases. Their passing
test-event count is not the fixture count.

Tests use `-count=1` to bypass Go's test-result cache while retaining the
build cache. `-json` supplies actual executed names and pass/fail/skip
events. A zero-match selection, a wholly omitted upstream fixture, or any
skipped configuration makes the helper fail. A mixed pass/skip run is not
reported as full coverage. `-list` alone does not enumerate dynamically
created `TestLocal` configurations.

Each run creates `target/typescript7/logs/<timestamp>-<unique>/` containing
`output.log`, `result.json`, and `source.patch`. The JSON records command,
commit, Go version, environment, source status, elapsed time and terminal
test events. Upstream baseline tracking identifies the exercised baseline
paths; newly written differences are preserved in `baseline-diffs/` before
the next run can clear them. Identify future imported cases by commit + fixture/test path +
configuration; retain executed, skipped and omitted dispositions explicitly.
This wrapper runs the Go reference, not the Rust implementation.

Local baselines are **differences**, not a complete dump of every successful
output. A matching result normally creates no baseline file. A `.delete`
marker represents a baseline becoming absent. Do not run `baseline-accept`
as part of an investigation or copy local files over the reference tree to
turn a failure green. Review outputs in the context of the test assertions.
The upstream writer can create a `.delete` marker without failing the Go
test. The helper therefore fails on newly written tracked differences,
including deletion markers, even when Go exits successfully.

The fixed runner deliberately skips some legacy options (including ES5,
System/UMD, Node10/Classic resolution) and rejects others (AMD and outFile).
Its `skippedTests` list also omits fixtures. These dispositions must feed
the version-transition inventory before changing the H2 completion scope.

## Print debugging

Use a passing official case first. Add a probe to the producer under
investigation, filter it to that case's **virtual** source filename, and
write to `os.Stderr`. In particular, LSP stdout is the JSON-RPC transport.
Log node kind/span, parent/container kind and selected type/symbol fields;
avoid recursively printing a complete compiler graph. Pointer addresses and
type IDs are useful within one run but are not stable comparison keys.

The checked-in example patch, `scripts/typescript7-check-this.patch`, probes
`Checker.checkThisExpression` after resolving its container and type. It
does not invoke additional checker operations to format the result. It logs
the resolved Type's flags and concrete data type, and captures the current
goroutine's stack once when requested.

Apply and remove the patch from the repository root, with no reference run
active:

```sh
git -C target/typescript7/upstream apply --unidiff-zero --check ../../../scripts/typescript7-check-this.patch
git -C target/typescript7/upstream apply --unidiff-zero ../../../scripts/typescript7-check-this.patch
TSRS_TRACE_FILE=checkingObjectWithThisInNamePositionNoCrash.ts TSRS_TRACE_STACK=1 python3 scripts/typescript7.py compiler checkingObjectWithThisInNamePositionNoCrash.ts
git -C target/typescript7/upstream apply --unidiff-zero --reverse --check ../../../scripts/typescript7-check-this.patch
git -C target/typescript7/upstream apply --unidiff-zero --reverse ../../../scripts/typescript7-check-this.patch
python3 scripts/typescript7.py compiler checkingObjectWithThisInNamePositionNoCrash.ts
git -C target/typescript7/upstream status --short
```

The patch omits context lines to avoid whitespace errors from storing a
Go diff inside this repository, so `--unidiff-zero` is required. Apply it
only to the documented pin. Reverse only this patch; preserve other investigation edits. The environment
variable itself does nothing without the probe. With another upstream pin,
re-read the producer before adapting the patch. Compare the uninstrumented,
instrumented and restored test results. Print-induced timing changes are
not performance or concurrency evidence.

## When a debugger helps

Use Delve when a value changes across several calls, stepping is clearer
than repeated prints, or a problem depends on multiple goroutines.
[`runtime/debug.Stack()`](https://pkg.go.dev/runtime/debug#Stack) only captures the current goroutine. For an
all-goroutine print snapshot, use [`runtime.Stack(buf, true)`](https://pkg.go.dev/runtime#Stack) with a buffer
large enough that the returned length is smaller than its capacity; retry
with a larger buffer if it fills. Avoid routine unbounded stack dumps.

The helper's `build --debug` uses the upstream flag
`-gcflags=all=-N -l` and writes `target/typescript7/bin/tsc-debug` separately
from the ordinary binary. With a compatible Delve installed, attach via
`dlv exec target/typescript7/bin/tsc-debug -- <compiler arguments>`.
Delve is not installed in the verified environment; interactive debugging
and `build --debug` are optional follow-ups, not part of the validation claim below.

## Verified session, 2026-09-07 JST

Source checkout was restored to a clean pinned tree after all probes.
These are local observations on macOS arm64, not performance guarantees:

| Observation | Result | Recorded elapsed time |
| --- | --- | --- |
| Embedded compiler build and `--version` | `7.1.0-dev` | Unchanged cached build: 0.644 s; initial setup/build was not timed as one operation |
| `checkingObjectWithThisInNamePositionNoCrash.ts` | All upstream checks pass, before instrumentation, with instrumentation, and after restoration | Restored run: 5.302 s |
| Same case after editing the Go checker, including recompilation and stack capture | Pass; `KindThisKeyword`, span `66:70`, `KindMethodDeclaration`, `*checker.ObjectType`; stack captured once | 77.128 s |
| `deleteExpressionMustBeOptional.ts` | Both `strict=true` and `strict=false` pass | 5.236 s |
| `TestGoToDefinitionImport3` | Pass | First FourSlash build/run: 43.027 s |
| `TestBasicEdit` | Edit followed by member completions passes | Cached build/run: 6.093 s |
| `arrayIterationLibES5TargetDifferent.ts` | Four configurations pass, two ES5 configurations skip; helper correctly exits 1 | 4.901 s |
| `APILibCheck.ts` and nonexistent `TestTsrsMissingReferenceCase` | No cases run; helper correctly exits 1 for both | Recorded in logs |
| Obsolete diagnostic baseline control | A temporary `constEnumOnlyModuleMerging.errors.txt` produces `.delete`; Go passes, helper correctly exits 1 | 4.905 s |
| `constEnumOnlyModuleMerging.ts` after removing that control | Pass, zero baseline changes, clean reference tree | 4.835 s |

The trace is retained in
`target/typescript7/logs/20260906T150122Z-zg9gjjhq/` with its applied patch.
The baseline-deletion control is retained in
`target/typescript7/logs/20260906T151713Z-fcr4t9_z/`; its temporary reference
file contained only `tsc-rs local negative control: an obsolete diagnostic
baseline must be detected.` and has been removed. Full upstream suites,
Rust compatibility tests and hosted CI were not run for this local workflow
step. H2 implementation resumes from its own slice boundary.

## Sources

- [Upstream development commands](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/CONTRIBUTING.md)
- [Build and debug flags](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/Herebyfile.mjs)
- [Compiler runner and option matrix](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/testrunner/compiler_runner.go)
- [Unsupported option dispositions](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/testutil/harnessutil/harnessutil.go)
- [Baseline writer](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/testutil/baseline/baseline.go)
- [Native FourSlash definition test](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/internal/fourslash/tests/goToDefinitionImport3_test.go)
- [Intentional native changes](https://github.com/microsoft/TypeScript/blob/1f70213d4922b434345f639b441681e470c7cfc1/tsc/CHANGES.md)
