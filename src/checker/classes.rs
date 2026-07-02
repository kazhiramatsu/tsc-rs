//! Class declaration checking: accessor visibility, override modifiers, member
//! duplicate/staticness/overload checks, constructor overloads, abstract member
//! implementation, and definite-assignment in constructors. Split out of `stmts.rs`.

use crate::ast::*;
use crate::binder::{flags, SymbolId};
use crate::checker::stmts::{
    class_member_prop_name, collect_this_spans, stmts_assign_this_prop, super_call_pos,
    DecoratorKind, MemberKind,
};
use crate::checker::{Checker, CtorFieldContext, CtorFieldContextKind, Slot};
use crate::diagnostics::gen;
use crate::types::{TypeId, TypeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstanceMemberKind {
    Property,
    Accessor,
    Method,
}

impl<'a> Checker<'a> {
    pub fn check_class_pub(&mut self, c: &'a ClassDecl) {
        self.check_class(c);
    }

    fn instance_member_kind(&self, member: SymbolId) -> Option<InstanceMemberKind> {
        let decls = &self.symbol(member).decls;
        if decls.iter().any(|d| {
            matches!(d, crate::binder::Decl::Method(f)
                if matches!(f.kind, FuncKind::Getter | FuncKind::Setter))
        }) {
            Some(InstanceMemberKind::Accessor)
        } else if decls.iter().any(
            |d| matches!(d, crate::binder::Decl::Method(f) if matches!(f.kind, FuncKind::Method)),
        ) {
            Some(InstanceMemberKind::Method)
        } else if decls
            .iter()
            .any(|d| matches!(d, crate::binder::Decl::PropertyDecl(_)))
        {
            Some(InstanceMemberKind::Property)
        } else {
            None
        }
    }

    fn inherited_instance_member_kind(
        &mut self,
        class_sym: SymbolId,
        name: &str,
    ) -> Option<(SymbolId, InstanceMemberKind)> {
        let (base_sym, _) = self.base_class_of(class_sym)?;
        let mut cur = Some(base_sym);
        while let Some(b) = cur {
            if let Some(member) = self.bind.symbols[b.0 as usize].members.get(name) {
                if let Some(kind) = self.instance_member_kind(member) {
                    return Some((b, kind));
                }
                return None;
            }
            cur = self.base_class_of(b).map(|(s, _)| s);
        }
        None
    }

    /// 2808: a get accessor must be at least as accessible as the setter
    fn check_accessor_visibility(&mut self, c: &'a ClassDecl) {
        use std::collections::HashMap as Map;
        fn level(mods: &Modifiers) -> u8 {
            if has_modifier(mods, ModifierKind::Private) {
                2
            } else if has_modifier(mods, ModifierKind::Protected) {
                1
            } else {
                0
            }
        }
        let mut pairs: Map<
            String,
            (
                Option<(&'a FunctionLike, u8)>,
                Option<(&'a FunctionLike, u8)>,
            ),
        > = Map::new();
        for m in &c.members {
            if let ClassMember::Method(f) = m {
                if let Some(n) = f.name.as_ref().and_then(|x| x.text()) {
                    let e = pairs.entry(n).or_default();
                    match f.kind {
                        FuncKind::Getter => e.0 = Some((f, level(&f.modifiers))),
                        FuncKind::Setter => e.1 = Some((f, level(&f.modifiers))),
                        _ => {}
                    }
                }
            }
        }
        for (_n, (g, s)) in pairs {
            if let (Some((gf, gl)), Some((sf, sl))) = (g, s) {
                if gl > sl {
                    for f in [gf, sf] {
                        self.error_at(
                            f.name.as_ref().unwrap().span(),
                            &gen::A_get_accessor_must_be_at_least_as_accessible_as_the_setter,
                            &[],
                        );
                    }
                }
            }
        }
    }

    /// 7032/7033: accessor signatures derive their property type from the
    /// getter when possible; otherwise a setter without an annotated value
    /// parameter leaves the property as implicit any.
    fn check_accessor_implicit_any(&mut self, c: &'a ClassDecl) {
        use std::collections::HashMap as Map;

        fn value_param(f: &FunctionLike) -> Option<&Param> {
            f.params.iter().find(|p| {
                !p.name
                    .as_ident()
                    .map(|id| id.name == "this")
                    .unwrap_or(false)
            })
        }

        fn is_private_name(name: &PropName) -> bool {
            matches!(name, PropName::Ident(id) if id.name.starts_with('#'))
        }

        let mut pairs: Map<(String, bool), (Option<&'a FunctionLike>, Option<&'a FunctionLike>)> =
            Map::new();
        for m in &c.members {
            let ClassMember::Method(f) = m else { continue };
            if !matches!(f.kind, FuncKind::Getter | FuncKind::Setter) {
                continue;
            }
            let Some(name) = f.name.as_ref() else {
                continue;
            };
            if is_private_name(name) {
                continue;
            }
            let Some(text) = name.text() else {
                continue;
            };
            let is_static = has_modifier(&f.modifiers, ModifierKind::Static);
            let entry = pairs.entry((text, is_static)).or_default();
            match f.kind {
                FuncKind::Getter => entry.0 = Some(f),
                FuncKind::Setter => entry.1 = Some(f),
                _ => {}
            }
        }

        for ((_name, _is_static), (getter, setter)) in pairs {
            let getter_has_type = getter
                .map(|g| g.return_type.is_some() || g.body.is_some())
                .unwrap_or(false);
            if let Some(g) = getter {
                if !getter_has_type {
                    if let Some(name) = &g.name {
                        let display_name = self.display_prop_name_for_error(name);
                        if self.reported.reported_7033_accessors.insert(node_key(g)) {
                            if self.options.no_implicit_any() {
                                self.error_at(
                                    name.span(),
                                    &gen::Property_0_implicitly_has_type_any_because_its_get_accessor_lacks_a_return_type_annotation,
                                    &[display_name],
                                );
                            } else {
                                self.suggestion_at(
                                    name.span(),
                                    &gen::Property_0_implicitly_has_type_any_because_its_get_accessor_lacks_a_return_type_annotation,
                                    &[display_name],
                                );
                            }
                        }
                    }
                }
            }
            if !getter_has_type {
                if let Some(s) = setter {
                    let setter_has_type = value_param(s).and_then(|p| p.ty.as_ref()).is_some();
                    if !setter_has_type {
                        if let Some(name) = &s.name {
                            let display_name = self.display_prop_name_for_error(name);
                            if self.reported.reported_7032_accessors.insert(node_key(s)) {
                                if self.options.no_implicit_any() {
                                    self.error_at(
                                        name.span(),
                                        &gen::Property_0_implicitly_has_type_any_because_its_set_accessor_lacks_a_parameter_type_annotation,
                                        &[display_name],
                                    );
                                } else {
                                    self.suggestion_at(
                                        name.span(),
                                        &gen::Property_0_implicitly_has_type_any_because_its_set_accessor_lacks_a_parameter_type_annotation,
                                        &[display_name],
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn class_setter_has_paired_getter(&self, c: &'a ClassDecl, setter: &'a FunctionLike) -> bool {
        if setter.kind != FuncKind::Setter {
            return false;
        }
        let Some(name) = setter.name.as_ref().and_then(|n| n.text()) else {
            return false;
        };
        let is_static = has_modifier(&setter.modifiers, ModifierKind::Static);
        c.members.iter().any(|m| {
            let ClassMember::Method(f) = m else {
                return false;
            };
            f.kind == FuncKind::Getter
                && has_modifier(&f.modifiers, ModifierKind::Static) == is_static
                && f.name.as_ref().and_then(|n| n.text()).as_deref() == Some(name.as_str())
        })
    }

    fn class_property_is_first_named_member(
        &self,
        c: &'a ClassDecl,
        prop: &'a PropertyDecl,
    ) -> bool {
        let Some(name) = prop.name.text() else {
            return false;
        };
        let is_static = has_modifier(&prop.modifiers, ModifierKind::Static);
        for m in &c.members {
            match m {
                ClassMember::Property(p) => {
                    if std::ptr::eq(p, prop) {
                        return true;
                    }
                    if has_modifier(&p.modifiers, ModifierKind::Static) == is_static
                        && p.name.text().as_deref() == Some(name.as_str())
                    {
                        return false;
                    }
                }
                ClassMember::Method(f) => {
                    if has_modifier(&f.modifiers, ModifierKind::Static) == is_static
                        && f.name.as_ref().and_then(|n| n.text()).as_deref() == Some(name.as_str())
                    {
                        return false;
                    }
                }
                _ => {}
            }
        }
        true
    }

    /// 4112/4113/4114: override-modifier consistency
    fn check_override_modifiers(&mut self, c: &'a ClassDecl, sym: SymbolId) {
        let base = self.base_class_of(sym).map(|(b, _)| b);
        let cname = self.symbol(sym).name.clone();
        for m in &c.members {
            let (mods, name_opt, span_opt) = match m {
                ClassMember::Property(p) => (&p.modifiers, p.name.text(), Some(p.name.span())),
                ClassMember::Method(f)
                    if matches!(
                        f.kind,
                        FuncKind::Method | FuncKind::Getter | FuncKind::Setter
                    ) =>
                {
                    (
                        &f.modifiers,
                        f.name.as_ref().and_then(|n| n.text()),
                        f.name.as_ref().map(|n| n.span()),
                    )
                }
                _ => continue,
            };
            let Some(name) = name_opt else { continue };
            let Some(span) = span_opt else { continue };
            if has_modifier(mods, ModifierKind::Static) {
                continue;
            }
            let has_override = has_modifier(mods, ModifierKind::Override);
            let in_base = base
                .map(|b| {
                    let mut cur = Some(b);
                    while let Some(bb) = cur {
                        if self.bind.symbols[bb.0 as usize]
                            .members
                            .get(&name)
                            .is_some()
                        {
                            return true;
                        }
                        cur = self.base_class_of(bb).map(|(s, _)| s);
                    }
                    false
                })
                .unwrap_or(false);
            if has_override {
                match base {
                    None => {
                        self.error_at(
                            span,
                            &gen::This_member_cannot_have_an_override_modifier_because_its_containing_class_0_does_not_extend_another_class,
                            &[cname.clone()],
                        );
                    }
                    Some(b) if !in_base => {
                        let bn = self.symbol(b).name.clone();
                        self.error_at(
                            span,
                            &gen::This_member_cannot_have_an_override_modifier_because_it_is_not_declared_in_the_base_class_0,
                            &[bn],
                        );
                    }
                    _ => {}
                }
            } else if self.options.no_implicit_override && in_base {
                let bn = base
                    .map(|b| self.symbol(b).name.clone())
                    .unwrap_or_default();
                self.error_at(
                    span,
                    &gen::This_member_must_have_an_override_modifier_because_it_overrides_a_member_in_the_base_class_0,
                    &[bn],
                );
            }
        }
    }

    /// 2300: duplicate instance member names (field vs field/method; accessor
    /// conflicts report at the later site only)
    fn check_class_member_duplicates(&mut self, c: &'a ClassDecl) {
        #[derive(Clone, Copy, PartialEq)]
        enum K {
            Field,
            Method,
            Get,
            Set,
        }
        let mut sites: Vec<(String, K, Span, bool)> = Vec::new();
        for m in &c.members {
            match m {
                ClassMember::Property(p) => {
                    if let Some(n) = p.name.text() {
                        let is_static = has_modifier(&p.modifiers, ModifierKind::Static);
                        sites.push((n, K::Field, p.name.span(), is_static));
                    }
                }
                ClassMember::Method(f) => {
                    if let Some(n) = f.name.as_ref().and_then(|x| x.text()) {
                        let k = match f.kind {
                            FuncKind::Getter => K::Get,
                            FuncKind::Setter => K::Set,
                            _ => K::Method,
                        };
                        let is_static = has_modifier(&f.modifiers, ModifierKind::Static);
                        sites.push((n, k, f.name.as_ref().unwrap().span(), is_static));
                    }
                }
                _ => {}
            }
        }
        let mut reported: Vec<Span> = Vec::new();
        for i in 0..sites.len() {
            for j in 0..i {
                if sites[i].0 != sites[j].0 || sites[i].3 != sites[j].3 {
                    continue;
                }
                let (ki, kj) = (sites[i].1, sites[j].1);
                // legal pairs: method overloads, get+set
                if ki == K::Method && kj == K::Method {
                    continue;
                }
                if matches!((ki, kj), (K::Get, K::Set) | (K::Set, K::Get)) {
                    continue;
                }
                let accessor_involved =
                    matches!(ki, K::Get | K::Set) || matches!(kj, K::Get | K::Set);
                let name = sites[i].0.clone();
                if accessor_involved {
                    // later site only
                    if !reported.contains(&sites[i].2) {
                        reported.push(sites[i].2);
                        self.error_at(sites[i].2, &gen::Duplicate_identifier_0, &[name.clone()]);
                    }
                } else {
                    for s in [sites[j].2, sites[i].2] {
                        if !reported.contains(&s) {
                            reported.push(s);
                            self.error_at(s, &gen::Duplicate_identifier_0, &[name.clone()]);
                        }
                    }
                }
            }
        }
    }

    /// 2387/2388: method overloads must agree on staticness
    fn check_method_overload_staticness(&mut self, c: &'a ClassDecl) {
        use std::collections::HashMap as Map;
        let mut groups: Map<String, Vec<(&'a FunctionLike, bool)>> = Map::new();
        for m in &c.members {
            if let ClassMember::Method(f) = m {
                if matches!(f.kind, FuncKind::Method) {
                    if let Some(n) = f.name.as_ref().and_then(|n| n.text()) {
                        let is_static = has_modifier(&f.modifiers, ModifierKind::Static);
                        groups.entry(n).or_default().push((f, is_static));
                    }
                }
            }
        }
        for (_n, decls) in groups {
            if decls.len() < 2 {
                continue;
            }
            let canonical = decls[0].1;
            if decls.iter().all(|(_, s)| *s == canonical) {
                continue;
            }
            for (f, is_static) in &decls[1..] {
                if *is_static != canonical {
                    let span = f.name.as_ref().map(|n| n.span()).unwrap_or(f.span);
                    let msg: &'static crate::diagnostics::DiagnosticMessage = if canonical {
                        &gen::Function_overload_must_be_static
                    } else {
                        &gen::Function_overload_must_not_be_static
                    };
                    self.error_at(span, msg, &[]);
                }
            }
        }
    }

    /// 2390/2392: constructor overloads need exactly one implementation
    fn check_ctor_overloads(&mut self, c: &'a ClassDecl) {
        let is_ambient = has_modifier(&c.modifiers, ModifierKind::Declare);
        if is_ambient {
            return;
        }
        let ctors: Vec<&'a FunctionLike> = c
            .members
            .iter()
            .filter_map(|m| match m {
                ClassMember::Constructor(f) => Some(&**f),
                _ => None,
            })
            .collect();
        if ctors.is_empty() {
            return;
        }
        let impls: Vec<&&'a FunctionLike> = ctors.iter().filter(|f| f.body.is_some()).collect();
        if impls.is_empty() {
            let f = ctors[ctors.len() - 1];
            let kw = Span::new(f.span.start as usize, f.span.start as usize + 11);
            self.error_at(kw, &gen::Constructor_implementation_is_missing, &[]);
        } else if impls.len() > 1 {
            for f in &impls {
                let kw = Span::new(f.span.start as usize, f.span.start as usize + 11);
                self.error_at(
                    kw,
                    &gen::Multiple_constructor_implementations_are_not_allowed,
                    &[],
                );
            }
        }
    }

    fn constructor_field_blocked_names(
        &self,
        c: &'a ClassDecl,
    ) -> Option<std::collections::HashSet<String>> {
        let ctor = c
            .members
            .iter()
            .filter_map(|m| match m {
                ClassMember::Constructor(f) => Some(&**f),
                _ => None,
            })
            .find(|f| f.body.is_some())
            .or_else(|| {
                c.members.iter().find_map(|m| match m {
                    ClassMember::Constructor(f) => Some(&**f),
                    _ => None,
                })
            })?;
        let scope = self.bind.node_scope.get(&node_key(ctor)).copied()?;
        let names = self
            .scope_at(scope)
            .values
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<std::collections::HashSet<_>>();
        (!names.is_empty()).then_some(names)
    }

    /// A non-abstract class must implement every abstract member inherited from
    /// an abstract base class (TS2515).
    fn check_abstract_implementations(&mut self, c: &'a ClassDecl, sym: SymbolId) {
        if has_modifier(&c.modifiers, ModifierKind::Abstract) {
            return;
        }
        // build the base chain (most-derived first)
        let mut chain = vec![sym];
        let mut cur = self.base_class_of(sym).map(|(b, _)| b);
        let mut guard = 0;
        while let Some(b) = cur {
            if chain.contains(&b) || guard > 256 {
                break;
            }
            chain.push(b);
            guard += 1;
            cur = self.base_class_of(b).map(|(s, _)| s);
        }
        if chain.len() < 2 {
            return; // no base class
        }
        let direct_base = chain[1];
        // most-derived declaration of each member name wins
        let mut seen: std::collections::HashMap<String, (bool, SymbolId)> =
            std::collections::HashMap::new();
        for &cls in &chain {
            let members = self.symbol(cls).members.0.clone();
            for (name, mid) in &members {
                if seen.contains_key(name) {
                    continue;
                }
                let is_abstract = self.symbol(*mid).flags & flags::ABSTRACT != 0;
                seen.insert(name.clone(), (!is_abstract, cls));
            }
        }
        let mut unimplemented: Vec<String> = seen
            .into_iter()
            .filter(|(_, (concrete, _))| !*concrete)
            .map(|(n, _)| n)
            .collect();
        unimplemented.sort();
        let name_span = c.name.as_ref().map(|n| n.span).unwrap_or(c.span);
        let derived_instance = self.types.intern_kind(TypeKind::Iface(sym));
        let dn = self.display_type(derived_instance);
        let bn = match &c.extends {
            Some(h) => self
                .base_instance_type(c, h)
                .map(|base| self.display_type(base))
                .unwrap_or_else(|| self.generic_name_with_params(direct_base)),
            None => self.generic_name_with_params(direct_base),
        };
        for mname in unimplemented {
            self.error_at(
                name_span,
                &gen::Non_abstract_class_0_does_not_implement_inherited_abstract_member_1_from_class_2,
                &[dn.clone(), mname, bn.clone()],
            );
        }
    }

    pub(crate) fn check_class(&mut self, c: &'a ClassDecl) {
        let key = node_key(c);
        if self.checked_decls.contains(&key) {
            return;
        }
        self.checked_decls.insert(key);
        let Some(sym) = self.bind.decl_symbol.get(&key).copied() else {
            return;
        };
        let scope = self
            .bind
            .node_scope
            .get(&key)
            .copied()
            .unwrap_or(self.current_scope);
        let prev_scope = self.current_scope;
        self.current_scope = scope;
        // `this` inside the class body/members refers to this class (paired with
        // the class_stack.pop() at the end of this function).
        self.stacks.class_stack.push(sym);
        // The class-body `ThisContainer` covers decorators, the heritage clause,
        // and non-static field initializers. Static members and method bodies
        // push their own container on top.
        self.stacks
            .this_container_stack
            .push(crate::checker::ThisContainer {
                class_owner: Some(sym),
                is_static: false,
                kind: crate::checker::ContainerKind::ClassBody,
                explicit_this: None,
            });
        if let Some(n) = &c.name {
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
            if RESERVED.contains(&n.name.as_str()) {
                self.error_at(n.span, &gen::Class_name_cannot_be_0, &[n.name.clone()]);
            }
        }
        for d in &c.decorators {
            self.check_decorator(d, "ClassDecoratorContext", DecoratorKind::Class);
        }
        self.check_ctor_overloads(c);
        self.check_accessor_visibility(c);
        self.check_accessor_implicit_any(c);
        self.check_method_overload_staticness(c);
        self.check_class_member_duplicates(c);
        self.check_override_modifiers(c, sym);
        self.check_abstract_implementations(c, sym);
        for m in &c.members {
            match m {
                ClassMember::Property(p) => {
                    for d in &p.decorators {
                        self.check_decorator(
                            d,
                            "ClassFieldDecoratorContext",
                            DecoratorKind::Property,
                        );
                    }
                }
                ClassMember::Method(f) => {
                    if matches!(f.kind, FuncKind::Getter | FuncKind::Setter) {
                        if let Some(n) = f.name.as_ref().and_then(|x| x.text()) {
                            if let Some((base, base_kind)) =
                                self.inherited_instance_member_kind(sym, &n)
                            {
                                let base_name = self.symbol(base).name.clone();
                                let derived_name = self.symbol(sym).name.clone();
                                let span = f.name.as_ref().unwrap().span();
                                match base_kind {
                                    InstanceMemberKind::Property => {
                                        self.error_at(
                                            span,
                                            &gen::_0_is_defined_as_a_property_in_class_1_but_is_overridden_here_in_2_as_an_accessor,
                                            &[n.clone(), base_name, derived_name],
                                        );
                                    }
                                    InstanceMemberKind::Method => {
                                        self.error_at(
                                            span,
                                            &gen::Class_0_defines_instance_member_function_1_but_extended_class_2_defines_it_as_instance_member_accessor,
                                            &[base_name, n.clone(), derived_name],
                                        );
                                    }
                                    InstanceMemberKind::Accessor => {}
                                }
                            }
                            if n == "constructor" {
                                self.error_at(
                                    f.name.as_ref().unwrap().span(),
                                    &gen::Class_constructor_may_not_be_an_accessor,
                                    &[],
                                );
                            }
                        }
                    } else if matches!(f.kind, FuncKind::Method) {
                        if let Some(n) = f.name.as_ref().and_then(|x| x.text()) {
                            if let Some((base, base_kind)) =
                                self.inherited_instance_member_kind(sym, &n)
                            {
                                let base_name = self.symbol(base).name.clone();
                                let derived_name = self.symbol(sym).name.clone();
                                let span = f.name.as_ref().unwrap().span();
                                match base_kind {
                                    InstanceMemberKind::Property => {
                                        self.error_at(
                                            span,
                                            &gen::Class_0_defines_instance_member_property_1_but_extended_class_2_defines_it_as_instance_member_function,
                                            &[base_name, n.clone(), derived_name],
                                        );
                                    }
                                    InstanceMemberKind::Accessor => {
                                        self.error_at(
                                            span,
                                            &gen::Class_0_defines_instance_member_accessor_1_but_extended_class_2_defines_it_as_instance_member_function,
                                            &[base_name, n.clone(), derived_name],
                                        );
                                    }
                                    InstanceMemberKind::Method => {}
                                }
                            }
                        }
                    }
                    if f.body.is_none() {
                        for d in &f.decorators {
                            self.error_at(
                                Span::new(d.at_span.start as usize, d.span.end as usize),
                                &gen::A_decorator_can_only_decorate_a_method_implementation_not_an_overload,
                                &[],
                            );
                        }
                        continue;
                    }
                    let ctxty = match f.kind {
                        FuncKind::Getter => "ClassGetterDecoratorContext",
                        FuncKind::Setter => "ClassSetterDecoratorContext",
                        _ => "ClassMethodDecoratorContext",
                    };
                    for d in &f.decorators {
                        self.check_decorator(d, ctxty, DecoratorKind::Method);
                    }
                }
                _ => {}
            }
        }

        // heritage
        let mut base_instance: Option<TypeId> = None;
        if let Some(h) = &c.extends {
            let et = self
                .extends_expr_static_type(c, h)
                .unwrap_or(self.types.error);
            let base_class_sym = self.class_symbol_from_expr(scope, &h.expr);
            if let Some(bsym) = base_class_sym {
                if bsym == sym || self.class_base_chain_contains(bsym, sym, &mut Vec::new()) {
                    self.report_base_cycle(sym);
                }
            }
            // private-constructor bases cannot be extended (2675)
            if let Some(bsym) = base_class_sym {
                let mut ctor_private = false;
                for d in self.symbol(bsym).decls.clone() {
                    if let crate::binder::Decl::Class(bc) = d {
                        for m in &bc.members {
                            if let ClassMember::Constructor(cf) = m {
                                if has_modifier(&cf.modifiers, ModifierKind::Private) {
                                    ctor_private = true;
                                }
                            }
                        }
                    }
                }
                if ctor_private {
                    let bn = self.symbol(bsym).name.clone();
                    self.error_at(
                        h.expr.span(),
                        &gen::Cannot_extend_a_class_0_Class_constructor_is_marked_as_private,
                        &[bn],
                    );
                }
                // heritage type arguments vs base type params (2315)
                if let Some(args) = &h.type_args {
                    let tparams = self.type_params_of_symbol(bsym);
                    if tparams.is_empty() && !args.is_empty() {
                        let bn = self.symbol(bsym).name.clone();
                        self.error_at(h.expr.span(), &gen::Type_0_is_not_generic, &[bn]);
                    }
                }
                // static side compatibility (2417) — properties only, the
                // construct signatures are exempt
                let self_extends = self
                    .bind
                    .decl_symbol
                    .get(&key)
                    .map(|&d| bsym == d)
                    .unwrap_or(false);
                if let (Some(&dsym), false) = (self.bind.decl_symbol.get(&key), self_extends) {
                    let d_statics = self.types.intern_kind(TypeKind::ClassStatics(dsym));
                    let b_statics = et;
                    let strip = |c2: &mut Self, t: TypeId| -> TypeId {
                        match c2.shape_of_type(t) {
                            Some(sid) => {
                                let mut sh = c2.types.shape(sid).clone();
                                sh.ctor_sigs.clear();
                                sh.call_sigs.clear();
                                let nid = c2.types.alloc_shape(sh);
                                c2.types.alloc(TypeKind::Anon(nid))
                            }
                            None => t,
                        }
                    };
                    let dp = strip(self, d_statics);
                    let bp = strip(self, b_statics);
                    if !self.is_assignable_to(dp, bp) {
                        let dd = self.display_type(d_statics);
                        let bd = self.display_type(b_statics);
                        self.rel.keep_head_for_missing = true;
                        self.report_relation_failure(
                            dp,
                            bp,
                            c.name.as_ref().map(|n| n.span).unwrap_or(c.span),
                            Some((
                                &gen::Class_static_side_0_incorrectly_extends_base_class_static_side_1,
                                vec![dd, bd],
                            )),
                        );
                        self.rel.keep_head_for_missing = false;
                    }
                }
            }
            if let Some(b) = self.base_instance_type(c, h) {
                base_instance = Some(b);
            } else if !self.types.is_any_or_error(et)
                && self.base_static_type_from_extends_type(et).is_none()
            {
                let d = {
                    let r = self.types.regular(et);
                    self.display_type(r)
                };
                self.error_at(
                    h.expr.span(),
                    &gen::Type_0_is_not_a_constructor_function_type,
                    &[d],
                );
            }
        }

        // ambient classes cannot contain implementations (1183)
        if has_modifier(&c.modifiers, ModifierKind::Declare) {
            for m in &c.members {
                if let ClassMember::Method(f) = m {
                    if let Some(crate::ast::FuncBody::Block(b)) = &f.body {
                        self.error_at(
                            Span::new(b.span.start as usize, b.span.start as usize + 1),
                            &gen::An_implementation_cannot_be_declared_in_ambient_contexts,
                            &[],
                        );
                    }
                }
            }
        }
        // private identifier grammar checks. 18028 (only available targeting
        let ctor_field_blocked_names = if self.options.use_define_for_class_fields() {
            None
        } else {
            self.constructor_field_blocked_names(c)
        };

        // member checks
        let class_is_abstract = has_modifier(&c.modifiers, ModifierKind::Abstract);
        let class_is_ambient =
            has_modifier(&c.modifiers, ModifierKind::Declare) || self.in_ambient_context();
        let instance = self.types.intern_kind(TypeKind::Iface(sym));
        for m in &c.members {
            match m {
                ClassMember::StaticBlock(b) => {
                    let bscope = self
                        .bind
                        .node_scope
                        .get(&node_key(b))
                        .copied()
                        .unwrap_or(scope);
                    let tc = crate::checker::ThisContainer {
                        class_owner: Some(sym),
                        is_static: true,
                        kind: crate::checker::ContainerKind::Method,
                        explicit_this: None,
                    };
                    self.with_current_scope(bscope, |this| {
                        this.with_this_container(tc, |this| {
                            this.cflags.in_class_static_block += 1;
                            this.check_statements(&b.stmts, bscope);
                            this.cflags.in_class_static_block -= 1;
                        });
                    });
                }
                ClassMember::Property(p) => {
                    if p.accessor_span.is_some() {
                        self.error_at(
                            p.name.span(),
                            &gen::Properties_with_the_accessor_modifier_are_only_available_when_targeting_ECMAScript_2015_and_higher,
                            &[],
                        );
                    }
                    if has_modifier(&p.modifiers, ModifierKind::Static) {
                        if let (Some(ty), Some(tps)) = (&p.ty, &c.type_params) {
                            let names: Vec<&str> =
                                tps.iter().map(|t| t.name.name.as_str()).collect();
                            let mut hits: Vec<Span> = Vec::new();
                            crate::checker::symbols::collect_type_ref_names_pub(
                                ty,
                                &mut |n, sp| {
                                    if names.contains(&n) {
                                        hits.push(sp);
                                    }
                                },
                            );
                            for sp in hits {
                                self.error_at(
                                    sp,
                                    &gen::Static_members_cannot_reference_class_type_parameters,
                                    &[],
                                );
                            }
                        }
                    }
                    self.check_member_modifiers_ext(
                        &p.modifiers,
                        MemberKind::Property,
                        class_is_abstract,
                    );
                    if p.init.is_some() && has_modifier(&p.modifiers, ModifierKind::Abstract) {
                        let n = self.display_prop_name_for_error(&p.name);
                        self.error_at(
                            p.name.span(),
                            &gen::Property_0_cannot_have_an_initializer_because_it_is_marked_abstract,
                            &[n],
                        );
                    }
                    if let (Some(_), Some(question_span)) = (p.accessor_span, p.question_span) {
                        self.error_at(
                            question_span,
                            &gen::An_accessor_property_cannot_be_declared_optional,
                            &[],
                        );
                    }
                    if let Some(exclam_span) = p.exclam_span {
                        if p.init.is_some() {
                            self.error_at(
                                exclam_span,
                                &gen::Declarations_with_initializers_cannot_also_have_definite_assignment_assertions,
                                &[],
                            );
                        } else if p.ty.is_none() {
                            self.error_at(
                                exclam_span,
                                &gen::Declarations_with_definite_assignment_assertions_must_also_have_type_annotations,
                                &[],
                            );
                        } else if has_modifier(&p.modifiers, ModifierKind::Static)
                            || has_modifier(&p.modifiers, ModifierKind::Abstract)
                            || has_modifier(&p.modifiers, ModifierKind::Declare)
                            || class_is_ambient
                        {
                            self.error_at(
                                exclam_span,
                                &gen::A_definite_assignment_assertion_is_not_permitted_in_this_context,
                                &[],
                            );
                        }
                    }
                    if let PropName::String { value, span } = &p.name {
                        if value == "constructor" {
                            self.error_at(
                                *span,
                                &gen::Classes_may_not_have_a_field_named_constructor,
                                &[],
                            );
                        }
                    }
                    if p.ty.is_none()
                        && p.init.is_none()
                        && self.is_canonical_decl_key(node_key(p))
                        && self.class_property_is_first_named_member(c, p)
                    {
                        if let Some(pn) = p.name.text() {
                            if self.options.no_implicit_any() {
                                self.error_at(
                                    p.name.span(),
                                    &gen::Member_0_implicitly_has_an_1_type,
                                    &[pn, "any".to_string()],
                                );
                            } else {
                                self.suggestion_at(
                                    p.name.span(),
                                    &gen::Member_0_implicitly_has_an_1_type_but_a_better_type_may_be_inferred_from_usage,
                                    &[pn, "any".to_string()],
                                );
                            }
                        }
                    }
                    // 2610: base declares this as an accessor
                    if let Some(pn) = p.name.text() {
                        if let Some((base, base_kind)) =
                            self.inherited_instance_member_kind(sym, &pn)
                        {
                            if base_kind == InstanceMemberKind::Accessor {
                                let base_name = self.symbol(base).name.clone();
                                let derived_name = self.symbol(sym).name.clone();
                                self.error_at(
                                    p.name.span(),
                                    &gen::_0_is_defined_as_an_accessor_in_class_1_but_is_overridden_here_in_2_as_an_instance_property,
                                    &[pn.clone(), base_name, derived_name],
                                );
                            }
                        }
                    }
                    let is_static = has_modifier(&p.modifiers, ModifierKind::Static);
                    let use_ctor_field_context = !is_static && ctor_field_blocked_names.is_some();
                    let field_name = p.name.text().unwrap_or_default();
                    let declared = p.ty.as_ref().map(|ty| {
                        let ctx = use_ctor_field_context.then(|| CtorFieldContext {
                            field_name: field_name.clone(),
                            blocked_names: ctor_field_blocked_names.clone().unwrap_or_default(),
                            kind: CtorFieldContextKind::TypeAnnotation,
                        });
                        self.with_ctor_field(ctx, |this| this.resolve_type_cached(ty, scope))
                    });
                    if let Some(init) = &p.init {
                        let this_ctx = is_static.then(|| crate::checker::ThisContainer {
                            class_owner: Some(sym),
                            is_static: true,
                            kind: crate::checker::ContainerKind::ClassBody,
                            explicit_this: None,
                        });
                        let it = self.with_opt_this_container(this_ctx, |this| {
                            this.prop_init_pos = Some(p.span.start as usize);
                            let field_ctx = use_ctor_field_context.then(|| CtorFieldContext {
                                field_name: field_name.clone(),
                                blocked_names: ctor_field_blocked_names.clone().unwrap_or_default(),
                                kind: CtorFieldContextKind::Initializer,
                            });
                            let it =
                                this.with_ctor_field(field_ctx, |this| this.check_expr(init, declared));
                            this.prop_init_pos = None;
                            it
                        });
                        if let Some(dt) = declared {
                            if !self.types.is_error(dt) {
                                self.check_assignable(it, dt, p.name.span(), None, Some(init));
                            }
                        }
                    } else if let Some(_dt) = declared {
                        // strictPropertyInitialization
                        if self.options.strict_property_initialization()
                            && !p.question
                            && !p.exclam
                            && !has_modifier(&p.modifiers, ModifierKind::Static)
                            && !has_modifier(&p.modifiers, ModifierKind::Declare)
                            && !has_modifier(&p.modifiers, ModifierKind::Abstract)
                            && !self.is_definitely_assigned_in_ctor(c, &p.name)
                        {
                            let n = self.display_prop_name_for_error(&p.name);
                            self.error_at(
                                p.name.span(),
                                &gen::Property_0_has_no_initializer_and_is_not_definitely_assigned_in_the_constructor,
                                &[n],
                            );
                        }
                    }
                }
                ClassMember::Method(f) => {
                    let mk = match f.kind {
                        FuncKind::Getter => MemberKind::Accessor,
                        FuncKind::Setter => MemberKind::Accessor,
                        _ => MemberKind::Method,
                    };
                    self.check_member_modifiers_ext(&f.modifiers, mk, class_is_abstract);
                    if matches!(f.kind, FuncKind::Getter | FuncKind::Setter)
                        && f.body.is_some()
                        && has_modifier(&f.modifiers, ModifierKind::Abstract)
                    {
                        if let Some(name) = &f.name {
                            self.error_at(
                                name.span(),
                                &gen::An_abstract_accessor_cannot_have_an_implementation,
                                &[],
                            );
                        }
                    }
                    let suppress_params = self.class_setter_has_paired_getter(c, f);
                    let suppress_return = class_is_ambient
                        && f.body.is_none()
                        && matches!(f.kind, FuncKind::Method)
                        && has_modifier(&f.modifiers, ModifierKind::Private);
                    if suppress_params && suppress_return {
                        self.with_suppressed_next_function_implicit_any_params(|this| {
                            this.with_suppressed_next_function_implicit_any_return(|this| {
                                this.check_function_body(f, None, true)
                            })
                        });
                    } else if suppress_params {
                        self.with_suppressed_next_function_implicit_any_params(|this| {
                            this.check_function_body(f, None, true)
                        });
                    } else if suppress_return {
                        self.with_suppressed_next_function_implicit_any_return(|this| {
                            this.check_function_body(f, None, true)
                        });
                    } else {
                        self.check_function_body(f, None, true);
                    }
                }
                ClassMember::Constructor(f) => {
                    let mut super_pos = u32::MAX;
                    if c.extends.is_some() {
                        if let Some(FuncBody::Block(b)) = &f.body {
                            match super_call_pos(&b.stmts) {
                                Some(pos) => super_pos = pos,
                                None => {
                                    self.error_at(
                                        Span::new(f.span.start as usize, f.span.start as usize + "constructor".len()),
                                        &gen::Constructors_for_derived_classes_must_contain_a_super_call,
                                        &[],
                                    );
                                }
                            }
                            // 17009: `this` before super()
                            let mut this_spans = Vec::new();
                            collect_this_spans(&b.stmts, &mut this_spans);
                            for ts in this_spans {
                                if ts.start < super_pos {
                                    self.error_at(
                                        ts,
                                        &gen::super_must_be_called_before_accessing_this_in_the_constructor_of_a_derived_class,
                                        &[],
                                    );
                                }
                            }
                        }
                    }
                    self.check_function_body(f, None, true);
                    self.flow.ctor_flow = None;
                }
                ClassMember::Index(_) => {}
            }
        }
        // heritage compatibility — tsc: silent whole-type check; on failure,
        // per-member wrapped 2416s; if none issued, the broad 2420/2415 head.
        let mut bases: Vec<(TypeId, &'static crate::diagnostics::DiagnosticMessage)> = Vec::new();
        if let Some(b) = base_instance {
            if !self.type_has_failed_base_resolution(b)
                && !self.type_base_chain_contains_class(b, sym)
            {
                bases.push((b, &gen::Class_0_incorrectly_extends_base_class_1));
            }
        }
        for impl_ref in &c.implements {
            if impl_ref.name.parts.len() == 1 && impl_ref.type_args.is_none() {
                let n = impl_ref.name.parts[0].name.as_str();
                if matches!(n, "string" | "number" | "boolean" | "bigint" | "symbol") {
                    self.error_at(
                        impl_ref.span,
                        &gen::A_class_cannot_implement_a_primitive_type_like_0_It_can_only_implement_other_named_object_types,
                        &[n.to_string()],
                    );
                    continue;
                }
            }
            let it = self.resolve_type_cached_ref(impl_ref, scope);
            if !self.types.is_error(it) {
                let is_class_target = matches!(self.types.kind(it), TypeKind::Iface(s)
                    if self.bind.symbols[s.0 as usize].flags & flags::CLASS != 0);
                let msg: &'static crate::diagnostics::DiagnosticMessage = if is_class_target {
                    &gen::Class_0_incorrectly_implements_class_1_Did_you_mean_to_extend_1_and_inherit_its_members_as_a_subclass
                } else {
                    &gen::Class_0_incorrectly_implements_interface_1
                };
                bases.push((it, msg));
            }
        }
        // non-abstract class must implement inherited abstract members — tsc
        for (base, broad) in bases {
            if self.is_assignable_to(instance, base) {
                continue;
            }
            let mut issued = false;
            for m in &c.members {
                if let Some(name) = class_member_prop_name(m) {
                    let Some(n) = name.text() else { continue };
                    let Some(bp) = self.prop_info_of_type(base, &n) else {
                        continue;
                    };
                    let Some(sp) = self.prop_info_of_type(instance, &n) else {
                        continue;
                    };
                    if !self.is_assignable_to(sp.ty, bp.ty) {
                        let cn = self.display_type(instance);
                        let bn = self.display_type(base);
                        self.report_relation_failure_wrapped(
                            sp.ty,
                            bp.ty,
                            name.span(),
                            (
                                &gen::Property_0_in_type_1_is_not_assignable_to_the_same_property_in_base_type_2,
                                vec![n, cn, bn],
                            ),
                        );
                        issued = true;
                    }
                }
            }
            if !issued {
                if let Some(name) = &c.name {
                    self.rel.keep_head_for_missing = true;
                    let cn = self.generic_name_with_params(sym);
                    let bn = self.display_type(base);
                    self.report_relation_failure(
                        instance,
                        base,
                        name.span,
                        Some((broad, vec![cn, bn])),
                    );
                    self.rel.keep_head_for_missing = false;
                }
            }
        }
        self.stacks.class_stack.pop();
        self.stacks.this_container_stack.pop();
        self.current_scope = prev_scope;
    }

    fn type_has_failed_base_resolution(&mut self, t: TypeId) -> bool {
        let rt = self.types.regular(t);
        match self.types.kind(rt).clone() {
            TypeKind::Iface(sym) | TypeKind::Ref(sym, _) | TypeKind::MappedIface(sym, _) => {
                self.symbol(sym).flags & flags::CLASS != 0
                    && self.res.resolution_failed.contains(&(sym, Slot::BaseType))
            }
            TypeKind::Intersection(members) | TypeKind::Union(members) => members
                .iter()
                .copied()
                .any(|m| self.type_has_failed_base_resolution(m)),
            _ => false,
        }
    }

    fn type_base_chain_contains_class(&mut self, t: TypeId, target: SymbolId) -> bool {
        let rt = self.types.regular(t);
        match self.types.kind(rt).clone() {
            TypeKind::Iface(sym) | TypeKind::Ref(sym, _) | TypeKind::MappedIface(sym, _) => {
                self.symbol(sym).flags & flags::CLASS != 0
                    && self.class_base_chain_contains(sym, target, &mut Vec::new())
            }
            TypeKind::Intersection(members) | TypeKind::Union(members) => members
                .iter()
                .copied()
                .any(|m| self.type_base_chain_contains_class(m, target)),
            _ => false,
        }
    }

    fn class_base_chain_contains(
        &mut self,
        cur: SymbolId,
        target: SymbolId,
        seen: &mut Vec<SymbolId>,
    ) -> bool {
        if cur == target {
            return true;
        }
        if seen.contains(&cur) {
            return false;
        }
        seen.push(cur);
        self.base_class_of(cur)
            .map(|(base, _)| self.class_base_chain_contains(base, target, seen))
            .unwrap_or(false)
    }

    fn is_definitely_assigned_in_ctor(&self, c: &'a ClassDecl, name: &PropName) -> bool {
        let Some(n) = name.text() else { return false };
        for m in &c.members {
            if let ClassMember::Constructor(f) = m {
                if let Some(FuncBody::Block(b)) = &f.body {
                    return stmts_assign_this_prop(&b.stmts, &n);
                }
            }
        }
        false
    }
}
