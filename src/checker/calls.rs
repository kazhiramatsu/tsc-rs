//! Call / new resolution: callability, overload resolution, `new` expressions,
//! and the context-sensitive-argument machinery that drives two-pass generic
//! inference. Split out of `exprs.rs`.

use crate::ast::*;
use crate::binder::{flags, SymbolId};
use crate::checker::exprs::{expected_args_display, expr_contains_optional_chain, node_key_expr};
use crate::checker::Checker;
use crate::diagnostics::{gen, DiagnosticMessage};
use crate::types::{SigId, TypeId, TypeKind};
use std::collections::HashMap;

struct CallCandidateTrial {
    sig: SigId,
    mapper: HashMap<SymbolId, TypeId>,
    arity_ok: bool,
    non_function_args_ok: bool,
    first_failed_arg_index: Option<usize>,
}

impl<'a> Checker<'a> {
    fn is_immediately_invoked_function_callee(e: &'a Expr) -> bool {
        match e {
            Expr::Arrow(_) | Expr::FunctionExpr(_) => true,
            Expr::Paren { inner, .. } => Self::is_immediately_invoked_function_callee(inner),
            _ => false,
        }
    }

    fn check_call_callee_expr(&mut self, callee: &'a Expr) -> TypeId {
        if Self::is_immediately_invoked_function_callee(callee) {
            self.with_suppressed_next_function_implicit_any_params(|this| {
                this.check_expr(callee, None)
            })
        } else {
            self.check_expr(callee, None)
        }
    }

    pub(crate) fn contextual_function_arg_count(&self, a: &Expr) -> u32 {
        match a {
            Expr::Arrow(_) | Expr::FunctionExpr(_) => 1,
            Expr::Paren { inner, .. } => self.contextual_function_arg_count(inner),
            Expr::Cond {
                when_true,
                when_false,
                ..
            } => {
                self.contextual_function_arg_count(when_true)
                    + self.contextual_function_arg_count(when_false)
            }
            _ => 0,
        }
    }

    fn is_conditional_function_arg(&self, a: &Expr) -> bool {
        match a {
            Expr::Paren { inner, .. } => self.is_conditional_function_arg(inner),
            Expr::Cond {
                when_true,
                when_false,
                ..
            } => {
                self.contextual_function_arg_count(when_true) > 0
                    || self.contextual_function_arg_count(when_false) > 0
            }
            _ => false,
        }
    }

    fn clear_function_like_expr_check_cache(&mut self, a: &'a Expr) {
        self.caches.expr_type_cache.remove(&node_key_expr(a));
        match a {
            Expr::Arrow(f) | Expr::FunctionExpr(f) => {
                self.checked_decls.remove(&node_key(&**f));
            }
            Expr::Paren { inner, .. } => self.clear_function_like_expr_check_cache(inner),
            _ => {}
        }
    }

    pub(crate) fn check_call_like(&mut self, e: &'a Expr, ctx: Option<TypeId>) -> TypeId {
        let Expr::Call {
            callee,
            args,
            type_args,
            question_dot,
            span,
        } = e
        else {
            unreachable!()
        };
        if matches!(&**callee, Expr::Super { .. }) {
            if let Some(targs) = type_args {
                if let Some(t0) = targs.first() {
                    let s = t0.span().start as usize;
                    self.error_at(
                        Span::new(s - 1, s),
                        &gen::super_may_not_use_type_arguments,
                        &[],
                    );
                }
            }
        }
        if type_args.is_some() {
            let ct = self.check_call_callee_expr(callee);
            if matches!(self.types.kind(ct), TypeKind::Any) {
                for a in args {
                    self.check_expr(a, None);
                }
                self.error_at(
                    *span,
                    &gen::Untyped_function_calls_may_not_accept_type_arguments,
                    &[],
                );
                return self.types.any;
            }
        }
        // super(...) resolves against the base class constructor
        if matches!(&**callee, Expr::Super { .. }) {
            if let Some(base_statics) = self.current_class_base_statics() {
                let sigs = self.ctor_signatures_of(base_statics);
                if let Some(sig) =
                    self.select_construct_signature(&sigs, args, type_args.as_deref())
                {
                    self.resolve_call(sig, args, type_args.as_deref(), *span, e, ctx);
                    return self.types.void;
                }
            }
            // 2335 reported by checking the callee as an expression
            self.check_expr(callee, None);
            for a in args {
                self.check_expr(a, None);
            }
            return self.types.void;
        }
        let mut callee_t = self.check_call_callee_expr(callee);
        if self.types.is_error(callee_t) {
            for a in args {
                self.check_expr(a, None);
            }
            return self.types.error;
        }
        if *question_dot || expr_contains_optional_chain(callee) {
            callee_t = self.non_nullable(callee_t);
        } else if self.options.strict_null_checks() {
            let members = self.types.union_members(callee_t);
            let nullish = members
                .iter()
                .any(|&m| matches!(self.types.kind(m), TypeKind::Null | TypeKind::Undefined));
            if nullish {
                let non_null = self.non_nullable(callee_t);
                let prefer_get_accessor_not_callable = self.call_signatures_of(non_null).is_empty()
                    && self.get_accessor_call_name_span(callee).is_some();
                if !prefer_get_accessor_not_callable {
                    callee_t = self.check_non_nullish(callee_t, callee, true);
                    if self.types.is_error(callee_t) {
                        for a in args {
                            self.check_expr(a, None);
                        }
                        return self.types.error;
                    }
                }
            }
        }
        if matches!(self.types.kind(callee_t), TypeKind::Unknown) {
            let t2 = self.check_non_nullish(callee_t, callee, false);
            let _ = t2;
            for a in args {
                self.check_expr(a, None);
            }
            return self.types.error;
        }
        if self.types.is_any_or_error(callee_t) {
            for a in args {
                self.check_expr(a, None);
            }
            return self.types.any;
        }
        // unions where every member is callable combine into one signature
        // with intersected parameters (string & number → never)
        if let TypeKind::Union(members) = self.types.kind(callee_t).clone() {
            let member_sigs: Vec<Vec<crate::types::SigId>> = members
                .iter()
                .map(|&m| self.call_signatures_of(m))
                .collect();
            if member_sigs.iter().all(|s| s.len() == 1) {
                let sigs: Vec<crate::types::Signature> = member_sigs
                    .iter()
                    .map(|s| self.types.sig(s[0]).clone())
                    .collect();
                let max_params = sigs.iter().map(|s| s.params.len()).max().unwrap_or(0);
                let mut params: Vec<crate::types::ParamInfo> = Vec::new();
                for i in 0..max_params {
                    let mut combined: Option<TypeId> = None;
                    for s in &sigs {
                        let pt = s
                            .params
                            .get(i)
                            .map(|p| p.ty)
                            .or(s.rest)
                            .unwrap_or(self.types.any);
                        combined = Some(match combined {
                            None => pt,
                            Some(c) => {
                                if c == pt || self.is_assignable_to(pt, c) {
                                    pt
                                } else if self.is_assignable_to(c, pt) {
                                    c
                                } else {
                                    self.types.never
                                }
                            }
                        });
                    }
                    params.push(crate::types::ParamInfo {
                        name: sigs[0]
                            .params
                            .get(i)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| format!("arg{}", i)),
                        ty: combined.unwrap_or(self.types.any),
                        decl_span: sigs[0].params.get(i).and_then(|p| p.decl_span),
                        decl_file: sigs[0].params.get(i).map(|p| p.decl_file).unwrap_or(0),
                        optional: sigs
                            .iter()
                            .any(|s| s.params.get(i).map(|p| p.optional).unwrap_or(true)),
                    });
                }
                let min_args = sigs.iter().map(|s| s.min_args).max().unwrap_or(0);
                let rets: Vec<TypeId> = sigs.iter().map(|s| s.ret).collect();
                let ret = self.types.union(rets);
                let combined_sig = self.types.alloc_sig(crate::types::Signature {
                    type_params: Vec::new(),
                    params,
                    min_args,
                    rest: None,
                    rest_name: None,
                    rest_tp: None,
                    ret,
                    decl_key: 0,
                    from_method: sigs.iter().any(|s| s.from_method),
                    ret_annotation_never: false,
                    predicate: None,
                    is_abstract: false,
                });
                return self.resolve_call(combined_sig, args, type_args.as_deref(), *span, e, ctx);
            }
        }
        // collect call signatures
        let sigs = self.call_signatures_of(callee_t);
        if sigs.is_empty() {
            self.report_not_callable(callee, callee_t, false);
            for a in args {
                self.check_expr(a, None);
            }
            return self.types.error;
        }
        if sigs.len() > 1 {
            let impl_info = match self.types.kind(callee_t) {
                TypeKind::Anon(sid) => {
                    let sid = *sid;
                    self.caches.overload_impl.get(&sid).copied()
                }
                _ => None,
            };
            return self.resolve_overloaded_call(
                &sigs,
                args,
                type_args.as_deref(),
                *span,
                impl_info,
                ctx,
            );
        }
        self.resolve_call(sigs[0], args, type_args.as_deref(), *span, e, ctx)
    }

    pub fn call_signatures_of(&mut self, t: TypeId) -> Vec<SigId> {
        let ap = self.apparent_type(t);
        match self.shape_of_type(ap) {
            Some(sid) => self.types.shape(sid).call_sigs.clone(),
            None => Vec::new(),
        }
    }

    pub(crate) fn ctor_signatures_of(&mut self, t: TypeId) -> Vec<SigId> {
        let ap = self.apparent_type(t);
        if let TypeKind::Intersection(members) = self.types.kind(ap).clone() {
            return self.intersection_ctor_signatures(&members);
        }
        match self.shape_of_type(ap) {
            Some(sid) => {
                let sigs = self.types.shape(sid).ctor_sigs.clone();
                self.normalize_mixin_constructor_sigs(sigs)
            }
            None => Vec::new(),
        }
    }

    fn intersection_ctor_signatures(&mut self, members: &[TypeId]) -> Vec<SigId> {
        let mut base_sigs: Vec<SigId> = Vec::new();
        let mut mixin_sigs: Vec<SigId> = Vec::new();
        for &m in members {
            let ap = self.apparent_type(m);
            let Some(sid) = self.shape_of_type(ap) else {
                continue;
            };
            let sigs = self.types.shape(sid).ctor_sigs.clone();
            for sig in sigs {
                if self.is_mixin_constructor_sig(sig) {
                    mixin_sigs.push(sig);
                } else {
                    base_sigs.push(sig);
                }
            }
        }
        self.combine_mixin_constructor_sigs(base_sigs, mixin_sigs)
    }

    fn normalize_mixin_constructor_sigs(&mut self, sigs: Vec<SigId>) -> Vec<SigId> {
        let mut base_sigs = Vec::new();
        let mut mixin_sigs = Vec::new();
        for sig in sigs {
            if self.is_mixin_constructor_sig(sig) {
                mixin_sigs.push(sig);
            } else {
                base_sigs.push(sig);
            }
        }
        self.combine_mixin_constructor_sigs(base_sigs, mixin_sigs)
    }

    fn combine_mixin_constructor_sigs(
        &mut self,
        base_sigs: Vec<SigId>,
        mixin_sigs: Vec<SigId>,
    ) -> Vec<SigId> {
        if mixin_sigs.is_empty() {
            return base_sigs;
        }

        let mixin_returns: Vec<TypeId> =
            mixin_sigs.iter().map(|&sig| self.sig_return(sig)).collect();
        if base_sigs.is_empty() {
            let ret = self.intersect_all(mixin_returns);
            let first = self.types.sig(mixin_sigs[0]).clone();
            let mut sig = first;
            sig.ret = ret;
            sig.type_params.clear();
            sig.params.clear();
            sig.min_args = 0;
            sig.rest = Some(self.types.any);
            sig.rest_name = Some("args".to_string());
            sig.rest_tp = None;
            sig.decl_key = 0;
            sig.predicate = None;
            return vec![self.types.alloc_sig(sig)];
        }

        let mut out = Vec::with_capacity(base_sigs.len());
        for base in base_sigs {
            let mut returns = Vec::with_capacity(mixin_returns.len() + 1);
            returns.push(self.sig_return(base));
            returns.extend(mixin_returns.iter().copied());
            let ret = self.intersect_all(returns);
            let mut sig = self.types.sig(base).clone();
            sig.ret = ret;
            sig.decl_key = 0;
            sig.predicate = None;
            out.push(self.types.alloc_sig(sig));
        }
        out
    }

    fn is_mixin_constructor_sig(&self, sig: SigId) -> bool {
        let sig = self.types.sig(sig);
        sig.params.is_empty()
            && sig.min_args == 0
            && sig
                .rest
                .map(|r| matches!(self.types.kind(r), TypeKind::Any))
                .unwrap_or(false)
    }

    fn select_construct_signature(
        &mut self,
        sigs: &[SigId],
        args: &'a [Expr],
        type_args: Option<&'a [TypeNode]>,
    ) -> Option<SigId> {
        let first = sigs.first().copied()?;
        if let Some(targs) = type_args {
            let matching: Vec<SigId> = sigs
                .iter()
                .copied()
                .filter(|&sig| self.types.sig(sig).type_params.len() == targs.len())
                .collect();
            for sig in matching.iter().copied() {
                if self.construct_signature_applicable_with_type_args(sig, args, targs) {
                    return Some(sig);
                }
            }
            return matching.first().copied().or(Some(first));
        }
        for &sig in sigs {
            if self.construct_signature_applicable(sig, args) {
                return Some(sig);
            }
        }
        Some(first)
    }

    fn construct_signature_applicable(&mut self, sig: SigId, args: &'a [Expr]) -> bool {
        let s = self.types.sig(sig).clone();
        let argc = args.len() as u32;
        let max = s.params.len() as u32;
        if argc < s.min_args || (s.rest.is_none() && argc > max) {
            return false;
        }
        let mapper = if s.type_params.is_empty() {
            HashMap::new()
        } else {
            self.infer_type_arguments(&s, args, None)
        };
        for (i, arg) in args.iter().enumerate() {
            if self.arg_needs_recheck(arg) {
                continue;
            }
            let at = self.check_expr(arg, None);
            let pt0 = s
                .params
                .get(i)
                .map(|p| p.ty)
                .or(s.rest)
                .unwrap_or(self.types.any);
            let pt = self.instantiate_type(pt0, &mapper);
            if !self.is_assignable_to(at, pt) {
                return false;
            }
        }
        true
    }

    fn construct_signature_applicable_with_type_args(
        &mut self,
        sig: SigId,
        args: &'a [Expr],
        type_args: &'a [TypeNode],
    ) -> bool {
        let s = self.types.sig(sig).clone();
        if s.type_params.len() != type_args.len() {
            return false;
        }
        let argc = args.len() as u32;
        let max = s.params.len() as u32;
        if argc < s.min_args || (s.rest.is_none() && argc > max) {
            return false;
        }
        let scope = self.current_scope;
        let mut mapper = HashMap::new();
        for (i, &tp) in s.type_params.iter().enumerate() {
            mapper.insert(tp, self.resolve_type(&type_args[i], scope));
        }
        for (i, arg) in args.iter().enumerate() {
            if self.arg_needs_recheck(arg) {
                continue;
            }
            let at = self.check_expr(arg, None);
            let pt0 = s
                .params
                .get(i)
                .map(|p| p.ty)
                .or(s.rest)
                .unwrap_or(self.types.any);
            let pt = self.instantiate_type(pt0, &mapper);
            if !self.is_assignable_to(at, pt) {
                return false;
            }
        }
        true
    }

    fn report_not_callable(&mut self, callee: &'a Expr, t: TypeId, is_new: bool) {
        if is_new {
            // unions: some constituents constructable → dedicated chain
            if let TypeKind::Union(members) = self.types.kind(t).clone() {
                let ctorable: Vec<bool> = members
                    .iter()
                    .map(|&m| !self.ctor_signatures_of(m).is_empty())
                    .collect();
                if ctorable.iter().any(|&c| c) {
                    let d = self.display_type(t);
                    let mut chain = crate::diagnostics::MessageChain::new(
                        &gen::This_expression_is_not_constructable,
                        &[],
                    );
                    let mut mid = crate::diagnostics::MessageChain::new(
                        &gen::Not_all_constituents_of_type_0_are_constructable,
                        &[d],
                    );
                    if let Some(idx) = ctorable.iter().position(|&c| !c) {
                        let md = self.display_type(members[idx]);
                        mid.next.push(crate::diagnostics::MessageChain::new(
                            &gen::Type_0_has_no_construct_signatures,
                            &[md],
                        ));
                    }
                    chain.next.push(mid);
                    self.error_chain_at(callee.span(), chain);
                    return;
                }
            }
            // `new` on a plain function (the legacy JS constructor pattern)
            // is an implicit any, not a hard error
            if !self.call_signatures_of(t).is_empty() {
                if self.options.no_implicit_any() {
                    self.error_at(
                        callee.span(),
                        &gen::new_expression_whose_target_lacks_a_construct_signature_implicitly_has_an_any_type,
                        &[],
                    );
                }
                return;
            }
            let d = self.apparent_type_display(t);
            let mut chain = crate::diagnostics::MessageChain::new(
                &gen::This_expression_is_not_constructable,
                &[],
            );
            chain.next.push(crate::diagnostics::MessageChain::new(
                &gen::Type_0_has_no_construct_signatures,
                &[d],
            ));
            self.error_chain_at(callee.span(), chain);
            return;
        }
        // calling a non-callable get-accessor result (6234)
        if let Some(name_span) = self.get_accessor_call_name_span(callee) {
            if !self.options.strict_null_checks() && self.is_nullish_only_type(t) {
                return;
            }
            let dsp = self.apparent_type_display(t);
            let mut chain = crate::diagnostics::MessageChain::new(
                &gen::This_expression_is_not_callable_because_it_is_a_get_accessor_Did_you_mean_to_use_it_without,
                &[],
            );
            chain.next.push(crate::diagnostics::MessageChain::new(
                &gen::Type_0_has_no_call_signatures,
                &[dsp],
            ));
            self.error_chain_at(name_span, chain);
            return;
        }
        // class value (or any constructable) called without new?
        if matches!(
            self.types.kind(t),
            TypeKind::ClassStatics(_) | TypeKind::MappedClassStatics(_, _)
        ) || !self.ctor_signatures_of(t).is_empty()
        {
            let d = self.display_type(t);
            self.error_at(
                callee.span(),
                &gen::Value_of_type_0_is_not_callable_Did_you_mean_to_include_new,
                &[d],
            );
            return;
        }
        if let TypeKind::Union(members) = self.types.kind(t).clone() {
            let callable: Vec<bool> = members
                .iter()
                .map(|&m| !self.call_signatures_of(m).is_empty())
                .collect();
            let d = self.display_type(t);
            let mut chain =
                crate::diagnostics::MessageChain::new(&gen::This_expression_is_not_callable, &[]);
            if callable.iter().any(|&c| c) {
                chain.next.push(crate::diagnostics::MessageChain::new(
                    &gen::Not_all_constituents_of_type_0_are_callable,
                    &[d],
                ));
                // first non-callable member
                if let Some(idx) = callable.iter().position(|&c| !c) {
                    let md = self.apparent_type_display(members[idx]);
                    let md = if matches!(
                        self.types.kind(members[idx]),
                        TypeKind::String
                            | TypeKind::StrLit(_)
                            | TypeKind::Number
                            | TypeKind::NumLit(_)
                    ) {
                        self.display_type(members[idx])
                    } else {
                        md
                    };
                    chain.next[0]
                        .next
                        .push(crate::diagnostics::MessageChain::new(
                            &gen::Type_0_has_no_call_signatures,
                            &[md],
                        ));
                }
            } else {
                chain.next.push(crate::diagnostics::MessageChain::new(
                    &gen::No_constituent_of_type_0_is_callable,
                    &[d],
                ));
            }
            self.error_chain_at(callee.span(), chain);
            return;
        }
        let d = self.apparent_type_display(t);
        let mut chain =
            crate::diagnostics::MessageChain::new(&gen::This_expression_is_not_callable, &[]);
        chain.next.push(crate::diagnostics::MessageChain::new(
            &gen::Type_0_has_no_call_signatures,
            &[d],
        ));
        self.error_chain_at(callee.span(), chain);
    }

    fn is_nullish_only_type(&self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::Null | TypeKind::Undefined | TypeKind::Void => true,
            TypeKind::Union(members) => {
                !members.is_empty() && members.iter().all(|&m| self.is_nullish_only_type(m))
            }
            _ => false,
        }
    }

    fn get_accessor_call_name_span(&mut self, callee: &'a Expr) -> Option<Span> {
        let Expr::PropAccess { obj, name, .. } = callee else {
            return None;
        };
        let obj_t = self.check_expr(obj, None);
        if self.types.is_error(obj_t) {
            return None;
        }
        let member = self.prop_info_of_type(obj_t, &name.name)?.symbol?;
        if self.symbol(member).flags & flags::GET_ACCESSOR != 0 {
            Some(name.span)
        } else {
            None
        }
    }

    pub(crate) fn check_new(&mut self, e: &'a Expr, ctx: Option<TypeId>) -> TypeId {
        let Expr::New {
            callee,
            args,
            type_args,
            span,
        } = e
        else {
            unreachable!()
        };
        let callee_t = self.check_expr(callee, None);
        if self.types.is_any_or_error(callee_t) {
            if let Some(args) = args {
                for a in args {
                    self.check_expr(a, None);
                }
            }
            return self.types.any;
        }
        // abstract class?
        let class_static_sym = match self.types.kind(callee_t).clone() {
            TypeKind::ClassStatics(sym) | TypeKind::MappedClassStatics(sym, _) => Some(sym),
            _ => None,
        };
        if let Some(sym) = class_static_sym {
            let is_abstract = self
                .symbol(sym)
                .decls
                .iter()
                .any(|d| matches!(d, crate::binder::Decl::Class(c) if has_modifier(&c.modifiers, ModifierKind::Abstract)));
            if is_abstract {
                self.error_at(
                    *span,
                    &gen::Cannot_create_an_instance_of_an_abstract_class,
                    &[],
                );
                return self.types.error;
            }
            // private/protected constructors
            let mut ctor_access: Option<ModifierKind> = None;
            for d in self.symbol(sym).decls.clone() {
                if let crate::binder::Decl::Class(c) = d {
                    for m in &c.members {
                        if let ClassMember::Constructor(f) = m {
                            if has_modifier(&f.modifiers, ModifierKind::Private) {
                                ctor_access = Some(ModifierKind::Private);
                            } else if has_modifier(&f.modifiers, ModifierKind::Protected) {
                                ctor_access = Some(ModifierKind::Protected);
                            }
                        }
                    }
                }
            }
            if let Some(acc) = ctor_access {
                let inside = self.stacks.class_stack.contains(&sym);
                if !inside {
                    let cn = self.symbol(sym).name.clone();
                    let msg: &'static DiagnosticMessage = if acc == ModifierKind::Private {
                        &gen::Constructor_of_class_0_is_private_and_only_accessible_within_the_class_declaration
                    } else {
                        &gen::Constructor_of_class_0_is_protected_and_only_accessible_within_the_class_declaration
                    };
                    self.error_at(*span, msg, &[cn]);
                    return self.types.error;
                }
            }
        }
        let sigs = self.ctor_signatures_of(callee_t);
        // A type that isn't a bare ClassStatics but whose construct signatures
        // are all abstract — e.g. a mixin intersection over an abstract base
        // (`typeof AbstractBase & (abstract new (...) => Mixin)`) — still cannot
        // be instantiated (2511). An abstract `ClassStatics` is already handled
        // above and returned early, so this never double-reports.
        if !sigs.is_empty() && sigs.iter().all(|&s| self.types.sig(s).is_abstract) {
            self.error_at(
                *span,
                &gen::Cannot_create_an_instance_of_an_abstract_class,
                &[],
            );
            return self.types.error;
        }
        if sigs.is_empty() {
            // call signatures only → 7009 under noImplicitAny
            let has_call = !self.call_signatures_of(callee_t).is_empty();
            if has_call && self.options.no_implicit_any() {
                self.error_at(
                    *span,
                    &gen::new_expression_whose_target_lacks_a_construct_signature_implicitly_has_an_any_type,
                    &[],
                );
                if let Some(args) = args {
                    for a in args {
                        self.check_expr(a, None);
                    }
                }
                return self.types.any;
            }
            self.report_not_callable(callee, callee_t, true);
            if let Some(args) = args {
                for a in args {
                    self.check_expr(a, None);
                }
            }
            return self.types.error;
        }
        let args_slice: &'a [Expr] = args.as_deref().unwrap_or(&[]);
        if sigs.len() > 1 {
            let sig = self
                .select_construct_signature(&sigs, args_slice, type_args.as_deref())
                .unwrap_or(sigs[0]);
            return self.resolve_call_with_options(
                sig,
                args_slice,
                type_args.as_deref(),
                *span,
                e,
                ctx,
                true,
            );
        }
        self.resolve_call(sigs[0], args_slice, type_args.as_deref(), *span, e, ctx)
    }

    /// overloaded calls: first applicable signature wins; none → 2769 chain
    fn resolve_overloaded_call(
        &mut self,
        sigs: &[SigId],
        args: &'a [Expr],
        _type_args: Option<&'a [TypeNode]>,
        call_span: Span,
        impl_info: Option<(SigId, u32, u32, usize)>,
        ctx: Option<TypeId>,
    ) -> TypeId {
        // tsc getTypeArgumentArityError: when no overload accepts the
        // when every overload fails on arity alone, report 2554 with a range;
        // Pre-check non-arrow arguments once. Function-like arguments are
        // checked per candidate against the (instantiated) parameter type, so
        // their bodies get contextual parameter types instead of implicit-any.
        let arg_types: Vec<Option<TypeId>> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                if matches!(a, Expr::Arrow(_) | Expr::FunctionExpr(_)) {
                    None
                } else {
                    let has_contextual_param = sigs.iter().any(|&sig| {
                        let s = self.types.sig(sig);
                        s.params.get(i).is_some() || s.rest.is_some()
                    });
                    let count = self.contextual_function_arg_count(a);
                    let after_conditional_function_arg = self.options.no_implicit_any()
                        && args[..i]
                            .iter()
                            .any(|prev| self.is_conditional_function_arg(prev));
                    let t = if has_contextual_param && count > 0 && !after_conditional_function_arg
                    {
                        self.with_suppressed_next_n_function_implicit_any_params(count, |this| {
                            this.check_expr(a, None)
                        })
                    } else {
                        self.check_expr(a, None)
                    };
                    Some(t)
                }
            })
            .collect();
        let mut first_failed_arg_index: Option<usize> = None;
        for &sig in sigs {
            let trial = self.call_candidate_trial(sig, args, &arg_types, ctx);
            if !trial.arity_ok {
                continue;
            }
            if let Some(i) = trial.first_failed_arg_index {
                first_failed_arg_index =
                    Some(first_failed_arg_index.map_or(i, |prev| std::cmp::min(prev, i)));
            }
            if trial.non_function_args_ok {
                let s = self.types.sig(trial.sig).clone();
                // This overload fits the non-arrow arguments; commit to it and
                // check any function-like arguments with their contextual
                // parameter types (also emits their body diagnostics once).
                for (i, a) in args.iter().enumerate() {
                    if matches!(a, Expr::Arrow(_) | Expr::FunctionExpr(_)) {
                        let pt0 = s
                            .params
                            .get(i)
                            .map(|p| p.ty)
                            .or(s.rest)
                            .unwrap_or(self.types.any);
                        let pt = self.instantiate_type(pt0, &trial.mapper);
                        let at = self.check_expr(a, Some(pt));
                        let _ = self.check_assignable(at, pt, a.span(), None, Some(a));
                    }
                }
                return self.instantiate_type(s.ret, &trial.mapper);
            }
        }
        // No overload accepted the arguments. Finalize argument types —
        // function-like arguments not yet checked are checked now (with no
        // contextual type) so their diagnostics surface and the chain can
        // display them.
        let mut arg_types_final: Vec<TypeId> = Vec::with_capacity(args.len());
        for (i, at) in arg_types.iter().enumerate() {
            let recheck_after_failed_contextual_arg =
                first_failed_arg_index.is_some_and(|failed_i| {
                    i > failed_i && self.contextual_function_arg_count(&args[i]) > 0
                }) && self.options.no_implicit_any();
            match at {
                Some(t) if !recheck_after_failed_contextual_arg => arg_types_final.push(*t),
                Some(_) => {
                    self.clear_function_like_expr_check_cache(&args[i]);
                    let t = self.check_expr(&args[i], None);
                    arg_types_final.push(t);
                }
                None => {
                    self.clear_function_like_expr_check_cache(&args[i]);
                    let t = self.check_expr(&args[i], None);
                    arg_types_final.push(t);
                }
            }
        }
        let arg_types = arg_types_final;
        // 2769: No overload matches this call.
        let mut head =
            crate::diagnostics::MessageChain::new(&gen::No_overload_matches_this_call, &[]);
        let n = sigs.len();
        let mut error_span = call_span;
        for (idx, &sig) in sigs.iter().enumerate() {
            let s = self.types.sig(sig).clone();
            let sig_display = self.display_sig_for_overload(sig);
            let mut over = crate::diagnostics::MessageChain::new(
                &gen::Overload_0_of_1_2_gave_the_following_error,
                &[(idx + 1).to_string(), n.to_string(), sig_display],
            );
            let argc = args.len() as u32;
            let max = s.params.len() as u32;
            if argc < s.min_args || (s.rest.is_none() && argc > max) {
                over.next.push(crate::diagnostics::MessageChain::new(
                    &gen::Expected_0_arguments_but_got_1,
                    &[expected_args_display(&s), argc.to_string()],
                ));
            } else {
                let mapper: HashMap<SymbolId, TypeId> = if s.type_params.is_empty() {
                    HashMap::new()
                } else {
                    self.infer_type_arguments(&s, args, ctx)
                };
                for (i, &at) in arg_types.iter().enumerate() {
                    let pt0 = s
                        .params
                        .get(i)
                        .map(|p| p.ty)
                        .or(s.rest)
                        .unwrap_or(self.types.any);
                    let pt = self.instantiate_type(pt0, &mapper);
                    if !self.is_assignable_to(at, pt) {
                        if idx == 0 {
                            error_span = args[i].span();
                        }
                        let ad = self.display_type_for_error(at, pt);
                        let pd = self.display_type(pt);
                        over.next.push(crate::diagnostics::MessageChain::new(
                            &gen::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1,
                            &[ad, pd],
                        ));
                        break;
                    }
                }
            }
            head.next.push(over);
        }
        self.error_chain_at(error_span, head);
        // TS2793: if the call would have succeeded against the hidden
        // implementation signature, point at it (overloads hide the impl).
        if let Some((isig, istart, ilen, ifile)) = impl_info {
            let s = self.types.sig(isig).clone();
            let argc = args.len() as u32;
            let max = s.params.len() as u32;
            let mut ok = argc >= s.min_args && (s.rest.is_some() || argc <= max);
            if ok {
                for (i, &at) in arg_types.iter().enumerate() {
                    let pt = s
                        .params
                        .get(i)
                        .map(|p| p.ty)
                        .or(s.rest)
                        .unwrap_or(self.types.any);
                    if !self.is_assignable_to(at, pt) {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                let ri = crate::diagnostics::RelatedInfo {
                    file: Some(ifile),
                    start: istart,
                    length: ilen,
                    message: crate::diagnostics::MessageChain::new(
                        &gen::The_call_would_have_succeeded_against_this_implementation_but_implementation_signatures_of_overloads_are_not_externally_visible,
                        &[],
                    ),
                };
                if let Some(d) = self.diags.last_mut() {
                    d.related.push(ri);
                }
            }
        }
        self.types.error
    }

    fn call_candidate_trial(
        &mut self,
        sig: SigId,
        args: &'a [Expr],
        arg_types: &[Option<TypeId>],
        ctx: Option<TypeId>,
    ) -> CallCandidateTrial {
        let s = self.types.sig(sig).clone();
        let argc = args.len() as u32;
        let max = s.params.len() as u32;
        let arity_ok = argc >= s.min_args && (s.rest.is_some() || argc <= max);
        if !arity_ok {
            return CallCandidateTrial {
                sig,
                mapper: HashMap::new(),
                arity_ok,
                non_function_args_ok: false,
                first_failed_arg_index: None,
            };
        }

        // Infer type arguments for generic overloads from the arguments.
        let mapper: HashMap<SymbolId, TypeId> = if s.type_params.is_empty() {
            HashMap::new()
        } else {
            self.infer_type_arguments(&s, args, ctx)
        };

        for (i, at) in arg_types.iter().enumerate() {
            let Some(at) = at else { continue }; // arrows checked after selection
            let pt0 = s
                .params
                .get(i)
                .map(|p| p.ty)
                .or(s.rest)
                .unwrap_or(self.types.any);
            let pt = self.instantiate_type(pt0, &mapper);
            if !self.is_assignable_to(*at, pt) {
                return CallCandidateTrial {
                    sig,
                    mapper,
                    arity_ok,
                    non_function_args_ok: false,
                    first_failed_arg_index: Some(i),
                };
            }
        }

        CallCandidateTrial {
            sig,
            mapper,
            arity_ok,
            non_function_args_ok: true,
            first_failed_arg_index: None,
        }
    }

    fn display_sig_for_overload(&mut self, sig: SigId) -> String {
        // '(a: number): void'
        let s = self.types.sig(sig).clone();
        let type_params = self.display_sig_type_params(sig);
        let mut parts: Vec<String> = Vec::new();
        for p in &s.params {
            let ty = self.display_type(p.ty);
            parts.push(format!(
                "{}{}: {}",
                p.name,
                if p.optional { "?" } else { "" },
                ty
            ));
        }
        if let Some(rest) = s.rest {
            let ty = self.display_type(rest);
            let name = s.rest_name.as_deref().unwrap_or("args");
            parts.push(format!("...{}: {}[]", name, ty));
        }
        let ret = self.sig_return(sig);
        let rd = self.display_type(ret);
        format!("{}({}): {}", type_params, parts.join(", "), rd)
    }

    /// generalized display for an argument type against a parameter target
    pub(crate) fn display_type_for_error(&mut self, src: TypeId, tgt: TypeId) -> String {
        let is_lit = matches!(
            self.types.kind(self.types.regular(src)),
            TypeKind::StrLit(_)
                | TypeKind::NumLit(_)
                | TypeKind::BigIntLit(_)
                | TypeKind::BoolLit(_)
        ) || self.types.regular(src) == self.types.boolean;
        if is_lit && !matches!(self.types.kind(tgt), TypeKind::Never) {
            let could_keep = self.type_could_keep_literal(tgt);
            if !could_keep {
                let r = self.types.regular(src);
                let w = self.types.widen_literal(r);
                return self.display_type(w);
            }
        }
        let r = self.types.regular(src);
        self.display_type(r)
    }

    /// signature application: explicit type args / inference, arity, per-arg checks
    fn resolve_call(
        &mut self,
        sig: SigId,
        args: &'a [Expr],
        type_args: Option<&'a [TypeNode]>,
        call_span: Span,
        call_expr: &'a Expr,
        ctx: Option<TypeId>,
    ) -> TypeId {
        self.resolve_call_with_options(sig, args, type_args, call_span, call_expr, ctx, false)
    }

    fn resolve_call_with_options(
        &mut self,
        sig: SigId,
        args: &'a [Expr],
        type_args: Option<&'a [TypeNode]>,
        call_span: Span,
        call_expr: &'a Expr,
        ctx: Option<TypeId>,
        stop_after_first_arg_error: bool,
    ) -> TypeId {
        let s = self.types.sig(sig).clone();
        let mut mapper: HashMap<SymbolId, TypeId> = HashMap::new();

        // An explicit `this` parameter must be satisfiable by the call's `this`
        // context. A free call (`f()`, not `obj.f()`) supplies a `void`/
        // `undefined` `this`, so calling a function that requires a non-void
        // `this` is TS2684.
        if let Some(&declared) = self.caches.sig_this_ty.get(&sig) {
            if let Expr::Call { callee, .. } = call_expr {
                if matches!(&**callee, Expr::Ident(_)) {
                    let void_t = self.types.void;
                    let skip = self.types.is_any_or_error(declared)
                        || matches!(
                            self.types.kind(declared),
                            TypeKind::Unknown | TypeKind::Void | TypeKind::Undefined
                        );
                    if !skip && !self.is_assignable_to(void_t, declared) {
                        let dt = self.display_type(declared);
                        self.error_at(
                            call_span,
                            &gen::The_this_context_of_type_0_is_not_assignable_to_method_s_this_of_type_1,
                            &["void".to_string(), dt],
                        );
                    }
                }
            }
        }

        if !s.type_params.is_empty() {
            if let Some(targs) = type_args {
                if targs.len() != s.type_params.len() {
                    let span = targs.first().map(|t| t.span()).unwrap_or(call_span);
                    self.error_at(
                        span,
                        &gen::Expected_0_type_arguments_but_got_1,
                        &[s.type_params.len().to_string(), targs.len().to_string()],
                    );
                    return self.types.error;
                }
                let scope = self.current_scope;
                for (i, &tp) in s.type_params.iter().enumerate() {
                    let at = self.resolve_type(&targs[i], scope);
                    mapper.insert(tp, at);
                }
                // constraint checks
                for (i, &tp) in s.type_params.iter().enumerate() {
                    if let Some(c) = self.constraint_of_type_param(tp) {
                        let c = self.instantiate_type(c, &mapper);
                        let at = mapper[&tp];
                        if !self.is_assignable_to(at, c) && !self.types.is_any_or_error(at) {
                            let ad = self.display_type(at);
                            let cd = self.display_type(c);
                            self.error_at(
                                targs[i].span(),
                                &gen::Type_0_does_not_satisfy_the_constraint_1,
                                &[ad, cd],
                            );
                        }
                    }
                }
            } else {
                mapper = self.infer_type_arguments(&s, args, ctx);
            }
        }

        // arity
        let has_spread = args.iter().any(|a| matches!(a, Expr::Spread { .. }));
        if !has_spread {
            let argc = args.len() as u32;
            let max = s.params.len() as u32;
            if argc < s.min_args || (s.rest.is_none() && argc > max) {
                if s.rest.is_some() {
                    self.error_at(
                        call_span,
                        &gen::Expected_at_least_0_arguments_but_got_1,
                        &[s.min_args.to_string(), argc.to_string()],
                    );
                } else if argc > max {
                    // span: first extra argument
                    let span = args[max as usize].span();
                    self.error_at(
                        span,
                        &gen::Expected_0_arguments_but_got_1,
                        &[expected_args_display(&s), argc.to_string()],
                    );
                } else {
                    self.error_at(
                        call_span,
                        &gen::Expected_0_arguments_but_got_1,
                        &[expected_args_display(&s), argc.to_string()],
                    );
                    // tsc adds "An argument for '<param>' was not provided." (TS6210)
                    // pointing at the first parameter that received no argument.
                    if let Some(p) = s.params.get(argc as usize) {
                        if let Some(sp) = p.decl_span {
                            let ri = crate::diagnostics::RelatedInfo {
                                file: Some(p.decl_file),
                                start: sp.start,
                                length: sp.len(),
                                message: crate::diagnostics::MessageChain::new(
                                    &gen::An_argument_for_0_was_not_provided,
                                    &[p.name.clone()],
                                ),
                            };
                            if let Some(d) = self.diags.last_mut() {
                                d.related.push(ri);
                            }
                        }
                    }
                }
            }
        } else {
            // spread argument must be a tuple or target a rest param
            for a in args {
                if let Expr::Spread { expr, span } = a {
                    let t = self.check_expr(expr, None);
                    let is_tuple = matches!(self.types.kind(t), TypeKind::Tuple(_));
                    if !is_tuple && s.rest.is_none() {
                        self.error_at(
                            *span,
                            &gen::A_spread_argument_must_either_have_a_tuple_type_or_be_passed_to_a_rest_parameter,
                            &[],
                        );
                    }
                }
            }
        }

        // per-argument checks
        let mut arg_i = 0usize;
        let mut suppress_arg_assignability = false;
        for a in args {
            if matches!(a, Expr::Spread { .. }) {
                arg_i += 1;
                continue;
            }
            let param_ty = s
                .params
                .get(arg_i)
                .map(|p| p.ty)
                .or(s.rest)
                .unwrap_or(self.types.any);
            let param_ty = self.instantiate_type(param_ty, &mapper);
            if suppress_arg_assignability {
                self.check_expr(a, None);
                arg_i += 1;
                continue;
            }
            // A context-sensitive function argument (an arrow / function
            // expression with non-annotated parameters) was first checked during
            // inference with provisional type arguments, so its parameter types
            // may reflect an intermediate inferred type. Drop its cached type so
            // it is re-evaluated against the final parameter type; its signature
            // is then rebuilt from the (re-established) contextual parameter
            // types. The body is not re-checked — `checked_decls` is left set —
            // so this re-evaluation emits no duplicate diagnostics, it only
            // refreshes the argument's parameter types for the assignability
            // check below.
            if !s.type_params.is_empty() && type_args.is_none() && self.arg_needs_recheck(a) {
                self.caches.expr_type_cache.remove(&node_key_expr(a));
                self.drop_nested_objlit_caches(a);
            }
            let at = self.check_expr(a, Some(param_ty));
            if !self.types.is_error(at) && !self.types.is_error(param_ty) {
                if !self.is_assignable_to(at, param_ty) {
                    self.check_assignable(
                        at,
                        param_ty,
                        a.span(),
                        Some((
                            &gen::Argument_of_type_0_is_not_assignable_to_parameter_of_type_1,
                            Vec::new(),
                        )),
                        Some(a),
                    );
                    if stop_after_first_arg_error {
                        suppress_arg_assignability = true;
                    }
                }
            }
            arg_i += 1;
        }
        let _ = call_expr;
        let ret = self.sig_return(sig);
        self.instantiate_type(ret, &mapper)
    }

    /// tsc-style inference: candidates per type param; literals kept when the
    /// param occurs at top level of the return type; common supertype keep-first.
    /// A function-expression argument is *context-sensitive* when one of its
    /// parameters has neither a type annotation nor an initializer: that
    /// parameter's type can only come from the contextual (parameter) type, so
    /// the argument cannot be checked until the relevant type parameters are
    /// inferred. Context-sensitive arguments are deferred to a second inference
    /// pass; a fully-annotated function expression is *not* context-sensitive
    /// and participates in the first pass, letting its return type constrain
    /// type parameters before the deferred arguments are contextually typed
    /// (e.g. `pipe(f, g)` infers `B` from `f`'s return so `g`'s parameter is
    /// typed as `B` rather than `unknown`).
    pub(crate) fn is_context_sensitive_arg(&self, a: &Expr) -> bool {
        let f = match a {
            Expr::Arrow(f) | Expr::FunctionExpr(f) => f,
            _ => return false,
        };
        f.params.iter().any(|p| {
            p.ty.is_none()
                && p.initializer.is_none()
                && p.name.as_ident().map(|i| i.name != "this").unwrap_or(true)
        })
    }

    /// True if an object-literal argument has a directly context-sensitive
    /// function property (`{ fn: v => … }`). Such a literal is typed twice for a
    /// generic call: once during inference with the property omitted, then fully
    /// once the type arguments are known — so its cached type must be dropped
    /// before the final per-argument check.
    /// True for an array-literal expression that contains a context-sensitive
    /// function element (`[x => x, …]`). Such a property of an object-literal
    /// argument contributes nothing reliable to inference and is omitted while
    /// inferring, then re-checked with the resolved type arguments.
    pub(crate) fn array_has_context_sensitive(&self, e: &Expr) -> bool {
        if let Expr::Array { elements, .. } = e {
            elements.iter().any(|el| self.is_context_sensitive_arg(el))
        } else {
            false
        }
    }

    /// A non-arrow object-literal method/accessor has an *implicit* `this`
    /// parameter that is contextually typed (notably by a `ThisType<T>` in the
    /// literal's contextual type). Per tsc such a function is context-sensitive:
    /// its body cannot be soundly checked until the contextual `this` — and thus
    /// any type arguments it depends on — are resolved, so it is deferred to the
    /// final per-argument check. (An explicit `this` parameter fixes `this`
    /// up-front, so it is not deferred.)
    pub(crate) fn objlit_method_is_context_sensitive(&self, f: &FunctionLike) -> bool {
        f.kind != FuncKind::Arrow
            && !f
                .params
                .iter()
                .any(|p| p.name.as_ident().map(|i| i.name == "this").unwrap_or(false))
    }

    fn object_arg_has_context_sensitive(&self, a: &Expr) -> bool {
        if let Expr::Object { props, .. } = a {
            props.iter().any(|p| {
                matches!(p, ObjectProp::Property { value, .. } if
                    self.is_context_sensitive_arg(value)
                        || self.array_has_context_sensitive(value)
                        || self.object_arg_has_context_sensitive(value))
            })
        } else {
            false
        }
    }

    /// True if an object literal *directly* contains a context-sensitive
    /// method/accessor (one whose implicit `this` is contextually typed). Used
    /// only to gate the inference pass-2 re-typing of a direct descriptor whose
    /// contextual type carries a top-level `ThisType<T>` (e.g. `defineProp`'s
    /// `desc`): that method's body must be checked with the resolved `this`, so
    /// the literal is re-typed once the type arguments are known. Methods nested
    /// inside *property values* are deliberately not counted here — their own
    /// contextual type drives whether they are deferred, and a nested literal
    /// without a top-level `ThisType` contextual stays on the pass-1 path.
    pub(crate) fn object_has_context_sensitive_method(&self, a: &Expr) -> bool {
        if let Expr::Object { props, .. } = a {
            props.iter().any(|p| match p {
                ObjectProp::Method(f) => self.objlit_method_is_context_sensitive(f),
                _ => false,
            })
        } else {
            false
        }
    }

    /// A generic call's argument whose cached type from the inference passes may
    /// reflect provisional/omitted information and so must be re-checked against
    /// the final parameter type: a context-sensitive function expression, or an
    /// object literal carrying one.
    pub(crate) fn arg_needs_recheck(&self, a: &Expr) -> bool {
        self.is_context_sensitive_arg(a) || self.object_arg_has_context_sensitive(a)
    }

    /// Drop the cached types of object literals nested inside `a`. During
    /// inference these literals were typed with their context-sensitive function
    /// properties omitted; before the final per-argument check they must be
    /// re-typed in full, so their (partial) cached types are removed.
    pub(crate) fn drop_nested_objlit_caches(&mut self, a: &'a Expr) {
        if let Expr::Object { props, .. } = a {
            for p in props {
                if let ObjectProp::Property { value, .. } = p {
                    if matches!(value, Expr::Object { .. }) {
                        self.caches.expr_type_cache.remove(&node_key_expr(value));
                    }
                    self.drop_nested_objlit_caches(value);
                }
            }
        }
    }
}
