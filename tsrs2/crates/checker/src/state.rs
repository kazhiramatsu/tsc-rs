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
}

/// tsc IndexInfo (createIndexInfo 59989).
#[derive(Clone, Debug)]
pub struct IndexInfo {
    pub key_type: TypeId,
    pub value_type: TypeId,
    pub is_readonly: bool,
    /// None for synthesized infos (anyBaseTypeIndexInfo 47282).
    pub declaration: Option<NodeId>,
}

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
    /// createAnonymousType + instantiations map (47170) — the fallback
    /// for missing arity>0 globals (getTypeOfGlobalSymbol 60619).
    pub empty_generic_type: TypeId,
    /// createAnonymousType + NonInferrableType (47179) — the vacuous-
    /// exclusion type in isEmptyAnonymousObjectType, live from 5.0.
    pub any_function_type: TypeId,
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

    // ---- M4 5.4: check-driver state ----
    /// Any program file with a top-level `declare global` block
    /// (NodeFlags::GLOBAL_AUGMENTATION module declaration). Computed at
    /// construction; the resolver's failure band treats such programs
    /// as undecidable for missing names until augmentation binding
    /// lands (5.8).
    pub(crate) has_global_augmentation: bool,
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

    // ---- M4 5.5: expression-checking state ----
    /// tsc typeofType (47100): union of the typeofNEFacts key literals
    /// in map-insertion order (stableTypeOrdering is absent from
    /// CompilerOptions, so the unsorted arm is the only one).
    pub(crate) typeof_type: TypeId,
    /// tsc contextualBindingPatterns (47408): pushed only by
    /// getTypeFromBindingPattern under includePatternInType (5.5b) —
    /// empty until then; checkIdentifier's nonInferrableAnyType
    /// circularity arm reads it from 5.5a.
    pub(crate) contextual_binding_patterns: Vec<NodeId>,

    // ---- M4 5.0: the diags sink ----
    /// tsc `diagnostics` (createDiagnosticCollection) — the semantic
    /// sink; the driver (5.4) drains it per program.
    pub diagnostics: DiagnosticList,

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
    /// Lazy getGlobal*Type memos (deferredGlobal* pattern 60679 for the
    /// deferred ones; the core init block 88788+ is deliberately LAZY
    /// here — m4-checker-skeleton-steps.md 5.0 — so each global starts
    /// resolving the moment 5.1's declared types exist).
    pub(crate) global_type_memos: crate::globals::GlobalTypeMemos,

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
            empty_generic_type: TypeId(0),
            any_function_type: TypeId(0),
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
            current_node: None,
            deferred_nodes: std::collections::HashMap::new(),
            typeof_type: TypeId(0),
            contextual_binding_patterns: Vec::new(),
            has_global_augmentation: false,
            diagnostics: Vec::new(),
            globals: SymbolTable::default(),
            undefined_symbol,
            global_this_symbol,
            arguments_symbol,
            require_symbol,
            unknown_symbol,
            pattern_ambient_modules: Vec::new(),
            global_type_memos: Default::default(),
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
        state.report_unreliable_mapper = state.alloc_mapper(
            crate::instantiate::TypeMapper::Function(
                crate::instantiate::FunctionMapper::ReportsUnreliable,
            ),
        );
        state.report_unmeasurable_mapper = state.alloc_mapper(
            crate::instantiate::TypeMapper::Function(
                crate::instantiate::FunctionMapper::ReportsUnmeasurable,
            ),
        );

        // The empty anonymous types from the checker init block
        // (47132/47160/47170/47179): resolved-empty from birth.
        state.empty_object_type = state.create_resolved_empty_anonymous_type(None);
        let empty_type_literal_symbol = state.binder.create_symbol(
            SymbolFlags::TYPE_LITERAL,
            InternalSymbolName::TYPE.to_owned(),
        );
        state.empty_type_literal_type =
            state.create_resolved_empty_anonymous_type(Some(empty_type_literal_symbol));
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
        // tsc-port: createTypeofType @6.0.3
        // tsc-hash: 917b32b2a4664e0000258fed2360fb1d20e0d4d5f6bc9eb52b7369a4d0e21eb4
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
        state.has_global_augmentation = (0..state.binder.file_count()).any(|index| {
            let source = state.binder.source(index);
            let tsrs2_syntax::NodeData::SourceFile(data) = &source.arena.node(source.root).data
            else {
                return false;
            };
            data.statements.is_some_and(|statements| {
                source
                    .arena
                    .node_array(statements)
                    .nodes
                    .iter()
                    .any(|&statement| {
                        tsrs2_binder::node_util::is_global_scope_augmentation(source, statement)
                    })
            })
        });
        state
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

    pub(crate) fn program_has_global_augmentation(&self) -> bool {
        self.has_global_augmentation
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
    fn find_resolution_cycle_start_index(
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

    /// tsc-port: createError @6.0.3
    /// tsc-hash: dedcf6cc6c301274f018ef98543f4abebe1b7826c45f601b914137812caa8cfa
    /// tsc-span: _tsc.js:47580-47582
    ///
    /// No location ⇒ createCompilerDiagnostic: a file-less,
    /// program-level diagnostic.
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
                    *flags = tsrs2_types::RelationComparisonResult::from_bits(
                        flags.bits() | bit.bits(),
                    );
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

    /// error + addRelatedInfo in one shot: the related info rides the
    /// dedupe comparison (tsc's insertSorted equality compares related
    /// information too — compare_diagnostics includes it).
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
            let javascript_file = name.ends_with(".js");
            let source = parse_source_file(
                (*name).to_owned(),
                (*text).to_owned(),
                ParseOptions {
                    language_variant: if javascript_file {
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
