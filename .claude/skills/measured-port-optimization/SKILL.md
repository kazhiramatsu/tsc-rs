---
name: measured-port-optimization
description: The measured-optimization loop for a port with a reference implementation (tsc-rs vs tsgo/tsrs) — find where the port executes more instructions than the reference, remove it in one-row patches, and merge only what measures and keeps every output byte-identical. Use when asked to make the compiler faster or lighter, to compare against tsrs/tsgo, to run a "lane" of perf rows, or to measure a candidate.
---

# Measured port optimization

The loop that produced #921–#926 on 2026-11-08 (VS Code −7.4% retired instructions in one day, outputs identical):
**oracle first, then attribution, then one-row patches, then measurement decides.** Nothing in it depends on understanding the
checker in advance; it depends on strong oracles, a reference implementation with the same function names, and discipline.

## 0. Laws (do not negotiate these in a lane)

- Outputs are the law: diagnostics, emit and conformance byte-identical (`scripts/perf/diffout.py` on the 8 bench projects, the hosted
  conformance run, `scripts/syntax_fingerprint.py --compare` for any parser/binder change). The `--extendedDiagnostics` work counters
  (Types/Instantiations) may change when outputs are identical — record them in the PR, never gate on them (owner, 2026-11-08).
- No mode flags, env knobs or parallel variants; change the existing code path (CLAUDE.md). No `unsafe` outside `store`/`arena`.
- Measure retired instructions at one thread (`TSRS_WORKERS=1 TSRS_CHECKERS=1`), warm-up 1 + **9 rounds**, and compare the **low mode**
  (`scripts/perf/lowmode.py`): VS Code's counts are bimodal run to run (two modes ≈ 0.75% apart, independent of the binary); 3-round
  medians lie for sub-1% deltas. Wall and cpu are reported, not judged. Footprint must not grow.
- One heavy job at a time (`nice -n 20`, `cargo -j 2`, `CARGO_INCREMENTAL=0`); ≤ 2 Codex lanes building; measurements in quiet windows.

## 1. Attribution ladder (coarse → fine; stop when a row is actionable)

1. Phase ledger (`--features phase-ledger`, `scripts/phase_ledger.py`): instructions per phase; against the reference's phase times.
2. Same-name function join against the reference (`scripts/perf/phasecmp.py`, `fcmp.py` on samply profiles of both binaries, 4 kHz):
   self/inclusive per tsgo-inherited leaf name. The reference binary needs symbols (tsrs's release profile strips them: build with
   `--config profile.release.strip=false --config 'profile.release.debug="line-tables-only"'`).
3. Entry counts in both ports (`-Cprofile-generate` + `-Cllvm-args=-pgo-instrument-entry=true`, `CFLAGS=-fno-profile-generate`): a function
   called more often than in the reference is an algorithmic divergence; same count and more self time is implementation tax.
4. Callers two levels up (`scripts/perf/callers.py PROFILE 'regex' --depth 2`) to name the responsible source function.
5. Temporary ledger sub-scopes or atomic counters in source (removed before the patch) to split a bucket and to prove before/after.
6. Three-way source read: our function, tsgo's Go function (`target/typescript7/upstream/tsc/internal/…`), the reference's version
   (tsrs: `notes/*.md` document every change with its measured delta and the rejected ones — read those first; `handover/arch-arena/tsrs-notes/`).

Classify every row: (a) more calls than the reference → algorithmic (reuse, caching, skipped work; tsgo may do the same extra work);
(b) same calls, more self time → port tax (copies where Go returns stored data, `format!` keys, string building, hash double-probes,
container growth, allocating predicates); (c) work-count divergence (types/instantiations created).

## 2. Lanes (Codex, one SPEC each; template in `templates/lane-spec.md`)

- One row = one patch against the lane's base (`target/lane-<name>/patches/<Row>-<topic>.patch`, plus `ALL.patch`); independent where
  possible. Cumulative series are acceptable only if the lane says so (then build cumulative prefixes and attribute by differences).
- `STATUS.md`: per row — done / verified (command + quoted output) / open / blocked, and before/after counts from temporary counters.
- Allowed builds: `cargo check`, focused `cargo test -p <crate> --lib <modules>`, `cargo fmt`, clippy of touched crates, one dev CLI build
  for a smoke run on hono (stdout, exit, emit identical to the unmodified base). No release builds, no benchmarks, no performance claims.
- Give the lane the evidence (profiles, `callers.py`, the reference's notes, the catalog rows) and the Go/reference line pointers; ask for
  the tsgo line and the equivalence argument for every site. Time box one working day; stop at the box.
- Lane gotchas: Codex's sandbox cannot run samply (`Unknown(1100)`) — the integrator records profiles; `nice` fails inside the sandbox (harmless);
  tests on process-global state must be run at `--test-threads=8` before trusting them (arena flake, #919); check patches apply on the
  common base before building.

## 3. Integrator pipeline

1. `scripts/perf/build-patches.sh BASE PREFIX patches...` → one release binary per row plus `main-<BASE>`, provenance rows in
   `$TSRS_PERF_ROOT/bin/MANIFEST.tsv` (`binprov.py check` before every window).
2. `ROUNDS=9 scripts/perf/measure-patches.sh main-<BASE> <PREFIX>-...` → instructions (8 projects) and `diffout.py` identity per row;
   summarize with `scripts/perf/lowmode.py <results>/patches-<tag>.jsonl`.
3. Decide per row: merge when the low-mode delta is ≤ −0.3 G on VS Code **or** ≤ −0.5% on any project, with identical outputs on all
   eight; reject noise-level rows (D2, D6, D7, F2, F6, A1, A8 were ≈ 0; D1 inlining was **+0.1…+0.5% everywhere**); hold mixed rows for
   splitting (D4). Record every row's numbers even when rejected.
4. PR per lane (template `templates/pr-body.md`): what, source identity (branch = main + patches; measured binaries = base + patch),
   the measured table, the checks run locally with their results, the checks left to hosted `ci`. Front-end change → fingerprint oracle
   result in the body. Counter changes → state them.
5. Merge when hosted `gates` passes (`gh pr merge N --merge --delete-branch`), record the merge in the owning packet, measure the
   integrated main against the pre-pivot base (9 rounds) and record it. Remove the PR worktree.

## 4. Lessons from the measured rows (2026-11-08)

- What paid most: `format!`/`to_string` on hot paths (A4 −1.1%), repeated contextual-type queries (A7 −1.0%), resolver pass-through +
  accessibility reads (E5 −1.3%), per-code-unit allocations in spelling (A2 −0.7%), parser cold-path splits (C5 −0.7%), union
  member-list copies (A5), accessor copies (A6), ASCII fast paths in path normalization and position indexing (F3 −0.7%, F4 −0.6%).
- What did not: pre-sizing containers (≈ 0.5% total — sampled rehash time was memory latency, not instructions), inlining early-return
  bodies (worse), Bloom filters on small symbol tables (0), one-probe hash rewrites (≈ 0), UTF-8 boundary copies (≈ 0), loader rows on
  module planning/resolver allocations/inclusion reasons (≈ 0 despite large census drops).
- Sampled self time over-predicts instruction gains for memory-bound code by ~8×; it predicts well for copies, formatting and repeated
  queries. Reference catalogs (tsrs's notes) transfer only after checking the mechanism exists here and the workload resembles theirs.
- Structural extras vs the reference that no one-day row removes: AST identity relocation (≈ 2% of VS Code), the UTF-16 position index,
  multi-layer program publication (≈ 1.2%) — the arena design's phase-B territory; a ceiling experiment (fe4) showed the naive version loses.
