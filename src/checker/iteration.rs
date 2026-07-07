use crate::ast::{node_key, ClassMember, Expr, FuncBody, FunctionLike, PropName, Stmt, TypeMember};
use crate::binder::Decl;
use crate::checker::symbols::Mapper;
use crate::checker::Checker;
use crate::diagnostics::gen;
use crate::text::Span;
use crate::types::{TypeId, TypeKind};

impl<'a> Checker<'a> {
    pub(crate) fn downlevel_iteration_is_enabled(&self) -> bool {
        self.options.downlevel_iteration == Some(true)
    }

    pub(crate) fn is_known_iterable_object_source(&mut self, ty: TypeId) -> bool {
        let apparent = self.apparent_type(ty);
        let sym = match self.types.kind(apparent) {
            TypeKind::Iface(sym)
            | TypeKind::Ref(sym, _)
            | TypeKind::MappedIface(sym, _)
            | TypeKind::ClassStatics(sym)
            | TypeKind::MappedClassStatics(sym, _) => *sym,
            _ => return false,
        };
        matches!(
            self.symbol(sym).name.as_str(),
            "Map" | "ReadonlyMap" | "Set" | "ReadonlySet"
        )
    }

    pub(crate) fn is_iterator_like_source(&mut self, ty: TypeId) -> bool {
        let Some(next_ty) = self.prop_of_type(ty, "next") else {
            return false;
        };
        if matches!(self.types.kind(self.types.regular(next_ty)), TypeKind::Any) {
            return true;
        }
        !self.call_signatures_of(next_ty).is_empty()
    }

    pub(crate) fn sync_generator_iteration_parts(
        &mut self,
        ty: TypeId,
    ) -> Option<(TypeId, TypeId)> {
        let regular = self.types.regular(ty);
        let TypeKind::Ref(sym, args) = self.types.kind(regular).clone() else {
            return None;
        };
        if self.symbol(sym).name != "Generator" {
            return None;
        }
        let yield_ty = args.first().copied().unwrap_or(self.types.unknown);
        let next_ty = args.get(2).copied().unwrap_or(self.types.unknown);
        Some((yield_ty, next_ty))
    }

    pub(crate) fn for_of_generator_element_type(
        &mut self,
        ty: TypeId,
        span: Span,
    ) -> Option<TypeId> {
        let (yield_ty, next_ty) = self.sync_generator_iteration_parts(ty)?;
        let sent_ty = self.types.undefined;
        if self.is_assignable_to(sent_ty, next_ty) {
            return Some(yield_ty);
        }
        let sent = self.display_type(sent_ty);
        let expected = self.display_type(next_ty);
        self.error_at(
            span,
            &gen::Cannot_iterate_value_because_the_next_method_of_its_iterator_expects_type_1_but_for_of_will_always_send_0,
            &[sent, expected],
        );
        Some(self.types.error)
    }

    pub(crate) fn symbol_iterator_member_type(&mut self, ty: TypeId) -> Option<TypeId> {
        let apparent0 = self.apparent_type(ty);
        let apparent = self.types.regular(apparent0);
        let (sym, mapper) = match self.types.kind(apparent).clone() {
            TypeKind::Iface(sym) => (sym, Mapper::new()),
            TypeKind::Ref(sym, args) => {
                let mut mapper = Mapper::new();
                for (i, param) in self.type_params_of_symbol(sym).into_iter().enumerate() {
                    if let Some(&arg) = args.get(i) {
                        mapper.insert(param, arg);
                    }
                }
                (sym, mapper)
            }
            TypeKind::MappedIface(sym, entries) => (sym, self.mapper_from_entries(&entries)),
            _ => return None,
        };
        let decls = self.symbol(sym).decls.clone();
        for decl in decls {
            match decl {
                Decl::Class(c) => {
                    for member in &c.members {
                        match member {
                            ClassMember::Property(p) if prop_name_is_symbol_iterator(&p.name) => {
                                let raw = self
                                    .bind
                                    .decl_symbol
                                    .get(&node_key(p))
                                    .copied()
                                    .map(|sym| self.type_of_symbol_lazy(sym))
                                    .unwrap_or_else(|| {
                                        let scope = self.scope_of_decl(node_key(p));
                                        p.ty.as_ref()
                                            .map(|ty| self.resolve_type(ty, scope))
                                            .unwrap_or(self.types.any)
                                    });
                                return Some(self.instantiate_type(raw, &mapper));
                            }
                            ClassMember::Method(f)
                                if f.name.as_ref().is_some_and(prop_name_is_symbol_iterator) =>
                            {
                                let raw = if method_body_is_single_return_this(f) {
                                    self.owner_instance_type(sym)
                                } else {
                                    let sig = self.signature_of(f);
                                    self.sig_return(sig)
                                };
                                return Some(self.instantiate_type(raw, &mapper));
                            }
                            _ => {}
                        }
                    }
                }
                Decl::Interface(i) => {
                    let scope = self
                        .bind
                        .node_scope
                        .get(&node_key(i))
                        .copied()
                        .unwrap_or(self.bind.global_scope);
                    for member in &i.members {
                        match member {
                            TypeMember::Prop(p) if prop_name_is_symbol_iterator(&p.name) => {
                                let raw =
                                    p.ty.as_ref()
                                        .map(|ty| self.resolve_type(ty, scope))
                                        .unwrap_or(self.types.any);
                                return Some(self.instantiate_type(raw, &mapper));
                            }
                            TypeMember::Method(m) if prop_name_is_symbol_iterator(&m.name) => {
                                let sig = self.method_signature(m, scope);
                                let raw = self.sig_return(sig);
                                return Some(self.instantiate_type(raw, &mapper));
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub(crate) fn is_downlevel_iterable_only_source(&mut self, ty: TypeId) -> bool {
        if self.types.is_any_or_error(ty) {
            return false;
        }
        let regular = self.types.regular(ty);
        if self.array_element_type(regular).is_some()
            || matches!(
                self.types.kind(regular),
                TypeKind::Tuple(_)
                    | TypeKind::ReadonlyTuple(_)
                    | TypeKind::String
                    | TypeKind::StrLit(_)
            )
        {
            return false;
        }
        match self.types.kind(regular).clone() {
            TypeKind::Union(members) => members
                .iter()
                .all(|&member| self.is_downlevel_iterable_only_source(member)),
            TypeKind::TypeParam(sym) => self
                .constraint_of_type_param(sym)
                .is_some_and(|constraint| self.is_downlevel_iterable_only_source(constraint)),
            _ => {
                self.is_known_iterable_object_source(regular)
                    || self
                        .symbol_iterator_member_type(regular)
                        .is_some_and(|iterator_ty| {
                            self.types.is_any_or_error(iterator_ty)
                                || self.is_iterator_like_source(iterator_ty)
                        })
            }
        }
    }

    pub(crate) fn report_downlevel_iteration_if_needed(&mut self, ty: TypeId, span: Span) -> bool {
        if self.downlevel_iteration_is_enabled() || !self.is_downlevel_iterable_only_source(ty) {
            return false;
        }
        let display = self.display_type(self.types.regular(ty));
        self.error_at(
            span,
            &gen::Type_0_can_only_be_iterated_through_when_using_the_downlevelIteration_flag_or_with_a_target_of_es2015_or_higher,
            &[display],
        );
        true
    }
}

fn prop_name_is_symbol_iterator(name: &PropName) -> bool {
    let PropName::Computed { expr, .. } = name else {
        return false;
    };
    matches!(
        expr.as_ref(),
        Expr::PropAccess {
            obj,
            name,
            question_dot: false,
            ..
        } if matches!(obj.as_ref(), Expr::Ident(id) if id.name == "Symbol") && name.name == "iterator"
    )
}

fn method_body_is_single_return_this(f: &FunctionLike) -> bool {
    if f.return_type.is_some() {
        return false;
    }
    let Some(FuncBody::Block(block)) = &f.body else {
        return false;
    };
    matches!(
        block.stmts.as_slice(),
        [Stmt::Return {
            expr: Some(expr), ..
        }] if matches!(expr, Expr::This { .. })
    )
}
