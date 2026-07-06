# Design: architectural-debt items (implement only when a workstream needs them)

Each item here blocks a small number of diagnostics TODAY but sits on an
architectural divergence from tsc. They are documented so an
implementer hits them prepared, not so they get scheduled eagerly —
none of them clears more than ~50 diags on its own right now.

## 1. Anonymous type-literal DECLARATION identity

**Blocks**: unknownControlFlow fx2/fx4 (2367×2, documented FN);
future getIntersectionType-parity work.

tsc gives every anonymous type-literal NODE its own type identity: two
structurally identical `{} | null` unions written in two places are
DIFFERENT type objects, so `getIntersectionType`'s membership map does
NOT dedupe them, which changes normalization outcomes (the fx2/fx4
truth table in the operator-sweep session). tsrs interns types
structurally (`TypeTable.intern_kind`) — structural equality IS
identity, so this distinction is inexpressible.

Faithful fix (big): give `DeferredObj`/`Anon` types a declaration-id
component in their interning key (node_key of the type literal), so
structural interning still dedupes REFERENCES to the same literal but
not distinct literals. Consequences to audit before attempting:
- every `t1 == t2` fast path in relations/narrowing now misses
  structurally-equal-but-distinct pairs (must fall through to
  structural comparison — correctness holds, perf and DISPLAY dedup
  may shift);
- union/intersection sorting is TypeId-ordered → display order of
  structurally-equal members may shift (byte-diff hazard across the
  whole corpus).

Recommendation: leave as documented FN until a workstream shows ≥50
diags blocked on it. If attempted: byte-identical harness run FIRST
(`TSRS_JOBS=1` diff) to enumerate display fallout before the classifier.

## 2. StringMapping type kind (`Uppercase<string>` et al.)

**Blocks**: stringMappingOverPatternLiterals standing FNs (~15: `string`
not assignable to `Uppercase<string>`, mapping-over-mapping identities);
2322 remainder items involving `Uppercase<T>` with T generic.

tsrs models `Uppercase<X>`:
- X literal/union-of-literals → mapped literals (exact);
- X template pattern → cased pattern (exact, landed @3242fdc);
- X = `string` or type param → **collapses to `string`** (lossy — the
  divergence).

Design: add `TypeKind::StringMapping(u8 /*case*/, TypeId /*inner*/)`
(case: 0=Upper,1=Lower,2=Cap,3=Uncap — same encoding as
`intrinsic_string_kind` in src/checker/aliases.rs).
- Construction: in `apply_intrinsic_string`, the `String | TypeParam`
  arm returns the wrapper instead of `string`.
- Relations (mirror tsc `_tsc.js` — grep `StringMapping` there;
  `isRelatedTo` arms around templateLiteralTypesDefinitelyUnrelated):
  - `StringMapping(k, i)` → `string`: true (it's stringlike);
  - `StrLit(s)` → `StringMapping(k, string)`: true iff
    `apply_case(k, s) == s` (tsc isMemberOfStringMapping);
  - `StringMapping(k, i1)` → `StringMapping(k, i2)`: relate inners;
  - `string` → `StringMapping(...)`: FALSE (the missing FN);
  - nesting rules: `Uppercase<Uppercase<T>>` normalizes (tsc collapses
    idempotent/overriding applications — pin exact pairs with oracle
    probes: Upper∘Lower, Cap∘Upper, etc. before implementing).
- Instantiation: `instantiate_type` maps inner; if inner instantiates
  to a literal/pattern, EVALUATE via `apply_intrinsic_string`.
- Display: `Uppercase<${inner}>` — needed for diagnostic-text parity.
- typeof/facts/template-literal-type interactions: StringMapping is
  StringLike everywhere (`type_facts`, `typeof_filter` "string" arm,
  template hole basing `base_type_for_comparison` → string).

Bounded, mechanical; ~1 day including gates. Worth doing right before
or during the 2322-remainder mining if those probes surface it again.

## 3. Inference literal-widening order (`getInferredType` fidelity)

**Blocks**: typeArgumentsWithStringLiteralTypes01 documented FN (2345);
observed check-order sensitivity (truncating a fixture changed which
lines diverged).

tsrs's `get_inferred_type`/`get_covariant_inference`
(src/checker/infer.rs) approximates tsc's candidate widening. tsc:

```
widenLiteralTypes = !hasPrimitiveConstraint(tp)
    && (isTypeParameterAtTopLevel(returnType, tp) ? some-rule : ...)
baseCandidates = widenLiteralTypes ? sameMap(candidates, getWidenedLiteralType) : candidates
```

(grep `_tsc.js` for `function getInferredType` and read the real
condition — the sketch above is from session memory, NOT verified.)

The check-order sensitivity means tsrs's widening depends on FRESHNESS
that decays through caches (`sig_ret_cache`, alias caches) — i.e. the
result depends on which call resolved a lazy return type first. The
tsc-true fix removes freshness from the equation: widening decided by
the RULE (constraint + top-level position), not by whether the
candidate happens to still be fresh.

Implementation: port `getInferredType`'s widening condition; add
`is_type_parameter_at_top_level(ret_ty, tp)` (syntactic walk over the
signature's return type: T itself, union/intersection members,
conditional branches — see tsc `isTypeParameterAtTopLevel`). Then
remove the freshness-dependent behavior from candidate recording (the
`infer_widen_objlit` cflag stays — that's a different rule about
object-literal arguments).

Gate normally; probe pins: typeArgumentsWithStringLiteralTypes01 (the
FN should flip to matching), plus the reduce-pattern note in
infer.rs's covariant-inference comment (guard against re-breaking it).

## 4. Smaller mapped items (one-liners to half-days)

- **2403 mapped-type identity in redeclaration compare (FP 276, 36
  files pinned as mappedTypeModifiers)**: `var` redeclaration
  compatibility compares DeferredMapped types by id; distinct
  instantiation ids of the same written type mismatch → 2403 FP. Fix:
  redeclaration compare should use is_identical/structural compare, not
  TypeId equality. Anchor: the 2403 rederive path in checker (grep
  `reported_2403` / `Subsequent_variable_declarations`).
- **`??=` nullish flow edges**: `FlowNode::Nullish` exists for `??`;
  `??=` still narrows linearly (memory: Stage-1 deferred). Mirror the
  `a ??= b` edges like `a ?? (a = b)`.
- **try/finally ReduceLabel**: tsrs joins finally with raw exception
  paths (wider than tsc) — 2564/2454 FP-side risk, corpus-invisible
  today, PINNED in test strict_property_initialization_flow_2564_2565.
- **Evolving arrays (autoArrayType)**: `var r = []; r.push(x)` element
  accumulation — zero corpus signal at current seams; revisit only if
  7005-family FNs appear in mining.
- **7043 infer-from-usage suggestions**: unimplemented family, frequent
  partner of 6133 in shadowing fixtures; standing FNs. Needs its own
  mining pass to size.
- **`debugger` as reserved word + real lookahead recovery**: currently
  two suppressions in `reportable_namespace_name_span`; the real fix is
  scanner + `nextTokenIsIdentifierOrStringLiteralOnSameLine` mirroring,
  coupled to the parse-error-gate work (same recovery-profile risk).
  Fold into parse-error-gate.md scope if that lands first.
