//! The Type data model (greenfield §4.2, core-interfaces §3).
//!
//! Allocation id IS identity: `TypeId` is the arena index, assigned in
//! creation order exactly like tsc's `typeCount` counter (createType,
//! _tsc.js 50095). Interning maps exist ONLY where tsc declares one
//! (tables.rs) — two structurally identical anonymous object types are
//! DISTINCT types.

use crate::flags::{AccessFlags, ElementFlags, IndexFlags, ObjectFlags, TypeFlags};

crate::id_type!(
    /// tsc Type.id (createType 50098-50099). Arena index; ids are
    /// program-run-local and never serialized.
    TypeId
);

crate::id_type!(
    /// tsc TypeMapper object identity. The mapper arena is checker-owned,
    /// but mapped-type semantic payloads retain this opaque identity
    /// without making the types crate depend on the checker.
    MapperId
);

crate::id_type!(
    /// Checker-local conditional-root arena identity. A root is shared by
    /// every distributed/instantiated form of one written conditional
    /// declaration, just as tsc shares the `root` object.
    ConditionalRootId
);

crate::id_type!(
    /// tsc Symbol id space. Lives in tsc-rs-types so Type.symbol can point
    /// at binder/checker symbols without a dependency cycle; the binder
    /// re-exports it and owns the arena.
    /// Its index is a position in the symbol identity space: persistent
    /// symbols lie below `TRANSIENT_SYMBOL_BIT`; checker-transient symbols
    /// carry that bit.
    SymbolId
);

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

/// A JavaScript string stored as UTF-16 code units rather than a Rust
/// `String`.
///
/// JavaScript strings may contain unpaired surrogates. Keeping this
/// payload lossless lets template matching, ordinary string literal
/// types, and synthesized type display preserve inputs such as
/// `\uD800` without identifying them with U+FFFD.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct TemplateText {
    units: Box<[u16]>,
}

impl TemplateText {
    /// Bytes this text owns on the heap, for memory accounting.
    pub fn heap_bytes(&self) -> usize {
        self.units.len() * std::mem::size_of::<u16>()
    }

    pub fn from_utf8(text: &str) -> Self {
        Self {
            units: text.encode_utf16().collect(),
        }
    }

    pub fn from_utf16(units: &[u16]) -> Self {
        Self {
            units: units.into(),
        }
    }

    pub fn from_js(text: crate::JsStr<'_>) -> Self {
        let bytes = text.as_bytes();
        if bytes.is_ascii() {
            // ASCII bytes are their own UTF-16 units.
            return Self {
                units: bytes.iter().map(|&byte| u16::from(byte)).collect(),
            };
        }
        Self {
            units: text.to_utf16().into_boxed_slice(),
        }
    }

    pub fn to_js_string(&self) -> crate::JsString {
        crate::JsString::from_code_units(&self.units)
    }

    pub fn units(&self) -> &[u16] {
        &self.units
    }

    pub fn len(&self) -> usize {
        self.units.len()
    }

    /// The UTF-8 byte length, what Go's `len()` of the text is: tsgo's
    /// node builder charges string literals by it (a lone surrogate
    /// encodes as three bytes, as Go's RuneError does).
    pub fn len_utf8(&self) -> usize {
        let mut bytes = 0;
        let mut units = self.units.iter().copied().peekable();
        while let Some(unit) = units.next() {
            bytes += match unit {
                0..=0x7f => 1,
                0x80..=0x7ff => 2,
                0xd800..=0xdbff
                    if units
                        .peek()
                        .is_some_and(|next| (0xdc00..=0xdfff).contains(next)) =>
                {
                    units.next();
                    4
                }
                _ => 3,
            };
        }
        bytes
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    pub fn push_text(&mut self, other: &Self) {
        let mut units = self.units.to_vec();
        units.extend_from_slice(other.units());
        self.units = units.into_boxed_slice();
    }

    /// Convert only when every UTF-16 unit belongs to a Unicode scalar.
    /// Callers must branch on `None`; replacing an unpaired surrogate
    /// would change JavaScript string identity.
    pub fn to_utf8(&self) -> Option<String> {
        String::from_utf16(&self.units).ok()
    }

    pub fn eq_utf8(&self, text: &str) -> bool {
        self.units.iter().copied().eq(text.encode_utf16())
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

/// tsc LiteralType.value: string | number | PseudoBigInt, read from or
/// given to the tables (`TypeTables::literal_value`; the record holds
/// [`LiteralData`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiteralValue<'a> {
    /// The text, canonical WTF-8 (byte equality is UTF-16 equality).
    String(crate::JsStr<'a>),
    Number(f64),
    BigInt(&'a PseudoBigInt),
}

/// A literal value owned by its holder (a completion list keeps the
/// literals of a union after the checker's borrow ends).
#[derive(Clone, Debug, PartialEq)]
pub enum LiteralValueOwned {
    String(crate::JsString),
    Number(f64),
    BigInt(PseudoBigInt),
}

impl LiteralValue<'_> {
    /// The value with its text or bigint copied.
    pub fn to_owned(self) -> LiteralValueOwned {
        match self {
            LiteralValue::String(text) => LiteralValueOwned::String(text.to_owned()),
            LiteralValue::Number(value) => LiteralValueOwned::Number(value),
            LiteralValue::BigInt(value) => LiteralValueOwned::BigInt(value.clone()),
        }
    }
}

impl LiteralValueOwned {
    /// The value borrowed.
    pub fn as_ref(&self) -> LiteralValue<'_> {
        match self {
            LiteralValueOwned::String(text) => LiteralValue::String(text.as_js()),
            LiteralValueOwned::Number(value) => LiteralValue::Number(*value),
            LiteralValueOwned::BigInt(value) => LiteralValue::BigInt(value),
        }
    }
}

crate::id_type!(
    /// A string literal's text in its tables' text list.
    LiteralTextId
);

crate::id_type!(
    /// A bigint literal's value in its tables' bigint list.
    BigIntId
);

crate::id_type!(
    /// An intrinsic type's names in its tables' intrinsic list.
    IntrinsicId
);

crate::id_type!(
    /// A tuple target's payload in its tables' tuple target list.
    TupleTargetId
);

crate::id_type!(
    /// A template literal type's payload in its tables' template list.
    TemplateLiteralId
);

crate::id_type!(
    /// A type's alias symbol and type arguments in its tables' alias list
    /// (most types have none).
    AliasId
);

/// An `f64` as two words, so a record that holds a number needs no
/// 8-byte alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NumberBits {
    low: u32,
    high: u32,
}

impl NumberBits {
    #[inline]
    pub fn new(value: f64) -> Self {
        let bits = value.to_bits();
        Self {
            low: bits as u32,
            high: (bits >> 32) as u32,
        }
    }

    #[inline]
    pub fn get(self) -> f64 {
        f64::from_bits(u64::from(self.low) | (u64::from(self.high) << 32))
    }
}

/// A literal type's value as the record holds it: a string by its text id,
/// a number by its bits, a bigint by its value id (the tables own the
/// texts and values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiteralData {
    String(LiteralTextId),
    Number(NumberBits),
    BigInt(BigIntId),
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
    /// One synthesized TypeParameter per element (empty for `[]`), as an
    /// interned list.
    pub type_parameters: crate::tables::TypeListId,
    /// Synthesized `thisType` TypeParameter (isThisType, constraint =
    /// this target).
    pub this_type: TypeId,
    pub element_flags: Box<[ElementFlags]>,
    /// countWhere(flags, Required|Variadic) (61158).
    pub min_length: usize,
    /// Length property's type, allocated before the target object as in
    /// createTupleTargetType, even when nobody has read its members yet.
    pub length_type: TypeId,
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MappedTypeData {
    pub declaration: u32,
    pub target: Option<TypeId>,
    pub mapper: Option<MapperId>,
}

/// Immutable semantic inputs of an inferred reverse-mapped object.
///
/// Member and symbol types resolve lazily in the checker. Arrays and
/// tuples are reversed directly and therefore never use this payload.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverseMappedTypeData {
    pub source: TypeId,
    pub mapped_type: TypeId,
    pub constraint_type: TypeId,
}

/// Immutable semantic root of a conditional type.
///
/// The syntax node is stored as a raw NodeId for the same dependency
/// boundary used by mapped types. Mutable instantiation maps stay in
/// the checker rather than on this shared payload.
#[derive(Clone, Debug, PartialEq)]
pub struct ConditionalRootData {
    pub node: u32,
    pub check_type: TypeId,
    pub extends_type: TypeId,
    pub is_distributive: bool,
    pub infer_type_parameters: Box<[TypeId]>,
    pub outer_type_parameters: Option<Box<[TypeId]>>,
    pub alias_symbol: Option<SymbolId>,
    pub alias_type_arguments: Option<Box<[TypeId]>>,
}

/// Immutable per-instantiation fields of a conditional type. Resolved
/// true/false/default/constraint types are checker-owned lazy links.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConditionalTypeData {
    pub root: ConditionalRootId,
    pub check_type: TypeId,
    pub extends_type: TypeId,
    pub mapper: Option<MapperId>,
    pub combined_mapper: Option<MapperId>,
}

/// Immutable semantic inputs of a substitution type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubstitutionTypeData {
    pub base_type: TypeId,
    pub constraint: TypeId,
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

/// An intrinsic type's names (createIntrinsicType 50111); boxed, there
/// being a few dozen intrinsic types.
#[derive(Clone, Debug, PartialEq)]
pub struct IntrinsicData {
    pub name: &'static str,
    pub debug_name: Option<&'static str>,
}

/// getTemplateLiteralType (62057): texts.len() == types.len() + 1.
#[derive(Clone, Debug, PartialEq)]
pub struct TemplateLiteralData {
    pub texts: Box<[TemplateText]>,
    /// The span types, as an interned list.
    pub types: crate::tables::TypeListId,
}

/// Per-kind payload (greenfield §4.2 TypeData). M3 carries the kinds
/// the relation pins can construct; StringMapping landed with M4 5.2;
/// IndexedAccess arrived with the keyof follow-up; Mapped and
/// ReverseMapped are live from phase 9.5; Conditional/Substitution
/// follow in phase 9.6.
///
/// Every variant is identities, bits and small flags (`Copy`, 24 bytes):
/// lists are interned type lists, texts and the few large payloads (tuple
/// targets, template literals, intrinsic names, bigints) are ids into the
/// tables' lists. The tag is a byte and the first variant carries nothing,
/// so an all-zero record is a valid (empty object) type: the type store's
/// contract (`tsc_store::StoreRecord`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum TypeData {
    /// Anonymous object types AND class/interface declared types —
    /// distinguished by object_flags (Anonymous vs Class/Interface).
    /// Members resolve lazily in the checker (resolveStructuredTypeMembers).
    Object,
    /// any/unknown/string/... incl. error/silentNever/wildcard/missing
    /// variants (createIntrinsicType 50111). The boolean literal
    /// intrinsics carry their fresh/regular twins (47070-47077).
    Intrinsic {
        name: IntrinsicId,
        fresh_type: Option<TypeId>,
        regular_type: Option<TypeId>,
    },
    /// String/number/bigint-literal types with their fresh/regular pair
    /// (freshness is STRUCTURE: fresh_type == self).
    Literal {
        value: LiteralData,
        fresh_type: Option<TypeId>,
        regular_type: Option<TypeId>,
    },
    /// Computed enum types (createComputedEnumType 57475): a freshable
    /// TypeFlags::Enum pair carrying the enum (or member) symbol —
    /// minted for enums whose member list is empty or whose members
    /// have no compile-time value; never interned.
    Enum {
        fresh_type: Option<TypeId>,
        regular_type: Option<TypeId>,
    },
    /// `unique symbol` (createUniqueESSymbolType 63112): one per
    /// declaration symbol (memoized in SymbolLinks.uniqueESSymbolType);
    /// escaped_name = `<late-bound prefix><symbol.escapedName>@<symbolId>` — the
    /// late-bound member name known-symbol lookups compare against.
    UniqueESSymbol {
        escaped_name: crate::EscapedName,
    },
    Union {
        /// The members, the interned list the union cache keys by.
        types: crate::tables::TypeListId,
        /// Denormalized origin union/intersection (4.2+).
        origin: Option<TypeId>,
        /// The lazily computed union regular type
        /// (getRegularTypeOfLiteralType 63078).
        regular_type: Option<TypeId>,
    },
    Intersection {
        types: crate::tables::TypeListId,
    },
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
        resolved_type_arguments: Option<crate::tables::TypeListId>,
    },
    /// The synthesized generic tuple TARGET (objectFlags Tuple|Reference):
    /// one payload per tuple shape in the tables' list.
    TupleTarget(TupleTargetId),
    /// Synthesized type parameters (tuple targets, thisType). Real
    /// declared type parameters are M4.
    TypeParameter {
        is_this_type: bool,
        /// tsgo's `isDistributed` (checker.go:23455-23462): the form a
        /// type parameter takes inside the branches of a conditional type
        /// that distributes over it; `constraint` is the original.
        is_distributed: bool,
        constraint: Option<TypeId>,
    },
    /// getTemplateLiteralType (62057): texts.len() == types.len() + 1.
    TemplateLiteral(TemplateLiteralId),
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
    /// Deferred conditional type. The shared root owns the written
    /// declaration and type-parameter environment; this payload owns
    /// the current check/extends pair and mapper identities.
    Conditional(ConditionalTypeData),
    /// Narrowing/NoInfer wrapper interned by `(base, constraint)`.
    Substitution(SubstitutionTypeData),
    /// tsc GenericType (InterfaceType & TypeReference): the declared
    /// type of a class, a generic interface, or a this-ful interface
    /// (getDeclaredTypeOfClassOrInterface 57387-57400). The target
    /// doubles as a reference to itself — `type.target = type;
    /// type.resolvedTypeArguments = type.typeParameters` — encoded by
    /// TypeTables::reference_target / ::type_arguments like tuple
    /// targets.
    GenericType {
        /// outerTypeParameters ++ localTypeParameters (may be empty:
        /// non-generic classes and this-ful interfaces), interned.
        type_parameters: crate::tables::TypeListId,
        /// Length of the outerTypeParameters prefix.
        outer_type_parameter_count: u32,
        /// Synthesized thisType (isThisType, constraint = this target).
        this_type: TypeId,
    },
}

const _: () = assert!(std::mem::size_of::<TypeData>() == 24);

/// tsc Type (core-interfaces §3): 40 bytes of words, in the type store.
/// `symbol` is None for intrinsics (tsc leaves it undefined); the alias
/// symbol and type arguments are one id into the tables' alias list
/// (`TypeTables::alias_symbol`, `alias_type_arguments`); the fresh/regular
/// twins live in the freshable variants' data.
#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    pub flags: TypeFlags,
    pub object_flags: ObjectFlags,
    pub symbol: Option<SymbolId>,
    pub alias: Option<AliasId>,
    pub data: TypeData,
}

const _: () = assert!(std::mem::size_of::<Type>() == 40);
const _: () = assert!(std::mem::align_of::<Type>() == 4);

/// A checker's types live in the type store at leased positions (see
/// `TypeTables::new`); an all-zero record is an empty object type.
impl tsc_store::StoreRecord for Type {
    const REGION: usize = 7;
}

impl Type {
    pub fn new(flags: TypeFlags, data: TypeData) -> Self {
        Self {
            flags,
            object_flags: ObjectFlags::from_bits(0),
            symbol: None,
            alias: None,
            data,
        }
    }

    /// tsc FreshableType.freshType — lazily wired
    /// (getFreshTypeOfLiteralType 63066); eagerly wired for the four
    /// boolean literal intrinsics (47070-47077). None for every other kind.
    #[inline]
    pub fn fresh_type(&self) -> Option<TypeId> {
        match self.data {
            TypeData::Intrinsic { fresh_type, .. }
            | TypeData::Literal { fresh_type, .. }
            | TypeData::Enum { fresh_type, .. } => fresh_type,
            _ => None,
        }
    }

    /// tsc FreshableType.regularType (createLiteralType 63063: defaults to
    /// self), and the lazily-computed union regular-type cache
    /// (getRegularTypeOfLiteralType 63078). None for every other kind.
    #[inline]
    pub fn regular_type(&self) -> Option<TypeId> {
        match self.data {
            TypeData::Intrinsic { regular_type, .. }
            | TypeData::Literal { regular_type, .. }
            | TypeData::Enum { regular_type, .. }
            | TypeData::Union { regular_type, .. } => regular_type,
            _ => None,
        }
    }

    /// Wire the fresh twin of a freshable type.
    pub fn set_fresh_type(&mut self, fresh: Option<TypeId>) {
        match &mut self.data {
            TypeData::Intrinsic { fresh_type, .. }
            | TypeData::Literal { fresh_type, .. }
            | TypeData::Enum { fresh_type, .. } => *fresh_type = fresh,
            _ => unreachable!("fresh types belong to freshable types"),
        }
    }

    /// Wire the regular twin of a freshable type or a union's regular type.
    pub fn set_regular_type(&mut self, regular: Option<TypeId>) {
        match &mut self.data {
            TypeData::Intrinsic { regular_type, .. }
            | TypeData::Literal { regular_type, .. }
            | TypeData::Enum { regular_type, .. }
            | TypeData::Union { regular_type, .. } => *regular_type = regular,
            _ => unreachable!("regular types belong to freshable and union types"),
        }
    }
}

/// A type's alias: the alias symbol and its type arguments
/// (`Type::alias` into `TypeTables`' alias list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeAlias {
    pub symbol: Option<SymbolId>,
    pub type_arguments: Option<crate::tables::TypeListId>,
}
