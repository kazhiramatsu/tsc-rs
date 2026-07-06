# Design: relation-core 2 — 2339 mining, 2322/2345 remainder, assignable-side private nominality

**Yield**: FP codes #1–#3 (2339 = 529, 2322 = 483, 2345 = 357 as of
@3242fdc). Unlike the operator cluster these are DIFFUSE — no single
root cause is pre-identified for 2339. This doc therefore specifies (a)
the mining procedure, (b) the two already-mapped sub-designs that are
ready to implement (private nominality; known 2322 residue), (c) traps.

## A. Mining procedure for 2339 (do this first; ~half a day)

2339 = "Property 'X' does not exist on type 'Y'". FP means tsrs fails a
property lookup that tsc resolves. Expected cluster families (verify,
don't assume): apparent-type gaps (primitives/enums missing lib
members), union/intersection member resolution, `this`-type members,
mapped/conditional types that tsrs defers to error, namespace/dotted
access (the KNOWN dotted-namespace flattening bug — see below).

1. Regenerate the absolute snapshot if `/tmp/fcc_*.json` is stale.
2. Extract per-file 2339 FPs and bucket by the DISPLAYED receiver type
   shape:

```python
import json, os, re
from collections import Counter
d = json.load(open('/tmp/fcc_rc1.json'))
files = Counter(); 
for m in d['mismatches']:
    n = sum(1 for t in m['gate_filtered_fp'] if t[1] == 2339)
    if n: files[os.path.basename(m['path'])] += n
for f, c in files.most_common(30): print(f'{c:4d}  {f}')
```

3. `scripts/probe.py` the top ~15 files; group by receiver-type KIND
   (read the full message text: "does not exist on type '<recv>'").
   Clusters ≥20 FPs each get their own gated fix-commit; mirror the tsc
   member-resolution path involved (`getPropertyOfType`,
   `getApparentType`, `getReducedApparentType` — grep `_tsc.js`).
4. KNOWN cluster you will hit: **dotted-namespace flattening** — the
   parser keeps ONE `NamespaceDecl` for `namespace a.b.c` (name = first
   part, span = whole path); no nested symbols, no implicit export.
   Fix = parser desugars to nested NamespaceDecls + binder
   implicit-export of the inner names. Sizeable; gate separately.
   Pinned fixtures: asiPreventsParsingAsNamespace05 (2339),
   chrome.debugger-style dotted names (2708), shadowedInternalModule
   phantoms. (From the conformance-sweep memory, item (h).)

## B. 2322/2345 remainder — mapped items ready to implement

### B1. Assignable-side private/protected nominality

Comparable-mode nominality landed in operator sweep 1
(`src/checker/relations.rs`, per-prop gate under
`self.rel.erase_generic_sigs` — search "propertyRelatedTo accessibility
gate"). Enabling it for the ASSIGNABLE relation naively cost 14 corpus
FPs (heritage/override paths); those FPs define the work:

1. **getTargetSymbol semantics.** tsc compares
   `getTargetSymbol(sourceProp) !== getTargetSymbol(targetProp)` — the
   UNINSTANTIATED origin symbol. tsrs `PropInfo.symbol` is already the
   member symbol and instantiation preserves it — verify with a generic
   class fixture (`class A<T> { private x: T }`, `A<string>` vs
   `A<number>` must relate).
2. **Inherited members must keep the base's symbol.** The 2430
   FP fixtures (interfaceExtendingClassWithPrivates2,
   mergedInterfacesWithInheritedPrivates3) failed because an interface
   extending a class re-declares inherited private props with a
   DIFFERENT (or None) `PropInfo.symbol` in `build_iface_shape`'s
   inherited-members path (`src/checker/shapes.rs`). Fix shape
   inheritance to carry the ORIGINAL member symbol through; that alone
   should clear the 2430s.
3. **Protected override tolerance.** tsc propertyRelatedTo (grep
   `_tsc.js` for "Property_0_is_protected_in_type_1") — for
   protected-protected with different target symbols, relation holds
   iff one declaring class derives from the other
   (`class_extends_class` already exists in relations.rs). This is
   already what the comparable-mode gate does; port as-is.
4. **Accessibility mismatch messages**: 2325
   (`Property_0_is_private_in_type_1_but_not_in_type_2`), 2442
   (`Types_have_separate_declarations_of_a_private_property_0`), both
   already wired in the comparable-mode gate — reuse.
5. Flip order: fix (2) FIRST with the gate still comparable-only, then
   flip the gate to both relations and run the classifier; the
   remaining NEW_FPs (mixinAccessModifiers: intersection-typed mixins,
   2341/2445; derivedClassOverridesProtectedMembers4;
   classConstructorParametersAccessibility3;
   implementingAnInterfaceExtendingClassWithProtecteds;
   autoAccessorAllowedModifiers 2417) each get probed and root-fixed.
   Mixins note: tsc merges accessibility across intersection
   constituents (getIntersectionType of class instances); expect to
   need "most-restrictive wins" in the intersection prop merge.

### B2. Known 2322 residue (from relation-core 1 probes)

- `functionLiterals`-style method-vs-function-literal bivariance is
  DONE (`Signature.from_method`). What remains in the
  assignmentCompat* families after @3242fdc is mostly:
  - **Overload-set → overload-set matching order**: tsc matches EVERY
    target signature against SOME source signature with erase=true;
    tsrs now does the same, but tsc ALSO sorts/instantiates via
    `getCanonicalSignature` for the 1-vs-1 generic case — if probes
    show order-dependent mismatches, that's the lever.
  - **Subtype vs assignable**: tsrs has one relation. tsc's SUBTYPE
    relation (strictSubtypeRelation) matters for overload resolution
    ranking and union reduction, not directly for 2322; do NOT build a
    subtype relation just for these — probe first.

### B3. 2345 remainder

2345 is mostly call-site inference quality. After the position-based
param pairing fix (@3242fdc), re-mine: the known leftovers are
`getInferredType` fidelity items:

- `widenLiteralTypes` rule: tsc widens covariant literal candidates
  unless the param has a primitive constraint or appears at top level
  of the return type (`isTypeParameterAtTopLevel`). tsrs approximates;
  the typeArgumentsWithStringLiteralTypes01 documented FN sits here.
  Mirror `getInferredType`/`getCovariantInference` fully
  (`src/checker/infer.rs`, `get_inferred_type` /
  `get_covariant_inference`) and the documented FN should fall out.
- Priority bits: tsrs has 5 of tsc's ~14 InferencePriority levels
  (`infer_prio` mod). Add levels only when a probe demands one.

## C. Traps

- The comparable relation (erase_generic_sigs) shares
  `signature_related` with the assignable relation via the
  `force_erase` parameter — when touching variance there, re-run BOTH
  the comparisonOperator* families and assignmentCompat* families.
- `is_assignable_to_bivariant_sigs` (single-sig method fast path) has
  its own duplicated param/rest logic; keep it in sync or fold it into
  `signature_related(force_bivariant)` — folding is the better
  refactor if you touch it at all.
- Any new relation behavior keyed on a MODE flag needs its own result
  cache (see `rel.comparable_cache` precedent) — the caches are keyed
  `(src,tgt)` only.
- After changing anything in inference, probe
  partiallyAnnotatedFunctionInferenceWithTypeParameter.ts and
  genericCallWithGenericSignatureArguments3.ts — both are sensitive
  pins that caught regressions this sweep.
