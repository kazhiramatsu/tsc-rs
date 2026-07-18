//! CheckerState — the per-program checker context.
//!
//! Owns the program's binder runs (per-file arenas behind the
//! ProgramBinder view), the TypeTables, the links tables, the
//! signature arena, the diagnostics sink, the resolution stack
//! (pushTypeResolution 55728), and the global environment
//! (initializeTypeChecker slice, M4 5.0). M3 built the single-file
//! seed; M4 5.0 extends it program-wide.

use tsrs2_binder::{Binder, InternalSymbolName, SymbolId, SymbolTable};
use tsrs2_diags::{Diagnostic, DiagnosticList, DiagnosticMessage, MessageChain};
use tsrs2_syntax::{NodeId, SourceFile};
use tsrs2_types::{
    CheckFlags, CompilerOptions, ObjectFlags, SignatureFlags, SymbolFlags, TypeData, TypeFlags,
    TypeId, TypeSystemPropertyName, TypeTables,
};

use crate::instantiate::MapperId;
use crate::links::{LinkSlot, LinksTables};
use crate::program::ProgramBinder;
use crate::relate::RelationCaches;

/// A query the M3 slice cannot answer yet; carries the blocking
/// machinery's name so relpin failures read as scoping facts, not bugs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Unsupported {
    pub reason: String,
}

impl Unsupported {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

pub type CheckResult2<T> = Result<T, Unsupported>;

/// A module augmentation whose target is behind resolver machinery the
/// in-memory program resolver does not model. Keep the augmentation's
/// own container symbol: its resolved member/index tables are the
/// authoritative description of what the missing merge could add.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedModuleAugmentation {
    pub module_reference: String,
    pub augmentation_file: String,
    /// Export/member path below the augmented external module. The
    /// module object itself is the empty path; `N.X` is `["N", "X"]`.
    pub container_path: Vec<String>,
    pub container_symbol: SymbolId,
}

/// One frame of the outofbandVarianceMarkerHandler save/replace chain
/// (47113): getVariancesWorker's per-parameter closure is a Base,
/// recursiveTypeRelatedTo's wrapper is a Propagating accumulator that
/// chains to whatever was below it.
#[derive(Clone, Copy, Debug)]
pub enum VarianceHandlerFrame {
    /// getVariancesWorker 67331-67332: `onlyUnreliable ? unreliable =
    /// true : unmeasurable = true` — does NOT chain further down.
    Base {
        unmeasurable: bool,
        unreliable: bool,
    },
    /// recursiveTypeRelatedTo 65805-65809: accumulates
    /// ReportsUnreliable/ReportsUnmeasurable bits AND calls the
    /// original handler.
    Propagating(tsrs2_types::RelationComparisonResult),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SignatureId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureKind {
    Call,
    Construct,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MembersId(pub u32);

/// tsc Signature (core-interfaces §4). M4 5.2 adds the instantiation
/// surface (typeParameters/target/mapper + the instantiations and
/// erased caches); composite signatures arrive with 5.3 union members.
#[derive(Clone, Debug)]
pub struct Signature {
    /// None for synthesized signatures (getDefaultConstructSignatures'
    /// base-less arm 57967, tsc unknownSignature).
    pub declaration: Option<NodeId>,
    pub flags: SignatureFlags,
    /// tsc signature.typeParameters — generic signature construction
    /// from declarations lands with the 5.2 follow-up; instantiateSignature
    /// writes the fresh clones here (63413-63419).
    pub type_parameters: Option<Vec<TypeId>>,
    /// Parameter symbols in declaration order, `this` excluded.
    pub parameters: Vec<SymbolId>,
    pub this_parameter: Option<SymbolId>,
    pub min_argument_count: u32,
    /// Lazy return type with tsc's resolving sentinel
    /// (getReturnTypeOfSignature 59810 / pushTypeResolution).
    pub resolved_return_type: LinkSlot<TypeId>,
    /// strictFunctionTypes variance keys on the DECLARATION kind
    /// (method bivariance — core-interfaces §4 from_method).
    pub from_method: bool,
    /// tsc signature.target (instantiateSignature 63433).
    pub target: Option<SignatureId>,
    /// tsc signature.mapper (63434).
    pub mapper: Option<MapperId>,
    /// tsc signature.instantiations (getSignatureInstantiationWithout
    /// FillingInTypeArguments 59903), keyed by getTypeListId.
    pub instantiations: std::collections::HashMap<String, SignatureId>,
    /// tsc signature.erasedSignatureCache (getErasedSignature 59927).
    pub erased_signature_cache: Option<SignatureId>,
    /// tsc signature.compositeKind (createUnionSignature 57890 /
    /// combineSignaturesOfUnionMembers 58205 / intersection mixin
    /// clones): TypeFlags::UNION or ::INTERSECTION.
    pub composite_kind: Option<TypeFlags>,
    /// tsc signature.compositeSignatures (58206).
    pub composite_signatures: Option<Vec<SignatureId>>,
    /// tsc signature.optionalCallSignatureCache (getOptionalCallSignature
    /// 57899-57903): the (inner, outer) call-chain clone pair.
    pub optional_call_signature_cache: (Option<SignatureId>, Option<SignatureId>),
    /// Rust-only flavor for signatures whose display-only declaration
    /// is elided. None derives the flavor from `declaration` exactly as
    /// tsc does; synthetic call/construct factories set it explicitly
    /// so cache contents do not depend on which consumer runs first.
    pub isolated_signature_kind: Option<SignatureKind>,
    /// tsc signature.isolatedSignatureType (getOrCreateTypeFromSignature
    /// 60287): the single-signature anonymous object type memo.
    pub isolated_signature_type: Option<TypeId>,
}

/// tsc IndexInfo (createIndexInfo 59989).
#[derive(Clone, Debug)]
pub struct IndexInfo {
    pub key_type: TypeId,
    pub value_type: TypeId,
    pub is_readonly: bool,
    /// None for synthesized infos (anyBaseTypeIndexInfo 47282).
    pub declaration: Option<NodeId>,
    /// tsc IndexInfo.components (createIndexInfo 5th arg): the
    /// computed-name member declarations behind an object-literal
    /// index synthesis (getObjectLiteralIndexInfo 74110-74117).
    /// Dormant at M4 (index-constraint related spans consume later).
    pub components: Option<Vec<NodeId>>,
}

/// tsc-port: createWideningContext @6.0.3
/// tsc-hash: 45090097a709c1c9f72722e2fbd4bc8281837f4b16d6d769e70215a9a94c65c4
/// tsc-span: _tsc.js:67939-67941
///
/// tsc WideningContext: parent chain + property name + the lazily
/// resolved sibling types/properties (getSiblingsOfContext /
/// getPropertiesOfContext fill the None slots in place).
#[derive(Clone, Debug)]
pub struct WideningContext {
    pub parent: Option<WideningContextId>,
    pub property_name: Option<String>,
    pub siblings: Option<Vec<TypeId>>,
    pub resolved_properties: Option<Vec<SymbolId>>,
}

pub type WideningContextId = usize;

/// tsc resolved structured-type members (setStructuredTypeMembers
/// 50198): members table + named properties + signatures + index infos.
#[derive(Clone, Debug, Default)]
pub struct ResolvedMembers {
    pub members: SymbolTable,
    pub properties: Vec<SymbolId>,
    pub call_signatures: Vec<SignatureId>,
    pub construct_signatures: Vec<SignatureId>,
    pub index_infos: Vec<IndexInfo>,
}

/// The target of an in-flight lazy resolution (tsc pushes the symbol/
/// type/signature/node object itself; ids + a discriminant give the
/// same identity semantics).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionTarget {
    Symbol(SymbolId),
    Type(TypeId),
    Signature(SignatureId),
    Node(NodeId),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FlowContainmentCandidates {
    pub alias_declarations: Vec<NodeId>,
    pub guard_nodes: Vec<NodeId>,
}

pub(crate) type FlowContainmentIndex =
    std::collections::HashMap<Option<NodeId>, FlowContainmentCandidates>;

pub struct CheckerState<'a> {
    pub binder: ProgramBinder<'a>,
    pub options: &'a CompilerOptions,
    pub tables: TypeTables,
    /// tsc strictFunctionTypes via getStrictOptionValue.
    pub strict_function_types: bool,
    pub links: LinksTables,
    pub signatures: Vec<Signature>,
    pub members: Vec<ResolvedMembers>,
    /// checker-key §1.5: five per-relation caches + enumRelation.
    pub relations: RelationCaches,
    /// tsc subtypeReductionCache (47000), list-id keyed.
    pub subtype_reduction_cache: std::collections::HashMap<String, Vec<tsrs2_types::TypeId>>,
    /// greenfield §4.3: all links writes assert this is zero.
    pub speculation_depth: u32,
    /// createAnonymousType(undefined, emptySymbols, ...) (_tsc.js 47132).
    pub empty_object_type: TypeId,
    /// createAnonymousType(emptyTypeLiteralSymbol, ...) (47160).
    pub empty_type_literal_type: TypeId,
    /// tsc unknownEmptyObjectType (47161-47168): the `{}` member of the
    /// unknown-as-union decomposition.
    pub unknown_empty_object_type: TypeId,
    /// tsc unknownUnionType (47169): strictNullChecks ?
    /// undefined | null | unknownEmptyObjectType : unknownType — the
    /// getAdjustedTypeWithFacts stand-in for `unknown`.
    pub unknown_union_type: TypeId,
    /// createAnonymousType + instantiations map (47170) — the fallback
    /// for missing arity>0 globals (getTypeOfGlobalSymbol 60619).
    pub empty_generic_type: TypeId,
    /// createAnonymousType + NonInferrableType (47179) — the vacuous-
    /// exclusion type in isEmptyAnonymousObjectType, live from 5.0.
    pub any_function_type: TypeId,
    /// tsc emptyJsxObjectType (47140-47148): the JSX attributes spread
    /// seed (ObjectFlags::JSX_ATTRIBUTES).
    pub empty_jsx_object_type: TypeId,
    /// tsc emptyFreshJsxObjectType (47149-47157): the JSX opening
    /// FRAGMENT's synthetic effective argument.
    pub empty_fresh_jsx_object_type: TypeId,
    /// deferredGlobalPromiseType (60750) memo — emptyGenericType when
    /// the reporting probe missed.
    pub deferred_global_promise_type: Option<TypeId>,
    /// deferredGlobalPromiseLikeType (60758) memo — same discipline.
    pub deferred_global_promise_like_type: Option<TypeId>,
    /// deferredGlobalPromiseConstructorSymbol (60766) memo.
    pub deferred_global_promise_constructor_symbol: Option<Option<SymbolId>>,
    // ---- M4 5.7: the call-resolution signature singletons ----
    /// tsc anySignature (47220).
    pub any_signature: SignatureId,
    /// tsc unknownSignature (47234).
    pub unknown_signature: SignatureId,
    /// tsc resolvingSignature (47248): the links.resolvedSignature
    /// in-flight sentinel — an actual signature VALUE in tsc, kept so
    /// the M6-dead `signature === resolvingSignature` guards transcribe.
    pub resolving_signature: SignatureId,
    /// tsc silentNeverSignature (47262).
    pub silent_never_signature: SignatureId,
    /// tsc isInferencePartiallyBlocked (47420) — only ever set by M6
    /// inference; resolveCall's reportErrors stays true until then.
    pub(crate) is_inference_partially_blocked: bool,
    /// tsc apparentArgumentCount (77606) — only the signature-help LSP
    /// entry point sets it; None forever in the compile pipeline.
    pub(crate) apparent_argument_count: Option<usize>,

    /// tsc noConstraintType (47188): "constraint computed, none"
    /// sentinel in TypeParameter.constraint slots — never exposed.
    pub no_constraint_type: TypeId,
    /// tsc circularConstraintType (47196): the circular-constraint
    /// sentinel from getImmediateBaseConstraint — never exposed.
    pub circular_constraint_type: TypeId,

    // ---- M4 5.2: instantiation state ----
    /// The TypeMapper arena — MapperId equality IS tsc's mapper object
    /// identity (findActiveMapper 73616 compares `===`).
    pub(crate) mappers: Vec<crate::instantiate::TypeMapper>,
    /// tsc restrictiveMapper (47103).
    pub(crate) restrictive_mapper: crate::instantiate::MapperId,
    /// tsc permissiveMapper (47104).
    pub(crate) permissive_mapper: crate::instantiate::MapperId,
    /// tsc uniqueLiteralMapper (47112).
    pub(crate) unique_literal_mapper: crate::instantiate::MapperId,
    /// tsc reportUnreliableMapper (47114).
    pub(crate) report_unreliable_mapper: crate::instantiate::MapperId,
    /// tsc reportUnmeasurableMapper (47123).
    pub(crate) report_unmeasurable_mapper: crate::instantiate::MapperId,

    // ---- M4 5.3b: variance measurement state ----
    /// tsc markerSuperType/markerSubType/markerOtherType (47212-47215):
    /// the symbol-less measurement probes; markerSubType's inline
    /// constraint is markerSuperType. The ForCheck pair (47216-47218)
    /// waits for 5.4's checkTypeParameterDeferred (2636) and
    /// reportRelationError's marker guard (65075).
    pub(crate) marker_super_type: TypeId,
    pub(crate) marker_sub_type: TypeId,
    pub(crate) marker_other_type: TypeId,
    /// tsc markerSuperTypeForCheck/markerSubTypeForCheck (47216-47218):
    /// the checkTypeParameterDeferred (2636) probe pair; sub's inline
    /// constraint is super, like the measurement pair.
    pub(crate) marker_super_type_for_check: TypeId,
    pub(crate) marker_sub_type_for_check: TypeId,
    /// tsc varianceTypeParameter (47423): set around the 2636
    /// assignability check; the typeToString type-parameter arm (51535)
    /// renders the ForCheck markers as `super-T`/`sub-T` from it.
    pub(crate) variance_type_parameter: Option<TypeId>,
    /// tsc markerTypes (47005): ids of createMarkerType results.
    pub(crate) marker_types: std::collections::HashSet<TypeId>,
    /// tsc inVarianceComputation (47422).
    pub(crate) in_variance_computation: bool,
    /// tsc outofbandVarianceMarkerHandler (47113) — the save/replace
    /// closure chain as an explicit stack: getVariancesWorker pushes a
    /// Base per measured parameter, recursiveTypeRelatedTo pushes a
    /// Propagating wrapper (only when a handler already exists, like
    /// tsc's `if (outofbandVarianceMarkerHandler)` gate); a marker
    /// firing accumulates into Propagating frames down to the nearest
    /// Base — exactly the reach of tsc's chained closures.
    pub(crate) variance_handler_stack: Vec<VarianceHandlerFrame>,
    /// tsc activeTypeMappers/activeTypeMappersCaches/activeTypeMappersCount
    /// (47412-47414): the instantiation cache stack.
    pub(crate) active_type_mappers: Vec<crate::instantiate::MapperId>,
    pub(crate) active_type_mappers_caches: Vec<std::collections::HashMap<String, TypeId>>,
    /// tsc instantiationDepth/instantiationCount (46451-46452); the
    /// count resets at tsc's three entry points — checkExpression,
    /// checkSourceElement, checkDeferredNode (wired at 5.4/5.5).
    pub(crate) instantiation_depth: u32,
    pub(crate) instantiation_count: u64,
    /// tsc totalInstantiationCount (46450).
    pub total_instantiation_count: u64,
    /// Symbols whose class/interface declared type is mid-computation.
    /// tsc writes the shell into the links eagerly (57387) so cyclic
    /// heritage observes "no thisType yet"; the success-only slot write
    /// keeps Err unwinds re-queryable, and this set reproduces the
    /// mid-cycle observable for isThislessInterface's base walk.
    pub(crate) class_interface_declared_in_progress: Vec<SymbolId>,
    /// Type parameters whose default is mid-resolution — tsc's
    /// resolvingDefaultType sentinel (getResolvedTypeParameterDefault
    /// 59049), as a set so Err unwinds leave the links slot Vacant.
    pub(crate) type_parameter_defaults_in_progress: Vec<TypeId>,

    // ---- M5 6.1: control-flow query state ----
    /// tsc flowAnalysisDisabled (69399): latched by the depth-2000
    /// limiter; every later query answers errorType.
    /// (flowInvocationCount pre-exists below — M4 wired it for the
    /// getTypeOfExpression cache gate.)
    pub(crate) flow_analysis_disabled: bool,
    /// tsc sharedFlowNodes/sharedFlowTypes (69395-69396): one Vec of
    /// pairs; each getFlowTypeOfReference query scans only its own
    /// window (sharedFlowStart..) and truncates back on exit — also on
    /// Unsupported unwind (the unwind invariant).
    pub(crate) shared_flow: Vec<(tsrs2_binder::flow::FlowId, crate::flow::FlowType)>,
    /// The ReduceLabel antecedent swap (getTypeAtFlowNode 70473): tsc
    /// mutates `target.antecedent` in place during try/finally walks;
    /// the binder graph is immutable to the checker, so the swapped
    /// lists live here keyed by (file, label) and every label read
    /// consults them. Entries are strictly scoped to an in-progress
    /// ReduceLabel arm (restored on exit AND on unwind).
    pub(crate) reduce_label_overrides: std::collections::HashMap<
        (usize, tsrs2_binder::flow::FlowId),
        Vec<tsrs2_binder::flow::FlowId>,
    >,

    // ---- M4 5.4: check-driver state ----
    /// Any program file with a top-level `declare global` block
    /// tsc currentNode (46448): the element/deferred-node the driver is
    /// inside — related-info anchor for depth-limiter diagnostics
    /// (instantiateTypeWithAlias's 2589).
    pub(crate) current_node: Option<NodeId>,
    /// tsc NodeLinks.deferredNodes, keyed by the owning file's root
    /// node. Driver state, not memoization (like the resolution stack),
    /// so it lives here instead of the clone-on-read NodeLinks; the JS
    /// Set's visit-inserts-during-forEach semantics are reproduced by
    /// index iteration over the IndexSet.
    pub(crate) deferred_nodes: std::collections::HashMap<NodeId, indexmap::IndexSet<NodeId>>,

    // ---- M4 5.8a: checkSourceFileWorker's per-file accumulators ----
    // tsc potential*Collisions + potentialUnusedRenamedBindingElements
    // InTypes (46441-46445): cleared at worker entry, drained at the
    // worker tail. Deliberately ABSENT from UnwindSnapshot: they are
    // per-FILE accumulators that legitimately grow across elements
    // (same class as widening_contexts), not transient per-element
    // stacks. The this/newTarget vectors exist per §0 but their
    // CaptureThis/CaptureNewTarget pushers are downlevel-emit paths —
    // drains stay empty until a pusher lands (ledger).
    pub(crate) potential_this_collisions: Vec<NodeId>,
    pub(crate) potential_new_target_collisions: Vec<NodeId>,
    pub(crate) potential_weak_map_set_collisions: Vec<NodeId>,
    pub(crate) potential_reflect_collisions: Vec<NodeId>,
    pub(crate) potential_unused_renamed_binding_elements_in_types: Vec<NodeId>,
    /// tsc deferredGlobalDisposableType (60882) — emptyObjectType memo
    /// on miss, like the Promise pair above.
    pub(crate) deferred_global_disposable_type: Option<TypeId>,
    /// tsc deferredGlobalAsyncDisposableType (60890).
    pub(crate) deferred_global_async_disposable_type: Option<TypeId>,
    /// tsc deferredGlobalExtractSymbol (60907): None = uncomputed;
    /// Some(Some(unknown_symbol)) = miss memo (getter filters it).
    pub(crate) deferred_global_extract_symbol: Option<Option<SymbolId>>,

    // ---- M4 5.5d: name-suggestion state ----
    /// tsc suggestionCount (47423), capped by maximumSuggestionCount
    /// (47424) = 10: the checker-wide Did-you-mean budget. Every
    /// resolution failure that passes the onFailed guard chain
    /// consumes one slot — including lib-suggestion (2583-family) and
    /// no-suggestion (plain 2304) failures, but NOT guard-arm-handled
    /// ones (2662/2663/2693). The noLib bootstrap burns all 10 (see
    /// run_init_global_type_probes); oracle-pinned via
    /// strictBindCallApply:false (burn 8, budget 2).
    pub(crate) suggestion_count: u32,
    /// initializeTypeChecker's reportErrors=true getGlobalType probes
    /// (88779-88850), resolved EAGERLY at init so their failures burn
    /// the suggestion budget at tsc's time even though our global TYPE
    /// materialization stays lazy (the documented 5.0 deviation). The
    /// lazy getters consult this memo instead of re-probing — one
    /// resolveName-with-message per name per program, like tsc.
    pub(crate) init_global_type_probes: std::collections::HashMap<&'static str, Option<SymbolId>>,
    /// tsc deferredGlobalNonNullableTypeAlias: None = uncomputed,
    /// Some(None) = miss (unknownSymbol memo), Some(Some(_)) = alias.
    pub(crate) deferred_global_non_nullable_type_alias: Option<Option<SymbolId>>,
    /// tsc deferredGlobalAwaitedSymbol (60927): None = uncomputed;
    /// the reportErrors=true miss memoizes unknownSymbol (Some(Some(
    /// unknown_symbol)) — the getter filters it back to None).
    pub(crate) deferred_global_awaited_symbol: Option<Option<SymbolId>>,
    /// tsc awaitedTypeStack (47421): the getAwaitedTypeNoAlias
    /// circularity guard.
    pub(crate) awaited_type_stack: Vec<TypeId>,
    /// tsc deferredGlobalOmitSymbol (60917).
    pub(crate) deferred_global_omit_symbol: Option<Option<SymbolId>>,

    // ---- M4 5.6: widening state ----
    /// tsc WideningContext objects (createWideningContext 67939) —
    /// arena-allocated per widening walk; tsc keeps them alive via
    /// closure captures, here they just accumulate for the session.
    pub(crate) widening_contexts: Vec<WideningContext>,
    /// tsc undefinedProperties (47426): the per-checker
    /// getUndefinedProperty cache keyed by escaped name.
    pub(crate) undefined_properties: std::collections::HashMap<String, SymbolId>,

    // ---- M4 5.5: expression-checking state ----
    /// tsc typeofType (47100): union of the typeofNEFacts key literals
    /// in map-insertion order (stableTypeOrdering is absent from
    /// CompilerOptions, so the unsorted arm is the only one).
    pub(crate) typeof_type: TypeId,
    /// tsc contextualBindingPatterns (47408): pushed only by
    /// getTypeFromBindingPattern under includePatternInType (5.5b);
    /// checkIdentifier's nonInferrableAnyType circularity arm reads it.
    pub(crate) contextual_binding_patterns: Vec<NodeId>,

    // ---- M4 5.5b: contextual-typing state ----
    /// tsc contextualTypeNodes/contextualTypes/contextualIsCache
    /// (47404-47406): the parallel-array contextual stack —
    /// contextualTypeCount is the shared Vec length (pushContextualType
    /// 73569 writes all three at the same index).
    pub(crate) contextual_type_nodes: Vec<NodeId>,
    pub(crate) contextual_types: Vec<Option<TypeId>>,
    pub(crate) contextual_is_cache: Vec<bool>,
    /// tsc inferenceContextNodes/inferenceContexts (47401-47402) —
    /// exists-but-empty until M6 ([INFER] §0): every pushed value is
    /// None until M6 defines the InferenceContext payload, so
    /// getInferenceContext answers None structurally.
    pub(crate) inference_context_nodes: Vec<NodeId>,
    pub(crate) inference_contexts: Vec<Option<crate::contextual::InferenceContextPlaceholder>>,
    /// tsc cachedTypes (47415): the string-keyed side cache
    /// (getCachedType/setCachedType 47484-47490) — `B{typeId}` literal-
    /// base unions, `D{nodeId},{typeId}` object-literal discrimination.
    pub(crate) cached_types: std::collections::HashMap<String, TypeId>,

    // ---- M5 flow state (shape only — dormant until M5) ----
    /// tsc flowLoopStart/flowLoopCount (46436-46437): flow-loop stack
    /// cursor; checkExpressionCached (80580) save-resets it NOW so the
    /// M5 fixpoint edits land inside an already-correct save/restore.
    pub(crate) flow_loop_start: u32,
    pub(crate) flow_loop_count: u32,
    /// tsc flowTypeCache (46434): getTypeOfExpression's TypeCached
    /// side table — None until M5's flow analysis bumps
    /// flowInvocationCount (the cache-write gate at 80906 is
    /// constant-false until then).
    pub(crate) flow_type_cache: Option<std::collections::HashMap<NodeId, TypeId>>,
    /// tsc flowInvocationCount (46433).
    pub(crate) flow_invocation_count: u32,
    /// tsrs-native temporary [FLOW M5] containment index. It caches
    /// syntax candidates only (per source and nearest function scope),
    /// so each failed diagnostic need not walk the whole source again.
    pub(crate) flow_containment_indexes:
        std::cell::RefCell<std::collections::HashMap<NodeId, FlowContainmentIndex>>,
    /// tsrs-native temporary M8/checkJs containment index. Each JS
    /// source is scanned once, grouping simple assignment declarations
    /// by their final property name. The receiver/scope checks remain at
    /// the use site because they depend on the queried type.
    pub(crate) js_assignment_containment_indexes: std::cell::RefCell<
        std::collections::HashMap<NodeId, std::collections::HashMap<String, Vec<NodeId>>>,
    >,

    // ---- M4 5.0: the diags sink ----
    /// tsc `diagnostics` (createDiagnosticCollection) — the semantic
    /// sink; the driver (5.4) drains it per program.
    pub diagnostics: DiagnosticList,
    /// File-less diagnostics that tsc adds after its
    /// previousGlobalDiagnostics snapshot and therefore exposes from
    /// getSemanticDiagnostics. Lazy initialization diagnostics not
    /// registered here remain program-global.
    pub visible_global_diagnostics: DiagnosticList,
    /// Syntax ranges whose check was partial because an Unsupported
    /// containment boundary or an unimplemented flow-sensitive
    /// diagnostic was reached. Only directives targeting one of these
    /// ranges are exempt from unused @ts-expect-error diagnostics.
    pub(crate) partially_checked_ranges: std::collections::HashMap<usize, Vec<(u32, u32)>>,
    /// Public audit records corresponding to recognized Unsupported
    /// containment events. Unlike the byte ranges above, these use
    /// diagnostic-compatible UTF-16 coordinates.
    pub(crate) partial_check_records: Vec<crate::PartialCheck>,
    /// Literal operands whose `satisfies` elaboration already emitted
    /// an inner diagnostic. Re-checks must not add the outer 1360.
    pub(crate) elaborated_satisfies_expressions: std::collections::HashSet<NodeId>,

    // ---- M4 5.0: the global environment (initializeTypeChecker) ----
    /// tsc `globals` (46488): non-module file locals merged in program
    /// order.
    pub globals: SymbolTable,
    /// tsc undefinedSymbol (46489).
    pub undefined_symbol: SymbolId,
    /// tsc globalThisSymbol (46491) — its exports ARE `globals` (the
    /// table lives on this struct; getExportsOfSymbol special-cases).
    pub global_this_symbol: SymbolId,
    /// tsc argumentsSymbol (46495). Its `.type` (IArguments) stays a
    /// lazy accessor — see globals.rs.
    pub arguments_symbol: SymbolId,
    /// tsc requireSymbol (46496).
    pub require_symbol: SymbolId,
    /// tsc unknownSymbol (47006).
    pub unknown_symbol: SymbolId,
    /// tsc patternAmbientModules (initializeTypeChecker 88754-88756).
    pub pattern_ambient_modules: Vec<(String, String, SymbolId)>,
    /// tsc patternAmbientModuleAugmentations (mergeModuleAugmentation
    /// 47865): augmentation name → the unidirectionally-merged symbol.
    pub pattern_ambient_module_augmentations: std::collections::HashMap<String, SymbolId>,
    /// Module augmentations whose targets sat in the resolver's
    /// Suppressed band (node_modules/baseUrl machinery). Receiver
    /// provenance plus the augmentation container's own resolved
    /// members/index infos scope downstream property-miss containment.
    pub unresolved_module_augmentations:
        std::collections::HashMap<Vec<String>, Vec<UnresolvedModuleAugmentation>>,
    /// Nearest visible node_modules package root for one augmentation
    /// source and package name. Package discovery is host-wide, so cache
    /// it outside the property-miss hot path after the first lookup.
    pub(crate) unresolved_package_root_cache:
        std::cell::RefCell<std::collections::HashMap<(String, String), Option<String>>>,
    /// tsrs-native (M4 5.8d): normalized file path → program file
    /// index — the host.getResolvedModule seam's lookup table
    /// (program-and-modules.md §2; later files shadow earlier
    /// same-name entries like the program layer's last-index-by-name).
    pub program_path_index: std::collections::HashMap<String, usize>,
    /// tsrs-native (M4 5.8d): EVERY host input path (normalized),
    /// including files the program layer drops (.json bodies, .js
    /// without allowJs) — the resolver's suppression probes read this
    /// set to decide whether a miss is tsc-undecidable (FP=0 rule).
    pub host_file_paths: std::collections::HashSet<String>,
    /// Normalized package.json path → whether its `"type"` is
    /// `"module"`. Node16/NodeNext use the nearest package scope to
    /// determine the implied emit format of plain .ts/.js files.
    pub host_package_json_module_types: std::collections::HashMap<String, bool>,
    /// Normalized package.json path → its non-empty `"name"` field.
    /// Bare self-name imports are undecidable only inside a matching
    /// package scope; an unrelated package.json must not hide 2307.
    pub host_package_json_names: std::collections::HashMap<String, String>,
    /// Per-source getJsxNamespaceContainerForImplicitImport cache.
    /// `Some(None)` records an attempted miss so repeated JSX nodes do
    /// not duplicate the runtime-module diagnostic.
    pub(crate) jsx_implicit_import_containers: std::collections::HashMap<usize, Option<SymbolId>>,
    /// Variable-like declarations whose effective type came from the
    /// modeled JSDoc @type subset. Checked-JS assembly uses their exact
    /// diagnostic spans to expose the resulting semantic diagnostics
    /// without admitting unrelated JSDoc-dependent approximations.
    pub(crate) jsdoc_typed_declarations: std::collections::HashSet<NodeId>,
    /// Lazy getGlobal*Type memos (deferredGlobal* pattern 60679 for the
    /// deferred ones; the core init block 88788+ is deliberately LAZY
    /// here — m4-checker-skeleton-steps.md 5.0 — so each global starts
    /// resolving the moment 5.1's declared types exist).
    pub(crate) global_type_memos: crate::globals::GlobalTypeMemos,
    /// tsc decoratorContextOverrideTypeCache (78504): the per-shape
    /// `{name, private, static}` anonymous-type intern keyed by
    /// `{p|P}{s|S}{nameType.id}`.
    pub(crate) decorator_context_override_type_cache: std::collections::HashMap<String, TypeId>,

    // ---- M4 5.0: the resolution stack (pushTypeResolution 55728) ----
    pub(crate) resolution_targets: Vec<ResolutionTarget>,
    pub(crate) resolution_results: Vec<bool>,
    pub(crate) resolution_property_names: Vec<TypeSystemPropertyName>,
    /// Bumped during speculative/independent passes so cycles don't
    /// leak across them (checker-foundations §1.2).
    pub resolution_start: usize,

    // ---- M4 5.0: merge bookkeeping ----
    /// tsc mergedSymbols (recordMergedSymbol 47689): source →
    /// merge-target, per checker. A side map — NOT a symbol field —
    /// so shared (cached) lib binders stay immutable across programs.
    pub(crate) merged_symbols: std::collections::HashMap<SymbolId, SymbolId>,

    // ---- M4 5.0: cross-file duplicate grouping ----
    /// tsc amalgamatedDuplicates (initializeTypeChecker 88736; flushed
    /// at 88882-88905). Keyed by the ordered file-name pair.
    pub(crate) amalgamated_duplicates:
        indexmap::IndexMap<(String, String), crate::merge::FilesDuplicates>,
}

impl<'a> CheckerState<'a> {
    /// Single-file construction — the M3 signature, kept for the
    /// relpin probe and unit tests. `source` must be the binder's file.
    pub fn new(
        source: &'a SourceFile,
        binder: &'a Binder<'a>,
        options: &'a CompilerOptions,
    ) -> Self {
        assert!(std::ptr::eq(binder.source, source));
        Self::from_program(vec![binder], options)
    }

    /// Program construction (M4 5.0): binders in program order, each
    /// bound with contiguous id bases. Runs the initializeTypeChecker
    /// slice (globals merge + intrinsic symbol seeds + duplicate
    /// flush); merge diagnostics land in `self.diagnostics`.
    pub fn from_program(binders: Vec<&'a Binder<'a>>, options: &'a CompilerOptions) -> Self {
        let strict_null_checks = options.strict_option_value(options.strict_null_checks);
        let strict_function_types = options.strict_option_value(options.strict_function_types);
        let exact_optional = options.exact_optional_property_types.unwrap_or(false);
        let tables = TypeTables::new(strict_null_checks, exact_optional);
        let mut binder = ProgramBinder::new(binders);

        // The checker init block's symbols (46488-46496, 47006), in
        // tsc allocation order.
        let undefined_symbol = binder.create_symbol(SymbolFlags::PROPERTY, "undefined".to_owned());
        let global_this_symbol = binder.create_symbol(SymbolFlags::MODULE, "globalThis".to_owned());
        let arguments_symbol = binder.create_symbol(SymbolFlags::PROPERTY, "arguments".to_owned());
        let require_symbol = binder.create_symbol(SymbolFlags::PROPERTY, "require".to_owned());
        let unknown_symbol = binder.create_symbol(SymbolFlags::PROPERTY, "unknown".to_owned());

        let mut state = Self {
            binder,
            options,
            tables,
            strict_function_types,
            links: LinksTables::default(),
            signatures: Vec::new(),
            members: Vec::new(),
            relations: RelationCaches::default(),
            subtype_reduction_cache: std::collections::HashMap::new(),
            speculation_depth: 0,
            empty_object_type: TypeId(0),
            empty_type_literal_type: TypeId(0),
            unknown_empty_object_type: TypeId(0),
            unknown_union_type: TypeId(0),
            empty_generic_type: TypeId(0),
            any_function_type: TypeId(0),
            empty_jsx_object_type: TypeId(0),
            empty_fresh_jsx_object_type: TypeId(0),
            deferred_global_promise_type: None,
            deferred_global_promise_like_type: None,
            deferred_global_promise_constructor_symbol: None,
            any_signature: SignatureId(0),
            unknown_signature: SignatureId(0),
            resolving_signature: SignatureId(0),
            silent_never_signature: SignatureId(0),
            is_inference_partially_blocked: false,
            apparent_argument_count: None,
            no_constraint_type: TypeId(0),
            circular_constraint_type: TypeId(0),
            mappers: Vec::new(),
            restrictive_mapper: crate::instantiate::MapperId(0),
            permissive_mapper: crate::instantiate::MapperId(0),
            unique_literal_mapper: crate::instantiate::MapperId(0),
            report_unreliable_mapper: crate::instantiate::MapperId(0),
            report_unmeasurable_mapper: crate::instantiate::MapperId(0),
            marker_super_type: TypeId(0),
            marker_sub_type: TypeId(0),
            marker_other_type: TypeId(0),
            marker_super_type_for_check: TypeId(0),
            marker_sub_type_for_check: TypeId(0),
            variance_type_parameter: None,
            marker_types: std::collections::HashSet::new(),
            in_variance_computation: false,
            variance_handler_stack: Vec::new(),
            active_type_mappers: Vec::new(),
            active_type_mappers_caches: Vec::new(),
            instantiation_depth: 0,
            instantiation_count: 0,
            total_instantiation_count: 0,
            class_interface_declared_in_progress: Vec::new(),
            type_parameter_defaults_in_progress: Vec::new(),
            flow_analysis_disabled: false,
            shared_flow: Vec::new(),
            reduce_label_overrides: std::collections::HashMap::new(),
            current_node: None,
            deferred_nodes: std::collections::HashMap::new(),
            potential_this_collisions: Vec::new(),
            potential_new_target_collisions: Vec::new(),
            potential_weak_map_set_collisions: Vec::new(),
            potential_reflect_collisions: Vec::new(),
            potential_unused_renamed_binding_elements_in_types: Vec::new(),
            deferred_global_disposable_type: None,
            deferred_global_async_disposable_type: None,
            deferred_global_extract_symbol: None,
            suggestion_count: 0,
            init_global_type_probes: std::collections::HashMap::new(),
            deferred_global_non_nullable_type_alias: None,
            deferred_global_awaited_symbol: None,
            awaited_type_stack: Vec::new(),
            deferred_global_omit_symbol: None,
            widening_contexts: Vec::new(),
            undefined_properties: std::collections::HashMap::new(),
            typeof_type: TypeId(0),
            contextual_binding_patterns: Vec::new(),
            contextual_type_nodes: Vec::new(),
            contextual_types: Vec::new(),
            contextual_is_cache: Vec::new(),
            inference_context_nodes: Vec::new(),
            inference_contexts: Vec::new(),
            cached_types: std::collections::HashMap::new(),
            flow_loop_start: 0,
            flow_loop_count: 0,
            flow_type_cache: None,
            flow_invocation_count: 0,
            flow_containment_indexes: Default::default(),
            js_assignment_containment_indexes: Default::default(),
            diagnostics: Vec::new(),
            visible_global_diagnostics: Vec::new(),
            partially_checked_ranges: std::collections::HashMap::new(),
            partial_check_records: Vec::new(),
            elaborated_satisfies_expressions: std::collections::HashSet::new(),
            globals: SymbolTable::default(),
            undefined_symbol,
            global_this_symbol,
            arguments_symbol,
            require_symbol,
            unknown_symbol,
            pattern_ambient_modules: Vec::new(),
            pattern_ambient_module_augmentations: std::collections::HashMap::new(),
            unresolved_module_augmentations: std::collections::HashMap::new(),
            unresolved_package_root_cache: Default::default(),
            program_path_index: std::collections::HashMap::new(),
            host_file_paths: std::collections::HashSet::new(),
            host_package_json_module_types: std::collections::HashMap::new(),
            host_package_json_names: std::collections::HashMap::new(),
            jsx_implicit_import_containers: std::collections::HashMap::new(),
            jsdoc_typed_declarations: std::collections::HashSet::new(),
            global_type_memos: Default::default(),
            decorator_context_override_type_cache: Default::default(),
            resolution_targets: Vec::new(),
            resolution_results: Vec::new(),
            resolution_property_names: Vec::new(),
            resolution_start: 0,
            merged_symbols: std::collections::HashMap::new(),
            amalgamated_duplicates: indexmap::IndexMap::new(),
        };
        // undefinedSymbol.declarations = [] (46490); globalThisSymbol
        // is Readonly (46491) and its exports are `globals` (46492) —
        // the table lives on CheckerState; globals gains the
        // "globalThis" entry itself (46494).
        state.links.set_symbol_check_flags(
            state.speculation_depth,
            global_this_symbol,
            CheckFlags::READONLY,
        );
        // The module resolver's path table (M4 5.8d): later files
        // shadow earlier same-name entries (lib.rs last_index_by_name).
        for index in 0..state.binder.file_count() {
            let normalized =
                Self::normalize_program_path(&state.binder.source(index).file_name, "");
            state.program_path_index.insert(normalized, index);
        }
        state
            .globals
            .insert("globalThis".to_owned(), global_this_symbol);

        // tsc restrictiveMapper/permissiveMapper (47103-47104): the two
        // function-mapper singletons.
        state.restrictive_mapper = state.alloc_mapper(crate::instantiate::TypeMapper::Function(
            crate::instantiate::FunctionMapper::Restrictive,
        ));
        state.permissive_mapper = state.alloc_mapper(crate::instantiate::TypeMapper::Function(
            crate::instantiate::FunctionMapper::Permissive,
        ));
        // tsc uniqueLiteralMapper (47112).
        state.unique_literal_mapper = state.alloc_mapper(crate::instantiate::TypeMapper::Function(
            crate::instantiate::FunctionMapper::UniqueLiteral,
        ));
        // tsc-port: reportUnreliableMapper + reportUnmeasurableMapper @6.0.3
        // tsc-hash: 23800396495e1d0c1eba42745b023673852d55da0f6a4b92c2f9704b086d044e
        // tsc-span: _tsc.js:47113-47131
        state.report_unreliable_mapper =
            state.alloc_mapper(crate::instantiate::TypeMapper::Function(
                crate::instantiate::FunctionMapper::ReportsUnreliable,
            ));
        state.report_unmeasurable_mapper =
            state.alloc_mapper(crate::instantiate::TypeMapper::Function(
                crate::instantiate::FunctionMapper::ReportsUnmeasurable,
            ));

        // The empty anonymous types from the checker init block
        // (47132/47160/47170/47179): resolved-empty from birth.
        state.empty_object_type = state.create_resolved_empty_anonymous_type(None);
        // tsc-port: emptyJsxObjectType + emptyFreshJsxObjectType @6.0.3
        // tsc-hash: 3adb5d2dbb1653e5c2d1e59e931b18b6d357619cc9836867b39209a05dd6a70b
        // tsc-span: _tsc.js:47140-47157
        state.empty_jsx_object_type = state.create_resolved_empty_anonymous_type(None);
        let jsx_flags =
            state.tables.object_flags_of(state.empty_jsx_object_type) | ObjectFlags::JSX_ATTRIBUTES;
        state
            .tables
            .type_mut(state.empty_jsx_object_type)
            .object_flags = jsx_flags;
        state.empty_fresh_jsx_object_type = state.create_resolved_empty_anonymous_type(None);
        let fresh_jsx_flags = state
            .tables
            .object_flags_of(state.empty_fresh_jsx_object_type)
            | ObjectFlags::JSX_ATTRIBUTES
            | ObjectFlags::FRESH_LITERAL
            | ObjectFlags::OBJECT_LITERAL
            | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL;
        state
            .tables
            .type_mut(state.empty_fresh_jsx_object_type)
            .object_flags = fresh_jsx_flags;
        let empty_type_literal_symbol = state.binder.create_symbol(
            SymbolFlags::TYPE_LITERAL,
            InternalSymbolName::TYPE.to_owned(),
        );
        state.empty_type_literal_type =
            state.create_resolved_empty_anonymous_type(Some(empty_type_literal_symbol));
        // tsc-port: unknownEmptyObjectType + unknownUnionType @6.0.3
        // tsc-hash: bec4f96b4a16d460fc25fd2ad7063b611a988b7d8ba22c1d10664f9dad0c5042
        // tsc-span: _tsc.js:47161-47169
        state.unknown_empty_object_type = state.create_resolved_empty_anonymous_type(None);
        state.unknown_union_type = if state
            .options
            .strict_option_value(state.options.strict_null_checks)
        {
            let members = [
                state.tables.intrinsics.undefined,
                state.tables.intrinsics.null,
                state.unknown_empty_object_type,
            ];
            state
                .get_union_type_ex(&members, tsrs2_types::UnionReduction::Literal)
                .expect("intrinsic unions cannot fail")
        } else {
            state.tables.intrinsics.unknown
        };
        // tsc-port: emptyGenericType @6.0.3
        // tsc-hash: 3f49927f2f7e3c7b65435327e84949b85526b9c0268f890dac1b470a84b51cab
        // tsc-span: _tsc.js:47170-47178
        // (`instantiations = new Map()` is implicit: the tables-level
        // createTypeReference interning map covers every target.)
        state.empty_generic_type = state.create_resolved_empty_anonymous_type(None);
        // tsc-port: anyFunctionType @6.0.3
        // tsc-hash: 9bb6a61eaf2a3ddc4a86843b2703fce53de34f8130af9d450944af65ebd7bd2e
        // tsc-span: _tsc.js:47179-47187
        state.any_function_type = state.create_resolved_empty_anonymous_type(None);
        let any_function_flags = state.tables.object_flags_of(state.any_function_type)
            | ObjectFlags::NON_INFERRABLE_TYPE;
        state.tables.type_mut(state.any_function_type).object_flags = any_function_flags;
        // tsc-port: noConstraintType + circularConstraintType @6.0.3
        // tsc-hash: 06e5cd556cafd99a8a477e291385d1cb488f4d28676756e76a7ef6135c9d198b
        // tsc-span: _tsc.js:47188-47203
        state.no_constraint_type = state.create_resolved_empty_anonymous_type(None);
        state.circular_constraint_type = state.create_resolved_empty_anonymous_type(None);
        // tsc-port: markerSuperType + markerSubType + markerOtherType @6.0.3
        // tsc-hash: b01ebfcde068d7826bd3c12f41c9869047b7a25af2c00e6dd66cca614c1d8f38
        // tsc-span: _tsc.js:47212-47215
        state.marker_super_type = state.tables.create_synthesized_type_parameter(None);
        state.marker_sub_type = state
            .tables
            .create_synthesized_type_parameter(Some(state.marker_super_type));
        state.marker_other_type = state.tables.create_synthesized_type_parameter(None);
        // tsc-port: markerSuperTypeForCheck + markerSubTypeForCheck @6.0.3
        // tsc-hash: 119485aecd36a63821c1191c69eb9b05cf44460f4dca261f773403ea54131d2b
        // tsc-span: _tsc.js:47216-47218
        state.marker_super_type_for_check = state.tables.create_synthesized_type_parameter(None);
        state.marker_sub_type_for_check = state
            .tables
            .create_synthesized_type_parameter(Some(state.marker_super_type_for_check));
        // tsc-port: anySignature + unknownSignature + resolvingSignature + silentNeverSignature @6.0.3
        // tsc-hash: 72420792b8e72d9feaad43a2af6671f0782fbcaec8d644171340e8bb2eff04f2
        // tsc-span: _tsc.js:47220-47275
        let any = state.tables.intrinsics.any;
        let error_type = state.tables.intrinsics.error;
        let silent_never = state.tables.intrinsics.silent_never;
        state.any_signature = state.create_singleton_signature(any);
        state.unknown_signature = state.create_singleton_signature(error_type);
        state.resolving_signature = state.create_singleton_signature(any);
        state.silent_never_signature = state.create_singleton_signature(silent_never);
        // tsc-port: createTypeofType @6.0.3
        // tsc-hash: fe7ba70726e502c98681faa1024d1b2ba663aac0e621246079642989bfa774df
        // tsc-span: _tsc.js:50136-50138
        // (typeofNEFacts insertion order, 46376-46385.)
        state.typeof_type = {
            let members: Vec<TypeId> = [
                "string",
                "number",
                "bigint",
                "boolean",
                "symbol",
                "undefined",
                "object",
                "function",
            ]
            .iter()
            .map(|name| state.tables.get_string_literal_type(name))
            .collect();
            state
                .get_union_type_ex(&members, tsrs2_types::UnionReduction::Literal)
                .expect("literal unions cannot fail")
        };

        // initializeTypeChecker slice (88732-88906): globals merge +
        // symbol-type seeds + amalgamated-duplicate flush.
        state.initialize_program_globals();
        state.run_init_global_type_probes();
        // (has_global_augmentation RETIRED 5.8d: declare-global exports
        // merge in merge_module_augmentations; the resolver failure
        // band and the JSX containment both lifted.)
        state
    }

    /// The 47220-47275 singleton shape: declaration-less, no
    /// parameters, minArg 0, pre-resolved return type.
    fn create_singleton_signature(&mut self, return_type: TypeId) -> SignatureId {
        self.alloc_signature(Signature {
            declaration: None,
            flags: SignatureFlags::NONE,
            type_parameters: None,
            parameters: Vec::new(),
            this_parameter: None,
            min_argument_count: 0,
            resolved_return_type: LinkSlot::Resolved(return_type),
            from_method: false,
            target: None,
            mapper: None,
            instantiations: std::collections::HashMap::new(),
            erased_signature_cache: None,
            composite_kind: None,
            composite_signatures: None,
            optional_call_signature_cache: (None, None),
            isolated_signature_kind: Some(SignatureKind::Construct),
            isolated_signature_type: None,
        })
    }

    /// tsc-port: createAnonymousType @6.0.3
    /// tsc-hash: 801cde8bdea7de88d9052f5f01d296c15ec067902d478f857925edd1106efb93
    /// tsc-span: _tsc.js:50208-50210
    pub(crate) fn create_resolved_empty_anonymous_type(
        &mut self,
        symbol: Option<SymbolId>,
    ) -> TypeId {
        let id = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
        self.tables.type_mut(id).object_flags = ObjectFlags::ANONYMOUS;
        self.tables.type_mut(id).symbol = symbol;
        let members = self.alloc_members(ResolvedMembers::default());
        self.links
            .set_type_members(self.speculation_depth, id, LinkSlot::Resolved(members));
        id
    }

    pub fn alloc_members(&mut self, members: ResolvedMembers) -> MembersId {
        let id = MembersId(self.members.len() as u32);
        self.members.push(members);
        id
    }

    pub fn members_of(&self, id: MembersId) -> &ResolvedMembers {
        &self.members[id.0 as usize]
    }

    /// In-place mutation for tsc's repeated setStructuredTypeMembers
    /// over the SAME ResolvedType object (resolveObjectTypeMembers
    /// 57829/57840: the early write makes partial members observable
    /// to mid-cycle readers, then inheritance mutates the table).
    pub fn members_mut(&mut self, id: MembersId) -> &mut ResolvedMembers {
        &mut self.members[id.0 as usize]
    }

    pub fn alloc_signature(&mut self, signature: Signature) -> SignatureId {
        let id = SignatureId(self.signatures.len() as u32);
        self.signatures.push(signature);
        id
    }

    pub fn signature_mut(&mut self, id: SignatureId) -> &mut Signature {
        &mut self.signatures[id.0 as usize]
    }

    pub fn signature_of(&self, id: SignatureId) -> &Signature {
        &self.signatures[id.0 as usize]
    }

    /// Empty member table shared by symbols that never had one.
    pub fn symbol_members(&self, symbol: SymbolId) -> &SymbolTable {
        &self.binder.symbol(symbol).members
    }

    /// File-scope name resolution for the relpin scratch program — the
    /// M3/5.0 slice of resolveEntityName: the first file's locals (its
    /// root scope), then the merged globals, meaning-filtered, with the
    /// getMergedSymbol chase (49932) so cross-file merged declarations
    /// surface. Full lexical walking is resolveName (M4 5.1).
    pub fn resolve_file_scope_name(&self, name: &str, meaning: SymbolFlags) -> Option<SymbolId> {
        let root = self.binder.source(0).root;
        let symbol = self
            .binder
            .locals_of(root)
            .and_then(|locals| locals.get(name).copied())
            .or_else(|| self.globals.get(name).copied())?;
        let symbol = self.get_merged_symbol(symbol);
        let flags = self.binder.symbol(symbol).flags;
        flags.intersects(meaning).then_some(symbol)
    }

    pub fn symbol_flags(&self, symbol: SymbolId) -> SymbolFlags {
        self.binder.symbol(symbol).flags
    }

    pub fn node_symbol(&self, node: NodeId) -> Option<SymbolId> {
        self.binder.node_symbol(node)
    }

    /// The binder's mutable node-flags view (ContainsThis etc.).
    pub fn node_flags(&self, node: NodeId) -> i32 {
        self.binder.flags_of(node).bits()
    }

    // ---- M4 5.0: the resolution stack ----

    /// tsc-port: pushTypeResolution @6.0.3
    /// tsc-hash: 3500b199892a5c2ff22cc970b8904518a03c9334df9126534194b902f0a23af4
    /// tsc-span: _tsc.js:55728-55744
    pub fn push_type_resolution(
        &mut self,
        target: ResolutionTarget,
        property_name: TypeSystemPropertyName,
    ) -> bool {
        if let Some(cycle_start) = self.find_resolution_cycle_start_index(target, property_name) {
            for result in &mut self.resolution_results[cycle_start..] {
                *result = false;
            }
            return false;
        }
        self.resolution_targets.push(target);
        self.resolution_results.push(true);
        self.resolution_property_names.push(property_name);
        true
    }

    /// tsc-port: findResolutionCycleStartIndex @6.0.3
    /// tsc-hash: 251a7ddb169a1bcea40755dfb143b2ccc1043fd9860cc5ad3658792bb09ada88
    /// tsc-span: _tsc.js:55745-55755
    pub(crate) fn find_resolution_cycle_start_index(
        &self,
        target: ResolutionTarget,
        property_name: TypeSystemPropertyName,
    ) -> Option<usize> {
        for i in (self.resolution_start..self.resolution_targets.len()).rev() {
            if self.resolution_target_has_property(
                self.resolution_targets[i],
                self.resolution_property_names[i],
            ) {
                return None;
            }
            if self.resolution_targets[i] == target
                && self.resolution_property_names[i] == property_name
            {
                return Some(i);
            }
        }
        None
    }

    /// tsc-port: resolutionTargetHasProperty @6.0.3
    /// tsc-hash: b8e73a933bc593ba2bcebdae85d9dd1766694a21805d7a2e7b9daed8fea56903
    /// tsc-span: _tsc.js:55756-55778
    ///
    /// Arms whose backing storage lands in a later stage are
    /// unreachable TODAY because no pushTypeResolution call site passes
    /// that property name yet (grep-able constructibility argument, per
    /// the M4 ledger rule); each arm un-stubs with its owning stage.
    fn resolution_target_has_property(
        &self,
        target: ResolutionTarget,
        property_name: TypeSystemPropertyName,
    ) -> bool {
        match property_name {
            TypeSystemPropertyName::TYPE => {
                let ResolutionTarget::Symbol(symbol) = target else {
                    unreachable!("Type resolution targets are symbols");
                };
                self.links.symbol(symbol).type_of_symbol.resolved().is_some()
            }
            TypeSystemPropertyName::DECLARED_TYPE => {
                let ResolutionTarget::Symbol(symbol) = target else {
                    unreachable!("DeclaredType resolution targets are symbols");
                };
                self.links.symbol(symbol).declared_type.resolved().is_some()
            }
            TypeSystemPropertyName::RESOLVED_RETURN_TYPE => {
                let ResolutionTarget::Signature(signature) = target else {
                    unreachable!("ResolvedReturnType resolution targets are signatures");
                };
                self.signature_of(signature)
                    .resolved_return_type
                    .resolved()
                    .is_some()
            }
            TypeSystemPropertyName::IMMEDIATE_BASE_CONSTRAINT => {
                let ResolutionTarget::Type(ty) = target else {
                    unreachable!("ImmediateBaseConstraint resolution targets are types");
                };
                self.links
                    .ty(ty)
                    .immediate_base_constraint
                    .resolved()
                    .is_some()
            }
            TypeSystemPropertyName::RESOLVED_TYPE_ARGUMENTS => {
                let ResolutionTarget::Type(ty) = target else {
                    unreachable!("ResolvedTypeArguments resolution targets are types");
                };
                // `!!type.resolvedTypeArguments` (55771) — only deferred
                // references are pushed under this property, and their
                // slot fills through getTypeArguments.
                self.tables.try_type_arguments(ty).is_some()
            }
            TypeSystemPropertyName::RESOLVED_BASE_TYPES => {
                let ResolutionTarget::Type(ty) = target else {
                    unreachable!("ResolvedBaseTypes resolution targets are types");
                };
                // `!!type.baseTypesResolved` (55772).
                self.links.ty(ty).base_types_resolved
            }
            TypeSystemPropertyName::RESOLVED_BASE_CONSTRUCTOR_TYPE => {
                let ResolutionTarget::Type(ty) = target else {
                    unreachable!("ResolvedBaseConstructorType resolution targets are types");
                };
                self.links
                    .ty(ty)
                    .resolved_base_constructor_type
                    .resolved()
                    .is_some()
            }
            TypeSystemPropertyName::WRITE_TYPE => {
                let ResolutionTarget::Symbol(symbol) = target else {
                    unreachable!("WriteType resolution targets are symbols");
                };
                self.links.symbol(symbol).write_type.resolved().is_some()
            }
            // ParameterInitializerContainsUndefined lands with 5.8.
            _ => unreachable!(
                "no pushTypeResolution call site passes {property_name:?} yet (owning stage per M4 doc)"
            ),
        }
    }

    /// tsc-port: popTypeResolution @6.0.3
    /// tsc-hash: c24c76f995413df7f5693c49607ca294f8d39b719a20966c40610f6ec684d48f
    /// tsc-span: _tsc.js:55779-55783
    pub fn pop_type_resolution(&mut self) -> bool {
        self.resolution_targets.pop();
        self.resolution_property_names.pop();
        self.resolution_results
            .pop()
            .expect("pop_type_resolution without matching push")
    }

    // ---- M4 5.0: error helpers (the `error` family, 47565-47587) ----

    /// tsc createDiagnosticForNode(InSourceFile): span from
    /// getErrorSpanForNode, positions in UTF-16 (the binder's
    /// diagnostic_for_node twin, program-wide).
    pub fn diagnostic_for_node(
        &self,
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> Diagnostic {
        let source = self.binder.source_of_node(node);
        let (start, end) = tsrs2_binder::node_util::get_error_span_for_node(source, node);
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let start_utf16 = to_utf16(start);
        let end_utf16 = to_utf16(end);
        Diagnostic::new(
            Some(source.file_name.clone()),
            Some(start_utf16),
            Some(end_utf16.saturating_sub(start_utf16)),
            MessageChain::new(message, &args),
        )
    }

    /// tsrs-native: containment bookkeeping for source ranges whose
    /// diagnostics may be incomplete.
    ///
    /// The program layer exempts only directives targeting one of
    /// these ranges instead of suppressing 2578 for the entire file.
    pub(crate) fn mark_partially_checked_node(&mut self, node: NodeId, reason: impl Into<String>) {
        let file_index = self.binder.file_index_of_node(node);
        let source = self.binder.source_of_node(node);
        let raw = source.arena.node(node);
        let range = (raw.pos, raw.end);
        let ranges = self.partially_checked_ranges.entry(file_index).or_default();
        if !ranges.contains(&range) {
            ranges.push(range);
        }
        let to_utf16 = |byte: u32| {
            source
                .line_map
                .byte_to_utf16
                .get(byte as usize)
                .copied()
                .unwrap_or(byte)
        };
        let start = to_utf16(raw.pos);
        let end = to_utf16(raw.end);
        let record = crate::PartialCheck {
            file_name: source.file_name.clone(),
            start,
            length: end.saturating_sub(start),
            reason: reason.into(),
        };
        if !self.partial_check_records.contains(&record) {
            self.partial_check_records.push(record);
        }
    }

    // ---- out-of-band variance marker handler (M4 5.3b) ----

    /// The reporter-mapper closures' `t === markerSuperType || ...`
    /// gate (47115/47124) followed by the handler call.
    pub(crate) fn fire_variance_marker_if_marker(&mut self, ty: TypeId, only_unreliable: bool) {
        if ty == self.marker_super_type
            || ty == self.marker_sub_type
            || ty == self.marker_other_type
        {
            self.fire_outofband_variance_marker(only_unreliable);
        }
    }

    /// Invoke the current handler chain: every Propagating wrapper
    /// accumulates and forwards; the nearest Base records and stops
    /// (its closure never chains to the handler it displaced).
    fn fire_outofband_variance_marker(&mut self, only_unreliable: bool) {
        for frame in self.variance_handler_stack.iter_mut().rev() {
            match frame {
                VarianceHandlerFrame::Propagating(flags) => {
                    let bit = if only_unreliable {
                        tsrs2_types::RelationComparisonResult::REPORTS_UNRELIABLE
                    } else {
                        tsrs2_types::RelationComparisonResult::REPORTS_UNMEASURABLE
                    };
                    *flags =
                        tsrs2_types::RelationComparisonResult::from_bits(flags.bits() | bit.bits());
                }
                VarianceHandlerFrame::Base {
                    unmeasurable,
                    unreliable,
                } => {
                    if only_unreliable {
                        *unreliable = true;
                    } else {
                        *unmeasurable = true;
                    }
                    return;
                }
            }
        }
    }

    /// tsc-port: createError @6.0.3
    /// tsc-hash: dedcf6cc6c301274f018ef98543f4abebe1b7826c45f601b914137812caa8cfa
    /// tsc-span: _tsc.js:47580-47582
    ///
    /// No location ⇒ createCompilerDiagnostic: a file-less,
    /// program-level diagnostic.
    pub fn create_error(
        &self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> Diagnostic {
        match location {
            Some(node) => self.diagnostic_for_node(node, message, args),
            None => {
                let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
                Diagnostic::new(None, None, None, MessageChain::new(message, &args))
            }
        }
    }

    /// tsc-port: error @6.0.3
    /// tsc-hash: be9cd419909a0ad4fd544342a9a6c97f837da3819b2844e45c7be96b438439c9
    /// tsc-span: _tsc.js:47583-47587
    ///
    /// diagnostics.add (createDiagnosticCollection 16229-16246) drops
    /// EXACT duplicates via insertSorted's equality comparer — first
    /// live emitter of duplicates is the circular-base-type pair of
    /// report sites (getBaseTypes pop-failure + hasBaseType arm), which
    /// tsc collapses to one 2310 per declaration.
    pub fn error_at(
        &mut self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> usize {
        let diagnostic = self.create_error(location, message, args);
        if let Some(existing) = self
            .diagnostics
            .iter()
            .position(|existing| *existing == diagnostic)
        {
            return existing;
        }
        self.diagnostics.push(diagnostic);
        self.diagnostics.len() - 1
    }

    /// tsc-port: errorSkippedOn @6.0.3
    /// tsc-hash: 2115d9b0e896363e61cba7bc292b4fe0af0c1b19a147c05501ae0ed04c333a86
    /// tsc-span: _tsc.js:47575-47579
    ///
    /// error() + `diagnostic.skippedOn = key` — "noEmit" is the only
    /// key tsc ever passes (collision band 83235-83353 + the
    /// __esModule marker 90103), so the flag is key-less here; the
    /// program layer drops flagged diagnostics when options.noEmit is
    /// set (filterSemanticDiagnostics, checker/src/lib.rs).
    pub fn error_skipped_on_no_emit(
        &mut self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> usize {
        let mut diagnostic = self.create_error(location, message, args);
        diagnostic.skipped_on_no_emit = true;
        self.push_error_diagnostic(diagnostic)
    }

    /// error + addRelatedInfo in one shot: the related info rides the
    /// dedupe comparison (tsc's insertSorted equality compares related
    /// information too — compare_diagnostics includes it).
    /// error_at for a PRE-BUILT diagnostic (chained heads, canonical
    /// heads): same insertSorted exact-duplicate dedupe.
    pub fn push_error_diagnostic(&mut self, diagnostic: Diagnostic) -> usize {
        if let Some(existing) = self
            .diagnostics
            .iter()
            .position(|existing| *existing == diagnostic)
        {
            return existing;
        }
        self.diagnostics.push(diagnostic);
        self.diagnostics.len() - 1
    }

    pub fn error_at_with_related(
        &mut self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[&str],
        related: Vec<tsrs2_diags::RelatedInfo>,
    ) -> usize {
        let mut diagnostic = self.create_error(location, message, args);
        diagnostic.related = related;
        if let Some(existing) = self
            .diagnostics
            .iter()
            .position(|existing| *existing == diagnostic)
        {
            return existing;
        }
        self.diagnostics.push(diagnostic);
        self.diagnostics.len() - 1
    }

    /// tsc-port: lookupOrIssueError @6.0.3
    /// tsc-hash: 9571aad04fba17397e7740b9b0f7b02e8646fb85b89ae01858ad7879ead111d6
    /// tsc-span: _tsc.js:47565-47574
    ///
    /// Returns the index of the (existing or new) diagnostic — tsc's
    /// DiagnosticCollection.lookup compares skip-related-information.
    pub fn lookup_or_issue_error(
        &mut self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> usize {
        let diagnostic = self.create_error(location, message, args);
        let found = self.diagnostics.iter().position(|existing| {
            existing.file_name == diagnostic.file_name
                && existing.start == diagnostic.start
                && existing.length == diagnostic.length
                && existing.code() == diagnostic.code()
                && existing.message_text() == diagnostic.message_text()
        });
        match found {
            Some(index) => index,
            None => {
                self.diagnostics.push(diagnostic);
                self.diagnostics.len() - 1
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use tsrs2_binder::Binder;
    use tsrs2_syntax::{parse_source_file, LanguageVariant, ParseOptions, SourceFile};
    use tsrs2_types::CompilerOptions;

    use super::CheckerState;

    /// Multi-file program construction mirroring check_program's parse/
    /// bind base chaining (M4 5.0).
    pub(crate) fn parse_program(files: &[(&str, &str)]) -> Vec<SourceFile> {
        let mut sources: Vec<SourceFile> = Vec::new();
        for (name, text) in files {
            let (node_id_base, node_array_id_base) = match sources.last() {
                Some(previous) => (previous.arena.node_end(), previous.arena.array_end()),
                None => (0, 0),
            };
            let javascript_file = name.ends_with(".js") || name.ends_with(".jsx");
            let jsx_file = name.ends_with(".tsx") || name.ends_with(".jsx");
            let source = parse_source_file(
                (*name).to_owned(),
                (*text).to_owned(),
                ParseOptions {
                    language_variant: if javascript_file || jsx_file {
                        LanguageVariant::Jsx
                    } else {
                        LanguageVariant::Standard
                    },
                    javascript_file,
                    node_id_base,
                    node_array_id_base,
                },
                None,
            );
            assert!(
                source.parse_diagnostics.is_empty(),
                "test source must parse cleanly: {:?}",
                source.parse_diagnostics
            );
            sources.push(source);
        }
        sources
    }

    pub(crate) fn with_program_state<R>(
        files: &[(&str, &str)],
        options: &CompilerOptions,
        run: impl FnOnce(&mut CheckerState) -> R,
    ) -> R {
        let sources = parse_program(files);
        let mut binders: Vec<Binder<'_>> = Vec::new();
        for source in &sources {
            let (seed, base) = match binders.last() {
                Some(previous) => (previous.next_symbol_id(), previous.symbols.next_id().0),
                None => (1, 0),
            };
            let mut binder = Binder::with_bases(source, options, seed, base);
            binder.bind_source_file();
            binders.push(binder);
        }
        let binder_refs: Vec<&Binder<'_>> = binders.iter().collect();
        let mut state = CheckerState::from_program(binder_refs, options);
        run(&mut state)
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::{CompilerOptions, SymbolFlags, TypeSystemPropertyName};

    use super::test_support::with_program_state;
    use super::ResolutionTarget;
    use crate::links::LinkSlot;

    #[test]
    fn resolution_stack_flags_same_target_same_property_cycles() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let s = state
                .binder
                .create_symbol(SymbolFlags::PROPERTY, "s".to_owned());
            assert!(state
                .push_type_resolution(ResolutionTarget::Symbol(s), TypeSystemPropertyName::TYPE));
            // Same (target, kind) again: a cycle — every entry from the
            // cycle start is flagged false.
            assert!(!state
                .push_type_resolution(ResolutionTarget::Symbol(s), TypeSystemPropertyName::TYPE));
            assert!(!state.pop_type_resolution());
        });
    }

    #[test]
    fn resolution_stack_distinguishes_property_names() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let s = state
                .binder
                .create_symbol(SymbolFlags::PROPERTY, "s".to_owned());
            assert!(state
                .push_type_resolution(ResolutionTarget::Symbol(s), TypeSystemPropertyName::TYPE));
            // One symbol can be mid-resolution for Type while safely
            // resolving DeclaredType (checker-foundations §1.2).
            assert!(state.push_type_resolution(
                ResolutionTarget::Symbol(s),
                TypeSystemPropertyName::DECLARED_TYPE
            ));
            assert!(state.pop_type_resolution());
            assert!(state.pop_type_resolution());
        });
    }

    #[test]
    fn resolution_stack_resolved_intermediate_breaks_cycle_scan() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let s = state
                .binder
                .create_symbol(SymbolFlags::PROPERTY, "s".to_owned());
            let u = state
                .binder
                .create_symbol(SymbolFlags::PROPERTY, "u".to_owned());
            assert!(state
                .push_type_resolution(ResolutionTarget::Symbol(u), TypeSystemPropertyName::TYPE));
            assert!(state
                .push_type_resolution(ResolutionTarget::Symbol(s), TypeSystemPropertyName::TYPE));
            // s's Type resolves while both are on the stack: the scan
            // stops at the first entry whose property is already
            // resolved (resolutionTargetHasProperty), so re-pushing u
            // is NOT a cycle.
            let any = state.tables.intrinsics.any;
            state
                .links
                .set_symbol_type(state.speculation_depth, s, LinkSlot::Resolved(any));
            assert!(state
                .push_type_resolution(ResolutionTarget::Symbol(u), TypeSystemPropertyName::TYPE));
            assert!(state.pop_type_resolution());
            assert!(state.pop_type_resolution());
            assert!(state.pop_type_resolution());
        });
    }
}

#[cfg(test)]
mod resolution_unwind_tests {
    use tsrs2_types::{CompilerOptions, SymbolFlags};

    use super::test_support::with_program_state;

    #[test]
    fn err_unwind_leaves_stack_balanced_and_slot_requeryable() {
        // An annotation the slice cannot type (conditional type, 5.2)
        // unwinds as Unsupported: the resolution stack must be balanced
        // and a SECOND query must fail identically instead of
        // fabricating a cached type (M3-review Resolving-dangling fix).
        with_program_state(
            &[(
                "a.ts",
                "declare var v: number extends string ? number : string;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let symbol = state
                    .resolve_file_scope_name("v", SymbolFlags::VALUE)
                    .expect("v resolves");
                let first = state.get_type_of_symbol(symbol);
                assert!(first.is_err(), "conditional annotations are out of slice");
                assert_eq!(state.resolution_targets.len(), 0);
                let second = state.get_type_of_symbol(symbol);
                assert_eq!(
                    first.unwrap_err().reason,
                    second.expect_err("still out of slice").reason
                );
                assert_eq!(state.resolution_targets.len(), 0);
            },
        );
    }
}
