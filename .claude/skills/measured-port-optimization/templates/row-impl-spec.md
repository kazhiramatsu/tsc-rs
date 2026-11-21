# Implementation task: rows of one service, one commit each

<!-- A brief for one implementation lane that follows a row review. Fill the angle brackets; keep every path RELATIVE
     to the lane's clone: the review as `target/lane/REVIEW.md` (its absolute paths made relative), the tables in
     `target/account/`, links `target/other-port`, `target/reference-go` and `target/tools` (the build-slot wrapper).
     No personal details, no home paths. The clone is clean and at the base commit before the lane starts. -->

You work in a clone of a Rust port of the TypeScript 7.1 compiler (reference implementation: the Go compiler `tsgo`).
The branch named at the end of this brief is checked out; your work goes on top of it.

Cycle-weighted samples of this port and of another Rust port of the same compiler (both keep the reference's function
names) were put on SERVICES of the type check and joined by function name over <n> real projects. A read-only review
then explained the rows of one service: for each row what our code does that costs the cycles, what the other port
and the reference do, and the change to OUR code. You implement that review's proposals, one commit a proposal.

Read first (all paths are relative to the clone's root):
- `target/lane/REVIEW.md` - the review: section 1 the rows, section 2 its ranking, section 3 what it did not verify,
  section 4 further rows. Its findings are CLAIMS. Its gains are estimates, mostly small: the point of this work is
  many small exact rows.
- `target/account/services-<project>.txt` and `target/account/join-<service>.txt` - the measurements behind it.
- `target/other-port` - the other port's source, for reading a mechanism only. `target/reference-go/internal/checker`
  - the Go reference, for the semantics.

## Hard rules
- BEHAVIOUR MUST NOT CHANGE. Every output of the compiler (diagnostics text and order, emitted files, exit codes) stays
  byte-identical at every thread count, and the work counters Types / Instantiations / Symbols stay the same. The order
  in which types, symbols, signatures, lists and names are created and interned must not move. A change that cannot
  be argued to keep this is not made: skip the proposal and say why in STATUS.md.
- Before implementing a proposal, re-read the code it cites and check its premise. If the premise is wrong, or the
  code differs from what the review describes, skip it and say why. Never implement what the review labels ELISION.
- A proposal that REMEMBERS an answer (a memo, a cached bit, a kept id): state in the commit message what the answer
  depends on, when that is final and what drops it; drop it on every such event (a discarded speculative candidate,
  members resolved or replaced later, a reset epoch). In debug builds every hit must be checked against a
  recomputation with `debug_assert!` wherever the recomputation has no side effect - the unit tests run with debug
  assertions, so the whole suite then cross-checks the memo.
- Do NOT copy code or comments from the other port or from the Go reference. Write our own code in this codebase's
  style: read the neighbouring code first (doc comments in plain English that say what and why; the `tsc-port:` /
  `tsgo-port:` header lines of existing functions stay as they are).
- One mechanism per commit. Message: `perf(<crate>[, <crate>]): <what changed, in plain words>` and a body that says
  what the code did, what it does now, why outputs and counters cannot change, and names the tests. Do not claim a
  measured gain of your own; quote the row's numbers from the review as the reason.
- A unit test for every new data structure, cache or invariant, in the crate's existing test modules and in their
  style. A pure reordering of side-effect-free tests needs no new test: say in the message why it is pure.
- Where a proposal is about a fast path that should return before a large frame is built: make the fast path a
  small function that can be inlined into callers and put the slow body in its own `#[inline(never)]` function.
  You cannot check the machine code here (no release builds); it is checked elsewhere.
- GENERATED FILES ONLY THROUGH THEIR GENERATOR: `crates/syntax/src/nodes.rs` is written by
  `python3 scripts/generate_nodes.py` (`--check` must exit 0 after every commit that touches the syntax crate).
- Only files under `crates/` (and `scripts/generate_nodes.py` if a node field is needed). No docs, no README, no
  ratchets, no dependency changes, no new crates. `unsafe` only in `crates/store` (every other crate forbids it).
- Builds only through the shared build slot, offline, ONE cargo command at a time. This session and every command it
  starts ALREADY run at the lowest scheduling priority. Do NOT call `nice`, `renice` or `taskpolicy`: the sandbox
  refuses a priority change, and such a refusal is not a failed check.
  `zsh target/tools/lane-cargo.sh cargo clippy -j 2 --offline -p <crate> [-p <crate>] --all-targets -- -D warnings`
  `zsh target/tools/lane-cargo.sh cargo test -j 2 --offline -p <crate> --lib <filter> -- --test-threads=2`
  After EACH commit: `cargo fmt --all --check`, the clippy line for the crates you touched, and the tests you added
  or that cover the code you changed. At the END, once: the whole library suites of the touched crates and clippy of
  all touched crates with all targets. The wrapper can wait a long time for its turn: wait for it, never bypass it,
  and stay in this session until the last command has returned.
- NO release builds, no running of the compiler on real projects, no benchmarks, no network, no `git push`. Do not
  touch anything outside this clone. Measurement is done elsewhere.

## How to deliver the commits
The sandbox does not let you write this clone's `.git` (`git commit` fails on `index.lock`). Keep the series in a
git directory of your own inside the clone:
  `git clone --bare --shared . target/lane/series.git`   (once; it borrows the objects of `.git`, which you can read)
  `git --git-dir=target/lane/series.git --work-tree=. add -A crates scripts && git --git-dir=target/lane/series.git --work-tree=. commit -m ...`
with a branch of the name given below in that git directory, starting at the base commit given below; commit as the
repository's configured identity (do not set another author). At the end write the bundle
  `git --git-dir=target/lane/series.git bundle create target/lane/series.bundle <base>..<branch>`
and leave the working tree equal to the series' tip.

## Order and stopping
Take the proposals in the order of the WORK LIST below. A proposal that needs more than about ninety minutes, or
whose equivalence you cannot argue, is skipped with its reason - go on with the next. Stop when the list is done.

## When you finish, or must stop
Write `STATUS.md` at the clone's root: for every proposal of the work list its commit hash, or SKIPPED with the
reason; the exact check commands you ran, each with its exit code and the commit it ran at; what you could not
verify; and anything in the review that you found to be wrong or imprecise (with file and line).

## This lane
- Branch `<branch>`, base commit `<commit>` (the branch's start in your own git directory).
- Service: <name> (`<our files>`). Measurements: `target/account/join-<service>.txt` and the service's block in
  `target/account/services-<project>.txt`.

## Work list
The proposals of `target/lane/REVIEW.md` in the order of its section 2 (the ranking), then those of its section 4.
Notes on single proposals:
- <what must stay exactly as it is and why: a pending change to the same code, a second reader of a counter, a
  mechanism of ours that the other port does not have, a front-end fact that reaches the fingerprint or the image>
