# tsc-rs

`tsc-rs` is a Rust implementation of TypeScript checker behavior, focused on
reproducing `tsc` diagnostics closely enough for differential testing.

## Repository Layout

- `src/`: Rust checker, parser, binder, diagnostics, and harness code.
- `lib/`: TypeScript library declarations used by the checker.
- `difftest/`: diagnostic comparison tooling and corpus files.
- `conf/`: generated conformance-case tooling.
- `scripts/`: repository-maintained helper scripts.
- `docs/`: project notes and handoff documentation.

## Current Status

Phase 1 is focused on restoring `fn_stack` to bracketed push/pop discipline and
fixing the existing checker bugs surfaced by that change.

See [docs/phase1-status.md](docs/phase1-status.md) for the current bug list,
remaining false positives, banked artifacts, and recommended next steps.
See [docs/setup.md](docs/setup.md) for the verification environment.

## Common Commands

```sh
./scripts/bootstrap.sh
cargo build
cargo test
./verify.sh quick
./verify.sh golden-check
```

Low-load golden check:

```sh
TSRS_BATCH_JOBS=1 TSRS_CLASSIFY_JOBS=1 ./verify.sh golden-check
```

`verify.sh` resolves paths from this checkout by default. Override `TSRS_ROOT`,
`TSRS_WORK`, `TSRS_LIB`, `TSRS_BIN_RELEASE`, or `TSRS_ORACLE` if you need to use
external artifacts.
