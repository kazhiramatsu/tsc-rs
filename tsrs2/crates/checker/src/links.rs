//! Links tables — the memo policy in one place (greenfield §4.3).
//!
//! Every slot is written ONCE; in-progress states are the explicit
//! `Resolving` value (mirroring tsc's resolving sentinels), never an
//! implicit absence. Speculative checking must not write links: every
//! write asserts the checker-wide speculation depth is zero (the
//! single rule that replaces the quiet/expr_type_cache pollution
//! family). M3 has no speculation yet — the assertion is the contract
//! future stages inherit.

use std::collections::HashMap;

use tsrs2_binder::SymbolId;
use tsrs2_syntax::NodeId;
use tsrs2_types::TypeId;

use crate::instantiate::MapperId;
use crate::state::SignatureId;

/// One memo slot: Vacant → Resolving → Resolved, one transition each.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum LinkSlot<T> {
    #[default]
    Vacant,
    Resolving,
    Resolved(T),
}

impl<T: Clone> LinkSlot<T> {
    pub fn resolved(&self) -> Option<T> {
        match self {
            LinkSlot::Resolved(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn is_resolving(&self) -> bool {
        matches!(self, LinkSlot::Resolving)
    }
}

/// tsc NodeLinks (getNodeLinks) — the per-node subset M3 consumes.
#[derive(Clone, Debug, Default)]
pub struct NodeLinks {
    /// tsc links.resolvedType (per-arm caching in the
    /// getTypeFromTypeNode workers).
    pub resolved_type: LinkSlot<TypeId>,
    /// tsc links.resolvedSignature (getSignatureFromDeclaration 59570).
    pub resolved_signature: LinkSlot<SignatureId>,
    /// tsc links.outerTypeParameters (getObjectTypeInstantiation 63466)
    /// on the instantiated type's declaration node.
    pub outer_type_parameters: LinkSlot<Box<[TypeId]>>,
    /// tsc links.resolvedSymbol (getResolvedSymbol 69389) — the
    /// unknownSymbol failure sentinel is cached like tsc's.
    pub resolved_symbol: LinkSlot<SymbolId>,
}

/// tsc SymbolLinks — the per-symbol subset M3 consumes.
#[derive(Clone, Debug, Default)]
pub struct SymbolLinks {
    /// tsc links.declaredType (getDeclaredTypeOfClassOrInterface 57381).
    pub declared_type: LinkSlot<TypeId>,
    /// tsc links.type (getTypeOfVariableOrParameterOrProperty 56633).
    pub type_of_symbol: LinkSlot<TypeId>,
    /// tsc TransientSymbol links.checkFlags (synthetic union/
    /// intersection properties, createUnionOrIntersectionProperty).
    pub check_flags: tsrs2_types::CheckFlags,
    /// tsc links.containingType for synthetic properties.
    pub containing_type: Option<TypeId>,
    /// tsc links.isDiscriminantProperty cache (isDiscriminantProperty
    /// 69562).
    pub is_discriminant_property: Option<bool>,
    /// tsc links.target for CheckFlags::INSTANTIATED symbols
    /// (instantiateSymbol 63455).
    pub target: Option<SymbolId>,
    /// tsc links.mapper for CheckFlags::INSTANTIATED symbols (63456).
    pub mapper: Option<MapperId>,
    /// tsc links.nameType — written by late-bound member binding (5.3);
    /// carried through instantiateSymbol's copy (63460).
    pub name_type: Option<TypeId>,
}

/// Resolved-members store — tsc keeps these directly on the type
/// object (setStructuredTypeMembers); a side table keeps Type immutable
/// after interning.
#[derive(Clone, Debug, Default)]
pub struct TypeLinks {
    pub resolved_members: LinkSlot<crate::state::MembersId>,
    /// tsc unionOrIntersection type.resolvedProperties
    /// (getPropertiesOfUnionOrIntersectionType 58721).
    pub resolved_properties: LinkSlot<Box<[SymbolId]>>,
    /// tsc unionType.keyPropertyName/constituentMap (getKeyPropertyName
    /// 69612): None name = the "" no-key-property sentinel.
    pub union_key_property: LinkSlot<UnionKeyProperty>,
    /// tsc unionType.resolvedReducedType (getReducedType 59289 +
    /// getReducedUnionType's self-stamp 59305).
    pub resolved_reduced_type: LinkSlot<TypeId>,
    /// tsc TypeParameter.constraint (getConstraintFromTypeParameter
    /// 60103) — Resolved(noConstraintType sentinel) = computed, none.
    pub type_parameter_constraint: LinkSlot<TypeId>,
    /// tsc type.resolvedBaseConstraint (getResolvedBaseConstraint
    /// 58916-58920).
    pub resolved_base_constraint: LinkSlot<TypeId>,
    /// tsc type.immediateBaseConstraint (getImmediateBaseConstraint
    /// 58921-58951; the ImmediateBaseConstraint resolution property).
    pub immediate_base_constraint: LinkSlot<TypeId>,
    /// tsc type.target for ObjectFlags::INSTANTIATED anonymous types
    /// (instantiateAnonymousType 63658).
    pub instantiated_target: Option<TypeId>,
    /// tsc type.mapper for ObjectFlags::INSTANTIATED anonymous types
    /// (63659).
    pub instantiated_mapper: Option<MapperId>,
    /// tsc TypeParameter.target (cloneTypeParameter 63403 /
    /// getRestrictiveTypeParameter 63400).
    pub type_parameter_target: Option<TypeId>,
    /// tsc TypeParameter.mapper (instantiateSignature 63418).
    pub type_parameter_mapper: Option<MapperId>,
    /// tsc type.permissiveInstantiation (getPermissiveInstantiation
    /// 63815).
    pub permissive_instantiation: LinkSlot<TypeId>,
    /// tsc type.restrictiveInstantiation (getRestrictiveInstantiation
    /// 63818; the result self-stamp makes the second write idempotent).
    pub restrictive_instantiation: LinkSlot<TypeId>,
}

/// The getKeyPropertyName cache payload.
#[derive(Clone, Debug, Default)]
pub struct UnionKeyProperty {
    pub name: Option<String>,
    pub constituent_map: Option<std::collections::HashMap<TypeId, TypeId>>,
}

#[derive(Debug, Default)]
pub struct LinksTables {
    node: HashMap<NodeId, NodeLinks>,
    symbol: HashMap<SymbolId, SymbolLinks>,
    ty: HashMap<TypeId, TypeLinks>,
    /// tsc unionType.propertyCache / propertyCacheWithoutObjectFunctionPropertyAugment
    /// (getUnionOrIntersectionProperty 59246) — a monotone cache, not a
    /// one-write slot; only successful synthesis is cached, like tsc.
    pub union_property_cache: HashMap<(TypeId, String, bool), SymbolId>,
}

impl LinksTables {
    pub fn node(&self, id: NodeId) -> NodeLinks {
        self.node.get(&id).cloned().unwrap_or_default()
    }

    pub fn symbol(&self, id: SymbolId) -> SymbolLinks {
        self.symbol.get(&id).cloned().unwrap_or_default()
    }

    pub fn ty(&self, id: TypeId) -> TypeLinks {
        self.ty.get(&id).cloned().unwrap_or_default()
    }

    fn assert_writable(speculation_depth: u32) {
        assert_eq!(
            speculation_depth, 0,
            "links writes are forbidden during speculation (greenfield §4.3)"
        );
    }

    fn write_slot<T: Clone + std::fmt::Debug>(slot: &mut LinkSlot<T>, next: LinkSlot<T>) {
        match (&*slot, &next) {
            (LinkSlot::Vacant, _) | (LinkSlot::Resolving, LinkSlot::Resolved(_)) => *slot = next,
            _ => panic!("links slot rewritten: {slot:?} -> {next:?}"),
        }
    }

    pub fn set_node_resolved_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: LinkSlot<TypeId>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(&mut self.node.entry(id).or_default().resolved_type, value);
    }

    pub fn set_node_resolved_signature(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: LinkSlot<SignatureId>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.node.entry(id).or_default().resolved_signature,
            value,
        );
    }

    pub fn set_symbol_declared_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(&mut self.symbol.entry(id).or_default().declared_type, value);
    }

    pub fn set_symbol_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.symbol.entry(id).or_default().type_of_symbol,
            value,
        );
    }

    pub fn set_symbol_synthetic(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        check_flags: tsrs2_types::CheckFlags,
        containing_type: TypeId,
        type_of_symbol: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        let links = self.symbol.entry(id).or_default();
        links.check_flags = check_flags;
        links.containing_type = Some(containing_type);
        Self::write_slot(
            &mut links.type_of_symbol,
            LinkSlot::Resolved(type_of_symbol),
        );
    }

    pub fn set_symbol_is_discriminant(&mut self, id: SymbolId, value: bool) {
        self.symbol.entry(id).or_default().is_discriminant_property = Some(value);
    }

    /// tsc createSymbol's checkFlags seed (47656) for transient symbols
    /// created outside the synthetic-property path.
    pub fn set_symbol_check_flags(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        check_flags: tsrs2_types::CheckFlags,
    ) {
        Self::assert_writable(speculation_depth);
        self.symbol.entry(id).or_default().check_flags = check_flags;
    }

    pub fn set_type_resolved_properties(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: Box<[SymbolId]>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().resolved_properties,
            LinkSlot::Resolved(value),
        );
    }

    pub fn set_type_resolved_reduced_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().resolved_reduced_type,
            LinkSlot::Resolved(value),
        );
    }

    pub fn set_type_union_key_property(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: UnionKeyProperty,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().union_key_property,
            LinkSlot::Resolved(value),
        );
    }

    pub fn set_type_parameter_constraint(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().type_parameter_constraint,
            LinkSlot::Resolved(value),
        );
    }

    pub fn set_type_resolved_base_constraint(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().resolved_base_constraint,
            LinkSlot::Resolved(value),
        );
    }

    pub fn set_type_immediate_base_constraint(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().immediate_base_constraint,
            LinkSlot::Resolved(value),
        );
    }

    pub fn set_node_resolved_symbol(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: SymbolId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.node.entry(id).or_default().resolved_symbol,
            LinkSlot::Resolved(value),
        );
    }

    pub fn set_node_outer_type_parameters(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: Box<[TypeId]>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.node.entry(id).or_default().outer_type_parameters,
            LinkSlot::Resolved(value),
        );
    }

    /// instantiateSymbol's transient-links seed (63455-63461): target +
    /// mapper (+ the nameType copy) written once at creation.
    pub fn set_symbol_instantiation_links(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        target: SymbolId,
        mapper: MapperId,
        name_type: Option<TypeId>,
    ) {
        Self::assert_writable(speculation_depth);
        let links = self.symbol.entry(id).or_default();
        assert!(
            links.target.is_none() && links.mapper.is_none(),
            "instantiation links written twice for {id:?}"
        );
        links.target = Some(target);
        links.mapper = Some(mapper);
        links.name_type = name_type;
    }

    /// instantiateAnonymousType's target/mapper seed (63658-63659),
    /// written once at creation of the instantiated shell.
    pub fn set_type_instantiation_links(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        target: TypeId,
        mapper: MapperId,
    ) {
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        assert!(
            links.instantiated_target.is_none() && links.instantiated_mapper.is_none(),
            "type instantiation links written twice for {id:?}"
        );
        links.instantiated_target = Some(target);
        links.instantiated_mapper = Some(mapper);
    }

    /// cloneTypeParameter/getRestrictiveTypeParameter target stamp.
    pub fn set_type_parameter_target(&mut self, speculation_depth: u32, id: TypeId, target: TypeId) {
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        assert!(
            links.type_parameter_target.is_none(),
            "type parameter target written twice for {id:?}"
        );
        links.type_parameter_target = Some(target);
    }

    /// instantiateSignature's fresh-parameter mapper stamp (63418).
    pub fn set_type_parameter_mapper(&mut self, speculation_depth: u32, id: TypeId, mapper: MapperId) {
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        assert!(
            links.type_parameter_mapper.is_none(),
            "type parameter mapper written twice for {id:?}"
        );
        links.type_parameter_mapper = Some(mapper);
    }

    pub fn set_type_permissive_instantiation(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().permissive_instantiation,
            LinkSlot::Resolved(value),
        );
    }

    /// getRestrictiveInstantiation self-stamps its result (63825-63826),
    /// so a second write with the SAME value is tolerated.
    pub fn set_type_restrictive_instantiation(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        let slot = &mut self.ty.entry(id).or_default().restrictive_instantiation;
        match &*slot {
            LinkSlot::Resolved(existing) if *existing == value => {}
            _ => Self::write_slot(slot, LinkSlot::Resolved(value)),
        }
    }

    pub fn set_type_members(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: LinkSlot<crate::state::MembersId>,
    ) {
        Self::assert_writable(speculation_depth);
        let slot = &mut self.ty.entry(id).or_default().resolved_members;
        // setStructuredTypeMembers writes an empty table first as a
        // re-entrancy guard, then the real one (58333/58339) — allow
        // Resolved -> Resolved for that one tsc-shaped double write.
        match (&*slot, &value) {
            (LinkSlot::Resolved(_), LinkSlot::Resolved(_)) => *slot = value,
            _ => Self::write_slot(slot, value),
        }
    }
}
