//! Conditional/substitution semantic model and lazy arm accessors.
//!
//! Phase 9.6a makes both type families constructible while leaving
//! conditional evaluation/distribution to 9.6c and relation/inference
//! consumption to 9.6d.

use tsrs2_syntax::{NodeData, NodeId};
use tsrs2_types::{IntersectionFlags, TypeData, TypeId};

use crate::state::{CheckResult2, CheckerState};

impl<'a> CheckerState<'a> {
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
