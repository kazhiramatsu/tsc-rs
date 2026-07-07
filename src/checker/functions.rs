//! Function checking: function-expression contextual typing and the
//! function-body pass (parameter init, return-path / reachability checks,
//! definite assignment, generator/async result typing). Split out of `exprs.rs`.

use crate::ast::*;
use crate::checker::exprs::{
    collect_await_spans, collect_binding_idents, collect_idents, collect_yield_spans,
    expr_mentions_super,
};
use crate::checker::flow::reachability::contains_return_with_expr;
use crate::checker::Checker;
use crate::diagnostics::gen;
use crate::types::{TypeId, TypeKind};

impl<'a> Checker<'a> {
    pub(crate) fn with_suppressed_next_function_implicit_any_params<R>(
        &mut self,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.with_suppressed_next_n_function_implicit_any_params(1, f)
    }

    pub(crate) fn with_suppressed_next_n_function_implicit_any_params<R>(
        &mut self,
        count: u32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let before = self.cflags.suppress_next_function_implicit_any_params;
        self.cflags.suppress_next_function_implicit_any_params += count;
        let out = f(self);
        if self.cflags.suppress_next_function_implicit_any_params > before {
            self.cflags.suppress_next_function_implicit_any_params = before;
        }
        out
    }

    pub(crate) fn with_suppressed_next_function_implicit_any_return<R>(
        &mut self,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let before = self.cflags.suppress_next_function_implicit_any_return;
        self.cflags.suppress_next_function_implicit_any_return += 1;
        let out = f(self);
        if self.cflags.suppress_next_function_implicit_any_return > before {
            self.cflags.suppress_next_function_implicit_any_return -= 1;
        }
        out
    }

    fn implicit_any_param_span(&self, p: &'a Param, fallback: Span) -> Span {
        p.decorators
            .first()
            .map(|d| d.span)
            .or_else(|| p.modifiers.first().map(|m| m.span))
            .unwrap_or(fallback)
    }

    fn should_suppress_implicit_any_param_for_grammar(
        &self,
        f: &'a FunctionLike,
        p: &'a Param,
    ) -> bool {
        if self.cflags.invalid_return_expr_depth > 0 {
            return true;
        }
        if self.cflags.in_class_static_block == 0 || f.kind != FuncKind::Arrow {
            return false;
        }
        p.name
            .as_ident()
            .map(|id| id.name == "await")
            .unwrap_or(false)
    }

    pub(crate) fn report_implicit_any_param(&mut self, p: &'a Param) {
        if self.cflags.suppress_next_function_implicit_any_params > 0 {
            return;
        }
        if p.ty.is_some()
            || p.initializer.is_some()
            || self.caches.param_ctx_types.contains_key(&node_key(p))
        {
            return;
        }
        if p.dotdotdot {
            if let Some(id) = p.name.as_ident() {
                let span = self.implicit_any_param_span(p, p.span);
                if self.options.no_implicit_any() {
                    self.error_at(
                        span,
                        &gen::Rest_parameter_0_implicitly_has_an_any_type,
                        &[id.name.clone()],
                    );
                } else {
                    self.suggestion_at(
                        span,
                        &gen::Rest_parameter_0_implicitly_has_an_any_type_but_a_better_type_may_be_inferred_from_usage,
                        &[id.name.clone()],
                    );
                }
            }
            return;
        }
        match &p.name {
            crate::ast::Binding::Ident(id) => {
                if id.name == "this" {
                    return;
                }
                let span = self.implicit_any_param_span(p, id.span);
                if self.options.no_implicit_any() {
                    self.error_at(
                        span,
                        &gen::Parameter_0_implicitly_has_an_1_type,
                        &[id.name.clone(), "any".into()],
                    );
                } else {
                    self.suggestion_at(
                        span,
                        &gen::Parameter_0_implicitly_has_an_1_type_but_a_better_type_may_be_inferred_from_usage,
                        &[id.name.clone(), "any".into()],
                    );
                }
            }
            pattern => {
                if self.options.no_implicit_any() {
                    let mut leaves: Vec<&crate::ast::Ident> = Vec::new();
                    collect_binding_idents(pattern, &mut leaves);
                    for id in leaves {
                        self.error_at(
                            id.span,
                            &gen::Binding_element_0_implicitly_has_an_1_type,
                            &[id.name.clone(), "any".into()],
                        );
                    }
                }
            }
        }
    }

    pub(crate) fn report_implicit_any_return_named(&mut self, span: Span, name: String) {
        if self.options.no_implicit_any() {
            self.error_at(
                span,
                &gen::_0_which_lacks_return_type_annotation_implicitly_has_an_1_return_type,
                &[name, "any".to_string()],
            );
        } else {
            self.suggestion_at(
                span,
                &gen::_0_implicitly_has_an_1_return_type_but_a_better_type_may_be_inferred_from_usage,
                &[name, "any".to_string()],
            );
        }
    }

    pub fn check_function_expression(
        &mut self,
        f: &'a FunctionLike,
        ctx: Option<TypeId>,
    ) -> TypeId {
        // contextual parameter types from the single call signature of ctx
        if let Some(c) = ctx {
            if let Some(sig0) = self.contextual_param_call_sig(c) {
                let csig = self.types.sig(sig0).clone();
                // When the function expression has more fixed parameters than the
                // contextual signature can supply, tsc applies no contextual
                // parameter types at all — every parameter becomes implicit-any.
                let arrow_fixed = f
                    .params
                    .iter()
                    .filter(|p| {
                        !p.dotdotdot && p.name.as_ident().map(|i| i.name != "this").unwrap_or(true)
                    })
                    .count();
                let arrow_has_rest = f.params.iter().any(|p| p.dotdotdot);
                let overflow =
                    csig.rest.is_none() && !arrow_has_rest && arrow_fixed > csig.params.len();
                if !overflow {
                    let mut value_i = 0usize;
                    for p in &f.params {
                        // an un-annotated parameter — whether a plain identifier
                        // or a destructuring pattern — takes its type from the
                        // contextual signature. (Destructuring patterns need this
                        // too so their binding elements are typed from the
                        // contextual member types rather than becoming
                        // implicit-any.)
                        let is_this = p.name.as_ident().map(|i| i.name == "this").unwrap_or(false);
                        let param_i = if is_this {
                            value_i
                        } else {
                            let j = value_i;
                            value_i += 1;
                            j
                        };
                        if p.ty.is_none() && !is_this {
                            let pt = csig.params.get(param_i).map(|pp| pp.ty).or(csig.rest);
                            if let Some(pt) = pt {
                                self.caches.param_ctx_types.insert(node_key(p), pt);
                            }
                        }
                    }
                }
            } else if f.kind == FuncKind::Setter {
                let mut value_param_seen = false;
                for p in &f.params {
                    let is_this = p.name.as_ident().map(|i| i.name == "this").unwrap_or(false);
                    if is_this {
                        continue;
                    }
                    if !value_param_seen && p.ty.is_none() && p.initializer.is_none() {
                        self.caches.param_ctx_types.insert(node_key(p), c);
                    }
                    value_param_seen = true;
                }
            }
        }
        let ctx_ret = ctx.and_then(|c| {
            if f.kind == FuncKind::Getter {
                Some(c)
            } else {
                let sig0 = self.contextual_call_sig(c)?;
                Some(self.sig_return(sig0))
            }
        });
        self.check_function_body(f, ctx_ret, /*require_annotated_params*/ true);
        self.function_type_of(f)
    }

    /// Walks a function body with the proper context. `report_implicit_params`:
    /// 7006 for parameters lacking annotation and context.
    pub fn check_function_body(
        &mut self,
        f: &'a FunctionLike,
        ctx_ret: Option<TypeId>,
        report_implicit_params: bool,
    ) {
        let key = node_key(f);
        if self.checked_decls.contains(&key) {
            return;
        }
        self.checked_decls.insert(key);
        let scope = self
            .bind
            .node_scope
            .get(&key)
            .copied()
            .unwrap_or(self.current_scope);
        // type-parameter constraints resolve eagerly (2313 circularity)
        let suppress_implicit_params = self.cflags.suppress_next_function_implicit_any_params > 0;
        if suppress_implicit_params {
            self.cflags.suppress_next_function_implicit_any_params -= 1;
        }
        let suppress_implicit_return = self.cflags.suppress_next_function_implicit_any_return > 0;
        if suppress_implicit_return {
            self.cflags.suppress_next_function_implicit_any_return -= 1;
        }
        self.check_unique_symbol_function_like(f);
        // implicit-any params: errors under noImplicitAny, suggestions otherwise.
        if report_implicit_params && !suppress_implicit_params {
            for p in &f.params {
                if self.should_suppress_implicit_any_param_for_grammar(f, p) {
                    continue;
                }
                self.report_implicit_any_param(p);
            }
        }
        // parameter list grammar (tsc checkGrammarParameterList)
        {
            let n = f.params.len();
            let mut seen_optional = false;
            for (i, p) in f.params.iter().enumerate() {
                let is_optional = p.question || p.initializer.is_some();
                if p.dotdotdot {
                    if i + 1 < n {
                        self.error_at(
                            p.dotdotdot_span.unwrap_or_else(|| {
                                Span::new(p.span.start as usize, p.span.start as usize + 3)
                            }),
                            &gen::A_rest_parameter_must_be_last_in_a_parameter_list,
                            &[],
                        );
                    }
                    if p.question {
                        // span: the question mark (end of name)
                        let qpos = p.name.span().end as usize;
                        self.error_at(
                            p.question_span.unwrap_or_else(|| Span::new(qpos, qpos + 1)),
                            &gen::A_rest_parameter_cannot_be_optional,
                            &[],
                        );
                    }
                    if p.initializer.is_some() {
                        self.error_at(
                            p.name.span(),
                            &gen::A_rest_parameter_cannot_have_an_initializer,
                            &[],
                        );
                    }
                } else {
                    if p.question && p.initializer.is_some() {
                        self.error_at(
                            p.name.span(),
                            &gen::Parameter_cannot_have_question_mark_and_initializer,
                            &[],
                        );
                    }
                    if !is_optional && seen_optional {
                        self.error_at(
                            p.name.span(),
                            &gen::A_required_parameter_cannot_follow_an_optional_parameter,
                            &[],
                        );
                    }
                }
                if is_optional {
                    seen_optional = true;
                }
            }
            if matches!(f.kind, FuncKind::Getter | FuncKind::Setter) {
                let value_params: Vec<&Param> = f
                    .params
                    .iter()
                    .filter(|p| {
                        !p.name
                            .as_ident()
                            .map(|id| id.name == "this")
                            .unwrap_or(false)
                    })
                    .collect();
                if f.kind == FuncKind::Getter && !value_params.is_empty() {
                    if let Some(name) = &f.name {
                        self.error_at(
                            name.span(),
                            &gen::A_get_accessor_cannot_have_parameters,
                            &[],
                        );
                    }
                }
                if f.kind == FuncKind::Setter && value_params.len() != 1 {
                    if let Some(name) = &f.name {
                        self.error_at(
                            name.span(),
                            &gen::A_set_accessor_must_have_exactly_one_parameter,
                            &[],
                        );
                    }
                }
            }
            if f.kind == FuncKind::Setter {
                let name_span = f.name.as_ref().map(|n| n.span());
                let value_param = f.params.iter().find(|p| {
                    !p.name
                        .as_ident()
                        .map(|id| id.name == "this")
                        .unwrap_or(false)
                });
                if let (Some(p), Some(nspan)) = (value_param, name_span) {
                    if p.initializer.is_some() {
                        self.error_at(
                            nspan,
                            &gen::A_set_accessor_parameter_cannot_have_an_initializer,
                            &[],
                        );
                    }
                    if p.question {
                        let qpos = p.name.span().end as usize;
                        self.error_at(
                            p.question_span.unwrap_or_else(|| Span::new(qpos, qpos + 1)),
                            &gen::A_set_accessor_cannot_have_an_optional_parameter,
                            &[],
                        );
                    }
                    if p.dotdotdot {
                        self.error_at(
                            p.dotdotdot_span.unwrap_or(nspan),
                            &gen::A_set_accessor_cannot_have_rest_parameter,
                            &[],
                        );
                    }
                }
                if f.return_type.is_some() {
                    if let Some(n2) = &f.name {
                        self.error_at(
                            n2.span(),
                            &gen::A_set_accessor_cannot_have_a_return_type_annotation,
                            &[],
                        );
                    }
                }
            }
            if f.kind == FuncKind::Getter || f.kind == FuncKind::Setter {
                if f.type_params.is_some() {
                    if let Some(n2) = &f.name {
                        self.error_at(
                            n2.span(),
                            &gen::An_accessor_cannot_have_type_parameters,
                            &[],
                        );
                    }
                }
            }
            if f.kind == FuncKind::Constructor {
                if let Some(tps) = &f.type_params {
                    if let Some(tp0) = tps.first() {
                        self.error_at(
                            tp0.name.span,
                            &gen::Type_parameters_cannot_appear_on_a_constructor_declaration,
                            &[],
                        );
                    }
                }
                if let Some(rt) = &f.return_type {
                    self.error_at(
                        rt.span(),
                        &gen::Type_annotation_cannot_appear_on_a_constructor_declaration,
                        &[],
                    );
                }
            }
            // rest parameter must be of an array type
            for p in &f.params {
                if p.dotdotdot {
                    if let Some(ty) = &p.ty {
                        let scope3 = self
                            .bind
                            .node_scope
                            .get(&key)
                            .copied()
                            .unwrap_or(self.current_scope);
                        let mut t = self.resolve_type(ty, scope3);
                        if p.question {
                            t = self.types.union(vec![t, self.types.undefined]);
                        }
                        let at = self.apparent_type(t);
                        let arrayish = |slf: &mut Self, ty: TypeId| {
                            slf.array_element_type(ty).is_some()
                                || matches!(
                                    slf.types.kind(ty),
                                    TypeKind::Tuple(_)
                                        | TypeKind::ReadonlyTuple(_)
                                        | TypeKind::ReadonlyArray(_)
                                        | TypeKind::Any
                                        | TypeKind::Error
                                )
                        };
                        // a type parameter constrained to an array/tuple
                        // (`<T extends unknown[]>(...args: T)`) is array-like via
                        // its apparent type.
                        let is_arrayish = arrayish(self, t) || arrayish(self, at);
                        if !is_arrayish {
                            self.error_at(
                                Span::new(p.span.start as usize, p.span.start as usize + 3),
                                &gen::A_rest_parameter_must_be_of_an_array_type,
                                &[],
                            );
                        }
                    }
                }
            }
        }
        // 'export' must precede 'declare' (1029)
        {
            let declare_pos = f
                .modifiers
                .iter()
                .position(|m| m.kind == ModifierKind::Declare);
            let export_pos = f
                .modifiers
                .iter()
                .position(|m| m.kind == ModifierKind::Export);
            if let (Some(dp), Some(ep)) = (declare_pos, export_pos) {
                if ep > dp {
                    self.error_at(
                        f.modifiers[ep].span,
                        &gen::_0_modifier_must_precede_1_modifier,
                        &["export".to_string(), "declare".to_string()],
                    );
                }
            }
        }
        // ambient implementations (1183) / declare+async (1040)
        if has_modifier(&f.modifiers, ModifierKind::Declare) {
            if let Some(crate::ast::FuncBody::Block(b)) = &f.body {
                self.error_at(
                    Span::new(b.span.start as usize, b.span.start as usize + 1),
                    &gen::An_implementation_cannot_be_declared_in_ambient_contexts,
                    &[],
                );
            }
            if let Some(m) = f.modifiers.iter().find(|m| m.kind == ModifierKind::Async) {
                self.error_at(
                    m.span,
                    &gen::_0_modifier_cannot_be_used_in_an_ambient_context,
                    &["async".to_string()],
                );
            }
        }
        // Parameter decorators. Under legacy (experimentalDecorators) they are
        // parameter initializers: ambient/signature-only → 2371; self/forward
        // references → 2372/2373
        let has_body = f.body.is_some();
        let param_names: Vec<Option<String>> = f
            .params
            .iter()
            .map(|p| p.name.as_ident().map(|i| i.name.clone()))
            .collect();
        for (pi, p) in f.params.iter().enumerate() {
            let Some(init) = &p.initializer else { continue };
            let is_ctor_init = f.kind == FuncKind::Constructor;
            if !has_body {
                self.error_at(
                    p.name.span(),
                    &gen::A_parameter_initializer_is_only_allowed_in_a_function_or_constructor_implementation,
                    &[],
                );
                continue;
            }
            // yield/await cannot appear in parameter initializers
            if f.is_generator {
                let mut yields: Vec<Span> = Vec::new();
                collect_yield_spans(init, &mut yields);
                for ys in yields {
                    self.error_at(
                        ys,
                        &gen::yield_expressions_cannot_be_used_in_a_parameter_initializer,
                        &[],
                    );
                    if f.return_type.is_none() && self.options.no_implicit_any() {
                        self.error_at(
                            ys,
                            &gen::yield_expression_implicitly_results_in_an_any_type_because_its_containing_generator_lacks_a_return_type_annotation,
                            &[],
                        );
                    }
                }
            }
            if has_modifier(&f.modifiers, ModifierKind::Async) {
                let mut awaits: Vec<Span> = Vec::new();
                collect_await_spans(init, &mut awaits);
                for asp in awaits {
                    self.error_at(
                        asp,
                        &gen::await_expressions_cannot_be_used_in_a_parameter_initializer,
                        &[],
                    );
                }
            }
            if is_ctor_init && expr_mentions_super(init) {
                self.cflags.in_ctor_param_init = true;
                self.check_expr(init, None);
                self.cflags.in_ctor_param_init = false;
            }
            let mut refs: Vec<&crate::ast::Ident> = Vec::new();
            collect_idents(init, &mut refs);
            for r in refs {
                if let Some(target) = param_names
                    .iter()
                    .position(|n| n.as_deref() == Some(r.name.as_str()))
                {
                    if target == pi {
                        if p.ty.is_none() && self.options.no_implicit_any() {
                            self.error_at(
                                p.name.span(),
                                &gen::_0_implicitly_has_type_any_because_it_does_not_have_a_type_annotation_and_is_referenced_directly_or_indirectly_in_its_own_initializer,
                                &[r.name.clone()],
                            );
                        }
                        self.error_at(
                            r.span,
                            &gen::Parameter_0_cannot_reference_itself,
                            &[r.name.clone()],
                        );
                    } else if target > pi {
                        self.error_at(
                            r.span,
                            &gen::Parameter_0_cannot_reference_identifier_1_declared_after_it,
                            &[param_names[pi].clone().unwrap_or_default(), r.name.clone()],
                        );
                    }
                }
            }
        }
        // 2677: `x is T` must be assignable to x's declared type
        if let Some(crate::ast::TypeNode::Predicate {
            param_name,
            ty: Some(pred_ty),
            ..
        }) = f.return_type.as_ref()
        {
            if let Some(p) = f.params.iter().find(|p| {
                p.name
                    .as_ident()
                    .map(|i| i.name == param_name.name)
                    .unwrap_or(false)
            }) {
                if p.dotdotdot {
                    self.error_at(
                        param_name.span,
                        &gen::A_type_predicate_cannot_reference_a_rest_parameter,
                        &[],
                    );
                    return;
                }
                if let Some(pty_node) = &p.ty {
                    let scope2 = self
                        .bind
                        .node_scope
                        .get(&key)
                        .copied()
                        .unwrap_or(self.current_scope);
                    let pt = self.resolve_type(pty_node, scope2);
                    let predt = self.resolve_type(pred_ty, scope2);
                    if !self.types.is_any_or_error(pt)
                        && !self.types.is_any_or_error(predt)
                        && !self.is_assignable_to(predt, pt)
                    {
                        self.rel.keep_head_for_missing = false;
                    }
                }
            }
        }
        // signature-only function declarations need return annotations. tsc
        // reports TS7010 under noImplicitAny and TS7050 as a suggestion when
        // noImplicitAny is off.
        if !suppress_implicit_return
            && !has_body
            && f.return_type.is_none()
            && !matches!(
                f.kind,
                FuncKind::Constructor | FuncKind::Getter | FuncKind::Setter
            )
        {
            if let Some(n) = &f.name {
                let nd = n.text().unwrap_or_default();
                self.report_implicit_any_return_named(n.span(), nd);
            }
        }
        // strict-mode parameter names: eval/arguments (1100/1210/1215, tsc
        // `this` parameter placement rules (2680/2681/2730)
        for p in &f.params {
            if p.name.as_ident().map(|i| i.name == "this").unwrap_or(false) {
                let span = p.name.span();
                if matches!(f.kind, FuncKind::Getter | FuncKind::Setter) {
                    self.error_at(
                        span,
                        &gen::get_and_set_accessors_cannot_declare_this_parameters,
                        &[],
                    );
                }
                if f.kind == FuncKind::Constructor {
                    self.error_at(
                        span,
                        &gen::A_0_parameter_must_be_the_first_parameter,
                        &["this".to_string()],
                    );
                }
            }
        }
        // 'declare' can never appear on a parameter (1090)
        // parameter properties are only allowed on constructor implementations
        if f.kind != FuncKind::Constructor {
            for p in &f.params {
                let prop_mod = p.modifiers.iter().find(|m| {
                    matches!(
                        m.kind,
                        ModifierKind::Public
                            | ModifierKind::Private
                            | ModifierKind::Protected
                            | ModifierKind::Readonly
                    )
                });
                if let Some(pm) = prop_mod {
                    self.error_at(
                        pm.span,
                        &gen::A_parameter_property_is_only_allowed_in_a_constructor_implementation,
                        &[],
                    );
                }
            }
        }
        // destructuring parameters: type their bindings
        for p in &f.params {
            if !matches!(&p.name, crate::ast::Binding::Ident(_)) {
                let scope2 = self
                    .bind
                    .node_scope
                    .get(&key)
                    .copied()
                    .unwrap_or(self.current_scope);
                let t = match &p.ty {
                    Some(ty) => self.resolve_type(ty, scope2),
                    None => self
                        .caches
                        .param_ctx_types
                        .get(&node_key(p))
                        .copied()
                        .unwrap_or(self.types.any),
                };
                self.destructure_binding(&p.name, t);
            }
        }
        // declared return type
        let declared_ret = f.return_type.as_ref().map(|rt| {
            let s = self
                .bind
                .node_scope
                .get(&key)
                .copied()
                .unwrap_or(self.current_scope);
            self.resolve_type(rt, s)
        });
        let is_async = has_modifier(&f.modifiers, ModifierKind::Async)
            && !matches!(f.kind, FuncKind::Getter | FuncKind::Setter);
        // return-path diagnostics see the awaited annotation even when the
        // ES5 Promise check below nulls declared_ret for body checking (tsc
        // unwrapReturnType reads the syntactic annotation's awaited type; an
        // un-unwrappable thenable maps to errorType = exempt).
        let ret_paths = if is_async {
            declared_ret.map(|t| {
                self.awaited_for_return_paths(t, 0)
                    .unwrap_or(self.types.error)
            })
        } else {
            declared_ret
        };
        let mut declared_ret = declared_ret;
        if is_async {
            if let (Some(rt_node), Some(dt)) = (f.return_type.as_ref(), declared_ret) {
                let is_promise_ref = matches!(
                    self.types.kind(dt),
                    crate::types::TypeKind::Ref(sym, _) if self.symbol(*sym).name == "Promise"
                ) || matches!(
                    self.types.kind(dt),
                    crate::types::TypeKind::Iface(sym) if self.symbol(*sym).name == "Promise"
                );
                if is_promise_ref {
                    // tsc: a pre-ES2015 async function needs a global Promise
                    // constructor *value*; when the lib provides one, there is
                    // no diagnostic (getGlobalPromiseConstructorSymbol).
                    let has_promise_value = self
                        .lookup_value(self.bind.global_scope, "Promise")
                        .is_some();
                    if self.options.script_target_rank() < 2 && !has_promise_value {
                        self.error_at(
                            rt_node.span(),
                            &gen::An_async_function_or_method_in_ES5_requires_the_Promise_constructor_Make_sure_you_have_a_declaration_for_the_Promise_constructor_or_include_ES2015_in_your_lib_option,
                            &[],
                        );
                    }
                    declared_ret = Some(self.awaited_type_pub(dt));
                } else {
                    let d = self.display_type(dt);
                    self.error_at(
                        rt_node.span(),
                        &gen::Type_0_is_not_a_valid_async_function_return_type_in_ES5_because_it_does_not_refer_to_a_Promise_compatible_constructor_value,
                        &[d],
                    );
                    declared_ret = None;
                }
            }
        }
        let ret_ctx = match declared_ret {
            Some(d) => Some(d),
            // an async function with no explicit return annotation returns the
            // awaited form of its contextual return type (`async () => 'a'`
            // against `() => Promise<string>` checks the body against `string`).
            None if is_async => ctx_ret.map(|c| self.awaited_type_pub(c)),
            None => ctx_ret,
        };
        // an explicit `this` parameter annotation types `this` inside the body
        let explicit_this = self.explicit_this_param_type(f, scope);
        // otherwise a contextual `this` staged by an enclosing object literal
        // (from its `ThisType<T>`) applies to a non-arrow method or function
        // expression; `.take()` consumes it so nested functions don't reuse it,
        // and arrows inherit `this` lexically and never consume it.
        let staged_this = if f.kind != FuncKind::Arrow {
            self.cflags.pending_objlit_this.take()
        } else {
            None
        };
        let this_param_ty = explicit_this.or(staged_this);
        let fn_ctx = super::FnCtx {
            return_type: ret_ctx,
            is_async: has_modifier(&f.modifiers, ModifierKind::Async)
                && !matches!(f.kind, FuncKind::Getter | FuncKind::Setter),
            is_generator: f.is_generator,
            kind: f.kind,
            fn_key: key,
            this_ty: this_param_ty,
        };
        // Build the corresponding `ThisContainer`. For a class method/accessor/
        // constructor we attach the owning class and its static-ness so that
        // `Expr::This` and `TypeNode::This` resolve correctly even when this
        // body is reached lazily (via `infer_return_from_body` from
        // `sig_return`); for a non-method function-like we mark
        // Arrow/NonArrowFn so the `this` walk treats fn-expr boundaries
        // correctly.
        let container_kind = match f.kind {
            FuncKind::Arrow => super::ContainerKind::Arrow,
            FuncKind::Method | FuncKind::Getter | FuncKind::Setter | FuncKind::Constructor => {
                super::ContainerKind::Method
            }
            FuncKind::Declaration | FuncKind::Expression => super::ContainerKind::NonArrowFn,
        };
        let owner_class = if matches!(
            f.kind,
            FuncKind::Method | FuncKind::Getter | FuncKind::Setter | FuncKind::Constructor
        ) {
            self.enclosing_class_of_fn(f)
        } else {
            None
        };
        let is_static = owner_class.is_some() && self.is_static_member(f);
        let tc = super::ThisContainer {
            class_owner: owner_class,
            is_static,
            kind: container_kind,
            explicit_this: this_param_ty,
        };
        // Push fn_stack + this_container for the body through a scoped guard, so
        // both frames are popped on every exit path (including any early return
        // added to the body) — the bracketed discipline whose violation caused
        // the Phase-1 regression.
        self.with_fn_ctx(fn_ctx, tc, |this| {
            this.check_function_body_inner(f, scope, ret_ctx, declared_ret, ret_paths)
        });
    }

    /// The body of `check_function_body`, run inside the `fn_stack` /
    /// `this_container_stack` guard (`with_fn_ctx`). Split out so those pops
    /// cannot be leaked by an early return added here.
    fn check_function_body_inner(
        &mut self,
        f: &'a FunctionLike,
        scope: crate::binder::ScopeId,
        ret_ctx: Option<TypeId>,
        declared_ret: Option<TypeId>,
        ret_paths: Option<TypeId>,
    ) {
        let prev_scope = self.current_scope;
        self.current_scope = scope;
        match &f.body {
            Some(FuncBody::Block(b)) => {
                self.prime_declarator_annotations(&b.stmts, scope);
                self.check_statements(&b.stmts, scope);
            }
            Some(FuncBody::Expr(e)) => {
                let t = self.check_expr(e, ret_ctx);
                // Only the arrow's OWN return annotation makes the body the
                // authoritative place to report a return-type mismatch. When the
                // expected return type comes purely from a contextual type (an
                // assignment target, a call argument's parameter type, a `return`
                // position, ...), the surrounding assignability check already
                // elaborates the mismatch onto this body expression, so checking
                // it here as well would emit the diagnostic twice (once during
                // inference with the candidate type, once in the final relation
                // check with the widened type).
                if let Some(declared) = declared_ret {
                    if !matches!(self.types.kind(declared), TypeKind::Void | TypeKind::Any) {
                        self.check_assignable(t, declared, e.span(), None, Some(e));
                    }
                }
            }
            None => {}
        }
        self.current_scope = prev_scope;
        if let Some(FuncBody::Block(b)) = &f.body {
            if f.kind == FuncKind::Getter && !contains_return_with_expr(&b.stmts) {
                if let Some(name) = &f.name {
                    self.error_at(name.span(), &gen::A_get_accessor_must_return_a_value, &[]);
                }
            }
            self.check_return_paths(f, ret_paths, b);
        }
    }
}
