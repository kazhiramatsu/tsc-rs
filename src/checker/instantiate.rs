//! Type instantiation: substituting type-parameter mappers through types and
//! signatures (the generic-application engine). Split out of `symbols.rs`.

use crate::checker::symbols::Mapper;
use crate::checker::Checker;
use crate::types::{IndexInfo, ParamInfo, PropInfo, Shape, Signature, TupleElem, TypeId, TypeKind};

impl<'a> Checker<'a> {
    pub(crate) fn mapper_from_entries(
        &self,
        entries: &[(crate::binder::SymbolId, TypeId)],
    ) -> Mapper {
        entries.iter().copied().collect()
    }

    pub(crate) fn mapper_entries_from_mapper(
        &self,
        mapper: &Mapper,
    ) -> Vec<(crate::binder::SymbolId, TypeId)> {
        let mut entries: Vec<_> = mapper
            .iter()
            .filter_map(|(&k, &v)| {
                if self.mapper_entry_is_identity(k, v) {
                    None
                } else {
                    Some((k, v))
                }
            })
            .collect();
        entries.sort_by_key(|(k, _)| k.0);
        entries
    }

    fn mapper_entry_is_identity(&self, key: crate::binder::SymbolId, value: TypeId) -> bool {
        matches!(self.types.kind(value), TypeKind::TypeParam(sym) if *sym == key)
    }

    fn mapper_is_effectively_empty(&self, mapper: &Mapper) -> bool {
        mapper
            .iter()
            .all(|(&key, &value)| self.mapper_entry_is_identity(key, value))
    }

    pub(crate) fn mapped_iface_type(
        &mut self,
        sym: crate::binder::SymbolId,
        mapper: &Mapper,
    ) -> TypeId {
        let entries = self.mapper_entries_from_mapper(mapper);
        if entries.is_empty() {
            return self.types.intern_kind(TypeKind::Iface(sym));
        }
        self.types.intern_kind(TypeKind::MappedIface(sym, entries))
    }

    pub(crate) fn mapped_class_statics_type(
        &mut self,
        sym: crate::binder::SymbolId,
        mapper: &Mapper,
    ) -> TypeId {
        let entries = self.mapper_entries_from_mapper(mapper);
        if entries.is_empty() {
            return self.types.intern_kind(TypeKind::ClassStatics(sym));
        }
        self.types
            .intern_kind(TypeKind::MappedClassStatics(sym, entries))
    }

    fn compose_mapped_entries(
        &mut self,
        entries: &[(crate::binder::SymbolId, TypeId)],
        mapper: &Mapper,
    ) -> Mapper {
        let mut out = Mapper::new();
        for &(k, v) in entries {
            out.insert(k, self.instantiate_type(v, mapper));
        }
        for (&k, &v) in mapper {
            out.entry(k).or_insert(v);
        }
        out
    }

    pub fn instantiate_type(&mut self, t: TypeId, mapper: &Mapper) -> TypeId {
        if mapper.is_empty() || self.mapper_is_effectively_empty(mapper) {
            return t;
        }
        // Three complementary bounds against infinitely-expanding types, mirroring
        // tsc (each collapses the type to `error`, matching tsc's TS2589):
        //   * `inst_depth` stops a single deeply-recursive instantiation chain
        //     (`type Flat<T> = T extends (infer U)[] ? Flat<U> : T`);
        //   * the relation comparator's isDeeplyNestedType (relations.rs) stops
        //     the *relation-driven* expansion that emits unboundedly many shallow
        //     instantiations — the primary guard for mutually-recursive generics
        //     like a `Vector<T> implements Seq<T>` whose method returns
        //     `Vector<Exclude<T, U>>`;
        //   * `inst_count` is a cumulative backstop (tsc's instantiationCount) for
        //     anything the first two miss; it resets per check_program, so it is
        //     per-program and cannot trip on legitimate generic-heavy code.
        self.guards.inst_count += 1;
        if self.guards.inst_depth > 100 || self.guards.inst_count > 5_000_000 {
            return self.types.error;
        }
        self.guards.inst_depth += 1;
        let r = self.instantiate_type_inner(t, mapper);
        self.guards.inst_depth -= 1;
        r
    }

    fn instantiate_type_inner(&mut self, t: TypeId, mapper: &Mapper) -> TypeId {
        match self.types.kind(t).clone() {
            TypeKind::TypeParam(sym) => mapper.get(&sym).copied().unwrap_or(t),
            TypeKind::Union(members) => {
                let ms: Vec<TypeId> = members
                    .iter()
                    .map(|&m| self.instantiate_type(m, mapper))
                    .collect();
                self.types.union(ms)
            }
            TypeKind::Intersection(members) => {
                // substitute each operand, then re-fold: a deferred `T & {}`
                // collapses to a concrete type once `T` is known.
                let ms: Vec<TypeId> = members
                    .iter()
                    .map(|&m| self.instantiate_type(m, mapper))
                    .collect();
                self.intersect_all(ms)
            }
            TypeKind::Ref(sym, args) => {
                let new_args: Vec<TypeId> = args
                    .iter()
                    .map(|&a| self.instantiate_type(a, mapper))
                    .collect();
                if new_args == args {
                    t
                } else {
                    self.types.ref_type(sym, new_args)
                }
            }
            TypeKind::ClassStatics(sym) => self.mapped_class_statics_type(sym, mapper),
            TypeKind::MappedClassStatics(sym, entries) => {
                let composed = self.compose_mapped_entries(&entries, mapper);
                self.mapped_class_statics_type(sym, &composed)
            }
            TypeKind::MappedIface(sym, entries) => {
                let composed = self.compose_mapped_entries(&entries, mapper);
                self.mapped_iface_type(sym, &composed)
            }
            TypeKind::ReadonlyArray(e) => {
                let ne = self.instantiate_type(e, mapper);
                if ne == e {
                    t
                } else {
                    self.types.intern_kind(TypeKind::ReadonlyArray(ne))
                }
            }
            TypeKind::Keyof(inner) => {
                let ni = self.instantiate_type(inner, mapper);
                if ni == inner {
                    t
                } else if matches!(
                    self.types.kind(ni),
                    TypeKind::Iface(_)
                        | TypeKind::Ref(..)
                        | TypeKind::TypeParam(_)
                        | TypeKind::EnumType(_)
                ) {
                    self.types.intern_kind(TypeKind::Keyof(ni))
                } else {
                    self.keyof_union(ni)
                }
            }
            TypeKind::IndexedAccess(obj, idx) => {
                let no = self.instantiate_type(obj, mapper);
                let ni = self.instantiate_type(idx, mapper);
                if no == obj && ni == idx {
                    t
                } else {
                    self.indexed_access_type(no, ni, None)
                }
            }
            TypeKind::ReadonlyTuple(elems) => {
                let nelems: Vec<crate::types::TupleElem> = elems
                    .iter()
                    .map(|e| crate::types::TupleElem {
                        ty: self.instantiate_type(e.ty, mapper),
                        ..*e
                    })
                    .collect();
                if nelems == elems {
                    t
                } else {
                    self.types.intern_kind(TypeKind::ReadonlyTuple(nelems))
                }
            }
            TypeKind::DeferredCond(key, captured) => {
                let Some(&(node, scope, file)) = self.deferred.deferred_conds.get(&key) else {
                    return t;
                };
                let prev_file = self.current_file;
                self.current_file = file;
                // The incoming mapper supplies bindings for otherwise-free
                // parameters; the captured bindings are this conditional's own
                // arguments (e.g. the `U` in a recursive `Flat<U>`) and must win
                // for shared keys, otherwise an outer instantiation of the same
                // alias would clobber the recursive argument and loop. Captured
                // values are in old terms, so resolve them against the incoming
                // mapper first.
                let mut composed: Mapper = Mapper::new();
                for (k, v) in mapper {
                    composed.insert(*k, *v);
                }
                for (k, v) in captured {
                    if v != t {
                        let nv = self.instantiate_type(v, mapper);
                        composed.insert(k, nv);
                    }
                }
                let r = self.evaluate_conditional(node, scope, &composed);
                self.current_file = prev_file;
                r
            }
            TypeKind::DeferredMapped(key, captured) => {
                let Some(&(node, scope, file)) = self.deferred.deferred_mappeds.get(&key) else {
                    return t;
                };
                let prev_file = self.current_file;
                self.current_file = file;
                let mut composed: Mapper = Mapper::new();
                for (k, v) in captured {
                    if v != t {
                        let nv = self.instantiate_type(v, mapper);
                        composed.insert(k, nv);
                    }
                }
                for (k, v) in mapper {
                    composed.insert(*k, *v);
                }
                let r = self.evaluate_mapped(node, scope, &composed);
                self.current_file = prev_file;
                r
            }
            TypeKind::TemplateLit(parts) => {
                let mut changed = false;
                let nparts: Vec<crate::types::TplPart> = parts
                    .iter()
                    .map(|p| match p {
                        crate::types::TplPart::Ty(t2) => {
                            let nt = self.instantiate_type(*t2, mapper);
                            changed |= nt != *t2;
                            crate::types::TplPart::Ty(nt)
                        }
                        s => s.clone(),
                    })
                    .collect();
                if !changed {
                    return t;
                }
                // re-run expansion with the instantiated parts
                let mut head = String::new();
                let mut pairs: Vec<(TypeId, String)> = Vec::new();
                let mut iter = nparts.into_iter().peekable();
                if let Some(crate::types::TplPart::Str(s)) = iter.peek() {
                    head = s.clone();
                    iter.next();
                }
                while let Some(p) = iter.next() {
                    if let crate::types::TplPart::Ty(t2) = p {
                        let text = match iter.peek() {
                            Some(crate::types::TplPart::Str(s)) => {
                                let s = s.clone();
                                iter.next();
                                s
                            }
                            _ => String::new(),
                        };
                        pairs.push((t2, text));
                    }
                }
                self.template_literal_type(head, pairs)
            }
            TypeKind::Tuple(elems) => {
                let mut nelems: Vec<TupleElem> = Vec::new();
                let mut changed = false;
                for e in &elems {
                    let nt = self.instantiate_type(e.ty, mapper);
                    if nt != e.ty {
                        changed = true;
                    }
                    if e.rest {
                        // a variadic rest spread (`...T`): if `T` became a
                        // concrete tuple, splice its elements inline so
                        // `[...T, ...U]` flattens; an array stays a rest element.
                        if let TypeKind::Tuple(inner) | TypeKind::ReadonlyTuple(inner) =
                            self.types.kind(nt).clone()
                        {
                            for ie in inner {
                                nelems.push(ie);
                            }
                            changed = true;
                            continue;
                        }
                    }
                    nelems.push(TupleElem { ty: nt, ..*e });
                }
                if !changed {
                    t
                } else {
                    self.types.tuple(nelems)
                }
            }
            TypeKind::DeferredObj(_) => {
                let Some(sid) = self.shape_of_type(t) else {
                    return t;
                };
                let anon = self.types.alloc(TypeKind::Anon(sid));
                // copy alias for display
                if let Some((s, a)) = self.types.alias(t).cloned() {
                    self.types.set_alias(anon, s, a);
                }
                self.instantiate_type(anon, mapper)
            }
            TypeKind::Anon(shape_id) => {
                let shape = self.types.shape(shape_id).clone();
                let mut changed = false;
                let mut nshape = Shape::default();
                for p in &shape.props {
                    let nt = self.instantiate_type(p.ty, mapper);
                    changed |= nt != p.ty;
                    nshape.props.push(PropInfo {
                        ty: nt,
                        ..p.clone()
                    });
                }
                for &s in &shape.call_sigs {
                    let ns = self.instantiate_sig(s, mapper);
                    changed |= ns != s;
                    nshape.call_sigs.push(ns);
                }
                for &s in &shape.ctor_sigs {
                    let ns = self.instantiate_sig(s, mapper);
                    changed |= ns != s;
                    nshape.ctor_sigs.push(ns);
                }
                for i in &shape.index_infos {
                    let nv = self.instantiate_type(i.value, mapper);
                    changed |= nv != i.value;
                    nshape.index_infos.push(IndexInfo {
                        key: i.key,
                        value: nv,
                        readonly: i.readonly,
                    });
                }
                if !changed {
                    t
                } else {
                    let sid = self.types.alloc_shape(nshape);
                    self.types.alloc(TypeKind::Anon(sid))
                }
            }
            _ => t,
        }
    }

    pub fn instantiate_sig(
        &mut self,
        sig: crate::types::SigId,
        mapper: &Mapper,
    ) -> crate::types::SigId {
        if mapper.is_empty() || self.mapper_is_effectively_empty(mapper) {
            return sig;
        }
        let s = self.types.sig(sig).clone();
        let mut changed = false;
        let params: Vec<ParamInfo> = s
            .params
            .iter()
            .map(|p| {
                let nt = self.instantiate_type(p.ty, mapper);
                changed |= nt != p.ty;
                ParamInfo {
                    name: p.name.clone(),
                    ty: nt,
                    optional: p.optional,
                    decl_span: p.decl_span,
                    decl_file: p.decl_file,
                }
            })
            .collect();
        let rest = s.rest.map(|r| {
            let nr = self.instantiate_type(r, mapper);
            changed |= nr != r;
            nr
        });
        let ret = self.sig_return(sig);
        let nret = self.instantiate_type(ret, mapper);
        changed |= nret != ret;
        if !changed {
            return sig;
        }
        self.types.alloc_sig(Signature {
            type_params: s
                .type_params
                .iter()
                .filter(|p| !mapper.contains_key(p))
                .copied()
                .collect(),
            params,
            min_args: s.min_args,
            rest,
            rest_name: s.rest_name.clone(),
            rest_tp: None,
            ret: nret,
            decl_key: 0,
            predicate: None,
            is_abstract: s.is_abstract,
        })
    }
}
