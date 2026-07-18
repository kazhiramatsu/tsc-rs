# Non-2XXX blockers roadmap

> **v1-era document.** Measurements and `src/` paths refer to the
> paused v1 codebase (tag `v1-final`); the current owner map for these
> bands is `greenfield/non-2xxx-first-order.md`. Sibling workstream
> docs referenced below (`type-checking-2xxx-roadmap.md`,
> `non-2xxx-quick-wins-steps.md`, `destructuring-parameter-implicit-any*`)
> now live under `archive/workstreams/`.

This is the cross-workstream design for everything OUTSIDE the 2XXX
band that blocks conformance convergence. It complements
`type-checking-2xxx-roadmap.md`: that document owns the 2XXX ceiling
(66.94% → 83.53%); this one owns the rest of the distance, plus the
gate-exempt lib axis and the infrastructure blockers that no
diagnostic family captures.

The same discipline applies as everywhere else: oracle probe before
believing anything, one owner per change, `NEW_FP = 0` at commit
(EXECUTION-GUIDE.md), and fresh mining before starting a branch —
every number below is a design baseline from one snapshot, not a
golden.

## Current Snapshot

Snapshot: `/tmp/fcc_after_yield_star_iterable.json`, after commit
`82d0b3b` (which already includes the first parse-gate tranche
`5412cb1` — see owner 1). Refresh before acting.

Gate-filtered mismatch files: 1,953 total, decomposing into

| Bucket | Files | Fixing it alone moves the gate metric to |
|---|---:|---|
| 2XXX-only diffs | 980 | 83.53% (the 2XXX roadmap's ceiling) |
| non-2XXX-only diffs | 661 | 78.13% |
| mixed (2XXX + non-2XXX) | 312 | flips only when BOTH sides are fixed |

On top of that, 90 files mismatch ONLY on the gate-exempt LIBCODES
(`{2304, 2318, 2583, 2584, 2792}`, `scripts/parallel_classify.py:26`).
They are invisible in the gate-filtered metric and account for most of
the raw-vs-filtered spread (65.41% vs 66.94%) — the lib axis, owner 9.

Non-2XXX families, ranked by sole-blocker count ("sole" = the file's
gate-filtered diff contains only this family; fixing the family alone
flips the file):

| Family | Sole | Involved | FP | FN | Top codes |
|---|---:|---:|---:|---:|---|
| parser recovery / 1XXX | 363 | 629 | 443 | 1,159 | 1005 (130/118), 1109 (65/85), 1055 (65/56), 1003, 1128; FN-only 1212, 1206, 1100, 1029, 1359 |
| implicit-any 7XXX (excl. 7043-50) | 72 | 170 | 299 | 319 | 7053 (82 FP), 7006 (82/61), 7026, 7057, 7031 (24/55) |
| unused band (6133 family) | 43 | 193 | 169 | 193 | 6133 (153/181), 6196, 6198/6199 |
| 18XXX (three sub-families) | 37 | 82 | 112 | 134 | 18048 (44 FP), 18033 (43 FP), 18046 (16 FP); FN-only 18013, 18016, 18037, 18010, 18028 |
| override / decl-emit 4XXX | 19 | 31 | 15 | 42 | 4114 (9/6), 4112, 4113; FN-only 4094, 4115, 4127 |
| infer-from-usage 7043-7050 | 10 | 52 | 0 | 128 | 7044 (76), 7045 (33), 7050, 7043, 7047 |
| super/new.target 17XXX | 8 | 18 | 10 | 77 | FN 17013 (34), 17009 (22), 17011; FP 17008 (6) |
| JS suggestions 80XXX | 5 | 11 | 8 | 20 | 80007 (8/4), 80008 (16 FN) |
| options 5XXX | 2 | 4 | 0 | 10 | 5076, 5087, 5061 |
| resolution-infra 6XXX / 8020 | 1 | 9 | 0 | 16 | 6138, 6263, 8020 |

Sole counts sum to 560 of the 661 non-2XXX-only files; the other 101
mix several non-2XXX families (almost always including 1XXX).

Of the 312 mixed files, the non-2XXX side is: parser 1XXX in 175,
unused in 100, implicit-any 7XXX in 79, infer-from-usage in 21. The
parse and unused families are therefore also co-blockers of the 2XXX
ceiling, not an independent tail.

### Refreshing this table

Run the FCC snapshot procedure (EXECUTION-GUIDE.md), then:

```python
python3 - <<'EOF'
import json
from collections import Counter, defaultdict
d = json.load(open('/tmp/fcc_<slug>.json'))          # <-- edit
def fam(c):
    if c in (6133,6192,6196,6198,6199): return 'unused'
    if 7043 <= c <= 7050: return 'infer-suggest'
    if 1000 <= c < 2000: return 'parse-1xxx'
    if 7000 <= c < 8000: return 'implicit-any-7xxx'
    if 17000 <= c < 18000: return '17xxx'
    if 18000 <= c < 19000: return '18xxx'
    if 2000 <= c < 3000: return '2xxx'
    if 4000 <= c < 5000: return '4xxx'
    return f'other-{c//1000}xxx'
stats = defaultdict(lambda: {'fp':Counter(),'fn':Counter(),'sole':0,'inv':0})
for m in d['mismatches']:
    fp, fn = m['gate_filtered_fp'], m['gate_filtered_fn']
    if not fp and not fn: continue                    # lib-axis-only file
    fams = {fam(x[1]) for x in fp+fn}
    for f in fams: stats[f]['inv'] += 1
    if len(fams) == 1: stats[next(iter(fams))]['sole'] += 1
    for x in fp: stats[fam(x[1])]['fp'][x[1]] += 1
    for x in fn: stats[fam(x[1])]['fn'][x[1]] += 1
for f, s in sorted(stats.items(), key=lambda kv: -kv[1]['sole']):
    print(f, 'sole', s['sole'], 'inv', s['inv'],
          'FP', sum(s['fp'].values()), s['fp'].most_common(5),
          'FN', sum(s['fn'].values()), s['fn'].most_common(5))
EOF
```

Code → message lookup: `vendor/diagnosticMessages.json` (the vendored
tsc table; keys are texts, values carry `code` and `category`).

## Verdict

One family dominates everything else combined: parser recovery plus
the parse-error gate residue. It is 55% of the non-2XXX-only files
(363), the top co-blocker of the mixed files (175), and — through
gate-suppressed semantic FNs — the prerequisite for the 2XXX FN side
(parser/ecmascript5 alone: 244 mismatch files with 192 2XXX FNs).
Nothing else in this document is worth a deep branch before that
family has a scheduled plan.

The rest is leaf and cluster work with clear owners. Two
single-root-cause quick wins stand out (18033 computed-enum FPs and
the unicodeExtendedEscapes scanner cluster), and two families already
have parked designs that only need reviving (unused FP: `u6-unused-fp.md`;
lib axis: `lib-gap-2304.md`).

## Ownership Map

### 1. Parser Recovery and the Parse-Error Gate (1XXX)

Sole 363 / involved 629; FP 443 / FN 1,159. The largest non-2XXX
owner and the only one that also gates other families' FNs.

Current state — IMPORTANT, the archived design's premise is stale:
commit `5412cb1` (2026-07-07) already landed the paired first tranche
(non-LHS `=` recovery + un-gating semantics from whole-file to
statement level). The whole-file gate is gone. The machinery today:

- `src/lib.rs:655` `collect_parse_error_stmts` computes the
  statement-key set; `lib.rs:766-768` passes it (plus
  `parse_error_files` / `parse_error_offsets`) into `checker::check`.
- `src/checker/stmts.rs:52`: statements in the set are skipped
  (usage-marked only, via `mark_parse_error_stmt_uses`).
- `src/checker/mod.rs:4403` `symbol_decls_only_in_parse_error_stmts`
  exempts symbols declared only inside skipped statements;
  `mod.rs:612` filters binder 2300s in parse-error files.
- `src/checker/regex.rs` header comment still claims "tsrs never runs
  the checker when any parse diagnostic exists" — stale for the same
  reason; fix when touching that file.

The residue decomposes into four distinct sub-problems (do not treat
"1XXX" as one workstream):

1. Recovery-profile divergence (both-sided codes): 1005 "'{0}'
   expected" (130 FP / 118 FN), 1109 "Expression expected" (65/85),
   1055 async-return-type (65/56), 1003, 1128. tsrs recovers at a
   different token than tsc, so the same construct produces different
   error positions AND different downstream statement shapes. Top
   clusters: `expressions/operators/incrementAndDecrement.ts` (16 FN),
   `es6/arrowFunction` (15 sole files), `es6/yieldExpressions` (14),
   `decorators/invalid` (13), `parser/ecmascript5/RegressionTests`
   (12).
2. Scanner cluster — `es6/unicodeExtendedEscapes` (25 sole files, the
   single largest directory). ROOT CAUSES CONFIRMED (steps:
   `non-2xxx-quick-wins-steps.md` QW-B/QW-C): (a) the 19 regex
   fixtures' FNs (1501/1508/1125/1198) exist because
   `src/checker/regex.rs` — a complete port of tsc's
   `scanRegularExpressionWorker` — is DEAD CODE: its entry point
   `check_grammar_regex_literal` has zero callers; (b) the 6
   string/template fixtures' 1199 FPs exist because tsrs's scanner
   sink lacks tsc's `parseErrorAtPosition` rule that collapses
   consecutive parse errors at the same start position.
3. FN-only grammar checks tsrs never emits: 1212/1359 (reserved-word
   identifier checks, 44+24 FN), 1206 "Decorators are not valid here"
   (41), 1100 strict-mode `arguments`/`eval` (40), 1029 modifier
   order (26). These are leaf ports on the parser/checker boundary,
   independent of recovery work.
4. Gate granularity: tsc self-censors per enclosing node
   (`containsParseError`); tsrs skips whole statements. The
   difference shows up as suppressed semantic diagnostics (2XXX,
   6133) in files whose 1XXX side already matches.

Design entry: `archive/workstreams/parse-error-gate.md` (+ steps) —
revive it, but refresh its premises first: its "tsrs drops ALL
semantics" yield analysis predates `5412cb1`, so its FN projections
overstate what is left. Re-mine, then decide how much of Part B
(node-level gating) is still needed.

Deep-design trigger: any recovery change flips whole-file diagnostic
profiles. Recovery work stays on a series-gated feature branch per
the archived doc; a single-commit "fix 1005 here" approach is how the
previous attempt got reverted.

### 2. Implicit-Any 7XXX (excluding 7043-7050)

Sole 72 / involved 170; FP 299 / FN 319.

These are not a subsystem: each code sits on top of a checker
mechanism that the 2XXX roadmap already assigns an owner. Route
accordingly instead of patching at the emission site:

- `TS7053` (82 FP, emitted at `src/checker/access.rs:1143`): FP
  clusters are `classes/staticIndexSignature/*` (26 FP over 4
  files — static index signatures unmodeled on the constructor
  side), `numericIndexingResults`, `indexerWithTuple`. Owner:
  member/access (2XXX roadmap), same `getApparentType`/index-info
  paths.
- `TS7006` (82 FP / 61 FN, `src/checker/functions.rs:113`): FP
  clusters `contextualTypeTupleEnd.ts` (15 — tuple-end contextual
  slots), `intraExpressionInferencesJsx.tsx` (8),
  legacy-decorator contextual types (6). Owner: contextual typing /
  call candidate — the same seam as execution plan section D step 3.
- `TS7031` (24 FP / 55 FN): ACTIVE design exists —
  `destructuring-parameter-implicit-any.md` + its steps doc. Do not
  duplicate.
- `TS7026` (37 FP / 34 FN, `src/checker/exprs.rs`): JSX intrinsic
  interface lookup; owner member/name/access.
- `TS7057` (36 FP, yield), `TS7023/7010` (implicit return types),
  `TS7032` (setter param), `TS7005` — smaller leaves; each needs its
  tsc anchor (`reportImplicitAny` call sites).
- `TS7027` "Unreachable code detected" (22 FN): flow owner, not
  implicit-any despite the code range; `./verify.sh mf` applies.

Hard rule (mirrors the 2XXX roadmap): never silence an implicit-any
FP by injecting a type. The FP means a contextual/member mechanism
produced the wrong type; fix that mechanism or leave it.

### 3. Unused / Suggestion Band (6133 family)

Sole 43 / involved 193; FP 169 / FN 193. Blocks 100 of the 312 mixed
files — second-biggest co-blocker of the 2XXX ceiling.

- FP side: parked design `archive/workstreams/u6-unused-fp.md` is
  ready to revive (root causes A/B already identified there:
  private-overload per-decl over-reporting, async-es5 family).
  Current top FP files match its mined buckets; re-mine to confirm.
- FN side: dominated by parse-gate residue (owner 1) — do NOT chase
  6133 FNs in files with 1XXX diffs. Remainder: the
  `usingDeclarations` cluster (8 sole files — `using` declarations
  in the unused-tracking machinery), `.d.ts` suggestion collection
  (tsrs skips `.d.ts` unused entirely; knowledge-base.md §4), and
  7043-family adjacency.
- Standing caveat for the whole suggestion band: the oracle runs
  `program.emit()` BEFORE `getSuggestionDiagnostics`, so emit
  transforms mark namespace/enum container names as referenced
  (mirrored as `unused_suggestion_emit_suppressed` — see the U5b
  notes in the conformance-sweep memory). When a suggestion-band
  FP/FN makes no sense, check emit-marking first.

### 4. 18XXX — Three Separate Sub-Families

Sole 37 / involved 82. The code range hides three unrelated owners:

- Nullish/unknown operand checks 18046/18047/18048 (16+4+44 FP,
  emitted at `src/checker/access.rs:112-155`): over-emission =
  flow-narrowing gaps. Owner: flow/operators (2XXX roadmap);
  `./verify.sh mf` required. Do not add local suppressions.
- `TS18033` computed-enum member values (43 FP concentrated in FOUR
  `enums/enumConstantMemberWith{TemplateLiterals,String}*` fixtures;
  emitted at `src/checker/stmts.rs:3260`): tsrs classifies
  template-literal/string constant initializers as computed where
  tsc's `evaluate`/`isConstantMemberAccess` treats them as constant.
  Single root cause, leaf, quick win — anchor to tsc
  `checkEnumMember`/`evaluate` before touching.
- Private-name family 18010/18013/18014/18016/18028 (all FN-heavy)
  plus the adjacent `TS2804` (33 FN) and `classes/members/privateNames`
  directory cluster (23 sole files, FN-dominant): 18013 and 2804 are
  UNIMPLEMENTED (leaf ports); 18016 (`src/checker/exprs.rs`) and
  18028 (`src/checker/classes.rs`) exist but under-emit. Owner:
  class semantics (2XXX roadmap owner section) — schedule as ONE
  privateNames cluster together with its 2XXX codes (2300 dup
  private names, 2339 on `#x` access), not code-by-code.
- Static-block statement grammar `TS18037`/`TS18041`/`TS1163`
  (13+6+2 FN in `classes/classStaticBlock/*`): tsrs classifies
  `await`/`return`/`yield` directly inside a static block as if the
  block were transparent, emitting 1375/1378/1108 FPs at the same
  positions where tsc emits 18037/18041/1163 — one wiring
  (fn-depth-aware static-block tracking) fixes both sides. Steps:
  `non-2xxx-quick-wins-steps.md` QW-D.

### 5. Override and Declaration-Emit 4XXX

Sole 19 (15 of them in `conformance/override/`) / involved 31.

- Override modifiers 4112/4113/4114/4115/4117/4127: partially
  implemented in `src/checker/classes.rs:278-345`
  (`check_override_modifiers`). Residue is both-sided (4114: 9 FP /
  6 FN) → fidelity work against tsc `checkMemberForOverrideModifier`
  (interfaces-as-base, abstract, parameter properties, dynamic
  names), not a new port.
- Exported-name visibility 4060/4078/4094 (FN-only): tsc emits these
  from declaration-emit analysis; fixtures carry `@declaration:
  true`. Unimplemented family — port only with the tsc
  `checkExportsOnMergedDeclarations`/declaration-diagnostics anchor,
  and verify how many corpus fixtures actually enable declaration
  emit before investing (19 sole files caps the yield).

### 6. super / new.target Order 17XXX

Sole 8 / involved 18; FN-dominant (77).

- `TS17013` meta-property placement (34 FN, all in
  `es6/newTarget/invalidNewTarget.*`): UNIMPLEMENTED — the parser
  folds `new.target` into the catch-all `Expr::ImportMeta` and
  discards the name, so the checker cannot see it. Leaf port
  anchored to tsc `checkNewTargetMetaProperty` /
  `getNewTargetContainer`. Steps:
  `non-2xxx-quick-wins-steps.md` QW-E.
- `TS17009`/`TS17011` super-before-this ordering (22+7 FN, partially
  implemented at `src/checker/classes.rs:1200`): under-emission in
  the statement-order tracking; fidelity against tsc
  `checkThisBeforeSuper`.
- `TS17008` JSX no-closing-tag (6 FP, `src/parser/expr.rs`): parser
  recovery adjacency — goes with owner 1 mining if it grows.

### 7. Suggestion Families: 7043-7050, 80007/80008, 8020

FN-only or near (7043-50: 128 FN, 0 FP; 80xxx: 8 FP / 20 FN).

Partial implementations already exist — do not assume greenfield:
7044/7047 at `src/checker/functions.rs:97-119`, 7050 at
`functions.rs:413`, 7045 at `classes.rs:1052` / `stmts.rs:2585`,
80007 at `exprs.rs:620`. The residue is coverage (which declarations
get the inferred-from-usage retry) and the emit-marking caveat from
owner 3. `TS80008` (16 FN, big numeric literals,
`binaryAndOctalIntegerLiteral` fixtures) and `TS8020` (9 FN, JSDoc
types in .ts) are unimplemented micro-leaves.

Priority: last among diagnostic families — suggestion-band FNs are
cheap corpus points but carry oracle-artifact risk, and 42 involved
files total cap the yield.

### 8. 5XXX Grammar Tail and Resolution-Infra Tail

- 5XXX (10 FN / 4 files, all unimplemented): despite the code range
  these are NOT compiler-option validation — they are grammar checks
  that happen to carry 5xxx codes: 5076 `??` mixed with `&&`/`||`
  without parentheses (parser-side in tsc), 5085/5086/5087
  labeled-tuple member grammar, 5061 ambient-module pattern, 5097
  `.ts`-extension imports. Micro-leaves; port with tsc anchors when
  a mined fixture needs them.
- `nonjsExtensions` / 6138/6263/6142 (7 FN), the `@module` matrix
  bail (TS6054, `declarationFileForTsJsImport` — 12×6133 infra gap
  from the U-sweep notes), and multi-file/module-resolution scope:
  these are harness/infrastructure decisions, owned by
  `stall-playbook.md` §2.5, not diagnostic ports. Record, don't
  chase.

### 9. Lib Axis (Gate-Exempt LIBCODES)

90 raw-only mismatch files; codes `{2304, 2318, 2583, 2584, 2792}`.
Parked design: `archive/workstreams/lib-gap-2304.md` (content gaps,
lib-version gating, global-merge asymmetries). Buys the RAW metric
and real-user correctness, not the gate-filtered number — schedule it
on its own merits, per that doc's caveat.

## Decision Matrix

| Observation | Action |
|---|---|
| File's diff contains any 1XXX divergence | Owner 1. Do not chase the file's semantic FNs first; they may be gate residue. |
| 6133 FN in a file with 1XXX diffs | Parse-gate residue, not U6. Skip until owner 1 moves. |
| Suggestion-band (6133/7043-50/80xxx) movement that makes no sense | Check the oracle emit-marking artifact before diagnosing tsrs. |
| 18048/18047/18046 FP | Flow-narrowing gap. Flow owner, `mf` gate; never a local suppression. |
| 18033 FP on an enum fixture | The stmts.rs:3260 constant-classification leaf. One tsc anchor, one commit. |
| Any privateNames fixture | Treat as one cluster (18xxx + 2804 + 2300/2339) under class semantics. |
| 7053/7006 FP | Member-access / contextual-typing owner from the 2XXX roadmap — shared mechanism, not an implicit-any leaf. |
| 4xxx on a non-`@declaration` fixture | Mis-mined; 4060/4078/4094 need declaration emit enabled. |
| Code has FP = 0 and FN > 0 here | Leaf port with a tsc anchor, exactly like the 2XXX FN-only inventory. |

## Sequencing

Phases are independent unless stated; N0 items can interleave with
2XXX scaffold work as §C-style local fixes (execution plan).

- N0 — quick wins, one owner and one commit each; implementation
  steps: `non-2xxx-quick-wins-steps.md` (five verified workstreams:
  QW-A enum constant evaluation 18033/1066, QW-B wire the dead
  regex validator, QW-C scanner same-start dedup, QW-D static-block
  statement grammar 18037/18041/1163, QW-E `new.target` 17013).
- N1 — parse recovery + gate granularity (the big one): re-mine,
  refresh `archive/workstreams/parse-error-gate.md` premises
  (post-`5412cb1` state above), then run it as the series-gated
  branch it was designed to be. Unlocks the es5 2XXX FN cluster and
  most 6133 FNs. This phase is also listed as 2XXX roadmap Phase 0
  work — same item, one schedule.
- N2 — unused FP finish: revive `u6-unused-fp.md`, re-mine buckets.
  Small, self-contained, good first workstream for a new agent.
- N3 — implicit-any clusters, scheduled WITH the matching 2XXX owner
  work (7053 with member-access mining, 7006 with the candidate
  boundary's contextual stage), never as standalone silencing.
- N4 — privateNames cluster (class semantics): 18013/2804/18010
  ports + 18016/18028 fidelity, together with the family's 2XXX
  codes from the FN-only inventory.
- N5 — override 4112-4117 fidelity; 17009/17011 ordering fidelity.
- N6 — suggestion band completion (7043-50 coverage, 80008, 8020)
  after re-checking the oracle artifact.
- Lib axis: independent of all of the above; per
  `lib-gap-2304.md`.

## Required Verification

The standard trio for everything (`cargo build --release`,
`cargo test --release`, `./verify.sh golden-check`), plus:

- owner 1 (parser/recovery/gate): series gate on a feature branch;
  whole-file profile flips are expected mid-series and must be
  explained in the workstream notes, never silently accepted;
- 18046/18047/18048 and 7027: `./verify.sh mf` (flow determinism);
- suggestion-band changes: probe with the oracle directly
  (`scripts/probe.py`) — suggestions ARE collected by the oracle,
  but emit-marking can make tsc's set smaller than the checker
  implies;
- scanner/regex changes (unicodeExtendedEscapes): keep
  `src/checker/regex.rs`'s stale-comment fix in the same commit only
  if the comment's claim is what the change corrects; otherwise note
  it.
