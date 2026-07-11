//! The Type data model (greenfield §4.2, core-interfaces §3).
//!
//! Allocation id IS identity: `TypeId` is the arena index, assigned in
//! creation order exactly like tsc's `typeCount` counter (createType,
//! _tsc.js 50095). Interning maps exist ONLY where tsc declares one
//! (tables.rs) — two structurally identical anonymous object types are
//! DISTINCT types.

use crate::flags::{AccessFlags, ElementFlags, IndexFlags, ObjectFlags, TypeFlags};

/// tsc Type.id (createType 50098-50099). Arena index; ids are
/// program-run-local and never serialized.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TypeId(pub u32);

/// tsc Symbol id space. Lives in tsrs2-types so Type.symbol can point
/// at binder/checker symbols without a dependency cycle; the binder
/// re-exports it and owns the arena.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SymbolId(pub u32);

/// tsc PseudoBigInt (18906): sign + base-10 digit string.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PseudoBigInt {
    pub negative: bool,
    pub base10_value: String,
}

impl PseudoBigInt {
    /// tsc-port: pseudoBigIntToString @6.0.3
    /// tsc-hash: 8a3816f7b6507e39962611c0e7433017d3012cb0499323f98fd87608a7c2191b
    /// tsc-span: _tsc.js:18965-18967
    pub fn to_base10_string(&self) -> String {
        if self.negative && self.base10_value != "0" {
            format!("-{}", self.base10_value)
        } else {
            self.base10_value.clone()
        }
    }
}

/// tsc LiteralType.value: string | number | PseudoBigInt.
#[derive(Clone, Debug, PartialEq)]
pub enum LiteralValue {
    String(String),
    Number(f64),
    BigInt(PseudoBigInt),
}

/// Synthesized tuple target payload (createTupleTargetType 61156).
/// The target doubles as a reference to itself: tsc sets
/// `type.target = type; type.resolvedTypeArguments = type.typeParameters`
/// (61192-61193) — TypeTables::reference_target / ::type_arguments
/// encode that aliasing.
///
/// M3 NOTE: tsc also synthesizes per-index `"0".."n"` property symbols
/// and the `length` property (61160-61185); those are deferred until a
/// consumer exists — the tuple RELATION arm reads only elementFlags +
/// type arguments (propertiesRelatedTo 66804-66805), never the
/// properties. Labeled element declarations (getNodeId-keyed in the
/// tupleTypes cache key) are likewise deferred with them.
#[derive(Clone, Debug, PartialEq)]
pub struct TupleTargetData {
    /// One synthesized TypeParameter per element (empty for `[]`).
    pub type_parameters: Box<[TypeId]>,
    /// Synthesized `thisType` TypeParameter (isThisType, constraint =
    /// this target).
    pub this_type: TypeId,
    pub element_flags: Box<[ElementFlags]>,
    /// countWhere(flags, Required|Variadic) (61158).
    pub min_length: usize,
    /// Leading fixed-element count — positions before the first
    /// Rest/Variadic element (61168-61176).
    pub fixed_length: usize,
    /// !!(combinedFlags & Variable) (61204).
    pub has_rest_element: bool,
    pub combined_flags: ElementFlags,
    pub readonly: bool,
    /// tsc labeledElementDeclarations (61207): per-element
    /// NamedTupleMember/Parameter node ids as raw u32s (the types
    /// crate is NodeId-free); None when no element is labeled.
    pub labeled_element_declarations: Option<Box<[Option<u32>]>>,
}

/// Per-kind payload (greenfield §4.2 TypeData). M3 carries the kinds
/// the relation pins can construct; StringMapping landed with M4 5.2;
/// IndexedAccess/Conditional/Mapped/Substitution arrive with the
/// keyof/indexed-access follow-up and M8.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeData {
    /// any/unknown/string/... incl. error/silentNever/wildcard/missing
    /// variants (createIntrinsicType 50111).
    Intrinsic {
        name: &'static str,
        debug_name: Option<&'static str>,
    },
    /// String/number/bigint/boolean-literal types. The fresh/regular
    /// pair lives on Type::fresh_type/regular_type (freshness is
    /// STRUCTURE: fresh_type == self).
    Literal {
        value: LiteralValue,
    },
    Union {
        types: Box<[TypeId]>,
        /// Denormalized origin union/intersection (4.2+).
        origin: Option<TypeId>,
    },
    Intersection {
        types: Box<[TypeId]>,
    },
    /// Anonymous object types AND class/interface declared types —
    /// distinguished by object_flags (Anonymous vs Class/Interface).
    /// Members resolve lazily in the checker (resolveStructuredTypeMembers).
    Object,
    /// createTypeReference (60169): an instantiation-free reference —
    /// tuple values like `[number, string]` point at a TupleTarget.
    /// `resolved_type_arguments` is `Some` from creation for plain
    /// references; DEFERRED references (createDeferredTypeReference
    /// 60188, node/mapper in checker TypeLinks) start `None` and are
    /// filled by the checker's lazy getTypeArguments (60202).
    Reference {
        target: TypeId,
        resolved_type_arguments: Option<Box<[TypeId]>>,
    },
    /// The synthesized generic tuple TARGET (objectFlags Tuple|Reference).
    TupleTarget(TupleTargetData),
    /// Synthesized type parameters (tuple targets, thisType). Real
    /// declared type parameters are M4.
    TypeParameter {
        is_this_type: bool,
        constraint: Option<TypeId>,
    },
    /// getTemplateLiteralType (62057): texts.len() == types.len() + 1.
    TemplateLiteral {
        texts: Box<[String]>,
        types: Box<[TypeId]>,
    },
    /// createStringMappingType (62163): `Uppercase<T>` over a generic
    /// operand — Type::symbol names the intrinsic alias.
    StringMapping {
        ty: TypeId,
    },
    /// createIndexType (61921): `keyof T` over a deferred operand.
    /// Origin index types (createOriginIndexType 61927, the union
    /// display denormalization) share the shape with IndexFlags::NONE.
    Index {
        ty: TypeId,
        index_flags: IndexFlags,
    },
    /// createIndexedAccessType (62168): `T[K]` over a generic pair;
    /// alias fields live on Type.
    IndexedAccess {
        object_type: TypeId,
        index_type: TypeId,
        access_flags: AccessFlags,
    },
    /// tsc GenericType (InterfaceType & TypeReference): the declared
    /// type of a class, a generic interface, or a this-ful interface
    /// (getDeclaredTypeOfClassOrInterface 57387-57400). The target
    /// doubles as a reference to itself — `type.target = type;
    /// type.resolvedTypeArguments = type.typeParameters` — encoded by
    /// TypeTables::reference_target / ::type_arguments like tuple
    /// targets.
    GenericType {
        /// outerTypeParameters ++ localTypeParameters (may be empty:
        /// non-generic classes and this-ful interfaces).
        type_parameters: Box<[TypeId]>,
        /// Length of the outerTypeParameters prefix.
        outer_type_parameter_count: usize,
        /// Synthesized thisType (isThisType, constraint = this target).
        this_type: TypeId,
    },
}

/// tsc Type (core-interfaces §3). `symbol` is None for intrinsics
/// (tsc leaves it undefined); alias fields arrive with M4 aliases.
#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    pub flags: TypeFlags,
    pub object_flags: ObjectFlags,
    pub symbol: Option<SymbolId>,
    pub alias_symbol: Option<SymbolId>,
    pub alias_type_arguments: Option<Box<[TypeId]>>,
    /// tsc FreshableType.freshType — lazily wired
    /// (getFreshTypeOfLiteralType 63066); eagerly wired for the four
    /// boolean literal intrinsics (47070-47077).
    pub fresh_type: Option<TypeId>,
    /// tsc FreshableType.regularType (createLiteralType 63063:
    /// defaults to self). Also the lazily-computed union regular-type
    /// cache (getRegularTypeOfLiteralType 63078).
    pub regular_type: Option<TypeId>,
    pub data: TypeData,
}

impl Type {
    pub fn new(flags: TypeFlags, data: TypeData) -> Self {
        Self {
            flags,
            object_flags: ObjectFlags::from_bits(0),
            symbol: None,
            alias_symbol: None,
            alias_type_arguments: None,
            fresh_type: None,
            regular_type: None,
            data,
        }
    }
}
