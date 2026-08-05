# Setup and verification

Active development uses a self-contained, Oxc-style virtual Cargo workspace
at the repository root. The root `Cargo.toml` has no package of its own;
member sources live under `crates/*/src`, and there is intentionally no
top-level `src/`. The conformance corpus (`ts-tests/`), pinned TypeScript
oracle (`vendor/typescript-6.0.3/`), and all goldens are checked in, so a
plain clone builds and verifies with no bootstrap step.

## Requirements

- **Rust** — installed via `rustup`; the repository
  `rust-toolchain.toml` pins the exact toolchain (with `clippy` and
  `rustfmt`) and rustup installs it automatically on first use.
  Bumping the pin is a deliberate, reviewed change.
- **Node** — only needed when running the oracle (probes, driver
  tests, golden refresh). The required version is pinned in
  `.node-version`; `oracle-refresh` refuses to write goldens
  from any other launched version.

## Verification

All gates run from the repository root:

```sh
cargo xtask ci                      # full merge-gate suite (must be green on main)
cargo xtask conformance             # conformance sweep (optionally --band 2xxx)
cargo xtask conformance --syntactic-only
cargo xtask invariants --suite all  # sampled determinism/idempotence developer run
cargo xtask invariants --suite all --full-corpus  # completion/CI row 10
cargo xtask completion              # report all 11 final completion rows
cargo xtask m8 trace --program-json target/probe/program.json --code 8020 \
  --out target/m8-trace.json        # targeted D2 trace; report-only
```

The local Rust phase of `cargo xtask ci` compiles
`cargo test --workspace --all-targets --no-run`
once, then launches the discovered test executables through an ordered
two-process pipeline with one harness thread per process. Every unit, binary,
integration, example, and benchmark test target remains covered, while the
workspace's documentation contains no executable Rust doctests. Set
`TSRS_CI_TEST_WORKERS=1` to diagnose order-sensitive resource issues. The
separate `cargo build --workspace` pass is intentionally omitted because
all-target Clippy type-checks every target and the test compile performs
codegen. Test binaries omit debug information to reduce link and startup I/O;
ordinary dev-profile binaries retain their debugging profile.

If every path changed from the trusted base ends in `.md` and README's
generated `STATUS` block is byte-identical to the base, do not run the
Cargo/Node/full-corpus commands above. Run `git diff --check` from the
repository root and review changed links, anchors, and generated-block
boundaries. Any non-Markdown or generated-status change uses the full merge
gate.

GitHub Actions intentionally does not repeat the local merge gate. It only
classifies the changed paths and runs the focused Windows host contracts plus
one compact filesystem-to-program parity smoke when host, path, or toolchain
inputs change; ordinary Rust, Node, semantic,
corpus, evidence, readiness, and performance checks remain local-only. The
Windows lane is capped at two Cargo/test workers and disables incremental
state and test debuginfo on its clean runner. It is a platform canary, not
acceptance evidence. The optional `cargo xtask ci --lane hosted
--history-sensitive --baseline <trusted-sha>` diagnostic remains available
locally for immutable-history investigation and is never selected
automatically by Actions.

`cargo xtask completion` is report-only during M8 and succeeds while naming
pending rows in `target/completion/report.json`. The post-M9 release gate is
`cargo xtask completion --require-done`, which fails unless all 11 rows are
green.

At the M9 entry state, `cargo xtask fuzz` exposes only the historical M8
`run`/`replay`/`reduce` smoke commands and completion row 11 is deliberately
pending. Do not treat that smoke as a nightly producer. The planned
preflight, real replay/reduction, bounded nightly, aggregation, and
steady-state commands land in the order fixed by the
[M9 execution contract](design/greenfield/m9-execution-and-close.md).

`cargo xtask m8 trace` is a planning probe, not a completion gate. Repeat
`--program-json` and `--code` to compare an emitting probe with a reviewed
non-emitting sibling. The command instruments only matching exact D2
diagnostic-reference offsets, reports `source_declarations_visited=0`,
collects per-probe V8 function coverage, and rejects any diagnostic change
against the ordinary oracle driver. Per-probe library caches reset so sibling
coverage is order-independent, and each probe runs in its own single-threaded
Node process to isolate V8 lazy-compilation state. The instrumented bundle is
cached by the source, inventory, tool, selected-code, and pinned-Node
fingerprint under
`target/m8/trace/cache/`.

The full gate list, the trusted-base variants, and the per-artifact
audit commands (`ratchet check`, `scope audit`, `families check`,
`escapes`) are documented in the repository `CLAUDE.md` ("Verification
quick reference") and in the
[convergence plan](design/greenfield/completion-convergence-plan.md).

Conformance runs the corpus in-process with its own parallelism
defaults; run it as-is (do not override the oracle scripts'
job-count environment variables) and expect the first run to be the
slowest while caches warm.

## Workspace package roles

Contributor commands address internal packages by a stable logical role rather
than their Cargo package name. For example, this keeps working if the checker
package is renamed:

```sh
cargo xtask test checker --lib
cargo xtask workspace audit
```

Each workspace member declares its role under `package.metadata.tsc-rs`.
`workspace audit` resolves those roles through `cargo metadata`, rejects
missing or duplicate roles, and checks the shared dependency aliases. It also
rejects direct Cargo package/bin selection in workflow steps, referenced local
composite actions, `scripts/`, and xtask command construction, then verifies
the generated dev-profile block. It is run automatically by every
`cargo xtask ci` lane.

Internal crates follow one naming convention:

| Layer | Pattern | Checker example |
| --- | --- | --- |
| Cargo package | `tsc-rs-<role>` | `tsc-rs-checker` |
| Workspace dependency alias | `tsc-<role>` | `tsc-checker` |
| Rust crate identifier | `tsc_<role>` | `tsc_checker` |
| Contributor command role | `<role>` | `checker` |

Use the full word `diagnostics` in every layer (`crates/diagnostics`,
`tsc-rs-diagnostics`, `tsc-diagnostics`, and `tsc_diagnostics`).

When changing only a Cargo package name, keep its root workspace-dependency
key as the stable Rust dependency alias, update that entry's `package` value,
then run:

```sh
cargo xtask workspace sync
cargo xtask workspace audit
```

`workspace sync` rewrites only the marked profile block in the root
`Cargo.toml`; it does not rename Rust library identifiers or source imports.
Those identifiers are a separate code-level API if they ever need to change.
The `cargo xtask` bootstrap command is the one role-resolution exception: Cargo
must select the `tsc-rs-xtask` package before the role resolver can start, so
that selector is centralized in `.cargo/config.toml` and works from any
workspace subdirectory.

Exact `tsrs2` spellings are retained only where changing them would change the
meaning of existing data: compatibility readers for the former `tsrs2/`
workspace path, versioned v1 hash-domain separators, and historical design
records. Test names, temporary paths, process labels, and fixture-only
environment variables use `tsc-rs` / `TSC_RS`. The shorter `tsrs` spellings in
serialized schemas and protocol markers are separate v1 contracts and require
an explicit schema migration before they can be renamed.

## The paused v1 codebase

The original `src/` implementation was removed from the working tree
on 2026-07-15 and is preserved at tag `v1-final`. Its bootstrap flow
(`scripts/bootstrap.sh`, `verify.sh`) only exists at that tag; the
archived instructions are in
[design/archive/v1-setup.md](design/archive/v1-setup.md).
