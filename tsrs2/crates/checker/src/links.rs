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
use tsrs2_types::{ConditionalRootId, TypeId};

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

// Debug-only census of OPEN `Resolving` sentinels on this thread.
// Every slot writer below reports its transition; the
// unsupported-unwind invariant reads the census at element/file
// boundaries (check.rs) — a leaked sentinel after an Err unwind is
// the "phantom mid-flight state" bug class the Err-revert twins
// exist for. Thread-local is sound because one program's check runs
// wholly on one thread (the conformance pool parallelizes across
// fixtures, never inside one). Release builds compile the census
// out and always answer 0.
#[cfg(debug_assertions)]
thread_local! {
    static RESOLVING_OPEN: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

#[inline]
fn note_resolving_transition(before: bool, after: bool) {
    #[cfg(debug_assertions)]
    if before != after {
        RESOLVING_OPEN.with(|cell| cell.set(cell.get() + if after { 1 } else { -1 }));
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (before, after);
    }
}

/// tsrs-native: debug census accessor for the unsupported-unwind
/// invariant (no tsc counterpart). The open-`Resolving` census for
/// this thread; 0 whenever no resolution is mid-flight. Debug builds
/// only — release answers 0.
pub fn debug_resolving_open() -> i64 {
    #[cfg(debug_assertions)]
    {
        RESOLVING_OPEN.with(std::cell::Cell::get)
    }
    #[cfg(not(debug_assertions))]
    {
        0
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
    /// tsc links.assertionExpressionType (checkAssertionWorker 77920):
    /// the operand type stashed for checkAssertionDeferred.
    pub assertion_expression_type: Option<TypeId>,
    /// tsc links.instantiationExpressionTypes (getInstantiationExpressionType
    /// 77980): exprType.id → instantiated result, STORE-BEFORE-ERROR.
    pub instantiation_expression_types: Option<std::collections::HashMap<TypeId, TypeId>>,
    /// tsc links.hasReportedStatementInAmbientContext
    /// (checkGrammarStatementInAmbientContext 90341): the once-flag on
    /// the offending statement OR its enclosing block. Stays false when
    /// grammarErrorOnFirstToken is parse-diagnostics-suppressed, like
    /// tsc's `links.x = grammarError(...)` assignment.
    pub has_reported_statement_in_ambient_context: bool,
    /// tsc links.contextFreeType (getContextFreeTypeOfExpression 80948).
    pub context_free_type: LinkSlot<TypeId>,
    /// tsc links.parameterInitializerContainsUndefined
    /// (parameterInitializerContainsUndefined 71602) on Parameter
    /// nodes — the removeOptionalityFromDeclaredType input.
    pub parameter_initializer_contains_undefined: Option<bool>,
    /// tsc links.spreadIndices (getContextualType's ArrayLiteral arm
    /// 73520): (first, last) spread element indices, computed once per
    /// array literal (getSpreadIndices 73248).
    pub spread_indices: Option<(Option<u32>, Option<u32>)>,
    /// tsc links.nonExistentPropCheckCache (reportNonexistentProperty
    /// 75417): `{typeId}|{isUncheckedJS}` dedupe keys — a grow-only
    /// diagnostic-path cache, never speculative.
    pub non_existent_prop_check_cache: std::collections::HashSet<String>,
    /// tsc links.jsxFlags (getIntrinsicTagSymbol 74540/74545) on JSX
    /// opening-like/closing elements — an accumulating flags word.
    pub jsx_flags: tsrs2_types::JsxFlags,
    /// tsc links.resolvedJsxElementAttributesType
    /// (getIntrinsicAttributesTypeFromJsxOpeningLikeElement 74731) —
    /// compute-once; written only on success so an Unsupported unwind
    /// re-computes.
    pub resolved_jsx_element_attributes_type: Option<TypeId>,
    /// tsc sourceFileLinks.jsxFragmentType (getJSXFragmentType 77373)
    /// on SourceFile nodes — the per-file fragment factory type memo
    /// (errorType is a real cached verdict, matching tsc).
    pub jsx_fragment_type: Option<TypeId>,
    /// tsc links.decoratorSignature (getESDecoratorCallSignature 78574 /
    /// getLegacyDecoratorCallSignature 78616) on the DECORATED node —
    /// Some(anySignature) is tsc's "no signature" sentinel.
    pub decorator_signature: Option<crate::state::SignatureId>,
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
    /// tsc symbol.isReferenced (markPropertyAsReferenced 75617) — M7
    /// unused-checks bookkeeping, inert until then.
    pub is_referenced: bool,
    /// tsc links.target for CheckFlags::INSTANTIATED symbols
    /// (instantiateSymbol 63455).
    pub target: Option<SymbolId>,
    /// tsc links.mapper for CheckFlags::INSTANTIATED symbols (63456).
    pub mapper: Option<MapperId>,
    /// tsc links.nameType — written by late-bound member binding (5.3);
    /// carried through instantiateSymbol's copy (63460).
    pub name_type: Option<TypeId>,
    /// tsc links.mappedType for CheckFlags::MAPPED property symbols
    /// synthesized by resolveMappedTypeMembers (58549), and for
    /// CheckFlags::REVERSE_MAPPED properties (58446/58449).
    pub mapped_type: Option<TypeId>,
    /// tsc links.propertyType / constraintType for fresh
    /// CheckFlags::REVERSE_MAPPED properties (58442, 58447/58450).
    pub property_type: Option<TypeId>,
    pub constraint_type: Option<TypeId>,
    /// tsc links.keyType for CheckFlags::MAPPED property symbols
    /// (58551); distinct from nameType after key remapping.
    pub key_type: Option<TypeId>,
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
    /// tsc links.uniqueESSymbolType (getESSymbolLikeTypeForNode 63127)
    /// — the per-declaration `unique symbol` type memo.
    pub unique_es_symbol_type: Option<TypeId>,
    /// tsc links.lateSymbol (addDeclarationToLateBoundSymbol 57652) —
    /// the late-bound symbol a member's own binder symbol resolved to.
    pub late_symbol: Option<SymbolId>,
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
    /// tsc links.originatingImport (cloneTypeAsModuleType 49769) — the
    /// import-site provenance an interop module clone carries; read by
    /// the invocation-error related-info band (64900/77252): dormant
    /// stores at M4 (related info is not a T0 observable).
    pub originating_import: Option<NodeId>,
    /// tsc links.leftSpread/rightSpread (getSpreadType 63024-63025):
    /// the merged-optional-property provenance pair. Dormant stores
    /// at M4 (read by getSyntheticElementAccess-side tooling later);
    /// kept for symbol-shape fidelity.
    pub left_spread: Option<SymbolId>,
    pub right_spread: Option<SymbolId>,
    /// tsc links.syntheticOrigin (getSpreadSymbol 63052 /
    /// getAnonymousPartialType 62955). Dormant store like the pair
    /// above.
    pub synthetic_origin: Option<SymbolId>,
    /// tsc links.typeParametersChecked (checkTypeParameterListsIdentical
    /// 84876) — the once-latch on multi-declaration class/interface
    /// symbols.
    pub type_parameters_checked: bool,
    /// tsc symbol.lastAssignmentPos (markNodeAssignments 71523): the
    /// last assignment's extended position in document order; NEGATIVE
    /// = a definite-assignment (`x!`-style or sticky), |i64::MAX| =
    /// "assigned in another function" (unknowable). Position 0 is
    /// treated as unmarked by isPastLastAssignment — tsc's JS
    /// falsiness, kept faithfully there.
    pub last_assignment_pos: Option<i64>,
    /// tsc links.aliasTarget (resolveAlias 49118): Resolving = the
    /// resolvingSymbol sentinel — NOT write-once (the re-entrant
    /// Circular_definition_of_import_alias_0 write and the
    /// sentinel-on-entry unknownSymbol collapse both rewrite it; M4
    /// 5.8d, the resolvedSignature protocol twin).
    pub alias_target: LinkSlot<SymbolId>,
    /// tsc links.typeOnlyDeclaration (markSymbolOfAliasDeclarationIf
    /// TypeOnly 49182): TRI-STATE — None = unset, Some(None) = the
    /// explicit `false` (computed, not type-only), Some(Some(node)) =
    /// the type-only declaration.
    pub type_only_declaration: Option<Option<NodeId>>,
    /// tsc links.typeOnlyExportStarName (49189): the export-star name
    /// when it differs from the source symbol's own name.
    pub type_only_export_star_name: Option<String>,
    /// tsc links.typeOnlyExportStarMap (getExportsOfModule 49841):
    /// written WITH the module-flavor resolved_exports; names whose
    /// only path in is a type-only `export type *` declaration.
    pub type_only_export_star_map: Option<std::collections::HashMap<String, NodeId>>,
    /// tsc links.exportsChecked (checkExternalModuleExports 86445) —
    /// the per-module once-guard.
    pub exports_checked: bool,
    /// tsc links.cjsExportMerged (getCommonJsExportEquals 49697).
    pub cjs_export_merged: Option<SymbolId>,
    /// tsc links.immediateTarget (getImmediateAliasedSymbol 50092) —
    /// compute-once; the inner Option is the target (None = tsc
    /// undefined result, still computed).
    pub immediate_target: Option<Option<SymbolId>>,
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
    /// tsc MappedType.typeParameter (getTypeParameterFromMappedType
    /// 58601): declaration-derived and computed once.
    pub mapped_type_parameter: LinkSlot<TypeId>,
    /// tsc MappedType.constraintType (58604).
    pub mapped_constraint_type: LinkSlot<TypeId>,
    /// tsc MappedType.nameType (58607). The inner None records a mapped
    /// declaration without an `as` clause.
    pub mapped_name_type: LinkSlot<Option<TypeId>>,
    /// tsc MappedType.templateType (58610).
    pub mapped_template_type: LinkSlot<TypeId>,
    /// tsc MappedType.modifiersType (58625), consumed when mapped
    /// members/instantiation land in 9.5b.
    pub mapped_modifiers_type: LinkSlot<TypeId>,
    /// tsc MappedType.containsError, set by a mapped-property type
    /// resolution cycle (58581). This is monotone diagnostic state.
    pub mapped_contains_error: bool,
    /// tsc MappedType.resolvedApparentType
    /// (getApparentTypeOfMappedType 59071).
    pub mapped_apparent_type: LinkSlot<TypeId>,
    /// tsc ConditionalType resolved arm/constraint caches. They are
    /// checker-owned because conditional types are immutable in the
    /// types arena. `Resolved(None)` is the stored false sentinel for
    /// `resolvedConstraintOfDistributive`.
    pub conditional_true_type: LinkSlot<TypeId>,
    pub conditional_false_type: LinkSlot<TypeId>,
    pub conditional_inferred_true_type: LinkSlot<TypeId>,
    pub conditional_default_constraint: LinkSlot<TypeId>,
    pub conditional_constraint_of_distributive: LinkSlot<Option<TypeId>>,
    /// tsc type.resolvedBaseConstraint (getResolvedBaseConstraint
    /// 58916-58920).
    pub resolved_base_constraint: LinkSlot<TypeId>,
    /// tsc type.immediateBaseConstraint (getImmediateBaseConstraint
    /// 58921-58951; the ImmediateBaseConstraint resolution property).
    pub immediate_base_constraint: LinkSlot<TypeId>,
    /// tsc synthType.syntheticType (getTypeWithSyntheticDefaultImportType
    /// 77789-77821) — the esModuleInterop default-wrap memo stamped on
    /// the module type itself.
    pub synthetic_type: Option<TypeId>,
    /// tsc synthType.defaultOnlyType
    /// (getTypeWithSyntheticDefaultOnly 77779-77787): the JSON ESM
    /// default-only wrapper memo.
    pub default_only_type: Option<TypeId>,
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
    /// tsc type.simplifiedForReading / simplifiedForWriting
    /// (getSimplifiedIndexedAccessType 62471-62475). Resolving IS
    /// tsc's circularConstraintType in-flight sentinel (re-entry
    /// returns the type itself); an Unsupported unwind reverts to
    /// Vacant per the unwind invariant.
    pub simplified_for_reading: LinkSlot<TypeId>,
    pub simplified_for_writing: LinkSlot<TypeId>,
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
    /// tsc Type.pattern (getTypeFromObjectBindingPattern 56522 /
    /// getTypeFromArrayBindingPattern 56541): the destructuring pattern
    /// the type was inferred FROM, under includePatternInType only —
    /// read by getContextualTypeForBinaryOperand's `type.pattern` test
    /// (72946) and the literals band. Checker-side because the types
    /// crate is NodeId-free (like tuple_label_declaration).
    pub pattern: Option<NodeId>,
    /// tsc TypeReference.literalType (createArrayLiteralType 74039):
    /// the once-per-reference ArrayLiteral-flagged clone.
    pub literal_type: Option<TypeId>,
    /// tsc PromiseOrAwaitedType.promisedTypeOfPromise
    /// (getPromisedTypeOfPromise 82316) — the memoized `then`
    /// onfulfilled parameter type.
    pub promised_type_of_promise: Option<TypeId>,
    /// tsc PromiseOrAwaitedType.awaitedTypeOfType
    /// (getAwaitedTypeNoAlias 82435) — the memoized awaited unwrap.
    pub awaited_type_of_type: Option<TypeId>,
    /// tsc type.widened (getWidenedTypeWithContext 68022/68049) —
    /// the context-free widening memo; context-carrying calls bypass
    /// it in both directions.
    pub widened: Option<TypeId>,
    /// tsc type[iterationTypesCacheKey] (get/setCachedIterationTypes
    /// 84056-84061): the five §4 verdict slots. `Some(No)` is the
    /// cached noIterationTypes poison — distinguishable from "never
    /// computed" (None), per the m4-58 §4 sentinel rule.
    pub iteration_types_of_iterable: Option<crate::iterate::IterationTypesResult>,
    pub iteration_types_of_async_iterable: Option<crate::iterate::IterationTypesResult>,
    pub iteration_types_of_iterator: Option<crate::iterate::IterationTypesResult>,
    pub iteration_types_of_async_iterator: Option<crate::iterate::IterationTypesResult>,
    pub iteration_types_of_iterator_result: Option<crate::iterate::IterationTypesResult>,
}

/// The getKeyPropertyName cache payload.
#[derive(Clone, Debug, Default)]
pub struct UnionKeyProperty {
    pub name: Option<String>,
    pub constituent_map: Option<std::collections::HashMap<TypeId, TypeId>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpeculativeTypeInstantiationKind {
    UniqueLiteralFilled,
    Permissive,
    Restrictive,
    BaseConstructor,
}

#[derive(Clone, Debug, Default)]
struct SpeculativeConditionalCacheSnapshot {
    true_type: LinkSlot<TypeId>,
    false_type: LinkSlot<TypeId>,
    inferred_true_type: LinkSlot<TypeId>,
    default_constraint: LinkSlot<TypeId>,
    constraint_of_distributive: LinkSlot<Option<TypeId>>,
}

type SpeculativeSymbolVarianceWrite = (u32, SymbolId, LinkSlot<Box<[tsrs2_types::VarianceFlags]>>);
type SpeculativeTypeOnlyAliasWrite = (u32, SymbolId, Option<Option<NodeId>>, Option<String>);

#[derive(Clone, Copy, Debug)]
pub(crate) struct SpeculativeLinksMarks {
    resolved_signatures: usize,
    declaration_signatures: usize,
    resolved_types: usize,
    decorator_signatures: usize,
    context_checked: usize,
    symbol_declared_types: usize,
    symbol_types: usize,
    symbol_write_types: usize,
    unique_es_symbol_types: usize,
    late_symbols: usize,
    symbol_variances: usize,
    symbol_type_parameters: usize,
    alias_targets: usize,
    type_only_aliases: usize,
    alias_instantiations: usize,
    conditional_instantiations: usize,
    type_instantiations: usize,
    conditional_caches: usize,
    type_members: usize,
    simplified_types: usize,
}

#[derive(Debug, Default)]
pub struct LinksTables {
    node: HashMap<NodeId, NodeLinks>,
    symbol: HashMap<SymbolId, SymbolLinks>,
    ty: HashMap<TypeId, TypeLinks>,
    /// Trial-local resolvedSignature protocol writes. Nested call
    /// resolution needs its Resolving sentinel and failure stash while
    /// a candidate is checked. Both rejection and selection restore the
    /// entry state; the enclosing non-speculative call frame owns the
    /// permanent call-node publication.
    speculative_resolved_signature_writes: Vec<(u32, NodeId, LinkSlot<crate::state::SignatureId>)>,
    /// Declaration-site getSignatureFromDeclaration publications.
    /// These must be visible throughout a candidate so every member
    /// view shares one SignatureId. Rejection restores them; selection
    /// commits them (and a nested commit promotes the entry snapshot).
    speculative_declaration_signature_writes:
        Vec<(u32, NodeId, LinkSlot<crate::state::SignatureId>)>,
    /// Trial-local type-node resolution publications. A candidate
    /// needs one stable type identity (and resolving sentinels) while
    /// it runs, but the AST cache must return to its entry state.
    speculative_resolved_type_writes: Vec<(u32, NodeId, LinkSlot<TypeId>, LinkSlot<SymbolId>)>,
    /// Trial-local decorator-signature protocol writes. The
    /// `any_signature` sentinel must remain visible to re-entrant
    /// decorator checks inside the same candidate.
    speculative_decorator_signature_writes: Vec<(u32, NodeId, Option<crate::state::SignatureId>)>,
    /// Trial-local ContextChecked once-flags. Contextual parameter
    /// types use the symbol-type journal; restoring both lets a later
    /// candidate perform its own first contextual check.
    speculative_context_checked_writes: Vec<(u32, NodeId, tsrs2_types::NodeCheckFlags)>,
    /// Trial-local declared-type publications for symbols first forced
    /// by candidate checking.
    speculative_symbol_declared_type_writes: Vec<(u32, SymbolId, LinkSlot<TypeId>)>,
    /// Trial-local value-type publications for symbols first forced by
    /// candidate checking.
    speculative_symbol_type_writes: Vec<(u32, SymbolId, LinkSlot<TypeId>)>,
    /// Trial-local accessor/instantiated-property write-type caches.
    speculative_symbol_write_type_writes: Vec<(u32, SymbolId, LinkSlot<TypeId>)>,
    /// Trial-local unique-symbol type publications.
    speculative_unique_es_symbol_type_writes: Vec<(u32, SymbolId, Option<TypeId>)>,
    /// Trial-local links from binder members to synthesized late-bound
    /// symbols.
    speculative_late_symbol_writes: Vec<(u32, SymbolId, Option<SymbolId>)>,
    /// Trial-local variance measurement publications.
    speculative_symbol_variance_writes: Vec<SpeculativeSymbolVarianceWrite>,
    /// Trial-local type-parameter list publications for generic aliases.
    speculative_symbol_type_parameter_writes: Vec<(u32, SymbolId, Option<Vec<TypeId>>)>,
    /// Trial-local alias-resolution sentinel/final slots.
    speculative_alias_target_writes: Vec<(u32, SymbolId, LinkSlot<SymbolId>)>,
    /// Trial-local type-only alias protocol state. The declaration
    /// sentinel and export-star name are restored together.
    speculative_type_only_alias_writes: Vec<SpeculativeTypeOnlyAliasWrite>,
    /// Trial-local generic-alias instantiation cache publications.
    speculative_alias_instantiation_writes: Vec<(u32, (SymbolId, String), Option<TypeId>)>,
    /// Trial-local conditional-root instantiation cache publications.
    /// A candidate needs stable identities for repeated instantiations,
    /// but those identities must not escape the candidate boundary.
    speculative_conditional_instantiation_writes:
        Vec<(u32, (ConditionalRootId, String), Option<TypeId>)>,
    /// Trial-local special-instantiation caches used by relation and
    /// reduction probes.
    speculative_type_instantiation_writes: Vec<(
        u32,
        TypeId,
        SpeculativeTypeInstantiationKind,
        LinkSlot<TypeId>,
    )>,
    /// Trial-local resolved branch and constraint caches on conditional
    /// types.
    speculative_conditional_cache_writes: Vec<(u32, TypeId, SpeculativeConditionalCacheSnapshot)>,
    /// Trial-local lazy structured-member publications. Fresh semantic
    /// types use `set_fresh_type_members` and are intentionally not
    /// journaled.
    speculative_type_member_writes: Vec<(u32, TypeId, LinkSlot<crate::state::MembersId>)>,
    /// Trial-local indexed-access simplification protocols. Both the
    /// circular sentinel and the completed simplification must remain
    /// visible for the duration of a candidate.
    speculative_simplified_type_writes: Vec<(u32, TypeId, bool, LinkSlot<TypeId>)>,
    /// tsc unionType.propertyCache / propertyCacheWithoutObjectFunctionPropertyAugment
    /// (getUnionOrIntersectionProperty 59246) — a monotone cache, not a
    /// one-write slot; only successful synthesis is cached, like tsc.
    /// Private since m4-review B10: the write goes through
    /// set_union_property (speculation assert).
    union_property_cache: HashMap<(TypeId, String, bool), SymbolId>,
    /// tsc type-alias links.instantiations (getDeclaredTypeOfTypeAlias
    /// 57417 seed + getTypeAliasInstantiation 60271), keyed by
    /// getTypeListId + getAliasId — a monotone cache like tsc's map.
    alias_instantiations: HashMap<(SymbolId, String), TypeId>,
    /// tsc ConditionalRoot.instantiations, keyed by the shared root
    /// object plus getTypeListId/getAliasId. Writes happen only after
    /// a complete result; a re-entrant outer evaluation may replace
    /// the inner complete result, matching Map.set.
    conditional_instantiations: HashMap<(ConditionalRootId, String), TypeId>,
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

    /// tsrs-native: read the immutable-root conditional instantiation cache.
    pub fn conditional_instantiation(&self, root: ConditionalRootId, key: &str) -> Option<TypeId> {
        self.conditional_instantiations
            .get(&(root, key.to_owned()))
            .copied()
    }

    /// tsrs-native: read the generic-alias instantiation cache.
    pub fn alias_instantiation(&self, symbol: SymbolId, key: &str) -> Option<TypeId> {
        self.alias_instantiations
            .get(&(symbol, key.to_owned()))
            .copied()
    }

    /// tsrs-native: publish a generic-alias instantiation within the
    /// current cache transaction.
    pub fn set_alias_instantiation(
        &mut self,
        speculation_depth: u32,
        symbol: SymbolId,
        key: String,
        value: TypeId,
    ) {
        let cache_key = (symbol, key);
        if speculation_depth != 0
            && !self
                .speculative_alias_instantiation_writes
                .iter()
                .any(|(depth, existing, _)| *depth == speculation_depth && existing == &cache_key)
        {
            let previous = self.alias_instantiations.get(&cache_key).copied();
            self.speculative_alias_instantiation_writes.push((
                speculation_depth,
                cache_key.clone(),
                previous,
            ));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        self.alias_instantiations.insert(cache_key, value);
    }

    /// tsrs-native: publish a conditional-root instantiation within the
    /// current cache transaction.
    pub fn set_conditional_instantiation(
        &mut self,
        speculation_depth: u32,
        root: ConditionalRootId,
        key: String,
        value: TypeId,
    ) {
        let cache_key = (root, key);
        if speculation_depth != 0
            && !self
                .speculative_conditional_instantiation_writes
                .iter()
                .any(|(depth, existing, _)| *depth == speculation_depth && existing == &cache_key)
        {
            let previous = self.conditional_instantiations.get(&cache_key).copied();
            self.speculative_conditional_instantiation_writes.push((
                speculation_depth,
                cache_key.clone(),
                previous,
            ));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        self.conditional_instantiations.insert(cache_key, value);
    }

    /// tsrs-native: initialize the cache owned by a freshly allocated
    /// conditional root. The root and its seed entry form one semantic
    /// object, so construction is safe inside a candidate transaction.
    pub fn set_fresh_conditional_instantiation(
        &mut self,
        root: ConditionalRootId,
        key: String,
        value: TypeId,
    ) {
        self.conditional_instantiations.insert((root, key), value);
    }

    #[track_caller]
    fn assert_writable(speculation_depth: u32) {
        assert_eq!(
            speculation_depth, 0,
            "links writes are forbidden during speculation (greenfield §4.3)"
        );
    }

    fn write_slot<T: Clone + std::fmt::Debug>(slot: &mut LinkSlot<T>, next: LinkSlot<T>) {
        match (&*slot, &next) {
            (LinkSlot::Vacant, _) | (LinkSlot::Resolving, LinkSlot::Resolved(_)) => {
                note_resolving_transition(slot.is_resolving(), next.is_resolving());
                *slot = next;
            }
            _ => panic!("links slot rewritten: {slot:?} -> {next:?}"),
        }
    }

    fn journal_node_resolution(&mut self, speculation_depth: u32, id: NodeId) {
        if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
            return;
        }
        if self
            .speculative_resolved_type_writes
            .iter()
            .any(|(depth, node, _, _)| *depth == speculation_depth && *node == id)
        {
            return;
        }
        let (resolved_type, resolved_symbol) = self
            .node
            .get(&id)
            .map(|links| (links.resolved_type.clone(), links.resolved_symbol.clone()))
            .unwrap_or_default();
        self.speculative_resolved_type_writes.push((
            speculation_depth,
            id,
            resolved_type,
            resolved_symbol,
        ));
    }

    fn journal_type_instantiation(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        kind: SpeculativeTypeInstantiationKind,
    ) {
        if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
            return;
        }
        if self
            .speculative_type_instantiation_writes
            .iter()
            .any(|(depth, ty, existing_kind, _)| {
                *depth == speculation_depth && *ty == id && *existing_kind == kind
            })
        {
            return;
        }
        let links = self.ty.get(&id).cloned().unwrap_or_default();
        let previous = match kind {
            SpeculativeTypeInstantiationKind::UniqueLiteralFilled => {
                links.unique_literal_filled_instantiation
            }
            SpeculativeTypeInstantiationKind::Permissive => links.permissive_instantiation,
            SpeculativeTypeInstantiationKind::Restrictive => links.restrictive_instantiation,
            SpeculativeTypeInstantiationKind::BaseConstructor => {
                links.resolved_base_constructor_type
            }
        };
        self.speculative_type_instantiation_writes
            .push((speculation_depth, id, kind, previous));
    }

    fn journal_conditional_cache(&mut self, speculation_depth: u32, id: TypeId) {
        if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
            return;
        }
        if self
            .speculative_conditional_cache_writes
            .iter()
            .any(|(depth, ty, _)| *depth == speculation_depth && *ty == id)
        {
            return;
        }
        let snapshot = self
            .ty
            .get(&id)
            .map(|links| SpeculativeConditionalCacheSnapshot {
                true_type: links.conditional_true_type.clone(),
                false_type: links.conditional_false_type.clone(),
                inferred_true_type: links.conditional_inferred_true_type.clone(),
                default_constraint: links.conditional_default_constraint.clone(),
                constraint_of_distributive: links.conditional_constraint_of_distributive.clone(),
            })
            .unwrap_or_default();
        self.speculative_conditional_cache_writes
            .push((speculation_depth, id, snapshot));
    }

    fn journal_symbol_type(&mut self, speculation_depth: u32, id: SymbolId) {
        if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
            return;
        }
        if self
            .speculative_symbol_type_writes
            .iter()
            .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            return;
        }
        let previous = self
            .symbol
            .get(&id)
            .map(|links| links.type_of_symbol.clone())
            .unwrap_or_default();
        self.speculative_symbol_type_writes
            .push((speculation_depth, id, previous));
    }

    fn journal_symbol_write_type(&mut self, speculation_depth: u32, id: SymbolId) {
        if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
            return;
        }
        if self
            .speculative_symbol_write_type_writes
            .iter()
            .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            return;
        }
        let previous = self
            .symbol
            .get(&id)
            .map(|links| links.write_type.clone())
            .unwrap_or_default();
        self.speculative_symbol_write_type_writes
            .push((speculation_depth, id, previous));
    }

    fn journal_alias_target(&mut self, speculation_depth: u32, id: SymbolId) {
        if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
            return;
        }
        if self
            .speculative_alias_target_writes
            .iter()
            .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            return;
        }
        let previous = self
            .symbol
            .get(&id)
            .map(|links| links.alias_target.clone())
            .unwrap_or_default();
        self.speculative_alias_target_writes
            .push((speculation_depth, id, previous));
    }

    fn journal_type_only_alias(&mut self, speculation_depth: u32, id: SymbolId) {
        if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
            return;
        }
        if self
            .speculative_type_only_alias_writes
            .iter()
            .any(|(depth, symbol, _, _)| *depth == speculation_depth && *symbol == id)
        {
            return;
        }
        let (declaration, export_star_name) = self
            .symbol
            .get(&id)
            .map(|links| {
                (
                    links.type_only_declaration,
                    links.type_only_export_star_name.clone(),
                )
            })
            .unwrap_or_default();
        self.speculative_type_only_alias_writes.push((
            speculation_depth,
            id,
            declaration,
            export_star_name,
        ));
    }

    pub fn set_node_resolved_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: LinkSlot<TypeId>,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        Self::write_slot(&mut self.node.entry(id).or_default().resolved_type, value);
    }

    /// getTypeFromTypeReference's tail assignments (60587-60588) are
    /// UNGUARDED in tsc: the resolvingDefaultType recursion
    /// (getResolvedTypeParameterDefault 59043) can re-enter the SAME
    /// reference node mid-computation, so the inner call caches first
    /// and the outer assignment overwrites it — the node's final
    /// resolved type/symbol is the OUTER result. One of the two
    /// write-twice sites the memo discipline sanctions (the other:
    /// overwrite_symbol_type_for_binding_element); both slots move
    /// together.
    pub fn overwrite_type_reference_resolution(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        symbol: SymbolId,
        value: TypeId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let links = self.node.entry(id).or_default();
        note_resolving_transition(links.resolved_symbol.is_resolving(), false);
        note_resolving_transition(links.resolved_type.is_resolving(), false);
        links.resolved_symbol = LinkSlot::Resolved(symbol);
        links.resolved_type = LinkSlot::Resolved(value);
    }

    /// getTypeFromImportTypeNode's resolvedSymbol writes are UNGUARDED
    /// in tsc: the qualifier walk stamps each link's symbol on the
    /// link and its parent (62864-62865) — for a one-deep chain the
    /// parent IS the import-type node — and resolveImportSymbolType
    /// (62883) then overwrites the node with the resolveSymbol'd face;
    /// the final write wins. Self-referential aliases can also
    /// re-enter the node mid-computation (the
    /// overwrite_type_reference_resolution recursion class). The
    /// import-type sanctioned overwrite pair, symbol half.
    pub fn overwrite_import_type_resolved_symbol(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: SymbolId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let links = self.node.entry(id).or_default();
        note_resolving_transition(links.resolved_symbol.is_resolving(), false);
        links.resolved_symbol = LinkSlot::Resolved(value);
    }

    /// The import-type sanctioned overwrite pair, type half (see
    /// overwrite_import_type_resolved_symbol; tsc 62828/62834/62862/
    /// 62868-62877 all assign links.resolvedType unguarded).
    pub fn overwrite_import_type_resolved_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: TypeId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let links = self.node.entry(id).or_default();
        note_resolving_transition(links.resolved_type.is_resolving(), false);
        links.resolved_type = LinkSlot::Resolved(value);
    }

    /// tsc-port: assignBindingElementTypes @6.0.3 (the unguarded write)
    /// tsc-hash: af5b07d61441384b942c4e0e5a478d8fdcf25921dff2daae68e0ff34ba6d11a3
    /// tsc-span: _tsc.js:78451-78467
    ///
    /// The per-element write is UNGUARDED in tsc: computing
    /// getBindingElementTypeFromParentType can force getTypeOfSymbol
    /// on the SAME element's symbol (a circular reference through the
    /// pattern — e.g. late-bound member resolution reaching back into
    /// the declaration), which caches the circularity scar; the outer
    /// assignment then REPAIRS it with the real binding-element type.
    /// The outer result must win — the second sanctioned write-twice
    /// site (see overwrite_type_reference_resolution).
    pub fn overwrite_symbol_type_for_binding_element(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: TypeId,
    ) {
        self.journal_symbol_type(speculation_depth, id);
        let links = self.symbol.entry(id).or_default();
        note_resolving_transition(links.type_of_symbol.is_resolving(), false);
        links.type_of_symbol = LinkSlot::Resolved(value);
    }

    pub fn set_node_context_free_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: LinkSlot<TypeId>,
    ) {
        // A context-free expression type is a reproducible lazy memo.
        // Its callers retain and return the computed type, so rejected
        // candidates need not publish it to the shared node.
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.node.entry(id).or_default().context_free_type,
            value,
        );
    }

    /// tsrs-native: the links-slot setter behind
    /// `links.parameterInitializerContainsUndefined ??= ...` (71615) —
    /// a compute-once ?? write (the caller checks is_none first, like
    /// tsc's ??=).
    pub fn set_node_parameter_initializer_contains_undefined(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: bool,
    ) {
        Self::assert_writable(speculation_depth);
        self.node
            .entry(id)
            .or_default()
            .parameter_initializer_contains_undefined = Some(value);
    }

    /// `links.spreadIndices ??= getSpreadIndices(...)` (73520) — a
    /// compute-once ?? write, not a LinkSlot (both `None` halves are
    /// meaningful values).
    pub fn set_node_spread_indices(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: (Option<u32>, Option<u32>),
    ) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        let links = self.node.entry(id).or_default();
        if links.spread_indices.is_none() {
            links.spread_indices = Some(value);
        }
    }

    /// `links.jsxFlags |= …` (getIntrinsicTagSymbol 74540/74545) — an
    /// accumulating flags word; re-entry ORs the same bits.
    pub fn add_node_jsx_flags(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: tsrs2_types::JsxFlags,
    ) {
        Self::assert_writable(speculation_depth);
        self.node.entry(id).or_default().jsx_flags |= value;
    }

    /// `links.resolvedJsxElementAttributesType = …` (74731) —
    /// compute-once; a rewrite is a protocol bug.
    pub fn set_node_resolved_jsx_element_attributes_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        let slot = &mut self
            .node
            .entry(id)
            .or_default()
            .resolved_jsx_element_attributes_type;
        match slot {
            None => *slot = Some(value),
            Some(existing) if *existing == value => {}
            _ => panic!("resolvedJsxElementAttributesType rewritten: {slot:?} -> {value:?}"),
        }
    }

    /// `sourceFileLinks.jsxFragmentType = …` (getJSXFragmentType
    /// 77377-77395) — compute-once per source file.
    pub fn set_node_jsx_fragment_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        let slot = &mut self.node.entry(id).or_default().jsx_fragment_type;
        match slot {
            None => *slot = Some(value),
            Some(existing) if *existing == value => {}
            _ => panic!("jsxFragmentType rewritten: {slot:?} -> {value:?}"),
        }
    }

    pub fn set_node_resolved_signature(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: LinkSlot<SignatureId>,
    ) {
        if speculation_depth != 0
            && !self
                .speculative_declaration_signature_writes
                .iter()
                .any(|(depth, node, _)| *depth == speculation_depth && *node == id)
        {
            let previous = self
                .node
                .get(&id)
                .map(|links| links.resolved_signature.clone())
                .unwrap_or_default();
            self.speculative_declaration_signature_writes
                .push((speculation_depth, id, previous));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        Self::write_slot(
            &mut self.node.entry(id).or_default().resolved_signature,
            value,
        );
    }

    /// getResolvedSignature's cache protocol (77491-77508) on CALL-LIKE
    /// nodes — the same NodeLinks field getSignatureFromDeclaration
    /// uses (disjoint node kinds, mirroring tsc). Unlike the write-once
    /// declaration path, the call protocol REWRITES: the resolving
    /// sentinel transitions to the result, resolveCall's failure stash
    /// (76630) precedes getResolvedSignature's own tail write with the
    /// SAME value, and a re-entrant resolution's concrete write feeds
    /// the outer early return (76621-76625). Tolerated transitions:
    /// Vacant→Resolving, Resolving→Resolving (re-entrant sentinel
    /// write), Resolving→Resolved, and Resolved→Resolved — INCLUDING
    /// a different value: tsc's tail write is a plain assignment
    /// (77505 `links.resolvedSignature = result`), and a re-entrant
    /// resolution (declaration-site body driving demanding the same
    /// call mid-flight, live since 5.8b) can pick a different
    /// overload than the outer frame; the OUTER (last) write wins,
    /// exactly like tsc.
    pub fn set_node_resolved_signature_call_protocol(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: LinkSlot<SignatureId>,
    ) {
        if speculation_depth != 0
            && !self
                .speculative_resolved_signature_writes
                .iter()
                .any(|(depth, node, _)| *depth == speculation_depth && *node == id)
        {
            let previous = self
                .node
                .get(&id)
                .map(|links| links.resolved_signature.clone())
                .unwrap_or_default();
            self.speculative_resolved_signature_writes
                .push((speculation_depth, id, previous));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        let slot = &mut self.node.entry(id).or_default().resolved_signature;
        match (&*slot, &value) {
            (LinkSlot::Vacant, LinkSlot::Resolving)
            | (LinkSlot::Resolving, LinkSlot::Resolving)
            | (LinkSlot::Resolving, LinkSlot::Resolved(_))
            | (LinkSlot::Resolved(_), LinkSlot::Resolved(_)) => {
                note_resolving_transition(slot.is_resolving(), value.is_resolving());
                *slot = value;
            }
            _ => panic!("call resolvedSignature protocol violated: {slot:?} -> {value:?}"),
        }
    }

    /// getContextuallyTypedParameterType's IIFE stash (72708-72712):
    /// tsc parks anySignature on the IIFE while checking the argument
    /// (so re-entrant getResolvedSignature reads short-circuit), then
    /// restores the prior value. A RAW swap — the ONLY writer allowed
    /// to take the slot back to Vacant (restoring a previously-vacant
    /// slot); both directions bypass the call-protocol transitions.
    pub fn swap_node_resolved_signature_iife(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: LinkSlot<SignatureId>,
    ) -> LinkSlot<SignatureId> {
        // This is an explicitly scoped park/restore pair, not a cache
        // publication; its caller restores the returned slot even when
        // the checked argument returns Err.
        let _ = speculation_depth;
        let slot = &mut self.node.entry(id).or_default().resolved_signature;
        note_resolving_transition(slot.is_resolving(), value.is_resolving());
        std::mem::replace(slot, value)
    }

    /// Err-unwind twin for the call protocol: tsc cannot fail inside
    /// resolveSignature, so an Unsupported unwind that left the
    /// sentinel must revert to Vacant — a later query re-resolves and
    /// fails identically instead of observing a phantom mid-flight
    /// sentinel. Only the frame that WROTE the sentinel reverts
    /// (Resolved stashes stay — they are real memos).
    pub fn revert_node_resolved_signature_call(&mut self, id: NodeId) {
        let slot = &mut self.node.entry(id).or_default().resolved_signature;
        if matches!(slot, LinkSlot::Resolving) {
            note_resolving_transition(true, false);
            *slot = LinkSlot::Vacant;
        }
    }

    /// tsrs-native: the RE-ENTRANT-frame arm of tsc 77505's `: cached`
    /// exit write (M4-review F7). A getResolvedSignature frame that
    /// entered over an outer frame's Resolving sentinel (cached ===
    /// resolvingSignature) restores THAT sentinel on every
    /// non-memoizing exit — mid-fixpoint completion, or the port's Err
    /// unwind — clobbering any stash an inner resolution parked in
    /// between, exactly as tsc's unconditional assignment writes
    /// `cached` back. Without this, an inner failure stash survives
    /// over the outer frame's sentinel and the outer frame's
    /// Resolving-gated Err revert can no longer see it (the F7 leak).
    pub fn restore_node_resolved_signature_call_resolving(&mut self, id: NodeId) {
        let slot = &mut self.node.entry(id).or_default().resolved_signature;
        if !matches!(slot, LinkSlot::Resolving) {
            note_resolving_transition(slot.is_resolving(), true);
            *slot = LinkSlot::Resolving;
        }
    }

    /// tsrs-native: the mid-fixpoint twin of tsc 77505's `: cached`
    /// exit write (getResolvedSignature's guard-fail arm) — tsc
    /// expresses it as one unconditional slot assignment; the typed
    /// LinkSlot protocol needs an explicit clear. A signature resolved
    /// while a flow loop fixpoint is in progress must leave NO memo
    /// behind — INCLUDING resolveCall's overload-failure stash
    /// (76629), which tsc's exit write clobbers back to `cached` in
    /// exactly this case. Clears Resolving AND Resolved back to
    /// Vacant (M5 6.3; the FP class it kills: a failure-stash
    /// poisoning the later statement-path check into skipping
    /// argument checking).
    pub fn clear_node_resolved_signature_call(&mut self, id: NodeId) {
        let slot = &mut self.node.entry(id).or_default().resolved_signature;
        if !matches!(slot, LinkSlot::Vacant) {
            note_resolving_transition(slot.is_resolving(), false);
            *slot = LinkSlot::Vacant;
        }
    }

    /// tsrs-native: capture the call-cache journal position at a
    /// speculation boundary.
    pub fn speculative_resolved_signature_mark(&self) -> usize {
        self.speculative_resolved_signature_writes.len()
    }

    /// tsrs-native: capture the declaration-signature transaction mark.
    pub fn speculative_declaration_signature_mark(&self) -> usize {
        self.speculative_declaration_signature_writes.len()
    }

    /// tsrs-native: capture the type-node-cache journal position at a
    /// speculation boundary.
    pub fn speculative_resolved_type_mark(&self) -> usize {
        self.speculative_resolved_type_writes.len()
    }

    /// tsrs-native: capture the decorator-signature journal position.
    pub fn speculative_decorator_signature_mark(&self) -> usize {
        self.speculative_decorator_signature_writes.len()
    }

    /// tsrs-native: capture the contextual-check flag journal position.
    pub fn speculative_context_checked_mark(&self) -> usize {
        self.speculative_context_checked_writes.len()
    }

    /// tsrs-native: capture the symbol-declared-type journal position
    /// at a speculation boundary.
    pub fn speculative_symbol_declared_type_mark(&self) -> usize {
        self.speculative_symbol_declared_type_writes.len()
    }

    /// tsrs-native: capture the symbol-value-type journal position.
    pub fn speculative_symbol_type_mark(&self) -> usize {
        self.speculative_symbol_type_writes.len()
    }

    /// tsrs-native: capture the symbol-write-type journal position.
    pub fn speculative_symbol_write_type_mark(&self) -> usize {
        self.speculative_symbol_write_type_writes.len()
    }

    /// tsrs-native: capture the unique-symbol cache journal position at
    /// a speculation boundary.
    pub fn speculative_unique_es_symbol_type_mark(&self) -> usize {
        self.speculative_unique_es_symbol_type_writes.len()
    }

    /// tsrs-native: capture the late-symbol journal position at a
    /// speculation boundary.
    pub fn speculative_late_symbol_mark(&self) -> usize {
        self.speculative_late_symbol_writes.len()
    }

    /// tsrs-native: capture the variance-cache journal position at a
    /// speculation boundary.
    pub fn speculative_symbol_variance_mark(&self) -> usize {
        self.speculative_symbol_variance_writes.len()
    }

    /// tsrs-native: capture the alias type-parameter journal position.
    pub fn speculative_symbol_type_parameter_mark(&self) -> usize {
        self.speculative_symbol_type_parameter_writes.len()
    }

    /// tsrs-native: capture the alias-target protocol journal position.
    pub fn speculative_alias_target_mark(&self) -> usize {
        self.speculative_alias_target_writes.len()
    }

    /// tsrs-native: capture the type-only alias journal position.
    pub fn speculative_type_only_alias_mark(&self) -> usize {
        self.speculative_type_only_alias_writes.len()
    }

    /// tsrs-native: capture the alias-instantiation journal position.
    pub fn speculative_alias_instantiation_mark(&self) -> usize {
        self.speculative_alias_instantiation_writes.len()
    }

    /// tsrs-native: capture the conditional-root instantiation journal.
    pub fn speculative_conditional_instantiation_mark(&self) -> usize {
        self.speculative_conditional_instantiation_writes.len()
    }

    /// tsrs-native: capture the special-instantiation cache journal
    /// position.
    pub fn speculative_type_instantiation_mark(&self) -> usize {
        self.speculative_type_instantiation_writes.len()
    }

    /// tsrs-native: capture the conditional-cache journal position.
    pub fn speculative_conditional_cache_mark(&self) -> usize {
        self.speculative_conditional_cache_writes.len()
    }

    /// tsrs-native: capture the structured-member journal position at
    /// a speculation boundary.
    pub fn speculative_type_members_mark(&self) -> usize {
        self.speculative_type_member_writes.len()
    }

    /// tsrs-native: capture the indexed-access simplification journal.
    pub fn speculative_simplified_type_mark(&self) -> usize {
        self.speculative_simplified_type_writes.len()
    }

    /// tsrs-native: capture every LinksTables speculation journal mark.
    pub(crate) fn speculative_marks(&self) -> SpeculativeLinksMarks {
        SpeculativeLinksMarks {
            resolved_signatures: self.speculative_resolved_signature_mark(),
            declaration_signatures: self.speculative_declaration_signature_mark(),
            resolved_types: self.speculative_resolved_type_mark(),
            decorator_signatures: self.speculative_decorator_signature_mark(),
            context_checked: self.speculative_context_checked_mark(),
            symbol_declared_types: self.speculative_symbol_declared_type_mark(),
            symbol_types: self.speculative_symbol_type_mark(),
            symbol_write_types: self.speculative_symbol_write_type_mark(),
            unique_es_symbol_types: self.speculative_unique_es_symbol_type_mark(),
            late_symbols: self.speculative_late_symbol_mark(),
            symbol_variances: self.speculative_symbol_variance_mark(),
            symbol_type_parameters: self.speculative_symbol_type_parameter_mark(),
            alias_targets: self.speculative_alias_target_mark(),
            type_only_aliases: self.speculative_type_only_alias_mark(),
            alias_instantiations: self.speculative_alias_instantiation_mark(),
            conditional_instantiations: self.speculative_conditional_instantiation_mark(),
            type_instantiations: self.speculative_type_instantiation_mark(),
            conditional_caches: self.speculative_conditional_cache_mark(),
            type_members: self.speculative_type_members_mark(),
            simplified_types: self.speculative_simplified_type_mark(),
        }
    }

    /// tsrs-native: commit one selected overload-candidate transaction.
    ///
    /// Discard candidate-local protocols and lazy cache publications.
    /// Semantic objects constructed during the candidate initialize
    /// their owned fields through the `set_fresh_*` setters instead.
    /// Declaration signatures are the exception: a selected
    /// contextual function must keep one stable SignatureId for its
    /// later deferred body check.
    pub(crate) fn commit_speculative_writes(
        &mut self,
        marks: SpeculativeLinksMarks,
        parent_depth: u32,
    ) {
        self.restore_speculative_resolved_signatures(marks.resolved_signatures);
        self.commit_speculative_declaration_signatures(marks.declaration_signatures, parent_depth);
        self.restore_speculative_resolved_types(marks.resolved_types);
        self.restore_speculative_decorator_signatures(marks.decorator_signatures);
        self.restore_speculative_context_checked(marks.context_checked);
        self.restore_speculative_symbol_declared_types(marks.symbol_declared_types);
        self.restore_speculative_symbol_types(marks.symbol_types);
        self.restore_speculative_symbol_write_types(marks.symbol_write_types);
        self.restore_speculative_unique_es_symbol_types(marks.unique_es_symbol_types);
        self.restore_speculative_late_symbols(marks.late_symbols);
        self.restore_speculative_symbol_variances(marks.symbol_variances);
        self.restore_speculative_symbol_type_parameters(marks.symbol_type_parameters);
        self.restore_speculative_alias_targets(marks.alias_targets);
        self.restore_speculative_type_only_aliases(marks.type_only_aliases);
        self.restore_speculative_alias_instantiations(marks.alias_instantiations);
        self.restore_speculative_conditional_instantiations(marks.conditional_instantiations);
        self.restore_speculative_type_instantiations(marks.type_instantiations);
        self.restore_speculative_conditional_caches(marks.conditional_caches);
        self.restore_speculative_type_members(marks.type_members);
        self.restore_speculative_simplified_types(marks.simplified_types);
    }

    /// tsrs-native: restore every LinksTables journal to its marks.
    pub(crate) fn restore_speculative_writes(&mut self, marks: SpeculativeLinksMarks) {
        self.restore_speculative_resolved_signatures(marks.resolved_signatures);
        self.restore_speculative_declaration_signatures(marks.declaration_signatures);
        self.restore_speculative_resolved_types(marks.resolved_types);
        self.restore_speculative_decorator_signatures(marks.decorator_signatures);
        self.restore_speculative_context_checked(marks.context_checked);
        self.restore_speculative_symbol_declared_types(marks.symbol_declared_types);
        self.restore_speculative_symbol_types(marks.symbol_types);
        self.restore_speculative_symbol_write_types(marks.symbol_write_types);
        self.restore_speculative_unique_es_symbol_types(marks.unique_es_symbol_types);
        self.restore_speculative_late_symbols(marks.late_symbols);
        self.restore_speculative_symbol_variances(marks.symbol_variances);
        self.restore_speculative_symbol_type_parameters(marks.symbol_type_parameters);
        self.restore_speculative_alias_targets(marks.alias_targets);
        self.restore_speculative_type_only_aliases(marks.type_only_aliases);
        self.restore_speculative_alias_instantiations(marks.alias_instantiations);
        self.restore_speculative_conditional_instantiations(marks.conditional_instantiations);
        self.restore_speculative_type_instantiations(marks.type_instantiations);
        self.restore_speculative_conditional_caches(marks.conditional_caches);
        self.restore_speculative_type_members(marks.type_members);
        self.restore_speculative_simplified_types(marks.simplified_types);
    }

    /// tsrs-native: speculation-transaction unwind for call caches.
    ///
    /// Restore trial-local call-resolution slots to their transaction
    /// entry values. This runs on commit as well as rollback: a
    /// successful candidate may use the temporary sentinel/stash, but
    /// permanent node caches are populated only outside speculation.
    pub fn restore_speculative_resolved_signatures(&mut self, mark: usize) {
        while self.speculative_resolved_signature_writes.len() > mark {
            let (_, node, previous) = self
                .speculative_resolved_signature_writes
                .pop()
                .expect("length checked");
            let slot = &mut self.node.entry(node).or_default().resolved_signature;
            note_resolving_transition(slot.is_resolving(), previous.is_resolving());
            *slot = previous;
        }
    }

    fn commit_speculative_declaration_signatures(&mut self, mark: usize, parent_depth: u32) {
        let committed: Vec<_> = self
            .speculative_declaration_signature_writes
            .drain(mark..)
            .collect();
        if parent_depth == 0 {
            return;
        }
        for (_, node, previous) in committed {
            if !self
                .speculative_declaration_signature_writes
                .iter()
                .any(|(depth, existing, _)| *depth == parent_depth && *existing == node)
            {
                self.speculative_declaration_signature_writes
                    .push((parent_depth, node, previous));
            }
        }
    }

    fn restore_speculative_declaration_signatures(&mut self, mark: usize) {
        while self.speculative_declaration_signature_writes.len() > mark {
            let (_, node, previous) = self
                .speculative_declaration_signature_writes
                .pop()
                .expect("length checked");
            let slot = &mut self.node.entry(node).or_default().resolved_signature;
            note_resolving_transition(slot.is_resolving(), previous.is_resolving());
            *slot = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for type-node caches.
    pub fn restore_speculative_resolved_types(&mut self, mark: usize) {
        while self.speculative_resolved_type_writes.len() > mark {
            let (_, node, previous_type, previous_symbol) = self
                .speculative_resolved_type_writes
                .pop()
                .expect("length checked");
            let links = self.node.entry(node).or_default();
            note_resolving_transition(
                links.resolved_type.is_resolving(),
                previous_type.is_resolving(),
            );
            note_resolving_transition(
                links.resolved_symbol.is_resolving(),
                previous_symbol.is_resolving(),
            );
            links.resolved_type = previous_type;
            links.resolved_symbol = previous_symbol;
        }
    }

    /// tsrs-native: speculation-transaction unwind for decorator
    /// signature sentinels and results.
    pub fn restore_speculative_decorator_signatures(&mut self, mark: usize) {
        while self.speculative_decorator_signature_writes.len() > mark {
            let (_, node, previous) = self
                .speculative_decorator_signature_writes
                .pop()
                .expect("length checked");
            self.node.entry(node).or_default().decorator_signature = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for ContextChecked
    /// once-flags.
    pub fn restore_speculative_context_checked(&mut self, mark: usize) {
        while self.speculative_context_checked_writes.len() > mark {
            let (_, node, previous) = self
                .speculative_context_checked_writes
                .pop()
                .expect("length checked");
            self.node.entry(node).or_default().check_flags = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for symbol
    /// declared-type caches.
    pub fn restore_speculative_symbol_declared_types(&mut self, mark: usize) {
        while self.speculative_symbol_declared_type_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_symbol_declared_type_writes
                .pop()
                .expect("length checked");
            let slot = &mut self.symbol.entry(symbol).or_default().declared_type;
            note_resolving_transition(slot.is_resolving(), previous.is_resolving());
            *slot = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for symbol
    /// value-type caches.
    pub fn restore_speculative_symbol_types(&mut self, mark: usize) {
        while self.speculative_symbol_type_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_symbol_type_writes
                .pop()
                .expect("length checked");
            let slot = &mut self.symbol.entry(symbol).or_default().type_of_symbol;
            note_resolving_transition(slot.is_resolving(), previous.is_resolving());
            *slot = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for symbol
    /// write-type caches.
    pub fn restore_speculative_symbol_write_types(&mut self, mark: usize) {
        while self.speculative_symbol_write_type_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_symbol_write_type_writes
                .pop()
                .expect("length checked");
            let slot = &mut self.symbol.entry(symbol).or_default().write_type;
            note_resolving_transition(slot.is_resolving(), previous.is_resolving());
            *slot = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for unique-symbol
    /// type caches.
    pub fn restore_speculative_unique_es_symbol_types(&mut self, mark: usize) {
        while self.speculative_unique_es_symbol_type_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_unique_es_symbol_type_writes
                .pop()
                .expect("length checked");
            self.symbol.entry(symbol).or_default().unique_es_symbol_type = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for late-symbol
    /// links.
    pub fn restore_speculative_late_symbols(&mut self, mark: usize) {
        while self.speculative_late_symbol_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_late_symbol_writes
                .pop()
                .expect("length checked");
            self.symbol.entry(symbol).or_default().late_symbol = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for variance caches.
    pub fn restore_speculative_symbol_variances(&mut self, mark: usize) {
        while self.speculative_symbol_variance_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_symbol_variance_writes
                .pop()
                .expect("length checked");
            let slot = &mut self.symbol.entry(symbol).or_default().variances;
            note_resolving_transition(slot.is_resolving(), previous.is_resolving());
            *slot = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for generic-alias
    /// type-parameter lists.
    pub fn restore_speculative_symbol_type_parameters(&mut self, mark: usize) {
        while self.speculative_symbol_type_parameter_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_symbol_type_parameter_writes
                .pop()
                .expect("length checked");
            self.symbol.entry(symbol).or_default().type_parameters = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for alias targets.
    pub fn restore_speculative_alias_targets(&mut self, mark: usize) {
        while self.speculative_alias_target_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_alias_target_writes
                .pop()
                .expect("length checked");
            let slot = &mut self.symbol.entry(symbol).or_default().alias_target;
            note_resolving_transition(slot.is_resolving(), previous.is_resolving());
            *slot = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for type-only alias
    /// sentinel/final state.
    pub fn restore_speculative_type_only_aliases(&mut self, mark: usize) {
        while self.speculative_type_only_alias_writes.len() > mark {
            let (_, symbol, declaration, export_star_name) = self
                .speculative_type_only_alias_writes
                .pop()
                .expect("length checked");
            let links = self.symbol.entry(symbol).or_default();
            links.type_only_declaration = declaration;
            links.type_only_export_star_name = export_star_name;
        }
    }

    /// tsrs-native: speculation-transaction unwind for generic-alias
    /// instantiation cache entries.
    pub fn restore_speculative_alias_instantiations(&mut self, mark: usize) {
        while self.speculative_alias_instantiation_writes.len() > mark {
            let (_, key, previous) = self
                .speculative_alias_instantiation_writes
                .pop()
                .expect("length checked");
            if let Some(previous) = previous {
                self.alias_instantiations.insert(key, previous);
            } else {
                self.alias_instantiations.remove(&key);
            }
        }
    }

    /// tsrs-native: speculation-transaction unwind for conditional-root
    /// instantiation cache entries.
    pub fn restore_speculative_conditional_instantiations(&mut self, mark: usize) {
        while self.speculative_conditional_instantiation_writes.len() > mark {
            let (_, key, previous) = self
                .speculative_conditional_instantiation_writes
                .pop()
                .expect("length checked");
            if let Some(previous) = previous {
                self.conditional_instantiations.insert(key, previous);
            } else {
                self.conditional_instantiations.remove(&key);
            }
        }
    }

    /// tsrs-native: speculation-transaction unwind for special type
    /// instantiation caches.
    pub fn restore_speculative_type_instantiations(&mut self, mark: usize) {
        while self.speculative_type_instantiation_writes.len() > mark {
            let (_, ty, kind, previous) = self
                .speculative_type_instantiation_writes
                .pop()
                .expect("length checked");
            let links = self.ty.entry(ty).or_default();
            let slot = match kind {
                SpeculativeTypeInstantiationKind::UniqueLiteralFilled => {
                    &mut links.unique_literal_filled_instantiation
                }
                SpeculativeTypeInstantiationKind::Permissive => &mut links.permissive_instantiation,
                SpeculativeTypeInstantiationKind::Restrictive => {
                    &mut links.restrictive_instantiation
                }
                SpeculativeTypeInstantiationKind::BaseConstructor => {
                    &mut links.resolved_base_constructor_type
                }
            };
            note_resolving_transition(slot.is_resolving(), previous.is_resolving());
            *slot = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for resolved
    /// conditional branches and constraints.
    pub fn restore_speculative_conditional_caches(&mut self, mark: usize) {
        while self.speculative_conditional_cache_writes.len() > mark {
            let (_, ty, previous) = self
                .speculative_conditional_cache_writes
                .pop()
                .expect("length checked");
            let links = self.ty.entry(ty).or_default();
            note_resolving_transition(
                links.conditional_true_type.is_resolving(),
                previous.true_type.is_resolving(),
            );
            note_resolving_transition(
                links.conditional_false_type.is_resolving(),
                previous.false_type.is_resolving(),
            );
            note_resolving_transition(
                links.conditional_inferred_true_type.is_resolving(),
                previous.inferred_true_type.is_resolving(),
            );
            note_resolving_transition(
                links.conditional_default_constraint.is_resolving(),
                previous.default_constraint.is_resolving(),
            );
            note_resolving_transition(
                links.conditional_constraint_of_distributive.is_resolving(),
                previous.constraint_of_distributive.is_resolving(),
            );
            links.conditional_true_type = previous.true_type;
            links.conditional_false_type = previous.false_type;
            links.conditional_inferred_true_type = previous.inferred_true_type;
            links.conditional_default_constraint = previous.default_constraint;
            links.conditional_constraint_of_distributive = previous.constraint_of_distributive;
        }
    }

    /// tsrs-native: speculation-transaction unwind for lazy member
    /// caches.
    pub fn restore_speculative_type_members(&mut self, mark: usize) {
        while self.speculative_type_member_writes.len() > mark {
            let (_, ty, previous) = self
                .speculative_type_member_writes
                .pop()
                .expect("length checked");
            self.ty.entry(ty).or_default().resolved_members = previous;
        }
    }

    /// tsrs-native: speculation-transaction unwind for indexed-access
    /// simplification protocols.
    pub fn restore_speculative_simplified_types(&mut self, mark: usize) {
        while self.speculative_simplified_type_writes.len() > mark {
            let (_, ty, writing, previous) = self
                .speculative_simplified_type_writes
                .pop()
                .expect("length checked");
            let links = self.ty.entry(ty).or_default();
            let slot = if writing {
                &mut links.simplified_for_writing
            } else {
                &mut links.simplified_for_reading
            };
            note_resolving_transition(slot.is_resolving(), previous.is_resolving());
            *slot = previous;
        }
    }

    pub fn set_symbol_variances(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<Box<[tsrs2_types::VarianceFlags]>>,
    ) {
        if speculation_depth != 0
            && !self
                .speculative_symbol_variance_writes
                .iter()
                .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            let previous = self
                .symbol
                .get(&id)
                .map(|links| links.variances.clone())
                .unwrap_or_default();
            self.speculative_symbol_variance_writes
                .push((speculation_depth, id, previous));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
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
        note_resolving_transition(true, false);
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
        if speculation_depth != 0
            && bits.intersects(tsrs2_types::NodeCheckFlags::CONTEXT_CHECKED)
            && !self
                .speculative_context_checked_writes
                .iter()
                .any(|(depth, node, _)| *depth == speculation_depth && *node == id)
        {
            let previous = self
                .node
                .get(&id)
                .map(|links| links.check_flags)
                .unwrap_or_default();
            self.speculative_context_checked_writes
                .push((speculation_depth, id, previous));
        }
        if speculation_depth != 0
            && !bits.intersects(
                tsrs2_types::NodeCheckFlags::IN_CHECK_IDENTIFIER
                    | tsrs2_types::NodeCheckFlags::ASSIGNMENTS_MARKED
                    | tsrs2_types::NodeCheckFlags::CONTEXT_CHECKED,
            )
        {
            return;
        }
        if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        let links = self.node.entry(id).or_default();
        links.check_flags =
            tsrs2_types::NodeCheckFlags::from_bits(links.check_flags.bits() | bits.bits());
    }

    /// tsrs-native: links-table setter (tsc plain flags mutation).
    /// `nodeLinks.flags &= ~bits` — the sanctioned clears: tsc's
    /// InCheckIdentifier re-entrance latch (getNarrowedTypeOfSymbol
    /// 72012/72015 sets then clears within one computation) and the
    /// AssignmentsMarked unwind revert (tsc cannot fail mid-marking;
    /// our marking can unwind, and a half-marked container must not
    /// stay latched).
    pub fn clear_node_check_flags(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        bits: tsrs2_types::NodeCheckFlags,
    ) {
        // The only callers clear scoped latches that may also be set
        // during a candidate. This is the balancing half of that
        // protocol, not a cache publication.
        let _ = speculation_depth;
        let links = self.node.entry(id).or_default();
        links.check_flags =
            tsrs2_types::NodeCheckFlags::from_bits(links.check_flags.bits() & !bits.bits());
    }

    /// tsrs-native: links-table setter (tsc plain property write).
    /// tsc `symbol.lastAssignmentPos = …` (markNodeAssignments) —
    /// PLAIN ASSIGNMENT by design: the marking pass overwrites in
    /// document order (last write wins) and flips the sign for
    /// definite assignments within the same pass.
    pub fn set_symbol_last_assignment_pos(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: Option<i64>,
    ) {
        let _ = speculation_depth;
        self.symbol.entry(id).or_default().last_assignment_pos = value;
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

    /// tsrs-native: links-table setter (tsc plain property write).
    /// getDecoratorCallSignature's memo — PLAIN ASSIGNMENT (tsc writes
    /// the anySignature sentinel first, then possibly overwrites within
    /// the same computation).
    pub fn set_node_decorator_signature(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: Option<crate::state::SignatureId>,
    ) {
        if speculation_depth != 0
            && !self
                .speculative_decorator_signature_writes
                .iter()
                .any(|(depth, node, _)| *depth == speculation_depth && *node == id)
        {
            let previous = self
                .node
                .get(&id)
                .and_then(|links| links.decorator_signature);
            self.speculative_decorator_signature_writes
                .push((speculation_depth, id, previous));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        self.node.entry(id).or_default().decorator_signature = value;
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
    /// Like every other revert twin this deliberately does NOT assert
    /// speculation_depth (the 7.0t convention, m4-review B35): a revert
    /// RESTORES pre-write state, which is always legal, and an unwind
    /// crossing a speculation boundary reaches twins INSIDE the region
    /// while depth > 0 (speculate.rs rolls back before the Err
    /// re-propagates, so OUTER twins fire at the entry depth).
    pub fn revert_node_enum_values_computed(&mut self, id: NodeId) {
        self.node.entry(id).or_default().enum_values_computed = false;
    }

    /// tsrs-native: links-table setter (tsc plain property write).
    /// checkTypeParameterListsIdentical's once-latch (84877). Like
    /// tsc, set BEFORE the identity walk runs — re-entry through the
    /// declared-type forcing sees the latch and skips.
    pub fn set_symbol_type_parameters_checked(&mut self, speculation_depth: u32, id: SymbolId) {
        Self::assert_writable(speculation_depth);
        self.symbol.entry(id).or_default().type_parameters_checked = true;
    }

    pub fn set_symbol_declared_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        if speculation_depth != 0
            && !self
                .speculative_symbol_declared_type_writes
                .iter()
                .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            let previous = self
                .symbol
                .get(&id)
                .map(|links| links.declared_type.clone())
                .unwrap_or_default();
            self.speculative_symbol_declared_type_writes
                .push((speculation_depth, id, previous));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        Self::write_slot(&mut self.symbol.entry(id).or_default().declared_type, value);
    }

    /// tsrs-native: declared type-parameter singleton initialization.
    ///
    /// `createTypeParameter` allocates the semantic type and stamps
    /// the declaring symbol as one indivisible operation. The type can
    /// immediately become part of another persistent semantic type
    /// (for example `Array<T>`), so its identity must not be replaced
    /// when a candidate transaction closes. This path is restricted to
    /// the diagnostic-free declared-type-parameter constructor.
    pub fn set_fresh_symbol_declared_type(&mut self, id: SymbolId, value: LinkSlot<TypeId>) {
        Self::write_slot(&mut self.symbol.entry(id).or_default().declared_type, value);
    }

    pub fn set_symbol_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        self.journal_symbol_type(speculation_depth, id);
        Self::write_slot(
            &mut self.symbol.entry(id).or_default().type_of_symbol,
            value,
        );
    }

    /// tsrs-native: candidate-local contextual symbol initialization.
    ///
    /// The parameter type must remain stable while a candidate is
    /// checked. Rejection restores it with the ContextChecked flag;
    /// selection commits both, matching tsc's contextual pin. The
    /// plain assignment also preserves tsc's `unknown` binding-pattern
    /// replacement.
    pub fn set_symbol_type_contextual(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        self.journal_symbol_type(speculation_depth, id);
        let slot = &mut self.symbol.entry(id).or_default().type_of_symbol;
        note_resolving_transition(slot.is_resolving(), value.is_resolving());
        *slot = value;
    }

    /// tsrs-native: fresh synthetic-symbol initialization.
    ///
    /// Initialize the type carried by a freshly allocated synthetic
    /// symbol. The symbol and this slot form one semantic object, so
    /// construction is safe inside a candidate trial.
    pub fn set_fresh_symbol_type(&mut self, id: SymbolId, value: LinkSlot<TypeId>) {
        Self::write_slot(
            &mut self.symbol.entry(id).or_default().type_of_symbol,
            value,
        );
    }

    /// tsrs-native: links-table setter (tsc plain property write).
    /// getTypeOfFuncClassEnumModule's memo write (56824) is a PLAIN
    /// ASSIGNMENT: a self-referential heritage clause (`class C
    /// extends C`) re-enters through getBaseTypeVariableOfClass and
    /// fills the slot mid-flight — tsc's outer write overwrites the
    /// re-entrant fill (the resolvedSignature 77505 precedent). Only
    /// that caller may rewrite Resolved→Resolved.
    pub fn set_symbol_type_func_class_enum_module(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: TypeId,
    ) {
        self.journal_symbol_type(speculation_depth, id);
        self.symbol.entry(id).or_default().type_of_symbol = LinkSlot::Resolved(value);
    }

    pub fn set_symbol_synthetic(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        check_flags: tsrs2_types::CheckFlags,
        containing_type: TypeId,
        type_of_symbol: TypeId,
    ) {
        let _ = speculation_depth;
        let links = self.symbol.entry(id).or_default();
        links.check_flags = check_flags;
        links.containing_type = Some(containing_type);
        Self::write_slot(
            &mut links.type_of_symbol,
            LinkSlot::Resolved(type_of_symbol),
        );
    }

    pub fn set_symbol_is_discriminant(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: bool,
    ) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        self.symbol.entry(id).or_default().is_discriminant_property = Some(value);
    }

    /// tsrs-native: getUnionOrIntersectionProperty's propertyCache
    /// read (59248).
    pub fn union_property(&self, key: &(TypeId, String, bool)) -> Option<SymbolId> {
        self.union_property_cache.get(key).copied()
    }

    /// tsrs-native: getUnionOrIntersectionProperty's propertyCache
    /// write (59252) — under the speculation assert since m4-review
    /// B10 (the payload symbol's links writes already were).
    pub fn set_union_property(
        &mut self,
        speculation_depth: u32,
        key: (TypeId, String, bool),
        value: SymbolId,
    ) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        self.union_property_cache.insert(key, value);
    }

    /// `links.uniqueESSymbolType = ...` (getESSymbolLikeTypeForNode
    /// 63127).
    pub fn set_symbol_unique_es_symbol_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        ty: TypeId,
    ) {
        if speculation_depth != 0
            && !self
                .speculative_unique_es_symbol_type_writes
                .iter()
                .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            let previous = self
                .symbol
                .get(&id)
                .and_then(|links| links.unique_es_symbol_type);
            self.speculative_unique_es_symbol_type_writes
                .push((speculation_depth, id, previous));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        self.symbol.entry(id).or_default().unique_es_symbol_type = Some(ty);
    }

    /// tsc createSymbol's checkFlags seed (47656) for transient symbols
    /// created outside the synthetic-property path.
    pub fn set_symbol_check_flags(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        check_flags: tsrs2_types::CheckFlags,
    ) {
        let _ = speculation_depth;
        self.symbol.entry(id).or_default().check_flags = check_flags;
    }

    /// `links.nameType = ...` on a fresh transient symbol (getSpreadSymbol
    /// 63054, checkObjectLiteral's late-bound member 74193).
    pub fn set_symbol_name_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        name_type: Option<TypeId>,
    ) {
        let _ = speculation_depth;
        self.symbol.entry(id).or_default().name_type = name_type;
    }

    /// tsrs-native: grouped LinksTables setter for tsc
    /// resolveMappedTypeMembers' fresh property-link writes
    /// (58549-58551).
    pub fn set_symbol_mapped_links(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        mapped_type: TypeId,
        name_type: TypeId,
        key_type: TypeId,
    ) {
        let _ = speculation_depth;
        let links = self.symbol.entry(id).or_default();
        assert!(
            links.mapped_type.is_none() && links.key_type.is_none(),
            "mapped symbol links rewritten"
        );
        links.mapped_type = Some(mapped_type);
        links.name_type = Some(name_type);
        links.key_type = Some(key_type);
    }

    /// tsrs-native: grouped fresh-link writes from
    /// resolveReverseMappedTypeMembers (58441-58450).
    pub fn set_symbol_reverse_mapped_links(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        name_type: Option<TypeId>,
        property_type: TypeId,
        mapped_type: TypeId,
        constraint_type: TypeId,
    ) {
        let _ = speculation_depth;
        let links = self.symbol.entry(id).or_default();
        assert!(
            links.mapped_type.is_none()
                && links.property_type.is_none()
                && links.constraint_type.is_none(),
            "reverse-mapped symbol links rewritten"
        );
        links.name_type = name_type;
        links.property_type = Some(property_type);
        links.mapped_type = Some(mapped_type);
        links.constraint_type = Some(constraint_type);
    }

    /// tsrs-native: grouped LinksTables setter for tsc
    /// resolveMappedTypeMembers' duplicate-remap union update
    /// (58537-58538).
    pub fn update_symbol_mapped_name_and_key(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        name_type: TypeId,
        key_type: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        let links = self.symbol.entry(id).or_default();
        assert!(
            links.mapped_type.is_some(),
            "only mapped symbols merge name/key links"
        );
        links.name_type = Some(name_type);
        links.key_type = Some(key_type);
    }

    /// `links.target = ...` (checkObjectLiteral 74209 — the object
    /// literal member's source symbol, not the instantiation target).
    pub fn set_symbol_target(&mut self, speculation_depth: u32, id: SymbolId, target: SymbolId) {
        let _ = speculation_depth;
        self.symbol.entry(id).or_default().target = Some(target);
    }

    /// `links.originatingImport = referenceParent` on a fresh interop
    /// clone (cloneTypeAsModuleType 49769).
    pub fn set_symbol_originating_import(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        reference_parent: NodeId,
    ) {
        Self::assert_writable(speculation_depth);
        self.symbol.entry(id).or_default().originating_import = Some(reference_parent);
    }

    /// `links.leftSpread/rightSpread` (getSpreadType 63024-63025).
    pub fn set_symbol_spread_pair(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        left: SymbolId,
        right: SymbolId,
    ) {
        Self::assert_writable(speculation_depth);
        let links = self.symbol.entry(id).or_default();
        links.left_spread = Some(left);
        links.right_spread = Some(right);
    }

    /// `links.syntheticOrigin` (getSpreadSymbol 63052 /
    /// getAnonymousPartialType 62955). Every caller initializes a freshly
    /// synthesized symbol, so this semantic stamp belongs to the transaction.
    pub fn set_symbol_synthetic_origin(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        origin: SymbolId,
    ) {
        let _ = speculation_depth;
        self.symbol.entry(id).or_default().synthetic_origin = Some(origin);
    }

    /// `type.literalType = cloneTypeReference(type)` (createArrayLiteralType
    /// 74039) — once-per-reference like the tsc field write.
    pub fn set_type_literal_type(&mut self, speculation_depth: u32, id: TypeId, literal: TypeId) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        assert!(links.literal_type.is_none(), "literalType rewritten");
        links.literal_type = Some(literal);
    }

    pub fn set_type_promised_type_of_promise(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        promised: TypeId,
    ) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        assert!(
            links.promised_type_of_promise.is_none(),
            "promisedTypeOfPromise rewritten"
        );
        links.promised_type_of_promise = Some(promised);
    }

    pub fn set_type_awaited_type_of_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        awaited: TypeId,
    ) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        assert!(
            links.awaited_type_of_type.is_none(),
            "awaitedTypeOfType rewritten"
        );
        links.awaited_type_of_type = Some(awaited);
    }

    /// checkAssertionWorker's links.assertionExpressionType stamp —
    /// re-checks overwrite (tsc reassigns freely).
    pub fn set_node_assertion_expression_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        ty: TypeId,
    ) {
        let _ = speculation_depth;
        self.node.entry(id).or_default().assertion_expression_type = Some(ty);
    }

    /// getInstantiationExpressionType's STORE-BEFORE-ERROR map insert.
    pub fn set_node_instantiation_expression_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        expr_type: TypeId,
        result: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
        self.node
            .entry(id)
            .or_default()
            .instantiation_expression_types
            .get_or_insert_with(Default::default)
            .insert(expr_type, result);
    }

    /// tsc `symbol.isReferenced = SymbolFlags.All` — freely repeatable.
    pub fn set_symbol_is_referenced(&mut self, speculation_depth: u32, id: SymbolId) {
        let _ = speculation_depth;
        self.symbol.entry(id).or_default().is_referenced = true;
    }

    /// nonExistentPropCheckCache add (75419-75423): returns true when
    /// the key was NEW (the caller reports), false on a repeat.
    pub fn insert_node_non_existent_prop_key(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        key: String,
    ) -> bool {
        // Candidate diagnostics are transactional; their de-duplication
        // key must not survive the candidate either. Treat each
        // speculative report as new and leave the shared set untouched.
        if speculation_depth != 0 {
            return true;
        }
        Self::assert_writable(speculation_depth);
        self.node
            .entry(id)
            .or_default()
            .non_existent_prop_check_cache
            .insert(key)
    }

    pub fn set_type_resolved_properties(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: Box<[SymbolId]>,
    ) {
        if speculation_depth != 0 {
            return;
        }
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
        if speculation_depth != 0 {
            return;
        }
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
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().union_key_property,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: initialize `result.pattern` (56522/56541) once on
    /// a fresh (or freshly-cloned) type. The type and its pattern form
    /// one semantic object, so construction is safe inside speculation.
    pub fn set_fresh_type_pattern(&mut self, id: TypeId, pattern: NodeId) {
        let links = self.ty.entry(id).or_default();
        assert!(links.pattern.is_none(), "type pattern rewritten");
        links.pattern = Some(pattern);
    }

    /// `type.widened = result` (getWidenedTypeWithContext 68049) —
    /// written by the first context-free widening; EQUAL-value
    /// rewrites are tolerated (tsc overwrites idempotently; the
    /// resolvedSymbol precedent from 5.5e).
    pub fn set_type_widened(&mut self, speculation_depth: u32, id: TypeId, widened: TypeId) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        assert!(
            links.widened.is_none() || links.widened == Some(widened),
            "type widened memo rewritten with a DIFFERENT value"
        );
        links.widened = Some(widened);
    }

    /// tsrs-native: the links half of setCachedIterationTypes
    /// (84059-84061; the tsc-port header lives on iterate.rs's
    /// set_cached_iteration_types). A PLAIN ASSIGNMENT like tsc's
    /// `type[cacheKey] = cachedTypes` — no write-once discipline: the
    /// for-await async-from-sync fallback legitimately OVERWRITES a
    /// cached AsyncIterable=No verdict (the async slow path caches No,
    /// then the sync branch re-caches the awaited sync-derived triple
    /// under the SAME async key, worker 84139-84174).
    pub fn set_type_iteration_types(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        key: crate::iterate::IterationCacheKey,
        value: crate::iterate::IterationTypesResult,
    ) {
        // This is a pure, reproducible memo. Candidate-local callers
        // already carry `value`, so publishing it is unnecessary and
        // would let a rejected overload warm shared type state.
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        let slot = match key {
            crate::iterate::IterationCacheKey::Iterable => &mut links.iteration_types_of_iterable,
            crate::iterate::IterationCacheKey::AsyncIterable => {
                &mut links.iteration_types_of_async_iterable
            }
            crate::iterate::IterationCacheKey::Iterator => &mut links.iteration_types_of_iterator,
            crate::iterate::IterationCacheKey::AsyncIterator => {
                &mut links.iteration_types_of_async_iterator
            }
            crate::iterate::IterationCacheKey::IteratorResult => {
                &mut links.iteration_types_of_iterator_result
            }
        };
        *slot = Some(value);
    }

    pub fn set_type_parameter_constraint(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        // A declared or targeted type parameter's constraint is a cold,
        // reproducible cache. Candidate checking must not publish it beyond
        // the speculation boundary.
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().type_parameter_constraint,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: fresh type-parameter initialization.
    ///
    /// Initializes the constraint of a type parameter created inside the
    /// current transaction. Unlike the lazy cache setter above, this is part
    /// of the fresh type's semantic state and may be written speculatively.
    pub fn set_fresh_type_parameter_constraint(&mut self, id: TypeId, value: TypeId) {
        Self::write_slot(
            &mut self.ty.entry(id).or_default().type_parameter_constraint,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.typeParameter.
    pub fn set_mapped_type_parameter(&mut self, speculation_depth: u32, id: TypeId, value: TypeId) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().mapped_type_parameter,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: initialize the type parameter owned by a freshly
    /// allocated mapped-type instantiation.
    pub fn set_fresh_mapped_type_parameter(&mut self, id: TypeId, value: TypeId) {
        Self::write_slot(
            &mut self.ty.entry(id).or_default().mapped_type_parameter,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.constraintType.
    pub fn set_mapped_constraint_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().mapped_constraint_type,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.nameType; None is a resolved absence.
    pub fn set_mapped_name_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: Option<TypeId>,
    ) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().mapped_name_type,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.templateType.
    pub fn set_mapped_template_type(&mut self, speculation_depth: u32, id: TypeId, value: TypeId) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().mapped_template_type,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.modifiersType.
    pub fn set_mapped_modifiers_type(&mut self, speculation_depth: u32, id: TypeId, value: TypeId) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().mapped_modifiers_type,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: LinksTables setter for tsc
    /// `mappedType.containsError = true` on a mapped-property type
    /// resolution cycle (58581).
    pub fn set_mapped_contains_error(&mut self, speculation_depth: u32, id: TypeId) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        self.ty.entry(id).or_default().mapped_contains_error = true;
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.resolvedApparentType.
    pub fn set_mapped_apparent_type(&mut self, speculation_depth: u32, id: TypeId, value: TypeId) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().mapped_apparent_type,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write ConditionalType.resolvedTrueType setter.
    pub fn set_conditional_true_type(&mut self, speculation_depth: u32, id: TypeId, value: TypeId) {
        self.journal_conditional_cache(speculation_depth, id);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().conditional_true_type,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write ConditionalType.resolvedFalseType setter.
    pub fn set_conditional_false_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        self.journal_conditional_cache(speculation_depth, id);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().conditional_false_type,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write ConditionalType.resolvedInferredTrueType setter.
    pub fn set_conditional_inferred_true_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        self.journal_conditional_cache(speculation_depth, id);
        Self::write_slot(
            &mut self
                .ty
                .entry(id)
                .or_default()
                .conditional_inferred_true_type,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write ConditionalType.resolvedDefaultConstraint setter.
    pub fn set_conditional_default_constraint(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        self.journal_conditional_cache(speculation_depth, id);
        Self::write_slot(
            &mut self
                .ty
                .entry(id)
                .or_default()
                .conditional_default_constraint,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write resolvedConstraintOfDistributive setter.
    pub fn set_conditional_constraint_of_distributive(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: Option<TypeId>,
    ) {
        self.journal_conditional_cache(speculation_depth, id);
        Self::write_slot(
            &mut self
                .ty
                .entry(id)
                .or_default()
                .conditional_constraint_of_distributive,
            LinkSlot::Resolved(value),
        );
    }

    pub fn set_type_resolved_base_constraint(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        if speculation_depth != 0 {
            return;
        }
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
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().immediate_base_constraint,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: links-table setter (tsc plain property write).
    /// getSimplifiedIndexedAccessType's per-direction cache
    /// (62471-62475): Resolving parks the circular sentinel, Resolved
    /// stores the simplification.
    pub fn set_type_simplified(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        writing: bool,
        value: LinkSlot<TypeId>,
    ) {
        if speculation_depth != 0
            && !self
                .speculative_simplified_type_writes
                .iter()
                .any(|(depth, ty, direction, _)| {
                    *depth == speculation_depth && *ty == id && *direction == writing
                })
        {
            let previous = self
                .ty
                .get(&id)
                .map(|links| {
                    if writing {
                        links.simplified_for_writing.clone()
                    } else {
                        links.simplified_for_reading.clone()
                    }
                })
                .unwrap_or_default();
            self.speculative_simplified_type_writes.push((
                speculation_depth,
                id,
                writing,
                previous,
            ));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        let links = self.ty.entry(id).or_default();
        let slot = if writing {
            &mut links.simplified_for_writing
        } else {
            &mut links.simplified_for_reading
        };
        Self::write_slot(slot, value);
    }

    /// tsrs-native: Err-unwind twin for the simplified cache — tsc
    /// cannot fail inside getSimplifiedIndexedAccessType, so an
    /// Unsupported unwind that left the sentinel must revert to
    /// Vacant; a later query re-simplifies instead of observing a
    /// phantom mid-flight sentinel.
    pub fn revert_type_simplified(&mut self, id: TypeId, writing: bool) {
        let links = self.ty.entry(id).or_default();
        let slot = if writing {
            &mut links.simplified_for_writing
        } else {
            &mut links.simplified_for_reading
        };
        assert!(
            matches!(slot, LinkSlot::Resolving),
            "simplified revert without an in-progress simplification for {id:?}"
        );
        note_resolving_transition(true, false);
        *slot = LinkSlot::Vacant;
    }

    /// lateBindMember's two-phase resolvedSymbol write (57665/57689):
    /// the member's own binder symbol parks first (the re-entrancy
    /// guard — checkComputedPropertyName may demand the container
    /// mid-bind), then the LATE symbol replaces it. This protocol
    /// setter permits that one rewrite; member-declaration nodes are
    /// disjoint from the identifier/access kinds the strict setter
    /// serves.
    pub fn set_node_resolved_symbol_late_bind(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: SymbolId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let slot = &mut self.node.entry(id).or_default().resolved_symbol;
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Resolved(value);
    }

    /// Err-unwind twin for the late-bind protocol: a container
    /// resolution cut short by Unsupported must leave every member it
    /// touched re-bindable — a parked memo would short-circuit the
    /// retry's lateBindMember and DROP the member from the rebuilt
    /// late table (5.7b review round #2).
    pub fn revert_node_resolved_symbol_late_bind(&mut self, id: NodeId) {
        let slot = &mut self.node.entry(id).or_default().resolved_symbol;
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Vacant;
    }

    /// Err-unwind twin for `links.lateSymbol`.
    pub fn clear_symbol_late_symbol(&mut self, id: SymbolId) {
        self.symbol.entry(id).or_default().late_symbol = None;
    }

    /// The instantiation root: follow links.target (instantiated /
    /// mapped symbols) to the underlying declaration symbol.
    fn instantiation_root(&self, id: SymbolId) -> SymbolId {
        let mut current = id;
        while let Some(target) = self.symbol.get(&current).and_then(|links| links.target) {
            if target == current {
                break;
            }
            current = target;
        }
        current
    }

    /// tsc reassigns links.resolvedSymbol unconditionally on every
    /// checkPropertyAccessExpression run — re-checks (the compound
    /// assignment writeOnly pass 80311, the condition-walker forcing
    /// 87443) legitimately rewrite the SAME value. Two different-value
    /// rewrites are sanctioned since the 5.8e interface lift, both
    /// with tsc's last-write-wins result:
    /// - EARLY→LATE: a property access checked DURING late-table
    ///   construction resolves through the pre-published early table;
    ///   the re-check resolves the member's late twin (verified via
    ///   links.lateSymbol).
    /// - Re-instantiation: each check run of an expression like
    ///   `[…].concat` re-instantiates the lib member against that
    ///   run's fresh literal type, so the two writes carry distinct
    ///   instantiated symbols over the SAME declaration symbol
    ///   (verified via the links.target chain).
    ///
    /// Any other different-value rewrite still breaks memo stability
    /// and panics.
    pub fn set_node_resolved_symbol(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: SymbolId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let sanctioned_rewrite = self
            .node
            .get(&id)
            .and_then(|links| links.resolved_symbol.resolved())
            .is_some_and(|existing| {
                existing != value
                    && (self
                        .symbol
                        .get(&existing)
                        .is_some_and(|links| links.late_symbol == Some(value))
                        || self.instantiation_root(existing) == self.instantiation_root(value))
            });
        let slot = &mut self.node.entry(id).or_default().resolved_symbol;
        match &*slot {
            LinkSlot::Resolved(existing) if *existing == value => {}
            // A candidate may re-check the same access after bypassing
            // a cold union-property cache, producing a new transient
            // property symbol. The whole slot is journaled and restored
            // at the boundary, so last-write-wins is trial-local.
            LinkSlot::Resolved(_) if sanctioned_rewrite || speculation_depth != 0 => {
                *slot = LinkSlot::Resolved(value);
            }
            LinkSlot::Resolved(existing) => {
                panic!("resolvedSymbol rewritten with a DIFFERENT value: {existing:?} -> {value:?}")
            }
            _ => {
                note_resolving_transition(slot.is_resolving(), false);
                *slot = LinkSlot::Resolved(value);
            }
        }
    }

    pub fn set_node_outer_type_parameters(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: Box<[TypeId]>,
    ) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.node.entry(id).or_default().outer_type_parameters,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: createUnionOrIntersectionProperty's identical-
    /// instantiation clone links (59179-59182) as one asserted setter —
    /// containingType + the source's mapper (unconditional in tsc;
    /// None clears nothing because the clone is fresh).
    pub fn set_symbol_union_clone_links(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        containing_type: TypeId,
        mapper: Option<MapperId>,
    ) {
        let _ = speculation_depth;
        let links = self.symbol.entry(id).or_default();
        links.containing_type = Some(containing_type);
        links.mapper = mapper;
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
        let _ = speculation_depth;
        let links = self.symbol.entry(id).or_default();
        assert!(
            links.target.is_none() && links.mapper.is_none(),
            "instantiation links written twice for {id:?}"
        );
        links.target = Some(target);
        links.mapper = Some(mapper);
        links.name_type = name_type;
    }

    /// getTypeWithSyntheticDefaultImportType's synthType.syntheticType
    /// stamp (77794/77817) — the `if (!synthType.syntheticType)` guard
    /// makes this write-once.
    pub fn set_type_synthetic_type(&mut self, speculation_depth: u32, id: TypeId, value: TypeId) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        assert!(
            links.synthetic_type.is_none(),
            "syntheticType written twice for {id:?}"
        );
        links.synthetic_type = Some(value);
    }

    /// tsrs-native: links-table setter for tsc's type.defaultOnlyType write.
    pub fn set_type_default_only_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        let links = self.ty.entry(id).or_default();
        assert!(
            links.default_only_type.is_none(),
            "defaultOnlyType written twice for {id:?}"
        );
        links.default_only_type = Some(value);
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
        let _ = speculation_depth;
        let links = self.ty.entry(id).or_default();
        assert!(
            links.instantiated_target.is_none() && links.instantiated_mapper.is_none(),
            "type instantiation links written twice for {id:?}"
        );
        links.instantiated_target = Some(target);
        links.instantiated_mapper = Some(mapper);
    }

    /// tsrs-native: getSignatureInstantiation's inferredTypeParameters
    /// arm (59894) — `newReturnType.mapper =
    /// instantiatedSignature.mapper`, the one site that writes a type
    /// mapper WITHOUT an instantiation target
    /// (the isolated SingleSignatureType is freshly minted per clone,
    /// so the once-only assert holds by construction; its reader is
    /// getObjectTypeInstantiation's 63484 `type.mapper` read — the
    /// 63496 arm combines the INCOMING mapper, not this field).
    pub fn set_type_isolated_signature_mapper(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        mapper: MapperId,
    ) {
        // This initializes the mapper carried by a freshly allocated
        // isolated-signature type. It is semantic object construction,
        // not publication into a cache on a pre-existing type.
        let _ = speculation_depth;
        let links = self.ty.entry(id).or_default();
        assert!(
            links.instantiated_target.is_none() && links.instantiated_mapper.is_none(),
            "isolated-signature mapper written over instantiation links for {id:?}"
        );
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

    /// The late-binding pre-write/rewrite protocol (57717 → 57763):
    /// the EARLY table parks in the slot so re-entrant reads observe
    /// it mid-bind, then the combined table rewrites. Err unwinds must
    /// revert to Vacant (tsc cannot fail here) — a parked early table
    /// left behind would silently hide late members from later
    /// queries.
    pub fn set_symbol_resolved_members_late_bind(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: tsrs2_binder::SymbolTable,
    ) {
        let _ = speculation_depth;
        let slot = &mut self.symbol.entry(id).or_default().resolved_members;
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Resolved(value);
    }

    /// Err-unwind twin for the late-binding protocol.
    pub fn revert_symbol_resolved_members(&mut self, id: SymbolId) {
        let slot = &mut self.symbol.entry(id).or_default().resolved_members;
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Vacant;
    }

    /// The resolvedExports flavor of the late-binding protocol.
    pub fn set_symbol_resolved_exports_late_bind(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: tsrs2_binder::SymbolTable,
    ) {
        let _ = speculation_depth;
        let slot = &mut self.symbol.entry(id).or_default().resolved_exports;
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Resolved(value);
    }

    /// Err-unwind twin for the resolvedExports flavor.
    pub fn revert_symbol_resolved_exports(&mut self, id: SymbolId) {
        let slot = &mut self.symbol.entry(id).or_default().resolved_exports;
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Vacant;
    }

    /// tsrs-native: links accessor — resolveAlias' aliasTarget slot
    /// protocol writer (the tsc counterpart is the inline assignment
    /// family inside resolveAlias 49118-49134).
    ///
    /// The resolvedSignature twin — Vacant→Resolving on entry,
    /// resolvedSignature twin — Vacant→Resolving on entry,
    /// Resolving→Resolved for both the normal tail write and the
    /// re-entrant cycle collapse (the outer frame then observes
    /// Resolved and reports 5303 without writing).
    pub fn set_symbol_alias_target(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<SymbolId>,
    ) {
        self.journal_alias_target(speculation_depth, id);
        Self::write_slot(&mut self.symbol.entry(id).or_default().alias_target, value);
    }

    /// tsrs-native: initialize a freshly synthesized alias symbol.
    ///
    /// The wrapper symbol and its pre-resolved target are constructed
    /// together; no pre-existing cache entry is published.
    pub fn set_fresh_symbol_alias_target(&mut self, id: SymbolId, value: LinkSlot<SymbolId>) {
        Self::write_slot(&mut self.symbol.entry(id).or_default().alias_target, value);
    }

    /// tsrs-native: links accessor — Err-unwind twin for the alias
    /// protocol; only the frame that wrote the sentinel reverts
    /// (Resolved memos stay).
    pub fn revert_symbol_alias_target(&mut self, id: SymbolId) {
        let slot = &mut self.symbol.entry(id).or_default().alias_target;
        if matches!(slot, LinkSlot::Resolving) {
            note_resolving_transition(true, false);
            *slot = LinkSlot::Vacant;
        }
    }

    /// tsrs-native: links accessor — links.typeOnlyDeclaration writes
    /// (49182-49201): tsc assigns
    /// PLAINLY — the type-only-declaration arm re-stamps the same
    /// node, getTypeOnlyAliasDeclaration pre-writes `false` then marks
    /// with overwriteEmpty; the caller enforces the first-write-wins/
    /// overwriteEmpty policy, this setter is the raw store.
    pub fn set_symbol_type_only_declaration(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: Option<NodeId>,
    ) {
        self.journal_type_only_alias(speculation_depth, id);
        self.symbol.entry(id).or_default().type_only_declaration = Some(value);
    }

    /// tsrs-native: links accessor — links.typeOnlyExportStarName
    /// (49189).
    pub fn set_symbol_type_only_export_star_name(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: String,
    ) {
        self.journal_type_only_alias(speculation_depth, id);
        self.symbol
            .entry(id)
            .or_default()
            .type_only_export_star_name = Some(value);
    }

    /// tsrs-native: links accessor — the MODULE flavor of
    /// resolvedExports (getExportsOfModule 49838):
    /// written together with typeOnlyExportStarMap. tsc's unguarded
    /// tail assignment tolerates a deterministic re-entrant duplicate
    /// (the worker has no sentinel), so Resolved→Resolved(equal) is
    /// accepted; a DIFFERENT table is a protocol bug.
    pub fn set_symbol_module_exports(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        exports: tsrs2_binder::SymbolTable,
        type_only_export_star_map: Option<std::collections::HashMap<String, NodeId>>,
    ) {
        // The worker owns its cycle guard and returns the completed
        // table directly. A candidate may consume that table without
        // publishing it to the shared module-symbol memo.
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        let links = self.symbol.entry(id).or_default();
        match &links.resolved_exports {
            LinkSlot::Vacant | LinkSlot::Resolving => {
                note_resolving_transition(links.resolved_exports.is_resolving(), false);
                links.resolved_exports = LinkSlot::Resolved(exports);
                links.type_only_export_star_map = type_only_export_star_map;
            }
            LinkSlot::Resolved(existing) if *existing == exports => {}
            LinkSlot::Resolved(_) => {
                panic!("module resolvedExports rewritten with a different table: {id:?}")
            }
        }
    }

    /// tsrs-native: links accessor — links.exportsChecked once-latch
    /// (checkExternalModuleExports 86445); monotone.
    pub fn set_symbol_exports_checked(&mut self, speculation_depth: u32, id: SymbolId) {
        Self::assert_writable(speculation_depth);
        self.symbol.entry(id).or_default().exports_checked = true;
    }

    /// tsrs-native: links accessor — links.immediateTarget
    /// (getImmediateAliasedSymbol 50097); compute-once.
    pub fn set_symbol_immediate_target(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: Option<SymbolId>,
    ) {
        Self::assert_writable(speculation_depth);
        let slot = &mut self.symbol.entry(id).or_default().immediate_target;
        match slot {
            None => *slot = Some(value),
            Some(existing) if *existing == value => {}
            _ => panic!("immediateTarget rewritten: {slot:?} -> {value:?}"),
        }
    }

    /// tsrs-native: links accessor — links.cjsExportMerged (49697);
    /// compute-once, the same merged symbol may be re-stamped.
    pub fn set_symbol_cjs_export_merged(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: SymbolId,
    ) {
        Self::assert_writable(speculation_depth);
        let slot = &mut self.symbol.entry(id).or_default().cjs_export_merged;
        match slot {
            None => *slot = Some(value),
            Some(existing) if *existing == value => {}
            _ => panic!("cjsExportMerged rewritten: {slot:?} -> {value:?}"),
        }
    }

    /// `links.lateSymbol = ...` (addDeclarationToLateBoundSymbol 57652)
    /// on the MEMBER's binder symbol.
    pub fn set_symbol_late_symbol(&mut self, speculation_depth: u32, id: SymbolId, late: SymbolId) {
        if speculation_depth != 0
            && !self
                .speculative_late_symbol_writes
                .iter()
                .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            let previous = self.symbol.get(&id).and_then(|links| links.late_symbol);
            self.speculative_late_symbol_writes
                .push((speculation_depth, id, previous));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        self.symbol.entry(id).or_default().late_symbol = Some(late);
    }

    /// resolveDeclaredMembers' declared-members stamp (57604-57613),
    /// written once per class/interface/tuple target.
    pub fn set_type_declared_members(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: crate::state::MembersId,
    ) {
        if speculation_depth != 0 {
            return;
        }
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
        let _ = speculation_depth;
        self.ty.entry(id).or_default().resolved_base_types = Some(value);
    }

    /// `type.baseTypesResolved = true` (57244).
    pub fn set_type_base_types_resolved(&mut self, speculation_depth: u32, id: TypeId) {
        let _ = speculation_depth;
        self.ty.entry(id).or_default().base_types_resolved = true;
    }

    /// tsrs-native: remove the temporary base-type publication used
    /// while a rollback-capable candidate computes a cold base list.
    pub fn clear_speculative_type_base_types(&mut self, id: TypeId) {
        let links = self.ty.entry(id).or_default();
        links.resolved_base_types = None;
        links.base_types_resolved = false;
    }

    /// getWriteTypeOfAccessors' links.writeType stamp (56800).
    pub fn set_symbol_write_type(&mut self, speculation_depth: u32, id: SymbolId, value: TypeId) {
        self.journal_symbol_write_type(speculation_depth, id);
        Self::write_slot(
            &mut self.symbol.entry(id).or_default().write_type,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: initialize the write type of a freshly synthesized
    /// property symbol.
    pub fn set_fresh_symbol_write_type(&mut self, id: SymbolId, value: TypeId) {
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
        self.journal_type_instantiation(
            speculation_depth,
            id,
            SpeculativeTypeInstantiationKind::BaseConstructor,
        );
        Self::write_slot(
            &mut self
                .ty
                .entry(id)
                .or_default()
                .resolved_base_constructor_type,
            LinkSlot::Resolved(value),
        );
    }

    /// createTupleTargetType's tupleLabelDeclaration stamp (61170).
    /// The sole caller initializes a freshly synthesized tuple member.
    pub fn set_symbol_tuple_label_declaration(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        declaration: NodeId,
    ) {
        let _ = speculation_depth;
        let links = self.symbol.entry(id).or_default();
        assert!(
            links.tuple_label_declaration.is_none(),
            "tuple label written twice for {id:?}"
        );
        links.tuple_label_declaration = Some(declaration);
    }

    /// getSingleBaseForNonAugmentingSubtype's cachedEquivalentBaseType
    /// stamp (67713), guarded by IdenticalBaseTypeCalculated.
    pub fn ty_mut_cached_equivalent_base_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::assert_writable(speculation_depth);
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

    /// tsrs-native: Err-unwind retraction (tsc has no failure mode
    /// here). The declared-members twin of `retract_type_members`:
    /// the 5.9c staged publication (tsc resolveDeclaredMembers fills
    /// the type in place) parks the table before the signature/index
    /// walks; an Unsupported unwind must leave the slot Vacant, not
    /// partial.
    pub fn retract_type_declared_members(&mut self, id: TypeId) {
        let slot = &mut self.ty.entry(id).or_default().declared_members;
        assert!(
            matches!(slot, LinkSlot::Resolved(_)),
            "retract without a declared-members write for {id:?}"
        );
        *slot = LinkSlot::Vacant;
    }

    /// tsrs-native: initialize createDeferredTypeReference's node/mapper
    /// stamp (60196-60197) once on a fresh deferred-reference shell.
    pub fn set_fresh_type_deferred_reference_links(
        &mut self,
        id: TypeId,
        node: NodeId,
        mapper: Option<MapperId>,
    ) {
        let links = self.ty.entry(id).or_default();
        assert!(
            links.deferred_node.is_none() && links.deferred_mapper.is_none(),
            "deferred reference links written twice for {id:?}"
        );
        links.deferred_node = Some(node);
        links.deferred_mapper = mapper;
    }

    /// cloneTypeParameter/getRestrictiveTypeParameter target stamp.
    pub fn set_type_parameter_target(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        target: TypeId,
    ) {
        let _ = speculation_depth;
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
        // Defaults are lazily derived from declarations (or a targeted
        // parameter) and can be recomputed after candidate speculation.
        if speculation_depth != 0 {
            return;
        }
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.ty.entry(id).or_default().type_parameter_default,
            LinkSlot::Resolved(value),
        );
    }

    /// instantiateSignature's fresh-parameter mapper stamp (63418).
    pub fn set_type_parameter_mapper(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        mapper: MapperId,
    ) {
        let _ = speculation_depth;
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
        if speculation_depth != 0
            && !self
                .speculative_symbol_type_parameter_writes
                .iter()
                .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            let previous = self
                .symbol
                .get(&id)
                .and_then(|links| links.type_parameters.clone());
            self.speculative_symbol_type_parameter_writes
                .push((speculation_depth, id, previous));
        } else if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        let links = self.symbol.entry(id).or_default();
        assert!(
            links.type_parameters.is_none(),
            "alias type parameters written twice for {id:?}"
        );
        links.type_parameters = Some(type_parameters);
    }

    pub fn set_type_resolved_index_type(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        if speculation_depth != 0 {
            return;
        }
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
        if speculation_depth != 0 {
            return;
        }
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
        self.journal_type_instantiation(
            speculation_depth,
            id,
            SpeculativeTypeInstantiationKind::UniqueLiteralFilled,
        );
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
        self.journal_type_instantiation(
            speculation_depth,
            id,
            SpeculativeTypeInstantiationKind::Permissive,
        );
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
        self.journal_type_instantiation(
            speculation_depth,
            id,
            SpeculativeTypeInstantiationKind::Restrictive,
        );
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
        if speculation_depth != 0
            && !self
                .speculative_type_member_writes
                .iter()
                .any(|(depth, ty, _)| *depth == speculation_depth && *ty == id)
        {
            let previous = self
                .ty
                .get(&id)
                .map(|links| links.resolved_members.clone())
                .unwrap_or_default();
            self.speculative_type_member_writes
                .push((speculation_depth, id, previous));
        }
        let links = self.ty.entry(id).or_default();
        if speculation_depth == 0 {
            Self::assert_writable(speculation_depth);
        }
        let slot = &mut links.resolved_members;
        // setStructuredTypeMembers writes an empty table first as a
        // re-entrancy guard, then the real one (58333/58339) — allow
        // Resolved -> Resolved for that one tsc-shaped double write.
        match (&*slot, &value) {
            (LinkSlot::Resolved(_), LinkSlot::Resolved(_)) => *slot = value,
            _ => Self::write_slot(slot, value),
        }
    }

    /// tsrs-native: fresh structured-type initialization.
    ///
    /// Initialize the resolved-member payload of a freshly allocated
    /// semantic type. This is object construction, not publication of
    /// a cold cache on a pre-existing type.
    pub fn set_fresh_type_members(&mut self, id: TypeId, value: LinkSlot<crate::state::MembersId>) {
        Self::write_slot(&mut self.ty.entry(id).or_default().resolved_members, value);
    }
}
