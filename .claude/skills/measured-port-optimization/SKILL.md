---
name: measured-port-optimization
description: The measured-optimization loop for a port with a reference implementation (tsc-rs vs tsgo/tsrs) — first an account of the whole gap that adds up on both ports, then one-row patches each bounded by a row of that account, merged only when time and peak memory measure better and every output stays byte-identical. Use when asked to make the compiler faster or lighter, to find why it is slower than tsrs/tsgo, to compare against them, to run a "lane" of perf rows, or to measure a candidate.
---

# Measured port optimization

Two things have worked, and both are the same idea — measure first, on an instrument that covers everything:

- 2026-11-08 (#921–#926, VS Code −7.4 % retired instructions in one day): **oracle first, then attribution, then one-row
  patches, then measurement decides.**
- 2026-11-21, after ten days of testing one suspected cause at a time and finding each worth a few per cent:
  **an account of the WHOLE gap that adds up, before any cause is tested** (section 1). It showed in one afternoon what
  the single-cause loop never could: where the gap is (a different phase on different projects), that there is no single
  cause, and a list of named rows with their bounds.

Nothing here depends on understanding the checker in advance. It depends on strong oracles, a reference with the same
function names, an account that sums to the gap, and discipline about what a number can carry.

Paths: `scripts/perf/` is in the repository. `H/` stands for `target/claude-tools/handover/arch-arena/` — the session's
untracked workbench on the owner's machine (tools in `H/tools/`, job scripts and logs in `H/ceilings/`, results in
`H/fable52-1111/`, the running record `H/RESUME.md`). Tools named `H/…` are local until they are promoted to
`scripts/perf/` (they carry machine paths; promote with environment roots as #927 did).

## 0. Laws (do not negotiate these in a lane)

- Outputs are the law: diagnostics, emit and conformance byte-identical (`scripts/perf/diffout.py` on the 8 bench projects, the
  hosted conformance run, `scripts/syntax_fingerprint.py --compare` for any parser/binder change). The `--extendedDiagnostics`
  work counters (Types/Instantiations) may change when outputs are identical — record them in the PR, never gate on them
  (owner, 2026-11-08). The other port's ELISIONS (it leaves out work the reference does and creates fewer types) are not
  adopted; a memo that returns what recomputing would return is allowed (section 4).
- No mode flags, env knobs or parallel variants; change the existing code path (CLAUDE.md). No `unsafe` outside `store`/`arena`.
- **The verdict is time and peak memory** (owner, 2026-11-14: 「命令数が増えても遅くならないことが重要ですよ」): wall at the
  default settings and at one thread, cycles at one thread (the direct measure of time when wall is noisy) and peak
  footprint, in a window with the base measured twice. Instructions are the ledger for finding work and the stable number
  for deltas that wall cannot resolve — not the verdict: an instruction increase that does not make the compiler slower is
  acceptable, and an instruction decrease that costs time is not a gain. A row that is slower for less peak is not landed
  by default; put the numbers to the owner.
- **Account first** (owner, 2026-11-21: 「計測しました、違いましたをずっと繰り返していますね…調査手法を変えたほうがいいんでは？」):
  a prototype, a census or a lane is started only when a row of the account bounds its gain. Say the bound and the
  decision rule ("under X % on project P: dropped") BEFORE building. Do not call a cause likely before the account supports
  it; write the observation and what would separate it. A correction an hour later costs more than a slower first answer.
- Measure every slice against its forecast (owner, 2026-11-04); below the band → cause analysis from a fresh profile and a
  re-plan before more work on that line.
- One heavy job at a time (`nice -n 20`, `cargo -j 2`, `CARGO_INCREMENTAL=0`); time is measured in quiet windows without
  concurrent builds, on performance cores, in the owner's slots. The machine's day / night rules are in CLAUDE.md and memory.

## 1. First: an account of the whole gap that adds up

Both ports are ports of one reference and keep its function names. That makes a complete, hypothesis-free itemization
possible: every cycle sample of both ports is put on the same list of names, and the differences sum to the gap.

1. **Record both ports** at one thread with whole call stacks, on a performance core: `H/tools/ck-rec3.sh OUT CASE…` from the
   session's shell with the one-core governor paused (`H/tools/run-rec-ours.sh OUT BINARY THEIRS_FROM CASE…` does the pause
   and restores the marker on every exit; `THEIRS_FROM=<finished directory>` records a candidate alone). A job of the
   scheduler's tree runs on EFFICIENCY cores: its cycles and cycles-an-instruction are not the product's (ts read
   0.435 | 0.405 there, 0.276 | 0.273 on a performance core). The recorder prints the share of cycles by core kind — check it.
2. **Totals from quiet alternating runs**, not from the recordings: `H/tools/gap-cells.py OUT.jsonl OURS THEIRS` (both ports
   in turn, medians; cell `whole/1t`). A recording's own total carries the recorder and the hour; only its SHARES are used,
   scaled to these medians.
3. **Phases from the stacks**: `H/tools/cyc-account.py DIR CELLS --out DIR` assigns a sample to load / bind / check by the
   outermost frame that names the phase. Never split "front end / check" as whole minus a `--noCheck` run: such a run is not
   the whole run's first part (next: 1,084 | 989 M cycles against load + bind 602 | 639 M inside the whole run).
4. **Load and bind by module**: `H/tools/cyc-modules.py DIR CELLS load` — the module of each sample's nearest own function.
   Binder, loader and module resolution correspond module to module between the ports; a boundary that one port inlines
   across does NOT: the other port inlines its scanner's entry (the whitespace run and the dispatch) into parser
   functions, so "scanner 125 | 92" on playwright was no excess of the scanner — scanner + parser + tree together are
   261 | 300 on next, 116 | 125 on ts (ours cheaper) and 159 | 139, 252 | 233 on vue and playwright. Compare the smallest
   group of modules that no inlining crosses, exactly as for functions in step 6.
5. **The check by service**: `H/tools/cyc-services.py DIR CELLS --out DIR` — a sample goes to its DEEPEST frame whose function
   is in a service family (lists of the reference's function names, the SAME lists for both ports: relation, instantiation,
   members, flow, calls, …). A helper with a port's own name matches no family and is charged to the service that called
   it — the same rule on both sides. The services partition the check; the differences sum to its gap.
6. **Rows**: the tool's join files (`join-<service>.txt`) list the functions that hold a service's cycles, joined by name
   over the projects. A row is a function (or a paired group) that costs more than theirs on three projects or more.
   The ports cut their functions differently: a name on one side only is usually the same work under another name or
   inlined into its caller — pair the lines before calling an excess, and when the same work sits under a reference-named
   frame in one port and under a port's own helper in the other (the "a frame on one side only" lines), FIX THE FAMILY LIST
   instead of explaining the difference away.
7. **Lines**: `H/tools/cyc-lines.py DIR CELLS CASE [--leaf REGEX] [--phase P]` gives our samples by innermost inlined function
   and source line. It needs the same source built with line tables (`CARGO_PROFILE_RELEASE_DEBUG=1
   CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO=packed`; `H/ceilings/job-lines-cs2.sh`). That build is NOT the measured binary byte
   for byte (crate hashes, function order and data addresses differ), but functions of the same demangled name and size
   hold the same instructions: carry a sampled address over as (function start, offset inside the function), and verify on
   a sample of functions before trusting it. Cargo leaves the dSYM as a relative LINK — copy the real bundle and give its
   inner file the binary's name.
8. **Read the result as a whole.** If one row carries the gap, that is the cause. If the table is flat — services 10–40 %
   dearer beside services 5–20 % cheaper, each made of rows of 0.1–0.3 % of a run — there is NO single cause: say so, stop
   looking for one, and work the rows (sections 4–6). A restructure is justified only by rows that bound it.

Worked example (2026-11-21, ours | theirs, M cycles of a one-thread run; files `H/fable52-1111/gap-1121/account/`): next
load 470 | 459, bind 133 | 180, check 4,284 | 4,025; vue load 230 | 172, check 1,865 | 1,902; playwright load 387 | 302,
check 1,728 | 1,640; ts check 1,220 | 1,145. The gap is the check on next and ts, the load on vue, both on playwright —
four projects had been treated as one "check-side memory" problem. Load: scanner + parser + tree are cheaper than theirs
on next and ts and 20 M dearer on vue and playwright; loader, module resolution and paths are dearer (playwright 80 | 46).
Check by service (next / vue / playwright / ts): relation +108 / +34 / +7 / +42, instantiation
+74 / +28 / +14 / +4, members +48 / +46 / −3 / +24, flow +13 / −4 / +22 / +34; unions −42 / −47 / −17 / −5, names
−32 / −7 / −9 / −8. Records put together and per-target instantiation tables — the two structures under study that week —
were each bounded at about 1 % of a run by this table.

## 2. Instruments, and what each number can carry

**Time and peak (the verdict).** `H/ceilings/chain-peak7.sh TAG BASE - - CAND…` then `H/tools/verdict-peak3.py TAG CAND…`
(default settings: base, candidates, base again, the other port; one thread: the same; 8 projects, 9 rounds);
`H/tools/pair-rusage.py` for one-thread pairs; `H/tools/peak-pair.py` for peak footprint.
- Judge a window by its own controls first (base against base-again, the base's rounds). Default-settings wall is bimodal
  even when quiet: best run per cell and its control. The FIRST configuration of a case in a round can read 5–14 % slow with
  the same binary — rotate, and read against `base-again`. A window disturbed by other work keeps its footprints and has its
  time re-measured; never publish its wall numbers.
- One-thread cycles resolve about 0.2 % over 9 rounds in a quiet window; a cycle difference under 1 % needs 15 rounds.
- A binary with the base's instructions is the window's ZERO; judge the binary that lands, attribute with the partial ones.
- Before a slot, write which production state each configuration stands for (a clamped background process is not a scout).

**Instructions (the ledger).** `scripts/perf/serial.py`, `ROUNDS=9 scripts/perf/measure-patches.sh`, `scripts/perf/lowmode.py`;
`H/tools/chain-instr.py OUT.jsonl BINARY…` for a stack's prefixes. A project's counts are multimodal run to run (modes
0.35–1.7 % apart, independent of the binary; in-mode rounds spread ≤ 0.08 %): compare the median of each binary's LOWEST
cluster, flag a low cluster under 3 rounds and re-measure with 15. Plausibility first: a row cannot regress a configuration
it does not run. Counts taken under the night clamp or beside heavy load are 0.6–1.9 % high and scatter: sub-1 % deltas only
from quiet runs. Leave `--extendedDiagnostics` off when comparing instructions (its statistics walks cost).

**Top-down counters and misses** (`xctrace`, the saved CPU Counters templates; `H/fable52-1111/stalls/counters_report.py`,
`miss_sites.py`, `H/tools/miss-kinds.py`):
- A miss COUNT is not a cost. Report it only with its kind, its owner and its cycles (owner, 2026-11-20, after ten days of
  "store / TLB misses are higher"). The counting mode "processing" gives the cycles that waited on memory.
- **Price an event before building on it**, with a microbenchmark that isolates it (`H/fable52-1111/gap-1121/tlbcost/`): on
  this machine the first-level data TLB holds 160 pages of 16 KiB; a miss the second level answers adds 6.1 cycles to a load
  that others wait for and 0.6 to independent loads. Kernel prices: a fresh page 8.5 K instructions / 3.4 K cycles, a
  deep-path `stat` 8.9 K, open + read + close 15.9 K.
- Miss samples have NO callers (depth 1); only the cycle profile has stacks (`H/tools/cyc-callers.py`). A cycle sample sits ON
  a stalled plain load, on the instruction AFTER an atomic, and after the mispredicted branch that tested a loaded hash
  group — so compare functions, not instructions. The kind of the sampled instruction is not an owner either ("other load"
  holds records, custom hash tables and string rows alike).
- Sampled self time over-predicts instruction gains for memory-bound code by about 8 times; it predicts well for copies,
  formatting and repeated queries.

**Census builds and simulations** (temporary counters on a `census/*` branch that never lands, printed under an env
variable; `H/ceilings/job-census-cen1.sh` is the model): cheap answers to "how often" and "would X hit" before anything is
built. Two rules: (1) validate a model's TOTAL against the real count before reading its shares — a simulated TLB over the
primary records' reads saw 6 % of the real misses, so its shares said nothing about the other 94 %; (2) partial coverage
is biased towards "no effect" when the mechanism is a capacity limit. Count-based censuses (allocations, pushes) predict
footprint and wall only loosely.

**Entry counts in both ports** (`-Cprofile-generate` + `-Cllvm-args=-pgo-instrument-entry=true`,
`CFLAGS=-fno-profile-generate`): a function entered far more often than in the reference is an algorithmic divergence —
often a path tsgo added that changes no output and was never ported (the checker was ported from tsc 6.0's JavaScript and
aligned to tsgo by outputs). Compare on the project that shows the gap, not only on VS Code.

**Kernel and libc**: a `DYLD_INSERT_LIBRARIES` counting dylib (it does not pass through `/usr/bin/time`); file opens are
serialized system-wide on this machine (a reader crew saturates at three).

**Memory**: judge by the footprint distribution at the peak; accounted bytes are not footprint. A row that claims fewer
bytes or a shared structure gets a direct footprint run on a project where the forecast is large BEFORE it is listed — a
unit test proves the structure, only an end-to-end count proves the path.

**Parity checks before blaming code**: both ports' build settings (profile, flags, panic strategy, allocator version —
equal as of 2026-11-21); the other port's VERSION with every ratio; binary provenance (`scripts/perf/binprov.py check` /
`pair` before every window — a wrong base costs a rerun).

## 3. Explaining a row (the attribution ladder; stop when the row is actionable)

1. The account's row (section 1) names the function and its bound. For instruction-shaped work: phase ledger
   (`--features phase-ledger`, `scripts/phase_ledger.py`) and the same-name join (`scripts/perf/phasecmp.py`, `fcmp.py` on
   samply profiles of both binaries, 4 kHz; the reference binary needs symbols).
2. Callers two levels up (`scripts/perf/callers.py PROFILE 'regex' --depth 2`, `H/tools/cyc-callers.py`) to name the
   responsible source function; lines (`cyc-lines.py`) to see which inlined piece holds the cycles.
3. Entry counts (above) to separate "called more often" from "costs more per call".
4. Temporary counters in source (removed before the patch) to split a bucket and to prove before / after.
5. Three-way source read: our function, tsgo's Go function (`target/typescript7/upstream/tsc/internal/…`), the other port's
   version (its `notes/*.md` document every change with its measured delta and the rejected ones — read those first). The
   other port is read for the MECHANISM only; nothing is copied from it or from the reference.

Classify every row: (a) more calls than the reference → algorithmic (reuse, a missing tsgo path); (b) same calls, more
self time → port tax (copies where Go returns stored data, `format!` keys, string building, double probes, container
growth, allocating predicates, a fast path buried in a large frame); (c) the other port remembers an answer the reference
recomputes → a memo, admissible only as section 4 says; (d) the other port leaves work out → an elision, not adopted.

Same-name tables lie in two ways: a zero on the other side can mean "no function of that name" (Go and the other port
attribute a field read to the caller), not "no such work"; and a bare leaf name collects unrelated functions. Compare by
full symbol name or by family, state the denominator, and give a row two independent views plus the mechanism in the code
before it goes into a brief.

## 4. Rows: admission, kinds, what paid and what did not

**Admission.** A row needs arithmetic, not a profile's impression: (calls or iterations that disappear) × (instructions or
cycles each) against the project's total, bounded by its row of the account. Skipping a call with an early return is about
20 instructions — 500 K calls are 0.05 % of next. Rows of 0.1–0.3 % are worth taking when they are exact and many: a stack
of 41 such commits measured −1.4 … −3.3 % one-thread cycles on eight projects (2026-11-21).

**Memos** (the other port has many; the owner approved counter-neutral ones on 2026-11-10): state what the answer depends
on, when that is final and what drops it (a discarded speculative candidate, members resolved or replaced later, a reset
epoch); in debug builds check every hit against a recomputation wherever the recomputation has no side effect (the unit
tests run with debug assertions, so the suite cross-checks the memo); a bounded memo needs an erasure argument. A port of a
tsgo path is identity-safe by construction — pin it with a unit test whose expectations are the reference binary's output
for a program taking every branch.

**What has paid:** `format!` / `to_string` on hot paths; repeated contextual-type and name queries; resolver pass-through;
per-code-unit allocations; cold-path splits (a fast path in a small inlinable function, the slow body `#[inline(never)]`);
union member-list and accessor copies; ASCII fast paths; skipping subtrees a pass cannot touch (decorator-free subtrees: VS
Code emit −7.17 %); a target-keyed resolver memo (86 % of resolver misses re-resolved an already-resolved file); removing a
copy or an indirection in a hot record; pre-sizing and fixed blocks for PEAK; packed hash keys; boxing an owned list with
exact capacity; direct field reads at the hottest decode sites (−1.1 % with diminishing returns).

**What has not** (do not propose again without a new count): pre-sizing containers for speed; inlining early-return bodies
blindly; Bloom filters on small symbol tables; one-probe hash rewrites; branch-poor rewrites of well-predicted loops;
trading `stat` probes for directory listings; shrinking a hot record by encoding (a tag, a sentinel, a boxed list: +0.1–0.5 %
each); side-record splits of a hot struct; a bump allocator for small blocks in allocation order (cycles +0.4 … +2.5 %);
the allocator's purge delay; the shared name table's inside (two attempts); a forced collect at the hand-off; the type
links beside the type record (L1 TLB misses −3 %, peak +1.3 %: dropped); parsing directly into final identities (+0.73 %
and +560 MiB); writing binder facts into the shared parsed tree while the binder borrows it (ownership first: who owns
the document decides what a row may write). An allocation count is not an instruction
forecast (630 K fewer small allocations measured neutral).

**A rejected row gets a review** (owner, 2026-11-09: a failed gate may be the writing): read its hot paths, name the
suspected cost with counts, write the neutral form as the next brief (M2 at +0.54 % became M4 at −0.13 % / −55 MiB).
Bisect a rejected patch into one-mechanism variants and measure them in ONE clean run before attributing.

**Check a cache key before comparing miss counts**: equal misses against the reference prove the port is faithful, not
that the work is needed. A cache above a layer must respect that layer's contract.

## 5. Lanes (Codex): claims until read and measured

Two stages, both from the account's join tables (templates in `templates/`):

1. **Row review, read-only** (`templates/row-review-spec.md`): the service table, the join file and a WORK LIST of rows with
   their numbers; for each row our `file:line`, the other port's and the reference's, the mechanism, the change, why
   outputs and counters cannot change, the expected gain bounded by the row, the pinning test. Six such reviews took ten
   to fifteen minutes each on 2026-11-21 and priced their proposals honestly small.
2. **Implementation, one commit a proposal** (`templates/row-impl-spec.md`): premise re-checked first, an elision never, the
   memo rules of section 4, a unit test a mechanism, builds only through the shared slot, no release builds and no
   performance claims, `STATUS.md` at the end.

Brief rules learned the hard way:
- RELATIVE paths only and no personal details: copy the evidence into the clone's untracked `target/` and link the other
  port's source and the reference there.
- Say how the work is delivered: a lane's sandbox cannot write its clone's `.git` — an own git directory inside the clone
  and a bundle. Name the generated files and their generators (`scripts/generate_nodes.py --check` after every commit).
- Plain words: a brief asking for an "adversarial … memory safety" review was stopped by the provider's filter; "regression
  review" passed.
- State the commit an export was built and tested on; a patch that applies cleanly on a newer base can still not compile
  or fail its own test (list the rows landed since that touch the same structures and read their changed invariants).
- A lane's verdict and code are CLAIMS: read every diff, have an independent regression review for intricate ones, measure.
  When a lane's work is off the mark, or the row is quicker to write than to hand over, do it yourself (owner, 2026-11-16
  and 2026-11-21). A lane's `DIFFS-found.md` is a work list, not an archive.
- Never share one `CARGO_TARGET_DIR` between two work trees (artifacts are reused as fresh by time stamp); a pass through a
  shared directory counts only for crates printed as built in that run. Cargo 1.100 writes a package's products under
  `target/<profile>/build/<package>/<hash>/out/`, not `deps/`.

## 6. Integrator pipeline

1. One release binary per row or stack on the common base, with provenance (`scripts/perf/build-patches.sh`, `binprov.py`).
2. The job, in this order (`H/ceilings/job-cs2.sh` and `job-scan1.sh` are the models): focused tests and
   `cargo clippy … --all-targets -- -D warnings` at the final bytes (`cargo check` does not run clippy's lints); the release
   build; the instruction ledger (the early sign); identity against the base on the eight projects at 1/1, 4/4 and the
   default settings (72 cells) plus repeated runs; the work counters; the emit oracle (and `H/tools/emit-vs-tsgo.py` for a row
   that touches the emitter or a transform — the candidate-against-base oracles cannot see a difference from tsgo that the
   base already has); the fingerprint oracle for a front-end change; quiet peak pairs.
3. The time verdict in a slot (section 2). A checker change also runs `scripts/conformance_ts71.py --workers 1 --checkers 4`
   and the comparison with the one-checker report.
4. PR per stack (`templates/pr-body.md`): what, source identity, the measured tables, the checks run locally with their
   results, the checks left to hosted `ci`, counter changes, bounded differences. Merge when hosted `gates` passes
   (`gh pr merge N --merge --delete-branch`), record the run and the merge in the owning packet.
5. A stack: lint it first; measure its counters and run the oracle step and every reader on the STACK's binary before
   arming; attribute a failure by building the prefixes at each lane's end. "Applies in order" proves nothing about
   behaviour.

Jobs and windows run through one scheduler and one heavy lock (first come, first served). Never edit a running script;
ask the lock's state instead of `pgrep -f` on a name the command itself carries; give a waiter a text the predecessor
prints once; a job listed in the scheduler's file STARTS; dry-read a job to its last decision line before listing it.

## 7. Mistakes that each cost a day (read before reporting a number)

- A cause reported from one ratio. Name the measurement that would distinguish it from the obvious alternative first.
- A prototype judged by another question than the one it was built for; a switch that was never exercised read as "no
  effect"; an item "closed" on a peer's unread verdict. Write the question at the top; open the evidence.
- Recordings and cycle ratios taken through the job scheduler (efficiency cores).
- "Front end / check" split by a `--noCheck` run.
- A simulation's shares read without checking its total against the real count.
- A leaf-name join: "accessor ≈ 13 % of check self" was unrelated functions of one name.
- A module-to-module comparison across a boundary one port inlines ("our scanner is 1.3–1.6 times theirs" — reported to
  the owner, a candidate built on it, corrected two hours later).
- A count gate passed and time lost (17,364 `stat` probes replaced by cached listings: +1.2 … +2.7 %).
- A bimodal case read from 3 rounds, or 7 of 9 candidate rounds in the high mode read as +1.47 %.
- A window's first configuration charged to the base; a window disturbed by other work published.
- A base binary that was not the base (provenance), or a build beside a measurement chain (+0.5 … +0.85 %).
- A shared cargo target directory; a `cp -R` of a linked dSYM; `$VAR:word` in zsh outside braces.
- A hand-edited generated file; a brief the provider's filter stopped; a lane's export tested on another base.
