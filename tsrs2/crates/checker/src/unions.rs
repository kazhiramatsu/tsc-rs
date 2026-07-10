//! Subtype/StrictSubtype activation (m3-types-relations-steps.md stage
//! 4.8): the Subtype-capable getUnionType twin with removeSubtypes,
//! the template-literal string reduction that needed the engine, and
//! getCommonSupertype.
//!
//! Twin rule (INVARIANT): every union built from CHECKER code — type
//! nodes (annotate), property synthesis (structural), cross-product
//! distribution (intersect), literal base-typing (engine) — routes
//! through get_union_type_ex/get_union_type_ex_with_origin here, so
//! Subtype reduction and the string-literal ∪ template reduction can
//! fire. The tables-side get_union_type is ONLY for unions built
//! inside pure tables constructors. Residual (ledgered in tables):
//! those tables-internal unions — template-literal distribution and
//! the tuple rest-window — skip the string-literal-vs-template
//! reduction until the constructors move checker-side with M4.

use tsrs2_types::{TypeData, TypeFlags, TypeId, UnionReduction};

use crate::relate::RelationKind;
use crate::state::{CheckResult2, CheckerState, Unsupported};

impl<'a> CheckerState<'a> {
    /// tsc-port: isTypeSubtypeOf @6.0.3
    /// tsc-hash: 6c987b105e7c93ba9a28ac8aeb98020ec479f32b55236d7c3f346af47dd04617
    /// tsc-span: _tsc.js:63913-63915
    pub fn is_type_subtype_of(&mut self, source: TypeId, target: TypeId) -> CheckResult2<bool> {
        self.is_type_related_to(source, target, RelationKind::Subtype)
    }

    /// tsc-port: isTypeStrictSubtypeOf @6.0.3
    /// tsc-hash: 05eb05f56c80a831b621a9d088f260f0fb71ec0d1c13ea6a0186bf5082ca63eb
    /// tsc-span: _tsc.js:63916-63918
    pub fn is_type_strict_subtype_of(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult2<bool> {
        self.is_type_related_to(source, target, RelationKind::StrictSubtype)
    }

    /// tsc-port: getUnionType @6.0.3
    /// tsc-hash: c0f3627f0a6e1cabf66d5b8cc24eabef75b60fe2d963fad1203f40d2543baf83
    /// tsc-span: _tsc.js:61505-61531
    ///
    /// tsc-port: getUnionTypeWorker @6.0.3
    /// tsc-hash: 93f55d81bb79032838d9e61c845728d71878ed18b25fb3c0463b2fc0aae692a1
    /// tsc-span: _tsc.js:61532-61585
    ///
    /// The checker-side twin: identical to the tables worker plus the
    /// engine-dependent reductions — UnionReduction::Subtype runs
    /// removeSubtypes (61553-61558; None on overflow → errorType) and
    /// the string-literal ∪ template mix runs
    /// removeStringLiteralsMatchedByTemplateLiterals (61547-61549).
    /// removeConstrainedTypeVariables stays unreachable until M4.
    pub fn get_union_type_ex(
        &mut self,
        types: &[TypeId],
        reduction: UnionReduction,
    ) -> CheckResult2<TypeId> {
        self.get_union_type_ex_with_origin(types, reduction, None)
    }

    /// The origin-carrying entry (the `origin` parameter at 61505);
    /// the fast-path cache is bypassed when an origin rides along,
    /// exactly as tsc's `!origin` guard does.
    pub fn get_union_type_ex_with_origin(
        &mut self,
        types: &[TypeId],
        reduction: UnionReduction,
        origin: Option<TypeId>,
    ) -> CheckResult2<TypeId> {
        if types.is_empty() {
            return Ok(self.tables.intrinsics.never);
        }
        if types.len() == 1 {
            return Ok(types[0]);
        }
        if types.len() == 2
            && origin.is_none()
            && (self.tables.flags_of(types[0]).intersects(TypeFlags::UNION)
                || self.tables.flags_of(types[1]).intersects(TypeFlags::UNION))
        {
            let infix = match reduction {
                UnionReduction::None => "N",
                UnionReduction::Subtype => "S",
                UnionReduction::Literal => "L",
            };
            let index = usize::from(types[0].0 >= types[1].0);
            let key = format!("{}{infix}{}", types[index].0, types[1 - index].0);
            if let Some(id) = self.tables.union_of_union_types_get(&key) {
                return Ok(id);
            }
            let id = self.get_union_type_ex_worker(types, reduction, None)?;
            self.tables.union_of_union_types_insert(key, id);
            return Ok(id);
        }
        self.get_union_type_ex_worker(types, reduction, origin)
    }

    fn get_union_type_ex_worker(
        &mut self,
        types: &[TypeId],
        reduction: UnionReduction,
        origin: Option<TypeId>,
    ) -> CheckResult2<TypeId> {
        let mut type_set: Vec<TypeId> = Vec::new();
        let includes = self.tables.add_types_to_union(&mut type_set, 0, types);
        if reduction != UnionReduction::None {
            if includes & TypeFlags::ANY_OR_UNKNOWN.bits() != 0 {
                return Ok(if includes & TypeFlags::ANY.bits() != 0 {
                    if includes & TypeFlags::INCLUDES_WILDCARD.bits() != 0 {
                        self.tables.intrinsics.wildcard
                    } else if includes & TypeFlags::INCLUDES_ERROR.bits() != 0 {
                        self.tables.intrinsics.error
                    } else {
                        self.tables.intrinsics.any
                    }
                } else {
                    self.tables.intrinsics.unknown
                });
            }
            if includes & TypeFlags::UNDEFINED.bits() != 0
                && type_set.len() >= 2
                && type_set[0] == self.tables.intrinsics.undefined
                && type_set[1] == self.tables.intrinsics.missing
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
                self.tables.remove_redundant_literal_types(
                    &mut type_set,
                    includes,
                    /*reduce_void_undefined*/ reduction == UnionReduction::Subtype,
                );
            }
            if includes & TypeFlags::STRING_LITERAL.bits() != 0
                && includes
                    & (TypeFlags::TEMPLATE_LITERAL.bits() | TypeFlags::STRING_MAPPING.bits())
                    != 0
            {
                self.remove_string_literals_matched_by_template_literals(&mut type_set)?;
            }
            if includes & TypeFlags::INCLUDES_CONSTRAINED_TYPE_VARIABLE.bits() != 0 {
                unreachable!(
                    "IsConstrainedTypeVariable intersections are unconstructible before M4"
                );
            }
            if reduction == UnionReduction::Subtype {
                let Some(reduced) =
                    self.remove_subtypes(type_set, includes & TypeFlags::OBJECT.bits() != 0)?
                else {
                    return Ok(self.tables.intrinsics.error);
                };
                type_set = reduced;
            }
            if type_set.is_empty() {
                return Ok(if includes & TypeFlags::NULL.bits() != 0 {
                    if includes & TypeFlags::INCLUDES_NON_WIDENING_TYPE.bits() != 0 {
                        self.tables.intrinsics.null
                    } else {
                        self.tables.intrinsics.null_widening
                    }
                } else if includes & TypeFlags::UNDEFINED.bits() != 0 {
                    if includes & TypeFlags::INCLUDES_NON_WIDENING_TYPE.bits() != 0 {
                        self.tables.intrinsics.undefined
                    } else {
                        self.tables.intrinsics.undefined_widening
                    }
                } else {
                    self.tables.intrinsics.never
                });
            }
        }
        Ok(self
            .tables
            .finish_union_type_set(type_set, includes, types, origin))
    }

    /// tsc-port: removeStringLiteralsMatchedByTemplateLiterals @6.0.3
    /// tsc-hash: 192c4f18f0b5a7c1addfb4212dc8d8154dff602a659a4841afc5dcf1d08306ea
    /// tsc-span: _tsc.js:61434-61446
    ///
    /// tsc-port: isTypeMatchedByTemplateLiteralOrStringMapping @6.0.3
    /// tsc-hash: 17932b7f92e200b7fde380e72dfcd2adb1dcd7b62e47544505c9153217a00149
    /// tsc-span: _tsc.js:61447-61449
    ///
    /// The StringMapping arm is dead until M4 intrinsic aliases.
    fn remove_string_literals_matched_by_template_literals(
        &mut self,
        types: &mut Vec<TypeId>,
    ) -> CheckResult2<()> {
        let templates: Vec<TypeId> = types
            .iter()
            .copied()
            .filter(|&t| self.tables.is_pattern_literal_type(t))
            .collect();
        if templates.is_empty() {
            return Ok(());
        }
        let mut i = types.len();
        while i > 0 {
            i -= 1;
            let t = types[i];
            if !self
                .tables
                .flags_of(t)
                .intersects(TypeFlags::STRING_LITERAL)
            {
                continue;
            }
            let mut matched = false;
            for &template in &templates {
                if self
                    .tables
                    .flags_of(template)
                    .intersects(TypeFlags::STRING_MAPPING)
                {
                    return Err(Unsupported::new("string-mapping members (M4 5.2)"));
                }
                if self.is_type_matched_by_template_literal_type(t, template)? {
                    matched = true;
                    break;
                }
            }
            if matched {
                types.remove(i);
            }
        }
        Ok(())
    }

    /// tsc-port: removeSubtypes @6.0.3
    /// tsc-hash: 9508e141ff54bc861e94ce761bb80ab314208438926a55dd5e018d722da6929a
    /// tsc-span: _tsc.js:61368-61421
    ///
    /// Returns None on the complexity overflow (the caller yields
    /// errorType; the 2799-family diagnostic is deferred with error
    /// reporting). M4 rows report Unsupported: type parameters with
    /// union constraints, class-derivation checks (getTargetType/
    /// isTypeDerivedFrom never fire — no class references exist).
    fn remove_subtypes(
        &mut self,
        mut types: Vec<TypeId>,
        has_object_types: bool,
    ) -> CheckResult2<Option<Vec<TypeId>>> {
        if types.len() < 2 {
            return Ok(Some(types));
        }
        let id = self.tables.get_type_list_id(&types);
        if let Some(cached) = self.subtype_reduction_cache.get(&id) {
            return Ok(Some(cached.clone()));
        }
        let mut has_empty_object = false;
        if has_object_types {
            for &t in &types {
                if self.tables.flags_of(t).intersects(TypeFlags::OBJECT)
                    && !self.tables.is_tuple_type(t)
                {
                    let members = self.resolve_structured_type_members(t)?;
                    let resolved = self.members_of(members);
                    if resolved.properties.is_empty()
                        && resolved.call_signatures.is_empty()
                        && resolved.construct_signatures.is_empty()
                        && resolved.index_infos.is_empty()
                    {
                        has_empty_object = true;
                        break;
                    }
                }
            }
        }
        let len = types.len();
        let mut i = len;
        let mut count = 0usize;
        while i > 0 {
            i -= 1;
            let source = types[i];
            let source_flags = self.tables.flags_of(source);
            if has_empty_object || source_flags.intersects(TypeFlags::STRUCTURED_OR_INSTANTIABLE) {
                if source_flags.intersects(TypeFlags::TYPE_PARAMETER) {
                    return Err(Unsupported::new(
                        "type-parameter subtype reduction (M4 5.1)",
                    ));
                }
                // The key-property fast filter (61389-61391).
                let key_property = if source_flags.intersects(TypeFlags::from_bits(
                    TypeFlags::OBJECT.bits()
                        | TypeFlags::INTERSECTION.bits()
                        | TypeFlags::INSTANTIABLE_NON_PRIMITIVE.bits(),
                )) {
                    let mut found = None;
                    for prop in self.get_properties_of_type(source)? {
                        let prop_type = self.get_type_of_symbol(prop)?;
                        if self.is_unit_type(prop_type) {
                            let name = self.binder.symbols.symbol(prop).escaped_name.clone();
                            let regular = self.tables.get_regular_type_of_literal_type(prop_type);
                            found = Some((name, regular));
                            break;
                        }
                    }
                    found
                } else {
                    None
                };
                for j in 0..types.len() {
                    let target = types[j];
                    if source == target {
                        continue;
                    }
                    if count == 100_000 {
                        let estimated = count / (len - i) * len;
                        if estimated > 1_000_000 {
                            return Ok(None);
                        }
                    }
                    count += 1;
                    if let Some((key_name, key_type)) = &key_property {
                        if self
                            .tables
                            .flags_of(target)
                            .intersects(TypeFlags::from_bits(
                                TypeFlags::OBJECT.bits()
                                    | TypeFlags::INTERSECTION.bits()
                                    | TypeFlags::INSTANTIABLE_NON_PRIMITIVE.bits(),
                            ))
                        {
                            if let Some(t) = self.get_type_of_property_of_type(target, key_name)? {
                                if self.is_unit_type(t)
                                    && self.tables.get_regular_type_of_literal_type(t) != *key_type
                                {
                                    continue;
                                }
                            }
                        }
                    }
                    if self.is_type_strict_subtype_of(source, target)? {
                        // Class-derivation exception (61407): class
                        // references are unconstructible before M4, so
                        // the guard passes.
                        types.remove(i);
                        break;
                    }
                }
            }
        }
        self.subtype_reduction_cache.insert(id, types.clone());
        Ok(Some(types))
    }

    /// tsc-port: getCommonSupertype @6.0.3
    /// tsc-hash: b138ffd79908d0133ab11dba75b0c50b41289c986f217e206cec63b98e3b5e42
    /// tsc-span: _tsc.js:67650-67657
    ///
    /// tsc-port: getSingleCommonSupertype @6.0.3
    /// tsc-hash: 26a0b52f2071b024b2973f931cacb18f3142ce7707437adff36dcbe288b3b981
    /// tsc-span: _tsc.js:67658-67661
    ///
    /// Consumed by widening JOINs (M6); ported with the stage per the
    /// steps doc.
    pub fn get_common_supertype(&mut self, types: &[TypeId]) -> CheckResult2<TypeId> {
        if types.len() == 1 {
            return Ok(types[0]);
        }
        let primary_types: Vec<TypeId> = if self.tables.strict_null_checks {
            types
                .iter()
                .map(|&t| {
                    self.tables.filter_type(t, |tables, u| {
                        !tables.flags_of(u).intersects(TypeFlags::NULLABLE)
                    })
                })
                .collect()
        } else {
            types.to_vec()
        };
        let supertype_or_union = if self.literal_types_with_same_base_type(&primary_types)? {
            self.get_union_type_ex(&primary_types, UnionReduction::Literal)?
        } else {
            self.get_single_common_supertype(&primary_types)?
        };
        if primary_types == types {
            return Ok(supertype_or_union);
        }
        let combined = self.get_combined_type_flags(types);
        Ok(self.get_nullable_type(supertype_or_union, combined & TypeFlags::NULLABLE.bits()))
    }

    fn get_single_common_supertype(&mut self, types: &[TypeId]) -> CheckResult2<TypeId> {
        let mut candidate = types[0];
        for &t in &types[1..] {
            if self.is_type_strict_subtype_of(candidate, t)? {
                candidate = t;
            }
        }
        let mut all = true;
        for &t in types {
            if t != candidate && !self.is_type_strict_subtype_of(t, candidate)? {
                all = false;
                break;
            }
        }
        if all {
            return Ok(candidate);
        }
        let mut fallback = types[0];
        for &t in &types[1..] {
            if self.is_type_subtype_of(fallback, t)? {
                fallback = t;
            }
        }
        Ok(fallback)
    }

    /// tsc-port: literalTypesWithSameBaseType @6.0.3
    /// tsc-hash: 64a3b81252aff06adb46f09200c9ee8fe4c9ed322791a89307acaa682bd387db
    /// tsc-span: _tsc.js:67634-67646
    fn literal_types_with_same_base_type(&mut self, types: &[TypeId]) -> CheckResult2<bool> {
        let mut common_base_type: Option<TypeId> = None;
        for &t in types {
            if self.tables.flags_of(t).intersects(TypeFlags::NEVER) {
                continue;
            }
            let base_type = self.get_base_type_of_literal_type(t)?;
            let common = *common_base_type.get_or_insert(base_type);
            if base_type == t || base_type != common {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// tsc-port: getCombinedTypeFlags @6.0.3
    /// tsc-hash: 024c9921958776863a186687a3e96eea4b3f31b0e0bae5ea5754b1f5a0b29ebc
    /// tsc-span: _tsc.js:67647-67649
    fn get_combined_type_flags(&self, types: &[TypeId]) -> i32 {
        let mut flags = 0i32;
        for &t in types {
            if self.tables.flags_of(t).intersects(TypeFlags::UNION) {
                if let TypeData::Union { types: members, .. } = self.tables.type_of(t).data.clone()
                {
                    flags |= self.get_combined_type_flags(&members);
                    continue;
                }
            }
            flags |= self.tables.flags_of(t).bits();
        }
        flags
    }

    /// tsc-port: getNullableType @6.0.3
    /// tsc-hash: e51ab12c29ed98598d2d368c3bd905b09fb8e31de0a718389a894ad5d7677537
    /// tsc-span: _tsc.js:67848-67851
    fn get_nullable_type(&mut self, ty: TypeId, flags: i32) -> TypeId {
        let missing = flags
            & !self.tables.flags_of(ty).bits()
            & (TypeFlags::UNDEFINED.bits() | TypeFlags::NULL.bits());
        if missing == 0 {
            return ty;
        }
        let undefined = self.tables.intrinsics.undefined;
        let null = self.tables.intrinsics.null;
        let members: Vec<TypeId> = if missing == TypeFlags::UNDEFINED.bits() {
            vec![ty, undefined]
        } else if missing == TypeFlags::NULL.bits() {
            vec![ty, null]
        } else {
            vec![ty, undefined, null]
        };
        self.tables
            .get_union_type(&members, UnionReduction::Literal)
    }

    /// tsc-port: extractRedundantTemplateLiterals @6.0.3
    /// tsc-hash: f65b255378da2d54bbb8d9ac96a51ec943346908d958d385961ee52c9d563b1c
    /// tsc-span: _tsc.js:61714-61731
    ///
    /// Returns true when the intersection collapses to never (a
    /// pattern-literal template with no matching string literal). The
    /// StringMapping arm is dead until M4.
    pub fn extract_redundant_template_literals(
        &mut self,
        types: &mut Vec<TypeId>,
    ) -> CheckResult2<bool> {
        let literals: Vec<TypeId> = types
            .iter()
            .copied()
            .filter(|&t| {
                self.tables
                    .flags_of(t)
                    .intersects(TypeFlags::STRING_LITERAL)
            })
            .collect();
        let mut i = types.len();
        while i > 0 {
            i -= 1;
            let t = types[i];
            if !self.tables.flags_of(t).intersects(TypeFlags::from_bits(
                TypeFlags::TEMPLATE_LITERAL.bits() | TypeFlags::STRING_MAPPING.bits(),
            )) {
                continue;
            }
            if self
                .tables
                .flags_of(t)
                .intersects(TypeFlags::STRING_MAPPING)
            {
                return Err(Unsupported::new("string-mapping members (M4 5.2)"));
            }
            for &literal in &literals {
                if self.is_type_subtype_of(literal, t)? {
                    types.remove(i);
                    break;
                } else if self.tables.is_pattern_literal_type(t) {
                    // tsc bails on the FIRST non-matching literal when
                    // the template is a pattern literal (61727-61728).
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_binder::bind_source_file;
    use tsrs2_syntax::{parse_source_file, LanguageVariant, ParseOptions};
    use tsrs2_types::{CompilerOptions, TypeData, TypeFlags, UnionReduction};

    use crate::relpin::find_probe_annotation;
    use crate::relpin::{probe_relation, RelpinQuery, RelpinRelation, RelpinVerdict};
    use crate::state::CheckerState;

    fn with_state<R>(text: &str, run: impl FnOnce(&mut CheckerState) -> R) -> R {
        let options = CompilerOptions::default();
        let source = parse_source_file(
            "unions-test.ts".to_owned(),
            text.to_owned(),
            ParseOptions {
                language_variant: LanguageVariant::Standard,
                javascript_file: false,
            },
            None,
        );
        assert!(source.parse_diagnostics.is_empty());
        let binder = bind_source_file(&source, &options);
        let mut state = CheckerState::new(&source, binder, &options);
        run(&mut state)
    }

    fn annotation(state: &mut CheckerState, name: &str) -> tsrs2_types::TypeId {
        let node = find_probe_annotation(state.source, name).expect("annotation");
        state.get_type_from_type_node(node).expect("resolves")
    }

    #[test]
    fn subtype_reduction_drops_strict_subtypes() {
        with_state(
            "declare var a: { a: number, b: string };\ndeclare var b: { a: number };\n",
            |state| {
                let literal_a = state.tables.get_string_literal_type("a");
                let string = state.tables.intrinsics.string;
                // "a" is a strict subtype of string: Subtype reduction
                // collapses to string (Literal reduction already does
                // via removeRedundantLiteralTypes — exercise the
                // object case for removeSubtypes proper).
                assert_eq!(
                    state
                        .get_union_type_ex(&[literal_a, string], UnionReduction::Subtype)
                        .expect("reduces"),
                    string
                );
                let wide = annotation(state, "a");
                let narrow = annotation(state, "b");
                // { a, b } is a strict subtype of { a }: the union
                // subtype-reduces to { a } alone.
                assert_eq!(
                    state
                        .get_union_type_ex(&[wide, narrow], UnionReduction::Subtype)
                        .expect("reduces"),
                    narrow
                );
                // Literal reduction keeps both members.
                let unreduced = state
                    .get_union_type_ex(&[wide, narrow], UnionReduction::Literal)
                    .expect("constructs");
                assert!(state
                    .tables
                    .flags_of(unreduced)
                    .intersects(TypeFlags::UNION));
                // reduceVoidUndefined: undefined folds into void under
                // Subtype reduction only.
                let void = state.tables.intrinsics.void;
                let undefined = state.tables.intrinsics.undefined;
                assert_eq!(
                    state
                        .get_union_type_ex(&[void, undefined], UnionReduction::Subtype)
                        .expect("reduces"),
                    void
                );
                let kept = state
                    .get_union_type_ex(&[void, undefined], UnionReduction::Literal)
                    .expect("constructs");
                assert!(state.tables.flags_of(kept).intersects(TypeFlags::UNION));
            },
        );
    }

    #[test]
    fn checker_side_unions_reduce_template_matched_string_literals() {
        with_state("declare var t: `a${string}`;\n", |state| {
            let template = annotation(state, "t");
            let abc = state.tables.get_string_literal_type("abc");
            let xyz = state.tables.get_string_literal_type("xyz");
            // "abc" matches `a${string}` and is absorbed; "xyz" does
            // not and survives.
            assert_eq!(
                state
                    .get_union_type_ex(&[abc, template], UnionReduction::Literal)
                    .expect("reduces"),
                template
            );
            let mixed = state
                .get_union_type_ex(&[xyz, template], UnionReduction::Literal)
                .expect("constructs");
            let TypeData::Union { types, .. } = &state.tables.type_of(mixed).data else {
                panic!("unmatched literal survives the union");
            };
            assert_eq!(types.len(), 2);
        });
    }

    #[test]
    fn intersections_extract_redundant_template_literals() {
        // Matching literal absorbs the template; a pattern template
        // with a non-matching literal collapses to never.
        assert!(matches!(
            probe_relation(&RelpinQuery {
                setup: "",
                source: "\"abc\" & `a${string}`",
                target: "\"abc\"",
                source_is_fresh: false,
                relation: RelpinRelation::Assignable,
                options: &CompilerOptions::default(),
            }),
            RelpinVerdict::Related
        ));
        assert!(matches!(
            probe_relation(&RelpinQuery {
                setup: "",
                source: "\"xyz\" & `a${string}`",
                target: "never",
                source_is_fresh: false,
                relation: RelpinRelation::Assignable,
                options: &CompilerOptions::default(),
            }),
            RelpinVerdict::Related
        ));
    }

    #[test]
    fn common_supertype_prefers_single_supertypes_over_unions() {
        with_state("declare var u: string | null;\n", |state| {
            let a = state.tables.get_string_literal_type("a");
            let b = state.tables.get_string_literal_type("b");
            let string = state.tables.intrinsics.string;
            // Same-base literals join as a union.
            let union = state.get_common_supertype(&[a, b]).expect("join");
            assert!(state.tables.flags_of(union).intersects(TypeFlags::UNION));
            // A strict supertype in the set wins.
            assert_eq!(
                state.get_common_supertype(&[a, string]).expect("join"),
                string
            );
            // Nullable members re-add their nullability after the join
            // (strictNullChecks default-on).
            let string_or_null = annotation(state, "u");
            let joined = state
                .get_common_supertype(&[string_or_null, a])
                .expect("join");
            let TypeData::Union { types, .. } = &state.tables.type_of(joined).data else {
                panic!("nullable join is a union");
            };
            assert!(types.contains(&state.tables.intrinsics.null));
            assert!(types.contains(&string));
        });
    }
}
