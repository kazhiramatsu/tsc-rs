<owner directive / lane name, base commit, where the lane's patches and STATUS are archived; which rows are in, which were rejected and why>

**What** (output-identical, no mode flags): <one sentence per row>

**Source identity**: branch `<branch>` = main `<commit>` + patches <rows>; measured binaries built from `<base>` + each patch
(`$TSRS_PERF_ROOT/bin/<prefix>-*`, `MANIFEST.tsv`).

**Measured** (`scripts/perf/serial.py`, 1 thread, retired instructions, 9 rounds, low mode; `<results file>`):

| row | VS Code | ts | next | zod | effect | hono | vue | playwright |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| … | … | | | | | | | |

**Counters** (`--extendedDiagnostics` Symbols/Types/Instantiations on VS Code and hono vs the base): unchanged | changed: <numbers>.

**Checks run (local)**: `cargo test -p <crates>` <n> passed, 0 failed; `cargo fmt --all -- --check` clean; `diffout.py` base vs each row's
binary and vs this exact branch's release binary on the 8 bench projects: outputs identical; [front end:] `scripts/syntax_fingerprint.py
--compare <baseline> --corpus all`: 0 differing rows of 143,190; [generator:] `scripts/generate_nodes.py` reproduces the committed files.
Not run locally (hosted `ci`): workspace tests, clippy, the one-checker conformance run and the suites.
