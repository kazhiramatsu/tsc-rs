//! Relation engine scaffolding (m3-types-relations-steps.md stage 4.4,
//! checker-key-functions §1.5, greenfield §4.7).
//!
//! Five relations, one cache each, never shared. tsc's sixth map
//! `enumRelation` (47455) is symbol-pair keyed and consumed only by
//! isEnumTypeRelatedTo — deliberately NOT a RelationKind. Relation
//! cache keys carry NO alias context (getAliasId belongs to
//! unionOfUnionTypes keys; greenfield §4.7 misstates this — the source
//! is authoritative).
//!
//! The engine body (isTypeRelatedTo/checkTypeRelatedTo/isRelatedTo/
//! recursiveTypeRelatedTo) is stage 4.5; structuredTypeRelatedTo is
//! stage 4.6.

use std::collections::HashMap;

use tsrs2_types::{
    IntersectionState, ObjectFlags, RelationComparisonResult, TypeData, TypeFlags, TypeId,
};

use crate::state::{CheckResult2, CheckerState, Unsupported};

/// checker-key §1.5: the five relations (tsc's five checker-scope
/// relation maps at 47450-47454).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RelationKind {
    Identity,
    Subtype,
    StrictSubtype,
    Assignable,
    Comparable,
}

impl RelationKind {
    pub const ALL: [RelationKind; 5] = [
        RelationKind::Identity,
        RelationKind::Subtype,
        RelationKind::StrictSubtype,
        RelationKind::Assignable,
        RelationKind::Comparable,
    ];

    pub const fn cache_index(self) -> usize {
        match self {
            RelationKind::Identity => 0,
            RelationKind::Subtype => 1,
            RelationKind::StrictSubtype => 2,
            RelationKind::Assignable => 3,
            RelationKind::Comparable => 4,
        }
    }
}

/// One relation's verdict cache: getRelationKey string →
/// RelationComparisonResult (Succeeded/Failed + Reports*/Overflow
/// bits).
pub type RelationCache = HashMap<String, RelationComparisonResult>;

/// The per-checker relation state: `[RelCache; 5]` plus the auxiliary
/// enumRelation map (symbol-id-pair keyed, 64683).
#[derive(Debug, Default)]
pub struct RelationCaches {
    per_relation: [RelationCache; 5],
    pub enum_relation: HashMap<String, RelationComparisonResult>,
}

impl RelationCaches {
    pub fn cache(&self, relation: RelationKind) -> &RelationCache {
        &self.per_relation[relation.cache_index()]
    }

    pub fn cache_mut(&mut self, relation: RelationKind) -> &mut RelationCache {
        &mut self.per_relation[relation.cache_index()]
    }
}

impl<'a> CheckerState<'a> {
    /// tsc-port: isUnconstrainedTypeParameter @6.0.3
    /// tsc-hash: bad6eb4e0a2eee658a8d5b50043703843f725626829e75c2c1380bf0d392f281
    /// tsc-span: _tsc.js:67385-67387
    ///
    /// getConstraintOfTypeParameter reduces to the TypeParameter's
    /// stored constraint until M4 declared type parameters (M3's only
    /// type parameters are tuple-target synthetics and thisTypes).
    fn is_unconstrained_type_parameter(&self, ty: TypeId) -> bool {
        matches!(
            &self.tables.type_of(ty).data,
            TypeData::TypeParameter {
                constraint: None,
                ..
            }
        )
    }

    /// tsc-port: isNonDeferredTypeReference @6.0.3
    /// tsc-hash: cb8bb666c09074ed8ab2209f7e57402afb6f429e949578ab03331acfede9277a
    /// tsc-span: _tsc.js:67388-67390
    ///
    /// Deferred references (type.node) arrive with M4 aliases; every
    /// M3 reference is node-free.
    fn is_non_deferred_type_reference(&self, ty: TypeId) -> bool {
        self.tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
    }

    /// tsc-port: isTypeReferenceWithGenericArguments @6.0.3
    /// tsc-hash: b4b42e8e4438b8e4fc50feded93c2cd44214d3d16431695fe434401f546b4bed
    /// tsc-span: _tsc.js:67391-67393
    fn is_type_reference_with_generic_arguments(&self, ty: TypeId) -> bool {
        self.is_non_deferred_type_reference(ty)
            && self.tables.type_arguments(ty).iter().any(|&t| {
                self.tables
                    .flags_of(t)
                    .intersects(TypeFlags::TYPE_PARAMETER)
                    || self.is_type_reference_with_generic_arguments(t)
            })
    }

    /// tsc-port: getGenericTypeReferenceRelationKey @6.0.3
    /// tsc-hash: 004ad018a3c49d240c00501664ce847fc50356bd6acf296d2211bbd28ae9c683
    /// tsc-span: _tsc.js:67394-67422
    ///
    /// The `*` constraint-broadened marker and `=N` type-parameter
    /// backrefs; type-parameter indices are shared across BOTH sides.
    fn get_generic_type_reference_relation_key(
        &self,
        source: TypeId,
        target: TypeId,
        post_fix: &str,
        ignore_constraints: bool,
    ) -> String {
        let mut type_parameters: Vec<TypeId> = Vec::new();
        let mut constraint_marker = "";
        let source_id = self.get_type_reference_id(
            source,
            0,
            ignore_constraints,
            &mut type_parameters,
            &mut constraint_marker,
        );
        let target_id = self.get_type_reference_id(
            target,
            0,
            ignore_constraints,
            &mut type_parameters,
            &mut constraint_marker,
        );
        format!("{constraint_marker}{source_id},{target_id}{post_fix}")
    }

    fn get_type_reference_id(
        &self,
        ty: TypeId,
        depth: u32,
        ignore_constraints: bool,
        type_parameters: &mut Vec<TypeId>,
        constraint_marker: &mut &'static str,
    ) -> String {
        let mut result = self.tables.reference_target(ty).0.to_string();
        for &t in self.tables.type_arguments(ty) {
            if self
                .tables
                .flags_of(t)
                .intersects(TypeFlags::TYPE_PARAMETER)
            {
                if ignore_constraints || self.is_unconstrained_type_parameter(t) {
                    let index = match type_parameters.iter().position(|&p| p == t) {
                        Some(index) => index,
                        None => {
                            type_parameters.push(t);
                            type_parameters.len() - 1
                        }
                    };
                    result.push('=');
                    result.push_str(&index.to_string());
                    continue;
                }
                *constraint_marker = "*";
            } else if depth < 4 && self.is_type_reference_with_generic_arguments(t) {
                result.push('<');
                result.push_str(&self.get_type_reference_id(
                    t,
                    depth + 1,
                    ignore_constraints,
                    type_parameters,
                    constraint_marker,
                ));
                result.push('>');
                continue;
            }
            result.push('-');
            result.push_str(&t.0.to_string());
        }
        result
    }

    /// tsc-port: getRelationKey @6.0.3
    /// tsc-hash: 9b426176f192d0d2d541f7b134eedb85ed04d05e388e762b401df39201e6f9e0
    /// tsc-span: _tsc.js:67423-67431
    ///
    /// Ids swap so the smaller comes first for the IDENTITY relation
    /// only; `:intersectionState` suffix when nonzero; NO alias
    /// context.
    pub fn get_relation_key(
        &self,
        source: TypeId,
        target: TypeId,
        intersection_state: IntersectionState,
        relation: RelationKind,
        ignore_constraints: bool,
    ) -> String {
        let (source, target) = if relation == RelationKind::Identity && source.0 > target.0 {
            (target, source)
        } else {
            (source, target)
        };
        let post_fix = if intersection_state.bits() != 0 {
            format!(":{}", intersection_state.bits())
        } else {
            String::new()
        };
        if self.is_type_reference_with_generic_arguments(source)
            && self.is_type_reference_with_generic_arguments(target)
        {
            return self.get_generic_type_reference_relation_key(
                source,
                target,
                &post_fix,
                ignore_constraints,
            );
        }
        format!("{},{}{post_fix}", source.0, target.0)
    }

    /// isEnumTypeRelatedTo (64673) — STUB until M4 5.3b: enum declared
    /// types (getTypeOfSymbol for enums, getEnumMemberValue) are
    /// unconstructible in M3, so the isSimpleTypeRelatedTo enum arms
    /// can never produce symbols to compare. The enumRelation cache
    /// (RelationCaches::enum_relation, keyed
    /// `getSymbolId(source),getSymbolId(target)`) is ready for the
    /// port; enum pins arrive with M4.
    pub fn is_enum_type_related_to(
        &mut self,
        _source: tsrs2_binder::SymbolId,
        _target: tsrs2_binder::SymbolId,
    ) -> CheckResult2<bool> {
        Err(Unsupported::new("enum compatibility (M4 5.3b)"))
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_binder::bind_source_file;
    use tsrs2_syntax::{parse_source_file, LanguageVariant, ParseOptions};
    use tsrs2_types::{CompilerOptions, ElementFlags, IntersectionState, RelationComparisonResult};

    use super::{RelationCaches, RelationKind};
    use crate::state::CheckerState;

    fn with_state<R>(run: impl FnOnce(&mut CheckerState) -> R) -> R {
        let options = CompilerOptions::default();
        let source = parse_source_file(
            "relate-test.ts".to_owned(),
            String::new(),
            ParseOptions {
                language_variant: LanguageVariant::Standard,
                javascript_file: false,
                ..ParseOptions::default()
            },
            None,
        );
        let binder = bind_source_file(&source, &options);
        let mut state = CheckerState::new(&source, binder, &options);
        run(&mut state)
    }

    #[test]
    fn relation_caches_are_per_relation() {
        let mut caches = RelationCaches::default();
        caches
            .cache_mut(RelationKind::Assignable)
            .insert("1,2".to_owned(), RelationComparisonResult::SUCCEEDED);
        assert!(caches.cache(RelationKind::Assignable).contains_key("1,2"));
        for relation in RelationKind::ALL {
            if relation != RelationKind::Assignable {
                assert!(
                    !caches.cache(relation).contains_key("1,2"),
                    "{relation:?} must not share the assignable cache"
                );
            }
        }
    }

    #[test]
    fn relation_keys_swap_ids_for_identity_only() {
        with_state(|state| {
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let (small, large) = if string.0 < number.0 {
                (string, number)
            } else {
                (number, string)
            };
            let identity = state.get_relation_key(
                large,
                small,
                IntersectionState::NONE,
                RelationKind::Identity,
                false,
            );
            assert_eq!(identity, format!("{},{}", small.0, large.0));
            let assignable = state.get_relation_key(
                large,
                small,
                IntersectionState::NONE,
                RelationKind::Assignable,
                false,
            );
            assert_eq!(assignable, format!("{},{}", large.0, small.0));
            let suffixed = state.get_relation_key(
                small,
                large,
                IntersectionState::TARGET,
                RelationKind::Assignable,
                false,
            );
            assert_eq!(suffixed, format!("{},{}:2", small.0, large.0));
        });
    }

    #[test]
    fn generic_reference_keys_use_backrefs() {
        with_state(|state| {
            // A tuple TARGET is a self-reference whose arguments are
            // its synthesized (unconstrained) type parameters — the
            // one M3-constructible generic-reference shape.
            let target = state
                .tables
                .get_tuple_target_type(&[ElementFlags::REQUIRED, ElementFlags::OPTIONAL], false)
                .expect("tuple target");
            let key = state.get_relation_key(
                target,
                target,
                IntersectionState::NONE,
                RelationKind::Assignable,
                false,
            );
            // Shared type-parameter indices across both sides.
            assert_eq!(key, format!("{}=0=1,{}=0=1", target.0, target.0));
            // A concrete tuple reference is NOT a generic reference:
            // plain id-pair key.
            let number = state.tables.intrinsics.number;
            let string = state.tables.intrinsics.string;
            let concrete = state
                .tables
                .create_normalized_type_reference(target, &[number, string])
                .expect("tuple reference");
            let key = state.get_relation_key(
                concrete,
                concrete,
                IntersectionState::NONE,
                RelationKind::Assignable,
                false,
            );
            assert_eq!(key, format!("{},{}", concrete.0, concrete.0));
        });
    }

    #[test]
    fn enum_relation_is_a_stubbed_sixth_map() {
        with_state(|state| {
            let symbol = tsrs2_binder::SymbolId(0);
            let err = state
                .is_enum_type_related_to(symbol, symbol)
                .expect_err("enum compatibility is an M4 row");
            assert!(err.reason.contains("M4"));
        });
    }
}
