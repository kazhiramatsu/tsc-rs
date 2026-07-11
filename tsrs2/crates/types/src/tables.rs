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
use crate::ty::{LiteralValue, PseudoBigInt, SymbolId, TupleTargetData, Type, TypeData, TypeId};

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

/// The M4-dependency escape hatch: a constructor arm whose inputs are
/// unconstructible before instantiation lands returns this instead of
/// inventing a type; the relpin probe surfaces it as Unsupported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct M4Dependency(pub &'static str);

/// tsc UnionReduction (checker-scope const enum: None=0, Literal=1,
/// Subtype=2). Subtype stubs to Literal until stage 4.8.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    /// unionOfUnionTypes (46990) — the 2-union getUnionType fast path,
    /// keyed `{smallerId}{N|S|L}{largerId}` (+ alias id, M4).
    union_of_union_types: HashMap<String, TypeId>,
    /// intersectionTypes (46991).
    intersection_types: HashMap<String, TypeId>,
    /// tupleTypes (46988), keyed per getTupleTargetType 61149.
    tuple_types: HashMap<String, TypeId>,
    /// templateLiteralTypes (46997), keyed per getTemplateLiteralType 62083.
    template_literal_types: HashMap<String, TypeId>,
    /// stringMappingTypes (46998), keyed `${symbolId},${typeId}`
    /// (getStringMappingTypeForGenericType 62154).
    string_mapping_types: HashMap<(SymbolId, TypeId), TypeId>,
    /// Per-target `type.instantiations` maps (createTypeReference
    /// 60170-60174 AND getObjectTypeInstantiation 63489-63492 — one map
    /// per target in tsc), flattened to one table keyed (target, id).
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
                template_constraint: TypeId(0),
                numeric_string: TypeId(0),
                unique_literal: TypeId(0),
            },
            string_literal_types: HashMap::new(),
            number_literal_types: HashMap::new(),
            bigint_literal_types: HashMap::new(),
            union_types: HashMap::new(),
            union_of_union_types: HashMap::new(),
            intersection_types: HashMap::new(),
            tuple_types: HashMap::new(),
            template_literal_types: HashMap::new(),
            string_mapping_types: HashMap::new(),
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
            self.type_mut(fresh).regular_type = Some(regular);
            self.type_mut(fresh).fresh_type = Some(fresh);
            self.type_mut(regular).regular_type = Some(regular);
            self.type_mut(regular).fresh_type = Some(fresh);
        }
        let boolean = self.get_union_type(&[false_regular, true_regular], UnionReduction::Literal);

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
        let string_or_number = self.get_union_type(&[string, number], UnionReduction::Literal);
        let string_number_symbol =
            self.get_union_type(&[string, number, es_symbol], UnionReduction::Literal);
        let number_or_bigint = self.get_union_type(&[number, bigint], UnionReduction::Literal);
        let template_constraint = self.get_union_type(
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
            let regular = self.get_union_type(&mapped, UnionReduction::Literal);
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

    /// tsc-port: getAliasId @6.0.3
    /// tsc-hash: 39a787bbf937e6b559ce913d2244295fd711853991752904a3b4f649be60db54
    /// tsc-span: _tsc.js:60151-60153
    pub fn get_alias_id(
        &self,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> String {
        match alias_symbol {
            None => String::new(),
            Some(symbol) => match alias_type_arguments {
                None => format!("@{}", symbol.0),
                Some(arguments) => {
                    format!("@{}:{}", symbol.0, self.get_type_list_id(arguments))
                }
            },
        }
    }

    /// The per-target instantiations map (`type.instantiations`) —
    /// createTypeReference keys by list id; getObjectTypeInstantiation
    /// keys by list id + alias id over the SAME map.
    pub fn instantiation_get(&self, target: TypeId, key: &str) -> Option<TypeId> {
        self.instantiations
            .get(&(target, key.to_owned()))
            .copied()
    }

    pub fn instantiation_insert(&mut self, target: TypeId, key: String, value: TypeId) {
        self.instantiations.insert((target, key), value);
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
    pub fn get_union_type(&mut self, types: &[TypeId], reduction: UnionReduction) -> TypeId {
        // Subtype reduction needs the relation engine (removeSubtypes
        // 61368) — the checker-side get_union_type_ex handles it; the
        // tables twin serves the pure construction callers, which are
        // all Literal/None.
        debug_assert!(
            reduction != UnionReduction::Subtype,
            "Subtype union reduction goes through CheckerState::get_union_type_ex"
        );
        self.get_union_type_with_origin(types, reduction, None)
    }

    /// The origin-carrying getUnionType entry (the `origin` parameter
    /// at 61505); intersection cross-product distribution passes a
    /// denormalized intersection origin through here.
    pub fn get_union_type_with_origin(
        &mut self,
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
            let infix = match reduction {
                UnionReduction::None => "N",
                UnionReduction::Subtype => "S",
                UnionReduction::Literal => "L",
            };
            let index = usize::from(types[0].0 >= types[1].0);
            let key = format!("{}{infix}{}", types[index].0, types[1 - index].0);
            if let Some(&id) = self.union_of_union_types.get(&key) {
                return id;
            }
            let id = self.get_union_type_worker(types, reduction, None);
            self.union_of_union_types.insert(key, id);
            return id;
        }
        self.get_union_type_worker(types, reduction, origin)
    }

    /// unionOfUnionTypes fast-path cache access for the checker-side
    /// Subtype-capable getUnionType twin.
    pub fn union_of_union_types_get(&self, key: &str) -> Option<TypeId> {
        self.union_of_union_types.get(key).copied()
    }

    pub fn union_of_union_types_insert(&mut self, key: String, id: TypeId) {
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
    ///   move checker-side (M4); they never take the unionOfUnionTypes
    ///   fast path with such mixes today, so the shared cache stays
    ///   consistent between the twins.
    /// - removeConstrainedTypeVariables (61550-61552) is unreachable:
    ///   ObjectFlags::IsConstrainedTypeVariable intersections are born
    ///   in getIntersectionType step 6 (M4 type variables).
    fn get_union_type_worker(
        &mut self,
        types: &[TypeId],
        reduction: UnionReduction,
        origin: Option<TypeId>,
    ) -> TypeId {
        let mut type_set: Vec<TypeId> = Vec::new();
        let includes = self.add_types_to_union(&mut type_set, 0, types);
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
            // constructors move checker-side (M4 5.3).
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
        self.finish_union_type_set(type_set, includes, types, origin)
    }

    /// The getUnionTypeWorker TAIL (61586-61612): named-union origin
    /// denormalization + objectFlags computation + interning. Shared
    /// with the checker-side Subtype-capable twin (stage 4.8).
    pub fn finish_union_type_set(
        &mut self,
        type_set: Vec<TypeId>,
        includes: i32,
        types: &[TypeId],
        origin: Option<TypeId>,
    ) -> TypeId {
        let mut origin = origin;
        if origin.is_none() && includes & TypeFlags::UNION.bits() != 0 {
            let mut named_unions: Vec<TypeId> = Vec::new();
            self.add_named_unions(&mut named_unions, types);
            let mut reduced_types: Vec<TypeId> = Vec::new();
            for &t in &type_set {
                let in_named = named_unions.iter().any(|&union| {
                    let TypeData::Union { types: members, .. } = &self.type_of(union).data else {
                        unreachable!("named unions are unions");
                    };
                    contains_type(members, t)
                });
                if !in_named {
                    reduced_types.push(t);
                }
            }
            // !aliasSymbol is vacuously true until M4 aliases.
            if named_unions.len() == 1 && reduced_types.is_empty() {
                return named_unions[0];
            }
            let named_types_count: usize = named_unions
                .iter()
                .map(|&union| {
                    let TypeData::Union { types: members, .. } = &self.type_of(union).data else {
                        unreachable!("named unions are unions");
                    };
                    members.len()
                })
                .sum();
            if named_types_count + reduced_types.len() == type_set.len() {
                for &union in &named_unions {
                    insert_type(&mut reduced_types, union);
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
        self.get_union_type_from_sorted_list(type_set, object_flags, origin)
    }

    /// tsc-port: addTypeToUnion @6.0.3
    /// tsc-hash: 2e2421304f1c9d829df070f06f76534b25526f5f8fa4995ca8c903b4fd8e2bad
    /// tsc-span: _tsc.js:61338-61357
    ///
    /// stableTypeOrdering is off: insertion keyed by type id with the
    /// append fast path (61350).
    pub fn add_type_to_union(
        &mut self,
        type_set: &mut Vec<TypeId>,
        mut includes: i32,
        ty: TypeId,
    ) -> i32 {
        let flags = self.flags_of(ty).bits();
        if flags & TypeFlags::NEVER.bits() == 0 {
            includes |= flags & TypeFlags::INCLUDES_MASK.bits();
            if flags & TypeFlags::INSTANTIABLE.bits() != 0 {
                includes |= TypeFlags::INCLUDES_INSTANTIABLE.bits();
            }
            if flags & TypeFlags::INTERSECTION.bits() != 0
                && self
                    .object_flags_of(ty)
                    .intersects(ObjectFlags::IS_CONSTRAINED_TYPE_VARIABLE)
            {
                includes |= TypeFlags::INCLUDES_CONSTRAINED_TYPE_VARIABLE.bits();
            }
            if ty == self.intrinsics.wildcard {
                includes |= TypeFlags::INCLUDES_WILDCARD.bits();
            }
            if self.is_error_type(ty) {
                includes |= TypeFlags::INCLUDES_ERROR.bits();
            }
            if !self.strict_null_checks && flags & TypeFlags::NULLABLE.bits() != 0 {
                if !self
                    .object_flags_of(ty)
                    .intersects(ObjectFlags::CONTAINS_WIDENING_TYPE)
                {
                    includes |= TypeFlags::INCLUDES_NON_WIDENING_TYPE.bits();
                }
            } else {
                match type_set.last() {
                    Some(&last) if ty.0 > last.0 => type_set.push(ty),
                    _ => {
                        if let Err(index) = type_set.binary_search(&ty) {
                            type_set.insert(index, ty);
                        }
                    }
                }
            }
        }
        includes
    }

    /// tsc-port: addTypesToUnion @6.0.3
    /// tsc-hash: 14b6eea2a85c949d2f78fa0e81a934b7a2168d3e5c42fb693dbcbdee0e65192c
    /// tsc-span: _tsc.js:61358-61367
    pub fn add_types_to_union(
        &mut self,
        type_set: &mut Vec<TypeId>,
        mut includes: i32,
        types: &[TypeId],
    ) -> i32 {
        let mut last_type: Option<TypeId> = None;
        for &ty in types {
            if Some(ty) != last_type {
                includes = if self.flags_of(ty).intersects(TypeFlags::UNION) {
                    let named = self.is_named_union_type(ty);
                    let TypeData::Union { types: members, .. } = self.type_of(ty).data.clone()
                    else {
                        unreachable!("union flag implies union data");
                    };
                    self.add_types_to_union(
                        type_set,
                        includes | (if named { TypeFlags::UNION.bits() } else { 0 }),
                        &members,
                    )
                } else {
                    self.add_type_to_union(type_set, includes, ty)
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
                    && contains_type(
                        types,
                        self.type_of(t)
                            .regular_type
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
        if self.type_of(ty).alias_symbol.is_some() {
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
            if self.type_of(t).alias_symbol.is_some() || (origin.is_some() && !origin_is_union) {
                if !named_unions.contains(&t) {
                    named_unions.push(t);
                }
            } else if let Some(origin) = origin.filter(|_| origin_is_union) {
                let TypeData::Union { types: members, .. } = self.type_of(origin).data.clone()
                else {
                    unreachable!("union origin with union flag has union data");
                };
                self.add_named_unions(named_unions, &members);
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
        let data = if flags.intersects(TypeFlags::UNION) {
            TypeData::Union {
                types: types.into_boxed_slice(),
                origin: None,
            }
        } else {
            TypeData::Intersection {
                types: types.into_boxed_slice(),
            }
        };
        self.create_type(flags, data)
    }

    /// tsc-port: isErrorType @6.0.3
    /// tsc-hash: 2c5f5aabe3bf8bf77f0dbb8160922bdde8b776142a5387117432426d7ca501c4
    /// tsc-span: _tsc.js:55821-55823
    pub fn is_error_type(&self, ty: TypeId) -> bool {
        ty == self.intrinsics.error
            || (self.flags_of(ty).intersects(TypeFlags::ANY)
                && self.type_of(ty).alias_symbol.is_some())
    }

    /// tsc-port: getUnionTypeFromSortedList @6.0.3
    /// tsc-hash: 1e98366b81464c049f6729b888516648a16e01e90f6b6e8264bf775700f93e6b
    /// tsc-span: _tsc.js:61613-61641
    ///
    /// getAliasId is empty until M4 aliases; the `#`-prefixed origin
    /// key form covers non-union/intersection origins (M4 index types)
    /// and is unreachable here. The 61634-61637 boolean special case
    /// sets flags |= Boolean (the union's "boolean" intrinsicName is
    /// display-only and lands with T2 display work).
    pub fn get_union_type_from_sorted_list(
        &mut self,
        types: Vec<TypeId>,
        precomputed_object_flags: ObjectFlags,
        origin: Option<TypeId>,
    ) -> TypeId {
        if types.is_empty() {
            return self.intrinsics.never;
        }
        if types.len() == 1 {
            return types[0];
        }
        let key = match origin {
            None => self.get_type_list_id(&types),
            Some(origin) => {
                let origin_flags = self.flags_of(origin);
                let (TypeData::Union { types: members, .. }
                | TypeData::Intersection { types: members }) = &self.type_of(origin).data
                else {
                    unreachable!("non-union/intersection origins arrive with M4 index types");
                };
                let members = members.clone();
                if origin_flags.intersects(TypeFlags::UNION) {
                    format!("|{}", self.get_type_list_id(&members))
                } else {
                    format!("&{}", self.get_type_list_id(&members))
                }
            }
        };
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
                origin,
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
        self.get_union_type(&[id, missing_or_undefined], UnionReduction::Literal)
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
    ) -> TypeId {
        let propagating = self.get_propagating_flags_of_types(&types, TypeFlags::NULLABLE);
        let id = self.create_type(
            TypeFlags::INTERSECTION,
            TypeData::Intersection {
                types: types.into_boxed_slice(),
            },
        );
        self.type_mut(id).object_flags =
            ObjectFlags::from_bits(object_flags.bits() | propagating.bits());
        id
    }

    /// intersectionTypes map access for the checker-side
    /// getIntersectionType (the map itself is tsc's 46991).
    pub fn intersection_types_get(&self, key: &str) -> Option<TypeId> {
        self.intersection_types.get(key).copied()
    }

    pub fn intersection_types_insert(&mut self, key: String, id: TypeId) {
        self.intersection_types.insert(key, id);
    }

    /// tsc-port: eachUnionContains @6.0.3
    /// tsc-hash: dd8bbe6c4f8b46240e483f5e92565e69ef3d3f801012640f7468c767851bcb05
    /// tsc-span: _tsc.js:61697-61713
    fn each_union_contains(&self, union_types: &[TypeId], ty: TypeId) -> bool {
        for &union in union_types {
            let TypeData::Union { types: members, .. } = &self.type_of(union).data else {
                unreachable!("primitive unions are unions");
            };
            if contains_type(members, ty) {
                continue;
            }
            if ty == self.intrinsics.missing {
                return contains_type(members, self.intrinsics.undefined);
            }
            if ty == self.intrinsics.undefined {
                return contains_type(members, self.intrinsics.missing);
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
                Some(primitive) if contains_type(members, primitive) => {}
                _ => return false,
            }
        }
        true
    }

    /// tsc-port: intersectUnionsOfPrimitiveTypes @6.0.3
    /// tsc-hash: 3a0d9554ee46b79147e66ba642df88c96207d15f0df796034c3dbab9e17c4ccf
    /// tsc-span: _tsc.js:61737-61776
    pub fn intersect_unions_of_primitive_types(&mut self, types: &mut Vec<TypeId>) -> bool {
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
            .flat_map(|&u| {
                let TypeData::Union { types: members, .. } = &self.type_of(u).data else {
                    unreachable!("primitive unions are unions");
                };
                members.to_vec()
            })
            .collect();
        for t in all_members {
            if insert_type(&mut checked, t) && self.each_union_contains(&union_types, t) {
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
                insert_type(&mut result, t);
            }
        }
        types[index] =
            self.get_union_type_from_sorted_list(result, ObjectFlags::PRIMITIVE_UNION, None);
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
        let TypeData::Union { types, .. } = &self.type_of(ty).data else {
            unreachable!("union flag implies union data");
        };
        types.first() == Some(&self.intrinsics.missing)
    }

    /// tsc-port: everyType @6.0.3
    /// tsc-hash: c4dd71e9f3d68c0f125a76cc961b9eafb0217706e0cfa0c096d2df786cc22253
    /// tsc-span: _tsc.js:69985-69987
    pub fn every_type(&self, ty: TypeId, f: impl Fn(&Self, TypeId) -> bool) -> bool {
        if self.flags_of(ty).intersects(TypeFlags::UNION) {
            let TypeData::Union { types, .. } = &self.type_of(ty).data else {
                unreachable!("union flag implies union data");
            };
            types.iter().all(|&t| f(self, t))
        } else {
            f(self, ty)
        }
    }

    /// tsc-port: filterType @6.0.3
    /// tsc-hash: acc1fb95f1d693ec174389f71c5609dca06afa6c24c05435dfab03b4dae827f4
    /// tsc-span: _tsc.js:69991-70021
    pub fn filter_type(&mut self, ty: TypeId, f: impl Fn(&Self, TypeId) -> bool) -> TypeId {
        if self.flags_of(ty).intersects(TypeFlags::UNION) {
            let TypeData::Union { types, origin } = self.type_of(ty).data.clone() else {
                unreachable!("union flag implies union data");
            };
            let filtered: Vec<TypeId> = types.iter().copied().filter(|&t| f(self, t)).collect();
            if filtered.len() == types.len() {
                return ty;
            }
            let mut new_origin = None;
            if let Some(origin) = origin {
                if self.flags_of(origin).intersects(TypeFlags::UNION) {
                    let TypeData::Union {
                        types: origin_types,
                        ..
                    } = self.type_of(origin).data.clone()
                    else {
                        unreachable!("union origin with union flag has union data");
                    };
                    let origin_filtered: Vec<TypeId> = origin_types
                        .iter()
                        .copied()
                        .filter(|&t| self.flags_of(t).intersects(TypeFlags::UNION) || f(self, t))
                        .collect();
                    if origin_types.len() - origin_filtered.len() == types.len() - filtered.len() {
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
            return self.get_union_type_from_sorted_list(filtered, object_flags, new_origin);
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
        if !flags.intersects(TypeFlags::UNION_OR_INTERSECTION)
            || self.type_of(ty).alias_symbol.is_some()
        {
            return 1;
        }
        if flags.intersects(TypeFlags::UNION) {
            if let TypeData::Union {
                origin: Some(origin),
                ..
            } = &self.type_of(ty).data
            {
                return self.get_constituent_count(*origin);
            }
        }
        let (TypeData::Union { types, .. } | TypeData::Intersection { types }) =
            &self.type_of(ty).data
        else {
            unreachable!("union/intersection flag implies member data");
        };
        self.get_constituent_count_of_types(types)
    }

    pub fn get_constituent_count_of_types(&self, types: &[TypeId]) -> usize {
        types.iter().map(|&t| self.get_constituent_count(t)).sum()
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
    /// itself for tuple targets (tsc `type.target = type`, 61192) and
    /// class/interface GenericType targets (57398).
    pub fn reference_target(&self, id: TypeId) -> TypeId {
        match &self.type_of(id).data {
            TypeData::Reference { target, .. } => *target,
            TypeData::TupleTarget(_) | TypeData::GenericType { .. } => id,
            _ => id,
        }
    }

    /// getTypeArguments for instantiation-free references (60202-60222:
    /// plain references have resolvedTypeArguments eagerly; the lazy
    /// node-reading branch belongs to M4 deferred references). Tuple
    /// and class/interface targets alias their own typeParameters
    /// (61193 / 57399).
    pub fn type_arguments(&self, id: TypeId) -> &[TypeId] {
        match &self.type_of(id).data {
            TypeData::Reference {
                resolved_type_arguments,
                ..
            } => resolved_type_arguments,
            TypeData::TupleTarget(data) => &data.type_parameters,
            TypeData::GenericType {
                type_parameters, ..
            } => type_parameters,
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
            expanded_types[first] = self.get_union_type(&window, UnionReduction::Literal);
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
            return self.get_union_type(&mapped, UnionReduction::Literal);
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
            } else if self.is_generic_index_type(t) || self.is_pattern_literal_placeholder_type(t)
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
    /// The Substitution arm is unreachable (that TypeFlag is
    /// unconstructible before conditional types, M8); isGenericMappedType
    /// is constant false the same way (no Mapped ObjectFlags are ever
    /// created before M8) — both guarded, not silently elided.
    fn get_generic_object_flags(&mut self, id: TypeId) -> ObjectFlags {
        let flags = self.flags_of(id);
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            if !self
                .object_flags_of(id)
                .intersects(ObjectFlags::IS_GENERIC_TYPE_COMPUTED)
            {
                let members: Vec<TypeId> = match &self.type_of(id).data {
                    TypeData::Union { types, .. } => types.to_vec(),
                    TypeData::Intersection { types } => types.to_vec(),
                    _ => unreachable!("union/intersection flag implies member data"),
                };
                let mut combined = 0i32;
                for member in members {
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
            unreachable!("substitution types are unconstructible before conditional types (M8)");
        }
        assert!(
            !self.object_flags_of(id).intersects(ObjectFlags::MAPPED),
            "mapped types are unconstructible before M8 (isGenericMappedType)"
        );
        let object = if flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
            || self.is_generic_tuple_type(id)
        {
            ObjectFlags::IS_GENERIC_OBJECT_TYPE.bits()
        } else {
            0
        };
        let index = if flags
            .intersects(TypeFlags::from_bits(
                TypeFlags::INSTANTIABLE_NON_PRIMITIVE.bits() | TypeFlags::INDEX.bits(),
            ))
            || self.is_generic_string_like_type(id)
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
        match &self.type_of(target).data {
            TypeData::TupleTarget(data) => {
                data.combined_flags.intersects(ElementFlags::VARIADIC)
            }
            _ => false,
        }
    }

    /// tsc-port: isGenericStringLikeType @6.0.3
    /// tsc-hash: bab14bed10f5a2ddbf5b7c73efdb4a5897ad9d2e48b2d79734b4deaa3f525617
    /// tsc-span: _tsc.js:62428-62430
    pub fn is_generic_string_like_type(&self, id: TypeId) -> bool {
        self.flags_of(id).intersects(TypeFlags::from_bits(
            TypeFlags::TEMPLATE_LITERAL.bits() | TypeFlags::STRING_MAPPING.bits(),
        )) && !self.is_pattern_literal_type(id)
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
    pub fn is_pattern_literal_type(&self, id: TypeId) -> bool {
        let flags = self.flags_of(id);
        if flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            let TypeData::TemplateLiteral { types, .. } = &self.type_of(id).data else {
                unreachable!("template flag implies template data");
            };
            return types
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

    /// tsc-port: checkCrossProductUnion @6.0.3
    /// tsc-hash: c916448698615d4762a0b12b7b0759c757fa2c19dedd842bc9448d9731fe9da1
    /// tsc-span: _tsc.js:61874-61883
    fn check_cross_product_union(&self, types: &[TypeId]) -> bool {
        self.get_cross_product_union_size(types) < 100_000
    }

    /// tsc-port: getCrossProductUnionSize @6.0.3
    /// tsc-hash: b5e9572ca4c32fb818e3a1e18ce1d7e91ca18afbf17a6d35fce122616a4dc3c0
    /// tsc-span: _tsc.js:61871-61873
    fn get_cross_product_union_size(&self, types: &[TypeId]) -> usize {
        let mut size: usize = 1;
        for &t in types {
            if self.flags_of(t).intersects(TypeFlags::UNION) {
                if let TypeData::Union { types: members, .. } = &self.type_of(t).data {
                    size = size.saturating_mul(members.len());
                }
            } else if self.flags_of(t).intersects(TypeFlags::NEVER) {
                size = 0;
            }
        }
        size
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

        // 47101-47102: templateConstraintType + numericStringType sit
        // between numberOrBigInt and uniqueLiteral (skipping them
        // shifted every later id off the oracle's).
        assert!(t.intrinsics.number_or_bigint < t.intrinsics.template_constraint);
        assert!(t.intrinsics.template_constraint < t.intrinsics.numeric_string);
        assert!(t.intrinsics.numeric_string < t.intrinsics.unique_literal);
    }

    #[test]
    fn template_constraint_and_numeric_string_shapes() {
        let mut t = tables();
        assert!(t
            .flags_of(t.intrinsics.template_constraint)
            .intersects(TypeFlags::UNION));
        match &t.type_of(t.intrinsics.numeric_string).data {
            TypeData::TemplateLiteral { texts, types } => {
                assert_eq!(texts.len(), 2);
                assert!(texts.iter().all(|text| text.is_empty()));
                assert_eq!(types.len(), 1);
                assert_eq!(types[0], t.intrinsics.number);
            }
            other => panic!("numeric_string should be a template literal: {other:?}"),
        }
        let number = t.intrinsics.number;
        let again = t.get_template_literal_type(&[String::new(), String::new()], &[number]);
        assert_eq!(again, t.intrinsics.numeric_string);
    }

    #[test]
    fn js_number_to_string_matches_ecmascript_number_to_string() {
        assert_eq!(js_number_to_string(0.0), "0");
        assert_eq!(js_number_to_string(-0.0), "0");
        assert_eq!(js_number_to_string(1.0), "1");
        assert_eq!(js_number_to_string(123.456), "123.456");
        assert_eq!(js_number_to_string(-1.5), "-1.5");
        // Above 2^63: `as i64` saturation fabricated 9223372036854775807.
        assert_eq!(js_number_to_string(1e19), "10000000000000000000");
        assert_eq!(js_number_to_string(1e20), "100000000000000000000");
        // The decimal/exponent thresholds (ES2023 6.1.6.1.20).
        assert_eq!(js_number_to_string(1e21), "1e+21");
        assert_eq!(js_number_to_string(2.5e21), "2.5e+21");
        assert_eq!(js_number_to_string(1e-6), "0.000001");
        assert_eq!(js_number_to_string(1e-7), "1e-7");
        assert_eq!(js_number_to_string(1.5e-7), "1.5e-7");
        assert_eq!(js_number_to_string(f64::INFINITY), "Infinity");
        assert_eq!(js_number_to_string(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(js_number_to_string(f64::NAN), "NaN");
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
        let a = t.get_union_type(&[one, two], UnionReduction::Literal);
        let b = t.get_union_type(&[two, one], UnionReduction::Literal);
        assert_eq!(a, b);
        // Flattening: (1 | 2) | 2 == 1 | 2.
        assert_eq!(t.get_union_type(&[a, two], UnionReduction::Literal), a);
        // Singletons collapse; empties are never.
        assert_eq!(t.get_union_type(&[one], UnionReduction::Literal), one);
        assert_eq!(
            t.get_union_type(&[], UnionReduction::Literal),
            t.intrinsics.never
        );
    }

    #[test]
    fn union_literal_reduction_drops_subsumed_literals() {
        let mut t = tables();
        let one = t.get_number_literal_type(1.0);
        let a = t.get_string_literal_type("a");
        // "a" | string reduces to string; 1 | number reduces to number.
        assert_eq!(
            t.get_union_type(&[a, t.intrinsics.string], UnionReduction::Literal),
            t.intrinsics.string
        );
        assert_eq!(
            t.get_union_type(&[one, t.intrinsics.number], UnionReduction::Literal),
            t.intrinsics.number
        );
        // Fresh literal folds into its regular partner.
        let fresh = t.get_fresh_type_of_literal_type(one);
        assert_eq!(
            t.get_union_type(&[fresh, one], UnionReduction::Literal),
            one
        );
        // UnionReduction::None keeps the subsumed literal.
        let unreduced = t.get_union_type(&[a, t.intrinsics.string], UnionReduction::None);
        let TypeData::Union { types, .. } = &t.type_of(unreduced).data else {
            panic!("unreduced union stays a union");
        };
        assert_eq!(types.len(), 2);
    }

    #[test]
    fn union_any_unknown_absorption() {
        let mut t = tables();
        let string = t.intrinsics.string;
        assert_eq!(
            t.get_union_type(&[t.intrinsics.any, string], UnionReduction::Literal),
            t.intrinsics.any
        );
        assert_eq!(
            t.get_union_type(&[t.intrinsics.unknown, string], UnionReduction::Literal),
            t.intrinsics.unknown
        );
        assert_eq!(
            t.get_union_type(&[t.intrinsics.wildcard, string], UnionReduction::Literal),
            t.intrinsics.wildcard
        );
        assert_eq!(
            t.get_union_type(&[t.intrinsics.error, string], UnionReduction::Literal),
            t.intrinsics.error
        );
        // never members vanish.
        assert_eq!(
            t.get_union_type(&[t.intrinsics.never, string], UnionReduction::Literal),
            string
        );
    }

    #[test]
    fn union_folds_nullable_members_without_strict_null_checks() {
        let mut loose = TypeTables::new(false, false);
        let number = loose.intrinsics.number;
        let null = loose.intrinsics.null;
        // number | null collapses to number at construction (61347-61349).
        assert_eq!(
            loose.get_union_type(&[number, null], UnionReduction::Literal),
            number
        );
        // All-nullable sets fold to the (non-)widening singletons.
        assert_eq!(loose.get_union_type(&[null], UnionReduction::Literal), null);
        // A widening null plus a NON-widening undefined: the
        // IncludesNonWideningType bit is global, so the null branch
        // returns the non-widening nullType (61566-61568).
        let widening = loose.intrinsics.null_widening;
        assert_eq!(
            loose.get_union_type(
                &[widening, loose.intrinsics.undefined],
                UnionReduction::Literal
            ),
            loose.intrinsics.null
        );
        assert_eq!(
            loose.get_union_type(&[widening, widening], UnionReduction::Literal),
            widening
        );
        // Under strictNullChecks nullable members stay.
        let mut strict = tables();
        let strict_union = strict.get_union_type(
            &[strict.intrinsics.number, strict.intrinsics.null],
            UnionReduction::Literal,
        );
        assert!(strict.flags_of(strict_union).intersects(TypeFlags::UNION));
    }

    #[test]
    fn union_dedups_missing_against_undefined() {
        // exactOptionalPropertyTypes tables: undefinedOrMissing = missing.
        let mut t = TypeTables::new(true, true);
        let missing = t.intrinsics.missing;
        let undefined = t.intrinsics.undefined;
        assert_ne!(missing, undefined);
        // undefined | missing folds to undefined (61540-61544).
        assert_eq!(
            t.get_union_type(&[undefined, missing], UnionReduction::Literal),
            undefined
        );
    }

    #[test]
    fn two_union_fast_path_caches_by_reduction() {
        let mut t = tables();
        let one = t.get_number_literal_type(1.0);
        let two = t.get_number_literal_type(2.0);
        let string = t.intrinsics.string;
        let union = t.get_union_type(&[one, two], UnionReduction::Literal);
        let first = t.get_union_type(&[union, string], UnionReduction::Literal);
        let second = t.get_union_type(&[union, string], UnionReduction::Literal);
        assert_eq!(first, second);
        let reversed = t.get_union_type(&[string, union], UnionReduction::Literal);
        // Same worker result; the cache key is order-normalized.
        assert_eq!(first, reversed);
    }

    #[test]
    fn named_union_members_denormalize_into_origin() {
        let mut t = tables();
        let one = t.get_number_literal_type(1.0);
        let two = t.get_number_literal_type(2.0);
        let named = t.get_union_type(&[one, two], UnionReduction::Literal);
        // Synthesize an alias (M4 machinery) to make the union "named".
        t.type_mut(named).alias_symbol = Some(crate::ty::SymbolId(0));
        // A union containing ONLY the named union returns it unchanged.
        let string = t.intrinsics.string;
        let widened = t.get_union_type(&[named, string], UnionReduction::Literal);
        let TypeData::Union { types, origin } = &t.type_of(widened).data else {
            panic!("union expected");
        };
        // typeSet is id-sorted: the string intrinsic precedes the
        // literal types allocated by this test.
        assert_eq!(types.as_ref(), [string, one, two]);
        let origin = origin.expect("named member denormalizes into an origin");
        let TypeData::Union {
            types: origin_types,
            ..
        } = &t.type_of(origin).data
        else {
            panic!("origin is a union");
        };
        // insertType keeps id order: string (intrinsic) precedes the
        // later-allocated named union.
        assert_eq!(origin_types.as_ref(), [string, named]);
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
