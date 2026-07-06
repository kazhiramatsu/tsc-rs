//! Type-argument inference: contextual inference of generic type parameters,
//! covariant/contravariant candidate collection, common-supertype/union
//! synthesis, and shape-directed inference. Split out of `exprs.rs`.

use crate::ast::*;
use crate::binder::SymbolId;
use crate::checker::symbols::Mapper;
use crate::checker::Checker;
use crate::types::{PropInfo, Shape, TypeId, TypeKind};
use std::collections::HashMap;

/// Inference priorities, modeled on tsc's `InferencePriority` bit flags. A
/// LOWER numeric value means HIGHER precedence: when an inference at a lower
/// priority is recorded for a type parameter, it discards any candidates that
/// were recorded at a higher (worse) priority; at equal priority, candidates
/// accumulate. This replaces the earlier two-tier `candidates`/`low_candidates`
/// split with the full priority lattice tsc uses.
pub mod infer_prio {
    /// the default, highest-precedence priority for a direct inference.
    pub const NONE: u32 = 0;
    /// a naked type variable appearing as a member of a union/intersection
    /// target — the wrapped/structural members take precedence, so
    /// `PromiseLike<T>` is preferred over a bare `T` in `T | PromiseLike<T>`.
    pub const NAKED_TYPE_VARIABLE: u32 = 1 << 0;
    /// inference from an array/object literal element. A concrete annotation
    /// (e.g. an explicitly typed callback parameter) outweighs it.
    pub const LITERAL: u32 = 1 << 2;
    /// inference made while matching a signature's return type against the
    /// contextual type. Under this priority candidates are *combined by union*
    /// rather than reduced to a common supertype.
    pub const RETURN_TYPE: u32 = 1 << 7;
    /// a reverse (homomorphic) mapped-type inference (`U` from `PropDescMap<U>`).
    /// Lower precedence than a direct inference, so when the same parameter is
    /// also inferred directly from another argument (`foo<T>(o: T, p: Partial<T>)`
    /// infers `T` from `o`, not the partial `p`), the direct inference wins.
    pub const HOMOMORPHIC_MAPPED: u32 = 1 << 3;
    /// the set of priorities under which candidates are combined by union
    /// (tsc's `PriorityImpliesCombination`).
    pub const PRIORITY_IMPLIES_COMBINATION: u32 = RETURN_TYPE;
}

/// Per-type-parameter inference state, mirroring tsc's `InferenceInfo`.
/// Covariant inferences (ordinary positions, property and return types) land in
/// `candidates`; contravariant inferences (function parameter positions) land in
/// `contra_candidates`. `priority` is the best (lowest) priority recorded so
/// far; candidates recorded at a worse priority are discarded.
#[derive(Clone)]
pub struct InferenceInfo {
    pub candidates: Vec<TypeId>,
    pub contra_candidates: Vec<TypeId>,
    pub priority: u32,
    pub has_priority: bool,
}

impl InferenceInfo {
    fn new() -> Self {
        InferenceInfo {
            candidates: Vec::new(),
            contra_candidates: Vec::new(),
            priority: u32::MAX,
            has_priority: false,
        }
    }
    fn is_empty(&self) -> bool {
        self.candidates.is_empty() && self.contra_candidates.is_empty()
    }
}

/// Convenience alias for the inference map threaded through `infer_from`.
pub type InferMap = HashMap<SymbolId, InferenceInfo>;

impl<'a> Checker<'a> {
    pub fn infer_type_arguments(
        &mut self,
        s: &crate::types::Signature,
        args: &'a [Expr],
        ctx: Option<TypeId>,
    ) -> HashMap<SymbolId, TypeId> {
        let mut infos: InferMap = HashMap::new();
        // pass 1: non-context-sensitive args (literals, identifiers, and
        // fully-annotated function expressions). A context-sensitive function
        // expression is deferred to pass 2.
        let mut arg_types: Vec<Option<TypeId>> = vec![None; args.len()];
        for (i, a) in args.iter().enumerate() {
            if self.is_context_sensitive_arg(a) {
                continue;
            }
            let param_ty = s.params.get(i).map(|p| p.ty).or(s.rest);
            // Supplying the (uninstantiated) parameter type as context lets an
            // array/object literal keep its element literals when the target is
            // a fresh type parameter (`ks: K[]` with `K extends keyof T`),
            // matching tsc's literal-preserving inference.
            let ctx_for_arg = if matches!(a, Expr::Array { .. } | Expr::Object { .. }) {
                param_ty
            } else {
                None
            };
            // An object-literal argument omits its context-sensitive function
            // properties while being typed for inference (see check_object_literal).
            if matches!(a, Expr::Object { .. }) {
                self.cflags.skip_ctx_sensitive = true;
            }
            let count = self.contextual_function_arg_count(a);
            let suppress_yield_functions = param_ty.is_some()
                && matches!(a, Expr::Arrow(f) | Expr::FunctionExpr(f) if f.is_generator);
            if suppress_yield_functions {
                self.cflags.suppress_yield_function_implicit_any_params += 1;
            }
            let t = if param_ty.is_some() && count > 0 {
                self.with_suppressed_next_n_function_implicit_any_params(count, |this| {
                    this.check_expr(a, ctx_for_arg)
                })
            } else {
                self.check_expr(a, ctx_for_arg)
            };
            if suppress_yield_functions {
                self.cflags.suppress_yield_function_implicit_any_params -= 1;
            }
            self.cflags.skip_ctx_sensitive = false;
            arg_types[i] = Some(t);
            if let Some(pt) = param_ty {
                // an array/object literal argument infers at LITERAL priority so
                // that a concrete annotation (e.g. a callback parameter type)
                // outweighs an element-derived inference.
                let prio = if matches!(a, Expr::Array { .. }) {
                    infer_prio::LITERAL
                } else {
                    infer_prio::NONE
                };
                let prev_widen = self.cflags.infer_widen_objlit;
                self.cflags.infer_widen_objlit = self.types.is_fresh(t);
                self.infer_from(pt, t, &s.type_params, &mut infos, prio, false);
                self.cflags.infer_widen_objlit = prev_widen;
            }
        }
        let mut mapper = self.get_inferred_types(s, &infos);
        // pass 2: context-sensitive args, contextually typed by the partial
        // mapper from pass 1. This includes an object literal carrying a
        // context-sensitive method (its `this` is contextually typed): pass 1
        // omitted that method, so the literal is re-typed here with the resolved
        // contextual type — `ThisType<T>` now concrete — and the type parameters
        // its method signatures constrain (e.g. `U` in `PropDesc<U>`) are then
        // inferred.
        for (i, a) in args.iter().enumerate() {
            let param_ty = s.params.get(i).map(|p| p.ty).or(s.rest);
            // A context-sensitive function argument (handled in every generic
            // call), or an object literal that both carries a context-sensitive
            // method and whose parameter type contains a `ThisType<T>` marker.
            // The latter is the only object-literal case whose pass-1 typing was
            // altered (its methods omitted), so re-typing here is confined to it.
            let needs_pass2 = self.is_context_sensitive_arg(a)
                || (matches!(a, Expr::Object { .. })
                    && self.object_has_context_sensitive_method(a)
                    && param_ty.is_some_and(|pt| self.this_type_from_contextual(pt).is_some()));
            if !needs_pass2 {
                continue;
            }
            if let Some(pt) = param_ty {
                // Contextual typing must not force a *literal* pass-1 inference
                // onto a callback parameter. For `reduce<U>(cb: (acc: U, …) => U,
                // init: U)` called as `reduce((a, b) => a + b, 0)`, pass 1 infers
                // `U = 0`; typing the callback with `acc: 0` would then re-infer
                // `U` contravariantly as `0`, defeating the `0`-from-`init` /
                // `number`-from-`a + b` widening. Widen literals in the mapper
                // used purely for this contextual type (tsc keeps inference
                // variables unfixed and widens here); the real candidate set —
                // and thus the final, possibly literal-preserving inference — is
                // unaffected.
                let ctx_mapper: HashMap<SymbolId, TypeId> = mapper
                    .iter()
                    .map(|(&k, &v)| (k, self.types.widen_literal(v)))
                    .collect();
                let mut ctx_ty = self.instantiate_type(pt, &ctx_mapper);
                if ctx_ty == self.types.unknown {
                    if let TypeKind::TypeParam(tp) = self.types.kind(pt).clone() {
                        if self.constraint_of_type_param(tp).is_some() {
                            ctx_ty = pt;
                        }
                    }
                }
                // an object-literal arg was typed in pass 1 with its
                // context-sensitive methods omitted; drop its (partial) cached
                // type so it is re-typed in full here with the resolved `this`.
                if matches!(a, Expr::Object { .. }) {
                    self.caches
                        .expr_type_cache
                        .remove(&crate::checker::exprs::node_key_expr(a));
                    self.drop_nested_objlit_caches(a);
                }
                let t = self.check_expr(a, Some(ctx_ty));
                arg_types[i] = Some(t);
                self.infer_from(pt, t, &s.type_params, &mut infos, infer_prio::NONE, false);
                // Re-derive the mapper so a later context-sensitive argument is
                // contextually typed with the type parameters this one just
                // constrained. This lets a chain of context-sensitive callbacks
                // propagate types left to right (`pipe3(f, g, h)`: `g` infers `C`
                // from its return, then `h`'s parameter is typed as `C`).
                mapper = self.get_inferred_types(s, &infos);
            }
        }
        mapper = self.get_inferred_types(s, &infos);
        // contextual inference: for type params that arguments left uninferred,
        // infer from the expected (contextual) type by matching the signature's
        // return type against it (tsc inferTypeArguments return-type pass). These
        // inferences carry RETURN_TYPE priority, under which candidates combine by
        // union rather than common supertype.
        if let Some(ctx_ty) = ctx {
            let uninferred: Vec<SymbolId> = s
                .type_params
                .iter()
                .copied()
                .filter(|tp| infos.get(tp).map_or(true, |i| i.is_empty()))
                .collect();
            if !uninferred.is_empty() {
                self.infer_from(
                    s.ret,
                    ctx_ty,
                    &uninferred,
                    &mut infos,
                    infer_prio::RETURN_TYPE,
                    false,
                );
                mapper = self.get_inferred_types(s, &infos);
            }
        }
        // variadic tuple inference: a bare type-parameter rest (`...args: T`)
        // infers `T` as a tuple of the remaining argument types, rather than a
        // union of them (which the per-argument pass would otherwise produce).
        if let Some(rtp) = s.rest_tp {
            let fixed = s.params.len();
            let mut elems: Vec<crate::types::TupleElem> = Vec::new();
            for i in fixed..args.len() {
                let at = match arg_types.get(i).copied().flatten() {
                    Some(t) => t,
                    None => self.check_expr(&args[i], None),
                };
                let w = self.types.widen_literal(self.types.regular(at));
                elems.push(crate::types::TupleElem {
                    ty: w,
                    optional: false,
                    rest: false,
                });
            }
            let tuple = self.types.intern_kind(TypeKind::Tuple(elems));
            mapper.insert(rtp, tuple);
        }
        // constraint clamping
        for &tp in &s.type_params {
            if let Some(c) = self.constraint_of_type_param(tp) {
                // instantiate the constraint with the other inferred arguments
                // (`K extends keyof T` becomes `keyof {…}` once T is known) so the
                // check sees the concrete constraint, not the raw type parameter.
                let c = self.instantiate_type(c, &mapper);
                if let Some(&inf) = mapper.get(&tp) {
                    if !self.is_assignable_to(inf, c) {
                        mapper.insert(tp, c);
                    }
                }
            }
        }
        mapper
    }

    /// Record `source` as a candidate for type parameter `tp` at the given
    /// `priority`, into the covariant or contravariant bucket. A strictly better
    /// (lower) priority discards previously-collected candidates; an equal
    /// priority appends; a worse priority is ignored. Mirrors tsc's candidate
    /// accumulation in `inferFromTypes`.
    pub(crate) fn add_inference_candidate(
        &mut self,
        infos: &mut InferMap,
        tp: SymbolId,
        source: TypeId,
        priority: u32,
        contravariant: bool,
    ) {
        let info = infos.entry(tp).or_insert_with(InferenceInfo::new);
        if !info.has_priority || priority < info.priority {
            info.candidates.clear();
            info.contra_candidates.clear();
            info.priority = priority;
            info.has_priority = true;
        }
        if priority == info.priority {
            if contravariant {
                if !info.contra_candidates.contains(&source) {
                    info.contra_candidates.push(source);
                }
            } else if !info.candidates.contains(&source) {
                info.candidates.push(source);
            }
        }
    }

    /// Reverse (homomorphic) mapped-type inference. For a target
    /// `{ [K in keyof X]: Template }` — a mapped type over `keyof X` where `X` is
    /// a naked inference parameter and the template references `X[K]` — matched
    /// against a concrete object `source`, infer `X` as an object whose keys are
    /// the source's keys and whose value at each key `P` is inferred from the
    /// template (with `X[K]` standing for the inferred value) against `source[P]`.
    /// Drives `defineProps`' `PropDescMap<U>` (`{ [K in keyof U]: PropDesc<U[K]> }`
    /// infers `U`) and Vue's `Accessors<P>`. Mirrors tsc's
    /// `inferTypeForHomomorphicMappedType`.
    #[allow(clippy::too_many_arguments)]
    fn infer_to_reverse_mapped(
        &mut self,
        mapped_key: usize,
        captured: &[(SymbolId, TypeId)],
        source: TypeId,
        tps: &[SymbolId],
        infos: &mut InferMap,
        priority: u32,
        contravariant: bool,
    ) {
        let Some(&(node, scope, file)) = self.deferred.deferred_mappeds.get(&mapped_key) else {
            return;
        };
        // the constraint must be `keyof X`
        let TypeNode::Keyof { ty: con_ty, .. } = &node.constraint else {
            return;
        };
        // a templateless mapped type (`{ [K in keyof X] }`) infers nothing
        let Some(value_node) = &node.value else {
            return;
        };
        let cap_mapper: Mapper = captured.iter().copied().collect();
        let prev_file = self.current_file;
        self.current_file = file;
        // X = the `keyof` operand. The template references it as the mapped
        // type's *own* parameter (`alias_sym`, e.g. `T` of `PropDescMap<T>`); the
        // inference parameter we actually solve for (`x_sym`, e.g. `U`) is that
        // parameter run through the captured substitution. We substitute the
        // template's `alias_sym` directly per property and add the assembled
        // candidate against `x_sym`.
        let x_raw = self.resolve_type(con_ty, scope);
        let alias_sym = match self.types.kind(x_raw) {
            TypeKind::TypeParam(s) => *s,
            _ => {
                self.current_file = prev_file;
                return;
            }
        };
        let x = self.instantiate_type(x_raw, &cap_mapper);
        let x_sym = match self.types.kind(x) {
            TypeKind::TypeParam(s) if tps.contains(s) => *s,
            _ => {
                self.current_file = prev_file;
                return;
            }
        };
        // resolve the value template once, with K bound to a placeholder param.
        let key_sym = self.synthetic_type_param(node_key(node), &node.key.name);
        self.tp
            .infer_mapped_env
            .push((node.key.name.clone(), key_sym));
        let value_raw = self.resolve_type(value_node, scope);
        self.tp.infer_mapped_env.pop();
        self.current_file = prev_file;
        // the source must be an object with named properties. Tuple/array
        // sources need structure-preserving (mapped tuple/array) inference, which
        // this does not synthesize; leaving them uninferred matches the prior
        // behavior and avoids a wrong object-shaped inference.
        if matches!(
            self.types.kind(source),
            TypeKind::Tuple(_) | TypeKind::ReadonlyTuple(_) | TypeKind::ReadonlyArray(_)
        ) || self.array_element_type(source).is_some()
        {
            return;
        }
        // A `readonly` homomorphic mapped type (`readonly [R in keyof T]: …`) is
        // paired in practice with a `const` type parameter, whose literal
        // preservation tsrs does not yet model: the argument's function
        // returns/literals are already widened by the time they reach inference,
        // so a reverse inference here would record a widened type (`string` for a
        // `() => "x"` template) that the call site then rejects against the
        // intended literal. Until `const` type parameters are supported, leave
        // such parameters uninferred (the prior behavior) rather than infer a
        // wrong, widened shape.
        if matches!(node.readonly_mod, Some(crate::ast::MappedModifier::Add)) {
            return;
        }
        let Some(src_sid) = self.shape_of_type(source) else {
            return;
        };
        let src_props = self.types.shape(src_sid).props.clone();
        if src_props.is_empty() {
            return;
        }
        let mut out = Shape::default();
        for sp in &src_props {
            // a fresh inference variable V standing in for `X[name]`.
            let v_key = node_key(node) ^ Self::name_hash(&sp.name);
            let v_sym = self.synthetic_type_param(v_key, "V");
            let v = self.types.intern_kind(TypeKind::TypeParam(v_sym));
            // bind the template's parameter to `{ name: V }` and `K` to `"name"`,
            // so the template's `X[K]` instantiates to `V`; the rest stays intact.
            let mut one = Shape::default();
            one.props.push(PropInfo {
                name: sp.name.clone(),
                ty: v,
                optional: false,
                readonly: false,
                is_method: false,
                symbol: None,
            });
            let one_id = self.types.alloc_shape(one);
            let x_obj = self.types.intern_kind(TypeKind::Anon(one_id));
            let name_lit = self.types.string_lit(&sp.name);
            let mut m2: Mapper = Mapper::new();
            m2.insert(alias_sym, x_obj);
            m2.insert(key_sym, name_lit);
            let tmpl = self.instantiate_type(value_raw, &m2);
            // infer V from the template against this source property's type.
            let mut sub: InferMap = HashMap::new();
            self.infer_from(tmpl, sp.ty, &[v_sym], &mut sub, priority, contravariant);
            let vt = sub
                .get(&v_sym)
                .map(|info| self.combine_inference_candidates(info))
                .unwrap_or(self.types.unknown);
            out.props.push(PropInfo {
                name: sp.name.clone(),
                ty: vt,
                optional: sp.optional,
                readonly: sp.readonly,
                is_method: false,
                symbol: None,
            });
        }
        let out_id = self.types.alloc_shape(out);
        let inferred = self.types.intern_kind(TypeKind::Anon(out_id));
        self.add_inference_candidate(
            infos,
            x_sym,
            inferred,
            priority | infer_prio::HOMOMORPHIC_MAPPED,
            contravariant,
        );
    }

    /// A small order-independent hash of a property name, used to mint a distinct
    /// synthetic inference variable per reverse-mapped source property.
    fn name_hash(name: &str) -> usize {
        let mut h: usize = 0xcbf29ce484222325;
        for b in name.bytes() {
            h ^= b as usize;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    /// Combine an inference variable's collected candidates into a single type,
    /// following the same covariant/contravariant resolution as
    /// `get_inferred_type` but without a signature context (used by reverse
    /// mapped-type inference, where each value variable is solved in isolation).
    /// Literal widening is intentionally *not* applied here: object-property
    /// matches already widen through `infer_from_shapes` when the source is a
    /// fresh literal, while values that should stay literal (a function return
    /// in a `readonly`/`const` template) are preserved.
    fn combine_inference_candidates(&mut self, info: &InferenceInfo) -> TypeId {
        let covariant = if !info.candidates.is_empty() {
            let processed: Vec<TypeId> = info
                .candidates
                .iter()
                .map(|&c| self.types.regular(c))
                .collect();
            Some(self.get_common_supertype(&processed))
        } else {
            None
        };
        if !info.contra_candidates.is_empty() {
            if let Some(cov) = covariant {
                let contra = info.contra_candidates.clone();
                let cov_subtype_of_all = contra.iter().all(|&t| self.is_assignable_to(cov, t));
                let anon = matches!(self.types.kind(cov), TypeKind::Anon(_));
                if cov_subtype_of_all && !anon {
                    return cov;
                }
            }
            return self.get_contravariant_inference(info);
        }
        covariant.unwrap_or(self.types.unknown)
    }

    /// Resolve every type parameter of `s` to its inferred type from `infos`,
    /// leaving `unknown` for any parameter no argument constrained.
    fn get_inferred_types(
        &mut self,
        s: &crate::types::Signature,
        infos: &InferMap,
    ) -> HashMap<SymbolId, TypeId> {
        let mut mapper = HashMap::new();
        for &tp in &s.type_params {
            let has_candidates = infos.get(&tp).map_or(false, |info| !info.is_empty());
            let t = if has_candidates {
                self.get_inferred_type(s, tp, infos)
            } else if let Some(d) = self.default_of_type_param(tp) {
                // no argument constrains this parameter: fall back to its default,
                // instantiated with the parameters resolved so far (a default may
                // reference an earlier parameter, e.g. `<T, U = T>`).
                self.instantiate_type(d, &mapper)
            } else {
                self.types.unknown
            };
            mapper.insert(tp, t);
        }
        mapper
    }

    /// The inferred type for a single parameter, following tsc's `getInferredType`:
    /// a covariant inference is preferred when it is a subtype of every
    /// contravariant candidate (and not an anonymous object type); otherwise the
    /// contravariant inference is used.
    fn get_inferred_type(
        &mut self,
        s: &crate::types::Signature,
        tp: SymbolId,
        infos: &InferMap,
    ) -> TypeId {
        let Some(info) = infos.get(&tp) else {
            return self.types.unknown;
        };
        if info.is_empty() {
            return self.types.unknown;
        }
        let covariant = if !info.candidates.is_empty() {
            Some(self.get_covariant_inference(s, tp, info))
        } else {
            None
        };
        if !info.contra_candidates.is_empty() {
            if let Some(cov) = covariant {
                let contra = info.contra_candidates.clone();
                let cov_subtype_of_all = contra.iter().all(|&t| self.is_assignable_to(cov, t));
                let anon = matches!(self.types.kind(cov), TypeKind::Anon(_));
                if cov_subtype_of_all && !anon {
                    return cov;
                }
            }
            return self.get_contravariant_inference(info);
        }
        covariant.unwrap_or(self.types.unknown)
    }

    /// Covariant inference: combine candidates either by union (under a
    /// combination priority such as a return-type inference) or by their common
    /// supertype, after literal widening unless the parameter keeps literals.
    fn get_covariant_inference(
        &mut self,
        s: &crate::types::Signature,
        tp: SymbolId,
        info: &InferenceInfo,
    ) -> TypeId {
        // keep literals when T appears at top level of the return type OR when T
        // has a primitive-ish constraint (tsc hasPrimitiveConstraint); otherwise
        // widen literal candidates.
        let ret_keeps =
            self.type_param_at_top_level(s.ret, tp) || self.has_primitive_constraint(tp);
        let processed: Vec<TypeId> = info
            .candidates
            .iter()
            .map(|&c| {
                let r = self.types.regular(c);
                if ret_keeps {
                    r
                } else {
                    self.types.widen_literal(r)
                }
            })
            .collect();
        if info.priority & infer_prio::PRIORITY_IMPLIES_COMBINATION != 0 {
            // return-type inferences are combined by union (with subtype
            // reduction handled by `union`).
            self.types.union(processed)
        } else {
            self.get_common_supertype(&processed)
        }
    }

    /// Contravariant inference: the common subtype (narrowest) of the
    /// contravariant candidates. tsc forms an intersection under combination
    /// priorities; lacking an intersection type, tsrs reduces to the narrowest
    /// candidate (FP-safe: a too-wide parameter type yields no spurious error).
    fn get_contravariant_inference(&mut self, info: &InferenceInfo) -> TypeId {
        let cands = info.contra_candidates.clone();
        let mut acc = cands[0];
        for &t in &cands[1..] {
            if self.is_assignable_to(t, acc) && !self.is_assignable_to(acc, t) {
                acc = t; // keep the strict subtype
            }
        }
        acc
    }

    /// The best common supertype of `types`, following tsc's `getCommonSupertype`:
    /// literals sharing a base type widen to their union; otherwise the list is
    /// reduced to the member that is a supertype of the others (keeping the first
    /// when none relate).
    fn get_common_supertype(&mut self, types: &[TypeId]) -> TypeId {
        match types.len() {
            0 => return self.types.unknown,
            1 => return types[0],
            _ => {}
        }
        if self.literals_share_base(types) {
            return self.types.union(types.to_vec());
        }
        let mut acc = types[0];
        for &t in &types[1..] {
            if self.is_assignable_to(acc, t) && !self.is_assignable_to(t, acc) {
                acc = t; // keep the supertype
            }
        }
        acc
    }

    /// Whether every type is a literal and they share the same base primitive
    /// (all string literals, all number literals, or all boolean literals), in
    /// which case their common supertype is their union (e.g. `1 | 2`).
    fn literals_share_base(&self, types: &[TypeId]) -> bool {
        #[derive(PartialEq, Clone, Copy)]
        enum Base {
            Str,
            Num,
            Bool,
        }
        let base_of = |t: TypeId, this: &Self| -> Option<Base> {
            match this.types.kind(t) {
                TypeKind::StrLit(_) => Some(Base::Str),
                TypeKind::NumLit(_) => Some(Base::Num),
                TypeKind::BoolLit(_) => Some(Base::Bool),
                _ => None,
            }
        };
        let Some(first) = base_of(types[0], self) else {
            return false;
        };
        types.iter().all(|&t| base_of(t, self) == Some(first))
    }

    /// The constraint of a type parameter, or `unknown` when it has none; any
    /// other type is returned unchanged. Used by the cast comparability check.
    pub(crate) fn param_constraint_or_unknown(&mut self, t: TypeId) -> TypeId {
        if let TypeKind::TypeParam(s) = self.types.kind(t) {
            let s = *s;
            self.constraint_of_type_param(s)
                .unwrap_or(self.types.unknown)
        } else {
            t
        }
    }

    /// Whether a `value as Type` conversion overlaps enough to be allowed
    /// without the 2352 warning. This mirrors tsc's *comparable* relation rather
    /// than plain assignability: a type parameter is compared through its
    /// constraint (unknown if unconstrained, so it overlaps any single concrete
    /// type), two distinct type parameters never overlap, and an indexed access
    /// is treated as indeterminate since it could resolve to any value type.
    pub(crate) fn cast_comparable(&mut self, a: TypeId, b: TypeId) -> bool {
        // tsc areTypesComparable = isTypeComparableTo(a,b) ||
        // isTypeComparableTo(b,a). The whole query runs in the
        // comparableRelation: generic signatures erase and union sources
        // need only one member (relations.rs, gated on erase_generic_sigs);
        // results are cached separately from the assignable relation.
        let saved = self.rel.erase_generic_sigs;
        self.rel.erase_generic_sigs = true;
        let r = self.comparable_dir(a, b) || self.comparable_dir(b, a);
        self.rel.erase_generic_sigs = saved;
        r
    }

    /// isTypeComparableTo(src, tgt), directional. Type-parameter operands
    /// keep the cast-tuned symmetric constraint overlap (matches oracle:
    /// `n as T`, `t as string`, `t === "x"` are all legal for unconstrained
    /// T); direction matters for intersections, where tsc's collapse rule
    /// (unionOrIntersectionRelatedTo: primitive target → instantiable
    /// members replaced by their base constraints, and if the re-formed
    /// intersection collapses the verdict is decided RIGHT THERE) is what
    /// makes `x === "hello"` an error for `x: T & number`.
    fn comparable_dir(&mut self, src: TypeId, tgt: TypeId) -> bool {
        if src == tgt {
            return true;
        }
        let pa = match self.types.kind(src) {
            TypeKind::TypeParam(s) => Some(*s),
            _ => None,
        };
        let pb = match self.types.kind(tgt) {
            TypeKind::TypeParam(s) => Some(*s),
            _ => None,
        };
        if pa.is_some() && pb.is_some() {
            return src == tgt;
        }
        if self.cast_keyof_overlaps(src, tgt) {
            return true;
        }
        // SOURCE type parameter: unconstrained overlaps any single concrete
        // type (oracle: `t as string` / `t === "x"` are legal); constrained
        // compares through the constraint, DIRECTIONALLY (tsc walks
        // isRelatedTo(constraint, target) — `T extends string|number` is
        // comparable to `"x"` via string~"x", but NOT to a disjoint object)
        if let Some(s_sym) = pa {
            return match self.constraint_of_type_param(s_sym) {
                None => true,
                Some(ca) => {
                    if self.cast_keyof_overlaps(ca, tgt) {
                        return true;
                    }
                    self.comparable_dir(ca, tgt)
                }
            };
        }
        // TARGET type parameter: tsc has NO constraint rule on this side —
        // `42` is not comparable to a constrained T even when 42 satisfies
        // the constraint (unknownControlFlow fx3/fx4). Unconstrained still
        // accepts (`n as T` legality comes from the source side, but keep
        // the unconstrained target permissive for the symmetric cast form).
        if let Some(t_sym) = pb {
            return match self.constraint_of_type_param(t_sym) {
                None => true,
                Some(cb) => {
                    if self.cast_keyof_overlaps(src, cb) {
                        return true;
                    }
                    self.is_assignable_to(src, tgt)
                }
            };
        }
        if matches!(self.types.kind(src), TypeKind::IndexedAccess(..))
            || matches!(self.types.kind(tgt), TypeKind::IndexedAccess(..))
        {
            return true;
        }
        if let TypeKind::Intersection(ms) = self.types.kind(src) {
            let mut ms = ms.clone();
            // tsc: comparable + primitive target → substitute type params
            // with their base constraints; a collapsed (non-intersection)
            // result decides bidirectionally without falling through —
            // `T&number` with `T extends string|number` collapses to
            // `number`, so `=== "hello"` errors
            if self.comparable_primitive_like(tgt)
                && ms
                    .iter()
                    .any(|&m| matches!(self.types.kind(m), TypeKind::TypeParam(_)))
            {
                let subst: Vec<TypeId> = ms
                    .iter()
                    .map(|&m| self.param_constraint_or_unknown(m))
                    .collect();
                let collapsed = self.intersect_all(subst);
                match self.types.kind(collapsed) {
                    TypeKind::Never => return false,
                    TypeKind::Intersection(nms) => ms = nms.clone(),
                    _ => {
                        return self.is_assignable_to(collapsed, tgt)
                            || self.is_assignable_to(tgt, collapsed);
                    }
                }
            }
            // someTypeRelatedToType: one constituent, directionally
            return ms.iter().any(|&m| self.comparable_dir(m, tgt));
        }
        if let TypeKind::Intersection(ms) = self.types.kind(tgt) {
            // typeRelatedToEachType: EVERY target constituent ('"hello"' is
            // not comparable to `T & number` because number rejects it;
            // `<number & Brand>0` still passes via the reverse direction's
            // number~0 overlap)
            let ms = ms.clone();
            return ms.iter().all(|&m| self.comparable_dir(src, m));
        }
        self.is_assignable_to(src, tgt)
    }

    /// tsc TypeFlags.Primitive projection for the comparable collapse rule
    /// (`boolean` is the true|false union in tsrs, hence the id check)
    fn comparable_primitive_like(&self, t: TypeId) -> bool {
        t == self.types.boolean
            || matches!(
                self.types.kind(t),
                TypeKind::String
                    | TypeKind::StrLit(_)
                    | TypeKind::TemplateLit(_)
                    | TypeKind::Number
                    | TypeKind::NumLit(_)
                    | TypeKind::Bigint
                    | TypeKind::BigIntLit(_)
                    | TypeKind::BoolLit(_)
                    | TypeKind::EsSymbol
                    | TypeKind::Null
                    | TypeKind::Undefined
                    | TypeKind::Void
                    | TypeKind::EnumType(_)
                    | TypeKind::EnumMember(_)
            )
    }

    fn cast_keyof_overlaps(&mut self, a: TypeId, b: TypeId) -> bool {
        if self.keyof_type_parameter_inner(a) {
            let keys = self.property_key_type();
            return self.is_assignable_to(b, keys) || self.is_assignable_to(keys, b);
        }
        if self.keyof_type_parameter_inner(b) {
            let keys = self.property_key_type();
            return self.is_assignable_to(a, keys) || self.is_assignable_to(keys, a);
        }
        false
    }

    fn has_primitive_constraint(&mut self, tp: SymbolId) -> bool {
        let Some(c) = self.constraint_of_type_param(tp) else {
            return false;
        };
        self.is_primitive_ish(c)
    }

    fn is_primitive_ish(&mut self, t: TypeId) -> bool {
        match self.types.kind(t).clone() {
            TypeKind::String
            | TypeKind::Number
            | TypeKind::Bigint
            | TypeKind::EsSymbol
            | TypeKind::StrLit(_)
            | TypeKind::NumLit(_)
            | TypeKind::BigIntLit(_)
            | TypeKind::BoolLit(_)
            | TypeKind::Keyof(_)
            | TypeKind::EnumType(_)
            | TypeKind::EnumMember(_) => true,
            TypeKind::Union(ms) => ms.iter().any(|&m| self.is_primitive_ish(m)),
            _ => false,
        }
    }

    fn type_param_at_top_level(&self, ret: TypeId, tp: SymbolId) -> bool {
        match self.types.kind(ret) {
            TypeKind::TypeParam(s) => *s == tp,
            TypeKind::Union(ms) => ms
                .iter()
                .any(|&m| matches!(self.types.kind(m), TypeKind::TypeParam(s) if *s == tp)),
            _ => false,
        }
    }

    /// Infer type-parameter candidates by matching a `target` type (which may
    /// mention the parameters in `tps`) against a `source` type, recording
    /// candidates into `infos` at the given `priority` and variance. This is the
    /// core of tsc's `inferFromTypes`. `contravariant` flips at function
    /// parameter positions so a callback's parameter types are inferred
    /// contravariantly.
    pub(crate) fn infer_from(
        &mut self,
        target: TypeId,
        source: TypeId,
        tps: &[SymbolId],
        infos: &mut InferMap,
        priority: u32,
        contravariant: bool,
    ) {
        // a type parameter we are inferring: record the source as a candidate.
        if let TypeKind::TypeParam(s) = self.types.kind(target) {
            if tps.contains(s) {
                let s = *s;
                self.add_inference_candidate(infos, s, source, priority, contravariant);
                return;
            }
        }
        // identical types contribute no information.
        if source == target {
            return;
        }
        match self.types.kind(target).clone() {
            TypeKind::Ref(t_sym, t_args) => {
                let mut handled = false;
                if let TypeKind::Ref(s_sym, s_args) = self.types.kind(source).clone() {
                    if t_sym == s_sym && t_args.len() == s_args.len() {
                        for (t, s) in t_args.iter().zip(s_args.iter()) {
                            self.infer_from(*t, *s, tps, infos, priority, contravariant);
                        }
                        handled = true;
                    }
                }
                // An array-like target (`Array<X>`, `Iterable<X>`,
                // `ReadonlyArray<X>`, `ArrayLike<X>`) against a tuple or array
                // source: infer X from the element type(s). Fires even when the
                // source is itself a `Ref` (e.g. `Promise<number>[]`), and drives
                // `Promise.all([...])`, whose parameter is
                // `Iterable<T | PromiseLike<T>>`.
                if !handled
                    && t_args.len() == 1
                    && (Some(t_sym) == self.array_symbol()
                        || Some(t_sym) == self.global_type_symbol("Iterable")
                        || Some(t_sym) == self.global_type_symbol("ReadonlyArray")
                        || Some(t_sym) == self.global_type_symbol("ArrayLike"))
                {
                    match self.types.kind(source).clone() {
                        TypeKind::Tuple(elems) | TypeKind::ReadonlyTuple(elems) => {
                            for e in elems {
                                self.infer_from(
                                    t_args[0],
                                    e.ty,
                                    tps,
                                    infos,
                                    priority,
                                    contravariant,
                                );
                            }
                            handled = true;
                        }
                        _ => {
                            if let Some(elem) = self.array_element_type(source) {
                                self.infer_from(
                                    t_args[0],
                                    elem,
                                    tps,
                                    infos,
                                    priority,
                                    contravariant,
                                );
                                handled = true;
                            }
                        }
                    }
                }
                if !handled {
                    // different named types (e.g. inferring `U` from
                    // `PromiseLike<infer U>` against `Promise<number>`): infer
                    // through the structural shapes of both sides.
                    self.infer_from_shapes(target, source, tps, infos, priority, contravariant);
                }
            }
            TypeKind::Union(t_members) => {
                self.infer_to_union(source, &t_members, tps, infos, priority, contravariant);
            }
            TypeKind::Intersection(t_members) => {
                // infer a lone naked type-parameter operand of `T & C & …` from
                // the source (`pigify<T>(y: T & Bear)` called with `Man & Bear`
                // infers `T = Man & Bear`); concrete operands only constrain.
                // With two or more naked parameters the split is ambiguous, so
                // leave them to default rather than binding each to the source.
                let naked: Vec<TypeId> = t_members
                    .iter()
                    .copied()
                    .filter(|&m| matches!(self.types.kind(m), TypeKind::TypeParam(s) if tps.contains(s)))
                    .collect();
                if naked.len() == 1 {
                    self.infer_from(naked[0], source, tps, infos, priority, contravariant);
                }
                // When no member is a naked inference parameter, infer through
                // each non-naked member by matching its shape against the source
                // (`PropDesc<U> & ThisType<T>` infers `U` from the `PropDesc<U>`
                // member — its expanded `Anon` shape hides `U` from
                // `type_contains_params`, so every member is recursed; the empty
                // `ThisType<T>` marker and any fully-concrete member infer
                // nothing). This is gated on the absence of a naked parameter:
                // with a naked member present (`{ produceThing: T1 } & TConfig`)
                // the naked binding above already captures the source, and also
                // inferring `T1` from the `{ produceThing: T1 }` member would
                // over-constrain it against the (widened) naked inference.
                if naked.is_empty() {
                    for m in t_members {
                        self.infer_from(m, source, tps, infos, priority, contravariant);
                    }
                }
            }
            TypeKind::DeferredMapped(mapped_key, captured) => {
                // reverse (homomorphic) mapped-type inference: infer `X` from
                // `{ [K in keyof X]: Template(X[K]) }` matched against a concrete
                // object source (`PropDescMap<U>` infers `U`, `Accessors<P>`
                // infers `P`). See `infer_to_reverse_mapped`.
                self.infer_to_reverse_mapped(
                    mapped_key,
                    &captured,
                    source,
                    tps,
                    infos,
                    priority,
                    contravariant,
                );
            }
            TypeKind::Tuple(p_elems) | TypeKind::ReadonlyTuple(p_elems) => {
                // infer from a tuple pattern (`[infer H, ...any[]]`): match the
                // leading fixed elements positionally, the rest element against
                // the middle source elements, and the trailing fixed elements
                // from the back.
                let a_elems = match self.types.kind(source).clone() {
                    TypeKind::Tuple(e) | TypeKind::ReadonlyTuple(e) => Some(e),
                    _ => None,
                };
                if let Some(a_elems) = a_elems {
                    match p_elems.iter().position(|e| e.rest) {
                        None => {
                            for (pe, ae) in p_elems.iter().zip(a_elems.iter()) {
                                self.infer_from(pe.ty, ae.ty, tps, infos, priority, contravariant);
                            }
                        }
                        Some(r) => {
                            for i in 0..r {
                                if let (Some(pe), Some(ae)) = (p_elems.get(i), a_elems.get(i)) {
                                    self.infer_from(
                                        pe.ty,
                                        ae.ty,
                                        tps,
                                        infos,
                                        priority,
                                        contravariant,
                                    );
                                }
                            }
                            let rest_et = p_elems[r].ty;
                            let trail_len = p_elems.len() - r - 1;
                            let a_mid_end = a_elems.len().saturating_sub(trail_len);
                            // a bare type-parameter rest (`[...T]`) captures the
                            // middle source elements as a *tuple* (variadic), not
                            // element-wise.
                            let rest_sym = match self.types.kind(rest_et) {
                                TypeKind::TypeParam(s) if tps.contains(s) => Some(*s),
                                _ => None,
                            };
                            if let Some(rsym) = rest_sym {
                                let mid: Vec<crate::types::TupleElem> = (r..a_mid_end)
                                    .filter_map(|k| a_elems.get(k).copied())
                                    .collect();
                                let tup = self.types.tuple(mid);
                                self.add_inference_candidate(
                                    infos,
                                    rsym,
                                    tup,
                                    priority,
                                    contravariant,
                                );
                            } else {
                                for k in r..a_mid_end {
                                    if let Some(ae) = a_elems.get(k) {
                                        self.infer_from(
                                            rest_et,
                                            ae.ty,
                                            tps,
                                            infos,
                                            priority,
                                            contravariant,
                                        );
                                    }
                                }
                            }
                            for j in 0..trail_len {
                                if let (Some(pe), Some(ae)) =
                                    (p_elems.get(r + 1 + j), a_elems.get(a_mid_end + j))
                                {
                                    self.infer_from(
                                        pe.ty,
                                        ae.ty,
                                        tps,
                                        infos,
                                        priority,
                                        contravariant,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            TypeKind::TemplateLit(parts) => {
                // infer from a template pattern (`\`on${infer K}\``): match the
                // concrete string literal and bind each infer placeholder.
                if let TypeKind::StrLit(s) = self.types.kind(source).clone() {
                    let parts = parts.clone();
                    let s = s.to_str_lossy().into_owned();
                    self.collect_template_candidates(&parts, &s, tps, infos, priority);
                }
            }
            TypeKind::Anon(_) | TypeKind::DeferredObj(_) => {
                // An object-type annotation containing a type parameter
                // (`o: { value: T }`) is a deferred object literal; resolve it to
                // a shape and infer through its properties (`T` from `value`).
                self.infer_from_shapes(target, source, tps, infos, priority, contravariant);
            }
            _ => {}
        }
    }

    /// Infer to a union `target` from `source`, following tsc's union handling:
    /// first drop source members identical to a target member (and the matched
    /// targets); then infer each remaining source member against every remaining
    /// non-type-variable target member, and against each naked type-variable
    /// member at `NAKED_TYPE_VARIABLE` priority so the wrapped/structural members
    /// take precedence (`PromiseLike<T>` over a bare `T`).
    fn infer_to_union(
        &mut self,
        source: TypeId,
        t_members: &[TypeId],
        tps: &[SymbolId],
        infos: &mut InferMap,
        priority: u32,
        contravariant: bool,
    ) {
        let source_members: Vec<TypeId> = match self.types.kind(source).clone() {
            TypeKind::Union(sm) => sm,
            _ => vec![source],
        };
        // inferFromMatchingTypes: pair off identical members on both sides.
        let mut matched_target = vec![false; t_members.len()];
        let mut remaining_sources: Vec<TypeId> = Vec::new();
        for &sm in &source_members {
            let mut matched = false;
            for (i, &tm) in t_members.iter().enumerate() {
                if !matched_target[i] && tm == sm {
                    matched_target[i] = true;
                    matched = true;
                    break;
                }
            }
            if !matched {
                remaining_sources.push(sm);
            }
        }
        let mut naked: Vec<TypeId> = Vec::new();
        let mut others: Vec<TypeId> = Vec::new();
        for (i, &m) in t_members.iter().enumerate() {
            if matched_target[i] {
                continue;
            }
            if matches!(self.types.kind(m), TypeKind::TypeParam(s) if tps.contains(s)) {
                naked.push(m);
            } else {
                others.push(m);
            }
        }
        for &sm in &remaining_sources {
            for &tm in &others {
                self.infer_from(tm, sm, tps, infos, priority, contravariant);
            }
            for &nk in &naked {
                self.infer_from(
                    nk,
                    sm,
                    tps,
                    infos,
                    priority | infer_prio::NAKED_TYPE_VARIABLE,
                    contravariant,
                );
            }
        }
    }

    /// Infer type-parameter candidates by matching the structural shapes of
    /// `param` and `arg` (shared properties, a single call signature, and a
    /// single construct signature). Guarded against unbounded recursion through
    /// self-referential shapes via `infer_depth`.
    /// During inference from a fresh object literal, a property whose target
    /// type is a type parameter with a primitive constraint keeps its literal
    /// (`<T extends number>` infers `5`, not `number`); otherwise the literal is
    /// widened.
    fn target_prop_keeps_literal(&mut self, pp_ty: TypeId, tps: &[SymbolId]) -> bool {
        let s = match self.types.kind(pp_ty) {
            TypeKind::TypeParam(s) => *s,
            _ => return false,
        };
        tps.contains(&s) && self.has_primitive_constraint(s)
    }

    pub(crate) fn infer_from_shapes(
        &mut self,
        target: TypeId,
        source: TypeId,
        tps: &[SymbolId],
        infos: &mut InferMap,
        priority: u32,
        contravariant: bool,
    ) {
        if self.guards.infer_depth > 4 {
            return;
        }
        self.guards.infer_depth += 1;
        if let (Some(p_sid), Some(a_sid)) = (self.shape_of_type(target), self.shape_of_type(source))
        {
            let p_shape = self.types.shape(p_sid).clone();
            let a_shape = self.types.shape(a_sid).clone();
            // A fresh object literal widens its property literals for inference
            // (`{ value: 5 }` infers `number` for `{ value: T }`), matching tsc's
            // mutable-location rule, and the same applies through nested object
            // literals. A read-only (`as const`) property keeps its literal, as
            // does a type parameter with a primitive constraint.
            // properties are covariant.
            for pp in &p_shape.props {
                if let Some(ap) = a_shape.prop(&pp.name) {
                    let src = if self.cflags.infer_widen_objlit
                        && !ap.readonly
                        && !self.target_prop_keeps_literal(pp.ty, tps)
                    {
                        self.types.widen_literal(self.types.regular(ap.ty))
                    } else {
                        ap.ty
                    };
                    self.infer_from(pp.ty, src, tps, infos, priority, contravariant);
                }
            }
            if p_shape.call_sigs.len() == 1 && a_shape.call_sigs.len() == 1 {
                let ps = self.types.sig(p_shape.call_sigs[0]).clone();
                let as_ = self.types.sig(a_shape.call_sigs[0]).clone();
                // Parameters are a contravariant position. We nonetheless infer
                // them covariantly here: a context-sensitive arrow argument's
                // parameters are typed *from* the contextual type during pass 2,
                // so inferring contravariantly from them would feed the
                // already-inferred type back as a contra candidate and (via the
                // co/contra preference in `get_inferred_type`) pin the parameter
                // to that literal — a false positive on the common
                // `reduce(arr, (a, b) => …, 0)` pattern. Covariant inference of
                // parameters is FP-safe (it only ever widens the result).
                for (pp, ap) in ps.params.iter().zip(as_.params.iter()) {
                    self.infer_from(pp.ty, ap.ty, tps, infos, priority, contravariant);
                }
                // a rest parameter typed as a bare infer (`(...args: infer P)`,
                // used by Parameters/ConstructorParameters) infers the tuple of
                // the remaining argument parameters. This is a homomorphic
                // capture and is recorded covariantly.
                if let Some(rest_tp) = ps.rest_tp {
                    if tps.contains(&rest_tp) {
                        let start = ps.params.len();
                        let rest_elems: Vec<crate::types::TupleElem> = as_
                            .params
                            .iter()
                            .skip(start)
                            .map(|p| crate::types::TupleElem {
                                ty: p.ty,
                                optional: p.optional,
                                rest: false,
                            })
                            .collect();
                        let tuple_ty = self.types.tuple(rest_elems);
                        self.add_inference_candidate(infos, rest_tp, tuple_ty, priority, false);
                    }
                }
                // the return type is covariant.
                let p_ret = self.sig_return(p_shape.call_sigs[0]);
                let a_ret = self.sig_return(a_shape.call_sigs[0]);
                self.infer_from(p_ret, a_ret, tps, infos, priority, contravariant);
            }
            if p_shape.ctor_sigs.len() == 1 && a_shape.ctor_sigs.len() == 1 {
                let ps = self.types.sig(p_shape.ctor_sigs[0]).clone();
                let as_ = self.types.sig(a_shape.ctor_sigs[0]).clone();
                // covariant parameter inference (see the call-signature note).
                for (pp, ap) in ps.params.iter().zip(as_.params.iter()) {
                    self.infer_from(pp.ty, ap.ty, tps, infos, priority, contravariant);
                }
                let p_ret = self.sig_return(p_shape.ctor_sigs[0]);
                let a_ret = self.sig_return(a_shape.ctor_sigs[0]);
                self.infer_from(p_ret, a_ret, tps, infos, priority, contravariant);
            }
        }
        self.guards.infer_depth -= 1;
    }

    // ── operators ───────────────────────────────────────────────────────────
}
