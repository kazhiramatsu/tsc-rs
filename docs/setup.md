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
cargo xtask invariants --suite all  # sampled determinism/idempotence developer run
cargo xtask invariants --suite all --full-corpus  # completion/CI row 10
cargo xtask completion              # report all 11 final completion rows
cargo xtask m8 trace --program-json target/probe/program.json --code 8020 \
  --out target/m8-trace.json        # targeted D2 trace; report-only
```

If every path changed from the trusted base ends in `.md` and README's
generated `STATUS` block is byte-identical to the base, do not run the
Cargo/Node/full-corpus commands above. Run `git diff --check` from the
repository root and review changed links, anchors, and generated-block
boundaries. Any non-Markdown or generated-status change uses the full merge
gate.

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

## The paused v1 codebase

The original `src/` implementation was removed from the working tree
on 2026-07-15 and is preserved at tag `v1-final`. Its bootstrap flow
(`scripts/bootstrap.sh`, `verify.sh`) only exists at that tag; the
archived instructions are in
[design/archive/v1-setup.md](design/archive/v1-setup.md).
