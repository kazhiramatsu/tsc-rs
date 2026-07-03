//! Lazy symbol typing + type-node resolution + member shapes.

use super::{Checker, CtorFieldContextKind, Slot};
use crate::ast::*;
use crate::binder::{flags, Decl, ScopeId, SymbolId};
use crate::diagnostics::gen;
use crate::types::{
    IndexInfo, ParamInfo, PropInfo, Shape, ShapeId, Signature, TupleElem, TypeId, TypeKind,
};
use std::collections::HashMap;

pub type Mapper = HashMap<SymbolId, TypeId>;

fn collect_return_exprs<'b>(stmts: &'b [crate::ast::Stmt], out: &mut Vec<&'b crate::ast::Expr>) {
    use crate::ast::Stmt;
    for s in stmts {
        match s {
            Stmt::Return { expr: Some(e), .. } => out.push(e),
            Stmt::Block(b) => collect_return_exprs(&b.stmts, out),
            Stmt::If { then, els, .. } => {
                collect_return_exprs(std::slice::from_ref(then), out);
                if let Some(e) = els {
                    collect_return_exprs(std::slice::from_ref(e), out);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::For { body, .. }
            | Stmt::ForIn { body, .. }
            | Stmt::ForOf { body, .. }
            | Stmt::Labeled { stmt: body, .. } => {
                collect_return_exprs(std::slice::from_ref(body), out)
            }
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                collect_return_exprs(&block.stmts, out);
                if let Some(c) = catch {
                    collect_return_exprs(&c.block.stmts, out);
                }
                if let Some(fin) = finally {
                    collect_return_exprs(&fin.stmts, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    collect_return_exprs(&c.stmts, out);
                }
            }
            _ => {}
        }
    }
}

impl<'a> Checker<'a> {
    /// Type parameters inside type annotations (method sigs, function types)
    /// aren't bound by the binder; mint their symbols on demand.
    pub fn check_fn_type_param_modifiers(&mut self, tps: &'a [TypeParamDecl]) {
        for tp in tps {
            if let Some((text, span)) = &tp.variance_span {
                self.error_at(
                    *span,
                    &gen::_0_modifier_can_only_appear_on_a_type_parameter_of_a_class_interface_or_type_alias,
                    &[text.clone()],
                );
            }
        }
    }

    pub fn check_type_param_defaults_order(&mut self, tps: &'a [TypeParamDecl]) {
        let names: Vec<&str> = tps.iter().map(|t| t.name.name.as_str()).collect();

        for (i, tp) in tps.iter().enumerate() {
            let Some(d) = &tp.default else { continue };
            let mut refs: Vec<(usize, Span)> = Vec::new();
            collect_type_ref_names(d, &mut |name, span| {
                if let Some(j) = names.iter().position(|n| *n == name) {
                    refs.push((j, span));
                }
            });
            for (j, span) in refs {
                if j > i {
                    self.error_at(
                        span,
                        &gen::Type_parameter_defaults_can_only_reference_previously_declared_type_parameters,
                        &[],
                    );
                }
            }
        }
    }

    pub fn check_type_param_name(&mut self, tp: &'a TypeParamDecl) {
        if let Some((kind, span)) = &tp.illegal_modifier {
            self.error_at(
                *span,
                &gen::_0_modifier_cannot_appear_on_a_type_parameter,
                &[crate::checker::stmts::modifier_text(*kind).to_string()],
            );
        }
        const RESERVED: &[&str] = &[
            "any",
            "boolean",
            "number",
            "string",
            "symbol",
            "void",
            "object",
            "undefined",
            "never",
            "unknown",
            "bigint",
        ];
        if RESERVED.contains(&tp.name.name.as_str())
            && self.report_once_node(2368, node_key(tp))
        {
            self.error_at(
                tp.name.span,
                &gen::Type_parameter_name_cannot_be_0,
                &[tp.name.name.clone()],
            );
        }
    }

    pub fn ensure_type_param_symbol(&mut self, tp: &'a TypeParamDecl) -> SymbolId {
        let key = node_key(tp);
        if let Some(&s) = self.bind.decl_symbol.get(&key) {
            return s;
        }
        if let Some(&s) = self.synth.decl_symbol.get(&key) {
            return s;
        }
        let id = self.alloc_synth_symbol(tp.name.name.clone(), vec![Decl::TypeParam(tp)]);
        self.synth.decl_symbol.insert(key, id);
        id
    }

    // ── well-known lib symbols ──────────────────────────────────────────────

    pub fn global_type_symbol(&self, name: &str) -> Option<SymbolId> {
        self.scope_at(self.bind.global_scope).types.get(name)
    }

    pub fn array_symbol(&self) -> Option<SymbolId> {
        self.global_type_symbol("Array")
    }

    pub fn array_type(&mut self, elem: TypeId) -> TypeId {
        match self.array_symbol() {
            Some(sym) => self.types.ref_type(sym, vec![elem]),
            None => self.types.error,
        }
    }

    pub fn array_element_type(&mut self, t: TypeId) -> Option<TypeId> {
        match self.types.kind(t) {
            TypeKind::Ref(sym, args) if Some(*sym) == self.array_symbol() && args.len() == 1 => {
                Some(args[0])
            }
            TypeKind::ReadonlyArray(e) => Some(*e),
            _ => None,
        }
    }

    // ── module resolution ───────────────────────────────────────────────────

    pub fn namespace_names_in_scope(&self, scope: ScopeId) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = Some(scope);
        while let Some(s) = cur {
            for (name, sym) in &self.scope_at(s).values.0 {
                if self.bind.symbols[sym.0 as usize].flags & flags::NAMESPACE != 0 {
                    out.push(name.clone());
                }
            }
            cur = self.scope_at(s).parent;
        }
        out
    }

    pub fn resolve_module(&self, from_file: usize, spec: &str) -> Option<usize> {
        if !spec.starts_with("./") && !spec.starts_with("../") {
            return None;
        }
        let joined = crate::binder::resolve_relative_module_base(&self.files[from_file].0, spec);
        // explicit .ts/.tsx specifiers resolve directly
        if joined.ends_with(".ts") || joined.ends_with(".tsx") {
            if let Some(idx) = self.files.iter().position(|(n, _, _)| *n == joined) {
                return Some(idx);
            }
        }
        for cand in [
            format!("{joined}.ts"),
            format!("{joined}.tsx"),
            format!("{joined}.d.ts"),
            format!("{joined}/index.ts"),
        ] {
            if let Some(idx) = self.files.iter().position(|(n, _, _)| *n == cand) {
                return Some(idx);
            }
        }
        None
    }

    /// Resolve an import alias symbol to its target symbol (errors reported at
    /// import-statement check time, not here).
    pub fn alias_target(&mut self, sym: SymbolId) -> Option<SymbolId> {
        let decl = *self.symbol(sym).decls.first()?;
        let file = self.symbol(sym).file;
        match decl {
            Decl::Import(spec, idecl) => {
                let target_file = self.resolve_module(file, &idecl.module.value)?;
                let exported = spec.prop_name.as_ref().unwrap_or(&spec.name);
                self.bind.exports.get(&target_file)?.get(&exported.name)
            }
            Decl::ImportDefault(idecl) => {
                let target_file = self.resolve_module(file, &idecl.module.value)?;
                self.bind.exports.get(&target_file)?.get("default")
            }
            _ => None,
        }
    }

    pub fn resolve_alias_chain(&mut self, sym: SymbolId) -> SymbolId {
        if let Some(crate::binder::Decl::ImportEquals(_, module)) = self.symbol(sym).decls.first() {
            let mv = module.value.clone();
            if let Some(target) = self.resolve_module(self.current_file, &mv) {
                if let Some(&eq) = self.bind.export_equals.get(&target) {
                    return eq;
                }
            }
            return sym;
        }
        let mut cur = sym;
        let mut hops = 0;
        while self.symbol(cur).flags & flags::ALIAS != 0 && hops < 16 {
            match self.alias_target(cur) {
                Some(t) => cur = t,
                None => break,
            }
            hops += 1;
        }
        cur
    }

    // ── type of symbol ──────────────────────────────────────────────────────

    /// like type_of_symbol but in-progress resolution yields `any` silently
    /// (lazily-built container shapes must not fabricate 7022 cycles)
    pub(crate) fn type_of_symbol_lazy(&mut self, sym: SymbolId) -> TypeId {
        if self
            .res
            .resolving
            .iter()
            .any(|(s, slot)| *s == sym && *slot == Slot::ValueType)
        {
            return self.types.any;
        }
        self.type_of_symbol(sym)
    }

    pub fn type_of_symbol(&mut self, sym: SymbolId) -> TypeId {
        if let Some(&t) = self.caches.sym_type.get(&sym) {
            return t;
        }
        if self
            .res
            .resolving
            .iter()
            .any(|(s, slot)| *s == sym && *slot == Slot::ValueType)
        {
            // circular initializer: `const a = a;` — 7022 handled at decl check
            self.res.resolution_failed.insert((sym, Slot::ValueType));
            return self.types.any;
        }
        self.res.resolving.push((sym, Slot::ValueType));
        let t = self.compute_type_of_symbol(sym);
        self.res.resolving.pop();
        self.caches.sym_type.insert(sym, t);
        t
    }

    fn compute_type_of_symbol(&mut self, sym: SymbolId) -> TypeId {
        let s = self.symbol(sym);
        let sflags = s.flags;
        if sflags & flags::ALIAS != 0 {
            let target = self.resolve_alias_chain(sym);
            if target == sym {
                return self.types.error;
            }
            return self.type_of_symbol(target);
        }
        if sflags & flags::CLASS != 0 {
            return self.class_value_type(sym);
        }
        if sflags & flags::ENUM != 0 {
            return self.types.intern_kind(TypeKind::EnumObject(sym));
        }
        if sflags & flags::NAMESPACE != 0 && sflags & flags::CLASS == 0 {
            return self.types.intern_kind(TypeKind::NamespaceObj(sym));
        }
        if sflags & flags::ENUM_MEMBER != 0 {
            self.ensure_enum_checked(sym);
            let computed = matches!(
                self.enums.enum_member_values.get(&sym),
                Some(super::EnumValue::Computed) | None
            );
            if computed {
                if let Some(parent) = self.symbol(sym).parent {
                    return self.types.intern_kind(TypeKind::EnumType(parent));
                }
            }
            return self.types.intern_kind(TypeKind::EnumMember(sym));
        }
        if sflags & flags::FUNCTION != 0 {
            let fdecls: Vec<&'a FunctionLike> = s
                .decls
                .iter()
                .filter_map(|d| match d {
                    Decl::Func(f) => Some(*f),
                    _ => None,
                })
                .collect();
            let overloads: Vec<&'a FunctionLike> = fdecls
                .iter()
                .copied()
                .filter(|f| f.body.is_none())
                .collect();
            // the implementation (body-bearing) decl, captured before `pick`
            // consumes the vecs, for the TS2793 related-info.
            let impl_decl: Option<&'a FunctionLike> = if overloads.is_empty() {
                None
            } else {
                fdecls.iter().copied().find(|f| f.body.is_some())
            };
            let pick: Vec<&'a FunctionLike> = if overloads.is_empty() {
                fdecls
            } else {
                overloads
            };
            if pick.is_empty() {
                return self.types.error;
            }
            let sigs: Vec<crate::types::SigId> =
                pick.iter().map(|f| self.signature_of(f)).collect();
            let shape = self.types.alloc_shape(Shape {
                props: Vec::new(),
                call_sigs: sigs,
                ctor_sigs: Vec::new(),
                index_infos: Vec::new(),
            });
            // remember the hidden implementation signature for the TS2793
            // related-info.
            if let Some(impl_f) = impl_decl {
                if let Some(nsp) = impl_f.name.as_ref().map(|nm| nm.span()) {
                    let ifile = self.symbol(sym).file;
                    let isig = self.signature_of(impl_f);
                    self.caches
                        .overload_impl
                        .insert(shape, (isig, nsp.start, nsp.len(), ifile));
                }
            }
            return self.types.alloc(TypeKind::Anon(shape));
        }
        // interface / type-literal methods may be overloaded: collect every
        // `MethodSig` declaration so the resulting function type carries all of
        // their call signatures (e.g. `m(): T; m(x): U;`).
        if sflags & flags::METHOD != 0 {
            let mdecls: Vec<&'a MethodSig> = self
                .symbol(sym)
                .decls
                .iter()
                .filter_map(|d| match d {
                    Decl::MethodSig(m) => Some(*m),
                    _ => None,
                })
                .collect();
            if mdecls.len() > 1 {
                let sigs: Vec<crate::types::SigId> = mdecls
                    .iter()
                    .map(|m| {
                        let scope = self.scope_of_decl_or_global(node_key(*m));
                        self.method_signature(m, scope)
                    })
                    .collect();
                let shape = self.types.alloc_shape(Shape {
                    props: Vec::new(),
                    call_sigs: sigs,
                    ctor_sigs: Vec::new(),
                    index_infos: Vec::new(),
                });
                return self.types.alloc(TypeKind::Anon(shape));
            }
        }
        let Some(decl) = s.decls.first().copied() else {
            return self.types.error;
        };
        match decl {
            Decl::Var(d, kind) => self.type_of_var_decl(d, kind),
            Decl::Param(p) => self.type_of_param(p),
            Decl::CatchVar(p) => {
                if let Some(ty) = &p.ty {
                    let scope = self.scope_of_decl(node_key(p));
                    self.resolve_type(ty, scope)
                } else if self.options.use_unknown_in_catch_variables() {
                    self.types.unknown
                } else {
                    self.types.any
                }
            }
            Decl::PropSig(p) => {
                let scope = self.scope_of_decl_or_global(node_key(p));
                let mut t = match &p.ty {
                    Some(ty) => self.resolve_type(ty, scope),
                    None => self.types.any,
                };
                if p.question
                    && self.options.strict_null_checks()
                    && !self.options.exact_optional_property_types
                {
                    t = self.types.union(vec![t, self.types.undefined]);
                }
                t
            }
            Decl::MethodSig(m) => {
                let scope = self.scope_of_decl_or_global(node_key(m));
                self.method_sig_type(m, scope)
            }
            Decl::PropertyDecl(p) => {
                let scope = self.scope_of_decl_or_global(node_key(p));
                let mut t = match &p.ty {
                    Some(ty) => self.resolve_type(ty, scope),
                    None => match &p.init {
                        Some(init) => {
                            // Lazy entry: same as `infer_return_from_body`,
                            // recover the owning class from `sym`'s parent and
                            // push a matching `ThisContainer` so `this` in the
                            // initializer resolves independently of how this
                            // path was reached.
                            let parent = self.symbol(sym).parent;
                            let owner_class = parent.filter(|&ps| {
                                self.symbol(ps).flags & crate::binder::flags::CLASS != 0
                            });
                            let is_static = owner_class.is_some()
                                && crate::ast::has_modifier(
                                    &p.modifiers,
                                    crate::ast::ModifierKind::Static,
                                );
                            let tc = owner_class.map(|_| crate::checker::ThisContainer {
                                class_owner: owner_class,
                                is_static,
                                kind: crate::checker::ContainerKind::ClassBody,
                                explicit_this: None,
                            });
                            let it = self.with_opt_this_container(tc, |this| {
                                this.with_current_scope(scope, |this| this.check_expr(init, None))
                            });
                            let w = self.types.regular(it);
                            self.types.widen_literal(w)
                        }
                        None => self.types.any,
                    },
                };
                if p.question
                    && self.options.strict_null_checks()
                    && !self.options.exact_optional_property_types
                {
                    t = self.types.union(vec![t, self.types.undefined]);
                }
                t
            }
            Decl::Method(f) => match f.kind {
                FuncKind::Getter => {
                    let scope = self
                        .bind
                        .node_scope
                        .get(&node_key(f))
                        .copied()
                        .unwrap_or(self.bind.global_scope);
                    match &f.return_type {
                        Some(rt) => self.resolve_type(rt, scope),
                        None => {
                            if let Some(t) = self.paired_setter_value_type(sym) {
                                t
                            } else {
                                let sig = self.signature_of(f);
                                self.sig_return(sig)
                            }
                        }
                    }
                }
                FuncKind::Setter => {
                    let scope = self
                        .bind
                        .node_scope
                        .get(&node_key(f))
                        .copied()
                        .unwrap_or(self.bind.global_scope);
                    match f.params.first().and_then(|p| p.ty.as_ref()) {
                        Some(ty) => self.resolve_type(ty, scope),
                        None => self.types.any,
                    }
                }
                _ => self.function_type_of(f),
            },
            _ => self.types.error,
        }
    }

    fn paired_setter_value_type(&mut self, sym: SymbolId) -> Option<TypeId> {
        let decls = self.symbol(sym).decls.clone();
        for d in decls {
            let Decl::Method(f) = d else { continue };
            if f.kind != FuncKind::Setter {
                continue;
            }
            let param = f.params.iter().find(|p| {
                !p.name
                    .as_ident()
                    .map(|id| id.name == "this")
                    .unwrap_or(false)
            })?;
            let ty = param.ty.as_ref()?;
            let scope = self
                .bind
                .node_scope
                .get(&node_key(f))
                .copied()
                .unwrap_or(self.bind.global_scope);
            return Some(self.resolve_type(ty, scope));
        }
        None
    }

    pub fn scope_of_decl(&self, key: usize) -> ScopeId {
        if let Some(&s) = self.synth.decl_scope.get(&key) {
            return s;
        }
        self.bind
            .decl_scope
            .get(&key)
            .copied()
            .unwrap_or(self.bind.global_scope)
    }
    fn scope_of_decl_or_global(&self, key: usize) -> ScopeId {
        self.scope_of_decl(key)
    }

    fn type_of_var_decl(&mut self, d: &'a VarDeclarator, kind: VarKind) -> TypeId {
        if let Some(ty) = &d.ty {
            let scope = self.scope_of_decl(node_key(d));
            return self.resolve_type(ty, scope);
        }
        if let Some(init) = &d.init {
            // `let x = null` / `let x = undefined` evolve to any (tsc
            // widenTypeForVariableLikeDeclaration widens nullish initializers
            // of unannotated let/var to any; CFA supplies the read types)
            let nullish_init = match init {
                Expr::NullLit { .. } => true,
                Expr::Ident(id) => id.name == "undefined",
                _ => false,
            };
            if matches!(kind, VarKind::Let | VarKind::Var) && d.ty.is_none() && nullish_init {
                return self.types.any;
            }
            let t = self.check_expr(init, None);
            return match kind {
                VarKind::Const => self.types.regular(t),
                _ => {
                    // only *widening* (fresh) literal types widen under let/var;
                    // `as const` and declared literal types are non-widening
                    let was_fresh = self.types.is_fresh(t);
                    let r = self.types.regular(t);
                    if was_fresh {
                        self.types.widen_literal(r)
                    } else {
                        r
                    }
                }
            };
        }
        self.types.any
    }

    fn type_of_param(&mut self, p: &'a Param) -> TypeId {
        let scope = self.scope_of_decl(node_key(p));
        let mut t = if let Some(ty) = &p.ty {
            self.resolve_type(ty, scope)
        } else if let Some(init) = &p.initializer {
            let it = self.check_expr(init, None);
            let r = self.types.regular(it);
            self.types.widen_literal(r)
        } else if p.dotdotdot {
            return self.array_type(self.types.any);
        } else if let Some(&ctx) = self.caches.param_ctx_types.get(&node_key(p)) {
            ctx
        } else {
            self.types.any
        };
        if p.question
            && self.options.strict_null_checks()
            && !self.options.exact_optional_property_types
        {
            t = self.types.union(vec![t, self.types.undefined]);
        }
        t
    }

    /// Recover the enclosing class symbol of a function-like declaration, if
    /// any. Used by lazy evaluation entry points (`infer_return_from_body`,
    /// `type_of_symbol(PropertyDecl)`, etc.) to set up a `ThisContainer`
    /// without relying on traversal-time `class_stack` state. Returns `None`
    /// for top-level functions, function expressions outside a class body,
    /// arrows assigned to a non-class location, etc.
    pub(crate) fn enclosing_class_of_fn(
        &self,
        f: &'a crate::ast::FunctionLike,
    ) -> Option<SymbolId> {
        let key = crate::ast::node_key(f);
        let msym = self.bind.decl_symbol.get(&key).copied()?;
        let parent = self.symbol(msym).parent?;
        if self.symbol(parent).flags & crate::binder::flags::CLASS != 0 {
            Some(parent)
        } else {
            None
        }
    }

    /// Whether a function-like is a *static* class member (static method,
    /// static getter/setter). For non-members this is `false`. Constructors
    /// are never static. Looks at the function's modifiers directly because
    /// the binder does not propagate `STATIC` onto the member's `Symbol.flags`.
    pub(crate) fn is_static_member(&self, f: &'a crate::ast::FunctionLike) -> bool {
        crate::ast::has_modifier(&f.modifiers, crate::ast::ModifierKind::Static)
    }

    /// Build the (anonymous) function type for a function-like declaration.
    pub fn function_type_of(&mut self, f: &'a FunctionLike) -> TypeId {
        let sig = self.signature_of(f);
        let shape = self.types.alloc_shape(Shape {
            props: Vec::new(),
            call_sigs: vec![sig],
            ctor_sigs: Vec::new(),
            index_infos: Vec::new(),
        });
        self.types.alloc(TypeKind::Anon(shape))
    }

    pub(crate) fn explicit_this_param_type(
        &mut self,
        f: &'a FunctionLike,
        scope: ScopeId,
    ) -> Option<TypeId> {
        f.params
            .iter()
            .find(|p| p.name.as_ident().map(|i| i.name == "this").unwrap_or(false))
            .map(|p| match p.ty.as_ref() {
                Some(ty) => self.resolve_type(ty, scope),
                None => self.types.any,
            })
    }

    pub fn signature_of(&mut self, f: &'a FunctionLike) -> crate::types::SigId {
        let scope = self
            .bind
            .node_scope
            .get(&node_key(f))
            .copied()
            .unwrap_or(self.bind.global_scope);
        let type_params: Vec<SymbolId> = f
            .type_params
            .as_ref()
            .map(|tps| {
                tps.iter()
                    .map(|tp| self.ensure_type_param_symbol(tp))
                    .collect()
            })
            .unwrap_or_default();
        let (params, min_args, rest, rest_name) = self.params_of_kind(&f.params, scope, f.kind);
        let rest_tp = self.tp.rest_tp_scratch;
        let ret = match &f.return_type {
            Some(rt) => self.resolve_type(rt, scope),
            None => {
                if f.is_generator {
                    self.generator_return_type(f)
                } else if f.body.is_some() {
                    // lazy body inference
                    self.types.error // placeholder replaced by sig_return()
                } else {
                    self.types.any
                }
            }
        };
        let has_lazy_ret = f.return_type.is_none() && f.body.is_some() && !f.is_generator;
        let predicate = if let Some(crate::ast::TypeNode::Predicate {
            param_name,
            asserts,
            ty,
            ..
        }) = f.return_type.as_ref()
        {
            let param = if param_name.name == "this" {
                -1
            } else {
                f.params
                    .iter()
                    .position(|p| {
                        p.name
                            .as_ident()
                            .map(|i| i.name == param_name.name)
                            .unwrap_or(false)
                    })
                    .map(|i| i as i32)
                    .unwrap_or(-2)
            };
            let pty = ty.as_ref().map(|t| self.resolve_type(t, scope));
            Some(crate::types::PredInfo {
                param,
                asserts: *asserts,
                ty: pty,
            })
        } else {
            None
        };
        // an explicit `this` parameter (`function f(this: T)`) — kept aside from
        // the regular parameters, recorded for call-site `this`-context checks.
        let this_ty = self.explicit_this_param_type(f, scope);
        let sig = self.types.alloc_sig(Signature {
            type_params,
            params,
            min_args,
            rest,
            rest_name,
            rest_tp,
            ret,
            decl_key: if has_lazy_ret { node_key(f) } else { 0 },
            ret_annotation_never: matches!(self.types.kind(ret), crate::types::TypeKind::Never),
            predicate,
            is_abstract: false,
        });
        if let Some(tt) = this_ty {
            self.caches.sig_this_ty.insert(sig, tt);
        }
        sig
    }

    pub fn params_of(
        &mut self,
        params: &'a [Param],
        scope: ScopeId,
    ) -> (Vec<ParamInfo>, u32, Option<TypeId>, Option<String>) {
        self.params_of_kind(params, scope, FuncKind::Declaration)
    }

    pub fn params_of_kind(
        &mut self,
        params: &'a [Param],
        scope: ScopeId,
        kind: FuncKind,
    ) -> (Vec<ParamInfo>, u32, Option<TypeId>, Option<String>) {
        let mut infos = Vec::new();
        let mut min_args = 0u32;
        let mut rest = None;
        let mut rest_name = None;
        let mut seen_optional = false;
        self.tp.rest_tp_scratch = None;
        for p in params {
            let name = p
                .name
                .as_ident()
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "_".into());
            if name == "this" {
                continue;
            }
            if p.dotdotdot {
                rest_name = Some(name);
                let t = match &p.ty {
                    Some(ty) => {
                        let at = self.resolve_type(ty, scope);
                        // remember a bare type-parameter rest (`...args: T`) so the
                        // call site can infer it as a tuple of the rest arguments.
                        if let TypeKind::TypeParam(sym) = self.types.kind(at) {
                            self.tp.rest_tp_scratch = Some(*sym);
                        }
                        self.array_element_type(at).unwrap_or(self.types.any)
                    }
                    None => self.types.any,
                };
                rest = Some(t);
                continue;
            }
            let mut t = match &p.ty {
                Some(ty) => self.resolve_type(ty, scope),
                None => match &p.initializer {
                    Some(init) => {
                        let flag = kind == FuncKind::Constructor;
                        if flag {
                            self.cflags.in_ctor_param_init = true;
                        }
                        let it = self.check_expr(init, None);
                        if flag {
                            self.cflags.in_ctor_param_init = false;
                        }
                        let r = self.types.regular(it);
                        self.types.widen_literal(r)
                    }
                    None => {
                        if let Some(&ctx) = self.caches.param_ctx_types.get(&node_key(p)) {
                            ctx
                        } else {
                            self.types.any
                        }
                    }
                },
            };
            let optional = p.question || p.initializer.is_some();
            if p.question
                && self.options.strict_null_checks()
                && !self.options.exact_optional_property_types
            {
                t = self.types.union(vec![t, self.types.undefined]);
            }
            if !optional && !seen_optional {
                min_args += 1;
            }
            if optional {
                seen_optional = true;
            }
            infos.push(ParamInfo {
                name,
                ty: t,
                optional,
                decl_span: Some(p.span),
                decl_file: self.current_file,
            });
        }
        (infos, min_args, rest, rest_name)
    }

    fn method_sig_type(&mut self, m: &'a MethodSig, scope: ScopeId) -> TypeId {
        let sig = self.method_signature(m, scope);
        let shape = self.types.alloc_shape(Shape {
            props: Vec::new(),
            call_sigs: vec![sig],
            ctor_sigs: Vec::new(),
            index_infos: Vec::new(),
        });
        self.types.alloc(TypeKind::Anon(shape))
    }

    pub fn method_signature(&mut self, m: &'a MethodSig, scope: ScopeId) -> crate::types::SigId {
        let type_params: Vec<SymbolId> = m
            .type_params
            .as_ref()
            .map(|tps| {
                tps.iter()
                    .map(|tp| self.ensure_type_param_symbol(tp))
                    .collect()
            })
            .unwrap_or_default();
        // Real signature-level type parameters resolve lexically: bind them into
        // a transient `TypeParams` scope (child of `scope`) and resolve the
        // parameter and return types under it. This isolates them correctly (they
        // are visible inside this signature but not in types it merely references)
        // — unlike the old global `infer_mapped_env` stack.
        let scope = match &m.type_params {
            Some(tps) => self.push_tp_scope(scope, tps),
            None => scope,
        };
        let (params, min_args, rest, rest_name) = self.params_of(&m.params, scope);
        let rest_tp = self.tp.rest_tp_scratch;
        let ret = match &m.return_type {
            Some(rt) => self.resolve_type(rt, scope),
            None => self.types.any,
        };
        let predicate = self.predicate_of(&m.return_type, &m.params, scope);
        self.types.alloc_sig(Signature {
            type_params,
            params,
            min_args,
            rest,
            rest_name,
            rest_tp,
            ret,
            decl_key: 0,
            ret_annotation_never: matches!(self.types.kind(ret), crate::types::TypeKind::Never),
            predicate,
            is_abstract: false,
        })
    }

    /// Extract a type-predicate (`x is T` / `asserts x`) from a member's return
    /// type, mapping the predicate's parameter name to its positional index.
    pub fn predicate_of(
        &mut self,
        return_type: &'a Option<crate::ast::TypeNode>,
        params: &'a [crate::ast::Param],
        scope: ScopeId,
    ) -> Option<crate::types::PredInfo> {
        if let Some(crate::ast::TypeNode::Predicate {
            param_name,
            asserts,
            ty,
            ..
        }) = return_type.as_ref()
        {
            let param = if param_name.name == "this" {
                -1
            } else {
                params
                    .iter()
                    .position(|p| {
                        p.name
                            .as_ident()
                            .map(|i| i.name == param_name.name)
                            .unwrap_or(false)
                    })
                    .map(|i| i as i32)
                    .unwrap_or(-2)
            };
            let pty = ty.as_ref().map(|t| self.resolve_type(t, scope));
            Some(crate::types::PredInfo {
                param,
                asserts: *asserts,
                ty: pty,
            })
        } else {
            None
        }
    }

    pub fn call_signature(&mut self, cs: &'a CallSig, scope: ScopeId) -> crate::types::SigId {
        let type_params: Vec<SymbolId> = cs
            .type_params
            .as_ref()
            .map(|tps| {
                tps.iter()
                    .map(|tp| self.ensure_type_param_symbol(tp))
                    .collect()
            })
            .unwrap_or_default();
        let scope = match &cs.type_params {
            Some(tps) => self.push_tp_scope(scope, tps),
            None => scope,
        };
        let (params, min_args, rest, rest_name) = self.params_of(&cs.params, scope);
        let rest_tp = self.tp.rest_tp_scratch;
        let ret = match &cs.return_type {
            Some(rt) => self.resolve_type(rt, scope),
            None => self.types.any,
        };
        let predicate = self.predicate_of(&cs.return_type, &cs.params, scope);
        self.types.alloc_sig(Signature {
            type_params,
            params,
            min_args,
            rest,
            rest_name,
            rest_tp,
            ret,
            decl_key: 0,
            ret_annotation_never: matches!(self.types.kind(ret), crate::types::TypeKind::Never),
            predicate,
            is_abstract: false,
        })
    }

    // ── type node resolution ────────────────────────────────────────────────

    pub fn resolve_type(&mut self, node: &'a TypeNode, scope: ScopeId) -> TypeId {
        self.resolve_type_inner(node, scope)
    }

    fn resolve_type_inner(&mut self, node: &'a TypeNode, scope: ScopeId) -> TypeId {
        match node {
            TypeNode::Keyword(k, _) => match k {
                KeywordTypeKind::Any | KeywordTypeKind::Intrinsic => self.types.any,
                KeywordTypeKind::Unknown => self.types.unknown,
                KeywordTypeKind::String => self.types.string,
                KeywordTypeKind::Number => self.types.number,
                KeywordTypeKind::Boolean => self.types.boolean,
                KeywordTypeKind::Object => self.types.non_primitive,
                KeywordTypeKind::Symbol => self.types.es_symbol,
                KeywordTypeKind::Bigint => self.types.bigint,
                KeywordTypeKind::Void => self.types.void,
                KeywordTypeKind::Undefined => self.types.undefined,
                KeywordTypeKind::Never => self.types.never,
                KeywordTypeKind::Null => self.types.null,
            },
            TypeNode::This(sp) => {
                // `this` type resolutions, in order:
                //   - in a static class member, the constructor function itself
                //     (`typeof Class` = `ClassStatics(owner)`);
                //   - on a *generic* owner, the parameterized self-type so type
                //     arguments flow through (`Box<number>.set(): this`);
                //   - otherwise, the *polymorphic* `this`: a synthetic type
                //     parameter whose constraint is `owner`. It is substituted
                //     by the receiver at each member access (see access.rs),
                //     so `Thing1.self(): this` returns the receiver, not
                //     `Thing1`.
                if let Some(&owner) = self
                    .stacks
                    .class_stack
                    .last()
                    .or_else(|| self.stacks.this_type_stack.last())
                {
                    let in_static = self
                        .stacks
                        .this_container_stack
                        .iter()
                        .rev()
                        .find_map(|c| match c.kind {
                            crate::checker::ContainerKind::ClassBody
                            | crate::checker::ContainerKind::Method
                                if c.class_owner == Some(owner) =>
                            {
                                Some(c.is_static)
                            }
                            _ => None,
                        })
                        .unwrap_or(false);
                    if in_static {
                        return self.class_value_type(owner);
                    }
                    let tps = self.type_params_of_symbol(owner);
                    if tps.is_empty() {
                        let p = self.this_param_of(owner);
                        self.types.intern_kind(TypeKind::TypeParam(p))
                    } else {
                        let args: Vec<TypeId> = tps
                            .iter()
                            .map(|&p| self.types.intern_kind(TypeKind::TypeParam(p)))
                            .collect();
                        self.types.ref_type(owner, args)
                    }
                } else {
                    self.error_at(
                        *sp,
                        &gen::A_this_type_is_available_only_in_a_non_static_member_of_a_class_or_interface,
                        &[],
                    );
                    self.types.error
                }
            }
            TypeNode::Paren { inner, .. } => self.resolve_type(inner, scope),
            TypeNode::LiteralString { value, .. } => self.types.string_lit_js(value),
            TypeNode::LiteralNumber { value, .. } => self.types.number_lit(*value),
            TypeNode::LiteralBigInt { text, .. } => self.types.bigint_lit(text),
            TypeNode::LiteralBool { value, .. } => {
                if *value {
                    self.types.true_t
                } else {
                    self.types.false_t
                }
            }
            TypeNode::Array { elem, .. } => {
                self.guards.alias_wrapper_depth += 1;
                let e = self.resolve_type(elem, scope);
                self.guards.alias_wrapper_depth -= 1;
                self.array_type(e)
            }
            TypeNode::Tuple { elems, .. } => {
                let mut tes = Vec::new();
                for e in elems {
                    self.guards.alias_wrapper_depth += 1;
                    let t = self.resolve_type(&e.ty, scope);
                    self.guards.alias_wrapper_depth -= 1;
                    // labeled `[...x?: T]` is TS5085 (grammar, reported in the
                    if e.dotdotdot && e.question {
                        let d = self.display_type(t);
                        let dots = e.ty.span().start as usize - 3;
                        self.error_at(
                            Span::new(dots, dots + 3),
                            &gen::A_rest_element_type_must_be_an_array_type,
                            &[],
                        );
                        let tstart = e.ty.span().start as usize;
                        let qend = e.ty.span().end as usize + 1;
                        self.error_at(
                            Span::new(tstart, qend),
                            &gen::_0_at_the_end_of_a_type_is_not_valid_TypeScript_syntax_Did_you_mean_to_write_1,
                            &["?".to_string(), format!("{} | undefined", d)],
                        );
                        tes.push(crate::types::TupleElem {
                            ty: self.types.any,
                            optional: false,
                            rest: true,
                        });
                        continue;
                    }
                    if e.dotdotdot {
                        // `...A`: spreading a concrete tuple expands its elements
                        // inline (`[...[a, b], c]` → `[a, b, c]`); spreading an
                        // array contributes a single rest element.
                        match self.types.kind(t).clone() {
                            TypeKind::Tuple(inner) | TypeKind::ReadonlyTuple(inner) => {
                                for ie in inner {
                                    tes.push(ie);
                                }
                                continue;
                            }
                            _ => match self.array_element_type(t) {
                                Some(el) => {
                                    tes.push(TupleElem {
                                        ty: el,
                                        optional: false,
                                        rest: true,
                                    });
                                    continue;
                                }
                                None => {
                                    // `...T` where T is a type parameter
                                    // (`<T extends any[]>(... [...T] ...)`) is a
                                    // variadic-tuple rest; keep T as a rest
                                    // element for later expansion rather than
                                    // erroring.
                                    if self.type_contains_params(t) {
                                        tes.push(TupleElem {
                                            ty: t,
                                            optional: false,
                                            rest: true,
                                        });
                                        continue;
                                    }
                                    if !self.types.is_any_or_error(t) {
                                        let dots = e.ty.span().start as usize - 3;
                                        self.error_at(
                                            Span::new(dots, dots + 3),
                                            &gen::A_rest_element_type_must_be_an_array_type,
                                            &[],
                                        );
                                    }
                                    tes.push(TupleElem {
                                        ty: self.types.any,
                                        optional: false,
                                        rest: true,
                                    });
                                    continue;
                                }
                            },
                        }
                    }
                    tes.push(TupleElem {
                        ty: t,
                        optional: e.question,
                        rest: false,
                    });
                }
                self.types.tuple(tes)
            }
            TypeNode::Union { members, .. } => {
                let ms: Vec<TypeId> = members
                    .iter()
                    .map(|m| self.resolve_type(m, scope))
                    .collect();
                self.types.union(ms)
            }
            TypeNode::Intersection { members, .. } => {
                let resolved: Vec<TypeId> = members
                    .iter()
                    .map(|m| self.resolve_type(m, scope))
                    .collect();
                self.intersect_all(resolved)
            }
            TypeNode::Function(f) => {
                let sig = self.function_type_node_sig(f, scope);
                let shape = self.types.alloc_shape(Shape {
                    props: Vec::new(),
                    call_sigs: vec![sig],
                    ctor_sigs: Vec::new(),
                    index_infos: Vec::new(),
                });
                self.types.alloc(TypeKind::Anon(shape))
            }
            TypeNode::Ctor(f) => {
                let sig = self.function_type_node_sig(f, scope);
                let shape = self.types.alloc_shape(Shape {
                    props: Vec::new(),
                    call_sigs: Vec::new(),
                    ctor_sigs: vec![sig],
                    index_infos: Vec::new(),
                });
                self.types.alloc(TypeKind::Anon(shape))
            }
            TypeNode::TypeLiteral { members, .. } => {
                let key = node_key(node);
                let diag_key = key | (1usize << 61);
                if self.checked_decls.insert(diag_key) {
                    self.check_type_member_implicit_any(members);
                }
                // deferred: members resolve lazily (allows `type B = { x: C }`
                // mutual references, like tsc's anonymous-type laziness). Capture
                // the active `infer` / mapped-key environment too: a literal like
                // `{ [s: string]: U }` written under an enclosing `infer U` (or a
                // mapped key) resolves lazily, by which point `infer_mapped_env` no
                // longer holds that name — without the snapshot the index
                // signature's `U` would fail to resolve. A signature type param is
                // instead carried by the captured `scope`, so it needs no snapshot.
                self.deferred.deferred_literals.insert(
                    key,
                    (members.as_slice(), scope, self.tp.infer_mapped_env.clone()),
                );
                self.types.intern_kind(TypeKind::DeferredObj(key))
            }
            TypeNode::TypeQuery {
                name, type_args, ..
            } => {
                if let Some(type_args) = type_args {
                    for arg in type_args {
                        self.resolve_type_cached(arg, scope);
                    }
                }
                // typeof entityName
                let first = &name.parts[0];
                if let Some((field_name, ident_name)) =
                    self.cflags.ctor_field_stack.last().and_then(|ctx| {
                        (ctx.kind == CtorFieldContextKind::TypeAnnotation
                            && ctx.blocked_names.contains(&first.name))
                        .then(|| (ctx.field_name.clone(), first.name.clone()))
                    })
                {
                    self.error_at(
                        first.span,
                        &gen::Type_of_instance_member_variable_0_cannot_reference_identifier_1_declared_in_the_constructor,
                        &[field_name, ident_name],
                    );
                    return self.types.error;
                }
                if first.name == "this" {
                    let Some(&cls) = self.stacks.class_stack.last() else {
                        self.error_at(first.span, &gen::Cannot_find_name_0, &[first.name.clone()]);
                        return self.types.error;
                    };
                    let in_static = self
                        .stacks
                        .this_container_stack
                        .iter()
                        .rev()
                        .find_map(|c| match c.kind {
                            crate::checker::ContainerKind::ClassBody
                            | crate::checker::ContainerKind::Method
                                if c.class_owner == Some(cls) =>
                            {
                                Some(c.is_static)
                            }
                            _ => None,
                        })
                        .unwrap_or(false);
                    let mut t = if in_static {
                        self.class_value_type(cls)
                    } else if self.type_params_of_symbol(cls).is_empty() {
                        // tsc flow-narrows type queries: `const d1: typeof
                        // this` under `this instanceof D` is the narrowed
                        // this. The builder maps root-level TypeQuery
                        // annotations; unmapped positions (and typeof-x
                        // queries generally) keep the declared type —
                        // documented gap pending full type-position mapping.
                        let p = self.this_param_of(cls);
                        self.flow_type_of_this_query(node_key(node), p)
                            .unwrap_or_else(|| self.types.intern_kind(TypeKind::TypeParam(p)))
                    } else {
                        let args: Vec<TypeId> = self
                            .type_params_of_symbol(cls)
                            .iter()
                            .map(|&p| self.types.intern_kind(TypeKind::TypeParam(p)))
                            .collect();
                        self.types.ref_type(cls, args)
                    };
                    for part in &name.parts[1..] {
                        let Some(p) = self.prop_of_type(t, &part.name) else {
                            return self.types.error;
                        };
                        t = p;
                    }
                    return t;
                }
                let Some(sym) = self.lookup_value(scope, &first.name) else {
                    self.error_at(first.span, &gen::Cannot_find_name_0, &[first.name.clone()]);
                    return self.types.error;
                };
                let sym = self.resolve_alias_chain(sym);
                let mut t = self.type_of_symbol(sym);
                for part in &name.parts[1..] {
                    let Some(p) = self.prop_of_type(t, &part.name) else {
                        return self.types.error;
                    };
                    t = p;
                }
                t
            }
            TypeNode::Keyof { ty, .. } => {
                let t = self.resolve_type(ty, scope);
                // keyof any = string | number | symbol
                if matches!(self.types.kind(t), TypeKind::Any) {
                    let (s, n, sy) = (self.types.string, self.types.number, self.types.es_symbol);
                    return self.types.union(vec![s, n, sy]);
                }
                // named types keep the `keyof X` display form; anonymous ones
                // resolve eagerly to the literal union (matches tsc output)
                if matches!(
                    self.types.kind(t),
                    TypeKind::Iface(_)
                        | TypeKind::Ref(..)
                        | TypeKind::TypeParam(_)
                        | TypeKind::EnumType(_)
                ) {
                    return self.types.intern_kind(TypeKind::Keyof(t));
                }
                self.keyof_union(t)
            }
            TypeNode::ReadonlyOp { ty, .. }
                if !matches!(&**ty, TypeNode::Array { .. } | TypeNode::Tuple { .. }) =>
            {
                let kw = node.span().start as usize;
                self.error_at(
                    Span::new(kw, kw + 8),
                    &gen::readonly_type_modifier_is_only_permitted_on_array_and_tuple_literal_types,
                    &[],
                );
                self.resolve_type(ty, scope)
            }
            TypeNode::ReadonlyOp { ty, .. } => {
                let t = self.resolve_type(ty, scope);
                if let TypeKind::Tuple(elems) = self.types.kind(t) {
                    let elems = elems.clone();
                    self.types.intern_kind(TypeKind::ReadonlyTuple(elems))
                } else if let Some(elem) = self.array_element_type(t) {
                    self.types.intern_kind(TypeKind::ReadonlyArray(elem))
                } else {
                    t
                }
            }
            TypeNode::IndexedAccess { obj, index, .. } => {
                let ot = self.resolve_type(obj, scope);
                let it = self.resolve_type(index, scope);
                self.indexed_access_type(ot, it, Some((index.span(), node.span())))
            }
            TypeNode::Ref(r) => self.resolve_type_ref(r, scope),
            TypeNode::Predicate { asserts, .. } => {
                // `asserts ...` signatures yield void; `x is T` yields boolean.
                if *asserts {
                    self.types.void
                } else {
                    self.types.boolean
                }
            }
            TypeNode::Infer { name, .. } => {
                // an enclosing conditional binds this infer name to a freshly
                // minted parameter (evaluate_conditional_single); use it so the
                // extends type, the inference, and the branch all share one
                // symbol. Fall back to a node-keyed parameter otherwise.
                let sym = self
                    .tp
                    .infer_mapped_env
                    .iter()
                    .rev()
                    .find(|(n, _)| n == &name.name)
                    .map(|(_, s)| *s)
                    .unwrap_or_else(|| self.synthetic_type_param(node_key(node), &name.name));
                self.types.intern_kind(TypeKind::TypeParam(sym))
            }
            TypeNode::Conditional(c) => {
                self.deferred
                    .deferred_conds
                    .insert(node_key(&**c), (&**c, scope, self.current_file));
                self.evaluate_conditional(c, scope, &Mapper::new())
            }
            TypeNode::Mapped(m) => {
                self.deferred
                    .deferred_mappeds
                    .insert(node_key(&**m), (&**m, scope, self.current_file));
                self.evaluate_mapped(m, scope, &Mapper::new())
            }
            TypeNode::TemplateLit { head, parts, .. } => {
                let mut resolved: Vec<(TypeId, String)> = Vec::new();
                for (t, text) in parts {
                    let rt = self.resolve_type(t, scope);
                    resolved.push((rt, text.clone()));
                }
                self.template_literal_type(head.clone(), resolved)
            }
        }
    }

    pub fn synthetic_type_param(&mut self, key: usize, name: &str) -> SymbolId {
        if let Some(&s) = self.deferred.synthetic_type_params.get(&key) {
            return s;
        }
        let id = self.alloc_synth_symbol(name.to_string(), Vec::new());
        self.deferred.synthetic_type_params.insert(key, id);
        id
    }

    pub(crate) fn type_contains_params(&mut self, t: TypeId) -> bool {
        match self.types.kind(t).clone() {
            TypeKind::TypeParam(_)
            | TypeKind::DeferredCond(..)
            | TypeKind::DeferredMapped(..)
            | TypeKind::IndexedAccess(..) => true,
            TypeKind::Keyof(i) => self.type_contains_params(i),
            TypeKind::Union(ms) => ms.iter().any(|&m| self.type_contains_params(m)),
            TypeKind::Intersection(ms) => ms.iter().any(|&m| self.type_contains_params(m)),
            TypeKind::Ref(_, args) => args.iter().any(|&a| self.type_contains_params(a)),
            TypeKind::ReadonlyArray(e) => self.type_contains_params(e),
            TypeKind::Tuple(es) | TypeKind::ReadonlyTuple(es) => {
                es.iter().any(|e| self.type_contains_params(e.ty))
            }
            TypeKind::TemplateLit(parts) => parts.iter().any(|p| match p {
                crate::types::TplPart::Ty(t2) => self.type_contains_params(*t2),
                _ => false,
            }),
            _ => false,
        }
    }

    fn function_type_node_sig(
        &mut self,
        f: &'a FunctionTypeNode,
        scope: ScopeId,
    ) -> crate::types::SigId {
        let type_params: Vec<SymbolId> = f
            .type_params
            .as_ref()
            .map(|tps| {
                tps.iter()
                    .map(|tp| self.ensure_type_param_symbol(tp))
                    .collect()
            })
            .unwrap_or_default();
        let scope = match &f.type_params {
            Some(tps) => self.push_tp_scope(scope, tps),
            None => scope,
        };
        let (params, min_args, rest, rest_name) = self.params_of(&f.params, scope);
        let rest_tp = self.tp.rest_tp_scratch;
        let ret = self.resolve_type(&f.return_type, scope);
        self.types.alloc_sig(Signature {
            type_params,
            params,
            min_args,
            rest,
            rest_name,
            rest_tp,
            ret,
            decl_key: 0,
            ret_annotation_never: matches!(self.types.kind(ret), crate::types::TypeKind::Never),
            predicate: None,
            is_abstract: f.is_abstract,
        })
    }

    pub fn shape_of_members(&mut self, members: &'a [TypeMember], scope: ScopeId) -> ShapeId {
        let mut shape = Shape::default();
        for m in members {
            match m {
                TypeMember::Prop(p) => {
                    let Some(name) = p.name.text() else { continue };
                    let mut t = match &p.ty {
                        Some(ty) => self.resolve_type(ty, scope),
                        None => self.types.any,
                    };
                    if p.question
                        && self.options.strict_null_checks()
                        && !self.options.exact_optional_property_types
                    {
                        t = self.types.union(vec![t, self.types.undefined]);
                    }
                    shape.props.push(PropInfo {
                        name,
                        ty: t,
                        optional: p.question,
                        readonly: p.readonly,
                        is_method: false,
                        symbol: self.bind.decl_symbol.get(&node_key(p)).copied(),
                    });
                }
                TypeMember::Method(ms) => {
                    let Some(name) = ms.name.text() else { continue };
                    let t = self.method_sig_type(ms, scope);
                    shape.props.push(PropInfo {
                        name,
                        ty: t,
                        optional: ms.question,
                        readonly: false,
                        is_method: true,
                        symbol: self.bind.decl_symbol.get(&node_key(ms)).copied(),
                    });
                }
                TypeMember::Call(cs) => {
                    let sig = self.call_signature(cs, scope);
                    shape.call_sigs.push(sig);
                }
                TypeMember::Ctor(cs) => {
                    let sig = self.call_signature(cs, scope);
                    shape.ctor_sigs.push(sig);
                }
                TypeMember::Index(idx) => {
                    let key = self.resolve_type(&idx.key_type, scope);
                    let value = self.resolve_type(&idx.value_type, scope);
                    shape.index_infos.push(IndexInfo {
                        key,
                        value,
                        readonly: idx.readonly,
                    });
                }
            }
        }
        self.types.alloc_shape(shape)
    }

    pub fn resolve_type_ref_pub(&mut self, r: &'a TypeRef, scope: ScopeId) -> TypeId {
        self.resolve_type_ref(r, scope)
    }

    /// Wrap a type as `Promise<Awaited<inner>>` (the result type of an async
    /// function body). Falls back to the bare type if no global Promise exists.
    pub fn promise_type(&mut self, inner: TypeId) -> TypeId {
        let awaited = self.awaited_type_pub(inner);
        if let Some(sym) = self.global_type_symbol("Promise") {
            self.types.ref_type(sym, vec![awaited])
        } else {
            inner
        }
    }

    /// signature return type, lazily inferred from the body when unannotated
    pub fn sig_return(&mut self, sig: crate::types::SigId) -> TypeId {
        let s = self.types.sig(sig).clone();
        if s.decl_key == 0 {
            return s.ret;
        }
        if let Some(&t) = self.caches.sig_ret_cache.get(&s.decl_key) {
            return t;
        }
        if let Some(pos) = self.flow.return_stack.iter().position(|&k| k == s.decl_key) {
            let cycle: Vec<usize> = self.flow.return_stack[pos..].to_vec();
            if cycle.len() > 1 && self.options.no_implicit_any() {
                for k in cycle {
                    if self.report_once_node(7023, k) {
                        if let Some(f) = self.bind.fn_decls.get(&k).copied() {
                            if let Some(name) = f.name_ident() {
                                self.error_at(
                                    name.span,
                                    &gen::_0_implicitly_has_return_type_any_because_it_does_not_have_a_return_type_annotation_and_is_referenced_directly_or_indirectly_in_one_of_its_return_expressions,
                                    &[name.name.clone()],
                                );
                            }
                        }
                    }
                }
            }
            return self.types.any;
        }
        self.flow.return_stack.push(s.decl_key);
        let f = self.bind.fn_decls.get(&s.decl_key).copied();
        let t = match f {
            Some(f) => {
                let inferred = self.infer_return_from_body(f);
                // an async function's inferred return type is `Promise<T>` where
                // `T` is the (awaited) type its body produces.
                if crate::ast::has_modifier(&f.modifiers, crate::ast::ModifierKind::Async)
                    && !matches!(
                        f.kind,
                        crate::ast::FuncKind::Getter | crate::ast::FuncKind::Setter
                    )
                {
                    self.promise_type(inferred)
                } else {
                    inferred
                }
            }
            None => self.types.any,
        };
        self.flow.return_stack.pop();
        self.caches.sig_ret_cache.insert(s.decl_key, t);
        t
    }

    /// Generator<Y, R, N>: Y = union of yielded types (literals kept),
    /// R = inferred return (widened), N = unknown
    fn generator_return_type(&mut self, f: &'a FunctionLike) -> TypeId {
        let Some(gen_sym) = self.global_type_symbol("Generator") else {
            let sid = self.types.alloc_shape(Shape::default());
            return self.types.alloc(TypeKind::Anon(sid));
        };
        let scope = self
            .bind
            .node_scope
            .get(&node_key(f))
            .copied()
            .unwrap_or(self.bind.global_scope);
        let prev = self.current_scope;
        self.current_scope = scope;
        let mut yields: Vec<TypeId> = Vec::new();
        if let Some(FuncBody::Block(b)) = &f.body {
            let mut exprs: Vec<&'a Expr> = Vec::new();
            collect_yield_exprs(&b.stmts, &mut exprs);
            for e in exprs {
                let t = self.check_expr(e, None);
                yields.push(self.types.regular(t));
            }
        }
        let y = if yields.is_empty() {
            self.types.never
        } else {
            self.types.union(yields)
        };
        let r = self.infer_return_from_body(f);
        self.current_scope = prev;
        let n = self.types.unknown;
        self.types.ref_type(gen_sym, vec![y, r, n])
    }

    fn infer_return_from_body(&mut self, f: &'a FunctionLike) -> TypeId {
        let scope = self
            .bind
            .node_scope
            .get(&node_key(f))
            .copied()
            .unwrap_or(self.bind.global_scope);
        let prev = self.current_scope;
        self.current_scope = scope;
        // Lazy evaluation entry: when called from `sig_return`, the
        // traversal-time `this_container_stack` from `check_function_body` is
        // *not* in effect (we may not have entered this body yet). Recreate
        // an equivalent container locally so `this` inside the body resolves
        // correctly — independently of how/when this body is reached.
        let owner_class = if matches!(
            f.kind,
            crate::ast::FuncKind::Method
                | crate::ast::FuncKind::Getter
                | crate::ast::FuncKind::Setter
                | crate::ast::FuncKind::Constructor
        ) {
            self.enclosing_class_of_fn(f)
        } else {
            None
        };
        let is_static = owner_class.is_some() && self.is_static_member(f);
        let container_kind = match f.kind {
            crate::ast::FuncKind::Arrow => crate::checker::ContainerKind::Arrow,
            crate::ast::FuncKind::Method
            | crate::ast::FuncKind::Getter
            | crate::ast::FuncKind::Setter
            | crate::ast::FuncKind::Constructor => crate::checker::ContainerKind::Method,
            crate::ast::FuncKind::Declaration | crate::ast::FuncKind::Expression => {
                crate::checker::ContainerKind::NonArrowFn
            }
        };
        // An explicit `this:` parameter annotation on this function-like.
        let explicit_this = self.explicit_this_param_type(f, scope);
        let tc = crate::checker::ThisContainer {
            class_owner: owner_class,
            is_static,
            kind: container_kind,
            explicit_this,
        };
        let result = self.with_this_container(tc, |this| match &f.body {
            Some(FuncBody::Expr(e)) => {
                let t = this.check_expr(e, None);
                let was_fresh = this.types.is_fresh(t);
                let r = this.types.regular(t);
                if was_fresh {
                    this.types.widen_literal(r)
                } else {
                    r
                }
            }
            Some(FuncBody::Block(b)) => {
                let mut returns: Vec<&'a Expr> = Vec::new();
                collect_return_exprs(&b.stmts, &mut returns);
                if returns.is_empty() {
                    this.types.void
                } else {
                    let mut parts = Vec::new();
                    for e in returns {
                        let t = this.check_expr(e, None);
                        let was_fresh = this.types.is_fresh(t);
                        let r = this.types.regular(t);
                        let r = if was_fresh {
                            this.types.widen_literal(r)
                        } else {
                            r
                        };
                        parts.push(r);
                    }
                    this.types.union(parts)
                }
            }
            None => this.types.any,
        });
        self.current_scope = prev;
        result
    }

    fn resolve_type_ref(&mut self, r: &'a TypeRef, scope: ScopeId) -> TypeId {
        let first = &r.name.parts[0];
        if first.name.is_empty() {
            return self.types.error;
        }
        // `infer` / mapped-key names (synthetic, transient): pushed onto
        // `infer_mapped_env` by conditional- and mapped-type evaluation, where they
        // are minted on the fly and so live in no lexical scope. (Real signature
        // type params now resolve lexically through their transient `TypeParams`
        // scope — see `push_tp_scope` — so `lookup_type` finds them above and they
        // never reach here.) Match a bare reference to such a name here, innermost
        // binding first.
        if r.name.parts.len() == 1 {
            if let Some(&(_, sym)) = self
                .tp
                .infer_mapped_env
                .iter()
                .rev()
                .find(|(n, _)| n == &first.name)
            {
                return self.types.intern_kind(TypeKind::TypeParam(sym));
            }
            // `intrinsic` marks compiler-implemented string-manipulation aliases
            // (Uppercase/Lowercase/Capitalize/Uncapitalize). The transformation
            // is not modeled; the alias body resolves to `any`.
            if first.name == "intrinsic" && self.lookup_type(scope, "intrinsic").is_none() {
                return self.types.any;
            }
        }
        // dotted: namespace-qualified type (Geo.Point)
        if r.name.parts.len() > 1 {
            let ns = self
                .lookup_type(scope, &first.name)
                .or_else(|| self.lookup_value(scope, &first.name))
                .map(|s| self.resolve_alias_chain(s))
                .filter(|s| self.symbol(*s).flags & (flags::NAMESPACE | flags::ENUM) != 0);
            let Some(ns) = ns else {
                // spelling suggestion across namespace names (2833)
                {
                    let cands = self.namespace_names_in_scope(scope);
                    if let Some(sug) = crate::checker::spelling_suggestion(
                        &first.name,
                        cands.iter().map(|s| s.as_str()),
                    ) {
                        let sug = sug.to_string();
                        let args = [first.name.clone(), sug.clone()];
                        let related = self
                            .lookup_type(scope, &sug)
                            .or_else(|| self.lookup_value(scope, &sug))
                            .and_then(|sym| self.declared_here_related(sym))
                            .into_iter()
                            .collect();
                        self.error_at_with_related(
                            first.span,
                            &gen::Cannot_find_namespace_0_Did_you_mean_1,
                            &args,
                            related,
                        );
                        return self.types.error;
                    }
                }
                // a type-only symbol used as a namespace?
                if let Some(tsym) = self.lookup_type(scope, &first.name) {
                    let tf = self.symbol(tsym).flags;
                    if tf & flags::NAMESPACE == 0 {
                        let second = &r.name.parts[1];
                        // member of the type? → 2713 with the bracket hint
                        let tt = self.types.intern_kind(TypeKind::Iface(tsym));
                        let has_member = tf & (flags::INTERFACE | flags::CLASS) != 0
                            && self.prop_info_of_type(tt, &second.name).is_some();
                        if has_member {
                            self.error_at(
                                r.name.span,
                                &gen::Cannot_access_0_1_because_0_is_a_type_but_not_a_namespace_Did_you_mean_to_retrieve_the_type_of_the_property_1_in_0_with_0_1,
                                &[first.name.clone(), second.name.clone()],
                            );
                        } else {
                            self.error_at(
                                first.span,
                                &gen::_0_only_refers_to_a_type_but_is_being_used_as_a_namespace_here,
                                &[first.name.clone()],
                            );
                        }
                        return self.types.error;
                    }
                }
                self.error_at(
                    first.span,
                    &gen::Cannot_find_namespace_0,
                    &[first.name.clone()],
                );
                return self.types.error;
            };
            self.symuse.used_symbols.insert(ns);
            let second = &r.name.parts[1];
            // enum member used as a type: `E.A` is the literal type of that
            // member. Enum members live in `members`, not `statics`.
            if self.symbol(ns).flags & flags::ENUM != 0 {
                if let Some(member) = self
                    .symbol(ns)
                    .members
                    .0
                    .iter()
                    .find(|(n, _)| n == &second.name)
                    .map(|(_, m)| *m)
                {
                    self.symuse.used_symbols.insert(member);
                    return self.types.intern_kind(TypeKind::EnumMember(member));
                }
                let nsn = self.symbol(ns).name.clone();
                self.error_at(
                    second.span,
                    &gen::Namespace_0_has_no_exported_member_1,
                    &[nsn, second.name.clone()],
                );
                return self.types.error;
            }
            let Some(member) = self.symbol(ns).statics.get(&second.name) else {
                let nsn = self.symbol(ns).name.clone();
                self.error_at(
                    second.span,
                    &gen::Namespace_0_has_no_exported_member_1,
                    &[nsn, second.name.clone()],
                );
                return self.types.error;
            };
            self.symuse.used_symbols.insert(member);
            let mflags = self.symbol(member).flags;
            if mflags & (flags::INTERFACE | flags::CLASS) != 0 {
                return self.class_iface_ref_type(member, r, scope);
            }
            if mflags & flags::TYPE_ALIAS != 0 {
                return self.alias_ref_type(member, r, scope);
            }
            if mflags & flags::ENUM != 0 {
                return self.types.intern_kind(TypeKind::EnumType(member));
            }
            return self.types.intern_kind(TypeKind::Iface(member));
        }
        // a pure-value namespace is not a type (2709)
        if r.name.parts.len() == 1 {
            if self.lookup_type(scope, &first.name).is_none() {
                if let Some(vsym) = self.lookup_value(scope, &first.name) {
                    let vf = self.symbol(vsym).flags;
                    if vf & flags::NAMESPACE != 0
                        && vf & (flags::CLASS | flags::INTERFACE | flags::ENUM) == 0
                    {
                        self.error_at(
                            first.span,
                            &gen::Cannot_use_namespace_0_as_a_type,
                            &[first.name.clone()],
                        );
                        return self.types.error;
                    }
                }
            }
        }
        let Some(sym0) = self.lookup_type(scope, &first.name) else {
            // value used as type?
            if let Some(vsym) = self.lookup_value(scope, &first.name) {
                let f = self.symbol(vsym).flags;
                if f & flags::TYPE == 0 && f & flags::ALIAS == 0 {
                    self.error_at(
                        first.span,
                        &gen::_0_refers_to_a_value_but_is_being_used_as_a_type_here_Did_you_mean_typeof_0,
                        &[first.name.clone()],
                    );
                    return self.types.error;
                }
            }
            let type_names = self.type_names_in_scope(scope);
            if let Some(sug) =
                super::spelling_suggestion(&first.name, type_names.iter().map(|s| s.as_str()))
            {
                self.error_at(
                    first.span,
                    &gen::Cannot_find_name_0_Did_you_mean_1,
                    &[first.name.clone(), sug.to_string()],
                );
            } else {
                self.error_at(first.span, &gen::Cannot_find_name_0, &[first.name.clone()]);
            }
            return self.types.error;
        };
        let sym = self.resolve_alias_chain(sym0);
        self.symuse.used_symbols.insert(sym0);
        self.symuse.used_symbols.insert(sym);
        let sflags = self.symbol(sym).flags;

        if sflags & flags::TYPE_PARAM != 0 {
            return self.types.intern_kind(TypeKind::TypeParam(sym));
        }
        if sflags & flags::ENUM != 0 {
            return self.types.intern_kind(TypeKind::EnumType(sym));
        }
        if sflags & flags::TYPE_ALIAS != 0 {
            return self.alias_ref_type(sym, r, scope);
        }
        if sflags & (flags::INTERFACE | flags::CLASS) != 0 {
            self.class_iface_ref_type(sym, r, scope)
        } else {
            // not a type (shouldn't reach: lookup_type only returns type-space)
            self.types.error
        }
    }

    /// A (possibly generic) class/interface type reference: arity checking,
    /// default filling, constraint checking. Shared by the bare and the
    /// namespace-qualified (`Geo.Point<T>`) resolution paths.
    fn class_iface_ref_type(&mut self, sym: SymbolId, r: &'a TypeRef, scope: ScopeId) -> TypeId {
        {
            let tparams = self.type_params_of_symbol(sym);
            let args = r.type_args.as_ref();
            if tparams.is_empty() {
                if args.is_some() {
                    let name = self.symbol(sym).name.clone();
                    self.error_at(r.span, &gen::Type_0_is_not_generic, &[name]);
                    return self.types.error;
                }
                return self.types.intern_kind(TypeKind::Iface(sym));
            }
            // empty `<>` is its own grammar error (1099)
            if let Some(a) = args {
                if a.is_empty() {
                    let lt = r.name.span.end as usize;
                    self.error_at(
                        Span::new(lt, lt + 1),
                        &gen::Type_argument_list_cannot_be_empty,
                        &[],
                    );
                }
            }
            // arity with defaults: only non-defaulted params are required
            let required = self.required_type_param_count(&tparams);
            let given = args.map(|a| a.len()).unwrap_or(0);
            if given < required || given > tparams.len() {
                let display = self.generic_name_with_params(sym);
                let span = if args.is_some() { r.span } else { r.name.span };
                if required < tparams.len() {
                    self.error_at(
                        span,
                        &gen::Generic_type_0_requires_between_1_and_2_type_arguments,
                        &[display, required.to_string(), tparams.len().to_string()],
                    );
                } else {
                    self.error_at(
                        span,
                        &gen::Generic_type_0_requires_1_type_argument_s,
                        &[display, required.to_string()],
                    );
                }
                return self.types.error;
            }
            let mut arg_types: Vec<TypeId> = match args {
                Some(args) => args.iter().map(|a| self.resolve_type(a, scope)).collect(),
                None => Vec::new(),
            };
            // fill defaults (instantiated with the args so far)
            for i in arg_types.len()..tparams.len() {
                let d = self.default_of_type_param(tparams[i]);
                let d = match d {
                    Some(d) => {
                        let mut mapper = Mapper::new();
                        for (j, &p) in tparams.iter().enumerate().take(i) {
                            mapper.insert(p, arg_types[j]);
                        }
                        self.instantiate_type(d, &mapper)
                    }
                    None => self.types.any,
                };
                arg_types.push(d);
            }
            if let Some(args) = args {
                self.check_type_arg_constraints(sym, &tparams, &arg_types, args);
            }
            self.types.ref_type(sym, arg_types)
        }
    }

    pub fn type_names_in_scope(&self, scope: ScopeId) -> Vec<String> {
        let mut names = Vec::new();
        let mut cur = Some(scope);
        while let Some(s) = cur {
            let sc = self.scope_at(s);
            for (n, _) in sc.types.iter() {
                if !names.contains(n) {
                    names.push(n.clone());
                }
            }
            cur = sc.parent;
        }
        names
    }

    pub fn generic_name_with_params(&mut self, sym: SymbolId) -> String {
        let name = self.symbol(sym).name.clone();
        let tparams = self.type_params_of_symbol(sym);
        if tparams.is_empty() {
            return name;
        }
        let names: Vec<String> = tparams
            .iter()
            .map(|tp| self.symbol(*tp).name.clone())
            .collect();
        format!("{}<{}>", name, names.join(", "))
    }

    pub fn type_params_of_symbol(&mut self, sym: SymbolId) -> Vec<SymbolId> {
        let s = self.symbol(sym);
        for d in s.decls.clone() {
            let tps = match d {
                Decl::Interface(i) => &i.type_params,
                Decl::Class(c) => &c.type_params,
                Decl::Alias(a) => &a.type_params,
                _ => &None,
            };
            if let Some(tps) = tps {
                return tps
                    .iter()
                    .filter_map(|tp| self.bind.decl_symbol.get(&node_key(tp)).copied())
                    .collect();
            }
        }
        Vec::new()
    }

    pub(crate) fn check_type_arg_constraints(
        &mut self,
        _sym: SymbolId,
        tparams: &[SymbolId],
        arg_types: &[TypeId],
        arg_nodes: &'a [TypeNode],
    ) {
        for (i, &tp) in tparams.iter().enumerate() {
            // a parameter past the supplied arguments was omitted and will be
            // filled from its default; its constraint is validated at the
            // declaration, and there is no argument node to anchor an error to.
            if i >= arg_types.len() || i >= arg_nodes.len() {
                continue;
            }
            if let Some(constraint) = self.constraint_of_type_param(tp) {
                // instantiate constraint with the supplied args (F-bounded)
                let mut mapper = Mapper::new();
                for (j, &p) in tparams.iter().enumerate() {
                    if j < arg_types.len() {
                        mapper.insert(p, arg_types[j]);
                    }
                }
                let c = self.instantiate_type(constraint, &mapper);
                let arg = arg_types[i];
                // generic contexts defer the check until instantiation
                if self.type_contains_params(arg) || self.type_contains_params(c) {
                    continue;
                }
                if !self.is_assignable_to(arg, c)
                    && !self.types.is_any_or_error(arg)
                    && !self.types.is_error(c)
                {
                    let a = self.display_type(arg);
                    let cd = self.display_type(c);
                    self.error_at(
                        arg_nodes[i].span(),
                        &gen::Type_0_does_not_satisfy_the_constraint_1,
                        &[a, cd],
                    );
                }
            }
        }
    }

    pub fn required_type_param_count(&mut self, tparams: &[SymbolId]) -> usize {
        let mut required = 0;
        for (i, &tp) in tparams.iter().enumerate() {
            let has_default = matches!(
                self.symbol(tp).decls.first(),
                Some(Decl::TypeParam(t)) if t.default.is_some()
            );
            if !has_default {
                required = i + 1;
            }
        }
        required
    }

    pub fn default_of_type_param(&mut self, tp: SymbolId) -> Option<TypeId> {
        let Some(Decl::TypeParam(t)) = self.symbol(tp).decls.first().copied() else {
            return None;
        };
        let d = t.default.as_ref()?;
        let scope = self.scope_of_decl(node_key(t));
        // resolve the default in its own file so any diagnostic it produces is
        // anchored to that file's text, not the caller's (a lib type parameter
        // default resolved during a call in user code would otherwise index the
        // user file with a lib offset).
        let file = self.symbol(tp).file;
        let prev = self.current_file;
        self.current_file = file;
        let r = self.resolve_type(d, scope);
        self.current_file = prev;
        Some(r)
    }

    /// The instance ("self") type of a class/interface owner: the parameterized
    /// reference for a generic owner so type arguments flow through, or the
    /// plain interface type otherwise.
    pub(crate) fn owner_instance_type(&mut self, owner: SymbolId) -> TypeId {
        let tps = self.type_params_of_symbol(owner);
        if tps.is_empty() {
            self.types.intern_kind(TypeKind::Iface(owner))
        } else {
            let args: Vec<TypeId> = tps
                .iter()
                .map(|&p| self.types.intern_kind(TypeKind::TypeParam(p)))
                .collect();
            self.types.ref_type(owner, args)
        }
    }

    /// Get (or create) the polymorphic `this`-type parameter for `owner`. A
    /// synthetic type-parameter symbol named "this"; its constraint is
    /// produced on demand by `constraint_of_type_param` from
    /// `owner_instance_type`, so apparent-type-driven member access and
    /// assignability against the owner work without further plumbing.
    pub(crate) fn this_param_of(&mut self, owner: SymbolId) -> SymbolId {
        if let Some(&p) = self.deferred.this_params.get(&owner) {
            return p;
        }
        // a key space distinct from mapped-key / infer synthetic params
        let key = 0x7468_6973usize.rotate_left(32) ^ (owner.0 as usize);
        let p = self.synthetic_type_param(key, "this");
        self.deferred.this_params.insert(owner, p);
        self.deferred.this_param_owner.insert(p, owner);
        p
    }

    /// If `tp` is a polymorphic `this` parameter, the owner it stands for.
    pub(crate) fn this_param_owner(&self, tp: SymbolId) -> Option<SymbolId> {
        self.deferred.this_param_owner.get(&tp).copied()
    }

    pub fn constraint_of_type_param(&mut self, tp: SymbolId) -> Option<TypeId> {
        // A polymorphic `this` parameter constrains to its owner's instance type,
        // so member access / assignability on an unsubstituted `this` see the
        // owner's members. This is independent of any user-written constraint
        // (a `this` parameter has no syntactic declaration).
        if let Some(owner) = self.this_param_owner(tp) {
            return Some(self.owner_instance_type(owner));
        }
        if self
            .res
            .resolving
            .iter()
            .any(|(s, slot)| *s == tp && *slot == Slot::Constraint)
        {
            // circular constraint — reported at the constraint type node
            if let Some(Decl::TypeParam(t)) = self.symbol(tp).decls.first().copied() {
                if self.res.resolution_failed.insert((tp, Slot::Constraint)) {
                    let name = self.symbol(tp).name.clone();
                    let file = self.symbol(tp).file;
                    let span = t
                        .constraint
                        .as_ref()
                        .map(|c| c.span())
                        .unwrap_or(t.name.span);
                    let prev = self.current_file;
                    self.current_file = file;
                    self.error_at(
                        span,
                        &gen::Type_parameter_0_has_a_circular_constraint,
                        &[name],
                    );
                    self.current_file = prev;
                }
            }
            return Some(self.types.error);
        }
        let Some(Decl::TypeParam(t)) = self.symbol(tp).decls.first().copied() else {
            return None;
        };
        let c = t.constraint.as_ref()?;
        self.res.resolving.push((tp, Slot::Constraint));
        let scope = self.scope_of_decl(node_key(t));
        let r = self.resolve_type(c, scope);
        // transitive: `T extends U` where U is itself a constrained param
        if let TypeKind::TypeParam(s2) = self.types.kind(r).clone() {
            self.constraint_of_type_param(s2);
        }
        self.res.resolving.pop();
        Some(r)
    }

    pub fn base_constraint_of_type_param(&mut self, tp: SymbolId) -> Option<TypeId> {
        let mut cur = tp;
        let mut seen = Vec::new();
        loop {
            if seen.contains(&cur) {
                return Some(self.types.error);
            }
            seen.push(cur);
            let constraint = self.constraint_of_type_param(cur)?;
            match self.types.kind(constraint).clone() {
                TypeKind::TypeParam(next) => cur = next,
                _ => return Some(constraint),
            }
        }
    }

    /// Infer the `infer`-placeholder bindings of a conditional type's `extends`
    /// clause: for each symbol in `tps`, returns its first inferred candidate (or
    /// `None` if unconstrained). Used by conditional-type evaluation, where
    /// `T extends Promise<infer U> ? … : …` infers `U` from the checked type.
    pub fn infer_conditional_bindings(
        &mut self,
        ext_t: TypeId,
        check_t: TypeId,
        tps: &[SymbolId],
    ) -> std::collections::HashMap<SymbolId, TypeId> {
        let mut infos: super::infer::InferMap = Default::default();
        self.infer_from(
            ext_t,
            check_t,
            tps,
            &mut infos,
            super::infer::infer_prio::NONE,
            false,
        );
        let mut out = std::collections::HashMap::new();
        for &tp in tps {
            if let Some(info) = infos.get(&tp) {
                if let Some(&c0) = info
                    .candidates
                    .first()
                    .or_else(|| info.contra_candidates.first())
                {
                    out.insert(tp, self.types.regular(c0));
                }
            }
        }
        out
    }

    pub fn ensure_enum_checked(&mut self, member_or_enum: SymbolId) {
        let target = if self.symbol(member_or_enum).flags & flags::ENUM_MEMBER != 0 {
            self.symbol(member_or_enum).parent
        } else {
            Some(member_or_enum)
        };
        let Some(enum_sym) = target else { return };
        if let Some(Decl::Enum(e)) = self.symbol(enum_sym).decls.first().copied() {
            self.check_enum_pub(e);
        }
    }

    /// (has_numeric_members, has_string_members)
    pub fn enum_member_kinds_of(&mut self, t: TypeId) -> (bool, bool) {
        let kind = self.types.kind(t).clone();
        let sym = match kind {
            TypeKind::EnumType(s) | TypeKind::EnumObject(s) => s,
            TypeKind::EnumMember(m) => {
                self.ensure_enum_checked(m);
                return match self.enums.enum_member_values.get(&m) {
                    Some(super::EnumValue::Str(_)) => (false, true),
                    _ => (true, false),
                };
            }
            _ => return (false, false),
        };
        self.ensure_enum_checked(sym);
        let members: Vec<SymbolId> = self.symbol(sym).members.0.iter().map(|(_, m)| *m).collect();
        let mut numeric = false;
        let mut string = false;
        for m in members {
            match self.enums.enum_member_values.get(&m) {
                Some(super::EnumValue::Str(_)) => string = true,
                _ => numeric = true,
            }
        }
        (numeric, string)
    }

    // ── instantiation ───────────────────────────────────────────────────────
}

/// walk a type node finding `infer X` declarations (callback gets node key + name)
pub(crate) fn collect_infer_nodes<'b>(
    node: &'b crate::ast::TypeNode,
    f: &mut impl FnMut(usize, &'b str),
) {
    use crate::ast::TypeNode as T;
    match node {
        T::Infer {
            name, constraint, ..
        } => {
            f(crate::ast::node_key(node), &name.name);
            if let Some(constraint) = constraint {
                collect_infer_nodes(constraint, f);
            }
        }
        T::Paren { inner, .. }
        | T::Array { elem: inner, .. }
        | T::Keyof { ty: inner, .. }
        | T::ReadonlyOp { ty: inner, .. } => collect_infer_nodes(inner, f),
        T::Union { members, .. } | T::Intersection { members, .. } => {
            for m in members {
                collect_infer_nodes(m, f);
            }
        }
        T::Tuple { elems, .. } => {
            for e in elems {
                collect_infer_nodes(&e.ty, f);
            }
        }
        T::Function(ft) | T::Ctor(ft) => {
            for p in &ft.params {
                if let Some(ty) = &p.ty {
                    collect_infer_nodes(ty, f);
                }
            }
            collect_infer_nodes(&ft.return_type, f);
        }
        T::Ref(r) => {
            if let Some(args) = &r.type_args {
                for a in args {
                    collect_infer_nodes(a, f);
                }
            }
        }
        T::IndexedAccess { obj, index, .. } => {
            collect_infer_nodes(obj, f);
            collect_infer_nodes(index, f);
        }
        T::TypeLiteral { members, .. } => {
            for m in members {
                if let crate::ast::TypeMember::Prop(p) = m {
                    if let Some(ty) = &p.ty {
                        collect_infer_nodes(ty, f);
                    }
                }
            }
        }
        T::TemplateLit { parts, .. } => {
            for (ty, _) in parts {
                collect_infer_nodes(ty, f);
            }
        }
        _ => {}
    }
}

/// yield argument expressions, skipping nested function bodies
fn collect_yield_exprs<'b>(stmts: &'b [crate::ast::Stmt], out: &mut Vec<&'b crate::ast::Expr>) {
    use crate::ast::{Expr, Stmt};
    fn walk_expr<'b>(e: &'b Expr, out: &mut Vec<&'b Expr>) {
        match e {
            Expr::Yield { expr, .. } => {
                if let Some(inner) = expr {
                    out.push(inner);
                }
            }
            Expr::Binary { left, right, .. } => {
                walk_expr(left, out);
                walk_expr(right, out);
            }
            Expr::Paren { inner, .. } => walk_expr(inner, out),
            Expr::Call { callee, args, .. } => {
                walk_expr(callee, out);
                for a in args {
                    walk_expr(a, out);
                }
            }
            _ => {}
        }
    }
    for s in stmts {
        match s {
            Stmt::Expr { expr, .. } => walk_expr(expr, out),
            Stmt::Return { expr: Some(e), .. } => walk_expr(e, out),
            Stmt::Var(v) => {
                for d in &v.decls {
                    if let Some(init) = &d.init {
                        walk_expr(init, out);
                    }
                }
            }
            Stmt::Block(b) => collect_yield_exprs(&b.stmts, out),
            Stmt::If {
                cond, then, els, ..
            } => {
                walk_expr(cond, out);
                collect_yield_exprs(std::slice::from_ref(then), out);
                if let Some(e) = els {
                    collect_yield_exprs(std::slice::from_ref(e), out);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::For { body, .. }
            | Stmt::ForIn { body, .. }
            | Stmt::ForOf { body, .. }
            | Stmt::Labeled { stmt: body, .. } => {
                collect_yield_exprs(std::slice::from_ref(body), out)
            }
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                collect_yield_exprs(&block.stmts, out);
                if let Some(c) = catch {
                    collect_yield_exprs(&c.block.stmts, out);
                }
                if let Some(fin) = finally {
                    collect_yield_exprs(&fin.stmts, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    collect_yield_exprs(&c.stmts, out);
                }
            }
            _ => {}
        }
    }
}

pub fn collect_type_ref_names_pub<'b>(
    node: &'b crate::ast::TypeNode,
    f: &mut impl FnMut(&'b str, Span),
) {
    collect_type_ref_names(node, f)
}

/// walk a type node calling back for each single-ident type reference
fn collect_type_ref_names<'b>(node: &'b crate::ast::TypeNode, f: &mut impl FnMut(&'b str, Span)) {
    use crate::ast::TypeNode as T;
    match node {
        T::Ref(r) => {
            if r.name.parts.len() == 1 {
                f(&r.name.parts[0].name, r.name.parts[0].span);
            }
            if let Some(args) = &r.type_args {
                for a in args {
                    collect_type_ref_names(a, f);
                }
            }
        }
        T::Paren { inner, .. }
        | T::Array { elem: inner, .. }
        | T::Keyof { ty: inner, .. }
        | T::ReadonlyOp { ty: inner, .. } => collect_type_ref_names(inner, f),
        T::Union { members, .. } | T::Intersection { members, .. } => {
            for m in members {
                collect_type_ref_names(m, f);
            }
        }
        T::IndexedAccess { obj, index, .. } => {
            collect_type_ref_names(obj, f);
            collect_type_ref_names(index, f);
        }
        _ => {}
    }
}
