//! Assignability-error elaboration: when a value fails to match its expected
//! type, drill into object- and array-literal members to attach nested
//! "types of property X are incompatible" diagnostics. Split out of `relations.rs`.

use crate::ast::*;
use crate::checker::relations::{is_numeric_name, RelCtx};
use crate::checker::Checker;
use crate::diagnostics::{gen, MessageChain, RelatedInfo};
use crate::types::{TypeId, TypeKind};

impl<'a> Checker<'a> {
    /// elaborateError: returns true if inner diagnostics were issued.
    pub fn elaborate_error(&mut self, expr: &'a Expr, src: TypeId, tgt: TypeId) -> bool {
        match expr {
            Expr::Paren { inner, .. } => self.elaborate_error(inner, src, tgt),
            Expr::Object { props, .. } => self.elaborate_object_literal(expr, props, src, tgt),
            Expr::Array { elements, .. } => self.elaborate_array_literal(elements, src, tgt),
            Expr::Arrow(f) => {
                // expression-body arrows with no annotated params
                let Some(crate::ast::FuncBody::Expr(body)) = &f.body else {
                    return false;
                };
                if f.params.iter().any(|p| p.ty.is_some()) {
                    return false;
                }
                let (Some(s_shape), Some(t_shape)) =
                    (self.shape_of_type(src), self.shape_of_type(tgt))
                else {
                    return false;
                };
                let (s_sigs, t_sigs) = (
                    self.types.shape(s_shape).call_sigs.clone(),
                    self.types.shape(t_shape).call_sigs.clone(),
                );
                if s_sigs.len() != 1 || t_sigs.is_empty() {
                    return false;
                }
                let s_ret = self.sig_return(s_sigs[0]);
                let rets: Vec<TypeId> = t_sigs.iter().map(|&s| self.sig_return(s)).collect();
                let t_ret = self.types.union(rets);
                if self.is_assignable_to(s_ret, t_ret) {
                    return false;
                }
                if matches!(self.types.kind(t_ret), TypeKind::Void) {
                    return false;
                }
                if self.elaborate_error(body, s_ret, t_ret) {
                    return true;
                }
                self.report_relation_failure(s_ret, t_ret, body.span(), None);
                true
            }
            Expr::Binary { op, right, .. }
                if matches!(op, crate::ast::BinOp::Assign | crate::ast::BinOp::Comma) =>
            {
                self.elaborate_error(right, src, tgt)
            }
            _ => false,
        }
    }

    /// Attach tsc's `The_expected_type_comes_from_property_0_which_is_declared_here_on_type_1`
    /// (TS6500) related-information to every diagnostic emitted since `before`,
    /// anchored at the declaration of the target property `t_sym` on `tgt`.
    /// Move any related-info queued on `ctx` onto the most recently emitted
    /// diagnostic (the one just created from `ctx.error_info`).
    pub(crate) fn attach_pending_related(&mut self, ctx: &mut RelCtx) {
        if ctx.pending_related.is_empty() {
            return;
        }
        if let Some(d) = self.diags.last_mut() {
            d.related.append(&mut ctx.pending_related);
        } else {
            ctx.pending_related.clear();
        }
    }

    fn add_expected_type_related_info(
        &mut self,
        before: usize,
        tgt: TypeId,
        t_sym: Option<crate::binder::SymbolId>,
    ) {
        if self.diags.len() == before {
            return;
        }
        let Some(sym) = t_sym else { return };
        let (decl_span, file, pname) = {
            let s = self.symbol(sym);
            let Some(decl) = s.decls.first() else { return };
            (decl.name_span(), s.file, s.name.clone())
        };
        let tname = self.display_type(tgt);
        let msg = MessageChain::new(
            &gen::The_expected_type_comes_from_property_0_which_is_declared_here_on_type_1,
            &[pname, tname],
        );
        let ri = RelatedInfo {
            file: Some(file),
            start: decl_span.start,
            length: decl_span.len(),
            message: msg,
        };
        for d in self.diags[before..].iter_mut() {
            d.related.push(ri.clone());
        }
    }

    fn elaborate_object_literal(
        &mut self,
        _obj: &'a Expr,
        props: &'a [ObjectProp],
        src: TypeId,
        tgt: TypeId,
    ) -> bool {
        let mut reported = false;
        for p in props {
            let (name, name_span, inner): (String, Span, Option<&'a Expr>) = match p {
                ObjectProp::Property { name, value, .. } => {
                    let Some(n) = name.text() else { continue };
                    (n, name.span(), Some(value))
                }
                ObjectProp::Shorthand { name, .. } => (name.name.clone(), name.span, None),
                ObjectProp::Method(f) => {
                    let Some(n) = f.name.as_ref().and_then(|n| n.text()) else {
                        continue;
                    };
                    let span = f.name.as_ref().unwrap().span();
                    (n, span, None)
                }
                ObjectProp::Spread { .. } => continue,
            };
            // The expected type for this member is a matching named property
            // of the target, or — failing that — the value type of an
            // applicable index signature (so an object literal assigned to
            // `{ [k: string]: number }` elaborates onto the offending member).
            let (t_ty, t_optional, t_sym) = if let Some(t_prop) = self.prop_info_of_type(tgt, &name)
            {
                (t_prop.ty, t_prop.optional, t_prop.symbol)
            } else if let Some(iv) = self.index_value_for_name(tgt, &name) {
                (iv, false, None)
            } else {
                continue;
            };
            let Some(s_prop_ty) = self.prop_of_type(src, &name) else {
                continue;
            };
            // exact-optional mismatches report the whole 2375 chain instead of
            // re-rooting into the property
            if self.options.exact_optional_property_types && t_optional {
                let src_has_undef = self
                    .types
                    .union_members(s_prop_ty)
                    .iter()
                    .any(|&m| matches!(self.types.kind(m), TypeKind::Undefined));
                if src_has_undef {
                    continue;
                }
            }
            if !self.is_assignable_to(s_prop_ty, t_ty) {
                reported = true;
                if let Some(inner) = inner {
                    if self.elaborate_error(inner, s_prop_ty, t_ty) {
                        continue;
                    }
                }
                // Diagnostic is produced directly at this level (the child did
                // not elaborate further); attach tsc's "The expected type comes
                // from property '…'" (TS6500) related-info pointing at the
                // target property's declaration.
                let before = self.diags.len();
                self.report_relation_failure(s_prop_ty, t_ty, name_span, None);
                self.add_expected_type_related_info(before, tgt, t_sym);
            }
        }
        reported
    }

    /// Value type of the index signature that would match a property named
    /// `name` on `tgt` (string index always applies; a numeric index applies
    /// to numeric-looking names), if any.
    fn index_value_for_name(&mut self, tgt: TypeId, name: &str) -> Option<TypeId> {
        let shape = self.shape_of_type(tgt)?;
        let shape = self.types.shape(shape);
        let numeric = is_numeric_name(name);
        let mut number_value = None;
        for i in &shape.index_infos {
            match self.types.kind(i.key) {
                TypeKind::String => return Some(i.value),
                TypeKind::Number if numeric => number_value = Some(i.value),
                _ => {}
            }
        }
        number_value
    }

    fn elaborate_array_literal(&mut self, elements: &'a [Expr], _src: TypeId, tgt: TypeId) -> bool {
        let Some(t_elem_for_array) = self.array_element_type(tgt) else {
            // tuple target: element-wise against positions
            if let TypeKind::Tuple(t_elems) = self.types.kind(tgt).clone() {
                let mut reported = false;
                for (i, el) in elements.iter().enumerate() {
                    let Some(te) = t_elems.get(i) else { break };
                    if matches!(el, Expr::Spread { .. }) {
                        continue;
                    }
                    let se_ty = self.check_expr(el, None);
                    if !self.is_assignable_to(se_ty, te.ty) {
                        reported = true;
                        if self.elaborate_error(el, se_ty, te.ty) {
                            continue;
                        }
                        self.report_relation_failure(se_ty, te.ty, el.span(), None);
                    }
                }
                return reported;
            }
            return false;
        };
        let mut reported = false;
        for el in elements.iter() {
            if matches!(el, Expr::Spread { .. }) {
                continue;
            }
            let se_ty = self.check_expr(el, None);
            if !self.is_assignable_to(se_ty, t_elem_for_array) {
                reported = true;
                if self.elaborate_error(el, se_ty, t_elem_for_array) {
                    continue;
                }
                // display the "mutable location" type (widened literal)
                let display_ty = self.types.regular(se_ty);
                let display_ty = self.types.widen_literal(display_ty);
                let report_ty = if self.is_assignable_to(display_ty, t_elem_for_array) {
                    se_ty
                } else {
                    display_ty
                };
                self.report_relation_failure(report_ty, t_elem_for_array, el.span(), None);
            }
        }
        reported
    }

    #[allow(dead_code)]
    fn element_type_at(&mut self, src: TypeId, i: usize) -> Option<TypeId> {
        match self.types.kind(src).clone() {
            TypeKind::Tuple(elems) => elems.get(i).map(|e| e.ty),
            _ => self.array_element_type(src),
        }
    }
}
