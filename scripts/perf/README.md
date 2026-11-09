# scripts/perf — the measured-optimization loop's tools

These are the integrator's tools for the loop described in the project skill
[`measured-port-optimization`](../../.claude/skills/measured-port-optimization/SKILL.md). They moved here from the
untracked `target/claude-tools/perf11` on 2026-11-08 (the 11-07 incident showed what untracked tools cost).

Roots (environment variables, all optional):

| variable | default | meaning |
| --- | --- | --- |
| `TSRS_REPO` | the repository this directory lives in | where `build-patches.sh` adds its measurement worktree and `binprov.py` reads commits |
| `TSRS_PERF_ROOT` | `$TSRS_REPO/target/perf` | `bin/` (measured binaries + `MANIFEST.tsv` provenance), `logs/`, `results/` |
| `TSRS_BENCH_ROOT` | `~/dev/bench-corpora` | the eight bench projects (`vscode/src`, `TypeScript-6.0/src/compiler`, `next.js/packages/next`, `zod`, `effect.noindex/packages/effect`, `hono`, `vue`, `playwright`) with their `tsconfig.bench-<variant>.json` |

| tool | what it does |
| --- | --- |
| `build-patches.sh BASE PREFIX [PATCH...]` | one release binary per patch on a common base in one reusable worktree; records provenance; removes only its own worktree |
| `measure-patches.sh BASE_NAME CAND...` (`ROUNDS=9`) | `serial.py` on the 8 projects (1 thread, warm-up 1 + N rounds) then `diffout.py` output identity per candidate |
| `serial.py OUT.jsonl LABEL=BIN,... CASE,... [--rounds N] [--warmup N]` | one-core runs under `/usr/bin/time -l`: retired instructions, cpu, peak footprint, rss; prints medians |
| `lowmode.py RECORD.jsonl [--gap PERCENT]` | low-mode deltas per binary and project: the median of each binary's lowest cluster of rounds (sorted counts split at gaps above 0.15 %), a table of low-cluster sizes and a warning under 3 rounds — the right summary for sub-1% deltas, because the projects' counts are multimodal (modes 0.35–1.7 % apart, binary-independent) |
| `diffout.py A B CASE,...` | byte comparison of exit code, stdout and stderr of two binaries, one thread |
| `binprov.py record|check|pair` | provenance rows for measured binaries (sha256, commit, patch sha, worktree); windows call `check` first |
| `phasecmp.py A.json.gz B.json.gz` | two samply profiles compared by phase root and by leaf name (the tsgo-inherited function names both ports share) |
| `fcmp.py`, `callers.py`, `children.py` | flat/leaf comparison, top callers (`--depth N`) and children of a frame in a samply profile |

Recording a profile (samply, 4 kHz, one thread), from the project directory:

```sh
TSRS_WORKERS=1 TSRS_CHECKERS=1 samply record --save-only --no-open --unstable-presymbolicate --rate 4000 \
  -o out.json.gz -- <tsc-rs binary> -p tsconfig.bench-stable.json --pretty false
```

Entry counts per function (true invocation counts, not CFG-edge counters): build with
`RUSTFLAGS='-Cprofile-generate=<dir> -Cllvm-args=-pgo-instrument-entry=true' CFLAGS='-fno-profile-generate -fno-profile-use'`
(the `CFLAGS` keep Apple clang's profile runtime out of mimalloc; see `scripts/pgo_build.py`), run once per project with
`LLVM_PROFILE_FILE`, then `llvm-profdata merge` and `llvm-profdata show --all-functions --counts`; block 0 is the entry count.

Phase-level instruction attribution: `scripts/phase_ledger.py` with a `--features phase-ledger` release build.
