# Setup and verification

For building and using the compiler, start with the [user guide](../README.md).
The [current verification policy](../CLAUDE.md#current-verification-policy)
owns the merge criteria; the README's [Run CI](../README.md#run-ci) section
describes the hosted jobs and their local equivalents. The historical
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

## Verification

Both hosted jobs run from the repository root:

```sh
python3 .github/ci/replay.py rust        # fmt, clippy, cargo test --workspace, codegen check
cargo build --release -p tsc-rs-conformance --bin conformance-ts71
python3 scripts/conformance_ts71.py --workers 4 --check   # TypeScript 7.1 baselines
```

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
