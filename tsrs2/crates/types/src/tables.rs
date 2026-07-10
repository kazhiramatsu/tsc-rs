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

use std::collections::HashMap;

use crate::flags::{ElementFlags, ObjectFlags, TypeFlags};
use crate::ty::{LiteralValue, PseudoBigInt, TupleTargetData, Type, TypeData, TypeId};

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
    pub unique_literal: TypeId,
}

/// The M4-dependency escape hatch: a constructor arm whose inputs are
/// unconstructible before instantiation lands returns this instead of
/// inventing a type; the relpin probe surfaces it as Unsupported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct M4Dependency(pub &'static str);

pub struct TypeTables {
    types: Vec<Type>,
    pub strict_null_checks: bool,
    pub exact_optional_property_types: bool,
    pub intrinsics: Intrinsics,
    // ---- tsc interning maps (_tsc.js 46988-47009); M3 subset ----
    /// stringLiteralTypes (46992), keyed by the string value.
    string_literal_types: HashMap<String, TypeId>,
    /// numberLiteralTypes (46993), keyed by the numeric value with JS
    /// Map SameValueZero semantics (-0 and +0 share a key).
    number_literal_types: HashMap<u64, TypeId>,
    /// bigIntLiteralTypes (46994), keyed by pseudoBigIntToString.
    bigint_literal_types: HashMap<String, TypeId>,
    /// unionTypes (46989), keyed by getTypeListId (+ alias id, M4).
    union_types: HashMap<String, TypeId>,
    /// intersectionTypes (46991).
    intersection_types: HashMap<String, TypeId>,
    /// tupleTypes (46988), keyed per getTupleTargetType 61149.
    tuple_types: HashMap<String, TypeId>,
    /// templateLiteralTypes (46997), keyed per getTemplateLiteralType 62083.
    template_literal_types: HashMap<String, TypeId>,
    /// Per-target `type.instantiations` maps (createTypeReference
    /// 60170-60174), flattened to one table keyed (target, list id).
    instantiations: HashMap<(TypeId, String), TypeId>,
}

impl TypeTables {
    pub fn new(strict_null_checks: bool, exact_optional_property_types: bool) -> Self {
        let mut tables = Self {
            types: Vec::new(),
            strict_null_checks,
            exact_optional_property_types,
            intrinsics: Intrinsics {
                any: TypeId(0),
                auto: TypeId(0),
                wildcard: TypeId(0),
                blocked_string: TypeId(0),
                error: TypeId(0),
                unresolved: TypeId(0),
                non_inferrable_any: TypeId(0),
                intrinsic_marker: TypeId(0),
                unknown: TypeId(0),
                undefined: TypeId(0),
                undefined_widening: TypeId(0),
                missing: TypeId(0),
                undefined_or_missing: TypeId(0),
                optional: TypeId(0),
                null: TypeId(0),
                null_widening: TypeId(0),
                string: TypeId(0),
                number: TypeId(0),
                bigint: TypeId(0),
                false_fresh: TypeId(0),
                false_regular: TypeId(0),
                true_fresh: TypeId(0),
                true_regular: TypeId(0),
                boolean: TypeId(0),
                es_symbol: TypeId(0),
                void: TypeId(0),
                never: TypeId(0),
                silent_never: TypeId(0),
                implicit_never: TypeId(0),
                unreachable_never: TypeId(0),
                non_primitive: TypeId(0),
                string_or_number: TypeId(0),
                string_number_symbol: TypeId(0),
                number_or_bigint: TypeId(0),
                unique_literal: TypeId(0),
            },
            string_literal_types: HashMap::new(),
            number_literal_types: HashMap::new(),
            bigint_literal_types: HashMap::new(),
            union_types: HashMap::new(),
            intersection_types: HashMap::new(),
            tuple_types: HashMap::new(),
            template_literal_types: HashMap::new(),
            instantiations: HashMap::new(),
        };
        tables.create_initial_types();
        tables
    }

    // ---- arena ----

    /// tsc-port: createType @6.0.3
    /// tsc-hash: 8c00f116e39f7085c9e81dd1e3e17f04ab51ff0630b2c75228a12d38c03ef0e6
    /// tsc-span: _tsc.js:50095-50102
    pub fn create_type(&mut self, flags: TypeFlags, data: TypeData) -> TypeId {
        let id = TypeId(self.types.len() as u32);
        self.types.push(Type::new(flags, data));
        id
    }

    pub fn type_of(&self, id: TypeId) -> &Type {
        &self.types[id.0 as usize]
    }

    pub fn type_mut(&mut self, id: TypeId) -> &mut Type {
        &mut self.types[id.0 as usize]
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
        let id = self.create_type(flags, TypeData::Intrinsic { name, debug_name });
        // objectFlags | CouldContainTypeVariablesComputed |
        // IsGenericTypeComputed | IsUnknownLikeUnionComputed |
        // IsNeverIntersectionComputed (50116).
        self.type_mut(id).object_flags =
            ObjectFlags::from_bits(object_flags.bits() | 524288 | 2097152 | 33554432 | 16777216);
        id
    }

    /// The initial type block, in tsc's allocation order (47011-47111)
    /// so ids line up run-for-run. Only the intrinsics M3 consumes are
    /// materialized; the block is complete regardless because later
    /// stages read ids positionally never numerically.
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
            self.type_mut(fresh).regular_type = Some(regular);
            self.type_mut(fresh).fresh_type = Some(fresh);
            self.type_mut(regular).regular_type = Some(regular);
            self.type_mut(regular).fresh_type = Some(fresh);
        }
        let boolean = self.get_union_type_interim(&[false_regular, true_regular]);

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
        let string_or_number = self.get_union_type_interim(&[string, number]);
        let string_number_symbol = self.get_union_type_interim(&[string, number, es_symbol]);
        let number_or_bigint = self.get_union_type_interim(&[number, bigint]);
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
        value: LiteralValue,
        regular_type: Option<TypeId>,
    ) -> TypeId {
        let id = self.create_type(flags, TypeData::Literal { value });
        self.type_mut(id).regular_type = Some(regular_type.unwrap_or(id));
        id
    }

    /// tsc-port: getFreshTypeOfLiteralType @6.0.3
    /// tsc-hash: 2879316237152de8adecc53c58c19f6255d644fb4c3288a787622eaff845e1f4
    /// tsc-span: _tsc.js:63066-63076
    pub fn get_fresh_type_of_literal_type(&mut self, id: TypeId) -> TypeId {
        if self.flags_of(id).intersects(TypeFlags::FRESHABLE) {
            if self.type_of(id).fresh_type.is_none() {
                let TypeData::Literal { value } = self.type_of(id).data.clone() else {
                    // Boolean literal intrinsics are wired eagerly and
                    // never reach this branch.
                    return self.type_of(id).fresh_type.unwrap_or(id);
                };
                let flags = self.flags_of(id);
                let fresh = self.create_literal_type(flags, value, Some(id));
                self.type_mut(fresh).fresh_type = Some(fresh);
                self.type_mut(id).fresh_type = Some(fresh);
            }
            return self.type_of(id).fresh_type.expect("fresh link just wired");
        }
        id
    }

    /// tsc-port: getRegularTypeOfLiteralType @6.0.3
    /// tsc-hash: 6fadff11dcdf0b9ab35cdbe7d059e1fb44a9b8752fe2bb9a5041ce54797ba276
    /// tsc-span: _tsc.js:63077-63079
    pub fn get_regular_type_of_literal_type(&mut self, id: TypeId) -> TypeId {
        let flags = self.flags_of(id);
        if flags.intersects(TypeFlags::FRESHABLE) {
            return self
                .type_of(id)
                .regular_type
                .expect("freshable regular link");
        }
        if flags.intersects(TypeFlags::UNION) {
            if let Some(regular) = self.type_of(id).regular_type {
                return regular;
            }
            let TypeData::Union { types, .. } = self.type_of(id).data.clone() else {
                unreachable!("union flag implies union data");
            };
            let mapped: Vec<TypeId> = types
                .iter()
                .map(|&member| self.get_regular_type_of_literal_type(member))
                .collect();
            let regular = self.get_union_type_interim(&mapped);
            self.type_mut(id).regular_type = Some(regular);
            return regular;
        }
        id
    }

    /// tsc-port: isFreshLiteralType @6.0.3
    /// tsc-hash: 7de9df61f3b6f272112277753c6b8cee2045e87ab509a4695367fcfcd7d6eba2
    /// tsc-span: _tsc.js:63080-63082
    pub fn is_fresh_literal_type(&self, id: TypeId) -> bool {
        self.flags_of(id).intersects(TypeFlags::FRESHABLE)
            && self.type_of(id).fresh_type == Some(id)
    }

    /// tsc-port: getStringLiteralType @6.0.3
    /// tsc-hash: e7516536a59ae1f2232f916cde9c1fd2fe6c1ab99c9469c9ce15761178d84a0d
    /// tsc-span: _tsc.js:63083-63086
    pub fn get_string_literal_type(&mut self, value: &str) -> TypeId {
        if let Some(&id) = self.string_literal_types.get(value) {
            return id;
        }
        let id = self.create_literal_type(
            TypeFlags::STRING_LITERAL,
            LiteralValue::String(value.to_owned()),
            None,
        );
        self.string_literal_types.insert(value.to_owned(), id);
        id
    }

    /// tsc-port: getNumberLiteralType @6.0.3
    /// tsc-hash: 429f1ea7085e0b59ba3e7bac892d644cab76b2c1954a46d0489ff38cd197f5dd
    /// tsc-span: _tsc.js:63087-63090
    pub fn get_number_literal_type(&mut self, value: f64) -> TypeId {
        let key = number_map_key(value);
        if let Some(&id) = self.number_literal_types.get(&key) {
            return id;
        }
        let id =
            self.create_literal_type(TypeFlags::NUMBER_LITERAL, LiteralValue::Number(value), None);
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
        let id = self.create_literal_type(
            TypeFlags::BIG_INT_LITERAL,
            LiteralValue::BigInt(value),
            None,
        );
        self.bigint_literal_types.insert(key, id);
        id
    }

    // ---- list ids & propagating flags ----

    /// tsc-port: getTypeListId @6.0.3
    /// tsc-hash: 08bbe30d7ae7370051e576d48d6bf3103d563a65a92d10740ec9f3c4546f9fea
    /// tsc-span: _tsc.js:60128-60150
    pub fn get_type_list_id(&self, types: &[TypeId]) -> String {
        let mut result = String::new();
        let length = types.len();
        let mut i = 0;
        while i < length {
            let start_id = types[i].0;
            let mut count = 1usize;
            while i + count < length && types[i + count].0 == start_id + count as u32 {
                count += 1;
            }
            if !result.is_empty() {
                result.push(',');
            }
            result.push_str(&start_id.to_string());
            if count > 1 {
                result.push(':');
                result.push_str(&count.to_string());
            }
            i += count;
        }
        result
    }

    /// tsc-port: getPropagatingFlagsOfTypes @6.0.3
    /// tsc-hash: 57ecf71d451f3f1480dbe807fb4e5d6c52f01b6d265bc22484e5b186307ff685
    /// tsc-span: _tsc.js:60154-60162
    fn get_propagating_flags_of_types(
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

    // ---- unions ----

    /// INTERIM (stage 4.1): flatten + dedup-by-id + sort-by-id feeding
    /// the real getUnionTypeFromSortedList port below. The full
    /// getUnionType/getUnionTypeWorker (61505/61532: UnionReduction
    /// modes, literal reduction, missingType/widening-variant folding)
    /// is stage 4.2 — every 4.1 caller passes already-reduced inputs.
    pub fn get_union_type_interim(&mut self, types: &[TypeId]) -> TypeId {
        let mut set: Vec<TypeId> = Vec::with_capacity(types.len());
        self.add_types_to_union_set_interim(&mut set, types);
        self.get_union_type_from_sorted_list(set, ObjectFlags::from_bits(0))
    }

    fn add_types_to_union_set_interim(&mut self, set: &mut Vec<TypeId>, types: &[TypeId]) {
        for &member in types {
            if self.flags_of(member).intersects(TypeFlags::UNION) {
                let TypeData::Union { types: inner, .. } = self.type_of(member).data.clone() else {
                    unreachable!("union flag implies union data");
                };
                self.add_types_to_union_set_interim(set, &inner);
            } else if let Err(index) = set.binary_search(&member) {
                set.insert(index, member);
            }
        }
    }

    /// tsc-port: getUnionTypeFromSortedList @6.0.3
    /// tsc-hash: 1e98366b81464c049f6729b888516648a16e01e90f6b6e8264bf775700f93e6b
    /// tsc-span: _tsc.js:61613-61641
    ///
    /// Alias/origin keying (getAliasId, `|`/`&`/`#` origin key forms)
    /// arrives with M4 aliases and stage 4.8 subtype reduction; the M3
    /// key is the plain getTypeListId form. The 61634-61637 boolean
    /// special case sets flags |= Boolean (the union's "boolean"
    /// intrinsicName is display-only and lands with T2 display work).
    pub fn get_union_type_from_sorted_list(
        &mut self,
        types: Vec<TypeId>,
        precomputed_object_flags: ObjectFlags,
    ) -> TypeId {
        if types.is_empty() {
            return self.intrinsics.never;
        }
        if types.len() == 1 {
            return types[0];
        }
        let key = self.get_type_list_id(&types);
        if let Some(&id) = self.union_types.get(&key) {
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
                types: types.into_boxed_slice(),
                origin: None,
            },
        );
        self.type_mut(id).object_flags = object_flags;
        self.union_types.insert(key, id);
        id
    }

    // ---- optionality (undefined widening into optional slots) ----

    /// tsc-port: getOptionalType @6.0.3
    /// tsc-hash: bb5a73a698a53842f916c77432005c59701edb3812c2a8886b2ff40155bcdc4b
    /// tsc-span: _tsc.js:67852-67856
    pub fn get_optional_type(&mut self, id: TypeId, is_property: bool) -> TypeId {
        debug_assert!(self.strict_null_checks);
        let missing_or_undefined = if is_property {
            self.intrinsics.undefined_or_missing
        } else {
            self.intrinsics.undefined
        };
        if id == missing_or_undefined {
            return id;
        }
        if self.flags_of(id).intersects(TypeFlags::UNION) {
            let TypeData::Union { types, .. } = &self.type_of(id).data else {
                unreachable!("union flag implies union data");
            };
            if types.first() == Some(&missing_or_undefined) {
                return id;
            }
        }
        self.get_union_type_interim(&[id, missing_or_undefined])
    }

    /// tsc-port: addOptionality @6.0.3
    /// tsc-hash: 2802085ebd92adbd4005f16c9656c306440c1d540776e43f5c0153c5bc3af21b
    /// tsc-span: _tsc.js:56029-56031
    pub fn add_optionality(&mut self, id: TypeId, is_property: bool, is_optional: bool) -> TypeId {
        if self.strict_null_checks && is_optional {
            self.get_optional_type(id, is_property)
        } else {
            id
        }
    }

    // ---- interim intersections (full getIntersectionType is 4.3) ----

    /// INTERIM (stage 4.1): identity interning by member list only —
    /// the eight-step getIntersectionType normalization (61789:
    /// flatten/never/any absorption/DisjointDomains/supertype
    /// reduction/union distribution) is stage 4.3. Until then the
    /// annotation arm constructs the raw member list.
    pub fn get_intersection_type_interim(&mut self, types: &[TypeId]) -> TypeId {
        if types.len() == 1 {
            return types[0];
        }
        let key = self.get_type_list_id(types);
        if let Some(&id) = self.intersection_types.get(&key) {
            return id;
        }
        let id = self.create_type(
            TypeFlags::INTERSECTION,
            TypeData::Intersection {
                types: types.to_vec().into_boxed_slice(),
            },
        );
        let object_flags = self.get_propagating_flags_of_types(types, TypeFlags::NULLABLE);
        self.type_mut(id).object_flags = object_flags;
        self.intersection_types.insert(key, id);
        id
    }

    // ---- references ----

    /// tsc-port: createTypeReference @6.0.3
    /// tsc-hash: 17f8bfecf79e7fa7858909317b8081cfc45fe59c0e11ba4cae5ba8b38abfeaff
    /// tsc-span: _tsc.js:60169-60180
    pub fn create_type_reference(&mut self, target: TypeId, type_arguments: &[TypeId]) -> TypeId {
        let key = (target, self.get_type_list_id(type_arguments));
        if let Some(&id) = self.instantiations.get(&key) {
            return id;
        }
        let symbol = self.type_of(target).symbol;
        let id = self.create_type(
            TypeFlags::OBJECT,
            TypeData::Reference {
                target,
                resolved_type_arguments: type_arguments.to_vec().into_boxed_slice(),
            },
        );
        let object_flags = ObjectFlags::from_bits(
            ObjectFlags::REFERENCE.bits()
                | self
                    .get_propagating_flags_of_types(type_arguments, TypeFlags::from_bits(0))
                    .bits(),
        );
        self.type_mut(id).object_flags = object_flags;
        self.type_mut(id).symbol = symbol;
        self.instantiations.insert(key, id);
        id
    }

    /// The reference target: a plain Reference's target, or the type
    /// itself for tuple targets (tsc `type.target = type`, 61192).
    pub fn reference_target(&self, id: TypeId) -> TypeId {
        match &self.type_of(id).data {
            TypeData::Reference { target, .. } => *target,
            TypeData::TupleTarget(_) => id,
            _ => id,
        }
    }

    /// getTypeArguments for instantiation-free references (60202-60222:
    /// plain references have resolvedTypeArguments eagerly; the lazy
    /// node-reading branch belongs to M4 deferred references). A tuple
    /// target aliases its own typeParameters (61193).
    pub fn type_arguments(&self, id: TypeId) -> &[TypeId] {
        match &self.type_of(id).data {
            TypeData::Reference {
                resolved_type_arguments,
                ..
            } => resolved_type_arguments,
            TypeData::TupleTarget(data) => &data.type_parameters,
            _ => &[],
        }
    }

    // ---- tuples ----

    /// tsc-port: getTupleTargetType @6.0.3
    /// tsc-hash: 180d4131d4865f2a4bb2c6da0d56e1fa6dfd835ca8f37214d9cdc6e3f10f7359
    /// tsc-span: _tsc.js:61145-61155
    ///
    /// The labeled-member key segment (getNodeId-keyed, 61149) is
    /// deferred with labeled tuple elements (ty.rs TupleTargetData).
    pub fn get_tuple_target_type(
        &mut self,
        element_flags: &[ElementFlags],
        readonly: bool,
    ) -> Result<TypeId, M4Dependency> {
        if element_flags.len() == 1 && element_flags[0].intersects(ElementFlags::REST) {
            // `[...T[]]` collapses to (readonly) Array<T> (61146-61148).
            return Err(M4Dependency(
                "single-rest tuple collapses to globalArrayType (M4 5.3)",
            ));
        }
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
        if let Some(&id) = self.tuple_types.get(&key) {
            return Ok(id);
        }
        let id = self.create_tuple_target_type(element_flags, readonly);
        self.tuple_types.insert(key, id);
        Ok(id)
    }

    /// tsc-port: createTupleTargetType @6.0.3
    /// tsc-hash: 174a20487ba5d9cbae89e49e7ce9bb1b4e37e4de1cc60e38ece5bfd3e9be0490
    /// tsc-span: _tsc.js:61156-61209
    ///
    /// Ported WITHOUT the per-index/`length` property symbol synthesis
    /// (61160-61185): tuple RELATIONS read elementFlags + type
    /// arguments only (propertiesRelatedTo 66804-66805); property
    /// synthesis lands with the first member-reading consumer (M4).
    fn create_tuple_target_type(
        &mut self,
        element_flags: &[ElementFlags],
        readonly: bool,
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
        let target = self.create_type(
            TypeFlags::OBJECT,
            TypeData::TupleTarget(TupleTargetData {
                type_parameters: type_parameters.clone().into_boxed_slice(),
                // Patched right below, after the target id exists —
                // tsc allocates thisType after the object (61194).
                this_type: TypeId(u32::MAX),
                element_flags: element_flags.to_vec().into_boxed_slice(),
                min_length,
                fixed_length,
                has_rest_element,
                combined_flags,
                readonly,
            }),
        );
        self.type_mut(target).object_flags =
            ObjectFlags::from_bits(ObjectFlags::TUPLE.bits() | ObjectFlags::REFERENCE.bits());
        let this_type = self.create_type_parameter(true, Some(target));
        let TypeData::TupleTarget(data) = &mut self.type_mut(target).data else {
            unreachable!("just created a tuple target");
        };
        data.this_type = this_type;
        // instantiations.set(getTypeListId(typeParameters), type) (61191).
        let key = (target, self.get_type_list_id(&type_parameters));
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
                constraint,
            },
        )
    }

    /// tsc-port: createTupleType @6.0.3
    /// tsc-hash: b2054126131f4beb527c2fc7b6ccacc23a1a2c0fecb62b02e76f01b77f1468f6
    /// tsc-span: _tsc.js:61141-61144
    pub fn create_tuple_type(
        &mut self,
        element_types: &[TypeId],
        element_flags: Option<&[ElementFlags]>,
        readonly: bool,
    ) -> Result<TypeId, M4Dependency> {
        let default_flags;
        let element_flags = match element_flags {
            Some(flags) => flags,
            None => {
                default_flags = vec![ElementFlags::REQUIRED; element_types.len()];
                &default_flags
            }
        };
        let target = self.get_tuple_target_type(element_flags, readonly)?;
        if element_types.is_empty() {
            return Ok(target);
        }
        self.create_normalized_type_reference(target, element_types)
    }

    /// tsc-port: createNormalizedTypeReference @6.0.3
    /// tsc-hash: 32b91334e6762e8ea63ac6a9be5f6689a4d112aa1db2d59986c736ac6735e143
    /// tsc-span: _tsc.js:61210-61212
    pub fn create_normalized_type_reference(
        &mut self,
        target: TypeId,
        type_arguments: &[TypeId],
    ) -> Result<TypeId, M4Dependency> {
        if self.object_flags_of(target).intersects(ObjectFlags::TUPLE) {
            self.create_normalized_tuple_type(target, type_arguments)
        } else {
            Ok(self.create_type_reference(target, type_arguments))
        }
    }

    /// tsc-port: createNormalizedTupleType @6.0.3
    /// tsc-hash: 5b7968f648c63d88544746d841015ff7800b723dbc071b96fb4d6f7ae0b18154
    /// tsc-span: _tsc.js:61213-61287
    ///
    /// M4-dependent arms return M4Dependency instead of a type: the
    /// union-in-variadic cross product (61218-61223, needs mapType over
    /// unions with error reporting), array-like variadic collapse
    /// (61252, getIndexTypeOfType) and the variadic-in-rest-window
    /// collapse (61262, getIndexedAccessType). None are reachable from
    /// M3 annotation shapes.
    pub fn create_normalized_tuple_type(
        &mut self,
        target: TypeId,
        element_types: &[TypeId],
    ) -> Result<TypeId, M4Dependency> {
        let TypeData::TupleTarget(data) = self.type_of(target).data.clone() else {
            unreachable!("createNormalizedTupleType requires a tuple target");
        };
        if !data.combined_flags.intersects(ElementFlags::NON_REQUIRED) {
            // No non-required elements: plain reference (61215-61217).
            return Ok(self.create_type_reference(target, element_types));
        }
        if data.combined_flags.intersects(ElementFlags::VARIADIC) {
            let has_union_variadic = element_types.iter().enumerate().any(|(i, &t)| {
                data.element_flags[i].intersects(ElementFlags::VARIADIC)
                    && self.flags_of(t).intersects(TypeFlags::from_bits(
                        TypeFlags::NEVER.bits() | TypeFlags::UNION.bits(),
                    ))
            });
            if has_union_variadic {
                return Err(M4Dependency(
                    "union/never variadic tuple element distribution (M4)",
                ));
            }
        }
        let mut expanded_types: Vec<TypeId> = Vec::new();
        let mut expanded_flags: Vec<ElementFlags> = Vec::new();
        let mut last_required_index: isize = -1;
        let mut first_rest_index: isize = -1;
        let mut last_optional_or_rest_index: isize = -1;
        {
            let mut add_element = |tables: &mut Self,
                                   expanded_types: &mut Vec<TypeId>,
                                   expanded_flags: &mut Vec<ElementFlags>,
                                   ty: TypeId,
                                   flags: ElementFlags| {
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
                    tables.add_optionality(ty, /*is_property*/ true, true)
                } else {
                    ty
                };
                expanded_types.push(pushed);
                expanded_flags.push(flags);
            };

            for (i, &element_type) in element_types.iter().enumerate() {
                let flags = data.element_flags[i];
                if flags.intersects(ElementFlags::VARIADIC) {
                    if self.flags_of(element_type).intersects(TypeFlags::ANY) {
                        add_element(
                            self,
                            &mut expanded_types,
                            &mut expanded_flags,
                            element_type,
                            ElementFlags::REST,
                        );
                    } else if self
                        .flags_of(element_type)
                        .intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
                    {
                        add_element(
                            self,
                            &mut expanded_types,
                            &mut expanded_flags,
                            element_type,
                            ElementFlags::VARIADIC,
                        );
                    } else if self.is_tuple_type(element_type) {
                        let inner_target = self.reference_target(element_type);
                        let TypeData::TupleTarget(inner) = self.type_of(inner_target).data.clone()
                        else {
                            unreachable!("tuple type targets a tuple target");
                        };
                        let inner_args: Vec<TypeId> = self.type_arguments(element_type).to_vec();
                        if inner_args.len() + expanded_types.len() >= 10_000 {
                            return Err(M4Dependency("tuple too large to represent (2799 family)"));
                        }
                        for (n, &inner_type) in inner_args.iter().enumerate() {
                            add_element(
                                self,
                                &mut expanded_types,
                                &mut expanded_flags,
                                inner_type,
                                inner.element_flags[n],
                            );
                        }
                    } else {
                        return Err(M4Dependency(
                            "array-like variadic element needs getIndexTypeOfType (M4)",
                        ));
                    }
                } else {
                    add_element(
                        self,
                        &mut expanded_types,
                        &mut expanded_flags,
                        element_type,
                        flags,
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
                return Err(M4Dependency(
                    "variadic element inside rest window needs getIndexedAccessType (M4)",
                ));
            }
            let window: Vec<TypeId> = expanded_types[first..=last].to_vec();
            expanded_types[first] = self.get_union_type_interim(&window);
            expanded_types.drain(first + 1..=last);
            expanded_flags.drain(first + 1..=last);
        }
        let tuple_target = self.get_tuple_target_type(&expanded_flags, data.readonly)?;
        if expanded_flags.is_empty() {
            Ok(tuple_target)
        } else {
            Ok(self.create_type_reference(tuple_target, &expanded_types))
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
    fn get_template_string_for_type(&self, id: TypeId) -> Option<String> {
        let ty = self.type_of(id);
        if ty.flags.intersects(TypeFlags::STRING_LITERAL) {
            if let TypeData::Literal {
                value: LiteralValue::String(value),
            } = &ty.data
            {
                return Some(value.clone());
            }
        }
        if ty.flags.intersects(TypeFlags::NUMBER_LITERAL) {
            if let TypeData::Literal {
                value: LiteralValue::Number(value),
            } = &ty.data
            {
                return Some(js_number_to_string(*value));
            }
        }
        if ty.flags.intersects(TypeFlags::BIG_INT_LITERAL) {
            if let TypeData::Literal {
                value: LiteralValue::BigInt(value),
            } = &ty.data
            {
                return Some(value.to_base10_string());
            }
        }
        if ty.flags.intersects(TypeFlags::from_bits(
            TypeFlags::BOOLEAN_LITERAL.bits() | TypeFlags::NULLABLE.bits(),
        )) {
            if let TypeData::Intrinsic { name, .. } = &ty.data {
                return Some((*name).to_owned());
            }
        }
        None
    }

    /// tsc-port: createTemplateLiteralType @6.0.3
    /// tsc-hash: 044da3c41e15fc13e9dbed4c062c7b6cd55d63aaef354968402604071716c263
    /// tsc-span: _tsc.js:62113-62118
    fn create_template_literal_type(&mut self, texts: Vec<String>, types: Vec<TypeId>) -> TypeId {
        self.create_type(
            TypeFlags::TEMPLATE_LITERAL,
            TypeData::TemplateLiteral {
                texts: texts.into_boxed_slice(),
                types: types.into_boxed_slice(),
            },
        )
    }

    /// tsc-port: getTemplateLiteralType @6.0.3
    /// tsc-hash: 8b75b60fe2a0a42ea0dbac6fc1a278f1707c3d540a076e246c5ffa452e8aa6b7
    /// tsc-span: _tsc.js:62057-62109
    ///
    /// The >=1e5 cross-product guard reports a diagnostic in tsc and
    /// yields errorType; the M3 port yields errorType silently (the
    /// probe never constructs unions that large).
    pub fn get_template_literal_type(&mut self, texts: &[String], types: &[TypeId]) -> TypeId {
        debug_assert_eq!(texts.len(), types.len() + 1);
        let union_index = types.iter().position(|&t| {
            self.flags_of(t).intersects(TypeFlags::from_bits(
                TypeFlags::NEVER.bits() | TypeFlags::UNION.bits(),
            ))
        });
        if let Some(union_index) = union_index {
            if !self.check_cross_product_union(types) {
                return self.intrinsics.error;
            }
            // mapType over the union constituent (62060).
            let member = types[union_index];
            if self.flags_of(member).intersects(TypeFlags::NEVER) {
                return member;
            }
            let TypeData::Union { types: members, .. } = self.type_of(member).data.clone() else {
                unreachable!("union flag implies union data");
            };
            let mut mapped = Vec::with_capacity(members.len());
            for &m in members.iter() {
                let mut replaced = types.to_vec();
                replaced[union_index] = m;
                mapped.push(self.get_template_literal_type(texts, &replaced));
            }
            return self.get_union_type_interim(&mapped);
        }
        if types.contains(&self.intrinsics.wildcard) {
            return self.intrinsics.wildcard;
        }
        let mut new_types: Vec<TypeId> = Vec::new();
        let mut new_texts: Vec<String> = Vec::new();
        let mut text = texts[0].clone();
        if !self.add_spans(&mut new_types, &mut new_texts, &mut text, texts, types) {
            return self.intrinsics.string;
        }
        if new_types.is_empty() {
            return self.get_string_literal_type(&text);
        }
        new_texts.push(text);
        if new_texts.iter().all(|t| t.is_empty()) {
            if new_types
                .iter()
                .all(|&t| self.flags_of(t).intersects(TypeFlags::STRING))
            {
                return self.intrinsics.string;
            }
            if new_types.len() == 1 && self.is_pattern_literal_type(new_types[0]) {
                return new_types[0];
            }
        }
        let key = format!(
            "{}|{}|{}",
            self.get_type_list_id(&new_types),
            new_texts
                .iter()
                .map(|t| t.len().to_string())
                .collect::<Vec<_>>()
                .join(","),
            new_texts.join("")
        );
        if let Some(&id) = self.template_literal_types.get(&key) {
            return id;
        }
        let id = self.create_template_literal_type(new_texts, new_types);
        self.template_literal_types.insert(key, id);
        id
    }

    /// addSpans inner function of getTemplateLiteralType (62089-62108).
    fn add_spans(
        &mut self,
        new_types: &mut Vec<TypeId>,
        new_texts: &mut Vec<String>,
        text: &mut String,
        texts: &[String],
        types: &[TypeId],
    ) -> bool {
        for (i, &t) in types.iter().enumerate() {
            let flags = self.flags_of(t);
            if flags.intersects(TypeFlags::from_bits(
                TypeFlags::LITERAL.bits() | TypeFlags::NULL.bits() | TypeFlags::UNDEFINED.bits(),
            )) {
                match self.get_template_string_for_type(t) {
                    Some(segment) => {
                        text.push_str(&segment);
                        text.push_str(&texts[i + 1]);
                    }
                    None => return false,
                }
            } else if flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
                let TypeData::TemplateLiteral {
                    texts: inner_texts,
                    types: inner_types,
                } = self.type_of(t).data.clone()
                else {
                    unreachable!("template flag implies template data");
                };
                text.push_str(&inner_texts[0]);
                let inner_tail: Vec<String> = inner_texts.to_vec();
                if !self.add_spans(new_types, new_texts, text, &inner_tail, &inner_types) {
                    return false;
                }
                text.push_str(&texts[i + 1]);
            } else if self.is_generic_index_type_placeholder(t)
                || self.is_pattern_literal_placeholder_type(t)
            {
                new_types.push(t);
                new_texts.push(std::mem::take(text));
                *text = texts[i + 1].clone();
            } else {
                return false;
            }
        }
        true
    }

    /// tsc isGenericIndexType (62437) — true only for instantiable
    /// index-ish types, none of which are constructible in M3; the
    /// pattern-placeholder check below carries all M3 traffic.
    fn is_generic_index_type_placeholder(&self, _id: TypeId) -> bool {
        false
    }

    /// tsc-port: isPatternLiteralPlaceholderType @6.0.3
    /// tsc-hash: a3ded0eca102eb77868c3ffd27dfbc429557ace0a417a21bd73defb876d950b1
    /// tsc-span: _tsc.js:62411-62424
    pub fn is_pattern_literal_placeholder_type(&self, id: TypeId) -> bool {
        let flags = self.flags_of(id);
        if flags.intersects(TypeFlags::INTERSECTION) {
            let TypeData::Intersection { types } = &self.type_of(id).data else {
                unreachable!("intersection flag implies intersection data");
            };
            let mut seen_placeholder = false;
            for &t in types.iter() {
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
    ///
    /// The StringMapping arm is dead until M4 (StringMapping types are
    /// unconstructible before intrinsic alias instantiation).
    pub fn is_pattern_literal_type(&self, id: TypeId) -> bool {
        if !self.flags_of(id).intersects(TypeFlags::TEMPLATE_LITERAL) {
            return false;
        }
        let TypeData::TemplateLiteral { types, .. } = &self.type_of(id).data else {
            unreachable!("template flag implies template data");
        };
        types
            .iter()
            .all(|&t| self.is_pattern_literal_placeholder_type(t))
    }

    /// tsc-port: checkCrossProductUnion @6.0.3
    /// tsc-hash: c916448698615d4762a0b12b7b0759c757fa2c19dedd842bc9448d9731fe9da1
    /// tsc-span: _tsc.js:61874-61883
    fn check_cross_product_union(&self, types: &[TypeId]) -> bool {
        let mut size: usize = 1;
        for &t in types {
            if self.flags_of(t).intersects(TypeFlags::UNION) {
                if let TypeData::Union { types: members, .. } = &self.type_of(t).data {
                    size = size.saturating_mul(members.len());
                }
            }
        }
        size < 100_000
    }
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

/// JS number-to-string for the template-literal folding path. Covers
/// the annotation-reachable shapes (integers and simple decimals); the
/// full JS dtoa shortest-round-trip algorithm is not needed until
/// non-literal sources exist.
fn js_number_to_string(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e21 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> TypeTables {
        TypeTables::new(/*strict_null_checks*/ true, /*eopt*/ false)
    }

    #[test]
    fn intrinsics_are_allocated_in_tsc_order() {
        let t = tables();
        // anyType is the first allocation, like tsc typeCount order.
        assert_eq!(t.intrinsics.any, TypeId(0));
        assert!(t.intrinsics.unknown < t.intrinsics.undefined);
        assert!(t.intrinsics.false_regular < t.intrinsics.true_fresh);
        // strictNullChecks aliases the widening variants (47033/47050).
        assert_eq!(t.intrinsics.undefined_widening, t.intrinsics.undefined);
        assert_eq!(t.intrinsics.null_widening, t.intrinsics.null);
        // exactOptionalPropertyTypes off aliases undefinedOrMissing.
        assert_eq!(t.intrinsics.undefined_or_missing, t.intrinsics.undefined);

        let loose = TypeTables::new(false, false);
        assert_ne!(
            loose.intrinsics.undefined_widening,
            loose.intrinsics.undefined
        );
        assert_ne!(loose.intrinsics.null_widening, loose.intrinsics.null);
    }

    #[test]
    fn boolean_is_a_flagged_union_of_regular_literals() {
        let t = tables();
        let boolean = t.type_of(t.intrinsics.boolean);
        assert!(boolean.flags.intersects(TypeFlags::UNION));
        assert!(boolean.flags.intersects(TypeFlags::BOOLEAN));
        let TypeData::Union { types, .. } = &boolean.data else {
            panic!("boolean must be a union");
        };
        assert_eq!(
            types.as_ref(),
            [t.intrinsics.false_regular, t.intrinsics.true_regular]
        );
    }

    #[test]
    fn literal_types_intern_by_value_and_wire_freshness() {
        let mut t = tables();
        let one_a = t.get_number_literal_type(1.0);
        let one_b = t.get_number_literal_type(1.0);
        let two = t.get_number_literal_type(2.0);
        assert_eq!(one_a, one_b);
        assert_ne!(one_a, two);

        // SameValueZero: -0 and +0 share an entry.
        assert_eq!(
            t.get_number_literal_type(-0.0),
            t.get_number_literal_type(0.0)
        );

        let fresh = t.get_fresh_type_of_literal_type(one_a);
        assert_ne!(fresh, one_a);
        assert!(t.is_fresh_literal_type(fresh));
        assert!(!t.is_fresh_literal_type(one_a));
        assert_eq!(t.get_fresh_type_of_literal_type(one_a), fresh);
        assert_eq!(t.get_fresh_type_of_literal_type(fresh), fresh);
        assert_eq!(t.get_regular_type_of_literal_type(fresh), one_a);

        let a = t.get_string_literal_type("a");
        assert_eq!(t.get_string_literal_type("a"), a);
        let big = t.get_bigint_literal_type(PseudoBigInt {
            negative: false,
            base10_value: "1".to_owned(),
        });
        assert_eq!(
            t.get_bigint_literal_type(PseudoBigInt {
                negative: false,
                base10_value: "1".to_owned(),
            }),
            big
        );
    }

    #[test]
    fn unions_intern_by_sorted_member_list() {
        let mut t = tables();
        let one = t.get_number_literal_type(1.0);
        let two = t.get_number_literal_type(2.0);
        let a = t.get_union_type_interim(&[one, two]);
        let b = t.get_union_type_interim(&[two, one]);
        assert_eq!(a, b);
        // Flattening: (1 | 2) | 2 == 1 | 2.
        assert_eq!(t.get_union_type_interim(&[a, two]), a);
        // Singletons collapse; empties are never.
        assert_eq!(t.get_union_type_interim(&[one]), one);
        assert_eq!(t.get_union_type_interim(&[]), t.intrinsics.never);
    }

    #[test]
    fn get_type_list_id_compresses_consecutive_ids() {
        let t = tables();
        assert_eq!(
            t.get_type_list_id(&[TypeId(5), TypeId(6), TypeId(7), TypeId(9)]),
            "5:3,9"
        );
        assert_eq!(t.get_type_list_id(&[TypeId(3)]), "3");
        assert_eq!(t.get_type_list_id(&[]), "");
    }

    #[test]
    fn tuple_targets_intern_by_flags_and_readonly() {
        let mut t = tables();
        let req = [ElementFlags::REQUIRED, ElementFlags::OPTIONAL];
        let a = t.get_tuple_target_type(&req, false).expect("target");
        let b = t.get_tuple_target_type(&req, false).expect("target");
        let readonly = t.get_tuple_target_type(&req, true).expect("target");
        assert_eq!(a, b);
        assert_ne!(a, readonly);
        let TypeData::TupleTarget(data) = &t.type_of(a).data else {
            panic!("tuple target expected");
        };
        assert_eq!(data.min_length, 1);
        assert_eq!(data.fixed_length, 2);
        assert!(!data.has_rest_element);

        let rest = [ElementFlags::REQUIRED, ElementFlags::REST];
        let with_rest = t.get_tuple_target_type(&rest, false).expect("target");
        let TypeData::TupleTarget(data) = &t.type_of(with_rest).data else {
            panic!("tuple target expected");
        };
        assert_eq!(data.min_length, 1);
        assert_eq!(data.fixed_length, 1);
        assert!(data.has_rest_element);
    }

    #[test]
    fn normalized_tuples_splice_variadic_tuples() {
        let mut t = tables();
        let number = t.intrinsics.number;
        let string = t.intrinsics.string;
        let boolean = t.intrinsics.boolean;
        // [string, boolean]
        let inner = t
            .create_tuple_type(&[string, boolean], None, false)
            .expect("inner tuple");
        // [number, ...[string, boolean]] normalizes to [number, string, boolean].
        let outer_flags = [ElementFlags::REQUIRED, ElementFlags::VARIADIC];
        let outer_target = t
            .get_tuple_target_type(&outer_flags, false)
            .expect("target");
        let outer = t
            .create_normalized_tuple_type(outer_target, &[number, inner])
            .expect("normalized");
        let direct = t
            .create_tuple_type(&[number, string, boolean], None, false)
            .expect("direct tuple");
        assert_eq!(outer, direct);
    }

    #[test]
    fn template_literal_types_fold_and_intern() {
        let mut t = tables();
        let string = t.intrinsics.string;
        let number = t.intrinsics.number;
        // `a${string}` interns by texts+types.
        let a1 = t.get_template_literal_type(&["a".into(), "".into()], &[string]);
        let a2 = t.get_template_literal_type(&["a".into(), "".into()], &[string]);
        assert_eq!(a1, a2);
        // All-literal spans fold to a plain string literal (62071-62073).
        let one = t.get_number_literal_type(1.0);
        let folded = t.get_template_literal_type(&["a".into(), "b".into()], &[one]);
        assert_eq!(folded, t.get_string_literal_type("a1b"));
        // `${string}` with empty texts collapses to string (62075-62078).
        let s = t.get_template_literal_type(&["".into(), "".into()], &[string]);
        assert_eq!(s, string);
        // `${number}` stays a pattern template.
        let n = t.get_template_literal_type(&["".into(), "".into()], &[number]);
        assert!(t.flags_of(n).intersects(TypeFlags::TEMPLATE_LITERAL));
        assert!(t.is_pattern_literal_type(n));
    }

    #[test]
    fn distinct_anonymous_object_types_never_intern() {
        let mut t = tables();
        let a = t.create_type(TypeFlags::OBJECT, TypeData::Object);
        let b = t.create_type(TypeFlags::OBJECT, TypeData::Object);
        assert_ne!(a, b);
    }

    #[test]
    fn optionality_follows_strict_null_checks() {
        let mut t = tables();
        let number = t.intrinsics.number;
        let optional = t.add_optionality(number, /*is_property*/ true, true);
        let TypeData::Union { types, .. } = &t.type_of(optional).data else {
            panic!("optional property type must union undefined");
        };
        assert!(types.contains(&t.intrinsics.undefined));
        assert_eq!(t.add_optionality(number, true, false), number);

        let mut loose = TypeTables::new(false, false);
        let number = loose.intrinsics.number;
        assert_eq!(loose.add_optionality(number, true, true), number);
    }
}
