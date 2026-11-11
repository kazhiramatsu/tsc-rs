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
  (`scripts/perf/lowmode.py`): a project's counts are multimodal run to run (modes 0.35–1.7 % apart — VS Code ≈ 0.86 %, zod ≈ 1.7 %,
  next ≈ 1.1–1.5 %, playwright ≈ 1.7 %, vue ≈ 0.6–0.9 %, TypeScript ≈ 0.35 % — independent of the binary; in-mode rounds spread ≤ 0.08 %),
  so the tool reports the median of each binary's lowest cluster with the cluster's size and flags a low cluster under 3 rounds
  (re-measure with 15). 3-round medians lie for sub-1% deltas. Wall and cpu are reported, not judged. Footprint must not grow.
  `serial.py` also records each run's cycles and `lowmode.py` prints their medians: instructions do not show stalls, cycles do, at a
  run-to-run spread of about 1.5 % (read a cycle difference under 1 % only from 15 or more rounds).
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
- Same-name tables lie in two ways (the "accessor ≈ 13% of check self" episode, 2026-11-08): a B = 0 column can mean the reference has
  no function of that name (Go attributes a field read to the caller), not that it does no such work; and a bare leaf name collects
  unrelated functions (`flags_of` of TypeFlags, `Vec::as_slice`, the store's `address`). Before quoting an excess, compare by full symbol
  name (`fcmp.py`) or by inline-attributed source file (the P4 method), and state the denominator. Row-level evidence needs two
  independent views (A/B full-name self, callers or children, entry counts) plus the mechanism in the code.
- Decode tax of the AST records: direct field reads at the 300 hottest `NodeData::` sites gave −1.1% on VS Code with diminishing returns
  (50 sites −0.81%, the next 250 −0.3%) and cover the whole `Node::data` subtree; the binder's routing asserts −0.65%; a direct child
  visitor in the generated code ≈ 0. The remaining accessor time is single loads any representation performs — a record rewrite is not
  justified by this mechanism.
- Already in the code (do not propose again from tsrs's catalog): H2.2 contextual type carried from `is_const_context_ex`, H4.6
  `resolve_name` pass-through kinds, R1.1 inference visited map reuse; the symbol tables cost the same as tsrs's (≈ 250 ms each on
  VS Code).
- Chains: run one sequential chain per measurement window; never wait on `pgrep -f` with a pattern another chain's command line can
  contain (two chains deadlocked for 2 h). The fingerprint oracle takes the `syntax_fingerprint` example binary
  (`cargo build --release -p tsc-rs-binder --example syntax_fingerprint`), not the CLI. Check a chain ~30 min after launch.

- Instruction parity is not time parity (2026-11-11): the checker ran tsrs's instruction count (67.4 vs 66.8 G on VS Code) and still took
  0.5–0.65 s longer (IPC 2.96 vs 3.31); the memo rows of 11-10 removed 7.7 G (10 %) of its instructions and 0.19 s (3.6 %) of its time.
  `--noCheck --noEmit` isolates the front end (ours 27–28 G vs tsrs 18.9 G: the whole instruction gap). Top-down counters need no root:
  `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun xctrace record --template 'CPU Counters' … --launch -- BIN ARGS`, then
  `xctrace export --xpath '…/table[@schema="CounterMetricAggregatedForProcess"]'` (rows: useful, back-end, front-end, discarded in 1/10000);
  ours 34 / 13 / 33 / 20 %, tsrs 37 / 9 / 36 / 19 %. A row that removes stalls (allocation traffic, growth copies, cache-missing loads) shows
  in cycles, not in instructions — price it there. Libc/kernel call counts of both binaries: a `DYLD_INSERT_LIBRARIES` counting dylib (it
  does not pass through `/usr/bin/time`); the file-system calls of ours and tsrs are the same, the memory calls are not.
- Check what a cache key really distinguishes before comparing miss counts with the reference (row L1, 2026-11-11): our resolver misses
  equalled tsgo's (57,192), and 86 % of them re-resolved a file another directory had already resolved (54,906 directory/specifier pairs,
  7,455 targets on VS Code). Equal counts against the reference prove the port is faithful, not that the work is needed.
- A cache above a layer must respect that layer's contract: the resolver's tests pin that a repeated request probes the host again, so the
  per-target memo lives in the loader, beside the per-directory cache that already assumes pure host observations.
- Syscall counts are not instructions (tax-l L2, 2026-11-08): replacing 17,364 `stat` probes with a cached `read_dir` per directory passed
  its count gate (6 probes per directory) and still cost +1.2% on VS Code and +2.7% on next.js — the listing and its cache cost more
  instructions than the probes saved. A row that trades one kind of syscall for another needs an instruction estimate of both sides.
- A branch-poor rewrite of a well-predicted loop costs more (proto-reloc R3, +1.08%): branchless delta tables and per-word min/max executed
  more instructions than the predictable branches they replaced. Under the store contract, parsing directly into final identities (R2)
  cost +0.73% and +560 MiB — the claimed-placement pushes and lease bookkeeping exceed the 143 ms relocation pass.
- Check that a SPEC's target still exists on the lane base by hunk ranges, not by grepping names (tax-k K2): the decode that K2 was to
  remove had already gone with P2 (#931), and a `grep -c` on the patch returned 0 because the hunk did not carry the function name.
- A reboot kills every lane and chain and clears `/private/tmp` (measurement worktrees); binaries, results and lane evidence live under
  `target/` and survive. Resume a lane with a short "finish" brief that names what it had already produced; re-run a chain from its log.
- Accounted bytes are not footprint (type32): the memory report's types row fell 18 MiB on VS Code while the peak footprint rounds moved
  ≈ 4 MiB; judge a memory row by the footprint distribution, and record the accounted rows beside it.
- Memory rows (2026-11-09, lanes members/signatures/nodelinks/fe-flags): shrinking a hot record by encoding — a representation tag or
  handle (M2 +0.54%), a boxed list in place of a pooled span (S4 +0.22%, 604k allocations), a bit-packed length with a sentinel (A +0.16%
  for 12 MiB of headers that were not even at the peak) — costs 0.1–0.5% every time. What paid: removing a copy or an indirection
  (M4 −0.13% / −55 MiB), pre-sizing and fixed blocks (N4 −0.15% / −37 MiB), packed hash keys (K2–K4, C2/C4). Judge a memory row by the
  footprint distribution at the peak, and by the hot read's load count before building it.
- Ownership first (fe-sym/fe-flags, 2026-11-09): a row that writes binder facts into the shared parsed tree cannot land while the binder
  borrows the registry's `ParsedDocument.source` — the walls are structural (who owns the document), not layout; see the shared-document
  design in `handover/arch-arena/forkdesign/` before proposing another in-node row.
- Side-record ("rare") splits of a hot struct cost instructions in every writing (signatures S2/S3/S4 and the bisected B2: +0.1…+0.5%) even when
  the moved fields are read under 2 M times; boxing an owned `Vec` list into `Box<[T]>` with exact capacity is free (B1 −0.08%, −26 MiB). Bisect a
  rejected patch into one-mechanism variants and measure them in ONE clean run before attributing (2026-11-09).
- Never build while a measurement chain is in its serial window (check `ceilings/logs/*` for an active `serial.py`): chain af's variants read
  +0.5…+0.85% under my concurrent clippy builds and ≈ 0 / +0.45 / +0.03 in the clean re-run.
- After a hand-resolved conflict run `cargo clippy -p <crate> --all-targets -- -D warnings` (the hosted job runs `--workspace --all-targets`):
  `cargo check` does not run clippy lints; `items_after_test_module` caught impl blocks appended after `mod tests` in 16 files (#940).
- Four-checker memory under determinism (fourchk-mem, dup-census): the +252 MiB of accounted growth is 264 MiB of other checkers' declaration
  records minus own, plus 84 MiB of capacity (74 slack); binder tables are already shared by pointer; representation rows can shave ~20–30 MiB.
  The 4-thread peak falls with the 1-thread structural rows, not with per-checker tricks.
- A lane patch written on an older base can apply cleanly to main and still not compile (S2b on #940: a consumed Vec dropped twice, a test on
  the old publish API): run `cargo check` and `cargo test --no-run` on main + patch right after the index-only apply check, before a chain.
- Bimodality is not VS Code-only (2026-11-09): zod (modes ≈ 1.8 % apart) and next (≈ 1.5 %) are bimodal and binary-independent; one 9-round run
  put 7/9 candidate rounds in the high mode and the lower-half median reported +1.47 % for a patch that never executes in noEmit. Plausibility
  first (a row cannot regress a configuration it does not run), then the sorted rounds, then 15 rounds for that case; `lowmode.py` reports
  the lowest cluster and flags a low cluster under 3 rounds (lowest-cluster rule, 2026-11-10).
- Sequence chains that build (identity gates) after the measurement queue, not interleaved with it — a quiet check at start is not enough — and
  give grep-based start conditions a text the predecessor prints once and never in its own wait echo (r9 gates fired early on their own text).
- Emitter rows (census → measured, 2026-11-09): skipping decorator-free subtrees in the legacy decorator pass cut VS Code emit by 7.17 % (−17.3 G;
  visits −70 %), while removing a 132 MB text copy (R3) or a 9,244-unit path plan (R9) moved instructions by ≤ 0.1 % and only footprint (−10 /
  −6 MiB on the TypeScript bundle). Visits of 14 M nodes cost 17 G; a memcpy of 132 MB costs almost nothing — price the walk, not the bytes.
