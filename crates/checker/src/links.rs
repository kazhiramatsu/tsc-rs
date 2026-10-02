//! Links tables — the memo policy in one place (greenfield §4.3).
//!
//! Every slot is written ONCE; in-progress states are the explicit
//! `Resolving` value (mirroring tsc's resolving sentinels), never an
//! implicit absence. Speculative checking must not write links: every
//! write asserts the checker-wide speculation depth is zero (the
//! single rule that replaces the quiet/expr_type_cache pollution
//! family). M3 has no speculation yet — the assertion is the contract
//! future stages inherit.
//!
//! Storage: a checker creates millions of link records on a large program,
//! nearly all of which set only a few fields. Each table therefore keeps
//! a small fixed record holding only the fields a large share of records set
//! (or that the relation and instantiation paths read for most identities),
//! and one sparse map per remaining field ([`NodeLinksCold`],
//! [`SymbolLinksCold`], [`TypeLinksCold`]), so a field costs memory only on
//! the records that set it.

use rustc_hash::FxHashMap;
use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

use tsc_binder::SymbolId;
use tsc_syntax::NodeId;
use tsc_types::perf::{self, PerfCounter};
use tsc_types::{ConditionalRootId, EscapedName, JsString, TypeId};

use crate::instantiate::MapperId;
use crate::state::{MembersId, SignatureId};

/// One memo slot: Vacant → Resolving → Resolved, one transition each.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum LinkSlot<T> {
    #[default]
    Vacant,
    Resolving,
    Resolved(T),
}

impl<T: Clone> LinkSlot<T> {
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn resolved(&self) -> Option<T> {
        match self {
            LinkSlot::Resolved(value) => Some(value.clone()),
            _ => None,
        }
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn is_resolving(&self) -> bool {
        matches!(self, LinkSlot::Resolving)
    }
}

/// A compiler identity an [`IdSlot`] can hold: every `id_type!` identity,
/// whose index is below `u32::MAX`.
pub trait SlotId: Copy {
    fn slot_index(self) -> u32;
    fn from_slot_index(index: u32) -> Self;
}

macro_rules! slot_ids {
    ($($id:ty),* $(,)?) => {$(
        impl SlotId for $id {
            #[inline]
            fn slot_index(self) -> u32 {
                self.index()
            }

            #[inline]
            fn from_slot_index(index: u32) -> Self {
                Self::new(index)
            }
        }
    )*};
}

slot_ids!(TypeId, SymbolId, SignatureId, MembersId);

/// A [`LinkSlot`] of a compiler identity in four bytes.
///
/// The enum needs a tag beside the identity and takes eight bytes; the
/// fixed link records hold millions of slots, so they store this packed
/// form and convert at the accessors. `raw` is 0 for Vacant, `u32::MAX` for
/// Resolving, and the identity's index plus one for Resolved.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct IdSlot<T> {
    raw: u32,
    id: std::marker::PhantomData<T>,
}

impl<T> Default for IdSlot<T> {
    fn default() -> Self {
        Self {
            raw: 0,
            id: std::marker::PhantomData,
        }
    }
}

impl<T: SlotId> IdSlot<T> {
    const RESOLVING: u32 = u32::MAX;

    /// The slot as a [`LinkSlot`].
    #[inline]
    pub fn get(self) -> LinkSlot<T> {
        match self.raw {
            0 => LinkSlot::Vacant,
            Self::RESOLVING => LinkSlot::Resolving,
            raw => LinkSlot::Resolved(T::from_slot_index(raw - 1)),
        }
    }

    /// As [`LinkSlot::resolved`].
    #[inline]
    pub fn resolved(self) -> Option<T> {
        match self.raw {
            0 | Self::RESOLVING => None,
            raw => Some(T::from_slot_index(raw - 1)),
        }
    }

    /// As [`LinkSlot::is_resolving`].
    #[inline]
    pub fn is_resolving(self) -> bool {
        self.raw == Self::RESOLVING
    }

    /// Store `slot`, returning the previous value.
    #[inline]
    pub fn replace(&mut self, slot: LinkSlot<T>) -> LinkSlot<T> {
        std::mem::replace(self, slot.into()).get()
    }
}

impl<T: SlotId> From<LinkSlot<T>> for IdSlot<T> {
    #[inline]
    fn from(slot: LinkSlot<T>) -> Self {
        let raw = match slot {
            LinkSlot::Vacant => 0,
            LinkSlot::Resolving => Self::RESOLVING,
            LinkSlot::Resolved(id) => {
                let index = id.slot_index();
                assert!(
                    index < Self::RESOLVING - 1,
                    "identity index {index} does not fit a link slot"
                );
                index + 1
            }
        };
        Self {
            raw,
            id: std::marker::PhantomData,
        }
    }
}

impl<T: SlotId + std::fmt::Debug> std::fmt::Debug for IdSlot<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(formatter)
    }
}

/// A links field few records set: its values by identity, where an absent
/// identity reads as the field's default (tsc's unset property).
///
/// A field of a fixed record costs its size on every record of the table; a
/// sparse field costs one hash entry on the records that set it, which is
/// less once fewer than about a third of the records do.
#[derive(Debug)]
pub struct SparseLinks<K, V> {
    values: FxHashMap<K, V>,
    /// The value every absent identity reads.
    absent: V,
}

impl<K, V: Default> Default for SparseLinks<K, V> {
    fn default() -> Self {
        Self {
            values: FxHashMap::default(),
            absent: V::default(),
        }
    }
}

impl<K: Copy + Eq + std::hash::Hash, V: Default> SparseLinks<K, V> {
    /// The value for `key`; the default when it was never written.
    #[inline]
    pub fn get(&self, key: K) -> &V {
        self.values.get(&key).unwrap_or(&self.absent)
    }

    /// The value for `key` for writing, created as the default on first use.
    #[inline]
    fn slot(&mut self, key: K) -> &mut V {
        self.values.entry(key).or_default()
    }

    /// Return `key` to the default (an unwound or retracted write).
    fn clear(&mut self, key: K) {
        self.values.remove(&key);
    }

    /// Store `value` for `key`, returning the previous value (the default
    /// when there was none): one lookup for a write-once check.
    fn replace(&mut self, key: K, value: V) -> V {
        self.values.insert(key, value).unwrap_or_default()
    }
}

impl<K: Copy + Eq + std::hash::Hash, V: Default + PartialEq> SparseLinks<K, V> {
    /// Store `value` for `key`; storing the default removes the entry, so
    /// writing an unset value (`None`, `false`) to a fresh record costs
    /// nothing.
    fn set(&mut self, key: K, value: V) {
        if value == self.absent {
            self.values.remove(&key);
        } else {
            self.values.insert(key, value);
        }
    }
}

impl<K, V> SparseLinks<K, V> {
    /// Entries and table bytes (memory accounting; heap owned by the values
    /// is not included).
    fn memory_usage(&self) -> (usize, usize) {
        (
            self.values.len(),
            self.values.capacity() * (std::mem::size_of::<(K, V)>() + 1) * 8 / 7,
        )
    }
}

/// Sum the memory accounting of a cold struct's sparse fields.
fn sparse_memory_usage<const N: usize>(fields: [(usize, usize); N]) -> (usize, usize) {
    fields
        .into_iter()
        .fold((0, 0), |(entries, bytes), (field_entries, field_bytes)| {
            (entries + field_entries, bytes + field_bytes)
        })
}

type SpeculativeResolvedTypeWrite = (
    u32,
    NodeId,
    LinkSlot<TypeId>,
    LinkSlot<SymbolId>,
    LinkSlot<TypeId>,
);

// Debug-only census of OPEN `Resolving` sentinels on this thread.
// Every slot writer below reports its transition; the
// abort-unwind invariant reads the census at element/file
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

/// tsrs-native: debug census accessor for the abort-unwind
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

/// tsc NodeLinks (getNodeLinks) — the fields most linked nodes set.
///
/// A checker links most of the nodes it resolves (millions on a large
/// program) and nearly every such node sets one or two of these fields: an
/// identifier its symbol, a type node its type, a call or declaration its
/// signature. The other NodeLinks fields live in [`NodeLinksCold`].
#[derive(Clone, Debug, Default)]
pub struct NodeLinks {
    /// tsc links.resolvedType (per-arm caching in the
    /// getTypeFromTypeNode workers).
    pub resolved_type: IdSlot<TypeId>,
    /// tsc links.resolvedSignature (getSignatureFromDeclaration 59570).
    pub resolved_signature: IdSlot<SignatureId>,
    /// tsc links.resolvedSymbol (getResolvedSymbol 69389) — the
    /// unknownSymbol failure sentinel is cached like tsc's.
    pub resolved_symbol: IdSlot<SymbolId>,
    /// tsc NodeLinks.flags (getNodeCheckFlags) — the driver's
    /// TypeChecked bit lands with M4 5.4; later stages OR in their own
    /// bits (a flags word accumulates, unlike the write-once slots).
    pub check_flags: tsc_types::NodeCheckFlags,
}

const _: () = assert!(std::mem::size_of::<NodeLinks>() == 16);

/// An array literal's (first, last) spread element indices.
pub type SpreadIndices = (Option<u32>, Option<u32>);

/// The NodeLinks fields few nodes set, one sparse map per field (see
/// [`SparseLinks`]).
#[derive(Debug, Default)]
pub struct NodeLinksCold {
    /// tsc links.resolvedJSDocType
    /// (getTypeFromJSDocValueReference 60406): the value-derived
    /// JSDoc type is cached separately from the enclosing type
    /// reference result.
    pub resolved_jsdoc_type: SparseLinks<NodeId, LinkSlot<TypeId>>,
    /// tsc NodeLinks.flags & EnumValuesComputed (85582) on
    /// EnumDeclaration nodes. Unlike tsc this REVERTS on CheckAbort
    /// unwind so a later query recomputes the tail of the member list.
    pub enum_values_computed: SparseLinks<NodeId, bool>,
    /// tsc NodeLinks.calculatedFlags (calculateNodeCheckFlagWorker 88132):
    /// which lazily computed NodeCheckFlags groups have already been
    /// derived for an unchecked (noCheck / excluded) source. Emit-only.
    pub calculated_flags: SparseLinks<NodeId, tsc_types::NodeCheckFlags>,
    /// tsc NodeLinks.isVisible (isDeclarationVisible 55591 and the
    /// declaration-emit alias painters). This is the deliberate MONOTONE
    /// exception to the table's write-once policy: absent→false,
    /// absent→true, and false→true are valid; true is absorbing. Emit and
    /// check-phase writers are forbidden in speculative contexts.
    pub(crate) is_visible: SparseLinks<NodeId, Option<bool>>,
    /// tsc links.containsArgumentsReference
    /// (containsArgumentsReference 59689): syntax-and-binding-stable
    /// result of the function-body traversal.
    pub contains_arguments_reference: SparseLinks<NodeId, Option<bool>>,
    /// tsc links.hasReportedStatementInAmbientContext
    /// (checkGrammarStatementInAmbientContext 90341): the once-flag on
    /// the offending statement OR its enclosing block. Stays false when
    /// grammarErrorOnFirstToken is parse-diagnostics-suppressed, like
    /// tsc's `links.x = grammarError(...)` assignment.
    pub has_reported_statement_in_ambient_context: SparseLinks<NodeId, bool>,
    /// tsc links.contextFreeType (getContextFreeTypeOfExpression 80948).
    pub context_free_type: SparseLinks<NodeId, LinkSlot<TypeId>>,
    /// tsc links.parameterInitializerContainsUndefined
    /// (parameterInitializerContainsUndefined 71602) on Parameter
    /// nodes — the removeOptionalityFromDeclaredType input.
    pub parameter_initializer_contains_undefined: SparseLinks<NodeId, Option<bool>>,
    /// tsc links.jsxFlags (getIntrinsicTagSymbol 74540/74545) on JSX
    /// opening-like/closing elements — an accumulating flags word.
    pub jsx_flags: SparseLinks<NodeId, tsc_types::JsxFlags>,
    /// tsc links.outerTypeParameters (getObjectTypeInstantiation 63466)
    /// on the instantiated type's declaration node.
    pub outer_type_parameters: SparseLinks<NodeId, LinkSlot<Box<[TypeId]>>>,
    /// tsc links.enumMemberValue (computeEnumMemberValues 85587) on
    /// EnumMember nodes.
    pub enum_member_value: SparseLinks<NodeId, Option<crate::evaluate::EvaluatorResult>>,
    /// tsc links.capturedBlockScopeBindings
    /// (checkNestedBlockScopedBinding 72267-72268): the block-scoped
    /// symbols a for-statement part captures, recorded beside the
    /// ContainsCapturedBlockScopeBinding flag and consumed only by the
    /// emit resolver's isBindingCapturedByNode.
    pub captured_block_scope_bindings: SparseLinks<NodeId, Vec<SymbolId>>,
    /// tsc links.assertionExpressionType (checkAssertionWorker 77920):
    /// the operand type stashed for checkAssertionDeferred.
    pub assertion_expression_type: SparseLinks<NodeId, Option<TypeId>>,
    /// tsc links.instantiationExpressionTypes (getInstantiationExpressionType
    /// 77980): exprType.id → instantiated result, STORE-BEFORE-ERROR.
    pub instantiation_expression_types:
        SparseLinks<NodeId, Option<rustc_hash::FxHashMap<TypeId, TypeId>>>,
    /// tsc links.spreadIndices (getContextualType's ArrayLiteral arm
    /// 73520): (first, last) spread element indices, computed once per
    /// array literal (getSpreadIndices 73248).
    pub spread_indices: SparseLinks<NodeId, Option<SpreadIndices>>,
    /// tsc links.nonExistentPropCheckCache (reportNonexistentProperty
    /// 75417): `{typeId}|{isUncheckedJS}` dedupe keys. Trial-local
    /// insertions are visible to re-entry and restored at the boundary.
    pub non_existent_prop_check_cache: SparseLinks<NodeId, rustc_hash::FxHashSet<String>>,
    /// tsc links.resolvedJsxElementAttributesType
    /// (getIntrinsicAttributesTypeFromJsxOpeningLikeElement 74731) —
    /// compute-once; written only on success so a CheckAbort unwind
    /// re-computes.
    pub resolved_jsx_element_attributes_type: SparseLinks<NodeId, Option<TypeId>>,
    /// tsc sourceFileLinks.jsxFragmentType (getJSXFragmentType 77373)
    /// on SourceFile nodes — the per-file fragment factory type memo
    /// (errorType is a real cached verdict, matching tsc).
    pub jsx_fragment_type: SparseLinks<NodeId, Option<TypeId>>,
    /// tsc links.decoratorSignature (getESDecoratorCallSignature 78574 /
    /// getLegacyDecoratorCallSignature 78616) on the DECORATED node —
    /// Some(anySignature) is tsc's "no signature" sentinel.
    pub decorator_signature: SparseLinks<NodeId, Option<SignatureId>>,
}

impl NodeLinksCold {
    fn memory_usage(&self) -> (usize, usize) {
        let Self {
            resolved_jsdoc_type,
            enum_values_computed,
            calculated_flags,
            is_visible,
            contains_arguments_reference,
            has_reported_statement_in_ambient_context,
            context_free_type,
            parameter_initializer_contains_undefined,
            jsx_flags,
            outer_type_parameters,
            enum_member_value,
            captured_block_scope_bindings,
            assertion_expression_type,
            instantiation_expression_types,
            spread_indices,
            non_existent_prop_check_cache,
            resolved_jsx_element_attributes_type,
            jsx_fragment_type,
            decorator_signature,
        } = self;
        sparse_memory_usage([
            resolved_jsdoc_type.memory_usage(),
            enum_values_computed.memory_usage(),
            calculated_flags.memory_usage(),
            is_visible.memory_usage(),
            contains_arguments_reference.memory_usage(),
            has_reported_statement_in_ambient_context.memory_usage(),
            context_free_type.memory_usage(),
            parameter_initializer_contains_undefined.memory_usage(),
            jsx_flags.memory_usage(),
            outer_type_parameters.memory_usage(),
            enum_member_value.memory_usage(),
            captured_block_scope_bindings.memory_usage(),
            assertion_expression_type.memory_usage(),
            instantiation_expression_types.memory_usage(),
            spread_indices.memory_usage(),
            non_existent_prop_check_cache.memory_usage(),
            resolved_jsx_element_attributes_type.memory_usage(),
            jsx_fragment_type.memory_usage(),
            decorator_signature.memory_usage(),
        ])
    }
}

/// tsc SymbolLinks — the fields a large share of symbol records set.
///
/// Declared symbols mostly carry their type and reference meaning;
/// instantiated symbols, the bulk of the records on a large program, carry
/// their check flags, target and mapper. The other SymbolLinks fields, except
/// the name type most symbol reads consult, live in [`SymbolLinksCold`].
#[derive(Clone, Debug, Default)]
pub struct SymbolLinks {
    /// tsc links.declaredType (getDeclaredTypeOfClassOrInterface 57381).
    pub declared_type: IdSlot<TypeId>,
    /// tsc links.type (getTypeOfVariableOrParameterOrProperty 56633).
    pub type_of_symbol: IdSlot<TypeId>,
    /// tsc TransientSymbol links.checkFlags (synthetic union/
    /// intersection properties, createUnionOrIntersectionProperty).
    pub check_flags: tsc_types::CheckFlags,
    /// tsc symbol.isReferenced — a SymbolFlags meaning mask, not a
    /// boolean. resolveName ORs the requested meaning into this field;
    /// the JSX/private-property direct markers write SymbolFlags::All.
    /// Unused type parameters and value declarations consume different
    /// bits of the same merged symbol.
    pub is_referenced: tsc_types::SymbolFlags,
    /// tsc links.target for CheckFlags::INSTANTIATED symbols
    /// (instantiateSymbol 63455).
    pub target: Option<SymbolId>,
    /// tsc links.mapper for CheckFlags::INSTANTIATED symbols (63456).
    pub mapper: Option<MapperId>,
    /// tsc links.nameType — written by late-bound member binding (5.3);
    /// carried through instantiateSymbol's copy (63460). Few symbols set
    /// it, but instantiation and property access read it for most.
    pub name_type: Option<TypeId>,
}

const _: () = assert!(std::mem::size_of::<SymbolLinks>() == 28);

/// The SymbolLinks fields few symbols set, one sparse map per field (see
/// [`SparseLinks`]).
#[derive(Debug, Default)]
pub struct SymbolLinksCold {
    /// tsc links.containingType for synthetic properties.
    pub containing_type: SparseLinks<SymbolId, Option<TypeId>>,
    /// tsc links.mappedType for CheckFlags::MAPPED property symbols
    /// synthesized by resolveMappedTypeMembers (58549), and for
    /// CheckFlags::REVERSE_MAPPED properties (58446/58449).
    pub mapped_type: SparseLinks<SymbolId, Option<TypeId>>,
    /// tsc links.keyType for CheckFlags::MAPPED property symbols
    /// (58551); distinct from nameType after key remapping.
    pub key_type: SparseLinks<SymbolId, Option<TypeId>>,
    /// tsc links.syntheticOrigin (getSpreadSymbol 63052 /
    /// getAnonymousPartialType 62955). Dormant store like the spread pair
    /// below.
    pub synthetic_origin: SparseLinks<SymbolId, Option<SymbolId>>,
    /// tsc links.inferredClassSymbol (mergeJSSymbols 77526-77538): source
    /// symbol-local map from the inferred target symbol id to that
    /// transient merged symbol. The key is the inferred symbol
    /// itself (not necessarily the incoming target), matching tsc's
    /// clone-then-publish protocol. Keyed by compiler-assigned ids and only
    /// ever probed (never iterated), so the lookup-only hasher applies; the
    /// std `RandomState` default would touch thread-local keys every time a
    /// vacant symbol entry is initialized.
    pub inferred_class_symbols: SparseLinks<SymbolId, FxHashMap<SymbolId, SymbolId>>,
    /// tsc links.specifierCache (getSpecifierForModuleSymbol 53088-53107):
    /// mode-aware cache key -> computed module specifier, populated by the
    /// dormant h2-7a-m-3 specifier synthesis and never read by the display
    /// path.
    pub specifier_cache:
        SparseLinks<SymbolId, Option<std::collections::BTreeMap<JsString, JsString>>>,
    /// tsc links.extendedContainersByFile (getAlternativeContainingModules
    /// 49954-49973): per enclosing Program file, the re-exporting module
    /// chains (module, alias) found through that file's imports.
    pub extended_containers_by_file: SparseLinks<SymbolId, FxHashMap<usize, Vec<Vec<SymbolId>>>>,
    /// tsc links.extendedContainers (49976-49988): the program-wide
    /// fallback over every external module, computed once per symbol.
    pub extended_containers: SparseLinks<SymbolId, Option<Vec<Vec<SymbolId>>>>,
    /// tsc SymbolLinks.referenced (markAliasSymbolAsReferenced 71930)
    /// — alias accessibility/emit bookkeeping. This is deliberately
    /// distinct from Symbol.isReferenced ([`SymbolLinks::is_referenced`]):
    /// unused locals consume the latter, while alias visibility consumers
    /// read this bit.
    pub alias_referenced: SparseLinks<SymbolId, bool>,
    /// tsc links.typeParameters for generic type-alias symbols
    /// (getDeclaredTypeOfTypeAlias 57416).
    pub type_parameters: SparseLinks<SymbolId, Option<Vec<TypeId>>>,
    /// tsc links.resolvedMembers (getResolvedMembersOrExportsOfSymbol
    /// 57712) — the early⊕late member table of a late-binding
    /// container; equal to `symbol.members` while no late-bindable
    /// member exists (the pre-5.5 slice).
    pub resolved_members: SparseLinks<SymbolId, LinkSlot<Arc<tsc_binder::SymbolTable>>>,
    /// tsc links.lateSymbol (addDeclarationToLateBoundSymbol 57652) —
    /// the late-bound symbol a member's own binder symbol resolved to.
    pub late_symbol: SparseLinks<SymbolId, Option<SymbolId>>,
    /// tsc links.writeType (getWriteTypeOfAccessors 56787) — the
    /// setter-side type; the WriteType resolution property.
    pub write_type: SparseLinks<SymbolId, LinkSlot<TypeId>>,
    /// tsc links.resolvedExports (getResolvedMembersOrExportsOfSymbol
    /// 57712, the static resolutionKind) — equal to `symbol.exports`
    /// while no late-bindable static member exists.
    pub resolved_exports: SparseLinks<SymbolId, LinkSlot<Arc<tsc_binder::SymbolTable>>>,
    /// tsc symbol.lastAssignmentPos (markNodeAssignments 71523): the
    /// last assignment's extended position in document order; NEGATIVE
    /// = a definite-assignment (`x!`-style or sticky), |i64::MAX| =
    /// "assigned in another function" (unknowable). Position 0 is
    /// treated as unmarked by isPastLastAssignment — tsc's JS
    /// falsiness, kept faithfully there.
    pub last_assignment_pos: SparseLinks<SymbolId, Option<i64>>,
    /// tsc links.aliasTarget (resolveAlias 49118): Resolving = the
    /// resolvingSymbol sentinel — NOT write-once (the re-entrant
    /// Circular_definition_of_import_alias_0 write and the
    /// sentinel-on-entry unknownSymbol collapse both rewrite it; M4
    /// 5.8d, the resolvedSignature protocol twin).
    pub alias_target: SparseLinks<SymbolId, LinkSlot<SymbolId>>,
    /// tsc links.typeOnlyDeclaration (markSymbolOfAliasDeclarationIf
    /// TypeOnly 49182): TRI-STATE — None = unset, Some(None) = the
    /// explicit `false` (computed, not type-only), Some(Some(node)) =
    /// the type-only declaration.
    pub type_only_declaration: SparseLinks<SymbolId, Option<Option<NodeId>>>,
    /// tsc links.deferralParent / deferralConstituents /
    /// deferralWriteConstituents. Union/intersection properties with more
    /// than two constituent properties retain their recipe and combine it
    /// only when the read or write type is first observed.
    pub deferral_parent: SparseLinks<SymbolId, Option<TypeId>>,
    pub deferral_constituents: SparseLinks<SymbolId, Option<Vec<TypeId>>>,
    pub deferral_write_constituents: SparseLinks<SymbolId, Option<Vec<TypeId>>>,
    /// tsc links.isDiscriminantProperty cache (isDiscriminantProperty
    /// 69562).
    pub is_discriminant_property: SparseLinks<SymbolId, Option<bool>>,
    /// tsc links.isDeclarationWithCollidingName cache
    /// (isSymbolOfDeclarationWithCollidingName 87924).
    pub is_declaration_with_colliding_name: SparseLinks<SymbolId, Option<bool>>,
    /// tsc links.isConstructorDeclaredProperty
    /// (isConstructorDeclaredProperty 56145): the syntax/annotation-stable
    /// classification of JS assignment-declared instance properties.
    pub is_constructor_declared_property: SparseLinks<SymbolId, Option<bool>>,
    /// tsc links.propertyType / constraintType for fresh
    /// CheckFlags::REVERSE_MAPPED properties (58442, 58447/58450).
    pub property_type: SparseLinks<SymbolId, Option<TypeId>>,
    pub constraint_type: SparseLinks<SymbolId, Option<TypeId>>,
    /// tsc links.tupleLabelDeclaration (createTupleTargetType 61170):
    /// the NamedTupleMember/Parameter node behind a synthesized tuple
    /// index property.
    pub tuple_label_declaration: SparseLinks<SymbolId, Option<NodeId>>,
    /// tsc links.uniqueESSymbolType (getESSymbolLikeTypeForNode 63127)
    /// — the per-declaration `unique symbol` type memo.
    pub unique_es_symbol_type: SparseLinks<SymbolId, Option<TypeId>>,
    /// tsc links.variances (getVariancesWorker 67315): Vacant =
    /// undefined, Resolving = the in-progress emptyArray sentinel
    /// (getVariances call sites answer Ternary.Unknown), Resolved =
    /// the measured list — possibly genuinely empty for zero-parameter
    /// alias symbols, which is DISTINCT from the sentinel exactly as
    /// tsc's fresh `[]` differs from the shared emptyArray.
    pub variances: SparseLinks<SymbolId, LinkSlot<Box<[tsc_types::VarianceFlags]>>>,
    /// tsc links.originatingImport (cloneTypeAsModuleType 49769) — the
    /// import-site provenance an interop module clone carries; read by
    /// the invocation-error related-info band (64900/77252): dormant
    /// stores at M4 (related info is not a T0 observable).
    pub originating_import: SparseLinks<SymbolId, Option<NodeId>>,
    /// tsc links.leftSpread/rightSpread (getSpreadType 63024-63025):
    /// the merged-optional-property provenance pair. Dormant stores
    /// at M4 (read by getSyntheticElementAccess-side tooling later);
    /// kept for symbol-shape fidelity, like `synthetic_origin`.
    pub left_spread: SparseLinks<SymbolId, Option<SymbolId>>,
    pub right_spread: SparseLinks<SymbolId, Option<SymbolId>>,
    /// tsc links.typeParametersChecked (checkTypeParameterListsIdentical
    /// 84876) — the once-latch on multi-declaration class/interface
    /// symbols.
    pub type_parameters_checked: SparseLinks<SymbolId, bool>,
    /// tsc links.typeOnlyExportStarName (49189): the export-star name
    /// when it differs from the source symbol's own name.
    pub type_only_export_star_name: SparseLinks<SymbolId, Option<EscapedName>>,
    /// tsc links.typeOnlyExportStarMap (getExportsOfModule 49841):
    /// written WITH the module-flavor resolved_exports; names whose
    /// only path in is a type-only `export type *` declaration.
    pub type_only_export_star_map:
        SparseLinks<SymbolId, Option<rustc_hash::FxHashMap<EscapedName, NodeId>>>,
    /// tsc links.exportsChecked (checkExternalModuleExports 86445) —
    /// the per-module once-guard.
    pub exports_checked: SparseLinks<SymbolId, bool>,
    /// tsc links.cjsExportMerged (getCommonJsExportEquals 49697).
    pub cjs_export_merged: SparseLinks<SymbolId, Option<SymbolId>>,
    /// tsc links.immediateTarget (getImmediateAliasedSymbol 50092) —
    /// compute-once; the inner Option is the target (None = tsc
    /// undefined result, still computed).
    pub immediate_target: SparseLinks<SymbolId, Option<Option<SymbolId>>>,
}

impl SymbolLinksCold {
    fn memory_usage(&self) -> (usize, usize) {
        let Self {
            containing_type,
            mapped_type,
            key_type,
            synthetic_origin,
            inferred_class_symbols,
            specifier_cache,
            extended_containers_by_file,
            extended_containers,
            alias_referenced,
            type_parameters,
            resolved_members,
            late_symbol,
            write_type,
            resolved_exports,
            last_assignment_pos,
            alias_target,
            type_only_declaration,
            deferral_parent,
            deferral_constituents,
            deferral_write_constituents,
            is_discriminant_property,
            is_declaration_with_colliding_name,
            is_constructor_declared_property,
            property_type,
            constraint_type,
            tuple_label_declaration,
            unique_es_symbol_type,
            variances,
            originating_import,
            left_spread,
            right_spread,
            type_parameters_checked,
            type_only_export_star_name,
            type_only_export_star_map,
            exports_checked,
            cjs_export_merged,
            immediate_target,
        } = self;
        sparse_memory_usage([
            containing_type.memory_usage(),
            mapped_type.memory_usage(),
            key_type.memory_usage(),
            synthetic_origin.memory_usage(),
            inferred_class_symbols.memory_usage(),
            specifier_cache.memory_usage(),
            extended_containers_by_file.memory_usage(),
            extended_containers.memory_usage(),
            alias_referenced.memory_usage(),
            type_parameters.memory_usage(),
            resolved_members.memory_usage(),
            late_symbol.memory_usage(),
            write_type.memory_usage(),
            resolved_exports.memory_usage(),
            last_assignment_pos.memory_usage(),
            alias_target.memory_usage(),
            type_only_declaration.memory_usage(),
            deferral_parent.memory_usage(),
            deferral_constituents.memory_usage(),
            deferral_write_constituents.memory_usage(),
            is_discriminant_property.memory_usage(),
            is_declaration_with_colliding_name.memory_usage(),
            is_constructor_declared_property.memory_usage(),
            property_type.memory_usage(),
            constraint_type.memory_usage(),
            tuple_label_declaration.memory_usage(),
            unique_es_symbol_type.memory_usage(),
            variances.memory_usage(),
            originating_import.memory_usage(),
            left_spread.memory_usage(),
            right_spread.memory_usage(),
            type_parameters_checked.memory_usage(),
            type_only_export_star_name.memory_usage(),
            type_only_export_star_map.memory_usage(),
            exports_checked.memory_usage(),
            cjs_export_merged.memory_usage(),
            immediate_target.memory_usage(),
        ])
    }
}

/// Resolved-members store — tsc keeps these directly on the type
/// object (setStructuredTypeMembers); a side table keeps Type immutable
/// after interning.
///
/// The fields below are the ones a large share of type records set
/// (resolved members, instantiation target and mapper), the
/// type-parameter constraint caches the relation paths read for most
/// generic types, and the deferred-reference node they test on every
/// reference; the other TypeLinks fields live in [`TypeLinksCold`].
#[derive(Clone, Debug, Default)]
pub struct TypeLinks {
    pub resolved_members: IdSlot<MembersId>,
    /// tsc TypeParameter.constraint (getConstraintFromTypeParameter
    /// 60103) — Resolved(noConstraintType sentinel) = computed, none.
    pub type_parameter_constraint: IdSlot<TypeId>,
    /// tsc type.resolvedBaseConstraint (getResolvedBaseConstraint
    /// 58916-58920).
    pub resolved_base_constraint: IdSlot<TypeId>,
    /// tsc type.immediateBaseConstraint (getImmediateBaseConstraint
    /// 58921-58951; the ImmediateBaseConstraint resolution property).
    pub immediate_base_constraint: IdSlot<TypeId>,
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
    pub type_parameter_default: IdSlot<TypeId>,
    /// tsc TypeReference.node for DEFERRED references
    /// (createDeferredTypeReference 60196): the TypeReference/ArrayType/
    /// TupleType node the lazy getTypeArguments reads. `Some` IS the
    /// deferred-ness test (isNonDeferredTypeReference 67388 checks
    /// !type.node) — it stays `Some` after the arguments resolve. Few
    /// references are deferred, but the relation paths test every one.
    pub deferred_node: Option<NodeId>,
}

const _: () = assert!(std::mem::size_of::<TypeLinks>() == 40);

/// The TypeLinks fields few types set, one sparse map per field (see
/// [`SparseLinks`]).
#[derive(Debug, Default)]
pub struct TypeLinksCold {
    /// tsc MappedType.typeParameter (getTypeParameterFromMappedType
    /// 58601): declaration-derived and computed once.
    pub mapped_type_parameter: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc MappedType.constraintType (58604).
    pub mapped_constraint_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc MappedType.nameType (58607). The inner None records a mapped
    /// declaration without an `as` clause.
    pub mapped_name_type: SparseLinks<TypeId, LinkSlot<Option<TypeId>>>,
    /// tsc MappedType.templateType (58610).
    pub mapped_template_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc MappedType.modifiersType (58625), consumed when mapped
    /// members/instantiation land in 9.5b.
    pub mapped_modifiers_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc MappedType.containsError, set by a mapped-property type
    /// resolution cycle (58581). This is monotone diagnostic state.
    pub mapped_contains_error: SparseLinks<TypeId, bool>,
    /// tsc MappedType.resolvedApparentType
    /// (getApparentTypeOfMappedType 59071).
    pub mapped_apparent_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc ConditionalType resolved arm/constraint caches. They are
    /// checker-owned because conditional types are immutable in the
    /// types arena. `Resolved(None)` is the stored false sentinel for
    /// `resolvedConstraintOfDistributive`.
    pub conditional_true_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    pub conditional_false_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    pub conditional_inferred_true_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    pub conditional_default_constraint: SparseLinks<TypeId, LinkSlot<TypeId>>,
    pub conditional_constraint_of_distributive: SparseLinks<TypeId, LinkSlot<Option<TypeId>>>,
    /// tsc synthType.syntheticType (getTypeWithSyntheticDefaultImportType
    /// 77789-77821) — the esModuleInterop default-wrap memo stamped on
    /// the module type itself.
    pub synthetic_type: SparseLinks<TypeId, Option<TypeId>>,
    /// tsc synthType.defaultOnlyType
    /// (getTypeWithSyntheticDefaultOnly 77779-77787): the JSON ESM
    /// default-only wrapper memo.
    pub default_only_type: SparseLinks<TypeId, Option<TypeId>>,
    /// tsc type.resolvedIndexType / resolvedStringIndexType
    /// (getIndexTypeForGenericType 61932).
    pub resolved_index_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    pub resolved_string_index_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc type.simplifiedForReading / simplifiedForWriting
    /// (getSimplifiedIndexedAccessType 62471-62475). Resolving IS
    /// tsc's circularConstraintType in-flight sentinel (re-entry
    /// returns the type itself); a CheckAbort unwind reverts to
    /// Vacant per the unwind invariant.
    pub simplified_for_reading: SparseLinks<TypeId, LinkSlot<TypeId>>,
    pub simplified_for_writing: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc type.uniqueLiteralFilledInstantiation (isReducibleIntersection
    /// 59322).
    pub unique_literal_filled_instantiation: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc type.permissiveInstantiation (getPermissiveInstantiation
    /// 63815).
    pub permissive_instantiation: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc type.restrictiveInstantiation (getRestrictiveInstantiation
    /// 63818; the result self-stamp makes the second write idempotent).
    pub restrictive_instantiation: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc TypeReference.cachedEquivalentBaseType
    /// (getSingleBaseForNonAugmentingSubtype 67713), guarded by the
    /// IdenticalBaseTypeCalculated/Exists object flags.
    pub cached_equivalent_base_type: SparseLinks<TypeId, Option<TypeId>>,
    /// tsc Type.pattern (getTypeFromObjectBindingPattern 56522 /
    /// getTypeFromArrayBindingPattern 56541): the destructuring pattern
    /// the type was inferred FROM, under includePatternInType only —
    /// read by getContextualTypeForBinaryOperand's `type.pattern` test
    /// (72946) and the literals band. Checker-side because the types
    /// crate is NodeId-free (like tuple_label_declaration).
    pub pattern: SparseLinks<TypeId, Option<NodeId>>,
    /// tsc TypeReference.literalType (createArrayLiteralType 74039):
    /// the once-per-reference ArrayLiteral-flagged clone.
    pub literal_type: SparseLinks<TypeId, Option<TypeId>>,
    /// tsc unionType.arrayFallbackSignatures (getSignaturesOfType
    /// 59397-59413): the synthesized call signatures for a union of
    /// matching Array/ReadonlyArray members.  A resolved empty slice is
    /// the negative-cache sentinel.
    pub array_fallback_signatures: SparseLinks<TypeId, LinkSlot<Box<[SignatureId]>>>,
    /// tsc unionOrIntersection type.resolvedProperties
    /// (getPropertiesOfUnionOrIntersectionType 58721).
    pub resolved_properties: SparseLinks<TypeId, LinkSlot<Box<[SymbolId]>>>,
    /// tsc unionType.keyPropertyName/constituentMap (getKeyPropertyName
    /// 69612): None name = the "" no-key-property sentinel.
    pub union_key_property: SparseLinks<TypeId, LinkSlot<UnionKeyProperty>>,
    /// tsc unionType.resolvedReducedType (getReducedType 59289 +
    /// getReducedUnionType's self-stamp 59305).
    pub resolved_reduced_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc TypeReference.mapper (60197): applied to the node-read
    /// arguments in getTypeArguments (60211); set by
    /// getObjectTypeInstantiation's deferred-reference result arm.
    pub deferred_mapper: SparseLinks<TypeId, Option<MapperId>>,
    /// tsc InterfaceTypeWithDeclaredMembers.declaredProperties/
    /// declaredCallSignatures/declaredConstructSignatures/
    /// declaredIndexInfos (resolveDeclaredMembers 57602) — one
    /// ResolvedMembers holding the OWN members, distinct from
    /// resolved_members (which merges heritage).
    pub declared_members: SparseLinks<TypeId, LinkSlot<MembersId>>,
    /// tsc InterfaceType.resolvedBaseTypes (getBaseTypes 57218).
    /// MUTABLE like tsc's field: interfaces initialize to [] and push
    /// per base; a mid-cycle reader observes the partial list.
    pub resolved_base_types: SparseLinks<TypeId, Option<Vec<TypeId>>>,
    /// tsc InterfaceType.baseTypesResolved (57224/57244) — set true
    /// even when the resolution stack flags a cycle, freezing whatever
    /// resolvedBaseTypes holds.
    pub base_types_resolved: SparseLinks<TypeId, bool>,
    /// tsc InterfaceType.resolvedBaseConstructorType
    /// (getBaseConstructorTypeOfClass 57146) — the checked extends
    /// expression type; the ResolvedBaseConstructorType resolution
    /// property.
    pub resolved_base_constructor_type: SparseLinks<TypeId, LinkSlot<TypeId>>,
    /// tsc PromiseOrAwaitedType.promisedTypeOfPromise
    /// (getPromisedTypeOfPromise 82316) — the memoized `then`
    /// onfulfilled parameter type.
    pub promised_type_of_promise: SparseLinks<TypeId, Option<TypeId>>,
    /// tsc PromiseOrAwaitedType.awaitedTypeOfType
    /// (getAwaitedTypeNoAlias 82435) — the memoized awaited unwrap.
    pub awaited_type_of_type: SparseLinks<TypeId, Option<TypeId>>,
    /// tsc type.widened (getWidenedTypeWithContext 68022/68049) —
    /// the context-free widening memo; context-carrying calls bypass
    /// it in both directions.
    pub widened: SparseLinks<TypeId, Option<TypeId>>,
    /// tsc type[iterationTypesCacheKey] (get/setCachedIterationTypes
    /// 84056-84061): the five §4 verdict slots. `Some(No)` is the
    /// cached noIterationTypes poison — distinguishable from "never
    /// computed" (None), per the m4-58 §4 sentinel rule.
    pub iteration_types_of_iterable:
        SparseLinks<TypeId, Option<crate::iterate::IterationTypesResult>>,
    pub iteration_types_of_async_iterable:
        SparseLinks<TypeId, Option<crate::iterate::IterationTypesResult>>,
    pub iteration_types_of_iterator:
        SparseLinks<TypeId, Option<crate::iterate::IterationTypesResult>>,
    pub iteration_types_of_async_iterator:
        SparseLinks<TypeId, Option<crate::iterate::IterationTypesResult>>,
    pub iteration_types_of_iterator_result:
        SparseLinks<TypeId, Option<crate::iterate::IterationTypesResult>>,
}

impl TypeLinksCold {
    /// The special-instantiation cache `kind` journals.
    fn instantiation_cache(
        &self,
        kind: SpeculativeTypeInstantiationKind,
    ) -> &SparseLinks<TypeId, LinkSlot<TypeId>> {
        match kind {
            SpeculativeTypeInstantiationKind::UniqueLiteralFilled => {
                &self.unique_literal_filled_instantiation
            }
            SpeculativeTypeInstantiationKind::Permissive => &self.permissive_instantiation,
            SpeculativeTypeInstantiationKind::Restrictive => &self.restrictive_instantiation,
            SpeculativeTypeInstantiationKind::BaseConstructor => {
                &self.resolved_base_constructor_type
            }
        }
    }

    fn instantiation_cache_mut(
        &mut self,
        kind: SpeculativeTypeInstantiationKind,
    ) -> &mut SparseLinks<TypeId, LinkSlot<TypeId>> {
        match kind {
            SpeculativeTypeInstantiationKind::UniqueLiteralFilled => {
                &mut self.unique_literal_filled_instantiation
            }
            SpeculativeTypeInstantiationKind::Permissive => &mut self.permissive_instantiation,
            SpeculativeTypeInstantiationKind::Restrictive => &mut self.restrictive_instantiation,
            SpeculativeTypeInstantiationKind::BaseConstructor => {
                &mut self.resolved_base_constructor_type
            }
        }
    }

    /// The simplifiedForWriting or simplifiedForReading cache.
    pub fn simplified(&self, writing: bool) -> &SparseLinks<TypeId, LinkSlot<TypeId>> {
        if writing {
            &self.simplified_for_writing
        } else {
            &self.simplified_for_reading
        }
    }

    fn simplified_mut(&mut self, writing: bool) -> &mut SparseLinks<TypeId, LinkSlot<TypeId>> {
        if writing {
            &mut self.simplified_for_writing
        } else {
            &mut self.simplified_for_reading
        }
    }

    fn memory_usage(&self) -> (usize, usize) {
        let Self {
            mapped_type_parameter,
            mapped_constraint_type,
            mapped_name_type,
            mapped_template_type,
            mapped_modifiers_type,
            mapped_contains_error,
            mapped_apparent_type,
            conditional_true_type,
            conditional_false_type,
            conditional_inferred_true_type,
            conditional_default_constraint,
            conditional_constraint_of_distributive,
            synthetic_type,
            default_only_type,
            resolved_index_type,
            resolved_string_index_type,
            simplified_for_reading,
            simplified_for_writing,
            unique_literal_filled_instantiation,
            permissive_instantiation,
            restrictive_instantiation,
            cached_equivalent_base_type,
            pattern,
            literal_type,
            array_fallback_signatures,
            resolved_properties,
            union_key_property,
            resolved_reduced_type,
            deferred_mapper,
            declared_members,
            resolved_base_types,
            base_types_resolved,
            resolved_base_constructor_type,
            promised_type_of_promise,
            awaited_type_of_type,
            widened,
            iteration_types_of_iterable,
            iteration_types_of_async_iterable,
            iteration_types_of_iterator,
            iteration_types_of_async_iterator,
            iteration_types_of_iterator_result,
        } = self;
        sparse_memory_usage([
            mapped_type_parameter.memory_usage(),
            mapped_constraint_type.memory_usage(),
            mapped_name_type.memory_usage(),
            mapped_template_type.memory_usage(),
            mapped_modifiers_type.memory_usage(),
            mapped_contains_error.memory_usage(),
            mapped_apparent_type.memory_usage(),
            conditional_true_type.memory_usage(),
            conditional_false_type.memory_usage(),
            conditional_inferred_true_type.memory_usage(),
            conditional_default_constraint.memory_usage(),
            conditional_constraint_of_distributive.memory_usage(),
            synthetic_type.memory_usage(),
            default_only_type.memory_usage(),
            resolved_index_type.memory_usage(),
            resolved_string_index_type.memory_usage(),
            simplified_for_reading.memory_usage(),
            simplified_for_writing.memory_usage(),
            unique_literal_filled_instantiation.memory_usage(),
            permissive_instantiation.memory_usage(),
            restrictive_instantiation.memory_usage(),
            cached_equivalent_base_type.memory_usage(),
            pattern.memory_usage(),
            literal_type.memory_usage(),
            array_fallback_signatures.memory_usage(),
            resolved_properties.memory_usage(),
            union_key_property.memory_usage(),
            resolved_reduced_type.memory_usage(),
            deferred_mapper.memory_usage(),
            declared_members.memory_usage(),
            resolved_base_types.memory_usage(),
            base_types_resolved.memory_usage(),
            resolved_base_constructor_type.memory_usage(),
            promised_type_of_promise.memory_usage(),
            awaited_type_of_type.memory_usage(),
            widened.memory_usage(),
            iteration_types_of_iterable.memory_usage(),
            iteration_types_of_async_iterable.memory_usage(),
            iteration_types_of_iterator.memory_usage(),
            iteration_types_of_async_iterator.memory_usage(),
            iteration_types_of_iterator_result.memory_usage(),
        ])
    }
}

/// The getKeyPropertyName cache payload.
#[derive(Clone, Debug, Default)]
pub struct UnionKeyProperty {
    pub name: Option<tsc_types::EscapedName>,
    pub constituent_map: Option<rustc_hash::FxHashMap<TypeId, TypeId>>,
}

/// One speculation journal: for each link a transaction wrote, the value
/// it held before that transaction's first write, in write order.
///
/// Commit and rollback pop entries back to the transaction's mark. A link
/// is journaled once per transaction depth; `journaled` answers that test
/// without scanning the open transactions' entries, a scan that made an
/// overload candidate resolving thousands of lazy links quadratic (6 % of a
/// serial playwright check).
#[derive(Debug)]
struct SpeculativeJournal<K, V> {
    entries: Vec<(u32, K, V)>,
    /// `(depth, link)` to the index of its entry in `entries`.
    journaled: FxHashMap<(u32, K), usize>,
}

impl<K, V> Default for SpeculativeJournal<K, V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            journaled: FxHashMap::default(),
        }
    }
}

impl<K: Copy + Eq + std::hash::Hash, V> SpeculativeJournal<K, V> {
    /// The position a transaction's commit or rollback restores.
    fn mark(&self) -> usize {
        self.entries.len()
    }

    fn contains(&self, depth: u32, key: K) -> bool {
        self.journaled.contains_key(&(depth, key))
    }

    /// The entry the transaction at `depth` already journaled for `key`.
    fn entry_mut(&mut self, depth: u32, key: K) -> Option<&mut V> {
        let index = *self.journaled.get(&(depth, key))?;
        Some(&mut self.entries[index].2)
    }

    /// Journal `key`'s value before the first write of the transaction at
    /// `depth`; callers check [`Self::contains`] first.
    fn push(&mut self, depth: u32, key: K, previous: V) {
        let replaced = self.journaled.insert((depth, key), self.entries.len());
        debug_assert!(
            replaced.is_none(),
            "a link is journaled once per transaction"
        );
        self.entries.push((depth, key, previous));
    }

    /// Remove the newest entry above `mark`.
    fn pop_above(&mut self, mark: usize) -> Option<(K, V)> {
        if self.entries.len() <= mark {
            return None;
        }
        let (depth, key, previous) = self.entries.pop()?;
        self.journaled.remove(&(depth, key));
        Some((key, previous))
    }
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

type SpeculativeSymbolVarianceWrite = (u32, SymbolId, LinkSlot<Box<[tsc_types::VarianceFlags]>>);
type SpeculativeTypeOnlyAliasWrite = (u32, SymbolId, Option<Option<NodeId>>, Option<EscapedName>);

/// Whether a speculative symbol-type write is merely a candidate-local
/// cache publication or semantic state selected by overload resolution.
///
/// Contextual parameter types and completed accessor types are once-results:
/// tsc leaves them (and the function's `ContextChecked` flag) in place after
/// a selected OR rejected candidate. Other lazy symbol-type
/// publications remain transaction-local and are discarded at either
/// boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpeculativeSymbolTypeDisposition {
    Temporary,
    CompletedOnceResult,
}

#[derive(Clone, Debug)]
struct SpeculativeSymbolTypeWrite {
    previous: LinkSlot<TypeId>,
    disposition: SpeculativeSymbolTypeDisposition,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SpeculativeLinksMarks {
    resolved_signatures: usize,
    declaration_signatures: usize,
    resolved_types: usize,
    decorator_signatures: usize,
    enum_values_computed: usize,
    context_checked: usize,
    symbol_declared_types: usize,
    symbol_types: usize,
    symbol_write_types: usize,
    unique_es_symbol_types: usize,
    late_symbols: usize,
    symbol_variances: usize,
    alias_targets: usize,
    type_only_aliases: usize,
    alias_instantiations: usize,
    conditional_instantiations: usize,
    type_instantiations: usize,
    conditional_caches: usize,
    type_members: usize,
    simplified_types: usize,
    non_existent_props: usize,
}

impl<K, T> PagedTable<K, T> {
    /// Record count and the bytes the table owns (memory accounting).
    fn memory_usage(&self) -> (usize, usize) {
        let pages = self.index.iter().filter(|page| page.is_some()).count() + self.far.len();
        let mut bytes = self.index.capacity() * std::mem::size_of::<Option<Box<[u32; PAGE_IDS]>>>()
            + pages * PAGE_IDS * std::mem::size_of::<u32>()
            + self.chunks.capacity() * std::mem::size_of::<Vec<T>>();
        for chunk in &self.chunks {
            bytes += chunk.capacity() * std::mem::size_of::<T>();
        }
        (self.len, bytes)
    }
}

impl LinksTables {
    /// The node, symbol and type link records and their sparse fields, by
    /// entries and bytes (memory accounting).
    pub(crate) fn memory_usage(&self) -> [(&'static str, usize, usize); 6] {
        let node = self.node.memory_usage();
        let symbol = self.symbol.memory_usage();
        let ty = self.ty.memory_usage();
        let node_cold = self.node_cold.memory_usage();
        let symbol_cold = self.symbol_cold.memory_usage();
        let type_cold = self.type_cold.memory_usage();
        [
            ("links: node (NodeLinks)", node.0, node.1),
            ("links: node sparse fields", node_cold.0, node_cold.1),
            ("links: symbol (SymbolLinks)", symbol.0, symbol.1),
            ("links: symbol sparse fields", symbol_cold.0, symbol_cold.1),
            ("links: type (TypeLinks)", ty.0, ty.1),
            ("links: type sparse fields", type_cold.0, type_cold.1),
        ]
    }
}

impl LinksTables {
    /// Pre-size the three ID-keyed tables from the Program's node and
    /// persistent-symbol counts, bounded so that no input can make the
    /// eager reservation exceed a fixed number of slots per table.
    ///
    /// The ratios come from the measured fill on the benchmark inputs
    /// (scale256: node links ≈ 1/9 of nodes, symbol links ≈ 71% of symbols,
    /// type links ≈ 20% of symbols) and are deliberately below the observed
    /// fill so a hint never allocates a larger table than lazy growth would
    /// have reached there; the caps bound the cost for inputs with many
    /// bound but rarely linked symbols (for example a large unused
    /// declaration file under `skipLibCheck`). Hints only reduce rehash
    /// work and never change lookup results.
    pub(crate) fn with_capacity_hint(_nodes: usize, _symbols: usize) -> Self {
        // The paged tables allocate index pages and record chunks on first
        // use, so the program's node/symbol counts need no up-front reserve.
        Self::default()
    }
}

/// A compiler-assigned dense identity usable as a paged-table key.
pub(crate) trait DenseKey: Copy {
    fn dense_index(self) -> usize;
}

impl DenseKey for NodeId {
    #[inline]
    fn dense_index(self) -> usize {
        self.index() as usize
    }
}

impl DenseKey for SymbolId {
    /// Persistent (binder) ids and checker-transient ids (those carrying
    /// `TRANSIENT_SYMBOL_BIT`) are each dense from zero; interleaving them
    /// keeps both in the dense page range instead of letting the transient
    /// bit address page 2^21.
    #[inline]
    fn dense_index(self) -> usize {
        let raw = self.index();
        if raw & tsc_types::TRANSIENT_SYMBOL_BIT != 0 {
            ((raw & !tsc_types::TRANSIENT_SYMBOL_BIT) as usize) * 2 + 1
        } else {
            (raw as usize) * 2
        }
    }
}

impl DenseKey for TypeId {
    #[inline]
    fn dense_index(self) -> usize {
        self.index() as usize
    }
}

/// Ids per index page (a page is allocated when an id in its range first
/// gets a record). A checker that links only part of a large program touches
/// pages sparsely: on VS Code with eight checkers, 256-id pages take a
/// quarter less index memory than 1,024-id pages did.
const PAGE_SHIFT: usize = 8;
const PAGE_IDS: usize = 1 << PAGE_SHIFT;
const CHUNK_SHIFT: usize = 8;
const CHUNK_RECORDS: usize = 1 << CHUNK_SHIFT;
/// Pages below this index (ids below 2^26) live in the direct vector; the
/// rare id far beyond the program's dense domain (a sentinel, or an enormous
/// program) takes the hashed far-page path instead of growing the vector to
/// reach it. VS Code's node ids reach 17 million and its symbols' dense
/// indices 34 million.
const DENSE_PAGES: usize = 1 << 18;

/// Side table keyed by a dense compiler-assigned id.
///
/// tsrs-native: tsc hangs a links object directly off each node/symbol/type;
/// Rust keeps the arenas immutable and stores the links here. A lookup is two
/// indexed loads (an index page, then the record slab) with no hashing; index
/// pages are allocated on first use per id range, and records live in
/// fixed-size chunks that never move once written, so a growing table neither
/// rehashes nor copies the large records. There is no removal: links are
/// monotone per checker, exactly like tsc's per-object fields.
#[derive(Debug)]
pub(crate) struct PagedTable<K, T> {
    /// id → record slot + 1 (0 = absent), per page of `PAGE_IDS` ids.
    index: Vec<Option<Box<[u32; PAGE_IDS]>>>,
    /// Pages at or beyond `DENSE_PAGES`.
    far: FxHashMap<usize, Box<[u32; PAGE_IDS]>>,
    chunks: Vec<Vec<T>>,
    len: usize,
    key: std::marker::PhantomData<K>,
}

impl<K, T> Default for PagedTable<K, T> {
    fn default() -> Self {
        Self {
            index: Vec::new(),
            far: FxHashMap::default(),
            chunks: Vec::new(),
            len: 0,
            key: std::marker::PhantomData,
        }
    }
}

impl<K: DenseKey, T: Default> PagedTable<K, T> {
    #[inline]
    fn page(&self, page_index: usize) -> Option<&[u32; PAGE_IDS]> {
        if page_index < DENSE_PAGES {
            self.index.get(page_index)?.as_deref()
        } else {
            self.far.get(&page_index).map(|page| &**page)
        }
    }

    #[inline]
    fn page_mut(&mut self, page_index: usize) -> &mut [u32; PAGE_IDS] {
        if page_index < DENSE_PAGES {
            if page_index >= self.index.len() {
                self.index.resize_with(page_index + 1, || None);
            }
            self.index[page_index].get_or_insert_with(|| Box::new([0; PAGE_IDS]))
        } else {
            self.far
                .entry(page_index)
                .or_insert_with(|| Box::new([0; PAGE_IDS]))
        }
    }

    #[inline]
    pub(crate) fn get(&self, key: K) -> Option<&T> {
        let id = key.dense_index();
        let page = self.page(id >> PAGE_SHIFT)?;
        let slot = page[id & (PAGE_IDS - 1)];
        if slot == 0 {
            return None;
        }
        let slot = (slot - 1) as usize;
        Some(&self.chunks[slot >> CHUNK_SHIFT][slot & (CHUNK_RECORDS - 1)])
    }

    /// The record for `key`, created as `T::default()` on first use (the
    /// `entry(key).or_default()` protocol of the previous map).
    #[inline]
    pub(crate) fn slot(&mut self, key: K) -> &mut T {
        let id = key.dense_index();
        let page_index = id >> PAGE_SHIFT;
        let offset = id & (PAGE_IDS - 1);
        let mut entry = self.page_mut(page_index)[offset];
        if entry == 0 {
            let slot = self.len;
            if slot & (CHUNK_RECORDS - 1) == 0 {
                self.chunks.push(Vec::with_capacity(CHUNK_RECORDS));
            }
            self.chunks[slot >> CHUNK_SHIFT].push(T::default());
            self.len += 1;
            entry = u32::try_from(slot + 1).expect("links record count fits u32");
            self.page_mut(page_index)[offset] = entry;
        }
        let slot = (entry - 1) as usize;
        &mut self.chunks[slot >> CHUNK_SHIFT][slot & (CHUNK_RECORDS - 1)]
    }
}

#[derive(Debug, Default)]
pub struct LinksTables {
    // Dense paged tables over compiler-assigned IDs (see PagedTable): no
    // hashing, no rehash, records never move. Their iteration order is not
    // observable. Keep the other cache and public field types unchanged.
    node: PagedTable<NodeId, NodeLinks>,
    symbol: PagedTable<SymbolId, SymbolLinks>,
    ty: PagedTable<TypeId, TypeLinks>,
    // The fields few records set, one sparse map per field.
    node_cold: NodeLinksCold,
    symbol_cold: SymbolLinksCold,
    type_cold: TypeLinksCold,
    // Immutable default records answered by the borrowed `read_*` accessors
    // on a miss (tsc's "links object with no fields yet"). Built once with
    // the table so a miss never constructs and drops a full record; they are
    // never mutated (writes go through the maps' entries), so every reader
    // observes exactly `Default::default()`.
    absent_node: NodeLinks,
    absent_symbol: SymbolLinks,
    absent_ty: TypeLinks,
    /// Trial-local resolvedSignature protocol writes. Nested call
    /// resolution needs its Resolving sentinel and failure stash while
    /// a candidate is checked. Both rejection and selection restore the
    /// entry state; the enclosing non-speculative call frame owns the
    /// permanent call-node publication.
    speculative_resolved_signature_writes: Vec<(u32, NodeId, LinkSlot<crate::state::SignatureId>)>,
    /// Declaration-site getSignatureFromDeclaration publications.
    /// These must be visible throughout a candidate so every member
    /// view shares one SignatureId. Completed candidates retain them
    /// (and a nested retain promotes the entry snapshot).
    speculative_declaration_signature_writes:
        Vec<(u32, NodeId, LinkSlot<crate::state::SignatureId>)>,
    /// Trial-local type-node resolution publications. A candidate
    /// needs one stable type identity (and resolving sentinels) while
    /// it runs, but the AST cache must return to its entry state.
    speculative_resolved_type_writes: Vec<SpeculativeResolvedTypeWrite>,
    /// Diagnostic keys must suppress re-entry inside a candidate without
    /// preventing a later candidate from reporting its own diagnostics.
    speculative_non_existent_prop_writes: Vec<(NodeId, String)>,
    /// Trial-local decorator-signature protocol writes. The
    /// `any_signature` sentinel must remain visible to re-entrant
    /// decorator checks inside the same candidate.
    speculative_decorator_signature_writes: Vec<(u32, NodeId, Option<crate::state::SignatureId>)>,
    /// Trial-local enum-value completion once-flags. Candidate checking
    /// may force an enum for the first time; the flag must be visible to
    /// re-entrant reads in that trial and restored at either boundary.
    speculative_enum_values_computed_writes: Vec<(u32, NodeId, bool)>,
    /// Trial-local ContextChecked once-flags. Completed candidates retain
    /// them and promote their rollback snapshot to an enclosing
    /// transaction; only explicit rollback/CheckAbort restores them.
    speculative_context_checked_writes: Vec<(u32, NodeId, tsc_types::NodeCheckFlags)>,
    /// Trial-local declared-type publications for symbols first forced
    /// by candidate checking.
    speculative_symbol_declared_type_writes: Vec<(u32, SymbolId, LinkSlot<TypeId>)>,
    /// Trial-local value-type publications for symbols first forced by
    /// candidate checking. The disposition distinguishes reproducible
    /// cache state from completed contextual parameter and accessor types.
    speculative_symbol_type_writes: SpeculativeJournal<SymbolId, SpeculativeSymbolTypeWrite>,
    /// Trial-local accessor/instantiated-property write-type caches.
    speculative_symbol_write_type_writes: Vec<(u32, SymbolId, LinkSlot<TypeId>)>,
    /// Trial-local unique-symbol type publications.
    speculative_unique_es_symbol_type_writes: Vec<(u32, SymbolId, Option<TypeId>)>,
    /// Trial-local links from binder members to synthesized late-bound
    /// symbols.
    speculative_late_symbol_writes: Vec<(u32, SymbolId, Option<SymbolId>)>,
    /// Trial-local variance measurement publications.
    speculative_symbol_variance_writes: Vec<SpeculativeSymbolVarianceWrite>,
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
    speculative_type_member_writes: SpeculativeJournal<TypeId, LinkSlot<crate::state::MembersId>>,
    /// Trial-local indexed-access simplification protocols. Both the
    /// circular sentinel and the completed simplification must remain
    /// visible for the duration of a candidate.
    speculative_simplified_type_writes: Vec<(u32, TypeId, bool, LinkSlot<TypeId>)>,
    /// tsc unionType.propertyCache / propertyCacheWithoutObjectFunctionPropertyAugment
    /// (getUnionOrIntersectionProperty 59246) — a monotone cache, not a
    /// one-write slot; only successful synthesis is cached, like tsc.
    /// Private since m4-review B10: the write goes through
    /// set_union_property (speculation assert).
    // Tuple keys remain owned: byte borrowing cannot query a heterogeneous tuple.
    union_property_cache: HashMap<(TypeId, EscapedName, bool), SymbolId>,
    /// tsc type-alias links.instantiations (getDeclaredTypeOfTypeAlias
    /// 57417 seed + getTypeAliasInstantiation 60271), keyed by
    /// getTypeListId + getAliasId — a monotone cache like tsc's map.
    alias_instantiations: HashMap<(SymbolId, String), TypeId>,
    /// tsc NodeLinks.serializedTypes (visitAndTransformType 51811): the
    /// per-enclosing-declaration reuse of already-built type nodes, keyed
    /// by `typeId|flags|internalFlags`. Node handles are arena-bound, so
    /// each entry records its TransformArena identity and misses for any
    /// other arena.
    serialized_type_nodes: HashMap<SerializedTypeNodeKey, SerializedTypeNodeEntry>,
    /// tsc ConditionalRoot.instantiations, keyed by the shared root
    /// object plus getTypeListId/getAliasId. Writes happen only after
    /// a complete result; a re-entrant outer evaluation may replace
    /// the inner complete result, matching Map.set.
    conditional_instantiations: HashMap<(ConditionalRootId, String), TypeId>,
}

/// tsc NodeLinks.serializedTypes key: (enclosing declaration, type,
/// NodeBuilderFlags bits, InternalNodeBuilderFlags bits).
pub type SerializedTypeNodeKey = (NodeId, TypeId, u32, u32);

/// tsc NodeLinks.serializedTypes entry (visitAndTransformType 51811):
/// the built node plus the side effects a cache hit must replay.
#[derive(Clone, Debug)]
pub struct SerializedTypeNodeEntry {
    pub arena_id: u64,
    pub node: tsc_emitter::TransformNode,
    pub truncating: bool,
    pub added_length: u32,
    pub tracked_symbols: Option<Vec<(SymbolId, Option<NodeId>, tsc_emitter::EmitSymbolMeaning)>>,
}

impl LinksTables {
    /// tsrs-native: the `links.serializedTypes` reuse cache accessor behind visitAndTransformType (@6.0.3; the serializedTypes map on NodeLinks).
    /// tsc links.serializedTypes read (visitAndTransformType 51811): an
    /// entry from another arena never hits.
    pub fn serialized_type_node(
        &self,
        key: &SerializedTypeNodeKey,
        arena_id: u64,
    ) -> Option<&SerializedTypeNodeEntry> {
        self.serialized_type_nodes
            .get(key)
            .filter(|entry| entry.arena_id == arena_id)
    }

    /// tsrs-native: the `links.serializedTypes` reuse cache accessor behind visitAndTransformType (@6.0.3; the serializedTypes map on NodeLinks).
    /// tsc links.serializedTypes write (visitAndTransformType 51811).
    pub fn set_serialized_type_node(
        &mut self,
        key: SerializedTypeNodeKey,
        entry: SerializedTypeNodeEntry,
    ) {
        self.serialized_type_nodes.insert(key, entry);
    }
}

impl LinksTables {
    /// tsrs-native: select an owned field result without cloning a whole
    /// links record. The borrow ends before the caller can mutate the checker
    /// again; public snapshot getters retain their owned-copy semantics.
    #[inline]
    pub(crate) fn read_node<R>(&self, id: NodeId, read: impl FnOnce(&NodeLinks) -> R) -> R {
        perf::bump(PerfCounter::LinksNodeReads);
        match self.node.get(id) {
            Some(links) => read(links),
            None => {
                perf::bump(PerfCounter::LinksNodeReadAbsent);
                read(&self.absent_node)
            }
        }
    }

    /// Number of symbol records this table holds (trace evidence).
    pub(crate) fn symbol_len(&self) -> usize {
        self.symbol.len
    }

    /// tsrs-native: owned projection of an immutable symbol-links field.
    #[inline]
    pub(crate) fn read_symbol<R>(&self, id: SymbolId, read: impl FnOnce(&SymbolLinks) -> R) -> R {
        perf::bump(PerfCounter::LinksSymbolReads);
        match self.symbol.get(id) {
            Some(links) => read(links),
            None => {
                perf::bump(PerfCounter::LinksSymbolReadAbsent);
                read(&self.absent_symbol)
            }
        }
    }

    /// tsrs-native: owned projection of an immutable type-links field.
    #[inline]
    pub(crate) fn read_ty<R>(&self, id: TypeId, read: impl FnOnce(&TypeLinks) -> R) -> R {
        perf::bump(PerfCounter::LinksTypeReads);
        match self.ty.get(id) {
            Some(links) => read(links),
            None => {
                perf::bump(PerfCounter::LinksTypeReadAbsent);
                read(&self.absent_ty)
            }
        }
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function. The record by
    /// reference (the default record when none was written): readers copy
    /// the fields they need rather than the whole record and its cold box.
    pub fn node(&self, id: NodeId) -> &NodeLinks {
        self.node.get(id).unwrap_or(&self.absent_node)
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function. By reference, as
    /// [`LinksTables::node`].
    pub fn symbol(&self, id: SymbolId) -> &SymbolLinks {
        self.symbol.get(id).unwrap_or(&self.absent_symbol)
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function. By reference, as
    /// [`LinksTables::node`].
    pub fn ty(&self, id: TypeId) -> &TypeLinks {
        self.ty.get(id).unwrap_or(&self.absent_ty)
    }

    /// The NodeLinks fields few nodes set: `node_cold().field.get(id)`.
    pub fn node_cold(&self) -> &NodeLinksCold {
        &self.node_cold
    }

    /// The SymbolLinks fields few symbols set: `symbol_cold().field.get(id)`.
    pub fn symbol_cold(&self) -> &SymbolLinksCold {
        &self.symbol_cold
    }

    /// The TypeLinks fields few types set: `type_cold().field.get(id)`.
    pub fn type_cold(&self) -> &TypeLinksCold {
        &self.type_cold
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

    fn write_slot<T: Clone + std::fmt::Debug>(slot: &mut LinkSlot<T>, next: LinkSlot<T>) {
        match (&*slot, &next) {
            (LinkSlot::Vacant, _) | (LinkSlot::Resolving, LinkSlot::Resolved(_)) => {
                match (&*slot, &next) {
                    (LinkSlot::Vacant, LinkSlot::Resolving) => {
                        perf::bump(PerfCounter::LinksSlotResolvingStarted)
                    }
                    (LinkSlot::Resolving, LinkSlot::Resolved(_)) => {
                        perf::bump(PerfCounter::LinksSlotResolvedFromResolving)
                    }
                    (LinkSlot::Vacant, LinkSlot::Resolved(_)) => {
                        perf::bump(PerfCounter::LinksSlotResolvedDirect)
                    }
                    _ => {}
                }
                note_resolving_transition(slot.is_resolving(), next.is_resolving());
                *slot = next;
            }
            (LinkSlot::Resolved(_), LinkSlot::Resolving)
            | (LinkSlot::Resolving, LinkSlot::Resolving) => {
                // A resolved cache is never reopened, and a re-entrant
                // start keeps the sentinel already in place.
            }
            _ => {
                // A trial may compute a cache twice before its first result
                // is observed; the later write wins, as tsc's plain property
                // assignment does.
                note_resolving_transition(slot.is_resolving(), next.is_resolving());
                *slot = next;
            }
        }
    }

    /// [`Self::write_slot`] for a packed slot.
    fn write_id_slot<T: SlotId + std::fmt::Debug>(slot: &mut IdSlot<T>, next: LinkSlot<T>) {
        let mut value = slot.get();
        Self::write_slot(&mut value, next);
        *slot = value.into();
    }

    fn journal_node_resolution(&mut self, speculation_depth: u32, id: NodeId) {
        if speculation_depth == 0 {
            return;
        }
        if self
            .speculative_resolved_type_writes
            .iter()
            .any(|(depth, node, _, _, _)| *depth == speculation_depth && *node == id)
        {
            return;
        }
        let (resolved_type, resolved_symbol) = self
            .node
            .get(id)
            .map(|links| (links.resolved_type.get(), links.resolved_symbol.get()))
            .unwrap_or_default();
        let resolved_jsdoc_type = self.node_cold.resolved_jsdoc_type.get(id).clone();
        self.speculative_resolved_type_writes.push((
            speculation_depth,
            id,
            resolved_type,
            resolved_symbol,
            resolved_jsdoc_type,
        ));
    }

    fn journal_type_instantiation(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        kind: SpeculativeTypeInstantiationKind,
    ) {
        if speculation_depth == 0 {
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
        let previous = self.type_cold.instantiation_cache(kind).get(id).clone();
        self.speculative_type_instantiation_writes
            .push((speculation_depth, id, kind, previous));
    }

    fn journal_conditional_cache(&mut self, speculation_depth: u32, id: TypeId) {
        if speculation_depth == 0 {
            return;
        }
        if self
            .speculative_conditional_cache_writes
            .iter()
            .any(|(depth, ty, _)| *depth == speculation_depth && *ty == id)
        {
            return;
        }
        let cold = &self.type_cold;
        let snapshot = SpeculativeConditionalCacheSnapshot {
            true_type: cold.conditional_true_type.get(id).clone(),
            false_type: cold.conditional_false_type.get(id).clone(),
            inferred_true_type: cold.conditional_inferred_true_type.get(id).clone(),
            default_constraint: cold.conditional_default_constraint.get(id).clone(),
            constraint_of_distributive: cold.conditional_constraint_of_distributive.get(id).clone(),
        };
        self.speculative_conditional_cache_writes
            .push((speculation_depth, id, snapshot));
    }

    fn journal_symbol_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        disposition: SpeculativeSymbolTypeDisposition,
    ) {
        if speculation_depth == 0 {
            return;
        }
        if let Some(existing) = self
            .speculative_symbol_type_writes
            .entry_mut(speculation_depth, id)
        {
            if disposition == SpeculativeSymbolTypeDisposition::CompletedOnceResult {
                existing.disposition = disposition;
            }
            return;
        }
        let previous = self
            .symbol
            .get(id)
            .map(|links| links.type_of_symbol.get())
            .unwrap_or_default();
        self.speculative_symbol_type_writes.push(
            speculation_depth,
            id,
            SpeculativeSymbolTypeWrite {
                previous,
                disposition,
            },
        );
    }

    fn journal_symbol_write_type(&mut self, speculation_depth: u32, id: SymbolId) {
        if speculation_depth == 0 {
            return;
        }
        if self
            .speculative_symbol_write_type_writes
            .iter()
            .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            return;
        }
        let previous = self.symbol_cold.write_type.get(id).clone();
        self.speculative_symbol_write_type_writes
            .push((speculation_depth, id, previous));
    }

    fn journal_alias_target(&mut self, speculation_depth: u32, id: SymbolId) {
        if speculation_depth == 0 {
            return;
        }
        if self
            .speculative_alias_target_writes
            .iter()
            .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            return;
        }
        let previous = self.symbol_cold.alias_target.get(id).clone();
        self.speculative_alias_target_writes
            .push((speculation_depth, id, previous));
    }

    fn journal_type_only_alias(&mut self, speculation_depth: u32, id: SymbolId) {
        if speculation_depth == 0 {
            return;
        }
        if self
            .speculative_type_only_alias_writes
            .iter()
            .any(|(depth, symbol, _, _)| *depth == speculation_depth && *symbol == id)
        {
            return;
        }
        let declaration = *self.symbol_cold.type_only_declaration.get(id);
        let export_star_name = *self.symbol_cold.type_only_export_star_name.get(id);
        self.speculative_type_only_alias_writes.push((
            speculation_depth,
            id,
            declaration,
            export_star_name,
        ));
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_resolved_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: LinkSlot<TypeId>,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        Self::write_id_slot(&mut self.node.slot(id).resolved_type, value);
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
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn overwrite_type_reference_resolution(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        symbol: Option<SymbolId>,
        value: TypeId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let links = self.node.slot(id);
        note_resolving_transition(links.resolved_symbol.is_resolving(), false);
        note_resolving_transition(links.resolved_type.is_resolving(), false);
        links.resolved_symbol = symbol.map_or(LinkSlot::Vacant, LinkSlot::Resolved).into();
        links.resolved_type = LinkSlot::Resolved(value).into();
    }

    /// getTypeFromJSDocValueReference's resolvedJSDocType assignment
    /// is guarded only before computation. Re-entrant evaluation may
    /// therefore publish an inner result before the outer assignment;
    /// tsc's final write wins.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn overwrite_node_resolved_jsdoc_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: TypeId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let slot = self.node_cold.resolved_jsdoc_type.slot(id);
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Resolved(value);
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
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn overwrite_import_type_resolved_symbol(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: SymbolId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let links = self.node.slot(id);
        note_resolving_transition(links.resolved_symbol.is_resolving(), false);
        links.resolved_symbol = LinkSlot::Resolved(value).into();
    }

    /// The import-type sanctioned overwrite pair, type half (see
    /// overwrite_import_type_resolved_symbol; tsc 62828/62834/62862/
    /// 62868-62877 all assign links.resolvedType unguarded).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn overwrite_import_type_resolved_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: TypeId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let links = self.node.slot(id);
        note_resolving_transition(links.resolved_type.is_resolving(), false);
        links.resolved_type = LinkSlot::Resolved(value).into();
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
        self.journal_symbol_type(
            speculation_depth,
            id,
            SpeculativeSymbolTypeDisposition::Temporary,
        );
        let links = self.symbol.slot(id);
        note_resolving_transition(links.type_of_symbol.is_resolving(), false);
        links.type_of_symbol = LinkSlot::Resolved(value).into();
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_context_free_type(&mut self, id: NodeId, value: LinkSlot<TypeId>) {
        // A context-free expression type is a reproducible lazy memo.
        // Its callers retain and return the computed type, so rejected
        // candidates need not publish it to the shared node.
        Self::write_slot(self.node_cold.context_free_type.slot(id), value);
    }

    /// tsrs-native: the links-slot setter behind
    /// `links.parameterInitializerContainsUndefined ??= ...` (71615) —
    /// a compute-once ?? write (the caller checks is_none first, like
    /// tsc's ??=).
    pub fn set_node_parameter_initializer_contains_undefined(&mut self, id: NodeId, value: bool) {
        // The value is derived from checking the initializer and therefore
        // belongs to the candidate transaction when overload resolution is
        // speculative.  Do not publish it from that path; the caller keeps
        // the computed result for the current check and the committed path
        // will populate the cache if it is still needed.
        self.node_cold
            .parameter_initializer_contains_undefined
            .set(id, Some(value));
    }

    /// `links.spreadIndices ??= getSpreadIndices(...)` (73520) — a
    /// compute-once ?? write, not a LinkSlot (both `None` halves are
    /// meaningful values).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_spread_indices(&mut self, id: NodeId, value: SpreadIndices) {
        let slot = self.node_cold.spread_indices.slot(id);
        if slot.is_none() {
            *slot = Some(value);
        }
    }

    /// `links.jsxFlags |= …` (getIntrinsicTagSymbol 74540/74545) — an
    /// accumulating flags word; re-entry ORs the same bits.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn add_node_jsx_flags(&mut self, id: NodeId, value: tsc_types::JsxFlags) {
        *self.node_cold.jsx_flags.slot(id) |= value;
    }

    /// `links.resolvedJsxElementAttributesType = …` (74731) —
    /// compute-once; a rewrite is a protocol bug.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_resolved_jsx_element_attributes_type(&mut self, id: NodeId, value: TypeId) {
        let slot = self.node_cold.resolved_jsx_element_attributes_type.slot(id);
        match slot {
            None => *slot = Some(value),
            Some(existing) if *existing == value => {}
            _ => panic!("resolvedJsxElementAttributesType rewritten: {slot:?} -> {value:?}"),
        }
    }

    /// `sourceFileLinks.jsxFragmentType = …` (getJSXFragmentType
    /// 77377-77395) — compute-once per source file.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_jsx_fragment_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: TypeId,
    ) {
        // Compute-once per source file in any check mode; the value does not
        // depend on the candidate (the rewrite check below still holds).
        let _ = speculation_depth;
        let slot = self.node_cold.jsx_fragment_type.slot(id);
        match slot {
            None => *slot = Some(value),
            Some(existing) if *existing == value => {}
            _ => panic!("jsxFragmentType rewritten: {slot:?} -> {value:?}"),
        }
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
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
                .get(id)
                .map(|links| links.resolved_signature.get())
                .unwrap_or_default();
            self.speculative_declaration_signature_writes
                .push((speculation_depth, id, previous));
        }
        Self::write_id_slot(&mut self.node.slot(id).resolved_signature, value);
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
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
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
                .get(id)
                .map(|links| links.resolved_signature.get())
                .unwrap_or_default();
            self.speculative_resolved_signature_writes
                .push((speculation_depth, id, previous));
        }
        let slot = &mut self.node.slot(id).resolved_signature;
        match (slot.get(), &value) {
            (LinkSlot::Vacant, LinkSlot::Resolving)
            | (LinkSlot::Resolving, LinkSlot::Resolving)
            | (LinkSlot::Resolving, LinkSlot::Resolved(_))
            | (LinkSlot::Resolved(_), LinkSlot::Resolved(_)) => {
                note_resolving_transition(slot.is_resolving(), value.is_resolving());
                *slot = value.into();
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
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
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
        let slot = &mut self.node.slot(id).resolved_signature;
        note_resolving_transition(slot.is_resolving(), value.is_resolving());
        slot.replace(value)
    }

    /// Err-unwind twin for the call protocol: tsc cannot fail inside
    /// resolveSignature, so a CheckAbort unwind that left the
    /// sentinel must revert to Vacant — a later query re-resolves and
    /// fails identically instead of observing a phantom mid-flight
    /// sentinel. Only the frame that WROTE the sentinel reverts
    /// (Resolved stashes stay — they are real memos).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn revert_node_resolved_signature_call(&mut self, id: NodeId) {
        let slot = &mut self.node.slot(id).resolved_signature;
        if slot.is_resolving() {
            note_resolving_transition(true, false);
            *slot = LinkSlot::Vacant.into();
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
        let slot = &mut self.node.slot(id).resolved_signature;
        if !slot.is_resolving() {
            note_resolving_transition(false, true);
            *slot = LinkSlot::Resolving.into();
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
        let slot = &mut self.node.slot(id).resolved_signature;
        if slot.get() != LinkSlot::Vacant {
            note_resolving_transition(slot.is_resolving(), false);
            *slot = LinkSlot::Vacant.into();
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

    /// tsrs-native: capture the enum-values-computed journal position.
    pub fn speculative_enum_values_computed_mark(&self) -> usize {
        self.speculative_enum_values_computed_writes.len()
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
        self.speculative_symbol_type_writes.mark()
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
        self.speculative_type_member_writes.mark()
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
            enum_values_computed: self.speculative_enum_values_computed_mark(),
            context_checked: self.speculative_context_checked_mark(),
            symbol_declared_types: self.speculative_symbol_declared_type_mark(),
            symbol_types: self.speculative_symbol_type_mark(),
            symbol_write_types: self.speculative_symbol_write_type_mark(),
            unique_es_symbol_types: self.speculative_unique_es_symbol_type_mark(),
            late_symbols: self.speculative_late_symbol_mark(),
            symbol_variances: self.speculative_symbol_variance_mark(),
            alias_targets: self.speculative_alias_target_mark(),
            type_only_aliases: self.speculative_type_only_alias_mark(),
            alias_instantiations: self.speculative_alias_instantiation_mark(),
            conditional_instantiations: self.speculative_conditional_instantiation_mark(),
            type_instantiations: self.speculative_type_instantiation_mark(),
            conditional_caches: self.speculative_conditional_cache_mark(),
            type_members: self.speculative_type_members_mark(),
            simplified_types: self.speculative_simplified_type_mark(),
            non_existent_props: self.speculative_non_existent_prop_writes.len(),
        }
    }

    /// tsrs-native: commit one selected overload-candidate transaction.
    ///
    /// Discard candidate-local protocols and lazy cache publications.
    /// Semantic objects constructed during the candidate initialize
    /// their owned fields through the `set_fresh_*` setters instead.
    /// Declaration signatures, contextual-check flags, contextual parameter
    /// types, and completed accessor types are the exceptions: these once-results
    /// must remain available to later candidates and deferred body checks.
    /// tsrs-native: close a completed candidate's journals. tsc keeps
    /// every link a candidate resolved, so nothing completed is taken back;
    /// the twin below applies the same rule on rollback and only in-progress
    /// sentinels return to their entry state.
    pub(crate) fn commit_speculative_writes(
        &mut self,
        marks: SpeculativeLinksMarks,
        _parent_depth: u32,
    ) {
        self.restore_speculative_writes(marks);
    }

    /// tsrs-native: restore every LinksTables journal to its marks.
    pub(crate) fn restore_speculative_writes(&mut self, marks: SpeculativeLinksMarks) {
        self.restore_speculative_resolved_signatures(marks.resolved_signatures);
        self.restore_speculative_declaration_signatures(marks.declaration_signatures);
        self.restore_speculative_resolved_types(marks.resolved_types);
        self.restore_speculative_decorator_signatures(marks.decorator_signatures);
        self.restore_speculative_enum_values_computed(marks.enum_values_computed);
        self.restore_speculative_context_checked(marks.context_checked);
        self.restore_speculative_symbol_declared_types(marks.symbol_declared_types);
        self.restore_speculative_symbol_types(marks.symbol_types);
        self.restore_speculative_symbol_write_types(marks.symbol_write_types);
        self.restore_speculative_unique_es_symbol_types(marks.unique_es_symbol_types);
        self.restore_speculative_late_symbols(marks.late_symbols);
        self.restore_speculative_symbol_variances(marks.symbol_variances);
        self.restore_speculative_alias_targets(marks.alias_targets);
        self.restore_speculative_type_only_aliases(marks.type_only_aliases);
        self.restore_speculative_alias_instantiations(marks.alias_instantiations);
        self.restore_speculative_conditional_instantiations(marks.conditional_instantiations);
        self.restore_speculative_type_instantiations(marks.type_instantiations);
        self.restore_speculative_conditional_caches(marks.conditional_caches);
        self.restore_speculative_type_members(marks.type_members);
        self.restore_speculative_simplified_types(marks.simplified_types);
        self.restore_speculative_non_existent_props(marks.non_existent_props);
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
            let slot = &mut self.node.slot(node).resolved_signature;
            if slot.is_resolving() {
                note_resolving_transition(true, previous.is_resolving());
                *slot = previous.into();
            }
        }
    }

    fn restore_speculative_declaration_signatures(&mut self, mark: usize) {
        while self.speculative_declaration_signature_writes.len() > mark {
            let (_, node, previous) = self
                .speculative_declaration_signature_writes
                .pop()
                .expect("length checked");
            let slot = &mut self.node.slot(node).resolved_signature;
            if slot.is_resolving() {
                note_resolving_transition(true, previous.is_resolving());
                *slot = previous.into();
            }
        }
    }

    /// tsrs-native: speculation-transaction unwind for type-node caches.
    ///
    /// tsc keeps every node resolution a candidate completed
    /// (`links.resolvedType`/`resolvedSymbol` are written in any check mode
    /// and never cleared), so a completed resolution stays published here
    /// too: returning it to its entry state made every later use of the
    /// node resolve — and instantiate — again (twice tsc's instantiation
    /// count and a fifth more types on a real program). Only a resolution
    /// the candidate abandoned mid-way, still `Resolving`, returns to its
    /// entry state, so no sentinel outlives its transaction.
    pub fn restore_speculative_resolved_types(&mut self, mark: usize) {
        while self.speculative_resolved_type_writes.len() > mark {
            let (_, node, previous_type, previous_symbol, previous_jsdoc_type) = self
                .speculative_resolved_type_writes
                .pop()
                .expect("length checked");
            let links = self.node.slot(node);
            if links.resolved_type.is_resolving() {
                note_resolving_transition(true, previous_type.is_resolving());
                links.resolved_type = previous_type.into();
            }
            if links.resolved_symbol.is_resolving() {
                note_resolving_transition(true, previous_symbol.is_resolving());
                links.resolved_symbol = previous_symbol.into();
            }
            if self.node_cold.resolved_jsdoc_type.get(node).is_resolving() {
                note_resolving_transition(true, previous_jsdoc_type.is_resolving());
                self.node_cold
                    .resolved_jsdoc_type
                    .set(node, previous_jsdoc_type);
            }
        }
    }

    /// tsrs-native: speculation-transaction unwind for decorator
    /// signature sentinels and results.
    pub fn restore_speculative_decorator_signatures(&mut self, mark: usize) {
        self.speculative_decorator_signature_writes.truncate(mark);
    }

    /// tsrs-native: speculation-transaction unwind for the enum value
    /// completion once-flag.
    pub fn restore_speculative_enum_values_computed(&mut self, mark: usize) {
        self.speculative_enum_values_computed_writes.truncate(mark);
    }

    /// tsrs-native: speculation-transaction unwind for ContextChecked
    /// once-flags.
    pub fn restore_speculative_context_checked(&mut self, mark: usize) {
        self.speculative_context_checked_writes.truncate(mark);
    }

    /// tsrs-native: speculation-transaction unwind for symbol
    /// declared-type caches.
    pub fn restore_speculative_symbol_declared_types(&mut self, mark: usize) {
        while self.speculative_symbol_declared_type_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_symbol_declared_type_writes
                .pop()
                .expect("length checked");
            let slot = &mut self.symbol.slot(symbol).declared_type;
            if slot.is_resolving() {
                note_resolving_transition(true, previous.is_resolving());
                *slot = previous.into();
            }
        }
    }

    /// tsrs-native: speculation-transaction unwind for symbol
    /// value-type caches.
    pub fn restore_speculative_symbol_types(&mut self, mark: usize) {
        while let Some((symbol, write)) = self.speculative_symbol_type_writes.pop_above(mark) {
            let slot = &mut self.symbol.slot(symbol).type_of_symbol;
            if slot.is_resolving() {
                perf::bump(PerfCounter::LinksSymbolTypeRollbacks);
            }
            if slot.is_resolving() {
                note_resolving_transition(true, write.previous.is_resolving());
                *slot = write.previous.into();
            }
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
            let slot = self.symbol_cold.write_type.slot(symbol);
            if slot.is_resolving() {
                note_resolving_transition(true, previous.is_resolving());
                *slot = previous;
            }
        }
    }

    /// tsrs-native: speculation-transaction unwind for unique-symbol
    /// type caches.
    pub fn restore_speculative_unique_es_symbol_types(&mut self, mark: usize) {
        self.speculative_unique_es_symbol_type_writes.truncate(mark);
    }

    /// tsrs-native: speculation-transaction unwind for late-symbol
    /// links.
    pub fn restore_speculative_late_symbols(&mut self, mark: usize) {
        self.speculative_late_symbol_writes.truncate(mark);
    }

    /// tsrs-native: speculation-transaction unwind for variance caches.
    pub fn restore_speculative_symbol_variances(&mut self, mark: usize) {
        while self.speculative_symbol_variance_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_symbol_variance_writes
                .pop()
                .expect("length checked");
            let slot = self.symbol_cold.variances.slot(symbol);
            if slot.is_resolving() {
                note_resolving_transition(true, previous.is_resolving());
                *slot = previous;
            }
        }
    }

    /// tsrs-native: speculation-transaction unwind for alias targets.
    pub fn restore_speculative_alias_targets(&mut self, mark: usize) {
        while self.speculative_alias_target_writes.len() > mark {
            let (_, symbol, previous) = self
                .speculative_alias_target_writes
                .pop()
                .expect("length checked");
            let slot = self.symbol_cold.alias_target.slot(symbol);
            if slot.is_resolving() {
                note_resolving_transition(true, previous.is_resolving());
                *slot = previous;
            }
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
            self.symbol_cold
                .type_only_declaration
                .set(symbol, declaration);
            self.symbol_cold
                .type_only_export_star_name
                .set(symbol, export_star_name);
        }
    }

    /// tsrs-native: speculation-transaction unwind for generic-alias
    /// instantiation cache entries.
    pub fn restore_speculative_alias_instantiations(&mut self, mark: usize) {
        self.speculative_alias_instantiation_writes.truncate(mark);
    }

    /// tsrs-native: speculation-transaction unwind for conditional-root
    /// instantiation cache entries.
    pub fn restore_speculative_conditional_instantiations(&mut self, mark: usize) {
        self.speculative_conditional_instantiation_writes
            .truncate(mark);
    }

    /// tsrs-native: speculation-transaction unwind for special type
    /// instantiation caches.
    pub fn restore_speculative_type_instantiations(&mut self, mark: usize) {
        while self.speculative_type_instantiation_writes.len() > mark {
            let (_, ty, kind, previous) = self
                .speculative_type_instantiation_writes
                .pop()
                .expect("length checked");
            let slot = self.type_cold.instantiation_cache_mut(kind).slot(ty);
            if slot.is_resolving() {
                note_resolving_transition(true, previous.is_resolving());
                *slot = previous;
            }
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
            let cold = &mut self.type_cold;
            if cold.conditional_true_type.get(ty).is_resolving() {
                note_resolving_transition(true, previous.true_type.is_resolving());
                cold.conditional_true_type.set(ty, previous.true_type);
            }
            if cold.conditional_false_type.get(ty).is_resolving() {
                note_resolving_transition(true, previous.false_type.is_resolving());
                cold.conditional_false_type.set(ty, previous.false_type);
            }
            if cold.conditional_inferred_true_type.get(ty).is_resolving() {
                note_resolving_transition(true, previous.inferred_true_type.is_resolving());
                cold.conditional_inferred_true_type
                    .set(ty, previous.inferred_true_type);
            }
            if cold.conditional_default_constraint.get(ty).is_resolving() {
                note_resolving_transition(true, previous.default_constraint.is_resolving());
                cold.conditional_default_constraint
                    .set(ty, previous.default_constraint);
            }
            if cold
                .conditional_constraint_of_distributive
                .get(ty)
                .is_resolving()
            {
                note_resolving_transition(true, previous.constraint_of_distributive.is_resolving());
                cold.conditional_constraint_of_distributive
                    .set(ty, previous.constraint_of_distributive);
            }
        }
    }

    /// tsrs-native: speculation-transaction unwind for lazy member
    /// caches.
    pub fn restore_speculative_type_members(&mut self, mark: usize) {
        while let Some((ty, previous)) = self.speculative_type_member_writes.pop_above(mark) {
            let slot = &mut self.ty.slot(ty).resolved_members;
            if slot.is_resolving() {
                *slot = previous.into();
            }
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
            let slot = self.type_cold.simplified_mut(writing).slot(ty);
            if slot.is_resolving() {
                note_resolving_transition(true, previous.is_resolving());
                *slot = previous;
            }
        }
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_variances(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<Box<[tsc_types::VarianceFlags]>>,
    ) {
        if speculation_depth != 0
            && !self
                .speculative_symbol_variance_writes
                .iter()
                .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            let previous = self.symbol_cold.variances.get(id).clone();
            self.speculative_symbol_variance_writes
                .push((speculation_depth, id, previous));
        }
        Self::write_slot(self.symbol_cold.variances.slot(id), value);
    }

    /// Err-unwind twin for the variances slot: tsc cannot fail inside
    /// getVariancesWorker, so a measurement cut short by CheckAbort
    /// must leave the slot re-queryable — Resolving reverts to Vacant.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn revert_symbol_variances(&mut self, id: SymbolId) {
        assert!(
            self.symbol_cold.variances.get(id).is_resolving(),
            "variances revert without an in-progress measurement for {id:?}"
        );
        note_resolving_transition(true, false);
        self.symbol_cold.variances.clear(id);
    }

    /// `nodeLinks.flags |= bits` — the NodeCheckFlags word accumulates
    /// (checkSourceFileWorker 87057 `links.flags |= NodeCheckFlags.TypeChecked`
    /// is the first writer).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn or_node_check_flags(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        bits: tsc_types::NodeCheckFlags,
    ) {
        if speculation_depth != 0
            && bits.intersects(tsc_types::NodeCheckFlags::CONTEXT_CHECKED)
            && !self
                .speculative_context_checked_writes
                .iter()
                .any(|(depth, node, _)| *depth == speculation_depth && *node == id)
        {
            let previous = self
                .node
                .get(id)
                .map(|links| links.check_flags)
                .unwrap_or_default();
            self.speculative_context_checked_writes
                .push((speculation_depth, id, previous));
        }
        if speculation_depth != 0
            && !bits.intersects(
                tsc_types::NodeCheckFlags::IN_CHECK_IDENTIFIER
                    | tsc_types::NodeCheckFlags::ASSIGNMENTS_MARKED
                    | tsc_types::NodeCheckFlags::CONTEXT_CHECKED,
            )
        {
            return;
        }
        let links = self.node.slot(id);
        links.check_flags =
            tsc_types::NodeCheckFlags::from_bits(links.check_flags.bits() | bits.bits());
    }

    /// `links.calculatedFlags |= bits` (calculateNodeCheckFlagWorker, e.g. 88170).
    /// Emit-time bookkeeping outside speculation; never reverted.
    /// tsrs-native: links storage primitive for calculateNodeCheckFlagWorker's
    /// `calculatedFlags |=` sites (88170/88179/88187/88199/88201/88213); current prose
    /// cites 88132, which is the noCheck guard: fix it
    pub fn or_calculated_flags(&mut self, id: NodeId, bits: tsc_types::NodeCheckFlags) {
        let flags = self.node_cold.calculated_flags.slot(id);
        *flags = tsc_types::NodeCheckFlags::from_bits(flags.bits() | bits.bits());
    }

    /// `links.isVisible = value` — the declaration-emit visibility slot is
    /// monotone rather than write-once because late alias discovery paints a
    /// previously memoized `false` declaration `true`. Once true, later memo
    /// attempts cannot make it invisible again.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub(crate) fn set_node_is_visible(&mut self, speculation_depth: u32, id: NodeId, value: bool) {
        debug_assert_eq!(
            speculation_depth, 0,
            "NodeLinks.isVisible writes are forbidden during speculation"
        );
        let slot = self.node_cold.is_visible.slot(id);
        *slot = Some(slot.unwrap_or(false) || value);
    }

    /// tsc `pushIfUnique(links.capturedBlockScopeBindings ||= [], symbol)`
    /// (checkNestedBlockScopedBinding 72267-72268). The write sits beside
    /// the ContainsCapturedBlockScopeBinding flag OR, so it follows that
    /// flag's speculation protocol: candidate-context writes are dropped
    /// and only the authoritative pass publishes.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn push_captured_block_scope_binding(&mut self, id: NodeId, symbol: SymbolId) {
        let bindings = self.node_cold.captured_block_scope_bindings.slot(id);
        if !bindings.contains(&symbol) {
            bindings.push(symbol);
        }
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
        bits: tsc_types::NodeCheckFlags,
    ) {
        // The only callers clear scoped latches that may also be set
        // during a candidate. This is the balancing half of that
        // protocol, not a cache publication.
        let _ = speculation_depth;
        let links = self.node.slot(id);
        links.check_flags =
            tsc_types::NodeCheckFlags::from_bits(links.check_flags.bits() & !bits.bits());
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
        self.symbol_cold.last_assignment_pos.set(id, value);
    }

    /// checkGrammarStatementInAmbientContext's once-flag (90344/90349):
    /// set only when the grammar error actually emitted.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_has_reported_statement_in_ambient_context(&mut self, id: NodeId) {
        self.node_cold
            .has_reported_statement_in_ambient_context
            .set(id, true);
    }

    /// tsrs-native: links-table setter for tsc's direct
    /// `links.containsArgumentsReference` memo-field write.
    ///
    /// The result depends only on the immutable syntax tree and bound
    /// name resolution, so it is safe to retain across speculation.
    pub fn set_node_contains_arguments_reference(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: bool,
    ) {
        let _ = speculation_depth;
        self.node_cold
            .contains_arguments_reference
            .set(id, Some(value));
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
            let previous = *self.node_cold.decorator_signature.get(id);
            self.speculative_decorator_signature_writes
                .push((speculation_depth, id, previous));
        }
        self.node_cold.decorator_signature.set(id, value);
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_enum_member_value(
        &mut self,
        _speculation_depth: u32,
        id: NodeId,
        value: crate::evaluate::EvaluatorResult,
    ) {
        // A completed member evaluation is a context-independent semantic
        // fact. Keep it across candidate commit/rollback just as the
        // CheckAbort unwind twin below keeps already-filled member slots;
        // only the enclosing enum-values-computed once-flag is provisional.
        let slot = self.node_cold.enum_member_value.slot(id);
        assert!(slot.is_none(), "enum member value rewritten");
        *slot = Some(value);
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_enum_values_computed(&mut self, speculation_depth: u32, id: NodeId) {
        if speculation_depth != 0
            && !self
                .speculative_enum_values_computed_writes
                .iter()
                .any(|(depth, node, _)| *depth == speculation_depth && *node == id)
        {
            let previous = *self.node_cold.enum_values_computed.get(id);
            self.speculative_enum_values_computed_writes
                .push((speculation_depth, id, previous));
        }
        self.node_cold.enum_values_computed.set(id, true);
    }

    /// CheckAbort-unwind twin of set_node_enum_values_computed — the
    /// once-flag must not stay observable after a failed compute
    /// (member value slots that DID fill are correct facts and stay).
    /// Like every other revert twin this deliberately does NOT assert
    /// speculation_depth (the 7.0t convention, m4-review B35): a revert
    /// RESTORES pre-write state, which is always legal, and an unwind
    /// crossing a speculation boundary reaches twins INSIDE the region
    /// while depth > 0 (speculate.rs rolls back before the Err
    /// re-propagates, so OUTER twins fire at the entry depth).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn revert_node_enum_values_computed(&mut self, id: NodeId) {
        self.node_cold.enum_values_computed.clear(id);
    }

    /// tsrs-native: links-table setter (tsc plain property write).
    /// checkTypeParameterListsIdentical's once-latch (84877). Like
    /// tsc, set BEFORE the identity walk runs — re-entry through the
    /// declared-type forcing sees the latch and skips.
    pub fn set_symbol_type_parameters_checked(&mut self, id: SymbolId) {
        self.symbol_cold.type_parameters_checked.set(id, true);
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
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
                .get(id)
                .map(|links| links.declared_type.get())
                .unwrap_or_default();
            self.speculative_symbol_declared_type_writes
                .push((speculation_depth, id, previous));
        }
        Self::write_id_slot(&mut self.symbol.slot(id).declared_type, value);
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
        Self::write_id_slot(&mut self.symbol.slot(id).declared_type, value);
    }

    /// tsrs-native: publish a class or interface declared-type identity
    /// monotonically across Rust overload-candidate transactions.
    ///
    /// Publish the declaration-owned identity of a class or interface.
    ///
    /// This is deliberately monotone across candidate speculation. A
    /// selected or rejected overload can retain a TypeId that points at
    /// the declared type through a signature or structurally interned
    /// reference. Rolling the symbol slot back would let the same symbol
    /// mint a second declared TypeId, splitting recursive relation keys.
    /// The class/interface constructor is diagnostic-free and completes
    /// before this publication, so no candidate-dependent result crosses
    /// the boundary.
    pub fn set_declaration_owned_symbol_declared_type(
        &mut self,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        Self::write_id_slot(&mut self.symbol.slot(id).declared_type, value);
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        self.journal_symbol_type(
            speculation_depth,
            id,
            SpeculativeSymbolTypeDisposition::Temporary,
        );
        Self::write_id_slot(&mut self.symbol.slot(id).type_of_symbol, value);
    }

    /// Retain a completed declaration-owned value type across completed
    /// overload candidates. Upstream accessor resolution fills links.type
    /// once, including when a candidate is rejected. Recomputing that type
    /// later can encounter a different contextual type and introduce a cycle.
    /// An aborted or explicitly rolled-back transaction still restores the
    /// original slot, together with its completed diagnostic journal.
    /// tsrs-native: journal protocol for upstream's accessor once-result.
    pub fn set_symbol_type_once(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        self.journal_symbol_type(
            speculation_depth,
            id,
            SpeculativeSymbolTypeDisposition::CompletedOnceResult,
        );
        Self::write_id_slot(&mut self.symbol.slot(id).type_of_symbol, value);
    }

    /// tsrs-native: candidate-local contextual symbol initialization.
    ///
    /// The parameter type must remain stable while a candidate is checked.
    /// A completed candidate retains it with the ContextChecked flag,
    /// matching tsc's cross-candidate contextual pin; explicit rollback and
    /// CheckAbort restore both. The plain assignment also preserves tsc's
    /// `unknown` binding-pattern replacement.
    pub fn set_symbol_type_contextual(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        self.journal_symbol_type(
            speculation_depth,
            id,
            SpeculativeSymbolTypeDisposition::CompletedOnceResult,
        );
        let slot = &mut self.symbol.slot(id).type_of_symbol;
        note_resolving_transition(slot.is_resolving(), value.is_resolving());
        *slot = value.into();
    }

    /// tsrs-native: fresh synthetic-symbol initialization.
    ///
    /// Initialize the type carried by a freshly allocated synthetic
    /// symbol. The symbol and this slot form one semantic object, so
    /// construction is safe inside a candidate trial.
    pub fn set_fresh_symbol_type(&mut self, id: SymbolId, value: LinkSlot<TypeId>) {
        Self::write_id_slot(&mut self.symbol.slot(id).type_of_symbol, value);
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
        self.journal_symbol_type(
            speculation_depth,
            id,
            SpeculativeSymbolTypeDisposition::Temporary,
        );
        self.symbol.slot(id).type_of_symbol = LinkSlot::Resolved(value).into();
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_synthetic(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        check_flags: tsc_types::CheckFlags,
        containing_type: TypeId,
        type_of_symbol: TypeId,
    ) {
        let _ = speculation_depth;
        let links = self.symbol.slot(id);
        links.check_flags = check_flags;
        Self::write_id_slot(
            &mut links.type_of_symbol,
            LinkSlot::Resolved(type_of_symbol),
        );
        self.symbol_cold
            .containing_type
            .set(id, Some(containing_type));
    }

    /// tsrs-native: group tsc's direct DeferredType symbol-link writes into
    /// one initialization of a freshly synthesized Rust symbol entry.
    ///
    /// Initialize tsc's DeferredType recipe on a freshly synthesized
    /// union/intersection property. The result slots intentionally remain
    /// vacant until getTypeOfSymbol/getWriteTypeOfSymbol observes them.
    pub fn set_symbol_synthetic_deferred(
        &mut self,
        id: SymbolId,
        check_flags: tsc_types::CheckFlags,
        containing_type: TypeId,
        constituents: Vec<TypeId>,
        write_constituents: Option<Vec<TypeId>>,
    ) {
        self.symbol.slot(id).check_flags = tsc_types::CheckFlags::from_bits(
            check_flags.bits() | tsc_types::CheckFlags::DEFERRED_TYPE.bits(),
        );
        let cold = &mut self.symbol_cold;
        cold.containing_type.set(id, Some(containing_type));
        cold.deferral_parent.set(id, Some(containing_type));
        cold.deferral_constituents.set(id, Some(constituents));
        cold.deferral_write_constituents.set(id, write_constituents);
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_is_discriminant(&mut self, id: SymbolId, value: bool) {
        self.symbol_cold
            .is_discriminant_property
            .set(id, Some(value));
    }

    /// tsrs-native: links-table setter for tsc's
    /// `links.isDeclarationWithCollidingName` memo write
    /// (isSymbolOfDeclarationWithCollidingName 87936/87949/87951). The
    /// verdict reads bound names and published check flags; the emit
    /// resolver computes it outside any speculative context.
    pub fn set_symbol_is_declaration_with_colliding_name(&mut self, id: SymbolId, value: bool) {
        self.symbol_cold
            .is_declaration_with_colliding_name
            .set(id, Some(value));
    }

    /// tsrs-native: SymbolLinks write adapter for tsc's direct
    /// links.isConstructorDeclaredProperty assignment. The verdict
    /// depends only on bound declarations and their annotations, so it
    /// remains valid across speculation like tsc's own memo.
    pub fn set_symbol_is_constructor_declared_property(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: bool,
    ) {
        let _ = speculation_depth;
        self.symbol_cold
            .is_constructor_declared_property
            .set(id, Some(value));
    }

    /// tsrs-native: Err-unwind twin for isConstructorDeclaredProperty's leading
    /// false recursion sentinel. TypeScript cannot throw from the
    /// synchronous worker, while Rust's checked dependency chain can
    /// return CheckAbort; a failed computation must remain retryable.
    pub fn clear_symbol_is_constructor_declared_property(&mut self, id: SymbolId) {
        self.symbol_cold.is_constructor_declared_property.clear(id);
    }

    /// tsrs-native: getUnionOrIntersectionProperty's propertyCache
    /// read (59248).
    pub fn union_property(&self, key: &(TypeId, EscapedName, bool)) -> Option<SymbolId> {
        self.union_property_cache.get(key).copied()
    }

    /// tsrs-native: getUnionOrIntersectionProperty's propertyCache
    /// write (59252) — under the speculation assert since m4-review
    /// B10 (the payload symbol's links writes already were).
    pub fn set_union_property(&mut self, key: (TypeId, EscapedName, bool), value: SymbolId) {
        self.union_property_cache.insert(key, value);
    }

    /// `links.uniqueESSymbolType = ...` (getESSymbolLikeTypeForNode
    /// 63127).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
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
            let previous = *self.symbol_cold.unique_es_symbol_type.get(id);
            self.speculative_unique_es_symbol_type_writes
                .push((speculation_depth, id, previous));
        }
        self.symbol_cold.unique_es_symbol_type.set(id, Some(ty));
    }

    /// tsc createSymbol's checkFlags seed (47656) for transient symbols
    /// created outside the synthetic-property path.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_check_flags(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        check_flags: tsc_types::CheckFlags,
    ) {
        let _ = speculation_depth;
        self.symbol.slot(id).check_flags = check_flags;
    }

    /// `links.nameType = ...` on a fresh transient symbol (getSpreadSymbol
    /// 63054, checkObjectLiteral's late-bound member 74193).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_name_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        name_type: Option<TypeId>,
    ) {
        let _ = speculation_depth;
        self.symbol.slot(id).name_type = name_type;
    }

    /// tsc-port: getSpecifierForModuleSymbol @6.0.3 (links.specifierCache write)
    /// tsc-hash: fb54cac83c15aa20c8dce40fd36a3e787e8fb678220640bc94fafddc12586efa
    /// tsc-span: _tsc.js:53103-53105
    /// Emit-only cache write (h2-7a-m-3 §3a): non-speculative contexts only —
    /// the dormant specifier synthesis runs outside check-phase speculation.
    pub fn set_symbol_specifier_cache_entry(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        cache_key: JsString,
        specifier: JsString,
    ) {
        debug_assert_eq!(
            speculation_depth, 0,
            "specifier cache writes are emit-side and non-speculative"
        );
        let _ = speculation_depth;
        self.symbol_cold
            .specifier_cache
            .slot(id)
            .get_or_insert_with(Default::default)
            .insert(cache_key, specifier);
    }

    /// tsc-port: getAlternativeContainingModules @6.0.3
    /// (links.extendedContainersByFile write, _tsc.js:49972)
    pub fn set_symbol_extended_containers_by_file(
        &mut self,
        id: SymbolId,
        file_index: usize,
        chains: Vec<Vec<SymbolId>>,
    ) {
        self.symbol_cold
            .extended_containers_by_file
            .slot(id)
            .insert(file_index, chains);
    }

    /// tsc-port: getAlternativeContainingModules @6.0.3
    /// (links.extendedContainers write, _tsc.js:49988)
    pub fn set_symbol_extended_containers(&mut self, id: SymbolId, chains: Vec<Vec<SymbolId>>) {
        self.symbol_cold.extended_containers.set(id, Some(chains));
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
        self.symbol.slot(id).name_type = Some(name_type);
        let cold = &mut self.symbol_cold;
        let previous_mapped_type = cold.mapped_type.replace(id, Some(mapped_type));
        let previous_key_type = cold.key_type.replace(id, Some(key_type));
        assert!(
            previous_mapped_type.is_none() && previous_key_type.is_none(),
            "mapped symbol links rewritten"
        );
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
        self.symbol.slot(id).name_type = name_type;
        let cold = &mut self.symbol_cold;
        let previous_property_type = cold.property_type.replace(id, Some(property_type));
        let previous_mapped_type = cold.mapped_type.replace(id, Some(mapped_type));
        let previous_constraint_type = cold.constraint_type.replace(id, Some(constraint_type));
        assert!(
            previous_mapped_type.is_none()
                && previous_property_type.is_none()
                && previous_constraint_type.is_none(),
            "reverse-mapped symbol links rewritten"
        );
    }

    /// tsrs-native: grouped LinksTables setter for tsc
    /// resolveMappedTypeMembers' duplicate-remap union update
    /// (58537-58538).
    pub fn update_symbol_mapped_name_and_key(
        &mut self,
        id: SymbolId,
        name_type: TypeId,
        key_type: TypeId,
    ) {
        assert!(
            self.symbol_cold.mapped_type.get(id).is_some(),
            "only mapped symbols merge name/key links"
        );
        self.symbol.slot(id).name_type = Some(name_type);
        self.symbol_cold.key_type.set(id, Some(key_type));
    }

    /// `links.target = ...` (checkObjectLiteral 74209 — the object
    /// literal member's source symbol, not the instantiation target).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_target(&mut self, speculation_depth: u32, id: SymbolId, target: SymbolId) {
        let _ = speculation_depth;
        self.symbol.slot(id).target = Some(target);
    }

    /// `links.originatingImport = referenceParent` on a fresh interop
    /// clone (cloneTypeAsModuleType 49769).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_originating_import(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        reference_parent: NodeId,
    ) {
        // Like `target` above: the clone is created for this resolution
        // (tsc returns a fresh clone per call), so the write is candidate-
        // local and a speculative overload candidate may make it.
        let _ = speculation_depth;
        self.symbol_cold
            .originating_import
            .set(id, Some(reference_parent));
    }

    /// `links.leftSpread/rightSpread` (getSpreadType 63024-63025).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_spread_pair(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        left: SymbolId,
        right: SymbolId,
    ) {
        // A memo keyed by its own operands: tsc rewrites it in any check
        // mode, and a candidate's pair is only ever read back with its
        // operands.
        let _ = speculation_depth;
        self.symbol_cold.left_spread.set(id, Some(left));
        self.symbol_cold.right_spread.set(id, Some(right));
    }

    /// `links.syntheticOrigin` (getSpreadSymbol 63052 /
    /// getAnonymousPartialType 62955). Every caller initializes a freshly
    /// synthesized symbol, so this semantic stamp belongs to the transaction.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_synthetic_origin(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        origin: SymbolId,
    ) {
        let _ = speculation_depth;
        self.symbol_cold.synthetic_origin.set(id, Some(origin));
    }

    /// `type.literalType = cloneTypeReference(type)` (createArrayLiteralType
    /// 74039) — once-per-reference like the tsc field write.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_literal_type(&mut self, id: TypeId, literal: TypeId) {
        let slot = self.type_cold.literal_type.slot(id);
        assert!(slot.is_none(), "literalType rewritten");
        *slot = Some(literal);
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_promised_type_of_promise(&mut self, id: TypeId, promised: TypeId) {
        let slot = self.type_cold.promised_type_of_promise.slot(id);
        assert!(slot.is_none(), "promisedTypeOfPromise rewritten");
        *slot = Some(promised);
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_awaited_type_of_type(&mut self, id: TypeId, awaited: TypeId) {
        let slot = self.type_cold.awaited_type_of_type.slot(id);
        assert!(slot.is_none(), "awaitedTypeOfType rewritten");
        *slot = Some(awaited);
    }

    /// checkAssertionWorker's links.assertionExpressionType stamp —
    /// re-checks overwrite (tsc reassigns freely).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_assertion_expression_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        ty: TypeId,
    ) {
        let _ = speculation_depth;
        self.node_cold.assertion_expression_type.set(id, Some(ty));
    }

    /// getInstantiationExpressionType's STORE-BEFORE-ERROR map insert.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_instantiation_expression_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        expr_type: TypeId,
        result: TypeId,
    ) {
        // A compute-once map keyed by the expression type: tsc fills it in
        // any check mode and the entry does not depend on the candidate.
        let _ = speculation_depth;
        self.node_cold
            .instantiation_expression_types
            .slot(id)
            .get_or_insert_with(Default::default)
            .insert(expr_type, result);
    }

    /// tsc `symbol.isReferenced = SymbolFlags.All` — freely repeatable.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_is_referenced(&mut self, speculation_depth: u32, id: SymbolId) {
        let _ = speculation_depth;
        self.symbol.slot(id).is_referenced = tsc_types::SymbolFlags::ALL;
    }

    /// tsrs-native: Links-table adapter for tsc resolveNameHelper
    /// 19767-19769: `result.isReferenced |= meaning`; no standalone
    /// tsc function.
    pub fn add_symbol_reference_meaning(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        meaning: tsc_types::SymbolFlags,
    ) {
        let _ = speculation_depth;
        self.symbol.slot(id).is_referenced |= meaning;
    }

    /// tsrs-native: grow-only LinksTables setter for tsc
    /// `getSymbolLinks(symbol).referenced = true`; freely repeatable.
    pub fn set_symbol_alias_referenced(&mut self, speculation_depth: u32, id: SymbolId) {
        let _ = speculation_depth;
        self.symbol_cold.alias_referenced.set(id, true);
    }

    /// nonExistentPropCheckCache add (75419-75423): returns true when
    /// the key was NEW (the caller reports), false on a repeat.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn insert_node_non_existent_prop_key(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        key: String,
    ) -> bool {
        if speculation_depth == 0 {
            return self
                .node_cold
                .non_existent_prop_check_cache
                .slot(id)
                .insert(key);
        }
        let inserted = self
            .node_cold
            .non_existent_prop_check_cache
            .slot(id)
            .insert(key.clone());
        if inserted {
            self.speculative_non_existent_prop_writes.push((id, key));
        }
        inserted
    }

    /// tsrs-native: restore newly inserted diagnostic de-duplication keys
    /// at either candidate boundary, preserving outer and permanent keys.
    fn restore_speculative_non_existent_props(&mut self, mark: usize) {
        self.speculative_non_existent_prop_writes.truncate(mark);
    }

    /// getPropertiesOfUnionOrIntersectionType's tail assignment (58743) is
    /// UNGUARDED in tsc: combining a property can force the SAME union's
    /// properties again (a mapped or indexed access over the union
    /// re-enters through getReducedApparentType), the inner call caches
    /// first, and the outer assignment overwrites it with its own list of
    /// combined symbols. The outer result wins — the third sanctioned
    /// write-twice site (see overwrite_type_reference_resolution).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_resolved_properties(&mut self, id: TypeId, value: Box<[SymbolId]>) {
        let slot = self.type_cold.resolved_properties.slot(id);
        if matches!(slot, LinkSlot::Resolved(_)) {
            *slot = LinkSlot::Resolved(value);
            return;
        }
        Self::write_slot(slot, LinkSlot::Resolved(value));
    }

    /// tsrs-native: publish getSignaturesOfType's Array/ReadonlyArray
    /// fallback cache through the Rust Links table's non-speculative setter.
    ///
    /// getSignaturesOfType's Array/ReadonlyArray union-member fallback
    /// cache (59397-59413).
    pub fn set_type_array_fallback_signatures(&mut self, id: TypeId, value: Box<[SignatureId]>) {
        Self::write_slot(
            self.type_cold.array_fallback_signatures.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_resolved_reduced_type(&mut self, id: TypeId, value: TypeId) {
        Self::write_slot(
            self.type_cold.resolved_reduced_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_union_key_property(&mut self, id: TypeId, value: UnionKeyProperty) {
        Self::write_slot(
            self.type_cold.union_key_property.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: initialize `result.pattern` (56522/56541) once on
    /// a fresh (or freshly-cloned) type. The type and its pattern form
    /// one semantic object, so construction is safe inside speculation.
    pub fn set_fresh_type_pattern(&mut self, id: TypeId, pattern: NodeId) {
        let slot = self.type_cold.pattern.slot(id);
        assert!(slot.is_none(), "type pattern rewritten");
        *slot = Some(pattern);
    }

    /// `type.widened = result` (getWidenedTypeWithContext 68049) —
    /// written by the first context-free widening; EQUAL-value
    /// rewrites are tolerated (tsc overwrites idempotently; the
    /// resolvedSymbol precedent from 5.5e).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_widened(&mut self, id: TypeId, widened: TypeId) {
        let slot = self.type_cold.widened.slot(id);
        assert!(
            slot.is_none() || *slot == Some(widened),
            "type widened memo rewritten with a DIFFERENT value"
        );
        *slot = Some(widened);
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
        id: TypeId,
        key: crate::iterate::IterationCacheKey,
        value: crate::iterate::IterationTypesResult,
    ) {
        // This is a pure, reproducible memo. Candidate-local callers
        // already carry `value`, so publishing it is unnecessary and
        // would let a rejected overload warm shared type state.
        let cold = &mut self.type_cold;
        let cache = match key {
            crate::iterate::IterationCacheKey::Iterable => &mut cold.iteration_types_of_iterable,
            crate::iterate::IterationCacheKey::AsyncIterable => {
                &mut cold.iteration_types_of_async_iterable
            }
            crate::iterate::IterationCacheKey::Iterator => &mut cold.iteration_types_of_iterator,
            crate::iterate::IterationCacheKey::AsyncIterator => {
                &mut cold.iteration_types_of_async_iterator
            }
            crate::iterate::IterationCacheKey::IteratorResult => {
                &mut cold.iteration_types_of_iterator_result
            }
        };
        *cache.slot(id) = Some(value);
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_parameter_constraint(&mut self, id: TypeId, value: TypeId) {
        // A declared or targeted type parameter's constraint is a cold,
        // reproducible cache. Candidate checking must not publish it beyond
        // the speculation boundary.
        Self::write_id_slot(
            &mut self.ty.slot(id).type_parameter_constraint,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: fresh type-parameter initialization.
    ///
    /// Initializes the constraint of a type parameter created inside the
    /// current transaction. Unlike the lazy cache setter above, this is part
    /// of the fresh type's semantic state and may be written speculatively.
    pub fn set_fresh_type_parameter_constraint(&mut self, id: TypeId, value: TypeId) {
        Self::write_id_slot(
            &mut self.ty.slot(id).type_parameter_constraint,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.typeParameter.
    pub fn set_mapped_type_parameter(&mut self, id: TypeId, value: TypeId) {
        Self::write_slot(
            self.type_cold.mapped_type_parameter.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: initialize the type parameter owned by a freshly
    /// allocated mapped-type instantiation.
    pub fn set_fresh_mapped_type_parameter(&mut self, id: TypeId, value: TypeId) {
        Self::write_slot(
            self.type_cold.mapped_type_parameter.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.constraintType.
    pub fn set_mapped_constraint_type(&mut self, id: TypeId, value: TypeId) {
        Self::write_slot(
            self.type_cold.mapped_constraint_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.nameType; None is a resolved absence.
    pub fn set_mapped_name_type(&mut self, id: TypeId, value: Option<TypeId>) {
        Self::write_slot(
            self.type_cold.mapped_name_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.templateType.
    pub fn set_mapped_template_type(&mut self, id: TypeId, value: TypeId) {
        Self::write_slot(
            self.type_cold.mapped_template_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.modifiersType.
    pub fn set_mapped_modifiers_type(&mut self, id: TypeId, value: TypeId) {
        Self::write_slot(
            self.type_cold.mapped_modifiers_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: LinksTables setter for tsc
    /// `mappedType.containsError = true` on a mapped-property type
    /// resolution cycle (58581).
    pub fn set_mapped_contains_error(&mut self, id: TypeId) {
        self.type_cold.mapped_contains_error.set(id, true);
    }

    /// tsrs-native: one-write TypeLinks setter for tsc
    /// MappedType.resolvedApparentType.
    pub fn set_mapped_apparent_type(&mut self, id: TypeId, value: TypeId) {
        Self::write_slot(
            self.type_cold.mapped_apparent_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: one-write ConditionalType.resolvedTrueType setter.
    pub fn set_conditional_true_type(&mut self, speculation_depth: u32, id: TypeId, value: TypeId) {
        self.journal_conditional_cache(speculation_depth, id);
        Self::write_slot(
            self.type_cold.conditional_true_type.slot(id),
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
            self.type_cold.conditional_false_type.slot(id),
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
            self.type_cold.conditional_inferred_true_type.slot(id),
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
            self.type_cold.conditional_default_constraint.slot(id),
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
        if self
            .type_cold
            .conditional_constraint_of_distributive
            .get(id)
            .resolved()
            .is_some_and(|existing| existing == value)
        {
            return;
        }
        self.journal_conditional_cache(speculation_depth, id);
        Self::write_slot(
            self.type_cold
                .conditional_constraint_of_distributive
                .slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_resolved_base_constraint(&mut self, id: TypeId, value: TypeId) {
        Self::write_id_slot(
            &mut self.ty.slot(id).resolved_base_constraint,
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_immediate_base_constraint(&mut self, id: TypeId, value: TypeId) {
        Self::write_id_slot(
            &mut self.ty.slot(id).immediate_base_constraint,
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
            let previous = self.type_cold.simplified(writing).get(id).clone();
            self.speculative_simplified_type_writes.push((
                speculation_depth,
                id,
                writing,
                previous,
            ));
        }
        Self::write_slot(self.type_cold.simplified_mut(writing).slot(id), value);
    }

    /// tsrs-native: Err-unwind twin for the simplified cache — tsc
    /// cannot fail inside getSimplifiedIndexedAccessType, so an
    /// CheckAbort unwind that left the sentinel must revert to
    /// Vacant; a later query re-simplifies instead of observing a
    /// phantom mid-flight sentinel.
    pub fn revert_type_simplified(&mut self, id: TypeId, writing: bool) {
        let cache = self.type_cold.simplified_mut(writing);
        assert!(
            cache.get(id).is_resolving(),
            "simplified revert without an in-progress simplification for {id:?}"
        );
        note_resolving_transition(true, false);
        cache.clear(id);
    }

    /// lateBindMember's two-phase resolvedSymbol write (57665/57689):
    /// the member's own binder symbol parks first (the re-entrancy
    /// guard — checkComputedPropertyName may demand the container
    /// mid-bind), then the LATE symbol replaces it. This protocol
    /// setter permits that one rewrite; member-declaration nodes are
    /// disjoint from the identifier/access kinds the strict setter
    /// serves.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_resolved_symbol_late_bind(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: SymbolId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let slot = &mut self.node.slot(id).resolved_symbol;
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Resolved(value).into();
    }

    /// Err-unwind twin for the late-bind protocol: a container
    /// resolution cut short by CheckAbort must leave every member it
    /// touched re-bindable — a parked memo would short-circuit the
    /// retry's lateBindMember and DROP the member from the rebuilt
    /// late table (5.7b review round #2).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn revert_node_resolved_symbol_late_bind(&mut self, id: NodeId) {
        let slot = &mut self.node.slot(id).resolved_symbol;
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Vacant.into();
    }

    /// Err-unwind twin for `links.lateSymbol`.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn clear_symbol_late_symbol(&mut self, id: SymbolId) {
        self.symbol_cold.late_symbol.clear(id);
    }

    /// The instantiation root: follow links.target (instantiated /
    /// mapped symbols) to the underlying declaration symbol.
    fn instantiation_root(&self, id: SymbolId) -> SymbolId {
        let mut current = id;
        while let Some(target) = self.symbol.get(current).and_then(|links| links.target) {
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
    /// Any other different-value rewrite is a later check's result over a
    /// candidate's persisted one and also wins.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_resolved_symbol(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: SymbolId,
    ) {
        self.journal_node_resolution(speculation_depth, id);
        let sanctioned_rewrite = self
            .node
            .get(id)
            .and_then(|links| links.resolved_symbol.resolved())
            .is_some_and(|existing| {
                existing != value
                    && (*self.symbol_cold.late_symbol.get(existing) == Some(value)
                        || self.instantiation_root(existing) == self.instantiation_root(value))
            });
        let slot = &mut self.node.slot(id).resolved_symbol;
        match slot.get() {
            LinkSlot::Resolved(existing) if existing == value => {}
            // tsc assigns links.resolvedSymbol on every check of the access
            // (checkPropertyAccessExpressionOrQualifiedName), speculative
            // candidates included, and a completed resolution now persists
            // past its candidate: the re-check's symbol — a fresh transient
            // property over the same declaration, or the late twin —
            // overwrites it, last write wins as in tsc.
            LinkSlot::Resolved(_) => {
                let _ = (sanctioned_rewrite, speculation_depth);
                *slot = LinkSlot::Resolved(value).into();
            }
            _ => {
                note_resolving_transition(slot.is_resolving(), false);
                *slot = LinkSlot::Resolved(value).into();
            }
        }
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_node_outer_type_parameters(&mut self, id: NodeId, value: Box<[TypeId]>) {
        Self::write_slot(
            self.node_cold.outer_type_parameters.slot(id),
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
        self.symbol.slot(id).mapper = mapper;
        self.symbol_cold
            .containing_type
            .set(id, Some(containing_type));
    }

    /// instantiateSymbol's transient-links seed (63455-63461): target +
    /// mapper (+ the nameType copy) written once at creation.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_instantiation_links(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        target: SymbolId,
        mapper: MapperId,
        name_type: Option<TypeId>,
    ) {
        let _ = speculation_depth;
        let links = self.symbol.slot(id);
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
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_synthetic_type(&mut self, id: TypeId, value: TypeId) {
        let slot = self.type_cold.synthetic_type.slot(id);
        assert!(slot.is_none(), "syntheticType written twice for {id:?}");
        *slot = Some(value);
    }

    /// tsrs-native: links-table setter for tsc's type.defaultOnlyType write.
    pub fn set_type_default_only_type(&mut self, id: TypeId, value: TypeId) {
        let slot = self.type_cold.default_only_type.slot(id);
        assert!(slot.is_none(), "defaultOnlyType written twice for {id:?}");
        *slot = Some(value);
    }

    /// instantiateAnonymousType's target/mapper seed (63658-63659),
    /// written once at creation of the instantiated shell.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_instantiation_links(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        target: TypeId,
        mapper: MapperId,
    ) {
        let _ = speculation_depth;
        let links = self.ty.slot(id);
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
        let links = self.ty.slot(id);
        assert!(
            links.instantiated_target.is_none() && links.instantiated_mapper.is_none(),
            "isolated-signature mapper written over instantiation links for {id:?}"
        );
        links.instantiated_mapper = Some(mapper);
    }

    /// getResolvedMembersOrExportsOfSymbol's links[resolutionKind]
    /// cache (57717/57763).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_resolved_members(
        &mut self,
        id: SymbolId,
        value: Arc<tsc_binder::SymbolTable>,
    ) {
        Self::write_slot(
            self.symbol_cold.resolved_members.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// The late-binding pre-write/rewrite protocol (57717 → 57763):
    /// the EARLY table parks in the slot so re-entrant reads observe
    /// it mid-bind, then the combined table rewrites. Err unwinds must
    /// revert to Vacant (tsc cannot fail here) — a parked early table
    /// left behind would silently hide late members from later
    /// queries.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_resolved_members_late_bind(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: Arc<tsc_binder::SymbolTable>,
    ) {
        let _ = speculation_depth;
        let slot = self.symbol_cold.resolved_members.slot(id);
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Resolved(value);
    }

    /// Err-unwind twin for the late-binding protocol.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn revert_symbol_resolved_members(&mut self, id: SymbolId) {
        note_resolving_transition(
            self.symbol_cold.resolved_members.get(id).is_resolving(),
            false,
        );
        self.symbol_cold.resolved_members.clear(id);
    }

    /// The resolvedExports flavor of the late-binding protocol.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_resolved_exports_late_bind(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: Arc<tsc_binder::SymbolTable>,
    ) {
        let _ = speculation_depth;
        let slot = self.symbol_cold.resolved_exports.slot(id);
        note_resolving_transition(slot.is_resolving(), false);
        *slot = LinkSlot::Resolved(value);
    }

    /// Err-unwind twin for the resolvedExports flavor.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn revert_symbol_resolved_exports(&mut self, id: SymbolId) {
        note_resolving_transition(
            self.symbol_cold.resolved_exports.get(id).is_resolving(),
            false,
        );
        self.symbol_cold.resolved_exports.clear(id);
    }

    /// tsrs-native: links accessor — resolveAlias' aliasTarget slot
    /// protocol writer (the tsc counterpart is the inline assignment
    /// family inside resolveAlias 49118-49134).
    ///
    /// The alias-target memo: written once, by the resolveAlias frame
    /// that owns the symbol's `AliasTarget` resolution (a cycle
    /// re-entry returns unknownSymbol without writing).
    pub fn set_symbol_alias_target(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<SymbolId>,
    ) {
        self.journal_alias_target(speculation_depth, id);
        Self::write_slot(self.symbol_cold.alias_target.slot(id), value);
    }

    /// tsrs-native: initialize a freshly synthesized alias symbol.
    ///
    /// The wrapper symbol and its pre-resolved target are constructed
    /// together; no pre-existing cache entry is published.
    pub fn set_fresh_symbol_alias_target(&mut self, id: SymbolId, value: LinkSlot<SymbolId>) {
        Self::write_slot(self.symbol_cold.alias_target.slot(id), value);
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
        self.symbol_cold.type_only_declaration.set(id, Some(value));
    }

    /// tsrs-native: links accessor — links.typeOnlyExportStarName
    /// (49189).
    pub fn set_symbol_type_only_export_star_name(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: EscapedName,
    ) {
        self.journal_type_only_alias(speculation_depth, id);
        self.symbol_cold
            .type_only_export_star_name
            .set(id, Some(value));
    }

    /// tsrs-native: links accessor — the MODULE flavor of
    /// resolvedExports (getExportsOfModule 49838):
    /// written together with typeOnlyExportStarMap. tsc's unguarded
    /// tail assignment tolerates a deterministic re-entrant duplicate
    /// (the worker has no sentinel), so Resolved→Resolved(equal) is
    /// accepted; a DIFFERENT table is a protocol bug.
    pub fn set_symbol_module_exports(
        &mut self,
        id: SymbolId,
        exports: Arc<tsc_binder::SymbolTable>,
        type_only_export_star_map: Option<rustc_hash::FxHashMap<EscapedName, NodeId>>,
    ) {
        // The worker owns its cycle guard and returns the completed
        // table directly. A candidate may consume that table without
        // publishing it to the shared module-symbol memo.
        let cold = &mut self.symbol_cold;
        let slot = cold.resolved_exports.slot(id);
        match &*slot {
            LinkSlot::Vacant | LinkSlot::Resolving => {
                note_resolving_transition(slot.is_resolving(), false);
                *slot = LinkSlot::Resolved(exports);
                cold.type_only_export_star_map
                    .set(id, type_only_export_star_map);
            }
            LinkSlot::Resolved(existing)
                if Arc::ptr_eq(existing, &exports) || **existing == *exports => {}
            LinkSlot::Resolved(_) => {
                panic!("module resolvedExports rewritten with a different table: {id:?}")
            }
        }
    }

    /// tsrs-native: links accessor — links.exportsChecked once-latch
    /// (checkExternalModuleExports 86445); monotone.
    pub fn set_symbol_exports_checked(&mut self, id: SymbolId) {
        self.symbol_cold.exports_checked.set(id, true);
    }

    /// tsrs-native: links accessor — links.immediateTarget
    /// (getImmediateAliasedSymbol 50097); compute-once.
    pub fn set_symbol_immediate_target(&mut self, id: SymbolId, value: Option<SymbolId>) {
        // A speculative alias query may consume the freshly computed
        // immediate target, but must not publish that memo globally.
        let slot = self.symbol_cold.immediate_target.slot(id);
        match slot {
            None => *slot = Some(value),
            Some(existing) if *existing == value => {}
            _ => panic!("immediateTarget rewritten: {slot:?} -> {value:?}"),
        }
    }

    /// tsrs-native: links accessor — links.cjsExportMerged (49697);
    /// compute-once, the same merged symbol may be re-stamped.
    pub fn set_symbol_cjs_export_merged(&mut self, id: SymbolId, value: SymbolId) {
        let slot = self.symbol_cold.cjs_export_merged.slot(id);
        match slot {
            None => *slot = Some(value),
            Some(existing) if *existing == value => {}
            _ => panic!("cjsExportMerged rewritten: {slot:?} -> {value:?}"),
        }
    }

    /// `links.inferredClassSymbol.set(getSymbolId(inferred), inferred)`
    /// (mergeJSSymbols 77538). This is a grow-only source-symbol cache;
    /// it is published with the fresh inferred semantic symbol even
    /// during speculation. The corresponding transient mutation or
    /// cloneSymbol merged-symbol redirection is likewise persistent.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_inferred_class_symbol(
        &mut self,
        speculation_depth: u32,
        source: SymbolId,
        inferred: SymbolId,
    ) {
        let _ = speculation_depth;
        let cache = self.symbol_cold.inferred_class_symbols.slot(source);
        match cache.get(&inferred).copied() {
            None => {
                cache.insert(inferred, inferred);
            }
            Some(existing) if existing == inferred => {}
            Some(existing) => {
                panic!("inferredClassSymbol rewritten for {source:?}/{inferred:?}: {existing:?}")
            }
        }
    }

    /// `links.lateSymbol = ...` (addDeclarationToLateBoundSymbol 57652)
    /// on the MEMBER's binder symbol.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_late_symbol(&mut self, speculation_depth: u32, id: SymbolId, late: SymbolId) {
        if speculation_depth != 0
            && !self
                .speculative_late_symbol_writes
                .iter()
                .any(|(depth, symbol, _)| *depth == speculation_depth && *symbol == id)
        {
            let previous = *self.symbol_cold.late_symbol.get(id);
            self.speculative_late_symbol_writes
                .push((speculation_depth, id, previous));
        }
        self.symbol_cold.late_symbol.set(id, Some(late));
    }

    /// resolveDeclaredMembers' declared-members stamp (57604-57613),
    /// written once per class/interface/tuple target.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_declared_members(&mut self, id: TypeId, value: crate::state::MembersId) {
        Self::write_slot(
            self.type_cold.declared_members.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// The tsc-mutable `type.resolvedBaseTypes` assignment (57225,
    /// 57253, 57320-57332): interfaces re-assign and push, so this
    /// setter deliberately allows overwrite.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_resolved_base_types(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: Vec<TypeId>,
    ) {
        let _ = speculation_depth;
        self.type_cold.resolved_base_types.set(id, Some(value));
    }

    /// `type.baseTypesResolved = true` (57244).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_base_types_resolved(&mut self, speculation_depth: u32, id: TypeId) {
        let _ = speculation_depth;
        self.type_cold.base_types_resolved.set(id, true);
    }

    /// tsrs-native: remove the temporary base-type publication used
    /// while a rollback-capable candidate computes a cold base list.
    pub fn clear_speculative_type_base_types(&mut self, id: TypeId) {
        self.type_cold.resolved_base_types.clear(id);
        self.type_cold.base_types_resolved.clear(id);
    }

    /// getWriteTypeOfAccessors' links.writeType stamp (56800).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_write_type(&mut self, speculation_depth: u32, id: SymbolId, value: TypeId) {
        self.journal_symbol_write_type(speculation_depth, id);
        Self::write_slot(
            self.symbol_cold.write_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: initialize the write type of a freshly synthesized
    /// property symbol.
    pub fn set_fresh_symbol_write_type(&mut self, id: SymbolId, value: TypeId) {
        Self::write_slot(
            self.symbol_cold.write_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// getResolvedMembersOrExportsOfSymbol's static-side cache (57763).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_resolved_exports(
        &mut self,
        id: SymbolId,
        value: Arc<tsc_binder::SymbolTable>,
    ) {
        Self::write_slot(
            self.symbol_cold.resolved_exports.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// getBaseConstructorTypeOfClass's resolvedBaseConstructorType
    /// stamp (57154/57186 — the ??= writes).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
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
            self.type_cold.resolved_base_constructor_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// createTupleTargetType's tupleLabelDeclaration stamp (61170).
    /// The sole caller initializes a freshly synthesized tuple member.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_symbol_tuple_label_declaration(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        declaration: NodeId,
    ) {
        let _ = speculation_depth;
        let slot = self.symbol_cold.tuple_label_declaration.slot(id);
        assert!(slot.is_none(), "tuple label written twice for {id:?}");
        *slot = Some(declaration);
    }

    /// getSingleBaseForNonAugmentingSubtype's cachedEquivalentBaseType
    /// stamp (67713), guarded by IdenticalBaseTypeCalculated.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn ty_mut_cached_equivalent_base_type(&mut self, id: TypeId, value: TypeId) {
        let slot = self.type_cold.cached_equivalent_base_type.slot(id);
        assert!(
            slot.is_none(),
            "equivalent base type written twice for {id:?}"
        );
        *slot = Some(value);
    }

    /// The Err-unwind retraction for the members slot: tsc has no
    /// failure mode here (setStructuredTypeMembers always completes),
    /// so a partially-populated table left by a CheckAbort unwind
    /// must not be observable — the slot reverts to Vacant and a later
    /// query re-resolves.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn retract_type_members(&mut self, id: TypeId) {
        let slot = &mut self.ty.slot(id).resolved_members;
        assert!(
            matches!(slot.get(), LinkSlot::Resolved(_)),
            "retract without a members write for {id:?}"
        );
        *slot = LinkSlot::Vacant.into();
    }

    /// tsrs-native: Err-unwind retraction (tsc has no failure mode
    /// here). The declared-members twin of `retract_type_members`:
    /// the 5.9c staged publication (tsc resolveDeclaredMembers fills
    /// the type in place) parks the table before the signature/index
    /// walks; a CheckAbort unwind must leave the slot Vacant, not
    /// partial.
    pub fn retract_type_declared_members(&mut self, id: TypeId) {
        assert!(
            matches!(
                self.type_cold.declared_members.get(id),
                LinkSlot::Resolved(_)
            ),
            "retract without a declared-members write for {id:?}"
        );
        self.type_cold.declared_members.clear(id);
    }

    /// tsrs-native: initialize createDeferredTypeReference's node/mapper
    /// stamp (60196-60197) once on a fresh deferred-reference shell.
    pub fn set_fresh_type_deferred_reference_links(
        &mut self,
        id: TypeId,
        node: NodeId,
        mapper: Option<MapperId>,
    ) {
        let links = self.ty.slot(id);
        assert!(
            links.deferred_node.is_none() && self.type_cold.deferred_mapper.get(id).is_none(),
            "deferred reference links written twice for {id:?}"
        );
        links.deferred_node = Some(node);
        if mapper.is_some() {
            self.type_cold.deferred_mapper.set(id, mapper);
        }
    }

    /// cloneTypeParameter/getRestrictiveTypeParameter target stamp.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_parameter_target(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        target: TypeId,
    ) {
        let _ = speculation_depth;
        let links = self.ty.slot(id);
        assert!(
            links.type_parameter_target.is_none(),
            "type parameter target written twice for {id:?}"
        );
        links.type_parameter_target = Some(target);
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_parameter_default(&mut self, id: TypeId, value: TypeId) {
        // Defaults are lazily derived from declarations (or a targeted
        // parameter) and can be recomputed after candidate speculation.
        Self::write_id_slot(
            &mut self.ty.slot(id).type_parameter_default,
            LinkSlot::Resolved(value),
        );
    }

    /// instantiateSignature's fresh-parameter mapper stamp (63418).
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_parameter_mapper(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        mapper: MapperId,
    ) {
        let _ = speculation_depth;
        let links = self.ty.slot(id);
        assert!(
            links.type_parameter_mapper.is_none(),
            "type parameter mapper written twice for {id:?}"
        );
        links.type_parameter_mapper = Some(mapper);
    }

    /// getDeclaredTypeOfTypeAlias's typeParameters stamp (57416).
    ///
    /// The list is immutable declaration shape, not candidate-local
    /// contextual state. A type alias instantiated while an overload
    /// candidate is being checked can immediately become part of a
    /// freshly allocated semantic type that survives the candidate
    /// transaction. Keep the parameter identities with that escaped
    /// type graph, just as `set_fresh_symbol_declared_type` keeps each
    /// parameter singleton. A later re-force after the surrounding
    /// declared-type cache was rolled back is an idempotent publication
    /// of the same list.
    /// tsrs-native: durable declaration-shape publication corresponding
    /// to tsc's ordinary links.typeParameters write.
    pub fn set_symbol_type_parameters(
        &mut self,
        _speculation_depth: u32,
        id: SymbolId,
        type_parameters: Vec<TypeId>,
    ) {
        let slot = self.symbol_cold.type_parameters.slot(id);
        match slot {
            Some(existing) => assert_eq!(
                *existing, type_parameters,
                "alias type parameters changed for {id:?}"
            ),
            None => *slot = Some(type_parameters),
        }
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    ///
    /// This is a durable pure memo, even while an overload candidate is
    /// speculative. `TypeId`s and their semantic inputs are immutable and
    /// append-only, so `keyof T` cannot acquire a candidate-dependent
    /// answer. Keeping the singleton also preserves tsc's object identity
    /// contract when the same index type is reached through two mappers
    /// (notably limited reverse-mapped constraints).
    pub fn set_type_resolved_index_type(
        &mut self,
        _speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::write_slot(
            self.type_cold.resolved_index_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function. Like the ordinary
    /// index-type slot above, this is a durable memo over an immutable
    /// `TypeId`, not candidate-owned semantic state.
    pub fn set_type_resolved_string_index_type(
        &mut self,
        _speculation_depth: u32,
        id: TypeId,
        value: TypeId,
    ) {
        Self::write_slot(
            self.type_cold.resolved_string_index_type.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
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
            self.type_cold.unique_literal_filled_instantiation.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
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
            self.type_cold.permissive_instantiation.slot(id),
            LinkSlot::Resolved(value),
        );
    }

    /// getRestrictiveInstantiation self-stamps its result (63825-63826),
    /// so a second write with the SAME value is tolerated.
    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
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
        let slot = self.type_cold.restrictive_instantiation.slot(id);
        match &*slot {
            LinkSlot::Resolved(existing) if *existing == value => {}
            _ => Self::write_slot(slot, LinkSlot::Resolved(value)),
        }
    }

    /// tsrs-native: Rust Links-table protocol for tsc's direct mutable
    /// links-field access; no standalone tsc function.
    pub fn set_type_members(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: LinkSlot<crate::state::MembersId>,
    ) {
        if speculation_depth != 0
            && !self
                .speculative_type_member_writes
                .contains(speculation_depth, id)
        {
            let previous = self
                .ty
                .get(id)
                .map(|links| links.resolved_members.get())
                .unwrap_or_default();
            self.speculative_type_member_writes
                .push(speculation_depth, id, previous);
        }
        let slot = &mut self.ty.slot(id).resolved_members;
        // setStructuredTypeMembers writes an empty table first as a
        // re-entrancy guard, then the real one (58333/58339) — allow
        // Resolved -> Resolved for that one tsc-shaped double write.
        match (slot.get(), &value) {
            (LinkSlot::Resolved(_), LinkSlot::Resolved(_)) => *slot = value.into(),
            _ => Self::write_id_slot(slot, value),
        }
    }

    /// tsrs-native: fresh structured-type initialization.
    ///
    /// Initialize the resolved-member payload of a freshly allocated
    /// semantic type. This is object construction, not publication of
    /// a cold cache on a pre-existing type.
    pub fn set_fresh_type_members(&mut self, id: TypeId, value: LinkSlot<crate::state::MembersId>) {
        Self::write_id_slot(&mut self.ty.slot(id).resolved_members, value);
    }
}
