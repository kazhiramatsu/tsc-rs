# Key checker functions: implementation-grade porting notes

Companion to greenfield.md §4–5. That doc gives the DATA MODEL; this
one is the ALGORITHMS — the load-bearing functions whose control flow
must be ported faithfully because their behavior is observable in
diagnostics (order, identity, caching, overload choice). Each section
gives: entry signature, a Rust-shaped skeleton mirroring the real
control flow, the non-obvious invariants, tsc line anchors (vendored
6.0.3 — re-grep if re-vendored), and the current-tsrs gap.

Read tsc-source-guide.md first for how to navigate `_tsc.js`. Every
skeleton below is a PORT TARGET, not pseudocode to improvise from — when
in doubt, read the cited lines and probe.

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
    if rel != Relation::Identity {
        // comparable: try REVERSED simple rules first (base ~ its literal etc.)
        if rel == Relation::Comparable && !self.is_never(tgt)
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
  (here, `isRelatedTo`, and `unionOrIntersectionRelatedTo`); the current
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
enum Relation { Identity, Subtype, StrictSubtype, Assignable, Comparable }
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

- `has_primitive_constraint` (69241): constraint (or a conditional's
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
    // CONSTRAINT CLAMP (tsc 69293): if the inference violates the constraint,
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
port it when doing inference fidelity (relation-core-2-steps STAGE I).

### 2.3 inferTypeArguments — contextual return inference — tsc 75938

The per-call driver. Two phases the current tsrs `infer_type_arguments`
approximates: (a) infer the type parameters from the CONTEXTUAL return
type first (`inferTypes(..., inferenceTargetType, ReturnType priority)`)
when the call site has a contextual type, producing a `returnMapper`;
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
    if candidates.is_empty() { /* 2657 no signatures */ return self.resolve_error_call(node); }

    let args = self.effective_call_arguments(node);
    let single_non_generic = candidates.len() == 1 && self.sig(candidates[0]).type_params.is_empty();
    let mut arg_check_mode = if !single_non_generic && args.iter().any(|&a| self.is_context_sensitive(a)) {
        CheckMode::SKIP_CONTEXT_SENSITIVE      // defer context-sensitive args to a 2nd pass
    } else { CheckMode::NORMAL };

    // PASS 1: subtype relation (picks the most specific overload)
    let mut result = None;
    if candidates.len() > 1 {
        result = self.choose_overload(&mut candidates, Relation::Subtype, single_non_generic, &mut arg_check_mode, args);
    }
    // PASS 2: assignable relation
    if result.is_none() {
        result = self.choose_overload(&mut candidates, Relation::Assignable, single_non_generic, &mut arg_check_mode, args);
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

## 4. Porting order (respecting dependencies)

1. `getRelationKey` + `Relation` enum + per-relation caches (data).
2. `Ternary`, the maybe-stack, `recursiveTypeRelatedTo`,
   `isDeeplyNestedType`, `recursion_identity` — the engine core.
3. `structuredTypeRelatedTo` arms, one family per commit.
4. Subtype/StrictSubtype relations + `getCommonSupertype`/union subtype
   reduction (unblocks covariant inference + overload pass 1).
5. `inferTypes`/`inferFromTypes` with `top_level`/`is_fixed`/priority
   bits; then `getCovariantInference` full rule +
   `isTypeParameterAtTopLevel`; then `getInferredType` full clamp.
6. `resolveCall`/`chooseOverload` two-pass + inference re-run;
   `getSignatureApplicabilityError` elaboration for T2+.

Each step is classifier-gated (0 NEW_FP). Steps 1–3 are a
byte-identical-then-flip migration per the house style
(stall-playbook §3): dark-launch the new engine behind a verify seam
tallying agreement vs the old bool engine before flipping read sites.
