# tsc-rs

`tsc-rs` is a Rust port of the TypeScript compiler (tsc 6.0.3), focused on
reproducing `tsc` diagnostics byte-for-byte, verified by differential
testing against the real `tsc` oracle over the TypeScript conformance
corpus.

## Repository Layout

- `tsrs2/`: the active codebase — a self-contained Cargo workspace with
  its own conformance corpus (`tsrs2/ts-tests/`) and pinned TypeScript
  oracle (`tsrs2/vendor/typescript-6.0.3/`).
- `docs/`: design documentation. `docs/design/greenfield/` is
  authoritative for `tsrs2/`; the remaining documents are v1-era notes
  kept for historical cross-references.

The v1 implementation that previously lived at the repository root
(`src/` and its tooling) is preserved at tag [`v1-final`]. Check out
that tag to resume it; its `scripts/bootstrap.sh` rebuilds the corpus
and oracle it needs.

## Verification

All gates run from `tsrs2/`:

```sh
cd tsrs2
cargo xtask ci            # full gate suite (must be green on main)
cargo xtask conformance   # conformance sweep (optionally --band 2xxx)
```

## License

Licensed under the [MIT License](LICENSE).
