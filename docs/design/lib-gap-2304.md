# Design: lib-gap axis — 2304 "Cannot find name" FNs (and friends)

**Yield**: 2304 is FN #1 (1,622 raw @3242fdc). Caveat before investing:
2304/2318 are in the classifier's `LIBCODES` ignore-set
(`scripts/parallel_classify.py`: `{2318, 2304, 2583, 2584, 2792}`) and
the historical gate filters them, so part of this family is EXCLUDED
from the gate-filtered metric. Check how much of the 1,622 is
gate-visible before scheduling (the sole-blocker table from 7/03 said
lib-gap sole = 126 files). This axis buys the RAW metric and real-user
correctness more than it buys the filtered number.

## Problem shape

tsrs checks against a CURATED lib (`lib/lib.tsrs.d.ts`, single file)
while the oracle runs with the same curated lib file — so pure lib
CONTENT gaps show on both sides symmetrically. The FNs come from three
asymmetries:

1. **Names the curated lib lacks entirely** but fixtures reference and
   the ORACLE still errors on differently (positions/counts) — e.g. the
   fixture expects 2304 on 3 references, tsrs bails earlier or dedups.
2. **lib-version gating**: tsc gates lib symbols by `@target`/`@lib`
   (es5 vs es2015+ vs esnext members). tsrs's single lib file has no
   version axis: fixtures with `@target: es5` should NOT see es2015
   names (oracle emits 2304/2318/2550 there; tsrs finds the name → FN
   of the error). This is the 2550 family too (FN 115:
   "Property 'X' does not exist ... Do you need to change your target
   library?").
3. **Global-augmentation / declaration-merging cases** where tsrs's
   binder doesn't merge fixture-declared globals with lib globals the
   way tsc does.

## Design

### Stage 1 — measure the axis precisely (half a day)

Mine `/tmp/fcc_*.json` FN entries for 2304/2318/2550/2583/2584/2792 and
bucket by the NAME in the message (oracle side carries the text via
probe). Output: a ranked name list ("Symbol", "Promise", "AsyncIterable",
Intl members, es2015 collection members, etc.) and a fixture list per
name. Decide cutline by count.

### Stage 2 — lib-version gating (the structural piece)

Implement tsc's lib resolution shape, minimally:

- Split `lib.tsrs.d.ts` into layered files mirroring tsc's lib
  hierarchy: `lib.es5.tsrs.d.ts`, `lib.es2015.tsrs.d.ts`, … (only the
  layers the corpus exercises: es5, es2015, es2017, es2020, es2021,
  es2022, esnext cover ~all fixtures). The current single file's
  content is ALREADY curated; this is a partition, not a rewrite.
  Annotate each declaration's layer by consulting the REAL tsc libs
  (`oracle/node_modules/typescript/lib/lib.*.d.ts`) for where each name
  first appears.
- Loader (`src/lib.rs` harness path + `scripts/parallel_classify.py`
  `compiler_options_from_directives`): pick layers by
  `@target`/`@lib` exactly like tsc's `getDefaultLibFileName` +
  lib-inclusion chain. BOTH sides (tsrs harness AND the oracle runner)
  must switch simultaneously or the classifier double-blinds — same
  trap as the BOM fix (change both or neither; land in one commit).
- The batch runner currently appends the ONE lib file to every program
  (`classify.py write_fixture_file` + tsrs option) — thread the layer
  list through both.

### Stage 3 — content top-up

With version gating in place, add missing declarations per Stage-1
ranking, layer by layer. Keep the curated-lib philosophy: minimal
surface that the corpus exercises, verbatim signatures from the real
tsc lib files (copy, don't paraphrase — display strings must match for
strict-mode comparisons later).

## Gates

Stage 2 is the risky one: it changes EVERY fixture's program inputs.
Gate it with the full classifier (0 NEW_FP / 0 NEW_FN target) — a
correct partition is byte-identical for fixtures whose target already
saw the full lib, and only ADDS oracle-matching 2304/2318/2550s for
lower targets. If the partition disturbs unrelated diags, the layer
assignment is wrong; do not compensate elsewhere.

Also refresh `verify.sh quick`'s expectations if any pinned fixture
output changes (2318 counts appear in the mf pin line).

## Non-goals

- Full `lib.dom.d.ts`: the corpus conformance set is script-target
  focused; DOM names appearing in the FN mining probably rank low.
  Confirm via Stage 1 before excluding permanently.
- `@lib` option parsing beyond what fixtures use (check
  `compiler_options_from_directives` for what's already plumbed).
