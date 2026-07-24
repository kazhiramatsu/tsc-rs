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

/// tsc TypeMapper object identity. The mapper arena is checker-owned,
/// but mapped-type semantic payloads retain this opaque identity
/// without making the types crate depend on the checker.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MapperId(pub u32);

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

/// A template literal type's fixed text, stored as JavaScript UTF-16
/// code units rather than a Rust `String`.
///
/// JavaScript strings may contain unpaired surrogates. Keeping this
/// narrow payload lossless lets template matching and synthesized
/// type display preserve inputs such as `\uD800` without changing the
/// UTF-8-facing representation used by ordinary string literal types.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct TemplateText {
    units: Vec<u16>,
}

impl TemplateText {
    pub fn from_utf8(text: &str) -> Self {
        Self {
            units: text.encode_utf16().collect(),
        }
    }

    pub fn from_utf16(units: &[u16]) -> Self {
        Self {
            units: units.to_vec(),
        }
    }

    pub fn units(&self) -> &[u16] {
        &self.units
    }

    pub fn len(&self) -> usize {
        self.units.len()
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    pub fn push_text(&mut self, other: &Self) {
        self.units.extend_from_slice(other.units());
    }

    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.units)
    }
}

impl From<String> for TemplateText {
    fn from(text: String) -> Self {
        Self::from_utf8(&text)
    }
}

impl From<&str> for TemplateText {
    fn from(text: &str) -> Self {
        Self::from_utf8(text)
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

/// Immutable semantic identity of a mapped object type.
///
/// The declaration is a raw syntax NodeId because the types crate is
/// intentionally syntax-free. Root mapped types have no target/mapper;
/// instantiated mapped types retain both. Lazily resolved constraint,
/// name, template, modifier-source, and member data live in the
/// checker's TypeLinks rather than mutating this payload.
#[derive(Clone, Debug, PartialEq)]
pub struct MappedTypeData {
    pub declaration: u32,
    pub target: Option<TypeId>,
    pub mapper: Option<MapperId>,
}

/// Immutable semantic inputs of an inferred reverse-mapped object.
///
/// Member and symbol types resolve lazily in the checker. Arrays and
/// tuples are reversed directly and therefore never use this payload.
#[derive(Clone, Debug, PartialEq)]
pub struct ReverseMappedTypeData {
    pub source: TypeId,
    pub mapped_type: TypeId,
    pub constraint_type: TypeId,
}

/// tsc MappedTypeModifiers, bit-compatible with the checker source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MappedTypeModifiers(i32);

impl MappedTypeModifiers {
    pub const NONE: Self = Self(0);
    pub const INCLUDE_READONLY: Self = Self(1);
    pub const EXCLUDE_READONLY: Self = Self(2);
    pub const INCLUDE_OPTIONAL: Self = Self(4);
    pub const EXCLUDE_OPTIONAL: Self = Self(8);

    pub const fn from_bits(bits: i32) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> i32 {
        self.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for MappedTypeModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

/// Per-kind payload (greenfield §4.2 TypeData). M3 carries the kinds
/// the relation pins can construct; StringMapping landed with M4 5.2;
/// IndexedAccess arrived with the keyof follow-up; Mapped and
/// ReverseMapped are live from phase 9.5; Conditional/Substitution
/// follow in phase 9.6.
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
    /// Computed enum types (createComputedEnumType 57475): a freshable
    /// TypeFlags::Enum pair carrying the enum (or member) symbol —
    /// minted for enums whose member list is empty or whose members
    /// have no compile-time value; never interned.
    Enum,
    /// `unique symbol` (createUniqueESSymbolType 63112): one per
    /// declaration symbol (memoized in SymbolLinks.uniqueESSymbolType);
    /// escaped_name = `__@<symbol.escapedName>@<symbolId>` — the
    /// late-bound member name known-symbol lookups compare against.
    UniqueESSymbol {
        escaped_name: String,
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
    /// Evolving (auto) array types (createEvolvingArrayType 70073,
    /// object_flags EvolvingArray): the element-type accumulator for
    /// `push`/`unshift`/index writes on an autoArrayType variable
    /// inside the flow walk; finalized to a real array type at the
    /// query postlude (finalizeEvolvingArrayType). Never structurally
    /// resolved. The checker memoizes elementType→evolving and
    /// evolving→finalArrayType maps (tsc evolvingArrayTypes /
    /// EvolvingArrayType.finalArrayType).
    EvolvingArray {
        element_type: TypeId,
    },
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
        texts: Box<[TemplateText]>,
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
    /// createObjectType(ObjectFlags::Mapped): declaration identity is
    /// intrinsic to the type; mutable resolutions stay in TypeLinks.
    Mapped(MappedTypeData),
    /// createObjectType(ObjectFlags::ReverseMapped | Anonymous):
    /// homomorphic inference records the source plus its mapped
    /// template and constraint. Mutable members stay in TypeLinks.
    ReverseMapped(ReverseMappedTypeData),
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
