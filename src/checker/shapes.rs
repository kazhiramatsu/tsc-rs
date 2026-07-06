//! Object shape computation: interning the member shape of object/interface/
//! intersection types, base-class resolution and static/instance shape
//! building, and property lookup over shapes. Split out of `symbols.rs`.

use crate::ast::*;
use crate::binder::{flags, Decl, SymbolId};
use crate::checker::symbols::Mapper;
use crate::checker::{Checker, Slot};
use crate::diagnostics::gen;
use crate::types::{IndexInfo, PropInfo, Shape, ShapeId, Signature, TypeId, TypeKind};

impl<'a> Checker<'a> {
    /// An anonymous object type with no members (`{}`). It constrains only
    /// "assignable to object" (i.e. not null/undefined), so intersecting with it
    /// strips the nullish members of the other operand.
    pub(crate) fn is_empty_object_type(&self, t: TypeId) -> bool {
        if let TypeKind::Anon(s) = self.types.kind(t) {
            let sh = self.types.shape(*s);
            sh.props.is_empty()
                && sh.call_sigs.is_empty()
                && sh.ctor_sigs.is_empty()
                && sh.index_infos.is_empty()
        } else {
            false
        }
    }

    pub(crate) fn empty_object_type(&mut self) -> TypeId {
        // NOTE (Tier-2 Stage 0): allocating a fresh shape per call means two
        // `T & {}` (`NonNullable<T>`) built at different times get different
        // TypeIds. Canonicalizing the id at boot flips TypeId-ordered union
        // display (e.g. `U | NonNullable<T>` → `NonNullable<T> | U`), so the
        // flow-verify harness normalizes by display instead; revisit when a
        // behavior-change window is open.
        let id = self.types.alloc_shape(Shape {
            props: Vec::new(),
            call_sigs: Vec::new(),
            ctor_sigs: Vec::new(),
            index_infos: Vec::new(),
        });
        self.types.intern_kind(TypeKind::Anon(id))
    }

    /// getIntersectionType. Normalize `members` into a single type or an
    /// `Intersection` node. Primitive and subtype reductions still collapse
    /// where tsc does, and `X & {}` strips nullish members. Distinct object
    /// operands remain as intersection members: their combined member shape is
    /// supplied by `shape_of_type(Intersection)`, while relation reporting can
    /// still point at the specific operand that failed.
    pub(crate) fn intersect_all(&mut self, members: Vec<TypeId>) -> TypeId {
        let mut flat: Vec<TypeId> = Vec::new();
        for m in members {
            match self.types.kind(m).clone() {
                TypeKind::Intersection(inner) => flat.extend(inner),
                _ => flat.push(m),
            }
        }
        // absorbing elements: never wins, then error/any; unknown is the identity.
        let mut has_never = false;
        let mut has_any = false;
        let mut has_error = false;
        flat.retain(|&m| match self.types.kind(m) {
            TypeKind::Never => {
                has_never = true;
                false
            }
            TypeKind::Error => {
                has_error = true;
                false
            }
            TypeKind::Any => {
                has_any = true;
                false
            }
            TypeKind::Unknown => false,
            _ => true,
        });
        if has_error {
            return self.types.error;
        }
        if has_never {
            return self.types.never;
        }
        if has_any {
            return self.types.any;
        }
        // distribute over a union operand: `(A | B) & R` == `(A & R) | (B & R)`,
        // so `(A | B | C) & { type: 'number' }` reduces each branch (the
        // non-matching ones collapse to `never`) exactly as tsc does. Restricted
        // to a parameter-free remainder: `(1 | 2) & T` stays symbolic so that
        // `T` remains assignable to it.
        if let Some(pos) = flat
            .iter()
            .position(|&m| matches!(self.types.kind(m), TypeKind::Union(_)))
        {
            let mut rest = flat.clone();
            rest.remove(pos);
            if rest.iter().all(|&m| !self.type_contains_params(m)) {
                let union_members = self.types.union_members(flat[pos]);
                let mut parts: Vec<TypeId> = Vec::new();
                for &u in &union_members {
                    let mut branch = rest.clone();
                    branch.push(u);
                    let b = self.intersect_all(branch);
                    // drop uninhabitable branches: a discriminant collision
                    // (`{type:'a'} & {type:'b'}` → a `never` property) removes
                    // that arm so `(A | B | C) & {type:'number'}` reduces to the
                    // matching member, matching tsc's reduced union type.
                    if matches!(self.types.kind(b), TypeKind::Never) || self.has_never_property(b) {
                        continue;
                    }
                    parts.push(b);
                }
                if parts.is_empty() {
                    return self.types.never;
                }
                return self.types.union(parts);
            }
        }
        // partition: type-param operands (defer), empty objects `{}` (nullish
        // filter), and the remaining concrete operands. `ThisType<T>` markers are
        // kept aside and re-attached as distinct members regardless of the other
        // operands: it is a contextual-`this` marker that must survive reduction
        // (`{ move(): void } & ThisType<{ x }>` keeps the marker so the object
        // literal's methods can read `this`), and being empty it adds no
        // assignability constraint.
        let this_sym = self.global_type_symbol("ThisType");
        let mut params: Vec<TypeId> = Vec::new();
        let mut empties = 0usize;
        let mut others: Vec<TypeId> = Vec::new();
        let mut markers: Vec<TypeId> = Vec::new();
        for m in flat {
            if let Some(ts) = this_sym {
                if matches!(self.types.kind(m), TypeKind::Ref(s, _) if *s == ts) {
                    markers.push(m);
                    continue;
                }
            }
            if self.type_contains_params(m) {
                params.push(m);
            } else if self.is_empty_object_type(m) {
                empties += 1;
            } else {
                others.push(m);
            }
        }
        // Reduce the concrete operands; `None` means two disjoint primitives
        // make the intersection empty.
        let reduced = match self.reduce_concrete_members(&others) {
            Some(r) => r,
            None => return self.types.never,
        };
        let core = if params.is_empty() {
            // all-concrete: `& {}` strips nullish; a single member collapses; two
            // or more irreducible members (`string & { name: string }`) stay an
            // intersection.
            match reduced.len() {
                0 => {
                    if empties > 0 {
                        self.empty_object_type()
                    } else {
                        self.types.unknown
                    }
                }
                1 => {
                    if empties > 0 {
                        self.remove_nullish(reduced[0])
                    } else {
                        reduced[0]
                    }
                }
                _ => self.types.intern_kind(TypeKind::Intersection(reduced)),
            }
        } else {
            // A type-parameter operand is present: keep a symbolic `Intersection`
            // node so the operation re-folds once the parameters are concrete
            // (`T & {}` → `string`, `keyof T & string`, `A<T> & B<T>`, …). All
            // consumers — keyof, indexed access, inference, assignability, member
            // lookup, display — handle `Intersection` directly.
            let mut node: Vec<TypeId> = params;
            node.extend(reduced);
            if empties > 0 {
                let e = self.empty_object_type();
                node.push(e);
            }
            let mut seen: Vec<TypeId> = Vec::new();
            node.retain(|&x| {
                if seen.contains(&x) {
                    false
                } else {
                    seen.push(x);
                    true
                }
            });
            if node.len() == 1 {
                node[0]
            } else {
                self.types.intern_kind(TypeKind::Intersection(node))
            }
        };
        if markers.is_empty() {
            return core;
        }
        // re-attach `ThisType<…>` markers (dropping a vacuous `unknown` core).
        let mut all: Vec<TypeId> = Vec::new();
        if !matches!(self.types.kind(core), TypeKind::Unknown) {
            all.push(core);
        }
        all.extend(markers);
        let mut seen: Vec<TypeId> = Vec::new();
        all.retain(|&x| {
            if seen.contains(&x) {
                false
            } else {
                seen.push(x);
                true
            }
        });
        if all.len() == 1 {
            all[0]
        } else {
            self.types.intern_kind(TypeKind::Intersection(all))
        }
    }

    /// Reduce concrete (non-parameter, non-empty-object) intersection operands.
    /// A subtype pair collapses to the narrower side; two disjoint primitives
    /// make the whole intersection empty (`string & number` → `never`).
    /// Distinct object operands are intentionally kept as members so diagnostic
    /// elaboration can preserve the failed intersection constituent. Returns the
    /// irreducible members, or `None` to signal `never`.
    fn reduce_concrete_members(&mut self, members: &[TypeId]) -> Option<Vec<TypeId>> {
        let mut acc: Vec<TypeId> = Vec::new();
        for &m in members {
            let mut placed = false;
            for i in 0..acc.len() {
                let a = acc[i];
                if self.is_assignable_to(m, a) {
                    acc[i] = m; // m is the narrower operand
                    placed = true;
                    break;
                } else if self.is_assignable_to(a, m) {
                    placed = true; // a is the narrower operand; keep it
                    break;
                } else if self.primitive_like(a) && self.primitive_like(m) {
                    return None; // disjoint primitives → never
                } else if self.disjoint_pair(a, m) {
                    return None; // `number & object`, `{a} & null`, … → never
                }
            }
            if !placed {
                acc.push(m);
            }
        }
        Some(acc)
    }

    /// Pairs with no common value beyond the primitive/primitive case already
    /// handled: a primitive intersected with `object` (`number & object`), or
    /// `null`/`undefined` intersected with any concrete type (`{a} & null`).
    fn disjoint_pair(&self, a: TypeId, b: TypeId) -> bool {
        let np = |c: &Self, x: TypeId, y: TypeId| {
            matches!(c.types.kind(x), TypeKind::NonPrimitive) && c.primitive_like(y)
        };
        if np(self, a, b) || np(self, b, a) {
            return true;
        }
        let nullish =
            |c: &Self, x: TypeId| matches!(c.types.kind(x), TypeKind::Null | TypeKind::Undefined);
        nullish(self, a) || nullish(self, b)
    }

    /// Whether an object type carries a `never`-typed property, which makes it
    /// uninhabitable (a reduced intersection collapses to `never`).
    fn has_never_property(&mut self, t: TypeId) -> bool {
        if let Some(sid) = self.shape_of_type(t) {
            let props: Vec<TypeId> = self.types.shape(sid).props.iter().map(|p| p.ty).collect();
            props
                .iter()
                .any(|&pt| matches!(self.types.kind(pt), TypeKind::Never))
        } else {
            false
        }
    }

    /// A primitive (or primitive-literal) type, for which the absence of a
    /// subtype relation means two operands are disjoint.
    fn primitive_like(&self, t: TypeId) -> bool {
        matches!(
            self.types.kind(t),
            TypeKind::String
                | TypeKind::Number
                | TypeKind::Bigint
                | TypeKind::EsSymbol
                | TypeKind::Null
                | TypeKind::Undefined
                | TypeKind::Void
                | TypeKind::StrLit(_)
                | TypeKind::NumLit(_)
                | TypeKind::BigIntLit(_)
                | TypeKind::BoolLit(_)
        )
    }

    /// Shape (members) of a type, if it has object-ish members.
    pub fn shape_of_type(&mut self, t: TypeId) -> Option<ShapeId> {
        if let Some(&s) = self.caches.members_cache.get(&t) {
            return Some(s);
        }
        let shape = match self.types.kind(t).clone() {
            TypeKind::Anon(s) => return Some(s),
            TypeKind::DeferredObj(key) => {
                let entry = self.deferred.deferred_literals.get(&key)?;
                let members = entry.0;
                let scope = entry.1;
                let env = entry.2.clone();
                // resolve the members under the `infer` / mapped-key environment
                // captured when the literal was written, so a synthetic name used
                // inside (`{ [s: string]: U }`) still resolves. Lexically scoped
                // params resolve through the captured `scope` instead.
                let saved = std::mem::replace(&mut self.tp.infer_mapped_env, env);
                let shape = self.shape_of_members(members, scope);
                self.tp.infer_mapped_env = saved;
                shape
            }
            TypeKind::Iface(sym) => self.build_iface_shape(sym, &Mapper::new()),
            TypeKind::Ref(sym, args) => {
                let tparams = self.type_params_of_symbol(sym);
                let mut mapper = Mapper::new();
                for (i, &p) in tparams.iter().enumerate() {
                    if let Some(&a) = args.get(i) {
                        mapper.insert(p, a);
                    }
                }
                self.build_iface_shape(sym, &mapper)
            }
            TypeKind::MappedIface(sym, entries) => {
                let mapper = self.mapper_from_entries(&entries);
                self.build_iface_shape(sym, &mapper)
            }
            TypeKind::ClassStatics(sym) => self.build_statics_shape(sym),
            TypeKind::MappedClassStatics(sym, entries) => {
                let mapper = self.mapper_from_entries(&entries);
                self.build_statics_shape_with_mapper(sym, &mapper)
            }
            TypeKind::NamespaceObj(sym) => {
                let members: Vec<(String, SymbolId)> = self.symbol(sym).members.0.clone();
                let mut shape = Shape::default();
                // merged function+namespace: the value is callable too
                if self.symbol(sym).flags & flags::FUNCTION != 0 {
                    let fdecls: Vec<&'a FunctionLike> = self
                        .symbol(sym)
                        .decls
                        .clone()
                        .into_iter()
                        .filter_map(|d| match d {
                            Decl::Func(f) => Some(f),
                            _ => None,
                        })
                        .collect();
                    for f in fdecls {
                        if f.body.is_some() || self.symbol(sym).decls.len() == 1 {
                            let sig = self.signature_of(f);
                            shape.call_sigs.push(sig);
                        }
                    }
                }
                for (name, mid) in members {
                    let ty = self.type_of_symbol_lazy(mid);
                    let mflags = self.symbol(mid).flags;
                    shape.props.push(PropInfo {
                        name,
                        ty,
                        optional: false,
                        readonly: false,
                        is_method: mflags & flags::FUNCTION != 0,
                        symbol: Some(mid),
                    });
                }
                self.types.alloc_shape(shape)
            }
            TypeKind::EnumObject(sym) => {
                let members: Vec<(String, SymbolId)> = self.symbol(sym).members.0.clone();
                let mut shape = Shape::default();
                for (name, mid) in members {
                    let ty = self.type_of_symbol(mid);
                    shape.props.push(PropInfo {
                        name,
                        ty,
                        optional: false,
                        readonly: true,
                        is_method: false,
                        symbol: Some(mid),
                    });
                }
                self.types.alloc_shape(shape)
            }
            TypeKind::Tuple(elems) | TypeKind::ReadonlyTuple(elems) => {
                // tuples expose Array<unionOfElems> members; a *fixed* tuple (no
                // rest element) additionally narrows `length` to its literal count.
                let elems = elems.clone();
                let elem_union = self.types.union(elems.iter().map(|e| e.ty).collect());
                let arr = self.array_type(elem_union);
                let base = self.shape_of_type(arr)?;
                let has_rest = elems.iter().any(|e| e.rest);
                if has_rest {
                    return Some(base);
                }
                let len_t = self.types.number_lit(elems.len() as f64);
                let mut shape = self.types.shape(base).clone();
                for p in shape.props.iter_mut() {
                    if p.name == "length" {
                        p.ty = len_t;
                        p.readonly = true;
                    }
                }
                self.types.alloc_shape(shape)
            }
            TypeKind::ReadonlyArray(e) => {
                let sym = self.global_type_symbol("ReadonlyArray")?;
                let tparams = self.type_params_of_symbol(sym);
                let mut mapper = Mapper::new();
                if let Some(&p) = tparams.first() {
                    mapper.insert(p, e);
                }
                self.build_iface_shape(sym, &mapper)
            }
            TypeKind::Intersection(members) => {
                // combined apparent members: a property present in several
                // operands takes their intersection; required if required in any.
                let members = members.clone();
                let mut props: Vec<PropInfo> = Vec::new();
                let mut call_sigs = Vec::new();
                let mut ctor_sigs = Vec::new();
                let mut index_infos = Vec::new();
                for &m in &members {
                    // use each member's apparent type so a primitive operand
                    // (`string` in `string & Brand`) contributes its interface
                    // methods (`toLowerCase`, …).
                    let am = self.apparent_type(m);
                    if let Some(sid) = self.shape_of_type(am) {
                        let msh = self.types.shape(sid).clone();
                        for p in msh.props {
                            if let Some(existing) = props.iter_mut().find(|e| e.name == p.name) {
                                let merged = self.intersect_all(vec![existing.ty, p.ty]);
                                existing.ty = merged;
                                existing.optional = existing.optional && p.optional;
                                existing.readonly = existing.readonly || p.readonly;
                            } else {
                                props.push(p);
                            }
                        }
                        call_sigs.extend(msh.call_sigs);
                        ctor_sigs.extend(msh.ctor_sigs);
                        index_infos.extend(msh.index_infos);
                    }
                }
                self.types.alloc_shape(Shape {
                    props,
                    call_sigs,
                    ctor_sigs,
                    index_infos,
                })
            }
            _ => return None,
        };
        self.caches.members_cache.insert(t, shape);
        Some(shape)
    }

    fn build_iface_shape(&mut self, sym: SymbolId, mapper: &Mapper) -> ShapeId {
        self.with_this_type(sym, |c| c.build_iface_shape_inner(sym, mapper))
    }

    fn build_iface_shape_inner(&mut self, sym: SymbolId, mapper: &Mapper) -> ShapeId {
        // own members from all decls (merged), then inherited
        let mut shape = Shape::default();
        let member_ids: Vec<(String, SymbolId)> = self.symbol(sym).members.0.clone();
        for (name, mid) in &member_ids {
            let mflags = self.symbol(*mid).flags;
            let t0 = self.type_of_symbol(*mid);
            let t = self.instantiate_type(t0, mapper);
            // an accessor with a getter but no setter is read-only
            let is_getter_only =
                mflags & flags::GET_ACCESSOR != 0 && mflags & flags::SET_ACCESSOR == 0;
            let readonly = mflags & flags::READONLY != 0 || is_getter_only;
            shape.props.push(PropInfo {
                name: name.clone(),
                ty: t,
                optional: mflags & flags::OPTIONAL != 0,
                readonly,
                is_method: mflags & flags::METHOD != 0,
                symbol: Some(*mid),
            });
        }
        // call/ctor/index signatures from interface decls
        let decls = self.symbol(sym).decls.clone();
        for d in &decls {
            if let Decl::Interface(i) = d {
                let scope = self
                    .bind
                    .node_scope
                    .get(&node_key(*i))
                    .copied()
                    .unwrap_or(self.bind.global_scope);
                for m in &i.members {
                    match m {
                        TypeMember::Call(cs) => {
                            let sig = self.call_signature(cs, scope);
                            let sig = self.instantiate_sig(sig, mapper);
                            shape.call_sigs.push(sig);
                        }
                        TypeMember::Ctor(cs) => {
                            let sig = self.call_signature(cs, scope);
                            let sig = self.instantiate_sig(sig, mapper);
                            shape.ctor_sigs.push(sig);
                        }
                        TypeMember::Index(idx) => {
                            let key = self.resolve_type(&idx.key_type, scope);
                            let value0 = self.resolve_type(&idx.value_type, scope);
                            let value = self.instantiate_type(value0, mapper);
                            shape.index_infos.push(IndexInfo {
                                key,
                                value,
                                readonly: idx.readonly,
                            });
                        }
                        _ => {}
                    }
                }
            }
            if let Decl::Class(c) = d {
                // a class body's index signature applies to its instances
                let scope = self
                    .bind
                    .node_scope
                    .get(&node_key(*c))
                    .copied()
                    .unwrap_or(self.bind.global_scope);
                for m in &c.members {
                    if let ClassMember::Index(idx) = m {
                        let key = self.resolve_type(&idx.key_type, scope);
                        let value0 = self.resolve_type(&idx.value_type, scope);
                        let value = self.instantiate_type(value0, mapper);
                        shape.index_infos.push(IndexInfo {
                            key,
                            value,
                            readonly: idx.readonly,
                        });
                    }
                }
            }
        }
        // inherited: interface extends / class extends — the BaseType slot
        // guards the entire resolve+merge so self-referential bases (2310)
        // terminate instead of recursing.
        let in_cycle = self
            .res
            .resolving
            .iter()
            .any(|(s, slot)| *s == sym && *slot == Slot::BaseType);
        if in_cycle {
            if self.cflags.in_heritage_expr == 0 {
                self.report_base_cycle(sym);
            }
            return self.types.alloc_shape(shape);
        }
        self.res.resolving.push((sym, Slot::BaseType));
        for d in &decls {
            match d {
                Decl::Interface(i) => {
                    let scope = self
                        .bind
                        .node_scope
                        .get(&node_key(*i))
                        .copied()
                        .unwrap_or(self.bind.global_scope);
                    for ext in &i.extends {
                        let t = self.resolve_type_ref_pub(ext, scope);
                        let t = self.instantiate_type(t, mapper);
                        // Members from an extended interface/type are inherited
                        // (own members already in `shape` take precedence).
                        self.merge_base_into_shape(&mut shape, t);
                    }
                }
                Decl::Class(c) => {
                    if let Some(h) = &c.extends {
                        if let Some(base) = self.base_instance_type(c, h) {
                            let base = self.instantiate_type(base, mapper);
                            self.merge_base_into_shape(&mut shape, base);
                        }
                    }
                }
                _ => {}
            }
        }
        self.res.resolving.pop();
        self.types.alloc_shape(shape)
    }

    pub(crate) fn report_base_cycle(&mut self, sym: SymbolId) {
        if self.res.resolution_failed.insert((sym, Slot::BaseType)) {
            let name = self.symbol(sym).name.clone();
            let span = self.symbol(sym).decls.first().map(|d| d.name_span());
            if let Some(span) = span {
                let file = self.symbol(sym).file;
                let prev = self.current_file;
                self.current_file = file;
                // classes report against the base EXPRESSION (2506);
                // interfaces against the base TYPE (2310)
                if self.symbol(sym).flags & flags::CLASS != 0 {
                    self.error_at(
                        span,
                        &gen::_0_is_referenced_directly_or_indirectly_in_its_own_base_expression,
                        &[name],
                    );
                } else {
                    self.error_at(
                        span,
                        &gen::Type_0_recursively_references_itself_as_a_base_type,
                        &[name],
                    );
                }
                self.current_file = prev;
            }
        }
    }

    /// the directly-extended class symbol, if any
    pub fn base_class_of(&mut self, sym: SymbolId) -> Option<(SymbolId, ())> {
        let decls = self.symbol(sym).decls.clone();
        for d in decls {
            if let Decl::Class(c) = d {
                if let Some(h) = &c.extends {
                    let scope = self
                        .bind
                        .node_scope
                        .get(&node_key(c))
                        .copied()
                        .unwrap_or(self.bind.global_scope);
                    if let Some(bsym) = self.class_symbol_from_expr(scope, &h.expr) {
                        if bsym != sym {
                            return Some((bsym, ()));
                        }
                    }
                }
            }
        }
        None
    }

    pub(crate) fn class_symbol_from_expr(
        &mut self,
        scope: crate::binder::ScopeId,
        expr: &'a Expr,
    ) -> Option<SymbolId> {
        match expr {
            Expr::Ident(id) => self
                .lookup_value(scope, &id.name)
                .map(|s| self.resolve_alias_chain(s))
                .filter(|&s| self.symbol(s).flags & flags::CLASS != 0),
            Expr::Paren { inner, .. } => self.class_symbol_from_expr(scope, inner),
            Expr::PropAccess {
                obj,
                name,
                question_dot: false,
                ..
            } => {
                let ns = self.namespace_symbol_from_expr(scope, obj)?;
                self.symbol(ns)
                    .members
                    .get(&name.name)
                    .map(|s| self.resolve_alias_chain(s))
                    .filter(|&s| self.symbol(s).flags & flags::CLASS != 0)
            }
            _ => None,
        }
    }

    fn namespace_symbol_from_expr(
        &mut self,
        scope: crate::binder::ScopeId,
        expr: &'a Expr,
    ) -> Option<SymbolId> {
        match expr {
            Expr::Ident(id) => self
                .lookup_type(scope, &id.name)
                .or_else(|| self.lookup_value(scope, &id.name))
                .map(|s| self.resolve_alias_chain(s))
                .filter(|&s| self.symbol(s).flags & (flags::NAMESPACE | flags::ENUM) != 0),
            Expr::Paren { inner, .. } => self.namespace_symbol_from_expr(scope, inner),
            Expr::PropAccess {
                obj,
                name,
                question_dot: false,
                ..
            } => {
                let ns = self.namespace_symbol_from_expr(scope, obj)?;
                self.symbol(ns)
                    .members
                    .get(&name.name)
                    .map(|s| self.resolve_alias_chain(s))
                    .filter(|&s| self.symbol(s).flags & (flags::NAMESPACE | flags::ENUM) != 0)
            }
            _ => None,
        }
    }

    pub(crate) fn class_value_type(&mut self, sym: SymbolId) -> TypeId {
        let statics = self.types.intern_kind(TypeKind::ClassStatics(sym));
        let Some(base) = self.class_value_mixin_base_type(sym) else {
            return statics;
        };
        if self.types.is_any_or_error(base) || base == statics {
            return statics;
        }
        self.intersect_all(vec![statics, base])
    }

    fn class_value_mixin_base_type(&mut self, sym: SymbolId) -> Option<TypeId> {
        let decls = self.symbol(sym).decls.clone();
        for d in decls {
            if let Decl::Class(c) = d {
                if !self.is_mixin_constructor_class(c) {
                    continue;
                }
                let Some(h) = &c.extends else { continue };
                let et = self.extends_expr_static_type(c, h)?;
                let rt = self.types.regular(et);
                if self.types.is_any_or_error(rt)
                    || matches!(
                        self.types.kind(rt),
                        TypeKind::ClassStatics(_) | TypeKind::MappedClassStatics(_, _)
                    )
                {
                    continue;
                }
                if self.ctor_signatures_of(rt).is_empty() {
                    continue;
                }
                return Some(rt);
            }
        }
        None
    }

    fn is_mixin_constructor_class(&mut self, c: &'a ClassDecl) -> bool {
        let mut ctors = c.members.iter().filter_map(|m| match m {
            ClassMember::Constructor(f) => Some(&**f),
            _ => None,
        });
        let Some(ctor) = ctors.next() else {
            return true;
        };
        if ctors.next().is_some() {
            return false;
        }
        if ctor.params.len() != 1 {
            return false;
        }
        let p = &ctor.params[0];
        if !p.dotdotdot {
            return false;
        }
        let scope = self
            .bind
            .node_scope
            .get(&node_key(ctor))
            .copied()
            .unwrap_or(self.bind.global_scope);
        let element = match &p.ty {
            Some(ty) => {
                let at = self.resolve_type(ty, scope);
                self.array_element_type(at).unwrap_or(self.types.any)
            }
            None => self.types.any,
        };
        matches!(self.types.kind(element), TypeKind::Any)
    }

    pub(crate) fn extends_expr_static_type(
        &mut self,
        c: &'a ClassDecl,
        h: &'a HeritageClause,
    ) -> Option<TypeId> {
        let scope = self
            .bind
            .node_scope
            .get(&node_key(c))
            .copied()
            .unwrap_or(self.bind.global_scope);
        self.cflags.in_heritage_expr += 1;
        let t = self.with_current_scope(scope, |this| this.check_expr(&h.expr, None));
        self.cflags.in_heritage_expr -= 1;
        Some(t)
    }

    fn is_nominal_class_instance_type(&self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::Iface(sym) | TypeKind::Ref(sym, _) | TypeKind::MappedIface(sym, _) => {
                self.symbol(*sym).flags & flags::CLASS != 0
            }
            _ => false,
        }
    }

    fn constructor_base_value_type(&mut self, et: TypeId) -> Option<TypeId> {
        let rt = self.types.regular(et);
        match self.types.kind(rt).clone() {
            TypeKind::TypeParam(tp) => {
                let c = self.constraint_of_type_param(tp)?;
                let cr = self.types.regular(c);
                if self.is_nominal_class_instance_type(cr) {
                    return None;
                }
                Some(cr)
            }
            _ => Some(rt),
        }
    }

    pub fn base_instance_type(
        &mut self,
        c: &'a ClassDecl,
        h: &'a HeritageClause,
    ) -> Option<TypeId> {
        // extends expression must be a class/constructor value. Identifier
        // bases can apply explicit heritage type arguments; non-ident bases
        // (class expressions, mixin calls, `as any`) use the expression type.
        let scope = self
            .bind
            .node_scope
            .get(&node_key(c))
            .copied()
            .unwrap_or(self.bind.global_scope);
        if let Expr::Ident(id) = &h.expr {
            let sym = self.lookup_value(scope, &id.name)?;
            let sym = self.resolve_alias_chain(sym);
            if self.symbol(sym).flags & flags::CLASS != 0 {
                let tparams = self.type_params_of_symbol(sym);
                if tparams.is_empty() {
                    return Some(self.types.intern_kind(TypeKind::Iface(sym)));
                }
                let args: Vec<TypeId> = match &h.type_args {
                    Some(args) => args.iter().map(|a| self.resolve_type(a, scope)).collect(),
                    None => tparams.iter().map(|_| self.types.any).collect(),
                };
                return Some(self.types.ref_type(sym, args));
            }
        }
        let vt = self.extends_expr_static_type(c, h)?;
        if let Some(args) = &h.type_args {
            return self.instance_type_from_extends_static_with_type_args(vt, args, scope);
        }
        self.instance_type_from_extends_static(vt)
    }

    pub(crate) fn instance_type_from_extends_static(&mut self, et: TypeId) -> Option<TypeId> {
        let rt = self.constructor_base_value_type(et)?;
        if self.types.is_any_or_error(rt) {
            return Some(rt);
        }
        let ctor_sigs = self.ctor_signatures_of(rt);
        ctor_sigs.first().map(|&sig| self.sig_return(sig))
    }

    fn instance_type_from_extends_static_with_type_args(
        &mut self,
        et: TypeId,
        args: &'a [TypeNode],
        scope: crate::binder::ScopeId,
    ) -> Option<TypeId> {
        let rt = self.constructor_base_value_type(et)?;
        if self.types.is_any_or_error(rt) {
            return Some(rt);
        }
        let ctor_sigs = self.ctor_signatures_of(rt);
        let arg_types: Vec<TypeId> = args.iter().map(|a| self.resolve_type(a, scope)).collect();
        for sig in ctor_sigs.iter().copied() {
            let s = self.types.sig(sig).clone();
            if s.type_params.len() != arg_types.len() {
                continue;
            }
            let mut mapper = Mapper::new();
            for (tp, arg) in s.type_params.iter().copied().zip(arg_types.iter().copied()) {
                mapper.insert(tp, arg);
            }
            let inst = self.instantiate_sig(sig, &mapper);
            return Some(self.sig_return(inst));
        }
        ctor_sigs.first().map(|&sig| self.sig_return(sig))
    }

    pub(crate) fn base_static_type_from_extends_type(&mut self, et: TypeId) -> Option<TypeId> {
        let rt = self.constructor_base_value_type(et)?;
        if self.types.is_any_or_error(rt) || !self.ctor_signatures_of(rt).is_empty() {
            return Some(rt);
        }
        None
    }

    fn merge_base_into_shape(&mut self, shape: &mut Shape, base: TypeId) {
        if let Some(bs) = self.shape_of_type(base) {
            let bshape = self.types.shape(bs).clone();
            for p in bshape.props {
                if shape.prop(&p.name).is_none() {
                    shape.props.push(p);
                }
            }
            for s in bshape.call_sigs {
                shape.call_sigs.push(s);
            }
            for s in bshape.ctor_sigs {
                shape.ctor_sigs.push(s);
            }
            for i in bshape.index_infos {
                shape.index_infos.push(i);
            }
        }
    }

    fn build_statics_shape(&mut self, sym: SymbolId) -> ShapeId {
        self.build_statics_shape_with_mapper(sym, &Mapper::new())
    }

    fn build_statics_shape_with_mapper(&mut self, sym: SymbolId, mapper: &Mapper) -> ShapeId {
        let mut shape = Shape::default();
        let statics: Vec<(String, SymbolId)> = self.symbol(sym).statics.0.clone();
        for (name, mid) in &statics {
            let mflags = self.symbol(*mid).flags;
            let t0 = self.type_of_symbol(*mid);
            let t = self.instantiate_type(t0, mapper);
            shape.props.push(PropInfo {
                name: name.clone(),
                ty: t,
                optional: mflags & flags::OPTIONAL != 0,
                readonly: mflags & flags::READONLY != 0,
                is_method: mflags & flags::METHOD != 0,
                symbol: Some(*mid),
            });
        }
        // constructor signatures
        let class_tps = self.type_params_of_symbol(sym);
        // an abstract class's construct signatures are abstract: `new`-ing the
        // static side (directly or via a mixin intersection) is an error (2511).
        let class_is_abstract = self.symbol(sym).decls.iter().any(
            |d| matches!(d, Decl::Class(c) if has_modifier(&c.modifiers, ModifierKind::Abstract)),
        );
        let instance = if class_tps.is_empty() {
            self.mapped_iface_type(sym, mapper)
        } else {
            // a generic class constructs `C<T...>`; the ctor signature carries the
            // class type parameters so `new C(arg)` can infer them from arguments.
            let args: Vec<TypeId> = class_tps
                .iter()
                .map(|&tp| self.types.intern_kind(TypeKind::TypeParam(tp)))
                .collect();
            let instance = self.types.ref_type(sym, args);
            if mapper.is_empty() {
                instance
            } else {
                self.mapped_iface_type(sym, mapper)
            }
        };
        let mut ctor_sigs = Vec::new();
        let decls = self.symbol(sym).decls.clone();
        for d in &decls {
            if let Decl::Class(c) = d {
                for m in &c.members {
                    if let ClassMember::Constructor(f) = m {
                        let scope = self
                            .bind
                            .node_scope
                            .get(&node_key(&**f))
                            .copied()
                            .unwrap_or(self.bind.global_scope);
                        let (params, min_args, rest, rest_name) =
                            self.params_of_kind(&f.params, scope, f.kind);
                        let rest_tp = self.tp.rest_tp_scratch;
                        let sig0 = self.types.alloc_sig(Signature {
                            type_params: class_tps.clone(),
                            params,
                            min_args,
                            rest,
                            rest_name,
                            rest_tp,
                            ret: instance,
                            decl_key: 0,
                            from_method: false,
                            ret_annotation_never: false,
                            predicate: None,
                            is_abstract: class_is_abstract,
                        });
                        let sig = self.instantiate_sig(sig0, mapper);
                        ctor_sigs.push(sig);
                    }
                }
            }
        }
        // Guard the base-class recursion below (inherited constructors and
        // inherited statics) against circular inheritance — `class C extends D {}
        // class D extends C {}` — which would otherwise recurse through each
        // class's static shape forever. The cycle is reported as 2506 during
        // instance-shape resolution; here we just stop before re-entering.
        let static_base_cycle = self
            .res
            .resolving
            .iter()
            .any(|(s, slot)| *s == sym && *slot == Slot::StaticBaseType);
        if !static_base_cycle {
            self.res.resolving.push((sym, Slot::StaticBaseType));
        }
        if ctor_sigs.is_empty() && !static_base_cycle {
            // default constructor (or inherited)
            let mut inherited = None;
            for d in &decls {
                if let Decl::Class(c) = d {
                    if let Some(h) = &c.extends {
                        if let Expr::Ident(id) = &h.expr {
                            let scope = self
                                .bind
                                .node_scope
                                .get(&node_key(*c))
                                .copied()
                                .unwrap_or(self.bind.global_scope);
                            if let Some(bsym) = self.lookup_value(scope, &id.name) {
                                let bsym = self.resolve_alias_chain(bsym);
                                if self.symbol(bsym).flags & flags::CLASS != 0 {
                                    let bstat =
                                        self.types.intern_kind(TypeKind::ClassStatics(bsym));
                                    if let Some(bshape) = self.shape_of_type(bstat) {
                                        let base_ctors = self.types.shape(bshape).ctor_sigs.clone();
                                        if !base_ctors.is_empty() {
                                            // substitute the base class's type
                                            // parameters with the `extends
                                            // Base<...>` arguments so an inherited
                                            // constructor exposes concrete
                                            // parameter types; the return type is
                                            // this class's own instance.
                                            let base_tps = self.type_params_of_symbol(bsym);
                                            let mut base_mapper = Mapper::new();
                                            if !base_tps.is_empty() {
                                                if let Some(targs) = &h.type_args {
                                                    for (i, &tp) in base_tps.iter().enumerate() {
                                                        if let Some(node) = targs.get(i) {
                                                            let ty = self.resolve_type(node, scope);
                                                            base_mapper.insert(tp, ty);
                                                        }
                                                    }
                                                }
                                            }
                                            let mut sigs = Vec::new();
                                            for bsig in base_ctors {
                                                let inst = self.instantiate_sig(bsig, &base_mapper);
                                                let mut s = self.types.sig(inst).clone();
                                                s.type_params = class_tps.clone();
                                                s.ret = instance;
                                                s.rest_tp = None;
                                                s.decl_key = 0;
                                                s.predicate = None;
                                                let sig = self.types.alloc_sig(s);
                                                sigs.push(self.instantiate_sig(sig, mapper));
                                            }
                                            inherited = Some(sigs);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            ctor_sigs = inherited.unwrap_or_else(|| {
                let sig = self.types.alloc_sig(Signature {
                    type_params: class_tps.clone(),
                    params: Vec::new(),
                    min_args: 0,
                    rest: None,
                    rest_name: None,
                    rest_tp: None,
                    ret: instance,
                    decl_key: 0,
                    from_method: false,
                    ret_annotation_never: false,
                    predicate: None,
                    is_abstract: class_is_abstract,
                });
                vec![self.instantiate_sig(sig, mapper)]
            });
        }
        // A construct signature's abstractness follows the *derived* class, not
        // the base it was inherited from: a concrete `class B extends Abstract`
        // is constructable even though it inherits Abstract's signature. Re-stamp
        // inherited signatures so `new B` is allowed and `new C` (C abstract) is
        // still rejected.
        let ctor_sigs: Vec<crate::types::SigId> = ctor_sigs
            .into_iter()
            .map(|s| {
                if self.types.sig(s).is_abstract == class_is_abstract {
                    s
                } else {
                    let mut sig = self.types.sig(s).clone();
                    sig.is_abstract = class_is_abstract;
                    self.types.alloc_sig(sig)
                }
            })
            .collect();
        shape.ctor_sigs = ctor_sigs;
        // inherited statics from the base class
        if !static_base_cycle {
            if let Some((bsym, _)) = self.base_class_of(sym) {
                if bsym != sym
                    && !self
                        .res
                        .resolving
                        .iter()
                        .any(|(s, slot)| *s == bsym && *slot == Slot::BaseType)
                {
                    let bt = self.types.intern_kind(TypeKind::ClassStatics(bsym));
                    if let Some(bsid) = self.shape_of_type(bt) {
                        let bprops = self.types.shape(bsid).props.clone();
                        for bp in bprops {
                            if shape.prop(&bp.name).is_none() {
                                shape.props.push(bp);
                            }
                        }
                    }
                }
            }
        }
        if !static_base_cycle {
            self.res.resolving.pop();
        }
        // merged class+namespace: namespace exports become statics
        if self.symbol(sym).flags & flags::NAMESPACE != 0 {
            let members: Vec<(String, SymbolId)> = self.symbol(sym).members.0.clone();
            for (name, mid) in members {
                if shape.prop(&name).is_none() {
                    let ty = self.type_of_symbol_lazy(mid);
                    shape.props.push(PropInfo {
                        name,
                        ty,
                        optional: false,
                        readonly: false,
                        is_method: false,
                        symbol: Some(mid),
                    });
                }
            }
        }
        self.types.alloc_shape(shape)
    }

    /// The apparent constraint of an indexed access with a literal key. The
    /// indexed access type itself stays symbolic, but member access can use this
    /// concrete/apparent constraint (`Readonly<T & { foo: string }>['foo']`
    /// presents string members while preserving the indexed-access identity).
    pub(crate) fn indexed_access_base_constraint(&mut self, t: TypeId) -> Option<TypeId> {
        if let TypeKind::IndexedAccess(obj, idx) = self.types.kind(t).clone() {
            if let TypeKind::StrLit(name) = self.types.kind(idx).clone() {
                let name = name.to_str_lossy().into_owned();
                if let Some(resolved) = self.indexed_access_property_constraint(obj, &name) {
                    return Some(resolved);
                }
            }
            if matches!(self.types.kind(idx), TypeKind::NumLit(_)) {
                let obj_c = self.apparent_type(obj);
                if obj_c != obj {
                    let resolved = self.indexed_access_type(obj_c, idx, None);
                    if !matches!(
                        self.types.kind(resolved),
                        TypeKind::IndexedAccess(..) | TypeKind::Error
                    ) {
                        return Some(resolved);
                    }
                }
            }
        }
        None
    }

    fn indexed_access_property_constraint(&mut self, obj: TypeId, name: &str) -> Option<TypeId> {
        match self.types.kind(obj).clone() {
            TypeKind::Intersection(members) => {
                let mut parts = Vec::new();
                for m in members {
                    if let Some(p) = self.prop_of_type(m, name) {
                        parts.push(p);
                    }
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(self.intersect_all(parts))
                }
            }
            _ => {
                let obj_c = self.apparent_type(obj);
                if obj_c == obj {
                    return None;
                }
                self.prop_of_type(obj_c, name)
            }
        }
    }

    /// The instance type a constructor value produces, for `x instanceof Ctor`
    /// narrowing when `Ctor` is not a class declaration but a constructor-typed
    /// value (e.g. the global `Error: ErrorConstructor`). Prefers the
    /// `prototype` property type, falling back to a construct signature's return
    /// type. An `any` result is ignored as uninformative.
    pub(crate) fn instance_type_from_constructor(&mut self, t: TypeId) -> Option<TypeId> {
        let sid = self.shape_of_type(t)?;
        if let Some(p) = self.types.shape(sid).prop("prototype").map(|p| p.ty) {
            if !matches!(self.types.kind(p), TypeKind::Any) {
                return Some(p);
            }
        }
        let ctor_sigs = self.types.shape(sid).ctor_sigs.clone();
        if let Some(&sig) = ctor_sigs.first() {
            let ret = self.sig_return(sig);
            // tsc getInstanceType erases the construct signature's type
            // parameters to `any`: `new <T>(x: T): B<T>` narrows instanceof
            // candidates against B<any>, so B<number> stays in the union
            // instead of collapsing to the raw generic B<T>
            let tps = self.types.sig(sig).type_params.clone();
            let ret = if tps.is_empty() {
                ret
            } else {
                let entries: Vec<_> = tps.iter().map(|&p| (p, self.types.any)).collect();
                let mapper = self.mapper_from_entries(&entries);
                self.instantiate_type(ret, &mapper)
            };
            if !matches!(self.types.kind(ret), TypeKind::Any) {
                return Some(ret);
            }
        }
        None
    }

    /// The *contextual* type of property `name` drawn from `t`. Like
    /// `prop_of_type`, but when `t` is a union the result is the union of the
    /// property over the members that *have* it (members lacking it are skipped,
    /// not disqualifying). This is what contextual typing needs: an object
    /// literal property whose contextual type comes from one arm of a
    /// discriminated union (`l` exists only on the `node` arm of a tree type)
    /// must still see that arm's type, whereas property *access* requires the
    /// property on every member and so uses `prop_of_type`.
    pub fn contextual_prop_type(&mut self, t: TypeId, name: &str) -> Option<TypeId> {
        let apparent = self.apparent_type(t);
        if let TypeKind::Union(members) = self.types.kind(apparent).clone() {
            let mut tys: Vec<TypeId> = Vec::new();
            for m in members {
                if let Some(pt) = self.contextual_prop_type(m, name) {
                    tys.push(pt);
                }
            }
            if tys.is_empty() {
                return None;
            }
            return Some(self.types.union(tys));
        }
        if let Some(p) = self.deferred_mapped_prop_info(apparent, name) {
            return Some(p.ty);
        }
        let shape = self.shape_of_type(apparent)?;
        if let Some(pt) = self.types.shape(shape).prop(name).map(|p| p.ty) {
            return Some(pt);
        }
        // Fall back to an index signature: a property with no matching named
        // member takes its contextual type from the number index signature (for a
        // numeric key) or the string index signature. This contextually types a
        // value written under `{ [k: string]: (s: T) => T }`, so its callback
        // parameter is not implicitly `any`.
        let infos = self.types.shape(shape).index_infos.clone();
        if name.parse::<f64>().is_ok() {
            for info in &infos {
                if matches!(self.types.kind(info.key), TypeKind::Number) {
                    return Some(info.value);
                }
            }
        }
        for info in &infos {
            if matches!(self.types.kind(info.key), TypeKind::String) {
                return Some(info.value);
            }
        }
        None
    }

    pub fn prop_of_type(&mut self, t: TypeId, name: &str) -> Option<TypeId> {
        let apparent = self.apparent_type(t);
        // The property of a union type is the union of that property over every
        // member; if any member lacks it, the property is not accessible. (The
        // boolean union is already mapped to the `Boolean` interface by
        // `apparent_type`, so it does not reach this branch.)
        if let TypeKind::Union(members) = self.types.kind(apparent).clone() {
            let mut tys = Vec::with_capacity(members.len());
            for m in members {
                tys.push(self.prop_of_type(m, name)?);
            }
            return Some(self.types.union(tys));
        }
        if let Some(p) = self.deferred_mapped_prop_info(apparent, name) {
            return Some(p.ty);
        }
        let shape = self.shape_of_type(apparent)?;
        let s = self.types.shape(shape);
        if let Some(p) = s.prop(name) {
            return Some(p.ty);
        }
        None
    }

    pub fn prop_info_of_type(&mut self, t: TypeId, name: &str) -> Option<PropInfo> {
        let apparent = self.apparent_type(t);
        if let Some(p) = self.deferred_mapped_prop_info(apparent, name) {
            return Some(p);
        }
        let shape = self.shape_of_type(apparent)?;
        self.types.shape(shape).prop(name).cloned()
    }
}
