//! TypeTables: the type arena, tsc's exact interning surface, and the
//! option-independent type constructors (greenfield §4.2).
//!
//! Interning maps mirror the checker-scope declarations at _tsc.js
//! 46988-47009 — and ONLY those. There is deliberately no map keyed on
//! anonymous object structure: two written `{}` type literals are
//! distinct types (allocation id is identity).
//!
//! Constructors that read the AST or symbol tables (getTypeFromTypeNode
//! arms, member resolution, signatures) live in the checker; everything
//! here is pure over TypeIds + flags.

// Interning tables keyed by compiler-assigned ids or by canonical key
// strings; never iterated, so the lookup-only hasher applies.
use rustc_hash::FxHashMap as HashMap;
use std::cmp::Ordering;

use crate::flags::{AccessFlags, ElementFlags, ObjectFlags, TypeFlags};
use crate::identity::{IdentityDomain, IdentityLease, IdentitySpace};
use crate::ty::{
    AliasId, BigIntId, ConditionalRootData, ConditionalRootId, ConditionalTypeData, IntrinsicData,
    IntrinsicId, LiteralData, LiteralTextId, LiteralValue, MappedTypeData, MapperId, NumberBits,
    PseudoBigInt, ReverseMappedTypeData, SubstitutionTypeData, SymbolId, TemplateLiteralData,
    TemplateLiteralId, TemplateText, TupleTargetData, TupleTargetId, Type, TypeAlias, TypeData,
    TypeId,
};
use crate::type_order::TypeOrder;
use tsc_store::StoreVec;

/// The named intrinsic/derived types created at checker construction,
/// in tsc's exact allocation order (_tsc.js 47011-47111). Conditional
/// creations (undefinedWideningType/nullWideningType under
/// strictNullChecks) mirror the source ternaries, so id order matches
/// tsc run-for-run under the same options.
#[derive(Clone, Debug)]
pub struct Intrinsics {
    pub any: TypeId,
    pub auto: TypeId,
    pub wildcard: TypeId,
    pub blocked_string: TypeId,
    pub error: TypeId,
    pub unresolved: TypeId,
    pub non_inferrable_any: TypeId,
    pub intrinsic_marker: TypeId,
    pub unknown: TypeId,
    pub undefined: TypeId,
    /// strictNullChecks ? undefinedType : fresh widening intrinsic (47033).
    pub undefined_widening: TypeId,
    pub missing: TypeId,
    /// exactOptionalPropertyTypes ? missingType : undefinedType (47041).
    pub undefined_or_missing: TypeId,
    pub optional: TypeId,
    pub null: TypeId,
    /// strictNullChecks ? nullType : fresh widening intrinsic (47050).
    pub null_widening: TypeId,
    pub string: TypeId,
    pub number: TypeId,
    pub bigint: TypeId,
    /// falseType/trueType are the FRESH boolean literals; the regular
    /// pair follows tsc 47054-47077.
    pub false_fresh: TypeId,
    pub false_regular: TypeId,
    pub true_fresh: TypeId,
    pub true_regular: TypeId,
    /// getUnionType([regularFalseType, regularTrueType]) (47078) —
    /// flags Union|Boolean per getUnionTypeFromSortedList 61634-61637.
    pub boolean: TypeId,
    pub es_symbol: TypeId,
    pub void: TypeId,
    pub never: TypeId,
    pub silent_never: TypeId,
    pub implicit_never: TypeId,
    pub unreachable_never: TypeId,
    pub non_primitive: TypeId,
    pub string_or_number: TypeId,
    pub string_number_symbol: TypeId,
    pub number_or_bigint: TypeId,
    /// getUnionType([string, number, boolean, bigint, null, undefined]) (47101).
    pub template_constraint: TypeId,
    /// getTemplateLiteralType(["", ""], [numberType]) (47102).
    pub numeric_string: TypeId,
    pub unique_literal: TypeId,
}

/// Element flags validated for tuple-target construction. A single
/// rest element (`[...T[]]`) is an Array target in tsc and therefore
/// must be handled by the checker, which owns the global Array types.
/// Keeping the field private makes that layer boundary explicit for
/// every future caller instead of relying on a runtime panic.
#[derive(Clone, Copy, Debug)]
pub struct TupleTargetFlags<'a>(&'a [ElementFlags]);

impl<'a> TupleTargetFlags<'a> {
    pub fn new(flags: &'a [ElementFlags]) -> Option<Self> {
        (!(flags.len() == 1 && flags[0].intersects(ElementFlags::REST))).then_some(Self(flags))
    }

    fn as_slice(self) -> &'a [ElementFlags] {
        self.0
    }
}

/// tsc UnionReduction (checker-scope const enum: None=0, Literal=1,
/// Subtype=2). Subtype stubs to Literal until stage 4.8.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnionReduction {
    None,
    Literal,
    Subtype,
}

/// tsc IntersectionFlags (None=0, NoSupertypeReduction=1,
/// NoConstraintReduction=2) — bit flags on getIntersectionType.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IntersectionFlags(pub i32);

impl IntersectionFlags {
    pub const NONE: Self = Self(0);
    pub const NO_SUPERTYPE_REDUCTION: Self = Self(1);
    pub const NO_CONSTRAINT_REDUCTION: Self = Self(2);

    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}

crate::id_type!(
    /// One interned list of types: equal lists intern to the same id (see
    /// [`TypeTables::intern_type_list`]).
    TypeListId
);

/// tsc getAliasId (60151-60153) as a value: the alias symbol and, with
/// one, the interned alias type arguments. Equal values are equal keys.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AliasKey {
    pub symbol: Option<SymbolId>,
    pub arguments: Option<TypeListId>,
}

/// The origin part of a union's interning key (getUnionTypeFromSortedList
/// 61615-61620): the origin's members for a union or intersection origin,
/// the index type's operand for an origin index type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnionOriginKey {
    None,
    Union(TypeListId),
    Intersection(TypeListId),
    Index(TypeId),
}

/// unionTypes (46989): the sorted member list (or the origin that stands
/// for it), the origin and the alias.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UnionKey {
    pub list: TypeListId,
    pub origin: UnionOriginKey,
    pub alias: AliasKey,
}

/// unionOfUnionTypes (46990): the two-union fast path's key, the smaller
/// id first (getUnionType 61469-61475), with the checker's alias.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UnionOfUnionKey {
    pub first: TypeId,
    pub second: TypeId,
    pub reduction: UnionReduction,
    pub alias: AliasKey,
}

impl UnionOfUnionKey {
    /// The key of `types[0]` and `types[1]`, in id order.
    pub fn of(types: &[TypeId], reduction: UnionReduction, alias: AliasKey) -> Self {
        let index = usize::from(types[0].index() >= types[1].index());
        Self {
            first: types[index],
            second: types[1 - index],
            reduction,
            alias,
        }
    }
}

/// intersectionTypes (46991): the type set and either the alias or the
/// no-constraint-reduction marker (getIntersectionType's `*`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IntersectionKey {
    pub list: TypeListId,
    pub alias: IntersectionAlias,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IntersectionAlias {
    NoConstraintReduction,
    Alias(AliasKey),
}

/// indexedAccessTypes (46996): object, index, the persistent access flags
/// and the alias (getIndexedAccessTypeOrUndefined 62580).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IndexedAccessKey {
    pub object: TypeId,
    pub index: TypeId,
    pub access_flags: i32,
    pub alias: AliasKey,
}

/// The key of a target's instantiations map (`type.instantiations`):
/// createTypeReference's `getTypeListId(typeArguments)` and
/// getObjectTypeInstantiation's list id plus `getAliasId(aliasSymbol,
/// aliasTypeArguments)`, as one value. The alias arguments count only
/// with an alias symbol, as in getAliasId. An instantiation of a single
/// signature type is keyed apart (tsgo getTypeInstantiationKey's `!`), so
/// the target's seeded self entry never answers it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InstantiationKey {
    pub arguments: TypeListId,
    pub alias_symbol: Option<SymbolId>,
    pub alias_arguments: Option<TypeListId>,
    pub single_signature: bool,
}

impl InstantiationKey {
    pub const fn plain(arguments: TypeListId) -> Self {
        Self {
            arguments,
            alias_symbol: None,
            alias_arguments: None,
            single_signature: false,
        }
    }
}

pub struct TypeTables {
    /// The checker's types, in the type store (see [`TypeTables::new`]); a
    /// type's identity is its index here.
    types: StoreVec<Type>,
    /// The lease of the list's store positions, held while the list uses
    /// them.
    #[expect(dead_code, reason = "held for the store positions' lifetime")]
    lease: Option<IdentityLease>,
    pub strict_null_checks: bool,
    pub exact_optional_property_types: bool,
    pub intrinsics: Intrinsics,
    // ---- tsc interning maps (_tsc.js 46988-47009); M3 subset ----
    /// stringLiteralTypes (46992), keyed by the string value: each entry
    /// is a text's id in `literal_texts` and its type, found by the hash
    /// of the text's canonical WTF-8 bytes (byte equality is UTF-16
    /// equality), so the text is held once.
    string_literal_types: hashbrown::HashTable<(LiteralTextId, TypeId)>,
    /// Interned type lists (see [`TypeTables::intern_type_list`]), all in
    /// one list: each list is its length (as the index of a `TypeId`)
    /// followed by its items, and a list's id is its length's position, so
    /// reading a list loads the length and finds the items beside it. The
    /// ids by the list's hash.
    list_items: Vec<TypeId>,
    list_ids: hashbrown::HashTable<TypeListId>,
    /// A list buffer reused by the builders that extend an interned list
    /// (no allocation per call).
    scratch_list: Vec<TypeId>,
    /// The interned empty list.
    empty_list: TypeListId,
    /// What the type records refer to by id: string literal texts
    /// (canonical WTF-8), bigint values, intrinsic names, tuple target and
    /// template literal payloads, and the alias of each aliased type.
    literal_texts: LiteralTexts,
    /// Room for the bytes of an ASCII UTF-16 text being looked up (a
    /// template literal's folded text), reused across lookups.
    literal_scratch: Vec<u8>,
    bigints: Vec<PseudoBigInt>,
    intrinsic_names: Vec<IntrinsicData>,
    tuple_targets: Vec<TupleTargetData>,
    template_literals: Vec<TemplateLiteralData>,
    aliases: Vec<TypeAlias>,
    /// numberLiteralTypes (46993), keyed by the numeric value with JS
    /// Map SameValueZero semantics (-0 and +0 share a key).
    number_literal_types: HashMap<u64, TypeId>,
    /// bigIntLiteralTypes (46994), keyed by pseudoBigIntToString.
    bigint_literal_types: HashMap<String, TypeId>,
    /// enumLiteralTypes (46995), keyed
    /// `${enumId}${string ? "@" : "#"}${value}` (getEnumLiteralType
    /// 63096) — the string interpolation collapses -0/+0 and every
    /// NaN, which EnumLiteralKey reproduces bitwise.
    enum_literal_types: HashMap<(SymbolId, EnumLiteralKey), TypeId>,
    /// unionTypes (46989), keyed by the member list and alias.
    union_types: HashMap<UnionKey, TypeId>,
    /// unionOfUnionTypes (46990) — the 2-union getUnionType fast path.
    union_of_union_types: HashMap<UnionOfUnionKey, TypeId>,
    /// intersectionTypes (46991).
    intersection_types: HashMap<IntersectionKey, TypeId>,
    /// tupleTypes (46988), keyed per getTupleTargetType 61149.
    tuple_types: HashMap<String, TypeId>,
    /// templateLiteralTypes (46997), keyed per getTemplateLiteralType 62083.
    template_literal_types: hashbrown::HashTable<(TemplateLiteralId, TypeId)>,
    /// stringMappingTypes (46998), keyed `${symbolId},${typeId}`
    /// (getStringMappingTypeForGenericType 62154).
    string_mapping_types: HashMap<(SymbolId, TypeId), TypeId>,
    /// indexedAccessTypes (46996).
    indexed_access_types: HashMap<IndexedAccessKey, TypeId>,
    /// substitutionTypes (47007), keyed `${baseId}>${constraintId}`
    /// (getOrCreateSubstitutionType 60440).
    substitution_types: HashMap<(TypeId, TypeId), TypeId>,
    /// Immutable conditional-root objects shared by every
    /// instantiation of one written conditional declaration.
    conditional_roots: Vec<ConditionalRootData>,
    /// Per-target `type.instantiations` maps (createTypeReference
    /// 60170-60174 AND getObjectTypeInstantiation 63489-63492 — one map
    /// per target in tsc), flattened to one table keyed (target, id).
    instantiations: HashMap<(TypeId, InstantiationKey), TypeId>,
}

/// A position in an interned list that does not borrow the tables, so a
/// loop that mutates the checker reads the list's items one at a time
/// (interned lists never change: the items read are the ones a copy would
/// have held).
#[derive(Clone, Copy, Debug)]
pub struct ListCursor {
    next: u32,
    end: u32,
}

impl ListCursor {
    /// The next item, if any.
    #[inline]
    pub fn next(&mut self, tables: &TypeTables) -> Option<TypeId> {
        (self.next < self.end).then(|| {
            let item = tables.list_items[self.next as usize];
            self.next += 1;
            item
        })
    }

    /// The items left.
    #[inline]
    pub fn len(&self) -> usize {
        (self.end - self.next) as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.next == self.end
    }
}

/// How many types a [`TypeListCopy`] holds without a heap buffer.
const COPY_INLINE: usize = 8;

/// A copy of a type list for a loop that mutates the checker while it reads
/// the list: up to eight types are held inline (no allocation; most unions,
/// intersections and type argument lists are that short), longer lists in a
/// heap buffer. It reads as a slice, and iterating it by value yields the
/// types, as iterating a copied `Vec` did.
#[derive(Clone, Debug)]
pub enum TypeListCopy {
    Inline(u8, [TypeId; COPY_INLINE]),
    Heap(Vec<TypeId>),
}

impl TypeListCopy {
    #[inline]
    pub fn new(types: &[TypeId]) -> Self {
        if types.len() <= COPY_INLINE {
            let mut inline = [TypeId::new(0); COPY_INLINE];
            inline[..types.len()].copy_from_slice(types);
            Self::Inline(types.len() as u8, inline)
        } else {
            Self::Heap(types.to_vec())
        }
    }
}

impl std::ops::Deref for TypeListCopy {
    type Target = [TypeId];

    #[inline]
    fn deref(&self) -> &[TypeId] {
        match self {
            Self::Inline(len, items) => &items[..*len as usize],
            Self::Heap(items) => items,
        }
    }
}

/// The types of a [`TypeListCopy`] by value, in order.
pub struct TypeListCopyIter {
    list: TypeListCopy,
    next: usize,
}

impl Iterator for TypeListCopyIter {
    type Item = TypeId;

    #[inline]
    fn next(&mut self) -> Option<TypeId> {
        let item = self.list.get(self.next).copied();
        self.next += usize::from(item.is_some());
        item
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.list.len() - self.next;
        (left, Some(left))
    }
}

impl ExactSizeIterator for TypeListCopyIter {}

impl IntoIterator for TypeListCopy {
    type Item = TypeId;
    type IntoIter = TypeListCopyIter;

    #[inline]
    fn into_iter(self) -> TypeListCopyIter {
        TypeListCopyIter {
            list: self,
            next: 0,
        }
    }
}

impl<'a> IntoIterator for &'a TypeListCopy {
    type Item = &'a TypeId;
    type IntoIter = std::slice::Iter<'a, TypeId>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// The interned list at `id` in `items` (see `TypeTables::list_items`).
#[inline]
fn list_at(items: &[TypeId], id: TypeListId) -> &[TypeId] {
    let at = id.index() as usize;
    let len = items[at].index() as usize;
    &items[at + 1..at + 1 + len]
}

/// The string literal texts of a checker's types, one after another in one
/// byte list (canonical WTF-8), each found by its span: a read is a slice,
/// with no allocation per text and no inline-or-heap branch.
#[derive(Clone, Debug, Default)]
struct LiteralTexts {
    bytes: Vec<u8>,
    spans: Vec<(u32, u32)>,
}

impl LiteralTexts {
    #[inline]
    fn bytes(&self, id: LiteralTextId) -> &[u8] {
        let (start, end) = self.spans[id.index() as usize];
        &self.bytes[start as usize..end as usize]
    }

    #[inline]
    fn get(&self, id: LiteralTextId) -> crate::JsStr<'_> {
        crate::JsStr::from_canonical_bytes(self.bytes(id))
    }

    fn push(&mut self, text: crate::JsStr<'_>) -> LiteralTextId {
        let id = LiteralTextId::new(self.spans.len() as u32);
        let start = u32::try_from(self.bytes.len()).expect("a checker's literal texts fit 4 GiB");
        self.bytes.extend_from_slice(text.as_bytes());
        let end = u32::try_from(self.bytes.len()).expect("a checker's literal texts fit 4 GiB");
        self.spans.push((start, end));
        id
    }

    fn heap_bytes(&self) -> usize {
        self.bytes.capacity() + self.spans.capacity() * std::mem::size_of::<(u32, u32)>()
    }
}

/// The complete former template cache key, without decimal/hex encoding.
fn hash_template_literal_key(types: &[TypeId], texts: &[TemplateText]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    types.hash(&mut hasher);
    texts.hash(&mut hasher);
    hasher.finish()
}

/// The hash of a string literal's text, by its canonical WTF-8 bytes.
#[inline]
fn hash_literal_text(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn hash_type_list(types: &[TypeId]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    types.hash(&mut hasher);
    hasher.finish()
}

/// Append `value` in decimal without the formatting machinery: cache keys
/// (type lists, aliases, relations) are built for every lookup.
pub fn push_decimal(result: &mut String, value: u32) {
    let mut digits = [0u8; 10];
    let mut index = digits.len();
    let mut rest = value;
    loop {
        index -= 1;
        digits[index] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    result.push_str(std::str::from_utf8(&digits[index..]).expect("decimal digits are ASCII"));
}

/// What a checker's type tables allocate, by structure (memory accounting).
#[derive(Clone, Debug, Default)]
pub struct TypesMemory {
    pub types: usize,
    pub type_bytes: usize,
    /// Type lists, texts and payloads the type records own on the heap.
    pub type_heap_bytes: usize,
    pub string_literals: usize,
    /// The string literal types' table (the texts are in `type_heap_bytes`).
    pub string_literal_map_bytes: usize,
    pub type_lists: usize,
    pub type_list_map_bytes: usize,
    /// Union, intersection, tuple, template literal and indexed-access maps
    /// keyed by list-id strings.
    pub string_keyed_entries: usize,
    pub string_keyed_bytes: usize,
    pub instantiations: usize,
    pub other_map_bytes: usize,
}

fn map_bytes<K, V>(map: &HashMap<K, V>) -> usize {
    map.capacity() * (std::mem::size_of::<(K, V)>() + 1) * 8 / 7
}

impl TypeTables {
    /// This checker's type tables by structure.
    pub fn memory_usage(&self) -> TypesMemory {
        // The records sit in the type store's claimed room: the room's
        // untouched tail is address space, so the records' bytes are the
        // length's.
        let mut usage = TypesMemory {
            types: self.types.len(),
            type_bytes: self.types.len() * std::mem::size_of::<Type>(),
            ..TypesMemory::default()
        };
        // What the records refer to by id (alias argument lists and member
        // lists are interned, counted with the lists).
        usage.type_heap_bytes = self.literal_texts.heap_bytes()
            + self
                .bigints
                .iter()
                .map(|value| value.base10_value.capacity())
                .sum::<usize>()
            + self.bigints.capacity() * std::mem::size_of::<PseudoBigInt>()
            + self.intrinsic_names.capacity() * std::mem::size_of::<IntrinsicData>()
            + self
                .tuple_targets
                .iter()
                .map(|data| {
                    std::mem::size_of_val(&*data.element_flags)
                        + data
                            .labeled_element_declarations
                            .as_deref()
                            .map_or(0, std::mem::size_of_val)
                })
                .sum::<usize>()
            + self.tuple_targets.capacity() * std::mem::size_of::<TupleTargetData>()
            + self
                .template_literals
                .iter()
                .map(|data| {
                    data.texts
                        .iter()
                        .map(TemplateText::heap_bytes)
                        .sum::<usize>()
                        + std::mem::size_of_val(&*data.texts)
                })
                .sum::<usize>()
            + self.template_literals.capacity() * std::mem::size_of::<TemplateLiteralData>()
            + self.aliases.capacity() * std::mem::size_of::<TypeAlias>()
            + self
                .types
                .iter()
                .map(|ty| match &ty.data {
                    TypeData::UniqueESSymbol { escaped_name } => escaped_name.heap_bytes(),
                    _ => 0,
                })
                .sum::<usize>();
        usage.string_literals = self.string_literal_types.len();
        usage.string_literal_map_bytes = self.string_literal_types.capacity()
            * (std::mem::size_of::<(LiteralTextId, TypeId)>() + 1);
        usage.type_lists = self.list_ids.len();
        usage.type_list_map_bytes = self.list_ids.capacity()
            * (std::mem::size_of::<TypeListId>() + 1)
            + self.list_items.capacity() * std::mem::size_of::<TypeId>();
        usage.string_keyed_entries += self.tuple_types.len() + self.template_literal_types.len();
        usage.string_keyed_bytes += map_bytes(&self.tuple_types)
            + self.tuple_types.keys().map(String::capacity).sum::<usize>()
            + self.template_literal_types.capacity()
                * (std::mem::size_of::<(TemplateLiteralId, TypeId)>() + 1);
        usage.other_map_bytes += map_bytes(&self.union_types)
            + map_bytes(&self.union_of_union_types)
            + map_bytes(&self.intersection_types)
            + map_bytes(&self.indexed_access_types);
        usage.instantiations = self.instantiations.len();
        usage.other_map_bytes += map_bytes(&self.number_literal_types)
            + map_bytes(&self.bigint_literal_types)
            + map_bytes(&self.enum_literal_types)
            + map_bytes(&self.string_mapping_types)
            + map_bytes(&self.substitution_types)
            + map_bytes(&self.instantiations)
            + self.conditional_roots.capacity() * std::mem::size_of::<ConditionalRootData>();
        usage
    }

    /// The room of a checker's type list in the type store. Untouched
    /// positions cost address space only; a list that outgrows its room
    /// continues on the heap, its indices unchanged.
    const TYPE_ROOM: u32 = 1 << 26;

    /// Tables whose types are built in the type store, at positions leased
    /// from `domain` (on the heap when the domain has no room left: the same
    /// list, read the same way).
    pub fn new(
        strict_null_checks: bool,
        exact_optional_property_types: bool,
        domain: &IdentityDomain,
    ) -> Self {
        let (types, lease) = match domain.lease(IdentitySpace::Type, Self::TYPE_ROOM) {
            Ok(lease) => (
                StoreVec::claim_room(lease.range().start(), Self::TYPE_ROOM),
                Some(lease),
            ),
            Err(_) => (StoreVec::new(), None),
        };
        let mut tables = Self {
            types,
            lease,
            strict_null_checks,
            exact_optional_property_types,
            intrinsics: Intrinsics {
                any: TypeId::new(0),
                auto: TypeId::new(0),
                wildcard: TypeId::new(0),
                blocked_string: TypeId::new(0),
                error: TypeId::new(0),
                unresolved: TypeId::new(0),
                non_inferrable_any: TypeId::new(0),
                intrinsic_marker: TypeId::new(0),
                unknown: TypeId::new(0),
                undefined: TypeId::new(0),
                undefined_widening: TypeId::new(0),
                missing: TypeId::new(0),
                undefined_or_missing: TypeId::new(0),
                optional: TypeId::new(0),
                null: TypeId::new(0),
                null_widening: TypeId::new(0),
                string: TypeId::new(0),
                number: TypeId::new(0),
                bigint: TypeId::new(0),
                false_fresh: TypeId::new(0),
                false_regular: TypeId::new(0),
                true_fresh: TypeId::new(0),
                true_regular: TypeId::new(0),
                boolean: TypeId::new(0),
                es_symbol: TypeId::new(0),
                void: TypeId::new(0),
                never: TypeId::new(0),
                silent_never: TypeId::new(0),
                implicit_never: TypeId::new(0),
                unreachable_never: TypeId::new(0),
                non_primitive: TypeId::new(0),
                string_or_number: TypeId::new(0),
                string_number_symbol: TypeId::new(0),
                number_or_bigint: TypeId::new(0),
                template_constraint: TypeId::new(0),
                numeric_string: TypeId::new(0),
                unique_literal: TypeId::new(0),
            },
            string_literal_types: hashbrown::HashTable::new(),
            list_items: Vec::new(),
            scratch_list: Vec::new(),
            list_ids: hashbrown::HashTable::new(),
            empty_list: TypeListId::new(0),
            literal_texts: LiteralTexts::default(),
            literal_scratch: Vec::new(),
            bigints: Vec::new(),
            intrinsic_names: Vec::new(),
            tuple_targets: Vec::new(),
            template_literals: Vec::new(),
            aliases: Vec::new(),
            number_literal_types: HashMap::default(),
            bigint_literal_types: HashMap::default(),
            enum_literal_types: HashMap::default(),
            union_types: HashMap::default(),
            union_of_union_types: HashMap::default(),
            intersection_types: HashMap::default(),
            tuple_types: HashMap::default(),
            template_literal_types: hashbrown::HashTable::new(),
            string_mapping_types: HashMap::default(),
            indexed_access_types: HashMap::default(),
            substitution_types: HashMap::default(),
            conditional_roots: Vec::new(),
            instantiations: HashMap::default(),
        };
        tables.empty_list = tables.intern_type_list(&[]);
        tables.create_initial_types();
        tables
    }

    /// Reserve lookup-only interning tables from the bound program. Their
    /// bucket order never assigns a type/list identity or visits a member:
    /// those orders live in `types` and `list_items`. The caps bound eager
    /// allocation when a large declaration file is mostly unchecked.
    pub fn reserve_capacity_hint(&mut self, nodes: usize, symbols: usize) {
        let lists = (symbols / 4).min(1 << 16);
        let literals = (nodes / 128).min(1 << 14);
        let items = &self.list_items;
        self.list_ids
            .reserve(lists.saturating_sub(self.list_ids.len()), |&id| {
                hash_type_list(list_at(items, id))
            });
        let texts = &self.literal_texts;
        self.string_literal_types.reserve(
            literals.saturating_sub(self.string_literal_types.len()),
            |&(text, _)| hash_literal_text(texts.bytes(text)),
        );
        self.instantiations
            .reserve(lists.saturating_sub(self.instantiations.len()));
    }

    // ---- arena ----

    /// tsc-port: createType @6.0.3
    /// tsc-hash: 8c00f116e39f7085c9e81dd1e3e17f04ab51ff0630b2c75228a12d38c03ef0e6
    /// tsc-span: _tsc.js:50095-50102
    pub fn create_type(&mut self, flags: TypeFlags, data: TypeData) -> TypeId {
        let id = TypeId::new(self.types.len() as u32);
        self.types.push(Type::new(flags, data));
        id
    }

    /// tsc createObjectType(ObjectFlags::Mapped) plus the mapped-only
    /// semantic fields. Root types carry neither target nor mapper;
    /// instantiated mapped types carry both or neither.
    pub fn create_mapped_type(
        &mut self,
        declaration: u32,
        target: Option<TypeId>,
        mapper: Option<MapperId>,
        symbol: Option<SymbolId>,
    ) -> TypeId {
        assert_eq!(
            target.is_some(),
            mapper.is_some(),
            "mapped target and mapper must be present together"
        );
        let id = self.create_type(
            TypeFlags::OBJECT,
            TypeData::Mapped(MappedTypeData {
                declaration,
                target,
                mapper,
            }),
        );
        let ty = self.type_mut(id);
        ty.object_flags = if target.is_some() {
            ObjectFlags::INSTANTIATED_MAPPED
        } else {
            ObjectFlags::MAPPED
        };
        ty.symbol = symbol;
        id
    }

    /// createObjectType(ObjectFlags::ReverseMapped | Anonymous) plus
    /// the three reverse-inference inputs assigned at 68422-68424.
    pub fn create_reverse_mapped_type(
        &mut self,
        source: TypeId,
        mapped_type: TypeId,
        constraint_type: TypeId,
    ) -> TypeId {
        let id = self.create_type(
            TypeFlags::OBJECT,
            TypeData::ReverseMapped(ReverseMappedTypeData {
                source,
                mapped_type,
                constraint_type,
            }),
        );
        self.type_mut(id).object_flags = ObjectFlags::REVERSE_MAPPED | ObjectFlags::ANONYMOUS;
        id
    }

    pub fn create_conditional_root(&mut self, data: ConditionalRootData) -> ConditionalRootId {
        let id = ConditionalRootId::new(self.conditional_roots.len() as u32);
        self.conditional_roots.push(data);
        id
    }

    pub fn conditional_root(&self, id: ConditionalRootId) -> &ConditionalRootData {
        &self.conditional_roots[id.index() as usize]
    }

    /// The allocation half of getConditionalType. Resolution and
    /// distribution are checker-owned because they read relations and
    /// syntax; this constructor preserves the exact semantic payload.
    pub fn create_conditional_type(
        &mut self,
        data: ConditionalTypeData,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> TypeId {
        let id = self.create_type(TypeFlags::CONDITIONAL, TypeData::Conditional(data));
        let alias_type_arguments = self.intern_alias_arguments(alias_type_arguments);
        self.set_alias(id, alias_symbol, alias_type_arguments);
        id
    }

    // ---- aliases ----

    /// `type.aliasSymbol`.
    #[inline]
    pub fn alias_symbol(&self, id: TypeId) -> Option<SymbolId> {
        self.type_of(id)
            .alias
            .and_then(|alias| self.aliases[alias.index() as usize].symbol)
    }

    /// The type's alias symbol and interned type arguments.
    #[inline]
    pub fn alias(&self, id: TypeId) -> Option<TypeAlias> {
        self.type_of(id)
            .alias
            .map(|alias| self.aliases[alias.index() as usize])
    }

    /// Set `type.aliasSymbol` and `type.aliasTypeArguments` (a type keeps
    /// one alias record; none when both are absent).
    pub fn set_alias(
        &mut self,
        id: TypeId,
        symbol: Option<SymbolId>,
        type_arguments: Option<TypeListId>,
    ) {
        let alias = TypeAlias {
            symbol,
            type_arguments,
        };
        let slot = match self.type_of(id).alias {
            _ if symbol.is_none() && type_arguments.is_none() => None,
            Some(existing) => {
                self.aliases[existing.index() as usize] = alias;
                Some(existing)
            }
            None => {
                let new = AliasId::new(self.aliases.len() as u32);
                self.aliases.push(alias);
                Some(new)
            }
        };
        self.type_mut(id).alias = slot;
    }

    /// Copy `source`'s alias to `target` (the instantiation and clone
    /// constructors that keep the alias of what they copy).
    pub fn copy_alias(&mut self, target: TypeId, source: TypeId) {
        let alias = self.alias(source);
        self.set_alias(
            target,
            alias.and_then(|alias| alias.symbol),
            alias.and_then(|alias| alias.type_arguments),
        );
    }

    // ---- payloads by id ----

    /// A string literal's text.
    #[inline]
    pub fn literal_text(&self, id: LiteralTextId) -> crate::JsStr<'_> {
        self.literal_texts.get(id)
    }

    /// A bigint literal's value.
    #[inline]
    pub fn bigint(&self, id: BigIntId) -> &PseudoBigInt {
        &self.bigints[id.index() as usize]
    }

    /// A literal type's value (None for a type that is no literal).
    #[inline]
    pub fn literal_value(&self, id: TypeId) -> Option<LiteralValue<'_>> {
        match self.type_of(id).data {
            TypeData::Literal { value, .. } => Some(self.literal_value_of(value)),
            _ => None,
        }
    }

    /// Whether two literal records' data name equal values: one text or
    /// bigint id is equal to itself without reading it.
    #[inline]
    pub fn literal_data_equal(&self, a: LiteralData, b: LiteralData) -> bool {
        match (a, b) {
            (LiteralData::String(a), LiteralData::String(b)) => {
                a == b || self.literal_text(a) == self.literal_text(b)
            }
            (LiteralData::Number(a), LiteralData::Number(b)) => a.get() == b.get(),
            (LiteralData::BigInt(a), LiteralData::BigInt(b)) => {
                a == b || self.bigint(a) == self.bigint(b)
            }
            _ => false,
        }
    }

    /// The value a literal record's data names.
    #[inline]
    pub fn literal_value_of(&self, value: LiteralData) -> LiteralValue<'_> {
        match value {
            LiteralData::String(text) => LiteralValue::String(self.literal_text(text)),
            LiteralData::Number(bits) => LiteralValue::Number(bits.get()),
            LiteralData::BigInt(value) => LiteralValue::BigInt(self.bigint(value)),
        }
    }

    /// An intrinsic type's names.
    #[inline]
    pub fn intrinsic_data(&self, id: IntrinsicId) -> &IntrinsicData {
        &self.intrinsic_names[id.index() as usize]
    }

    /// `type.intrinsicName` of an intrinsic type (None otherwise).
    #[inline]
    pub fn intrinsic_name(&self, id: TypeId) -> Option<&'static str> {
        match self.type_of(id).data {
            TypeData::Intrinsic { name, .. } => Some(self.intrinsic_data(name).name),
            _ => None,
        }
    }

    /// A tuple target's payload.
    #[inline]
    pub fn tuple_target(&self, id: TupleTargetId) -> &TupleTargetData {
        &self.tuple_targets[id.index() as usize]
    }

    /// The tuple target payload of a tuple target type (None otherwise).
    #[inline]
    pub fn tuple_target_of(&self, id: TypeId) -> Option<&TupleTargetData> {
        match self.type_of(id).data {
            TypeData::TupleTarget(target) => Some(self.tuple_target(target)),
            _ => None,
        }
    }

    /// A template literal type's payload.
    #[inline]
    pub fn template_literal(&self, id: TemplateLiteralId) -> &TemplateLiteralData {
        &self.template_literals[id.index() as usize]
    }

    /// The members of a union or intersection type (None otherwise).
    #[inline]
    pub fn union_or_intersection_members(&self, id: TypeId) -> Option<&[TypeId]> {
        match self.type_of(id).data {
            TypeData::Union { types, .. } | TypeData::Intersection { types } => {
                Some(self.list(types))
            }
            _ => None,
        }
    }

    /// The members of a union or intersection type, which `id` must be.
    #[inline]
    pub fn members_of(&self, id: TypeId) -> &[TypeId] {
        self.union_or_intersection_members(id)
            .unwrap_or_else(|| unreachable!("union/intersection flag implies member data"))
    }

    /// tsc-port: getSubstitutionType @6.0.3
    /// tsc-hash: 29e3ffe089091bb53cd7d22c227b19a1218c758083d65ddaf3a53641137109db
    /// tsc-span: _tsc.js:60431-60433
    pub fn get_substitution_type(&mut self, base_type: TypeId, constraint: TypeId) -> TypeId {
        if self
            .flags_of(constraint)
            .intersects(TypeFlags::ANY_OR_UNKNOWN)
            || constraint == base_type
            || self.flags_of(base_type).intersects(TypeFlags::ANY)
        {
            base_type
        } else {
            self.get_or_create_substitution_type(base_type, constraint)
        }
    }

    /// tsc-port: getOrCreateSubstitutionType @6.0.3
    /// tsc-hash: 7498c528d8c1f8f140dc1ca555944da7013098688ed1a92199cff2abbbe19059
    /// tsc-span: _tsc.js:60434-60445
    pub fn get_or_create_substitution_type(
        &mut self,
        base_type: TypeId,
        constraint: TypeId,
    ) -> TypeId {
        if let Some(&cached) = self.substitution_types.get(&(base_type, constraint)) {
            return cached;
        }
        let id = self.create_type(
            TypeFlags::SUBSTITUTION,
            TypeData::Substitution(SubstitutionTypeData {
                base_type,
                constraint,
            }),
        );
        self.substitution_types.insert((base_type, constraint), id);
        id
    }

    pub fn substitution_data(&self, id: TypeId) -> Option<&SubstitutionTypeData> {
        match &self.type_of(id).data {
            TypeData::Substitution(data) => Some(data),
            _ => None,
        }
    }

    /// tsc-port: isNoInferType @6.0.3
    /// tsc-hash: 32d14160415d1fc5aa0ab2a5bdf66de37add6af7de1320fd38937f4d4d7d610e
    /// tsc-span: _tsc.js:60428-60430
    pub fn is_no_infer_type(&self, id: TypeId) -> bool {
        self.substitution_data(id).is_some_and(|data| {
            self.flags_of(data.constraint)
                .intersects(TypeFlags::UNKNOWN)
        })
    }

    /// The record of `id`: one load from the type store (a type's identity
    /// is its index in this checker's list, which never moves).
    #[inline(always)]
    pub fn type_of(&self, id: TypeId) -> &Type {
        self.types.record(id.index())
    }

    pub fn type_mut(&mut self, id: TypeId) -> &mut Type {
        &mut self.types[id.index() as usize]
    }

    /// Reserve room for `additional` more types so a checker sized from its
    /// syntax fills the arena without the doubling copies of a growing
    /// vector. Untouched capacity stays virtual memory. The interning maps
    /// grow on their own: a hash table spreads its entries over every page
    /// it reserves, so a reservation sized for the largest programs was
    /// resident in full on every checker (VS Code: 20 MiB per checker for
    /// 45,000 string literals), while rehashing costs well under a
    /// millisecond.
    pub fn reserve_types(&mut self, additional: usize) {
        self.types.reserve(additional);
    }

    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    pub fn flags_of(&self, id: TypeId) -> TypeFlags {
        self.type_of(id).flags
    }

    pub fn object_flags_of(&self, id: TypeId) -> ObjectFlags {
        self.type_of(id).object_flags
    }

    // ---- intrinsics (_tsc.js 47011-47111) ----

    /// tsc-port: createIntrinsicType @6.0.3
    /// tsc-hash: 0ca13ea8127906bdc9c068b5c0cbd39317a3155c4854006be85e065647aceb46
    /// tsc-span: _tsc.js:50111-50118
    fn create_intrinsic_type(
        &mut self,
        flags: TypeFlags,
        name: &'static str,
        object_flags: ObjectFlags,
        debug_name: Option<&'static str>,
    ) -> TypeId {
        let name = {
            let id = IntrinsicId::new(self.intrinsic_names.len() as u32);
            self.intrinsic_names
                .push(IntrinsicData { name, debug_name });
            id
        };
        let id = self.create_type(
            flags,
            TypeData::Intrinsic {
                name,
                fresh_type: None,
                regular_type: None,
            },
        );
        // objectFlags | CouldContainTypeVariablesComputed |
        // IsGenericTypeComputed | IsUnknownLikeUnionComputed |
        // IsNeverIntersectionComputed (50116).
        self.type_mut(id).object_flags =
            ObjectFlags::from_bits(object_flags.bits() | 524288 | 2097152 | 33554432 | 16777216);
        id
    }

    /// tsc getTypeFromTypeAliasReference's Unresolved arm creates an
    /// error intrinsic per alias identity, then stamps the synthetic
    /// unresolved TypeAlias symbol and its written type arguments.
    ///
    /// The JS debugIntrinsicName (`alias ${id}`) is debugger-only and
    /// deliberately omitted from the semantic payload.
    pub fn create_error_type_with_alias(
        &mut self,
        alias_symbol: SymbolId,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> TypeId {
        let none = ObjectFlags::from_bits(0);
        let id = self.create_intrinsic_type(TypeFlags::ANY, "error", none, None);
        let alias_type_arguments = self.intern_alias_arguments(alias_type_arguments);
        self.set_alias(id, Some(alias_symbol), alias_type_arguments);
        id
    }

    /// The initial type block, in tsc's allocation order (47011-47111)
    /// so ids line up run-for-run — EVERY type-allocating statement in
    /// that span is materialized (the mapper vars allocate no types);
    /// skipping one would shift every later id and reorder sorted
    /// unions relative to the oracle.
    fn create_initial_types(&mut self) {
        let none = ObjectFlags::from_bits(0);
        let widening = ObjectFlags::CONTAINS_WIDENING_TYPE;
        let non_inferrable = ObjectFlags::NON_INFERRABLE_TYPE;

        let any = self.create_intrinsic_type(TypeFlags::ANY, "any", none, None);
        let auto = self.create_intrinsic_type(TypeFlags::ANY, "any", non_inferrable, Some("auto"));
        let wildcard = self.create_intrinsic_type(TypeFlags::ANY, "any", none, Some("wildcard"));
        let blocked_string =
            self.create_intrinsic_type(TypeFlags::ANY, "any", none, Some("blocked string"));
        let error = self.create_intrinsic_type(TypeFlags::ANY, "error", none, None);
        let unresolved = self.create_intrinsic_type(TypeFlags::ANY, "unresolved", none, None);
        let non_inferrable_any =
            self.create_intrinsic_type(TypeFlags::ANY, "any", widening, Some("non-inferrable"));
        let intrinsic_marker = self.create_intrinsic_type(TypeFlags::ANY, "intrinsic", none, None);
        let unknown = self.create_intrinsic_type(TypeFlags::UNKNOWN, "unknown", none, None);
        let undefined = self.create_intrinsic_type(TypeFlags::UNDEFINED, "undefined", none, None);
        let undefined_widening = if self.strict_null_checks {
            undefined
        } else {
            self.create_intrinsic_type(
                TypeFlags::UNDEFINED,
                "undefined",
                widening,
                Some("widening"),
            )
        };
        let missing =
            self.create_intrinsic_type(TypeFlags::UNDEFINED, "undefined", none, Some("missing"));
        let undefined_or_missing = if self.exact_optional_property_types {
            missing
        } else {
            undefined
        };
        let optional =
            self.create_intrinsic_type(TypeFlags::UNDEFINED, "undefined", none, Some("optional"));
        let null = self.create_intrinsic_type(TypeFlags::NULL, "null", none, None);
        let null_widening = if self.strict_null_checks {
            null
        } else {
            self.create_intrinsic_type(TypeFlags::NULL, "null", widening, Some("widening"))
        };
        let string = self.create_intrinsic_type(TypeFlags::STRING, "string", none, None);
        let number = self.create_intrinsic_type(TypeFlags::NUMBER, "number", none, None);
        let bigint = self.create_intrinsic_type(TypeFlags::BIG_INT, "bigint", none, None);

        // Boolean literal quadruple + eager fresh/regular wiring
        // (47054-47077). The fresh nodes carry the "fresh" debug name.
        let false_fresh =
            self.create_intrinsic_type(TypeFlags::BOOLEAN_LITERAL, "false", none, Some("fresh"));
        let false_regular =
            self.create_intrinsic_type(TypeFlags::BOOLEAN_LITERAL, "false", none, None);
        let true_fresh =
            self.create_intrinsic_type(TypeFlags::BOOLEAN_LITERAL, "true", none, Some("fresh"));
        let true_regular =
            self.create_intrinsic_type(TypeFlags::BOOLEAN_LITERAL, "true", none, None);
        for (fresh, regular) in [(true_fresh, true_regular), (false_fresh, false_regular)] {
            self.type_mut(fresh).set_regular_type(Some(regular));
            self.type_mut(fresh).set_fresh_type(Some(fresh));
            self.type_mut(regular).set_regular_type(Some(regular));
            self.type_mut(regular).set_fresh_type(Some(fresh));
        }
        // The intrinsic unions below read the same under both orders: the
        // stable comparator ranks these intrinsics by TypeFlags value in their
        // creation order, so the id order is passed.
        let boolean = self.get_union_type(
            None,
            &[false_regular, true_regular],
            UnionReduction::Literal,
        );

        let es_symbol = self.create_intrinsic_type(TypeFlags::ES_SYMBOL, "symbol", none, None);
        let void = self.create_intrinsic_type(TypeFlags::VOID, "void", none, None);
        let never = self.create_intrinsic_type(TypeFlags::NEVER, "never", none, None);
        let silent_never =
            self.create_intrinsic_type(TypeFlags::NEVER, "never", non_inferrable, Some("silent"));
        let implicit_never =
            self.create_intrinsic_type(TypeFlags::NEVER, "never", none, Some("implicit"));
        let unreachable_never =
            self.create_intrinsic_type(TypeFlags::NEVER, "never", none, Some("unreachable"));
        let non_primitive =
            self.create_intrinsic_type(TypeFlags::NON_PRIMITIVE, "object", none, None);
        let string_or_number =
            self.get_union_type(None, &[string, number], UnionReduction::Literal);
        let string_number_symbol =
            self.get_union_type(None, &[string, number, es_symbol], UnionReduction::Literal);
        let number_or_bigint =
            self.get_union_type(None, &[number, bigint], UnionReduction::Literal);
        let template_constraint = self.get_union_type(
            None,
            &[string, number, boolean, bigint, null, undefined],
            UnionReduction::Literal,
        );
        let numeric_string =
            self.get_template_literal_type(&[String::new(), String::new()], &[number]);
        let unique_literal =
            self.create_intrinsic_type(TypeFlags::NEVER, "never", none, Some("unique literal"));

        self.intrinsics = Intrinsics {
            any,
            auto,
            wildcard,
            blocked_string,
            error,
            unresolved,
            non_inferrable_any,
            intrinsic_marker,
            unknown,
            undefined,
            undefined_widening,
            missing,
            undefined_or_missing,
            optional,
            null,
            null_widening,
            string,
            number,
            bigint,
            false_fresh,
            false_regular,
            true_fresh,
            true_regular,
            boolean,
            es_symbol,
            void,
            never,
            silent_never,
            implicit_never,
            unreachable_never,
            non_primitive,
            string_or_number,
            string_number_symbol,
            number_or_bigint,
            template_constraint,
            numeric_string,
            unique_literal,
        };
    }

    // ---- literal types (_tsc.js 63060-63101) ----

    /// tsc-port: createLiteralType @6.0.3
    /// tsc-hash: 70ae7834a5e20f8d52883afe8f22acd1f627833120c1bfa6c75d0cf6014ed72d
    /// tsc-span: _tsc.js:63060-63065
    fn create_literal_type(
        &mut self,
        flags: TypeFlags,
        value: LiteralData,
        regular_type: Option<TypeId>,
    ) -> TypeId {
        let id = self.create_type(
            flags,
            TypeData::Literal {
                value,
                fresh_type: None,
                regular_type: None,
            },
        );
        self.type_mut(id)
            .set_regular_type(Some(regular_type.unwrap_or(id)));
        id
    }

    /// The record data of a literal value: a string's text and a bigint's
    /// value join the tables' lists.
    fn literal_data(&mut self, value: LiteralValue<'_>) -> LiteralData {
        match value {
            LiteralValue::String(text) => LiteralData::String(self.literal_texts.push(text)),
            LiteralValue::Number(value) => LiteralData::Number(NumberBits::new(value)),
            LiteralValue::BigInt(value) => {
                let id = BigIntId::new(self.bigints.len() as u32);
                self.bigints.push(value.clone());
                LiteralData::BigInt(id)
            }
        }
    }

    /// tsc-port: getFreshTypeOfLiteralType @6.0.3
    /// tsc-hash: 2879316237152de8adecc53c58c19f6255d644fb4c3288a787622eaff845e1f4
    /// tsc-span: _tsc.js:63066-63076
    pub fn get_fresh_type_of_literal_type(&mut self, id: TypeId) -> TypeId {
        if self.flags_of(id).intersects(TypeFlags::FRESHABLE) {
            if self.type_of(id).fresh_type().is_none() {
                let TypeData::Literal { value, .. } = self.type_of(id).data else {
                    // Boolean literal intrinsics are wired eagerly and
                    // never reach this branch.
                    return self.type_of(id).fresh_type().unwrap_or(id);
                };
                let flags = self.flags_of(id);
                // The fresh twin names the same text or value.
                let fresh = self.create_literal_type(flags, value, Some(id));
                // 63069: createLiteralType(type.flags, type.value,
                // type.symbol, type) — the fresh twin keeps the symbol
                // (enum literals carry their member symbol).
                let symbol = self.type_of(id).symbol;
                self.type_mut(fresh).symbol = symbol;
                self.type_mut(fresh).set_fresh_type(Some(fresh));
                self.type_mut(id).set_fresh_type(Some(fresh));
            }
            return self
                .type_of(id)
                .fresh_type()
                .expect("fresh link just wired");
        }
        id
    }

    /// tsc-port: getRegularTypeOfLiteralType @6.0.3
    /// tsc-hash: 6fadff11dcdf0b9ab35cdbe7d059e1fb44a9b8752fe2bb9a5041ce54797ba276
    /// tsc-span: _tsc.js:63077-63079
    pub fn get_regular_type_of_literal_type(&mut self, order: TypeOrder<'_>, id: TypeId) -> TypeId {
        let flags = self.flags_of(id);
        if flags.intersects(TypeFlags::FRESHABLE) {
            return self
                .type_of(id)
                .regular_type()
                .expect("freshable regular link");
        }
        if flags.intersects(TypeFlags::UNION) {
            if let Some(regular) = self.type_of(id).regular_type() {
                return regular;
            }
            let TypeData::Union { types, .. } = self.type_of(id).data else {
                unreachable!("union flag implies union data");
            };
            let mut mapped: Vec<TypeId> = Vec::with_capacity(self.list(types).len());
            let mut changed = false;
            for index in 0..self.list(types).len() {
                let member = self.list(types)[index];
                let regular = self.get_regular_type_of_literal_type(order, member);
                changed |= regular != member;
                mapped.push(regular);
            }
            // mapType's no-change identity (70050): an all-regular
            // union — an enum declared type in particular — is its own
            // regular twin; re-interning would drop the alias-keyed
            // EnumLiteral stamp.
            let regular = if changed {
                self.get_union_type(order, &mapped, UnionReduction::Literal)
            } else {
                id
            };
            self.type_mut(id).set_regular_type(Some(regular));
            return regular;
        }
        id
    }

    /// tsc-port: isFreshLiteralType @6.0.3
    /// tsc-hash: 7de9df61f3b6f272112277753c6b8cee2045e87ab509a4695367fcfcd7d6eba2
    /// tsc-span: _tsc.js:63080-63082
    pub fn is_fresh_literal_type(&self, id: TypeId) -> bool {
        self.flags_of(id).intersects(TypeFlags::FRESHABLE)
            && self.type_of(id).fresh_type() == Some(id)
    }

    /// tsc-port: getStringLiteralType @6.0.3
    /// tsc-hash: e7516536a59ae1f2232f916cde9c1fd2fe6c1ab99c9469c9ce15761178d84a0d
    /// tsc-span: _tsc.js:63083-63086
    pub fn get_string_literal_type<'a>(&mut self, value: impl Into<crate::JsStr<'a>>) -> TypeId {
        let value = value.into();
        crate::perf::bump(crate::perf::PerfCounter::StringLiteralLookups);
        let bytes = value.as_bytes();
        let hash = hash_literal_text(bytes);
        let texts = &self.literal_texts;
        if let Some(&(_, id)) = self
            .string_literal_types
            .find(hash, |&(text, _)| texts.bytes(text) == bytes)
        {
            crate::perf::bump(crate::perf::PerfCounter::StringLiteralHits);
            return id;
        }
        let text = self.literal_texts.push(value);
        let id =
            self.create_literal_type(TypeFlags::STRING_LITERAL, LiteralData::String(text), None);
        let texts = &self.literal_texts;
        self.string_literal_types
            .insert_unique(hash, (text, id), |&(text, _)| {
                hash_literal_text(texts.bytes(text))
            });
        id
    }

    /// The string literal type of a text held as UTF-16 code units (a
    /// template literal's text), including unpaired surrogates.
    pub fn get_string_literal_type_from_text(&mut self, value: &TemplateText) -> TypeId {
        self.get_string_literal_type_from_utf16(value.units())
    }

    pub fn get_string_literal_type_from_utf16(&mut self, units: &[u16]) -> TypeId {
        // ASCII units are their own bytes, which are canonical WTF-8: narrow
        // them in one pass that also notes any unit past ASCII.
        let mut text = std::mem::take(&mut self.literal_scratch);
        text.clear();
        let mut high = 0u16;
        text.extend(units.iter().map(|&unit| {
            high |= unit;
            unit as u8
        }));
        let id = if high < 0x80 {
            self.get_string_literal_type(crate::JsStr::from_canonical_bytes(&text))
        } else {
            self.get_string_literal_type(crate::JsString::from_code_units(units).as_js())
        };
        self.literal_scratch = text;
        id
    }

    /// tsc-port: getNumberLiteralType @6.0.3
    /// tsc-hash: 429f1ea7085e0b59ba3e7bac892d644cab76b2c1954a46d0489ff38cd197f5dd
    /// tsc-span: _tsc.js:63087-63090
    pub fn get_number_literal_type(&mut self, value: f64) -> TypeId {
        let key = number_map_key(value);
        crate::perf::bump(crate::perf::PerfCounter::NumberLiteralLookups);
        if let Some(&id) = self.number_literal_types.get(&key) {
            crate::perf::bump(crate::perf::PerfCounter::NumberLiteralHits);
            return id;
        }
        let id = self.create_literal_type(
            TypeFlags::NUMBER_LITERAL,
            LiteralData::Number(NumberBits::new(value)),
            None,
        );
        self.number_literal_types.insert(key, id);
        id
    }

    /// tsc-port: getBigIntLiteralType @6.0.3
    /// tsc-hash: 23e38bde33e1cab63c19035809ef876dc81ce39069c200fa1b95ee4ba9b935a8
    /// tsc-span: _tsc.js:63091-63095
    pub fn get_bigint_literal_type(&mut self, value: PseudoBigInt) -> TypeId {
        let key = value.to_base10_string();
        if let Some(&id) = self.bigint_literal_types.get(&key) {
            return id;
        }
        let data = self.literal_data(LiteralValue::BigInt(&value));
        let id = self.create_literal_type(TypeFlags::BIG_INT_LITERAL, data, None);
        self.bigint_literal_types.insert(key, id);
        id
    }

    /// tsc-port: getEnumLiteralType @6.0.3
    /// tsc-hash: c8e9cdc61226148788803c9ca77210091f4d7a34f246486b01f6d38bc582069c
    /// tsc-span: _tsc.js:63096-63101
    ///
    /// The cache key carries the ENUM symbol id; the member `symbol`
    /// only lands on the first-created type, so duplicate member
    /// values inside one enum share the first member's literal type
    /// (tsc's map behaves identically).
    pub fn get_enum_literal_type(
        &mut self,
        value: LiteralValue<'_>,
        enum_id: SymbolId,
        symbol: SymbolId,
    ) -> TypeId {
        let (key, flags) = match value {
            LiteralValue::String(text) => (
                EnumLiteralKey::String(text.to_owned()),
                TypeFlags::from_bits(
                    TypeFlags::ENUM_LITERAL.bits() | TypeFlags::STRING_LITERAL.bits(),
                ),
            ),
            LiteralValue::Number(number) => (
                EnumLiteralKey::Number(enum_literal_number_key(number)),
                TypeFlags::from_bits(
                    TypeFlags::ENUM_LITERAL.bits() | TypeFlags::NUMBER_LITERAL.bits(),
                ),
            ),
            LiteralValue::BigInt(_) => unreachable!(
                "enum member values are string | number (EvaluatorResult never holds bigints)"
            ),
        };
        if let Some(&id) = self.enum_literal_types.get(&(enum_id, key.clone())) {
            return id;
        }
        let data = self.literal_data(value);
        let id = self.create_literal_type(flags, data, None);
        self.type_mut(id).symbol = Some(symbol);
        self.enum_literal_types.insert((enum_id, key), id);
        id
    }

    /// tsc-port: createComputedEnumType @6.0.3
    /// tsc-hash: 4ffe57fd4f2cdca71e2991992b3bfba4253dfe4fa5a38838e6acb53e894fa4f1
    /// tsc-span: _tsc.js:57475-57483
    ///
    /// Returns the REGULAR twin (tsc returns `regularType`); the fresh
    /// twin hangs off Type::fresh_type like the boolean quadruple.
    pub fn create_computed_enum_type(&mut self, symbol: SymbolId) -> TypeId {
        let unwired = TypeData::Enum {
            fresh_type: None,
            regular_type: None,
        };
        let regular = self.create_type(TypeFlags::ENUM, unwired);
        self.type_mut(regular).symbol = Some(symbol);
        let fresh = self.create_type(TypeFlags::ENUM, unwired);
        self.type_mut(fresh).symbol = Some(symbol);
        self.type_mut(regular).set_regular_type(Some(regular));
        self.type_mut(regular).set_fresh_type(Some(fresh));
        self.type_mut(fresh).set_regular_type(Some(regular));
        self.type_mut(fresh).set_fresh_type(Some(fresh));
        regular
    }

    /// tsc-port: createUniqueESSymbolType @6.0.3
    /// tsc-hash: 24679b8eb2b6106a5821310dd0100a840d74facaa1fb3ebcacf571754c8de27d
    /// tsc-span: _tsc.js:63112-63116
    ///
    /// Not freshable; the caller (getESSymbolLikeTypeForNode) supplies
    /// the late-bound `<prefix><name>@<id>` escaped name and owns the per-symbol memo.
    pub fn create_unique_es_symbol_type(
        &mut self,
        symbol: SymbolId,
        escaped_name: crate::EscapedName,
    ) -> TypeId {
        let id = self.create_type(
            TypeFlags::UNIQUE_ES_SYMBOL,
            TypeData::UniqueESSymbol { escaped_name },
        );
        self.type_mut(id).symbol = Some(symbol);
        id
    }

    // ---- list ids & propagating flags ----

    /// The identity of a list of types as one value: equal lists intern to
    /// the same id, so a cache that tsc keys by a `getTypeListId` string can
    /// carry this id in a value key instead of formatting the list.
    pub fn intern_type_list(&mut self, types: &[TypeId]) -> TypeListId {
        let hash = hash_type_list(types);
        let items = &self.list_items;
        if let Some(&id) = self.list_ids.find(hash, |&id| list_at(items, id) == types) {
            return id;
        }
        let at = u32::try_from(self.list_items.len()).expect("type list items fit u32");
        let len = u32::try_from(types.len()).expect("a type list's length fits u32");
        let id = TypeListId::new(at);
        // The length word and payload are one append: reserve both before
        // pushing the word so a boundary cannot grow the pool twice.
        self.list_items.reserve(1 + types.len());
        self.list_items.push(TypeId::new(len));
        self.list_items.extend_from_slice(types);
        let items = &self.list_items;
        self.list_ids
            .insert_unique(hash, id, |&id| hash_type_list(list_at(items, id)));
        id
    }

    /// The interned list `id`.
    #[inline]
    pub fn list(&self, id: TypeListId) -> &[TypeId] {
        list_at(&self.list_items, id)
    }

    /// A copy of the interned list `id` (see [`TypeListCopy`]).
    #[inline]
    pub fn list_copy(&self, id: TypeListId) -> TypeListCopy {
        TypeListCopy::new(self.list(id))
    }

    /// A cursor over the interned list `id` (see [`ListCursor`]).
    #[inline]
    pub fn cursor(&self, id: TypeListId) -> ListCursor {
        let at = id.index();
        ListCursor {
            next: at + 1,
            end: at + 1 + self.list_items[at as usize].index(),
        }
    }

    /// createTypeReference over the interned `arguments` followed by
    /// `last` (getTypeWithThisArgument's this slot), built in a reused
    /// buffer.
    pub fn create_type_reference_with_last(
        &mut self,
        target: TypeId,
        arguments: TypeListId,
        last: TypeId,
    ) -> TypeId {
        let mut buffer = std::mem::take(&mut self.scratch_list);
        buffer.clear();
        buffer.extend_from_slice(self.list(arguments));
        buffer.push(last);
        let id = self.create_type_reference(target, &buffer);
        self.scratch_list = buffer;
        id
    }

    /// `type.aliasTypeArguments`.
    pub fn alias_type_arguments(&self, id: TypeId) -> Option<&[TypeId]> {
        self.alias(id)
            .and_then(|alias| alias.type_arguments)
            .map(|list| self.list(list))
    }

    /// `type.aliasTypeArguments` as the interned list id.
    #[inline]
    pub fn alias_type_argument_list(&self, id: TypeId) -> Option<TypeListId> {
        self.alias(id).and_then(|alias| alias.type_arguments)
    }

    /// Intern `arguments` for a type's alias (none for none).
    pub fn intern_alias_arguments(&mut self, arguments: Option<&[TypeId]>) -> Option<TypeListId> {
        arguments.map(|arguments| self.intern_type_list(arguments))
    }

    /// tsc-port: getTypeListId @6.0.3
    /// tsc-hash: 08bbe30d7ae7370051e576d48d6bf3103d563a65a92d10740ec9f3c4546f9fea
    /// tsc-span: _tsc.js:60128-60150
    pub fn get_type_list_id(&self, types: &[TypeId]) -> String {
        let mut result = String::with_capacity(types.len() * 6);
        self.push_type_list_id(types, &mut result);
        result
    }

    /// `get_type_list_id` appended to `result` (cache keys are built for
    /// every union, alias and instantiation lookup).
    pub fn push_type_list_id(&self, types: &[TypeId], result: &mut String) {
        crate::perf::bump(crate::perf::PerfCounter::TypeListIdCalls);
        let start_len = result.len();
        let length = types.len();
        let mut i = 0;
        while i < length {
            let start_id = types[i].index();
            let mut count = 1usize;
            while i + count < length && types[i + count].index() == start_id + count as u32 {
                count += 1;
            }
            if result.len() > start_len {
                result.push(',');
            }
            push_decimal(result, start_id);
            if count > 1 {
                result.push(':');
                push_decimal(
                    result,
                    u32::try_from(count).expect("type list length fits u32"),
                );
            }
            i += count;
        }
        crate::perf::add(
            crate::perf::PerfCounter::TypeListIdBytes,
            (result.len() - start_len) as u64,
        );
    }

    /// tsc getAliasId as a value (see [`AliasKey`]): the arguments count
    /// only with an alias symbol.
    pub fn alias_key(
        &mut self,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> AliasKey {
        crate::perf::bump(crate::perf::PerfCounter::AliasIdCalls);
        AliasKey {
            symbol: alias_symbol,
            arguments: match (alias_symbol, alias_type_arguments) {
                (Some(_), Some(arguments)) => Some(self.intern_type_list(arguments)),
                _ => None,
            },
        }
    }

    /// tsc-port: getAliasId @6.0.3
    /// tsc-hash: 39a787bbf937e6b559ce913d2244295fd711853991752904a3b4f649be60db54
    /// tsc-span: _tsc.js:60151-60153
    pub fn get_alias_id(
        &self,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> String {
        let mut result = String::new();
        self.push_alias_id(alias_symbol, alias_type_arguments, &mut result);
        result
    }

    /// `get_alias_id` appended to `result`.
    pub fn push_alias_id(
        &self,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
        result: &mut String,
    ) {
        crate::perf::bump(crate::perf::PerfCounter::AliasIdCalls);
        let Some(symbol) = alias_symbol else {
            return;
        };
        result.push('@');
        push_decimal(result, symbol.index());
        if let Some(arguments) = alias_type_arguments {
            result.push(':');
            self.push_type_list_id(arguments, result);
        }
    }

    /// The per-target instantiations map (`type.instantiations`) —
    /// createTypeReference keys by list id; getObjectTypeInstantiation
    /// keys by list id + alias id over the SAME map.
    pub fn instantiation_get(&self, target: TypeId, key: InstantiationKey) -> Option<TypeId> {
        crate::perf::bump(crate::perf::PerfCounter::InstantiationLookups);
        let hit = self.instantiations.get(&(target, key)).copied();
        if hit.is_some() {
            crate::perf::bump(crate::perf::PerfCounter::InstantiationHits);
        }
        hit
    }

    pub fn instantiation_insert(&mut self, target: TypeId, key: InstantiationKey, value: TypeId) {
        self.instantiations.insert((target, key), value);
    }

    /// The instantiations-map key of `type_arguments` under an alias, for a
    /// single signature type or not.
    pub fn instantiation_key(
        &mut self,
        type_arguments: &[TypeId],
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
        single_signature: bool,
    ) -> InstantiationKey {
        InstantiationKey {
            arguments: self.intern_type_list(type_arguments),
            alias_symbol,
            alias_arguments: alias_symbol
                .and(alias_type_arguments)
                .map(|arguments| self.intern_type_list(arguments)),
            single_signature,
        }
    }

    /// tsc-port: getPropagatingFlagsOfTypes @6.0.3
    /// tsc-hash: 57ecf71d451f3f1480dbe807fb4e5d6c52f01b6d265bc22484e5b186307ff685
    /// tsc-span: _tsc.js:60154-60162
    pub fn get_propagating_flags_of_types(
        &self,
        types: &[TypeId],
        exclude_kinds: TypeFlags,
    ) -> ObjectFlags {
        let mut result = 0i32;
        for &member in types {
            if !self.flags_of(member).intersects(exclude_kinds) {
                result |= self.object_flags_of(member).bits();
            }
        }
        ObjectFlags::from_bits(result & ObjectFlags::PROPAGATING_FLAGS.bits())
    }

    // ---- unions (stage 4.2: the full getUnionType port) ----

    /// tsc-port: getUnionType @6.0.3
    /// tsc-hash: c0f3627f0a6e1cabf66d5b8cc24eabef75b60fe2d963fad1203f40d2543baf83
    /// tsc-span: _tsc.js:61505-61531
    ///
    /// Alias parameters (aliasSymbol/aliasTypeArguments) arrive with M4
    /// aliases; getAliasId is the empty string until then, so the
    /// unionOfUnionTypes fast-path key is `{smallerId}{infix}{largerId}`.
    pub fn get_union_type(
        &mut self,
        order: TypeOrder<'_>,
        types: &[TypeId],
        reduction: UnionReduction,
    ) -> TypeId {
        // Subtype reduction needs the relation engine (removeSubtypes
        // 61368) — the checker-side get_union_type_ex handles it; the
        // tables twin serves the pure construction callers, which are
        // all Literal/None.
        debug_assert!(
            reduction != UnionReduction::Subtype,
            "Subtype union reduction goes through CheckerState::get_union_type_ex"
        );
        self.get_union_type_with_origin(order, types, reduction, None)
    }

    /// The origin-carrying getUnionType entry (the `origin` parameter
    /// at 61505); intersection cross-product distribution passes a
    /// denormalized intersection origin through here.
    pub fn get_union_type_with_origin(
        &mut self,
        order: TypeOrder<'_>,
        types: &[TypeId],
        reduction: UnionReduction,
        origin: Option<TypeId>,
    ) -> TypeId {
        if types.is_empty() {
            return self.intrinsics.never;
        }
        if types.len() == 1 {
            return types[0];
        }
        if types.len() == 2
            && origin.is_none()
            && (self.flags_of(types[0]).intersects(TypeFlags::UNION)
                || self.flags_of(types[1]).intersects(TypeFlags::UNION))
        {
            let key = UnionOfUnionKey::of(types, reduction, AliasKey::default());
            crate::perf::bump(crate::perf::PerfCounter::UnionOfUnionLookups);
            if let Some(&id) = self.union_of_union_types.get(&key) {
                crate::perf::bump(crate::perf::PerfCounter::UnionOfUnionHits);
                return id;
            }
            let id = self.get_union_type_worker(order, types, reduction, None);
            self.union_of_union_types.insert(key, id);
            return id;
        }
        self.get_union_type_worker(order, types, reduction, origin)
    }

    /// unionOfUnionTypes fast-path cache access for the checker-side
    /// Subtype-capable getUnionType twin.
    pub fn union_of_union_types_get(&self, key: UnionOfUnionKey) -> Option<TypeId> {
        crate::perf::bump(crate::perf::PerfCounter::UnionOfUnionLookups);
        let hit = self.union_of_union_types.get(&key).copied();
        if hit.is_some() {
            crate::perf::bump(crate::perf::PerfCounter::UnionOfUnionHits);
        }
        hit
    }

    pub fn union_of_union_types_insert(&mut self, key: UnionOfUnionKey, id: TypeId) {
        self.union_of_union_types.insert(key, id);
    }

    /// tsc-port: getUnionTypeWorker @6.0.3
    /// tsc-hash: 93f55d81bb79032838d9e61c845728d71878ed18b25fb3c0463b2fc0aae692a1
    /// tsc-span: _tsc.js:61532-61585
    ///
    /// M3 dispositions inside the worker:
    /// - UnionReduction::Subtype STUBS to Literal reduction until stage
    ///   4.8 flips it on (removeSubtypes + the reduceVoidUndefined flag
    ///   both wait there; nothing before overloads/JOINs consumes it).
    /// - removeStringLiteralsMatchedByTemplateLiterals (61547-61549)
    ///   lives on the CHECKER twin only (its matcher recurses into the
    ///   engine). Twin rule: checker code never calls this worker —
    ///   every checker-side union routes through get_union_type_ex
    ///   (unions.rs). The residual skip is the tables-INTERNAL unions
    ///   (template-literal distribution, tuple rest-window), which can
    ///   carry literal ∪ template mixes and then intern an unreduced
    ///   set — a known identity divergence until those constructors
    ///   move checker-side (M8 — re-owned 2026-07-19, review R14:
    ///   M4 5.3 closed without moving them; the divergence class is
    ///   unchanged); they never take the unionOfUnionTypes
    ///   fast path with such mixes today, so the shared cache stays
    ///   consistent between the twins.
    /// - removeConstrainedTypeVariables (61550-61552) is unreachable:
    ///   ObjectFlags::IsConstrainedTypeVariable intersections are born
    ///   in getIntersectionType step 6 (M4 type variables).
    fn get_union_type_worker(
        &mut self,
        order: TypeOrder<'_>,
        types: &[TypeId],
        reduction: UnionReduction,
        origin: Option<TypeId>,
    ) -> TypeId {
        let mut type_set: Vec<TypeId> = Vec::new();
        #[cfg(feature = "perf-counters")]
        crate::perf::union_collection(crate::perf::UnionCollectionSite::Tables, types.len());
        let includes = self.add_types_to_union(order, &mut type_set, 0, types);
        if reduction != UnionReduction::None {
            if includes & TypeFlags::ANY_OR_UNKNOWN.bits() != 0 {
                return if includes & TypeFlags::ANY.bits() != 0 {
                    if includes & TypeFlags::INCLUDES_WILDCARD.bits() != 0 {
                        self.intrinsics.wildcard
                    } else if includes & TypeFlags::INCLUDES_ERROR.bits() != 0 {
                        self.intrinsics.error
                    } else {
                        self.intrinsics.any
                    }
                } else {
                    self.intrinsics.unknown
                };
            }
            if includes & TypeFlags::UNDEFINED.bits() != 0
                && type_set.len() >= 2
                && type_set[0] == self.intrinsics.undefined
                && type_set[1] == self.intrinsics.missing
            {
                type_set.remove(1);
            }
            if includes
                & (TypeFlags::ENUM.bits()
                    | TypeFlags::LITERAL.bits()
                    | TypeFlags::UNIQUE_ES_SYMBOL.bits()
                    | TypeFlags::TEMPLATE_LITERAL.bits()
                    | TypeFlags::STRING_MAPPING.bits())
                != 0
                || (includes & TypeFlags::VOID.bits() != 0
                    && includes & TypeFlags::UNDEFINED.bits() != 0)
            {
                // reduceVoidUndefined = !!(unionReduction & Subtype):
                // false until 4.8 activates real Subtype reduction.
                self.remove_redundant_literal_types(
                    order,
                    &mut type_set,
                    includes,
                    /*reduce_void_undefined*/ false,
                );
            }
            if includes & TypeFlags::STRING_LITERAL.bits() != 0
                && includes
                    & (TypeFlags::TEMPLATE_LITERAL.bits() | TypeFlags::STRING_MAPPING.bits())
                    != 0
            {
                // removeStringLiteralsMatchedByTemplateLiterals stub —
                // see the worker doc comment (4.6 hook).
            }
            // removeConstrainedTypeVariables (61550-61552) is a
            // relation-dependent reduction: only the CHECKER twin runs
            // it (unions.rs — twin rule). A constrained intersection
            // reaching a tables-INTERNAL union (template distribution,
            // tuple rest-window, e.g. `[...(T & string)[]]` elements)
            // interns unreduced — same ledgered identity-divergence
            // class as the removeStringLiterals skip above, until those
            // constructors move checker-side (M8 — re-owned
            // 2026-07-19, review R14: M4 5.3 closed without moving
            // them).
            if type_set.is_empty() {
                return if includes & TypeFlags::NULL.bits() != 0 {
                    if includes & TypeFlags::INCLUDES_NON_WIDENING_TYPE.bits() != 0 {
                        self.intrinsics.null
                    } else {
                        self.intrinsics.null_widening
                    }
                } else if includes & TypeFlags::UNDEFINED.bits() != 0 {
                    if includes & TypeFlags::INCLUDES_NON_WIDENING_TYPE.bits() != 0 {
                        self.intrinsics.undefined
                    } else {
                        self.intrinsics.undefined_widening
                    }
                } else {
                    self.intrinsics.never
                };
            }
        }
        self.finish_union_type_set(order, type_set, includes, types, None, None, origin)
    }

    /// The getUnionTypeWorker TAIL (61558-61585): named-union origin
    /// denormalization + objectFlags computation + interning. Shared
    /// with the checker-side Subtype-capable twin (stage 4.8).
    #[allow(clippy::too_many_arguments)]
    pub fn finish_union_type_set(
        &mut self,
        order: TypeOrder<'_>,
        type_set: Vec<TypeId>,
        includes: i32,
        types: &[TypeId],
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
        origin: Option<TypeId>,
    ) -> TypeId {
        let mut origin = origin;
        if origin.is_none() && includes & TypeFlags::UNION.bits() != 0 {
            let mut named_unions: Vec<TypeId> = Vec::new();
            self.add_named_unions(&mut named_unions, types);
            let mut reduced_types: Vec<TypeId> = Vec::new();
            for &t in &type_set {
                let in_named = named_unions.iter().any(|&union| {
                    crate::type_order::contains_type(self, order, self.members_of(union), t)
                });
                if !in_named {
                    reduced_types.push(t);
                }
            }
            if alias_symbol.is_none() && named_unions.len() == 1 && reduced_types.is_empty() {
                return named_unions[0];
            }
            let named_types_count: usize = named_unions
                .iter()
                .map(|&union| self.members_of(union).len())
                .sum();
            if named_types_count + reduced_types.len() == type_set.len() {
                for &union in &named_unions {
                    crate::type_order::insert_type(self, order, &mut reduced_types, union);
                }
                origin = Some(
                    self.create_origin_union_or_intersection_type(TypeFlags::UNION, reduced_types),
                );
            }
        }
        let object_flags = ObjectFlags::from_bits(
            (if includes & TypeFlags::NOT_PRIMITIVE_UNION.bits() != 0 {
                0
            } else {
                ObjectFlags::PRIMITIVE_UNION.bits()
            }) | (if includes & TypeFlags::INTERSECTION.bits() != 0 {
                ObjectFlags::CONTAINS_INTERSECTIONS.bits()
            } else {
                0
            }),
        );
        self.get_union_type_from_sorted_list(
            type_set,
            object_flags,
            alias_symbol,
            alias_type_arguments,
            origin,
        )
    }

    /// tsc-port: addTypeToUnion @6.0.3
    /// tsc-hash: 2e2421304f1c9d829df070f06f76534b25526f5f8fa4995ca8c903b4fd8e2bad
    /// tsc-span: _tsc.js:61338-61357
    ///
    /// A single insertion uses content order when requested; explicit ID
    /// ordering keeps the append fast path (61350).
    pub fn add_type_to_union(
        &mut self,
        order: TypeOrder<'_>,
        type_set: &mut Vec<TypeId>,
        mut includes: i32,
        ty: TypeId,
    ) -> i32 {
        if self.include_type_in_union(&mut includes, ty) {
            match order {
                None => match type_set.last() {
                    Some(&last) if ty.index() > last.index() => type_set.push(ty),
                    _ => {
                        if let Err(index) = type_set.binary_search(&ty) {
                            type_set.insert(index, ty);
                        }
                    }
                },
                Some(_) => {
                    crate::type_order::insert_type(self, order, type_set, ty);
                }
            }
        }
        includes
    }

    /// Go's `addTypesToUnion.addType` flags and filtering, shared by bulk
    /// append collection and single insertion. Whether to store this member
    /// is separate from its contribution to `includes`.
    fn include_type_in_union(&self, includes: &mut i32, ty: TypeId) -> bool {
        let flags = self.flags_of(ty).bits();
        if flags & TypeFlags::NEVER.bits() == 0 {
            *includes |= flags & TypeFlags::INCLUDES_MASK.bits();
            if flags & TypeFlags::INSTANTIABLE.bits() != 0 {
                *includes |= TypeFlags::INCLUDES_INSTANTIABLE.bits();
            }
            if flags & TypeFlags::INTERSECTION.bits() != 0
                && self
                    .object_flags_of(ty)
                    .intersects(ObjectFlags::IS_CONSTRAINED_TYPE_VARIABLE)
            {
                *includes |= TypeFlags::INCLUDES_CONSTRAINED_TYPE_VARIABLE.bits();
            }
            if ty == self.intrinsics.wildcard {
                *includes |= TypeFlags::INCLUDES_WILDCARD.bits();
            }
            if self.is_error_type(ty) {
                *includes |= TypeFlags::INCLUDES_ERROR.bits();
            }
            if !self.strict_null_checks && flags & TypeFlags::NULLABLE.bits() != 0 {
                if !self
                    .object_flags_of(ty)
                    .intersects(ObjectFlags::CONTAINS_WIDENING_TYPE)
                {
                    *includes |= TypeFlags::INCLUDES_NON_WIDENING_TYPE.bits();
                }
            } else {
                return true;
            }
        }
        false
    }

    /// tsc-port: addTypesToUnion @6.0.3
    /// tsc-hash: 14b6eea2a85c949d2f78fa0e81a934b7a2168d3e5c42fb693dbcbdee0e65192c
    /// tsc-span: _tsc.js:61358-61367
    ///
    /// With content ordering, append filtered members in source traversal
    /// order, keeping duplicate occurrences until after Go's stable sort
    /// (`checker.go:addTypesToUnion`). Explicit ID ordering retains the
    /// one-by-one ID insertion path.
    pub fn add_types_to_union(
        &mut self,
        order: TypeOrder<'_>,
        type_set: &mut Vec<TypeId>,
        includes: i32,
        types: &[TypeId],
    ) -> i32 {
        let Some(ctx) = order else {
            return self.add_types_to_union_in_place(order, type_set, includes, types);
        };
        let includes = self.add_types_to_union_in_place(order, type_set, includes, types);
        #[cfg(feature = "perf-counters")]
        let sort_length = type_set.len();
        crate::goslices::sort_types_like_tsgo(type_set, |a, b| {
            #[cfg(feature = "perf-counters")]
            crate::perf::union_sort_comparison(sort_length);
            crate::type_order::compare_types(self, ctx, a, b) == Ordering::Less
        });
        type_set.dedup();
        includes
    }

    /// Flatten in source order, suppressing adjacent identical source types.
    /// Content-ordered collection appends; ID-ordered collection inserts.
    fn add_types_to_union_in_place(
        &mut self,
        order: TypeOrder<'_>,
        type_set: &mut Vec<TypeId>,
        mut includes: i32,
        types: &[TypeId],
    ) -> i32 {
        // tsgo addTypesToUnion starts with len(sourceTypes) slots. Recursive
        // flattening also knows the size of each constituent batch.
        type_set.reserve(types.len());
        let mut last_type: Option<TypeId> = None;
        for &ty in types {
            if Some(ty) != last_type {
                includes = if self.flags_of(ty).intersects(TypeFlags::UNION) {
                    let named = self.is_named_union_type(ty);
                    let members = self.members_of(ty).to_vec();
                    #[cfg(feature = "perf-counters")]
                    crate::perf::union_collection(
                        crate::perf::UnionCollectionSite::Recursive,
                        members.len(),
                    );
                    self.add_types_to_union_in_place(
                        order,
                        type_set,
                        includes | (if named { TypeFlags::UNION.bits() } else { 0 }),
                        &members,
                    )
                } else if order.is_some() {
                    if self.include_type_in_union(&mut includes, ty) {
                        type_set.push(ty);
                    }
                    includes
                } else {
                    self.add_type_to_union(None, type_set, includes, ty)
                };
                last_type = Some(ty);
            }
        }
        includes
    }

    /// tsc-port: removeRedundantLiteralTypes @6.0.3
    /// tsc-hash: a63f90845eb37ca5c250c229a45e22b0d616ab03684122545adffe5d897ac53b
    /// tsc-span: _tsc.js:61422-61433
    pub fn remove_redundant_literal_types(
        &mut self,
        order: TypeOrder<'_>,
        types: &mut Vec<TypeId>,
        includes: i32,
        reduce_void_undefined: bool,
    ) {
        let mut i = types.len();
        while i > 0 {
            i -= 1;
            let t = types[i];
            let flags = self.flags_of(t).bits();
            let remove = (flags
                & (TypeFlags::STRING_LITERAL.bits()
                    | TypeFlags::TEMPLATE_LITERAL.bits()
                    | TypeFlags::STRING_MAPPING.bits())
                != 0
                && includes & TypeFlags::STRING.bits() != 0)
                || (flags & TypeFlags::NUMBER_LITERAL.bits() != 0
                    && includes & TypeFlags::NUMBER.bits() != 0)
                || (flags & TypeFlags::BIG_INT_LITERAL.bits() != 0
                    && includes & TypeFlags::BIG_INT.bits() != 0)
                || (flags & TypeFlags::UNIQUE_ES_SYMBOL.bits() != 0
                    && includes & TypeFlags::ES_SYMBOL.bits() != 0)
                || (reduce_void_undefined
                    && flags & TypeFlags::UNDEFINED.bits() != 0
                    && includes & TypeFlags::VOID.bits() != 0)
                || (self.is_fresh_literal_type(t)
                    && crate::type_order::contains_type(
                        self,
                        order,
                        types,
                        self.type_of(t)
                            .regular_type()
                            .expect("freshable regular link"),
                    ));
            if remove {
                types.remove(i);
            }
        }
    }

    /// tsc-port: isNamedUnionType @6.0.3
    /// tsc-hash: 5918f24c6d2d6d1ca28159360fade58ceb3b0bf74601134724051ef167ac8f3b
    /// tsc-span: _tsc.js:61485-61487
    fn is_named_union_type(&self, ty: TypeId) -> bool {
        if !self.flags_of(ty).intersects(TypeFlags::UNION) {
            return false;
        }
        if self.alias_symbol(ty).is_some() {
            return true;
        }
        matches!(
            &self.type_of(ty).data,
            TypeData::Union {
                origin: Some(_),
                ..
            }
        )
    }

    /// tsc-port: addNamedUnions @6.0.3
    /// tsc-hash: 94c03515e02b98df071ed4f1ade5d64e7ff4094852487d8e6a20be83b24a9074
    /// tsc-span: _tsc.js:61488-61499
    fn add_named_unions(&self, named_unions: &mut Vec<TypeId>, types: &[TypeId]) {
        for &t in types {
            if !self.flags_of(t).intersects(TypeFlags::UNION) {
                continue;
            }
            let TypeData::Union { origin, .. } = &self.type_of(t).data else {
                unreachable!("union flag implies union data");
            };
            let origin = *origin;
            let origin_is_union =
                origin.is_some_and(|origin| self.flags_of(origin).intersects(TypeFlags::UNION));
            if self.alias_symbol(t).is_some() || (origin.is_some() && !origin_is_union) {
                if !named_unions.contains(&t) {
                    named_unions.push(t);
                }
            } else if let Some(origin) = origin.filter(|_| origin_is_union) {
                self.add_named_unions(named_unions, self.members_of(origin));
            }
        }
    }

    /// tsc-port: createOriginUnionOrIntersectionType @6.0.3
    /// tsc-hash: 36351a5700e8e23571af5136217b05539f52103b573b3e7ea4a5b6649ca3c53c
    /// tsc-span: _tsc.js:61500-61504
    ///
    /// tsc origin types are id-less (createOriginType 50108 skips
    /// typeCount); here they draw arena ids like every type — origins
    /// are never keyed by id, only by their member list, so the
    /// divergence is unobservable.
    pub fn create_origin_union_or_intersection_type(
        &mut self,
        flags: TypeFlags,
        types: Vec<TypeId>,
    ) -> TypeId {
        let types = self.intern_type_list(&types);
        let data = if flags.intersects(TypeFlags::UNION) {
            TypeData::Union {
                types,
                origin: None,
                regular_type: None,
            }
        } else {
            TypeData::Intersection { types }
        };
        self.create_type(flags, data)
    }

    /// tsc-port: isErrorType @6.0.3
    /// tsc-hash: 2c5f5aabe3bf8bf77f0dbb8160922bdde8b776142a5387117432426d7ca501c4
    /// tsc-span: _tsc.js:55821-55823
    pub fn is_error_type(&self, ty: TypeId) -> bool {
        ty == self.intrinsics.error
            || (self.flags_of(ty).intersects(TypeFlags::ANY) && self.alias_symbol(ty).is_some())
    }

    /// tsc-port: getUnionTypeFromSortedList @6.0.3
    /// tsc-hash: 1e98366b81464c049f6729b888516648a16e01e90f6b6e8264bf775700f93e6b
    /// tsc-span: _tsc.js:61613-61641
    ///
    /// The `#`-prefixed origin key form covers non-union/intersection
    /// origins (M4 index types) and is unreachable here. The
    /// 61634-61637 boolean special case sets flags |= Boolean (the
    /// union's "boolean" intrinsicName is display-only and lands with
    /// T2 display work).
    pub fn get_union_type_from_sorted_list(
        &mut self,
        types: Vec<TypeId>,
        precomputed_object_flags: ObjectFlags,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
        origin: Option<TypeId>,
    ) -> TypeId {
        if types.is_empty() {
            return self.intrinsics.never;
        }
        if types.len() == 1 {
            return types[0];
        }
        // 61615-61620: a union or intersection origin stands for its
        // members in the key; an origin index type (createOriginIndexType)
        // is keyed with the members.
        let members = self.intern_type_list(&types);
        let (list, origin_key) = match origin {
            None => (members, UnionOriginKey::None),
            Some(origin) => match self.type_of(origin).data {
                TypeData::Union { types: list, .. } => (list, UnionOriginKey::Union(list)),
                TypeData::Intersection { types: list } => {
                    (list, UnionOriginKey::Intersection(list))
                }
                TypeData::Index { ty, .. } => (members, UnionOriginKey::Index(ty)),
                _ => unreachable!("union origins are unions/intersections/index types"),
            },
        };
        let key = UnionKey {
            list,
            origin: origin_key,
            alias: self.alias_key(alias_symbol, alias_type_arguments),
        };
        crate::perf::bump(crate::perf::PerfCounter::UnionLookups);
        if let Some(&id) = self.union_types.get(&key) {
            crate::perf::bump(crate::perf::PerfCounter::UnionHits);
            return id;
        }
        let boolean_pair = types.len() == 2
            && self
                .flags_of(types[0])
                .intersects(TypeFlags::BOOLEAN_LITERAL)
            && self
                .flags_of(types[1])
                .intersects(TypeFlags::BOOLEAN_LITERAL);
        let object_flags = ObjectFlags::from_bits(
            precomputed_object_flags.bits()
                | self
                    .get_propagating_flags_of_types(&types, TypeFlags::NULLABLE)
                    .bits(),
        );
        let mut flags = TypeFlags::UNION;
        if boolean_pair {
            flags = TypeFlags::from_bits(flags.bits() | TypeFlags::BOOLEAN.bits());
        }
        let id = self.create_type(
            flags,
            TypeData::Union {
                types: members,
                origin,
                regular_type: None,
            },
        );
        let alias_type_arguments = self.intern_alias_arguments(alias_type_arguments);
        self.type_mut(id).object_flags = object_flags;
        self.set_alias(id, alias_symbol, alias_type_arguments);
        self.union_types.insert(key, id);
        id
    }

    // ---- optionality (undefined widening into optional slots) ----

    /// tsc-port: getOptionalType @6.0.3
    /// tsc-hash: bb5a73a698a53842f916c77432005c59701edb3812c2a8886b2ff40155bcdc4b
    /// tsc-span: _tsc.js:67852-67856
    pub fn get_optional_type(
        &mut self,
        order: TypeOrder<'_>,
        id: TypeId,
        is_property: bool,
    ) -> TypeId {
        debug_assert!(self.strict_null_checks);
        let missing_or_undefined = if is_property {
            self.intrinsics.undefined_or_missing
        } else {
            self.intrinsics.undefined
        };
        if id == missing_or_undefined {
            return id;
        }
        if self.flags_of(id).intersects(TypeFlags::UNION)
            && self.members_of(id).first() == Some(&missing_or_undefined)
        {
            return id;
        }
        self.get_union_type(order, &[id, missing_or_undefined], UnionReduction::Literal)
    }

    /// tsc-port: addOptionality @6.0.3
    /// tsc-hash: 2802085ebd92adbd4005f16c9656c306440c1d540776e43f5c0153c5bc3af21b
    /// tsc-span: _tsc.js:56029-56031
    pub fn add_optionality(
        &mut self,
        order: TypeOrder<'_>,
        id: TypeId,
        is_property: bool,
        is_optional: bool,
    ) -> TypeId {
        if self.strict_null_checks && is_optional {
            self.get_optional_type(order, id, is_property)
        } else {
            id
        }
    }

    // ---- intersections (storage + the pure 4.3 helpers; the
    // getIntersectionType body lives in the checker because
    // isEmptyAnonymousObjectType reads binder symbol tables) ----

    /// tsc-port: createIntersectionType @6.0.3
    /// tsc-hash: 56af6a3c920b516675671661232426c3194fd7375a2ef9d35d0e51997cc016ac
    /// tsc-span: _tsc.js:61777-61788
    ///
    /// Alias fields are M4; intersections keep INSERTION order (the
    /// typeMembershipMap is identity-keyed and order-preserving, so
    /// `A & B` and `B & A` are distinct types, unlike unions).
    pub fn create_intersection_type(
        &mut self,
        types: Vec<TypeId>,
        object_flags: ObjectFlags,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> TypeId {
        let propagating = self.get_propagating_flags_of_types(&types, TypeFlags::NULLABLE);
        let types = self.intern_type_list(&types);
        let id = self.create_type(TypeFlags::INTERSECTION, TypeData::Intersection { types });
        let alias_type_arguments = self.intern_alias_arguments(alias_type_arguments);
        self.type_mut(id).object_flags =
            ObjectFlags::from_bits(object_flags.bits() | propagating.bits());
        self.set_alias(id, alias_symbol, alias_type_arguments);
        id
    }

    /// intersectionTypes map access for the checker-side
    /// getIntersectionType (the map itself is tsc's 46991).
    pub fn intersection_types_get(&self, key: IntersectionKey) -> Option<TypeId> {
        crate::perf::bump(crate::perf::PerfCounter::IntersectionLookups);
        let hit = self.intersection_types.get(&key).copied();
        if hit.is_some() {
            crate::perf::bump(crate::perf::PerfCounter::IntersectionHits);
        }
        hit
    }

    pub fn intersection_types_insert(&mut self, key: IntersectionKey, id: TypeId) {
        self.intersection_types.insert(key, id);
    }

    /// tsc-port: eachUnionContains @6.0.3
    /// tsc-hash: dd8bbe6c4f8b46240e483f5e92565e69ef3d3f801012640f7468c767851bcb05
    /// tsc-span: _tsc.js:61697-61713
    fn each_union_contains(
        &self,
        order: TypeOrder<'_>,
        union_types: &[TypeId],
        ty: TypeId,
    ) -> bool {
        for &union in union_types {
            let members = self.members_of(union);
            if crate::type_order::contains_type(self, order, members, ty) {
                continue;
            }
            if ty == self.intrinsics.missing {
                return crate::type_order::contains_type(
                    self,
                    order,
                    members,
                    self.intrinsics.undefined,
                );
            }
            if ty == self.intrinsics.undefined {
                return crate::type_order::contains_type(
                    self,
                    order,
                    members,
                    self.intrinsics.missing,
                );
            }
            let flags = self.flags_of(ty);
            let primitive = if flags.intersects(TypeFlags::STRING_LITERAL) {
                Some(self.intrinsics.string)
            } else if flags.intersects(TypeFlags::from_bits(
                TypeFlags::ENUM.bits() | TypeFlags::NUMBER_LITERAL.bits(),
            )) {
                Some(self.intrinsics.number)
            } else if flags.intersects(TypeFlags::BIG_INT_LITERAL) {
                Some(self.intrinsics.bigint)
            } else if flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL) {
                Some(self.intrinsics.es_symbol)
            } else {
                None
            };
            match primitive {
                Some(primitive)
                    if crate::type_order::contains_type(self, order, members, primitive) => {}
                _ => return false,
            }
        }
        true
    }

    /// tsc-port: intersectUnionsOfPrimitiveTypes @6.0.3
    /// tsc-hash: 3a0d9554ee46b79147e66ba642df88c96207d15f0df796034c3dbab9e17c4ccf
    /// tsc-span: _tsc.js:61737-61776
    pub fn intersect_unions_of_primitive_types(
        &mut self,
        order: TypeOrder<'_>,
        types: &mut Vec<TypeId>,
    ) -> bool {
        let Some(index) = types.iter().position(|&t| {
            self.object_flags_of(t)
                .intersects(ObjectFlags::PRIMITIVE_UNION)
        }) else {
            return false;
        };
        let mut union_types: Vec<TypeId> = Vec::new();
        let mut i = index + 1;
        while i < types.len() {
            let t = types[i];
            if self
                .object_flags_of(t)
                .intersects(ObjectFlags::PRIMITIVE_UNION)
            {
                if union_types.is_empty() {
                    union_types.push(types[index]);
                }
                union_types.push(t);
                types.remove(i);
            } else {
                i += 1;
            }
        }
        if union_types.is_empty() {
            return false;
        }
        let mut checked: Vec<TypeId> = Vec::new();
        let mut result: Vec<TypeId> = Vec::new();
        let all_members: Vec<TypeId> = union_types
            .iter()
            .flat_map(|&u| self.members_of(u).iter().copied())
            .collect();
        for t in all_members {
            if crate::type_order::insert_type(self, order, &mut checked, t)
                && self.each_union_contains(order, &union_types, t)
            {
                if t == self.intrinsics.undefined
                    && result.first() == Some(&self.intrinsics.missing)
                {
                    continue;
                }
                if t == self.intrinsics.missing
                    && result.first() == Some(&self.intrinsics.undefined)
                {
                    result[0] = self.intrinsics.missing;
                    continue;
                }
                crate::type_order::insert_type(self, order, &mut result, t);
            }
        }
        types[index] = self.get_union_type_from_sorted_list(
            result,
            ObjectFlags::PRIMITIVE_UNION,
            None,
            None,
            None,
        );
        true
    }

    /// tsc-port: containsMissingType @6.0.3
    /// tsc-hash: 7e0d6e0ae9e6bc78feeacabc1f160a8e8020b593453d55d682dd1df22ab6b6fb
    /// tsc-span: _tsc.js:67886-67888
    pub fn contains_missing_type(&self, ty: TypeId) -> bool {
        if ty == self.intrinsics.missing {
            return true;
        }
        if !self.flags_of(ty).intersects(TypeFlags::UNION) {
            return false;
        }
        self.members_of(ty).first() == Some(&self.intrinsics.missing)
    }

    /// tsc-port: everyType @6.0.3
    /// tsc-hash: c4dd71e9f3d68c0f125a76cc961b9eafb0217706e0cfa0c096d2df786cc22253
    /// tsc-span: _tsc.js:69985-69987
    pub fn every_type(&self, ty: TypeId, f: impl Fn(&Self, TypeId) -> bool) -> bool {
        if self.flags_of(ty).intersects(TypeFlags::UNION) {
            self.members_of(ty).iter().all(|&t| f(self, t))
        } else {
            f(self, ty)
        }
    }

    /// tsc-port: filterType @6.0.3
    /// tsc-hash: acc1fb95f1d693ec174389f71c5609dca06afa6c24c05435dfab03b4dae827f4
    /// tsc-span: _tsc.js:69991-70021
    pub fn filter_type(&mut self, ty: TypeId, f: impl Fn(&Self, TypeId) -> bool) -> TypeId {
        if self.flags_of(ty).intersects(TypeFlags::UNION) {
            let TypeData::Union { types, origin, .. } = self.type_of(ty).data else {
                unreachable!("union flag implies union data");
            };
            let types = self.list(types);
            // Most filters keep every member: answer that without a copy.
            if types.iter().all(|&t| f(self, t)) {
                return ty;
            }
            let filtered: Vec<TypeId> = types.iter().copied().filter(|&t| f(self, t)).collect();
            let types_len = types.len();
            let mut new_origin = None;
            if let Some(origin) = origin {
                if self.flags_of(origin).intersects(TypeFlags::UNION) {
                    let origin_types = self.members_of(origin);
                    let origin_filtered: Vec<TypeId> = origin_types
                        .iter()
                        .copied()
                        .filter(|&t| self.flags_of(t).intersects(TypeFlags::UNION) || f(self, t))
                        .collect();
                    if origin_types.len() - origin_filtered.len() == types_len - filtered.len() {
                        if origin_filtered.len() == 1 {
                            return origin_filtered[0];
                        }
                        new_origin = Some(self.create_origin_union_or_intersection_type(
                            TypeFlags::UNION,
                            origin_filtered,
                        ));
                    }
                }
            }
            let object_flags = ObjectFlags::from_bits(
                self.object_flags_of(ty).bits()
                    & (ObjectFlags::PRIMITIVE_UNION.bits()
                        | ObjectFlags::CONTAINS_INTERSECTIONS.bits()),
            );
            return self.get_union_type_from_sorted_list(
                filtered,
                object_flags,
                None,
                None,
                new_origin,
            );
        }
        if self.flags_of(ty).intersects(TypeFlags::NEVER) || f(self, ty) {
            ty
        } else {
            self.intrinsics.never
        }
    }

    /// tsc-port: removeFromEach @6.0.3
    /// tsc-hash: ad294a4d1ee7064aac25b0e83ddceca1d8b633837436f1dad73006e6d47c664e
    /// tsc-span: _tsc.js:61732-61736
    pub fn remove_from_each(&mut self, types: &mut [TypeId], flag: TypeFlags) {
        for slot in types.iter_mut() {
            *slot = self.filter_type(*slot, |tables, t| !tables.flags_of(t).intersects(flag));
        }
    }

    /// tsc-port: getConstituentCount @6.0.3
    /// tsc-hash: a54c64b0add33c0e6883bba90b310d201745020bff8dfced0f04954d8ba10173
    /// tsc-span: _tsc.js:61903-61905
    ///
    /// tsc-port: getConstituentCountOfTypes @6.0.3
    /// tsc-hash: eaad3a531103f2cdc19e9e6a682df9bb69d718812ee0315221c3adfbfe475f35
    /// tsc-span: _tsc.js:61906-61908
    pub fn get_constituent_count(&self, ty: TypeId) -> usize {
        let flags = self.flags_of(ty);
        if !flags.intersects(TypeFlags::UNION_OR_INTERSECTION) || self.alias_symbol(ty).is_some() {
            return 1;
        }
        if flags.intersects(TypeFlags::UNION) {
            if let TypeData::Union {
                origin: Some(origin),
                ..
            } = self.type_of(ty).data
            {
                return self.get_constituent_count(origin);
            }
        }
        self.get_constituent_count_of_types(self.members_of(ty))
    }

    pub fn get_constituent_count_of_types(&self, types: &[TypeId]) -> usize {
        types.iter().map(|&t| self.get_constituent_count(t)).sum()
    }

    // ---- references ----

    /// tsc-port: createTypeReference @6.0.3
    /// tsc-hash: 17f8bfecf79e7fa7858909317b8081cfc45fe59c0e11ba4cae5ba8b38abfeaff
    /// tsc-span: _tsc.js:60169-60180
    pub fn create_type_reference(&mut self, target: TypeId, type_arguments: &[TypeId]) -> TypeId {
        self.create_type_reference_ex(target, type_arguments, ObjectFlags::NONE)
    }

    /// tsgo-port: Checker.createTypeReferenceEx @7.1
    /// (checker/checker.go:25572-25584)
    ///
    /// `extra_flags` reach only a reference this call creates: a reference
    /// that exists already is returned as it is.
    pub fn create_type_reference_ex(
        &mut self,
        target: TypeId,
        type_arguments: &[TypeId],
        extra_flags: ObjectFlags,
    ) -> TypeId {
        let arguments = self.intern_type_list(type_arguments);
        let key = (target, InstantiationKey::plain(arguments));
        if let Some(&id) = self.instantiations.get(&key) {
            return id;
        }
        let symbol = self.type_of(target).symbol;
        let id = self.create_type(
            TypeFlags::OBJECT,
            TypeData::Reference {
                target,
                resolved_type_arguments: Some(arguments),
            },
        );
        let object_flags = ObjectFlags::from_bits(
            ObjectFlags::REFERENCE.bits()
                | extra_flags.bits()
                | self
                    .get_propagating_flags_of_types(type_arguments, TypeFlags::from_bits(0))
                    .bits(),
        );
        self.type_mut(id).object_flags = object_flags;
        self.type_mut(id).symbol = symbol;
        self.instantiations.insert(key, id);
        id
    }

    /// The type-object slice of createDeferredTypeReference (60188-
    /// 60201): a Reference shell with NO resolved type arguments and NO
    /// propagating flags (the arguments are unknown until the checker's
    /// lazy getTypeArguments reads the node). NOT interned — tsc creates
    /// a fresh object per call; identity comes from the caller's caches
    /// (node links / target.instantiations). The node, mapper and alias
    /// fields live checker-side (TypeLinks / Type alias fields).
    /// tsc-port: cloneTypeReference @6.0.3
    /// tsc-hash: acb47c4c400a97d9673a211f924708dc2b88b81a3c9a26d300e701aca159ddc5
    /// tsc-span: _tsc.js:60181-60187
    ///
    /// A fresh UNINTERNED copy — the caller stamps pattern/alias
    /// fields on it without leaking into the shared interned
    /// reference (getTypeFromArrayBindingPattern 56541).
    pub fn clone_type_reference(&mut self, source: TypeId) -> TypeId {
        let source_type = self.type_of(source);
        let (flags, symbol, object_flags) = (
            source_type.flags,
            source_type.symbol,
            source_type.object_flags,
        );
        let data = match source_type.data {
            data @ TypeData::Reference { .. } => data,
            // A zero-arity tuple IS its target (createTupleType 61224
            // returns the tupleTarget for arity 0); the target
            // self-aliases (target = self, resolvedTypeArguments =
            // typeParameters = []), so the clone reads through the
            // same aliasing tsc's dynamic fields encode.
            TypeData::TupleTarget(target) if self.tuple_target(target).element_flags.is_empty() => {
                TypeData::Reference {
                    target: source,
                    resolved_type_arguments: Some(self.intern_type_list(&[])),
                }
            }
            _ => panic!("cloneTypeReference over a non-reference"),
        };
        let id = self.create_type(flags, data);
        self.type_mut(id).object_flags = object_flags;
        self.type_mut(id).symbol = symbol;
        id
    }

    pub fn create_deferred_reference_shell(&mut self, target: TypeId) -> TypeId {
        let symbol = self.type_of(target).symbol;
        let id = self.create_type(
            TypeFlags::OBJECT,
            TypeData::Reference {
                target,
                resolved_type_arguments: None,
            },
        );
        self.type_mut(id).object_flags = ObjectFlags::REFERENCE;
        self.type_mut(id).symbol = symbol;
        id
    }

    /// The `type.resolvedTypeArguments ??= ...` write of getTypeArguments'
    /// resolved arm (60211): the caller tested the slot before it computed
    /// `arguments`, and the assignment then replaces whatever a re-entrant
    /// resolution stored in between.
    pub fn set_resolved_type_arguments(&mut self, id: TypeId, arguments: &[TypeId]) {
        let arguments = self.intern_type_list(arguments);
        let TypeData::Reference {
            resolved_type_arguments,
            ..
        } = &mut self.type_mut(id).data
        else {
            unreachable!("resolved type arguments live on Reference types");
        };
        *resolved_type_arguments = Some(arguments);
    }

    /// The `??=` write of getTypeArguments' circular arm (60213): nothing
    /// is evaluated between the test and the store, so only a vacant slot
    /// is filled.
    pub fn set_resolved_type_arguments_if_vacant(&mut self, id: TypeId, arguments: &[TypeId]) {
        let arguments = self.intern_type_list(arguments);
        let TypeData::Reference {
            resolved_type_arguments,
            ..
        } = &mut self.type_mut(id).data
        else {
            unreachable!("resolved type arguments live on Reference types");
        };
        if resolved_type_arguments.is_none() {
            *resolved_type_arguments = Some(arguments);
        }
    }

    /// The reference target: a plain Reference's target, or the type
    /// itself for tuple targets (tsc `type.target = type`, 61192) and
    /// class/interface GenericType targets (57398).
    pub fn reference_target(&self, id: TypeId) -> TypeId {
        match &self.type_of(id).data {
            TypeData::Reference { target, .. } => *target,
            TypeData::TupleTarget(_) | TypeData::GenericType { .. } => id,
            _ => id,
        }
    }

    /// getTypeArguments for references whose arguments are already
    /// resolved (60202: the `type.resolvedTypeArguments` fast path).
    /// Tuple and class/interface targets alias their own typeParameters
    /// (61193 / 57399). Deferred references that have not been forced
    /// yet are a CALLER BUG here — route through the checker's lazy
    /// CheckerState::get_type_arguments (60202-60225) instead.
    pub fn type_arguments(&self, id: TypeId) -> &[TypeId] {
        self.try_type_arguments(id).unwrap_or_else(|| {
            unreachable!(
                "deferred reference type arguments read before resolution — \
                 route through CheckerState::get_type_arguments"
            )
        })
    }

    /// `type_arguments` that reports unresolved deferred references as
    /// `None` instead of panicking (non-Reference types keep the empty
    /// slice they had under the infallible accessor).
    pub fn try_type_arguments(&self, id: TypeId) -> Option<&[TypeId]> {
        self.try_type_argument_list(id).map(|list| self.list(list))
    }

    /// [`Self::try_type_arguments`] as the interned list id (an empty list
    /// for a type that is no reference).
    pub fn try_type_argument_list(&self, id: TypeId) -> Option<TypeListId> {
        match self.type_of(id).data {
            TypeData::Reference {
                resolved_type_arguments,
                ..
            } => resolved_type_arguments,
            TypeData::TupleTarget(target) => Some(self.tuple_target(target).type_parameters),
            TypeData::GenericType {
                type_parameters, ..
            } => Some(type_parameters),
            _ => Some(self.empty_list),
        }
    }

    // ---- tuples ----

    /// tsc-port: getTupleTargetType @6.0.3
    /// tsc-hash: 180d4131d4865f2a4bb2c6da0d56e1fa6dfd835ca8f37214d9cdc6e3f10f7359
    /// tsc-span: _tsc.js:61145-61155
    ///
    pub fn get_tuple_target_type(
        &mut self,
        order: TypeOrder<'_>,
        element_flags: TupleTargetFlags<'_>,
        readonly: bool,
        named_member_declarations: Option<&[Option<u32>]>,
    ) -> TypeId {
        let element_flags = element_flags.as_slice();
        let mut key = element_flags
            .iter()
            .map(|&flags| {
                if flags.intersects(ElementFlags::REQUIRED) {
                    "#"
                } else if flags.intersects(ElementFlags::OPTIONAL) {
                    "?"
                } else if flags.intersects(ElementFlags::REST) {
                    "."
                } else {
                    "*"
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        if readonly {
            key.push('R');
        }
        // The labeled-member key segment (getNodeId-keyed, 61149).
        let named = named_member_declarations
            .filter(|declarations| declarations.iter().any(Option::is_some));
        if let Some(declarations) = named {
            key.push(',');
            key.push_str(
                &declarations
                    .iter()
                    .map(|declaration| match declaration {
                        Some(id) => id.to_string(),
                        None => "_".to_owned(),
                    })
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        if let Some(&id) = self.tuple_types.get(&key) {
            return id;
        }
        let id = self.create_tuple_target_type(order, element_flags, readonly, named);
        self.tuple_types.insert(key, id);
        id
    }

    /// tsc-port: createTupleTargetType @6.0.3
    /// tsc-hash: 174a20487ba5d9cbae89e49e7ce9bb1b4e37e4de1cc60e38ece5bfd3e9be0490
    /// tsc-span: _tsc.js:61156-61209
    ///
    /// Property symbols are checker-owned and materialized on first member
    /// read. Their types are allocated here in upstream order, including
    /// length literals before the tuple target object (61177-61186).
    fn create_tuple_target_type(
        &mut self,
        order: TypeOrder<'_>,
        element_flags: &[ElementFlags],
        readonly: bool,
        named_member_declarations: Option<&[Option<u32>]>,
    ) -> TypeId {
        let arity = element_flags.len();
        let min_length = element_flags
            .iter()
            .filter(|flags| {
                flags.intersects(ElementFlags::from_bits(
                    ElementFlags::REQUIRED.bits() | ElementFlags::VARIADIC.bits(),
                ))
            })
            .count();
        let mut type_parameters = Vec::with_capacity(arity);
        let mut combined_flags = ElementFlags::from_bits(0);
        let mut fixed_length = 0usize;
        for &flags in element_flags {
            type_parameters.push(self.create_type_parameter(false, None));
            combined_flags |= flags;
            if !combined_flags.intersects(ElementFlags::VARIABLE) {
                fixed_length += 1;
            }
        }
        let has_rest_element = combined_flags.intersects(ElementFlags::VARIABLE);
        let length_type = if has_rest_element {
            self.intrinsics.number
        } else {
            let literals: Vec<TypeId> = (min_length..=arity)
                .map(|length| self.get_number_literal_type(length as f64))
                .collect();
            self.get_union_type(order, &literals, UnionReduction::Literal)
        };
        let type_parameters = self.intern_type_list(&type_parameters);
        let payload = TupleTargetId::new(self.tuple_targets.len() as u32);
        self.tuple_targets.push(TupleTargetData {
            type_parameters,
            // Patched right below, after the target id exists —
            // tsc allocates thisType after the object (61194).
            this_type: TypeId::new(u32::MAX - 1),
            element_flags: element_flags.to_vec().into_boxed_slice(),
            min_length,
            length_type,
            fixed_length,
            has_rest_element,
            combined_flags,
            readonly,
            labeled_element_declarations: named_member_declarations
                .map(|declarations| declarations.to_vec().into_boxed_slice()),
        });
        let target = self.create_type(TypeFlags::OBJECT, TypeData::TupleTarget(payload));
        self.type_mut(target).object_flags =
            ObjectFlags::from_bits(ObjectFlags::TUPLE.bits() | ObjectFlags::REFERENCE.bits());
        let this_type = self.create_type_parameter(true, Some(target));
        self.tuple_targets[payload.index() as usize].this_type = this_type;
        // instantiations.set(getTypeListId(typeParameters), type) (61191).
        let key = (target, InstantiationKey::plain(type_parameters));
        self.instantiations.insert(key, target);
        target
    }

    /// tsc-port: createTypeParameter @6.0.3
    /// tsc-hash: f1f92ddef5952eadb0e36a8f3d03cede86a4e4652f64eb186d08fd99034af3a1
    /// tsc-span: _tsc.js:50139-50141
    fn create_type_parameter(&mut self, is_this_type: bool, constraint: Option<TypeId>) -> TypeId {
        self.create_type(
            TypeFlags::TYPE_PARAMETER,
            TypeData::TypeParameter {
                is_this_type,
                is_distributed: false,
                constraint,
            },
        )
    }

    /// createTypeParameter for the checker's symbol-less marker
    /// parameters (47212-47215) — inline constraint, no thisType.
    pub fn create_synthesized_type_parameter(&mut self, constraint: Option<TypeId>) -> TypeId {
        self.create_type_parameter(/*is_this_type*/ false, constraint)
    }

    /// tsc-port: createTupleType @6.0.3
    /// tsc-hash: b2054126131f4beb527c2fc7b6ccacc23a1a2c0fecb62b02e76f01b77f1468f6
    /// tsc-span: _tsc.js:61141-61144
    #[cfg(test)]
    pub fn create_tuple_type(
        &mut self,
        order: TypeOrder<'_>,
        element_types: &[TypeId],
        element_flags: Option<&[ElementFlags]>,
        readonly: bool,
        named_member_declarations: Option<&[Option<u32>]>,
    ) -> Option<TypeId> {
        let default_flags;
        let element_flags = match element_flags {
            Some(flags) => flags,
            None => {
                default_flags = vec![ElementFlags::REQUIRED; element_types.len()];
                &default_flags
            }
        };
        let flags = TupleTargetFlags::new(element_flags)?;
        let target = self.get_tuple_target_type(order, flags, readonly, named_member_declarations);
        if element_types.is_empty() {
            return Some(target);
        }
        self.create_normalized_type_reference(order, target, element_types)
    }

    /// tsc-port: createNormalizedTypeReference @6.0.3
    /// tsc-hash: 32b91334e6762e8ea63ac6a9be5f6689a4d112aa1db2d59986c736ac6735e143
    /// tsc-span: _tsc.js:61210-61212
    #[cfg(test)]
    pub fn create_normalized_type_reference(
        &mut self,
        order: TypeOrder<'_>,
        target: TypeId,
        type_arguments: &[TypeId],
    ) -> Option<TypeId> {
        if self.object_flags_of(target).intersects(ObjectFlags::TUPLE) {
            self.create_normalized_tuple_type(order, target, type_arguments)
        } else {
            Some(self.create_type_reference(target, type_arguments))
        }
    }

    /// tsc-port: createNormalizedTupleType @6.0.3
    /// tsc-hash: 5b7968f648c63d88544746d841015ff7800b723dbc071b96fb4d6f7ae0b18154
    /// tsc-span: _tsc.js:61213-61287
    ///
    /// The tables twin is compiled only for this crate's unit tests.
    /// The checker twin (annotate.rs create_normalized_tuple_type_full)
    /// owns every production normalization, so checker-dependent arms
    /// (union-in-variadic cross product 61218-61223, array-like
    /// variadic collapse 61252, variadic-in-rest-window collapse
    /// 61262, deferred-argument forcing 61240) are proven-dead guards
    /// here (L-TWIN caller inventory, M4-end sweep).
    #[cfg(test)]
    pub fn create_normalized_tuple_type(
        &mut self,
        order: TypeOrder<'_>,
        target: TypeId,
        element_types: &[TypeId],
    ) -> Option<TypeId> {
        let Some(data) = self.tuple_target_of(target).cloned() else {
            unreachable!("createNormalizedTupleType requires a tuple target");
        };
        if !data.combined_flags.intersects(ElementFlags::NON_REQUIRED) {
            // No non-required elements: plain reference (61215-61217).
            return Some(self.create_type_reference(target, element_types));
        }
        if data.combined_flags.intersects(ElementFlags::VARIADIC) {
            let has_union_variadic = element_types.iter().enumerate().any(|(i, &t)| {
                data.element_flags[i].intersects(ElementFlags::VARIADIC)
                    && self.flags_of(t).intersects(TypeFlags::from_bits(
                        TypeFlags::NEVER.bits() | TypeFlags::UNION.bits(),
                    ))
            });
            if has_union_variadic {
                // The union/never variadic distribution (61218-61223)
                // runs live in the checker twin
                // (create_normalized_tuple_type_full); production code
                // cannot call this test-only twin (L-TWIN).
                unreachable!(
                    "union/never variadic distribution lives in the checker twin (L-TWIN)"
                );
            }
        }
        let mut expanded_types: Vec<TypeId> = Vec::new();
        let mut expanded_flags: Vec<ElementFlags> = Vec::new();
        let mut expanded_declarations: Vec<Option<u32>> = Vec::new();
        let outer_labels = data.labeled_element_declarations.clone();
        let outer_declaration = move |i: usize| -> Option<u32> {
            outer_labels
                .as_ref()
                .and_then(|declarations| declarations.get(i).copied())
                .flatten()
        };
        let mut last_required_index: isize = -1;
        let mut first_rest_index: isize = -1;
        let mut last_optional_or_rest_index: isize = -1;
        {
            let mut add_element = |tables: &mut Self,
                                   expanded_types: &mut Vec<TypeId>,
                                   expanded_flags: &mut Vec<ElementFlags>,
                                   expanded_declarations: &mut Vec<Option<u32>>,
                                   ty: TypeId,
                                   flags: ElementFlags,
                                   declaration: Option<u32>| {
                if flags.intersects(ElementFlags::REQUIRED) {
                    last_required_index = expanded_flags.len() as isize;
                }
                if flags.intersects(ElementFlags::REST) && first_rest_index < 0 {
                    first_rest_index = expanded_flags.len() as isize;
                }
                if flags.intersects(ElementFlags::from_bits(
                    ElementFlags::OPTIONAL.bits() | ElementFlags::REST.bits(),
                )) {
                    last_optional_or_rest_index = expanded_flags.len() as isize;
                }
                let pushed = if flags.intersects(ElementFlags::OPTIONAL) {
                    tables.add_optionality(order, ty, /*is_property*/ true, true)
                } else {
                    ty
                };
                expanded_types.push(pushed);
                expanded_flags.push(flags);
                expanded_declarations.push(declaration);
            };

            for (i, &element_type) in element_types.iter().enumerate() {
                let flags = data.element_flags[i];
                if flags.intersects(ElementFlags::VARIADIC) {
                    if self.flags_of(element_type).intersects(TypeFlags::ANY) {
                        add_element(
                            self,
                            &mut expanded_types,
                            &mut expanded_flags,
                            &mut expanded_declarations,
                            element_type,
                            ElementFlags::REST,
                            outer_declaration(i),
                        );
                    } else if self
                        .flags_of(element_type)
                        .intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
                    {
                        add_element(
                            self,
                            &mut expanded_types,
                            &mut expanded_flags,
                            &mut expanded_declarations,
                            element_type,
                            ElementFlags::VARIADIC,
                            outer_declaration(i),
                        );
                    } else if self.is_tuple_type(element_type) {
                        let inner_target = self.reference_target(element_type);
                        let Some(inner) = self.tuple_target_of(inner_target).cloned() else {
                            unreachable!("tuple type targets a tuple target");
                        };
                        // tsc forces the arguments lazily here
                        // (getTypeArguments 61240); tables cannot reach
                        // the checker, so the checker twin owns every
                        // production deferred-argument path (L-TWIN).
                        let Some(inner_args) = self
                            .try_type_arguments(element_type)
                            .map(<[TypeId]>::to_vec)
                        else {
                            unreachable!(
                                "deferred tuple arguments force through the checker twin (L-TWIN)"
                            );
                        };
                        if inner_args.len() + expanded_types.len() >= 10_000 {
                            // The 2799/2800 report lives in the checker
                            // twin (L-TWIN caller inventory).
                            unreachable!(
                                "oversized tuples report through the checker twin (L-TWIN)"
                            );
                        }
                        for (n, &inner_type) in inner_args.iter().enumerate() {
                            let inner_declaration = inner
                                .labeled_element_declarations
                                .as_ref()
                                .and_then(|declarations| declarations.get(n).copied())
                                .flatten();
                            add_element(
                                self,
                                &mut expanded_types,
                                &mut expanded_flags,
                                &mut expanded_declarations,
                                inner_type,
                                inner.element_flags[n],
                                inner_declaration,
                            );
                        }
                    } else {
                        // The array-like variadic collapse
                        // (getIndexTypeOfType, 61252) runs live in the
                        // checker twin; production code cannot call
                        // this test-only twin (L-TWIN).
                        unreachable!(
                            "array-like variadic collapse lives in the checker twin (L-TWIN)"
                        );
                    }
                } else {
                    add_element(
                        self,
                        &mut expanded_types,
                        &mut expanded_flags,
                        &mut expanded_declarations,
                        element_type,
                        flags,
                        outer_declaration(i),
                    );
                }
            }
        }
        // Optional elements before the last required one become
        // required (61258-61260).
        for flags in expanded_flags
            .iter_mut()
            .take(last_required_index.max(0) as usize)
        {
            if flags.intersects(ElementFlags::OPTIONAL) {
                *flags = ElementFlags::REQUIRED;
            }
        }
        // Collapse everything from the first rest element through the
        // last optional/rest element into a single rest union
        // (61261-61266).
        if first_rest_index >= 0 && first_rest_index < last_optional_or_rest_index {
            let first = first_rest_index as usize;
            let last = last_optional_or_rest_index as usize;
            if expanded_flags[first..=last]
                .iter()
                .any(|flags| flags.intersects(ElementFlags::VARIADIC))
            {
                // The variadic-in-rest-window collapse
                // (getIndexedAccessType, 61262) runs live in the
                // checker twin; production code cannot call this
                // test-only twin (L-TWIN).
                unreachable!("variadic-in-rest-window collapse lives in the checker twin (L-TWIN)");
            }
            let window: Vec<TypeId> = expanded_types[first..=last].to_vec();
            expanded_types[first] = self.get_union_type(order, &window, UnionReduction::Literal);
            expanded_types.drain(first + 1..=last);
            expanded_flags.drain(first + 1..=last);
            expanded_declarations.drain(first + 1..=last);
        }
        let named = expanded_declarations
            .iter()
            .any(Option::is_some)
            .then_some(expanded_declarations.as_slice());
        let flags = TupleTargetFlags::new(&expanded_flags)?;
        let tuple_target = self.get_tuple_target_type(order, flags, data.readonly, named);
        if expanded_flags.is_empty() {
            Some(tuple_target)
        } else {
            Some(self.create_type_reference(tuple_target, &expanded_types))
        }
    }

    pub fn is_tuple_type(&self, id: TypeId) -> bool {
        // tsc isTupleType (67791): Reference whose target is a Tuple.
        self.object_flags_of(id).intersects(ObjectFlags::REFERENCE)
            && self
                .object_flags_of(self.reference_target(id))
                .intersects(ObjectFlags::TUPLE)
    }

    // ---- template literal types ----

    /// tsc-port: getTemplateStringForType @6.0.3
    /// tsc-hash: 12f4d4dec01084eb847c25479a03a47c5bee30ced949361e1ab0cd181ebd0259
    /// tsc-span: _tsc.js:62110-62112
    fn get_template_string_for_type(&self, id: TypeId) -> Option<TemplateText> {
        let ty = self.type_of(id);
        if ty.flags.intersects(TypeFlags::STRING_LITERAL) {
            if let Some(LiteralValue::String(value)) = self.literal_value(id) {
                return Some(TemplateText::from_js(value));
            }
        }
        if ty.flags.intersects(TypeFlags::NUMBER_LITERAL) {
            if let Some(LiteralValue::Number(value)) = self.literal_value(id) {
                return Some(TemplateText::from(js_number_to_string(value)));
            }
        }
        if ty.flags.intersects(TypeFlags::BIG_INT_LITERAL) {
            if let Some(LiteralValue::BigInt(value)) = self.literal_value(id) {
                return Some(TemplateText::from(value.to_base10_string()));
            }
        }
        if ty.flags.intersects(TypeFlags::from_bits(
            TypeFlags::BOOLEAN_LITERAL.bits() | TypeFlags::NULLABLE.bits(),
        )) {
            if let Some(name) = self.intrinsic_name(id) {
                return Some(TemplateText::from_utf8(name));
            }
        }
        None
    }

    /// tsc-port: createTemplateLiteralType @6.0.3
    /// tsc-hash: 044da3c41e15fc13e9dbed4c062c7b6cd55d63aaef354968402604071716c263
    /// tsc-span: _tsc.js:62113-62118
    fn create_template_literal_type(
        &mut self,
        texts: Vec<TemplateText>,
        types: Vec<TypeId>,
    ) -> TypeId {
        let types = self.intern_type_list(&types);
        let payload = TemplateLiteralId::new(self.template_literals.len() as u32);
        self.template_literals.push(TemplateLiteralData {
            texts: texts.into_boxed_slice(),
            types,
        });
        self.create_type(
            TypeFlags::TEMPLATE_LITERAL,
            TypeData::TemplateLiteral(payload),
        )
    }

    /// tsgo `getTemplateLiteralType` (checker.go:29687) of spans that are
    /// no union: the checker maps a union span over its members first
    /// (`mapType`). The error type when the template is over the size
    /// limits.
    pub fn get_template_literal_type(&mut self, texts: &[String], types: &[TypeId]) -> TypeId {
        let texts = texts
            .iter()
            .map(|text| TemplateText::from_utf8(text))
            .collect::<Vec<_>>();
        self.get_template_literal_type_from_texts(&texts, types)
    }

    /// Lossless UTF-16 entry used by parsed template fragments and by
    /// transformations of existing template literal types.
    pub fn get_template_literal_type_from_texts(
        &mut self,
        texts: &[TemplateText],
        types: &[TypeId],
    ) -> TypeId {
        self.try_get_template_literal_type_from_texts(texts, types)
            .unwrap_or(self.intrinsics.error)
    }

    /// [`Self::get_template_literal_type_from_texts`] that tells the
    /// checker when the type is over tsgo's size limits.
    pub fn try_get_template_literal_type_from_texts(
        &mut self,
        texts: &[TemplateText],
        types: &[TypeId],
    ) -> Result<TypeId, TemplateLiteralTooLarge> {
        debug_assert_eq!(texts.len(), types.len() + 1);
        // The checker maps a union span over its members first.
        debug_assert!(!types.iter().any(|&t| {
            self.flags_of(t)
                .intersects(TypeFlags::NEVER | TypeFlags::UNION)
        }));
        if types.contains(&self.intrinsics.wildcard) {
            return Ok(self.intrinsics.wildcard);
        }
        let mut spans = TemplateSpans {
            new_types: Vec::new(),
            new_texts: Vec::new(),
            text: texts[0].clone(),
            text_bytes: utf8_length(&texts[0]),
            moved_bytes: 0,
        };
        if !self.add_spans(&mut spans, texts, types)? {
            return Ok(self.intrinsics.string);
        }
        let TemplateSpans {
            new_types,
            mut new_texts,
            text,
            ..
        } = spans;
        if new_types.is_empty() {
            return Ok(self.get_string_literal_type_from_text(&text));
        }
        new_texts.push(text);
        if new_texts.iter().all(|t| t.is_empty()) {
            if new_types
                .iter()
                .all(|&t| self.flags_of(t).intersects(TypeFlags::STRING))
            {
                return Ok(self.intrinsics.string);
            }
            if new_types.len() == 1 && self.is_pattern_literal_type(new_types[0]) {
                return Ok(new_types[0]);
            }
        }
        // Hash the same ordered type list and length-delimited UTF-16
        // segments as the former formatted key. Entries refer to the
        // immutable payload: lookups need neither key strings nor copies.
        let hash = hash_template_literal_key(&new_types, &new_texts);
        if let Some(&(_, id)) = self.template_literal_types.find(hash, |&(payload, _)| {
            let data = &self.template_literals[payload.index() as usize];
            self.list(data.types) == new_types && *data.texts == new_texts
        }) {
            return Ok(id);
        }
        let id = self.create_template_literal_type(new_texts, new_types);
        let TypeData::TemplateLiteral(payload) = self.type_of(id).data else {
            unreachable!("created a template literal above");
        };
        let payloads = &self.template_literals;
        let items = &self.list_items;
        self.template_literal_types
            .insert_unique(hash, (payload, id), |&(payload, _)| {
                let data = &payloads[payload.index() as usize];
                hash_template_literal_key(list_at(items, data.types), &data.texts)
            });
        Ok(id)
    }

    /// addSpans inner function of getTemplateLiteralType (62089-62108),
    /// with tsgo's size limits (checker.go:29637-29667): Ok(false) falls
    /// back to `string`.
    fn add_spans(
        &mut self,
        spans: &mut TemplateSpans,
        texts: &[TemplateText],
        types: &[TypeId],
    ) -> Result<bool, TemplateLiteralTooLarge> {
        for (i, &t) in types.iter().enumerate() {
            let flags = self.flags_of(t);
            if flags.intersects(TypeFlags::from_bits(
                TypeFlags::LITERAL.bits() | TypeFlags::NULL.bits() | TypeFlags::UNDEFINED.bits(),
            )) {
                match self.get_template_string_for_type(t) {
                    Some(segment) => {
                        spans.push_text(&segment);
                        spans.push_text(&texts[i + 1]);
                    }
                    None => return Ok(false),
                }
            } else if flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
                let TypeData::TemplateLiteral(inner) = self.type_of(t).data else {
                    unreachable!("template flag implies template data");
                };
                let inner = self.template_literal(inner);
                let inner_texts: Vec<TemplateText> = inner.texts.to_vec();
                let inner_types: Vec<TypeId> = self.list(inner.types).to_vec();
                spans.push_text(&inner_texts[0]);
                if !self.add_spans(spans, &inner_texts, &inner_types)? {
                    return Ok(false);
                }
                spans.push_text(&texts[i + 1]);
            } else if self.is_generic_index_type(t) || self.is_pattern_literal_placeholder_type(t) {
                spans.new_types.push(t);
                let text = std::mem::replace(&mut spans.text, texts[i + 1].clone());
                spans.new_texts.push(text);
                spans.moved_bytes += spans.text_bytes;
                spans.text_bytes = utf8_length(&texts[i + 1]);
            } else {
                return Ok(false);
            }
            if spans.moved_bytes + spans.text_bytes > MAX_TEMPLATE_LITERAL_TYPE_LENGTH
                || spans.new_types.len() > MAX_TEMPLATE_LITERAL_TYPE_SPANS
            {
                return Err(TemplateLiteralTooLarge);
            }
        }
        Ok(true)
    }

    /// tsc-port: isGenericType @6.0.3
    /// tsc-hash: 24d5fea9e8bd61b2e658f4c5d9423ee7926ff364e97370617a604cd6e4b0f5c0
    /// tsc-span: _tsc.js:62431-62433
    pub fn is_generic_type(&mut self, id: TypeId) -> bool {
        !self.get_generic_object_flags(id).is_empty()
    }

    /// tsc-port: isGenericObjectType @6.0.3
    /// tsc-hash: 4d244e3f861e1e795557d5b8d2b382f4b38e29d03abebf3ca45c053d7d362d78
    /// tsc-span: _tsc.js:62434-62436
    pub fn is_generic_object_type(&mut self, id: TypeId) -> bool {
        self.get_generic_object_flags(id)
            .intersects(ObjectFlags::IS_GENERIC_OBJECT_TYPE)
    }

    /// tsc-port: isGenericIndexType @6.0.3
    /// tsc-hash: 5c0f99fb189bd26062582025df8d17cef96a72ac19f530aa2753ed2c40e1bf7e
    /// tsc-span: _tsc.js:62437-62439
    pub fn is_generic_index_type(&mut self, id: TypeId) -> bool {
        self.get_generic_object_flags(id)
            .intersects(ObjectFlags::IS_GENERIC_INDEX_TYPE)
    }

    /// tsc-port: getGenericObjectFlags @6.0.3
    /// tsc-hash: f3a4b640057f3aa5519de7fd4c29d117ae33ec29fb1c7618f8e4456782af7b02
    /// tsc-span: _tsc.js:62440-62455
    ///
    /// Mapped genericity depends on checker-owned constraint and
    /// name-type resolution, so mapped callers route through
    /// CheckerState::get_generic_object_flags instead of this
    /// syntax-free fallback.
    fn get_generic_object_flags(&mut self, id: TypeId) -> ObjectFlags {
        let flags = self.flags_of(id);
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            if !self
                .object_flags_of(id)
                .intersects(ObjectFlags::IS_GENERIC_TYPE_COMPUTED)
            {
                let (TypeData::Union { types, .. } | TypeData::Intersection { types }) =
                    self.type_of(id).data
                else {
                    unreachable!("union/intersection flag implies member data");
                };
                let mut combined = 0i32;
                for index in 0..self.list(types).len() {
                    let member = self.list(types)[index];
                    combined |= self.get_generic_object_flags(member).bits();
                }
                let updated = self.object_flags_of(id).bits()
                    | ObjectFlags::IS_GENERIC_TYPE_COMPUTED.bits()
                    | combined;
                self.type_mut(id).object_flags = ObjectFlags::from_bits(updated);
            }
            return ObjectFlags::from_bits(
                self.object_flags_of(id).bits() & ObjectFlags::IS_GENERIC_TYPE.bits(),
            );
        }
        if flags.intersects(TypeFlags::SUBSTITUTION) {
            let TypeData::Substitution(data) = self.type_of(id).data else {
                unreachable!("Substitution flag implies substitution data");
            };
            let combined = self.get_generic_object_flags(data.base_type)
                | self.get_generic_object_flags(data.constraint);
            return combined & ObjectFlags::IS_GENERIC_TYPE;
        }
        let object = if flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
            || self.is_generic_tuple_type(id)
        {
            ObjectFlags::IS_GENERIC_OBJECT_TYPE.bits()
        } else {
            0
        };
        let index = if flags.intersects(TypeFlags::from_bits(
            TypeFlags::INSTANTIABLE_NON_PRIMITIVE.bits() | TypeFlags::INDEX.bits(),
        )) || self.is_generic_string_like_type(id)
        {
            ObjectFlags::IS_GENERIC_INDEX_TYPE.bits()
        } else {
            0
        };
        ObjectFlags::from_bits(object | index)
    }

    /// tsc-port: isGenericTupleType @6.0.3
    /// tsc-hash: f741808e027436939d394e907796a4f2e0d4af5699ad71f6efa223032506de8b
    /// tsc-span: _tsc.js:67794-67796
    pub fn is_generic_tuple_type(&self, id: TypeId) -> bool {
        if !self.is_tuple_type(id) {
            return false;
        }
        let target = self.reference_target(id);
        self.tuple_target_of(target)
            .is_some_and(|data| data.combined_flags.intersects(ElementFlags::VARIADIC))
    }

    /// tsc-port: isGenericStringLikeType @6.0.3
    /// tsc-hash: bab14bed10f5a2ddbf5b7c73efdb4a5897ad9d2e48b2d79734b4deaa3f525617
    /// tsc-span: _tsc.js:62428-62430
    pub fn is_generic_string_like_type(&self, id: TypeId) -> bool {
        self.flags_of(id).intersects(TypeFlags::from_bits(
            TypeFlags::TEMPLATE_LITERAL.bits() | TypeFlags::STRING_MAPPING.bits(),
        )) && !self.is_pattern_literal_type(id)
    }

    /// The indexedAccessTypes interning slice of
    /// getIndexedAccessTypeOrUndefined (62580-62585) +
    /// createIndexedAccessType (62168-62175).
    pub fn get_indexed_access_type_interned(
        &mut self,
        object_type: TypeId,
        index_type: TypeId,
        persistent_access_flags: AccessFlags,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> TypeId {
        let key = IndexedAccessKey {
            object: object_type,
            index: index_type,
            access_flags: persistent_access_flags.bits(),
            alias: self.alias_key(alias_symbol, alias_type_arguments),
        };
        if let Some(&id) = self.indexed_access_types.get(&key) {
            return id;
        }
        let id = self.create_type(
            TypeFlags::INDEXED_ACCESS,
            TypeData::IndexedAccess {
                object_type,
                index_type,
                access_flags: persistent_access_flags,
            },
        );
        let alias_type_arguments = self.intern_alias_arguments(alias_type_arguments);
        self.set_alias(id, alias_symbol, alias_type_arguments);
        self.indexed_access_types.insert(key, id);
        id
    }

    /// tsc-port: getStringMappingTypeForGenericType @6.0.3
    /// tsc-hash: 13f62a7fd92dfaef3f9aa4f498ae4fbe312414346a27b6a1d13bbedbfeb2830f
    /// tsc-span: _tsc.js:62154-62161
    ///
    /// tsc-port: createStringMappingType @6.0.3
    /// tsc-hash: 6d59ad0c176bb2fa63221db0e827ffe78a7d62ffc3924cf6ef7b159e95bcd129
    /// tsc-span: _tsc.js:62162-62167
    pub fn get_string_mapping_type_for_generic_type(
        &mut self,
        symbol: SymbolId,
        ty: TypeId,
    ) -> TypeId {
        if let Some(&id) = self.string_mapping_types.get(&(symbol, ty)) {
            return id;
        }
        let id = self.create_type(TypeFlags::STRING_MAPPING, TypeData::StringMapping { ty });
        self.type_mut(id).symbol = Some(symbol);
        self.string_mapping_types.insert((symbol, ty), id);
        id
    }

    /// tsc-port: isPatternLiteralPlaceholderType @6.0.3
    /// tsc-hash: a3ded0eca102eb77868c3ffd27dfbc429557ace0a417a21bd73defb876d950b1
    /// tsc-span: _tsc.js:62411-62424
    pub fn is_pattern_literal_placeholder_type(&self, id: TypeId) -> bool {
        let flags = self.flags_of(id);
        if flags.intersects(TypeFlags::INTERSECTION) {
            let mut seen_placeholder = false;
            for &t in self.members_of(id) {
                let t_flags = self.flags_of(t);
                if t_flags.intersects(TypeFlags::from_bits(
                    TypeFlags::LITERAL.bits() | TypeFlags::NULLABLE.bits(),
                )) || self.is_pattern_literal_placeholder_type(t)
                {
                    seen_placeholder = true;
                } else if !t_flags.intersects(TypeFlags::OBJECT) {
                    return false;
                }
            }
            return seen_placeholder;
        }
        flags.intersects(TypeFlags::from_bits(
            TypeFlags::ANY.bits()
                | TypeFlags::STRING.bits()
                | TypeFlags::NUMBER.bits()
                | TypeFlags::BIG_INT.bits(),
        )) || self.is_pattern_literal_type(id)
    }

    /// tsc-port: isPatternLiteralType @6.0.3
    /// tsc-hash: e0410a951bc5d53936ee7030a6f63b1937313b20ed7ca58f6d9802d990bdcf75
    /// tsc-span: _tsc.js:62425-62427
    pub fn is_pattern_literal_type(&self, id: TypeId) -> bool {
        let flags = self.flags_of(id);
        if flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            let TypeData::TemplateLiteral(data) = self.type_of(id).data else {
                unreachable!("template flag implies template data");
            };
            return self
                .list(self.template_literal(data).types)
                .iter()
                .all(|&t| self.is_pattern_literal_placeholder_type(t));
        }
        if flags.intersects(TypeFlags::STRING_MAPPING) {
            let TypeData::StringMapping { ty } = self.type_of(id).data else {
                unreachable!("string-mapping flag implies string-mapping data");
            };
            return self.is_pattern_literal_placeholder_type(ty);
        }
        false
    }
}

/// tsc-port: containsType @6.0.3
/// tsc-hash: eb85169b6f340700fb536db728d227aa1b9585f36371df6db5f7fd8270934d9d
/// tsc-span: _tsc.js:61327-61329
///
/// stableTypeOrdering off: binary search keyed by type id over an
/// id-sorted list.
pub fn contains_type(types: &[TypeId], ty: TypeId) -> bool {
    types.binary_search(&ty).is_ok()
}

/// tsc-port: insertType @6.0.3
/// tsc-hash: d8d7b81478222333611e89cdb9821b7f8219f29a999e05ff0a6ddb912f9cbb9b
/// tsc-span: _tsc.js:61330-61337
pub fn insert_type(types: &mut Vec<TypeId>, ty: TypeId) -> bool {
    if let Err(index) = types.binary_search(&ty) {
        types.insert(index, ty);
        return true;
    }
    false
}

/// JS Map keys use SameValueZero: -0 and +0 share an entry. Literal
/// values from source text are never NaN.
fn number_map_key(value: f64) -> u64 {
    if value == 0.0 {
        0f64.to_bits()
    } else {
        value.to_bits()
    }
}

/// getEnumLiteralType's cache key mixes the value into a STRING
/// (`${enumId}#${value}`), so `${-0}` = `${0}` = "0" and every NaN
/// prints "NaN" — evaluator results CAN be -0 or NaN (`A = 0 * -1`,
/// `A = 0 / 0` in non-const enums).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum EnumLiteralKey {
    String(crate::JsString),
    Number(u64),
}

fn enum_literal_number_key(value: f64) -> u64 {
    if value.is_nan() {
        f64::NAN.to_bits()
    } else {
        number_map_key(value)
    }
}

/// JS `String(number)` — ECMAScript Number::toString(10). Rust's
/// float formatting supplies the shortest round-trip digits; the
/// decimal-vs-exponent layout rules (decimal for -6 < n <= 21,
/// exponent with explicit sign otherwise, -0 prints "0") are the
/// spec's, so folded template texts match tsc's `"" + type.value`
/// exactly (getTemplateStringForType, _tsc.js:62110).
pub fn js_number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value == f64::INFINITY {
        return "Infinity".to_string();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    // tsgo jsnum/string.go:29-34: safe integers need only decimal
    // conversion. This also avoids LowerExp, exponent parsing and the
    // temporary mantissa/digit strings for numeric property names.
    if value.abs() <= 9_007_199_254_740_991.0 {
        let integer = value as i64;
        if integer as f64 == value {
            return integer.to_string();
        }
    }
    if value < 0.0 {
        return format!("-{}", js_number_to_string(-value));
    }
    // Shortest round-trip digits: "d[.ddd]e<exp>" from LowerExp.
    let exp_form = format!("{value:e}");
    let (mantissa, exp) = exp_form.split_once('e').expect("LowerExp always emits 'e'");
    let exp: i32 = exp.parse().expect("LowerExp exponent is an integer");
    let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    // value = 0.<digits> * 10^n with k significant digits.
    let k = digits.len() as i32;
    let n = exp + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e >= 0 { "+" } else { "-" };
        if k == 1 {
            format!("{digits}e{sign}{}", e.abs())
        } else {
            format!("{}.{}e{sign}{}", &digits[..1], &digits[1..], e.abs())
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/tables/tests.rs"]
mod tests;

/// tsgo's limits on a template literal type built by getTemplateLiteralType
/// (checker.go:29610-29616): its text, in UTF-8 bytes as Go counts them,
/// and its placeholders. Recursive instantiations that double the text or
/// the placeholders every step reach them long before the tail-recursion
/// limit (TypeScript issue 63271).
pub const MAX_TEMPLATE_LITERAL_TYPE_LENGTH: usize = 50_000_000;
pub const MAX_TEMPLATE_LITERAL_TYPE_SPANS: usize = 100_000;

/// A template literal type over the limits; the checker reports TS2589.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemplateLiteralTooLarge;

/// The spans getTemplateLiteralType collects, with the UTF-8 lengths of
/// the finished segments and of the one being written.
struct TemplateSpans {
    new_types: Vec<TypeId>,
    new_texts: Vec<TemplateText>,
    text: TemplateText,
    text_bytes: usize,
    moved_bytes: usize,
}

impl TemplateSpans {
    fn push_text(&mut self, segment: &TemplateText) {
        self.text.push_text(segment);
        self.text_bytes += utf8_length(segment);
    }
}

/// The UTF-8 length of a JavaScript string; a lone surrogate counts three
/// bytes, as Go writes it.
fn utf8_length(text: &TemplateText) -> usize {
    let units = text.units();
    let mut length = 0;
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        length += match unit {
            0..=0x7F => 1,
            0x80..=0x7FF => 2,
            0xD800..=0xDBFF
                if units
                    .get(index + 1)
                    .is_some_and(|next| (0xDC00..=0xDFFF).contains(next)) =>
            {
                index += 1;
                4
            }
            _ => 3,
        };
        index += 1;
    }
    length
}
