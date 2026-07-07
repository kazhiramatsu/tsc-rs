//! Apparent types: the apparent (member-bearing) type of a value, `keyof`
//! over unions, and indexed-access (`T[K]`) resolution. Split out of `symbols.rs`.

use crate::ast::Span;
use crate::checker::Checker;
use crate::diagnostics::gen;
use crate::types::{TypeId, TypeKind};

impl<'a> Checker<'a> {
    /// The `this` type implied by an object literal's contextual type: the
    /// argument of a `ThisType<T>` constituent. `ThisType<T>` → `T`; an
    /// intersection contributes the intersection of every member's `ThisType`
    /// argument (`M & ThisType<D & M>` → `D & M`); `None` if no marker is
    /// present. The marker is preserved through intersection reduction because
    /// `ThisType<T>`/`PropDesc<U>` both carry type parameters and so are kept as
    /// distinct intersection members rather than merged.
    pub(crate) fn this_type_from_contextual(&mut self, ctx: TypeId) -> Option<TypeId> {
        let this_sym = self.global_type_symbol("ThisType")?;
        self.this_type_from_contextual_inner(ctx, this_sym)
    }

    fn this_type_from_contextual_inner(
        &mut self,
        ctx: TypeId,
        this_sym: crate::binder::SymbolId,
    ) -> Option<TypeId> {
        match self.types.kind(ctx).clone() {
            TypeKind::Ref(sym, args) if sym == this_sym => args.first().copied(),
            TypeKind::Intersection(members) => {
                let mut found: Vec<TypeId> = Vec::new();
                for m in members {
                    if let Some(t) = self.this_type_from_contextual_inner(m, this_sym) {
                        found.push(t);
                    }
                }
                match found.len() {
                    0 => None,
                    1 => Some(found[0]),
                    _ => Some(self.intersect_all(found)),
                }
            }
            _ => None,
        }
    }

    /// apparent type: primitives → their global interface types
    pub fn apparent_type(&mut self, t: TypeId) -> TypeId {
        let global = |c: &mut Self, n: &str| -> Option<TypeId> {
            c.global_type_symbol(n)
                .map(|s| c.types.intern_kind(TypeKind::Iface(s)))
        };
        match self.types.kind(t).clone() {
            TypeKind::String | TypeKind::StrLit(_) => global(self, "String").unwrap_or(t),
            TypeKind::Number | TypeKind::NumLit(_) => global(self, "Number").unwrap_or(t),
            TypeKind::BoolLit(_) => global(self, "Boolean").unwrap_or(t),
            TypeKind::Union(_) if t == self.types.boolean => global(self, "Boolean").unwrap_or(t),
            TypeKind::Bigint | TypeKind::BigIntLit(_) => t,
            TypeKind::EnumType(sym) | TypeKind::EnumMember(sym) => {
                let (numeric, string) = self.enum_member_kinds_of(t);
                let _ = sym;
                if string && !numeric {
                    global(self, "String").unwrap_or(t)
                } else {
                    global(self, "Number").unwrap_or(t)
                }
            }
            TypeKind::TypeParam(sym) => {
                // getApparentType: a type parameter presents the apparent type of
                // its constraint, so `x.a` is allowed when `T extends { a: ... }`.
                if let Some(c) = self.constraint_of_type_param(sym) {
                    self.apparent_type(c)
                } else {
                    t
                }
            }
            TypeKind::IndexedAccess(..) => {
                // `T[K]` where the base resolves through its constraint:
                // e.g. `this["props"]` where `this` is a polymorphic type
                // parameter with constraint `Iface(B)`, so `apparent` peels
                // through to the named property's type. Falls back to the
                // original if the constraint route does not produce a
                // resolved type.
                if let Some(c) = self.indexed_access_base_constraint(t) {
                    self.apparent_type(c)
                } else {
                    t
                }
            }
            _ => t,
        }
    }
    pub fn apparent_type_display(&mut self, t: TypeId) -> String {
        let a = self.apparent_type(t);
        if a != t {
            return self.display_type(a);
        }
        self.display_type(t)
    }

    /// keyof as the union of property-name literals
    pub fn keyof_union(&mut self, t: TypeId) -> TypeId {
        let inner = match self.types.kind(t) {
            TypeKind::Keyof(i) => *i,
            _ => t,
        };
        // For a type parameter, `keyof T` is the keys of T's constraint
        // (matches tsc: `getApparentTypeOfTypeParameter` is used as the source
        // of the key list). Crucially, this lets `keyof this-param` produce
        // the owner's property names, so `K extends keyof this` is non-empty
        // and `"x"` is assignable to it inside the owner's body.
        if let TypeKind::TypeParam(tp) = self.types.kind(inner).clone() {
            if let Some(c) = self.constraint_of_type_param(tp) {
                return self.keyof_union(c);
            }
        }
        match self.types.kind(inner).clone() {
            TypeKind::Union(members) if members.iter().any(|&m| self.type_contains_params(m)) => {
                let keys = members
                    .iter()
                    .map(|&m| self.types.intern_kind(TypeKind::Keyof(m)))
                    .collect();
                return self.intersect_all(keys);
            }
            TypeKind::Intersection(members)
                if members.iter().any(|&m| self.type_contains_params(m)) =>
            {
                let keys = members
                    .iter()
                    .map(|&m| self.types.intern_kind(TypeKind::Keyof(m)))
                    .collect();
                return self.types.union(keys);
            }
            _ => {}
        }
        let shape_id = self.shape_of_type(inner);
        let names: Vec<TypeId> = match shape_id {
            Some(sid) => {
                let names: Vec<String> = self
                    .types
                    .shape(sid)
                    .props
                    .iter()
                    .map(|p| p.name.clone())
                    .collect();
                names.iter().map(|n| self.types.string_lit(n)).collect()
            }
            None => Vec::new(),
        };
        if names.is_empty() {
            self.types.never
        } else {
            self.types.union(names)
        }
    }

    /// Whether a type-parameter constraint `c` makes its parameter a valid
    /// index for `obj`: it is `keyof obj`, or an intersection that still has
    /// `keyof obj` as an operand (`keyof T & string`).
    pub(crate) fn index_constraint_covers(&self, c: TypeId, obj: TypeId) -> bool {
        match self.types.kind(c) {
            TypeKind::Keyof(inner) => *inner == obj,
            TypeKind::Intersection(ms) => {
                let ms = ms.clone();
                ms.iter().any(|&m| self.index_constraint_covers(m, obj))
            }
            _ => false,
        }
    }

    /// T[K]: resolve when possible; defer when type parameters are involved.
    /// `spans` = (index node, whole T[K] node): 2339 uses the index span,
    /// 2536 the whole node.
    pub fn indexed_access_type(
        &mut self,
        obj: TypeId,
        index: TypeId,
        spans: Option<(Span, Span)>,
    ) -> TypeId {
        if self.types.is_any_or_error(obj) || self.types.is_any_or_error(index) {
            return self.types.any;
        }
        // `never[K]` (e.g. from a reduced intersection `(A & B)['kind']`) is
        // `never`; never has every key.
        if matches!(self.types.kind(obj), TypeKind::Never) {
            return self.types.never;
        }
        let contains_param = matches!(self.types.kind(obj), TypeKind::TypeParam(_))
            || matches!(
                self.types.kind(index),
                TypeKind::TypeParam(_) | TypeKind::Keyof(_)
            )
            || self.type_contains_params(obj)
            || self.type_contains_params(index);
        if contains_param {
            // K must be constrained to keyof T (2536 otherwise)
            if matches!(self.types.kind(obj), TypeKind::TypeParam(_)) {
                if let TypeKind::TypeParam(ksym) = self.types.kind(index).clone() {
                    // synthetic mapped-key / infer symbols carry no decls and
                    // are implicitly keyof-constrained by their context
                    let synthetic = self.symbol(ksym).decls.is_empty();
                    let kc = self.constraint_of_type_param(ksym);
                    let ok = synthetic
                        || match kc {
                            // `K` indexes `T` when its constraint is `keyof T`,
                            // or a refinement that still contains `keyof T` as an
                            // intersection operand (e.g. `keyof T & string`).
                            Some(c) => self.index_constraint_covers(c, obj),
                            None => false,
                        };
                    if !ok {
                        if let Some((_, full)) = spans {
                            let kd = self.display_type(index);
                            let od = self.display_type(obj);
                            self.error_at(
                                full,
                                &gen::Type_0_cannot_be_used_to_index_type_1,
                                &[kd, od],
                            );
                            return self.types.error;
                        }
                    }
                }
            }
            return self.types.intern_kind(TypeKind::IndexedAccess(obj, index));
        }
        match self.types.kind(index).clone() {
            TypeKind::StrLit(name) => match self.prop_of_type(obj, name.to_str_lossy().as_ref()) {
                Some(t) => t,
                None => {
                    if let Some((idx_span, _)) = spans {
                        let d = self.display_type(obj);
                        self.error_at(
                            idx_span,
                            &gen::Property_0_does_not_exist_on_type_1,
                            &[name.to_str_lossy().into_owned(), d],
                        );
                    }
                    self.types.error
                }
            },
            TypeKind::Union(members) => {
                let parts: Vec<TypeId> = members
                    .iter()
                    .map(|&m| self.indexed_access_type(obj, m, spans))
                    .collect();
                self.types.union(parts)
            }
            TypeKind::NumLit(bits) => {
                let v = f64::from_bits(bits);
                match self.types.kind(obj).clone() {
                    TypeKind::Tuple(elems) | TypeKind::ReadonlyTuple(elems) => elems
                        .get(v as usize)
                        .map(|e| e.ty)
                        .unwrap_or(self.types.error),
                    _ => self.array_element_type(obj).unwrap_or(self.types.any),
                }
            }
            TypeKind::Number => self.array_element_type(obj).unwrap_or(self.types.any),
            _ => self.types.any,
        }
    }
}
