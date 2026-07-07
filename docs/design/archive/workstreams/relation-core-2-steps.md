# Step-by-step: relation-core 2 (companion to relation-core-2.md)

Follow docs/design/EXECUTION-GUIDE.md's loop. These stages are
independent — each can be its own session/agent. Ordered by
weak-model-friendliness (easiest first).

---

## STAGE N — assignable-side private nominality [M→T]

Current state: the nominal gate exists and WORKS but only in comparable
mode. Anchor: `src/checker/relations.rs`, search for the string
`propertyRelatedTo accessibility gate`. The gate reads:

```rust
let (s_np, t_np) = if self.rel.erase_generic_sigs {
    (self.prop_nonpublic(sp), self.prop_nonpublic(tp))
} else {
    ((false, false), (false, false))
};
```

The goal is to delete the `else` arm (gate always on). Enabling it
naively produced EXACTLY these 14 corpus FPs (2026-07-06 measurement)
— they are the work list, one micro-fixture each:

```
autoAccessorAllowedModifiers.ts            1  2417
classConstructorParametersAccessibility3   1  2415
derivedClassOverridesProtectedMembers4     1  2415
derivedClassTransitivity4                  1  2415
implementingAnInterfaceExtendingClassWithProtecteds 2  2415+2420
interfaceExtendingClassWithPrivates2       1  2430
interfaceExtendingClassWithProtecteds2     1  2430
mergedInterfacesWithInheritedPrivates3     1  2430
mixinAccessModifiers                       5  2341x2+2445x3
```

### Step N.1 — reproduce the baseline

Flip the gate on (delete the else), build, run
`./verify.sh golden-check`, CONFIRM the FP list matches the table
(files may have shifted since @3242fdc — an entry disappearing is fine;
a NEW entry means the codebase moved: re-derive its micro before
continuing). Then `git stash` the flip — the sub-fixes below land
FIRST, each gated green on its own, and the flip lands LAST.

### Step N.2 — inherited members must carry the ORIGIN symbol

Hypothesis for the 2430 trio (interface extends class): somewhere in
the interface-inherits-class path the `PropInfo.symbol` of an inherited
private member is lost or replaced. KNOWN FACT (verified 2026-07-06):
`merge_base_into_shape` (src/checker/shapes.rs:902) copies base
PropInfo verbatim INCLUDING `symbol` — so the loss, if any, is in a
DIFFERENT path (candidates: `base_instance_type` +
`instantiate_type` of the base — check whether instantiation of an
Iface into Ref/MappedIface rebuilds props without symbols; or the
heritage-clause check `class_extends_class`-adjacent code that compares
DERIVED against BASE uses fresh shapes).

Procedure (do not skip): micro-fixture

```
// @target: es2015
class Foo { private x: string; constructor() { this.x = ''; } }
interface I extends Foo {}
declare var i: I; declare var f: Foo;
f = i;  // tsc: OK (same declaration)
i = f;  // tsc: OK
```

With the gate flipped on (temporarily), run the probe; if either line
errors 2322/2442, add debug prints in the per-prop gate printing
`sp.symbol`/`tp.symbol` for name=="x", find which side is None or
different, then fix THAT construction site to preserve the symbol.
Remove prints. Gate this fix alone (with the nominal gate still
comparable-only): `./verify.sh golden-check` must be 0/0 —
symbol-preservation alone must not change any diagnostic.

### Step N.3 — the heritage-check message paths (2415/2430)

2415/2430 are "class incorrectly extends"/"interface incorrectly
extends" — emitted by heritage checks that call the relation with a
reporting ctx. tsc treats private-member mismatch in heritage specially
(the derived DECLARES its own private with the same name → 2415 with
"types have separate declarations..." chain; but an OVERRIDE of a
protected member in a legit derived class is FINE). For each fixture in
the N.1 list: probe, extract the oracle's exact top-level code and
chain, and adjust ONLY:

- `prop_nonpublic` equality logic (private-vs-private same-name in a
  DERIVED-vs-BASE heritage check: tsc compares getTargetSymbol — if
  tsrs symbols differ but one declaring class extends the other AND
  the member is PRIVATE, tsc still ERRORS (privates never override) —
  so the expected fix direction is in the ctx MESSAGE, not the verdict;
  verify per-fixture with probes), and
- the mixin path (2341/2445): mixinAccessModifiers uses INTERSECTION
  types of class instances. Find where intersection prop merge builds
  the combined PropInfo (`intersect_all` / apparent-member combine in
  shapes.rs) and make the merged prop carry: symbol = the FIRST
  nonpublic constituent's symbol, accessibility = most restrictive.
  Probe `type M = A & B` where A has `protected p` and B has
  `public p` — pin tsc's verdict for `m.p` access and `M`-vs-`A`
  relations before coding.

Each sub-fix: own commit, own 0/0 gate (nominal gate still off).

### Step N.4 — flip and land

Un-stash the flip (or re-apply: delete the else arm), full gate.
Expected: 0 NEW_FP (every mode from N.1 addressed), plus OK_ADDs
(2442/2325/2415 the oracle also emits) and possibly OK_RMs. Land with
`cargo fmt`, tests, golden-save.

STOP-POINT: if after N.2+N.3 the flip still shows FPs in mixin
fixtures, stop with notes — intersection accessibility may need the
stronger-model treatment.

---

## STAGE M — 2339 mining [T, needs judgment — weaker models: steps M.1–M.2 only]

### Step M.1 — produce the ranked cluster report (mechanical)

```python
python3 - <<'EOF'
import json, os
from collections import Counter, defaultdict
d = json.load(open('/tmp/fcc_rc1.json'))   # regenerate per README if stale
files = Counter(); positions = defaultdict(list)
for m in d['mismatches']:
    fps = [t for t in m['gate_filtered_fp'] if t[1] == 2339]
    if fps:
        b = os.path.basename(m['path'])
        files[b] += len(fps); positions[b] = fps
print(sum(files.values()), 'total 2339 FPs in', len(files), 'files')
for f, c in files.most_common(25): print(f'{c:4d}  {f}')
EOF
```

### Step M.2 — per-file receiver-kind classification (mechanical)

For the top 25 files: probe each, and for every one-sided 2339 line on
the tsrs side, record the receiver type name from the message text
("does not exist on type '<RECV>'"). Bucket RECV into: primitive /
enum / union / intersection / type-param / mapped-conditional (name
contains `<` or `keyof`) / namespace-dotted / class-instance / other.
Write the table into `docs/design/NOTES-<date>-2339.md`. STOP here if
you are a low-capability agent — the bucket→fix mapping needs a
stronger model, EXCEPT:

### Step M.3 — the pre-mapped dotted-namespace cluster [T]

If M.2 shows a `namespace-dotted` bucket (expected): the design is in
relation-core-2.md §A.4 (parser desugars `namespace a.b.c` into nested
NamespaceDecls; binder implicitly exports inner names). Anchors:
`src/parser/stmt.rs` namespace parsing (grep `NamespaceDecl`),
`src/binder.rs` namespace binding (grep `Namespace`). Known couplings
from the unused sweep: `reportable_namespace_name_span` suppressions
(`debugger`-named and cross-line namespaces) and unused-namespace 6133
reporting at "the first dotted part" — both have pinned behaviors in
the U5b commit; run the unused-fixture families
(`asiPreventsParsingAsNamespace*`, `shadowedInternalModule*`) through
probe before AND after.

---

## STAGE I — getInferredType widening fidelity [P]

Fix target: the documented FN (typeArgumentsWithStringLiteralTypes01)
+ check-order sensitivity. Anchors: `src/checker/infer.rs`
`get_inferred_type` / `get_covariant_inference` /
`has_primitive_constraint`.

1. Read tsc's `getInferredType` + `getCovariantInference`
   (grep `_tsc.js` for `function getCovariantInference`) and extract
   the EXACT `widenLiteralTypes` condition (three clauses in tsc 6.x;
   do not trust any summary, including this doc's).
2. Port `is_type_parameter_at_top_level` (tsc
   `isTypeParameterAtTopLevel`): recursive over return type: the param
   itself / union member / intersection member / conditional true-false
   branches at top level.
3. Replace the freshness-based widening in `get_covariant_inference`
   with the rule. Search for uses of `is_fresh` / `widen_literal`
   inside infer.rs candidate handling — every such use must be
   justified by a line in tsc's inferTypes/getInferredType or removed.
4. Pins to probe after: typeArgumentsWithStringLiteralTypes01 (FN
   should flip to match), partiallyAnnotatedFunctionInferenceWithTypeParameter,
   genericCallWithGenericSignatureArguments3, and the reduce-pattern
   comment above `infer_from_shapes`'s covariant note (fixture:
   `reduce(arr, (a, b) => a + b, 0)`-shaped code — grep the corpus for
   `reduce(` users if unsure).
5. Full gate; 0/0 target.
