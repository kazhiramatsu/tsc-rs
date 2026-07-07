//! Member & element access: property access with nullish handling, element
//! access, member-access control (visibility) checks, and entity-name
//! resolution. Split out of `exprs.rs`.

use crate::ast::*;
use crate::binder::flags;
use crate::checker::{Checker, RefKey};
use crate::diagnostics::{gen, DiagnosticMessage};
use crate::types::{PropInfo, TypeId, TypeKind};

impl<'a> Checker<'a> {
    fn display_prop_access_object_type(&mut self, obj_t: TypeId) -> String {
        if let TypeKind::TypeParam(tp) = self.types.kind(obj_t).clone() {
            if let Some(owner) = self.this_param_owner(tp) {
                return self.generic_name_with_params(owner);
            }
        }
        self.display_type(obj_t)
    }

    /// entity-name-like receiver: dotted identifier chain (for 18047/18048/18049/18046)
    fn entity_name_text(&self, e: &Expr) -> Option<String> {
        // optional-chain links render without `?.` in nullability errors
        fn walk(e: &Expr, out: &mut String) -> bool {
            match e {
                Expr::Ident(id) => {
                    out.push_str(&id.name);
                    true
                }
                Expr::PropAccess { obj, name, .. } => {
                    if !walk(obj, out) {
                        return false;
                    }
                    out.push('.');
                    out.push_str(&name.name);
                    true
                }
                _ => false,
            }
        }
        let mut s = String::new();
        if walk(e, &mut s) {
            return Some(s);
        }
        return self.entity_name_text_old(e);
    }

    fn entity_name_text_old(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Ident(id) => Some(id.name.clone()),
            Expr::PropAccess {
                obj,
                name,
                question_dot: false,
                ..
            } => {
                let base = self.entity_name_text(obj)?;
                Some(format!("{}.{}", base, name.name))
            }
            _ => None,
        }
    }

    pub(crate) fn ref_key_of(&self, e: &Expr) -> Option<RefKey> {
        match e {
            Expr::Ident(id) => {
                let sym = self.lookup_value(self.current_scope, &id.name)?;
                Some(RefKey(sym, Vec::new()))
            }
            Expr::PropAccess {
                obj,
                name,
                question_dot: false,
                ..
            } => {
                let mut k = self.ref_key_of(obj)?;
                k.1.push(name.name.clone());
                Some(k)
            }
            Expr::Paren { inner, .. } => self.ref_key_of(inner),
            _ => None,
        }
    }

    /// nullish receiver checks; returns the non-nullish type to continue with
    pub(crate) fn check_non_nullish(
        &mut self,
        t: TypeId,
        recv: &'a Expr,
        for_call: bool,
    ) -> TypeId {
        self.check_non_nullish_ex(t, recv, for_call, true)
    }

    /// binary/unary OPERAND variant (tsc checkNonNullType at operator
    /// positions): `void` is NOT nullable there — `t + f` with `f: void`
    /// keeps its 2365, no 18048 (receivers keep treating void like
    /// undefined, matching the existing corpus-validated behavior)
    pub(crate) fn check_operand_non_nullish(&mut self, t: TypeId, operand: &'a Expr) -> TypeId {
        self.check_non_nullish_ex(t, operand, false, false)
    }

    fn check_non_nullish_ex(
        &mut self,
        t: TypeId,
        recv: &'a Expr,
        for_call: bool,
        void_as_undef: bool,
    ) -> TypeId {
        if matches!(self.types.kind(t), TypeKind::Unknown) {
            if let Some(name) = self.entity_name_text(recv) {
                self.error_at(recv.span(), &gen::_0_is_of_type_unknown, &[name]);
            } else {
                self.error_at(recv.span(), &gen::Object_is_of_type_unknown, &[]);
            }
            return self.types.error;
        }
        if !self.options.strict_null_checks() {
            // a receiver whose type is EXACTLY nullish errors even without
            // strictNullChecks (tsc checkNonNullType: getNonNullableType
            // collapses it to never) — e.g. `let x; x.foo` where the flow
            // type is `undefined`. Mixed unions keep the lenient path.
            let members = self.types.union_members(t);
            let pure_nullish = !members.is_empty()
                && members
                    .iter()
                    .all(|&m| matches!(self.types.kind(m), TypeKind::Null | TypeKind::Undefined));
            if !pure_nullish {
                return t;
            }
        }
        let members = self.types.union_members(t);
        let has_null = members
            .iter()
            .any(|&m| matches!(self.types.kind(m), TypeKind::Null));
        let has_undef = members.iter().any(|&m| match self.types.kind(m) {
            TypeKind::Undefined => true,
            TypeKind::Void => void_as_undef,
            _ => false,
        });
        if has_null || has_undef {
            if for_call {
                let msg: &'static DiagnosticMessage = match (has_null, has_undef) {
                    (true, true) => {
                        &gen::Cannot_invoke_an_object_which_is_possibly_null_or_undefined
                    }
                    (true, false) => &gen::Cannot_invoke_an_object_which_is_possibly_null,
                    (false, _) => &gen::Cannot_invoke_an_object_which_is_possibly_undefined,
                };
                self.error_at(recv.span(), msg, &[]);
            } else if let Some(name) = self.entity_name_text(recv) {
                let msg: &'static DiagnosticMessage = match (has_null, has_undef) {
                    (true, true) => &gen::_0_is_possibly_null_or_undefined,
                    (true, false) => &gen::_0_is_possibly_null,
                    (false, _) => &gen::_0_is_possibly_undefined,
                };
                self.error_at(recv.span(), msg, &[name]);
            } else {
                let msg: &'static DiagnosticMessage = match (has_null, has_undef) {
                    (true, true) => &gen::Object_is_possibly_null_or_undefined,
                    (true, false) => &gen::Object_is_possibly_null,
                    (false, _) => &gen::Object_is_possibly_undefined,
                };
                self.error_at(recv.span(), msg, &[]);
            }
            let non_null = self.non_nullable(t);
            if matches!(self.types.kind(non_null), TypeKind::Never) {
                return self.types.error;
            }
            return non_null;
        }
        t
    }

    /// TS2565 (tsc getFlowTypeOfAccessExpression's assumeUninitialized
    /// path): a `this.<name>` read whose control-flow container is the
    /// DECLARING class's constructor, where the property is an
    /// initializer-less, non-static, non-`!`, non-abstract, non-optional
    /// declaration — run the seeded walk from the read; a surviving
    /// `undefined` is a use before assignment. Emission-only: the read
    /// keeps the checker-computed member type (tsc returns propType after
    /// the error).
    fn da_check_this_prop_read(&mut self, e: &'a Expr, name: &Ident) {
        if !self.options.strict_null_checks() || !self.options.strict_property_initialization() {
            return;
        }
        // an assignment TARGET is not a read (tsc AssignmentKind.Definite):
        // `this.x = 1` must not 2565 itself, nor pattern leaves
        // (`({x: this.x} = …)`)
        if self.cflags.assign_target == crate::checker::exprs::node_key_expr(e)
            || self.cflags.pattern_target > 0
        {
            return;
        }
        let Some(f) = self.stacks.fn_stack.last() else {
            return;
        };
        if f.kind != FuncKind::Constructor {
            return;
        }
        let Some(&cls) = self.stacks.class_stack.last() else {
            return;
        };
        let Some(msym) = self.bind.symbols[cls.0 as usize].members.get(&name.name) else {
            return;
        };
        if self.symbol(msym).flags & (flags::ABSTRACT | flags::AMBIENT) != 0 {
            return;
        }
        let prop_ok = self.symbol(msym).decls.iter().all(|d| {
            matches!(d,
                crate::binder::Decl::PropertyDecl(p) if p.init.is_none()
                    && !p.exclam
                    && !p.question
                    && !has_modifier(&p.modifiers, ModifierKind::Static)
                    && !has_modifier(&p.modifiers, ModifierKind::Abstract)
                    && !has_modifier(&p.modifiers, ModifierKind::Declare))
        });
        if !prop_ok {
            return;
        }
        let this_sym = self.this_param_of(cls);
        let key = crate::checker::RefKey(this_sym, vec![name.name.clone()]);
        self.fresolve.this_sym = Some(this_sym);
        let prop_ty = self.declared_type_of_ref(&key);
        self.fresolve.this_sym = None;
        let Some(prop_ty) = prop_ty else { return };
        if matches!(
            self.types.kind(prop_ty),
            TypeKind::Any | TypeKind::Unknown | TypeKind::Error
        ) || self.contains_undefined_member(prop_ty)
        {
            return;
        }
        let undef = self.types.undefined;
        let initial = self.types.union(vec![prop_ty, undef]);
        self.fresolve.this_sym = Some(this_sym);
        let t = self.flow_type_of_da_read(
            crate::checker::exprs::node_key_expr(e),
            &key,
            initial,
            name.span,
            true,
        );
        self.fresolve.this_sym = None;
        if let Some(t) = t {
            if self.contains_undefined_member(t) {
                self.error_at(
                    name.span,
                    &gen::Property_0_is_used_before_being_assigned,
                    &[name.name.clone()],
                );
            }
        }
    }

    pub fn non_nullable(&mut self, t: TypeId) -> TypeId {
        match self.types.kind(t).clone() {
            TypeKind::Null | TypeKind::Undefined => self.types.never,
            TypeKind::Union(members) => {
                let mut kept: Vec<TypeId> = Vec::new();
                for member in members {
                    let nn = self.non_nullable(member);
                    if !matches!(self.types.kind(nn), TypeKind::Never) {
                        kept.push(nn);
                    }
                }
                if kept.is_empty() {
                    self.types.never
                } else {
                    self.types.union(kept)
                }
            }
            TypeKind::TypeParam(_) => self.symbolic_non_nullable(t),
            _ => t,
        }
    }

    pub(crate) fn symbolic_non_nullable(&mut self, t: TypeId) -> TypeId {
        let empty = self.empty_object_type();
        let nn = self.intersect_all(vec![t, empty]);
        if matches!(self.types.kind(nn), TypeKind::Intersection(_)) {
            if let Some(sym) = self.global_type_symbol("NonNullable") {
                self.types.set_alias(nn, sym, vec![t]);
            }
        }
        nn
    }

    fn type_may_be_nullish(&mut self, t: TypeId) -> bool {
        match self.types.kind(t).clone() {
            TypeKind::Null | TypeKind::Undefined | TypeKind::Void => true,
            TypeKind::Union(members) => members.iter().any(|&m| self.type_may_be_nullish(m)),
            TypeKind::Intersection(members) => {
                if members.iter().any(|&m| self.is_empty_object_type(m)) {
                    return false;
                }
                members.iter().all(|&m| self.type_may_be_nullish(m))
            }
            TypeKind::TypeParam(tp) => match self.constraint_of_type_param(tp) {
                Some(constraint) => self.type_may_be_nullish(constraint),
                None => true,
            },
            TypeKind::Any | TypeKind::Unknown => true,
            _ => false,
        }
    }

    pub(crate) fn check_prop_access(&mut self, e: &'a Expr) -> TypeId {
        let Expr::PropAccess {
            obj,
            question_dot,
            name,
            ..
        } = e
        else {
            unreachable!()
        };
        let prev_flag = self.enums.const_enum_ident_ok;
        self.enums.const_enum_ident_ok = true;
        // the receiver of a member-access target is a read even inside an
        // assignment pattern (tsc accessKind: PropertyAccess → Read)
        let mut obj_t = self.in_read_position(|c| c.check_expr(obj, None));
        self.enums.const_enum_ident_ok = prev_flag;
        if self.types.is_error(obj_t) {
            return self.types.error;
        }
        let optional_out = *question_dot && self.type_may_be_nullish(obj_t);
        if *question_dot {
            let non_null = self.non_nullable(obj_t);
            obj_t = non_null;
        } else {
            obj_t = self.check_non_nullish(obj_t, obj, false);
            if self.types.is_error(obj_t) {
                return self.types.error;
            }
        }
        if self.types.is_any_or_error(obj_t) {
            return self.types.any;
        }
        // tsc checkPropertyAccessibilityAtLocation, isSuper arm: ES5 fields
        // #private members are only accessible inside their class (18013)
        // 2565 (tsc getFlowTypeOfAccessExpression): a `this.prop` read
        // inside the DECLARING class's own constructor, where prop is an
        // initializer-less, non-static, non-`!`, non-abstract property —
        // seed `propType | undefined` at the read's flow node; a surviving
        // undefined is a use before assignment.
        if matches!(&**obj, Expr::This { .. }) {
            self.da_check_this_prop_read(e, name);
        }
        // 2729: this.X in a property initializer where X is declared later.
        // Resolves the owning class from `class_stack` and chooses
        // members/statics from `this_container_stack`'s static-ness, so this
        // doesn't depend on the `this` expression's representation.
        if let (Some(init_pos), Expr::This { .. }) = (self.prop_init_pos, &**obj) {
            if let Some(&cls) = self.stacks.class_stack.last() {
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
                let table = if in_static {
                    &self.bind.symbols[cls.0 as usize].statics
                } else {
                    &self.bind.symbols[cls.0 as usize].members
                };
                let later = table.get(&name.name).and_then(|m| {
                    if !self.options.use_define_for_class_fields()
                        && self
                            .symbol(m)
                            .decls
                            .iter()
                            .any(|d| matches!(d, crate::binder::Decl::Param(_)))
                    {
                        None
                    } else {
                        self.symbol(m)
                            .decls
                            .first()
                            .map(|d| (d.name_span().start as usize, m))
                    }
                });
                if let Some((pos, sym)) = later {
                    if pos > init_pos {
                        self.error_at_declared_here(
                            name.span,
                            &gen::Property_0_is_used_before_its_initialization,
                            &[name.name.clone()],
                            sym,
                        );
                    }
                }
            }
        }
        let this_receiver = if matches!(&**obj, Expr::Super { .. }) {
            self.current_this_receiver_type().unwrap_or(obj_t)
        } else {
            obj_t
        };
        if self.cflags.assign_target == crate::checker::exprs::node_key_expr(e)
            && !self.cflags.assign_target_rw
            && !*question_dot
            && self.prop_info_of_type(obj_t, &name.name).is_none()
            && self.is_function_expando_base(obj)
        {
            return self.types.any;
        }
        if let Some(sym) = self.function_expando_base_symbol(obj) {
            if let Some(&ty) = self.function_expando_props.get(&(sym, name.name.clone())) {
                return ty;
            }
        }
        let result = self.prop_access_type(obj_t, name, this_receiver);
        // usage tracking: mark the accessed member symbol used so check_unused
        // (private members, namespace/enum exports) does not flag it. Works for
        // any type whose property carries a symbol (Iface / NamespaceObj / Enum).
        // For private/#-named members tsc applies markPropertyAsReferenced
        // rules: a write-only access does not mark (unless the member is a
        // setter — the write invokes it), and neither does a method reading
        // itself via `this.m` inside its own body (isSelfTypeAccess).
        // #names are lexically scoped: when the receiver-type lookup
        // SUCCEEDS, the symbol tsc marks is the one the enclosing class
        // bodies resolve (`new Child().#foo` inside Parent marks PARENT's
        // #foo, not Child's shadowing slot). A FAILED lookup (`Child.#bar`
        // through a subclass -> 2339) marks nothing at all.
        let receiver_sym = self
            .prop_info_of_type(obj_t, &name.name)
            .and_then(|pi| pi.symbol);
        let mark = if name.name.starts_with('#') {
            receiver_sym.map(|p| self.lookup_private_member(&name.name).unwrap_or(p))
        } else {
            receiver_sym
        };
        if let Some(msym) = mark {
            let is_private = name.name.starts_with('#')
                || self.symbol(msym).decls.first().is_some_and(|d| {
                    let mods = match d {
                        crate::binder::Decl::PropertyDecl(p) => &p.modifiers,
                        crate::binder::Decl::Method(f) => &f.modifiers,
                        crate::binder::Decl::Param(p) => &p.modifiers,
                        _ => return false,
                    };
                    mods.iter()
                        .any(|m| m.kind == crate::ast::ModifierKind::Private)
                });
            let skip = is_private && {
                let plain_write = (self.cflags.assign_target
                    == crate::checker::exprs::node_key_expr(e)
                    && !self.cflags.assign_target_rw)
                    || self.cflags.pattern_target > 0;
                // auto-accessor fields count as setters too: writing one
                // invokes the generated setter (tsc gives them SetAccessor
                // flags, so the write-only exception never applies)
                let is_setter = self.symbol(msym).flags & crate::binder::flags::SET_ACCESSOR != 0
                    || self.symbol(msym).decls.first().is_some_and(|d| {
                        matches!(d, crate::binder::Decl::PropertyDecl(p)
                            if p.accessor_span.is_some())
                    });
                let self_access = matches!(&**obj, Expr::This { .. })
                    && self
                        .stacks
                        .fn_stack
                        .last()
                        .is_some_and(|f| self.bind.decl_symbol.get(&f.fn_key) == Some(&msym));
                (plain_write && !is_setter) || self_access
            };
            if !skip {
                self.symuse.used_symbols.insert(msym);
            }
        }
        let mut result = match result {
            Some(t) => t,
            None => return self.types.error,
        };
        // narrowing on the path: the flow-graph resolver is the single
        // engine (Stage 4). A resolver answer equal to its own declared
        // baseline means "no narrowing" — keep the checker-computed member
        // type (`prop_access_type` handles this-substitution and namespace
        // members the resolver's declared walk does not).
        if self.cflags.assign_target != crate::checker::exprs::node_key_expr(e)
            && self.cflags.pattern_target == 0
        {
            if let Some(k) = self.ref_key_of(e) {
                let resolved = self.flow_type_of_read(crate::checker::exprs::node_key_expr(e), &k);
                if let Some(t) = resolved {
                    if Some(t) != self.declared_type_of_ref(&k) {
                        result = t;
                    }
                }
            }
        }
        if optional_out {
            result = self.types.union(vec![result, self.types.undefined]);
        }
        result
    }

    pub(crate) fn function_expando_base_symbol(
        &self,
        obj: &Expr,
    ) -> Option<crate::binder::SymbolId> {
        let Expr::Ident(id) = obj else {
            return None;
        };
        let Some(sym) = self.lookup_value(self.current_scope, &id.name) else {
            return None;
        };
        self.symbol(sym)
            .decls
            .iter()
            .any(|decl| match decl {
                crate::binder::Decl::Func(_) => true,
                crate::binder::Decl::Var(d, VarKind::Const) => {
                    matches!(d.init, Some(Expr::FunctionExpr(_)) | Some(Expr::Arrow(_)))
                }
                _ => false,
            })
            .then_some(sym)
    }

    fn is_function_expando_base(&self, obj: &Expr) -> bool {
        self.function_expando_base_symbol(obj).is_some()
    }

    /// Substitute the polymorphic `this` of a member's declaring owner with the
    /// receiver type. So `t.self()` (where `self(): this` is declared on
    /// `Thing1` and `t: Thing1 & Thing2`) returns the full receiver rather
    /// than `Thing1`, and a fluent `extend<T>(): this & T` accumulates across
    /// a call chain. If the member's owner has no `this`-parameter (no `this`
    /// ever appeared in its declared types), no work is done.
    fn instantiate_this_in_member(
        &mut self,
        member_ty: TypeId,
        member_sym: Option<crate::binder::SymbolId>,
        receiver: TypeId,
    ) -> TypeId {
        let Some(msym) = member_sym else {
            return member_ty;
        };
        let Some(owner) = self.symbol(msym).parent else {
            return member_ty;
        };
        let Some(&tp) = self.deferred.this_params.get(&owner) else {
            return member_ty;
        };
        let mut m: crate::checker::symbols::Mapper = Default::default();
        m.insert(tp, receiver);
        self.instantiate_type(member_ty, &m)
    }

    fn prop_access_type(
        &mut self,
        obj_t: TypeId,
        name: &'a Ident,
        this_receiver: TypeId,
    ) -> Option<TypeId> {
        if name.name.is_empty() {
            return Some(self.types.error);
        }
        // union: property must exist on every member
        if let TypeKind::Union(members) = self.types.kind(obj_t).clone() {
            let mut parts = Vec::new();
            let any_present = members
                .iter()
                .any(|&m| self.prop_info_of_type(m, &name.name).is_some());
            for m in members {
                match self.prop_of_type(m, &name.name) {
                    Some(t) => parts.push(t),
                    None => {
                        let d = self.display_prop_access_object_type(obj_t);
                        if any_present {
                            // present on some constituents: name the first
                            // missing one as a chain child
                            let md = self.display_type(m);
                            let mut chain = crate::diagnostics::MessageChain::new(
                                &gen::Property_0_does_not_exist_on_type_1,
                                &[name.name.clone(), d],
                            );
                            chain.next.push(crate::diagnostics::MessageChain::new(
                                &gen::Property_0_does_not_exist_on_type_1,
                                &[name.name.clone(), md],
                            ));
                            self.error_chain_at(name.span, chain);
                        } else {
                            self.error_at(
                                name.span,
                                &gen::Property_0_does_not_exist_on_type_1,
                                &[name.name.clone(), d],
                            );
                        }
                        return None;
                    }
                }
            }
            return Some(self.types.union(parts));
        }
        if let Some(p) = self.prop_info_of_type(obj_t, &name.name) {
            self.check_member_access_control(&p, name);
            let ty = self.instantiate_this_in_member(p.ty, p.symbol, this_receiver);
            return Some(ty);
        }
        let function_like = matches!(
            self.types.kind(self.types.regular(obj_t)),
            TypeKind::ClassStatics(_) | TypeKind::MappedClassStatics(_, _)
        ) || self.shape_of_type(obj_t).is_some_and(|sid| {
            let shape = self.types.shape(sid);
            !shape.call_sigs.is_empty() || !shape.ctor_sigs.is_empty()
        });
        if function_like {
            if let Some(fun_sym) = self.global_type_symbol("Function") {
                let fun_ty = self.types.intern_kind(TypeKind::Iface(fun_sym));
                if let Some(p) = self.prop_info_of_type(fun_ty, &name.name) {
                    self.check_member_access_control(&p, name);
                    let ty = self.instantiate_this_in_member(p.ty, p.symbol, this_receiver);
                    return Some(ty);
                }
            }
        }
        let object_member_apparent = self.apparent_type(obj_t);
        if !matches!(
            self.types.kind(object_member_apparent),
            TypeKind::EnumObject(_)
        ) && self.shape_of_type(object_member_apparent).is_some()
        {
            if let Some(obj_sym) = self.global_type_symbol("Object") {
                let obj_ty = self.types.intern_kind(TypeKind::Iface(obj_sym));
                if let Some(p) = self.prop_info_of_type(obj_ty, &name.name) {
                    self.check_member_access_control(&p, name);
                    return Some(p.ty);
                }
            }
        }
        // index signature fallback
        if let Some(sid) = {
            let ap = self.apparent_type(obj_t);
            self.shape_of_type(ap)
        } {
            let infos = self.types.shape(sid).index_infos.clone();
            for info in infos {
                if matches!(self.types.kind(info.key), TypeKind::String) {
                    return Some(info.value);
                }
            }
        }
        // static member accessed via instance → 2576
        if let TypeKind::Iface(cls) = self.types.kind(obj_t).clone() {
            if self.symbol(cls).flags & flags::CLASS != 0
                && self.symbol(cls).statics.get(&name.name).is_some()
            {
                let cn = self.symbol(cls).name.clone();
                self.error_at(
                    name.span,
                    &gen::Property_0_does_not_exist_on_type_1_Did_you_mean_to_access_the_static_member_2_instead,
                    &[name.name.clone(), cn.clone(), format!("{}.{}", cn, name.name)],
                );
                return None;
            }
        }
        // 2550: well-known later-ES member (lib suggestion)
        if let Some(libv) = self.later_es_member_lib_for_type(obj_t, &name.name) {
            let d = self.display_prop_access_object_type(obj_t);
            self.error_at(
                name.span,
                &gen::Property_0_does_not_exist_on_type_1_Do_you_need_to_change_your_target_library_Try_changing_the_lib_compiler_option_to_2_or_later,
                &[name.name.clone(), d, libv.to_string()],
            );
            return None;
        }
        // missing → 2339/2551 with suggestion from props
        let ap = self.apparent_type(obj_t);
        let display = self.display_prop_access_object_type(obj_t);
        let cands: Vec<String> = self
            .shape_of_type(ap)
            .map(|sid| {
                self.types
                    .shape(sid)
                    .props
                    .iter()
                    .map(|p| p.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        if let Some(sug) = super::spelling_suggestion(&name.name, cands.iter().map(|s| s.as_str()))
        {
            let sug = sug.to_string();
            let related = self
                .prop_info_of_type(ap, &sug)
                .and_then(|p| p.symbol)
                .and_then(|sym| self.declared_here_related(sym))
                .into_iter()
                .collect();
            self.error_at_with_related(
                name.span,
                &gen::Property_0_does_not_exist_on_type_1_Did_you_mean_2,
                &[name.name.clone(), display, sug],
                related,
            );
        } else {
            self.error_at(
                name.span,
                &gen::Property_0_does_not_exist_on_type_1,
                &[name.name.clone(), display],
            );
        }
        None
    }

    fn later_es_member_lib_for_type(&mut self, obj_t: TypeId, name: &str) -> Option<&'static str> {
        if let TypeKind::Union(members) = self.types.kind(obj_t).clone() {
            let mut found = None;
            for m in members {
                let lib = self.later_es_member_lib_for_type(m, name)?;
                if let Some(prev) = found {
                    if prev != lib {
                        return None;
                    }
                }
                found = Some(lib);
            }
            return found;
        }
        let is_named = |this: &mut Self, t: TypeId, expected: &str| {
            let apparent = this.apparent_type(t);
            match this.types.kind(apparent).clone() {
                TypeKind::Iface(sym)
                | TypeKind::Ref(sym, _)
                | TypeKind::MappedIface(sym, _)
                | TypeKind::ClassStatics(sym)
                | TypeKind::MappedClassStatics(sym, _) => this.symbol(sym).name == expected,
                _ => false,
            }
        };
        let is_array = |this: &mut Self, t: TypeId| {
            if let Some(elem) = this.array_element_type(t) {
                return !matches!(this.types.kind(elem), TypeKind::Never);
            }
            is_named(this, t, "Array") || is_named(this, t, "ReadonlyArray")
        };
        let is_string = |this: &mut Self, t: TypeId| {
            matches!(this.types.kind(t), TypeKind::String | TypeKind::StrLit(_))
                || is_named(this, t, "String")
        };
        Some(match name {
            "includes" if is_string(self, obj_t) || is_array(self, obj_t) => "es2016",
            "padStart" | "padEnd" if is_string(self, obj_t) => "es2017",
            "trimStart" | "trimEnd" if is_string(self, obj_t) => "es2019",
            "matchAll" if is_string(self, obj_t) => "es2020",
            "replaceAll" if is_string(self, obj_t) => "es2021",
            "at" if is_string(self, obj_t) || is_array(self, obj_t) => "es2022",
            "flat" | "flatMap" if is_array(self, obj_t) => "es2019",
            "findLast" | "findLastIndex" if is_array(self, obj_t) => "es2023",
            "values" | "keys" | "entries" if is_array(self, obj_t) => "es2015",
            "fromEntries" if is_named(self, obj_t, "ObjectConstructor") => "es2019",
            "allSettled" if is_named(self, obj_t, "PromiseConstructor") => "es2020",
            "escape" if is_named(self, obj_t, "RegExpConstructor") => "es2025",
            "groups" if is_named(self, obj_t, "RegExpExecArray") => "es2018",
            "dispose" | "asyncDispose" if is_named(self, obj_t, "SymbolConstructor") => "esnext",
            _ => return None,
        })
    }

    fn check_member_access_control(&mut self, p: &PropInfo, name: &'a Ident) {
        let Some(msym) = p.symbol else { return };
        let Some(declaring) = self.symbol(msym).parent else {
            return;
        };
        let decls = self.symbol(msym).decls.clone();
        let (is_private, is_protected) = decls
            .first()
            .map(|d| {
                let mods = match d {
                    crate::binder::Decl::PropertyDecl(p) => &p.modifiers,
                    crate::binder::Decl::Method(f) => &f.modifiers,
                    crate::binder::Decl::Param(p) => &p.modifiers,
                    _ => return (false, false),
                };
                (
                    has_modifier(mods, ModifierKind::Private),
                    has_modifier(mods, ModifierKind::Protected),
                )
            })
            .unwrap_or((false, false));
        if !is_private && !is_protected {
            return;
        }
        if is_private {
            if !self.stacks.class_stack.contains(&declaring) {
                let cn = self.symbol(declaring).name.clone();
                self.error_at(
                    name.span,
                    &gen::Property_0_is_private_and_only_accessible_within_class_1,
                    &[name.name.clone(), cn],
                );
            }
            return;
        }
        // protected: current class must be the declaring class or derived from it
        let mut ok = false;
        if let Some(&cur) = self.stacks.class_stack.last() {
            ok = self.class_derives_from_or_contains_instance(cur, declaring);
        }
        if !ok {
            let cn = self.symbol(declaring).name.clone();
            self.error_at(
                name.span,
                &gen::Property_0_is_protected_and_only_accessible_within_class_1_and_its_subclasses,
                &[name.name.clone(), cn],
            );
        }
    }

    fn class_derives_from_or_contains_instance(
        &mut self,
        cur: crate::binder::SymbolId,
        declaring: crate::binder::SymbolId,
    ) -> bool {
        let mut seen = Vec::new();
        self.class_derives_from_or_contains_instance_inner(cur, declaring, &mut seen)
    }

    fn class_derives_from_or_contains_instance_inner(
        &mut self,
        cur: crate::binder::SymbolId,
        declaring: crate::binder::SymbolId,
        seen: &mut Vec<crate::binder::SymbolId>,
    ) -> bool {
        if cur == declaring {
            return true;
        }
        if seen.contains(&cur) {
            return false;
        }
        seen.push(cur);

        if let Some((base, _)) = self.base_class_of(cur) {
            if self.class_derives_from_or_contains_instance_inner(base, declaring, seen) {
                return true;
            }
        }

        let decls = self.symbol(cur).decls.clone();
        for d in decls {
            if let crate::binder::Decl::Class(cd) = d {
                if let Some(h) = &cd.extends {
                    if let Some(base) = self.base_instance_type(cd, h) {
                        let mut type_seen = Vec::new();
                        if self.type_contains_class_instance(base, declaring, seen, &mut type_seen)
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn type_contains_class_instance(
        &mut self,
        t: TypeId,
        declaring: crate::binder::SymbolId,
        class_seen: &mut Vec<crate::binder::SymbolId>,
        type_seen: &mut Vec<TypeId>,
    ) -> bool {
        let rt = self.types.regular(t);
        if type_seen.contains(&rt) {
            return false;
        }
        type_seen.push(rt);

        match self.types.kind(rt).clone() {
            TypeKind::Iface(sym) | TypeKind::Ref(sym, _) | TypeKind::MappedIface(sym, _) => {
                self.symbol(sym).flags & flags::CLASS != 0
                    && self
                        .class_derives_from_or_contains_instance_inner(sym, declaring, class_seen)
            }
            TypeKind::Anon(sid) => {
                let props = self.types.shape(sid).props.clone();
                props.iter().any(|p| {
                    p.symbol
                        .and_then(|member| self.symbol(member).parent)
                        .is_some_and(|parent| {
                            self.symbol(parent).flags & flags::CLASS != 0
                                && self.class_derives_from_or_contains_instance_inner(
                                    parent, declaring, class_seen,
                                )
                        })
                })
            }
            TypeKind::Intersection(members) | TypeKind::Union(members) => members
                .iter()
                .copied()
                .any(|m| self.type_contains_class_instance(m, declaring, class_seen, type_seen)),
            TypeKind::TypeParam(tp) => self.constraint_of_type_param(tp).is_some_and(|c| {
                self.type_contains_class_instance(c, declaring, class_seen, type_seen)
            }),
            _ => false,
        }
    }

    pub(crate) fn check_elem_access(&mut self, e: &'a Expr) -> TypeId {
        let Expr::ElemAccess {
            obj,
            index,
            question_dot,
            ..
        } = e
        else {
            unreachable!()
        };
        let prev_flag = self.enums.const_enum_ident_ok;
        self.enums.const_enum_ident_ok = true;
        // receiver + index of an element-access target are reads even inside
        // an assignment pattern (tsc accessKind: PropertyAccess → Read)
        let mut obj_t = self.in_read_position(|c| c.check_expr(obj, None));
        self.enums.const_enum_ident_ok = prev_flag;
        let idx_t = self.in_read_position(|c| c.check_expr(index, None));
        if self.types.is_error(obj_t) || self.types.is_error(idx_t) {
            return self.types.error;
        }
        if *question_dot {
            obj_t = self.non_nullable(obj_t);
        } else {
            obj_t = self.check_non_nullish(obj_t, obj, false);
            if self.types.is_error(obj_t) {
                return self.types.error;
            }
        }
        if self.types.is_any_or_error(obj_t) {
            return self.types.any;
        }
        let idx_r = self.types.regular(idx_t);
        let is_plain_write_target = self.cflags.assign_target
            == crate::checker::exprs::node_key_expr(e)
            && !self.cflags.assign_target_rw
            || self.cflags.pattern_target > 0;
        if !is_plain_write_target {
            if let Some(t) = self.generic_constraint_index_read(obj_t, idx_r) {
                return t;
            }
        }
        // a generic object (type parameter, or a parameterized intersection like
        // `T & U`) indexed by a generic key resolves to an indexed access type,
        // preserving genericity (`o[k]` → `o[K]`) rather than going through the
        // constraint's apparent shape (which would reject the key). A literal or
        // numeric index still uses the shape below.
        //
        // A parameterized intersection's members are unknown, so any non-trivial
        // index defers (every `(T & U)[k]` is valid and generic).
        if matches!(self.types.kind(obj_t), TypeKind::Intersection(_))
            && self.type_contains_params(obj_t)
            && !matches!(
                self.types.kind(idx_r),
                TypeKind::NumLit(_) | TypeKind::Number
            )
        {
            return self.indexed_access_type(obj_t, idx_r, Some((index.span(), e.span())));
        }
        if matches!(self.types.kind(obj_t), TypeKind::TypeParam(_))
            && matches!(
                self.types.kind(idx_r),
                TypeKind::TypeParam(_) | TypeKind::Keyof(_)
            )
        {
            return self.indexed_access_type(obj_t, idx_r, Some((index.span(), e.span())));
        }
        if let TypeKind::EnumObject(esym) = self.types.kind(obj_t).clone() {
            let is_const = self
                .symbol(esym)
                .decls
                .iter()
                .any(|d| matches!(d, crate::binder::Decl::Enum(e) if e.is_const));
            if is_const && !matches!(self.types.kind(idx_r), TypeKind::StrLit(_)) {
                self.error_at(
                    index.span(),
                    &gen::A_const_enum_member_can_only_be_accessed_using_a_string_literal,
                    &[],
                );
                return self.types.error;
            }
            // a numeric (non-const) enum has a reverse mapping `[n: number] -> name`,
            // so indexing it by a number — or by a numeric enum member, whose
            // value is a number — yields the member name as a string.
            if !is_const {
                let enum_t = self.types.intern_kind(TypeKind::EnumType(esym));
                let (numeric, _) = self.enum_member_kinds_of(enum_t);
                let idx_numeric = matches!(
                    self.types.kind(idx_r),
                    TypeKind::NumLit(_) | TypeKind::Number
                ) || (matches!(self.types.kind(idx_r), TypeKind::EnumMember(_))
                    && self.enum_member_kinds_of(idx_r).0)
                    // compound indexes (`Directive | (T & number)` after a
                    // typeof guard) apply to the `[n: number]` reverse map
                    // whenever they are number-assignable
                    || {
                        let num = self.types.number;
                        self.is_assignable_to(idx_r, num)
                    };
                if numeric && idx_numeric {
                    return self.types.intern_kind(TypeKind::String);
                }
            }
        }
        // literal index → property lookup
        let prop_name = match self.types.kind(idx_r) {
            TypeKind::StrLit(s) => Some(s.to_str_lossy().into_owned()),
            TypeKind::NumLit(bits) => Some(crate::js_num::to_js_string(f64::from_bits(*bits))),
            _ => None,
        };
        // positional tuple element by a literal numeric index: a fixed tuple
        // (no rest element) yields the element type at that position, not the
        // union of all element types. Rest tuples and out-of-bounds indices
        // fall through to the array-union behavior below.
        if let TypeKind::NumLit(bits) = self.types.kind(idx_r) {
            let elems = match self.types.kind(obj_t) {
                TypeKind::Tuple(es) | TypeKind::ReadonlyTuple(es) => Some(es.clone()),
                _ => None,
            };
            if let Some(elems) = elems {
                let n = f64::from_bits(*bits);
                if n >= 0.0 && n.fract() == 0.0 {
                    let n = n as usize;
                    match elems.iter().position(|e| e.rest) {
                        None => {
                            // fixed tuple: the element type at that position.
                            if let Some(el) = elems.get(n) {
                                let ty = el.ty;
                                let opt = el.optional;
                                return if opt {
                                    self.types.union(vec![ty, self.types.undefined])
                                } else {
                                    ty
                                };
                            }
                        }
                        Some(ri) => {
                            if n < ri {
                                // a fixed element in the prefix before the rest.
                                let ty = elems[n].ty;
                                let opt = elems[n].optional;
                                return if opt {
                                    self.types.union(vec![ty, self.types.undefined])
                                } else {
                                    ty
                                };
                            } else if ri + 1 == elems.len() {
                                // the rest element is last: any index at or beyond
                                // it has the rest element type (already the element
                                // type, e.g. `string` for `...string[]`).
                                return elems[ri].ty;
                            }
                            // a fixed suffix follows the rest: the position is
                            // ambiguous, so fall through to the array-union below.
                        }
                    }
                }
            }
        }
        let ap = self.apparent_type(obj_t);
        let shape = self.shape_of_type(ap);
        if let Some(name) = &prop_name {
            if let Some(sid) = shape {
                if let Some(p) = self.types.shape(sid).prop(name) {
                    return p.ty;
                }
            }
        }
        // index signatures
        if let Some(sid) = shape {
            let infos = self.types.shape(sid).index_infos.clone();
            let idx_is_number = self.index_category_numeric(idx_r);
            let idx_is_string = self.index_category_stringy(idx_r);
            for info in &infos {
                if matches!(self.types.kind(info.key), TypeKind::Number) && idx_is_number {
                    if self.options.no_unchecked_indexed_access {
                        return self.types.union(vec![info.value, self.types.undefined]);
                    }
                    return info.value;
                }
            }
            for info in &infos {
                if matches!(self.types.kind(info.key), TypeKind::String)
                    && (idx_is_string || idx_is_number)
                {
                    if self.options.no_unchecked_indexed_access {
                        return self.types.union(vec![info.value, self.types.undefined]);
                    }
                    return info.value;
                }
            }
            // template-literal / branded index signatures: an index assignable to
            // the signature's key type selects it; an index matching several
            // signatures (`combo['foo-test-bar']` against both `foo-${string}`
            // and `${string}-bar`) yields the intersection of their values.
            let mut matched: Vec<TypeId> = Vec::new();
            for info in &infos {
                if !matches!(
                    self.types.kind(info.key),
                    TypeKind::String | TypeKind::Number
                ) && self.is_assignable_to(idx_r, info.key)
                {
                    matched.push(info.value);
                }
            }
            if !matched.is_empty() {
                let v = self.intersect_all(matched);
                if self.options.no_unchecked_indexed_access {
                    return self.types.union(vec![v, self.types.undefined]);
                }
                return v;
            }
            // no applicable index signature
            if !idx_is_number && !idx_is_string && !self.types.is_any_or_error(idx_t) {
                // A symbol index resolves to a symbol-keyed property (`unique
                // symbol`) or an implicit-any element (general `symbol`); tsc
                // never reports TS2538 for it. Without symbol-keyed property
                // modelling, fall back permissively to `any`.
                if matches!(self.types.kind(idx_r), TypeKind::EsSymbol) {
                    return self.types.any;
                }
                let d = self.display_type(idx_r);
                self.error_at(
                    index.span(),
                    &gen::Type_0_cannot_be_used_as_an_index_type,
                    &[d],
                );
                return self.types.error;
            }
            if self.options.no_implicit_any() {
                let it = self.display_type(idx_r);
                let ot = self.display_type(obj_t);
                let mut chain = crate::diagnostics::MessageChain::new(
                    &gen::Element_implicitly_has_an_any_type_because_expression_of_type_0_can_t_be_used_to_index_type_1,
                    &[it.clone(), ot.clone()],
                );
                chain.next.push(crate::diagnostics::MessageChain::new(
                    &gen::No_index_signature_with_a_parameter_of_type_0_was_found_on_type_1,
                    &[it, ot],
                ));
                self.error_chain_at(e.span(), chain);
                return self.types.error;
            }
        }
        self.types.any
    }

    fn generic_constraint_index_read(&mut self, obj_t: TypeId, idx_t: TypeId) -> Option<TypeId> {
        let TypeKind::TypeParam(obj_sym) = self.types.kind(obj_t).clone() else {
            return None;
        };
        let key_covers_obj = match self.types.kind(idx_t).clone() {
            TypeKind::Keyof(inner) => inner == obj_t,
            TypeKind::TypeParam(key_sym) => self
                .constraint_of_type_param(key_sym)
                .is_some_and(|c| self.index_constraint_covers(c, obj_t)),
            _ => false,
        };
        if !key_covers_obj {
            return None;
        }
        let constraint = self.constraint_of_type_param(obj_sym)?;
        let apparent = self.apparent_type(constraint);
        let sid = self.shape_of_type(apparent)?;
        let infos = self.types.shape(sid).index_infos.clone();
        let mut string_value = None;
        let mut number_value = None;
        for info in infos {
            match self.types.kind(info.key) {
                TypeKind::String => string_value = Some(info.value),
                TypeKind::Number => number_value = Some(info.value),
                _ => {}
            }
        }
        let value = string_value.or(number_value)?;
        if self.options.no_unchecked_indexed_access {
            Some(self.types.union(vec![value, self.types.undefined]))
        } else {
            Some(value)
        }
    }

    /// Index belongs to the `number` category — `number`/`numeric literal`, or
    /// an intersection carrying such a member (`number & Brand`).
    fn index_category_numeric(&self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::Number | TypeKind::NumLit(_) => true,
            TypeKind::Intersection(ms) => {
                let ms = ms.clone();
                ms.iter().any(|&m| self.index_category_numeric(m))
            }
            _ => false,
        }
    }

    /// Index belongs to the `string` category — `string`/`string literal`, or
    /// an intersection carrying such a member (`string & Brand`).
    fn index_category_stringy(&self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::String | TypeKind::StrLit(_) => true,
            TypeKind::Intersection(ms) => {
                let ms = ms.clone();
                ms.iter().any(|&m| self.index_category_stringy(m))
            }
            _ => false,
        }
    }

    // ── calls ───────────────────────────────────────────────────────────────
}
