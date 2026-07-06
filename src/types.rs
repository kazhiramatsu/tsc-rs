//! Type table: interned types with tsc-compatible identity. Creation order is
//! LOAD-BEARING — union members are stored sorted by TypeId and tsc sorts by
//! internal type id, so intrinsics must be interned in tsc's boot order.

use crate::binder::SymbolId;
use crate::jsstr::JsString;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct TypeId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SigId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ShapeId(pub u32);

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TypeKind {
    Any,
    Unknown,
    /// internal error type — behaves like `any`, suppresses cascades
    Error,
    Undefined,
    Null,
    String,
    Number,
    Bigint,
    EsSymbol,
    Void,
    Never,
    NonPrimitive, // `object`
    StrLit(JsString),
    /// f64 bits
    NumLit(u64),
    BigIntLit(String),
    BoolLit(bool),
    /// members sorted ascending by TypeId, deduped, flattened
    Union(Vec<TypeId>),
    /// `A & B & …`. Members are normalized by `intersect_all`: flattened,
    /// any/error/never absorbed, the empty object `{}` folded into nullish
    /// removal, concrete unrelated object operands kept distinct (their apparent
    /// members combine). A node is retained only when it cannot collapse to a
    /// single type — an operand still mentions a type parameter (`T & {}`), or
    /// several unrelated objects intersect.
    Intersection(Vec<TypeId>),
    /// anonymous object/function type with eagerly-resolved shape
    Anon(ShapeId),
    /// type-literal node whose members resolve lazily (keyed by node identity)
    DeferredObj(usize),
    /// interface or class-instance type (members lazy via symbol)
    Iface(SymbolId),
    /// generic reference: target interface/class symbol + type args
    Ref(SymbolId, Vec<TypeId>),
    /// class statics (`typeof C`)
    ClassStatics(SymbolId),
    /// class statics with captured outer type-parameter substitutions, used for
    /// class expressions returned from or stored inside generic contexts.
    MappedClassStatics(SymbolId, Vec<(SymbolId, TypeId)>),
    /// class instance with captured outer type-parameter substitutions. `Ref`
    /// covers the class's own type arguments; this also preserves outer
    /// function/class parameters captured by a class expression.
    MappedIface(SymbolId, Vec<(SymbolId, TypeId)>),
    Tuple(Vec<TupleElem>),
    TypeParam(SymbolId),
    /// readonly T[] — only readonly arrays in v1
    ReadonlyArray(TypeId),
    /// the enum type itself (annotation position)
    EnumType(SymbolId),
    /// a const enum member's literal type (symbol = the member)
    EnumMember(SymbolId),
    /// the enum object value (`const y = Color`)
    EnumObject(SymbolId),
    /// a namespace value (`typeof Geo`)
    NamespaceObj(SymbolId),
    /// `as const` array: readonly tuple
    ReadonlyTuple(Vec<TupleElem>),
    /// keyof over a *named* type (display preserved as `keyof X`)
    Keyof(TypeId),
    /// unresolved indexed access T[K] (contains type params)
    IndexedAccess(TypeId, TypeId),
    /// generic conditional type, keyed by node identity + captured mapper
    DeferredCond(usize, Vec<(SymbolId, TypeId)>),
    /// generic mapped type, keyed by node identity + captured mapper
    DeferredMapped(usize, Vec<(SymbolId, TypeId)>),
    /// template literal pattern type (`a-${string}`)
    TemplateLit(Vec<TplPart>),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TplPart {
    Str(String),
    Ty(TypeId),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TupleElem {
    pub ty: TypeId,
    pub optional: bool,
    pub rest: bool,
}

#[derive(Clone, Debug)]
pub struct PropInfo {
    pub name: String,
    pub ty: TypeId,
    pub optional: bool,
    pub readonly: bool,
    /// method-declared (affects display `m(): void` vs `f: () => void`)
    pub is_method: bool,
    /// symbol for class members (access control); None for anonymous shapes
    pub symbol: Option<SymbolId>,
}

#[derive(Clone, Copy, Debug)]
pub struct IndexInfo {
    pub key: TypeId,
    pub value: TypeId,
    pub readonly: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Shape {
    pub props: Vec<PropInfo>,
    pub call_sigs: Vec<SigId>,
    pub ctor_sigs: Vec<SigId>,
    pub index_infos: Vec<IndexInfo>,
}

impl Shape {
    pub fn prop(&self, name: &str) -> Option<&PropInfo> {
        self.props.iter().find(|p| p.name == name)
    }
}

#[derive(Clone, Debug)]
pub struct ParamInfo {
    pub name: String,
    pub ty: TypeId,
    pub optional: bool,
    /// declaration site of the parameter name (for "An argument for '…' was not
    /// provided" related-info); None for synthesized signatures.
    pub decl_span: Option<crate::ast::Span>,
    pub decl_file: usize,
}

#[derive(Clone, Debug)]
pub struct Signature {
    pub type_params: Vec<SymbolId>,
    pub params: Vec<ParamInfo>,
    /// min required argument count
    pub min_args: u32,
    /// rest parameter element type
    pub rest: Option<TypeId>,
    /// source name of the rest parameter for signature display.
    pub rest_name: Option<String>,
    /// when the rest parameter is a bare type parameter (`...args: T`), the
    /// parameter symbol — enables variadic tuple inference at call sites.
    pub rest_tp: Option<SymbolId>,
    pub ret: TypeId,
    /// declaration ptr for lazy body-inferred return types (0 = none)
    pub decl_key: usize,
    /// declared as a class/interface METHOD (tsc keeps method parameters
    /// bivariant even under strictFunctionTypes; compareSignaturesRelated
    /// keys strictVariance on the TARGET declaration kind)
    pub from_method: bool,
    /// the SYNTACTIC return annotation resolved to `never` at declaration
    /// (tsc getReturnTypeFromAnnotation): drives never-returning-call
    /// reachability. Instantiation copies it verbatim, so `(): T`
    /// instantiated at `never` stays false while `(): never` stays true.
    pub ret_annotation_never: bool,
    /// type-predicate / assertion info from a `x is T` / `asserts x is T`
    /// return type, used for call-site narrowing. None for ordinary returns.
    pub predicate: Option<PredInfo>,
    /// abstract construct signature (`abstract new (...) => X`, or a mixed-in
    /// abstract base). `new`-ing a type whose construct signature is abstract
    /// is an error (2511).
    pub is_abstract: bool,
}

/// A `x is T` (asserts=false) or `asserts x [is T]` (asserts=true) predicate.
#[derive(Clone, Copy, Debug)]
pub struct PredInfo {
    /// index of the named parameter, or -1 for `this`, or -2 if unresolved.
    pub param: i32,
    pub asserts: bool,
    /// the asserted/narrowed type; None for a plain `asserts x` (truthiness).
    pub ty: Option<TypeId>,
}

pub struct TypeData {
    pub kind: TypeKind,
    /// display alias (type X = ...) — set at creation, first one wins
    pub alias: Option<(SymbolId, Vec<TypeId>)>,
    /// literal/object freshness pairs
    pub fresh_of: Option<TypeId>, // set on the REGULAR type: its fresh variant
    pub regular_of: Option<TypeId>, // set on the FRESH type: its regular variant
}

pub struct TypeTable {
    pub data: Vec<TypeData>,
    pub shapes: Vec<Shape>,
    pub sigs: Vec<Signature>,
    intern: HashMap<TypeKind, TypeId>,
    // intrinsic ids (tsc boot order)
    pub any: TypeId,
    pub unknown: TypeId,
    pub error: TypeId,
    pub undefined: TypeId,
    pub null: TypeId,
    pub string: TypeId,
    pub number: TypeId,
    pub bigint: TypeId,
    pub false_t: TypeId,
    pub true_t: TypeId,
    pub boolean: TypeId,
    pub es_symbol: TypeId,
    pub void: TypeId,
    pub never: TypeId,
    pub non_primitive: TypeId,
}

impl TypeTable {
    pub fn new() -> TypeTable {
        let mut t = TypeTable {
            data: Vec::new(),
            shapes: Vec::new(),
            sigs: Vec::new(),
            intern: HashMap::new(),
            any: TypeId(0),
            unknown: TypeId(0),
            error: TypeId(0),
            undefined: TypeId(0),
            null: TypeId(0),
            string: TypeId(0),
            number: TypeId(0),
            bigint: TypeId(0),
            false_t: TypeId(0),
            true_t: TypeId(0),
            boolean: TypeId(0),
            es_symbol: TypeId(0),
            void: TypeId(0),
            never: TypeId(0),
            non_primitive: TypeId(0),
        };
        // tsc checker boot order (so relative TypeIds — and therefore union
        // member order — match): any, unknown, error(unresolved), undefined,
        // null, string, number, bigint, false, true, boolean, symbol, void,
        // never, nonPrimitive.
        t.any = t.intern_kind(TypeKind::Any);
        t.unknown = t.intern_kind(TypeKind::Unknown);
        t.error = t.intern_kind(TypeKind::Error);
        t.undefined = t.intern_kind(TypeKind::Undefined);
        t.null = t.intern_kind(TypeKind::Null);
        t.string = t.intern_kind(TypeKind::String);
        t.number = t.intern_kind(TypeKind::Number);
        t.bigint = t.intern_kind(TypeKind::Bigint);
        t.false_t = t.intern_kind(TypeKind::BoolLit(false));
        t.true_t = t.intern_kind(TypeKind::BoolLit(true));
        t.boolean = t.union(vec![t.false_t, t.true_t]);
        t.es_symbol = t.intern_kind(TypeKind::EsSymbol);
        t.void = t.intern_kind(TypeKind::Void);
        t.never = t.intern_kind(TypeKind::Never);
        t.non_primitive = t.intern_kind(TypeKind::NonPrimitive);
        t
    }

    pub fn kind(&self, id: TypeId) -> &TypeKind {
        &self.data[id.0 as usize].kind
    }
    pub fn alias(&self, id: TypeId) -> Option<&(SymbolId, Vec<TypeId>)> {
        self.data[id.0 as usize].alias.as_ref()
    }
    pub fn shape(&self, id: ShapeId) -> &Shape {
        &self.shapes[id.0 as usize]
    }
    pub fn sig(&self, id: SigId) -> &Signature {
        &self.sigs[id.0 as usize]
    }

    pub fn intern_kind(&mut self, kind: TypeKind) -> TypeId {
        if let Some(&id) = self.intern.get(&kind) {
            return id;
        }
        let id = TypeId(self.data.len() as u32);
        self.data.push(TypeData {
            kind: kind.clone(),
            alias: None,
            fresh_of: None,
            regular_of: None,
        });
        self.intern.insert(kind, id);
        id
    }

    /// non-interned type (anon shapes etc.)
    pub fn alloc(&mut self, kind: TypeKind) -> TypeId {
        let id = TypeId(self.data.len() as u32);
        self.data.push(TypeData {
            kind,
            alias: None,
            fresh_of: None,
            regular_of: None,
        });
        id
    }

    pub fn alloc_shape(&mut self, shape: Shape) -> ShapeId {
        self.shapes.push(shape);
        ShapeId(self.shapes.len() as u32 - 1)
    }

    pub fn alloc_sig(&mut self, sig: Signature) -> SigId {
        self.sigs.push(sig);
        SigId(self.sigs.len() as u32 - 1)
    }

    pub fn string_lit(&mut self, v: &str) -> TypeId {
        self.intern_kind(TypeKind::StrLit(JsString::from(v)))
    }
    /// Intern a string literal type from a faithful JS string value (preserves
    /// lone surrogates, so distinct JS values get distinct types).
    pub fn string_lit_js(&mut self, v: &JsString) -> TypeId {
        self.intern_kind(TypeKind::StrLit(v.clone()))
    }
    pub fn number_lit(&mut self, v: f64) -> TypeId {
        self.intern_kind(TypeKind::NumLit(v.to_bits()))
    }
    pub fn bigint_lit(&mut self, text: &str) -> TypeId {
        self.intern_kind(TypeKind::BigIntLit(text.to_string()))
    }

    /// the fresh (literal-expression) variant of a literal/object type
    pub fn fresh(&mut self, regular: TypeId) -> TypeId {
        if let Some(f) = self.data[regular.0 as usize].fresh_of {
            return f;
        }
        let kind = self.data[regular.0 as usize].kind.clone();
        let fresh = self.alloc(kind);
        self.data[fresh.0 as usize].regular_of = Some(regular);
        self.data[regular.0 as usize].fresh_of = Some(fresh);
        fresh
    }

    /// regular (assigned) variant — identity for non-fresh types
    pub fn regular(&self, t: TypeId) -> TypeId {
        self.data[t.0 as usize].regular_of.unwrap_or(t)
    }

    pub fn is_fresh(&self, t: TypeId) -> bool {
        self.data[t.0 as usize].regular_of.is_some()
    }

    /// tsc getUnionType with Literal reduction: flatten, map to regular,
    /// dedupe, sort by id, collapse singletons. `never` members drop out;
    /// `any`/`unknown` absorb.
    pub fn union(&mut self, members: Vec<TypeId>) -> TypeId {
        let mut flat: Vec<TypeId> = Vec::new();
        let mut has_any = false;
        let mut has_unknown = false;
        let mut has_error = false;
        fn push(
            table: &TypeTable,
            flat: &mut Vec<TypeId>,
            t: TypeId,
            has_any: &mut bool,
            has_unknown: &mut bool,
            has_error: &mut bool,
        ) {
            match table.kind(t) {
                TypeKind::Union(inner) => {
                    for &m in inner.clone().iter() {
                        push(table, flat, m, has_any, has_unknown, has_error);
                    }
                }
                TypeKind::Any => *has_any = true,
                TypeKind::Error => *has_error = true,
                TypeKind::Unknown => *has_unknown = true,
                TypeKind::Never => {}
                _ => flat.push(table.regular(t)),
            }
        }
        for m in members {
            push(
                self,
                &mut flat,
                m,
                &mut has_any,
                &mut has_unknown,
                &mut has_error,
            );
        }
        if has_error {
            return self.error;
        }
        if has_any {
            return self.any;
        }
        if has_unknown {
            return self.unknown;
        }
        flat.sort();
        flat.dedup();
        // literal/base-primitive subsumption: drop literals whose base primitive
        // is also present (tsc union reduction)
        let has_string = flat
            .iter()
            .any(|&t| matches!(self.kind(t), TypeKind::String));
        let has_number = flat
            .iter()
            .any(|&t| matches!(self.kind(t), TypeKind::Number));
        let has_bigint = flat
            .iter()
            .any(|&t| matches!(self.kind(t), TypeKind::Bigint));
        flat.retain(|&t| match self.kind(t) {
            TypeKind::StrLit(_) => !has_string,
            TypeKind::NumLit(_) => !has_number,
            TypeKind::BigIntLit(_) => !has_bigint,
            _ => true,
        });
        match flat.len() {
            0 => self.never,
            1 => flat[0],
            _ => self.intern_kind(TypeKind::Union(flat)),
        }
    }

    pub fn ref_type(&mut self, target: SymbolId, args: Vec<TypeId>) -> TypeId {
        self.intern_kind(TypeKind::Ref(target, args))
    }

    pub fn tuple(&mut self, elems: Vec<TupleElem>) -> TypeId {
        self.intern_kind(TypeKind::Tuple(elems))
    }

    pub fn set_alias(&mut self, t: TypeId, sym: SymbolId, args: Vec<TypeId>) {
        if self.data[t.0 as usize].alias.is_none() {
            self.data[t.0 as usize].alias = Some((sym, args));
        }
    }

    pub fn set_alias_force(&mut self, t: TypeId, sym: SymbolId, args: Vec<TypeId>) {
        self.data[t.0 as usize].alias = Some((sym, args));
    }

    /// remove given members from a union (narrowing helper)
    pub fn filter_union(&mut self, t: TypeId, keep: impl Fn(&TypeTable, TypeId) -> bool) -> TypeId {
        match self.kind(t).clone() {
            TypeKind::Union(members) => {
                let kept: Vec<TypeId> = members.into_iter().filter(|&m| keep(self, m)).collect();
                self.union(kept)
            }
            _ => {
                if keep(self, t) {
                    t
                } else {
                    self.never
                }
            }
        }
    }

    pub fn union_members(&self, t: TypeId) -> Vec<TypeId> {
        match self.kind(t) {
            TypeKind::Union(m) => m.clone(),
            _ => vec![t],
        }
    }

    pub fn is_error(&self, t: TypeId) -> bool {
        matches!(self.kind(t), TypeKind::Error)
    }
    pub fn is_any_or_error(&self, t: TypeId) -> bool {
        matches!(self.kind(t), TypeKind::Any | TypeKind::Error)
    }
    pub fn is_nullish(&self, t: TypeId) -> bool {
        matches!(
            self.kind(t),
            TypeKind::Undefined | TypeKind::Null | TypeKind::Void
        )
    }

    /// widened form of a literal type (fresh OR regular literal → base primitive)
    pub fn widen_literal(&mut self, t: TypeId) -> TypeId {
        match self.kind(t) {
            TypeKind::StrLit(_) => self.string,
            TypeKind::NumLit(_) => self.number,
            TypeKind::BigIntLit(_) => self.bigint,
            TypeKind::BoolLit(_) => self.boolean,
            TypeKind::Union(members) => {
                let widened: Vec<TypeId> = members
                    .clone()
                    .iter()
                    .map(|&m| self.widen_literal(m))
                    .collect();
                self.union(widened)
            }
            _ => t,
        }
    }

    /// fresh literal → regular literal only (assignment widening under context)
    pub fn widen_fresh(&self, t: TypeId) -> TypeId {
        self.regular(t)
    }
}
