# Key checker functions: implementation-grade porting notes

Companion to greenfield.md §4–5. That doc gives the DATA MODEL; this
one is the ALGORITHMS — the load-bearing functions whose control flow
must be ported faithfully because their behavior is observable in
diagnostics (order, identity, caching, overload choice). Each section
gives: entry signature, a Rust-shaped skeleton mirroring the real
control flow, the non-obvious invariants, tsc line anchors (vendored
6.0.3 — re-grep if re-vendored), and the current-tsrs gap.

Sections: §1 relation engine, §2 inference, §3 overload resolution,
§4 control-flow analysis (narrowing + reachability), §5 porting order.
§1–3 are type-side algorithms the current tsrs approximates and would
be REBUILT; §4 is a design tsrs already validated (Tier-2 CFG) and
would be PORTED as-is — the notes there fix the exact shape and flag
the two pieces tsrs approximates.

Read tsc-source-guide.md first for how to navigate `_tsc.js`. Every
skeleton below is a PORT TARGET, not pseudocode to improvise from — when
in doubt, read the cited lines and probe. The MACHINERY these four
algorithms sit on (lazy type computation + cycle stack, the check
driver's eager/deferred ordering, contextual typing, type
construction/normalization, widening, instantiation, member access) is
in [checker-foundations.md](checker-foundations.md) — read it first if
you are building the whole checker rather than one algorithm.

Convention: `T` = TypeId, `Ternary` = { False=0, Unknown=1, Maybe=3,
True=-1 } (tsc's exact values; `& 1` tests truthiness the way tsc's
`if (result)` does — Maybe(3) and True(-1) are both truthy).

---

## 1. The relation engine

The single most important subsystem. tsc has ONE recursive engine
(`checkTypeRelatedTo`) parameterized by a `Relation` object, returning
`Ternary`, with a **maybe-stack** that defers caching of results which
depended on an in-progress recursion. The current tsrs `is_assignable_to`
is a bool engine with a coinductive `return true` shortcut and a single
cache — it CANNOT express Maybe, so it caches coinductive trues that tsc
discards (stall-playbook §2.1). This is the port that removes an entire
class of order/recursion divergence.

Call graph:
```
isTypeRelatedTo (64762)          fast paths + cache probe; bool result
  └ checkTypeRelatedTo (64842)   sets up error/maybe state, one call:
      └ isRelatedTo (65147)      normalize, simple rules, dispatch
          ├ unionOrIntersectionRelatedTo   (skip-caching small unions)
          └ recursiveTypeRelatedTo (65725)  the maybe-stack + cache
              └ structuredTypeRelatedTo (65872)  members/sigs/index/etc.
```

### 1.1 isTypeRelatedTo (entry, bool) — tsc 64762

```rust
fn is_type_related_to(&mut self, mut src: T, mut tgt: T, rel: Relation) -> bool {
    if self.is_fresh_literal(src) { src = self.regular_type(src); }
    if self.is_fresh_literal(tgt) { tgt = self.regular_type(tgt); }
    if src == tgt { return true; }
    if rel != RelationKind::Identity {
        // comparable: try REVERSED simple rules first (base ~ its literal etc.)
        if rel == RelationKind::Comparable && !self.is_never(tgt)
              && self.is_simple_type_related_to(tgt, src, rel)
           || self.is_simple_type_related_to(src, tgt, rel) { return true; }
    } else {
        // identity fast path for non-structured singletons
        if !self.any_flag(src|tgt, UNION_OR_INTERSECTION|INDEXED_ACCESS|CONDITIONAL|SUBSTITUTION) {
            if self.flags(src) != self.flags(tgt) { return false; }
            if self.has_flag(src, SINGLETON) { return true; }
        }
    }
    // object×object: consult the cache directly (no error node ⇒ silent)
    if self.has_flag(src, OBJECT) && self.has_flag(tgt, OBJECT) {
        if let Some(r) = self.rel_cache(rel).get(self.relation_key(src, tgt, IntersectionState::None, rel, false)) {
            return r.succeeded();
        }
    }
    if self.any_flag(src, STRUCTURED_OR_INSTANTIABLE) || self.any_flag(tgt, STRUCTURED_OR_INSTANTIABLE) {
        return self.check_type_related_to(src, tgt, rel, /*error_node*/ None).succeeded();
    }
    false
}
```

Invariants:
- Fresh→regular normalization at the ENTRY only (freshness is structural,
  greenfield §4.2). The `src == tgt` check after it is why interning
  matters: two distinct `{}` literals are `!=` here and fall through to
  structural comparison — exactly tsc.
- The comparable-relation reversed-simple rule appears in THREE places
  (here at 64773, and TWICE inside `isRelatedTo` — the entry check on
  the ORIGINAL types at 65150 and the post-normalization check at
  65197; none in `unionOrIntersectionRelatedTo`); the current
  tsrs has it in `related()` — keep all three when porting.

### 1.2 recursiveTypeRelatedTo — the maybe-stack — tsc 65725

This is the piece with no tsrs equivalent. Port it exactly.

```rust
fn recursive_type_related_to(&mut self, src: T, tgt: T, report: bool,
        istate: IntersectionState, rflags: RecursionFlags) -> Ternary {
    if self.rel_overflow { return Ternary::False; }
    let id = self.relation_key(src, tgt, istate, self.cur_rel, false);
    if let Some(entry) = self.rel_cache(self.cur_rel).get(id) {
        // (skip the reporting-rerun branch when report && Failed && !Overflow)
        if !(report && entry.failed() && !entry.overflow()) {
            // …variance-marker + overflow-report handling…
            return if entry.succeeded() { Ternary::True } else { Ternary::False };
        }
    }
    if self.relation_count <= 0 { self.rel_overflow = true; return Ternary::False; }

    // maybe-stack membership: a key already IN PROGRESS ⇒ Maybe (NOT cached)
    if self.maybe_set.contains(id) { return Ternary::Maybe; }
    // '*'-prefixed keys (constraint-broadened) also match a broader equivalent
    if id.starts_with('*') {
        let broad = self.relation_key(src, tgt, istate, self.cur_rel, /*ignoreConstraints*/ true);
        if self.maybe_set.contains(broad) { return Ternary::Maybe; }
    }
    if self.source_depth == 100 || self.target_depth == 100 { self.rel_overflow = true; return Ternary::False; }

    let maybe_start = self.maybe_count;
    self.maybe_keys.push(id); self.maybe_set.insert(id); self.maybe_count += 1;

    // push recursion stacks; set expanding flags via isDeeplyNestedType
    let saved_expanding = self.expanding_flags;
    if rflags.contains(SOURCE) {
        self.source_stack[self.source_depth] = src; self.source_depth += 1;
        if !self.expanding_flags.source && self.is_deeply_nested(src, &self.source_stack, self.source_depth) {
            self.expanding_flags.source = true;
        }
    }
    if rflags.contains(TARGET) { /* symmetric */ }

    let result = if self.expanding_flags == BOTH {
        Ternary::Maybe                                   // depth-limited: assume, don't recurse
    } else {
        self.structured_type_related_to(src, tgt, report, istate)
    };

    if rflags.contains(SOURCE) { self.source_depth -= 1; }
    if rflags.contains(TARGET) { self.target_depth -= 1; }
    self.expanding_flags = saved_expanding;

    match result {
        r if r != Ternary::False => {
            // cache ONLY when unwinding to the root, or definitely True
            if r == Ternary::True || (self.source_depth == 0 && self.target_depth == 0) {
                self.reset_maybe_stack(maybe_start, /*mark_succeeded*/ r == Ternary::True || r == Ternary::Maybe);
            }
        }
        _ => {
            self.rel_cache_mut(self.cur_rel).set(id, RelationResult::FAILED | self.propagating_variance);
            self.relation_count -= 1;
            self.reset_maybe_stack(maybe_start, /*mark_succeeded*/ false);
        }
    }
    result
}

fn reset_maybe_stack(&mut self, start: usize, mark_succeeded: bool) {
    for i in start..self.maybe_count {
        self.maybe_set.remove(self.maybe_keys[i]);
        if mark_succeeded {
            self.rel_cache_mut(self.cur_rel).set(self.maybe_keys[i], RelationResult::SUCCEEDED | self.propagating_variance);
            self.relation_count -= 1;
        }
    }
    self.maybe_keys.truncate(start); self.maybe_count = start;
}
```

THE critical invariants (each one is a divergence if dropped):
- **Maybe results are NEVER cached mid-recursion.** A `(src,tgt)` seen
  while already on the maybe-stack returns Maybe. Only when the whole
  chain resolves True (or unwinds to depth 0) are the accumulated maybe
  keys committed to the cache as Succeeded. tsrs's `relation_stack →
  return true` shortcut is the WRONG approximation of this — it caches
  the coinductive success permanently.
- `relation_count` starts at `16e6 - relation.size >> 3` and decrements
  on every cached result; hitting 0 is `ComplexityOverflow`. Depth 100
  is `StackDepthOverflow`. Both are real diagnostics
  (`Excessive_complexity/stack_depth_comparing_types`).
- `getRelationKey` (67423) includes ALIAS and INTERSECTION-STATE
  context, and has a `'*'` prefix form for constraint-broadened keys.
  The cache is keyed by this string, per relation. Port it before the
  engine or the cache mis-hits.

### 1.3 isDeeplyNestedType — tsc 67465

Replaces tsrs's `MAX_NEST=3` thread-local heuristic.

```rust
fn is_deeply_nested(&mut self, mut ty: T, stack: &[T], depth: usize, max: usize /*=3*/) -> bool {
    if depth < max { return false; }
    if self.object_flags(ty).contains(INSTANTIATED | MAPPED) { ty = self.mapped_target_with_symbol(ty); }
    if let TypeData::Intersection { members } = self.data(ty) {
        return members.iter().any(|&m| self.is_deeply_nested(m, stack, depth, max));
    }
    let identity = self.recursion_identity(ty);   // tsc getRecursionIdentity
    let (mut count, mut last_id) = (0, 0);
    for i in 0..depth {
        let t = stack[i];
        if self.has_matching_recursion_identity(t, identity) {
            if self.type_id(t) >= last_id { count += 1; if count >= max { return true; } }
            last_id = self.type_id(t);
        }
    }
    false
}
```

`getRecursionIdentity` groups types that share an expansion source
(same symbol / same node / same alias). Port it faithfully — the
current tsrs `recursion_identity` exists but is unaudited; §2.4 of the
stall playbook covers this.

### 1.4 structuredTypeRelatedTo (65872) — the body

Too large to inline; port section by section, each with a ledger entry:
type-parameter/index/conditional/substitution arms, then
`propertiesRelatedTo` / `signaturesRelatedTo` (already partly in tsrs —
`signature_related`) / `indexInfosRelatedTo`. Key ordering facts the
current tsrs already learned (keep them): excess-property checks BEFORE
structural (fresh object literals), common-property checks for weak
types, variance-driven type-argument comparison via `relateVariances`.

### 1.5 The five relations

```rust
enum RelationKind { Identity, Subtype, StrictSubtype, Assignable, Comparable }
```

- **Assignable / Comparable**: already exercised by tsrs.
- **Subtype / StrictSubtype**: needed for union reduction
  (`getUnionType` with `UnionReduction::Subtype`), overload ranking
  (resolveCall's first pass, §3), and literal-widening decisions
  (`getCommonSupertype`). The current tsrs fakes these with assignable;
  building them is where new conformance comes from (stall-playbook R3).
- **Identity**: `getRelationKey`-cached, no error reporting
  (assert in checkTypeRelatedTo), used by redeclaration compat (the
  2403 family — architectural-debt.md §4 wants identity compare here).

Each relation gets its OWN cache: `[RelCache; 5]` keyed by relation.
Never share (the current `comparable_cache` split is the 2-relation
special case of this).

tsc also has a sixth auxiliary map, `enumRelation`, used only by
`isEnumTypeRelatedTo` to cache enum-compatibility verdicts per
symbol pair. It is intentionally not a `RelationKind` variant; model
it separately if enum-compatibility caching ever becomes necessary.

---

## 2. Inference

Call graph:
```
inferTypeArguments (75938)   per call: contextual-return inference, then
  └ inferTypes (68637) ─ inferFromTypes (68646)   fill inference.candidates
getInferredType (69271)      resolve ONE type parameter from its candidates
  ├ getCovariantInference (69263)   candidates → widen rule → supertype
  └ getContravariantInference       contra-candidates → intersection/subtype
```

### 2.1 getCovariantInference — the widenLiteralTypes rule — tsc 69263

THIS resolves the documented FN (typeArgumentsWithStringLiteralTypes01)
and the order-dependence in stall-playbook §2.2. Port the exact
condition — do not trust prose.

```rust
fn get_covariant_inference(&mut self, inf: &InferenceInfo, sig: SignatureId) -> T {
    let candidates = self.union_object_and_array_literal_candidates(&inf.candidates);
    let primitive_constraint = self.has_primitive_constraint(inf.type_parameter)
                            || self.is_const_type_variable(inf.type_parameter);
    let widen_literals = !primitive_constraint
        && inf.top_level
        && (inf.is_fixed || !self.is_type_parameter_at_top_level_in_return_type(sig, inf.type_parameter));
    let base = if primitive_constraint {
        candidates.iter().map(|&c| self.regular_type_of_literal(c)).collect()   // keep literal, drop fresh
    } else if widen_literals {
        candidates.iter().map(|&c| self.widened_literal_type(c)).collect()      // widen to base
    } else {
        candidates
    };
    let unwidened = if inf.priority.intersects(PRIORITY_IMPLIES_COMBINATION) {
        self.get_union_type(&base, UnionReduction::Subtype)   // needs Subtype relation (§1.5)
    } else {
        self.get_common_supertype(&base)                      // needs Subtype relation
    };
    self.get_widened_type(unwidened)
}
```

- `has_primitive_constraint` (69240): constraint (or a conditional's
  default constraint) is Primitive | Index | TemplateLiteral |
  StringMapping. The StringMapping arm is why greenfield makes
  StringMapping a first-class kind (architectural-debt.md §2).
- `isTypeParameterAtTopLevelInReturnType` (68352) → `isTypeParameterAtTopLevel`:
  recursive syntactic walk of the return type (the param itself, union
  members, conditional branches, ...). Port it; it is the missing piece
  that makes widening RULE-based instead of freshness-based.
- `inf.top_level` and `inf.is_fixed` are per-inference bits set during
  `inferFromTypes` (top_level = inferred at a top-level position;
  is_fixed = pinned by a prior fixing pass). The current tsrs
  `InferenceInfo` lacks both — add them when porting inferTypes.

### 2.2 getInferredType — constraint clamp + covariant/contravariant choice — tsc 69271

```rust
fn get_inferred_type(&mut self, ctx: &mut InferenceContext, i: usize) -> T {
    if let Some(t) = ctx.inferences[i].inferred_type { return t; }
    let inf = &ctx.inferences[i];
    let (mut inferred, mut fallback) = (None, None);
    if ctx.signature.is_some() {
        let cov = if !inf.candidates.is_empty() { Some(self.get_covariant_inference(inf, ctx.signature.unwrap())) } else { None };
        let con = if !inf.contra_candidates.is_empty() { Some(self.get_contravariant_inference(inf)) } else { None };
        if cov.is_some() || con.is_some() {
            // prefer covariant when it is a subtype of every contra candidate
            // AND does not conflict with sibling inferences (the `every(...)` clause)
            let prefer_cov = cov.map_or(false, |c| con.is_none()
                || (!self.any_flag(c, NEVER|ANY)
                    && inf.contra_candidates.iter().all(|&t| self.is_type_assignable_to(c, t))
                    && ctx.inferences.iter().all(|o| /* sibling non-conflict clause, tsc 69281 */ true)));
            inferred = if prefer_cov { cov } else { con };
            fallback = if prefer_cov { con } else { cov };
        } else if ctx.flags.contains(NO_DEFAULT) {
            inferred = Some(self.silent_never);
        } else if let Some(d) = self.default_from_type_parameter(inf.type_parameter) {
            inferred = Some(self.instantiate_type(d, /*backreference+nonFixing mapper*/ ...));
        }
    } else {
        inferred = Some(self.get_type_from_inference(inf));
    }
    let mut result = inferred.unwrap_or_else(|| self.default_type_argument_type(ctx.flags.contains(ANY_DEFAULT)));
    // CONSTRAINT CLAMP (tsc 69295): if the inference violates the constraint,
    // ReturnType-priority inferences FILTER to the compatible part, others → never→fallback→constraint
    if let Some(constraint) = self.constraint_of_type_parameter(inf.type_parameter) {
        let inst = self.instantiate_type(constraint, ctx.non_fixing_mapper);
        if let Some(t) = inferred {
            let with_this = self.type_with_this_argument(inst, t);
            if !(ctx.compare_types)(t, with_this) {
                let filtered = if inf.priority == InferencePriority::ReturnType {
                    self.filter_type(t, |x| (ctx.compare_types)(x, with_this))
                } else { self.never };
                result = if !self.is_never(filtered) { filtered } else { /* fall to next */ result };
                if self.is_never(filtered) {
                    result = fallback.filter(|&f| (ctx.compare_types)(f, self.type_with_this_argument(inst, f)))
                                     .unwrap_or(inst);
                }
            }
        } else {
            result = fallback.filter(|&f| (ctx.compare_types)(f, self.type_with_this_argument(inst, f))).unwrap_or(inst);
        }
    }
    ctx.inferences[i].inferred_type = Some(result);
    result
}
```

The current tsrs relation-core-1 added a SIMPLIFIED constraint clamp
(`relations.rs`, "clamps an inference to the parameter's constraint")
that just replaces with the constraint. The full version above (filter
for ReturnType priority, fallback-then-constraint) is the real thing —
port it when doing inference fidelity (historical notes:
archive/workstreams/relation-core-2-steps.md STAGE I).

### 2.3 inferTypeArguments — contextual return inference — tsc 75938

The per-call driver. Two phases the current tsrs `infer_type_arguments`
approximates: (a) contextual-return pre-inference — TWO passes in the
source (75944-75961), not one: (a1) `inferTypes(...,
inferenceTargetType, ReturnType priority)` against the contextual
type under the outer context's NoDefault clone; (a2) a FRESH
`returnContext = createInferenceContext(...)` (75957) doing
priority-None inference under `createOuterReturnMapper(outerContext)`
— the `returnMapper` comes from cloneInferredPartOfContext of THAT
context (75960), NOT from the ReturnType-priority pass;
(b) then infer from arguments position by position
(`getTypeAtPosition` + `checkExpressionWithContextualType`), rest via
`getSpreadArgumentType` (76002). Port (a) — tsrs lacks the
contextual-return pre-inference, which matters for
`const x: Foo = genericCall(...)` inference quality.

---

## 3. Overload resolution (resolveCall)

Call graph:
```
resolveCall (76579)
  ├ reorderCandidates (75768)   dedup + order signatures
  ├ getEffectiveCallArguments (76295)
  ├ chooseOverload(candidates, SUBTYPE, …)     first pass  ← needs Subtype
  ├ chooseOverload(candidates, ASSIGNABLE, …)  second pass
  │   └ per candidate: arity → typeargs/inference → getSignatureApplicabilityError
  └ getCandidateForOverloadFailure (+ error elaboration)
```

### 3.1 resolveCall skeleton — tsc 76579

```rust
fn resolve_call(&mut self, node: NodeId, signatures: &[SignatureId],
        candidates_out: Option<&mut Vec<SignatureId>>, check_mode: CheckMode,
        chain_flags: CallChainFlags, head_message: Option<&DiagnosticMessage>) -> SignatureId {
    let report_errors = !self.inference_partially_blocked && candidates_out.is_none();
    // check explicit type arguments' source elements
    // …
    let mut candidates = /* candidates_out or new */;
    self.reorder_candidates(signatures, &mut candidates, chain_flags);
    if candidates.is_empty() { /* 2346 Call_target_does_not_contain_any_signatures */ return self.resolve_error_call(node); }

    let args = self.effective_call_arguments(node);
    let single_non_generic = candidates.len() == 1 && self.sig(candidates[0]).type_params.is_empty();
    let mut arg_check_mode = if !single_non_generic && args.iter().any(|&a| self.is_context_sensitive(a)) {
        CheckMode::SKIP_CONTEXT_SENSITIVE      // defer context-sensitive args to a 2nd pass
    } else { CheckMode::NORMAL };

    // PASS 1: subtype relation (picks the most specific overload)
    let mut result = None;
    if candidates.len() > 1 {
        result = self.choose_overload(&mut candidates, RelationKind::Subtype, single_non_generic, &mut arg_check_mode, args);
    }
    // PASS 2: assignable relation
    if result.is_none() {
        result = self.choose_overload(&mut candidates, RelationKind::Assignable, single_non_generic, &mut arg_check_mode, args);
    }
    if let Some(r) = result { return r; }
    // failure: pick a candidate for error elaboration (getCandidateForOverloadFailure)
    // and, if report_errors, emit the No_overload_matches_this_call chain
    self.candidate_for_overload_failure(node, &candidates, args, candidates_out.is_some(), check_mode)
}
```

### 3.2 chooseOverload — the inference re-run — tsc 76763

The subtle part the current tsrs does not do: arguments are first
checked with `SkipContextSensitive` and inference runs with
`SkipGenericFunctions`; if the candidate passes, inference is RE-RUN
without those flags (context-sensitive args now checked against the
resolved signature) before committing.

```rust
fn choose_overload(&mut self, candidates: &mut [SignatureId], rel: Relation,
        single_non_generic: bool, arg_check_mode: &mut CheckMode, args: &[NodeId]) -> Option<SignatureId> {
    self.candidates_for_argument_error = None; /* + arity/typearg error slots */
    if single_non_generic {
        let c = candidates[0];
        if self.has_type_args(node) || !self.has_correct_arity(node, args, c) { return None; }
        if self.signature_applicability_error(node, args, c, rel, CheckMode::NORMAL, false).is_some() {
            self.candidates_for_argument_error = Some(vec![c]); return None;
        }
        return Some(c);
    }
    for idx in 0..candidates.len() {
        let cand = candidates[idx];
        if !self.has_correct_type_arg_arity(cand) || !self.has_correct_arity(node, args, cand) { continue; }
        let mut check_candidate;
        let mut inf_ctx = None;
        if !self.sig(cand).type_params.is_empty() {
            let type_args = if self.has_type_args(node) {
                match self.check_type_arguments(cand, false) { Some(t) => t, None => { /* typearg error */ continue; } }
            } else {
                let ctx = self.create_inference_context(cand);
                let t = self.infer_type_arguments(node, cand, args, *arg_check_mode | CheckMode::SKIP_GENERIC_FUNCTIONS, &ctx);
                *arg_check_mode |= if ctx.flags.skipped_generic_function() { CheckMode::SKIP_GENERIC_FUNCTIONS } else { CheckMode::NORMAL };
                inf_ctx = Some(ctx); t
            };
            check_candidate = self.get_signature_instantiation(cand, &type_args, inf_ctx.as_ref());
            if self.non_array_rest_type(cand).is_some() && !self.has_correct_arity(node, args, check_candidate) { /* arity error */ continue; }
        } else {
            check_candidate = cand;
        }
        if self.signature_applicability_error(node, args, check_candidate, rel, *arg_check_mode, false).is_some() {
            self.candidates_for_argument_error.get_or_insert_with(Vec::new).push(check_candidate); continue;
        }
        if *arg_check_mode != CheckMode::NORMAL {
            // RE-RUN: context-sensitive args now checked against the resolved sig
            *arg_check_mode = CheckMode::NORMAL;
            if let Some(ctx) = &inf_ctx {
                let type_args = self.infer_type_arguments(node, cand, args, *arg_check_mode, ctx);
                check_candidate = self.get_signature_instantiation(cand, &type_args, Some(ctx));
                if self.non_array_rest_type(cand).is_some() && !self.has_correct_arity(node, args, check_candidate) { continue; }
            }
            if self.signature_applicability_error(node, args, check_candidate, rel, *arg_check_mode, false).is_some() {
                self.candidates_for_argument_error.get_or_insert_with(Vec::new).push(check_candidate); continue;
            }
        }
        candidates[idx] = check_candidate;   // memoize the instantiated winner
        return Some(check_candidate);
    }
    None
}
```

Invariants:
- **Two relations, in order**: subtype pass (most specific) then
  assignable pass. Requires the Subtype relation (§1.5) — until it
  exists, the subtype pass must be skipped and overload SPECIFICITY is
  wrong for ambiguous sets.
- **SkipContextSensitive / SkipGenericFunctions then re-run** — omitting
  this mis-infers context-sensitive callback arguments (the
  partiallyAnnotatedFunctionInference family lives near here).
- `getSignatureApplicabilityError` (76194): checks `this`-arg, then each
  positional arg via `checkTypeRelatedToAndOptionallyElaborate` (the
  elaboration is where argument-mismatch chains come from), then the
  rest/spread. Errors are COLLECTED (not emitted) during selection;
  emitted only by resolveCall's failure path with the
  `Overload_N_of_M` / `No_overload_matches_this_call` chain shaping.

### 3.3 Error elaboration shape (for T2/T3 parity)

resolveCall's failure branch (76631+) is where the overload error chain
is built: 1 or >3 candidates → last-overload error with
`The_last_overload_gave_the_following_error`; 2–3 candidates → the
minimum-error-count candidate's diagnostics wrapped in
`No_overload_matches_this_call`. Port this only when targeting T2+
(message text); T0 only needs the code+position, which comes from the
selected error candidate.

---

## 4. Control-flow analysis (narrowing + reachability)

Unlike §1–3, the current tsrs already has a tsc-shaped flow engine: a
bind-time FlowNode graph and a `get_flow_type_of_reference` resolver
(Tier-2, docs/determinism-design.md). So for a rebuild this is a
STRAIGHT PORT of a design tsrs validated; the notes below fix the exact
shape and flag the two pieces tsrs approximates (the incomplete-type
loop-convergence mechanism, and full `isMatchingReference`).

Two halves: **bind time** builds the graph; **check time** resolves a
reference's type by walking it backward.

Call graph (check time):
```
getFlowTypeOfReference (70394)
  └ getTypeAtFlowNode (70420)   backward walk, flags dispatch, shared cache
      ├ getTypeAtFlowAssignment (70502)   None ⇒ walk to antecedent
      ├ getTypeAtFlowCall (70566)         assertions / never-returns
      ├ getTypeAtFlowCondition (70614) ─ narrowType (71400)  the narrower
      ├ getTypeAtSwitchClause                narrow by discriminant/typeof
      ├ getTypeAtFlowBranchLabel (70653)  JOIN (union of antecedents)
      ├ getTypeAtFlowLoopLabel (70694)    JOIN + incomplete-type fixpoint
      └ getTypeAtFlowArrayMutation (70588)  evolving arrays
isReachableFlowNode (70240)   separate reachability walk (7027/exhaustiveness/7029/...)
```

### 4.1 The FlowType wrapper and the "incomplete" bit — THE key mechanism

tsc does NOT pass bare types through the flow walk. It passes `FlowType`
= either a plain type OR `{ flags: Incomplete, type }`. The `incomplete`
bit means "this type was computed while a loop back-edge was still being
resolved, so it is a lower bound, not final." This is what makes loop
narrowing terminate correctly. The current tsrs handles loops via a
"Cycle" resolution (per Tier-2 memory) — a rebuild should port the real
incomplete-type machinery instead; it is more faithful.

```rust
enum FlowType { Type(T), Incomplete(T) }   // tsc createFlowType / isIncomplete
fn type_from_flow_type(ft: FlowType) -> T { match ft { FlowType::Type(t)|FlowType::Incomplete(t) => t } }
```

### 4.2 getTypeAtFlowNode — the backward walk — tsc 70420

```rust
fn get_type_at_flow_node(&mut self, mut flow: FlowId) -> FlowType {
    // NB: in tsc 6.0.3 flowDepth is a LOCAL of getFlowTypeOfReference
    // (70397), reset per reference query — not checker-global state;
    // sharedFlowNodes/Types ARE globals trimmed via sharedFlowStart.
    if self.flow_depth == 2000 { self.flow_analysis_disabled = true; /* report + errorType */ }
    self.flow_depth += 1;
    let mut shared: Option<FlowId> = None;
    loop {
        let flags = self.flow_flags(flow);
        if flags.contains(SHARED) {
            // shared-flow cache: a node reached by multiple paths is memoized
            // within THIS getFlowTypeOfReference invocation (sharedFlowStart..Count)
            for i in self.shared_start..self.shared_count {
                if self.shared_nodes[i] == flow { self.flow_depth -= 1; return self.shared_types[i]; }
            }
            shared = Some(flow);
        }
        let ty: FlowType = if flags.contains(ASSIGNMENT) {
            match self.get_type_at_flow_assignment(flow) { Some(t) => t, None => { flow = self.antecedent(flow); continue; } }
        } else if flags.contains(CALL) {
            match self.get_type_at_flow_call(flow) { Some(t) => t, None => { flow = self.antecedent(flow); continue; } }
        } else if flags.intersects(CONDITION) {           // TrueCondition|FalseCondition
            self.get_type_at_flow_condition(flow)
        } else if flags.contains(SWITCH_CLAUSE) {
            self.get_type_at_switch_clause(flow)
        } else if flags.intersects(LABEL) {               // BranchLabel|LoopLabel
            if self.antecedents(flow).len() == 1 { flow = self.antecedents(flow)[0]; continue; }  // fast path
            if flags.contains(BRANCH_LABEL) { self.get_type_at_flow_branch_label(flow) }
            else { self.get_type_at_flow_loop_label(flow) }
        } else if flags.contains(ARRAY_MUTATION) {
            match self.get_type_at_flow_array_mutation(flow) { Some(t) => t, None => { flow = self.antecedent(flow); continue; } }
        } else if flags.contains(REDUCE_LABEL) {
            // try/finally ReduceLabel: temporarily swap the target's antecedents
            let node = self.flow_node(flow); let saved = self.label_antecedents(node.target);
            self.set_label_antecedents(node.target, node.antecedents);
            let t = self.get_type_at_flow_node(self.antecedent(flow));
            self.set_label_antecedents(node.target, saved); t
        } else if flags.contains(START) {
            // Start: if the container differs from flowContainer AND the reference
            // is a bare non-this/non-access ref, resume in the OUTER container's flow
            let container = self.flow_container_node(flow);
            if container.is_some() && container != Some(self.flow_container)
               && !self.reference_is_access_or_nonarrow_this() {
                flow = self.container_flow_node(container); continue;
            }
            FlowType::Type(self.initial_type)
        } else {
            FlowType::Type(self.convert_auto_to_any(self.declared_type))  // Unreachable terminus
        };
        if let Some(s) = shared {
            self.shared_nodes[self.shared_count] = s; self.shared_types[self.shared_count] = ty; self.shared_count += 1;
        }
        self.flow_depth -= 1;
        return ty;
    }
}
```

Invariants:
- **Assignment/Call/ArrayMutation return `Option`**: `None` means "this
  node does not affect the reference" ⇒ `continue` to the antecedent.
  This antecedent-walk-on-None is the single most common pattern; the
  current tsrs Step::Assign arm mirrors it.
- **Shared-flow cache** is per-`getFlowTypeOfReference` call (reset via
  `sharedFlowStart`), NOT global — a node reached by N paths is computed
  once per reference query. tsrs should confirm it has this (perf +
  correctness for diamond CFGs).
- **Start outer-resume**: bare `const` refs declared outside the current
  flow container resume in the outer container (funcexpr/arrow capture);
  property/element accesses and non-arrow `this` do NOT. tsrs has this
  (`FlowNode::Start{outer}` from Tier-2).
- **Depth 2000** disables flow analysis for the whole file with a flow
  error — a real, if rare, diagnostic.

### 4.3 getTypeAtFlowBranchLabel — the JOIN — tsc 70653

```rust
fn get_type_at_flow_branch_label(&mut self, flow: FlowId) -> FlowType {
    let mut antecedent_types = Vec::new();
    let (mut subtype_reduction, mut seen_incomplete) = (false, false);
    let mut bypass: Option<FlowId> = None;
    for &ante in self.antecedents(flow) {
        // an EMPTY switch clause (clauseStart==clauseEnd) is deferred as bypassFlow
        if bypass.is_none() && self.is_empty_switch_clause(ante) { bypass = Some(ante); continue; }
        let ft = self.get_type_at_flow_node(ante);
        let t = type_from_flow_type(ft);
        // short-circuit: an antecedent that is exactly the unnarrowed declared type
        if t == self.declared_type && self.declared_type == self.initial_type { return FlowType::Type(t); }
        push_if_unique(&mut antecedent_types, t);
        if !self.is_type_subset_of(t, self.initial_type) { subtype_reduction = true; }
        if let FlowType::Incomplete(_) = ft { seen_incomplete = true; }
    }
    if let Some(bp) = bypass {
        // the default/no-match path of a non-exhaustive switch rejoins here
        let ft = self.get_type_at_flow_node(bp); let t = type_from_flow_type(ft);
        if !self.is_never(t) && !antecedent_types.contains(&t) && !self.is_exhaustive_switch(bp) {
            /* same short-circuit + push + subtype/incomplete bookkeeping */
        }
    }
    FlowType::new(self.get_union_or_evolving_array_type(&antecedent_types,
                    if subtype_reduction { UnionReduction::Subtype } else { UnionReduction::Literal }),
                  seen_incomplete)
}
```

- `getUnionOrEvolvingArrayType` (70756) does the union AND
  `recombineUnknownType` (re-joins a decomposed `{}|null|undefined` back
  to `unknown` — tsrs learned this in Stage-1, keep it) and returns the
  IDENTICAL `declaredType` object when the union equals it (identity
  preservation matters for the branch short-circuit above).
- `subtypeReduction` uses the SUBTYPE relation (§1.5) — another reason
  Subtype is load-bearing.

### 4.4 getTypeAtFlowLoopLabel — the fixpoint — tsc 70694

The convergence mechanism. First antecedent (loop entry) is resolved
normally; subsequent antecedents (back-edges) are resolved with the
CURRENT partial `antecedentTypes` published on a `flowLoopNodes` stack,
so a self-reference during back-edge resolution returns the
accumulated-so-far union tagged `incomplete`. Terminates because each
pass only adds members.

```rust
fn get_type_at_flow_loop_label(&mut self, flow: FlowId) -> FlowType {
    let id = self.flow_node_id(flow);
    let key = self.get_or_set_cache_key();                // getFlowCacheKey; None ⇒ declared
    if key.is_none() { return FlowType::Type(self.declared_type); }
    if let Some(c) = self.flow_loop_caches[id].get(key) { return FlowType::Type(c); }
    // in-progress back-edge: return the partial union as INCOMPLETE
    for i in self.flow_loop_start..self.flow_loop_count {
        if self.flow_loop_nodes[i] == flow && self.flow_loop_keys[i] == key && !self.flow_loop_types[i].is_empty() {
            return FlowType::Incomplete(self.get_union_or_evolving_array_type(&self.flow_loop_types[i], UnionReduction::Literal));
        }
    }
    let mut antecedent_types = Vec::new();
    let mut subtype_reduction = false;
    let mut first: Option<FlowType> = None;
    for &ante in self.antecedents(flow) {
        let ft = if first.is_none() {
            let f = self.get_type_at_flow_node(ante); first = Some(f); f
        } else {
            // publish partial types for the back-edge walk, clear the flow-type cache
            self.flow_loop_nodes[self.flow_loop_count] = flow; self.flow_loop_keys[self.flow_loop_count] = key;
            self.flow_loop_types[self.flow_loop_count] = antecedent_types.clone(); self.flow_loop_count += 1;
            let saved = self.flow_type_cache.take();
            let f = self.get_type_at_flow_node(ante);
            self.flow_type_cache = saved; self.flow_loop_count -= 1;
            if let Some(c) = self.flow_loop_caches[id].get(key) { return FlowType::Type(c); }  // finalized during recursion
            f
        };
        let t = type_from_flow_type(ft);
        push_if_unique(&mut antecedent_types, t);
        if !self.is_type_subset_of(t, self.initial_type) { subtype_reduction = true; }
        if t == self.declared_type { break; }            // reached the widest possible; stop
    }
    let result = self.get_union_or_evolving_array_type(&antecedent_types,
                    if subtype_reduction { UnionReduction::Subtype } else { UnionReduction::Literal });
    if let Some(FlowType::Incomplete(_)) = first { return FlowType::Incomplete(result); }  // don't cache while incomplete
    self.flow_loop_caches[id].insert(key, result);
    FlowType::Type(result)
}
```

- The `flowTypeCache = void 0` swap during back-edge resolution is what
  prevents caching partial results into the per-reference flow-type
  cache. Get this wrong and loop narrowing either over- or under-narrows.
- `t == declaredType` early break: once an antecedent widens to the
  declared type, no further widening is possible.

### 4.5 getTypeAtFlowCondition → narrowType — tsc 70614 / 71400

```rust
fn get_type_at_flow_condition(&mut self, flow: FlowId) -> FlowType {
    let ft = self.get_type_at_flow_node(self.antecedent(flow));
    let t = type_from_flow_type(ft);
    if self.is_never(t) { return ft; }                    // never stays never (dead edge)
    let assume_true = self.flow_flags(flow).contains(TRUE_CONDITION);
    let narrowed = self.narrow_type(self.finalize_evolving_array(t), self.flow_node_expr(flow), assume_true);
    if narrowed == t { ft } else { FlowType::new(narrowed, ft.is_incomplete()) }
}
```

`narrowType` (71400) is the dispatch the current tsrs `narrow_by_condition`
mirrors. The port structure:

```rust
fn narrow_type(&mut self, ty: T, expr: NodeId, assume_true: bool) -> T {
    // optional-chain root / ?? / ??= left operand ⇒ narrowTypeByOptionality
    if self.is_optional_chain_root(expr) || self.is_nullish_coalesce_left(expr) {
        return self.narrow_type_by_optionality(ty, expr, assume_true);
    }
    match self.node_kind(expr) {
        Identifier => {
            // const-variable INLINING: narrow through `const c = <guard>; if (c)`
            // guarded by inlineLevel < 5 and isConstantReference(reference)
            if !self.is_matching_reference(self.reference, expr) && self.inline_level < 5 {
                if let Some(init) = self.constant_var_initializer(expr) {
                    self.inline_level += 1;
                    let r = self.narrow_type(ty, init, assume_true);
                    self.inline_level -= 1; return r;
                }
            }
            self.narrow_type_by_truthiness(ty, expr, assume_true)   // fallthrough
        }
        ThisKeyword | SuperKeyword | PropertyAccess | ElementAccess =>
            self.narrow_type_by_truthiness(ty, expr, assume_true),
        Call => self.narrow_type_by_call_expression(ty, expr, assume_true),
        Paren | NonNull | Satisfies => self.narrow_type(ty, self.inner_expr(expr), assume_true),
        Binary => self.narrow_type_by_binary_expression(ty, expr, assume_true),
        PrefixUnary if self.is_bang(expr) => self.narrow_type(ty, self.operand(expr), !assume_true),
        _ => ty,
    }
}
```

Sub-narrowers (each already partly in tsrs `narrowing.rs` — mirror the
tsc source when touching them): `narrowTypeByTruthiness` (70855;
Truthy/Falsy facts via `getAdjustedTypeWithFacts`, + discriminant-property
path), `narrowTypeByBinaryExpression` (`===`/`!==`/`==`/`!=` →
`narrowTypeByEquality` 71048, `instanceof`, `in` →
`narrowTypeByInKeyword`, assignment), `narrowTypeByTypeof` (71081),
`narrowTypeByDiscriminant` (70815 — the discriminated-union workhorse),
`narrowTypeBySwitchOnDiscriminant`. The **facts model**
(`getTypeFacts`/`getTypeWithFacts`/`getAdjustedTypeWithFacts`) is tsrs's
`type_facts`/`facts_filter` (operator sweep) — bit-compatible fact
values let these port verbatim.

### 4.6 isMatchingReference — narrowing's identity gate — tsc 69448

Everything above only narrows when the guarded expression IS the
reference being resolved. tsc's `isMatchingReference` is a structural
match (identifiers by resolved symbol; property/element access by
accessed-name + matching receiver; `this`/`super`/meta-property; comma
and assignment unwrapping). The current tsrs models references as
`RefKey(SymbolId, Vec<PropName>)` (`ref_key_of`) — a SIMPLER model that
works for the common cases but is not full `isMatchingReference` (e.g.
element access with a constant/unassigned-local index, 69476). A rebuild
should port `isMatchingReference` + `getAccessedPropertyName` directly;
retrofitting tsrs means extending `ref_key_of` case by case as mining
demands (each gap is a narrowing FN).

### 4.7 isReachableFlowNode — reachability — tsc 70240

Separate from type resolution: a boolean backward walk used for 7027
(unreachable code), switch exhaustiveness
(isExhaustiveSwitchStatement/computeExhaustiveSwitchStatement
78920/78933 — NOT "2367", which is checkBinary's unintentional-
comparison elaboration at 80408), 2534/2355/2366 return
checks, 7029. The current tsrs `reachable_walk` (Stage-3) mirrors this.

```rust
fn is_reachable_flow_node_worker(&mut self, mut flow: FlowId, mut no_cache: bool) -> bool {
    loop {
        if Some(flow) == self.last_flow_node { return self.last_flow_reachable; }
        let flags = self.flow_flags(flow);
        if flags.contains(SHARED) && !no_cache {
            let id = self.flow_node_id(flow);
            return *self.flow_node_reachable.entry(id).or_insert_with(|| self.is_reachable_worker(flow, true));
        }
        if flags.intersects(ASSIGNMENT|CONDITION|ARRAY_MUTATION) { flow = self.antecedent(flow); }
        else if flags.contains(CALL) {
            // never-returning call OR asserts-false ⇒ UNREACHABLE past here
            if let Some(sig) = self.effects_signature(flow) {
                if self.is_asserts_false(sig, flow) { return false; }
                if self.is_never(self.return_type(sig)) { return false; }
            }
            flow = self.antecedent(flow);
        }
        else if flags.contains(BRANCH_LABEL) { return self.antecedents(flow).iter().any(|&f| self.is_reachable_worker(f, false)); }  // OR
        else if flags.contains(LOOP_LABEL) { let a = self.antecedents(flow); if a.is_empty() { return false; } flow = a[0]; }        // entry edge
        else if flags.contains(SWITCH_CLAUSE) { if self.is_empty_exhaustive_clause(flow) { return false; } flow = self.antecedent(flow); }
        else if flags.contains(REDUCE_LABEL) { /* antecedent swap like §4.2 */ }
        else { return !flags.contains(UNREACHABLE); }
    }
}
```

- **`lastFlowNode` single-entry cache**: the immediately previous query
  is memoized (common because reachability is asked per statement in
  order). Plus a per-shared-node cache. Keep both.
- Never-call detection (`getEffectsSignature` + never return / asserts
  false) is the tricky gate; tsrs has the `stmt_position_calls` /
  `ret_annotation_never` machinery from Stage-3 — mirror
  `getEffectsSignature` when extending it.

### 4.8 Bind-time construction (keep tsrs's, it's correct)

The graph is built in the binder: `createFlowCondition`/`Assignment`/
`Call`/`BranchLabel`/`LoopLabel`/`ReduceLabel`, wired by `addAntecedent`
and `finishFlowLabel`. `bindCondition` (43193) splits into true/false
targets via `doWithConditionalBranches` (this is where `&&`/`||`/`??`/
optional-chain edges come from — bindCondition does NOT add plain
Cond nodes for logical/optional-chain expressions; the sub-expression
binding already created the edges). `bindWhileStatement` (43218) shows
the loop-label pattern: pre-loop LoopLabel, body BranchLabel, post
BranchLabel, back-edge added after the body. tsrs's `flow_graph.rs`
already implements this family (Tier-2, byte-identical-gated). For a
rebuild: port the `FlowFlags` bit-compatibly and the bind*Statement
family verbatim; it is the foundation everything in §4 walks.

### 4.9 getInitial/AssignedType + narrowable adjustment

`getInitialType` (69905) / `getAssignedType` feed the Assignment/Init
arms; `getNarrowableTypeForReference` (71640, cited in
checker-key-functions §3-adjacent) substitutes generic union
constraints at constraint positions. The auto/evolving-array machinery
(`convertAutoToAny`, `getEvolvingArrayType`, `finalizeEvolvingArrayType`)
handles `let x; … x` and `var a = []; a.push(…)` — tsrs has auto-CFA
(Stage-4) but NOT evolving arrays (documented gap; architectural-debt.md
§4). Port evolving arrays here if 7005-family mining demands it.

---

## 5. Porting order (respecting dependencies)

1. `getRelationKey` + `Relation` enum + per-relation caches (data).
2. `Ternary`, the maybe-stack, `recursiveTypeRelatedTo`,
   `isDeeplyNestedType`, `recursion_identity` — the engine core.
3. `structuredTypeRelatedTo` arms, one family per commit.
4. Subtype/StrictSubtype relations + `getCommonSupertype`/union subtype
   reduction (unblocks covariant inference + overload pass 1 + the
   flow JOIN's subtypeReduction).
5. `inferTypes`/`inferFromTypes` with `top_level`/`is_fixed`/priority
   bits; then `getCovariantInference` full rule +
   `isTypeParameterAtTopLevel`; then `getInferredType` full clamp.
6. `resolveCall`/`chooseOverload` two-pass + inference re-run;
   `getSignatureApplicabilityError` elaboration for T2+.
7. Flow: bind-time graph (`FlowFlags` + bind*Statement) → FlowType/
   incomplete wrapper + `getTypeAtFlowNode` walk → branch/loop JOINs →
   `narrowType` dispatch + sub-narrowers (reuse the fact model) →
   `isMatchingReference` → `isReachableFlowNode`. In a rebuild this
   depends on §1 (narrowing uses relations, JOINs use Subtype) but is
   otherwise self-contained; tsrs already validated the design, so port
   it as-is rather than re-deriving.

Each step is classifier-gated (0 NEW_FP). Steps 1–3 are a
byte-identical-then-flip migration per the house style
(stall-playbook §3): dark-launch the new engine behind a verify seam
tallying agreement vs the old bool engine before flipping read sites.
The flow engine has a proven precedent for exactly this dark-launch
(the `TSRS_FLOW_VERIFY` seam from Tier-2 — reuse the pattern).
