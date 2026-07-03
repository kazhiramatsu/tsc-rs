//! Expression checking: literals & freshness, member access with nullish
//! handling, calls with generic inference, operators.

use super::{operators::TruthinessContext, Checker, CtorFieldContextKind, RefKey};
use crate::ast::*;
use crate::binder::{flags, SymbolId};
use crate::diagnostics::{gen, MessageChain, RelatedInfo};
use crate::types::{PropInfo, Shape, TypeId, TypeKind};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComputedKeyKind {
    String,
    Number,
}

impl<'a> Checker<'a> {
    fn syntactic_computed_key_kind(&self, expr: &'a Expr) -> Option<ComputedKeyKind> {
        match expr {
            Expr::Paren { inner, .. } | Expr::NonNull { expr: inner, .. } => {
                self.syntactic_computed_key_kind(inner)
            }
            Expr::Assertion { expr: inner, .. } => self.syntactic_computed_key_kind(inner),
            Expr::StrLit { .. } | Expr::Template { .. } => Some(ComputedKeyKind::String),
            Expr::NumLit { .. } => Some(ComputedKeyKind::Number),
            Expr::Unary {
                op:
                    UnaryOp::Plus | UnaryOp::Minus | UnaryOp::Tilde | UnaryOp::Void | UnaryOp::Delete,
                ..
            }
            | Expr::Update { .. } => Some(ComputedKeyKind::Number),
            Expr::Binary {
                op: BinOp::Add,
                left,
                right,
                ..
            } => {
                let lk = self.syntactic_computed_key_kind(left);
                let rk = self.syntactic_computed_key_kind(right);
                if matches!(lk, Some(ComputedKeyKind::String))
                    || matches!(rk, Some(ComputedKeyKind::String))
                {
                    Some(ComputedKeyKind::String)
                } else if matches!(lk, Some(ComputedKeyKind::Number))
                    && matches!(rk, Some(ComputedKeyKind::Number))
                {
                    Some(ComputedKeyKind::Number)
                } else {
                    None
                }
            }
            Expr::Binary {
                op:
                    BinOp::Sub
                    | BinOp::Mul
                    | BinOp::Div
                    | BinOp::Mod
                    | BinOp::Exp
                    | BinOp::Shl
                    | BinOp::Shr
                    | BinOp::UShr
                    | BinOp::BitAnd
                    | BinOp::BitOr
                    | BinOp::BitXor,
                ..
            } => Some(ComputedKeyKind::Number),
            _ => None,
        }
    }

    fn contextual_index_type_for_computed_key(
        &mut self,
        ctx: TypeId,
        key: ComputedKeyKind,
    ) -> Option<TypeId> {
        let apparent = self.apparent_type(ctx);
        if let TypeKind::Union(members) = self.types.kind(apparent).clone() {
            let mut tys = Vec::new();
            for m in members {
                if let Some(t) = self.contextual_index_type_for_computed_key(m, key) {
                    tys.push(t);
                }
            }
            if tys.is_empty() {
                return None;
            }
            return Some(self.types.union(tys));
        }
        let shape = self.shape_of_type(apparent)?;
        let infos = self.types.shape(shape).index_infos.clone();
        if key == ComputedKeyKind::Number {
            for info in &infos {
                if matches!(self.types.kind(info.key), TypeKind::Number) {
                    return Some(info.value);
                }
            }
        }
        for info in &infos {
            if matches!(self.types.kind(info.key), TypeKind::String) {
                return Some(info.value);
            }
        }
        None
    }

    fn contextual_type_for_computed_name(
        &mut self,
        ctx: Option<TypeId>,
        name: &'a PropName,
    ) -> Option<TypeId> {
        let ctx = ctx?;
        let PropName::Computed { expr, .. } = name else {
            return None;
        };
        let key = self.syntactic_computed_key_kind(expr)?;
        self.contextual_index_type_for_computed_key(ctx, key)
    }

    pub fn check_expr(&mut self, e: &'a Expr, ctx: Option<TypeId>) -> TypeId {
        let key = match e {
            // identifiers may narrow differently in different positions; don't cache
            Expr::Ident(_) | Expr::This { .. } | Expr::Super { .. } => 0,
            _ => node_key_expr(e),
        };
        if key != 0 {
            if let Some(&t) = self.caches.expr_type_cache.get(&key) {
                return t;
            }
        }
        let t = self.check_expr_uncached(e, ctx);
        // exploratory scaffold runs roll their diagnostics back — caching
        // their result would make the real pass short-circuit here and never
        // re-emit those diagnostics
        if key != 0 && self.fresolve.quiet == 0 {
            self.caches.expr_type_cache.insert(key, t);
        }
        t
    }

    fn check_expr_uncached(&mut self, e: &'a Expr, ctx: Option<TypeId>) -> TypeId {
        match e {
            Expr::NumLit { value, .. } => {
                let r = self.types.number_lit(*value);
                self.types.fresh(r)
            }
            Expr::StrLit { value, .. } => {
                let r = self.types.string_lit_js(value);
                self.types.fresh(r)
            }
            Expr::BigIntLit { text, .. } => {
                let r = self.types.bigint_lit(text.trim_end_matches('n'));
                self.types.fresh(r)
            }
            Expr::BoolLit { value, .. } => {
                let r = if *value {
                    self.types.true_t
                } else {
                    self.types.false_t
                };
                self.types.fresh(r)
            }
            Expr::NullLit { .. } => self.types.null,
            Expr::RegexLit { .. } => self
                .global_type_symbol("RegExp")
                .map(|s| self.types.intern_kind(TypeKind::Iface(s)))
                .unwrap_or(self.types.any),
            Expr::Template { parts, .. } => {
                // Collect the leading text and, for each substitution, its type
                // and the text that follows it.
                let mut head = crate::jsstr::JsString::new();
                let mut pieces: Vec<(TypeId, crate::jsstr::JsString)> = Vec::new();
                for p in parts {
                    match p {
                        TemplatePart::Str(s) => match pieces.last_mut() {
                            Some((_, txt)) => txt.push_js(s),
                            None => head.push_js(s),
                        },
                        TemplatePart::Expr(ex) => {
                            let t = self.check_expr(ex, None);
                            let r = self.types.regular(t);
                            if matches!(self.types.kind(r), TypeKind::EsSymbol) {
                                self.error_at(
                                    ex.span(),
                                    &gen::Implicit_conversion_of_a_symbol_to_a_string_will_fail_at_runtime_Consider_wrapping_this_expression_in_String,
                                    &[],
                                );
                            }
                            pieces.push((r, crate::jsstr::JsString::new()));
                        }
                    }
                }
                if pieces.is_empty() {
                    let r = self.types.string_lit_js(&head);
                    return self.types.fresh(r);
                }
                let mut any_generic = false;
                for (t, _) in &pieces {
                    if self.type_contains_params(*t) {
                        any_generic = true;
                        break;
                    }
                }
                if any_generic {
                    // a generic substitution (e.g. a string-constrained type
                    // parameter) yields a template literal *type* that keeps the
                    // placeholder, matching tsc (`\`prefix-${T}\``), rather than
                    // widening the whole expression to `string`.
                    let head_s = head.to_str_lossy().into_owned();
                    let parts_v: Vec<(TypeId, String)> = pieces
                        .into_iter()
                        .map(|(t, txt)| (t, txt.to_str_lossy().into_owned()))
                        .collect();
                    self.template_literal_type(head_s, parts_v)
                } else {
                    self.types.string
                }
            }
            Expr::Ident(id) => self.check_ident(id),
            Expr::This { span } => {
                let t = self.check_this_expr(*span);
                // tsc flow-narrows `this` like any reference: under
                // `if (this instanceof D)` both the VALUE `this` and a
                // `typeof this` annotation read the narrowed type. The
                // builder maps This exprs; the polymorphic this stays
                // when nothing narrows (write positions read declared).
                if self.cflags.pattern_target == 0 {
                    if let TypeKind::TypeParam(p) = self.types.kind(t) {
                        let p = *p;
                        if self.this_param_owner(p).is_some() {
                            if let Some(n) = self.flow_type_of_this_query(node_key_expr(e), p) {
                                return n;
                            }
                        }
                    }
                }
                t
            }
            Expr::Super { span } => {
                if self.cflags.in_ctor_param_init {
                    self.error_at(
                        *span,
                        &gen::super_cannot_be_referenced_in_constructor_arguments,
                        &[],
                    );
                    self.error_at(
                        *span,
                        &gen::super_must_be_called_before_accessing_a_property_of_super_in_the_constructor_of_a_derived_class,
                        &[],
                    );
                    return self.types.error;
                }
                let enclosing = self
                    .stacks
                    .fn_stack
                    .iter()
                    .rev()
                    .find(|f| f.kind != FuncKind::Arrow)
                    .map(|f| f.kind);
                if matches!(
                    enclosing,
                    Some(FuncKind::Declaration | FuncKind::Expression)
                ) {
                    self.error_at(
                        *span,
                        &gen::super_can_only_be_referenced_in_members_of_derived_classes_or_object_literal_expressions,
                        &[],
                    );
                    return self.types.error;
                }
                match self.current_super_type() {
                    Some(b) => b,
                    None => {
                        self.error_at(
                            *span,
                            &gen::super_can_only_be_referenced_in_a_derived_class,
                            &[],
                        );
                        self.types.error
                    }
                }
            }
            Expr::Paren { inner, .. } => self.check_expr(inner, ctx),
            Expr::Array { elements, .. } => self.check_array_literal(elements, ctx),
            Expr::Object { props, span } => self.check_object_literal(props, *span, ctx),
            Expr::Arrow(f) | Expr::FunctionExpr(f) => self.check_function_expression(f, ctx),
            Expr::Call { .. } => {
                // assertion effects (`asserts x is T`) are the resolver's
                // Call arm
                self.check_call_like(e, ctx)
            }
            Expr::New { .. } => self.check_new(e, ctx),
            Expr::PropAccess { .. } => self.check_prop_access(e),
            Expr::ElemAccess { .. } => self.check_elem_access(e),
            Expr::Unary { op, operand, .. } => self.check_unary(*op, operand),
            Expr::Update { operand, span, .. } => self.check_update(operand, *span),
            Expr::Binary { .. } => self.check_binary(e, ctx),
            Expr::Cond {
                cond,
                when_true,
                when_false,
                ..
            } => {
                let ct = self.check_expr(cond, None);
                self.check_testable(cond, ct, TruthinessContext::Condition);
                // branch narrowing is the resolver's Cond edges
                let t1 = self.check_expr(when_true, ctx);
                let t2 = self.check_expr(when_false, ctx);
                let (r1, r2) = (self.types.regular(t1), self.types.regular(t2));
                self.types.union(vec![r1, r2])
            }
            Expr::Assertion {
                expr,
                ty,
                kind,
                kw_span,
                ..
            } => match kind {
                AssertionKind::ConstAssert => {
                    let valid_target = matches!(
                        &**expr,
                        Expr::NumLit { .. }
                            | Expr::StrLit { .. }
                            | Expr::BoolLit { .. }
                            | Expr::Array { .. }
                            | Expr::Object { .. }
                            | Expr::Template { .. }
                            | Expr::BigIntLit { .. }
                            | Expr::PropAccess { .. }
                    ) || matches!(&**expr, Expr::Unary { op: UnaryOp::Minus, operand, .. }
                        if matches!(&**operand, Expr::NumLit { .. }));
                    if !valid_target {
                        self.error_at(
                            expr.span(),
                            &gen::A_const_assertion_can_only_be_applied_to_references_to_enum_members_or_string_number_boolean_array_or_object_literals,
                            &[],
                        );
                    }
                    let prev = self.cflags.in_const_assertion;
                    self.cflags.in_const_assertion = true;
                    let et = self.check_expr(expr, None);
                    self.cflags.in_const_assertion = prev;
                    self.types.regular(et)
                }
                AssertionKind::Satisfies => {
                    let scope = self.current_scope;
                    let tt = self.resolve_type(ty, scope);
                    let et = self.check_expr(expr, Some(tt));
                    if !self.types.is_error(et) && !self.types.is_error(tt) {
                        self.check_assignable(
                            et,
                            tt,
                            *kw_span,
                            Some((
                                &gen::Type_0_does_not_satisfy_the_expected_type_1,
                                Vec::new(),
                            )),
                            Some(expr),
                        );
                    }
                    et
                }
                _ => {
                    let scope = self.current_scope;
                    let tt = self.resolve_type(ty, scope);
                    let et = self.check_expr(expr, Some(tt));
                    let rs = self.types.regular(et);
                    if !self.types.is_any_or_error(rs)
                        && !self.types.is_any_or_error(tt)
                        && !matches!(self.types.kind(rs), TypeKind::Unknown)
                        && !matches!(self.types.kind(tt), TypeKind::Unknown)
                        && !self.cast_comparable(rs, tt)
                    {
                        let sd = self.display_type_for_error(rs, tt);
                        let td = self.display_type(tt);
                        self.error_at(
                            e.span(),
                            &gen::Conversion_of_type_0_to_type_1_may_be_a_mistake_because_neither_type_sufficiently_overlaps_with_the_other_If_this_was_intentional_convert_the_expression_to_unknown_first,
                            &[sd, td],
                        );
                    }
                    tt
                }
            },
            Expr::NonNull { expr, .. } => {
                // `x!` asserts definite assignment for a DIRECT identifier
                // operand (tsc checkIdentifier's NonNullExpression-parent
                // disjunct); parens break it — `(x)!` still checks
                if let Expr::Ident(id) = &**expr {
                    self.cflags.nonnull_ident = node_key(id);
                }
                let t = self.check_expr(expr, ctx);
                self.non_nullable(t)
            }
            Expr::Await { expr, span } => {
                match self.stacks.fn_stack.last() {
                    Some(f) if !f.is_async => {
                        self.error_at(
                            *span,
                            &gen::await_expressions_are_only_allowed_within_async_functions_and_at_the_top_levels_of_modules,
                            &[],
                        );
                    }
                    Some(_) => {}
                    None => {
                        // top level
                        if !self.files[self.current_file].2.is_module {
                            self.error_at(*span, &gen::await_expressions_are_only_allowed_at_the_top_level_of_a_file_when_that_file_is_a_module_but_this_file_has_no_imports_or_exports_Consider_adding_an_empty_export_to_make_this_file_a_module, &[]);
                        }
                        let module_ok = matches!(
                            self.options.module_kind(),
                            "es2022"
                                | "esnext"
                                | "system"
                                | "node16"
                                | "node18"
                                | "node20"
                                | "nodenext"
                                | "preserve"
                        );
                        if !module_ok || self.options.script_target_rank() < 4 {
                            self.error_at(*span, &gen::Top_level_await_expressions_are_only_allowed_when_the_module_option_is_set_to_es2022_esnext_system_node16_node18_node20_nodenext_or_preserve_and_the_target_option_is_set_to_es2017_or_higher, &[]);
                        }
                    }
                }
                let t = self.check_expr(expr, None);
                // `await` on a value without a `then` method has no effect on its
                // type — tsc surfaces this as a suggestion (TS80007).
                let reg = self.types.regular(t);
                if !self.types.is_any_or_error(reg)
                    && !matches!(self.types.kind(reg), TypeKind::Unknown | TypeKind::Never)
                    && self.prop_info_of_type(reg, "then").is_none()
                {
                    self.unused_diag(
                        *span,
                        &gen::await_has_no_effect_on_the_type_of_this_expression,
                        &[],
                        false,
                    );
                }
                self.awaited_type(t)
            }
            Expr::ImportCall { args, .. } => {
                // `import(specifier)` yields a module namespace at runtime;
                // modeled as `Promise<any>`. Arguments are still checked.
                for a in args {
                    self.check_expr(a, None);
                }
                self.promise_type(self.types.any)
            }
            Expr::ImportMeta { .. } => self.types.any,
            Expr::Yield {
                expr,
                delegate,
                span,
            } => {
                let in_generator = self
                    .stacks
                    .fn_stack
                    .last()
                    .map(|f| f.is_generator)
                    .unwrap_or(false);
                if !in_generator {
                    self.error_at(
                        *span,
                        &gen::A_yield_expression_is_only_allowed_in_a_generator_body,
                        &[],
                    );
                }
                if let Some(e2) = expr {
                    // a plain `yield v` must produce the generator's yield type;
                    // `yield* it` delegates to an iterable and is not checked here.
                    let yt = if *delegate {
                        None
                    } else {
                        self.current_yield_type()
                    };
                    let count = if self.cflags.suppress_yield_function_implicit_any_params > 0 {
                        self.contextual_function_arg_count(e2)
                    } else {
                        0
                    };
                    let et = if count > 0 {
                        self.with_suppressed_next_n_function_implicit_any_params(count, |this| {
                            this.check_expr(e2, yt)
                        })
                    } else {
                        self.check_expr(e2, yt)
                    };
                    if let Some(yt) = yt {
                        if !self.types.is_any_or_error(yt) {
                            self.check_assignable(et, yt, e2.span(), None, Some(e2));
                        }
                    }
                }
                // a used yield result needs the generator's TNext: without a
                // return-type annotation it is implicitly any (7057)
                let key = node_key_expr(e);
                if in_generator && !self.yield_statement_positions.contains(&key) {
                    let annotated = self
                        .stacks
                        .fn_stack
                        .last()
                        .map(|f| f.return_type.is_some())
                        .unwrap_or(false);
                    if !annotated && self.options.no_implicit_any() {
                        self.error_at(
                            *span,
                            &gen::yield_expression_implicitly_results_in_an_any_type_because_its_containing_generator_lacks_a_return_type_annotation,
                            &[],
                        );
                    }
                }
                self.types.any
            }
            Expr::Spread { expr, .. } => self.check_expr(expr, None),
            Expr::JsxElement(j) => {
                self.check_jsx(j);
                self.types.any
            }
            Expr::ClassExpr(c) => {
                if let Some(&sym) = self.bind.decl_symbol.get(&node_key(&**c)) {
                    self.check_class_pub(c);
                    self.class_value_type(sym)
                } else {
                    self.types.any
                }
            }
            Expr::Missing { .. } => self.types.error,
        }
    }

    fn check_this_expr(&mut self, span: Span) -> TypeId {
        let in_namespace_body = self.cflags.namespace_stack.last().is_some_and(|ctx| {
            self.stacks.fn_stack.len() == ctx.fn_depth
                && self.stacks.class_stack.len() == ctx.class_depth
                && self.stacks.this_container_stack.len() == ctx.this_container_depth
        });
        if in_namespace_body {
            self.error_at(
                span,
                &gen::this_cannot_be_referenced_in_a_module_or_namespace_body,
                &[],
            );
            if self.options.no_implicit_this() {
                self.error_at(
                    span,
                    &gen::this_implicitly_has_type_any_because_it_does_not_have_a_type_annotation,
                    &[],
                );
            }
            return self.types.any;
        }

        for container in self.stacks.this_container_stack.iter().rev().copied() {
            match container.kind {
                crate::checker::ContainerKind::Arrow => continue,
                crate::checker::ContainerKind::NonArrowFn => {
                    if let Some(t) = container.explicit_this {
                        return t;
                    }
                    if self.options.no_implicit_this() {
                        self.error_at(
                            span,
                            &gen::this_implicitly_has_type_any_because_it_does_not_have_a_type_annotation,
                            &[],
                        );
                    }
                    return self.types.any;
                }
                crate::checker::ContainerKind::ClassBody
                | crate::checker::ContainerKind::Method => {
                    if let Some(t) = container.explicit_this {
                        return t;
                    }
                    if let Some(cls) = container.class_owner {
                        return self.class_this_expr_type(cls, container.is_static);
                    }
                }
                crate::checker::ContainerKind::InterfaceBody => {}
            }
        }

        self.types.any
    }

    pub(crate) fn class_this_expr_type(&mut self, cls: SymbolId, is_static: bool) -> TypeId {
        if is_static {
            return self.class_value_type(cls);
        }
        // The `this` expression carries the *polymorphic* `this` — same
        // representation as the `this` type annotation — so a value `let self:
        // this = this` is accepted, and `this` is still assignable to its class
        // via the parameter's constraint. Member access substitutes this
        // parameter with the concrete receiver, preserving generic type
        // arguments and fluent subtypes.
        let p = self.this_param_of(cls);
        self.types.intern_kind(TypeKind::TypeParam(p))
    }

    pub(crate) fn current_this_receiver_type(&mut self) -> Option<TypeId> {
        for container in self.stacks.this_container_stack.iter().rev().copied() {
            match container.kind {
                crate::checker::ContainerKind::Arrow => continue,
                crate::checker::ContainerKind::NonArrowFn => return container.explicit_this,
                crate::checker::ContainerKind::ClassBody
                | crate::checker::ContainerKind::Method => {
                    if let Some(t) = container.explicit_this {
                        return Some(t);
                    }
                    if let Some(cls) = container.class_owner {
                        return Some(self.class_this_expr_type(cls, container.is_static));
                    }
                }
                crate::checker::ContainerKind::InterfaceBody => {}
            }
        }
        None
    }

    pub fn current_class_base(&mut self) -> Option<TypeId> {
        let &cls = self.stacks.class_stack.last()?;
        let decls = self.symbol(cls).decls.clone();
        for d in decls {
            if let crate::binder::Decl::Class(c) = d {
                if let Some(h) = &c.extends {
                    return self.base_instance_type(c, h);
                }
            }
        }
        None
    }

    fn expression_extends_base_type_for_super(&mut self, expr: &'a Expr) -> Option<TypeId> {
        match expr {
            Expr::Paren { inner, .. } => self.expression_extends_base_type_for_super(inner),
            Expr::ClassExpr(_) => {
                let key = node_key_expr(expr);
                Some(
                    self.caches
                        .expr_type_cache
                        .get(&key)
                        .copied()
                        .unwrap_or_else(|| self.check_expr(expr, None)),
                )
            }
            Expr::Assertion { ty, kind, .. } if *kind != AssertionKind::Satisfies => {
                let tt = self.resolve_type(ty, self.current_scope);
                let rt = self.types.regular(tt);
                if self.types.is_any_or_error(rt) {
                    Some(rt)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub(crate) fn current_super_type(&mut self) -> Option<TypeId> {
        // Static context: `super` is the constructor function of the base class
        // (`typeof Base`), not the instance type. Walk the `this_container_stack`
        // skipping arrows, so `super.w()` inside an arrow inside a static field
        // initializer still sees the static context.
        let in_static = self
            .stacks
            .this_container_stack
            .iter()
            .rev()
            .find_map(|c| match c.kind {
                crate::checker::ContainerKind::ClassBody
                | crate::checker::ContainerKind::Method
                    if c.class_owner.is_some() =>
                {
                    Some(c.is_static)
                }
                crate::checker::ContainerKind::Arrow => None,
                _ => Some(false),
            })
            .unwrap_or(false);
        if in_static {
            self.current_class_base_statics()
        } else {
            self.current_class_base()
        }
    }

    pub(crate) fn current_class_base_statics(&mut self) -> Option<TypeId> {
        let &cls = self.stacks.class_stack.last()?;
        let decls = self.symbol(cls).decls.clone();
        for d in decls {
            if let crate::binder::Decl::Class(c) = d {
                if let Some(h) = &c.extends {
                    let et = if matches!(h.expr, Expr::Ident(_)) {
                        let key = node_key_expr(&h.expr);
                        self.caches
                            .expr_type_cache
                            .get(&key)
                            .copied()
                            .unwrap_or_else(|| self.check_expr(&h.expr, None))
                    } else {
                        self.expression_extends_base_type_for_super(&h.expr)?
                    };
                    return self.base_static_type_from_extends_type(et);
                }
            }
        }
        None
    }

    pub fn check_ident_pub(&mut self, id: &'a Ident) -> TypeId {
        self.check_ident(id)
    }

    fn check_ident(&mut self, id: &'a Ident) -> TypeId {
        if id.name.starts_with('#') {
            self.error_at(
                id.span,
                &gen::Private_identifiers_are_not_allowed_outside_class_bodies,
                &[],
            );
            return self.types.error;
        }
        if id.name == "undefined" {
            return self.types.undefined;
        }
        if let Some((kind, field_name)) = self.cflags.ctor_field_stack.last().and_then(|ctx| {
            ctx.blocked_names
                .contains(&id.name)
                .then(|| (ctx.kind, ctx.field_name.clone()))
        }) {
            match kind {
                CtorFieldContextKind::Initializer => {
                    if let Some(&cls) = self.stacks.class_stack.last() {
                        if self.symbol(cls).members.get(&id.name).is_some() {
                            self.error_at(
                                id.span,
                                &gen::Cannot_find_name_0_Did_you_mean_the_instance_member_this_0,
                                &[id.name.clone()],
                            );
                            return self.types.error;
                        }
                        if self.symbol(cls).statics.get(&id.name).is_some() {
                            let cn = self.symbol(cls).name.clone();
                            self.error_at(
                                id.span,
                                &gen::Cannot_find_name_0_Did_you_mean_the_static_member_1_0,
                                &[id.name.clone(), cn],
                            );
                            return self.types.error;
                        }
                    }
                    self.error_at(
                        id.span,
                        &gen::Initializer_of_instance_member_variable_0_cannot_reference_identifier_1_declared_in_the_constructor,
                        &[field_name, id.name.clone()],
                    );
                    return self.types.error;
                }
                CtorFieldContextKind::TypeAnnotation => {}
            }
        }
        if id.name == "arguments" && self.lookup_value(self.current_scope, "arguments").is_none() {
            let mut crossed_arrow = false;
            let mut in_function = false;
            for f in self.stacks.fn_stack.iter().rev() {
                if f.kind == FuncKind::Arrow {
                    crossed_arrow = true;
                } else {
                    in_function = true;
                    break;
                }
            }
            if in_function {
                // tsc only rejects arrow-captured `arguments` below ES2015
                // (checkIdentifier: languageVersion < ScriptTarget.ES2015);
                // ES2015+ arrows close over the outer binding without error.
                if crossed_arrow && self.options.script_target_rank() < 2 {
                    self.error_at(
                        id.span,
                        &gen::The_arguments_object_cannot_be_referenced_in_an_arrow_function_in_ES5_Consider_using_a_standard_function_expression,
                        &[],
                    );
                }
                return self
                    .global_type_symbol("IArguments")
                    .map(|s| self.types.intern_kind(TypeKind::Iface(s)))
                    .unwrap_or(self.types.any);
            }
        }
        let scope = self.current_scope;
        let Some(sym) = self.resolve_value_ident(id, scope) else {
            return self.types.error;
        };
        self.symuse.used_symbols.insert(sym);
        if self.symbol(sym).flags & flags::ALIAS != 0 {
            if let Some(crate::binder::Decl::Import(spec, _)) = self.symbol(sym).decls.first() {
                if spec.type_only {
                    self.error_at(
                        id.span,
                        &gen::_0_cannot_be_used_as_a_value_because_it_was_imported_using_import_type,
                        &[id.name.clone()],
                    );
                    return self.types.error;
                }
            }
        }
        let sym = if self.symbol(sym).flags & flags::ALIAS != 0 {
            self.resolve_alias_chain(sym)
        } else {
            sym
        };
        // type-only symbols used as values: interface/type alias
        self.symuse.used_symbols.insert(sym);
        // const enums may only appear as property/index access receivers
        if self.symbol(sym).flags & flags::ENUM != 0 && !self.enums.const_enum_ident_ok {
            let is_const_enum = self
                .symbol(sym)
                .decls
                .iter()
                .any(|d| matches!(d, crate::binder::Decl::Enum(e) if e.is_const));
            if is_const_enum {
                self.error_at(
                    id.span,
                    &gen::const_enums_can_only_be_used_in_property_or_index_access_expressions_or_the_right_hand_side_of_an_import_declaration_or_export_assignment_or_type_query,
                    &[],
                );
            }
        }
        // narrowing: the flow-graph resolver is the single engine (Stage 4);
        // reads it cannot place (type positions, out-of-context re-checks,
        // re-entrancy) read the declared type, as tsc does
        let key = RefKey(sym, Vec::new());
        // a destructuring-pattern / parenthesized assignment-target leaf is
        // a write position: declared type, no DA/auto checks (tsc
        // AssignmentKind.Definite)
        if self.cflags.pattern_target > 0 {
            return self.type_of_symbol(sym);
        }
        // definite-assignment candidates run the walk seeded with
        // `declared | undefined` instead (Stage 2, tsc initialType) — the
        // one query yields both the 2454 verdict and the narrowed type
        if let Some(t) = self.da_check_ident_read(id, sym) {
            return t;
        }
        // auto (unannotated noImplicitAny let/var) reads take the CFA-seeded
        // walk: the declared `any` would swallow the control-flow type
        // (Stage 4, tsc autoType; TS7005/7034 on capture reads)
        if let Some(t) = self.auto_check_ident_read(id, sym) {
            return t;
        }
        if let Some(t) = self.flow_type_of_read(node_key(id), &key) {
            return t;
        }
        self.type_of_symbol(sym)
    }

    fn check_array_literal(&mut self, elements: &'a [Expr], ctx: Option<TypeId>) -> TypeId {
        if self.cflags.in_const_assertion {
            let mut elems = Vec::new();
            for el in elements {
                let t = self.check_expr(el, None);
                let r = self.types.regular(t);
                elems.push(crate::types::TupleElem {
                    ty: r,
                    optional: false,
                    rest: false,
                });
            }
            return self.types.intern_kind(TypeKind::ReadonlyTuple(elems));
        }
        // contextual tuple?
        if let Some(c) = ctx {
            let tuple_ctx = match self.types.kind(c) {
                TypeKind::Tuple(es) | TypeKind::ReadonlyTuple(es) => Some(es.clone()),
                _ => None,
            };
            if let Some(t_elems) = tuple_ctx {
                let mut elems = Vec::new();
                for (i, el) in elements.iter().enumerate() {
                    let ectx = t_elems.get(i).map(|e| e.ty);
                    let t = self.check_expr(el, ectx);
                    let t = self.contextual_member_type(t, ectx);
                    elems.push(crate::types::TupleElem {
                        ty: t,
                        optional: false,
                        rest: false,
                    });
                }
                return self.types.tuple(elems);
            }
        }
        let elem_ctx = ctx.and_then(|c| self.array_element_type(c));
        let mut member_types = Vec::new();
        for el in elements {
            if let Expr::Spread { expr, .. } = el {
                let st = self.check_expr(expr, None);
                let sr = self.types.regular(st);
                let arrayish = self.types.is_any_or_error(sr)
                    || self.array_element_type(sr).is_some()
                    || matches!(
                        self.types.kind(sr),
                        TypeKind::Tuple(_) | TypeKind::ReadonlyTuple(_)
                    );
                if !arrayish {
                    let d = self.display_type(sr);
                    self.error_at(expr.span(), &gen::Type_0_is_not_an_array_type, &[d]);
                }
                let t = self.check_expr(expr, None);
                if let Some(inner) = self.array_element_type(t) {
                    member_types.push(inner);
                }
                continue;
            }
            let t = self.check_expr(el, elem_ctx);
            let t = self.contextual_member_type(t, elem_ctx);
            member_types.push(t);
        }
        let elem = if member_types.is_empty() {
            // An empty array literal with a contextual array type takes that
            // element type (`const x: number[] = []` / `x = []` where `x:
            // number[]` is `number[]`, not `never[]`), so a later `x.push(1)`
            // is valid. With no contextual array type it stays `never[]`.
            elem_ctx.unwrap_or(self.types.never)
        } else {
            self.types.union(member_types)
        };
        self.array_type(elem)
    }

    /// literal preservation: keep the (regular) literal when the contextual
    /// type contains a literal of the same kind or a type parameter; widen
    /// otherwise (mutable-location rule). `as const` keeps everything.
    fn contextual_member_type(&mut self, t: TypeId, ctx: Option<TypeId>) -> TypeId {
        let r = self.types.regular(t);
        if self.cflags.in_const_assertion {
            return r;
        }
        match ctx {
            Some(c) if self.type_could_keep_literal(c) => r,
            _ => self.types.widen_literal(r),
        }
    }

    pub(crate) fn type_could_keep_literal(&mut self, c: TypeId) -> bool {
        match self.types.kind(c).clone() {
            TypeKind::StrLit(_)
            | TypeKind::NumLit(_)
            | TypeKind::BigIntLit(_)
            | TypeKind::BoolLit(_) => true,
            TypeKind::TypeParam(_) => true,
            TypeKind::Union(ms) => ms.iter().any(|&m| self.type_could_keep_literal(m)),
            _ => false,
        }
    }

    fn check_computed_name_grammar(&mut self, name: &'a PropName) {
        if let PropName::Computed { expr, .. } = name {
            self.check_expr(expr, None);
            if matches!(
                &**expr,
                Expr::Binary {
                    op: BinOp::Comma,
                    ..
                }
            ) {
                self.error_at(
                    expr.span(),
                    &gen::A_comma_expression_is_not_allowed_in_a_computed_property_name,
                    &[],
                );
            }
        }
    }

    fn check_objlit_accessor_duplicates(&mut self, props: &'a [ObjectProp]) {
        use std::collections::HashMap as Map;
        let mut acc: Map<String, Vec<(bool, Span)>> = Map::new(); // (is_get, span)
        for p in props {
            if let ObjectProp::Method(f) = p {
                let is_get = matches!(f.kind, FuncKind::Getter);
                let is_set = matches!(f.kind, FuncKind::Setter);
                if is_get || is_set {
                    if let Some(n) = f.name.as_ref().and_then(|x| x.text()) {
                        acc.entry(n)
                            .or_default()
                            .push((is_get, f.name.as_ref().unwrap().span()));
                    }
                }
            }
        }
        // 1119: plain property + accessor sharing a name
        let mut plain: Vec<(String, Span)> = Vec::new();
        for p in props {
            match p {
                ObjectProp::Property { name, .. } => {
                    if let Some(n) = name.text() {
                        plain.push((n, name.span()));
                    }
                }
                ObjectProp::Shorthand { name, .. } => plain.push((name.name.clone(), name.span)),
                _ => {}
            }
        }
        for (name, sites) in &acc {
            if let Some((_, pspan)) = plain.iter().find(|(n, _)| n == name) {
                self.error_at(*pspan, &gen::Duplicate_identifier_0, &[name.clone()]);
                for (i, (_, aspan)) in sites.iter().enumerate() {
                    if i == 0 {
                        self.error_at(
                            *aspan,
                            &gen::An_object_literal_cannot_have_property_and_accessor_with_the_same_name,
                            &[],
                        );
                    }
                    self.error_at(*aspan, &gen::Duplicate_identifier_0, &[name.clone()]);
                }
            }
        }
        for (name, sites) in acc {
            let gets = sites.iter().filter(|(g, _)| *g).count();
            let sets = sites.iter().filter(|(g, _)| !*g).count();
            if gets > 1 || sets > 1 {
                let mut seen_get = false;
                let mut seen_set = false;
                for (is_get, span) in &sites {
                    self.error_at(*span, &gen::Duplicate_identifier_0, &[name.clone()]);
                    let dup = if *is_get { seen_get } else { seen_set };
                    if dup {
                        self.error_at(
                            *span,
                            &gen::An_object_literal_cannot_have_multiple_get_Slashset_accessors_with_the_same_name,
                            &[],
                        );
                    }
                    if *is_get {
                        seen_get = true;
                    } else {
                        seen_set = true;
                    }
                }
            }
        }
    }

    fn check_objlit_accessor_implicit_any(&mut self, props: &'a [ObjectProp]) {
        use std::collections::HashMap as Map;

        fn value_param(f: &FunctionLike) -> Option<&Param> {
            f.params.iter().find(|p| {
                !p.name
                    .as_ident()
                    .map(|id| id.name == "this")
                    .unwrap_or(false)
            })
        }

        fn is_private_name(name: &PropName) -> bool {
            matches!(name, PropName::Ident(id) if id.name.starts_with('#'))
        }

        let accessor_key = |this: &Self, name: &PropName| -> Option<String> {
            if is_private_name(name) {
                return None;
            }
            match name {
                PropName::Computed { .. } => {
                    let display = this.display_prop_name_for_error(name);
                    (!display.is_empty()).then_some(display)
                }
                _ => name.text(),
            }
        };

        let mut pairs: Map<String, (Option<&'a FunctionLike>, Option<&'a FunctionLike>)> =
            Map::new();
        for p in props {
            let ObjectProp::Method(f) = p else { continue };
            if !matches!(f.kind, FuncKind::Getter | FuncKind::Setter) {
                continue;
            }
            let Some(name) = f.name.as_ref() else {
                continue;
            };
            let Some(text) = accessor_key(self, name) else {
                continue;
            };
            let entry = pairs.entry(text).or_default();
            match f.kind {
                FuncKind::Getter => entry.0 = Some(f),
                FuncKind::Setter => entry.1 = Some(f),
                _ => {}
            }
        }

        for (_name, (getter, setter)) in pairs {
            let getter_has_type = getter
                .map(|g| g.return_type.is_some() || g.body.is_some())
                .unwrap_or(false);
            if let Some(g) = getter {
                if !getter_has_type {
                    if let Some(name) = &g.name {
                        let display_name = self.display_prop_name_for_error(name);
                        if self.report_once_node(7033, node_key(g)) {
                            if self.options.no_implicit_any() {
                                self.error_at(
                                    name.span(),
                                    &gen::Property_0_implicitly_has_type_any_because_its_get_accessor_lacks_a_return_type_annotation,
                                    &[display_name],
                                );
                            } else {
                                self.suggestion_at(
                                    name.span(),
                                    &gen::Property_0_implicitly_has_type_any_because_its_get_accessor_lacks_a_return_type_annotation,
                                    &[display_name],
                                );
                            }
                        }
                    }
                }
            }
            if !getter_has_type {
                if let Some(s) = setter {
                    let setter_has_type = value_param(s).and_then(|p| p.ty.as_ref()).is_some();
                    if !setter_has_type {
                        if let Some(name) = &s.name {
                            let display_name = self.display_prop_name_for_error(name);
                            if self.report_once_node(7032, node_key(s)) {
                                if self.options.no_implicit_any() {
                                    self.error_at(
                                        name.span(),
                                        &gen::Property_0_implicitly_has_type_any_because_its_set_accessor_lacks_a_parameter_type_annotation,
                                        &[display_name],
                                    );
                                } else {
                                    self.suggestion_at(
                                        name.span(),
                                        &gen::Property_0_implicitly_has_type_any_because_its_set_accessor_lacks_a_parameter_type_annotation,
                                        &[display_name],
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn check_object_literal(
        &mut self,
        props: &'a [ObjectProp],
        span: Span,
        ctx: Option<TypeId>,
    ) -> TypeId {
        // Consume the skip-context-sensitive flag and reset it immediately, so a
        // context-sensitive function property of *this* literal is omitted during
        // inference while nested expressions are still checked normally.
        let skip_cs = std::mem::replace(&mut self.cflags.skip_ctx_sensitive, false);
        // `this` type for this literal's methods, from a `ThisType<T>` in the
        // contextual type. Staged per-method just before checking each non-arrow
        // method/function-expression body (see below).
        let objlit_this = ctx.and_then(|c| self.this_type_from_contextual(c));
        let mut paired_setters = std::collections::HashSet::new();
        let mut getter_return_ctx = std::collections::HashMap::new();
        for p in props {
            let ObjectProp::Method(setter) = p else {
                continue;
            };
            if setter.kind != FuncKind::Setter {
                continue;
            }
            let Some(name) = setter.name.as_ref().and_then(|n| n.text()) else {
                continue;
            };
            let has_getter = props.iter().any(|q| {
                let ObjectProp::Method(getter) = q else {
                    return false;
                };
                getter.kind == FuncKind::Getter
                    && getter.name.as_ref().and_then(|n| n.text()).as_deref() == Some(name.as_str())
            });
            if has_getter {
                paired_setters.insert(node_key(&**setter));
                if let Some(value_param) = setter.params.iter().find(|p| {
                    !p.name
                        .as_ident()
                        .map(|id| id.name == "this")
                        .unwrap_or(false)
                }) {
                    if let Some(ty) = &value_param.ty {
                        let scope = self
                            .bind
                            .node_scope
                            .get(&node_key(&**setter))
                            .copied()
                            .unwrap_or(self.current_scope);
                        let setter_value_ty = self.resolve_type(ty, scope);
                        for q in props {
                            let ObjectProp::Method(getter) = q else {
                                continue;
                            };
                            if getter.kind == FuncKind::Getter
                                && getter.return_type.is_none()
                                && getter.name.as_ref().and_then(|n| n.text()).as_deref()
                                    == Some(name.as_str())
                            {
                                getter_return_ctx.insert(node_key(&**getter), setter_value_ty);
                            }
                        }
                    }
                }
            }
        }
        self.check_objlit_accessor_duplicates(props);
        self.check_objlit_accessor_duplicates(props);
        self.check_objlit_accessor_implicit_any(props);
        // 2783: a literal prop later overwritten by a spread
        {
            let mut lit_props: Vec<(String, Span)> = Vec::new();
            for p in props {
                match p {
                    ObjectProp::Property { name, span, .. } => {
                        if let Some(n) = name.text() {
                            lit_props.push((n, *span));
                        }
                    }
                    ObjectProp::Shorthand { name, eq_span, .. } => {
                        if let Some(es) = eq_span {
                            self.error_at(
                            *es,
                            &gen::Did_you_mean_to_use_a_Colon_An_can_only_follow_a_property_name_when_the_containing_object_literal_is_part_of_a_destructuring_pattern,
                            &[],
                        );
                        }
                        lit_props.push((name.name.clone(), name.span));
                    }
                    ObjectProp::Spread {
                        expr,
                        span: spread_span,
                    } => {
                        let t = self.check_expr(expr, None);
                        if let Some(sid) = self.shape_of_type(t) {
                            let names: Vec<String> = self
                                .types
                                .shape(sid)
                                .props
                                .iter()
                                .map(|pp| pp.name.clone())
                                .collect();
                            let mut still_unoverwritten = Vec::new();
                            for (n, sp) in lit_props.drain(..) {
                                if names.contains(&n) {
                                    let related = vec![RelatedInfo {
                                        file: Some(self.current_file),
                                        start: spread_span.start,
                                        length: spread_span.len(),
                                        message: MessageChain::new(
                                            &gen::This_spread_always_overwrites_this_property,
                                            &[],
                                        ),
                                    }];
                                    self.error_at_with_related(
                                        sp,
                                        &gen::_0_is_specified_more_than_once_so_this_usage_will_be_overwritten,
                                        &[n.clone()],
                                        related,
                                    );
                                } else {
                                    still_unoverwritten.push((n, sp));
                                }
                            }
                            lit_props = still_unoverwritten;
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut contribs: Vec<Contribution> = Vec::new();
        let mut spans: Vec<(String, Span)> = Vec::new();
        let mut seen: Vec<(String, Span)> = Vec::new();
        for p in props {
            match p {
                ObjectProp::Property {
                    name,
                    value,
                    question_span,
                    ..
                } => {
                    if let Some(qs) = question_span {
                        self.error_at(*qs, &gen::An_object_member_cannot_be_declared_optional, &[]);
                    }
                    self.check_computed_name_grammar(name);
                    // computed names with literal keys check against the
                    // contextual property type (2418)
                    if let PropName::Computed { expr: kexpr, .. } = name {
                        let kt = self.check_expr(kexpr, None);
                        let kr = self.types.regular(kt);
                        if let TypeKind::StrLit(kname) = self.types.kind(kr).clone() {
                            let kname = kname.to_str_lossy().into_owned();
                            let pctx = ctx.and_then(|c| self.contextual_prop_type(c, &kname));
                            let vt = self.check_expr(value, pctx);
                            if let Some(pt) = pctx {
                                let vr = self.types.regular(vt);
                                if !self.types.is_any_or_error(vr)
                                    && !self.types.is_error(pt)
                                    && !self.is_assignable_to(vr, pt)
                                {
                                    let vd = self.display_type_for_error(vr, pt);
                                    let pd = self.display_type(pt);
                                    self.error_at(
                                        name.span(),
                                        &gen::Type_of_computed_property_s_value_is_0_which_is_not_assignable_to_type_1,
                                        &[vd, pd],
                                    );
                                }
                            }
                            // the contextual type wins for the resulting shape
                            // so the outer relation doesn't double-report
                            let vr = self.types.regular(vt);
                            let prop_ty = pctx.unwrap_or_else(|| self.types.widen_literal(vr));
                            contribs.push(Contribution::Own(PropInfo {
                                name: kname,
                                ty: prop_ty,
                                optional: false,
                                readonly: false,
                                is_method: false,
                                symbol: None,
                            }));
                            continue;
                        }
                    }
                    let Some(n) = name.text() else {
                        if let Some(pctx) = self.contextual_type_for_computed_name(ctx, name) {
                            self.check_expr(value, Some(pctx));
                            continue;
                        }
                        self.check_expr(value, None);
                        continue;
                    };
                    self.check_dup_object_prop(&mut seen, &n, name.span());
                    // During type-argument inference, omit a context-sensitive
                    // function property: typing it now would pin its parameters
                    // to the still-unresolved type parameter (polluting inference
                    // and checking its body too early). An array property holding
                    // context-sensitive functions is omitted likewise. Both are
                    // checked when the literal is re-typed with the final type
                    // arguments.
                    if skip_cs
                        && (self.is_context_sensitive_arg(value)
                            || self.array_has_context_sensitive(value))
                    {
                        continue;
                    }
                    let pctx = ctx.and_then(|c| self.contextual_prop_type(c, &n));
                    // Propagate the inference skip into a nested object-literal
                    // property so its own context-sensitive function properties
                    // are held back too (`{ inner: { fn: v => … } }`).
                    if skip_cs && matches!(value, Expr::Object { .. }) {
                        self.cflags.skip_ctx_sensitive = true;
                    }
                    let t = self.check_expr(value, pctx);
                    // Nested excess properties: a fresh object-literal value whose
                    // regular form is assignable but whose fresh form is not carries
                    // properties absent from the contextual type. The outer relation
                    // regularizes this property and misses it, so report it here.
                    if let Some(pt) = pctx {
                        let tr = self.types.regular(t);
                        if self.types.is_fresh(t)
                            && matches!(self.types.kind(t), TypeKind::Anon(_))
                            && !self.types.is_any_or_error(tr)
                            && !self.types.is_error(pt)
                            && self.is_assignable_to(tr, pt)
                            && !self.is_assignable_to(t, pt)
                        {
                            self.check_assignable(t, pt, value.span(), None, Some(value));
                        }
                    }
                    let t = self.contextual_member_type(t, pctx);
                    contribs.push(Contribution::Own(PropInfo {
                        name: n.clone(),
                        ty: t,
                        optional: false,
                        readonly: self.cflags.in_const_assertion,
                        is_method: false,
                        symbol: None,
                    }));
                    spans.push((n, name.span()));
                }
                ObjectProp::Shorthand { name, eq_span, .. } => {
                    if let Some(es) = eq_span {
                        self.error_at(
                            *es,
                            &gen::Did_you_mean_to_use_a_Colon_An_can_only_follow_a_property_name_when_the_containing_object_literal_is_part_of_a_destructuring_pattern,
                            &[],
                        );
                        continue;
                    }
                    if self.lookup_value(self.current_scope, &name.name).is_none() {
                        self.error_at(
                            name.span,
                            &gen::No_value_exists_in_scope_for_the_shorthand_property_0_Either_declare_one_or_provide_an_initializer,
                            &[name.name.clone()],
                        );
                        contribs.push(Contribution::Own(PropInfo {
                            name: name.name.clone(),
                            ty: self.types.error,
                            optional: false,
                            readonly: false,
                            is_method: false,
                            symbol: None,
                        }));
                        continue;
                    }
                    self.check_dup_object_prop(&mut seen, &name.name, name.span);
                    let t = self.check_ident(name);
                    let t = self.contextual_member_type(t, None);
                    contribs.push(Contribution::Own(PropInfo {
                        name: name.name.clone(),
                        ty: t,
                        optional: false,
                        readonly: false,
                        is_method: false,
                        symbol: None,
                    }));
                    spans.push((name.name.clone(), name.span));
                }
                ObjectProp::Method(f) => {
                    let n = f.name.as_ref().and_then(|nm| nm.text());
                    if n.is_none() {
                        let pctx = f
                            .name
                            .as_ref()
                            .and_then(|name| self.contextual_type_for_computed_name(ctx, name));
                        if let Some(pctx) = pctx {
                            self.cflags.pending_objlit_this = objlit_this;
                            self.check_function_expression(f, Some(pctx));
                            self.cflags.pending_objlit_this = None;
                        } else if ctx.is_none() && f.kind == FuncKind::Setter {
                            for p in &f.params {
                                if p.name
                                    .as_ident()
                                    .map(|id| id.name == "this")
                                    .unwrap_or(false)
                                {
                                    continue;
                                }
                                if p.ty.is_none() {
                                    self.report_implicit_any_param(p);
                                }
                            }
                        }
                        continue;
                    }
                    // During type-argument inference omit a context-sensitive
                    // method *only when this literal has a contextual `this`*
                    // (a `ThisType<T>` in its contextual type): checking its body
                    // now would pin `this` to the still-unresolved `ThisType<T>`
                    // (`T` a free parameter), spuriously failing member access on
                    // `this`. It is re-typed with the resolved type arguments in
                    // inference pass 2, at which point `this` is concrete. A method
                    // in a literal *without* a contextual `this` is left alone
                    // (its `this` does not depend on the type arguments).
                    if skip_cs
                        && objlit_this.is_some()
                        && self.objlit_method_is_context_sensitive(f)
                    {
                        continue;
                    }
                    let is_accessor = matches!(f.kind, FuncKind::Getter | FuncKind::Setter);
                    if !is_accessor {
                        if let (Some(name), Some(n)) = (f.name.as_ref(), n.as_ref()) {
                            self.check_dup_object_prop(&mut seen, n, name.span());
                        }
                    }
                    let pctx = ctx
                        .and_then(|c| {
                            n.as_ref()
                                .and_then(|name| self.contextual_prop_type(c, name))
                        })
                        .or_else(|| getter_return_ctx.get(&node_key(&**f)).copied());
                    // stage the contextual `this` (from the literal's `ThisType`)
                    // for this method's body; consumed once by check_function_body.
                    self.cflags.pending_objlit_this = objlit_this;
                    let t = if f.body.is_none() {
                        self.with_suppressed_next_function_implicit_any_return(|this| {
                            if paired_setters.contains(&node_key(&**f)) {
                                this.with_suppressed_next_function_implicit_any_params(|this| {
                                    this.check_function_expression(f, pctx)
                                })
                            } else {
                                this.check_function_expression(f, pctx)
                            }
                        })
                    } else if paired_setters.contains(&node_key(&**f)) {
                        self.with_suppressed_next_function_implicit_any_params(|this| {
                            this.check_function_expression(f, pctx)
                        })
                    } else {
                        self.check_function_expression(f, pctx)
                    };
                    self.cflags.pending_objlit_this = None;
                    // an accessor contributes a plain data property: a getter's
                    // type is its return type, a setter's is its parameter type.
                    let (prop_ty, is_method) = match f.kind {
                        FuncKind::Getter => {
                            let sigs = self.call_signatures_of(t);
                            let rty = sigs.first().map(|&s| self.sig_return(s)).unwrap_or(t);
                            (rty, false)
                        }
                        FuncKind::Setter => {
                            let sigs = self.call_signatures_of(t);
                            let pty = sigs
                                .first()
                                .and_then(|&s| self.types.sig(s).params.first().map(|p| p.ty))
                                .unwrap_or(self.types.any);
                            (pty, false)
                        }
                        _ => (t, true),
                    };
                    let n = n.unwrap();
                    contribs.push(Contribution::Own(PropInfo {
                        name: n.clone(),
                        ty: prop_ty,
                        optional: false,
                        readonly: false,
                        is_method,
                        symbol: None,
                    }));
                    spans.push((n, f.name.as_ref().unwrap().span()));
                }
                ObjectProp::Spread { expr, .. } => {
                    let t = self.check_expr(expr, None);
                    let mut treg = self.types.regular(t);
                    if self.types.is_any_or_error(treg) {
                        continue;
                    }
                    // `...(cond && { x })` spreads `false | { x }` now that
                    // `&&` keeps the definitely-falsy left: those constituents
                    // contribute nothing at runtime, so drop them before
                    // distributing — also what keeps the repeated-null-check
                    // perf fixture linear instead of forking shapes per
                    // spread. (tsc instead merges to all-optional props via
                    // tryMergeUnionOfObjectTypeAndEmptyObject; divergence
                    // kept for corpus stability.)
                    if let TypeKind::Union(members) = self.types.kind(treg).clone() {
                        let kept: Vec<TypeId> = members
                            .into_iter()
                            .filter(|&m| {
                                !matches!(
                                    self.types.kind(m).clone(),
                                    TypeKind::BoolLit(false) | TypeKind::StrLit(_) | TypeKind::NumLit(_) | TypeKind::BigIntLit(_)
                                ) || self.type_facts(m) & crate::checker::operators::facts::TRUTHY != 0
                            })
                            .collect();
                        treg = self.types.union(kept);
                    }
                    // union spread distributes: { ...(A | B) } => { ...A } | { ...B }
                    if let TypeKind::Union(members) = self.types.kind(treg).clone() {
                        let mut member_props: Vec<Vec<PropInfo>> = Vec::new();
                        let mut ok = true;
                        for m in members {
                            if matches!(self.types.kind(m), TypeKind::Null | TypeKind::Undefined) {
                                member_props.push(Vec::new());
                            } else if let Some(sid) = self.shape_of_type(m) {
                                member_props.push(self.types.shape(sid).props.clone());
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        if !ok {
                            let s = expr.span().start as usize - 3;
                            self.error_at(
                                Span::new(s, s + 3),
                                &gen::Spread_types_may_only_be_created_from_object_types,
                                &[],
                            );
                        } else if !member_props.is_empty() {
                            contribs.push(Contribution::SpreadUnion(member_props));
                        }
                        continue;
                    }
                    let spreadable = self.shape_of_type(treg).is_some()
                        && !matches!(
                            self.types.kind(treg),
                            TypeKind::String
                                | TypeKind::Number
                                | TypeKind::Bigint
                                | TypeKind::EsSymbol
                                | TypeKind::StrLit(_)
                                | TypeKind::NumLit(_)
                                | TypeKind::BigIntLit(_)
                                | TypeKind::BoolLit(_)
                        );
                    if !spreadable {
                        let s = expr.span().start as usize - 3;
                        self.error_at(
                            Span::new(s, s + 3),
                            &gen::Spread_types_may_only_be_created_from_object_types,
                            &[],
                        );
                    }
                    if let Some(sid) = self.shape_of_type(t) {
                        contribs.push(Contribution::Spread(self.types.shape(sid).props.clone()));
                    }
                }
            }
        }
        let _ = span;
        // Partition contributions into ordered groups (a maximal run of explicit
        // members forms one group preserving source order; each spread is its own
        // group), then fold left-to-right with getSpreadType ordering: each newer
        // group's properties are emitted first, then the earlier ones not already
        // present. Union spreads fork the in-progress shapes over their members.
        enum Group {
            Concrete(Vec<PropInfo>),
            Union(Vec<Vec<PropInfo>>),
        }
        let mut groups: Vec<Group> = Vec::new();
        let mut cur: Vec<PropInfo> = Vec::new();
        for c in contribs {
            match c {
                Contribution::Own(p) => cur.push(p),
                Contribution::Spread(ps) => {
                    if !cur.is_empty() {
                        groups.push(Group::Concrete(std::mem::take(&mut cur)));
                    }
                    groups.push(Group::Concrete(ps));
                }
                Contribution::SpreadUnion(members) => {
                    if !cur.is_empty() {
                        groups.push(Group::Concrete(std::mem::take(&mut cur)));
                    }
                    groups.push(Group::Union(members));
                }
            }
        }
        if !cur.is_empty() {
            groups.push(Group::Concrete(cur));
        }

        let mut shapes: Vec<Vec<PropInfo>> = vec![Vec::new()];
        for g in &groups {
            match g {
                Group::Concrete(ps) => {
                    let mut next: Vec<Vec<PropInfo>> = Vec::with_capacity(shapes.len());
                    for sh in &shapes {
                        next.push(self.spread_combine(sh, ps));
                    }
                    shapes = next;
                }
                Group::Union(members) => {
                    let mut next: Vec<Vec<PropInfo>> =
                        Vec::with_capacity(shapes.len() * members.len());
                    for sh in &shapes {
                        for mp in members {
                            next.push(self.spread_combine(sh, mp));
                        }
                    }
                    shapes = next;
                }
            }
        }
        let mk = |slf: &mut Self, props: Vec<PropInfo>| {
            let mut shape = Shape::default();
            shape.props = props;
            let sid = slf.types.alloc_shape(shape);
            let regular = slf.types.alloc(TypeKind::Anon(sid));
            slf.types.fresh(regular)
        };
        if shapes.len() == 1 {
            let fresh = mk(self, shapes.pop().unwrap());
            self.caches.fresh_obj_props.insert(fresh, spans);
            fresh
        } else {
            let members: Vec<TypeId> = shapes.into_iter().map(|props| mk(self, props)).collect();
            self.types.union(members)
        }
    }

    /// getSpreadType(left, right): the result lists `right`'s properties first
    /// (in source order), then `left`'s properties that `right` does not have.
    /// An overlapping property takes `right`'s position; if `right`'s property is
    /// OPTIONAL the type is the union of both (required unless both are optional,
    /// since the earlier value survives when the optional one is absent), and a
    /// REQUIRED `right` property fully overrides.
    fn spread_combine(&mut self, left: &[PropInfo], right: &[PropInfo]) -> Vec<PropInfo> {
        let mut out: Vec<PropInfo> = Vec::with_capacity(left.len() + right.len());
        for rp in right {
            if let Some(lp) = left.iter().find(|q| q.name == rp.name) {
                if rp.optional {
                    let opt = lp.optional && rp.optional;
                    let mut ty = self.types.union(vec![lp.ty, rp.ty]);
                    if !opt {
                        let undef = self.types.undefined;
                        ty = self.types.filter_union(ty, |_t, m| m != undef);
                    }
                    out.push(PropInfo {
                        name: rp.name.clone(),
                        ty,
                        optional: opt,
                        readonly: lp.readonly || rp.readonly,
                        is_method: rp.is_method,
                        symbol: rp.symbol,
                    });
                } else {
                    out.push(rp.clone());
                }
            } else {
                out.push(rp.clone());
            }
        }
        for lp in left {
            if !right.iter().any(|q| q.name == lp.name) {
                out.push(lp.clone());
            }
        }
        out
    }

    fn check_dup_object_prop(&mut self, seen: &mut Vec<(String, Span)>, name: &str, span: Span) {
        if seen.iter().any(|(n, _)| n == name) {
            self.error_at(
                span,
                &gen::An_object_literal_cannot_have_multiple_properties_with_the_same_name,
                &[],
            );
        }
        seen.push((name.to_string(), span));
    }

    /// The call signature to use as context for a function expression. Looks
    /// through a union for a single callable constituent, so an optional
    /// callback parameter typed `((v: T) => U) | undefined | null` (as in
    /// `Promise.then`) still types the arrow's parameters.
    pub(crate) fn contextual_call_sig(&mut self, c: TypeId) -> Option<crate::types::SigId> {
        if let Some(sid) = self.shape_of_type(c) {
            let sigs = self.types.shape(sid).call_sigs.clone();
            if sigs.len() == 1 {
                return Some(sigs[0]);
            }
            if sigs.len() > 1 {
                return None;
            }
        }
        if let TypeKind::Union(members) = self.types.kind(c).clone() {
            let mut found: Option<crate::types::SigId> = None;
            for &m in &members {
                for s in self.call_signatures_of(m) {
                    if found.is_some() {
                        return None; // ambiguous: more than one callable member
                    }
                    found = Some(s);
                }
            }
            return found;
        }
        None
    }

    pub(crate) fn contextual_param_call_sig(&mut self, c: TypeId) -> Option<crate::types::SigId> {
        if let Some(sig) = self.contextual_call_sig(c) {
            return Some(sig);
        }
        let apparent = self.apparent_type(c);
        if apparent != c {
            return self.contextual_call_sig(apparent);
        }
        None
    }

    /// the finite set of constituent types of `t`, or `None` when `t` is not a
    /// finite domain (so exhaustiveness cannot be proven).
    pub(crate) fn exhaustive_members(&mut self, t: TypeId) -> Option<Vec<TypeId>> {
        match self.types.kind(t).clone() {
            TypeKind::Union(ms) => {
                let mut out = Vec::new();
                for m in ms {
                    out.extend(self.exhaustive_members(m)?);
                }
                Some(out)
            }
            TypeKind::StrLit(_)
            | TypeKind::NumLit(_)
            | TypeKind::BoolLit(_)
            | TypeKind::EnumMember(_) => Some(vec![t]),
            TypeKind::EnumType(sym) | TypeKind::EnumObject(sym) => {
                let members: Vec<SymbolId> =
                    self.symbol(sym).members.0.iter().map(|(_, m)| *m).collect();
                Some(
                    members
                        .into_iter()
                        .map(|m| self.types.intern_kind(TypeKind::EnumMember(m)))
                        .collect(),
                )
            }
            _ => None,
        }
    }

    // ── member access ───────────────────────────────────────────────────────

    /// JSX without `--jsx` / JSX.IntrinsicElements: 17004 at each opening
    /// element tag, 7026 at opening AND closing intrinsic tags
    fn check_jsx(&mut self, j: &'a JsxElement) {
        let is_intrinsic = j
            .tag
            .as_ref()
            .map(|t| {
                t.name
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_lowercase())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if is_intrinsic && self.options.no_implicit_any() {
            self.error_at(
                j.span,
                &gen::JSX_element_implicitly_has_type_any_because_no_interface_JSX_0_exists,
                &["IntrinsicElements".to_string()],
            );
        }
        if let Some(tag) = &j.tag {
            // component tags resolve as values
            if !is_intrinsic {
                self.check_ident_pub(tag);
            }
        }
        self.error_at(
            j.span,
            &gen::Cannot_use_JSX_unless_the_jsx_flag_is_provided,
            &[],
        );
        for a in &j.attrs {
            if let Some(v) = &a.value {
                self.check_expr(v, None);
            }
        }
        for c in &j.children {
            match c {
                JsxChild::Element(e) => self.check_jsx(e),
                JsxChild::Expr(e) => {
                    self.check_expr(e, None);
                    if let Expr::Binary {
                        op: BinOp::Comma, ..
                    } = e
                    {
                        self.error_at(
                            e.span(),
                            &gen::JSX_expressions_may_not_use_the_comma_operator_Did_you_mean_to_write_an_array,
                            &[],
                        );
                    }
                }
                JsxChild::Text => {}
            }
        }
        if let (Some(cspan), true) = (j.closing_span, is_intrinsic) {
            if self.options.no_implicit_any() {
                self.error_at(
                    cspan,
                    &gen::JSX_element_implicitly_has_type_any_because_no_interface_JSX_0_exists,
                    &["IntrinsicElements".to_string()],
                );
            }
        }
    }

    pub fn awaited_type_pub(&mut self, t: TypeId) -> TypeId {
        self.awaited_type(t)
    }

    /// The yield type of the enclosing generator (the first type argument of its
    /// declared `Generator`/`Iterator`/`IterableIterator` return type), if any.
    fn current_yield_type(&mut self) -> Option<TypeId> {
        let rt = self.stacks.fn_stack.last().and_then(|f| f.return_type)?;
        match self.types.kind(rt).clone() {
            TypeKind::Ref(sym, args) if !args.is_empty() => {
                let name = self.symbol(sym).name.clone();
                if matches!(
                    name.as_str(),
                    "Generator"
                        | "IterableIterator"
                        | "Iterator"
                        | "Iterable"
                        | "AsyncGenerator"
                        | "AsyncIterableIterator"
                        | "AsyncIterator"
                        | "AsyncIterable"
                ) {
                    Some(args[0])
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn awaited_type(&mut self, t: TypeId) -> TypeId {
        if let TypeKind::Ref(sym, args) = self.types.kind(t).clone() {
            if self.symbol(sym).name == "Promise" && args.len() == 1 {
                return args[0];
            }
        }
        t
    }
}

/// A pending contribution to an object-literal shape, recorded in source order
/// so the shape can be folded with union-spread distribution applied at the end.
enum Contribution {
    /// an explicit/shorthand/method property: fully overrides on collision
    Own(PropInfo),
    /// a spread of a concrete object: each property merged via getSpreadType
    Spread(Vec<PropInfo>),
    /// a spread of a union: fork the in-progress shapes over the members
    SpreadUnion(Vec<Vec<PropInfo>>),
}

pub(crate) fn expected_args_display(s: &crate::types::Signature) -> String {
    let max = s.params.len() as u32;
    if s.min_args == max {
        max.to_string()
    } else {
        format!("{}-{}", s.min_args, max)
    }
}

pub(crate) fn node_key_expr(e: &Expr) -> usize {
    e as *const Expr as usize
}

/// yield expression spans within an initializer (no nested functions)
pub(crate) fn collect_yield_spans(e: &crate::ast::Expr, out: &mut Vec<Span>) {
    use crate::ast::Expr as E;
    match e {
        E::Yield { span, .. } => out.push(*span),
        E::Binary { left, right, .. } => {
            collect_yield_spans(left, out);
            collect_yield_spans(right, out);
        }
        E::Paren { inner, .. } => collect_yield_spans(inner, out),
        _ => {}
    }
}

pub(crate) fn collect_await_spans(e: &crate::ast::Expr, out: &mut Vec<Span>) {
    use crate::ast::Expr as E;
    match e {
        E::Await { span, .. } => out.push(Span::new(span.start as usize, span.start as usize + 5)),
        E::Binary { left, right, .. } => {
            collect_await_spans(left, out);
            collect_await_spans(right, out);
        }
        E::Paren { inner, .. } => collect_await_spans(inner, out),
        _ => {}
    }
}

/// shallow test for a `super` mention (constructor-argument grammar)
pub(crate) fn expr_mentions_super(e: &crate::ast::Expr) -> bool {
    use crate::ast::Expr as E;
    match e {
        E::Super { .. } => true,
        E::PropAccess { obj, .. } | E::ElemAccess { obj, .. } => expr_mentions_super(obj),
        E::Call { callee, .. } => expr_mentions_super(callee),
        E::Paren { inner, .. } => expr_mentions_super(inner),
        E::Binary { left, right, .. } => expr_mentions_super(left) || expr_mentions_super(right),
        _ => false,
    }
}

/// does the expression contain a `?.` link on its access path?
pub(crate) fn expr_contains_optional_chain(e: &crate::ast::Expr) -> bool {
    use crate::ast::Expr as E;
    match e {
        E::PropAccess {
            obj, question_dot, ..
        } => *question_dot || expr_contains_optional_chain(obj),
        E::ElemAccess { obj, .. } => expr_contains_optional_chain(obj),
        E::Paren { inner, .. } => expr_contains_optional_chain(inner),
        _ => false,
    }
}

pub fn collect_binding_idents_pub<'b>(
    b: &'b crate::ast::Binding,
    out: &mut Vec<&'b crate::ast::Ident>,
) {
    collect_binding_idents(b, out)
}

/// shallow identifier references in an expression (no nested functions)
pub(crate) fn collect_idents<'b>(e: &'b crate::ast::Expr, out: &mut Vec<&'b crate::ast::Ident>) {
    use crate::ast::Expr as E;
    match e {
        E::Ident(id) => out.push(id),
        E::Binary { left, right, .. } => {
            collect_idents(left, out);
            collect_idents(right, out);
        }
        E::Unary { operand, .. } => collect_idents(operand, out),
        E::Paren { inner, .. } => collect_idents(inner, out),
        E::Call { callee, args, .. } => {
            collect_idents(callee, out);
            for a in args {
                collect_idents(a, out);
            }
        }
        E::PropAccess { obj, .. } => collect_idents(obj, out),
        E::Cond {
            cond,
            when_true,
            when_false,
            ..
        } => {
            collect_idents(cond, out);
            collect_idents(when_true, out);
            collect_idents(when_false, out);
        }
        _ => {}
    }
}

/// leaf identifiers of a binding pattern, in source order
pub(crate) fn collect_binding_idents<'b>(
    b: &'b crate::ast::Binding,
    out: &mut Vec<&'b crate::ast::Ident>,
) {
    match b {
        crate::ast::Binding::Ident(id) => out.push(id),
        crate::ast::Binding::Object(p) => {
            for prop in &p.props {
                collect_binding_idents(&prop.binding, out);
            }
            if let Some(rest) = &p.rest {
                collect_binding_idents(rest, out);
            }
        }
        crate::ast::Binding::Array(p) => {
            for el in p.elements.iter().flatten() {
                collect_binding_idents(&el.binding, out);
            }
        }
    }
}
