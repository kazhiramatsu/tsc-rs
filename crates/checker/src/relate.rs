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

use tsc_binder::SymbolId;
use tsc_syntax::SyntaxKind;
use tsc_types::{
    IntersectionState, ObjectFlags, RelationComparisonResult, SymbolFlags, TypeFlags, TypeId,
};

use crate::evaluate::EvalValue;
use crate::state::{CheckResult, CheckerState};

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

    /// tsrs-native: closed-enum index used by Rust's RelationCaches
    /// fixed array; tsc closes over distinct cache variables.
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

/// Rust-owned result of tsc's enum-relation `errorReporter` callback.
///
/// The verdict cache stores only success/failure. A reporting relation that
/// observes a cached failure deliberately recomputes the comparison and
/// carries one of these typed reasons back to the relation error stack.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EnumRelationError {
    MissingMember {
        source_member: SymbolId,
        target_enum: SymbolId,
    },
    MismatchedMemberValue {
        target_enum: SymbolId,
        target_member: SymbolId,
        expected: EvalValue,
        given: EvalValue,
    },
    StringVsUnknownNumeric {
        target_enum: SymbolId,
        target_member: SymbolId,
        known_string: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EnumRelationOutcome {
    Related,
    Unrelated(Option<EnumRelationError>),
}

impl EnumRelationOutcome {
    /// tsrs-native: closed-enum verdict projection for the typed replacement
    /// of tsc's boolean-plus-errorReporter result.
    pub(crate) const fn is_related(&self) -> bool {
        matches!(self, Self::Related)
    }
}

impl RelationCaches {
    /// tsrs-native: Rust fixed-array accessor for tsc's separate
    /// relation cache objects.
    pub fn cache(&self, relation: RelationKind) -> &RelationCache {
        &self.per_relation[relation.cache_index()]
    }

    /// tsrs-native: mutable Rust fixed-array accessor for tsc's
    /// separate relation cache objects.
    pub fn cache_mut(&mut self, relation: RelationKind) -> &mut RelationCache {
        &mut self.per_relation[relation.cache_index()]
    }
}

impl<'a> CheckerState<'a> {
    /// tsc-port: isUnconstrainedTypeParameter @6.0.3
    /// tsc-hash: bad6eb4e0a2eee658a8d5b50043703843f725626829e75c2c1380bf0d392f281
    /// tsc-span: _tsc.js:67385-67387
    ///
    /// getConstraintOfTypeParameter FORCES constraint resolution
    /// (declared parameters keep theirs in the lazy links slot; the
    /// inline TypeData field belongs to tables-synthesized tuple
    /// parameters only), so the whole relation-key computation is
    /// fallible and `&mut`. A non-forcing links read would make the
    /// key depend on resolution order.
    fn is_unconstrained_type_parameter(&mut self, ty: TypeId) -> CheckResult<bool> {
        Ok(self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::TYPE_PARAMETER)
            && self.get_constraint_of_type_parameter(ty)?.is_none())
    }

    /// tsc-port: isNonDeferredTypeReference @6.0.3
    /// tsc-hash: cb8bb666c09074ed8ab2209f7e57402afb6f429e949578ab03331acfede9277a
    /// tsc-span: _tsc.js:67388-67390
    ///
    /// `!type.node` — a deferred reference stays "deferred" here even
    /// after its arguments resolve (the node marker never clears), so
    /// the resolved-arguments read below it never sees a vacant slot.
    fn is_non_deferred_type_reference(&self, ty: TypeId) -> bool {
        self.tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
            && self.links.ty(ty).deferred_node.is_none()
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
        &mut self,
        source: TypeId,
        target: TypeId,
        post_fix: &str,
        ignore_constraints: bool,
    ) -> CheckResult<String> {
        let mut type_parameters: Vec<TypeId> = Vec::new();
        let mut constraint_marker = "";
        let source_id = self.get_type_reference_id(
            source,
            0,
            ignore_constraints,
            &mut type_parameters,
            &mut constraint_marker,
        )?;
        let target_id = self.get_type_reference_id(
            target,
            0,
            ignore_constraints,
            &mut type_parameters,
            &mut constraint_marker,
        )?;
        Ok(format!(
            "{constraint_marker}{source_id},{target_id}{post_fix}"
        ))
    }

    fn get_type_reference_id(
        &mut self,
        ty: TypeId,
        depth: u32,
        ignore_constraints: bool,
        type_parameters: &mut Vec<TypeId>,
        constraint_marker: &mut &'static str,
    ) -> CheckResult<String> {
        let mut result = self.tables.reference_target(ty).0.to_string();
        let arguments = self.tables.type_arguments(ty).to_vec();
        for t in arguments {
            if self
                .tables
                .flags_of(t)
                .intersects(TypeFlags::TYPE_PARAMETER)
            {
                if ignore_constraints || self.is_unconstrained_type_parameter(t)? {
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
                )?);
                result.push('>');
                continue;
            }
            result.push('-');
            result.push_str(&t.0.to_string());
        }
        Ok(result)
    }

    /// tsc-port: getRelationKey @6.0.3
    /// tsc-hash: 9b426176f192d0d2d541f7b134eedb85ed04d05e388e762b401df39201e6f9e0
    /// tsc-span: _tsc.js:67423-67431
    ///
    /// Ids swap so the smaller comes first for the IDENTITY relation
    /// only; `:intersectionState` suffix when nonzero; NO alias
    /// context.
    pub fn get_relation_key(
        &mut self,
        source: TypeId,
        target: TypeId,
        intersection_state: IntersectionState,
        relation: RelationKind,
        ignore_constraints: bool,
    ) -> CheckResult<String> {
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
        Ok(format!("{},{}{post_fix}", source.0, target.0))
    }

    /// tsc-port: isEnumTypeRelatedTo @6.0.3
    /// tsc-hash: 253c223b70908f75bb9e3be5803ad582f0412432e034ab00e01cae4a891a9dce
    /// tsc-span: _tsc.js:64673-64732
    ///
    /// Boolean callers use the cached verdict directly. Reporting callers
    /// request a typed error and therefore replay cached failures, matching
    /// tsc's `entry & Failed && errorReporter` guard without retaining a
    /// callback borrow across checker operations.
    pub fn is_enum_type_related_to(
        &mut self,
        source: SymbolId,
        target: SymbolId,
    ) -> CheckResult<bool> {
        Ok(self
            .enum_type_relation(source, target, /*collect_error*/ false)?
            .is_related())
    }

    /// tsrs-native: typed outcome/cache worker behind the pinned
    /// `isEnumTypeRelatedTo` port, replacing its callback borrow with an
    /// owned `EnumRelationError` while retaining cached-verdict replay.
    pub(crate) fn enum_type_relation(
        &mut self,
        source: SymbolId,
        target: SymbolId,
        collect_error: bool,
    ) -> CheckResult<EnumRelationOutcome> {
        let source_symbol = if self
            .binder
            .symbol(source)
            .flags
            .intersects(SymbolFlags::ENUM_MEMBER)
        {
            self.get_parent_of_symbol(source)
                .expect("enum member symbols have enum parents")
        } else {
            source
        };
        let target_symbol = if self
            .binder
            .symbol(target)
            .flags
            .intersects(SymbolFlags::ENUM_MEMBER)
        {
            self.get_parent_of_symbol(target)
                .expect("enum member symbols have enum parents")
        } else {
            target
        };
        if source_symbol == target_symbol {
            return Ok(EnumRelationOutcome::Related);
        }
        {
            let source_data = self.binder.symbol(source_symbol);
            let target_data = self.binder.symbol(target_symbol);
            if source_data.escaped_name != target_data.escaped_name
                || !source_data.flags.intersects(SymbolFlags::REGULAR_ENUM)
                || !target_data.flags.intersects(SymbolFlags::REGULAR_ENUM)
            {
                return Ok(EnumRelationOutcome::Unrelated(None));
            }
        }
        let id = format!("{},{}", source_symbol.0, target_symbol.0);
        if let Some(&entry) = self.relations.enum_relation.get(&id) {
            if !(collect_error && entry.intersects(RelationComparisonResult::FAILED)) {
                return Ok(if entry.intersects(RelationComparisonResult::SUCCEEDED) {
                    EnumRelationOutcome::Related
                } else {
                    EnumRelationOutcome::Unrelated(None)
                });
            }
        }
        let target_enum_type = self.get_type_of_symbol(target_symbol)?;
        let source_enum_type = self.get_type_of_symbol(source_symbol)?;
        let source_properties = self.get_properties_of_type_full(source_enum_type)?;
        for source_property in source_properties {
            if !self
                .binder
                .symbol(source_property)
                .flags
                .intersects(SymbolFlags::ENUM_MEMBER)
            {
                continue;
            }
            let name = self.binder.symbol(source_property).escaped_name.clone();
            let target_property = self
                .get_property_of_type_full(target_enum_type, &name)?
                .filter(|&property| {
                    self.binder
                        .symbol(property)
                        .flags
                        .intersects(SymbolFlags::ENUM_MEMBER)
                });
            let Some(target_property) = target_property else {
                self.relations
                    .enum_relation
                    .insert(id, RelationComparisonResult::FAILED);
                return Ok(EnumRelationOutcome::Unrelated(collect_error.then_some(
                    EnumRelationError::MissingMember {
                        source_member: source_property,
                        target_enum: target_symbol,
                    },
                )));
            };
            let source_declaration = self
                .get_declaration_of_kind(source_property, SyntaxKind::EnumMember)
                .expect("binder invariant: ENUM_MEMBER symbols carry their EnumMember declaration");
            let target_declaration = self
                .get_declaration_of_kind(target_property, SyntaxKind::EnumMember)
                .expect("binder invariant: ENUM_MEMBER symbols carry their EnumMember declaration");
            let source_value = self.get_enum_member_value(source_declaration)?.value;
            let target_value = self.get_enum_member_value(target_declaration)?.value;
            if source_value != target_value {
                let source_is_string = matches!(source_value, Some(EvalValue::Str(_)));
                let target_is_string = matches!(target_value, Some(EvalValue::Str(_)));
                if (source_value.is_some() && target_value.is_some())
                    || source_is_string
                    || target_is_string
                {
                    let error = if collect_error {
                        match (target_value.clone(), source_value.clone()) {
                            (Some(expected), Some(given)) => {
                                Some(EnumRelationError::MismatchedMemberValue {
                                    target_enum: target_symbol,
                                    target_member: target_property,
                                    expected,
                                    given,
                                })
                            }
                            (expected, given) => {
                                let known_string = expected
                                    .or(given)
                                    .and_then(|value| match value {
                                        EvalValue::Str(value) => Some(value),
                                        EvalValue::Num(_) => None,
                                    })
                                    .expect("string/unknown enum mismatch has a known string");
                                Some(EnumRelationError::StringVsUnknownNumeric {
                                    target_enum: target_symbol,
                                    target_member: target_property,
                                    known_string,
                                })
                            }
                        }
                    } else {
                        None
                    };
                    self.relations
                        .enum_relation
                        .insert(id, RelationComparisonResult::FAILED);
                    return Ok(EnumRelationOutcome::Unrelated(error));
                }
            }
        }
        self.relations
            .enum_relation
            .insert(id, RelationComparisonResult::SUCCEEDED);
        Ok(EnumRelationOutcome::Related)
    }
}

#[cfg(test)]
#[path = "../tests/unit/relate/tests.rs"]
mod tests;
