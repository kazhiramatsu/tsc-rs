//! Type aliases: intrinsic string-manipulation aliases (`Uppercase`,
//! `Lowercase`, ...), the `NonNullable` intrinsic, and resolution of declared
//! type-alias references to their target types. Split out of `symbols.rs`.

use crate::ast::*;
use crate::binder::{Decl, ScopeId, SymbolId};
use crate::checker::symbols::Mapper;
use crate::checker::{Checker, Slot};
use crate::diagnostics::gen;
use crate::types::{TypeId, TypeKind};

impl<'a> Checker<'a> {
    /// True when the alias body is the bare `intrinsic` marker (lib-defined
    /// Uppercase/Lowercase/Capitalize/Uncapitalize).
    fn is_intrinsic_alias(&self, sym: SymbolId) -> bool {
        self.symbol(sym).decls.iter().any(|d| {
            matches!(d, Decl::Alias(a)
                if matches!(&a.ty, crate::ast::TypeNode::Ref(r)
                    if r.name.parts.len() == 1 && r.name.parts[0].name == "intrinsic"))
        })
    }

    /// Apply an intrinsic string transform to (literal) string types, mapping
    /// over unions; non-literal operands are returned unchanged.
    fn apply_string_intrinsic(&mut self, case: StrCase, arg: TypeId) -> TypeId {
        let reg = self.types.regular(arg);
        match self.types.kind(reg).clone() {
            TypeKind::StrLit(s) => {
                let src = s.to_str_lossy();
                let out = case.apply(&src);
                self.types.string_lit(&out)
            }
            TypeKind::Union(members) => {
                let mapped: Vec<TypeId> = members
                    .iter()
                    .map(|&m| self.apply_string_intrinsic(case, m))
                    .collect();
                self.types.union(mapped)
            }
            // a template PATTERN maps its literal parts; hole types map
            // recursively (digits have no case, so number/bigint holes are
            // identity; a boolean hole becomes its cased literal union) —
            // Uppercase<`aA${number}${boolean}`> = `AA${number}${"TRUE"|"FALSE"}`
            TypeKind::TemplateLit(parts) => {
                let mapped: Vec<crate::types::TplPart> = parts
                    .iter()
                    .map(|p| match p {
                        crate::types::TplPart::Str(s) => crate::types::TplPart::Str(case.apply(s)),
                        crate::types::TplPart::Ty(t) => {
                            let mt = if *t == self.types.boolean {
                                let tt = self.types.string_lit(&case.apply("true"));
                                let ff = self.types.string_lit(&case.apply("false"));
                                self.types.union(vec![tt, ff])
                            } else {
                                self.apply_string_intrinsic(case, *t)
                            };
                            crate::types::TplPart::Ty(mt)
                        }
                    })
                    .collect();
                self.types.intern_kind(TypeKind::TemplateLit(mapped))
            }
            _ => arg,
        }
    }

    /// Identify a compiler-implemented string-manipulation alias by its `=
    /// intrinsic` body. Returns 0=Uppercase, 1=Lowercase, 2=Capitalize,
    /// 3=Uncapitalize.
    fn intrinsic_string_kind(&self, sym: SymbolId) -> Option<u8> {
        let Some(Decl::Alias(a)) = self.symbol(sym).decls.first().copied() else {
            return None;
        };
        let is_intrinsic = matches!(&a.ty,
            crate::ast::TypeNode::Ref(rf)
                if rf.name.parts.len() == 1 && rf.name.parts[0].name == "intrinsic");
        if !is_intrinsic {
            return None;
        }
        match a.name.name.as_str() {
            "Uppercase" => Some(0),
            "Lowercase" => Some(1),
            "Capitalize" => Some(2),
            "Uncapitalize" => Some(3),
            _ => None,
        }
    }

    /// Apply an intrinsic string transform over a (possibly union) type.
    /// String literals are transformed exactly; non-literal string-ish types
    /// collapse to `string`.
    fn apply_intrinsic_string(&mut self, kind: u8, s: TypeId) -> TypeId {
        match self.types.kind(s).clone() {
            TypeKind::StrLit(js) => {
                let orig = js.to_str_lossy().into_owned();
                let out = match kind {
                    0 => orig.to_uppercase(),
                    1 => orig.to_lowercase(),
                    2 => {
                        let mut it = orig.chars();
                        match it.next() {
                            Some(f) => f.to_uppercase().chain(it).collect(),
                            None => orig,
                        }
                    }
                    _ => {
                        let mut it = orig.chars();
                        match it.next() {
                            Some(f) => f.to_lowercase().chain(it).collect(),
                            None => orig,
                        }
                    }
                };
                self.types.string_lit(&out)
            }
            TypeKind::Union(members) => {
                let mapped: Vec<TypeId> = members
                    .iter()
                    .map(|&m| self.apply_intrinsic_string(kind, m))
                    .collect();
                self.types.union(mapped)
            }
            // template PATTERNS keep their shape with literal parts cased
            // (apply_string_intrinsic) — collapsing to `string` lost
            // Uppercase<`aA${number}`> ~ `AA${number}` equivalence
            TypeKind::TemplateLit(_) => {
                let case = match kind {
                    0 => StrCase::Upper,
                    1 => StrCase::Lower,
                    2 => StrCase::Capitalize,
                    _ => StrCase::Uncapitalize,
                };
                self.apply_string_intrinsic(case, s)
            }
            TypeKind::String | TypeKind::TypeParam(_) => self.types.string,
            _ => s,
        }
    }

    /// Detect an alias whose body is `T & {}` (the `NonNullable` pattern): an
    /// intersection of the single type parameter with an empty object literal.
    fn is_nonnullable_alias(&self, sym: SymbolId) -> bool {
        let Some(Decl::Alias(a)) = self.symbol(sym).decls.first().copied() else {
            return false;
        };
        let crate::ast::TypeNode::Intersection { members, .. } = &a.ty else {
            return false;
        };
        let Some(tps) = a.type_params.as_ref() else {
            return false;
        };
        if members.len() != 2 || tps.len() != 1 {
            return false;
        }
        let tp_name = tps[0].name.name.as_str();
        let mut saw_param = false;
        let mut saw_empty_obj = false;
        for m in members {
            match m {
                crate::ast::TypeNode::Ref(rf)
                    if rf.name.parts.len() == 1 && rf.name.parts[0].name == tp_name =>
                {
                    saw_param = true;
                }
                crate::ast::TypeNode::TypeLiteral { members, .. } if members.is_empty() => {
                    saw_empty_obj = true;
                }
                _ => return false,
            }
        }
        saw_param && saw_empty_obj
    }

    /// Remove `null` and `undefined` from a type (the effect of `& {}`).
    pub(crate) fn remove_nullish(&mut self, t: TypeId) -> TypeId {
        match self.types.kind(t).clone() {
            TypeKind::Null | TypeKind::Undefined => self.types.never,
            TypeKind::Union(members) => {
                let kept: Vec<TypeId> = members
                    .iter()
                    .copied()
                    .filter(|&m| {
                        !matches!(self.types.kind(m), TypeKind::Null | TypeKind::Undefined)
                    })
                    .collect();
                if kept.is_empty() {
                    self.types.never
                } else {
                    self.types.union(kept)
                }
            }
            _ => t,
        }
    }

    pub fn alias_ref_type(&mut self, sym: SymbolId, r: &'a TypeRef, scope: ScopeId) -> TypeId {
        // `NonNullable<T> = T & {}`: build the intersection so a concrete
        // argument has its nullish members stripped now, while an abstract
        // argument stays symbolic (`T & {}`) and re-folds on instantiation.
        if self.is_nonnullable_alias(sym) {
            if let Some(args) = r.type_args.as_ref() {
                if args.len() == 1 {
                    let arg = self.resolve_type(&args[0], scope);
                    return self.non_nullable(arg);
                }
            }
        }
        // intrinsic string-manipulation aliases (Uppercase/Lowercase/
        // Capitalize/Uncapitalize): apply the transform to the resolved
        // argument rather than resolving the `intrinsic` body to `any`.
        if let Some(kind) = self.intrinsic_string_kind(sym) {
            if let Some(args) = r.type_args.as_ref() {
                if args.len() == 1 {
                    let s = self.resolve_type(&args[0], scope);
                    return self.apply_intrinsic_string(kind, s);
                }
            }
        }
        let declared = self.declared_alias_type(sym);
        let tparams = self.type_params_of_symbol(sym);
        let args = match r.type_args.as_ref() {
            Some(args) => {
                if tparams.is_empty() {
                    let name = self.symbol(sym).name.clone();
                    self.error_at(r.span, &gen::Type_0_is_not_generic, &[name]);
                    return self.types.error;
                }
                let required = self.required_type_param_count(&tparams);
                if args.len() < required || args.len() > tparams.len() {
                    let display = self.generic_name_with_params(sym);
                    if required < tparams.len() {
                        self.error_at(
                            r.span,
                            &gen::Generic_type_0_requires_between_1_and_2_type_arguments,
                            &[display, required.to_string(), tparams.len().to_string()],
                        );
                    } else {
                        self.error_at(
                            r.span,
                            &gen::Generic_type_0_requires_1_type_argument_s,
                            &[display, required.to_string()],
                        );
                    }
                    return self.types.error;
                }
                let provided: Vec<TypeId> =
                    args.iter().map(|a| self.resolve_type(a, scope)).collect();
                self.check_type_arg_constraints(sym, &tparams, &provided, args);
                // fill any omitted trailing parameters from their defaults,
                // substituting earlier parameters as we go (`<T, U = T>`)
                let mut mapper = Mapper::new();
                let mut full = Vec::with_capacity(tparams.len());
                for (i, &p) in tparams.iter().enumerate() {
                    let at = if i < provided.len() {
                        provided[i]
                    } else {
                        let d = self.default_of_type_param(p).unwrap_or(self.types.any);
                        self.instantiate_type(d, &mapper)
                    };
                    mapper.insert(p, at);
                    full.push(at);
                }
                full
            }
            None => {
                let required = self.required_type_param_count(&tparams);
                if required > 0 {
                    let display = self.generic_name_with_params(sym);
                    self.error_at(
                        r.name.span,
                        &gen::Generic_type_0_requires_1_type_argument_s,
                        &[display, required.to_string()],
                    );
                    return self.types.error;
                }
                if tparams.is_empty() {
                    Vec::new()
                } else {
                    // every parameter is defaulted: instantiate with the defaults
                    let mut mapper = Mapper::new();
                    let mut full = Vec::with_capacity(tparams.len());
                    for &p in &tparams {
                        let d = self.default_of_type_param(p).unwrap_or(self.types.any);
                        let dt = self.instantiate_type(d, &mapper);
                        mapper.insert(p, dt);
                        full.push(dt);
                    }
                    full
                }
            }
        };
        // compiler-implemented string-manipulation aliases
        // (Uppercase/Lowercase/Capitalize/Uncapitalize): apply the transform to
        // the (literal) argument rather than resolving the `intrinsic` body.
        if args.len() == 1 {
            let case = match self.symbol(sym).name.as_str() {
                "Uppercase" => Some(StrCase::Upper),
                "Lowercase" => Some(StrCase::Lower),
                "Capitalize" => Some(StrCase::Capitalize),
                "Uncapitalize" => Some(StrCase::Uncapitalize),
                _ => None,
            };
            if let Some(case) = case {
                if self.is_intrinsic_alias(sym) {
                    return self.apply_string_intrinsic(case, args[0]);
                }
            }
        }
        if args.is_empty() {
            declared
        } else {
            let mut mapper = Mapper::new();
            for (i, &p) in tparams.iter().enumerate() {
                mapper.insert(p, args[i]);
            }
            let t = self.instantiate_type(declared, &mapper);
            // conditional aliases that EVALUATED lose the alias name (tsc shows
            // the resolved branch); mapped/other aliases keep it
            let declared_was_cond = matches!(self.types.kind(declared), TypeKind::DeferredCond(..));
            let evaluated_away =
                declared_was_cond && !matches!(self.types.kind(t), TypeKind::DeferredCond(..));
            if !evaluated_away
                && matches!(
                    self.types.kind(t),
                    TypeKind::Union(_)
                        | TypeKind::Intersection(_)
                        | TypeKind::Anon(_)
                        | TypeKind::DeferredObj(_)
                        | TypeKind::Tuple(_)
                        | TypeKind::ReadonlyTuple(_)
                        | TypeKind::DeferredCond(..)
                        | TypeKind::DeferredMapped(..)
                        | TypeKind::Keyof(_)
                        | TypeKind::IndexedAccess(..)
                )
            {
                // alias threading (empirically calibrated against tsc):
                //   type Y = NoStr<X>        (target body: conditional) → 'Y'
                //   type P2 = Omit<U, "k">  (target body: alias ref)   → 'P2'
                //   type UP = Partial<...>   (target body: mapped lit)  → 'Partial<...>'
                let target_body_chains = declared_was_cond
                    || self
                        .symbol(sym)
                        .decls
                        .first()
                        .map(|d| matches!(d, Decl::Alias(a) if matches!(&a.ty, crate::ast::TypeNode::Ref(_) | crate::ast::TypeNode::Conditional(_))))
                        .unwrap_or(false);
                match self.deferred.pending_alias.last().copied() {
                    Some((outer, body_key)) if body_key == node_key(r) && target_body_chains => {
                        self.types.set_alias_force(t, outer, Vec::new())
                    }
                    _ => self.types.set_alias(t, sym, args),
                }
            }
            t
        }
    }

    pub fn declared_alias_type(&mut self, sym: SymbolId) -> TypeId {
        if let Some(&t) = self.caches.alias_type_cache.get(&sym) {
            return t;
        }
        if let Some(cycle_start) = self
            .res
            .resolving
            .iter()
            .position(|(s, slot)| *s == sym && *slot == Slot::AliasTarget)
        {
            // A self-reference reached through a deferring wrapper (array/tuple/
            // function element) is a legal recursive type — e.g.
            // `type Json = ... | Json[]`. Break the cycle permissively rather
            // than reporting TS2456 (which only applies to a *direct* circular
            // alias such as `type A = A` or `type A = A | string`).
            if self.guards.alias_wrapper_depth > 0 {
                return self.types.any;
            }
            // Every type alias on the resolution stack from the re-entered alias
            // up to the top participates in this cycle. tsc reports TS2456 on
            // *each* of them (one failed popTypeResolution per stack frame), so
            // we do too. Reporting only the re-entered alias made the emitted
            // set depend on which alias the resolver entered first: within a
            // single per-file worker the shared `alias_type_cache` suppresses
            // the sibling's cycle report, so a cross-file cycle (`A = B` in one
            // file, `B = A` in another) surfaced one or both TS2456s depending
            // on how the work-stealing pool happened to co-locate the files —
            // nondeterministic output. Emitting the whole cycle makes the set
            // worker-layout-independent; cross-worker duplicates are folded by
            // `sort_and_dedupe`.
            let cycle_aliases: Vec<SymbolId> = self.res.resolving[cycle_start..]
                .iter()
                .filter(|(_, slot)| *slot == Slot::AliasTarget)
                .map(|(s, _)| *s)
                .collect();
            for csym in cycle_aliases {
                if self.res.resolution_failed.insert((csym, Slot::AliasTarget)) {
                    if let Some(Decl::Alias(a)) = self.symbol(csym).decls.first().copied() {
                        let name = self.symbol(csym).name.clone();
                        let file = self.symbol(csym).file;
                        let prev = self.current_file;
                        self.current_file = file;
                        self.error_at(
                            a.name.span,
                            &gen::Type_alias_0_circularly_references_itself,
                            &[name],
                        );
                        self.current_file = prev;
                    }
                }
            }
            return self.types.error;
        }
        let Some(Decl::Alias(a)) = self.symbol(sym).decls.first().copied() else {
            return self.types.error;
        };
        self.res.resolving.push((sym, Slot::AliasTarget));
        let scope = self
            .bind
            .node_scope
            .get(&node_key(a))
            .copied()
            .unwrap_or(self.bind.global_scope);
        let body_ref_key = match &a.ty {
            crate::ast::TypeNode::Ref(r) => node_key(r),
            _ => 0,
        };
        self.deferred.pending_alias.push((sym, body_ref_key));
        let t = self.resolve_type(&a.ty, scope);
        self.deferred.pending_alias.pop();
        self.res.resolving.pop();
        // alias display metadata: only structured types carry aliases.
        // template-literal bodies (and their expansions) display raw in tsc.
        // `keyof X` bodies that evaluate to a literal union also display
        // expanded (`"a" | "b"`, not the alias name) in tsc.
        let is_template_body = matches!(&a.ty, crate::ast::TypeNode::TemplateLit { .. });
        let is_keyof_body = matches!(&a.ty, crate::ast::TypeNode::Keyof { .. });
        if !is_template_body
            && !is_keyof_body
            && matches!(
                self.types.kind(t),
                TypeKind::Union(_)
                    | TypeKind::Intersection(_)
                    | TypeKind::Anon(_)
                    | TypeKind::Tuple(_)
                    | TypeKind::ReadonlyTuple(_)
                    | TypeKind::DeferredObj(_)
            )
        {
            self.types.set_alias(t, sym, Vec::new());
        }
        self.caches.alias_type_cache.insert(sym, t);
        t
    }

    // ── member shapes ───────────────────────────────────────────────────────
}

/// Intrinsic string-manipulation kinds (TS `Uppercase`/`Lowercase`/
/// `Capitalize`/`Uncapitalize`).
#[derive(Clone, Copy)]
pub(crate) enum StrCase {
    Upper,
    Lower,
    Capitalize,
    Uncapitalize,
}

impl StrCase {
    fn apply(self, s: &str) -> String {
        match self {
            StrCase::Upper => s.to_uppercase(),
            StrCase::Lower => s.to_lowercase(),
            StrCase::Capitalize => {
                let mut chars = s.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
            StrCase::Uncapitalize => {
                let mut chars = s.chars();
                match chars.next() {
                    Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
        }
    }
}
