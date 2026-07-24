//! Conditional/substitution resolution and lazy arm accessors.

use tsrs2_binder::SymbolId;
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    ConditionalRootId, InferenceFlags, InferencePriority, IntersectionFlags, TypeData, TypeFlags,
    TypeId, UnionReduction,
};

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

    /// tsc-port: isSimpleTupleType @6.0.3
    /// tsc-hash: 016f0b673361ee9d1979e1e801cb3c648e9b4ffc677c5eb72f0bb3e4ff94f1c9
    /// tsc-span: _tsc.js:62640-62642
    fn is_simple_tuple_type_node(&self, node: NodeId) -> bool {
        let node = self.skip_type_parentheses(node);
        let NodeData::TupleType(data) = self.data_of(node) else {
            return false;
        };
        let elements = self.nodes_of(data.elements);
        !elements.is_empty()
            && !elements.iter().any(|&element| match self.kind_of(element) {
                SyntaxKind::OptionalType | SyntaxKind::RestType => true,
                SyntaxKind::NamedTupleMember => {
                    let NodeData::NamedTupleMember(data) = self.data_of(element) else {
                        unreachable!("NamedTupleMember kind implies payload");
                    };
                    data.question_token.is_some() || data.dot_dot_dot_token.is_some()
                }
                _ => false,
            })
    }

    fn skip_type_parentheses(&self, node: NodeId) -> NodeId {
        let mut node = node;
        while let NodeData::ParenthesizedType(data) = self.data_of(node) {
            let Some(inner) = data.r#type else {
                break;
            };
            node = inner;
        }
        node
    }

    /// tsc-port: isDeferredType @6.0.3
    /// tsc-hash: d10ada7ef2807c62198b24164c0e9e667afb80a7b56457ad39e0f9c93faf717c
    /// tsc-span: _tsc.js:62643-62645
    fn is_deferred_conditional_type(
        &mut self,
        ty: TypeId,
        check_tuples: bool,
    ) -> CheckResult2<bool> {
        if self.is_generic_type(ty)? {
            return Ok(true);
        }
        if check_tuples && self.tables.is_tuple_type(ty) {
            for element in self.get_type_arguments(ty)? {
                if self.is_generic_type(element)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: getConditionalType @6.0.3
    /// tsc-hash: d44ace02934af8e88ac3c8ff742291cb3411a2d6c71b3c2a1069501a097c943e
    /// tsc-span: _tsc.js:62646-62746
    pub(crate) fn get_conditional_type(
        &mut self,
        mut root: ConditionalRootId,
        mut mapper: Option<tsrs2_types::MapperId>,
        for_constraint: bool,
        mut alias_symbol: Option<SymbolId>,
        mut alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<TypeId> {
        let mut extra_types = Vec::new();
        let mut tail_count = 0usize;
        loop {
            if tail_count == 1_000 {
                self.error_at(
                    self.current_node,
                    &diagnostics::Type_instantiation_is_excessively_deep_and_possibly_infinite,
                    &[],
                );
                return Ok(self.tables.intrinsics.error);
            }

            let root_data = self.tables.conditional_root(root).clone();
            let actual_check_type = self.get_actual_type_variable(root_data.check_type)?;
            let check_type = self.instantiate_type(actual_check_type, mapper)?;
            let extends_type = self.instantiate_type(root_data.extends_type, mapper)?;
            if self.tables.is_error_type(check_type) || self.tables.is_error_type(extends_type) {
                return Ok(self.tables.intrinsics.error);
            }
            if check_type == self.tables.intrinsics.wildcard
                || extends_type == self.tables.intrinsics.wildcard
            {
                return Ok(self.tables.intrinsics.wildcard);
            }

            let NodeData::ConditionalType(node) = self.data_of(NodeId(root_data.node)).clone()
            else {
                unreachable!("conditional root points at a ConditionalType node");
            };
            let check_node = node
                .check_type
                .expect("parser invariant: ConditionalType check_type always parsed");
            let extends_node = node
                .extends_type
                .expect("parser invariant: ConditionalType extends_type always parsed");
            let simple_check = self.is_simple_tuple_type_node(check_node);
            let simple_extends = self.is_simple_tuple_type_node(extends_node);
            let check_tuples = simple_check
                && simple_extends
                && match (
                    self.data_of(self.skip_type_parentheses(check_node)),
                    self.data_of(self.skip_type_parentheses(extends_node)),
                ) {
                    (NodeData::TupleType(left), NodeData::TupleType(right)) => {
                        self.nodes_of(left.elements).len() == self.nodes_of(right.elements).len()
                    }
                    _ => false,
                };
            let check_type_deferred =
                self.is_deferred_conditional_type(check_type, check_tuples)?;

            let mut combined_mapper = None;
            if !root_data.infer_type_parameters.is_empty() {
                let context = self.create_inference_context(
                    &root_data.infer_type_parameters,
                    None,
                    InferenceFlags::NONE,
                    None,
                );
                if let Some(outer_mapper) = mapper {
                    let non_fixing = self.inference_context(context).non_fixing_mapper;
                    let combined = self.combine_type_mappers(Some(non_fixing), outer_mapper);
                    self.inference_context_mut(context).non_fixing_mapper = combined;
                }
                if !check_type_deferred {
                    let inferences = self.inference_context(context).inferences.clone();
                    self.infer_types(
                        &inferences,
                        check_type,
                        extends_type,
                        InferencePriority::NO_CONSTRAINTS | InferencePriority::ALWAYS_STRICT,
                        false,
                    )?;
                }
                let inference_mapper = self.inference_context(context).mapper;
                combined_mapper = Some(match mapper {
                    Some(outer_mapper) => {
                        self.combine_type_mappers(Some(inference_mapper), outer_mapper)
                    }
                    None => inference_mapper,
                });
            }

            let inferred_extends_type = match combined_mapper {
                Some(combined_mapper) => {
                    self.instantiate_type(root_data.extends_type, Some(combined_mapper))?
                }
                None => extends_type,
            };
            if !check_type_deferred
                && !self.is_deferred_conditional_type(inferred_extends_type, check_tuples)?
            {
                let inferred_flags = self.tables.flags_of(inferred_extends_type);
                let check_flags = self.tables.flags_of(check_type);
                let permissive_check = self.get_permissive_instantiation(check_type)?;
                let permissive_extends =
                    self.get_permissive_instantiation(inferred_extends_type)?;
                if !inferred_flags.intersects(TypeFlags::ANY_OR_UNKNOWN)
                    && (check_flags.intersects(TypeFlags::ANY)
                        || !self.is_type_assignable_to(permissive_check, permissive_extends)?)
                {
                    let mutually_possible_for_constraint =
                        if for_constraint && !inferred_flags.intersects(TypeFlags::NEVER) {
                            let mut some_assignable = false;
                            for member in self.union_members_or_self(permissive_extends) {
                                if self.is_type_assignable_to(member, permissive_check)? {
                                    some_assignable = true;
                                    break;
                                }
                            }
                            some_assignable
                        } else {
                            false
                        };
                    if check_flags.intersects(TypeFlags::ANY) || mutually_possible_for_constraint {
                        let true_node = node
                            .true_type
                            .expect("parser invariant: ConditionalType true_type always parsed");
                        let true_type = self.get_type_from_type_node(true_node)?;
                        extra_types
                            .push(self.instantiate_type(true_type, combined_mapper.or(mapper))?);
                    }

                    let false_node = node
                        .false_type
                        .expect("parser invariant: ConditionalType false_type always parsed");
                    let false_type = self.get_type_from_type_node(false_node)?;
                    if let TypeData::Conditional(false_data) =
                        self.tables.type_of(false_type).data.clone()
                    {
                        let new_root = self.tables.conditional_root(false_data.root).clone();
                        if self.parent_of(NodeId(new_root.node)) == Some(NodeId(root_data.node))
                            && (!new_root.is_distributive
                                || new_root.check_type == root_data.check_type)
                        {
                            root = false_data.root;
                            continue;
                        }
                        if let Some((new_root_id, new_mapper)) =
                            self.conditional_tail_recurse(false_type, mapper)?
                        {
                            root = new_root_id;
                            mapper = Some(new_mapper);
                            alias_symbol = None;
                            alias_type_arguments = None;
                            if self.tables.conditional_root(root).alias_symbol.is_some() {
                                tail_count += 1;
                            }
                            continue;
                        }
                    }
                    let result = self.instantiate_type(false_type, mapper)?;
                    return if extra_types.is_empty() {
                        Ok(result)
                    } else {
                        extra_types.push(result);
                        self.get_union_type_ex(&extra_types, UnionReduction::Literal)
                    };
                }

                let restrictive_check = self.get_restrictive_instantiation(check_type)?;
                let restrictive_extends =
                    self.get_restrictive_instantiation(inferred_extends_type)?;
                if inferred_flags.intersects(TypeFlags::ANY_OR_UNKNOWN)
                    || self.is_type_assignable_to(restrictive_check, restrictive_extends)?
                {
                    let true_node = node
                        .true_type
                        .expect("parser invariant: ConditionalType true_type always parsed");
                    let true_type = self.get_type_from_type_node(true_node)?;
                    let true_mapper = combined_mapper.or(mapper);
                    if let Some((new_root_id, new_mapper)) =
                        self.conditional_tail_recurse(true_type, true_mapper)?
                    {
                        root = new_root_id;
                        mapper = Some(new_mapper);
                        alias_symbol = None;
                        alias_type_arguments = None;
                        if self.tables.conditional_root(root).alias_symbol.is_some() {
                            tail_count += 1;
                        }
                        continue;
                    }
                    let result = self.instantiate_type(true_type, true_mapper)?;
                    return if extra_types.is_empty() {
                        Ok(result)
                    } else {
                        extra_types.push(result);
                        self.get_union_type_ex(&extra_types, UnionReduction::Literal)
                    };
                }
            }

            let result_alias_symbol = alias_symbol.or(root_data.alias_symbol);
            let result_alias_arguments = if alias_symbol.is_some() {
                alias_type_arguments.map(<[TypeId]>::to_vec)
            } else {
                match root_data.alias_type_arguments.as_deref() {
                    Some(arguments) => match mapper {
                        Some(mapper) => Some(self.instantiate_types(arguments, mapper)?),
                        None => Some(arguments.to_vec()),
                    },
                    None => None,
                }
            };
            let result_check_type = self.instantiate_type(root_data.check_type, mapper)?;
            let result_extends_type = self.instantiate_type(root_data.extends_type, mapper)?;
            let result = self.tables.create_conditional_type(
                tsrs2_types::ConditionalTypeData {
                    root,
                    check_type: result_check_type,
                    extends_type: result_extends_type,
                    mapper,
                    combined_mapper,
                },
                result_alias_symbol,
                result_alias_arguments.as_deref(),
            );
            return if extra_types.is_empty() {
                Ok(result)
            } else {
                extra_types.push(result);
                self.get_union_type_ex(&extra_types, UnionReduction::Literal)
            };
        }
    }

    fn conditional_tail_recurse(
        &mut self,
        new_type: TypeId,
        new_mapper: Option<tsrs2_types::MapperId>,
    ) -> CheckResult2<Option<(ConditionalRootId, tsrs2_types::MapperId)>> {
        let Some(new_mapper) = new_mapper else {
            return Ok(None);
        };
        let TypeData::Conditional(data) = self.tables.type_of(new_type).data.clone() else {
            return Ok(None);
        };
        let new_root = self.tables.conditional_root(data.root).clone();
        let Some(outer_parameters) = new_root.outer_type_parameters.as_deref() else {
            return Ok(None);
        };
        let type_parameter_mapper = self.combine_type_mappers(data.mapper, new_mapper);
        let mut type_arguments = Vec::with_capacity(outer_parameters.len());
        for &parameter in outer_parameters {
            type_arguments.push(self.get_mapped_type(parameter, type_parameter_mapper)?);
        }
        let new_root_mapper =
            self.create_type_mapper(outer_parameters.to_vec(), Some(type_arguments));
        let new_check_type = if new_root.is_distributive {
            Some(self.get_mapped_type(new_root.check_type, new_root_mapper)?)
        } else {
            None
        };
        if new_check_type.is_none()
            || new_check_type == Some(new_root.check_type)
            || !self
                .tables
                .flags_of(new_check_type.expect("checked above"))
                .intersects(TypeFlags::UNION | TypeFlags::NEVER)
        {
            Ok(Some((data.root, new_root_mapper)))
        } else {
            Ok(None)
        }
    }

    /// tsc-port: isDistributionDependent @6.0.3
    /// tsc-hash: 38b54dec45a443bed1c4b247799bfb22ee1bf1c9cd5d680c5f332ab644609e5b
    /// tsc-span: _tsc.js:62767-62769
    #[allow(dead_code)] // relation consumer lands in 9.6d
    pub(crate) fn is_distribution_dependent(
        &mut self,
        root: ConditionalRootId,
    ) -> CheckResult2<bool> {
        let root = self.tables.conditional_root(root).clone();
        if !root.is_distributive {
            return Ok(false);
        }
        let NodeData::ConditionalType(node) = self.data_of(NodeId(root.node)).clone() else {
            unreachable!("conditional root points at a ConditionalType node");
        };
        let true_node = node
            .true_type
            .expect("parser invariant: ConditionalType true_type always parsed");
        let false_node = node
            .false_type
            .expect("parser invariant: ConditionalType false_type always parsed");
        Ok(
            self.is_type_parameter_possibly_referenced(root.check_type, true_node)?
                || self.is_type_parameter_possibly_referenced(root.check_type, false_node)?,
        )
    }

    /// tsc-port: getConditionalTypeInstantiation @6.0.3
    /// tsc-hash: 55399529c46ef1b785efaaa0c9f2c3ee27201ee5d2596a6682dc37049dd61f4e
    /// tsc-span: _tsc.js:63658-63674
    pub(crate) fn get_conditional_type_instantiation(
        &mut self,
        ty: TypeId,
        mapper: tsrs2_types::MapperId,
        for_constraint: bool,
        alias_symbol: Option<SymbolId>,
        alias_type_arguments: Option<&[TypeId]>,
    ) -> CheckResult2<TypeId> {
        let TypeData::Conditional(data) = self.tables.type_of(ty).data.clone() else {
            unreachable!("Conditional flag implies conditional data");
        };
        let root = self.tables.conditional_root(data.root).clone();
        let Some(outer_parameters) = root.outer_type_parameters.as_deref() else {
            return Ok(ty);
        };
        let mut type_arguments = Vec::with_capacity(outer_parameters.len());
        for &parameter in outer_parameters {
            type_arguments.push(self.get_mapped_type(parameter, mapper)?);
        }
        let mut key = if for_constraint {
            "C".to_owned()
        } else {
            String::new()
        };
        key.push_str(&self.tables.get_type_list_id(&type_arguments));
        key.push_str(&self.tables.get_alias_id(alias_symbol, alias_type_arguments));
        if let Some(cached) = self.links.conditional_instantiation(data.root, &key) {
            return Ok(cached);
        }

        let new_mapper = self.create_type_mapper(outer_parameters.to_vec(), Some(type_arguments));
        let distribution_type = if root.is_distributive {
            let mapped = self.get_mapped_type(root.check_type, new_mapper)?;
            Some(self.get_reduced_type(mapped)?)
        } else {
            None
        };
        let result = if let Some(distribution_type) = distribution_type.filter(|&distribution| {
            distribution != root.check_type
                && self
                    .tables
                    .flags_of(distribution)
                    .intersects(TypeFlags::UNION | TypeFlags::NEVER)
        }) {
            self.map_type_with_alias(
                distribution_type,
                &mut |state, member| {
                    let member_mapper =
                        state.prepend_type_mapping(root.check_type, member, Some(new_mapper));
                    state.get_conditional_type(
                        data.root,
                        Some(member_mapper),
                        for_constraint,
                        None,
                        None,
                    )
                },
                alias_symbol,
                alias_type_arguments,
            )?
        } else {
            self.get_conditional_type(
                data.root,
                Some(new_mapper),
                for_constraint,
                alias_symbol,
                alias_type_arguments,
            )?
        };
        if self.speculation_depth == 0 {
            self.links.set_conditional_instantiation(
                self.speculation_depth,
                data.root,
                key,
                result,
            );
        }
        Ok(result)
    }

    /// tsc-port: getDefaultConstraintOfConditionalType @6.0.3
    /// tsc-hash: 8f751462d98ed604349f64f9e1f7e58e7236b17625eb01e9f4a310efbe603225
    /// tsc-span: _tsc.js:58826-58833
    pub(crate) fn get_default_constraint_of_conditional_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.ty(ty).conditional_default_constraint.resolved() {
            return Ok(cached);
        }
        let true_constraint = self.get_inferred_true_type_from_conditional_type(ty)?;
        let false_constraint = self.get_false_type_from_conditional_type(ty)?;
        let resolved = if self
            .tables
            .flags_of(true_constraint)
            .intersects(TypeFlags::ANY)
        {
            false_constraint
        } else if self
            .tables
            .flags_of(false_constraint)
            .intersects(TypeFlags::ANY)
        {
            true_constraint
        } else {
            self.get_union_type_ex(
                &[true_constraint, false_constraint],
                UnionReduction::Literal,
            )?
        };
        if self.speculation_depth == 0 {
            self.links
                .set_conditional_default_constraint(self.speculation_depth, ty, resolved);
        }
        Ok(resolved)
    }

    /// tsc-port: getConstraintOfDistributiveConditionalType @6.0.3
    /// tsc-hash: 21f63be335d12237aca227b40fc03156a1a788547fb8c8f22f2721d9d2f8a697
    /// tsc-span: _tsc.js:58834-58860
    fn get_constraint_of_distributive_conditional_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        if let Some(cached) = self
            .links
            .ty(ty)
            .conditional_constraint_of_distributive
            .resolved()
        {
            return Ok(cached);
        }
        let TypeData::Conditional(data) = self.tables.type_of(ty).data.clone() else {
            unreachable!("Conditional flag implies conditional data");
        };
        let root = self.tables.conditional_root(data.root).clone();
        let already_restrictive =
            self.links.ty(ty).restrictive_instantiation.resolved() == Some(ty);
        if root.is_distributive && !already_restrictive {
            let simplified = self.get_simplified_type(data.check_type, /*writing*/ false)?;
            let constraint = if simplified == data.check_type {
                self.get_constraint_of_type(simplified)?
            } else {
                Some(simplified)
            };
            if let Some(constraint) = constraint.filter(|&constraint| constraint != data.check_type)
            {
                let mapper = self.prepend_type_mapping(root.check_type, constraint, data.mapper);
                let instantiated = self.get_conditional_type_instantiation(
                    ty, mapper, /*for_constraint*/ true, None, None,
                )?;
                if !self
                    .tables
                    .flags_of(instantiated)
                    .intersects(TypeFlags::NEVER)
                {
                    if self.speculation_depth == 0 {
                        self.links.set_conditional_constraint_of_distributive(
                            self.speculation_depth,
                            ty,
                            Some(instantiated),
                        );
                    }
                    return Ok(Some(instantiated));
                }
            }
        }
        if self.speculation_depth == 0 {
            self.links
                .set_conditional_constraint_of_distributive(self.speculation_depth, ty, None);
        }
        Ok(None)
    }

    /// tsc-port: getConstraintFromConditionalType @6.0.3
    /// tsc-hash: 76287e46a746ef44269b09bd11a2a17b46045cdb05315b9785e6fb4a8c2fe304
    /// tsc-span: _tsc.js:58861-58863
    pub(crate) fn get_constraint_from_conditional_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        match self.get_constraint_of_distributive_conditional_type(ty)? {
            Some(constraint) => Ok(constraint),
            None => self.get_default_constraint_of_conditional_type(ty),
        }
    }

    /// tsc-port: getConstraintOfConditionalType @6.0.3
    /// tsc-hash: 06dcd01dedd19ddded51c31a501080357617a118295bb0cd22ebde5e6f66a69e
    /// tsc-span: _tsc.js:58864-58866
    pub(crate) fn get_constraint_of_conditional_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        if self.has_non_circular_base_constraint(ty)? {
            self.get_constraint_from_conditional_type(ty).map(Some)
        } else {
            Ok(None)
        }
    }

    /// tsc-port: getSimplifiedConditionalType @6.0.3
    /// tsc-hash: b9c105055fc1f31849a4d53368f0344bb12c3a15746ffdbcdb165d648a9e3c7f
    /// tsc-span: _tsc.js:62507-62526
    pub(crate) fn get_simplified_conditional_type(
        &mut self,
        ty: TypeId,
        writing: bool,
    ) -> CheckResult2<TypeId> {
        let TypeData::Conditional(data) = self.tables.type_of(ty).data.clone() else {
            unreachable!("Conditional flag implies conditional data");
        };
        let true_type = self.get_true_type_from_conditional_type(ty)?;
        let false_type = self.get_false_type_from_conditional_type(ty)?;
        if self
            .tables
            .flags_of(false_type)
            .intersects(TypeFlags::NEVER)
            && self.get_actual_type_variable(true_type)?
                == self.get_actual_type_variable(data.check_type)?
        {
            let check_flags = self.tables.flags_of(data.check_type);
            let restrictive_check = self.get_restrictive_instantiation(data.check_type)?;
            let restrictive_extends = self.get_restrictive_instantiation(data.extends_type)?;
            if check_flags.intersects(TypeFlags::ANY)
                || self.is_type_assignable_to(restrictive_check, restrictive_extends)?
            {
                return self.get_simplified_type(true_type, writing);
            }
            if self.is_intersection_empty(data.check_type, data.extends_type)? {
                return Ok(self.tables.intrinsics.never);
            }
        } else if self.tables.flags_of(true_type).intersects(TypeFlags::NEVER)
            && self.get_actual_type_variable(false_type)?
                == self.get_actual_type_variable(data.check_type)?
        {
            let check_flags = self.tables.flags_of(data.check_type);
            let restrictive_check = self.get_restrictive_instantiation(data.check_type)?;
            let restrictive_extends = self.get_restrictive_instantiation(data.extends_type)?;
            if !check_flags.intersects(TypeFlags::ANY)
                && self.is_type_assignable_to(restrictive_check, restrictive_extends)?
            {
                return Ok(self.tables.intrinsics.never);
            }
            if check_flags.intersects(TypeFlags::ANY)
                || self.is_intersection_empty(data.check_type, data.extends_type)?
            {
                return self.get_simplified_type(false_type, writing);
            }
        }
        Ok(ty)
    }

    /// tsc-port: isIntersectionEmpty @6.0.3
    /// tsc-hash: e2589e93386fafff767385da0f49b090b8021b31c0814151161039ae403160da
    /// tsc-span: _tsc.js:62533-62535
    fn is_intersection_empty(&mut self, left: TypeId, right: TypeId) -> CheckResult2<bool> {
        let intersection = self.get_intersection_type(&[left, right], IntersectionFlags::NONE)?;
        let reduced = self.get_union_type_ex(
            &[intersection, self.tables.intrinsics.never],
            UnionReduction::Literal,
        )?;
        Ok(self.tables.flags_of(reduced).intersects(TypeFlags::NEVER))
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
    use tsrs2_types::{CompilerOptions, SymbolFlags, TypeData, TypeFlags, TypeId};

    use crate::relpin::find_probe_annotation;
    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    fn annotation_type(state: &mut CheckerState, name: &str) -> TypeId {
        let node = find_probe_annotation(state.binder.source(0), name)
            .unwrap_or_else(|| panic!("annotation for {name}"));
        state
            .get_type_from_type_node(node)
            .unwrap_or_else(|err| panic!("{name} resolves: {}", err.reason))
    }

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

    #[test]
    fn conditional_resolution_distribution_inference_and_simplification() {
        with_program_state(
            &[(
                "a.ts",
                "type Select<T> = T extends string ? T : never;\n\
                 type Identity<T> = T extends infer U ? U : never;\n\
                 declare let falseBranch: number extends string ? 1 : 2;\n\
                 declare let inferred: string extends infer U ? U : never;\n\
                 declare let distributed: Select<\"a\" | 1>;\n\
                 declare let expectedDistributed: \"a\";\n\
                 declare let identity: Identity<\"x\" | 2>;\n\
                 declare let expectedIdentity: \"x\" | 2;\n\
                 function deferred<T>() {\n\
                   let branch: T extends string ? 1 : 2;\n\
                   let expectedDefault: 1 | 2;\n\
                   let same: T extends unknown ? T : never;\n\
                   let expectedSame: T;\n\
                 }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                assert_eq!(
                    annotation_type(state, "falseBranch"),
                    state.tables.get_number_literal_type(2.0),
                );
                assert_eq!(
                    annotation_type(state, "inferred"),
                    state.tables.intrinsics.string,
                );
                assert_eq!(
                    annotation_type(state, "distributed"),
                    annotation_type(state, "expectedDistributed"),
                );
                assert_eq!(
                    annotation_type(state, "identity"),
                    annotation_type(state, "expectedIdentity"),
                );

                let branch = annotation_type(state, "branch");
                assert!(state
                    .tables
                    .flags_of(branch)
                    .intersects(TypeFlags::CONDITIONAL));
                let default_constraint = state
                    .get_default_constraint_of_conditional_type(branch)
                    .expect("default conditional constraint");
                assert_eq!(
                    default_constraint,
                    annotation_type(state, "expectedDefault"),
                );

                let same = annotation_type(state, "same");
                let simplified = state
                    .get_simplified_type(same, /*writing*/ false)
                    .expect("conditional simplification");
                assert_eq!(simplified, annotation_type(state, "expectedSame"));
            },
        );
    }

    #[test]
    fn conditional_checker_consumers_do_not_fabricate_constraints_or_cycles() {
        with_program_state(
            &[(
                "a.ts",
                "type PropertyKey = string | number | symbol;\n\
                 type UnexpectedError<T extends PropertyKey> = T;\n\
                 type Example<T, U> = {\n\
                   [K in keyof T]: K extends keyof U ? UnexpectedError<K> : K\n\
                 };\n\
                 type StrictExtract<T, U> = T extends U ? U extends T ? T : never : never;\n\
                 type StrictExclude<T, U> = T extends StrictExtract<T, U> ? never : T;\n\
                 type A<T> = { [Q in { [P in keyof T]: P }[keyof T]]: T[Q] };\n\
                 type B<T, V> = A<{ [Q in keyof T]: StrictExclude<B<T[Q], V>, {}> }>;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let codes: Vec<u32> = state
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.code())
                    .collect();
                let positions: Vec<(u32, Option<u32>, Option<u32>)> = state
                    .diagnostics
                    .iter()
                    .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
                    .collect();
                assert!(
                    !codes.iter().any(|code| matches!(code, 2315 | 2344 | 2456)),
                    "{positions:?}"
                );
            },
        );
    }
}
