# Convergence roadmap & working protocol

Status as of 2026-07-06, main @3242fdc.
Absolute conformance (all 5907 fixtures vs the real tsc 6.0.3 oracle):
**raw exact-match 60.52% / gate-filtered 62.35%**, FP 5,231 / FN 7,806
(`/tmp/fcc_rc1.json`; regenerate with the command below — /tmp is ephemeral).

This directory is the implementation-ready design set for the remaining
high-impact workstreams. Each doc is written so that an agent with no
session context can implement it. **Read this file first — the working
protocol below is not optional; it is how this project avoids shipping
regressions.**

Document map:
- **[EXECUTION-GUIDE.md](EXECUTION-GUIDE.md)** — the operating manual
  for implementing agents (step loop, FP triage, stop conditions, hard
  prohibitions). Low-capability agents follow it LITERALLY.
- `<workstream>.md` — the design (why + architecture + tsc anchors).
- `<workstream>-steps.md` — numbered implementation steps with code
  skeletons, per-step verification commands, expected outputs, and
  difficulty labels ([M] mechanical / [P] probe-first / [T] triage-
  heavy). Weak models execute the [M]/[P] steps and STOP at the marked
  stop-points in [T] stages.
- `NOTES-*.md` — reports produced by implementing agents (mining
  tables, stop-notes). Never deleted, only appended.
- **[knowledge-base.md](knowledge-base.md)** — pinned non-obvious facts
  (oracle emit artifact, standing families with known root causes,
  probe noise, harness gotchas, relation-engine invariants, tooling
  landmines). Check it BEFORE investigating any confusing FP/FN.
- **[tsc-source-guide.md](tsc-source-guide.md)** — how to read the
  vendored `_tsc.js` (techniques, verified function/line index for
  this build, checkMode bits, structural facts confirmed so far).
- **[checker-key-functions.md](checker-key-functions.md)** —
  implementation-grade porting notes for the load-bearing algorithms
  (the relation engine `isTypeRelatedTo`/`recursiveTypeRelatedTo` with
  the maybe-stack/Ternary caching, inference `getInferredType`/
  `getCovariantInference` with the exact widenLiteralTypes rule, and
  overload resolution `resolveCall`/`chooseOverload` with the two-pass
  relation + inference re-run). Rust-shaped skeletons that mirror the
  real control flow, with tsc line anchors and the current-tsrs gap.
  Read alongside greenfield §4–5 (data model) and stall-playbook §2.1
  (why the relation engine is the highest-leverage rebuild).
- **[stall-playbook.md](stall-playbook.md)** — strategic layer: how to
  detect/attribute a convergence stall, the catalog of architectural
  ceilings (relation-engine Ternary×5, resolution-order freshness,
  declaration-identity types, instantiation depth, module infra,
  strict-mode frontier) each with symptom signature + migration design,
  and the mandatory house style for big refactors. Consult it whenever
  two consecutive workstreams under-deliver, and BEFORE starting any
  refactor larger than one subsystem.
- **[greenfield.md](greenfield.md)** — the from-scratch design at
  implementation grade: architecture verdict (tsc-shaped core, with
  the empirical argument), workspace/crate layout, concrete core types
  (allocation-identity Type model with tsc's exact interning surface,
  links tables with the one-write/speculation rule, Ternary×5 relation
  engine, tsc-bit-compatible generated flags/SyntaxKind), diagnostics
  pipeline (emit-free suggestion band), the day-1 harness (program.json
  /diagnostics.json schemas, oracle process pool, in-repo goldens,
  T0–T4 ratchets, invariant suite, differential fuzzer with reducer),
  port-ledger tooling, performance/determinism budget, the strangler
  adoption map into THIS repo (§10), rebuild-trigger conditions (§11),
  and a milestone plan M0–M9 with measurable acceptance gates (§12).
  Read §10–§11 before proposing any rewrite.

## Priority table (expected yield, highest first)

| # | Workstream | Design | Steps | Standing damage | Expected yield |
|---|-----------|--------|-------|-----------------|----------------|
| 1 | Parse-error semantic gate + non-LHS `=` recovery (PAIRED — do together) | [parse-error-gate.md](parse-error-gate.md) | [steps](parse-error-gate-steps.md) | whole-file FN flips: every fixture with ≥1 syntax error loses ALL semantic diags (6133×387 mostly here, 1005×125, 1109×80, parserRealSource11 alone = 87 FN) | largest single FN lever; hundreds of file flips |
| 2 | Relation-core 2: 2339 mining + assignable-side private nominality | [relation-core-2.md](relation-core-2.md) | [steps](relation-core-2-steps.md) | 2339 = FP #1 (529), 2322 #2 (483), 2345 #3 (357); nominality FPs 2415/2430/2445 mapped | few hundred FPs |
| 3 | lib-gap axis (2304) | [lib-gap-2304.md](lib-gap-2304.md) | [steps](lib-gap-2304-steps.md) | 2304 = FN #1 (1,622 raw; partially excluded from the gate-filtered metric) | raw-metric heavy; moderate filtered yield |
| 4 | U6: unused-FP finish | [u6-unused-fp.md](u6-unused-fp.md) (buckets + root causes A/B/C inside) | FP 6133 = 156 | small, self-contained; best FIRST workstream for a new agent | ~100 FPs |
| 5 | Architectural debt (do only when a workstream is blocked on it) | [architectural-debt.md](architectural-debt.md) | anon-`{}` identity, StringMapping kind, inference widen ordering, 2403 mapped-identity (276) | unblocks documented FNs |

## Working protocol (MANDATORY)

### Environment

- Repo: `/Users/hiramatsu/dev/tsc-rs`. Oracle: `oracle/` (tsc 6.0.3,
  vendored; read `oracle/node_modules/typescript/lib/_tsc.js` for tsc
  source-of-truth). Corpus: `ts-tests/` (5,907 conformance fixtures).
- `scripts/bootstrap.sh` re-provisions everything under `/tmp`
  (chunk lists `/tmp/chunk{1,2,3,_tail}.txt`, golden
  `/tmp/golden_diag.txt`). /tmp is wiped on reboot — if the golden is
  missing, run bootstrap, then IMMEDIATELY refresh the golden from the
  CURRENT commit (`./verify.sh golden-save`) so the baseline matches HEAD.
- **Never override oracle-script parallelism.** Run
  `full_conformance_compare.py` / `parallel_classify.py` with their
  defaults (no `TSRS_CLASSIFY_JOBS` in the environment), in the
  background. `TSRS_JOBS=1` (the tsrs checker's own default) is a
  separate knob; leave it alone too.
- macOS: no `timeout`/`gtimeout`; `sed -n "A,+Np"` is unsupported (BSD
  sed) — use explicit ranges.

### The gate — nothing lands without it

1. Build: `cargo build --release` (probe and verify use
   `target/release/tsrs`; a stale binary silently invalidates every probe).
2. Tests: `cargo test --release` — 89 integration tests, all green.
3. Classifier: `./verify.sh golden-check` (background; takes minutes).
   **0 NEW_FP is a hard gate.** NEW_FN does not fail the script but the
   sweep standard is 0/0; every accepted NEW_FN must be root-caused and
   documented in the commit message (see "documented FNs" below).
4. Flow-affecting changes additionally: `./verify.sh mf`
   (worker-transport byte-identity across jobs 1–16).
5. `cargo fmt` before committing. Commit. THEN `./verify.sh golden-save`
   to refresh the golden to the committed output.
6. Re-measure absolutes when a workstream completes:
   `python3 scripts/full_conformance_compare.py --snapshot /tmp/golden_diag.txt --lib lib/lib.tsrs.d.ts --out-json /tmp/fcc_X.json --out-txt /tmp/fcc_X.txt`
   (serial ≈ 15 min, default-parallel ≈ 4 min; 5,907/5,907 classified,
   `tsc_fail` must be 0).

### Investigation tools

- `python3 scripts/probe.py <fixture.ts>` — side-by-side tsrs/oracle on
  one fixture. Lines prefixed `*` are one-sided; everything before the
  `--- tsc:` divider is the tsrs side (one-sided there = FP), after it =
  oracle side (one-sided = FN). Awk splitter:
  `awk '/--- tsc:/{s=1} /^  \* main/{print (s?"FN ":"FP ") $0}'`
- Micro-fixtures: write minimal `.ts` files (WITH `// @...` harness
  directives — batch base options are strict unless overridden) into a
  scratch dir and probe them. The oracle answers any semantics question
  in seconds; prefer probing over reasoning from memory of tsc behavior.
  This session's rule of thumb: **when two derivations disagree, stop
  deriving and probe.**
- Per-file delta vs golden: use `/tmp/golden_now.txt` (written by
  `golden-check`) against `/tmp/golden_diag.txt`, keyed by FULL absolute
  path. Two landmines that produced bogus deltas this session:
  relative-vs-absolute path keys, and fixtures whose names are prefixes
  of other fixtures (`grep` both `partiallyAnnotatedFunctionInference`
  files). Match on the exact golden key.
- Line numbers: `main.ts:N` = fixture line N after directive-header
  stripping. Directive count varies per fixture; verify alignment by
  column/message text, not arithmetic.

### Design rules learned the hard way (read before writing code)

- **Mirror tsc source; never curve-fit.** Grep
  `oracle/node_modules/typescript/lib/_tsc.js` for the function, read it,
  port its actual branch structure. Every "plausible" shortcut this
  sweep tried was caught by the classifier.
- **Expect masked-deficiency reveals.** tsrs contains compensating
  approximations; making one subsystem tsc-true exposes its neighbors as
  NEW_FPs. The correct response is to root-fix the revealed deficiency
  (it kills standing FPs too), not to soften the new code. Budget gate
  iterations for this: recent workstreams needed 3–4 rounds to 0 NEW_FP.
- **Relation-cache hygiene:** `is_assignable_to` memoizes per
  `(src,tgt)` in `rel.relation_cache`; the comparable relation runs
  under `rel.erase_generic_sigs` with its own `rel.comparable_cache`.
  Any new relation MODE needs its own cache or it cross-pollutes.
- **`fresolve.quiet` for exploratory checks:** scaffolded/speculative
  `check_expr` runs must not populate `expr_type_cache` or consume
  `report_once_*` budgets (see Tier-2 memory; violating this loses
  diagnostics permanently).
- Check-order sensitivity exists (lazy resolution + caches). If a
  fixture behaves differently truncated vs whole, you are looking at a
  resolution-order artifact; time-box it and document rather than chase
  (cf. typeArgumentsWithStringLiteralTypes01).

### Currently accepted documented FNs (do not "rediscover")

1. unknownControlFlow fx2/fx4 (2367×2): tsc `getIntersectionType` keys
   anonymous `{}` type literals by DECLARATION identity; tsrs interns
   structurally. See architectural-debt.md §1.
2. typeArgumentsWithStringLiteralTypes01 (2345×1): check-order-dependent
   literal widening in inference. See architectural-debt.md §3.

## History (context for why the code looks the way it does)

- Tier-1/Tier-2 refactor: CFG flow resolver is the only flow engine
  (docs/determinism-design.md; fact stack retired 2026-07-03).
- Unused sweep U1–U5c: tsc checkUnusedLocals/ClassMembers parity.
- Operator sweep 1 (@b8cf99e): the tsc COMPARABLE relation
  (erase-generics, union-source SOME, reversed-simple, intersection
  constraint-collapse, comparable-mode private nominality, disjoint
  `===` narrows to never). −1,063 standing FPs.
- Relation-core 1 (@3242fdc): instantiateSignatureInContextOf +
  constraint clamp, multi-sig erase, tsc assignment narrowing
  (union-only reduce), class-method-overload binder merge,
  `Signature.from_method` variance, number→enum simple rule,
  Uppercase-over-patterns + template normalization, position-based
  param inference. −140 standing FPs.

Cumulative sweep since Tier-2 merge: **+1,461 correct additions /
−1,721 standing FP removals**, zero shipped regressions.
