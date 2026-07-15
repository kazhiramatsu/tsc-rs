# tsc-rs

`tsc-rs` is a Rust port of the TypeScript compiler (tsc 6.0.3), focused on
reproducing `tsc` diagnostics byte-for-byte, verified by differential
testing against the real `tsc` oracle over the TypeScript conformance
corpus.

## Status

Milestones M1 (parser), M2 (binder), and M3 (types & relations) are
complete; M4 (checker) is in progress at stage 5.8e. Measured over the
full conformance corpus (5,908 fixtures / 7,691 cases) as of 2026-07-15:

| Metric | Value |
| --- | --- |
| Exact diagnostic match, all bands | **37.7943%** (18,413 / 48,719) |
| Exact diagnostic match, 2xxx band | **46.0174%** (9,625 / 20,916) |
| False positives | **0** (hard gate) |
| Relation pins vs. tsc oracle | 403 / 403 agree |
| Determinism/idempotence invariants | 275 programs, all pass |

Every merge to `main` must keep the match counts monotonically
non-decreasing (integer ratchet) while holding the FP=0 gate.

## Repository Layout

- `tsrs2/`: the codebase — a self-contained Cargo workspace with its own
  conformance corpus (`tsrs2/ts-tests/`) and pinned TypeScript oracle
  (`tsrs2/vendor/typescript-6.0.3/`).
- `docs/`: design documentation; `docs/design/greenfield/` is
  authoritative.

## Verification

All gates run from `tsrs2/`:

```sh
cd tsrs2
cargo xtask ci            # full gate suite (must be green on main)
cargo xtask conformance   # conformance sweep (optionally --band 2xxx)
```

## License

Licensed under the [MIT License](LICENSE).
