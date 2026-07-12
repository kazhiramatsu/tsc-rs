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
    /// tsc links.enumMemberValue (computeEnumMemberValues 85587) on
    /// EnumMember nodes.
    pub enum_member_value: Option<crate::evaluate::EvaluatorResult>,
    /// tsc NodeLinks.flags & EnumValuesComputed (85582) on
    /// EnumDeclaration nodes. Unlike tsc this REVERTS on Unsupported
    /// unwind so a later query recomputes the tail of the member list.
    pub enum_values_computed: bool,
    /// tsc NodeLinks.flags (getNodeCheckFlags) — the driver's
    /// TypeChecked bit lands with M4 5.4; later stages OR in their own
    /// bits (a flags word accumulates, unlike the write-once slots).
    pub check_flags: tsrs2_types::NodeCheckFlags,
    /// tsc links.hasReportedStatementInAmbientContext
    /// (checkGrammarStatementInAmbientContext 90341): the once-flag on
    /// the offending statement OR its enclosing block. Stays false when
    /// grammarErrorOnFirstToken is parse-diagnostics-suppressed, like
    /// tsc's `links.x = grammarError(...)` assignment.
    pub has_reported_statement_in_ambient_context: bool,
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
    /// tsc links.typeParameters for generic type-alias symbols
    /// (getDeclaredTypeOfTypeAlias 57416).
    pub type_parameters: Option<Vec<TypeId>>,
    /// tsc links.resolvedMembers (getResolvedMembersOrExportsOfSymbol
    /// 57712) — the early⊕late member table of a late-binding
    /// container; equal to `symbol.members` while no late-bindable
    /// member exists (the pre-5.5 slice).
    pub resolved_members: LinkSlot<tsrs2_binder::SymbolTable>,
    /// tsc links.tupleLabelDeclaration (createTupleTargetType 61170):
    /// the NamedTupleMember/Parameter node behind a synthesized tuple
    /// index property.
    pub tuple_label_declaration: Option<NodeId>,
    /// tsc links.writeType (getWriteTypeOfAccessors 56787) — the
    /// setter-side type; the WriteType resolution property.
    pub write_type: LinkSlot<TypeId>,
    /// tsc links.resolvedExports (getResolvedMembersOrExportsOfSymbol
    /// 57712, the static resolutionKind) — equal to `symbol.exports`
    /// while no late-bindable static member exists.
    pub resolved_exports: LinkSlot<tsrs2_binder::SymbolTable>,
    /// tsc links.variances (getVariancesWorker 67315): Vacant =
    /// undefined, Resolving = the in-progress emptyArray sentinel
    /// (getVariances call sites answer Ternary.Unknown), Resolved =
    /// the measured list — possibly genuinely empty for zero-parameter
    /// alias symbols, which is DISTINCT from the sentinel exactly as
    /// tsc's fresh `[]` differs from the shared emptyArray.
    pub variances: LinkSlot<Box<[tsrs2_types::VarianceFlags]>>,
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
    /// tsc TypeParameter.default (getResolvedTypeParameterDefault
    /// 59043) — Resolved(noConstraintType) = computed, none;
    /// Resolved(circularConstraintType) = the cycle sentinel. The
    /// resolvingDefaultType in-flight sentinel is the checker's
    /// in-progress set, so Err unwinds stay re-queryable.
    pub type_parameter_default: LinkSlot<TypeId>,
    /// tsc type.resolvedIndexType / resolvedStringIndexType
    /// (getIndexTypeForGenericType 61932).
    pub resolved_index_type: LinkSlot<TypeId>,
    pub resolved_string_index_type: LinkSlot<TypeId>,
    /// tsc type.uniqueLiteralFilledInstantiation (isReducibleIntersection
    /// 59322).
    pub unique_literal_filled_instantiation: LinkSlot<TypeId>,
    /// tsc type.permissiveInstantiation (getPermissiveInstantiation
    /// 63815).
    pub permissive_instantiation: LinkSlot<TypeId>,
    /// tsc type.restrictiveInstantiation (getRestrictiveInstantiation
    /// 63818; the result self-stamp makes the second write idempotent).
    pub restrictive_instantiation: LinkSlot<TypeId>,
    /// tsc TypeReference.node for DEFERRED references
    /// (createDeferredTypeReference 60196): the TypeReference/ArrayType/
    /// TupleType node the lazy getTypeArguments reads. `Some` IS the
    /// deferred-ness test (isNonDeferredTypeReference 67388 checks
    /// !type.node) — it stays `Some` after the arguments resolve.
    pub deferred_node: Option<NodeId>,
    /// tsc TypeReference.mapper (60197): applied to the node-read
    /// arguments in getTypeArguments (60211); set by
    /// getObjectTypeInstantiation's deferred-reference result arm.
    pub deferred_mapper: Option<MapperId>,
    /// tsc InterfaceTypeWithDeclaredMembers.declaredProperties/
    /// declaredCallSignatures/declaredConstructSignatures/
    /// declaredIndexInfos (resolveDeclaredMembers 57602) — one
    /// ResolvedMembers holding the OWN members, distinct from
    /// resolved_members (which merges heritage).
    pub declared_members: LinkSlot<crate::state::MembersId>,
    /// tsc InterfaceType.resolvedBaseTypes (getBaseTypes 57218).
    /// MUTABLE like tsc's field: interfaces initialize to [] and push
    /// per base; a mid-cycle reader observes the partial list.
    pub resolved_base_types: Option<Vec<TypeId>>,
    /// tsc InterfaceType.baseTypesResolved (57224/57244) — set true
    /// even when the resolution stack flags a cycle, freezing whatever
    /// resolvedBaseTypes holds.
    pub base_types_resolved: bool,
    /// tsc TypeReference.cachedEquivalentBaseType
    /// (getSingleBaseForNonAugmentingSubtype 67713), guarded by the
    /// IdenticalBaseTypeCalculated/Exists object flags.
    pub cached_equivalent_base_type: Option<TypeId>,
    /// tsc InterfaceType.resolvedBaseConstructorType
    /// (getBaseConstructorTypeOfClass 57146) — the checked extends
    /// expression type; the ResolvedBaseConstructorType resolution
    /// property.
    pub resolved_base_constructor_type: LinkSlot<TypeId>,
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
    /// tsc type-alias links.instantiations (getDeclaredTypeOfTypeAlias
    /// 57417 seed + getTypeAliasInstantiation 60271), keyed by
    /// getTypeListId + getAliasId — a monotone cache like tsc's map.
    pub alias_instantiations: HashMap<(SymbolId, String), TypeId>,
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

    /// getTypeFromTypeReference's tail assignments (60587-60588) are
    /// UNGUARDED in tsc: the resolvingDefaultType recursion
    /// (getResolvedTypeParameterDefault 59043) can re-enter the SAME
    /// reference node mid-computation, so the inner call caches first
    /// and the outer assignment overwrites it — the node's final
    /// resolved type/symbol is the OUTER result. This is the only
    /// write-twice site the memo discipline sanctions; both slots move
    /// together.
    pub fn overwrite_type_reference_resolution(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        symbol: SymbolId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        let links = self.node.entry(id).or_default();
        links.resolved_symbol = LinkSlot::Resolved(symbol);
        links.resolved_type = LinkSlot::Resolved(value);
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

    pub fn set_symbol_variances(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<Box<[tsrs2_types::VarianceFlags]>>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(&mut self.symbol.entry(id).or_default().variances, value);
    }

    /// Err-unwind twin for the variances slot: tsc cannot fail inside
    /// getVariancesWorker, so a measurement cut short by Unsupported
    /// must leave the slot re-queryable — Resolving reverts to Vacant.
    pub fn revert_symbol_variances(&mut self, id: SymbolId) {
        let slot = &mut self.symbol.entry(id).or_default().variances;
        assert!(
            matches!(slot, LinkSlot::Resolving),
            "variances revert without an in-progress measurement for {id:?}"
        );
        *slot = LinkSlot::Vacant;
    }

    /// `nodeLinks.flags |= bits` — the NodeCheckFlags word accumulates
    /// (checkSourceFileWorker 87057 `links.flags |= NodeCheckFlags.TypeChecked`
    /// is the first writer).
    pub fn or_node_check_flags(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        bits: tsrs2_types::NodeCheckFlags,
    ) {
        Self::assert_writable(speculation_depth);
        let links = self.node.entry(id).or_default();
        links.check_flags =
            tsrs2_types::NodeCheckFlags::from_bits(links.check_flags.bits() | bits.bits());
    }

    /// checkGrammarStatementInAmbientContext's once-flag (90344/90349):
    /// set only when the grammar error actually emitted.
    pub fn set_node_has_reported_statement_in_ambient_context(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
    ) {
        Self::assert_writable(speculation_depth);
        self.node
            .entry(id)
            .or_default()
            .has_reported_statement_in_ambient_context = true;
    }

    pub fn set_node_enum_member_value(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: crate::evaluate::EvaluatorResult,
    ) {
        Self::assert_writable(speculation_depth);
        let slot = &mut self.node.entry(id).or_default().enum_member_value;
        assert!(slot.is_none(), "enum member value rewritten");
        *slot = Some(value);
    }

    pub fn set_node_enum_values_computed(&mut self, speculation_depth: u32, id: NodeId) {
        Self::assert_writable(speculation_depth);
        self.node.entry(id).or_default().enum_values_computed = true;
    }

    /// Unsupported-unwind twin of set_node_enum_values_computed — the
    /// once-flag must not stay observable after a failed compute
    /// (member value slots that DID fill are correct facts and stay).
    pub fn revert_node_enum_values_computed(&mut self, speculation_depth: u32, id: NodeId) {
        Self::assert_writable(speculation_depth);
        self.node.entry(id).or_default().enum_values_computed = false;
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

    /// getResolvedMembersOrExportsOfSymbol's links[resolutionKind]
    /// cache (57717/57763).
    pub fn set_symbol_resolved_members(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: tsrs2_binder::SymbolTable,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.symbol.entry(id).or_default().resolved_members,
            LinkSlot::Resolved(value),
        );
    }

    /// resolveDeclaredMembers' declared-members stamp (57604-57613),
    /// written once per class/interface/tuple target.
    pub fn set_type_declared_members(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: crate::state::MembersId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().declared_members,
            LinkSlot::Resolved(value),
        );
    }

    /// The tsc-mutable `type.resolvedBaseTypes` assignment (57225,
    /// 57253, 57320-57332): interfaces re-assign and push, so this
    /// setter deliberately allows overwrite.
    pub fn set_type_resolved_base_types(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: Vec<TypeId>,
    ) {
        Self::assert_writable(speculation_depth);
        self.ty.entry(id).or_default().resolved_base_types = Some(value);
    }

    /// `type.baseTypesResolved = true` (57244).
    pub fn set_type_base_types_resolved(&mut self, speculation_depth: u32, id: TypeId) {
        Self::assert_writable(speculation_depth);
        self.ty.entry(id).or_default().base_types_resolved = true;
    }

    /// getWriteTypeOfAccessors' links.writeType stamp (56800).
    pub fn set_symbol_write_type(&mut self, speculation_depth: u32, id: SymbolId, value: TypeId) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.symbol.entry(id).or_default().write_type,
            LinkSlot::Resolved(value),
        );
    }

    /// getResolvedMembersOrExportsOfSymbol's static-side cache (57763).
    pub fn set_symbol_resolved_exports(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: tsrs2_binder::SymbolTable,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.symbol.entry(id).or_default().resolved_exports,
            LinkSlot::Resolved(value),
        );
    }

    /// getBaseConstructorTypeOfClass's resolvedBaseConstructorType
    /// stamp (57154/57186 — the ??= writes).
    pub fn set_type_resolved_base_constructor_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().resolved_base_constructor_type,
            LinkSlot::Resolved(value),
        );
    }

    /// createTupleTargetType's tupleLabelDeclaration stamp (61170).
    pub fn set_symbol_tuple_label_declaration(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        declaration: NodeId,
    ) {
        Self::assert_writable(speculation_depth);
        let links = self.symbol.entry(id).or_default();
        assert!(
            links.tuple_label_declaration.is_none(),
            "tuple label written twice for {id:?}"
        );
        links.tuple_label_declaration = Some(declaration);
    }

    /// getSingleBaseForNonAugmentingSubtype's cachedEquivalentBaseType
    /// stamp (67713), guarded by IdenticalBaseTypeCalculated.
    pub fn ty_mut_cached_equivalent_base_type(&mut self, id: TypeId, value: TypeId) {
        let links = self.ty.entry(id).or_default();
        assert!(
            links.cached_equivalent_base_type.is_none(),
            "equivalent base type written twice for {id:?}"
        );
        links.cached_equivalent_base_type = Some(value);
    }

    /// The Err-unwind retraction for the members slot: tsc has no
    /// failure mode here (setStructuredTypeMembers always completes),
    /// so a partially-populated table left by an Unsupported unwind
    /// must not be observable — the slot reverts to Vacant and a later
    /// query re-resolves.
    pub fn retract_type_members(&mut self, id: TypeId) {
        let slot = &mut self.ty.entry(id).or_default().resolved_members;
        assert!(
            matches!(slot, LinkSlot::Resolved(_)),
            "retract without a members write for {id:?}"
        );
        *slot = LinkSlot::Vacant;
    }

    /// createDeferredTypeReference's node/mapper stamp (60196-60197),
    /// written once when the deferred reference shell is created.
    pub fn set_type_deferred_reference_links(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        node: NodeId,
        mapper: Option<MapperId>,
    ) {
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        assert!(
            links.deferred_node.is_none() && links.deferred_mapper.is_none(),
            "deferred reference links written twice for {id:?}"
        );
        links.deferred_node = Some(node);
        links.deferred_mapper = mapper;
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

    pub fn set_type_parameter_default(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().type_parameter_default,
            LinkSlot::Resolved(value),
        );
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

    /// getDeclaredTypeOfTypeAlias's typeParameters stamp (57416),
    /// written once when a generic alias's declared type resolves.
    pub fn set_symbol_type_parameters(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        type_parameters: Vec<TypeId>,
    ) {
        Self::assert_writable(speculation_depth);
        let links = self.symbol.entry(id).or_default();
        assert!(
            links.type_parameters.is_none(),
            "alias type parameters written twice for {id:?}"
        );
        links.type_parameters = Some(type_parameters);
    }

    pub fn set_type_resolved_index_type(&mut self, speculation_depth: u32, id: TypeId, value: TypeId) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().resolved_index_type,
            LinkSlot::Resolved(value),
        );
    }

    pub fn set_type_resolved_string_index_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().resolved_string_index_type,
            LinkSlot::Resolved(value),
        );
    }

    pub fn set_type_unique_literal_filled_instantiation(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self
                .ty
                .entry(id)
                .or_default()
                .unique_literal_filled_instantiation,
            LinkSlot::Resolved(value),
        );
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
