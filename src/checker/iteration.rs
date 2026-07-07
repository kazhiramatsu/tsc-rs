use crate::ast::{ClassMember, Expr, PropName, TypeMember};
use crate::binder::Decl;
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

    pub(crate) fn has_symbol_iterator_member(&mut self, ty: TypeId) -> bool {
        let apparent0 = self.apparent_type(ty);
        let apparent = self.types.regular(apparent0);
        let sym = match self.types.kind(apparent) {
            TypeKind::Iface(sym) | TypeKind::Ref(sym, _) | TypeKind::MappedIface(sym, _) => *sym,
            _ => return false,
        };
        self.symbol(sym).decls.iter().any(|decl| match decl {
            Decl::Class(c) => c.members.iter().any(|m| match m {
                ClassMember::Property(p) => prop_name_is_symbol_iterator(&p.name),
                ClassMember::Method(f) => f.name.as_ref().is_some_and(prop_name_is_symbol_iterator),
                _ => false,
            }),
            Decl::Interface(i) => i.members.iter().any(|m| match m {
                TypeMember::Prop(p) => prop_name_is_symbol_iterator(&p.name),
                TypeMember::Method(m) => prop_name_is_symbol_iterator(&m.name),
                _ => false,
            }),
            _ => false,
        })
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
                self.has_symbol_iterator_member(regular) && self.is_iterator_like_source(regular)
                    || self.is_known_iterable_object_source(regular)
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
