//! Statement checking: declarations, control flow with flow-lite narrowing,
//! return paths, imports.

use super::{operators::TruthinessContext, Checker, EnumValue};
use crate::ast::*;
use crate::binder::{Decl, ScopeId, SymbolId};
use crate::diagnostics::gen;
use crate::types::{TypeId, TypeKind};

/// TS7027 statement classification (see `reach_kind`).
enum ReachKind {
    /// executable — the lazy reachability walk applies
    Plain,
    /// class/enum/namespace declaration — structural walk only
    Decl,
    /// non-executable — never reported, breaks contiguous runs
    Exempt,
}

/// tsc isInstantiatedModule: a namespace is instantiated (a value exists
/// at runtime) unless it contains only interfaces, type aliases, and
/// non-instantiated namespaces — const enums count only when preserved.
fn namespace_is_instantiated(body: &[Stmt], preserve_const_enums: bool) -> bool {
    body.iter().any(|s| match s {
        Stmt::Interface(_) | Stmt::TypeAlias(_) | Stmt::Empty { .. } => false,
        Stmt::Enum(e) => !e.is_const || preserve_const_enums,
        Stmt::Namespace(n) => namespace_is_instantiated(&n.body, preserve_const_enums),
        _ => true,
    })
}

impl<'a> Checker<'a> {
    pub fn check_statements(&mut self, stmts: &'a [Stmt], scope: ScopeId) {
        let prev = self.current_scope;
        self.current_scope = scope;
        // TS7027 range grouping (tsc checkSourceElementUnreachable): one
        // diagnostic per contiguous run of executable-and-unreachable
        // statements, spanning the first's start to the last's end.
        // Non-executable statements break a run; consumed statements are
        // remembered so check_statement's per-statement hook (which also
        // serves non-list positions like `while (false) foo();`) does not
        // re-report them.
        // The run-start query happens as each statement is reached — after all
        // preceding siblings were checked — because reachability can depend on
        // check-time state (exhaustive_switches) recorded by an earlier
        // statement; a premature walk would memoize a stale "reachable".
        // The forward extension only inspects statements inside the dead
        // region, whose verdicts cannot be flipped by later registrations.
        let do_ranges = !self.flow.within_unreachable_code
            && self.options.allow_unreachable_code != Some(true);
        for (i, stmt) in stmts.iter().enumerate() {
            if do_ranges
                && !self.flow.reported_unreachable.contains(&node_key(stmt))
                && self.stmt_is_unreachable(stmt)
            {
                let mut last = i;
                self.flow.reported_unreachable.insert(node_key(stmt));
                for (j, next) in stmts.iter().enumerate().skip(i + 1) {
                    if matches!(self.reach_kind(next), ReachKind::Exempt)
                        || !self.stmt_is_unreachable(next)
                    {
                        break;
                    }
                    self.flow.reported_unreachable.insert(node_key(next));
                    last = j;
                }
                let as_error = self.options.allow_unreachable_code == Some(false);
                let span = Span::new(stmt.span().start as usize, stmts[last].span().end as usize);
                self.unused_diag(span, &gen::Unreachable_code_detected, &[], as_error);
            }
            self.check_statement(stmt);
        }
        self.current_scope = prev;
    }

    /// TS7027 kind classification (tsc isPotentiallyExecutableNode + the
    /// declaration split of isSourceElementUnreachable).
    fn reach_kind(&self, stmt: &Stmt) -> ReachKind {
        match stmt {
            // hoisted / type-only / non-executable — never reported, and
            // they break a contiguous unreachable run
            Stmt::Func(_)
            | Stmt::Interface(_)
            | Stmt::TypeAlias(_)
            | Stmt::Empty { .. }
            | Stmt::Import(_)
            | Stmt::ImportEquals { .. }
            | Stmt::ExportNamed { .. }
            | Stmt::ExportDefault { .. }
            | Stmt::ExportAssign { .. }
            | Stmt::Missing { .. } => ReachKind::Exempt,
            // a Block is not itself executable (tsc kind 242 <
            // FirstStatement): its first executable INNER statement gets
            // the report via the per-statement hook
            Stmt::Block(_) => ReachKind::Exempt,
            // a bare non-block-scoped `var` with no initializers hoists
            Stmt::Var(v) => {
                if matches!(v.kind, VarKind::Var) && v.decls.iter().all(|d| d.init.is_none()) {
                    ReachKind::Exempt
                } else {
                    ReachKind::Plain
                }
            }
            // declarations carry no flowNode in tsc: only the binder's
            // structural bit applies (a never-call upstream does NOT make
            // them unreachable), with enum/namespace instantiation filters
            Stmt::Class(_) => ReachKind::Decl,
            Stmt::Enum(e) => {
                if e.is_const && !self.preserve_const_enums_like() {
                    ReachKind::Exempt
                } else {
                    ReachKind::Decl
                }
            }
            Stmt::Namespace(n) => {
                if namespace_is_instantiated(&n.body, self.preserve_const_enums_like()) {
                    ReachKind::Decl
                } else {
                    ReachKind::Exempt
                }
            }
            _ => ReachKind::Plain,
        }
    }

    fn preserve_const_enums_like(&self) -> bool {
        self.options.preserve_const_enums == Some(true) || self.options.isolated_modules_like()
    }

    /// Is this statement unreachable for TS7027? Plain statements use the
    /// lazy walk (never-calls and exhaustive switches terminate flow);
    /// class/enum/namespace declarations use the structural walk only.
    fn stmt_is_unreachable(&mut self, stmt: &'a Stmt) -> bool {
        let kind = self.reach_kind(stmt);
        if matches!(kind, ReachKind::Exempt) {
            return false;
        }
        let Some(&flow) = self.bind.stmt_flow.get(&node_key(stmt)) else {
            return false;
        };
        match kind {
            ReachKind::Plain => !self.is_reachable_flow(flow),
            ReachKind::Decl => !self.is_structurally_reachable(flow),
            ReachKind::Exempt => false,
        }
    }

    fn check_statement(&mut self, stmt: &'a Stmt) {
        // TS7027 per-statement hook (tsc checkSourceElementWorker): covers
        // non-list positions (`while (false) foo();`); list members were
        // range-grouped by check_statements. Descendants of an unreachable
        // statement are suppressed for the duration of its check.
        let was_within = self.flow.within_unreachable_code;
        if !was_within && self.options.allow_unreachable_code != Some(true) {
            if self.flow.reported_unreachable.contains(&node_key(stmt)) {
                self.flow.within_unreachable_code = true;
            } else if self.stmt_is_unreachable(stmt) {
                self.flow.reported_unreachable.insert(node_key(stmt));
                let as_error = self.options.allow_unreachable_code == Some(false);
                self.unused_diag(stmt.span(), &gen::Unreachable_code_detected, &[], as_error);
                self.flow.within_unreachable_code = true;
            }
        }
        self.check_statement_inner(stmt);
        self.flow.within_unreachable_code = was_within;
    }

    fn check_statement_inner(&mut self, stmt: &'a Stmt) {
        if self.current_file != self.lib_file
            && self.files[self.current_file].0.ends_with(".d.ts")
            && self.stacks.fn_stack.is_empty()
        {
            let needs = match stmt {
                Stmt::Var(v) => {
                    !has_modifier(&v.modifiers, ModifierKind::Declare)
                        && !has_modifier(&v.modifiers, ModifierKind::Export)
                }
                Stmt::Func(f) => {
                    !has_modifier(&f.modifiers, ModifierKind::Declare)
                        && !has_modifier(&f.modifiers, ModifierKind::Export)
                }
                Stmt::Class(c2) => {
                    !has_modifier(&c2.modifiers, ModifierKind::Declare)
                        && !has_modifier(&c2.modifiers, ModifierKind::Export)
                }
                _ => false,
            };
            if needs {
                let sp = stmt.span();
                self.error_at(
                    Span::new(sp.start as usize, sp.start as usize + 1),
                    &gen::Top_level_declarations_in_d_ts_files_must_start_with_either_a_declare_or_export_modifier,
                    &[],
                );
            }
        }
        let stmt_mods: Option<&Modifiers> = match stmt {
            Stmt::Var(v) => Some(&v.modifiers),
            Stmt::Func(f) => Some(&f.modifiers),
            Stmt::Class(c) => Some(&c.modifiers),
            Stmt::Interface(i) => Some(&i.modifiers),
            Stmt::TypeAlias(t) => Some(&t.modifiers),
            Stmt::Enum(e) => Some(&e.modifiers),
            Stmt::Namespace(n) => Some(&n.modifiers),
            _ => None,
        };
        if let Some(mods) = stmt_mods {
            for m in mods.iter() {
                if matches!(
                    m.kind,
                    ModifierKind::Private | ModifierKind::Protected | ModifierKind::Public
                ) {
                    self.error_at(
                        m.span,
                        &gen::_0_modifier_cannot_appear_on_a_module_or_namespace_element,
                        &[modifier_text(m.kind).to_string()],
                    );
                }
            }
            // 'export' must precede 'declare' (1029)
            let dp = mods.iter().position(|m| m.kind == ModifierKind::Declare);
            let ep = mods.iter().position(|m| m.kind == ModifierKind::Export);
            if let (Some(d), Some(e)) = (dp, ep) {
                if e > d {
                    self.error_at(
                        mods[e].span,
                        &gen::_0_modifier_must_precede_1_modifier,
                        &["export".to_string(), "declare".to_string()],
                    );
                }
            }
            // 'abstract' only on classes (and their members)
            if !matches!(stmt, Stmt::Class(_)) {
                if let Some(m) = mods.iter().find(|m| m.kind == ModifierKind::Abstract) {
                    self.error_at(
                        m.span,
                        &gen::abstract_modifier_can_only_appear_on_a_class_method_or_property_declaration,
                        &[],
                    );
                }
            }
        }
        match stmt {
            Stmt::Var(v) => self.check_var_stmt(v, false),
            Stmt::Func(f) => {
                self.check_overload_group(f);
                self.check_function_body(f, None, true);
            }
            Stmt::Class(c) => self.check_class(c),
            Stmt::Interface(i) => {
                if let Some(tps) = &i.type_params {
                    for tp in tps {
                        if let Some(cs) = tp.const_span {
                            self.error_at(
                                cs,
                                &gen::_0_modifier_can_only_appear_on_a_type_parameter_of_a_function_method_or_class,
                                &["const".to_string()],
                            );
                        }
                    }
                }
                // 2428: merged declarations need identical type parameters
                if let Some(&sym) = self.bind.decl_symbol.get(&node_key(&**i)) {
                    if self.report_once_sym(2428, sym) {
                        let decls: Vec<&'a InterfaceDecl> = self
                            .symbol(sym)
                            .decls
                            .iter()
                            .filter_map(|d| match d {
                                crate::binder::Decl::Interface(idecl) => Some(*idecl),
                                _ => None,
                            })
                            .collect();
                        if decls.len() > 1 {
                            let text = |c: &Self, sp: Span| -> String {
                                c.files[c.current_file]
                                    .1
                                    .text
                                    .get(sp.start as usize..sp.end as usize)
                                    .unwrap_or("")
                                    .to_string()
                            };
                            let sig = |d: &InterfaceDecl| -> Vec<String> {
                                d.type_params
                                    .as_ref()
                                    .map(|tps| {
                                        tps.iter()
                                            .map(|t| {
                                                let c = t
                                                    .constraint
                                                    .as_ref()
                                                    .map(|cn| text(self, cn.span()))
                                                    .unwrap_or_default();
                                                format!("{}:{}", t.name.name, c)
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default()
                            };
                            let first = sig(decls[0]);
                            if decls.iter().any(|d| sig(d) != first) {
                                let name = self.symbol(sym).name.clone();
                                for d in decls {
                                    let prev = self.current_file;
                                    self.current_file = self
                                        .bind
                                        .decl_file
                                        .get(&node_key(d))
                                        .copied()
                                        .unwrap_or(prev);
                                    self.error_at(
                                        d.name.span,
                                        &gen::All_declarations_of_0_must_have_identical_type_parameters,
                                        &[name.clone()],
                                    );
                                    self.current_file = prev;
                                }
                            }
                        }
                    }
                }
                let iscope = self
                    .bind
                    .node_scope
                    .get(&node_key(&**i))
                    .copied()
                    .unwrap_or(self.current_scope);
                self.check_type_member_grammar(&i.members);
                self.check_duplicate_index_signatures(&i.members);
                self.check_index_compatibility(&i.members, iscope);
                // resolve member types eagerly (memoized per node) so errors surface
                let scope = self
                    .bind
                    .node_scope
                    .get(&node_key(&**i))
                    .copied()
                    .unwrap_or(self.current_scope);
                let prev = self.current_scope;
                self.current_scope = scope;
                if !self.check_interface_base_conflicts(i, scope) {
                    for ext in &i.extends {
                        self.check_interface_extends(i, ext, scope);
                    }
                }
                let pushed_this = self.bind.decl_symbol.get(&node_key(&**i)).copied();
                self.with_opt_this_type(pushed_this, |c| {
                    for m in &i.members {
                        c.resolve_type_member_types(m, scope);
                    }
                });
                // force base-type resolution (2310 cycles)
                if let Some(sym) = self.bind.decl_symbol.get(&node_key(&**i)).copied() {
                    let t = self.types.intern_kind(TypeKind::Iface(sym));
                    self.shape_of_type(t);
                    self.check_iface_merge_conflicts(sym, i);
                }
                self.current_scope = prev;
            }
            Stmt::ExportDefault { expr, .. } => {
                self.check_expr(expr, None);
            }
            Stmt::ExportAssign { expr, span, .. } => {
                self.check_expr(expr, None);
                // cannot mix with other exported elements (2309)
                let has_other = self
                    .bind
                    .exports
                    .get(&self.current_file)
                    .map(|t| !t.0.is_empty())
                    .unwrap_or(false);
                if has_other {
                    self.error_at(
                        Span::new(span.start as usize, span.start as usize + 6),
                        &gen::An_export_assignment_cannot_be_used_in_a_module_with_other_exported_elements,
                        &[],
                    );
                }
            }
            Stmt::ImportEquals { module, .. } => {
                if self
                    .resolve_module(self.current_file, &module.value)
                    .is_none()
                    && (module.value.starts_with("./") || module.value.starts_with("../"))
                {
                    self.error_at(
                        module.span,
                        &gen::Cannot_find_module_0_or_its_corresponding_type_declarations,
                        &[module.value.clone()],
                    );
                }
            }
            Stmt::With {
                obj, body, kw_span, ..
            } => {
                if self.options.strict.unwrap_or(false) {
                    self.error_at(
                        *kw_span,
                        &gen::with_statements_are_not_allowed_in_strict_mode,
                        &[],
                    );
                }
                self.error_at(
                    *kw_span,
                    &gen::The_with_statement_is_not_supported_All_symbols_in_a_with_block_will_have_type_any,
                    &[],
                );
                self.check_expr(obj, None);
                self.check_statement(body);
            }
            Stmt::Enum(e) => self.check_enum(e),
            Stmt::Namespace(n) => {
                let scope = self
                    .bind
                    .node_scope
                    .get(&node_key(&**n))
                    .copied()
                    .unwrap_or(self.current_scope);
                if has_modifier(&n.modifiers, ModifierKind::Declare) {
                    for s in &n.body {
                        match s {
                            Stmt::Var(v) => {
                                if let Some(m) =
                                    v.modifiers.iter().find(|m| m.kind == ModifierKind::Declare)
                                {
                                    self.error_at(
                                        m.span,
                                        &gen::A_declare_modifier_cannot_be_used_in_an_already_ambient_context,
                                        &[],
                                    );
                                }
                            }
                            Stmt::Func(_)
                            | Stmt::Class(_)
                            | Stmt::Interface(_)
                            | Stmt::TypeAlias(_)
                            | Stmt::Enum(_)
                            | Stmt::Namespace(_)
                            | Stmt::Empty { .. }
                            | Stmt::ExportNamed(_) => {}
                            other => {
                                let sp = other.span();
                                self.error_at(
                                    Span::new(sp.start as usize, sp.start as usize + 2),
                                    &gen::Statements_are_not_allowed_in_ambient_contexts,
                                    &[],
                                );
                            }
                        }
                    }
                }
                self.cflags.namespace_depth += 1;
                let pushed_ambient = has_modifier(&n.modifiers, ModifierKind::Declare);
                if pushed_ambient {
                    self.cflags.ambient_context_depth += 1;
                }
                let ns_ctx = crate::checker::NamespaceContext {
                    fn_depth: self.stacks.fn_stack.len(),
                    class_depth: self.stacks.class_stack.len(),
                    this_container_depth: self.stacks.this_container_stack.len(),
                };
                self.with_namespace(ns_ctx, |this| this.check_statements(&n.body, scope));
                if pushed_ambient {
                    self.cflags.ambient_context_depth -= 1;
                }
                self.cflags.namespace_depth -= 1;
                // 2395: merged declarations must agree on export-ness
                if let Some(&sym) = self.bind.decl_symbol.get(&node_key(&**n)) {
                    if self.report_once_sym(2395, sym) {
                        let mut sites: Vec<(Span, bool)> = Vec::new();
                        for d in self.symbol(sym).decls.clone() {
                            match d {
                                crate::binder::Decl::Func(f2) => sites.push((
                                    d.name_span(),
                                    has_modifier(&f2.modifiers, ModifierKind::Export),
                                )),
                                crate::binder::Decl::Namespace(nn) => sites.push((
                                    nn.name.span,
                                    has_modifier(&nn.modifiers, ModifierKind::Export),
                                )),
                                crate::binder::Decl::Class(c2) => sites.push((
                                    d.name_span(),
                                    has_modifier(&c2.modifiers, ModifierKind::Export),
                                )),
                                _ => {}
                            }
                        }
                        if sites.len() > 1 && !sites.iter().all(|(_, e)| *e == sites[0].1) {
                            let name = self.symbol(sym).name.clone();
                            for (span, _) in sites {
                                self.error_at(
                                    span,
                                    &gen::Individual_declarations_in_merged_declaration_0_must_be_all_exported_or_all_local,
                                    &[name.clone()],
                                );
                            }
                        }
                    }
                }
            }
            Stmt::TypeAlias(t) => {
                const RESERVED_ALIAS: &[&str] = &[
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
                if RESERVED_ALIAS.contains(&t.name.name.as_str()) {
                    self.error_at(
                        t.name.span,
                        &gen::Type_alias_name_cannot_be_0,
                        &[t.name.name.clone()],
                    );
                }
                if let Some(sym) = self.bind.decl_symbol.get(&node_key(&**t)).copied() {
                    self.declared_alias_type(sym);
                }
            }
            Stmt::Return { expr, span } => self.check_return(expr.as_ref(), *span),
            Stmt::If {
                cond, then, els, ..
            } => {
                if let Stmt::Empty { span } = &**then {
                    self.error_at(
                        *span,
                        &gen::The_body_of_an_if_statement_cannot_be_the_empty_statement,
                        &[],
                    );
                }
                let ct = self.check_expr(cond, None);
                self.check_testable(cond, ct, TruthinessContext::Condition);
                // narrowing is the resolver's job (Cond edges in the flow
                // graph); the checker just walks the branches
                self.check_statement(then);
                if let Some(els) = els {
                    self.check_statement(els);
                }
            }
            Stmt::While { cond, body, .. } => {
                let ct = self.check_expr(cond, None);
                self.check_testable(cond, ct, TruthinessContext::LoopCondition);
                self.flow.loop_depth += 1;
                self.check_statement(body);
                self.flow.loop_depth -= 1;
            }
            Stmt::DoWhile { body, cond, .. } => {
                self.flow.loop_depth += 1;
                self.check_statement(body);
                self.flow.loop_depth -= 1;
                let ct = self.check_expr(cond, None);
                self.check_testable(cond, ct, TruthinessContext::LoopCondition);
            }
            Stmt::For {
                init,
                cond,
                incr,
                body,
                ..
            } => {
                let scope = self
                    .bind
                    .node_scope
                    .get(&node_key(stmt))
                    .copied()
                    .unwrap_or(self.current_scope);
                let prev = self.current_scope;
                self.current_scope = scope;
                if let Some(init) = init {
                    match &**init {
                        ForInit::Var(v) => self.check_var_stmt(v, true),
                        ForInit::Expr(e) => {
                            self.check_expr(e, None);
                        }
                    }
                }
                if let Some(c) = cond {
                    let ct = self.check_expr(c, None);
                    self.check_testable(c, ct, TruthinessContext::LoopCondition);
                }
                self.flow.loop_depth += 1;
                self.check_statement(body);
                self.flow.loop_depth -= 1;
                if let Some(i) = incr {
                    self.check_expr(i, None);
                }
                self.current_scope = prev;
            }
            Stmt::ForIn {
                left,
                expr,
                body,
                init_span,
                extra_decl_span,
                ..
            } => {
                if let ForInit::Var(v) = &**left {
                    if v.decls.is_empty() {
                        self.error_at(v.span, &gen::Variable_declaration_list_cannot_be_empty, &[]);
                    }
                    for d in &v.decls {
                        if !matches!(&d.name, Binding::Ident(_)) {
                            self.error_at(
                                d.name.span(),
                                &gen::The_left_hand_side_of_a_for_in_statement_cannot_be_a_destructuring_pattern,
                                &[],
                            );
                            let s = self.types.string;
                            self.destructure_binding(&d.name, s);
                        }
                    }
                }
                if let Some(s) = init_span {
                    self.error_at(
                        *s,
                        &gen::The_variable_declaration_of_a_for_in_statement_cannot_have_an_initializer,
                        &[],
                    );
                }
                if let Some(s) = extra_decl_span {
                    self.error_at(
                        *s,
                        &gen::Only_a_single_variable_declaration_is_allowed_in_a_for_in_statement,
                        &[],
                    );
                }
                if let ForInit::Var(v) = &**left {
                    for d in &v.decls {
                        if d.ty.is_some() {
                            self.error_at(
                                d.name.span(),
                                &gen::The_left_hand_side_of_a_for_in_statement_cannot_use_a_type_annotation,
                                &[],
                            );
                        }
                    }
                }
                if let ForInit::Expr(lhs) = &**left {
                    if let Expr::Ident(id) = lhs {
                        if let Some(sym) = self.lookup_value(self.current_scope, &id.name) {
                            self.symuse.assigned_symbols.insert(sym);
                        }
                    }
                    let lt = self.check_target_type(lhs);
                    let lr = self.types.regular(lt);
                    let ok = matches!(
                        self.types.kind(lr),
                        TypeKind::String | TypeKind::Any | TypeKind::Error
                    );
                    if !ok {
                        self.error_at(
                            lhs.span(),
                            &gen::The_left_hand_side_of_a_for_in_statement_must_be_of_type_string_or_any,
                            &[],
                        );
                    }
                }
                {
                    let rt = self.check_expr(expr, None);
                    let rr = self.types.regular(rt);
                    let primitive = matches!(
                        self.types.kind(rr),
                        TypeKind::String
                            | TypeKind::Number
                            | TypeKind::Bigint
                            | TypeKind::StrLit(_)
                            | TypeKind::NumLit(_)
                            | TypeKind::BoolLit(_)
                            | TypeKind::BigIntLit(_)
                            | TypeKind::Undefined
                            | TypeKind::Null
                            | TypeKind::Void
                            | TypeKind::EsSymbol
                            | TypeKind::Never
                    );
                    if primitive {
                        let d = self.display_type(rr);
                        self.error_at(
                            expr.span(),
                            &gen::The_right_hand_side_of_a_for_in_statement_must_be_of_type_any_an_object_type_or_a_type_parameter_but_here_has_type_0,
                            &[d],
                        );
                    }
                }
                let scope = self
                    .bind
                    .node_scope
                    .get(&node_key(stmt))
                    .copied()
                    .unwrap_or(self.current_scope);
                let prev = self.current_scope;
                self.current_scope = scope;
                self.check_expr(expr, None);
                self.flow.loop_depth += 1;
                self.check_statement(body);
                self.flow.loop_depth -= 1;
                self.current_scope = prev;
            }
            Stmt::ForOf {
                left,
                expr,
                body,
                await_span,
                init_span,
                extra_decl_span,
                ..
            } => {
                if let ForInit::Var(v) = &**left {
                    if v.decls.is_empty() {
                        self.error_at(v.span, &gen::Variable_declaration_list_cannot_be_empty, &[]);
                    }
                }
                if let ForInit::Expr(lhs) = &**left {
                    let ref_like = matches!(
                        lhs,
                        Expr::Ident(_) | Expr::PropAccess { .. } | Expr::ElemAccess { .. }
                    );
                    let lt = self.check_target_type(lhs);
                    let et = {
                        let rt = self.check_expr(expr, None);
                        self.array_element_type(rt)
                    };
                    if let Some(elem) = et {
                        if !self.types.is_error(lt) && !self.types.is_any_or_error(elem) {
                            self.check_assignable(elem, lt, lhs.span(), None, None);
                        }
                    }
                    if !ref_like {
                        self.error_at(
                            lhs.span(),
                            &gen::The_left_hand_side_of_a_for_of_statement_must_be_a_variable_or_a_property_access,
                            &[],
                        );
                    }
                }
                if let Some(s) = init_span {
                    self.error_at(
                        *s,
                        &gen::The_variable_declaration_of_a_for_of_statement_cannot_have_an_initializer,
                        &[],
                    );
                }
                if let Some(s) = extra_decl_span {
                    self.error_at(
                        *s,
                        &gen::Only_a_single_variable_declaration_is_allowed_in_a_for_of_statement,
                        &[],
                    );
                }
                if let ForInit::Var(v) = &**left {
                    for d in &v.decls {
                        if d.ty.is_some() {
                            self.error_at(
                                d.name.span(),
                                &gen::The_left_hand_side_of_a_for_of_statement_cannot_use_a_type_annotation,
                                &[],
                            );
                        }
                    }
                }
                if let Some(aspan) = await_span {
                    if self.stacks.fn_stack.is_empty() {
                        // top level: needs a module + a capable module option
                        if !self.files[self.current_file].2.is_module {
                            self.error_at(
                                *aspan,
                                &gen::for_await_loops_are_only_allowed_at_the_top_level_of_a_file_when_that_file_is_a_module_but_this_file_has_no_imports_or_exports_Consider_adding_an_empty_export_to_make_this_file_a_module,
                                &[],
                            );
                        }
                        self.error_at(
                            *aspan,
                            &gen::Top_level_for_await_loops_are_only_allowed_when_the_module_option_is_set_to_es2022_esnext_system_node16_node18_node20_nodenext_or_preserve_and_the_target_option_is_set_to_es2017_or_higher,
                            &[],
                        );
                    } else {
                        let in_async = self.stacks.fn_stack.iter().rev().any(|f| f.is_async);
                        if !in_async {
                            self.error_at(
                                *aspan,
                                &gen::for_await_loops_are_only_allowed_within_async_functions_and_at_the_top_levels_of_modules,
                                &[],
                            );
                        }
                    }
                }
                let scope = self
                    .bind
                    .node_scope
                    .get(&node_key(stmt))
                    .copied()
                    .unwrap_or(self.current_scope);
                let prev = self.current_scope;
                self.current_scope = scope;
                let t = self.check_expr(expr, None);
                let elem = self.for_of_element_type(t, expr);
                if let ForInit::Var(v) = &**left {
                    for d in &v.decls {
                        if let Some(sym) = self.bind.decl_symbol.get(&node_key(d)).copied() {
                            self.caches.sym_type.insert(sym, elem);
                        }
                    }
                }
                self.flow.loop_depth += 1;
                self.check_statement(body);
                self.flow.loop_depth -= 1;
                self.current_scope = prev;
            }
            Stmt::Block(b) => {
                let scope = self
                    .bind
                    .node_scope
                    .get(&node_key(b))
                    .copied()
                    .unwrap_or(self.current_scope);
                self.check_statements(&b.stmts, scope);
            }
            Stmt::Expr { expr, .. } => {
                if matches!(expr, Expr::Yield { .. }) {
                    self.yield_statement_positions.insert(node_key(expr));
                }
                self.check_expr(expr, None);
            }
            Stmt::Throw { expr, .. } => {
                self.check_expr(expr, None);
            }
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                let scope = self
                    .bind
                    .node_scope
                    .get(&node_key(block))
                    .copied()
                    .unwrap_or(self.current_scope);
                self.check_statements(&block.stmts, scope);
                if let Some(c) = catch {
                    let bscope = self
                        .bind
                        .node_scope
                        .get(&node_key(&c.block))
                        .copied()
                        .unwrap_or(self.current_scope);
                    self.check_statements(&c.block.stmts, bscope);
                }
                if let Some(f) = finally {
                    let fscope = self
                        .bind
                        .node_scope
                        .get(&node_key(f))
                        .copied()
                        .unwrap_or(self.current_scope);
                    self.check_statements(&f.stmts, fscope);
                }
            }
            Stmt::Switch { expr, cases, .. } => {
                let t = self.check_expr(expr, None);
                let scope = self
                    .bind
                    .node_scope
                    .get(&node_key(stmt))
                    .copied()
                    .unwrap_or(self.current_scope);
                let mut saw_default = false;
                self.flow.switch_depth += 1;
                for c in cases {
                    if let Some(test) = &c.test {
                        let tt = self.check_expr(test, None);
                        let _ = tt;
                    }
                    self.narrowed(|this| {
                        if let Some(test) = &c.test {
                            if this.ref_key_of_pub(expr).is_some() {
                                this.narrow_switch_case(expr, test);
                            }
                            let ct = this.caches.expr_type_cache
                                .get(&(test as *const Expr as usize))
                                .copied()
                                .unwrap_or(this.types.any);
                            let cr = this.types.regular(ct);
                            let sr = this.types.regular(t);
                            if !this.types.is_any_or_error(cr)
                                && !this.types.is_any_or_error(sr)
                                && !this.is_assignable_to(cr, sr)
                                && !this.is_assignable_to(sr, cr)
                            {
                                let cd = this.display_type_for_error(cr, sr);
                                let sd = this.display_type(sr);
                                this.error_at(
                                    test.span(),
                                    &gen::Type_0_is_not_comparable_to_type_1,
                                    &[cd, sd],
                                );
                            }
                        } else {
                            if saw_default {
                                this.error_at(
                                    Span::new(c.span.start as usize, c.span.start as usize + 7),
                                    &gen::A_default_clause_cannot_appear_more_than_once_in_a_switch_statement,
                                    &[],
                                );
                            }
                            saw_default = true;
                            // the default clause sees the discriminant narrowed by
                            // the negation of every case label, so an exhaustive
                            // switch leaves it `never` (assertNever type-checks).
                            if this.ref_key_of_pub(expr).is_some() {
                                for c2 in cases {
                                    if let Some(t2) = &c2.test {
                                        this.narrow_switch_case_negative(expr, t2);
                                    }
                                }
                            }
                        }
                        // type-check the case body under the narrowed discriminant
                        this.check_statements(&c.stmts, scope);
                    });
                }
                self.flow.switch_depth -= 1;
                // record exhaustiveness (no default, but case labels cover every
                // member of the finite discriminant) for return-reachability; the
                // discriminant and labels are already type-checked above, so this
                // reads cached types in the correct scope.
                if !saw_default {
                    let dr = self.types.regular(t);
                    if let Some(members) = self.exhaustive_members(dr) {
                        let labels: Vec<crate::types::TypeId> = cases
                            .iter()
                            .filter_map(|c| c.test.as_ref())
                            .map(|ce| {
                                let ct = self
                                    .caches
                                    .expr_type_cache
                                    .get(&(ce as *const Expr as usize))
                                    .copied()
                                    .unwrap_or(self.types.any);
                                self.types.regular(ct)
                            })
                            .collect();
                        let covered = members.iter().all(|&m| {
                            labels.iter().any(|&l| {
                                l == m
                                    || (self.is_assignable_to(m, l) && self.is_assignable_to(l, m))
                            })
                        });
                        if covered {
                            self.flow.exhaustive_switches.insert(node_key(stmt));
                        }
                    }
                }
                // 7029 Fallthrough case in switch.
                if self.options.no_fallthrough_cases_in_switch {
                    // tsc: the binder records a fallthroughFlowNode only for
                    // non-last clauses with statements; empty clauses merge
                    // into the next label without a warning. Reachability of
                    // the clause body's end is the lazy walk (a trailing
                    // never-call suppresses the warning).
                    for (i, c) in cases.iter().enumerate() {
                        if i + 1 < cases.len()
                            && !c.stmts.is_empty()
                            && self
                                .bind
                                .clause_fallthrough
                                .get(&node_key(c))
                                .copied()
                                .map_or(false, |fl| self.is_reachable_flow(fl))
                        {
                            self.error_at(c.span, &gen::Fallthrough_case_in_switch, &[]);
                        }
                    }
                }
            }
            Stmt::Labeled {
                label, stmt: inner, ..
            } => {
                self.flow.label_stack.push(label.name.clone());
                self.check_statement(inner);
                self.flow.label_stack.pop();
            }
            Stmt::Import(i) => {
                if !self.stacks.fn_stack.is_empty() {
                    self.error_at(
                        Span::new(i.span.start as usize, i.span.start as usize + 6),
                        &gen::An_import_declaration_can_only_be_used_at_the_top_level_of_a_namespace_or_module,
                        &[],
                    );
                } else {
                    self.check_import(i)
                }
            }
            Stmt::ExportNamed(e) => {
                if e.module.is_none() {
                    for spec in &e.specifiers {
                        let local = spec.prop_name.as_ref().unwrap_or(&spec.name);
                        let resolvable =
                            self.lookup_value(self.current_scope, &local.name).is_some()
                                || self.lookup_type(self.current_scope, &local.name).is_some();
                        if !resolvable {
                            self.error_at(
                                local.span,
                                &gen::Cannot_find_name_0,
                                &[local.name.clone()],
                            );
                        }
                    }
                }
            }
            Stmt::Break { label, span } => {
                if let Some(l) = label {
                    if !self.flow.label_stack.contains(&l.name) {
                        self.error_at(
                            Span::new(span.start as usize, span.start as usize + 5),
                            &gen::A_break_statement_can_only_jump_to_a_label_of_an_enclosing_statement,
                            &[],
                        );
                    }
                } else if self.flow.loop_depth == 0 && self.flow.switch_depth == 0 {
                    self.error_at(
                        *span,
                        &gen::A_break_statement_can_only_be_used_within_an_enclosing_iteration_or_switch_statement,
                        &[],
                    );
                }
            }
            Stmt::Continue { label, span } => {
                if let Some(l) = label {
                    if !self.flow.label_stack.contains(&l.name) {
                        self.error_at(
                            Span::new(span.start as usize, span.start as usize + 8),
                            &gen::A_continue_statement_can_only_jump_to_a_label_of_an_enclosing_iteration_statement,
                            &[],
                        );
                    }
                } else if self.flow.loop_depth == 0 {
                    self.error_at(
                        *span,
                        &gen::A_continue_statement_can_only_be_used_within_an_enclosing_iteration_statement,
                        &[],
                    );
                }
            }
            Stmt::Empty { .. } | Stmt::Missing { .. } => {}
        }
    }

    /// 2717: subsequent (merged) property declarations must keep the same type
    fn check_iface_merge_conflicts(&mut self, sym: SymbolId, i: &'a InterfaceDecl) {
        let scope = self
            .bind
            .node_scope
            .get(&node_key(i))
            .copied()
            .unwrap_or(self.current_scope);
        for m in &i.members {
            let TypeMember::Prop(p) = m else { continue };
            let key = node_key(p);
            if self.bind.decl_symbol.contains_key(&key) {
                continue; // canonical declaration
            }
            if !self.report_once_node(2717, key) {
                continue;
            }
            let Some(name) = p.name.text() else { continue };
            let Some(member) = self.symbol(sym).members.get(&name) else {
                continue;
            };
            let first_t = self.type_of_symbol(member);
            let this_t = match &p.ty {
                Some(ty) => self.resolve_type_cached(ty, scope),
                None => self.types.any,
            };
            if first_t != this_t {
                let ft = self.display_type(first_t);
                let tt = self.display_type(this_t);
                self.error_at(
                    p.name.span(),
                    &gen::Subsequent_property_declarations_must_have_the_same_type_Property_0_must_be_of_type_1_but_here_has_type_2,
                    &[name, ft, tt],
                );
            }
        }
    }

    fn resolve_type_member_types(&mut self, m: &'a TypeMember, scope: ScopeId) {
        match m {
            TypeMember::Prop(p) => {
                if let Some(ty) = &p.ty {
                    self.resolve_type_cached(ty, scope);
                }
            }
            TypeMember::Method(ms) => {
                let scope = match ms.type_params.as_deref() {
                    Some(tps) => self.push_tp_scope(scope, tps),
                    None => scope,
                };
                for p in &ms.params {
                    if let Some(ty) = &p.ty {
                        self.resolve_type_cached(ty, scope);
                    }
                }
                if let Some(rt) = &ms.return_type {
                    self.resolve_type_cached(rt, scope);
                }
            }
            TypeMember::Call(cs) | TypeMember::Ctor(cs) => {
                let scope = match cs.type_params.as_deref() {
                    Some(tps) => self.push_tp_scope(scope, tps),
                    None => scope,
                };
                for p in &cs.params {
                    if let Some(ty) = &p.ty {
                        self.resolve_type_cached(ty, scope);
                    }
                }
                if let Some(rt) = &cs.return_type {
                    self.resolve_type_cached(rt, scope);
                }
            }
            TypeMember::Index(idx) => {
                self.resolve_type_cached(&idx.key_type, scope);
                self.resolve_type_cached(&idx.value_type, scope);
            }
        }
    }

    /// 2391 (implementation missing) / 2394 (overload incompatible with impl)
    fn check_overload_group(&mut self, f: &'a FunctionLike) {
        let Some(sym) = self.bind.decl_symbol.get(&node_key(f)).copied() else {
            return;
        };
        let group_key = sym.0 as usize | (1usize << 62);
        if self.checked_decls.contains(&group_key) {
            return;
        }
        self.checked_decls.insert(group_key);
        if self.symbol(sym).file == self.lib_file || self.in_dts() {
            return;
        }
        let decls: Vec<&'a FunctionLike> = self
            .symbol(sym)
            .decls
            .clone()
            .into_iter()
            .filter_map(|d| match d {
                Decl::Func(f) => Some(f),
                _ => None,
            })
            .collect();
        // 2384: signatures must be all ambient or non-ambient
        if decls.len() > 1 {
            let ambient: Vec<bool> = decls
                .iter()
                .map(|f| has_modifier(&f.modifiers, ModifierKind::Declare))
                .collect();
            if ambient.iter().any(|&a| a) && ambient.iter().any(|&a| !a) {
                if let Some(n) = decls[0].name_ident() {
                    self.error_at(
                        n.span,
                        &gen::Overload_signatures_must_all_be_ambient_or_non_ambient,
                        &[],
                    );
                }
            }
        }
        let overloads: Vec<&'a FunctionLike> = decls
            .iter()
            .copied()
            .filter(|f| {
                f.body.is_none()
                    && !has_modifier(&f.modifiers, ModifierKind::Declare)
                    && !has_modifier(&f.modifiers, ModifierKind::Abstract)
            })
            .collect();
        if overloads.is_empty() {
            return;
        }
        let impls: Vec<&'a FunctionLike> =
            decls.iter().copied().filter(|f| f.body.is_some()).collect();
        if impls.is_empty() {
            if let Some(name) = overloads[0].name_ident() {
                self.error_at(
                    name.span,
                    &gen::Function_implementation_is_missing_or_not_immediately_following_the_declaration,
                    &[],
                );
            }
            return;
        }
        let impl_sig = self.signature_of(impls[0]);
        for o in &overloads {
            let osig = self.signature_of(o);
            if !self.sig_assignable_for_overload(impl_sig, osig) {
                if let Some(name) = o.name_ident() {
                    self.error_at(
                        name.span,
                        &gen::This_overload_signature_is_not_compatible_with_its_implementation_signature,
                        &[],
                    );
                }
            }
        }
    }

    /// assigns types to destructuring-pattern bindings; reports 2339/2461/2493
    pub fn destructure_binding(&mut self, b: &'a Binding, source: TypeId) {
        if let Binding::Array(p) = b {
            let n = p.elements.len();
            for (i, el) in p.elements.iter().enumerate() {
                if let Some(el) = el {
                    if el.rest && i + 1 < n {
                        self.error_at(
                            el.binding.span(),
                            &gen::A_rest_element_must_be_last_in_a_destructuring_pattern,
                            &[],
                        );
                    }
                }
            }
        }
        self.destructure_binding_inner(b, source)
    }

    fn destructure_binding_inner(&mut self, b: &'a Binding, source: TypeId) {
        match b {
            Binding::Ident(id) => {
                if let Some(&sym) = self.bind.decl_symbol.get(&node_key(id)) {
                    self.caches.sym_type.insert(sym, source);
                }
            }
            Binding::Object(p) => {
                let src_err = self.types.is_any_or_error(source);
                for prop in &p.props {
                    let Some(key) = prop.key.text() else { continue };
                    let prop_t = if src_err {
                        Some(self.types.any)
                    } else {
                        self.prop_of_type(source, &key)
                    };
                    let t = match prop_t {
                        Some(t) => t,
                        None => {
                            let d = self.apparent_type_display(source);
                            self.error_at(
                                prop.key.span(),
                                &gen::Property_0_does_not_exist_on_type_1,
                                &[key, d],
                            );
                            self.types.error
                        }
                    };
                    let t = match &prop.default {
                        Some(dflt) => {
                            let dt = self.check_expr(dflt, None);
                            let dr = self.types.regular(dt);
                            let non_undef = self.non_nullable_undef_only(t);
                            self.types.union(vec![non_undef, dr])
                        }
                        None => t,
                    };
                    self.destructure_binding(&prop.binding, t);
                }
                if let Some(rest) = &p.rest {
                    let sreg = self.types.regular(source);
                    let obj_ok = self.types.is_any_or_error(sreg)
                        || matches!(
                            self.types.kind(sreg),
                            TypeKind::Anon(_)
                                | TypeKind::DeferredObj(_)
                                | TypeKind::Iface(_)
                                | TypeKind::Ref(..)
                        );
                    if !obj_ok {
                        self.error_at(
                            rest.span(),
                            &gen::Rest_types_may_only_be_created_from_object_types,
                            &[],
                        );
                    }
                    self.destructure_binding(rest, source);
                }
            }
            Binding::Array(p) => {
                if self.types.is_any_or_error(source) {
                    for el in p.elements.iter().flatten() {
                        self.destructure_binding(&el.binding, self.types.any);
                    }
                    return;
                }
                let arr_elem = self.array_element_type(source);
                let tuple_elems = match self.types.kind(source).clone() {
                    TypeKind::Tuple(e) | TypeKind::ReadonlyTuple(e) => Some(e),
                    _ => None,
                };
                if arr_elem.is_none() && tuple_elems.is_none() {
                    let d = self.display_type(source);
                    self.error_at(p.span, &gen::Type_0_is_not_an_array_type, &[d]);
                    for el in p.elements.iter().flatten() {
                        self.destructure_binding(&el.binding, self.types.error);
                    }
                    return;
                }
                for (i, el) in p.elements.iter().enumerate() {
                    let Some(el) = el else { continue };
                    let t = if el.rest {
                        // a rest binding captures the remaining tuple elements as
                        // a tuple (`[a, ...rest]: [number, string, boolean]` →
                        // `rest: [string, boolean]`); for an array source it is
                        // the array type itself.
                        if let Some(elems) = &tuple_elems {
                            let rest_elems: Vec<crate::types::TupleElem> =
                                elems.iter().skip(i).copied().collect();
                            self.types.tuple(rest_elems)
                        } else {
                            source
                        }
                    } else if let Some(elems) = &tuple_elems {
                        match elems.get(i) {
                            Some(e) => e.ty,
                            None => {
                                let d = self.display_type(source);
                                self.error_at(
                                    el.span,
                                    &gen::Tuple_type_0_of_length_1_has_no_element_at_index_2,
                                    &[d, elems.len().to_string(), i.to_string()],
                                );
                                self.types.error
                            }
                        }
                    } else {
                        let mut t = arr_elem.unwrap();
                        if self.options.no_unchecked_indexed_access {
                            t = self.types.union(vec![t, self.types.undefined]);
                        }
                        t
                    };
                    let t = match &el.default {
                        Some(dflt) => {
                            let dt = self.check_expr(dflt, None);
                            let dr = self.types.regular(dt);
                            let non_undef = self.non_nullable_undef_only(t);
                            self.types.union(vec![non_undef, dr])
                        }
                        None => t,
                    };
                    self.destructure_binding(&el.binding, t);
                }
            }
        }
    }

    fn non_nullable_undef_only(&mut self, t: TypeId) -> TypeId {
        self.types
            .filter_union(t, |tt, m| !matches!(tt.kind(m), TypeKind::Undefined))
    }

    /// ES decorator checking: missing context global (file-less 2318), then
    /// callability (1238/1240/1241 + chain) and the zero-arg heuristic (1329)
    pub(crate) fn check_decorator(
        &mut self,
        d: &'a Decorator,
        context_type: &str,
        kind: DecoratorKind,
    ) {
        let t = self.check_expr(&d.expr, None);
        if self.types.is_any_or_error(t) {
            return;
        }
        let sigs = self.call_signatures_of(t);
        if sigs.is_empty() {
            let head: &'static crate::diagnostics::DiagnosticMessage = match kind {
                DecoratorKind::Class => {
                    &gen::Unable_to_resolve_signature_of_class_decorator_when_called_as_an_expression
                }
                DecoratorKind::Property => {
                    &gen::Unable_to_resolve_signature_of_property_decorator_when_called_as_an_expression
                }
                DecoratorKind::Method => {
                    &gen::Unable_to_resolve_signature_of_method_decorator_when_called_as_an_expression
                }
            };
            let mut chain = crate::diagnostics::MessageChain::new(head, &[]);
            let mut not_callable =
                crate::diagnostics::MessageChain::new(&gen::This_expression_is_not_callable, &[]);
            let dsp = self.apparent_type_display(t);
            not_callable
                .next
                .push(crate::diagnostics::MessageChain::new(
                    &gen::Type_0_has_no_call_signatures,
                    &[dsp],
                ));
            chain.next.push(not_callable);
            self.error_chain_at(d.expr.span(), chain);
            return;
        }
        // `@f` where f takes zero parameters → 1329 (did you mean @f()?)
        let s = self.types.sig(sigs[0]).clone();
        if s.params.is_empty() && s.rest.is_none() {
            if let Expr::Ident(id) = &d.expr {
                self.error_at(
                    d.span,
                    &gen::_0_accepts_too_few_arguments_to_be_used_as_a_decorator_here_Did_you_mean_to_call_it_first_and_write_0,
                    &[id.name.clone()],
                );
                return;
            }
        }
        // signature application reaches the decorator context type, which our
        // lib doesn't declare (tsc reports a file-less 2318 once per type)
        if self.global_type_symbol(context_type).is_none()
            && self
                .reported
                .reported_missing_globals
                .insert(context_type.to_string())
        {
            self.diags.push(crate::diagnostics::Diagnostic {
                file: None,
                start: 0,
                length: 0,
                message: crate::diagnostics::MessageChain::new(
                    &gen::Cannot_find_global_type_0,
                    &[context_type.to_string()],
                ),
                related: Vec::new(),
            });
        }
    }

    pub fn check_duplicate_modifiers(&mut self, mods: &Modifiers) {
        for (i, m) in mods.iter().enumerate() {
            // accessibility duplicates report TS1028 elsewhere
            if matches!(
                m.kind,
                ModifierKind::Public | ModifierKind::Private | ModifierKind::Protected
            ) {
                continue;
            }
            if mods[..i].iter().any(|p| p.kind == m.kind) {
                self.error_at(
                    m.span,
                    &gen::_0_modifier_already_seen,
                    &[modifier_text(m.kind).to_string()],
                );
            }
        }
    }

    /// 2374: at most one index signature per key type
    /// 1070: visibility/static modifiers on type members; 7013 construct sigs
    /// Whether a type may be an index-signature parameter type: `string`,
    /// `number`, `symbol`, a template-literal pattern, or a (non-generic)
    /// intersection with at least one such member (`string & Brand`).
    pub(crate) fn is_valid_index_key_type(&self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::String
            | TypeKind::Number
            | TypeKind::EsSymbol
            | TypeKind::TemplateLit(_)
            | TypeKind::Error => true,
            TypeKind::Intersection(ms) => {
                let ms = ms.clone();
                ms.iter().any(|&m| self.is_valid_index_key_type(m))
            }
            TypeKind::Union(ms) => {
                let ms = ms.clone();
                !ms.is_empty() && ms.iter().all(|&m| self.is_valid_index_key_type(m))
            }
            _ => false,
        }
    }

    pub fn check_type_member_implicit_any(&mut self, members: &'a [TypeMember]) {
        self.check_type_member_grammar_impl(members, true);
    }

    pub fn check_type_member_grammar(&mut self, members: &'a [TypeMember]) {
        self.check_type_member_grammar_impl(members, false);
    }

    fn check_type_member_grammar_impl(
        &mut self,
        members: &'a [TypeMember],
        implicit_any_only: bool,
    ) {
        let mut implicit_any_prop_names = std::collections::HashSet::new();
        for m in members {
            match m {
                TypeMember::Prop(p) => {
                    let implicit_any_name = self.type_member_implicit_any_name(&p.name);
                    let first_implicit_any_name = implicit_any_name
                        .as_ref()
                        .map(|n| implicit_any_prop_names.insert(n.clone()))
                        .unwrap_or(false);
                    if p.ty.is_none() && first_implicit_any_name {
                        if let Some(n) = implicit_any_name {
                            if self.options.no_implicit_any() {
                                self.error_at(
                                    p.name.span(),
                                    &gen::Member_0_implicitly_has_an_1_type,
                                    &[n, "any".to_string()],
                                );
                            } else {
                                self.suggestion_at(
                                    p.name.span(),
                                    &gen::Member_0_implicitly_has_an_1_type_but_a_better_type_may_be_inferred_from_usage,
                                    &[n, "any".to_string()],
                                );
                            }
                        }
                    }
                    if !implicit_any_only {
                        for im in &p.illegal_modifiers {
                            self.error_at(
                                im.span,
                                &gen::_0_modifier_cannot_appear_on_a_type_member,
                                &[modifier_text(im.kind).to_string()],
                            );
                        }
                    }
                }
                TypeMember::Method(ms) => {
                    if ms.return_type.is_none() {
                        if let Some(n) = self.type_member_implicit_any_name(&ms.name) {
                            self.report_implicit_any_return_named(ms.span, n);
                        }
                    }
                    for p in &ms.params {
                        self.report_implicit_any_param(p);
                    }
                    for (i, p) in ms.params.iter().enumerate() {
                        if p.ty.is_none() {
                            if let Some(id) = p.name.as_ident() {
                                if matches!(
                                    id.name.as_str(),
                                    "string"
                                        | "number"
                                        | "boolean"
                                        | "symbol"
                                        | "object"
                                        | "any"
                                        | "unknown"
                                        | "never"
                                        | "bigint"
                                ) {
                                    self.error_at(
                                        id.span,
                                        &gen::Parameter_has_a_name_but_no_type_Did_you_mean_0_Colon_1,
                                        &[format!("arg{}", i), id.name.clone()],
                                    );
                                }
                            }
                        }
                    }
                }
                TypeMember::Call(c) => {
                    if c.return_type.is_none() && self.options.no_implicit_any() {
                        self.error_at(
                            Span::new(c.span.start as usize, c.span.start as usize + 4),
                            &gen::Call_signature_which_lacks_return_type_annotation_implicitly_has_an_any_return_type,
                            &[],
                        );
                    }
                    for p in &c.params {
                        self.report_implicit_any_param(p);
                    }
                }
                TypeMember::Ctor(c) => {
                    if c.return_type.is_none() && self.options.no_implicit_any() {
                        self.error_at(
                            Span::new(c.span.start as usize, c.span.start as usize + 3),
                            &gen::Construct_signature_which_lacks_return_type_annotation_implicitly_has_an_any_return_type,
                            &[],
                        );
                    }
                    for p in &c.params {
                        self.report_implicit_any_param(p);
                    }
                }
                TypeMember::Index(ix) => {
                    if implicit_any_only {
                        continue;
                    }
                    if let Some(s) = ix.declare_span {
                        self.error_at(
                            s,
                            &gen::_0_modifier_cannot_appear_on_an_index_signature,
                            &["declare".to_string()],
                        );
                    }
                    if let Some(s) = ix.rest_span {
                        self.error_at(
                            s,
                            &gen::An_index_signature_cannot_have_a_rest_parameter,
                            &[],
                        );
                    }
                    if let Some(s) = ix.modifier_span {
                        self.error_at(
                            s,
                            &gen::A_parameter_property_is_only_allowed_in_a_constructor_implementation,
                            &[],
                        );
                        self.error_at(
                            ix.param_name.span,
                            &gen::An_index_signature_parameter_cannot_have_an_accessibility_modifier,
                            &[],
                        );
                    }
                    if let Some(s) = ix.question_span {
                        self.error_at(
                            s,
                            &gen::An_index_signature_parameter_cannot_have_a_question_mark,
                            &[],
                        );
                    }
                    if ix.missing_value {
                        self.error_at(
                            Span::new(ix.span.start as usize, ix.span.start as usize + 1),
                            &gen::An_index_signature_must_have_a_type_annotation,
                            &[],
                        );
                    }
                    if ix.rest_span.is_some()
                        || ix.modifier_span.is_some()
                        || ix.question_span.is_some()
                    {
                        continue;
                    }
                    let scope = self.current_scope;
                    let kt = self.resolve_type(&ix.key_type, scope);
                    let ok = self.is_valid_index_key_type(kt);
                    if !ok {
                        self.error_at(
                            ix.param_name.span,
                            &gen::An_index_signature_parameter_type_must_be_string_number_symbol_or_a_template_literal_type,
                            &[],
                        );
                    }
                }
            }
        }
    }

    fn type_member_implicit_any_name(&self, name: &PropName) -> Option<String> {
        match name {
            PropName::Computed { .. } => {
                let display = self.display_prop_name_for_error(name);
                (!display.is_empty()).then_some(display)
            }
            _ => name.text(),
        }
    }

    /// 2411/2413: members must be compatible with index signatures
    pub fn check_index_compatibility(&mut self, members: &'a [TypeMember], scope: ScopeId) {
        let mut str_index: Option<TypeId> = None;
        let mut num_index: Option<(TypeId, Span)> = None;
        for m in members {
            if let TypeMember::Index(ix) = m {
                let kt = self.resolve_type(&ix.key_type, scope);
                let vt = self.resolve_type(&ix.value_type, scope);
                if matches!(self.types.kind(kt), TypeKind::Number) {
                    num_index = Some((vt, ix.span));
                } else {
                    str_index = Some(vt);
                }
            }
        }
        if str_index.is_none() && num_index.is_none() {
            return;
        }
        for m in members {
            if let TypeMember::Prop(p) = m {
                let Some(ty) = &p.ty else { continue };
                let Some(pn) = p.name.text() else { continue };
                let pt = self.resolve_type(ty, scope);
                let is_numeric_name = pn.parse::<f64>().is_ok();
                let applicable = if is_numeric_name {
                    num_index.map(|(t, _)| t).or(str_index)
                } else {
                    str_index
                };
                if let Some(it) = applicable {
                    if !self.types.is_any_or_error(pt) && !self.is_assignable_to(pt, it) {
                        let pd = self.display_type(pt);
                        let id = self.display_type(it);
                        let kind_label = if is_numeric_name && num_index.is_some() {
                            "number"
                        } else {
                            "string"
                        };
                        let display_name = self.display_prop_name_for_error(&p.name);
                        self.error_at(
                            p.name.span(),
                            &gen::Property_0_of_type_1_is_not_assignable_to_2_index_type_3,
                            &[display_name, pd, kind_label.to_string(), id],
                        );
                    }
                }
            }
        }
        if let (Some(st), Some((nt, nspan))) = (str_index, num_index) {
            if !self.is_assignable_to(nt, st) {
                let nd = self.display_type(nt);
                let sd = self.display_type(st);
                self.error_at(
                    nspan,
                    &gen::_0_index_type_1_is_not_assignable_to_2_index_type_3,
                    &["number".to_string(), nd, "string".to_string(), sd],
                );
            }
        }
    }

    pub fn check_duplicate_index_signatures(&mut self, members: &'a [TypeMember]) {
        let mut string_sigs: Vec<Span> = Vec::new();
        let mut number_sigs: Vec<Span> = Vec::new();
        for m in members {
            if let TypeMember::Index(ix) = m {
                let scope = self.current_scope;
                let kt = self.resolve_type(&ix.key_type, scope);
                if matches!(self.types.kind(kt), TypeKind::Number) {
                    number_sigs.push(ix.span);
                } else {
                    string_sigs.push(ix.span);
                }
            }
        }
        for (sigs, label) in [(string_sigs, "string"), (number_sigs, "number")] {
            if sigs.len() > 1 {
                for s in sigs {
                    self.error_at(
                        s,
                        &gen::Duplicate_index_signature_for_type_0,
                        &[label.to_string()],
                    );
                }
            }
        }
    }

    pub(crate) fn check_member_modifiers_ext(
        &mut self,
        mods: &Modifiers,
        kind: MemberKind,
        class_is_abstract: bool,
    ) {
        self.check_duplicate_modifiers(mods);
        let _ = kind;
        self.check_member_modifiers_priv(mods);
        if kind != MemberKind::Property {
            if let Some(m) = mods.iter().find(|m| m.kind == ModifierKind::Declare) {
                self.error_at(
                    m.span,
                    &gen::_0_modifier_cannot_appear_on_class_elements_of_this_kind,
                    &["declare".to_string()],
                );
            }
        }
        if !class_is_abstract {
            if let Some(m) = mods.iter().find(|m| m.kind == ModifierKind::Abstract) {
                self.error_at(
                    m.span,
                    &gen::Abstract_methods_can_only_appear_within_an_abstract_class,
                    &[],
                );
            }
        }
        if kind == MemberKind::Accessor {
            if let Some(m) = mods.iter().find(|m| m.kind == ModifierKind::Async) {
                self.error_at(
                    m.span,
                    &gen::_0_modifier_cannot_be_used_here,
                    &["async".to_string()],
                );
            }
        }
        if let Some(am) = mods.iter().find(|m| m.kind == ModifierKind::Abstract) {
            for bad in [ModifierKind::Private, ModifierKind::Static] {
                if mods.iter().any(|m| m.kind == bad) {
                    self.error_at(
                        am.span,
                        &gen::_0_modifier_cannot_be_used_with_1_modifier,
                        &[modifier_text(bad).to_string(), "abstract".to_string()],
                    );
                }
            }
        }
        self.check_member_modifiers(mods, kind);
    }

    fn check_member_modifiers_priv(&mut self, _mods: &Modifiers) {}

    fn check_member_modifiers(&mut self, mods: &Modifiers, kind: MemberKind) {
        let mut seen_accessibility = false;
        let mut seen_static = false;
        let mut seen_readonly = false;
        let mut seen: Vec<ModifierKind> = Vec::new();
        for m in mods {
            let text = modifier_text(m.kind);
            let is_acc = matches!(
                m.kind,
                ModifierKind::Public | ModifierKind::Private | ModifierKind::Protected
            );
            if kind == MemberKind::Ctor
                && matches!(
                    m.kind,
                    ModifierKind::Static
                        | ModifierKind::Async
                        | ModifierKind::Abstract
                        | ModifierKind::Readonly
                        | ModifierKind::Override
                )
            {
                self.error_at(
                    m.span,
                    &gen::_0_modifier_cannot_appear_on_a_constructor_declaration,
                    &[text.into()],
                );
                continue;
            }
            if is_acc {
                if seen_accessibility {
                    self.error_at(m.span, &gen::Accessibility_modifier_already_seen, &[]);
                } else if seen_static {
                    self.error_at(
                        m.span,
                        &gen::_0_modifier_must_precede_1_modifier,
                        &[text.into(), "static".into()],
                    );
                } else if seen_readonly {
                    self.error_at(
                        m.span,
                        &gen::_0_modifier_must_precede_1_modifier,
                        &[text.into(), "readonly".into()],
                    );
                }
                seen_accessibility = true;
            } else if seen.contains(&m.kind) {
                self.error_at(m.span, &gen::_0_modifier_already_seen, &[text.into()]);
            }
            match m.kind {
                ModifierKind::Static => {
                    if seen_readonly {
                        self.error_at(
                            m.span,
                            &gen::_0_modifier_must_precede_1_modifier,
                            &["static".into(), "readonly".into()],
                        );
                    }
                    seen_static = true;
                }
                ModifierKind::Readonly => {
                    if kind == MemberKind::Method || kind == MemberKind::Accessor {
                        self.error_at(
                            m.span,
                            &gen::readonly_modifier_can_only_appear_on_a_property_declaration_or_index_signature,
                            &[],
                        );
                    }
                    seen_readonly = true;
                }
                _ => {}
            }
            seen.push(m.kind);
        }
    }

    pub fn resolve_type_cached(&mut self, ty: &'a TypeNode, scope: ScopeId) -> TypeId {
        let key = ty as *const TypeNode as usize;
        if let Some(&t) = self.caches.node_type_cache.get(&key) {
            return t;
        }
        let t = self.resolve_type(ty, scope);
        self.caches.node_type_cache.insert(key, t);
        t
    }

    /// 2320: bases contribute non-identical same-name members the interface
    /// itself doesn't redeclare. Returns true when reported (suppresses 2430).
    fn check_interface_base_conflicts(&mut self, i: &'a InterfaceDecl, scope: ScopeId) -> bool {
        let exts = &i.extends;
        if exts.len() < 2 {
            return false;
        }
        let own: std::collections::HashSet<String> = i
            .members
            .iter()
            .filter_map(|m| match m {
                TypeMember::Prop(p) => p.name.text(),
                TypeMember::Method(ms) => ms.name.text(),
                _ => None,
            })
            .collect();
        let bases: Vec<(TypeId, String)> = exts
            .iter()
            .map(|e| {
                let t = self.resolve_type_cached_ref(e, scope);
                let d = self.display_type(t);
                (t, d)
            })
            .collect();
        for ai in 0..bases.len() {
            for bi in ai + 1..bases.len() {
                let (at, an) = (&bases[ai].0, &bases[ai].1);
                let (bt, bn) = (&bases[bi].0, &bases[bi].1);
                let (Some(ash), Some(bsh)) = (self.shape_of_type(*at), self.shape_of_type(*bt))
                else {
                    continue;
                };
                let a_props = self.types.shape(ash).props.clone();
                let b_shape = self.types.shape(bsh).clone();
                for ap in &a_props {
                    if own.contains(&ap.name) {
                        continue;
                    }
                    if let Some(bp) = b_shape.prop(&ap.name) {
                        let identical = self.is_assignable_to(ap.ty, bp.ty)
                            && self.is_assignable_to(bp.ty, ap.ty);
                        if !identical {
                            let iname =
                                self.generic_name_with_params(self.bind.decl_symbol[&node_key(i)]);
                            let mut chain = crate::diagnostics::MessageChain::new(
                                &gen::Interface_0_cannot_simultaneously_extend_types_1_and_2,
                                &[iname, an.clone(), bn.clone()],
                            );
                            chain.next.push(crate::diagnostics::MessageChain::new(
                                &gen::Named_property_0_of_types_1_and_2_are_not_identical,
                                &[ap.name.clone(), an.clone(), bn.clone()],
                            ));
                            self.error_chain_at(i.name.span, chain);
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn check_interface_extends(&mut self, i: &'a InterfaceDecl, ext: &'a TypeRef, scope: ScopeId) {
        // primitive keywords in heritage position (2840)
        if ext.name.parts.len() == 1 && ext.type_args.is_none() {
            let n = ext.name.parts[0].name.as_str();
            let prim = match n {
                "string" => Some(self.types.string),
                "number" => Some(self.types.number),
                "boolean" => Some(self.types.boolean),
                "bigint" => Some(self.types.bigint),
                "symbol" => Some(self.types.es_symbol),
                _ => None,
            };
            if let Some(p) = prim {
                let d = self.display_type(p);
                self.error_at(
                    ext.span,
                    &gen::An_interface_cannot_extend_a_primitive_type_like_0_It_can_only_extend_other_named_object_types,
                    &[d],
                );
                return;
            }
        }
        let base = self.resolve_type_cached_ref(ext, scope);
        if self.types.is_error(base) {
            return;
        }
        // primitives cannot be extended (2840)
        if matches!(
            self.types.kind(base),
            TypeKind::String | TypeKind::Number | TypeKind::Bigint | TypeKind::EsSymbol
        ) {
            let d = self.display_type(base);
            self.error_at(
                ext.span,
                &gen::An_interface_cannot_extend_a_primitive_type_like_0_It_can_only_extend_other_named_object_types,
                &[d],
            );
            return;
        }
        let derived_sym = self.bind.decl_symbol.get(&node_key(i)).copied();
        let Some(dsym) = derived_sym else { return };
        let derived = self.types.intern_kind(TypeKind::Iface(dsym));
        if !self.is_assignable_to(derived, base) {
            self.rel.keep_head_for_missing = false;
            let dn = self.generic_name_with_params(dsym);
            let bn = self.display_type(base);
            self.report_relation_failure(
                derived,
                base,
                i.name.span,
                Some((
                    &gen::Interface_0_incorrectly_extends_interface_1,
                    vec![dn, bn],
                )),
            );
        }
    }

    pub(crate) fn resolve_type_cached_ref(&mut self, r: &'a TypeRef, scope: ScopeId) -> TypeId {
        // reuse the Ref resolution path through a transient TypeNode-like call
        let key = r as *const TypeRef as usize;
        if let Some(&t) = self.caches.node_type_cache.get(&key) {
            return t;
        }
        let t = self.resolve_type_ref_pub(r, scope);
        self.caches.node_type_cache.insert(key, t);
        t
    }

    pub fn check_enum_pub(&mut self, e: &'a EnumDecl) {
        self.check_enum(e);
    }

    fn check_enum_merge_initializers(&mut self, e: &'a EnumDecl, sym: SymbolId) {
        let decls: Vec<usize> = self
            .symbol(sym)
            .decls
            .iter()
            .filter_map(|d| match d {
                crate::binder::Decl::Enum(ed) => Some(node_key(*ed)),
                _ => None,
            })
            .collect();
        if decls.len() < 2 || decls.first() == Some(&node_key(e)) {
            return;
        }
        if let Some(first) = e.members.first() {
            if first.init.is_none() {
                self.error_at(
                    first.name.span(),
                    &gen::In_an_enum_with_multiple_declarations_only_one_declaration_can_omit_an_initializer_for_its_first_enum_element,
                    &[],
                );
            }
        }
    }

    fn check_enum(&mut self, e: &'a EnumDecl) {
        const RESERVED_ENUM: &[&str] = &[
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
        if RESERVED_ENUM.contains(&e.name.name.as_str()) {
            self.error_at(
                e.name.span,
                &gen::Enum_name_cannot_be_0,
                &[e.name.name.clone()],
            );
        }
        if self.options.erasable_syntax_only && !has_modifier(&e.modifiers, ModifierKind::Declare) {
            self.error_at(
                e.name.span,
                &gen::This_syntax_is_not_allowed_when_erasableSyntaxOnly_is_enabled,
                &[],
            );
        }
        let key = node_key(e);
        if self.checked_decls.contains(&key) {
            return;
        }
        self.checked_decls.insert(key);
        let Some(_sym) = self.bind.decl_symbol.get(&key).copied() else {
            return;
        };
        // value assignment pass: auto-increment, string members, computed checks
        let mut prev_numeric: Option<f64> = Some(-1.0);
        if let Some(&sym) = self.bind.decl_symbol.get(&node_key(e)) {
            self.check_enum_merge_initializers(e, sym);
            // 2567: const/non-const enum declarations cannot merge
            if self.report_once_sym(2567, sym) {
                let decls: Vec<(&'a EnumDecl, Span)> = self
                    .symbol(sym)
                    .decls
                    .iter()
                    .filter_map(|d| match d {
                        crate::binder::Decl::Enum(ed) => Some((*ed, ed.name.span)),
                        _ => None,
                    })
                    .collect();
                if decls.len() > 1 && !decls.iter().all(|(d, _)| d.is_const == decls[0].0.is_const)
                {
                    for (_, span) in decls {
                        self.error_at(
                            span,
                            &gen::Enum_declarations_can_only_merge_with_namespace_or_other_enum_declarations,
                            &[],
                        );
                    }
                }
            }
        }
        for m in &e.members {
            if let Some(n) = m.name.text() {
                if n.parse::<f64>().is_ok() {
                    self.error_at(
                        m.name.span(),
                        &gen::An_enum_member_cannot_have_a_numeric_name,
                        &[],
                    );
                }
            }
        }
        for m in &e.members {
            if let PropName::Computed { span, .. } = &m.name {
                self.error_at(
                    *span,
                    &gen::Computed_property_names_are_not_allowed_in_enums,
                    &[],
                );
            }
            let Some(mname) = m.name.text() else { continue };
            let msym = self.bind.decl_symbol.get(&node_key(m)).copied();
            match &m.init {
                None => match prev_numeric {
                    Some(prev) => {
                        let value = prev + 1.0;
                        prev_numeric = Some(value);
                        if let Some(msym) = msym {
                            self.enums
                                .enum_member_values
                                .insert(msym, EnumValue::Number(value));
                        }
                    }
                    None => {
                        self.error_at(m.name.span(), &gen::Enum_member_must_have_initializer, &[]);
                        prev_numeric = None;
                    }
                },
                Some(init) => {
                    match self.const_eval_enum_init(init) {
                        Some(EnumValue::Number(v)) => {
                            prev_numeric = Some(v);
                            if let Some(msym) = msym {
                                self.enums
                                    .enum_member_values
                                    .insert(msym, EnumValue::Number(v));
                            }
                        }
                        Some(EnumValue::Str(s)) => {
                            prev_numeric = None;
                            if let Some(msym) = msym {
                                self.enums
                                    .enum_member_values
                                    .insert(msym, EnumValue::Str(s));
                            }
                        }
                        Some(EnumValue::Computed) | None => {
                            if has_modifier(&e.modifiers, ModifierKind::Declare) {
                                self.error_at(
                                    init.span(),
                                    &gen::In_ambient_enum_declarations_member_initializer_must_be_constant_expression,
                                    &[],
                                );
                            }
                            // const enums require constant initializers
                            if e.is_const {
                                self.error_at(
                                    init.span(),
                                    &gen::const_enum_member_initializers_must_be_constant_expressions,
                                    &[],
                                );
                            }
                            // computed member: its value type must be number
                            let t = self.check_expr(init, None);
                            let r = self.types.regular(t);
                            let num = self.types.number;
                            if !self.types.is_any_or_error(r) && !self.is_assignable_to(r, num) {
                                let d = self.display_type(r);
                                self.error_at(
                                    init.span(),
                                    &gen::Type_0_is_not_assignable_to_type_1_as_required_for_computed_enum_member_values,
                                    &[d, "number".to_string()],
                                );
                            }
                            prev_numeric = None;
                            if let Some(msym) = msym {
                                self.enums
                                    .enum_member_values
                                    .insert(msym, EnumValue::Computed);
                            }
                        }
                    }
                }
            }
            let _ = mname;
        }
    }

    fn const_eval_enum_init(&mut self, e: &'a Expr) -> Option<EnumValue> {
        match e {
            Expr::NumLit { value, .. } => Some(EnumValue::Number(*value)),
            Expr::StrLit { value, .. } => Some(EnumValue::Str(value.to_str_lossy().into_owned())),
            Expr::Unary {
                op: UnaryOp::Minus,
                operand,
                ..
            } => match self.const_eval_enum_init(operand)? {
                EnumValue::Number(v) => Some(EnumValue::Number(-v)),
                _ => None,
            },
            Expr::Paren { inner, .. } => self.const_eval_enum_init(inner),
            Expr::Binary {
                op, left, right, ..
            } => {
                let l = self.const_eval_enum_init(left)?;
                let r = self.const_eval_enum_init(right)?;
                if let (EnumValue::Number(a), EnumValue::Number(b)) = (l, r) {
                    let v = match op {
                        BinOp::Add => a + b,
                        BinOp::Sub => a - b,
                        BinOp::Mul => a * b,
                        BinOp::Shl => ((a as i64) << (b as i64)) as f64,
                        BinOp::BitOr => ((a as i64) | (b as i64)) as f64,
                        _ => return None,
                    };
                    Some(EnumValue::Number(v))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn check_var_stmt(&mut self, v: &'a VarStmt, in_for: bool) {
        let is_declare =
            has_modifier(&v.modifiers, ModifierKind::Declare) || self.in_ambient_context();
        for d in &v.decls {
            let key = node_key(d);
            if self.checked_decls.contains(&key) {
                continue;
            }
            self.checked_decls.insert(key);
            // `const x: number;` with no initializer (outside ambient contexts and
            // for-in/of headers) is TS1155. Destructuring patterns get TS1182 below.
            if matches!(v.kind, VarKind::Const) && d.init.is_none() && !is_declare && !in_for {
                if let Binding::Ident(id) = &d.name {
                    self.error_at(
                        id.span,
                        &gen::_0_declarations_must_be_initialized,
                        &["const".to_string()],
                    );
                }
            }
            if let Some(exclam_span) = d.exclam_span {
                if d.init.is_some() {
                    self.error_at(
                        exclam_span,
                        &gen::Declarations_with_initializers_cannot_also_have_definite_assignment_assertions,
                        &[],
                    );
                } else if d.ty.is_none() {
                    self.error_at(
                        exclam_span,
                        &gen::Declarations_with_definite_assignment_assertions_must_also_have_type_annotations,
                        &[],
                    );
                } else if is_declare {
                    self.error_at(
                        exclam_span,
                        &gen::A_definite_assignment_assertion_is_not_permitted_in_this_context,
                        &[],
                    );
                }
            }
            if !self.options.no_implicit_any()
                && d.ty.is_none()
                && d.init.is_none()
                && self.is_canonical_decl_key(key)
            {
                if let Binding::Ident(id) = &d.name {
                    self.suggestion_at(
                        id.span,
                        &gen::Variable_0_implicitly_has_an_1_type_but_a_better_type_may_be_inferred_from_usage,
                        &[id.name.clone(), "any".to_string()],
                    );
                }
            }
            let declared =
                d.ty.as_ref()
                    .map(|ty| self.resolve_type_cached(ty, self.current_scope));
            if let Some(init) = &d.init {
                let it = self.check_expr(init, declared);
                // record `const c = <type-guard>` for aliased-condition narrowing
                if matches!(v.kind, VarKind::Const) {
                    if let Binding::Ident(_) = &d.name {
                        if d.ty.is_none() && Self::is_guard_like_expr(init) {
                            if let Some(sym) = self.bind.decl_symbol.get(&key).copied() {
                                self.cond_aliases.insert(sym, init);
                            }
                        }
                    }
                }
                if let Some(dt) = declared {
                    // tsc checkVariableLikeDeclaration: the initializer must be
                    // assignable to the declared type. Elaboration (via the
                    // initializer expr) relocates the error onto the offending
                    // object/array-literal member when applicable; otherwise it
                    // lands on the binding name (matching tsc's error node).
                    if !self.types.is_error(dt) {
                        if matches!(d.ty.as_ref(), Some(TypeNode::Intersection { .. })) {
                            // intersection targets report the assignment head
                            // (TS2322) with the missing/incompatible member
                            // elaborated underneath, matching tsc (which never
                            // collapses `A & B` to a bare object type).
                            let sd = self.display_type_for_error(it, dt);
                            let td = self.display_type(dt);
                            let prev = self.rel.keep_head_for_missing;
                            self.rel.keep_head_for_missing = true;
                            self.check_assignable(
                                it,
                                dt,
                                d.name.span(),
                                Some((&gen::Type_0_is_not_assignable_to_type_1, vec![sd, td])),
                                Some(init),
                            );
                            self.rel.keep_head_for_missing = prev;
                        } else {
                            self.check_assignable(it, dt, d.name.span(), None, Some(init));
                        }
                    }
                    // declaration narrowing (`let x: U = init` reducing the
                    // union declared type) lives in the resolver's Init arm
                    let source = declared.unwrap_or_else(|| {
                        let r = self.types.regular(it);
                        self.types.widen_literal(r)
                    });
                    self.destructure_binding(&d.name, source);
                }
                // an un-annotated destructuring pattern (`const [a] = arr`)
                // takes its element types from the (widened) initializer type;
                // without this the bindings would be left as implicit `any`.
                if declared.is_none() && !matches!(&d.name, Binding::Ident(_)) {
                    let r = self.types.regular(it);
                    let source = self.types.widen_literal(r);
                    self.destructure_binding(&d.name, source);
                }
            } else if let (Some(dt), false) = (declared, matches!(&d.name, Binding::Ident(_))) {
                self.destructure_binding(&d.name, dt);
            }
            // strict-mode declaration names: eval/arguments (1100/1210/1215,
            // tsc checkAmbientInitializer: a const without a type annotation
            // destructuring declarations need an initializer (1182)
            if !matches!(&d.name, Binding::Ident(_)) && d.init.is_none() && d.ty.is_none() {
                self.error_at(
                    d.name.span(),
                    &gen::A_destructuring_declaration_must_have_an_initializer,
                    &[],
                );
                if self.options.no_implicit_any() {
                    let mut leaves: Vec<&crate::ast::Ident> = Vec::new();
                    super::exprs::collect_binding_idents_pub(&d.name, &mut leaves);
                    for id in leaves {
                        self.error_at(
                            id.span,
                            &gen::Binding_element_0_implicitly_has_an_1_type,
                            &[id.name.clone(), "any".to_string()],
                        );
                    }
                }
            }
            // circular initializer (7022)
            if let Some(sym) = self.bind.decl_symbol.get(&key).copied() {
                if d.init.is_some() {
                    self.symuse.assigned_symbols.insert(sym);
                }
                let _ = self.type_of_symbol(sym);
                if self
                    .res
                    .resolution_failed
                    .remove(&(sym, super::Slot::ValueType))
                {
                    if d.ty.is_some() {
                        // annotation references itself (typeof x in own type)
                        let name = self.symbol(sym).name.clone();
                        self.error_at(
                            d.name.span(),
                            &gen::_0_is_referenced_directly_or_indirectly_in_its_own_type_annotation,
                            &[name],
                        );
                    } else if self.options.no_implicit_any() {
                        let name = self.symbol(sym).name.clone();
                        self.error_at(
                            d.name.span(),
                            &gen::_0_implicitly_has_type_any_because_it_does_not_have_a_type_annotation_and_is_referenced_directly_or_indirectly_in_its_own_initializer,
                            &[name],
                        );
                    }
                }
                if self
                    .res
                    .resolution_failed
                    .remove(&(sym, super::Slot::ValueType))
                {
                    if d.ty.is_some() {
                        // annotation references itself (typeof x in own type)
                        let name = self.symbol(sym).name.clone();
                        self.error_at(
                            d.name.span(),
                            &gen::_0_is_referenced_directly_or_indirectly_in_its_own_type_annotation,
                            &[name],
                        );
                    } else if self.options.no_implicit_any() {
                        let name = self.symbol(sym).name.clone();
                        self.error_at(
                            d.name.span(),
                            &gen::_0_implicitly_has_type_any_because_it_does_not_have_a_type_annotation_and_is_referenced_directly_or_indirectly_in_its_own_initializer,
                            &[name],
                        );
                    }
                }
                // 2403: subsequent var declarations must have the same type
                self.check_subsequent_var_decl(sym, d, v.kind);
            }
        }
    }

    fn check_subsequent_var_decl(&mut self, sym: SymbolId, d: &'a VarDeclarator, kind: VarKind) {
        if kind != VarKind::Var {
            return;
        }
        let decls = self.symbol(sym).decls.clone();
        if decls.len() < 2 {
            return;
        }
        // first decl's type is canonical
        let first = decls
            .iter()
            .find_map(|dd| match dd {
                Decl::Var(vd, _) => Some(*vd),
                _ => None,
            })
            .unwrap();
        if std::ptr::eq(first, d) {
            return;
        }
        let first_t = {
            let saved = self.caches.sym_type.remove(&sym);
            // re-derive the FIRST declarator's type: its initializer's reads
            // key the ORIGINAL nodes, so the flow walk reproduces the
            // first-pass narrowing no matter where this re-check runs (the
            // old `fresolve.suppress` wrap was a dark-launch artifact — it
            // sent these reads to the lexical fact fallback, which described
            // THIS declarator's position instead)
            let t = self.declared_var_type(first);
            if let Some(s) = saved {
                self.caches.sym_type.insert(sym, s);
            }
            t
        };
        let this_t = self.declared_var_type(d);
        if first_t != this_t {
            let name = self.symbol(sym).name.clone();
            let ft = self.display_type(first_t);
            let tt = self.display_type(this_t);
            let related = self
                .related_on_symbol_decl(sym, &gen::_0_was_also_declared_here, &[name.clone()])
                .into_iter()
                .collect();
            self.error_at_with_related(
                d.name.span(),
                &gen::Subsequent_variable_declarations_must_have_the_same_type_Variable_0_must_be_of_type_1_but_here_has_type_2,
                &[name, ft, tt],
                related,
            );
        }
    }

    fn declared_var_type(&mut self, d: &'a VarDeclarator) -> TypeId {
        if let Some(ty) = &d.ty {
            return self.resolve_type_cached(ty, self.current_scope);
        }
        if let Some(init) = &d.init {
            let t = self.check_expr(init, None);
            let r = self.types.regular(t);
            return self.types.widen_literal(r);
        }
        self.types.any
    }

    pub(crate) fn in_dts(&self) -> bool {
        self.files[self.current_file].0.ends_with(".d.ts")
    }

    pub(crate) fn in_ambient_context(&self) -> bool {
        self.in_dts() || self.cflags.ambient_context_depth > 0
    }

    fn check_return(&mut self, expr: Option<&'a Expr>, span: Span) {
        if expr.is_some() && self.stacks.fn_stack.last().map(|f| f.kind) == Some(FuncKind::Setter) {
            self.error_at(
                Span::new(span.start as usize, span.start as usize + 6),
                &gen::Setters_cannot_return_a_value,
                &[],
            );
        }
        let invalid_return = self.stacks.fn_stack.is_empty();
        if invalid_return {
            self.error_at(
                span,
                &gen::A_return_statement_can_only_be_used_within_a_function_body,
                &[],
            );
        }
        let ret_ctx = self.stacks.fn_stack.last().and_then(|f| f.return_type);
        match expr {
            Some(e) => {
                if invalid_return {
                    self.cflags.invalid_return_expr_depth += 1;
                }
                let t = self.check_expr(e, ret_ctx);
                if invalid_return {
                    self.cflags.invalid_return_expr_depth -= 1;
                }
                if let Some(declared) = ret_ctx {
                    if !self.types.is_error(t)
                        && !self.types.is_error(declared)
                        && !matches!(self.types.kind(declared), TypeKind::Any | TypeKind::Void)
                    {
                        self.check_assignable(t, declared, span, None, Some(e));
                    }
                }
            }
            None => {}
        }
    }

    fn for_of_element_type(&mut self, t: TypeId, expr: &'a Expr) -> TypeId {
        if self.types.is_any_or_error(t) {
            return self.types.any;
        }
        if let Some(e) = self.array_element_type(t) {
            return e;
        }
        if let TypeKind::Tuple(elems) = self.types.kind(t).clone() {
            return self.types.union(elems.iter().map(|e| e.ty).collect());
        }
        if matches!(self.types.kind(t), TypeKind::String | TypeKind::StrLit(_)) {
            return self.types.string;
        }
        let d = self.display_type(t);
        self.error_at(
            expr.span(),
            &gen::Type_0_is_not_an_array_type_or_a_string_type,
            &[d],
        );
        self.types.error
    }

    fn check_import(&mut self, i: &'a ImportDecl) {
        // tsc checkGrammarModifiers: `declare` → 1079; modifiers that pass the
        let Some(target) = self.resolve_module(self.current_file, &i.module.value) else {
            self.error_at(
                i.module.span,
                &gen::Cannot_find_module_0_or_its_corresponding_type_declarations,
                &[i.module.value.clone()],
            );
            return;
        };
        let exports = self.bind.exports.get(&target).cloned().unwrap_or_default();
        if let Some(named) = &i.named {
            for spec in named {
                let exported = spec.prop_name.as_ref().unwrap_or(&spec.name);
                if exports.get(&exported.name).is_none() {
                    let module_display = format!("\"{}\"", i.module.value);
                    // a default-exporting module suggests the default-import form
                    if exports.get("default").is_some() {
                        self.error_at(
                            exported.span,
                            &gen::Module_0_has_no_exported_member_1_Did_you_mean_to_use_import_1_from_0_instead,
                            &[module_display, exported.name.clone()],
                        );
                        continue;
                    }
                    // declared locally but not exported (or exported renamed)?
                    if let Some(&mscope) = self.bind.module_scope.get(&target) {
                        let local = self
                            .scope_at(mscope)
                            .values
                            .get(&exported.name)
                            .or(self.scope_at(mscope).types.get(&exported.name));
                        if let Some(lsym) = local {
                            let renamed = exports
                                .0
                                .iter()
                                .find(|(_, s)| *s == lsym)
                                .map(|(n, _)| n.clone());
                            match renamed {
                                Some(rn) => self.error_at(
                                    exported.span,
                                    &gen::Module_0_declares_1_locally_but_it_is_exported_as_2,
                                    &[module_display, exported.name.clone(), rn],
                                ),
                                None => self.error_at(
                                    exported.span,
                                    &gen::Module_0_declares_1_locally_but_it_is_not_exported,
                                    &[module_display, exported.name.clone()],
                                ),
                            }
                            continue;
                        }
                    }
                    let cands: Vec<String> = exports.iter().map(|(n, _)| n.clone()).collect();
                    if let Some(sug) =
                        super::spelling_suggestion(&exported.name, cands.iter().map(|s| s.as_str()))
                    {
                        let sug = sug.to_string();
                        self.error_at(
                            exported.span,
                            &gen::_0_has_no_exported_member_named_1_Did_you_mean_2,
                            &[module_display, exported.name.clone(), sug],
                        );
                    } else {
                        self.error_at(
                            exported.span,
                            &gen::Module_0_has_no_exported_member_1,
                            &[module_display, exported.name.clone()],
                        );
                    }
                }
            }
        }
        if let Some(d) = &i.default_name {
            if exports.get("default").is_none() {
                // tsc displays the module symbol (resolved path sans extension)
                let resolved = self.files[target].0.trim_end_matches(".ts").to_string();
                self.error_at(
                    d.span,
                    &gen::Module_0_has_no_default_export,
                    &[format!("\"{}\"", resolved)],
                );
            }
        }
    }

    // ── classes (P9 core) ───────────────────────────────────────────────────
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecoratorKind {
    Class,
    Property,
    Method,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    Property,
    Method,
    Accessor,
    Ctor,
}

pub(crate) fn modifier_text(k: ModifierKind) -> &'static str {
    match k {
        ModifierKind::Export => "export",
        ModifierKind::Declare => "declare",
        ModifierKind::Abstract => "abstract",
        ModifierKind::Public => "public",
        ModifierKind::Private => "private",
        ModifierKind::Protected => "protected",
        ModifierKind::Static => "static",
        ModifierKind::Readonly => "readonly",
        ModifierKind::Async => "async",
        ModifierKind::Override => "override",
        ModifierKind::Default => "default",
        ModifierKind::Accessor => "accessor",
        ModifierKind::In => "in",
        ModifierKind::Out => "out",
    }
}

pub(crate) fn super_call_pos(stmts: &[Stmt]) -> Option<u32> {
    for s in stmts {
        if let Stmt::Expr { expr, .. } = s {
            if let Expr::Call { callee, span, .. } = expr {
                if matches!(&**callee, Expr::Super { .. }) {
                    return Some(span.start);
                }
            }
        }
    }
    None
}

pub(crate) fn class_member_prop_name(m: &ClassMember) -> Option<&PropName> {
    match m {
        ClassMember::Property(p) if !has_modifier(&p.modifiers, ModifierKind::Static) => {
            Some(&p.name)
        }
        ClassMember::Method(f) if !has_modifier(&f.modifiers, ModifierKind::Static) => {
            f.name.as_ref()
        }
        _ => None,
    }
}

pub(crate) fn collect_this_spans(stmts: &[Stmt], out: &mut Vec<Span>) {
    fn walk_expr(e: &Expr, out: &mut Vec<Span>) {
        match e {
            Expr::This { span } => out.push(*span),
            Expr::PropAccess { obj, .. } => walk_expr(obj, out),
            Expr::ElemAccess { obj, index, .. } => {
                walk_expr(obj, out);
                walk_expr(index, out);
            }
            Expr::Call { callee, args, .. } => {
                walk_expr(callee, out);
                for a in args {
                    walk_expr(a, out);
                }
            }
            Expr::Binary { left, right, .. } => {
                walk_expr(left, out);
                walk_expr(right, out);
            }
            Expr::Unary { operand, .. } | Expr::Update { operand, .. } => walk_expr(operand, out),
            Expr::Paren { inner, .. } => walk_expr(inner, out),
            Expr::Assertion { expr, .. }
            | Expr::NonNull { expr, .. }
            | Expr::Spread { expr, .. }
            | Expr::Await { expr, .. } => walk_expr(expr, out),
            Expr::Cond {
                cond,
                when_true,
                when_false,
                ..
            } => {
                walk_expr(cond, out);
                walk_expr(when_true, out);
                walk_expr(when_false, out);
            }
            _ => {}
        }
    }
    for s in stmts {
        match s {
            Stmt::Expr { expr, .. } | Stmt::Throw { expr, .. } => walk_expr(expr, out),
            Stmt::Var(v) => {
                for d in &v.decls {
                    if let Some(init) = &d.init {
                        walk_expr(init, out);
                    }
                }
            }
            Stmt::Return { expr: Some(e), .. } => walk_expr(e, out),
            Stmt::If { cond, .. } => walk_expr(cond, out),
            Stmt::Block(b) => collect_this_spans(&b.stmts, out),
            _ => {}
        }
    }
}

