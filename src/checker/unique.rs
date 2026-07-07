use super::Checker;
use crate::ast::*;
use crate::diagnostics::gen;

struct DirectUnique {
    span: Span,
    operand_span: Span,
    valid_symbol: bool,
    simple_symbol: bool,
}

fn peel_parens(ty: &TypeNode) -> &TypeNode {
    match ty {
        TypeNode::Paren { inner, .. } => peel_parens(inner),
        _ => ty,
    }
}

fn direct_unique(ty: &TypeNode) -> Option<DirectUnique> {
    let TypeNode::Unique {
        ty,
        span,
        valid_symbol,
    } = peel_parens(ty)
    else {
        return None;
    };
    Some(DirectUnique {
        span: *span,
        operand_span: ty.span(),
        valid_symbol: *valid_symbol,
        simple_symbol: matches!(&**ty, TypeNode::Keyword(KeywordTypeKind::Symbol, _)),
    })
}

impl<'a> Checker<'a> {
    pub(crate) fn check_unique_symbol_var_decl(
        &mut self,
        decl: &'a VarDeclarator,
        kind: VarKind,
        in_variable_statement: bool,
    ) {
        let Some(ty) = &decl.ty else { return };
        if let Some(unique) = direct_unique(ty) {
            if !unique.valid_symbol {
                self.error_at(
                    unique.operand_span,
                    &gen::_0_expected,
                    &["symbol".to_string()],
                );
                return;
            }
            if !unique.simple_symbol {
                self.error_at(
                    unique.span,
                    &gen::unique_symbol_types_are_not_allowed_here,
                    &[],
                );
            } else if !matches!(decl.name, Binding::Ident(_)) {
                self.error_at(
                    unique.span,
                    &gen::unique_symbol_types_may_not_be_used_on_a_variable_declaration_with_a_binding_name,
                    &[],
                );
            } else if kind != VarKind::Const {
                self.error_at(
                    decl.name.span(),
                    &gen::A_variable_whose_type_is_a_unique_symbol_type_must_be_const,
                    &[],
                );
            } else if !in_variable_statement {
                self.error_at(
                    decl.name.span(),
                    &gen::unique_symbol_types_are_only_allowed_on_variables_in_a_variable_statement,
                    &[],
                );
            }
            return;
        }
        self.report_unique_symbols_in_type(ty);
    }

    pub(crate) fn check_unique_symbol_class_property(&mut self, prop: &'a PropertyDecl) {
        let Some(ty) = &prop.ty else { return };
        if let Some(unique) = direct_unique(ty) {
            if !unique.valid_symbol {
                self.error_at(
                    unique.operand_span,
                    &gen::_0_expected,
                    &["symbol".to_string()],
                );
                return;
            }
            if !unique.simple_symbol {
                self.error_at(
                    unique.span,
                    &gen::unique_symbol_types_are_not_allowed_here,
                    &[],
                );
            } else {
                let is_static = has_modifier(&prop.modifiers, ModifierKind::Static);
                let is_readonly = has_modifier(&prop.modifiers, ModifierKind::Readonly);
                if !(is_static && is_readonly) {
                    self.error_at(
                        prop.name.span(),
                        &gen::A_property_of_a_class_whose_type_is_a_unique_symbol_type_must_be_both_static_and_readonly,
                        &[],
                    );
                }
            }
            return;
        }
        self.report_unique_symbols_in_type(ty);
    }

    pub(crate) fn check_unique_symbol_type_params(&mut self, tps: &'a Option<Vec<TypeParamDecl>>) {
        if let Some(tps) = tps {
            for tp in tps {
                if let Some(ty) = &tp.constraint {
                    self.report_unique_symbols_in_type(ty);
                }
                if let Some(ty) = &tp.default {
                    self.report_unique_symbols_in_type(ty);
                }
            }
        }
    }

    pub(crate) fn check_unique_symbol_function_like(&mut self, f: &'a FunctionLike) {
        self.check_unique_symbol_type_params(&f.type_params);
        for p in &f.params {
            if let Some(ty) = &p.ty {
                self.report_unique_symbols_in_type(ty);
            }
        }
        if let Some(ty) = &f.return_type {
            self.report_unique_symbols_in_type(ty);
        }
    }

    pub(crate) fn check_unique_symbol_type_member(&mut self, member: &'a TypeMember) {
        match member {
            TypeMember::Prop(prop) => self.check_unique_symbol_prop_sig(prop),
            TypeMember::Method(method) => {
                self.check_unique_symbol_type_params(&method.type_params);
                for p in &method.params {
                    if let Some(ty) = &p.ty {
                        self.report_unique_symbols_in_type(ty);
                    }
                }
                if let Some(ty) = &method.return_type {
                    self.report_unique_symbols_in_type(ty);
                }
            }
            TypeMember::Call(sig) | TypeMember::Ctor(sig) => {
                self.check_unique_symbol_type_params(&sig.type_params);
                for p in &sig.params {
                    if let Some(ty) = &p.ty {
                        self.report_unique_symbols_in_type(ty);
                    }
                }
                if let Some(ty) = &sig.return_type {
                    self.report_unique_symbols_in_type(ty);
                }
            }
            TypeMember::Index(index) => {
                self.report_unique_symbols_in_type(&index.key_type);
                self.report_unique_symbols_in_type(&index.value_type);
            }
        }
    }

    fn check_unique_symbol_prop_sig(&mut self, prop: &'a PropSig) {
        let Some(ty) = &prop.ty else { return };
        if let Some(unique) = direct_unique(ty) {
            if !unique.valid_symbol {
                self.error_at(
                    unique.operand_span,
                    &gen::_0_expected,
                    &["symbol".to_string()],
                );
                return;
            }
            if !unique.simple_symbol {
                self.error_at(
                    unique.span,
                    &gen::unique_symbol_types_are_not_allowed_here,
                    &[],
                );
            } else if !prop.readonly {
                self.error_at(
                    prop.name.span(),
                    &gen::A_property_of_an_interface_or_type_literal_whose_type_is_a_unique_symbol_type_must_be_readonly,
                    &[],
                );
            }
            return;
        }
        self.report_unique_symbols_in_type(ty);
    }

    pub(crate) fn report_unique_symbols_in_type(&mut self, ty: &'a TypeNode) {
        match ty {
            TypeNode::Unique {
                ty,
                span,
                valid_symbol,
            } => {
                if *valid_symbol {
                    self.error_at(*span, &gen::unique_symbol_types_are_not_allowed_here, &[]);
                } else {
                    self.error_at(ty.span(), &gen::_0_expected, &["symbol".to_string()]);
                }
            }
            TypeNode::Paren { inner, .. }
            | TypeNode::Array { elem: inner, .. }
            | TypeNode::Keyof { ty: inner, .. }
            | TypeNode::ReadonlyOp { ty: inner, .. } => self.report_unique_symbols_in_type(inner),
            TypeNode::Tuple { elems, .. } => {
                for elem in elems {
                    self.report_unique_symbols_in_type(&elem.ty);
                }
            }
            TypeNode::Union { members, .. } | TypeNode::Intersection { members, .. } => {
                for member in members {
                    self.report_unique_symbols_in_type(member);
                }
            }
            TypeNode::Function(f) | TypeNode::Ctor(f) => {
                self.check_unique_symbol_type_params(&f.type_params);
                for p in &f.params {
                    if let Some(ty) = &p.ty {
                        self.report_unique_symbols_in_type(ty);
                    }
                }
                self.report_unique_symbols_in_type(&f.return_type);
            }
            TypeNode::TypeLiteral { members, .. } => {
                for member in members {
                    self.check_unique_symbol_type_member(member);
                }
            }
            TypeNode::TypeQuery { type_args, .. } | TypeNode::Ref(TypeRef { type_args, .. }) => {
                if let Some(args) = type_args {
                    for arg in args {
                        self.report_unique_symbols_in_type(arg);
                    }
                }
            }
            TypeNode::IndexedAccess { obj, index, .. } => {
                self.report_unique_symbols_in_type(obj);
                self.report_unique_symbols_in_type(index);
            }
            TypeNode::Conditional(c) => {
                self.report_unique_symbols_in_type(&c.check);
                self.report_unique_symbols_in_type(&c.extends_ty);
                self.report_unique_symbols_in_type(&c.true_ty);
                self.report_unique_symbols_in_type(&c.false_ty);
            }
            TypeNode::Predicate { ty, .. } => {
                if let Some(ty) = ty {
                    self.report_unique_symbols_in_type(ty);
                }
            }
            TypeNode::Infer { constraint, .. } => {
                if let Some(ty) = constraint {
                    self.report_unique_symbols_in_type(ty);
                }
            }
            TypeNode::Mapped(mapped) => {
                self.report_unique_symbols_in_type(&mapped.constraint);
                if let Some(ty) = &mapped.name_type {
                    self.report_unique_symbols_in_type(ty);
                }
                if let Some(ty) = &mapped.value {
                    self.report_unique_symbols_in_type(ty);
                }
            }
            TypeNode::TemplateLit { parts, .. } => {
                for (ty, _) in parts {
                    self.report_unique_symbols_in_type(ty);
                }
            }
            TypeNode::Keyword(..)
            | TypeNode::This(_)
            | TypeNode::LiteralString { .. }
            | TypeNode::LiteralNumber { .. }
            | TypeNode::LiteralBigInt { .. }
            | TypeNode::LiteralBool { .. } => {}
        }
    }
}
