<owner directive / lane or stack name, base commit, where the reviews, patches and STATUS files are archived; which rows are in, which were rejected and why>

**What** (output-identical, no mode flags): <one sentence per row or commit>

**Source identity**: branch `<branch>` = main `<commit>` + <rows>; measured binary `<name>` (sha256 `<first 16>`) built from exactly
these bytes on the common base; base binary `<name>`; the other port `<version>` (`MANIFEST.tsv` / `binprov.py pair` ok).

**Time and peak** (the verdict; window `<tag>`, base measured twice, 8 projects, 9 rounds):

| | VS Code | ts | next | zod | effect | hono | vue | playwright |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| one-thread cycles vs base (A/A `<x>` %) | … | | | | | | | |
| wall, default settings, vs base-again (A/A `<x>` %) | … | | | | | | | |
| wall, default settings, vs the other port | … | | | | | | | |
| peak footprint, MiB | … | | | | | | | |

**Ledger** (retired instructions, one thread, 9 rounds, low mode; not the verdict): <per row or per lane>.

**Counters** (`--extendedDiagnostics` Symbols/Types/Instantiations at 1 and 4 checkers vs the base): unchanged | changed: <numbers>.

**Checks run (local, at these bytes)**: `cargo test -p <crates>` <n> passed, 0 failed; `cargo clippy … --all-targets -- -D warnings` clean;
`cargo fmt --all -- --check` clean; identity against the base on the 8 bench projects at 1/1, 4/4 and the default settings (72 cells)
and <n> repeated runs: outputs identical; emit oracle <n> of <n> same [emitter / transform: `emit-vs-tsgo` 0 different];
[front end:] `scripts/syntax_fingerprint.py --compare`: 0 differing rows of <n>; [generator:] `scripts/generate_nodes.py --check` exit 0;
[checker:] `scripts/conformance_ts71.py --workers 1 --checkers 4` compared with the one-checker report: the known <n> configurations.
Not run locally (hosted `ci`): workspace tests, the one-checker conformance run and the suites.

**Bounded differences and assumptions** (what a reviewer should know a row relies on): <per row, or "none">
