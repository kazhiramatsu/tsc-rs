//! Conditional, mapped, and template-literal type evaluation: the
//! `T extends U ? X : Y` engine (with `infer` binding), `{ [K in ...]: ... }`
//! mapped types, and template-literal type expansion. Split out of `symbols.rs`.

use crate::ast::*;
use crate::binder::{ScopeId, SymbolId};
use crate::checker::symbols::{collect_infer_nodes, Mapper};
use crate::checker::Checker;
use crate::diagnostics::gen;
use crate::types::{IndexInfo, PropInfo, Shape, TypeId, TypeKind};

impl<'a> Checker<'a> {
    /// evaluate (or defer) a conditional type under `mapper`
    pub fn evaluate_conditional(
        &mut self,
        c: &'a ConditionalTypeNode,
        scope: ScopeId,
        mapper: &Mapper,
    ) -> TypeId {
        if self.guards.eval_depth > 50 {
            return self.types.error; // instantiation too deep backstop
        }
        self.guards.eval_depth += 1;
        let r = self.evaluate_conditional_impl(c, scope, mapper);
        self.guards.eval_depth -= 1;
        r
    }

    fn evaluate_conditional_impl(
        &mut self,
        c: &'a ConditionalTypeNode,
        scope: ScopeId,
        mapper: &Mapper,
    ) -> TypeId {
        let check_raw = self.resolve_type(&c.check, scope);
        let mut check_t = self.instantiate_type(check_raw, mapper);
        if matches!(self.types.kind(check_t), TypeKind::Keyof(_))
            && !self.type_contains_params(check_t)
        {
            check_t = self.keyof_union(check_t);
        }
        if self.type_contains_params(check_t) {
            // defer with the captured mapper entries that the node may need
            let mut captured: Vec<(SymbolId, TypeId)> =
                mapper.iter().map(|(k, v)| (*k, *v)).collect();
            captured.sort_by_key(|(s, _)| s.0);
            return self
                .types
                .intern_kind(TypeKind::DeferredCond(node_key(c), captured));
        }
        // distributivity: bare type-param check distributes over unions
        let distributive = matches!(&c.check, TypeNode::Ref(r) if r.name.parts.len() == 1 && r.type_args.is_none());
        if distributive {
            if let TypeKind::Union(members) = self.types.kind(check_t).clone() {
                // identify the checked parameter symbol so each member substitutes it
                let param_sym = match &c.check {
                    TypeNode::Ref(r) => self.lookup_type(scope, &r.name.parts[0].name),
                    _ => None,
                };
                if let Some(psym) = param_sym {
                    let parts: Vec<TypeId> = members
                        .iter()
                        .map(|&m| {
                            let mut m2 = mapper.clone();
                            m2.insert(psym, m);
                            self.evaluate_conditional_single(c, scope, &m2, m)
                        })
                        .collect();
                    return self.types.union(parts);
                }
            }
        }
        self.evaluate_conditional_single(c, scope, mapper, check_t)
    }

    /// Mint a brand-new type-parameter symbol for an `infer` binding, distinct
    /// on every call (used so recursive conditionals don't share infer symbols
    /// across depths).
    fn fresh_infer_param(&mut self, name: &str) -> SymbolId {
        self.alloc_synth_symbol(name.to_string(), Vec::new())
    }

    fn evaluate_conditional_single(
        &mut self,
        c: &'a ConditionalTypeNode,
        scope: ScopeId,
        mapper: &Mapper,
        check_t: TypeId,
    ) -> TypeId {
        // collect infer declarations within the extends type
        let mut infer_pairs: Vec<(String, SymbolId)> = Vec::new();
        {
            let mut pending: Vec<(usize, String)> = Vec::new();
            collect_infer_nodes(&c.extends_ty, &mut |node, name| {
                pending.push((node, name.to_string()));
            });
            // A recursive conditional (`Flat<T> = T extends Array<infer U> ?
            // Flat<U> : T`) re-enters the same `infer` node at each depth; a
            // node-keyed synthetic parameter would be shared across levels and an
            // outer binding for it would pre-substitute the inner extends type,
            // defeating the inner inference. Mint a fresh parameter per
            // evaluation instead so each level infers independently.
            for (_node, name) in pending {
                let sym = self.fresh_infer_param(&name);
                infer_pairs.push((name, sym));
            }
        }
        let infer_syms: Vec<SymbolId> = infer_pairs.iter().map(|(_, s)| *s).collect();
        for (name, sym) in &infer_pairs {
            self.tp.infer_mapped_env.push((name.clone(), *sym));
        }
        let ext_raw = self.resolve_type(&c.extends_ty, scope);
        let ext_t = self.instantiate_type(ext_raw, mapper);
        let mut full_mapper = mapper.clone();
        if !infer_syms.is_empty() {
            let inferred = self.infer_conditional_bindings(ext_t, check_t, &infer_syms);
            for &is in &infer_syms {
                let t = inferred.get(&is).copied().unwrap_or(self.types.unknown);
                full_mapper.insert(is, t);
            }
        }
        let ext_inst = self.instantiate_type(ext_t, &full_mapper);
        let matched = self.is_assignable_to(check_t, ext_inst);
        let branch = if matched { &c.true_ty } else { &c.false_ty };
        let raw = self.resolve_type(branch, scope);
        for _ in &infer_pairs {
            self.tp.infer_mapped_env.pop();
        }
        self.instantiate_type(raw, &full_mapper)
    }

    /// evaluate (or defer) a mapped type under `mapper`
    pub fn evaluate_mapped(
        &mut self,
        m: &'a MappedTypeNode,
        scope: ScopeId,
        mapper: &Mapper,
    ) -> TypeId {
        if self.guards.eval_depth > 50 {
            return self.types.error;
        }
        self.guards.eval_depth += 1;
        let r = self.evaluate_mapped_impl(m, scope, mapper);
        self.guards.eval_depth -= 1;
        r
    }

    fn evaluate_mapped_impl(
        &mut self,
        m: &'a MappedTypeNode,
        scope: ScopeId,
        mapper: &Mapper,
    ) -> TypeId {
        let constraint_raw = self.resolve_type(&m.constraint, scope);
        let constraint_t = self.instantiate_type(constraint_raw, mapper);
        if self.type_contains_params(constraint_t) {
            let mut captured: Vec<(SymbolId, TypeId)> =
                mapper.iter().map(|(k, v)| (*k, *v)).collect();
            captured.sort_by_key(|(s, _)| s.0);
            return self
                .types
                .intern_kind(TypeKind::DeferredMapped(node_key(m), captured));
        }
        // homomorphic info: `[K in keyof X]` copies optional/readonly from X
        let homomorphic_src = self.homomorphic_mapped_source(m, scope, mapper);
        let keys_t = match self.types.kind(constraint_t) {
            TypeKind::Keyof(_) => self.keyof_union(constraint_t),
            _ => constraint_t,
        };
        if m.value.is_none()
            && self.options.no_implicit_any()
            && self.report_once_node(7039, node_key(m))
        {
            self.error_at(
                Span::new(m.span.start as usize, m.span.start as usize + 1),
                &gen::Mapped_object_type_implicitly_has_an_any_template_type,
                &[],
            );
        }
        let key_sym = self.synthetic_type_param(node_key(m), &m.key.name);
        let value_raw = self.resolve_mapped_value_type(m, scope, key_sym);
        // `as` key remapping (resolved once with the K placeholder)
        let name_raw = m.name_type.as_ref().map(|nt| {
            self.tp.infer_mapped_env.push((m.key.name.clone(), key_sym));
            let r = self.resolve_type(nt, scope);
            self.tp.infer_mapped_env.pop();
            r
        });
        let mut shape = Shape::default();
        for key in self.types.union_members(keys_t) {
            // a primitive key type (`[P in string]`, e.g. Record<string, T>)
            // produces an index signature rather than enumerated properties.
            if matches!(self.types.kind(key), TypeKind::String | TypeKind::Number) {
                let mut m2 = mapper.clone();
                m2.insert(key_sym, key);
                let vt = self.instantiate_type(value_raw, &m2);
                let readonly = matches!(m.readonly_mod, Some(crate::ast::MappedModifier::Add));
                shape.index_infos.push(IndexInfo {
                    key,
                    value: vt,
                    readonly,
                });
                continue;
            }
            let TypeKind::StrLit(name) = self.types.kind(key).clone() else {
                continue;
            };
            let name = name.to_str_lossy().into_owned();
            let mut m2 = mapper.clone();
            m2.insert(key_sym, key);
            // remap the property name; `never` filters the member out
            let name = match name_raw {
                Some(nr) => {
                    // keep K bound while the (possibly conditional) name type is
                    // evaluated so `as K extends … ? … : never` can resolve `K`.
                    self.tp.infer_mapped_env.push((m.key.name.clone(), key_sym));
                    let nk = self.instantiate_type(nr, &m2);
                    self.tp.infer_mapped_env.pop();
                    match self.types.kind(nk).clone() {
                        TypeKind::StrLit(s) => s.to_str_lossy().into_owned(),
                        TypeKind::Never => continue,
                        _ => name,
                    }
                }
                None => name,
            };
            let mut vt = self.instantiate_type(value_raw, &m2);
            let src_prop = homomorphic_src.and_then(|s| self.prop_info_of_type(s, &name));
            let mut optional = src_prop.as_ref().map(|p| p.optional).unwrap_or(false);
            let mut readonly = src_prop.as_ref().map(|p| p.readonly).unwrap_or(false);
            match m.optional_mod {
                Some(crate::ast::MappedModifier::Add) => optional = true,
                Some(crate::ast::MappedModifier::Remove) => optional = false,
                None => {}
            }
            match m.readonly_mod {
                Some(crate::ast::MappedModifier::Add) => readonly = true,
                Some(crate::ast::MappedModifier::Remove) => readonly = false,
                None => {}
            }
            if optional
                && self.options.strict_null_checks()
                && !self.options.exact_optional_property_types
            {
                vt = self.types.union(vec![vt, self.types.undefined]);
            }
            if matches!(m.optional_mod, Some(crate::ast::MappedModifier::Remove)) {
                vt = self
                    .types
                    .filter_union(vt, |tt, mm| !matches!(tt.kind(mm), TypeKind::Undefined));
            }
            shape.props.push(PropInfo {
                name,
                ty: vt,
                optional,
                readonly,
                is_method: false,
                symbol: None,
            });
        }
        let sid = self.types.alloc_shape(shape);
        self.types.alloc(TypeKind::Anon(sid))
    }

    fn homomorphic_mapped_source(
        &mut self,
        m: &'a MappedTypeNode,
        scope: ScopeId,
        mapper: &Mapper,
    ) -> Option<TypeId> {
        match &m.constraint {
            TypeNode::Keyof { ty, .. } => {
                let raw = self.resolve_type(ty, scope);
                Some(self.instantiate_type(raw, mapper))
            }
            _ => None,
        }
    }

    fn resolve_mapped_value_type(
        &mut self,
        m: &'a MappedTypeNode,
        scope: ScopeId,
        key_sym: SymbolId,
    ) -> TypeId {
        match &m.value {
            Some(v) => {
                self.tp.infer_mapped_env.push((m.key.name.clone(), key_sym));
                let r = self.resolve_type(v, scope);
                self.tp.infer_mapped_env.pop();
                r
            }
            None => self.types.any,
        }
    }

    pub(crate) fn deferred_mapped_prop_info(&mut self, t: TypeId, name: &str) -> Option<PropInfo> {
        let TypeKind::DeferredMapped(key, captured) = self.types.kind(t).clone() else {
            return None;
        };
        let &(m, scope, file) = self.deferred.deferred_mappeds.get(&key)?;

        // Key remapping is not generally invertible from an output property name
        // back to the source key. Keep such mapped types opaque for member lookup
        // unless evaluate_mapped can materialize the whole type.
        if m.name_type.is_some() {
            return None;
        }

        let mapper: Mapper = captured.iter().copied().collect();
        let prev_file = self.current_file;
        let before_diags = self.diags.len();
        self.current_file = file;

        let result = self.deferred_mapped_prop_info_inner(m, scope, &mapper, name);

        self.current_file = prev_file;
        self.diags.truncate(before_diags);
        result
    }

    pub(crate) fn deferred_homomorphic_array_view_type(&mut self, t: TypeId) -> Option<TypeId> {
        let TypeKind::DeferredMapped(key, captured) = self.types.kind(t).clone() else {
            return None;
        };
        let &(m, scope, file) = self.deferred.deferred_mappeds.get(&key)?;
        if m.name_type.is_some() {
            return None;
        }

        let mapper: Mapper = captured.iter().copied().collect();
        let prev_file = self.current_file;
        let before_diags = self.diags.len();
        self.current_file = file;

        let key_sym = self.synthetic_type_param(node_key(m), &m.key.name);
        let value_raw = self.resolve_mapped_value_type(m, scope, key_sym);
        let result = self.homomorphic_array_view_type_inner(m, scope, &mapper, key_sym, value_raw);

        self.current_file = prev_file;
        self.diags.truncate(before_diags);
        result
    }

    fn deferred_mapped_prop_info_inner(
        &mut self,
        m: &'a MappedTypeNode,
        scope: ScopeId,
        mapper: &Mapper,
        name: &str,
    ) -> Option<PropInfo> {
        let constraint_raw = self.resolve_type(&m.constraint, scope);
        let constraint_t = self.instantiate_type(constraint_raw, mapper);
        let keys_t = match self.types.kind(constraint_t) {
            TypeKind::Keyof(_) => self.keyof_union(constraint_t),
            _ => constraint_t,
        };
        let key_t = self.types.string_lit(name);
        if !self.is_assignable_to(key_t, keys_t) {
            return None;
        }

        let key_sym = self.synthetic_type_param(node_key(m), &m.key.name);
        let value_raw = self.resolve_mapped_value_type(m, scope, key_sym);
        if let Some(prop) =
            self.deferred_homomorphic_array_prop_info(m, scope, mapper, name, key_sym, value_raw)
        {
            return Some(prop);
        }

        let mut mapper = mapper.clone();
        mapper.insert(key_sym, key_t);
        let mut ty = self.instantiate_type(value_raw, &mapper);

        let homomorphic_src = self.homomorphic_mapped_source(m, scope, &mapper);
        let src_prop = homomorphic_src.and_then(|s| self.prop_info_of_type(s, name));
        let mut optional = src_prop.as_ref().map(|p| p.optional).unwrap_or(false);
        let mut readonly = src_prop.as_ref().map(|p| p.readonly).unwrap_or(false);

        match m.optional_mod {
            Some(crate::ast::MappedModifier::Add) => optional = true,
            Some(crate::ast::MappedModifier::Remove) => optional = false,
            None => {}
        }
        match m.readonly_mod {
            Some(crate::ast::MappedModifier::Add) => readonly = true,
            Some(crate::ast::MappedModifier::Remove) => readonly = false,
            None => {}
        }
        if optional
            && self.options.strict_null_checks()
            && !self.options.exact_optional_property_types
        {
            ty = self.types.union(vec![ty, self.types.undefined]);
        }
        if matches!(m.optional_mod, Some(crate::ast::MappedModifier::Remove)) {
            ty = self
                .types
                .filter_union(ty, |tt, mm| !matches!(tt.kind(mm), TypeKind::Undefined));
        }

        Some(PropInfo {
            name: name.to_string(),
            ty,
            optional,
            readonly,
            is_method: false,
            symbol: src_prop.and_then(|p| p.symbol),
        })
    }

    fn deferred_homomorphic_array_prop_info(
        &mut self,
        m: &'a MappedTypeNode,
        scope: ScopeId,
        mapper: &Mapper,
        name: &str,
        key_sym: SymbolId,
        value_raw: TypeId,
    ) -> Option<PropInfo> {
        let mapped_array =
            self.homomorphic_array_view_type_inner(m, scope, mapper, key_sym, value_raw)?;
        self.prop_info_of_type(mapped_array, name)
    }

    fn homomorphic_array_view_type_inner(
        &mut self,
        m: &'a MappedTypeNode,
        scope: ScopeId,
        mapper: &Mapper,
        key_sym: SymbolId,
        value_raw: TypeId,
    ) -> Option<TypeId> {
        let source = self.homomorphic_mapped_source(m, scope, mapper)?;
        let source = self.apparent_type(source);
        let (_, source_readonly) = self.array_like_element_type(source)?;

        let mut elem_mapper = mapper.clone();
        elem_mapper.insert(key_sym, self.types.number);
        let mut elem = self.instantiate_type(value_raw, &elem_mapper);
        if matches!(m.optional_mod, Some(crate::ast::MappedModifier::Add))
            && self.options.strict_null_checks()
            && !self.options.exact_optional_property_types
        {
            elem = self.types.union(vec![elem, self.types.undefined]);
        }
        if matches!(m.optional_mod, Some(crate::ast::MappedModifier::Remove)) {
            elem = self
                .types
                .filter_union(elem, |tt, mm| !matches!(tt.kind(mm), TypeKind::Undefined));
        }

        let readonly = match m.readonly_mod {
            Some(crate::ast::MappedModifier::Add) => true,
            Some(crate::ast::MappedModifier::Remove) => false,
            None => source_readonly,
        };
        let mapped_array = if readonly {
            self.types.intern_kind(TypeKind::ReadonlyArray(elem))
        } else {
            self.array_type(elem)
        };
        Some(mapped_array)
    }

    fn array_like_element_type(&mut self, t: TypeId) -> Option<(TypeId, bool)> {
        match self.types.kind(t).clone() {
            TypeKind::Tuple(elems) => {
                let elem = self.types.union(elems.iter().map(|e| e.ty).collect());
                Some((elem, false))
            }
            TypeKind::ReadonlyTuple(elems) => {
                let elem = self.types.union(elems.iter().map(|e| e.ty).collect());
                Some((elem, true))
            }
            TypeKind::ReadonlyArray(e) => Some((e, true)),
            _ => self.array_element_type(t).map(|e| (e, false)),
        }
    }

    /// build a template-literal type; expands eagerly when all parts are
    /// finite string-literal unions (cross product, leftmost varying slowest)
    pub fn template_literal_type(&mut self, head: String, parts: Vec<(TypeId, String)>) -> TypeId {
        fn finite(c: &mut Checker, t: TypeId) -> Option<Vec<String>> {
            match c.types.kind(t).clone() {
                TypeKind::StrLit(s) => Some(vec![s.to_str_lossy().into_owned()]),
                TypeKind::NumLit(bits) => {
                    Some(vec![crate::js_num::to_js_string(f64::from_bits(bits))])
                }
                TypeKind::BoolLit(b) => Some(vec![if b { "true".into() } else { "false".into() }]),
                TypeKind::Union(ms) => {
                    let mut out = Vec::new();
                    for m in ms {
                        out.extend(finite(c, m)?);
                    }
                    Some(out)
                }
                _ => None,
            }
        }
        let mut expansions: Vec<String> = vec![head.clone()];
        let mut all_finite = true;
        for (t, text) in &parts {
            match finite(self, *t) {
                Some(vals) => {
                    let mut next = Vec::with_capacity(expansions.len() * vals.len());
                    for prefix in &expansions {
                        for v in &vals {
                            next.push(format!("{}{}{}", prefix, v, text));
                        }
                    }
                    expansions = next;
                }
                None => {
                    all_finite = false;
                    break;
                }
            }
        }
        if all_finite && !parts.is_empty() {
            let lits: Vec<TypeId> = expansions
                .iter()
                .map(|s| self.types.string_lit(s))
                .collect();
            return self.types.union(lits);
        }
        if parts.is_empty() {
            return self.types.string_lit(&head);
        }
        let mut tpl: Vec<crate::types::TplPart> = Vec::new();
        if !head.is_empty() {
            tpl.push(crate::types::TplPart::Str(head));
        }
        for (t, text) in parts {
            tpl.push(crate::types::TplPart::Ty(t));
            if !text.is_empty() {
                tpl.push(crate::types::TplPart::Str(text));
            }
        }
        self.types.intern_kind(TypeKind::TemplateLit(tpl))
    }
}
