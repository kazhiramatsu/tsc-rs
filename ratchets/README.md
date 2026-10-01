# Verification artifacts

Repository-owned accepted-state data lives here; prose design lives under
[docs/design](../docs/design/README.md).

| Location | Contents |
| --- | --- |
| [ts71/](ts71/) | The deepest error tier and emit tier each TypeScript 7.1 conformance configuration has reached, per vendored native profile (`scripts/conformance_ts71.py --check` / `--update`) |

The tsc 6.0.3-era artifacts (the M8 accepted set, escapes, families, the
H0–H2 qualification records and their pins) were deleted with the TypeScript
7.1 cutover; their last state is tag `v0.1.0` (branch `release/6.0.3`).
