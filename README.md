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

The checker is measured against the real `tsc` oracle over ~5,900 conformance
fixtures. As of the latest sweep it reaches **~62% gate-filtered exact
file-level diagnostic match**, climbing monotonically with zero shipped
regressions (every change passes a hard "0 new false positives" gate).

Foundations in place: the control-flow graph resolver is the sole flow engine
(the earlier fact-stack is retired), unused-locals/members mirror `tsc`, and the
comparable relation plus generic-signature inference match `tsc`'s structure.

The remaining conformance work — priorities, per-workstream designs with
step-by-step implementation guides, the mandatory verification protocol, a
pinned knowledge base, the architectural stall playbook, and a from-scratch
rebuild design — is documented under [docs/design/](docs/design/README.md).
**Start any handoff there.**

See [docs/setup.md](docs/setup.md) for the verification environment and
[docs/determinism-design.md](docs/determinism-design.md) for the flow-engine
and determinism design.

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

## License

Licensed under the [MIT License](LICENSE).
