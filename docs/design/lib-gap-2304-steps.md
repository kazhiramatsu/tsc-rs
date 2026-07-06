# Step-by-step: lib-gap 2304 (companion to lib-gap-2304.md)

Follow docs/design/EXECUTION-GUIDE.md. Stage 1 and Stage 3 are safe for
low-capability agents; Stage 2 (lib layering) changes every fixture's
input and should be reviewed by a stronger model before landing.

## STAGE 1 — mining report [M]

### Step 1.1 — is this axis worth it right now?

The classifier IGNORES codes {2318, 2304, 2583, 2584, 2792} (see
`LIBCODES` in scripts/parallel_classify.py) and the gate-filtered
metric partially excludes them. Compute the gate-VISIBLE damage first:

```python
python3 - <<'EOF'
import json
from collections import Counter
d = json.load(open('/tmp/fcc_rc1.json'))  # regenerate if stale (README)
LIB = {2304, 2318, 2550, 2583, 2584, 2792}
raw = Counter(); filt = Counter()
for m in d['mismatches']:
    for t in m['raw_fn']:
        if t[1] in LIB: raw[t[1]] += 1
    for t in m['gate_filtered_fn']:
        if t[1] in LIB: filt[t[1]] += 1
print('raw   ', dict(raw))
print('gated ', dict(filt))
EOF
```

If `gated` totals < ~300, record the numbers in a NOTES file and ask
the operator whether to proceed — the effort may be better spent on
relation-core-2.

### Step 1.2 — ranked missing-name list [M]

For every fixture with 2304/2318/2550 FNs, probe and extract the NAME
from the oracle-side message ("Cannot find name 'X'" / "Property 'X'
does not exist"). Emit a table `name → count → example fixture` into
`docs/design/NOTES-<date>-libgap.md`, sorted by count. Suggested
automation: probe writes to stdout; a regex over the oracle section
lines `TS2304 .* 'name '([A-Za-z0-9_$]+)'` collects names.

## STAGE 2 — lib layering [T — REVIEW GATE before landing]

Read lib-gap-2304.md §Stage 2 for the design. Execution constraints
for a weak model:

1. Partition `lib/lib.tsrs.d.ts` into `lib/lib.es5.tsrs.d.ts` +
   `lib/lib.es2015.tsrs.d.ts` + … WITHOUT editing any declaration
   text. To decide a declaration's layer: grep the REAL tsc lib
   directory for the symbol:
   `grep -l "interface PromiseConstructor" oracle/node_modules/typescript/lib/lib.es*.d.ts`
   → the LOWEST lib file that declares it is its layer. Members added
   in later versions (e.g. `Array.prototype.includes` in es2016) must
   move to an interface-merging augmentation in the later layer, the
   way the real libs do it (`interface Array<T> { includes(...): boolean; }`
   in `lib.es2016.array.include.d.ts`). Copy the real libs' split
   points exactly.
2. Loading: two call sites must agree IN THE SAME COMMIT:
   - tsrs harness: where the batch runner injects the lib file into the
     program (grep `lib.tsrs.d.ts` in src/ and src/harness/).
  - oracle runner: `scripts/parallel_classify.py` /
     `full_conformance_compare.py` pass the lib text through the
     in-memory oracle payload. Both take a `--lib` path; extend both to
     accept the layered set and select by the
     fixture's `@target` (mapping: es3/es5→[es5], es6/es2015→+es2015,
     … esnext→all). The target string is already parsed —
     grep `script_target_rank` (tsrs) and `target` handling in
     parallel_classify's `compiler_options_from_directives`.
3. Gate expectation: fixtures whose `@target` ≥ the highest layer are
   byte-identical; lower targets ADD oracle-matching 2304/2318/2550s.
   Any change in an UNRELATED code (not in the LIB set) = the partition
   moved a declaration wrongly; find it by probing the changed fixture
   and grepping which layer its missing/extra name landed in.
4. STOP-POINT: before merging to main, produce the classifier summary
   + a 10-fixture spot-check table in NOTES and request review.

## STAGE 3 — content top-up [M]

With layering landed (or, if Stage 2 was deferred, into the single
lib file): for each name in the Stage-1 table, copy the declaration
VERBATIM from the real tsc lib (`oracle/node_modules/typescript/lib/`)
into the matching layer file. Batch ~10 names per commit, full gate per
batch (0 NEW_FP / 0 NEW_FN — additions can only remove FNs; a NEW_FP
means the added declaration interacts with an approximation elsewhere:
drop that name from the batch, record it in NOTES with the probe, and
continue with the rest).

Priority from the last mining (2026-07-06, verify against your own
Stage-1 table): Symbol.* well-known members, Promise/PromiseLike
variants (2318 "Global type" family), Iterable/AsyncIterable arity
(the lib pin `TS2317 Global type 'Iterable' must have 3 type
parameter(s)` appears in probes constantly — fixing the Iterable
family also cleans probe noise), Intl.*, es2015 collection members.
