//! Type parameters + the base-constraint machinery (M4 5.1c).
//!
//! Declared type parameters become constructible here
//! (getDeclaredTypeOfTypeParameter), which flips the constrained-
//! type-variable reductions live: getIntersectionType step 6 and
//! removeConstrainedTypeVariables both read getBaseConstraintOfType.

use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{ElementFlags, ObjectFlags, TypeData, TypeFlags, TypeId, TypeSystemPropertyName};

use crate::links::LinkSlot;
use crate::state::{CheckResult2, CheckerState, ResolutionTarget, Unsupported};

impl<'a> CheckerState<'a> {
    /// tsc-port: getDeclaredTypeOfTypeParameter @6.0.3
    /// tsc-hash: 30b8ff7a52ce3cf5c9dac31023284fba4d3d2efe6c3ff7562a62d784dd0b2f8f
    /// tsc-span: _tsc.js:57494-57497
    ///
    /// createTypeParameter (50139): flags TypeParameter + symbol. The
    /// inline TypeData constraint stays None for DECLARED parameters —
    /// their constraint is the lazy links slot (tsc's mutable
    /// `TypeParameter.constraint`); the inline field belongs to the
    /// tables-synthesized parameters (tuple targets).
    pub fn get_declared_type_of_type_parameter(
        &mut self,
        symbol: tsrs2_binder::SymbolId,
    ) -> TypeId {
        if let Some(declared) = self.links.symbol(symbol).declared_type.resolved() {
            return declared;
        }
        let ty = self.tables.create_type(
            TypeFlags::TYPE_PARAMETER,
            TypeData::TypeParameter {
                is_this_type: false,
                constraint: None,
            },
        );
        self.tables.type_mut(ty).symbol = Some(symbol);
        self.links
            .set_symbol_declared_type(self.speculation_depth, symbol, LinkSlot::Resolved(ty));
        ty
    }

    /// tsc-port: getConstraintDeclaration @6.0.3
    /// tsc-hash: fc71627075c903670d496f877070b8920b60d20169eba4040b803c4a0948cef0
    /// tsc-span: _tsc.js:60056-60058
    ///
    /// getEffectiveConstraintOfTypeParameter's JSDoc arm elided.
    pub(crate) fn get_constraint_declaration(&self, ty: TypeId) -> Option<NodeId> {
        let symbol = self.tables.type_of(ty).symbol?;
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find_map(|declaration| match self.data_of(declaration) {
                NodeData::TypeParameter(data)
                    if self.kind_of(declaration) == SyntaxKind::TypeParameter =>
                {
                    data.constraint
                }
                _ => None,
            })
    }

    /// tsc-port: getConstraintFromTypeParameter @6.0.3
    /// tsc-hash: fa1aac56612d0c8da18fd3ea66665ae49d75136dbfdd70073388dadd59e1f4d8
    /// tsc-span: _tsc.js:60103-60122
    ///
    /// The instantiated-parameter arm (typeParameter.target, 60105-60107)
    /// is live since 5.2 (cloneTypeParameter constructs targeted
    /// parameters). The inferred-constraint arm
    /// (getInferredTypeParameterConstraint — infer-declared parameters)
    /// unwinds as Unsupported: infer type parameters ARE resolvable by
    /// name (resolveName's InferType arm), but their constraints need
    /// conditional-type machinery (5.2/M8).
    pub fn get_constraint_from_type_parameter(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        if let Some(cached) = self.links.ty(ty).type_parameter_constraint.resolved() {
            return Ok((cached != self.no_constraint_type).then_some(cached));
        }
        // Tables-synthesized parameters carry their constraint inline
        // (tuple markers); mirror it into the lazy slot's semantics.
        if let TypeData::TypeParameter {
            constraint: Some(inline),
            ..
        } = self.tables.type_of(ty).data
        {
            return Ok(Some(inline));
        }
        if let Some(target) = self.links.ty(ty).type_parameter_target {
            let target_constraint = self.get_constraint_of_type_parameter(target)?;
            let mapper = self.links.ty(ty).type_parameter_mapper;
            let constraint = match target_constraint {
                Some(target_constraint) => self.instantiate_type(target_constraint, mapper)?,
                None => self.no_constraint_type,
            };
            self.links
                .set_type_parameter_constraint(self.speculation_depth, ty, constraint);
            return Ok((constraint != self.no_constraint_type).then_some(constraint));
        }
        let constraint = match self.get_constraint_declaration(ty) {
            None => {
                if self.is_infer_type_parameter(ty) {
                    return Err(Unsupported::new(
                        "infer-type parameter constraints (conditional types, M4 5.2/M8)",
                    ));
                }
                self.no_constraint_type
            }
            Some(declaration) => {
                let mut resolved = self.get_type_from_type_node(declaration)?;
                if self.tables.flags_of(resolved).intersects(TypeFlags::ANY)
                    && !self.tables.is_error_type(resolved)
                {
                    // `T extends any` means unknown (60114-60116); the
                    // mapped-type parent arm selects keyof-compatible
                    // string|number|symbol.
                    let in_mapped_type = self
                        .parent_of(declaration)
                        .and_then(|parameter| self.parent_of(parameter))
                        .is_some_and(|host| self.kind_of(host) == SyntaxKind::MappedType);
                    resolved = if in_mapped_type {
                        self.tables.intrinsics.string_number_symbol
                    } else {
                        self.tables.intrinsics.unknown
                    };
                }
                resolved
            }
        };
        self.links
            .set_type_parameter_constraint(self.speculation_depth, ty, constraint);
        Ok((constraint != self.no_constraint_type).then_some(constraint))
    }

    fn is_infer_type_parameter(&self, ty: TypeId) -> bool {
        let Some(symbol) = self.tables.type_of(ty).symbol else {
            return false;
        };
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .any(|&declaration| {
                self.parent_of(declaration)
                    .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::InferType)
            })
    }

    /// tsc-port: getConstraintOfTypeParameter @6.0.3
    /// tsc-hash: 5f56e7c84102dffca04d5d332ed18e02993636a6d42d065099ce51b7544889ff
    /// tsc-span: _tsc.js:58787-58789
    pub fn get_constraint_of_type_parameter(
        &mut self,
        type_parameter: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        if self.has_non_circular_base_constraint(type_parameter)? {
            self.get_constraint_from_type_parameter(type_parameter)
        } else {
            Ok(None)
        }
    }

    /// tsc-port: getConstraintOfType @6.0.3
    /// tsc-hash: 777f460fb1f80fa7dca265a22f81147c4d585c4afe7985df22dece9922edba03
    /// tsc-span: _tsc.js:58784-58786
    ///
    /// The Conditional arm (getConstraintOfConditionalType) falls to
    /// the base-constraint default — those TypeFlags are
    /// unconstructible until M8 and the base computation escapes on
    /// them.
    pub fn get_constraint_of_type(&mut self, ty: TypeId) -> CheckResult2<Option<TypeId>> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::TYPE_PARAMETER) {
            return self.get_constraint_of_type_parameter(ty);
        }
        if flags.intersects(TypeFlags::INDEXED_ACCESS) {
            return self.get_constraint_of_indexed_access(ty);
        }
        self.get_base_constraint_of_type(ty)
    }

    /// tsc-port: getConstraintOfIndexedAccess @6.0.3
    /// tsc-hash: a648a76d94ff4c4a99a6b6b8955914b6434c6d7de3653ff6a4ea043d938d7dc1
    /// tsc-span: _tsc.js:58798-58800
    pub fn get_constraint_of_indexed_access(&mut self, ty: TypeId) -> CheckResult2<Option<TypeId>> {
        if self.has_non_circular_base_constraint(ty)? {
            self.get_constraint_from_indexed_access(ty)
        } else {
            Ok(None)
        }
    }

    /// tsc-port: getSimplifiedTypeOrConstraint @6.0.3
    /// tsc-hash: ffdf0b790c2083f9e1b1989d7331f0e6c398a16d089296fee81429185ca13132
    /// tsc-span: _tsc.js:58801-58808
    pub fn get_simplified_type_or_constraint(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        let simplified = self.get_simplified_type(ty, /*writing*/ false)?;
        if simplified != ty {
            Ok(Some(simplified))
        } else {
            self.get_constraint_of_type(ty)
        }
    }

    /// tsc-port: getConstraintFromIndexedAccess @6.0.3
    /// tsc-hash: 9b4db0b05453c8a4498e23b0bae23b37a34724a3f3778fd8d62391ba9cf6a2a4
    /// tsc-span: _tsc.js:58809-58825
    ///
    /// The isMappedTypeGenericIndexedAccess head (58810-58812) escapes
    /// conservatively until the mapped payload lands in M8.
    fn get_constraint_from_indexed_access(&mut self, ty: TypeId) -> CheckResult2<Option<TypeId>> {
        let TypeData::IndexedAccess {
            object_type,
            index_type,
            access_flags,
        } = self.tables.type_of(ty).data
        else {
            unreachable!("indexed-access flag implies indexed-access data");
        };
        if self.is_generic_mapped_type_state(object_type)? {
            return Err(Unsupported::new(
                "indexed-access constraints over mapped types \
                 (substituteIndexedMappedType, 9.5c/M8)",
            ));
        }
        let index_constraint = self.get_simplified_type_or_constraint(index_type)?;
        if let Some(index_constraint) = index_constraint {
            if index_constraint != index_type {
                let indexed_access = self.get_indexed_access_type_or_undefined(
                    object_type,
                    index_constraint,
                    access_flags,
                    None,
                    None,
                    None,
                )?;
                if let Some(indexed_access) = indexed_access {
                    return Ok(Some(indexed_access));
                }
            }
        }
        let object_constraint = self.get_simplified_type_or_constraint(object_type)?;
        if let Some(object_constraint) = object_constraint {
            if object_constraint != object_type {
                return self.get_indexed_access_type_or_undefined(
                    object_constraint,
                    index_type,
                    access_flags,
                    None,
                    None,
                    None,
                );
            }
        }
        Ok(None)
    }

    /// tsc-port: getBaseConstraintOfType @6.0.3
    /// tsc-hash: 47e7f23df7e41ce015ff767e755075856f1d6369debf1ce324188f18bf818d10
    /// tsc-span: _tsc.js:58902-58908
    pub fn get_base_constraint_of_type(&mut self, ty: TypeId) -> CheckResult2<Option<TypeId>> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(
            TypeFlags::INSTANTIABLE_NON_PRIMITIVE
                | TypeFlags::UNION_OR_INTERSECTION
                | TypeFlags::TEMPLATE_LITERAL
                | TypeFlags::STRING_MAPPING,
        ) || self.is_generic_tuple_type(ty)
        {
            let constraint = self.get_resolved_base_constraint(ty)?;
            return Ok((constraint != self.no_constraint_type
                && constraint != self.circular_constraint_type)
                .then_some(constraint));
        }
        if flags.intersects(TypeFlags::INDEX) {
            return Ok(Some(self.tables.intrinsics.string_number_symbol));
        }
        Ok(None)
    }

    /// tsc-port: getBaseConstraintOrType @6.0.3
    /// tsc-hash: ebed4b0b0c7de6bb47c5051884d887d3e3f0af9e4e18c7306b27b5610000d460
    /// tsc-span: _tsc.js:58909-58911
    pub fn get_base_constraint_or_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        Ok(self.get_base_constraint_of_type(ty)?.unwrap_or(ty))
    }

    /// tsc-port: hasNonCircularBaseConstraint @6.0.3
    /// tsc-hash: 3d434cfa6881cf8b34e77bf800c7d290a117f4e1f0c19d548b348766682798ee
    /// tsc-span: _tsc.js:58912-58914
    pub fn has_non_circular_base_constraint(&mut self, ty: TypeId) -> CheckResult2<bool> {
        Ok(self.get_resolved_base_constraint(ty)? != self.circular_constraint_type)
    }

    /// tsc-port: getResolvedBaseConstraint @6.0.3
    /// tsc-hash: 76ee41842b8482f7c2b97c06ffb69c0a793e81007d4c8be3325e3a63450a1c55
    /// tsc-span: _tsc.js:58915-59025
    ///
    /// computeBaseConstraint arms present: TypeParameter,
    /// Union/Intersection, TemplateLiteral, StringMapping,
    /// IndexedAccess, generic tuple, and the default identity.
    /// Conditional/Substitution escape (those TypeFlags are
    /// unconstructible before their type nodes land — the escape
    /// fires if they ever appear rather than mis-computing). The
    /// circular-constraint related-info (currentNode) is driver
    /// state — 5.4.
    pub fn get_resolved_base_constraint(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.ty(ty).resolved_base_constraint.resolved() {
            return Ok(cached);
        }
        let mut stack: Vec<crate::engine::RecursionIdentity> = Vec::new();
        let resolved = self.get_immediate_base_constraint(ty, &mut stack)?;
        self.links
            .set_type_resolved_base_constraint(self.speculation_depth, ty, resolved);
        Ok(resolved)
    }

    fn get_immediate_base_constraint(
        &mut self,
        t: TypeId,
        stack: &mut Vec<crate::engine::RecursionIdentity>,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.ty(t).immediate_base_constraint.resolved() {
            return Ok(cached);
        }
        if !self.push_type_resolution(
            ResolutionTarget::Type(t),
            TypeSystemPropertyName::IMMEDIATE_BASE_CONSTRAINT,
        ) {
            return Ok(self.circular_constraint_type);
        }
        let mut result: Option<TypeId> = None;
        let identity = self.get_recursion_identity(t);
        let computed = if stack.len() < 10 || (stack.len() < 50 && !stack.contains(&identity)) {
            stack.push(identity);
            // 58929-58933: the constraint is computed over the
            // SIMPLIFIED type (reading direction).
            let computed = match self.get_simplified_type(t, /*writing*/ false) {
                Ok(simplified) => self.compute_base_constraint(simplified, stack),
                Err(err) => Err(err),
            };
            stack.pop();
            match computed {
                Ok(computed) => {
                    result = computed;
                    Ok(())
                }
                Err(err) => Err(err),
            }
        } else {
            Ok(())
        };
        if let Err(err) = computed {
            self.pop_type_resolution();
            return Err(err);
        }
        let resolved = if self.pop_type_resolution() {
            result.unwrap_or(self.no_constraint_type)
        } else {
            if self
                .tables
                .flags_of(t)
                .intersects(TypeFlags::TYPE_PARAMETER)
            {
                if let Some(error_node) = self.get_constraint_declaration(t) {
                    let name = self
                        .tables
                        .type_of(t)
                        .symbol
                        .map(|s| self.symbol_display_name(s))
                        .unwrap_or_default();
                    self.error_at(
                        Some(error_node),
                        &diagnostics::Type_parameter_0_has_a_circular_constraint,
                        &[&name],
                    );
                }
            }
            self.circular_constraint_type
        };
        self.links
            .set_type_immediate_base_constraint(self.speculation_depth, t, resolved);
        Ok(resolved)
    }

    /// getBaseConstraint (58953-58956): sentinel-filtered immediate.
    fn get_base_constraint_inner(
        &mut self,
        t: TypeId,
        stack: &mut Vec<crate::engine::RecursionIdentity>,
    ) -> CheckResult2<Option<TypeId>> {
        let c = self.get_immediate_base_constraint(t, stack)?;
        Ok((c != self.no_constraint_type && c != self.circular_constraint_type).then_some(c))
    }

    /// computeBaseConstraint (58957-59024).
    fn compute_base_constraint(
        &mut self,
        t: TypeId,
        stack: &mut Vec<crate::engine::RecursionIdentity>,
    ) -> CheckResult2<Option<TypeId>> {
        let flags = self.tables.flags_of(t);
        if flags.intersects(TypeFlags::TYPE_PARAMETER) {
            let constraint = self.get_constraint_from_type_parameter(t)?;
            let is_this_type = matches!(
                self.tables.type_of(t).data,
                TypeData::TypeParameter {
                    is_this_type: true,
                    ..
                }
            );
            return match constraint {
                Some(constraint) if !is_this_type => {
                    self.get_base_constraint_inner(constraint, stack)
                }
                other => Ok(other),
            };
        }
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let types: Vec<TypeId> = match &self.tables.type_of(t).data {
                TypeData::Union { types, .. } => types.to_vec(),
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("union/intersection flag implies member data"),
            };
            let mut base_types: Vec<TypeId> = Vec::new();
            let mut different = false;
            for member in &types {
                match self.get_base_constraint_inner(*member, stack)? {
                    Some(base) => {
                        if base != *member {
                            different = true;
                        }
                        base_types.push(base);
                    }
                    None => different = true,
                }
            }
            if !different {
                return Ok(Some(t));
            }
            if flags.intersects(TypeFlags::UNION) && base_types.len() == types.len() {
                return Ok(Some(self.get_union_type_ex(
                    &base_types,
                    tsrs2_types::UnionReduction::Literal,
                )?));
            }
            if flags.intersects(TypeFlags::INTERSECTION) && !base_types.is_empty() {
                return Ok(Some(self.get_intersection_type(
                    &base_types,
                    tsrs2_types::IntersectionFlags::NONE,
                )?));
            }
            return Ok(None);
        }
        if flags.intersects(TypeFlags::INDEX) {
            return Ok(Some(self.tables.intrinsics.string_number_symbol));
        }
        if flags.intersects(TypeFlags::TEMPLATE_LITERAL) {
            let (texts, types) = match &self.tables.type_of(t).data {
                TypeData::TemplateLiteral { texts, types } => (texts.to_vec(), types.to_vec()),
                _ => unreachable!("template flag implies template data"),
            };
            let mut constraints: Vec<TypeId> = Vec::new();
            for member in &types {
                if let Some(constraint) = self.get_base_constraint_inner(*member, stack)? {
                    constraints.push(constraint);
                }
            }
            return Ok(Some(if constraints.len() == types.len() {
                self.tables
                    .get_template_literal_type_from_texts(&texts, &constraints)
            } else {
                self.tables.intrinsics.string
            }));
        }
        if flags.intersects(TypeFlags::STRING_MAPPING) {
            // 58996-58999: the operand's base constraint, re-mapped —
            // or stringType when the operand has none.
            let TypeData::StringMapping { ty: inner } = self.tables.type_of(t).data else {
                unreachable!("string-mapping flag implies string-mapping data");
            };
            let constraint = self.get_base_constraint_inner(inner, stack)?;
            return Ok(Some(match constraint {
                Some(constraint) if constraint != inner => {
                    let symbol = self
                        .tables
                        .type_of(t)
                        .symbol
                        .expect("string-mapping types carry the intrinsic symbol");
                    self.get_string_mapping_type(symbol, constraint)?
                }
                _ => self.tables.intrinsics.string,
            }));
        }
        if flags.intersects(TypeFlags::INDEXED_ACCESS) {
            // 59000-59008: base constraints of both sides re-access.
            // substituteIndexedMappedType is a 9.5c consumer and must
            // not be skipped now that mapped objects are constructible.
            let TypeData::IndexedAccess {
                object_type,
                index_type,
                access_flags,
            } = self.tables.type_of(t).data
            else {
                unreachable!("indexed-access flag implies indexed-access data");
            };
            if self.is_generic_mapped_type_state(object_type)? {
                return Err(Unsupported::new(
                    "base constraint of a generic mapped indexed access \
                     (substituteIndexedMappedType, 9.5c/M8)",
                ));
            }
            let base_object = self.get_base_constraint_inner(object_type, stack)?;
            let base_index = self.get_base_constraint_inner(index_type, stack)?;
            let base_indexed_access = match (base_object, base_index) {
                (Some(base_object), Some(base_index)) => self
                    .get_indexed_access_type_or_undefined(
                        base_object,
                        base_index,
                        access_flags,
                        None,
                        None,
                        None,
                    )?,
                _ => None,
            };
            return match base_indexed_access {
                Some(base) => self.get_base_constraint_inner(base, stack),
                None => Ok(None),
            };
        }
        if flags.intersects(TypeFlags::CONDITIONAL | TypeFlags::SUBSTITUTION) {
            return Err(Unsupported::new(
                "computeBaseConstraint for Conditional/Substitution (M8 — those \
                 TypeFlags are unconstructible before their type nodes land)",
            ));
        }
        if self.is_generic_tuple_type(t) {
            // 59016-59022: variadic type-parameter elements step to
            // their base constraints when every constituent of the
            // constraint is a non-generic array/tuple.
            let target = self.tables.reference_target(t);
            let TypeData::TupleTarget(data) = self.tables.type_of(target).data.clone() else {
                unreachable!("generic tuple targets a tuple target");
            };
            // getElementTypes (61313-61316): arguments sliced to arity.
            let mut elements = self.get_type_arguments(t)?;
            elements.truncate(data.type_parameters.len());
            let global_array = self.global_array_type()?;
            let global_readonly = self.global_readonly_array_type()?;
            let mut new_elements = Vec::with_capacity(elements.len());
            for (i, &element) in elements.iter().enumerate() {
                let mut new_element = element;
                if self
                    .tables
                    .flags_of(element)
                    .intersects(TypeFlags::TYPE_PARAMETER)
                    && data.element_flags[i].intersects(ElementFlags::VARIADIC)
                {
                    if let Some(constraint) = self.get_base_constraint_inner(element, stack)? {
                        if constraint != element
                            && self.every_type(constraint, |state, c| {
                                let is_array = state
                                    .tables
                                    .object_flags_of(c)
                                    .intersects(ObjectFlags::REFERENCE)
                                    && {
                                        let target = state.tables.reference_target(c);
                                        target == global_array || target == global_readonly
                                    };
                                (is_array || state.tables.is_tuple_type(c))
                                    && !state.tables.is_generic_tuple_type(c)
                            })
                        {
                            new_element = constraint;
                        }
                    }
                }
                new_elements.push(new_element);
            }
            return Ok(Some(self.create_tuple_type_forced(
                &new_elements,
                Some(&data.element_flags[..]),
                data.readonly,
                data.labeled_element_declarations.as_deref(),
            )?));
        }
        Ok(Some(t))
    }

    /// tsc-port: someType @6.0.3
    /// tsc-hash: 8e5789de3a26e5360e1ab114a5ebff02124f84c975c9a78322fec459e3dc336d
    /// tsc-span: _tsc.js:69982-69984
    pub fn some_type(&self, ty: TypeId, f: impl Fn(&Self, TypeId) -> bool) -> bool {
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            let TypeData::Union { types, .. } = &self.tables.type_of(ty).data else {
                unreachable!("union flag implies union data");
            };
            return types.iter().any(|&member| f(self, member));
        }
        f(self, ty)
    }

    /// tsc-port: everyType @6.0.3
    /// tsc-hash: c4dd71e9f3d68c0f125a76cc961b9eafb0217706e0cfa0c096d2df786cc22253
    /// tsc-span: _tsc.js:69985-69987
    pub fn every_type(&self, ty: TypeId, f: impl Fn(&Self, TypeId) -> bool) -> bool {
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            let TypeData::Union { types, .. } = &self.tables.type_of(ty).data else {
                unreachable!("union flag implies union data");
            };
            return types.iter().all(|&member| f(self, member));
        }
        f(self, ty)
    }

    /// Union constituents, or the type itself (the someType/everyType
    /// traversal set).
    pub(crate) fn union_members_or_self(&self, ty: TypeId) -> Vec<TypeId> {
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            }
        } else {
            vec![ty]
        }
    }

    /// tsc-port: isGenericStringLikeType @6.0.3
    /// tsc-hash: bab14bed10f5a2ddbf5b7c73efdb4a5897ad9d2e48b2d79734b4deaa3f525617
    /// tsc-span: _tsc.js:62428-62430
    pub fn is_generic_string_like_type(&self, ty: TypeId) -> bool {
        self.tables
            .flags_of(ty)
            .intersects(TypeFlags::TEMPLATE_LITERAL | TypeFlags::STRING_MAPPING)
            && !self.tables.is_pattern_literal_type(ty)
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::{CompilerOptions, SymbolFlags, TypeFlags};

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    fn annotation_of_var(state: &CheckerState, name: &str) -> tsrs2_syntax::NodeId {
        crate::relpin::find_probe_annotation(state.binder.source(0), name)
            .expect("var with annotation")
    }

    fn declared_type_parameter(state: &mut CheckerState, name: &str) -> tsrs2_types::TypeId {
        let symbol = state
            .resolve_name(
                Some(state.binder.source(0).root),
                name,
                SymbolFlags::TYPE_PARAMETER,
                None,
                false,
                false,
            )
            .expect("resolve_name")
            .or_else(|| {
                // Type parameters live in their container's scope; walk
                // from the first identifier inside the function body.
                let source = state.binder.source(0);
                let inside = source
                    .arena
                    .node_ids()
                    .find(|&id| {
                        source.arena.node(id).kind == tsrs2_syntax::SyntaxKind::VariableDeclaration
                    })
                    .expect("var declaration");
                state
                    .resolve_name(
                        Some(inside),
                        name,
                        SymbolFlags::TYPE_PARAMETER,
                        None,
                        false,
                        false,
                    )
                    .expect("resolve_name")
            })
            .expect("type parameter resolves");
        state.get_declared_type_of_type_parameter(symbol)
    }

    #[test]
    fn intersection_with_covering_constraint_collapses_to_type_parameter() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T extends string>() { var v: T & string; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("intersection resolves");
                let t = declared_type_parameter(state, "T");
                assert_eq!(resolved, t, "T & string collapses to T (step 6)");
            },
        );
    }

    #[test]
    fn intersection_with_disjoint_primitive_collapses_to_never() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T extends string>() { var v: T & number; }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("intersection resolves");
                assert_eq!(resolved, state.tables.intrinsics.never);
            },
        );
    }

    #[test]
    fn union_of_constrained_intersections_collapses_to_type_parameter() {
        with_program_state(
            &[(
                "a.ts",
                "function f<T extends \"a\" | \"b\">() { var v: (T & \"a\") | (T & \"b\"); }\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("union resolves");
                let t = declared_type_parameter(state, "T");
                assert_eq!(
                    resolved, t,
                    "removeConstrainedTypeVariables collapses the union to T"
                );
            },
        );
    }

    #[test]
    fn circular_constraint_reports_2313_and_disables_collapse() {
        with_program_state(
            &[("a.ts", "function f<T extends T>() { var v: T & string; }\n")],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("intersection resolves without collapse");
                // No collapse: the intersection interns as-is.
                assert!(state
                    .tables
                    .flags_of(resolved)
                    .intersects(TypeFlags::INTERSECTION));
                let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
                assert_eq!(codes, [2313]);
                let t = declared_type_parameter(state, "T");
                let constraint = state
                    .get_constraint_of_type_parameter(t)
                    .expect("constraint query in slice");
                assert_eq!(constraint, None, "circular constraint yields none");
            },
        );
    }

    #[test]
    fn unconstrained_type_parameter_intersections_intern_plainly() {
        with_program_state(
            &[("a.ts", "function f<T>() { var v: T & string; }\n")],
            &CompilerOptions::default(),
            |state| {
                let annotation = annotation_of_var(state, "v");
                let resolved = state
                    .get_type_from_type_node(annotation)
                    .expect("intersection resolves");
                assert!(state
                    .tables
                    .flags_of(resolved)
                    .intersects(TypeFlags::INTERSECTION));
                assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            },
        );
    }
}
