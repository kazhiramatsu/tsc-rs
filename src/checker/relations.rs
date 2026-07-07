//! The assignability relation + tsc's exact error elaboration machinery,
//! ported from checker.ts (checkTypeRelatedTo and friends, TS 6.0).

use super::Checker;
use crate::ast::{Expr, MappedModifier, Span, TypeNode};
use crate::checker::symbols::Mapper;
use crate::diagnostics::{gen, DiagnosticMessage, MessageChain, RelatedInfo};
use crate::types::{TypeId, TypeKind};

#[derive(Clone)]
struct RelationDisplay {
    src: TypeId,
    tgt: TypeId,
    source: Option<String>,
    target: Option<String>,
}

struct DeferredMappedRelationParts {
    constraint: TypeId,
    value: TypeId,
    optional_strength: u8,
    value_non_nullable: bool,
    simple_value_template: bool,
}

#[derive(Clone, Copy)]
enum KeyofRelationSide {
    Source,
    Target,
}

struct KeyofRelationView {
    effective: TypeId,
    display_override: Option<String>,
}

pub struct RelCtx {
    /// chain head built bottom-up (chainDiagnosticMessages prepends)
    pub error_info: Option<MessageChain>,
    pub incompatible_stack: Vec<(&'static DiagnosticMessage, Vec<String>)>,
    pub override_next: u32,
    pub skip_parent: u32,
    last_skipped: Option<RelationDisplay>,
    pub error_span: Span,
    /// excess-property / standalone reporting may pre-empt everything
    pub reported_standalone: bool,
    /// tsc associateRelatedInfo: related-info queued during reporting, attached
    /// to the diagnostic when it is finally created.
    pub pending_related: Vec<RelatedInfo>,
    display_overrides: Vec<RelationDisplay>,
}

impl RelCtx {
    fn new(error_span: Span) -> RelCtx {
        RelCtx {
            error_info: None,
            incompatible_stack: Vec::new(),
            override_next: 0,
            skip_parent: 0,
            last_skipped: None,
            error_span,
            reported_standalone: false,
            pending_related: Vec::new(),
            display_overrides: Vec::new(),
        }
    }

    fn push_display_override(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        source: Option<String>,
        target: Option<String>,
    ) -> bool {
        if source.is_none() && target.is_none() {
            return false;
        }
        self.display_overrides.push(RelationDisplay {
            src,
            tgt,
            source,
            target,
        });
        true
    }

    fn pop_display_override(&mut self) {
        self.display_overrides.pop();
    }

    fn display_override_for(&self, src: TypeId, tgt: TypeId) -> (Option<String>, Option<String>) {
        self.display_overrides
            .iter()
            .rev()
            .find(|o| o.src == src && o.tgt == tgt)
            .map(|o| (o.source.clone(), o.target.clone()))
            .unwrap_or((None, None))
    }
}

impl<'a> Checker<'a> {
    // ── public interface ────────────────────────────────────────────────────

    /// checkTypeAssignableToAndOptionallyElaborate
    pub fn check_assignable(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        error_span: Span,
        head: Option<(&'static DiagnosticMessage, Vec<String>)>,
        expr: Option<&'a Expr>,
    ) -> bool {
        if self.is_assignable_to(src, tgt) {
            return true;
        }
        if let Some(e) = expr {
            if self.elaborate_error(e, src, tgt) {
                return false;
            }
        }
        self.report_relation_failure(src, tgt, error_span, head);
        false
    }

    /// silent relation query (with coinductive cycle handling)
    pub fn is_assignable_to(&mut self, src: TypeId, tgt: TypeId) -> bool {
        if src == tgt {
            return true;
        }
        let comparable = self.rel.erase_generic_sigs;
        let cached = if comparable {
            self.rel.comparable_cache.get(&(src, tgt))
        } else {
            self.rel.relation_cache.get(&(src, tgt))
        };
        if let Some(&r) = cached {
            return r;
        }
        if self
            .rel
            .relation_stack
            .iter()
            .any(|&(s, t)| s == src && t == tgt)
        {
            return true; // assume true while in progress (coinduction)
        }
        if self.rel.relation_stack.len() > 100 {
            self.rel.relation_depth_overflow = true;
            return true;
        }
        self.rel.relation_stack.push((src, tgt));
        let top_level = self.rel.relation_stack.len() == 1;
        let r = self.related(src, tgt, &mut None);
        self.rel.relation_stack.pop();
        if top_level {
            if comparable {
                self.rel.comparable_cache.insert((src, tgt), r);
            } else {
                self.rel.relation_cache.insert((src, tgt), r);
            }
        }
        r
    }

    /// full reporting run (assumes the silent check already failed)
    pub fn report_relation_failure(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        error_span: Span,
        head: Option<(&'static DiagnosticMessage, Vec<String>)>,
    ) {
        let mut ctx = RelCtx::new(error_span);
        self.rel.relation_depth_overflow = false;
        let mut opt = Some(&mut ctx);
        let _ = self.related_with_head(src, tgt, &mut opt, head.clone());
        // final flush
        if !ctx.incompatible_stack.is_empty() {
            self.flush_incompatible_stack(&mut ctx);
        }
        if ctx.reported_standalone {
            return;
        }
        if self.rel.relation_depth_overflow {
            let s = self.display_type(src);
            let t = self.display_type(tgt);
            self.rel_report_error(
                &mut ctx,
                &gen::Excessive_stack_depth_comparing_types_0_and_1,
                vec![s, t],
            );
        }
        if let Some(chain) = ctx.error_info.take() {
            let span = ctx.error_span;
            self.error_chain_at(span, chain);
            self.attach_pending_related(&mut ctx);
        }
    }

    /// tsc's containingMessageChain: run the relation reporting with no head,
    /// then wrap the resulting chain in `wrapper` (e.g. 2416).
    pub fn report_relation_failure_wrapped(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        error_span: Span,
        wrapper: (&'static DiagnosticMessage, Vec<String>),
    ) {
        let mut ctx = RelCtx::new(error_span);
        self.rel.relation_depth_overflow = false;
        let mut opt = Some(&mut ctx);
        let _ = self.related_with_head(src, tgt, &mut opt, None);
        if !ctx.incompatible_stack.is_empty() {
            self.flush_incompatible_stack(&mut ctx);
        }
        if ctx.reported_standalone {
            return;
        }
        let mut outer = MessageChain::new(wrapper.0, &wrapper.1);
        if let Some(chain) = ctx.error_info.take() {
            outer.next = vec![chain];
        }
        let span = ctx.error_span;
        self.error_chain_at(span, outer);
        self.attach_pending_related(&mut ctx);
    }

    /// One failing-level reporting entry: silent-fail then report with head.
    fn related_with_head(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        ctx: &mut Option<&mut RelCtx>,
        head: Option<(&'static DiagnosticMessage, Vec<String>)>,
    ) -> bool {
        let r = self.related(src, tgt, ctx);
        if !r {
            if let Some(c) = ctx.as_deref_mut() {
                if !c.reported_standalone {
                    self.report_error_results(c, src, tgt, head);
                }
            }
        }
        r
    }

    fn related_with_display_override(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        ctx: &mut Option<&mut RelCtx>,
        source: Option<String>,
        target: Option<String>,
        head: Option<(&'static DiagnosticMessage, Vec<String>)>,
    ) -> bool {
        let pushed = match ctx.as_deref_mut() {
            Some(c) => c.push_display_override(src, tgt, source, target),
            None => false,
        };
        let related = self.related_with_head(src, tgt, ctx, head);
        if pushed {
            if let Some(c) = ctx.as_deref_mut() {
                c.pop_display_override();
            }
        }
        related
    }

    // ── error machinery (ports of reportError & co.) ───────────────────────

    fn rel_report_error(
        &mut self,
        ctx: &mut RelCtx,
        msg: &'static DiagnosticMessage,
        args: Vec<String>,
    ) {
        if !ctx.incompatible_stack.is_empty() {
            self.flush_incompatible_stack(ctx);
        }
        if msg.elided {
            return;
        }
        if self.parse_error_files.contains(&self.current_file)
            && matches!(msg.code, 2339 | 2345 | 2365 | 2769 | 7008 | 7053)
        {
            ctx.reported_standalone = true;
            return;
        }
        if msg.code == 2322
            && self.parse_error_files.contains(&self.current_file)
            && !self.parse_error_within_next_lines(self.current_file, ctx.error_span, 10)
        {
            ctx.reported_standalone = true;
            return;
        }
        if ctx.skip_parent == 0 {
            let mut chain = MessageChain::new(msg, &args);
            if let Some(old) = ctx.error_info.take() {
                chain.next = vec![old];
            }
            ctx.error_info = Some(chain);
        } else {
            ctx.skip_parent -= 1;
        }
    }

    fn rel_report_error_force(
        &mut self,
        ctx: &mut RelCtx,
        msg: &'static DiagnosticMessage,
        args: Vec<String>,
    ) {
        // like rel_report_error but ignores the elided flag (secondary roots)
        if !ctx.incompatible_stack.is_empty() {
            self.flush_incompatible_stack(ctx);
        }
        if ctx.skip_parent == 0 {
            let mut chain = MessageChain::new(msg, &args);
            if let Some(old) = ctx.error_info.take() {
                chain.next = vec![old];
            }
            ctx.error_info = Some(chain);
        } else {
            ctx.skip_parent -= 1;
        }
    }

    fn report_incompatible(
        &mut self,
        ctx: &mut RelCtx,
        msg: &'static DiagnosticMessage,
        args: Vec<String>,
    ) {
        ctx.override_next += 1;
        ctx.last_skipped = None;
        ctx.incompatible_stack.push((msg, args));
    }

    fn flush_incompatible_stack(&mut self, ctx: &mut RelCtx) {
        let stack = std::mem::take(&mut ctx.incompatible_stack);
        let info = ctx.last_skipped.take();
        if stack.len() == 1 {
            let (msg, args) = stack.into_iter().next().unwrap();
            self.rel_report_error(ctx, msg, args);
            if let Some(info) = info {
                self.report_relation_error_with_display(
                    ctx,
                    None,
                    info.src,
                    info.tgt,
                    info.source,
                    info.target,
                );
            }
            return;
        }
        let mut path = String::new();
        let mut secondary: Vec<(&'static DiagnosticMessage, Vec<String>)> = Vec::new();
        let mut stack = stack;
        while let Some((msg, args)) = stack.pop() {
            match msg.code {
                2326 => {
                    // Types_of_property_0_are_incompatible
                    if path.starts_with("new ") {
                        path = format!("({})", path);
                    }
                    let s = &args[0];
                    if path.is_empty() {
                        path = s.clone();
                    } else if is_identifier_text(s) {
                        path = format!("{}.{}", path, s);
                    } else if s.starts_with('[') && s.ends_with(']') {
                        path = format!("{}{}", path, s);
                    } else {
                        path = format!("{}[{}]", path, s);
                    }
                }
                2202 | 2203 | 2204 | 2205 => {
                    if path.is_empty() {
                        let mapped: &'static DiagnosticMessage = match msg.code {
                            2204 => &gen::Call_signature_return_types_0_and_1_are_incompatible,
                            2205 => &gen::Construct_signature_return_types_0_and_1_are_incompatible,
                            _ => msg,
                        };
                        secondary.insert(0, (mapped, args));
                    } else {
                        let prefix = if msg.code == 2203 || msg.code == 2205 {
                            "new "
                        } else {
                            ""
                        };
                        let params = if msg.code == 2204 || msg.code == 2205 {
                            ""
                        } else {
                            "..."
                        };
                        path = format!("{}{}({})", prefix, path, params);
                    }
                }
                2200 => unreachable!(),
                _ => {
                    // tuple-position messages (2626/2627) would land here
                    secondary.insert(0, (msg, args));
                }
            }
        }
        if !path.is_empty() {
            if path.ends_with(')') {
                self.rel_report_error(
                    ctx,
                    &gen::The_types_returned_by_0_are_incompatible_between_these_types,
                    vec![path],
                );
            } else {
                self.rel_report_error(
                    ctx,
                    &gen::The_types_of_0_are_incompatible_between_these_types,
                    vec![path],
                );
            }
        } else if !secondary.is_empty() {
            secondary.remove(0);
        }
        for (msg, args) in secondary {
            self.rel_report_error_force(ctx, msg, args);
        }
        if let Some(info) = info {
            self.report_relation_error_with_display(
                ctx,
                None,
                info.src,
                info.tgt,
                info.source,
                info.target,
            );
        }
    }

    /// port of reportRelationError
    fn report_relation_error(
        &mut self,
        ctx: &mut RelCtx,
        head: Option<(&'static DiagnosticMessage, Vec<String>)>,
        src: TypeId,
        tgt: TypeId,
    ) {
        self.report_relation_error_with_display(ctx, head, src, tgt, None, None);
    }

    fn report_relation_error_with_display(
        &mut self,
        ctx: &mut RelCtx,
        head: Option<(&'static DiagnosticMessage, Vec<String>)>,
        src: TypeId,
        tgt: TypeId,
        source_override: Option<String>,
        target_override: Option<String>,
    ) {
        if !ctx.incompatible_stack.is_empty() {
            self.flush_incompatible_stack(ctx);
        }
        let (ctx_source_override, ctx_target_override) = ctx.display_override_for(src, tgt);
        let source_override = source_override.or(ctx_source_override);
        let target_override = target_override.or(ctx_target_override);
        let has_display_override = source_override.is_some() || target_override.is_some();
        let source_type = source_override
            .clone()
            .unwrap_or_else(|| self.display_type(src));
        let target_type = target_override.unwrap_or_else(|| self.display_type(tgt));
        // literal generalization
        let mut generalized_source_type = source_type.clone();
        if source_override.is_none()
            && !matches!(self.types.kind(tgt), TypeKind::Never)
            && self.is_literal_type(src)
            && !self.type_could_have_top_level_singletons(tgt)
        {
            let g = self.base_type_of_literal(src);
            generalized_source_type = self.display_type(g);
        }
        self.add_type_parameter_constraint_related_info(ctx, src, &target_type);
        if let Some((msg, args)) = head {
            if msg.text.contains("{0}") {
                // placeholder heads replace the relation message (2345 style)
                let args = if args.is_empty() {
                    vec![generalized_source_type.clone(), target_type.clone()]
                } else {
                    args
                };
                self.report_type_parameter_instantiation_info(
                    ctx,
                    src,
                    tgt,
                    &generalized_source_type,
                    &target_type,
                );
                self.rel_report_error(ctx, msg, args);
                return;
            }
            // plain heads wrap it (2677 style): default message becomes child
            if ctx.error_info.is_none() {
                self.report_type_parameter_instantiation_info(
                    ctx,
                    src,
                    tgt,
                    &generalized_source_type,
                    &target_type,
                );
                self.rel_report_error(
                    ctx,
                    &gen::Type_0_is_not_assignable_to_type_1,
                    vec![generalized_source_type.clone(), target_type.clone()],
                );
            }
            self.rel_report_error(ctx, msg, args);
            return;
        }
        // string literal vs union: spelling suggestion → 2820
        if !has_display_override && matches!(self.types.kind(src), TypeKind::StrLit(_)) {
            if let TypeKind::Union(members) = self.types.kind(tgt).clone() {
                let TypeKind::StrLit(sval) = self.types.kind(src).clone() else {
                    unreachable!()
                };
                let sval = sval.to_str_lossy().into_owned();
                let cands: Vec<String> = members
                    .iter()
                    .filter_map(|&m| match self.types.kind(m) {
                        TypeKind::StrLit(v) => Some(v.to_str_lossy().into_owned()),
                        _ => None,
                    })
                    .collect();
                if let Some(sug) =
                    super::spelling_suggestion(&sval, cands.iter().map(|s| s.as_str()))
                {
                    let sug_display = format!("\"{}\"", sug);
                    self.rel_report_error(
                        ctx,
                        &gen::Type_0_is_not_assignable_to_type_1_Did_you_mean_2,
                        vec![generalized_source_type, target_type, sug_display],
                    );
                    return;
                }
            }
        }
        let msg: &'static DiagnosticMessage = if source_type == target_type {
            &gen::Type_0_is_not_assignable_to_type_1_Two_different_types_with_this_name_exist_but_they_are_unrelated
        } else if self.options.exact_optional_property_types
            && self.exact_optional_mismatch(src, tgt)
        {
            &gen::Type_0_is_not_assignable_to_type_1_with_exactOptionalPropertyTypes_Colon_true_Consider_adding_undefined_to_the_types_of_the_target_s_properties
        } else {
            &gen::Type_0_is_not_assignable_to_type_1
        };
        self.report_type_parameter_instantiation_info(
            ctx,
            src,
            tgt,
            &generalized_source_type,
            &target_type,
        );
        self.rel_report_error(ctx, msg, vec![generalized_source_type, target_type]);
    }

    fn add_type_parameter_constraint_related_info(
        &mut self,
        ctx: &mut RelCtx,
        src: TypeId,
        target_type: &str,
    ) {
        let TypeKind::TypeParam(source_sym) = self.types.kind(src).clone() else {
            return;
        };
        if self.type_param_has_declared_constraint(source_sym) {
            return;
        }
        if let Some(related) = self.related_on_symbol_decl(
            source_sym,
            &gen::This_type_parameter_might_need_an_extends_0_constraint,
            &[target_type.to_string()],
        ) {
            ctx.pending_related.push(related);
        }
    }

    fn type_param_has_declared_constraint(&self, sym: crate::binder::SymbolId) -> bool {
        matches!(
            self.symbol(sym).decls.first(),
            Some(crate::binder::Decl::TypeParam(tp)) if tp.constraint.is_some()
        )
    }

    fn report_type_parameter_instantiation_info(
        &mut self,
        ctx: &mut RelCtx,
        src: TypeId,
        tgt: TypeId,
        source_type: &str,
        target_type: &str,
    ) {
        let TypeKind::TypeParam(target_sym) = self.types.kind(tgt).clone() else {
            return;
        };
        if let Some(constraint) = self.type_parameter_relation_constraint(target_sym) {
            let saved_overflow = self.rel.relation_depth_overflow;
            let assignable_to_constraint = self.is_assignable_to(src, constraint);
            self.rel.relation_depth_overflow = saved_overflow;
            if assignable_to_constraint {
                let constraint_type = self.display_type(constraint);
                self.rel_report_error(
                    ctx,
                    &gen::_0_is_assignable_to_the_constraint_of_type_1_but_1_could_be_instantiated_with_a_different_subtype_of_constraint_2,
                    vec![source_type.to_string(), target_type.to_string(), constraint_type],
                );
                return;
            }
        }
        self.rel_report_error(
            ctx,
            &gen::_0_could_be_instantiated_with_an_arbitrary_type_which_could_be_unrelated_to_1,
            vec![target_type.to_string(), source_type.to_string()],
        );
    }

    /// port of reportErrorResults (the per-failing-level head logic)
    fn report_error_results(
        &mut self,
        ctx: &mut RelCtx,
        src: TypeId,
        tgt: TypeId,
        head: Option<(&'static DiagnosticMessage, Vec<String>)>,
    ) {
        let mut maybe_suppress = ctx.override_next > 0;
        if maybe_suppress {
            ctx.override_next -= 1;
        }
        // array-like specials (readonly → mutable, tuple arity already in chain)
        if self.is_readonly_array_like(src) && self.is_mutable_array_or_tuple(tgt) {
            let s = self.display_type(src);
            let t = self.display_type(tgt);
            self.rel_report_error(
                ctx,
                &gen::The_type_0_is_readonly_and_cannot_be_assigned_to_the_mutable_type_1,
                vec![s, t],
            );
            maybe_suppress = ctx.error_info.is_some();
        }
        if head.is_none() && maybe_suppress {
            let (source, target) = ctx.display_override_for(src, tgt);
            ctx.last_skipped = Some(RelationDisplay {
                src,
                tgt,
                source,
                target,
            });
            return;
        }
        self.report_relation_error(ctx, head, src, tgt);
    }

    /// eOPT: a target optional property whose source type includes undefined
    fn exact_optional_mismatch(&mut self, src: TypeId, tgt: TypeId) -> bool {
        let (Some(ss), Some(ts)) = (self.shape_of_type(src), self.shape_of_type(tgt)) else {
            return false;
        };
        let t_shape = self.types.shape(ts).clone();
        let s_shape = self.types.shape(ss).clone();
        t_shape.props.iter().any(|tp| {
            tp.optional
                && s_shape.prop(&tp.name).map_or(false, |sp| {
                    self.types
                        .union_members(sp.ty)
                        .iter()
                        .any(|&m| matches!(self.types.kind(m), TypeKind::Undefined))
                })
        })
    }

    fn display_without_single_undefined(&mut self, ty: TypeId) -> Option<String> {
        let TypeKind::Union(members) = self.types.kind(ty).clone() else {
            return None;
        };
        let mut saw_undefined = false;
        let mut non_undefined = None;
        for member in members {
            if matches!(self.types.kind(member), TypeKind::Undefined) {
                saw_undefined = true;
                continue;
            }
            if non_undefined.is_some() {
                return None;
            }
            non_undefined = Some(member);
        }
        if saw_undefined {
            non_undefined.map(|member| self.display_type(member))
        } else {
            None
        }
    }

    pub(crate) fn is_literal_type_pub(&self, t: TypeId) -> bool {
        self.is_literal_type(t)
    }

    fn is_literal_type(&self, t: TypeId) -> bool {
        matches!(
            self.types.kind(t),
            TypeKind::StrLit(_)
                | TypeKind::NumLit(_)
                | TypeKind::BigIntLit(_)
                | TypeKind::BoolLit(_)
                | TypeKind::EnumMember(_)
        ) || t == self.types.boolean
    }

    fn base_type_of_literal(&mut self, t: TypeId) -> TypeId {
        match self.types.kind(t) {
            TypeKind::StrLit(_) => self.types.string,
            TypeKind::NumLit(_) => self.types.number,
            TypeKind::BigIntLit(_) => self.types.bigint,
            TypeKind::BoolLit(_) => self.types.boolean,
            TypeKind::EnumMember(m) => {
                let parent = self.symbol(*m).parent;
                match parent {
                    Some(p) => self.types.intern_kind(TypeKind::EnumType(p)),
                    None => t,
                }
            }
            _ => t,
        }
    }

    fn type_could_have_top_level_singletons(&mut self, t: TypeId) -> bool {
        if t == self.types.boolean {
            return false;
        }
        match self.types.kind(t).clone() {
            TypeKind::Union(members) => members
                .iter()
                .any(|&m| self.type_could_have_top_level_singletons(m)),
            TypeKind::TypeParam(sym) => {
                if let Some(c) = self.constraint_of_type_param(sym) {
                    if c != t {
                        return self.type_could_have_top_level_singletons(c);
                    }
                }
                false
            }
            TypeKind::StrLit(_)
            | TypeKind::NumLit(_)
            | TypeKind::BigIntLit(_)
            | TypeKind::BoolLit(_) => true,
            TypeKind::EnumType(_) | TypeKind::EnumMember(_) => true,
            TypeKind::Undefined | TypeKind::Null => true,
            TypeKind::Keyof(_) => {
                let expanded = self.keyof_union(t);
                self.type_could_have_top_level_singletons(expanded)
            }
            TypeKind::TemplateLit(_) => true,
            _ => false,
        }
    }

    fn is_readonly_array_like(&self, t: TypeId) -> bool {
        matches!(
            self.types.kind(t),
            TypeKind::ReadonlyArray(_) | TypeKind::ReadonlyTuple(_)
        )
    }
    fn is_mutable_array_or_tuple(&self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::Ref(sym, _) => Some(*sym) == self.array_symbol(),
            TypeKind::Tuple(_) => true,
            _ => false,
        }
    }

    fn iterable_target_element_type(&mut self, t: TypeId) -> Option<TypeId> {
        let iterable = self.global_type_symbol("Iterable")?;
        match self.types.kind(t) {
            TypeKind::Ref(sym, args) if *sym == iterable && args.len() == 1 => Some(args[0]),
            _ => None,
        }
    }

    fn iterable_source_element_type(&mut self, t: TypeId) -> Option<TypeId> {
        if let Some(elem) = self.array_element_type(t) {
            return Some(elem);
        }
        match self.types.kind(t).clone() {
            TypeKind::Tuple(elems) | TypeKind::ReadonlyTuple(elems) => {
                let elem_types = elems.iter().map(|e| e.ty).collect();
                Some(self.types.union(elem_types))
            }
            _ => None,
        }
    }

    pub(crate) fn property_key_type(&mut self) -> TypeId {
        self.types.union(vec![
            self.types.string,
            self.types.number,
            self.types.es_symbol,
        ])
    }

    fn keyof_inner(&self, t: TypeId) -> Option<TypeId> {
        match self.types.kind(t) {
            TypeKind::Keyof(inner) => Some(*inner),
            _ => None,
        }
    }

    pub(crate) fn keyof_type_parameter_inner(&self, t: TypeId) -> bool {
        self.keyof_inner(t)
            .is_some_and(|inner| matches!(self.types.kind(inner), TypeKind::TypeParam(_)))
    }

    fn keyof_relation_view(
        &mut self,
        t: TypeId,
        side: KeyofRelationSide,
    ) -> Option<KeyofRelationView> {
        self.keyof_inner(t)?;
        match side {
            KeyofRelationSide::Source => {
                if self.keyof_type_parameter_inner(t) {
                    Some(KeyofRelationView {
                        effective: self.property_key_type(),
                        display_override: Some("string | number | symbol".to_string()),
                    })
                } else {
                    Some(KeyofRelationView {
                        effective: self.keyof_union(t),
                        display_override: None,
                    })
                }
            }
            KeyofRelationSide::Target => {
                let display_override = self
                    .should_preserve_keyof_target_display(t)
                    .then(|| self.display_type(t));
                Some(KeyofRelationView {
                    effective: self.keyof_union(t),
                    display_override,
                })
            }
        }
    }

    fn source_keyof_relation_type(&mut self, t: TypeId) -> TypeId {
        self.keyof_relation_view(t, KeyofRelationSide::Source)
            .map(|view| view.effective)
            .unwrap_or(t)
    }

    fn should_preserve_keyof_target_display(&self, t: TypeId) -> bool {
        let Some(inner) = self.keyof_inner(t) else {
            return false;
        };
        match self.types.kind(inner) {
            TypeKind::TypeParam(_) => true,
            TypeKind::Iface(sym) | TypeKind::Ref(sym, _) => {
                self.symbol(*sym).flags & crate::binder::flags::CLASS != 0
            }
            _ => false,
        }
    }

    fn type_parameter_relation_constraint(
        &mut self,
        sym: crate::binder::SymbolId,
    ) -> Option<TypeId> {
        let constraint = self.base_constraint_of_type_param(sym)?;
        if matches!(self.types.kind(constraint), TypeKind::Keyof(_)) {
            Some(self.source_keyof_relation_type(constraint))
        } else {
            Some(constraint)
        }
    }

    fn relate_source_keyof(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        ctx: &mut Option<&mut RelCtx>,
    ) -> bool {
        let Some(view) = self.keyof_relation_view(src, KeyofRelationSide::Source) else {
            return false;
        };
        let expanded = view.effective;
        if ctx.is_none() {
            return self.is_assignable_to(expanded, tgt);
        }
        if self.is_assignable_to(expanded, tgt) {
            return true;
        }

        if let TypeKind::TypeParam(target_sym) = self.types.kind(tgt).clone() {
            if !self.source_keyof_expands_against_type_parameter(src, target_sym) {
                return false;
            }
            let related = self.related_with_display_override(
                expanded,
                tgt,
                ctx,
                view.display_override,
                None,
                None,
            );
            debug_assert!(!related);
            return false;
        }

        let related = self.related_with_display_override(
            expanded,
            tgt,
            ctx,
            view.display_override,
            None,
            None,
        );
        debug_assert!(!related);
        if let Some(c) = ctx.as_deref_mut() {
            c.override_next += 1;
        }
        false
    }

    fn relate_to_target_keyof(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        ctx: &mut Option<&mut RelCtx>,
    ) -> bool {
        if let TypeKind::TypeParam(sym) = self.types.kind(src).clone() {
            if let Some(constraint) = self.constraint_of_type_param(sym) {
                if self.constraint_is_within_keyof_target(constraint, tgt) {
                    return true;
                }
            }
        }
        let Some(view) = self.keyof_relation_view(tgt, KeyofRelationSide::Target) else {
            return false;
        };
        let expanded = view.effective;
        if ctx.is_none() {
            return self.is_assignable_to(src, expanded);
        }
        if self.is_assignable_to(src, expanded) {
            return true;
        }
        let related = self.related_with_display_override(
            src,
            expanded,
            ctx,
            None,
            view.display_override,
            None,
        );
        debug_assert!(!related);
        if !related {
            if let Some(c) = ctx.as_deref_mut() {
                c.override_next += 1;
            }
        }
        false
    }

    fn source_keyof_expands_against_type_parameter(
        &mut self,
        src: TypeId,
        target_sym: crate::binder::SymbolId,
    ) -> bool {
        let Some(constraint) = self.type_parameter_relation_constraint(target_sym) else {
            return false;
        };
        let saved_overflow = self.rel.relation_depth_overflow;
        let assignable = self.is_assignable_to(src, constraint);
        self.rel.relation_depth_overflow = saved_overflow;
        assignable
    }

    fn constraint_is_within_keyof_target(&mut self, constraint: TypeId, target: TypeId) -> bool {
        if constraint == target {
            return true;
        }
        match self.types.kind(constraint).clone() {
            TypeKind::Keyof(c_inner) => match self.types.kind(target).clone() {
                TypeKind::Keyof(t_inner) => c_inner == t_inner,
                _ => false,
            },
            TypeKind::Intersection(members) => members
                .iter()
                .any(|&member| self.constraint_is_within_keyof_target(member, target)),
            TypeKind::TypeParam(sym) => self
                .constraint_of_type_param(sym)
                .is_some_and(|c| self.constraint_is_within_keyof_target(c, target)),
            _ => false,
        }
    }

    /// (private, protected) declaration modifiers of a class-member property
    /// (None-symbol shapes — object literals, mapped types — are public)
    fn prop_nonpublic(&self, p: &crate::types::PropInfo) -> (bool, bool) {
        let Some(msym) = p.symbol else {
            return (false, false);
        };
        self.symbol(msym)
            .decls
            .first()
            .map(|d| {
                let mods = match d {
                    crate::binder::Decl::PropertyDecl(p) => &p.modifiers,
                    crate::binder::Decl::Method(f) => &f.modifiers,
                    crate::binder::Decl::Param(p) => &p.modifiers,
                    _ => return (false, false),
                };
                (
                    crate::ast::has_modifier(mods, crate::ast::ModifierKind::Private),
                    crate::ast::has_modifier(mods, crate::ast::ModifierKind::Protected),
                )
            })
            .unwrap_or((false, false))
    }

    fn class_extends_class(
        &mut self,
        source: crate::binder::SymbolId,
        target: crate::binder::SymbolId,
    ) -> bool {
        let mut cur = Some(source);
        let mut seen = std::collections::HashSet::new();
        while let Some(sym) = cur {
            if sym == target {
                return true;
            }
            if !seen.insert(sym) {
                return false;
            }
            cur = self.base_class_of(sym).map(|(base, _)| base);
        }
        false
    }

    fn explain_deferred_mapped_relation(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        ctx: &mut Option<&mut RelCtx>,
    ) -> bool {
        let Some(src_key) = self.deferred_mapped_node_key(src) else {
            return false;
        };
        let Some(src_node) = self
            .deferred
            .deferred_mappeds
            .get(&src_key)
            .map(|&(node, _, _)| node)
        else {
            return false;
        };
        let common_key_sym = self.synthetic_type_param(src_key, &src_node.key.name);
        let (Some(src_parts), Some(tgt_parts)) = (
            self.deferred_mapped_relation_parts(src, common_key_sym),
            self.deferred_mapped_relation_parts(tgt, common_key_sym),
        ) else {
            return false;
        };

        // For mapped-object assignability, the target key space must be covered
        // by the source key space. If it is not, tsc reports that key relation as
        // the child reason under the mapped-type relation.
        let source_keys = self.display_type(src_parts.constraint);
        let target_keys = self.display_type(tgt_parts.constraint);
        if source_keys != target_keys {
            let related =
                self.related_with_head(tgt_parts.constraint, src_parts.constraint, ctx, None);
            if related {
                if let Some(c) = ctx.as_deref_mut() {
                    self.report_relation_error(c, None, tgt_parts.constraint, src_parts.constraint);
                }
            }
            return true;
        }

        if !self.is_assignable_to(src_parts.value, tgt_parts.value) {
            let related = self.related_with_head(src_parts.value, tgt_parts.value, ctx, None);
            debug_assert!(!related);
            return true;
        }

        false
    }

    fn deferred_mapped_related(&mut self, src: TypeId, tgt: TypeId) -> bool {
        let Some(src_key) = self.deferred_mapped_node_key(src) else {
            return false;
        };
        let Some(src_node) = self
            .deferred
            .deferred_mappeds
            .get(&src_key)
            .map(|&(node, _, _)| node)
        else {
            return false;
        };
        let common_key_sym = self.synthetic_type_param(src_key, &src_node.key.name);
        let (Some(src_parts), Some(tgt_parts)) = (
            self.deferred_mapped_relation_parts(src, common_key_sym),
            self.deferred_mapped_relation_parts(tgt, common_key_sym),
        ) else {
            return false;
        };
        if !matches!(self.types.kind(src_parts.constraint), TypeKind::Keyof(_))
            || !matches!(self.types.kind(tgt_parts.constraint), TypeKind::Keyof(_))
            || !src_parts.simple_value_template
            || !tgt_parts.simple_value_template
        {
            return false;
        }
        let keys_covered = self.display_type(src_parts.constraint)
            == self.display_type(tgt_parts.constraint)
            || self.is_assignable_to(tgt_parts.constraint, src_parts.constraint);
        keys_covered
            && src_parts.optional_strength >= tgt_parts.optional_strength
            && self.mapped_value_related(
                src_parts.value,
                tgt_parts.value,
                src_parts.value_non_nullable,
                tgt_parts.value_non_nullable,
            )
    }

    fn mapped_value_related(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        src_non_nullable: bool,
        tgt_non_nullable: bool,
    ) -> bool {
        if tgt_non_nullable && !src_non_nullable {
            return false;
        }
        if src == tgt {
            return true;
        }
        if self.is_non_nullable_intersection(tgt) && !self.is_non_nullable_intersection(src) {
            return false;
        }
        self.is_assignable_to(src, tgt)
    }

    fn is_non_nullable_intersection(&self, t: TypeId) -> bool {
        let TypeKind::Intersection(members) = self.types.kind(t) else {
            return false;
        };
        members.iter().any(|&m| self.is_empty_object_type(m))
    }

    fn deferred_mapped_node_key(&self, t: TypeId) -> Option<usize> {
        match self.types.kind(t) {
            TypeKind::DeferredMapped(key, _) => Some(*key),
            _ => None,
        }
    }

    fn deferred_mapped_relation_parts(
        &mut self,
        t: TypeId,
        value_key_sym: crate::binder::SymbolId,
    ) -> Option<DeferredMappedRelationParts> {
        let TypeKind::DeferredMapped(key, captured) = self.types.kind(t).clone() else {
            return None;
        };
        let &(node, scope, file) = self.deferred.deferred_mappeds.get(&key)?;
        let mapper: Mapper = captured.iter().copied().collect();

        let prev_file = self.current_file;
        let before_diags = self.diags.len();
        self.current_file = file;

        let constraint_raw = self.resolve_type(&node.constraint, scope);
        let constraint = self.instantiate_type(constraint_raw, &mapper);

        let value_raw = match &node.value {
            Some(value_node) => {
                self.tp
                    .infer_mapped_env
                    .push((node.key.name.clone(), value_key_sym));
                let resolved = self.resolve_type(value_node, scope);
                self.tp.infer_mapped_env.pop();
                resolved
            }
            None => self.types.any,
        };
        let value = self.instantiate_type(value_raw, &mapper);

        self.current_file = prev_file;
        self.diags.truncate(before_diags);

        let optional_strength = match node.optional_mod {
            Some(MappedModifier::Add) => 0,
            None => 1,
            Some(MappedModifier::Remove) => 2,
        };
        let value_non_nullable = node
            .value
            .as_ref()
            .is_some_and(|v| Self::is_non_nullable_type_node(v));
        let simple_value_template = node
            .value
            .as_ref()
            .is_some_and(|v| Self::is_simple_mapped_value_node(v));

        Some(DeferredMappedRelationParts {
            constraint,
            value,
            optional_strength,
            value_non_nullable,
            simple_value_template,
        })
    }

    fn is_non_nullable_type_node(node: &TypeNode) -> bool {
        matches!(node, TypeNode::Ref(r)
            if r.name.parts.len() == 1 && r.name.parts[0].name == "NonNullable")
    }

    fn is_simple_mapped_value_node(node: &TypeNode) -> bool {
        match node {
            TypeNode::IndexedAccess { .. } => true,
            TypeNode::Ref(r)
                if r.name.parts.len() == 1 && r.name.parts[0].name == "NonNullable" =>
            {
                r.type_args
                    .as_ref()
                    .and_then(|args| args.first())
                    .is_some_and(Self::is_simple_mapped_value_node)
            }
            _ => false,
        }
    }

    fn explain_indexed_access_relation(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        ctx: &mut Option<&mut RelCtx>,
    ) -> bool {
        let (TypeKind::IndexedAccess(src_obj, src_idx), TypeKind::IndexedAccess(tgt_obj, tgt_idx)) =
            (self.types.kind(src).clone(), self.types.kind(tgt).clone())
        else {
            return false;
        };

        let same_index = src_idx == tgt_idx
            || (self.is_assignable_to(src_idx, tgt_idx) && self.is_assignable_to(tgt_idx, src_idx))
            || self.display_type(src_idx) == self.display_type(tgt_idx);
        if same_index && !self.is_assignable_to(src_obj, tgt_obj) {
            let related = self.related_with_head(src_obj, tgt_obj, ctx, None);
            debug_assert!(!related);
            return true;
        }

        false
    }

    fn indexed_access_related(&mut self, src: TypeId, tgt: TypeId) -> bool {
        let (TypeKind::IndexedAccess(src_obj, src_idx), TypeKind::IndexedAccess(tgt_obj, tgt_idx)) =
            (self.types.kind(src).clone(), self.types.kind(tgt).clone())
        else {
            return false;
        };
        let same_index = src_idx == tgt_idx
            || (self.is_assignable_to(src_idx, tgt_idx) && self.is_assignable_to(tgt_idx, src_idx))
            || self.display_type(src_idx) == self.display_type(tgt_idx);
        same_index && self.is_assignable_to(src_obj, tgt_obj)
    }

    // ── the structural relation itself ──────────────────────────────────────

    /// A stable key under which structurally-growing instantiations are
    /// recognised (tsc's getRecursionIdentity). Generic references, interfaces
    /// and type parameters are keyed by their symbol; an indexed access by the
    /// identity of its innermost object type; deferred conditional/mapped types
    /// by their defining node; everything else by the type itself. The high bits
    /// tag the key space so the three id domains never collide.
    fn recursion_identity(&self, t: TypeId) -> u64 {
        match self.types.kind(t) {
            TypeKind::Ref(sym, _) | TypeKind::Iface(sym) | TypeKind::TypeParam(sym) => {
                (1u64 << 40) | sym.0 as u64
            }
            TypeKind::IndexedAccess(obj, _) => {
                let mut o = *obj;
                while let TypeKind::IndexedAccess(inner, _) = self.types.kind(o) {
                    o = *inner;
                }
                self.recursion_identity(o)
            }
            TypeKind::DeferredCond(node, _) | TypeKind::DeferredMapped(node, _) => {
                (2u64 << 40) | *node as u64
            }
            _ => (3u64 << 40) | t.0 as u64,
        }
    }

    fn related(&mut self, src: TypeId, tgt: TypeId, ctx: &mut Option<&mut RelCtx>) -> bool {
        if src == tgt {
            return true;
        }
        // comparable mode: tsc tries the simple rules REVERSED at every
        // recursion level (`relation === comparableRelation &&
        // isSimpleTypeRelatedTo(target, source)`), which is what makes a
        // BASE primitive comparable to one of its literals — `s === "x"`
        // with s: string. Forward literal→base already relates normally.
        if self.rel.erase_generic_sigs {
            let s = self.types.regular(src);
            let t = self.types.regular(tgt);
            let reversed_simple = matches!(
                (self.types.kind(s), self.types.kind(t)),
                (TypeKind::String, TypeKind::StrLit(_))
                    | (TypeKind::String, TypeKind::TemplateLit(_))
                    | (TypeKind::Number, TypeKind::NumLit(_))
                    | (TypeKind::Bigint, TypeKind::BigIntLit(_))
                    // reversed t&Unknown: `unknown` overlaps everything in
                    // the comparable relation (T extends unknown === 42)
                    | (TypeKind::Unknown, _)
            ) || (s == self.types.boolean
                && matches!(self.types.kind(t), TypeKind::BoolLit(_)))
                // `object` (NonPrimitive) overlaps the empty object type
                // (tsc: assignable/comparable admit any object-flagged
                // source into an empty target — `{} as T` with
                // `T extends object`)
                || (matches!(self.types.kind(s), TypeKind::NonPrimitive)
                    && self.is_empty_object_type(t));
            if reversed_simple {
                return true;
            }
        }
        // Bound infinitely-expanding comparisons the way tsc's isDeeplyNestedType
        // does. is_assignable_to applies a relation-stack guard, but the internal
        // `related`/`related_with_head` recursion (type-argument variance,
        // conditional and alias expansion) bypasses it. Rather than a raw depth
        // cap — which a finite-but-deep comparison would trip — track the
        // recursion identity of each side and, once a structurally-growing
        // instantiation recurs on BOTH sides past a small bound, assume the types
        // relate and stop. This catches mutually-recursive generics whose nesting
        // deepens each step (`interface B<T> extends A<T> { foo: B<B<T>> }`) and
        // conditional-driven expansions (`Vector<Exclude<T, U>>`).
        thread_local!(static REL_STACK: std::cell::RefCell<Vec<(u64, u64)>> =
            std::cell::RefCell::new(Vec::new()));
        const MAX_NEST: usize = 3;
        let src_id = self.recursion_identity(src);
        let tgt_id = self.recursion_identity(tgt);
        let deeply_nested = REL_STACK.with(|st| {
            let st = st.borrow();
            if st.len() < MAX_NEST {
                return false;
            }
            st.iter().filter(|(s, _)| *s == src_id).count() >= MAX_NEST
                && st.iter().filter(|(_, t)| *t == tgt_id).count() >= MAX_NEST
        });
        if deeply_nested {
            self.rel.relation_depth_overflow = true;
            return true;
        }
        REL_STACK.with(|st| st.borrow_mut().push((src_id, tgt_id)));
        struct Pop;
        impl Drop for Pop {
            fn drop(&mut self) {
                REL_STACK.with(|st| {
                    st.borrow_mut().pop();
                });
            }
        }
        let _pop = Pop;
        let sk = self.types.kind(src).clone();
        let tk = self.types.kind(tgt).clone();

        // any/error wildcard both directions
        if matches!(sk, TypeKind::Any | TypeKind::Error)
            || matches!(tk, TypeKind::Any | TypeKind::Error)
        {
            return true;
        }
        if matches!(tk, TypeKind::Unknown) {
            return true;
        }
        if matches!(sk, TypeKind::Never) {
            return true;
        }

        // fresh → regular for comparison purposes (excess check below uses fresh)
        let src_regular = self.types.regular(src);

        // excess property check for fresh object literals
        if self.types.is_fresh(src) && matches!(self.types.kind(src), TypeKind::Anon(_)) {
            if self.is_excess_property_check_target(tgt) {
                if self.has_excess_properties(src, tgt, ctx) {
                    return false;
                }
            }
        }
        let src = src_regular;
        if src == tgt {
            return true;
        }
        let sk = self.types.kind(src).clone();

        if !self.options.strict_null_checks() && matches!(sk, TypeKind::Undefined | TypeKind::Null)
        {
            return true;
        }

        match (&sk, &tk) {
            // literals → base primitives
            (TypeKind::StrLit(_), TypeKind::String) => return true,
            (TypeKind::NumLit(_), TypeKind::Number) => return true,
            (TypeKind::BigIntLit(_), TypeKind::Bigint) => return true,
            (TypeKind::StrLit(a), TypeKind::StrLit(b)) => return a == b,
            (TypeKind::NumLit(a), TypeKind::NumLit(b)) => return a == b,
            (TypeKind::BoolLit(a), TypeKind::BoolLit(b)) => return a == b,
            (TypeKind::Undefined, TypeKind::Undefined | TypeKind::Void) => return true,
            (TypeKind::Null, TypeKind::Null) => return true,
            (TypeKind::Void, TypeKind::Void) => return true,
            // undefined assignable to void
            _ => {}
        }

        // source union: every member must be related
        // (the boolean intrinsic union reports as a unit, like tsc)
        if let TypeKind::Union(members) = &sk {
            // comparable relation: SOME member suffices (tsc isRelatedTo
            // `relation === comparableRelation ? someTypeRelatedToType : ...`
            // — `C | undefined` is comparable to `Base | undefined` through
            // the shared `undefined`)
            if self.rel.erase_generic_sigs {
                let members = members.clone();
                return members.iter().any(|&m| self.is_assignable_to(m, tgt));
            }
            if src == self.types.boolean {
                // `boolean` (= true | false) is assignable to a target iff both
                // `true` and `false` are. We check via decomposition but never
                // recurse into per-member reporting, so the diagnostic keeps
                // "boolean" as a unit (e.g. boolean→number reports `boolean`,
                // not `true`). This still lets boolean match a union that
                // contains boolean (number | boolean, boolean | string, …).
                let tt = self.types.true_t;
                let ff = self.types.false_t;
                return self.is_assignable_to(tt, tgt) && self.is_assignable_to(ff, tgt);
            }
            let members = members.clone();
            if ctx.is_none() {
                return members.iter().all(|&m| self.is_assignable_to(m, tgt));
            }
            // reporting: find first failing member, recurse with reporting
            for &m in &members {
                if !self.is_assignable_to(m, tgt) {
                    let r = self.related_with_head(m, tgt, ctx, None);
                    debug_assert!(!r);
                    return false;
                }
            }
            return true;
        }

        // source intersection: a subtype of each operand, with the combined
        // apparent members. If a single operand already satisfies the target
        // we are done (`A & B <: A`, and `T & U <: 1 | 2 | 3` via `T`'s
        // constraint); otherwise the merged shape is compared structurally
        // below via `shape_of_type`. Checked before the target-union case so an
        // intersection source is not lost to per-member union decomposition.
        if let TypeKind::Intersection(members) = &sk {
            let members = members.clone();
            if !self.is_object_like(tgt) && members.iter().any(|&m| self.is_assignable_to(m, tgt)) {
                return true;
            }
            // fall through to structural comparison (combined shape vs target)
        }

        // keyof source: compare through its relation-effective key union. This
        // deliberately runs before target union/intersection/object handling so
        // diagnostics report the expanded key domain, while type-parameter
        // targets can still keep the original `keyof T` head when tsc does.
        if matches!(sk, TypeKind::Keyof(_)) {
            return self.relate_source_keyof(src, tgt, ctx);
        }

        // target union: some member must accept source (covers `boolean`,
        // i.e. `false | true`, so e.g. a `true` literal matches a member).
        if let TypeKind::Union(members) = &tk {
            let members = members.clone();
            if members.iter().any(|&m| self.is_assignable_to(src, m)) {
                return true;
            }
            // A type-parameter source relates to the union as a whole through
            // its constraint (`T extends 1 | 2` ⟹ `T <: 1 | 2`); per-member
            // decomposition alone would miss this, since `1 | 2` is assignable
            // to neither `1` nor `2`.
            if let TypeKind::TypeParam(s_sym) = &sk {
                if let Some(c) = self.constraint_of_type_param(*s_sym) {
                    return self.is_assignable_to(c, tgt);
                }
            }
            return false;
        }

        // target intersection: the source must satisfy every operand.
        if let TypeKind::Intersection(members) = &tk {
            let members = members.clone();
            let all_members_object_like = members.iter().all(|&m| self.is_object_like(m));
            if all_members_object_like
                && self.is_object_like(src)
                && self.is_object_like(tgt)
                && self.structured_related(src, tgt, &mut None)
            {
                return true;
            }
            if ctx.is_none() {
                return members.iter().all(|&m| self.is_assignable_to(src, m));
            }
            for &m in &members {
                if !self.is_assignable_to(src, m) {
                    let r = self.related_with_head(src, m, ctx, None);
                    debug_assert!(!r);
                    return false;
                }
            }
            return true;
        }

        // non-primitive `object`
        if matches!(tk, TypeKind::NonPrimitive) {
            return self.is_object_like(src);
        }

        // keyof target: expand to its literal union for relation purposes.
        if matches!(tk, TypeKind::Keyof(_)) {
            return self.relate_to_target_keyof(src, tgt, ctx);
        }
        if matches!(sk, TypeKind::IndexedAccess(..)) && matches!(tk, TypeKind::IndexedAccess(..)) {
            if self.indexed_access_related(src, tgt) {
                return true;
            }
        }
        // an indexed access on a type parameter relates through its base
        // constraint (`T['length']` → `number`), matching tsc; deferred
        // conditionals and mapped types remain opaque below.
        if matches!(sk, TypeKind::IndexedAccess(..)) {
            if let Some(sc) = self.indexed_access_base_constraint(src) {
                return self.related(sc, tgt, ctx);
            }
        }
        if matches!(tk, TypeKind::IndexedAccess(..)) {
            if let Some(tc) = self.indexed_access_base_constraint(tgt) {
                return self.related(src, tc, ctx);
            }
        }
        if matches!(sk, TypeKind::DeferredMapped(..)) && matches!(tk, TypeKind::DeferredMapped(..))
        {
            if self.deferred_mapped_related(src, tgt) {
                return true;
            }
        }
        if ctx.is_some()
            && matches!(sk, TypeKind::DeferredMapped(..))
            && matches!(tk, TypeKind::DeferredMapped(..))
        {
            if self.explain_deferred_mapped_relation(src, tgt, ctx) {
                return false;
            }
        }
        if ctx.is_some()
            && matches!(sk, TypeKind::IndexedAccess(..))
            && matches!(tk, TypeKind::IndexedAccess(..))
        {
            if self.explain_indexed_access_relation(src, tgt, ctx) {
                return false;
            }
        }
        if matches!(sk, TypeKind::DeferredMapped(..)) {
            if let Some(view) = self.deferred_homomorphic_array_view_type(src) {
                return self.related(view, tgt, ctx);
            }
        }
        if matches!(tk, TypeKind::DeferredMapped(..)) {
            if let Some(view) = self.deferred_homomorphic_array_view_type(tgt) {
                return self.related(src, view, ctx);
            }
        }
        // unresolved indexed access / deferred generics behave opaquely
        if matches!(
            sk,
            TypeKind::IndexedAccess(..) | TypeKind::DeferredCond(..) | TypeKind::DeferredMapped(..)
        ) || matches!(
            tk,
            TypeKind::IndexedAccess(..) | TypeKind::DeferredCond(..) | TypeKind::DeferredMapped(..)
        ) {
            return false;
        }
        // template literal patterns
        if let TypeKind::TemplateLit(parts) = &tk {
            let parts = parts.clone();
            if let TypeKind::StrLit(s) = &sk {
                let s = s.to_str_lossy().into_owned();
                return self.template_matches_typed(&parts, &s);
            }
            if let TypeKind::TemplateLit(src_parts) = &sk {
                let src_parts = src_parts.clone();
                return self.template_pattern_related(&src_parts, &parts);
            }
            return false;
        }
        if matches!(sk, TypeKind::TemplateLit(_)) {
            // a template pattern is a subtype of string
            return matches!(tk, TypeKind::String);
        }

        // enums (nominal)
        match (&sk, &tk) {
            (TypeKind::EnumMember(m), TypeKind::EnumType(e)) => {
                return self.symbol(*m).parent == Some(*e);
            }
            (TypeKind::EnumMember(a), TypeKind::EnumMember(b)) => return a == b,
            (TypeKind::EnumType(a), TypeKind::EnumType(b)) => {
                if a == b {
                    return true;
                }
                // empty enums (`const enum Tag {}`) have no members, so they are
                // structurally `{}` and mutually assignable — `string & Tag1`
                // and `string & Tag2` interconvert.
                let a_empty = self.symbol(*a).members.0.is_empty();
                let b_empty = self.symbol(*b).members.0.is_empty();
                return a_empty && b_empty;
            }
            (TypeKind::EnumType(_) | TypeKind::EnumMember(_), TypeKind::Number) => {
                let (n, s) = self.enum_member_kinds_of(src);
                return n && !s;
            }
            (TypeKind::EnumType(_) | TypeKind::EnumMember(_), TypeKind::String) => {
                let (n, s) = self.enum_member_kinds_of(src);
                return s && !n;
            }
            (TypeKind::NumLit(bits), TypeKind::EnumType(e)) => {
                // numeric literal assignable iff it equals some member's value
                let v = f64::from_bits(*bits);
                self.ensure_enum_checked(*e);
                let members: Vec<crate::binder::SymbolId> =
                    self.symbol(*e).members.0.iter().map(|(_, m)| *m).collect();
                return members.iter().any(|m| {
                    matches!(self.enums.enum_member_values.get(m), Some(super::EnumValue::Number(mv)) if *mv == v)
                });
            }
            (TypeKind::NumLit(bits), TypeKind::EnumMember(m)) => {
                let v = f64::from_bits(*bits);
                return matches!(self.enums.enum_member_values.get(m), Some(super::EnumValue::Number(mv)) if *mv == v);
            }
            // tsc isSimpleTypeRelatedTo (assignable/comparable): plain
            // `number` is assignable to numeric enum TYPES (`e = n` is
            // legal; numeric literals still need a matching member value —
            // the arms above). Member targets deliberately excluded: tsc
            // allows those too, but tsrs's covariant-inference candidate
            // ranking keys on this relation and mis-picks enum members over
            // `number` (genericCallWithGenericSignatureArguments3).
            (TypeKind::Number, TypeKind::EnumType(_)) => {
                let (n, _) = self.enum_member_kinds_of(tgt);
                return n;
            }
            (_, TypeKind::EnumType(_) | TypeKind::EnumMember(_)) => return false,
            (TypeKind::EnumType(_) | TypeKind::EnumMember(_), _) => {}
            _ => {}
        }

        // type parameters
        if let (TypeKind::TypeParam(s_sym), TypeKind::TypeParam(t_sym)) = (&sk, &tk) {
            if let (Some(s_owner), Some(t_owner)) =
                (self.this_param_owner(*s_sym), self.this_param_owner(*t_sym))
            {
                return s_owner == t_owner || self.class_extends_class(s_owner, t_owner);
            }
        }
        if let TypeKind::TypeParam(s_sym) = &sk {
            // T assignable to its constraint's supertypes
            if let Some(c) = self.constraint_of_type_param(*s_sym) {
                if self
                    .rel
                    .relation_stack
                    .iter()
                    .any(|&(s, t)| s == c && t == tgt)
                {
                    return true;
                }
                self.rel.relation_stack.push((c, tgt));
                let r = self.related(c, tgt, ctx);
                self.rel.relation_stack.pop();
                return r;
            }
            return false;
        }
        if matches!(tk, TypeKind::TypeParam(_)) {
            return false;
        }

        // The global `Object` interface is the boxed/non-nullish top type:
        // unlike lowercase `object`, primitives and `{}` satisfy it, but
        // `unknown`, `null`, and `undefined` do not under strict null checks.
        if self.is_global_object_type(tgt) {
            return !matches!(
                sk,
                TypeKind::Unknown | TypeKind::Undefined | TypeKind::Null | TypeKind::Void
            );
        }
        if self.is_global_object_type(src) && self.type_requires_call_or_construct(tgt) {
            return false;
        }

        // tuples (readonly source → mutable target is rejected before this)
        let s_tuple = match &sk {
            TypeKind::Tuple(e) => Some((e.clone(), false)),
            TypeKind::ReadonlyTuple(e) => Some((e.clone(), true)),
            _ => None,
        };
        let t_tuple = match &tk {
            TypeKind::Tuple(e) => Some((e.clone(), false)),
            TypeKind::ReadonlyTuple(e) => Some((e.clone(), true)),
            _ => None,
        };
        if let (Some((s_elems, s_ro)), Some((t_elems, t_ro))) = (&s_tuple, &t_tuple) {
            if *s_ro && !*t_ro {
                return false; // 4104 head in report path
            }
            return self.tuple_related(src, tgt, s_elems.clone(), t_elems.clone(), ctx);
        }
        // tuple → array (readonly tuple only to readonly arrays)
        if let Some((s_elems, s_ro)) = &s_tuple {
            let t_elem = match &tk {
                TypeKind::Ref(t_sym, t_args)
                    if Some(*t_sym) == self.array_symbol() && t_args.len() == 1 =>
                {
                    if *s_ro {
                        return false; // readonly → mutable array: 4104
                    }
                    Some(t_args[0])
                }
                TypeKind::ReadonlyArray(e) => Some(*e),
                _ => None,
            };
            if let Some(elem) = t_elem {
                return s_elems.iter().all(|e| self.is_assignable_to(e.ty, elem));
            }
        }
        // array/readonly-array relations
        if let Some(s_elem) = self.array_element_type(src) {
            if let Some(t_elem) = self.array_element_type(tgt) {
                let src_ro = self.is_readonly_array_like(src);
                let tgt_ro = self.is_readonly_array_like(tgt);
                if src_ro && !tgt_ro {
                    return false; // 4104 head in report path
                }
                // element-wise (covariant)
                return self.related_with_head(s_elem, t_elem, ctx, None);
            }
        }
        // array/tuple -> Iterable<T>. Computed `[Symbol.iterator]` members are
        // not represented as ordinary shape properties, so relying only on
        // structural props makes `Iterable<T>` effectively empty. Preserve the
        // standard iterable element relation here, matching the lib contract.
        if let Some(t_elem) = self.iterable_target_element_type(tgt) {
            if let Some(s_elem) = self.iterable_source_element_type(src) {
                return self.related_with_head(s_elem, t_elem, ctx, None);
            }
        }

        // weak-type check (target all-optional, no common properties) applies
        // to any source with an apparent shape, primitives included
        if self.is_weak_type(tgt) {
            let s_app = self.apparent_type(src);
            if let Some(sh) = self.shape_of_type(s_app) {
                let s_props = self.types.shape(sh).props.clone();
                if !s_props.is_empty() {
                    let t_shape_id = self.shape_of_type(tgt).unwrap();
                    let t_shape = self.types.shape(t_shape_id).clone();
                    let common = s_props.iter().any(|sp| t_shape.prop(&sp.name).is_some());
                    if !common {
                        if let Some(c) = ctx.as_deref_mut() {
                            let s = self.display_type(src);
                            let t = self.display_type(tgt);
                            self.rel_report_error(
                                c,
                                &gen::Type_0_has_no_properties_in_common_with_type_1,
                                vec![s, t],
                            );
                            c.reported_standalone = true;
                            if let Some(chain) = c.error_info.take() {
                                let span = c.error_span;
                                self.error_chain_at(span, chain);
                            }
                        }
                        return false;
                    }
                }
            }
        }

        // structured: needs shapes on both sides
        if self.is_object_like(src) && self.is_object_like(tgt) {
            return self.structured_related(src, tgt, ctx);
        }
        // a primitive source presents its apparent object type (String/Number/
        // Boolean) for structural comparison against an object type, so e.g.
        // `string` satisfies `{ length: number }`. The check is silent: a
        // failure falls through so the caller reports TS2322 for the primitive
        // rather than a property-level TS2741.
        if self.is_object_like(tgt) {
            let s_app = self.apparent_type(src);
            if s_app != src
                && self.is_object_like(s_app)
                && self.structured_related(s_app, tgt, &mut None)
            {
                return true;
            }
        }

        false
    }

    fn is_weak_type(&mut self, t: TypeId) -> bool {
        if !self.is_object_like(t) {
            return false;
        }
        let Some(sid) = self.shape_of_type(t) else {
            return false;
        };
        let s = self.types.shape(sid);
        !s.props.is_empty()
            && s.props.iter().all(|p| p.optional)
            && s.call_sigs.is_empty()
            && s.ctor_sigs.is_empty()
            && s.index_infos.is_empty()
    }

    pub(crate) fn is_object_like(&mut self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::Anon(_)
            | TypeKind::DeferredObj(_)
            | TypeKind::Iface(_)
            | TypeKind::Ref(..)
            | TypeKind::MappedIface(_, _)
            | TypeKind::ClassStatics(_)
            | TypeKind::MappedClassStatics(_, _)
            | TypeKind::Tuple(_)
            | TypeKind::ReadonlyArray(_)
            | TypeKind::NamespaceObj(_)
            | TypeKind::EnumObject(_) => true,
            TypeKind::Intersection(_) => self.shape_of_type(t).is_some(),
            _ => false,
        }
    }

    pub(crate) fn is_global_object_type(&mut self, t: TypeId) -> bool {
        let global = self.global_type_symbol("Object");
        let is_object = match self.types.kind(t) {
            TypeKind::Iface(sym) | TypeKind::Ref(sym, _) | TypeKind::MappedIface(sym, _) => {
                Some(*sym) == global || self.symbol(*sym).name == "Object"
            }
            _ => false,
        };
        if !is_object {
            return false;
        }
        self.shape_of_type(t)
            .map(|sid| self.types.shape(sid).index_infos.is_empty())
            .unwrap_or(true)
    }

    fn type_requires_call_or_construct(&mut self, t: TypeId) -> bool {
        let Some(sid) = self.shape_of_type(t) else {
            return false;
        };
        let shape = self.types.shape(sid);
        !shape.call_sigs.is_empty() || !shape.ctor_sigs.is_empty()
    }

    fn tuple_related(
        &mut self,
        _src: TypeId,
        _tgt: TypeId,
        s_elems: Vec<crate::types::TupleElem>,
        t_elems: Vec<crate::types::TupleElem>,
        ctx: &mut Option<&mut RelCtx>,
    ) -> bool {
        let s_arity = s_elems.len();
        let t_min = t_elems.iter().filter(|e| !e.optional && !e.rest).count();
        let t_has_rest = t_elems.iter().any(|e| e.rest);
        if s_arity < t_min {
            if let Some(c) = ctx.as_deref_mut() {
                self.rel_report_error(
                    c,
                    &gen::Source_has_0_element_s_but_target_requires_1,
                    vec![s_arity.to_string(), t_min.to_string()],
                );
            }
            return false;
        }
        if !t_has_rest && s_arity > t_elems.len() {
            if let Some(c) = ctx.as_deref_mut() {
                self.rel_report_error(
                    c,
                    &gen::Source_has_0_element_s_but_target_allows_only_1,
                    vec![s_arity.to_string(), t_elems.len().to_string()],
                );
            }
            return false;
        }
        let s_has_rest = s_elems.iter().any(|e| e.rest);
        let t_rest_pos = t_elems.iter().position(|e| e.rest);
        match t_rest_pos {
            // target has a single rest element and the source is a fixed tuple:
            // match the leading fixed elements from the front, the trailing fixed
            // elements from the back, and every middle source element against the
            // rest element type (`[number, ...boolean[], string]`).
            Some(r) if !s_has_rest => {
                let lead = &t_elems[..r];
                let rest_et = t_elems[r].ty;
                let trail: Vec<crate::types::TupleElem> = t_elems[r + 1..].to_vec();
                let trail_len = trail.len();
                let mid_end = s_arity.saturating_sub(trail_len);
                for (i, te) in lead.iter().enumerate() {
                    let Some(se) = s_elems.get(i) else { break };
                    if !self.is_assignable_to(se.ty, te.ty) {
                        if ctx.is_some() {
                            return self.related_with_head(se.ty, te.ty, ctx, None);
                        }
                        return false;
                    }
                }
                for k in lead.len()..mid_end {
                    let Some(se) = s_elems.get(k) else { break };
                    if !self.is_assignable_to(se.ty, rest_et) {
                        if ctx.is_some() {
                            return self.related_with_head(se.ty, rest_et, ctx, None);
                        }
                        return false;
                    }
                }
                for (j, te) in trail.iter().enumerate() {
                    let si = mid_end + j;
                    let Some(se) = s_elems.get(si) else { break };
                    if !self.is_assignable_to(se.ty, te.ty) {
                        if ctx.is_some() {
                            return self.related_with_head(se.ty, te.ty, ctx, None);
                        }
                        return false;
                    }
                }
                true
            }
            _ => {
                for (i, se) in s_elems.iter().enumerate() {
                    let te = t_elems.get(i).or_else(|| t_elems.iter().find(|e| e.rest));
                    let Some(te) = te else { break };
                    if !self.is_assignable_to(se.ty, te.ty) {
                        if ctx.is_some() {
                            return self.related_with_head(se.ty, te.ty, ctx, None);
                        }
                        return false;
                    }
                }
                true
            }
        }
    }

    fn structured_related(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        ctx: &mut Option<&mut RelCtx>,
    ) -> bool {
        let Some(t_shape_id) = self.shape_of_type(tgt) else {
            return false;
        };
        let Some(s_shape_id) = self.shape_of_type(src) else {
            return false;
        };
        let t_shape = self.types.shape(t_shape_id).clone();
        let s_shape = self.types.shape(s_shape_id).clone();

        // unmatched (missing) properties
        let missing: Vec<&crate::types::PropInfo> = t_shape
            .props
            .iter()
            .filter(|tp| {
                !tp.optional
                    && s_shape.prop(&tp.name).is_none()
                    && !self.shape_has_index_for(&s_shape, &tp.name)
            })
            .collect();
        if !missing.is_empty() {
            if let Some(c) = ctx.as_deref_mut() {
                self.report_unmatched_properties(c, src, tgt, &missing);
            }
            return false;
        }

        let mut result = true;
        // per-property relation
        for tp in &t_shape.props {
            let Some(sp) = s_shape.prop(&tp.name) else {
                continue;
            };
            let mut sp_ty = sp.ty;
            // optional source prop adds undefined
            if sp.optional && self.options.strict_null_checks() {
                sp_ty = self.types.union(vec![sp_ty, self.types.undefined]);
            }
            let mut tp_ty = tp.ty;
            let target_display_without_optional_undefined = tp.optional
                && self.options.strict_null_checks()
                && !self.options.exact_optional_property_types;
            if target_display_without_optional_undefined {
                tp_ty = self.types.union(vec![tp_ty, self.types.undefined]);
            }
            let _ = &tp_ty;
            // tsc propertyRelatedTo accessibility gate: private/protected
            // members are NOMINAL — they relate only when both sides carry
            // the modifier AND originate in the same declaration. Two
            // classes each declaring `private a: string` are neither
            // assignable nor comparable (2442); private-vs-public is 2325.
            // COMPARABLE MODE ONLY for now: the assignable relation has
            // heritage/override paths (2415/2430/2445) whose tsc semantics
            // (getTargetSymbol, override tolerance, interface-inherited
            // members) this gate does not yet mirror — enabling it there
            // trades 14 corpus FPs for the 8 it fixes. Full propertyRelatedTo
            // parity is a separate work item.
            let (s_np, t_np) = if self.rel.erase_generic_sigs {
                (self.prop_nonpublic(sp), self.prop_nonpublic(tp))
            } else {
                ((false, false), (false, false))
            };
            if s_np != t_np {
                if let Some(c) = ctx.as_deref_mut() {
                    let (priv_side, other) = if s_np.0 || s_np.1 {
                        (self.display_type(src), self.display_type(tgt))
                    } else {
                        (self.display_type(tgt), self.display_type(src))
                    };
                    self.rel_report_error(
                        c,
                        &gen::Property_0_is_private_in_type_1_but_not_in_type_2,
                        vec![tp.name.clone(), priv_side, other],
                    );
                }
                if ctx.is_none() {
                    return false;
                }
                result = false;
                break;
            }
            if (s_np.0 || s_np.1) && sp.symbol != tp.symbol {
                // protected members tolerate related declaring classes;
                // private require identity
                let tolerated = s_np.1 && !s_np.0 && {
                    let sc = sp.symbol.and_then(|m| self.symbol(m).parent);
                    let tc = tp.symbol.and_then(|m| self.symbol(m).parent);
                    match (sc, tc) {
                        (Some(a), Some(b)) => {
                            self.class_extends_class(a, b) || self.class_extends_class(b, a)
                        }
                        _ => false,
                    }
                };
                if !tolerated {
                    if let Some(c) = ctx.as_deref_mut() {
                        self.rel_report_error(
                            c,
                            &gen::Types_have_separate_declarations_of_a_private_property_0,
                            vec![tp.name.clone()],
                        );
                    }
                    if ctx.is_none() {
                        return false;
                    }
                    result = false;
                    break;
                }
            }
            // method-declared members compare bivariantly (tsc keeps methods
            // bivariant even under strictFunctionTypes)
            let bivariant_ok = (tp.is_method || sp.is_method)
                && self.is_assignable_to_bivariant_sigs(sp_ty, tp_ty);
            if !bivariant_ok && !self.is_assignable_to(sp_ty, tp_ty) {
                if let Some(_c) = ctx.as_deref_mut() {
                    let target_display = if target_display_without_optional_undefined {
                        self.display_without_single_undefined(tp.ty)
                            .or_else(|| Some(self.display_type(tp.ty)))
                    } else {
                        None
                    };
                    let r = self.related_with_display_override(
                        sp_ty,
                        tp_ty,
                        ctx,
                        None,
                        target_display,
                        None,
                    );
                    debug_assert!(!r);
                    if let Some(c2) = ctx.as_deref_mut() {
                        self.report_incompatible(
                            c2,
                            &gen::Types_of_property_0_are_incompatible,
                            vec![tp.name.clone()],
                        );
                    }
                }
                result = false;
                if ctx.is_none() {
                    return false;
                }
                break;
            }
        }
        if !result {
            return false;
        }

        // call signatures (tsc signaturesRelatedTo): each target call signature
        // must be matched by some source call signature (params contravariant,
        // return covariant via signature_related). Methods live in `props`; this
        // handles callable object/function types like `() => T`.
        if !t_shape.call_sigs.is_empty() {
            if s_shape.call_sigs.is_empty() {
                if let Some(c) = ctx.as_deref_mut() {
                    let sd = self.display_type(src);
                    let td = self.display_type(tgt);
                    self.rel_report_error(
                        c,
                        &gen::Type_0_is_not_assignable_to_type_1,
                        vec![sd, td],
                    );
                }
                return false;
            }
            // tsc signaturesRelatedTo: overloaded (multi-signature) lists
            // relate with BOTH signatures' type params erased to any; only
            // the single-vs-single case instantiates a generic source in
            // context (and erases only under the comparable relation)
            let multi = s_shape.call_sigs.len() > 1 || t_shape.call_sigs.len() > 1;
            for &t_sig in &t_shape.call_sigs {
                let matched = s_shape
                    .call_sigs
                    .iter()
                    .any(|&s_sig| self.signature_related(s_sig, t_sig, false, multi, &mut None));
                if !matched {
                    if let Some(&s_sig) = s_shape.call_sigs.first() {
                        let _ = self.signature_related(s_sig, t_sig, false, multi, ctx);
                    }
                    return false;
                }
            }
        }
        // construct signatures
        if !t_shape.ctor_sigs.is_empty() {
            if s_shape.ctor_sigs.is_empty() {
                if let Some(c) = ctx.as_deref_mut() {
                    let sd = self.display_type(src);
                    let td = self.display_type(tgt);
                    self.rel_report_error(
                        c,
                        &gen::Type_0_is_not_assignable_to_type_1,
                        vec![sd, td],
                    );
                }
                return false;
            }
            let multi = s_shape.ctor_sigs.len() > 1 || t_shape.ctor_sigs.len() > 1;
            for &t_sig in &t_shape.ctor_sigs {
                let matched = s_shape
                    .ctor_sigs
                    .iter()
                    .any(|&s_sig| self.signature_related(s_sig, t_sig, true, multi, &mut None));
                if !matched {
                    if let Some(&s_sig) = s_shape.ctor_sigs.first() {
                        let _ = self.signature_related(s_sig, t_sig, true, multi, ctx);
                    }
                    return false;
                }
            }
        }

        // index signatures
        for ti in &t_shape.index_infos {
            // every source prop assignable to target index value type
            for sp in &s_shape.props {
                if matches!(self.types.kind(ti.key), TypeKind::Number) && !is_numeric_name(&sp.name)
                {
                    continue;
                }
                if !self.is_assignable_to(sp.ty, ti.value) {
                    if let Some(c) = ctx.as_deref_mut() {
                        let pn = sp.name.clone();
                        let pt = self.display_type(sp.ty);
                        let key = self.display_type(ti.key);
                        let vt = self.display_type(ti.value);
                        self.rel_report_error(
                            c,
                            &gen::Property_0_of_type_1_is_not_assignable_to_2_index_type_3,
                            vec![pn, pt, format!("'{}'", key), vt],
                        );
                    }
                    return false;
                }
            }
            if let Some(si) = s_shape.index_infos.iter().find(|si| si.key == ti.key) {
                if !self.is_assignable_to(si.value, ti.value) {
                    return false;
                }
            }
            // A source need not declare its own index signature: every concrete
            // property was already checked against the target's index value type
            // above, and an empty source satisfies a string/number index
            // signature vacuously (`const d: { [k: string]: number } = {}` is
            // valid). So there is no further requirement here.
        }
        true
    }

    fn is_shape_meaningfully_empty(&self, s: &crate::types::Shape) -> bool {
        s.props.is_empty()
            && s.call_sigs.is_empty()
            && s.ctor_sigs.is_empty()
            && s.index_infos.is_empty()
    }

    fn shape_has_index_for(&self, shape: &crate::types::Shape, name: &str) -> bool {
        shape.index_infos.iter().any(|i| {
            matches!(self.types.kind(i.key), TypeKind::String)
                || (matches!(self.types.kind(i.key), TypeKind::Number) && is_numeric_name(name))
        })
    }

    /// port of reportUnmatchedProperty (head-aware elision)
    fn report_unmatched_properties(
        &mut self,
        ctx: &mut RelCtx,
        src: TypeId,
        tgt: TypeId,
        missing: &[&crate::types::PropInfo],
    ) {
        // shouldSkipElaboration = true unless head is 2420/2720; we don't know
        // the head here — tsc checks the OUTER head message. We thread it via
        // ctx: heads 2420/2720 set `keep_head_for_missing`.
        let should_skip = !self.rel.keep_head_for_missing;
        if missing.len() == 1 {
            let prop_name = missing[0].name.clone();
            let s = self.display_type(src);
            let t = self.display_type(tgt);
            self.rel_report_error(
                ctx,
                &gen::Property_0_is_missing_in_type_1_but_required_in_type_2,
                vec![prop_name.clone(), s, t],
            );
            // tsc associateRelatedInfo: "'<prop>' is declared here." (TS2728)
            // pointing at the missing target property's declaration.
            if let Some(sym) = missing[0].symbol {
                if let Some((decl_span, file)) = {
                    let sy = self.symbol(sym);
                    sy.decls.first().map(|d| (d.name_span(), sy.file))
                } {
                    ctx.pending_related.push(RelatedInfo {
                        file: Some(file),
                        start: decl_span.start,
                        length: decl_span.len(),
                        message: MessageChain::new(&gen::_0_is_declared_here, &[prop_name]),
                    });
                }
            }
            if should_skip && ctx.error_info.is_some() {
                ctx.override_next += 1;
            }
        } else {
            let s = self.display_type(src);
            let t = self.display_type(tgt);
            if missing.len() > 5 {
                let names: Vec<String> = missing.iter().take(4).map(|p| p.name.clone()).collect();
                self.rel_report_error(
                    ctx,
                    &gen::Type_0_is_missing_the_following_properties_from_type_1_Colon_2_and_3_more,
                    vec![s, t, names.join(", "), (missing.len() - 4).to_string()],
                );
            } else {
                let names: Vec<String> = missing.iter().map(|p| p.name.clone()).collect();
                self.rel_report_error(
                    ctx,
                    &gen::Type_0_is_missing_the_following_properties_from_type_1_Colon_2,
                    vec![s, t, names.join(", ")],
                );
            }
            if should_skip && ctx.error_info.is_some() {
                ctx.override_next += 1;
            }
        }
    }

    /// single-call-sig function types compared with bivariant parameters
    pub fn is_assignable_to_bivariant_sigs(&mut self, src: TypeId, tgt: TypeId) -> bool {
        let (Some(ss), Some(ts)) = (self.shape_of_type(src), self.shape_of_type(tgt)) else {
            return false;
        };
        let s_sigs = self.types.shape(ss).call_sigs.clone();
        let t_sigs = self.types.shape(ts).call_sigs.clone();
        if s_sigs.len() != 1 || t_sigs.len() != 1 {
            return false;
        }
        let s = self.types.sig(s_sigs[0]).clone();
        let t = self.types.sig(t_sigs[0]).clone();
        if s.min_args > (t.params.len() as u32) && t.rest.is_none() {
            return false;
        }
        for (i, sp) in s.params.iter().enumerate() {
            let t_param_ty = t
                .params
                .get(i)
                .map(|p| p.ty)
                .or(t.rest)
                .unwrap_or(self.types.any);
            if !self.is_assignable_to(t_param_ty, sp.ty)
                && !self.is_assignable_to(sp.ty, t_param_ty)
            {
                return false;
            }
        }
        // source rest slot, same bivariant check (a rest-only signature
        // otherwise never compares its element — `fn(...a: Base[])` vs
        // `fn(...a: C[])` must fail)
        if let Some(s_rest) = s.rest {
            let i = s.params.len();
            let t_count = t.params.len() + usize::from(t.rest.is_some());
            if i + 1 <= t_count {
                let t_param_ty = t
                    .params
                    .get(i)
                    .map(|p| p.ty)
                    .or(t.rest)
                    .unwrap_or(self.types.any);
                if !self.is_assignable_to(t_param_ty, s_rest)
                    && !self.is_assignable_to(s_rest, t_param_ty)
                {
                    return false;
                }
            }
        }
        let s_ret = self.sig_return(s_sigs[0]);
        let t_ret = self.sig_return(t_sigs[0]);
        if matches!(self.types.kind(t_ret), TypeKind::Void) {
            return true;
        }
        self.is_assignable_to(s_ret, t_ret)
    }

    /// isImplementationCompatibleWithOverload: returns bivariant (or void
    /// target), parameters checked ignoring return types.
    pub fn sig_assignable_for_overload(
        &mut self,
        impl_sig: crate::types::SigId,
        overload: crate::types::SigId,
    ) -> bool {
        let i_ret = self.sig_return(impl_sig);
        let o_ret = self.sig_return(overload);
        let rets_ok = matches!(self.types.kind(o_ret), TypeKind::Void)
            || self.is_assignable_to(o_ret, i_ret)
            || self.is_assignable_to(i_ret, o_ret);
        if !rets_ok {
            return false;
        }
        let s = self.types.sig(impl_sig).clone();
        let t = self.types.sig(overload).clone();
        if s.min_args > (t.params.len() as u32) && t.rest.is_none() {
            return false;
        }
        for (i, sp) in s.params.iter().enumerate() {
            let t_param_ty = t
                .params
                .get(i)
                .map(|p| p.ty)
                .or(t.rest)
                .unwrap_or(self.types.any);
            if !self.is_assignable_to(t_param_ty, sp.ty) {
                return false;
            }
        }
        true
    }

    /// compareSignaturesRelated (params contravariant-ish with strictFunctionTypes
    /// relaxed to bivariant for methods; v1: target-param → source-param check)
    fn signature_related(
        &mut self,
        s_sig: crate::types::SigId,
        t_sig: crate::types::SigId,
        is_ctor: bool,
        force_erase: bool,
        ctx: &mut Option<&mut RelCtx>,
    ) -> bool {
        let s = self.types.sig(s_sig).clone();
        let t = self.types.sig(t_sig).clone();
        // Generic signatures carry their own type parameters; `<T>(x: T) => T`
        // must relate to `<U>(x: U) => U`. tsc instantiates the source in the
        // context of the target's type parameters — here we map the source's
        // parameters onto the target's positionally so the bodies compare with a
        // common set of type variables. Erasure of BOTH signatures' own type
        // params to `any` (tsc getErasedSignature) applies in comparable mode
        // (`eraseGenerics = relation === comparableRelation`) — what lets
        // `{ fn<T>(x: T): T }` compare against `{ fn(): string }` — AND
        // whenever either side's signature list is overloaded (force_erase;
        // tsc signaturesRelatedTo passes erase=true outside the 1-vs-1 case).
        let erase = force_erase || self.rel.erase_generic_sigs;
        let mk_erase = |this: &mut Self,
                        tps: &[crate::binder::SymbolId]|
         -> Option<crate::checker::symbols::Mapper> {
            if tps.is_empty() {
                return None;
            }
            let mut m = crate::checker::symbols::Mapper::new();
            for &p in tps {
                m.insert(p, this.types.any);
            }
            Some(m)
        };
        let (tp_mapper, t_mapper) = if erase {
            (
                mk_erase(self, &s.type_params),
                mk_erase(self, &t.type_params),
            )
        } else if !s.type_params.is_empty() && s.type_params.len() == t.type_params.len() {
            let mut m = crate::checker::symbols::Mapper::new();
            for (sp, tp) in s.type_params.iter().zip(t.type_params.iter()) {
                let tp_ty = self.types.intern_kind(TypeKind::TypeParam(*tp));
                m.insert(*sp, tp_ty);
            }
            (Some(m), None)
        } else if !s.type_params.is_empty() {
            // tsc compareSignaturesRelated: a generic SOURCE signature whose
            // type params don't line up with the target's is INSTANTIATED IN
            // THE CONTEXT of the target (instantiateSignatureInContextOf) —
            // T in `<T>(x: T) => T[]` infers from the target's parameter and
            // return types (applyToParameterTypes/applyToReturnTypes), then
            // the relation proceeds with that instantiation. Without this,
            // `<T>(x: T) => T[]` never relates to `(x: number) => number[]`.
            let mut infos: crate::checker::infer::InferMap = Default::default();
            let s_positions = s.params.len() + usize::from(s.rest.is_some());
            for i in 0..s_positions {
                let s_ty = s.params.get(i).map(|p| p.ty).or(s.rest);
                let t_ty = t.params.get(i).map(|p| p.ty).or(t.rest);
                if let (Some(sp), Some(tp)) = (s_ty, t_ty) {
                    self.infer_from(
                        sp,
                        tp,
                        &s.type_params,
                        &mut infos,
                        crate::checker::infer::infer_prio::NONE,
                        false,
                    );
                }
            }
            let s_ret_raw = self.sig_return(s_sig);
            let t_ret_raw = self.sig_return(t_sig);
            self.infer_from(
                s_ret_raw,
                t_ret_raw,
                &s.type_params,
                &mut infos,
                crate::checker::infer::infer_prio::RETURN_TYPE,
                false,
            );
            let mut inferred = self.get_inferred_types(&s, &infos);
            // tsc getInferredType clamps an inference to the parameter's
            // constraint — `<T extends Derived>(...x: T[]) => T` inferred
            // against `(...x: Base[]) => Base` yields T = Derived, and the
            // relation then correctly FAILS (Base ⊄ Derived)
            for &tp in &s.type_params {
                if let Some(c0) = self.constraint_of_type_param(tp) {
                    let c = self.instantiate_type(c0, &inferred);
                    if let Some(&cur) = inferred.get(&tp) {
                        if !self.is_assignable_to(cur, c) {
                            inferred.insert(tp, c);
                        }
                    }
                }
            }
            (Some(inferred), None)
        } else {
            (None, None)
        };
        let map_src = |this: &mut Self, ty: TypeId| -> TypeId {
            match &tp_mapper {
                Some(m) => this.instantiate_type(ty, m),
                None => ty,
            }
        };
        let map_tgt = |this: &mut Self, ty: TypeId| -> TypeId {
            match &t_mapper {
                Some(m) => this.instantiate_type(ty, m),
                None => ty,
            }
        };
        // param count: source must not require more than target provides
        if s.min_args > (t.params.len() as u32) && t.rest.is_none() {
            return false;
        }
        // parameters: contravariant
        for (i, sp) in s.params.iter().enumerate() {
            let t_param_ty = t
                .params
                .get(i)
                .map(|p| p.ty)
                .or(t.rest)
                .unwrap_or(self.types.any);
            let t_param_ty = map_tgt(self, t_param_ty);
            let s_param_ty = map_src(self, sp.ty);
            let related = self.is_assignable_to(t_param_ty, s_param_ty)
                || (!is_ctor
                    && !self.options.strict_function_types()
                    && self.is_assignable_to(s_param_ty, t_param_ty));
            if !related {
                if ctx.is_some() {
                    let target_display = if sp.optional {
                        self.display_without_single_undefined(s_param_ty)
                    } else {
                        None
                    };
                    let r = self.related_with_display_override(
                        t_param_ty,
                        s_param_ty,
                        ctx,
                        None,
                        target_display,
                        None,
                    );
                    debug_assert!(!r);
                    if let Some(c) = ctx.as_deref_mut() {
                        let t_name = t
                            .params
                            .get(i)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| "args".into());
                        self.rel_report_error(
                            c,
                            &gen::Types_of_parameters_0_and_1_are_incompatible,
                            vec![sp.name.clone(), t_name],
                        );
                    }
                }
                return false;
            }
        }
        // the source REST slot (tsc position-based params: with a rest param
        // involved paramCount = min(sourceCount, targetCount), so the source
        // rest element compares against the target's type at that position —
        // `(...a: Base[])` vs `(...a: C[])` is how the fixed loop above,
        // which only walks source fixed params, misses Base~C entirely).
        // Variance follows the TARGET's declaration kind like tsc
        // compareSignaturesRelated strictVariance: methods stay bivariant
        // even under strictFunctionTypes (never[]→ReadonlyArray<T> via
        // concat), plain function types are strictly contravariant
        // (`(...x: Base[]) => Base` must reject a Derived-constrained
        // generic source).
        if let Some(s_rest) = s.rest {
            let i = s.params.len();
            let t_count = t.params.len() + usize::from(t.rest.is_some());
            if i + 1 <= t_count {
                let t_param_ty = t
                    .params
                    .get(i)
                    .map(|p| p.ty)
                    .or(t.rest)
                    .unwrap_or(self.types.any);
                let t_param_ty = map_tgt(self, t_param_ty);
                let s_param_ty = map_src(self, s_rest);
                let bivariant =
                    !is_ctor && (!self.options.strict_function_types() || t.from_method);
                let related = self.is_assignable_to(t_param_ty, s_param_ty)
                    || (bivariant && self.is_assignable_to(s_param_ty, t_param_ty));
                if !related {
                    if ctx.is_some() {
                        let r = self.related_with_display_override(
                            t_param_ty, s_param_ty, ctx, None, None, None,
                        );
                        debug_assert!(!r);
                        if let Some(c) = ctx.as_deref_mut() {
                            let s_name = s.rest_name.clone().unwrap_or_else(|| "args".into());
                            let t_name = t
                                .params
                                .get(i)
                                .map(|p| p.name.clone())
                                .or_else(|| t.rest_name.clone())
                                .unwrap_or_else(|| "args".into());
                            self.rel_report_error(
                                c,
                                &gen::Types_of_parameters_0_and_1_are_incompatible,
                                vec![s_name, t_name],
                            );
                        }
                    }
                    return false;
                }
            }
        }
        // return types (void target accepts anything)
        let s_ret = self.sig_return(s_sig);
        let s_ret = map_src(self, s_ret);
        let t_ret = self.sig_return(t_sig);
        let t_ret = map_tgt(self, t_ret);
        if matches!(self.types.kind(t_ret), TypeKind::Void) {
            return true;
        }
        if !self.is_assignable_to(s_ret, t_ret) {
            if ctx.is_some() {
                let r = self.related_with_head(s_ret, t_ret, ctx, None);
                debug_assert!(!r);
                if let Some(c) = ctx.as_deref_mut() {
                    let sr = self.display_type(s_ret);
                    let tr = self.display_type(t_ret);
                    let no_args = s.params.is_empty()
                        && s.rest.is_none()
                        && t.params.is_empty()
                        && t.rest.is_none();
                    let msg: &'static DiagnosticMessage = match (is_ctor, no_args) {
                        (false, false) => &gen::Call_signature_return_types_0_and_1_are_incompatible,
                        (true, false) => &gen::Construct_signature_return_types_0_and_1_are_incompatible,
                        (false, true) => &gen::Call_signatures_with_no_arguments_have_incompatible_return_types_0_and_1,
                        (true, true) => &gen::Construct_signatures_with_no_arguments_have_incompatible_return_types_0_and_1,
                    };
                    self.report_incompatible(c, msg, vec![sr, tr]);
                }
            }
            return false;
        }
        true
    }

    /// compareTypePredicateRelatedTo: each errorReporter call wraps the

    fn is_excess_property_check_target(&mut self, tgt: TypeId) -> bool {
        match self.types.kind(tgt).clone() {
            TypeKind::Anon(s) => !self.is_shape_meaningfully_empty(&self.types.shape(s).clone()),
            TypeKind::DeferredObj(_) => match self.shape_of_type(tgt) {
                Some(s) => !self.is_shape_meaningfully_empty(&self.types.shape(s).clone()),
                None => false,
            },
            TypeKind::Iface(_) | TypeKind::Ref(..) => {
                // not arrays
                self.array_element_type(tgt).is_none()
            }
            TypeKind::Union(members) => {
                for m in members {
                    if self.is_excess_property_check_target(m) {
                        return true;
                    }
                }
                false
            }
            TypeKind::NonPrimitive => false,
            _ => false,
        }
    }

    fn has_excess_properties(
        &mut self,
        src: TypeId,
        tgt: TypeId,
        ctx: &mut Option<&mut RelCtx>,
    ) -> bool {
        let Some(s_shape_id) = self.shape_of_type(src) else {
            return false;
        };
        let s_props = self.types.shape(s_shape_id).props.clone();
        for sp in &s_props {
            if !self.is_known_property(tgt, &sp.name) {
                if let Some(c) = ctx.as_deref_mut() {
                    // retarget the error to the offending property's name node
                    if let Some(span) = self.fresh_prop_span(src, &sp.name) {
                        c.error_span = span;
                    }
                    let t_display = self.display_type(tgt);
                    // suggestion from target props
                    let cands = self.known_property_names(tgt);
                    if let Some(sug) =
                        super::spelling_suggestion(&sp.name, cands.iter().map(|s| s.as_str()))
                    {
                        let sug = sug.to_string();
                        self.rel_report_error(
                            c,
                            &gen::Object_literal_may_only_specify_known_properties_but_0_does_not_exist_in_type_1_Did_you_mean_to_write_2,
                            vec![sp.name.clone(), t_display.clone(), sug],
                        );
                    } else {
                        self.rel_report_error(
                            c,
                            &gen::Object_literal_may_only_specify_known_properties_and_0_does_not_exist_in_type_1,
                            vec![sp.name.clone(), t_display],
                        );
                    }
                    c.reported_standalone = true;
                    // flush into a standalone diagnostic immediately
                    if let Some(chain) = c.error_info.take() {
                        let span = c.error_span;
                        self.error_chain_at(span, chain);
                    }
                }
                return true;
            }
        }
        false
    }

    fn is_known_property(&mut self, tgt: TypeId, name: &str) -> bool {
        match self.types.kind(tgt).clone() {
            TypeKind::Union(members) => members.iter().any(|&m| self.is_known_property(m, name)),
            _ => {
                if let Some(shape_id) = self.shape_of_type(tgt) {
                    let shape = self.types.shape(shape_id);
                    if shape.prop(name).is_some() {
                        return true;
                    }
                    let has_index = self.shape_has_index_for(&shape.clone(), name);
                    if has_index {
                        return true;
                    }
                    false
                } else {
                    true
                }
            }
        }
    }

    fn known_property_names(&mut self, tgt: TypeId) -> Vec<String> {
        let mut names = Vec::new();
        match self.types.kind(tgt).clone() {
            TypeKind::Union(members) => {
                for m in members {
                    names.extend(self.known_property_names(m));
                }
            }
            _ => {
                if let Some(shape_id) = self.shape_of_type(tgt) {
                    for p in &self.types.shape(shape_id).props {
                        names.push(p.name.clone());
                    }
                }
            }
        }
        names
    }

    /// span of a property name inside the fresh object literal that produced `src`
    fn fresh_prop_span(&mut self, src: TypeId, name: &str) -> Option<Span> {
        self.caches
            .fresh_obj_props
            .get(&src)?
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| *s)
    }

    // ── elaboration (re-rooting into literal expressions) ───────────────────

    /// Anchored match of a concrete string against template-literal parts,
    /// honouring each placeholder's *type* (e.g. `${number}` only matches a
    /// numeric run). Backtracks over placeholder split points; falls back to
    /// the type-agnostic matcher for very long inputs to bound the work.
    pub fn template_matches_typed(&mut self, parts: &[crate::types::TplPart], s: &str) -> bool {
        if s.len() > 256 {
            return template_matches(parts, s);
        }
        self.tpl_match_from(parts, 0, s)
    }

    fn template_pattern_related(
        &mut self,
        src: &[crate::types::TplPart],
        tgt: &[crate::types::TplPart],
    ) -> bool {
        self.tpl_pattern_from(src.to_vec(), tgt, 0)
    }

    fn tpl_pattern_from(
        &mut self,
        src: Vec<crate::types::TplPart>,
        tgt: &[crate::types::TplPart],
        ti: usize,
    ) -> bool {
        use crate::types::TplPart;
        if ti == tgt.len() {
            return src.is_empty();
        }
        match &tgt[ti] {
            TplPart::Str(text) => self
                .consume_template_literal_prefix(src, text)
                .is_some_and(|rest| self.tpl_pattern_from(rest, tgt, ti + 1)),
            TplPart::Ty(ty) => {
                let ty = *ty;
                let next_literal = tgt[ti + 1..].iter().find_map(|part| match part {
                    TplPart::Str(s) if !s.is_empty() => Some(s.as_str()),
                    _ => None,
                });
                if let Some(lit) = next_literal {
                    for (prefix, rest) in self.split_template_pattern_before_literal(&src, lit) {
                        if self.template_segment_assignable_to_placeholder(&prefix, ty)
                            && self.tpl_pattern_from(rest, tgt, ti + 1)
                        {
                            return true;
                        }
                    }
                    false
                } else {
                    self.template_segment_assignable_to_placeholder(&src, ty)
                }
            }
        }
    }

    fn consume_template_literal_prefix(
        &self,
        mut parts: Vec<crate::types::TplPart>,
        literal: &str,
    ) -> Option<Vec<crate::types::TplPart>> {
        use crate::types::TplPart;
        if literal.is_empty() {
            return Some(parts);
        }
        let Some(first) = parts.first_mut() else {
            return None;
        };
        match first {
            TplPart::Str(s) if s.starts_with(literal) => {
                s.drain(..literal.len());
                if s.is_empty() {
                    parts.remove(0);
                }
                Some(parts)
            }
            _ => None,
        }
    }

    fn split_template_pattern_before_literal(
        &self,
        parts: &[crate::types::TplPart],
        literal: &str,
    ) -> Vec<(Vec<crate::types::TplPart>, Vec<crate::types::TplPart>)> {
        use crate::types::TplPart;
        let mut out = Vec::new();
        let mut prefix = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            match part {
                TplPart::Ty(t) => prefix.push(TplPart::Ty(*t)),
                TplPart::Str(s) => {
                    let mut start = 0;
                    while let Some(pos) = s[start..].find(literal) {
                        let pos = start + pos;
                        let mut pre = prefix.clone();
                        if pos > 0 {
                            pre.push(TplPart::Str(s[..pos].to_string()));
                        }
                        let mut rest = Vec::new();
                        rest.push(TplPart::Str(s[pos..].to_string()));
                        rest.extend(parts[i + 1..].iter().cloned());
                        out.push((pre, rest));
                        start = pos + literal.len();
                    }
                    prefix.push(TplPart::Str(s.clone()));
                }
            }
        }
        out
    }

    fn template_segment_assignable_to_placeholder(
        &mut self,
        parts: &[crate::types::TplPart],
        target: TypeId,
    ) -> bool {
        use crate::types::TplPart;
        if parts.is_empty() {
            return self.placeholder_accepts(target, "");
        }
        let mut static_text = String::new();
        let all_static = parts.iter().all(|part| match part {
            TplPart::Str(s) => {
                static_text.push_str(s);
                true
            }
            _ => false,
        });
        if all_static {
            return self.placeholder_accepts(target, &static_text);
        }
        if matches!(self.types.kind(target), TypeKind::String) {
            return parts.iter().all(|part| match part {
                TplPart::Str(_) => true,
                TplPart::Ty(t) => self.template_placeholder_subsumes(*t, target),
            });
        }
        parts.len() == 1
            && match parts[0] {
                TplPart::Str(ref s) => self.placeholder_accepts(target, s),
                TplPart::Ty(t) => self.template_placeholder_subsumes(t, target),
            }
    }

    fn template_placeholder_subsumes(&mut self, src: TypeId, tgt: TypeId) -> bool {
        if src == tgt || matches!(self.types.kind(tgt), TypeKind::String) {
            return true;
        }
        match self.types.kind(src).clone() {
            TypeKind::TypeParam(sym) => self
                .constraint_of_type_param(sym)
                .is_some_and(|c| self.template_placeholder_subsumes(c, tgt)),
            TypeKind::Union(members) if src == self.types.boolean => {
                self.template_placeholder_subsumes(self.types.true_t, tgt)
                    && self.template_placeholder_subsumes(self.types.false_t, tgt)
            }
            TypeKind::NumLit(_) => matches!(self.types.kind(tgt), TypeKind::Number),
            TypeKind::BigIntLit(_) => matches!(self.types.kind(tgt), TypeKind::Bigint),
            TypeKind::BoolLit(_) => tgt == self.types.boolean,
            _ => self.is_assignable_to(src, tgt),
        }
    }

    fn tpl_match_from(&mut self, parts: &[crate::types::TplPart], pi: usize, rest: &str) -> bool {
        use crate::types::TplPart;
        if pi == parts.len() {
            return rest.is_empty();
        }
        match &parts[pi] {
            TplPart::Str(text) => match rest.strip_prefix(text.as_str()) {
                Some(stripped) => self.tpl_match_from(parts, pi + 1, stripped),
                None => false,
            },
            TplPart::Ty(ty) => {
                let ty = *ty;
                // If the next part is a literal, only split points immediately
                // before an occurrence of it can succeed; otherwise try every
                // boundary. Always include the full remainder (trailing
                // placeholder).
                for k in 0..=rest.len() {
                    if !rest.is_char_boundary(k) {
                        continue;
                    }
                    let (consumed, remainder) = rest.split_at(k);
                    if self.placeholder_accepts(ty, consumed)
                        && self.tpl_match_from(parts, pi + 1, remainder)
                    {
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Match `s` against template `parts`, binding each `infer` placeholder (a
    /// type parameter in `tps`) to the substring it captured.
    pub fn collect_template_candidates(
        &mut self,
        parts: &[crate::types::TplPart],
        s: &str,
        tps: &[crate::binder::SymbolId],
        infos: &mut super::infer::InferMap,
        priority: u32,
    ) {
        if s.len() > 256 {
            return;
        }
        let mut binds: Vec<(crate::binder::SymbolId, String)> = Vec::new();
        if self.tpl_capture(parts, 0, s, tps, &mut binds) {
            for (sym, text) in binds {
                let lit = self.types.string_lit(&text);
                self.add_inference_candidate(infos, sym, lit, priority, false);
            }
        }
    }

    fn tpl_capture(
        &mut self,
        parts: &[crate::types::TplPart],
        pi: usize,
        rest: &str,
        tps: &[crate::binder::SymbolId],
        binds: &mut Vec<(crate::binder::SymbolId, String)>,
    ) -> bool {
        use crate::types::TplPart;
        if pi == parts.len() {
            return rest.is_empty();
        }
        match &parts[pi] {
            TplPart::Str(text) => match rest.strip_prefix(text.as_str()) {
                Some(r) => self.tpl_capture(parts, pi + 1, r, tps, binds),
                None => false,
            },
            TplPart::Ty(ty) => {
                let ty = *ty;
                let infer_sym = match self.types.kind(ty) {
                    TypeKind::TypeParam(s) if tps.contains(s) => Some(*s),
                    _ => None,
                };
                for k in 0..=rest.len() {
                    if !rest.is_char_boundary(k) {
                        continue;
                    }
                    let (consumed, remainder) = rest.split_at(k);
                    let ok = match infer_sym {
                        Some(_) => true,
                        None => self.placeholder_accepts(ty, consumed),
                    };
                    if ok {
                        let mark = binds.len();
                        if let Some(sym) = infer_sym {
                            binds.push((sym, consumed.to_string()));
                        }
                        if self.tpl_capture(parts, pi + 1, remainder, tps, binds) {
                            return true;
                        }
                        binds.truncate(mark);
                    }
                }
                false
            }
        }
    }

    /// Whether a placeholder of type `ty` accepts the concrete substring `text`.
    /// Permissive for unmodelled types (an over-broad match is a missed error,
    /// never a false positive).
    fn placeholder_accepts(&mut self, ty: TypeId, text: &str) -> bool {
        match self.types.kind(ty).clone() {
            TypeKind::String => true,
            TypeKind::Number => is_template_number(text),
            TypeKind::StrLit(s) => s.to_str_lossy().as_ref() == text,
            TypeKind::NumLit(bits) => crate::js_num::to_js_string(f64::from_bits(bits)) == text,
            TypeKind::BoolLit(b) => (if b { "true" } else { "false" }) == text,
            TypeKind::Union(ms) => ms.iter().any(|&m| self.placeholder_accepts(m, text)),
            TypeKind::TemplateLit(inner) => {
                let inner = inner.clone();
                self.template_matches_typed(&inner, text)
            }
            _ => true,
        }
    }
}

/// Whether `text` is a numeric literal that `${number}` would match.
fn is_template_number(text: &str) -> bool {
    let t = text.strip_prefix('-').unwrap_or(text);
    matches!(t.chars().next(), Some(c) if c.is_ascii_digit() || c == '.')
        && t.parse::<f64>().is_ok()
        && t.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
}

/// anchored match of a string literal against template-literal parts
fn template_matches(parts: &[crate::types::TplPart], s: &str) -> bool {
    use crate::types::TplPart;
    let mut pos = 0usize;
    let bytes = s.as_bytes();
    let n = parts.len();
    for (i, p) in parts.iter().enumerate() {
        match p {
            TplPart::Str(text) => {
                if i == 0 {
                    if !s[pos..].starts_with(text.as_str()) {
                        return false;
                    }
                    pos += text.len();
                } else if i == n - 1 {
                    // trailing literal: must be a suffix beyond current pos
                    if s.len() < pos + text.len() || !s.ends_with(text.as_str()) {
                        return false;
                    }
                } else {
                    match s[pos..].find(text.as_str()) {
                        Some(off) => pos += off + text.len(),
                        None => return false,
                    }
                }
            }
            TplPart::Ty(_) => {
                // ${string}/${number}/... matches greedily (validated by the
                // following literal segment); nothing to consume here
                let _ = bytes;
            }
        }
    }
    true
}

fn is_identifier_text(s: &str) -> bool {
    !s.is_empty()
        && s.chars().enumerate().all(|(i, c)| {
            c == '_' || c == '$' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
        })
}

pub(crate) fn is_numeric_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}
