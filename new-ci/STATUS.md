# SPEC2 execution status

Completed through Milestone 2. Milestone 3 was not started.

All work and verification stayed inside `new-ci/`; repository contents were
read-only input, Cargo commands used `--manifest-path new-ci/Cargo.toml` with
`--offline`, and no root-workspace Cargo command or network access was used.

## Milestone 1

M1 was complete and committed before this M2 execution. Its acceptance record
remains:

- 27,255 pins indexed across 389 scanned files.
- Hosted-policy family count: 16.
- Harness seed coverage: 19/19.
- M8 fuzz source-reference files: 2/2.
- Oracle audit findings: 0.
- Incident validation: `e8e32f61` 1124/1124 and `e1957f77` 6/6.
- `cargo test --offline --manifest-path new-ci/Cargo.toml`: 9 tests passed.

## Milestone 2

Implemented the standalone `plan` binary and registered it in
`new-ci/Cargo.toml`. It reuses the M1 oracle extraction, compares immutable
Git snapshots, classifies core versus envelope changes, follows artifact and
oracle dependencies to a topological stale order, and predicts the family-2
through family-6 pin surfaces. It writes the ignored generated report
`new-ci/plan-report.md`.

Acceptance command:

```text
cargo run --offline --manifest-path new-ci/Cargo.toml --bin plan -- 9bacb97e..e1957f77
```

Acceptance result:

- PASS: 55/61 ladder rungs predicted stale, including the complete ladder
  closure reachable from the changed `crates/xtask/src/main.rs` root.
- 34 stale rungs were identified as transitive after an upstream re-mint.
- Six measured events were predicted: true positives 6/6, precision 1.000,
  recall 1.000.
- The report contains 2,441 projection-labelled graph edges and 77 predicted
  non-ladder surface groups.
- `cargo fmt --manifest-path new-ci/Cargo.toml -- --check` passed.
- `cargo test --offline --manifest-path new-ci/Cargo.toml` passed.
- `cargo clippy --offline --manifest-path new-ci/Cargo.toml --all-targets -- -D warnings` passed.

## Commit

No commit was attempted for M2. The reviewer commits the `new-ci` changes in
this worktree, as requested.
