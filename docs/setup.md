# Setup and verification

For building and using the compiler, start with the [user guide](../README.md).
The [current verification policy](../CLAUDE.md#current-verification-policy)
owns the merge criteria; [CI](#ci) below describes the hosted jobs and their
local equivalents. The historical
accepted-state snapshot of the tsc 6.0.3 line lives in
[verification-status.md](verification-status.md).

Active development uses a self-contained, Oxc-style virtual Cargo workspace
at the repository root. The root `Cargo.toml` has no package of its own;
member sources live under `crates/*/src`, and there is intentionally no
top-level `src/`. The TypeScript 7.1 test cases, standard libraries and
reference baselines are vendored under `vendor/typescript-native/<profile>/`
(see its [README](../vendor/typescript-native/README.md)), so a plain clone
builds and verifies with no bootstrap step.

## Requirements

- **Rust** — installed via `rustup`; the repository `rust-toolchain.toml`
  pins the exact toolchain (with `clippy` and `rustfmt`) and rustup installs
  it automatically on first use. Bumping the pin is a deliberate, reviewed
  change.
- **Python 3.11 or newer** — runs the conformance supervisor
  (`scripts/conformance_ts71.py`), its sharded-control comparison and the
  vendoring script.
- **Go** — only to build the reference compiler, `tsgo`, from the vendored
  commit with `scripts/typescript7.py` (`setup` fetches the pinned commit
  into `target/typescript7/upstream`, moving an existing clean checkout to
  the pin; `build` runs `go -C tsc build ./cmd/tsc` there with the Go
  toolchain the script pins, which Go downloads on first use) when a
  behavior question needs the reference's answer. Neither Go nor Node.js is
  needed to build, test or run tsc-rs.

## CI

The compiler is checked against TypeScript 7.1's conformance baselines and
the workspace's Rust tests. You can run these checks locally or inspect
their GitHub Actions logs.

### What CI verifies

| Check | What it verifies |
| --- | --- |
| **TypeScript 7.1 conformance** (`conformance (TypeScript 7.1)`) | Runs the `compiler` and `conformance` test cases vendored from TypeScript 7.1 through the native test runner's configuration expansion, on one checker, and compares the diagnostics (`.errors.txt`), the emitted JavaScript, declaration files and source maps (`.js`, `.js.map`) the type and symbol baselines (`.types`, `.symbols`: the type and the symbol at every expression and declaration name, as tsgo's test runner writes them) the source map records (`.sourcemap.txt`: every emitted line with its spans against the source text, as tsgo's recorder writes them) and the module resolution traces (`.trace.json`: the `--traceResolution` lines, sanitized as tsgo's harness does) with the vendored reference baselines byte for byte (`scripts/conformance_ts71.py --check`). The ratchet `ratchets/ts71/` records the error tier, the emit tier and the type, symbol, source map record and trace tiers each configuration has reached and rejects regressions. A sharded control (`--checkers 4`) is compared with the one-checker report by `scripts/conformance_ts71_compare.py`; the design packet records the configurations where they differ. The configurations the native runner itself skips (`target: es5`, the `umd` and `system` module kinds, the `node10` and `classic` resolutions, `baseUrl`, `esModuleInterop: false`, `alwaysStrict: false`) or never produces baselines for (`amd`, `outFile`, its skip list) are not compared. The same job then runs the `transpile` test cases through single-file transpilation (`transpileModule` and `transpileDeclaration`) as the native transpile runner does, the command-line and tsconfig parsing tests (the arguments and configuration files the native Go tests keep in their tables), the `tsc` and `tsc -b` tests (each scenario's command lines run over an in-memory system with its edits in between, as the native harness's test system does, recorded from the Go tests by `scripts/tsctests_scenarios.py`), the `tsc --watch` and `tsc -b --watch` tests (their edits as watch cycles) and the API encoder's dumps of two parsed sources, and compares each of their baselines (`transpile`, `tsoptions/commandLineParsing`, `config/tsconfigParsing`, `tsc`, `tsbuild`, `tscWatch`, `tsbuildWatch`, `api`) byte for byte (`scripts/suites_ts71.py --check`, ratchet `ratchets/ts71/suites-<profile>.tsv`). |
| **Rust checks** (`rust`) | Checks formatting, runs Clippy over every target of the workspace, executes every workspace test target (`cargo test --workspace`) and verifies that the generated diagnostic catalog and the API encoder's tables are current (`python3 .github/ci/replay.py rust`). |

The `gates` check requires both jobs to succeed; a change that touches only
`docs/`, the root `README.md`, `CONTRIBUTING.md` or `LICENSE` selects neither.

These results demonstrate the behavior covered by those tests. Some cases
have recorded differences from the reference; a passing run does not imply
complete TypeScript compatibility. See the README's
[limitations](../README.md#limitations) for the user-facing restrictions.

### Run locally

Run these commands from the repository root using a POSIX shell (for example,
Bash or Zsh), with the [requirements](#requirements) above. Rustup selects
the Rust version and tools from [rust-toolchain.toml](../rust-toolchain.toml). The TypeScript test inputs and
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
tsconfig parsing, tsc and tsc -b; a few seconds):

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

## Verification during development

During implementation, run the affected conformance cases
(`target/release/conformance-ts71 --case <suite>/<path> --dump <dir>`, or
`scripts/conformance_ts71.py --filter <text> --check`) and the adjacent unit
tests of the crates you changed. The ratchet `ratchets/ts71/` records the
tier each configuration has reached; `--update` raises it at the final
bytes and never lowers a row.

Private unit-test bodies live under each crate's `tests/unit/` tree and are
included from `src` through `#[path]` modules, so they retain private-item
access without keeping test implementations beside production code. Broad
integration files live under `tests/integration/` behind a single
`tests/contracts.rs` target per crate; a few focused targets (the
declaration-emit and UTF-16 contracts) stay separate files under `tests/`.

`cargo xtask codegen diagnostics` regenerates `crates/diagnostics/src/gen.rs`
from the vendored `diagnosticMessages.json`; the hosted `rust` job verifies
the generated file with `diagnostics-check`.

The `tsc-rs` executable embeds the vendored TypeScript 7.1 standard-library
bytes at build time. After it has been built or installed, running a project
does not require this repository's `vendor/` directory.

## Workspace package roles

Each workspace member declares its role under `package.metadata.tsc-rs`.
Internal crates follow one naming convention:

| Layer | Pattern | Checker example |
| --- | --- | --- |
| Cargo package | `tsc-rs-<role>` | `tsc-rs-checker` |
| Workspace dependency alias | `tsc-<role>` | `tsc-checker` |
| Rust crate identifier | `tsc_<role>` | `tsc_checker` |

Use the full word `diagnostics` in every layer (`crates/diagnostics`,
`tsc-rs-diagnostics`, `tsc-diagnostics`, and `tsc_diagnostics`). When
changing only a Cargo package name, keep its root workspace-dependency key as
the stable Rust dependency alias and update that entry's `package` value; the
dev-profile `opt-level` block in the root `Cargo.toml` lists the compute
crates by package name and is maintained by hand. The `cargo xtask` selector
is centralized in `.cargo/config.toml` and works from any workspace
subdirectory.

Exact `tsrs2` spellings are retained only where changing them would change the
meaning of existing data: versioned v1 hash-domain separators and historical
design records. Test names, temporary paths, process labels, and fixture-only
environment variables use `tsc-rs` / `TSC_RS`.

## The tsc 6.0.3 line and the paused v1 codebase

The tsc 6.0.3-compatible line ended with release v0.1.0 (tag `v0.1.0`,
branch `release/6.0.3`); its oracle, corpora, acceptance tooling and
verification records exist only at that tag. The original v1 `src/`
implementation is preserved at tag `v1-final`; the archived instructions are
in [design/archive/v1-setup.md](design/archive/v1-setup.md).
