# Stall playbook: what to do when convergence stops

Audience: whoever is steering the project when the per-commit yield
drops. This doc does three things: (1) defines how to DETECT and
ATTRIBUTE a stall, (2) catalogs the architectural ceilings we already
know about, each with a symptom signature and a migration design,
(3) fixes the house style for large refactors (the pattern that already
worked twice) so a big rework doesn't regress the sweep.

Historical baseline for calibration: as of @3242fdc the sweep had moved
gate-filtered conformance 50.4% → 62.35% in ~4 days of sessions with
zero shipped regressions. Historical per-workstream yield: operator
sweep −1,063 FPs, relation-core-1 −140 FPs, unused sweep ≈ −500 FPs +
1,200 adds. Expect yields of that ORDER while the mapped workstreams
(parse-error gate, relation-core-2, lib-gap) remain; a stall before
those mapped workstreams remained was an execution problem, not an
architecture problem.

## 1. Stall detection & attribution

A stall is NOT "a workstream took several gate rounds" (normal: ≤4).
Declare a stall only when BOTH:

- two consecutive completed workstreams each moved gate-filtered
  conformance < +0.15pt, AND
- the FP/FN long tail shows no cluster ≥ ~80 diagnostics
  (top-8 codes each < 80 in the fcc snapshot).

### Attribution procedure (run before choosing any remedy)

1. Fresh absolute snapshot (README command) → `/tmp/fcc_cur.json`.
2. Regenerate the **sole-blocker lever table**: for each mismatched
   file, compute which diagnostic FAMILY, if fixed alone, would flip
   the file to exact match. Script sketch (family = code buckets:
   relation {2322,2345,2339,2769,2554,2367,2365}, unused
   {6133,6196,6198,6199,6192}, lib {2304,2318,2550,2583,2584,2792},
   parser {1005..1999}, implicit-any {7005,7006,7034,7044,7050},
   iteration {2488,2802}, other):

```python
import json
from collections import Counter
d = json.load(open('/tmp/fcc_cur.json'))
FAMS = {...}  # code -> family map as above
def fam(c):
    return FAMS.get(c, 'other')
sole = Counter(); involved = Counter()
for m in d['mismatches']:
    fams = {fam(t[1]) for t in m['gate_filtered_fp']} | {fam(t[1]) for t in m['gate_filtered_fn']}
    for f in fams: involved[f] += 1
    if len(fams) == 1: sole[fams.pop()] += 1
print('sole:', sole.most_common()); print('involved:', involved.most_common())
```

3. Read the distribution:
   - One family dominates `sole` → not a stall; that family's
     workstream is just not done. Go back to it.
   - `sole` is flat but ONE family dominates `involved` → that family
     is load-bearing for many files: prioritize it even if individual
     clusters look small.
   - Everything flat, clusters tiny, and mining sessions produce
     one-off fixes each worth <10 diags → **architectural stall**;
     continue to §2 and match a signature.
4. Additionally sample 20 random mismatched files and hand-classify
   the divergence cause into: missing-feature / wrong-approximation /
   order-dependence / identity-model / display-only. The bucket with
   plurality picks the §2 remedy.

## 2. Known architectural ceilings (signature → remedy)

Ordered by likelihood of being the binding constraint, based on what
this sweep already scraped against.

### 2.1 Relation engine: bool + single-mode vs tsc's Ternary × 5 relations

**Symptom signature**: relation-family FPs/FNs that flip depending on
which side is queried first, or that mining traces to the coinduction
shortcut (`relation_stack` hit → `true`), or to cross-relation cache
reuse; fixes in one relation family regress another in the same gate
round repeatedly.

**Divergence**: tsrs has ONE boolean relation (`is_assignable_to`) with
one `(src,tgt)` cache plus a bolted-on comparable mode
(`erase_generic_sigs` + `comparable_cache`). tsc has FIVE relations
(identity / subtype / strictSubtype / assignable / comparable), each
with its own cache, and results are **Ternary** (True/Maybe/False):
`Maybe` marks results that depended on an in-progress recursion and are
NOT cached, which is exactly the class of bug the bool engine cannot
express (we cache coinductive `true`s that tsc would discard).

**Remedy design** (large; the Tier-2 of the type side):
- `enum Relation { Identity, Subtype, StrictSubtype, Assignable, Comparable }`
  threaded as a parameter through `related()` and everything it calls;
  the `erase_generic_sigs` flag and `force_erase` parameter dissolve
  into `relation == Comparable` checks (mechanical translation of the
  sites that consult them today).
- `enum Ternary { False, Maybe, True }` return type for the internal
  engine; public `is_assignable_to` keeps its bool signature
  (`!= False`). Cache insert ONLY when the result did not depend on the
  maybe-stack (mirror tsc `checkTypeRelatedTo`'s `maybeKeys` handling —
  read it in `_tsc.js` first; it also RE-CHECKS maybe entries).
- Per-relation cache: `HashMap<(Relation, TypeId, TypeId), bool>`.
- Migration: stage R0 = introduce the enum with Assignable everywhere +
  byte-identical gate; R1 = Ternary internals, cache policy switch,
  classifier gate (expect standing-FP movement); R2 = move comparable
  onto the enum (delete the flag) — byte-identical; R3 = introduce
  Subtype where tsc uses it (union subtype reduction, overload
  ranking, literal widening decisions) — one call-site family per
  commit, classifier-gated. R3 is where new conformance comes from.
- Prerequisite for: signature-relation fidelity beyond erasure,
  union/intersection reduction parity, strictSubtype-driven fixes.

### 2.2 Lazy resolution order-dependence (freshness & cache decay)

**Symptom signature**: probe results change when a fixture is truncated
or statements reordered (already observed:
typeArgumentsWithStringLiteralTypes01); diagnostics differ between
first and second identical calls; mining notes say "works in micro,
fails in fixture".

**Divergence**: tsrs decides literal widening (and some alias/return
resolution) based on FRESHNESS that decays through caches
(`sig_ret_cache`, `alias_type_cache`, `expr_type_cache`), so the
outcome depends on which caller resolved a lazy thing first. tsc keeps
freshness as an intrinsic property (`regularType`/`freshType` pair on
literal types) and makes widening decisions from RULES
(getInferredType's widenLiteralTypes, getWidenedLiteralTypeForInitializer),
not from arrival order.

**Remedy design** (medium):
- Make freshness a stable bit: tsrs already has `types.fresh(t)` /
  `regular(t)` pairs — the defect is call sites deciding via
  `is_fresh(cached_thing)`. Inventory every `is_fresh` /
  `widen_literal` / `widen_fresh` call site (~grep count as of
  @3242fdc: small, <30) and classify each against the tsc rule it
  should implement; replace order-dependent ones with the rule
  (archive/workstreams/relation-core-2-steps.md STAGE I covers the
  historical inference instance).
- Add a determinism check to CI: run 20 fixtures BOTH whole and
  bisected (first half only) and assert the shared prefix's
  diagnostics agree. Cheap harness, catches the whole class.

### 2.3 Declaration-identity types (structural interning ceiling)

**Symptom signature**: divergences that reduce to "tsc distinguishes
two structurally identical types" — anonymous `{}` literals
(unknownControlFlow fx2/fx4), fresh object literals in
getIntersectionType, `mappedTypeModifiers`-style identity comparisons
(2403 family, 276 FPs), alias-identity display differences.

**Divergence**: tsrs interns types structurally; identity == structure.
tsc allocates a type object per declaration/instantiation site and
compares identity by object reference; structure only matters inside
relations.

**Remedy design** (large, invasive — LAST RESORT ordering):
- Do NOT abandon interning; add an optional identity component:
  `TypeKind::DeferredObj(node_key)` already carries one — extend the
  same idea to `Anon` shapes born from type-literal nodes
  (`Anon(ShapeId)` → `Anon(ShapeId, Option<DeclId>)`) and thread
  DeclId through `shape_of_members`/`method_sig_type`.
- Union/intersection member dedup then keys on (kind incl. DeclId), so
  two written `{}`s stay distinct — which is the tsc behavior the
  fx2/fx4 FNs need — while REFERENCES to one written type still dedupe.
- Fallout control: TypeId-ordered union DISPLAY may shift (known
  hazard: the NonNullable display flip documented in the Tier-2
  memory). Full-corpus byte-diff BEFORE the classifier; every display
  change must be triaged as display-only vs semantic.
- Only worth it if attribution (§1.4) shows identity-model plurality;
  today's known damage is ~280 diags (2403 family + 2 FNs), and the
  2403 part has a cheaper targeted fix (identity-compare at the
  redeclaration site — architectural-debt.md §4).

### 2.4 Instantiation & recursion-identity fidelity

**Symptom signature**: "Excessive stack depth" (2589-family) FPs or
hangs on recursive generics; relation results that differ from tsc on
deeply-instantiated types; `relation_depth_overflow` firing in probes;
the `REL_STACK`/`MAX_NEST=3` heuristic in `related()` implicated.

**Divergence**: tsc bounds recursion via `isDeeplyNestedType`
(recursion identity, depth 5 per side, instantiation-count limits
100/5M) and instantiation caches keyed by mapper; tsrs approximates
with a thread-local nesting heuristic and a HashMap mapper without
instantiation caching.

**Remedy design** (medium): port `getRecursionIdentity` faithfully
(tsrs has `recursion_identity` — audit against tsc's), replace
MAX_NEST=3 with tsc's per-side depth-5 rule, add the
instantiation-depth/count guards (`instantiationDepth`,
`instantiationCount` in `instantiateType`), and cache signature/type
instantiation keyed by (target, mapper-hash). Do it when the FP mining
first attributes ≥30 diags to depth/instantiation, not before.

### 2.5 Multi-file / module-resolution infrastructure

**Symptom signature**: fixtures bailing with 6054 or being skipped
(declarationFileForTsJsImport family — 12 diags today); growth of
`@module: node18/nodenext` fixtures in FN mining; import-alias 2702
standing FP (dotted `import x = require()` use).

**Divergence**: the harness handles multi-file fixtures but not
package.json-rooted resolution, multi-value `@module` matrices, or
alias→module-exports namespace modeling.

**Remedy design** (contained infra): implement `@module` matrix
expansion in BOTH `parse_fixture` implementations (harness +
parallel_classify — the BOM lesson: same commit), minimal
package.json `main`/`types` resolution in the harness resolver, and
the alias→exports-namespace model in the binder (memory item (j)).
Yield is small today (~30 diags); schedule when the parser/relation
axes are exhausted.

### 2.6 Display/strict-mode frontier (the metric AFTER this one)

When gate-filtered exact-match crosses ~70%, the next fidelity axis is
STRICT comparison (`full_conformance_compare.py --strict`: category,
full span, message chains, relatedInformation). Known debts for that
frontier, collect them as encountered but do not fix early:
- alias-name preservation rules (`set_alias`/`set_alias_force`
  threading — partially built during the unused sweep);
- message-chain shape parity in relation failures (rel_report_error
  ordering, elaboration depth);
- span conventions (getErrorSpanForNode mirroring — done for unused,
  not audited elsewhere);
- related-information population (2728 'declared here' etc. — partial).

## 3. House style for big refactors (the pattern that worked)

Every large migration in this repo that succeeded used the same shape —
codified here as requirements for any §2 remedy:

1. **Dark-launch stage**: build the new machinery alongside the old,
   OFF by default, with a verify seam that tallies agreement
   (`TSRS_FLOW_VERIFY` precedent: match/mismatch counters over the full
   corpus, mismatches triaged into "new engine right" / "old engine
   right" / "noise" BEFORE any flip).
2. **Byte-identical gates for pure-refactor stages** (`TSRS_JOBS=1`
   fixed, full-corpus diff) — refactor commits and behavior commits
   never mix.
3. **Flip stage** behind the classifier (0 NEW_FP hard, NEW_FN
   documented-or-zero), one seam at a time when seams exist.
4. **Retirement stage**: the old path is DELETED, not left as fallback
   (the fact-stack retirement precedent) — a live fallback rots and
   double-maintains.
5. Every stage lands on main only when its OWN gates pass; long
   migrations ride a feature branch with the SERIES gate at the end
   (archived parse-error-gate precedent).
6. Determinism check (`verify.sh mf`) is mandatory for anything
   touching flow, worker transport, or output ordering.

## 4. Explicit non-goals (recorded so nobody re-litigates them cheaply)

- Emit/transform parity (we only chase getSuggestionDiagnostics-visible
  emit effects, like the U5b namespace suppression).
- Language-service features (completions, quickinfo).
- Performance beyond "full corpus in seconds" (Tier-3 skip list:
  symbol-table perf, variance cache — revisit only if corpus wall-clock
  exceeds ~60s at TSRS_JOBS=1, which would slow every gate).
- Rewriting the checker to literally transliterate checker.ts. The
  house approach stays: mirror tsc's DECISION STRUCTURE at divergence
  sites, keep tsrs's own architecture elsewhere.
