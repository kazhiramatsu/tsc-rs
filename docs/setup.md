# Setup and verification

Active development lives in `tsrs2/`, a self-contained Cargo
workspace: the conformance corpus (`tsrs2/ts-tests/`), the pinned
TypeScript oracle (`tsrs2/vendor/typescript-6.0.3/`), and all goldens
are checked in, so a plain clone builds and verifies with no bootstrap
step.

## Requirements

- **Rust** — installed via `rustup`; the repository
  `rust-toolchain.toml` pins the exact toolchain (with `clippy` and
  `rustfmt`) and rustup installs it automatically on first use.
  Bumping the pin is a deliberate, reviewed change.
- **Node** — only needed when running the oracle (probes, driver
  tests, golden refresh). The required version is pinned in
  `tsrs2/.node-version`; `oracle-refresh` refuses to write goldens
  from any other launched version.

## Verification

All gates run from `tsrs2/`:

```sh
cd tsrs2
cargo xtask ci                      # full merge-gate suite (must be green on main)
cargo xtask conformance             # conformance sweep (optionally --band 2xxx)
cargo xtask conformance --syntactic-only
cargo xtask invariants --suite all  # determinism/idempotence invariants
```

The full gate list, the trusted-base variants, and the per-artifact
audit commands (`ratchet check`, `scope audit`, `families check`,
`escapes`) are documented in the repository `CLAUDE.md` ("Verification
quick reference") and in the
[convergence plan](design/greenfield/completion-convergence-plan.md).

Conformance runs the corpus in-process with its own parallelism
defaults; run it as-is (do not override the oracle scripts'
job-count environment variables) and expect the first run to be the
slowest while caches warm.

## The paused v1 codebase

The original `src/` implementation was removed from the working tree
on 2026-07-15 and is preserved at tag `v1-final`. Its bootstrap flow
(`scripts/bootstrap.sh`, `verify.sh`) only exists at that tag; the
archived instructions are in
[design/archive/v1-setup.md](design/archive/v1-setup.md).
