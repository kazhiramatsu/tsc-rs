//! Conditional/substitution semantic model and lazy arm accessors.
//!
//! Phase 9.6a makes both type families constructible while leaving
//! conditional evaluation/distribution to 9.6c and relation/inference
//! consumption to 9.6d.

use tsrs2_syntax::{NodeData, NodeId};
use tsrs2_types::{IntersectionFlags, TypeData, TypeId};

use crate::state::{CheckResult2, CheckerState};

impl<'a> CheckerState<'a> {
    /// tsc-port: getNoInferType @6.0.3
    /// tsc-hash: 1dfcae3e626dbcf419c9b26d662b6fa4d15c0efb6f1eaacd87b06b85d14a04dd
    /// tsc-span: _tsc.js:60421-60423
    pub(crate) fn get_no_infer_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self.is_no_infer_target_type(ty)? {
            Ok(self
                .tables
                .get_or_create_substitution_type(ty, self.tables.intrinsics.unknown))
        } else {
            Ok(ty)
        }
    }

    /// tsc-port: isNoInferTargetType @6.0.3
    /// tsc-hash: 466180cf407380cd069742ed83680cbd4e7335791a8b76318d0f4369bdf42d84
    /// tsc-span: _tsc.js:60424-60426
    fn is_no_infer_target_type(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(tsrs2_types::TypeFlags::UNION_OR_INTERSECTION) {
            let members: Vec<TypeId> = match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } | TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("union/intersection flag implies member data"),
            };
            for member in members {
                if self.is_no_infer_target_type(member)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if flags.intersects(tsrs2_types::TypeFlags::SUBSTITUTION) {
            if self.tables.is_no_infer_type(ty) {
                return Ok(false);
            }
            let TypeData::Substitution(data) = self.tables.type_of(ty).data.clone() else {
                unreachable!("Substitution flag implies substitution data");
            };
            return self.is_no_infer_target_type(data.base_type);
        }
        if flags.intersects(tsrs2_types::TypeFlags::OBJECT) {
            return Ok(!self.is_empty_anonymous_object_type(ty)?);
        }
        Ok(flags.intersects(tsrs2_types::TypeFlags::INSTANTIABLE)
            && !self.tables.is_pattern_literal_type(ty))
    }

    /// tsc-port: getTrueTypeFromConditionalType @6.0.3
    /// tsc-hash: 3bc8c100391c728a2d646188cc6f497bfae8befb8364762df568410bdfbe630f
    /// tsc-span: _tsc.js:62746-62748
    pub(crate) fn get_true_type_from_conditional_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.ty(ty).conditional_true_type.resolved() {
            return Ok(cached);
        }
        let TypeData::Conditional(data) = self.tables.type_of(ty).data.clone() else {
            unreachable!("Conditional flag implies conditional data");
        };
        let root = self.tables.conditional_root(data.root).clone();
        let NodeData::ConditionalType(node) = self.data_of(NodeId(root.node)) else {
            unreachable!("conditional root points at a ConditionalType node");
        };
        let true_node = node
            .true_type
            .expect("parser invariant: ConditionalType true_type always parsed");
        let written = self.get_type_from_type_node(true_node)?;
        let resolved = self.instantiate_type(written, data.mapper)?;
        self.links
            .set_conditional_true_type(self.speculation_depth, ty, resolved);
        Ok(resolved)
    }

    /// tsc-port: getFalseTypeFromConditionalType @6.0.3
    /// tsc-hash: 8317b2419a9d7338fab82d1a4e84abde8a11b8f4258f4a8ae01bae40c90d6c3e
    /// tsc-span: _tsc.js:62749-62751
    pub(crate) fn get_false_type_from_conditional_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.ty(ty).conditional_false_type.resolved() {
            return Ok(cached);
        }
        let TypeData::Conditional(data) = self.tables.type_of(ty).data.clone() else {
            unreachable!("Conditional flag implies conditional data");
        };
        let root = self.tables.conditional_root(data.root).clone();
        let NodeData::ConditionalType(node) = self.data_of(NodeId(root.node)) else {
            unreachable!("conditional root points at a ConditionalType node");
        };
        let false_node = node
            .false_type
            .expect("parser invariant: ConditionalType false_type always parsed");
        let written = self.get_type_from_type_node(false_node)?;
        let resolved = self.instantiate_type(written, data.mapper)?;
        self.links
            .set_conditional_false_type(self.speculation_depth, ty, resolved);
        Ok(resolved)
    }

    /// tsc-port: getInferredTrueTypeFromConditionalType @6.0.3
    /// tsc-hash: 07b0e79843a8500a14e00f338f3f8a1079db186fad242ed9d1997c2908f74cb5
    /// tsc-span: _tsc.js:62752-62754
    pub fn get_inferred_true_type_from_conditional_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.ty(ty).conditional_inferred_true_type.resolved() {
            return Ok(cached);
        }
        let TypeData::Conditional(data) = self.tables.type_of(ty).data.clone() else {
            unreachable!("Conditional flag implies conditional data");
        };
        let resolved = if let Some(mapper) = data.combined_mapper {
            let root = self.tables.conditional_root(data.root).clone();
            let NodeData::ConditionalType(node) = self.data_of(NodeId(root.node)) else {
                unreachable!("conditional root points at a ConditionalType node");
            };
            let true_node = node
                .true_type
                .expect("parser invariant: ConditionalType true_type always parsed");
            let written = self.get_type_from_type_node(true_node)?;
            self.instantiate_type(written, Some(mapper))?
        } else {
            self.get_true_type_from_conditional_type(ty)?
        };
        self.links
            .set_conditional_inferred_true_type(self.speculation_depth, ty, resolved);
        Ok(resolved)
    }

    /// tsc-port: getSubstitutionIntersection @6.0.3
    /// tsc-hash: 5bddb04660c7780c154c4dc8330df3a5cd62e27322c28dc4bb5c56b75f01162d
    /// tsc-span: _tsc.js:60446-60448
    pub(crate) fn get_substitution_intersection(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let TypeData::Substitution(data) = self.tables.type_of(ty).data.clone() else {
            unreachable!("Substitution flag implies substitution data");
        };
        if self.tables.is_no_infer_type(ty) {
            Ok(data.base_type)
        } else {
            self.get_intersection_type(&[data.constraint, data.base_type], IntersectionFlags::NONE)
        }
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::{CompilerOptions, SymbolFlags, TypeData, TypeFlags};

    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;

    #[test]
    fn no_infer_type_production() {
        with_program_state(
            &[(
                "a.ts",
                "type NoInfer<T> = intrinsic;\n\
                 declare let primitive: NoInfer<string>;\n\
                 declare let object: NoInfer<{ x: string }>;\n\
                 function keys<T>() { let key: keyof NoInfer<T>; }\n\
                 declare function choose<T extends string>(value: T, fallback: NoInfer<T>): T;\n\
                 choose(\"foo\", \"bar\");\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let primitive_node =
                    find_probe_annotation(state.binder.source(0), "primitive").expect("primitive");
                let primitive = state
                    .get_type_from_type_node(primitive_node)
                    .expect("primitive NoInfer erases");
                assert_eq!(primitive, state.tables.intrinsics.string);

                let object_node =
                    find_probe_annotation(state.binder.source(0), "object").expect("object");
                let object = state
                    .get_type_from_type_node(object_node)
                    .expect("object NoInfer constructs");
                assert!(state.tables.is_no_infer_type(object));
                assert_eq!(
                    state.type_to_string_slice(object).expect("NoInfer display"),
                    "NoInfer<{ x: string; }>"
                );

                let key_node = find_probe_annotation(state.binder.source(0), "key").expect("key");
                let key = state
                    .get_type_from_type_node(key_node)
                    .expect("keyof NoInfer constructs");
                let TypeData::Substitution(key_data) = state.tables.type_of(key).data.clone()
                else {
                    panic!("keyof NoInfer<T> preserves the inference barrier");
                };
                assert!(state.tables.is_no_infer_type(key));
                assert!(state
                    .tables
                    .flags_of(key_data.base_type)
                    .intersects(TypeFlags::INDEX));

                state.check_source_file(0);
                assert!(state
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code() == 2345));

                let choose = state
                    .resolve_file_scope_name("choose", SymbolFlags::FUNCTION)
                    .expect("choose resolves");
                assert!(state.get_type_of_symbol(choose).is_ok());
            },
        );
    }
}
