//! CheckerState — the per-program checker context.
//!
//! Owns the program's binder runs (per-file arenas behind the
//! ProgramBinder view), the TypeTables, the links tables, the
//! signature arena, the diagnostics sink, the resolution stack
//! (pushTypeResolution 55728), and the global environment
//! (initializeTypeChecker slice, M4 5.0). M3 built the single-file
//! seed; M4 5.0 extends it program-wide.

use std::sync::Arc;
use tsc_binder::{Binder, InternalSymbolName, SymbolId, SymbolTable};
use tsc_diagnostics::JsStr;
use tsc_diagnostics::{Diagnostic, DiagnosticList, DiagnosticMessage, MessageChain};
use tsc_syntax::{NodeId, NodeLookup, Published, SourceFile};
use tsc_types::{
    CheckFlags, CompilerOptions, ExpandingFlags, InlineOrBox, ObjectFlags, PseudoBigInt,
    SignatureFlags, SymbolFlags, TypeData, TypeFlags, TypeId, TypeSystemPropertyName, TypeTables,
};

use crate::instantiate::MapperId;
use crate::links::{LinkSlot, LinksTables};
use crate::member_table::{FreezeMembers, FrozenMembers};
use crate::program::{ProgramBinder, ProgramSnapshot};
use crate::relate::RelationCaches;

pub use crate::diagnostic_sink::DiagnosticSink;

/// Value keys for the checker's side cache. These preserve the partitions
/// formerly identified by the D/A/N/I string prefixes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum CachedTypeKey {
    Discriminated(NodeId, TypeId),
    AssignmentReduced(TypeId, TypeId),
    Narrowed(TypeId, TypeId, u8),
    IntersectionThis(TypeId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackageJsonModuleType {
    Module,
    CommonJs,
    Other,
    Missing,
}

/// One Program file's memoized implied-node-format answers; `None` means
/// not computed yet, `Some(answer)` stores the worker's (possibly `None`)
/// result. See `CheckerState::implied_node_format_memo`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ImpliedNodeFormatMemo {
    pub(crate) for_emit: Option<Option<crate::modules::ModuleResolutionMode>>,
    pub(crate) for_file: Option<Option<crate::modules::ModuleResolutionMode>>,
}

/// The checker's failure channel, which no value inhabits: checker functions
/// keep their `CheckResult` signatures while `Err` is unrepresentable, so
/// every `?` compiles to nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckAbort {}

impl CheckAbort {
    /// tsrs-native: debug label for the Rust checker-abort channel.
    pub const fn description(self) -> &'static str {
        match self {}
    }
}

impl std::fmt::Display for CheckAbort {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {}
    }
}

pub type CheckResult<T> = Result<T, CheckAbort>;

/// A module augmentation whose target is behind resolver machinery the
/// in-memory program resolver does not model. Keep the augmentation's
/// own container symbol: its resolved member/index tables are the
/// authoritative description of what the missing merge could add.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedModuleAugmentation {
    pub module_reference: tsc_types::JsString,
    pub augmentation_file: tsc_types::JsString,
    /// Export/member path below the augmented external module. The
    /// module object itself is the empty path; `N.X` is `["N", "X"]`.
    pub container_path: Vec<tsc_types::EscapedName>,
    pub container_symbol: SymbolId,
}

tsc_types::id_type!(SignatureId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureKind {
    Call,
    Construct,
}

tsc_types::id_type!(MembersId);

/// The checker's signatures and the minimum-argument memo kept beside them:
/// the one owner of their storage, so its layout can change behind these
/// methods. A signature's identity is its position, given in creation
/// order; signatures are never removed.
#[derive(Default)]
pub struct SignatureStore {
    signatures: Vec<Signature>,
    /// tsc `signature.resolvedMinArgumentCount` (getMinArgumentCount 78287):
    /// a byte a signature, by its index - zero while not computed, else the
    /// count plus one. A signature's identity is never reused, so the byte
    /// is the signature's own; a count of 255 or more is computed at every
    /// request. The table grows to the highest signature asked.
    resolved_min_argument_counts: Vec<u8>,
}

impl SignatureStore {
    /// The signature `id`.
    #[inline(always)]
    pub fn get(&self, id: SignatureId) -> &Signature {
        &self.signatures[id.index() as usize]
    }

    /// The signature `id`, to write its memo fields.
    #[inline(always)]
    pub fn get_mut(&mut self, id: SignatureId) -> &mut Signature {
        &mut self.signatures[id.index() as usize]
    }

    /// The signatures created so far.
    pub fn len(&self) -> usize {
        self.signatures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.signatures.is_empty()
    }

    /// Every signature, in creation order.
    pub fn iter(&self) -> std::slice::Iter<'_, Signature> {
        self.signatures.iter()
    }

    /// Store `signature`; its identity.
    #[inline]
    pub(crate) fn push(&mut self, signature: Signature) -> SignatureId {
        let id = SignatureId::new(self.signatures.len() as u32);
        self.signatures.push(signature);
        id
    }

    /// Room for `additional` more signatures.
    pub(crate) fn reserve(&mut self, additional: usize) {
        self.signatures.reserve(additional);
    }

    /// The minimum argument count kept for `id`, if one was.
    #[inline]
    pub(crate) fn kept_min_argument_count(&self, id: SignatureId) -> Option<usize> {
        match self.resolved_min_argument_counts.get(id.index() as usize) {
            Some(&kept) if kept != 0 => Some(usize::from(kept) - 1),
            _ => None,
        }
    }

    /// Keep `count` as the minimum argument count of `id` (a count of 255
    /// or more is not kept).
    pub(crate) fn keep_min_argument_count(&mut self, id: SignatureId, count: usize) {
        if let Ok(kept) = u8::try_from(count + 1) {
            let index = id.index() as usize;
            if self.resolved_min_argument_counts.len() <= index {
                self.resolved_min_argument_counts.resize(index + 1, 0);
            }
            self.resolved_min_argument_counts[index] = kept;
        }
    }

    /// The bytes the minimum-argument memo holds.
    pub(crate) fn memo_bytes(&self) -> usize {
        self.resolved_min_argument_counts.capacity()
    }

    /// The memory report's row of the reserved signatures; the resident
    /// bytes of their unused tail.
    pub(crate) fn print_reserved(&self, label: &str) -> usize {
        crate::memory::print_reserved_vec(label, "signatures", &self.signatures)
    }
}

/// Common signature fields and both owned list headers stay together.
/// Computed memo fields share an optional cold owner.
#[repr(C)]
pub struct Signature {
    pub parameters: tsc_types::ThinBoxSlice<SymbolId>,
    type_parameters: tsc_types::OptionalBoxSlice<TypeId>,
    cold: tsc_types::PackedBox<SignatureMemo>,
    pub declaration: Option<NodeId>,
    pub flags: SignatureFlags,
    pub this_parameter: Option<SymbolId>,
    pub resolved_return_type: Option<TypeId>,
    pub target: Option<SignatureId>,
    pub mapper: Option<MapperId>,
}

#[derive(Debug, Default)]
struct SignatureMemo {
    composite_kind: Option<TypeFlags>,
    composite_signatures: tsc_types::OptionalBoxSlice<SignatureId>,
    predicate: Option<Box<crate::narrow::TypePredicate>>,
    optional_call_signature_cache: (Option<SignatureId>, Option<SignatureId>),
    erased_signature_cache: Option<SignatureId>,
    canonical_signature_cache: Option<SignatureId>,
    base_signature_cache: Option<SignatureId>,
    isolated_signature_type: Option<TypeId>,
}
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::size_of::<Signature>() == 48);
    assert!(std::mem::size_of::<SignatureMemo>() == 48);
};
impl Clone for SignatureMemo {
    fn clone(&self) -> Self {
        Self {
            composite_kind: self.composite_kind,
            composite_signatures: self.composite_signatures.clone(),
            predicate: None,
            optional_call_signature_cache: self.optional_call_signature_cache,
            erased_signature_cache: self.erased_signature_cache,
            canonical_signature_cache: self.canonical_signature_cache,
            base_signature_cache: self.base_signature_cache,
            isolated_signature_type: self.isolated_signature_type,
        }
    }
}
impl std::fmt::Debug for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signature")
            .field("declaration", &self.declaration)
            .field("flags", &self.flags)
            .field("type_parameters", &self.type_parameters())
            .field("parameters", &self.parameters)
            .field("this_parameter", &self.this_parameter)
            .field("min_argument_count", &self.min_argument_count())
            .field(
                "resolved_return_type",
                &self
                    .resolved_return_type
                    .map_or(LinkSlot::Vacant, LinkSlot::Resolved),
            )
            .field("from_method", &self.is_from_method())
            .field("target", &self.target)
            .field("mapper", &self.mapper)
            .field("erased_signature_cache", &self.erased_signature_cache())
            .field(
                "canonical_signature_cache",
                &self.canonical_signature_cache(),
            )
            .field("base_signature_cache", &self.base_signature_cache())
            .field("composite_kind", &self.composite_kind())
            .field("composite_signatures", &self.composite_signatures())
            .field(
                "optional_call_signature_cache",
                &self.optional_call_signature_cache(),
            )
            .field("isolated_signature_kind", &self.isolated_signature_kind())
            .field("isolated_signature_type", &self.isolated_signature_type())
            .finish()
    }
}
impl Default for Signature {
    #[inline]
    fn default() -> Self {
        Self {
            parameters: Default::default(),
            type_parameters: Default::default(),
            cold: Default::default(),
            declaration: None,
            flags: SignatureFlags::NONE,
            this_parameter: None,
            resolved_return_type: None,
            target: None,
            mapper: None,
        }
    }
}
impl Clone for Signature {
    fn clone(&self) -> Self {
        let mut result = Self {
            parameters: self.parameters.clone(),
            type_parameters: self.type_parameters.clone(),
            cold: self.cold.clone(),
            declaration: self.declaration,
            flags: self.flags,
            this_parameter: self.this_parameter,
            resolved_return_type: self.resolved_return_type,
            target: self.target,
            mapper: self.mapper,
        };
        // The predicate memo belonged to the original numeric identity.
        result.clear_predicate();
        result
    }
}
impl Signature {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    fn memo(&self) -> Option<&SignatureMemo> {
        self.cold.get()
    }
    fn memo_mut(&mut self) -> &mut SignatureMemo {
        self.cold.ensure_with(SignatureMemo::default)
    }
    fn prune_memo(&mut self) {
        if self.memo().is_some_and(|m| {
            m.composite_kind.is_none()
                && m.composite_signatures.is_none()
                && m.predicate.is_none()
                && m.optional_call_signature_cache == (None, None)
                && m.erased_signature_cache.is_none()
                && m.canonical_signature_cache.is_none()
                && m.base_signature_cache.is_none()
                && m.isolated_signature_type.is_none()
        }) {
            self.cold.clear();
        }
    }
    #[inline]
    pub fn set_parameters(&mut self, parameters: impl Into<tsc_types::ThinBoxSlice<SymbolId>>) {
        self.parameters = parameters.into();
    }
    #[inline]
    pub fn min_argument_count(&self) -> u32 {
        self.cold.count()
    }
    #[inline]
    pub fn set_min_argument_count(&mut self, value: u32) {
        self.cold.set_count(value);
    }
    #[inline]
    pub fn is_from_method(&self) -> bool {
        self.cold.tag(0)
    }
    #[inline]
    pub fn set_from_method(&mut self, value: bool) {
        self.cold.set_tag(0, value);
    }
    #[inline]
    pub fn isolated_signature_kind(&self) -> Option<SignatureKind> {
        match self.cold.kind() {
            0 => None,
            1 => Some(SignatureKind::Call),
            2 => Some(SignatureKind::Construct),
            _ => unreachable!(),
        }
    }
    #[inline]
    pub fn set_isolated_signature_kind(&mut self, value: Option<SignatureKind>) {
        self.cold.set_kind(match value {
            None => 0,
            Some(SignatureKind::Call) => 1,
            Some(SignatureKind::Construct) => 2,
        });
    }
    #[inline]
    pub fn has_type_parameters(&self) -> bool {
        self.type_parameters.is_some()
    }
    #[inline]
    pub fn has_no_type_parameters(&self) -> bool {
        self.type_parameters.is_none()
    }
    #[inline]
    pub fn type_parameters(&self) -> Option<&[TypeId]> {
        self.type_parameters.as_deref()
    }
    pub fn cloned_type_parameters(&self) -> Option<Box<[TypeId]>> {
        self.type_parameters()
            .map(|s| s.to_vec().into_boxed_slice())
    }
    pub fn take_type_parameters(&mut self) -> Option<Box<[TypeId]>> {
        self.type_parameters.take()
    }
    pub fn set_type_parameters(&mut self, value: Option<Box<[TypeId]>>) {
        self.type_parameters.set(value);
    }
    #[inline]
    pub fn composite_signatures(&self) -> Option<&[SignatureId]> {
        if !self.cold.tag(2) {
            return None;
        }
        self.memo().and_then(|m| m.composite_signatures.as_deref())
    }
    pub fn cloned_composite_signatures(&self) -> Option<Box<[SignatureId]>> {
        self.composite_signatures()
            .map(|s| s.to_vec().into_boxed_slice())
    }
    pub fn set_composite_signatures(&mut self, value: Option<Box<[SignatureId]>>) {
        if value.is_some() || self.memo().is_some() {
            self.memo_mut().composite_signatures.set(value);
            self.refresh_composite_presence();
            self.prune_memo();
        }
    }
    fn refresh_composite_presence(&mut self) {
        let present = self
            .memo()
            .is_some_and(|m| m.composite_kind.is_some() || m.composite_signatures.is_some());
        self.cold.set_tag(2, present);
    }
    #[inline]
    pub fn composite_kind(&self) -> Option<TypeFlags> {
        if !self.cold.tag(2) {
            return None;
        }
        self.memo().and_then(|m| m.composite_kind)
    }
    pub fn set_composite_kind(&mut self, value: Option<TypeFlags>) {
        if value.is_some() || self.memo().is_some() {
            self.memo_mut().composite_kind = value;
            self.refresh_composite_presence();
            self.prune_memo();
        }
    }
    #[inline]
    pub fn erased_signature_cache(&self) -> Option<SignatureId> {
        self.memo().and_then(|m| m.erased_signature_cache)
    }
    pub fn set_erased_signature_cache(&mut self, value: Option<SignatureId>) {
        if value.is_some() || self.memo().is_some() {
            self.memo_mut().erased_signature_cache = value;
            self.prune_memo();
        }
    }
    #[inline]
    pub fn canonical_signature_cache(&self) -> Option<SignatureId> {
        self.memo().and_then(|m| m.canonical_signature_cache)
    }
    pub fn set_canonical_signature_cache(&mut self, value: Option<SignatureId>) {
        if value.is_some() || self.memo().is_some() {
            self.memo_mut().canonical_signature_cache = value;
            self.prune_memo();
        }
    }
    #[inline]
    pub fn base_signature_cache(&self) -> Option<SignatureId> {
        self.memo().and_then(|m| m.base_signature_cache)
    }
    pub fn set_base_signature_cache(&mut self, value: Option<SignatureId>) {
        if value.is_some() || self.memo().is_some() {
            self.memo_mut().base_signature_cache = value;
            self.prune_memo();
        }
    }
    #[inline]
    pub fn isolated_signature_type(&self) -> Option<TypeId> {
        self.memo().and_then(|m| m.isolated_signature_type)
    }
    pub fn set_isolated_signature_type(&mut self, value: Option<TypeId>) {
        if value.is_some() || self.memo().is_some() {
            self.memo_mut().isolated_signature_type = value;
            self.prune_memo();
        }
    }
    #[inline]
    pub fn optional_call_signature_cache(&self) -> (Option<SignatureId>, Option<SignatureId>) {
        self.memo()
            .map_or((None, None), |m| m.optional_call_signature_cache)
    }
    pub fn optional_call_signature_cache_mut(
        &mut self,
    ) -> &mut (Option<SignatureId>, Option<SignatureId>) {
        &mut self.memo_mut().optional_call_signature_cache
    }
    pub fn set_optional_call_signature_cache(
        &mut self,
        value: (Option<SignatureId>, Option<SignatureId>),
    ) {
        if value != (None, None) || self.memo().is_some() {
            self.memo_mut().optional_call_signature_cache = value;
            self.prune_memo();
        }
    }
    #[inline]
    pub(crate) fn type_predicate(&self) -> Option<Option<&crate::narrow::TypePredicate>> {
        if self.cold.tag(1) {
            Some(None)
        } else {
            self.memo().and_then(|m| m.predicate.as_deref().map(Some))
        }
    }
    pub(crate) fn set_type_predicate(&mut self, value: Option<crate::narrow::TypePredicate>) {
        self.cold.set_tag(1, value.is_none());
        if value.is_some() || self.memo().is_some() {
            self.memo_mut().predicate = value.map(Box::new);
            self.prune_memo();
        }
    }
    fn clear_predicate(&mut self) {
        self.cold.set_tag(1, false);
        if let Some(m) = self.cold.get_mut() {
            m.predicate = None;
        }
        self.prune_memo();
    }
    pub(crate) fn heap_bytes(&self) -> usize {
        self.parameters.heap_bytes()
            + self.type_parameters.heap_bytes()
            + self.cold.heap_bytes()
            + self.memo().map_or(0, |m| {
                m.composite_signatures.heap_bytes()
                    + m.predicate
                        .as_ref()
                        .map_or(0, |_| std::mem::size_of::<crate::narrow::TypePredicate>())
            })
    }
    /// A new identity retains its source shape and starts with empty caches.
    pub(crate) fn without_caches(&self) -> Self {
        let mut result = Self {
            parameters: self.parameters.clone(),
            declaration: self.declaration,
            flags: self.flags,
            this_parameter: self.this_parameter,
            target: self.target,
            mapper: self.mapper,
            ..Self::default()
        };
        result.set_min_argument_count(self.min_argument_count());
        result.set_from_method(self.is_from_method());
        result.set_isolated_signature_kind(self.isolated_signature_kind());
        result.set_type_parameters(self.cloned_type_parameters());
        result.set_composite_signatures(self.cloned_composite_signatures());
        result.set_composite_kind(self.composite_kind());
        result
    }
}

/// The computed-name member declarations behind an object-literal index
/// signature (tsc IndexInfo.components), as a handle into the checker's list
/// of them ([`CheckerState::index_info_components`]): few infos have
/// components, and with a handle an [`IndexInfo`] is twenty bytes and `Copy`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexInfoComponents(std::num::NonZeroU32);

/// tsc IndexInfo (createIndexInfo 59989).
#[derive(Clone, Copy, Debug)]
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
    /// A copy of the info shares the list, which is never changed.
    pub components: Option<IndexInfoComponents>,
    /// Identity marker for tsc's enumNumberIndexInfo singleton
    /// (47276): getLiteralTypeFromProperties (62002) and
    /// inferToMappedType (69005, M6) exclude that ONE object by `!==`.
    /// The port synthesizes the info fresh per enum, so the marker
    /// carries the identity; every other construction — including
    /// tsc-visible copies (createIndexInfo clones like
    /// getIndexInfoWithReadonly / instantiateIndexInfo) — is false.
    pub is_enum_number_index_info: bool,
    /// Identity of tsc's anyBaseTypeIndexInfo singleton. Retaining an info
    /// preserves this bit; createIndexInfo paths clear it, even for an
    /// unchanged string-to-any shape. Declaration synthesis excludes only
    /// the singleton, while semantic indexing still uses it.
    pub is_any_base_type_index_info: bool,
}

const _: () = assert!(std::mem::size_of::<IndexInfo>() == 20);

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
    pub property_name: Option<tsc_types::EscapedName>,
    pub siblings: Option<Vec<TypeId>>,
    pub resolved_properties: Option<Vec<SymbolId>>,
}

pub type WideningContextId = usize;

/// An immutable list shared by its readers: a resolved type's properties,
/// signatures, or index infos. Cloning it is a reference count (the getters hand it out
/// instead of copying the list), an empty list holds no allocation, it
/// reads as a slice, and iterating it by value yields the ids, as iterating
/// a copied `Vec` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedList<T: Clone>(Option<Arc<[T]>>);

/// A resolved type's properties (see [`SharedList`]).
pub type SymbolIds = SharedList<SymbolId>;

/// A resolved type's call or construct signatures: an immutable list its
/// readers share, like a [`SharedList`]. Most such lists hold ONE signature
/// (a function type, a method), and that one is kept in the handle itself:
/// no allocation, and a copy where a longer list counts a reference. It
/// reads as a slice, and iterating it by value yields the ids.
#[derive(Clone, Debug)]
pub struct SignatureIds(SignatureList);

#[derive(Clone, Debug)]
enum SignatureList {
    /// No signature, or one.
    Small(Option<SignatureId>),
    /// Two signatures or more.
    Shared(Arc<[SignatureId]>),
}

const _: () = assert!(size_of::<SignatureIds>() == size_of::<SharedList<SignatureId>>());

impl SignatureIds {
    /// The empty list.
    #[inline]
    pub const fn new() -> Self {
        Self(SignatureList::Small(None))
    }

    /// The list of one signature.
    #[inline]
    pub const fn one(signature: SignatureId) -> Self {
        Self(SignatureList::Small(Some(signature)))
    }

    /// The list's signature when it holds exactly one.
    #[inline]
    pub fn single(&self) -> Option<SignatureId> {
        match self.0 {
            SignatureList::Small(one) => one,
            SignatureList::Shared(_) => None,
        }
    }

    /// Bytes the list owns on the heap (its counts and records), for
    /// memory accounting.
    pub fn heap_bytes(&self) -> usize {
        match &self.0 {
            SignatureList::Small(_) => 0,
            SignatureList::Shared(list) => {
                2 * size_of::<usize>() + std::mem::size_of_val::<[SignatureId]>(list)
            }
        }
    }

    pub(crate) fn heap_bytes_once(&self, usage: &mut tsc_diagnostics::MemoryUsage) -> usize {
        match &self.0 {
            SignatureList::Shared(list) if usage.claim_arc(list) => self.heap_bytes(),
            _ => 0,
        }
    }
}

impl Default for SignatureIds {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl From<&[SignatureId]> for SignatureIds {
    #[inline]
    fn from(list: &[SignatureId]) -> Self {
        Self(match *list {
            [] => SignatureList::Small(None),
            [one] => SignatureList::Small(Some(one)),
            _ => SignatureList::Shared(Arc::from(list)),
        })
    }
}

impl From<Vec<SignatureId>> for SignatureIds {
    #[inline]
    fn from(list: Vec<SignatureId>) -> Self {
        if list.len() < 2 {
            Self::from(&list[..])
        } else {
            Self(SignatureList::Shared(Arc::from(list)))
        }
    }
}

impl std::ops::Deref for SignatureIds {
    type Target = [SignatureId];

    #[inline]
    fn deref(&self) -> &[SignatureId] {
        match &self.0 {
            SignatureList::Small(None) => &[],
            SignatureList::Small(Some(one)) => std::slice::from_ref(one),
            SignatureList::Shared(list) => list,
        }
    }
}

impl PartialEq for SignatureIds {
    fn eq(&self, other: &Self) -> bool {
        self[..] == other[..]
    }
}

impl Eq for SignatureIds {}

/// The signatures of a [`SignatureIds`] by value, in order.
pub struct SignatureIdsIter {
    list: SignatureIds,
    next: usize,
}

impl Iterator for SignatureIdsIter {
    type Item = SignatureId;

    #[inline]
    fn next(&mut self) -> Option<SignatureId> {
        let signature = self.list.get(self.next).copied();
        self.next += usize::from(signature.is_some());
        signature
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.list.len() - self.next;
        (left, Some(left))
    }
}

impl ExactSizeIterator for SignatureIdsIter {}

impl IntoIterator for SignatureIds {
    type Item = SignatureId;
    type IntoIter = SignatureIdsIter;

    fn into_iter(self) -> SignatureIdsIter {
        SignatureIdsIter {
            list: self,
            next: 0,
        }
    }
}

impl<'a> IntoIterator for &'a SignatureIds {
    type Item = &'a SignatureId;
    type IntoIter = std::slice::Iter<'a, SignatureId>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A record's call or construct signatures, borrowed from it: a slice of
/// its list, or the one signature the record keeps in its own word. It
/// reads as a slice.
#[derive(Clone, Copy, Debug)]
pub enum SignatureSlice<'a> {
    /// No signature, or the record's one.
    Small(Option<SignatureId>),
    /// The record's list.
    Slice(&'a [SignatureId]),
}

impl std::ops::Deref for SignatureSlice<'_> {
    type Target = [SignatureId];

    #[inline]
    fn deref(&self) -> &[SignatureId] {
        match self {
            Self::Small(None) => &[],
            Self::Small(Some(one)) => std::slice::from_ref(one),
            Self::Slice(list) => list,
        }
    }
}

impl PartialEq for SignatureSlice<'_> {
    fn eq(&self, other: &Self) -> bool {
        self[..] == other[..]
    }
}

impl PartialEq<&[SignatureId]> for SignatureSlice<'_> {
    fn eq(&self, other: &&[SignatureId]) -> bool {
        self[..] == **other
    }
}

impl<const N: usize> PartialEq<&[SignatureId; N]> for SignatureSlice<'_> {
    fn eq(&self, other: &&[SignatureId; N]) -> bool {
        self[..] == other[..]
    }
}

/// Immutable resolved index infos; cloning this list copies only its Arc.
pub type IndexInfos = SharedList<IndexInfo>;

impl<T: Clone> SharedList<T> {
    /// The empty list.
    #[inline]
    pub const fn new() -> Self {
        Self(None)
    }

    /// Construction already checked the final length before allocating.
    #[inline]
    pub(crate) fn from_nonempty_arc(list: Arc<[T]>) -> Self {
        debug_assert!(!list.is_empty());
        Self(Some(list))
    }

    /// Bytes the list owns on the heap (its counts and records), for
    /// memory accounting.
    pub fn heap_bytes(&self) -> usize {
        self.0.as_ref().map_or(0, |list| {
            2 * std::mem::size_of::<usize>() + std::mem::size_of_val::<[T]>(list)
        })
    }

    pub(crate) fn counted_heap_bytes(&self, seen: &mut rustc_hash::FxHashSet<*const T>) -> usize {
        if self
            .0
            .as_ref()
            .is_some_and(|list| seen.insert(list.as_ptr()))
        {
            self.heap_bytes()
        } else {
            0
        }
    }
}

impl<T: Clone> Default for SharedList<T> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> From<Vec<T>> for SharedList<T> {
    #[inline]
    fn from(list: Vec<T>) -> Self {
        Self((!list.is_empty()).then(|| Arc::from(list)))
    }
}

impl<T: Clone> From<Arc<[T]>> for SharedList<T> {
    #[inline]
    fn from(list: Arc<[T]>) -> Self {
        Self((!list.is_empty()).then_some(list))
    }
}

impl<T: Clone> From<&[T]> for SharedList<T> {
    #[inline]
    fn from(list: &[T]) -> Self {
        Self((!list.is_empty()).then(|| Arc::from(list)))
    }
}

impl<T: Clone> std::ops::Deref for SharedList<T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        self.0.as_deref().unwrap_or(&[])
    }
}

/// The records of a [`SharedList`] by value, in order.
pub struct SharedListIter<T: Clone> {
    list: SharedList<T>,
    next: usize,
}

impl<T: Clone> Iterator for SharedListIter<T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        let record = self.list.get(self.next).cloned();
        self.next += usize::from(record.is_some());
        record
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.list.len() - self.next;
        (left, Some(left))
    }
}

impl<T: Clone> ExactSizeIterator for SharedListIter<T> {}

impl<T: Clone> IntoIterator for SharedList<T> {
    type Item = T;
    type IntoIter = SharedListIter<T>;

    fn into_iter(self) -> SharedListIter<T> {
        SharedListIter {
            list: self,
            next: 0,
        }
    }
}

impl<'a, T: Clone> IntoIterator for &'a SharedList<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// tsc resolved structured-type members (setStructuredTypeMembers
/// 50198): members table + named properties + signatures + index infos.
#[derive(Default)]
pub struct ResolvedMembers {
    /// Declared, instantiated, or declared plus inherited members.
    pub members: FrozenMembers,
    pub properties: SymbolIds,
    /// The signatures and index infos, which most resolved types have none
    /// of (three empty lists were 72 of a record's 152 bytes, over a million
    /// records on VS Code). Of the records that have any, seven in ten have
    /// ONE call signature and nothing else (a function type, a method): that
    /// signature, or one construct signature, is kept in the word itself
    /// ([`ExtrasKind`]); every other combination is a boxed record.
    // With no signature or index lists, the word can hold the property verdict.
    extras: InlineOrBox<MembersExtras>,
}

/// What the record's word holds when it is not a boxed [`MembersExtras`]:
/// the low two bits of its inline payload, the rest being the kind's own
/// payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExtrasKind {
    /// No list. The payload is the weak-properties verdict: 0 not asked,
    /// 1 false, 2 true.
    None = 0,
    /// One call signature and nothing else. The payload is its position.
    OneCall = 1,
    /// One construct signature and nothing else, likewise.
    OneConstruct = 2,
}

const EXTRAS_KIND_BITS: usize = 2;
const EXTRAS_KIND_MASK: usize = (1 << EXTRAS_KIND_BITS) - 1;

/// The call and construct signatures and the index infos of a resolved
/// type, allocated when the type has any.
#[derive(Clone, Debug, Default)]
pub struct MembersExtras {
    pub call_signatures: SignatureIds,
    pub construct_signatures: SignatureIds,
    pub index_infos: IndexInfos,
}

const _: () = {
    assert!(size_of::<InlineOrBox<MembersExtras>>() == size_of::<Option<Box<MembersExtras>>>());
    assert!(size_of::<ResolvedMembers>() == 48);
};

impl Clone for ResolvedMembers {
    fn clone(&self) -> Self {
        Self {
            members: self.members.clone(),
            properties: self.properties.clone(),
            // A copy carries the lists, not the verdict.
            extras: match self.inline_extras() {
                Some((ExtrasKind::None, _)) => InlineOrBox::default(),
                _ => self.extras.clone(),
            },
        }
    }
}

impl std::fmt::Debug for ResolvedMembers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedMembers")
            .field("members", &self.members)
            .field("properties", &self.properties)
            .field("call_signatures", &&*self.call_signatures())
            .field("construct_signatures", &&*self.construct_signatures())
            .field("index_infos", &self.index_infos())
            .finish()
    }
}

impl ResolvedMembers {
    /// The word's inline form: its kind and the kind's payload.
    #[inline]
    fn inline_extras(&self) -> Option<(ExtrasKind, usize)> {
        let bits = self.extras.inline_bits()?;
        let kind = match bits & EXTRAS_KIND_MASK {
            0 => ExtrasKind::None,
            1 => ExtrasKind::OneCall,
            2 => ExtrasKind::OneConstruct,
            _ => unreachable!("no such kind is written"),
        };
        Some((kind, bits >> EXTRAS_KIND_BITS))
    }

    /// The one signature the word holds inline, when it is of `kind`.
    #[inline]
    fn inline_signature(&self, kind: ExtrasKind) -> Option<SignatureId> {
        match self.extras.inline_bits() {
            Some(bits) if bits & EXTRAS_KIND_MASK == kind as usize => {
                SignatureId::from_position((bits >> EXTRAS_KIND_BITS) as u32)
            }
            _ => None,
        }
    }

    /// Whether the type has a call signature. Like the three readings
    /// below it reads the word alone unless the record is a boxed one: a
    /// property lookup asks it of every object type.
    #[inline]
    pub fn has_call_signatures(&self) -> bool {
        match self.extras.inline_bits() {
            Some(bits) => bits & EXTRAS_KIND_MASK == ExtrasKind::OneCall as usize,
            None => !self.call_signatures().is_empty(),
        }
    }

    /// Whether the type has a construct signature.
    #[inline]
    pub fn has_construct_signatures(&self) -> bool {
        match self.extras.inline_bits() {
            Some(bits) => bits & EXTRAS_KIND_MASK == ExtrasKind::OneConstruct as usize,
            None => !self.construct_signatures().is_empty(),
        }
    }

    /// Whether the type has a call or a construct signature.
    #[inline]
    pub fn has_signatures(&self) -> bool {
        match self.extras.inline_bits() {
            Some(bits) => bits & EXTRAS_KIND_MASK != ExtrasKind::None as usize,
            None => !self.call_signatures().is_empty() || !self.construct_signatures().is_empty(),
        }
    }

    /// The type's call signature when it has exactly one and no construct
    /// signature, or (`construct`) its construct signature when it has
    /// exactly one and no call signature: getSingleSignature's shape.
    #[inline]
    pub fn single_signature(&self, construct: bool) -> Option<SignatureId> {
        let kind = if construct {
            ExtrasKind::OneConstruct
        } else {
            ExtrasKind::OneCall
        };
        match self.extras.as_boxed() {
            None => self.inline_signature(kind),
            Some(extras) => {
                let (wanted, other) = if construct {
                    (&extras.construct_signatures, &extras.call_signatures)
                } else {
                    (&extras.call_signatures, &extras.construct_signatures)
                };
                if other.is_empty() {
                    wanted.single()
                } else {
                    None
                }
            }
        }
    }

    #[inline]
    fn inline_word(kind: ExtrasKind, payload: usize) -> InlineOrBox<MembersExtras> {
        InlineOrBox::inline((payload << EXTRAS_KIND_BITS) | kind as usize)
    }

    /// The word for the three lists: inline for none and for one call or
    /// one construct signature alone, a boxed record otherwise.
    fn extras_word(
        call_signatures: SignatureIds,
        construct_signatures: SignatureIds,
        index_infos: IndexInfos,
    ) -> InlineOrBox<MembersExtras> {
        if index_infos.is_empty() {
            if construct_signatures.is_empty() {
                if call_signatures.is_empty() {
                    return InlineOrBox::default();
                }
                if let Some(call) = call_signatures.single() {
                    return Self::inline_word(ExtrasKind::OneCall, call.position() as usize);
                }
            } else if call_signatures.is_empty() {
                if let Some(construct) = construct_signatures.single() {
                    return Self::inline_word(
                        ExtrasKind::OneConstruct,
                        construct.position() as usize,
                    );
                }
            }
        }
        InlineOrBox::boxed(MembersExtras {
            call_signatures,
            construct_signatures,
            index_infos,
        })
    }

    /// The three lists, taken out of the word (which is left without any).
    fn take_lists(&mut self) -> (SignatureIds, SignatureIds, IndexInfos) {
        let call = self.inline_signature(ExtrasKind::OneCall);
        let construct = self.inline_signature(ExtrasKind::OneConstruct);
        let lists = match self.extras.as_boxed_mut() {
            Some(extras) => (
                std::mem::take(&mut extras.call_signatures),
                std::mem::take(&mut extras.construct_signatures),
                std::mem::take(&mut extras.index_infos),
            ),
            None => (
                call.map_or_else(SignatureIds::new, SignatureIds::one),
                construct.map_or_else(SignatureIds::new, SignatureIds::one),
                IndexInfos::new(),
            ),
        };
        self.extras = InlineOrBox::default();
        lists
    }

    /// Whether the record has no signature and no index info: only then
    /// can its word hold the weak-properties verdict.
    #[inline]
    fn has_no_lists(&self) -> bool {
        matches!(self.inline_extras(), Some((ExtrasKind::None, _)))
    }

    pub(crate) fn weak_properties(&self) -> Option<bool> {
        match self.inline_extras() {
            Some((ExtrasKind::None, 1)) => Some(false),
            Some((ExtrasKind::None, 2)) => Some(true),
            _ => None,
        }
    }

    /// Keep the verdict in the word of a record without lists.
    fn set_weak_properties(&mut self, weak: bool) {
        debug_assert!(self.has_no_lists());
        self.extras = Self::inline_word(ExtrasKind::None, if weak { 2 } else { 1 });
    }

    fn forget_weak_properties(&mut self) {
        if self.has_no_lists() {
            self.extras = InlineOrBox::default();
        }
    }

    #[inline]
    pub fn new(
        members: impl FreezeMembers,
        properties: Vec<SymbolId>,
        call_signatures: Vec<SignatureId>,
        construct_signatures: Vec<SignatureId>,
        index_infos: impl Into<IndexInfos>,
    ) -> Self {
        let (members, properties) = members.freeze_with_properties(properties);
        Self::from_frozen(
            members,
            properties,
            call_signatures,
            construct_signatures,
            index_infos,
        )
    }

    /// Publish an existing frozen table and an already selected property list.
    #[inline]
    pub(crate) fn from_frozen(
        members: FrozenMembers,
        properties: SymbolIds,
        call_signatures: Vec<SignatureId>,
        construct_signatures: Vec<SignatureId>,
        index_infos: impl Into<IndexInfos>,
    ) -> Self {
        Self {
            members,
            properties,
            extras: Self::extras_word(
                call_signatures.into(),
                construct_signatures.into(),
                index_infos.into(),
            ),
        }
    }

    /// A later property update shares only after exact ordered equality.
    #[cfg(test)]
    pub(crate) fn set_properties(&mut self, properties: &[SymbolId]) {
        self.forget_weak_properties();
        self.properties = if properties == self.members.symbols() {
            self.members.symbol_list()
        } else {
            properties.into()
        };
    }

    /// The call signatures: read them as a slice.
    #[inline]
    pub fn call_signatures(&self) -> SignatureSlice<'_> {
        match self.extras.as_boxed() {
            Some(extras) => SignatureSlice::Slice(&extras.call_signatures),
            None => SignatureSlice::Small(self.inline_signature(ExtrasKind::OneCall)),
        }
    }

    /// The construct signatures: read them as a slice.
    #[inline]
    pub fn construct_signatures(&self) -> SignatureSlice<'_> {
        match self.extras.as_boxed() {
            Some(extras) => SignatureSlice::Slice(&extras.construct_signatures),
            None => SignatureSlice::Small(self.inline_signature(ExtrasKind::OneConstruct)),
        }
    }

    /// The call signatures, shared (the one signature of most records by
    /// value, a longer list by a reference count: not a copy).
    pub fn call_signature_list(&self) -> SignatureIds {
        match self.extras.as_boxed() {
            Some(extras) => extras.call_signatures.clone(),
            None => self
                .inline_signature(ExtrasKind::OneCall)
                .map_or_else(SignatureIds::new, SignatureIds::one),
        }
    }

    /// The construct signatures, shared likewise.
    pub fn construct_signature_list(&self) -> SignatureIds {
        match self.extras.as_boxed() {
            Some(extras) => extras.construct_signatures.clone(),
            None => self
                .inline_signature(ExtrasKind::OneConstruct)
                .map_or_else(SignatureIds::new, SignatureIds::one),
        }
    }

    pub fn index_infos(&self) -> &[IndexInfo] {
        self.extras
            .as_boxed()
            .map_or(&[], |extras| &extras.index_infos[..])
    }

    /// The immutable list, shared across recursive checker calls.
    pub fn index_info_list(&self) -> IndexInfos {
        self.extras
            .as_boxed()
            .map_or_else(IndexInfos::new, |extras| extras.index_infos.clone())
    }

    /// Replace the call signatures (tsc writes the three lists one at a
    /// time); an empty list written to a record without lists changes
    /// nothing.
    pub fn set_call_signatures(&mut self, call_signatures: Vec<SignatureId>) {
        if !call_signatures.is_empty() || !self.has_no_lists() {
            let (_, construct_signatures, index_infos) = self.take_lists();
            self.extras =
                Self::extras_word(call_signatures.into(), construct_signatures, index_infos);
        }
    }

    pub fn set_construct_signatures(&mut self, construct_signatures: Vec<SignatureId>) {
        if !construct_signatures.is_empty() || !self.has_no_lists() {
            let (call_signatures, _, index_infos) = self.take_lists();
            self.extras =
                Self::extras_word(call_signatures, construct_signatures.into(), index_infos);
        }
    }

    pub fn set_index_infos(&mut self, index_infos: Vec<IndexInfo>) {
        if !index_infos.is_empty() || !self.has_no_lists() {
            let (call_signatures, construct_signatures, _) = self.take_lists();
            self.extras =
                Self::extras_word(call_signatures, construct_signatures, index_infos.into());
        }
    }

    /// Replace the three lists.
    pub fn set_lists(
        &mut self,
        call_signatures: Vec<SignatureId>,
        construct_signatures: Vec<SignatureId>,
        index_infos: Vec<IndexInfo>,
    ) {
        self.extras = Self::extras_word(
            call_signatures.into(),
            construct_signatures.into(),
            index_infos.into(),
        );
    }

    /// Bytes the lists own on the heap, for memory accounting.
    pub fn extras_heap_bytes(&self) -> usize {
        self.extras.as_boxed().map_or(0, |extras| {
            std::mem::size_of::<MembersExtras>()
                + extras.call_signatures.heap_bytes()
                + extras.construct_signatures.heap_bytes()
                + extras.index_infos.heap_bytes()
        })
    }
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

/// A mapped-type syntax node whose semantic shell is visible while its
/// constraint is being resolved. tsc publishes that shell in the node links
/// before walking the constraint; keeping the in-flight identity here gives
/// recursive reads the same object without committing a partial link on an
/// abort.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InProgressMappedType {
    pub node: NodeId,
    pub ty: TypeId,
}

/// One deferred report (tsgo `addDeferredDiagnostic`).
pub(crate) enum DeferredDiagnostic {
    /// Generated-code collisions read the completed private/super flags
    /// when the common callback queue is produced (checker.go:10774,10808).
    WeakMapSetCollision(NodeId),
    ReflectCollision(NodeId),
    /// `reportNonexistentProperty` of an access to a missing property
    /// (checker.go:11547-11550).
    NonexistentProperty {
        prop_node: NodeId,
        containing_type: TypeId,
        is_unchecked_js: bool,
    },
    /// `reportTypeNotIterableError` of a type without iteration types
    /// (checker.go:6464-6466, 6514-6522), with the rows that become its
    /// related information.
    TypeNotIterable {
        target: crate::iterate::DeferredIterationErrorTarget,
        ty: TypeId,
        allow_async_iterables: bool,
        related: Vec<tsc_diagnostics::RelatedInfo>,
    },
}
/// The member-reset epoch and viable positions of a contextual union/name pair.
pub(crate) type ContextualRelevantPositions = (u64, Arc<[u32]>);

pub struct CheckerState<'a> {
    pub binder: ProgramBinder<'a>,
    pub options: &'a CompilerOptions,
    /// Hook-less, session-owned substrate for checker display nodes: the
    /// arena is retained between uses and wrapped into a transformation
    /// result for each print by [`Self::with_emit_display`]. Sources are
    /// mounted lazily by [`Self::emit_display_target`].
    pub(crate) emit_display: Option<tsc_emitter::TransformArena>,
    /// Whether [`Self::take_emit_display`] has lent the arena out: a display
    /// built while another is being built (an inaccessible symbol's error
    /// name) must use an arena of its own.
    pub(crate) emit_display_taken: bool,
    /// tsgo's transient `meta` member of `ImportMetaExpression`
    /// (getGlobalImportMetaExpressionType), the symbol at `meta` of
    /// `import.meta`; created on first use.
    pub(crate) import_meta_expression_meta_symbol: Option<SymbolId>,
    /// tsgo IndexInfo.indexSymbol (getApplicableIndexSymbol): the synthetic
    /// `__index` symbol of an index signature, by (object type, key type).
    pub(crate) index_symbols: rustc_hash::FxHashMap<(TypeId, TypeId), SymbolId>,
    pub(crate) emit_display_sources:
        std::collections::BTreeMap<usize, tsc_emitter::TransformSourceId>,
    /// Scratch child list shared by the per-node check traversals: one
    /// buffer per checker instead of one allocation per node with children.
    pub(crate) child_scratch: Vec<tsc_syntax::NodeId>,
    pub(crate) instantiation_type_lists: Vec<Vec<tsc_types::TypeId>>,
    pub(crate) instantiation_text_lists: Vec<Vec<tsc_types::TemplateText>>,
    pub(crate) property_scratch: Vec<crate::structural::PropertyScratch>,
    pub(crate) property_name_sets: Vec<rustc_hash::FxHashSet<tsc_types::EscapedName>>,
    pub(crate) unaffected_members: crate::unaffected_members::UnaffectedMembers,
    pub tables: TypeTables,
    /// tsc strictFunctionTypes via getStrictOptionValue.
    pub strict_function_types: bool,
    /// tsc `stableTypeOrdering` (46478): union members and property lists
    /// follow the content order of `compareTypes` instead of type ids.
    pub stable_type_ordering: bool,
    pub links: LinksTables,
    /// The signatures and their minimum-argument memo.
    pub signatures: SignatureStore,
    /// The lists an [`IndexInfoComponents`] names, by its value less one.
    index_info_components: Vec<Box<[NodeId]>>,
    /// The assignment-marking walks in progress (isPastLastAssignment's
    /// markNodeAssignments): a position read inside one is not yet final.
    pub(crate) assignment_marking_depth: u32,
    /// tsc signature.instantiations (getSignatureInstantiationWithout
    /// FillingInTypeArguments 59903): every signature's map in one table
    /// keyed by the signature and the type arguments' interned list (tsc's
    /// getTypeListId), so a signature record does not carry an (almost
    /// always empty) map.
    pub(crate) signature_instantiations:
        rustc_hash::FxHashMap<(SignatureId, tsc_types::TypeListId), SignatureId>,
    members: Vec<ResolvedMembers>,
    /// tsrs: do not shortcut a table while a reentrant member resolution may be building it.
    pub(crate) members_resolving: usize,
    pub(crate) intersection_recursion_ids:
        rustc_hash::FxHashMap<TypeId, Arc<[crate::engine::RecursionIdentity]>>,
    pub(crate) augment_filter: Option<(u64, crate::member_table::NameFilter)>,
    /// The name index of the last table published for an instantiation of
    /// these source members: the next instantiation's builder follows it and
    /// shares it when the name sequence repeats. A hint only — a table that
    /// leaves the sequence builds its own index.
    pub(crate) member_shapes: rustc_hash::FxHashMap<MembersId, crate::member_table::MemberShape>,
    pub(crate) declared_property_indexes:
        rustc_hash::FxHashMap<MembersId, crate::annotate::DeclaredPropertyIndex>,
    /// The one name index of every name sequence a published table of more
    /// than eight members has: tables that repeat a sequence share it,
    /// whichever path built them.
    pub(crate) member_indexes: crate::member_table::NameIndexes,
    /// Viable positions only; property types still depend on the contextual name type.
    pub(crate) contextual_relevant:
        rustc_hash::FxHashMap<(TypeId, tsc_types::EscapedName), ContextualRelevantPositions>,
    pub(crate) deferred_reference_members: crate::annotate::LazyMemberTables,
    pub(crate) named_member_order_buffers:
        std::cell::Cell<crate::type_order::NamedMemberOrderBuffers>,
    /// checker-key §1.5: five per-relation caches + enumRelation.
    pub relations: RelationCaches,
    /// tsc subtypeReductionCache (47000), list-id keyed.
    pub subtype_reduction_cache:
        rustc_hash::FxHashMap<tsc_types::TypeListId, Vec<tsc_types::TypeId>>,
    /// The visited map of the inference walk not in progress, kept with its
    /// table so a walk does not grow one from empty (tsgo allocates one per
    /// inferTypes call).
    pub(crate) inference_visited: crate::inference::InferenceVisited,
    pub(crate) infer_memo: crate::infer_memo::InferMemo,
    pub(crate) expression_checks: u64,
    pub(crate) impure_union_reductions: u64,
    /// Parameters whose contextual type `unknown` is published while their
    /// binding pattern is still being evaluated: assignParameterType replaces
    /// it with the pattern's type afterwards. An inference walk made inside
    /// that window can read the provisional value, so the inference memo
    /// neither stores nor reuses a walk while this is non-zero.
    pub(crate) provisional_parameter_types: u32,
    /// The inference walk's slot snapshot and its two stacks, kept between
    /// walks with their room (a walk inside a walk takes empty ones).
    pub(crate) inference_walk_buffers: crate::inference::InferenceWalkBuffers,
    /// tsc arrayVariances, shared by every array and tuple query.
    pub(crate) array_variances: SharedList<tsc_types::VarianceFlags>,
    /// greenfield §4.3: all links writes assert this is zero.
    pub speculation_depth: u32,
    /// Order-sensitivity guard (slice W2c): armed only by the sharded driver;
    /// records order-consuming operations over shard-local types.
    pub(crate) order_guard: crate::order_guard::OrderGuard,
    #[cfg(test)]
    pub(crate) speculation_commit_count: u32,
    #[cfg(test)]
    pub(crate) speculation_rollback_count: u32,
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
    /// tsgo `isInferencePartiallyBlocked`: set while the language service
    /// asks for a contextual type with the node's inferences blocked
    /// (`runWithInferenceBlockedFromSourceNode`); resolveCall reports no
    /// errors then.
    pub(crate) is_inference_partially_blocked: bool,
    /// tsgo `skipDirectInferenceNodes`: the node and its ancestors up to
    /// the containing call while inference from them is blocked.
    pub(crate) skip_direct_inference_nodes: std::collections::HashSet<NodeId>,
    /// tsgo `Checker.callResolutionStack` (checker/checker.go:798): the
    /// calls whose overloads are being chosen, innermost last.
    pub(crate) call_resolution_stack: Vec<NodeId>,
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
    /// The TypeMapper arena and the source and target lists of the array
    /// and deferred mappers — MapperId equality IS tsc's mapper object
    /// identity (findActiveMapper 73616 compares `===`).
    pub(crate) mappers: crate::instantiate::MapperStore,
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
    /// The display slice's face of the nodeBuilder's
    /// context.visitedTypes (visitAndTransformType, 51852): anonymous
    /// object types currently being rendered by typeToString's slice.
    /// tsc renders a revisit as the type-alias-for-literal reference
    /// or the `...` elision placeholder — faces outside the slice, so
    /// the walk escalates there instead of recursing unboundedly.
    /// tsc leaves the symbol-LESS entry path unguarded (createAnonymous-
    /// TypeNode's else arm calls createTypeNodeFromObjectType direct);
    /// the slice guards both — divergence is observable only on a
    /// symbol-less self-containing type, which cannot be constructed.
    pub(crate) display_visited_types: rustc_hash::FxHashSet<TypeId>,
    /// tsgo NodeBuilderContext.symbolDepth for the error display path.
    pub(crate) display_symbol_depth: rustc_hash::FxHashMap<SymbolId, u32>,
    /// nodeBuilder context.inferTypeParameters. Conditional-type
    /// rendering installs the root's infer parameters only while its
    /// extends type is serialized; the type-parameter arm uses this
    /// typed context to emit `infer T` (and a non-inferred constraint)
    /// instead of an ordinary `T` reference.
    pub(crate) display_infer_type_parameters: Vec<TypeId>,
    /// nodeBuilder context.approximateLength/maxTruncationLength/
    /// truncating for one typeToString call. These fields are parked
    /// on CheckerState because the bounded renderer is method-based;
    /// each public entry saves, initializes, and restores them so
    /// recursive rendering shares one context while reentrant root
    /// calls do not.
    pub(crate) display_approximate_length: usize,
    pub(crate) display_max_truncation_length: usize,
    /// A signatureToString render: tsgo builds its node without NoTruncation
    /// whatever `noErrorTruncation` says, so an elision is `...`.
    pub(crate) display_signature_string: bool,
    pub(crate) display_truncating: bool,
    /// nodeBuilder context.reverseMappedStack
    /// (shouldUsePlaceholderForProperty, 52221-52240). It contains the
    /// reverse-mapped properties whose types are currently being
    /// serialized, so recursive and deeply nested mapped displays
    /// terminate through tsc's elision placeholder.
    pub(crate) display_reverse_mapped_stack: Vec<SymbolId>,
    /// nodeBuilder context.flags' NoTypeReduction bit. Ordinary
    /// typeToString calls leave it false; elaborateNeverIntersection
    /// sets it so the explanation can name the original intersection.
    pub(crate) display_no_type_reduction: bool,
    /// The display slice's face of nodeBuilder
    /// context.enclosingDeclaration. getTypeNamesForErrorDisplay
    /// (50748) seeds it with the symbol's value declaration for a
    /// non-context-sensitive expression; reusable signatures/mapped
    /// nodes temporarily enter their own scope. Annotation reuse,
    /// entity-name accessibility/shortest symbol chains, and
    /// unique-symbol ancestry all consume the parked declaration.
    pub(crate) display_enclosing: Option<NodeId>,
    /// mapToTypeNodes' same-written-name retry is running: UseFullyQualifiedType
    /// added to typeToString's flags, which keep UseAliasDefinedOutsideCurrentScope
    /// (no module roots), unlike getTypeNameForErrorDisplay's retry.
    pub(crate) display_collision_retry: bool,
    /// nodeBuilder context.mapper while rendering instantiated
    /// signatures. This is deliberately separate from
    /// active_type_mappers, which is instantiateType's recursion/cache
    /// stack rather than display context. Nested signature rendering
    /// pushes another mapper and restores it on every exit.
    pub(crate) display_mappers: Vec<MapperId>,
    /// createRecoveryBoundary's `hadError` cell and the nested
    /// visitExistingNodeTreeSymbols depth for the bounded reused-node
    /// printer. A TypePredicate deliberately leaves the error armed so
    /// the nearest enclosing non-predicate TypeNode is rebuilt
    /// semantically; IndexedAccess/keyof simple-node probes bypass a
    /// child boundary for the same reason.
    pub(crate) display_reuse_had_error: bool,
    pub(crate) display_reuse_visit_depth: usize,
    /// Standard-printer indentation while an existing TypeNode clone
    /// recursively prints initializer expressions and their bodies.
    /// typeToString supplies a writer whose newline text is empty, but
    /// whose indentation remains four spaces per nesting level.
    pub(crate) display_clone_indent: usize,
    /// The standard display writer's virtual `lineStart` state at the
    /// entry of the current cloned expression/body node. Callers that
    /// materialize a line event's indentation into their local String
    /// still pass `true`: a nested decorator list's leading writeLine
    /// must not add that indentation a second time. Every recursive
    /// entry saves and restores this bit.
    pub(crate) display_clone_at_line_start: bool,
    /// tsc markerTypes (47005): ids of createMarkerType results.
    pub(crate) marker_types: rustc_hash::FxHashSet<TypeId>,
    /// tsgo varianceStack (checker/checker.go:797): the generic types
    /// whose variances are being measured, outermost first. Not empty is
    /// tsc 6.0's inVarianceComputation.
    pub(crate) variance_stack: Vec<crate::variance::VarianceStackEntry>,
    /// tsgo reliabilityFlags (checker/checker.go:742): the
    /// ReportsUnmeasurable/ReportsUnreliable bits the comparison in
    /// progress has seen. The report mappers set them on a marker type,
    /// recursiveTypeRelatedTo collects them per comparison and stores
    /// them with its result, and getVariancesWorker reads them per
    /// measured type parameter. tsc 6.0 had a chain of
    /// outofbandVarianceMarkerHandler closures that existed only during
    /// a variance measurement.
    pub(crate) reliability_flags: tsc_types::RelationComparisonResult,
    /// tsc activeTypeMappers/activeTypeMappersCaches/activeTypeMappersCount
    /// (47412-47414): the instantiation cache stack.
    pub(crate) active_type_mappers: Vec<crate::instantiate::MapperId>,
    /// Each depth owns its map until checker teardown. Inactive slots are empty.
    pub(crate) active_type_mappers_caches: Vec<ActiveMapperCache>,
    /// tsgo instantiationStack (checker.go:598): the types being
    /// instantiated, innermost last; its length is tsc's instantiationDepth.
    /// The instantiation count resets at tsc's three entry points —
    /// checkExpression, checkSourceElement, checkDeferredNode (wired at
    /// 5.4/5.5).
    pub(crate) instantiation_stack: Vec<TypeId>,
    pub(crate) instantiation_count: u64,
    /// tsc totalInstantiationCount (46450).
    pub total_instantiation_count: u64,
    /// Symbols whose class/interface declared type is mid-computation.
    /// Type parameters whose default is mid-resolution — tsc's
    /// resolvingDefaultType sentinel (getResolvedTypeParameterDefault
    /// 59049), as a set so Err unwinds leave the links slot Vacant.
    pub(crate) type_parameter_defaults_in_progress: Vec<TypeId>,
    /// Mapped type nodes whose shells have been allocated but whose
    /// constraints are still resolving. This is the transaction-safe form
    /// of tsc's eager `links.resolvedType = type` publication (62624).
    pub(crate) mapped_types_in_progress: Vec<InProgressMappedType>,

    // ---- M5 6.1: control-flow query state ----
    /// tsc flowAnalysisDisabled (69399): latched by the depth-2000
    /// limiter; every later query answers errorType.
    /// (flowInvocationCount pre-exists below — M4 wired it for the
    /// getTypeOfExpression cache gate.)
    pub(crate) flow_analysis_disabled: bool,
    /// The fast preset's history rules. Where the reference's answer depends
    /// on what a checker happened to meet first, these make it a matter of
    /// the Program alone, so a pool that deals its files by time prints one
    /// output:
    /// - a type reference's type-argument constraint check in a file this
    ///   checker is not checking and has not checked waits for that file's
    ///   own check ([`CheckerState::defers_type_argument_constraints`]);
    /// - the instantiations of a check end with it
    ///   ([`CheckerState::end_instantiation_count`]);
    /// - a widened object literal's filled-in optional property is kept by
    ///   the sibling property it stands for (`undefined_properties_by_property`);
    /// - a merged interface's checks run at its first declaration in each
    ///   file (`runs_merged_interface_checks`).
    pub(crate) canonical_history: bool,
    /// The file whose check runs now.
    pub(crate) checking_file: Option<usize>,
    /// By file: whether its tree and bind records go back to the system once
    /// its check is captured (the fast preset's leaves,
    /// `ShardFileQueue::release_leaves`); empty when nothing is released. No
    /// other file refers to such a file, and a walk over every file passes
    /// over it ([`Self::releases_file`]).
    pub(crate) released_leaves: std::sync::Arc<[bool]>,
    /// By file: the type references whose type-argument constraint check
    /// waits for the file's check.
    pub(crate) deferred_type_argument_checks: rustc_hash::FxHashMap<usize, Vec<NodeId>>,
    /// Bounded flow answers and their dependency registers; independent of
    /// speculation rollback.
    pub(crate) flow_memo: crate::flow_memo::FlowMemo,
    pub(crate) no_flow_context_nodes: crate::flow::caches::NoFlowContextNodes,
    /// Exact, bounded memo for ordered union input lists.
    pub(crate) union_front_cache: crate::union_front_cache::UnionFrontCache,
    /// tsc sharedFlowNodes/sharedFlowTypes (69395-69396), with each
    /// answer's dependencies; each query scans only its own
    /// window (sharedFlowStart..) and truncates back on exit — also on
    /// CheckAbort unwind (the unwind invariant).
    pub(crate) shared_flow: crate::flow_memo::SharedFlowState,
    /// Temporary flow overrides and switch markers, restored on candidate rollback.
    pub(crate) speculative_flow: crate::speculate::SpeculativeFlow,
    /// tsc evolvingArrayTypes (70079): elementType→evolving-array memo
    /// (tsc indexes a sparse array by elementType.id).
    pub(crate) evolving_array_types: rustc_hash::FxHashMap<TypeId, TypeId>,
    /// tsc's IsGenericTypeComputed memo (getGenericObjectFlags 62442/62448):
    /// the generic object/index flags of a type, computed once. A real
    /// program's mapped/conditional instantiation queries them constantly
    /// (29% of a Next.js check recomputed them).
    pub(crate) generic_object_flags: rustc_hash::FxHashMap<TypeId, ObjectFlags>,
    /// tsc EvolvingArrayType.finalArrayType (getFinalArrayType 70091):
    /// evolving→final memo — the arena Type is immutable once minted,
    /// so the per-type lazy slot lives here.
    pub(crate) final_array_types: rustc_hash::FxHashMap<TypeId, TypeId>,
    /// tsgo cachedTypes[CachedTypeKindRegularObjectLiteral]
    /// (getRegularTypeOfObjectLiteral): fresh object literal type → its
    /// regular twin. Object types carry no fresh/regular words; only the
    /// freshable and union records do.
    pub(crate) regular_object_literal_types: rustc_hash::FxHashMap<TypeId, TypeId>,
    /// tsgo propertiesTypes (getLiteralTypeFromProperties, checker.go:
    /// 27267-27294): the union of a type's property and index key types, by
    /// (type, include, includeOrigin).
    pub(crate) properties_types: rustc_hash::FxHashMap<(TypeId, TypeFlags, bool), TypeId>,
    /// tsc flowLoopCaches (47428): per-loop-label finalized-fixpoint
    /// memo, `getFlowNodeId(flow)` → (flow-cache-key → type). FlowIds
    /// are per-file arena indices here, so (file, FlowId) is the
    /// stable identity tsc's lazily assigned flow.id provides. Lives
    /// across queries (never trimmed), like tsc's.
    pub(crate) flow_loop_caches: crate::flow::caches::FlowIdentityCache<
        rustc_hash::FxHashMap<crate::flow::FlowCacheKey, TypeId>,
    >,
    /// tsc lastFlowNode/lastFlowNodeReachable (47401-47402): the
    /// single-entry reachability memo — the immediately previous
    /// isReachableFlowNode query (reachability is asked per statement
    /// in source order, so the repeat rate is high). (file, FlowId) is
    /// the stable identity for tsc's object equality. Invalidated by
    /// the worker's ReduceLabel arm (70312) exactly like tsc; never
    /// trimmed otherwise.
    pub(crate) last_flow_node: Option<(usize, tsc_binder::flow::FlowId)>,
    pub(crate) last_flow_node_reachable: bool,
    /// tsc flowNodeReachable (47434): the per-SHARED-node reachability
    /// memo (a getFlowNodeId-indexed sparse array there; (file, FlowId)
    /// here like flowLoopCaches). Lives across queries. Err unwinds
    /// leave it unwritten — no undecided verdict outlives its walk.
    pub(crate) flow_node_reachable: crate::flow::caches::FlowNodeCache,
    /// tsc flowNodePostSuper (47435): the per-SHARED-node memo for
    /// constructor `super()` ordering. Like flowNodeReachable, the
    /// stable key is the owning file plus its arena-local FlowId.
    pub(crate) flow_node_post_super: crate::flow::caches::FlowNodeCache,
    /// tsc withinUnreachableCode (46457): once one 7027 range is
    /// reported, elements checked INSIDE it stay silent; saved and
    /// restored by check_source_element like currentNode.
    pub(crate) within_unreachable_code: bool,
    /// tsc reportedUnreachableNodes (46458): statements already
    /// covered by an aggregated 7027 range; cleared per checked file
    /// (86985's `= void 0`).
    pub(crate) reported_unreachable_nodes: rustc_hash::FxHashSet<NodeId>,
    /// tsrs-native: number of source files whose checkSourceFileWorker body
    /// ran (not skipped by skipTypeChecking and not already TypeChecked).
    /// H2.8c evidence counter; never consulted by checking itself.
    pub(crate) checked_source_files: u32,
    /// This checker's `--generateTrace` tracer (tsgo `Checker.tracer`):
    /// its check events carry the checker's index, and its types file is
    /// written when the state ends.
    pub(crate) tracer: Option<tsc_types::tracing::CheckerTracer>,
    // ---- M4 5.4: check-driver state ----
    /// Any program file with a top-level `declare global` block
    /// tsc currentNode (46454): the element/deferred-node the driver is
    /// inside — related-info anchor for depth-limiter diagnostics
    /// (instantiateTypeWithAlias's 2589).
    pub(crate) current_node: Option<NodeId>,
    /// `TSRS_LINE_PROFILE`: per-source-line time and work of every
    /// statement-level check (see `line_profile`).
    pub(crate) line_profile: crate::line_profile::LineProfiler<'a>,
    /// Operation counters the line profile snapshots around each element
    /// (`line_profile::OP_*`); relation counts are needed only by the line and lane profiles.
    pub(crate) profile_ops: [u64; crate::line_profile::OPS],
    /// tsc NodeLinks.deferredNodes, keyed by the owning file's root
    /// node. Driver state, not memoization (like the resolution stack),
    /// so it lives here instead of the clone-on-read NodeLinks; the JS
    /// Set's visit-inserts-during-forEach semantics are reproduced by
    /// index iteration over the IndexSet.
    pub(crate) deferred_nodes: rustc_hash::FxHashMap<NodeId, indexmap::IndexSet<NodeId>>,
    /// Cleared after a file's deferred drain; a nested file check keeps its
    /// own live set in `deferred_nodes` and can take another spare.
    pub(crate) deferred_node_sets: Vec<indexmap::IndexSet<NodeId>>,

    // Deferred reports and renamed bindings survive candidate rejection;
    // these accumulators are not transient per-element stacks.
    /// tsgo `deferredDiagnosticCallbacks`: reports produced in discovery
    /// order at the file tail or global-diagnostic getter. Type display
    /// waits for resolution to finish; collisions read completed flags.
    pub(crate) deferred_diagnostics: Vec<DeferredDiagnostic>,
    /// tsgo `declarationFileLinks.aliasesMarked` (emitresolver.go:236-246):
    /// the source files whose export assignments and export specifiers
    /// have marked their targets visible for declaration emit.
    pub(crate) declaration_emit_aliases_marked: rustc_hash::FxHashSet<NodeId>,
    /// Retained syntax inputs are allocated only for declaration preparation.
    pub(crate) declaration_emit_inputs: Option<Box<crate::declaration_emit::DeclarationEmitInputs>>,
    /// The ambient module symbols of the script files, kept out of the
    /// globals merge until the global-scope augmentations are in (tsgo
    /// initializeChecker's `ambientModuleSymbols`).
    pub(crate) deferred_ambient_module_symbols: Vec<(tsc_types::EscapedName, SymbolId)>,
    pub(crate) potential_unused_renamed_binding_elements_in_types: Vec<NodeId>,
    /// tsc allPotentiallyUnusedIdentifiers, keyed by the owning source
    /// file's root. A checker visit can force a declaration in another
    /// file, so registrations cannot be attributed to the file whose
    /// worker happens to be active.
    ///
    /// The per-file entry keeps tsc's first-registration order (the
    /// registered array is drained in order), while membership is an O(1)
    /// set lookup: a Vec `contains` probe per registration was quadratic
    /// in the number of declarations of one file.
    pub(crate) potentially_unused_identifiers:
        rustc_hash::FxHashMap<NodeId, indexmap::IndexSet<NodeId, rustc_hash::FxBuildHasher>>,
    /// tsc deferredGlobalDisposableType (60882) — emptyObjectType memo
    /// on miss, like the Promise pair above.
    pub(crate) deferred_global_disposable_type: Option<TypeId>,
    /// tsc deferredGlobalAsyncDisposableType (60890).
    pub(crate) deferred_global_async_disposable_type: Option<TypeId>,
    /// tsc deferredGlobalExtractSymbol (60907): None = uncomputed;
    /// Some(Some(unknown_symbol)) = miss memo (getter filters it).
    pub(crate) deferred_global_extract_symbol: Option<Option<SymbolId>>,

    /// initializeTypeChecker's reportErrors=true getGlobalType probes,
    /// resolved eagerly at init even though our global TYPE
    /// materialization stays lazy (the documented 5.0 deviation). The
    /// lazy getters consult this memo instead of re-probing — one
    /// resolveName-with-message per name per program, like tsc.
    pub(crate) init_global_type_probes: rustc_hash::FxHashMap<&'static str, Option<SymbolId>>,
    /// tsc deferredGlobalNonNullableTypeAlias: None = uncomputed,
    /// Some(None) = miss (unknownSymbol memo), Some(Some(_)) = alias.
    pub(crate) deferred_global_non_nullable_type_alias: Option<Option<SymbolId>>,
    /// tsc deferredGlobalRecordSymbol (61016): the `in`-narrowing
    /// unknown-widening alias, memoized like NonNullable above.
    pub(crate) deferred_global_record_symbol: Option<Option<SymbolId>>,
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
    pub(crate) undefined_properties: rustc_hash::FxHashMap<tsc_types::EscapedName, SymbolId>,
    /// The same cache under the preset's history ([`Self::canonical_history`]):
    /// by the sibling property the filled-in property stands for. The
    /// reference's entry carries the declarations of the first property of
    /// that NAME the checker ever widened, so the property sorts, and is
    /// printed, at a declaration that depends on the checker's earlier files.
    pub(crate) undefined_properties_by_property: rustc_hash::FxHashMap<SymbolId, SymbolId>,

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
    /// Contextual types and inference contexts have independent scopes.
    pub(crate) contextual_types: crate::contextual::ContextualTypeStack,
    pub(crate) inference_contexts: Vec<crate::contextual::InferenceContextEntry>,
    /// M6 7.1: the InferenceContext arena — InferenceContextId
    /// equality IS tsc's context object identity (contexts are GC
    /// objects there). E-class speculation state like `mappers`:
    /// logical ids are append-only, never truncated, and context MUTATIONS
    /// deliberately survive failed candidate trials — chooseOverload
    /// reuses the SAME context across its NORMAL-mode re-run
    /// (76842-76844), so trial-time accumulation is tsc semantics,
    /// not rollback debt. Physical slots are reusable only after the complete
    /// creator returns and no stored mapper retains the context.
    pub(crate) inference_context_arena:
        crate::inference_storage::InferencePool<crate::inference::InferenceContext>,
    /// tsc InferenceInfo objects (68300): infos are GC objects with
    /// IDENTITY in tsc — context slots, thunk captures, and 7.4's
    /// detached higher-order arrays all share/replace the same
    /// objects, so the port gives them their own E-class arena
    /// (append-only, same discipline as the context arena) and
    /// contexts hold `InferenceInfoId` slots.
    pub(crate) inference_info_arena:
        crate::inference_storage::InferencePool<crate::inference::InferenceInfo>,
    pub(crate) inference_lifetimes: crate::inference_storage::InferenceLifetimes,
    /// tsc reverseHomomorphicMappedCache / reverseMappedCache
    /// (46991-46992). The value's None is the stored JS `undefined`
    /// verdict and is distinct from an absent key.
    pub(crate) reverse_homomorphic_mapped_cache:
        rustc_hash::FxHashMap<(TypeId, TypeId, TypeId), Option<TypeId>>,
    pub(crate) reverse_mapped_cache:
        rustc_hash::FxHashMap<(TypeId, TypeId, TypeId), Option<TypeId>>,
    /// tsc reverseMappedSourceStack / reverseMappedTargetStack /
    /// reverseExpandingFlags (46993-46995).
    pub(crate) reverse_mapped_source_stack: Vec<TypeId>,
    pub(crate) reverse_mapped_target_stack: Vec<TypeId>,
    pub(crate) reverse_expanding_flags: ExpandingFlags,
    /// tsc cachedTypes (47415), with value keys for its disjoint prefixes.
    pub(crate) cached_types: rustc_hash::FxHashMap<CachedTypeKey, TypeId>,

    // ---- M5 flow state ----
    /// tsc flowLoopStart (47396): the loop-label fixpoint stack cursor;
    /// checkExpressionCached (80580) save-resets it around uncached
    /// checks so a nested check cannot see an outer in-progress
    /// fixpoint's partial unions.
    pub(crate) flow_loop_start: u32,
    /// tsc flowLoopNodes/flowLoopKeys/flowLoopTypes (47429-47431) +
    /// flowLoopCount (47397): the in-progress fixpoint stack, one entry per
    /// loop label currently resolving a back-edge. flowLoopCount ≡
    /// `flow_loop_stack.len()` (tsc's arrays are never trimmed; the
    /// count is the logical length — Vec push/pop is the same shape).
    pub(crate) flow_loop_stack: Vec<crate::flow::FlowLoopEntry>,
    /// tsc flowTypeCache (47403): getTypeOfExpression's TypeCached
    /// side table — live since 6.3 (the loop-label back-edge walk
    /// clears it; getTypeOfExpression writes it once
    /// flowInvocationCount moves).
    pub(crate) flow_type_cache: Option<rustc_hash::FxHashMap<NodeId, TypeId>>,
    /// tsc flowInvocationCount (47400).
    pub(crate) flow_invocation_count: u32,
    /// tsc inlineLevel (46453): the const-variable guard-inlining
    /// recursion depth of narrowType's Identifier arm (checker-scope
    /// in tsc, NOT per-query — a nested flow query inherits the
    /// in-progress depth).
    pub(crate) inline_level: u32,
    /// tsc links.switchTypes (getSwitchClauseTypes 69938) — state-side
    /// (links writes are speculation-guarded; this cache is a stable
    /// per-switch type list, written only on full success).
    pub(crate) switch_types_cache: rustc_hash::FxHashMap<NodeId, Vec<TypeId>>,
    /// tsc links.isExhaustive settled verdicts (isExhaustiveSwitchStatement
    /// 78920) — state-side for the same reason.
    pub(crate) exhaustive_switch_cache: rustc_hash::FxHashMap<NodeId, bool>,
    /// tsc links.effectsSignature (getEffectsSignature 70195), where the
    /// node HAS an effects signature. The memoized unknownSignature verdict
    /// (nearly every call's) is the node's
    /// `NodeCheckFlags::NO_EFFECTS_SIGNATURE` bit: the flow walk asks at
    /// every call it passes, and the bit is read from the links record the
    /// call's resolved signature already lies in.
    pub(crate) effects_signature_cache: rustc_hash::FxHashMap<NodeId, SignatureId>,
    // ---- M4 5.0: the diags sink ----
    /// tsc `diagnostics` (createDiagnosticCollection) — the semantic
    /// sink; the driver (5.4) drains it per program.
    /// Experimental API: Vec access preserves order; arbitrary mutation
    /// invalidates the lookup index. Assign a Vec with `.into()`, or extract
    /// it with `.into_vec()`. Higher-level check results still expose Vecs.
    pub diagnostics: DiagnosticSink,
    /// File-less diagnostics that tsc adds after its
    /// previousGlobalDiagnostics snapshot and therefore exposes from
    /// getSemanticDiagnostics. Lazy initialization diagnostics not
    /// registered here remain program-global.
    pub visible_global_diagnostics: DiagnosticList,
    /// Syntax ranges whose check stopped at a typed abort containment
    /// boundary or an unimplemented flow-sensitive diagnostic. Only
    /// directives targeting one of these (pos, end) ranges are exempt
    /// from unused @ts-expect-error diagnostics — the
    /// preceding-directive-only rule (the mapped-type
    /// blanket-exemption pin).
    pub(crate) partially_checked_ranges: rustc_hash::FxHashMap<usize, Vec<(u32, u32)>>,
    /// tsrs-native: call-like nodes whose getResolvedSignature frame
    /// unwound with CheckAbort and left the resolved_signature slot
    /// Vacant. check_deferred_node's containment skip keys on this to
    /// tell a containment-reverted Vacant from the benign mid-fixpoint
    /// clear (tsc 77505's `: cached` on a loop-dirty fresh frame),
    /// which is fully re-resolvable and must keep its deferred checks.
    /// A stale entry whose slot later resolves is inert (the skip
    /// requires the slot to still be Vacant).
    pub(crate) contained_call_resolutions: rustc_hash::FxHashSet<NodeId>,
    /// Public audit records for explicit partial-model containment
    /// events. Unlike the byte ranges above, these use
    /// diagnostic-compatible UTF-16 coordinates. Oracle-crash aborts
    /// are range-only and deliberately do not create these records.
    pub(crate) partial_check_records: Vec<crate::PartialCheck>,
    // ---- M4 5.0: the global environment (initializeTypeChecker) ----
    /// tsc `globals` (46488): non-module file locals merged in program
    /// order.
    /// The merged global scope. Shared through an `Arc` because
    /// globalThis's exports ARE this table (initializeTypeChecker 46492)
    /// and scope walks read it while the checker keeps resolving: readers
    /// clone the handle, and the initialization-time merge writes copy on
    /// write only while a reader still holds one.
    pub globals: std::sync::Arc<SymbolTable>,
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
    /// tsc unresolvedSymbols (47008): full entity-name path to the
    /// synthetic TypeAlias symbol used to preserve a missing written
    /// name after resolveEntityName has reported it.
    pub(crate) unresolved_symbols: rustc_hash::FxHashMap<String, SymbolId>,
    /// tsc errorTypes (47009): alias identity (symbol + written type
    /// arguments) to the per-reference error intrinsic. These remain
    /// Any-like semantically while typeToString prints the alias face.
    pub(crate) error_types: rustc_hash::FxHashMap<String, TypeId>,
    /// tsc patternAmbientModules (initializeTypeChecker 88754-88756).
    pub pattern_ambient_modules: Vec<(tsc_types::JsString, tsc_types::JsString, SymbolId)>,
    /// tsc patternAmbientModuleAugmentations (mergeModuleAugmentation
    /// 47865): augmentation name → the unidirectionally-merged symbol.
    pub pattern_ambient_module_augmentations: rustc_hash::FxHashMap<tsc_types::JsString, SymbolId>,
    /// tsgo patternAmbientModuleAugmentationTargets (checker.go:773):
    /// augmentation name → the merged pattern module it augments.
    pub pattern_ambient_module_augmentation_targets:
        rustc_hash::FxHashMap<tsc_types::JsString, SymbolId>,
    /// tsgo moduleImportAttributesTypes: a module symbol's import
    /// attributes type (getTypeOfModuleImportAttributes).
    pub(crate) module_import_attributes_types: rustc_hash::FxHashMap<SymbolId, TypeId>,
    /// tsgo resolvingExplicitTypeOfSymbol: the symbols whose explicit type
    /// getExplicitTypeOfSymbol is resolving.
    pub(crate) resolving_explicit_type_of_symbol: rustc_hash::FxHashSet<SymbolId>,
    /// tsgo conditionalConstraintDepth: the conditional constraints
    /// computeBaseConstraint is resolving.
    pub(crate) conditional_constraint_depth: u32,
    /// Module augmentations whose targets sat in the resolver's
    /// Suppressed band (node_modules/baseUrl machinery). Receiver
    /// provenance plus the augmentation container's own resolved
    /// members/index infos scope downstream property-miss containment.
    pub unresolved_module_augmentations:
        rustc_hash::FxHashMap<Vec<tsc_types::EscapedName>, Vec<UnresolvedModuleAugmentation>>,
    /// Nearest visible node_modules package root for one augmentation
    /// source and package name. Package discovery is host-wide, so cache
    /// it outside the property-miss hot path after the first lookup.
    pub(crate) unresolved_package_root_cache: std::cell::RefCell<
        rustc_hash::FxHashMap<
            (tsc_types::JsString, tsc_types::JsString),
            Option<tsc_types::JsString>,
        >,
    >,
    /// Per-Program-file memo of getImpliedNodeFormatForFile /
    /// getImpliedNodeFormatForEmit results. Both are pure functions of the
    /// file name, the installed authoritative metadata, the package-scope
    /// host tables and the options — all fixed before checking starts. The
    /// memo is filled on first use per file and cleared when the
    /// authoritative provider is installed, so it never observes a stale
    /// table. Indexed by Program file index; shorter than `file_count()`
    /// until first use.
    pub(crate) implied_node_format_memo: std::cell::RefCell<Vec<ImpliedNodeFormatMemo>>,
    /// H0 production-only exact module provider. Its source identities are
    /// caller tokens. The borrowed projection maps them to binder indexes
    /// after library filtering and shadowing.
    pub(crate) authoritative_module_provider: Option<&'a dyn crate::AuthoritativeModuleProvider>,
    pub(super) authoritative_sources: &'a crate::AuthoritativeSources,
    /// First fail-closed host-table failure. This is intentionally separate
    /// from CheckAbort: augmentation recovery may contain an oracle crash,
    /// but it must never turn an incomplete authoritative table into success.
    pub(crate) authoritative_module_failure: std::cell::OnceCell<crate::AuthoritativeModuleFailure>,
    /// The provider's resolved answers to this checker's last requests
    /// ([`crate::modules::ResolvedModuleCache`]).
    pub(crate) resolved_module_cache: std::cell::RefCell<crate::modules::ResolvedModuleCache>,
    /// Two nodes that the last search for an enclosing import-equals
    /// declaration found to lie outside every one (its innermost block and
    /// its outermost block or root): a later search ends at either
    /// ([`CheckerState::is_part_of_import_equals_module_reference`]).
    pub(crate) outside_import_equals: std::cell::Cell<(Option<NodeId>, Option<NodeId>)>,
    /// The same two kept nodes for the search of an enclosing interface,
    /// type alias or type literal
    /// ([`CheckerState::is_in_ambient_or_type_node`]).
    pub(crate) outside_type_declarations: std::cell::Cell<(Option<NodeId>, Option<NodeId>)>,
    /// tsgo's checkExternalModuleNameInGlobalScope resolves module names the
    /// program does not collect (an import misplaced in a block). While it
    /// does, a name missing from the authoritative table is unresolved
    /// (TS2307), as tsgo's `GetResolvedModule` answers nil, rather than an
    /// authority failure.
    pub(crate) module_name_outside_program: std::cell::Cell<bool>,
    /// Exact input text for host reads, including host-only package manifests.
    /// Do not reconstruct a readFile result from parsed JSON or source membership.
    /// tsrs-native (M4 5.8d): its keys are EVERY host input path (normalized),
    /// including files the program layer drops (.json bodies, .js without
    /// allowJs) — the resolver's suppression probes read them through
    /// [`CheckerState::host_file_paths`] and [`CheckerState::is_host_file_path`]
    /// to decide whether a miss is tsc-undecidable (FP=0 rule).
    pub(crate) host_input_snapshots: std::sync::Arc<
        rustc_hash::FxHashMap<tsc_types::JsString, std::sync::Arc<tsc_diagnostics::TextSnapshot>>,
    >,
    /// The checker's module-specifier host view over its files, built on
    /// the first specifier lookup and shared by the rest (the file set and
    /// host inputs are fixed once the state is set up); see
    /// [`CheckerState::module_specifier_host`].
    pub(crate) basic_module_specifier_host:
        std::cell::OnceCell<std::sync::Arc<crate::node_builder::chains::BasicModuleSpecifierHost>>,
    /// tsrs-native: the harness ProgramJson `cwd` (tsc
    /// host.getCurrentDirectory). The oracle host absolutizes every
    /// program fileName against it (program-host.mjs
    /// absoluteProgramFileName), so DISPLAY-side path rendering —
    /// getSpecifierForModuleSymbol's source-file specifiers and
    /// getFullyQualifiedName's quoted module names — must root
    /// relative file names here, not at `/`. Resolution-side
    /// normalization stays "/"-rooted: all internal paths share that
    /// base, and the two worlds never compare paths with each other.
    pub host_current_directory: tsc_types::JsString,
    /// Normalized package.json path → its four-way `"type"` evidence.
    /// Explicit `module`, `commonjs`, another string, and a missing
    /// value remain distinct: implied-format consumers collapse only
    /// the states their tsc worker collapses, while
    /// createModeMismatchDetails distinguishes a missing value from
    /// any explicit value.
    pub(crate) host_package_json_module_types:
        std::sync::Arc<rustc_hash::FxHashMap<tsc_types::JsString, PackageJsonModuleType>>,
    /// Normalized package.json path → parsed host JSON. The
    /// resolver seam reads `exports`/`imports` targets and must retain
    /// object insertion order because Node condition objects are
    /// first-match, not unordered maps.
    pub(crate) host_package_json_values: std::sync::Arc<
        rustc_hash::FxHashMap<tsc_types::JsString, std::sync::Arc<tsc_program::JsonValue>>,
    >,
    /// Normalized package.json path → its non-empty `"name"` field.
    /// Bare self-name imports are undecidable only inside a matching
    /// package scope; an unrelated package.json must not hide 2307.
    pub host_package_json_names:
        std::sync::Arc<rustc_hash::FxHashMap<tsc_types::JsString, tsc_types::JsString>>,
    /// checkExternalEmitHelpers' per-source resolveHelpersModule memo.
    /// `None` is a cached missing or provenance-suppressed `tslib`;
    /// the first definite miss has already emitted 2354.
    pub(crate) external_helpers_modules: rustc_hash::FxHashMap<NodeId, Option<SymbolId>>,
    /// tsgo's per-file `requestedExternalEmitHelpers` (sourceFileLinks),
    /// keyed by the source file's root node.
    pub(crate) requested_external_emit_helpers: rustc_hash::FxHashMap<NodeId, u32>,
    /// tsgo's `TypeParameter.distributedType` (checker.go:23455-23462):
    /// the distributed form of each type parameter, created once. It is a
    /// field of the type in tsgo, so speculation never discards it.
    pub(crate) distributed_type_parameters: rustc_hash::FxHashMap<TypeId, TypeId>,
    /// Per-source getJsxNamespaceContainerForImplicitImport cache.
    /// `Some(None)` records an attempted miss so repeated JSX nodes do
    /// not duplicate the runtime-module diagnostic.
    pub(crate) jsx_implicit_import_containers: rustc_hash::FxHashMap<usize, Option<SymbolId>>,
    /// tsgo getCombinedNodeFlagsCached: the last tree-stable node and result.
    pub(crate) combined_node_flags_cache: std::cell::Cell<Option<(NodeId, tsc_types::NodeFlags)>>,
    /// Persistent symbols with at most one declaration that were last found
    /// not deprecated, a slot holding the symbol's index plus one (zero for
    /// an empty slot): [`CheckerState::is_deprecated_symbol`] asks it first.
    pub(crate) not_deprecated_front: Box<[std::cell::Cell<u32>]>,
    /// Persistent class members last found neither private nor named by a
    /// private identifier, kept as `not_deprecated_front` keeps its
    /// symbols: [`CheckerState::mark_property_as_referenced`] asks it first.
    pub(crate) not_private_front: Box<[std::cell::Cell<u32>]>,
    /// Persistent symbols last found not to be static private identifier
    /// properties, kept the same way
    /// ([`CheckerState::is_static_private_identifier_property`]).
    pub(crate) not_static_private_front: Box<[std::cell::Cell<u32>]>,
    /// The declaration modifier flags of the persistent symbols last
    /// asked: two cells a slot (the read side, the write side), each a
    /// symbol's index plus one and its flags
    /// ([`CheckerState::get_declaration_modifier_flags_from_symbol_write`]).
    pub(crate) declaration_modifier_front: Box<[std::cell::Cell<(u32, i32)>]>,
    /// The syntactic container of a starting parent in this published program.
    pub(crate) control_flow_containers: [crate::expr::ControlFlowContainerSlot; 16],
    /// tsc `node.jsDoc.jsDocCache` (getJSDocTagsWorker): effective,
    /// ownership-filtered tags are syntax-stable and computed once per
    /// host. The checker keeps the cache outside the immutable arena.
    pub(crate) jsdoc_tag_cache: std::cell::RefCell<rustc_hash::FxHashMap<NodeId, Vec<NodeId>>>,
    /// Lazy getGlobal*Type memos (deferredGlobal* pattern 60679 for the
    /// deferred ones; the core init block 88788+ is deliberately LAZY
    /// here — m4-checker-skeleton-steps.md 5.0 — so each global starts
    /// resolving the moment 5.1's declared types exist).
    pub(crate) global_type_memos: crate::globals::GlobalTypeMemos,
    /// tsc decoratorContextOverrideTypeCache (78504): the per-shape
    /// `{name, private, static}` anonymous-type intern keyed by
    /// `{p|P}{s|S}{nameType.id}`.
    pub(crate) decorator_context_override_type_cache: rustc_hash::FxHashMap<String, TypeId>,

    // ---- M6 7.5: the parked RelationFrame loan ----
    /// tsrs-native home of tsc's isRelatedToWorker closure frame
    /// (engine.rs RelationFrame/RelationFrameLoan): the B8 generic
    /// arm parks its loan here around instantiateSignatureInContextOf
    /// so the constraint clamp — including re-entrant resolutions
    /// through the non-fixing mapper's deferred thunks — compares
    /// under the live frame. None outside that window.
    pub(crate) relation_frame_loan: crate::engine::RelationFrameLoan,

    // ---- M4 5.0: the resolution stack (pushTypeResolution 55728) ----
    pub(crate) resolution_targets: Vec<ResolutionTarget>,
    pub(crate) resolution_results: Vec<bool>,
    pub(crate) resolution_property_names: Vec<TypeSystemPropertyName>,
    /// Serials parallel to the live resolution_targets prefix. Push/pop
    /// trim any tail left by speculation's existing stack restoration.
    pub(crate) resolution_serials: Vec<u32>,
    /// Bumped during speculative/independent passes so cycles don't
    /// leak across them (checker-foundations §1.2).
    pub resolution_start: usize,

    // ---- M4 5.0: merge bookkeeping ----
    /// tsc mergedSymbols (recordMergedSymbol 47689): source →
    /// merge-target, per checker. A side map — NOT a symbol field —
    /// so shared (cached) lib binders stay immutable across programs.
    pub(crate) merged_symbols: rustc_hash::FxHashMap<SymbolId, SymbolId>,
    /// One bit per symbol (by its dense index): whether `merged_symbols`
    /// has it. tsgo tests `symbol.MergeId != 0` before its map; here the
    /// map is asked only for the few symbols that were merged.
    pub(crate) merged_symbol_bits: Vec<u64>,
    /// The relation checker's work lists, kept between calls instead of
    /// allocated per `checkTypeRelatedTo` (tsrs recycles them likewise).
    #[allow(clippy::vec_box)] // Active walks and frame loans transfer individual boxes.
    pub(crate) relation_work_pool: Vec<Box<crate::engine::RelationWork>>,
    /// Resolved conditional branches in front of their links (see
    /// `conditional::ConditionalBranchSlot`).
    pub(crate) conditional_branches: Box<[crate::conditional::ConditionalBranchSlot]>,
    /// The reverse index (target → sources) for the stage-3.4c
    /// expando-record consults: the record lives on per-file binder
    /// symbols, and amalgamated-duplicate merging clones them into
    /// fresh program symbols, so a merged symbol consults its merge
    /// sources (tsc needs no equivalent — it binds expando members
    /// into the symbol table itself).
    pub(crate) merged_symbol_sources: rustc_hash::FxHashMap<SymbolId, Vec<SymbolId>>,
}

impl<'a> CheckerState<'a> {
    /// Publish no borrowed view to the detached cleanup worker. The driver has
    /// already copied the order reasons, marks, diagnostics and work counters.
    pub(crate) fn defer_finished_storage(&mut self) {
        let storage = (
            self.tables.take_finished_storage(),
            self.binder.take_finished_storage(),
            self.infer_memo.take_finished_storage(),
            self.flow_memo.take_finished_storage(),
            std::mem::take(&mut self.index_symbols),
            std::mem::take(&mut self.emit_display_sources),
            std::mem::take(&mut self.child_scratch),
            std::mem::take(&mut self.instantiation_type_lists),
            std::mem::take(&mut self.instantiation_text_lists),
            std::mem::take(&mut self.property_scratch),
            std::mem::take(&mut self.property_name_sets),
            std::mem::take(&mut self.unaffected_members),
            std::mem::take(&mut self.links),
            std::mem::take(&mut self.signatures),
            std::mem::take(&mut self.signature_instantiations),
            std::mem::take(&mut self.members),
            std::mem::take(&mut self.intersection_recursion_ids),
            std::mem::take(&mut self.contextual_relevant),
            std::mem::take(&mut self.deferred_reference_members),
            std::mem::take(&mut self.named_member_order_buffers),
            std::mem::take(&mut self.relations),
            std::mem::take(&mut self.subtype_reduction_cache),
            std::mem::take(&mut self.inference_visited),
            std::mem::take(&mut self.inference_walk_buffers),
            std::mem::take(&mut self.skip_direct_inference_nodes),
            std::mem::take(&mut self.call_resolution_stack),
            std::mem::take(&mut self.mappers),
            std::mem::take(&mut self.display_visited_types),
            std::mem::take(&mut self.display_symbol_depth),
            std::mem::take(&mut self.display_infer_type_parameters),
            std::mem::take(&mut self.display_reverse_mapped_stack),
            std::mem::take(&mut self.display_mappers),
            std::mem::take(&mut self.marker_types),
            std::mem::take(&mut self.variance_stack),
            std::mem::take(&mut self.active_type_mappers),
            std::mem::take(&mut self.active_type_mappers_caches),
            std::mem::take(&mut self.instantiation_stack),
            std::mem::take(&mut self.type_parameter_defaults_in_progress),
            std::mem::take(&mut self.mapped_types_in_progress),
            std::mem::take(&mut self.shared_flow),
            std::mem::take(&mut self.speculative_flow),
            std::mem::take(&mut self.evolving_array_types),
            std::mem::take(&mut self.generic_object_flags),
            std::mem::take(&mut self.final_array_types),
            std::mem::take(&mut self.regular_object_literal_types),
            std::mem::take(&mut self.properties_types),
            std::mem::take(&mut self.flow_loop_caches),
            std::mem::take(&mut self.flow_node_reachable),
            std::mem::take(&mut self.flow_node_post_super),
            std::mem::take(&mut self.reported_unreachable_nodes),
            std::mem::take(&mut self.deferred_nodes),
            std::mem::take(&mut self.deferred_node_sets),
            std::mem::take(&mut self.deferred_diagnostics),
            std::mem::take(&mut self.declaration_emit_aliases_marked),
            std::mem::take(&mut self.deferred_ambient_module_symbols),
            std::mem::take(&mut self.potential_unused_renamed_binding_elements_in_types),
            std::mem::take(&mut self.potentially_unused_identifiers),
            std::mem::take(&mut self.init_global_type_probes),
            std::mem::take(&mut self.awaited_type_stack),
            std::mem::take(&mut self.widening_contexts),
            std::mem::take(&mut self.undefined_properties),
            std::mem::take(&mut self.undefined_properties_by_property),
            std::mem::take(&mut self.contextual_binding_patterns),
            std::mem::take(&mut self.contextual_types),
            std::mem::take(&mut self.inference_contexts),
            std::mem::take(&mut self.inference_context_arena),
            std::mem::take(&mut self.inference_info_arena),
            std::mem::take(&mut self.reverse_homomorphic_mapped_cache),
            std::mem::take(&mut self.reverse_mapped_cache),
            std::mem::take(&mut self.reverse_mapped_source_stack),
            std::mem::take(&mut self.reverse_mapped_target_stack),
            std::mem::take(&mut self.cached_types),
            std::mem::take(&mut self.flow_loop_stack),
            std::mem::take(&mut self.flow_type_cache),
            std::mem::take(&mut self.switch_types_cache),
            std::mem::take(&mut self.exhaustive_switch_cache),
            std::mem::take(&mut self.effects_signature_cache),
            std::mem::take(&mut self.partially_checked_ranges),
            std::mem::take(&mut self.contained_call_resolutions),
            std::mem::take(&mut self.partial_check_records),
            std::mem::take(&mut self.unresolved_symbols),
            std::mem::take(&mut self.error_types),
            std::mem::take(&mut self.pattern_ambient_modules),
            std::mem::take(&mut self.pattern_ambient_module_augmentations),
            std::mem::take(&mut self.pattern_ambient_module_augmentation_targets),
            std::mem::take(&mut self.module_import_attributes_types),
            std::mem::take(&mut self.resolving_explicit_type_of_symbol),
            std::mem::take(&mut self.unresolved_module_augmentations),
            std::mem::take(&mut self.unresolved_package_root_cache),
            std::mem::take(&mut self.implied_node_format_memo),
            std::mem::take(&mut self.external_helpers_modules),
            std::mem::take(&mut self.requested_external_emit_helpers),
            std::mem::take(&mut self.distributed_type_parameters),
            std::mem::take(&mut self.jsx_implicit_import_containers),
            std::mem::take(&mut self.jsdoc_tag_cache),
            std::mem::take(&mut self.global_type_memos),
            std::mem::take(&mut self.decorator_context_override_type_cache),
            std::mem::take(&mut self.resolution_targets),
            std::mem::take(&mut self.resolution_results),
            std::mem::take(&mut self.resolution_property_names),
            std::mem::take(&mut self.resolution_serials),
            std::mem::take(&mut self.merged_symbols),
            std::mem::take(&mut self.merged_symbol_bits),
            std::mem::take(&mut self.merged_symbol_sources),
            std::mem::take(&mut self.array_variances),
            std::mem::take(&mut self.augment_filter),
            std::mem::take(&mut self.union_front_cache),
            std::mem::take(&mut self.diagnostics),
            std::mem::take(&mut self.visible_global_diagnostics),
            std::mem::take(&mut self.globals),
            std::mem::take(&mut self.relation_work_pool),
            std::mem::take(&mut self.conditional_branches),
            std::mem::take(&mut self.order_guard),
        );
        crate::finished_storage::defer(storage);
    }

    /// Return scalar lookup buffers after the final owned shard result has
    /// been published. The state keeps its records and identity leases.
    pub(crate) fn release_finished_lookup_buffers(&mut self) {
        fn release<K, V>(map: &mut rustc_hash::FxHashMap<K, V>) {
            const { assert!(!std::mem::needs_drop::<(K, V)>()) };
            drop(std::mem::take(map));
        }

        release(&mut self.effects_signature_cache);
        release(&mut self.generic_object_flags);
        release(&mut self.cached_types);
        release(&mut self.signature_instantiations);
        release(&mut self.regular_object_literal_types);
        release(&mut self.properties_types);
        release(&mut self.index_symbols);
        release(&mut self.reverse_mapped_cache);
        release(&mut self.reverse_homomorphic_mapped_cache);
        release(&mut self.merged_symbols);
        self.relations.release_scalar_lookup_buffers();
        self.tables.release_scalar_lookup_buffers();
    }

    /// With `TSRS_MEMORY_REPORT` set, print what this checker allocates, by
    /// structure, to stderr under `label`. The arenas reserved from the
    /// program's size count the records they hold: their untouched room is
    /// address space, not memory.
    // Only commutative ownership sums; printed rows are ordered by BTreeMap.
    #[allow(clippy::iter_over_hash_type)]
    pub(crate) fn report_memory(
        &self,
        label: &str,
        snapshot: &ProgramSnapshot,
        include_shared: bool,
    ) {
        #[cfg(feature = "flow-memo-shadow")]
        self.flow_memo.report(label);
        #[cfg(feature = "union-cache-shadow")]
        self.union_front_cache.report();
        #[cfg(feature = "infer-memo-shadow")]
        self.infer_memo.report(label);
        if std::env::var_os("TSRS_MEMORY_REPORT").is_none() {
            return;
        }
        fn map_bytes<K, V>(map: &rustc_hash::FxHashMap<K, V>) -> usize {
            map.capacity() * (std::mem::size_of::<(K, V)>() + 1) * 8 / 7
        }
        let mut shared_usage = tsc_diagnostics::MemoryUsage::default();
        let (program_heap, mut program_binder) =
            crate::memory::snapshot_heap_usage(snapshot, &mut shared_usage);
        let persistent_tables: rustc_hash::FxHashSet<_> =
            program_binder.tables.keys().copied().collect();
        let mut usage = if include_shared {
            shared_usage
        } else {
            tsc_diagnostics::MemoryUsage::default()
        };
        if include_shared {
            if let Some(provider) = self.authoritative_module_provider {
                provider.add_memory_usage(&mut usage);
            }
        }
        crate::memory::checker_owners(self, &mut usage, include_shared);
        usage.vec(
            "instantiation scratch lists",
            &self.instantiation_type_lists,
        );
        for list in &self.instantiation_type_lists {
            usage.vec("instantiation scratch capacity", list);
        }
        usage.vec(
            "instantiation scratch lists",
            &self.instantiation_text_lists,
        );
        for list in &self.instantiation_text_lists {
            usage.vec("instantiation scratch capacity", list);
        }
        let mut rows: Vec<(&str, usize, usize)> = Vec::new();
        let types = self.tables.memory_usage();
        rows.push(("types (Type)", types.types, types.type_bytes));
        rows.push((
            "type-owned lists and texts",
            types.types,
            types.type_heap_bytes,
        ));
        rows.push((
            "string literal table",
            types.string_literals,
            types.string_literal_map_bytes,
        ));
        rows.push(("type list map", types.type_lists, types.type_list_map_bytes));
        rows.push((
            "list-id string maps",
            types.string_keyed_entries,
            types.string_keyed_bytes,
        ));
        rows.push((
            "instantiation/other type maps",
            types.instantiations,
            types.other_map_bytes,
        ));
        rows.extend(self.links.memory_usage());
        let (flow_entries, flow_bytes) = self.flow_memo.memory_usage();
        rows.push(("flow memo and walk scratch", flow_entries, flow_bytes));
        let (entries, bytes) = self.no_flow_context_nodes.memory_usage();
        rows.push(("annotation context bits", entries, bytes));
        let mut tables = std::mem::take(&mut program_binder.tables);
        let persistent_table_bytes: usize = tables.values().sum();
        let (symbols, symbol_bytes, owned) = self.binder.transient_memory_usage(&mut tables);
        let (table_arc_layout, table_arc_offset) = std::alloc::Layout::new::<[usize; 2]>()
            .extend(std::alloc::Layout::new::<tsc_binder::SymbolTable>())
            .expect("symbol table Arc layout");
        for table in tables.keys() {
            if (include_shared || !persistent_tables.contains(table))
                && usage.claim(*table as usize - table_arc_offset)
            {
                let bytes = table_arc_layout.pad_to_align().size();
                usage.add("symbol table Arc headers", 1, bytes, bytes);
            }
        }
        rows.push(("transient symbols (Symbol)", symbols, symbol_bytes));
        rows.push((
            "transient names/decls/tables",
            symbols,
            owned + tables.values().sum::<usize>() - persistent_table_bytes,
        ));
        let signature_heap: usize = self
            .signatures
            .iter()
            .map(Signature::heap_bytes)
            .sum::<usize>()
            + map_bytes(&self.signature_instantiations);
        rows.push((
            "signatures (Signature)",
            self.signatures.len(),
            self.signatures.len() * std::mem::size_of::<Signature>()
                + signature_heap
                + self.signatures.memo_bytes(),
        ));
        let mut member_symbols = rustc_hash::FxHashSet::default();
        let members_heap: usize = self
            .members
            .iter()
            .map(|members| {
                members.members.counted_heap_bytes(&mut member_symbols)
                    + members.properties.counted_heap_bytes(&mut member_symbols)
                    + members.extras_heap_bytes_once(&mut usage)
            })
            .sum();
        rows.push((
            "resolved members",
            self.members.len(),
            self.members.len() * std::mem::size_of::<ResolvedMembers>()
                + members_heap
                + self.index_info_components.capacity() * std::mem::size_of::<Box<[NodeId]>>()
                + self
                    .index_info_components
                    .iter()
                    .map(|nodes| std::mem::size_of_val(&**nodes))
                    .sum::<usize>()
                + map_bytes(&self.declared_property_indexes)
                + self
                    .declared_property_indexes
                    .values()
                    .map(|index| index.heap_bytes())
                    .sum::<usize>(),
        ));
        rows.push((
            "deferred reference members",
            self.deferred_reference_members.len(),
            self.deferred_reference_members.table_bytes()
                + self
                    .deferred_reference_members
                    .iter()
                    .map(|members| members.heap_bytes(&mut usage))
                    .sum::<usize>()
                + self.unaffected_members.heap_bytes(),
        ));
        rows.push(("type mappers", self.mappers.len(), self.mappers.bytes()));
        let (relations, relation_bytes) = self.relations.memory_usage();
        rows.push(("relation caches", relations, relation_bytes));
        rows.push((
            "subtype reduction cache",
            self.subtype_reduction_cache.len(),
            map_bytes(&self.subtype_reduction_cache)
                + self
                    .subtype_reduction_cache
                    .values()
                    .map(|list| list.capacity() * 4)
                    .sum::<usize>(),
        ));
        rows.push((
            "cached/error types, unresolved",
            self.cached_types.len()
                + self.regular_object_literal_types.len()
                + self.properties_types.len()
                + self.error_types.len()
                + self.unresolved_symbols.len(),
            map_bytes(&self.cached_types)
                + map_bytes(&self.regular_object_literal_types)
                + map_bytes(&self.properties_types)
                + map_bytes(&self.error_types)
                + self.error_types.keys().map(String::capacity).sum::<usize>()
                + map_bytes(&self.unresolved_symbols)
                + self
                    .unresolved_symbols
                    .keys()
                    .map(String::capacity)
                    .sum::<usize>(),
        ));
        rows.push((
            "flow caches",
            self.flow_loop_caches.len() + self.flow_node_reachable.len(),
            self.flow_loop_caches.memory_bytes()
                + self.flow_loop_caches.values().map(map_bytes).sum::<usize>()
                + self.flow_node_reachable.memory_bytes()
                + self.flow_node_post_super.memory_bytes(),
        ));
        rows.push((
            "inference arenas",
            self.inference_context_arena.records().len()
                + self.inference_info_arena.records().len(),
            self.inference_context_arena.records().len()
                * std::mem::size_of::<crate::inference::InferenceContext>()
                + self.inference_info_arena.records().len()
                    * std::mem::size_of::<crate::inference::InferenceInfo>(),
        ));
        let total: usize = rows.iter().map(|row| row.2).sum();
        let main_checker_heap = total
            - types.type_bytes
            - symbol_bytes
            - self.binder.transient_declaration_store_bytes()
            + types.type_record_heap_bytes
            + self.binder.transient_record_heap_bytes();
        let name_bytes = if include_shared {
            tsc_types::EscapedName::interned_names().1
        } else {
            0
        };
        let main_heap = main_checker_heap
            + if include_shared { program_heap } else { 0 }
            + name_bytes
            + usage.total().requested;
        for (name, count, bytes) in rows {
            eprintln!(
                "[memory] {label} {name:<32} {count:>11} {:>10.1} MiB",
                bytes as f64 / (1024.0 * 1024.0)
            );
        }
        eprintln!(
            "[memory] {label} {:<32} {:>11} {:>10.1} MiB",
            "checker total (accounted)",
            "",
            total as f64 / (1024.0 * 1024.0)
        );
        usage.print(label);
        let resident_tail_bytes = crate::memory::print_reserved_arenas(self, label);
        let main_heap = main_heap + resident_tail_bytes;
        eprintln!("[memory] {label} observed complete resident arena tails: {resident_tail_bytes} B (added once to main estimate)");
        eprintln!(
            "[memory] {label} main heap owners estimate: {} B ({:.3} MiB)",
            main_heap,
            main_heap as f64 / 1048576.0
        );
        if include_shared {
            eprintln!(
                "[memory] source characters (separate text heap): {} B ({:.3} MiB)",
                usage.source_text_bytes,
                usage.source_text_bytes as f64 / 1048576.0
            );
        }
        eprintln!("[memory] accounting: end-of-check checkpoint; legacy arena rows use initialized records, legacy lists/hash rows use capacity estimates. New rows separate initialized/requested bytes (value layouts include padding; hash/IndexSet/BTree layouts are estimates from public capacities). Shared program/host owners appear only in serial/shard 0. Complete resident tail pages of the six arena Vecs and the two dense links Vecs are added once; other unused room, allocator rounding/free space, native mutex backing, available parse bodies and edited persistent-line trees are excluded; no usable/resident/physical-peak claim. Diagnostic owners are live here; peak-only clones are not inferred.");
    }

    /// Single-file construction for the relation probe and unit tests.
    /// `source` must be the binder's file.
    /// tsrs-native: single-file test/probe adapter around the Rust
    /// multi-file CheckerState constructor.
    pub fn new(
        source: &'a SourceFile,
        binder: &'a Binder<'a>,
        options: &'a CompilerOptions,
    ) -> Self {
        assert!(std::ptr::eq(binder.source, source));
        Self::from_program(vec![binder], options)
    }

    /// tsrs-native: install the exact H0 host seam and stable source-index projection.
    pub(super) fn install_authoritative_module_provider(
        &mut self,
        provider: &'a dyn crate::AuthoritativeModuleProvider,
        sources: &'a crate::AuthoritativeSources,
    ) -> Result<(), crate::AuthoritativeModuleFailure> {
        #[cfg(debug_assertions)]
        {
            let (by_token, invalid) = crate::AuthoritativeSources::index_files(
                &sources.files,
                (0..self.binder.file_count()).map(|index| &self.binder.source(index).file_name),
            );
            debug_assert_eq!(sources.invalid, invalid);
            debug_assert_eq!(sources.by_token, by_token);
        }
        if let Some(detail) = &sources.invalid {
            return Err(crate::AuthoritativeModuleFailure::InvalidMetadata {
                detail: detail.clone(),
            });
        }
        self.authoritative_module_provider = Some(provider);
        self.authoritative_sources = sources;
        // The memo derives from the tables replaced above, and the kept
        // answers were the replaced provider's.
        self.implied_node_format_memo.borrow_mut().clear();
        self.resolved_module_cache.borrow_mut().clear();
        Ok(())
    }

    /// tsrs-native: retain the first exact-host failure outside checker recovery control flow.
    pub(crate) fn record_authoritative_module_failure(
        &self,
        failure: crate::AuthoritativeModuleFailure,
    ) {
        let _ = self.authoritative_module_failure.set(failure);
    }

    /// tsrs-native: transfer the first exact-host failure to the production driver boundary.
    pub(crate) fn take_authoritative_module_failure(
        &mut self,
    ) -> Option<crate::AuthoritativeModuleFailure> {
        self.authoritative_module_failure.take()
    }

    /// Program construction (M4 5.0): binders in program order, each
    /// bound with contiguous id bases. Runs the initializeTypeChecker
    /// slice (globals merge + intrinsic symbol seeds + duplicate
    /// flush); merge diagnostics land in `self.diagnostics`.
    /// tsrs-native: Rust checker arena/cache construction around the
    /// separately ledgered initializeTypeChecker slices.
    pub fn from_program(binders: Vec<&'a Binder<'a>>, options: &'a CompilerOptions) -> Self {
        Self::from_program_binder(ProgramBinder::new(binders), options, true, false, 1)
    }

    /// Fresh checker-session construction over an already parsed and bound
    /// immutable ProgramSnapshot. The snapshot is borrowed for the session;
    /// no parser or binder work is performed here, and all checker caches
    /// remain session-local.
    /// tsrs-native: fresh checker session over an immutable ProgramSnapshot.
    pub fn from_snapshot(snapshot: &'a ProgramSnapshot, options: &'a CompilerOptions) -> Self {
        Self::from_program_binder(
            ProgramBinder::from_snapshot(snapshot),
            options,
            true,
            false,
            1,
        )
    }

    /// [`Self::from_snapshot`] without the globals merge: the caller installs
    /// the module resolution view and then runs
    /// [`Self::initialize_deferred_program_globals`]. Merging two
    /// declarations of an ambient module resolves an alias among their
    /// exports (mergeSymbol's resolveSymbol(target)), and that can need a
    /// module of the program.
    ///
    /// `pool_size` is the number of checkers that share the program's files
    /// (1 for a checker that checks them all): it sizes the lookup tables
    /// (see [`Self::lookup_share`]).
    /// tsrs-native: two-step construction for the program driver.
    pub(crate) fn from_snapshot_deferring_globals(
        snapshot: &'a ProgramSnapshot,
        options: &'a CompilerOptions,
        measure_lane_costs: bool,
        pool_size: usize,
    ) -> Self {
        Self::from_program_binder(
            ProgramBinder::from_snapshot(snapshot),
            options,
            false,
            measure_lane_costs,
            pool_size,
        )
    }

    /// The divisor of the program's counts for a checker's lookup-table
    /// reservations: 1 for the one checker of a program, 2 for a checker of a
    /// pool. A checker of a pool resolves its own files and what they
    /// reach: at four checkers between a quarter and a half of the type
    /// lists, instantiations and relation pairs the one checker of the same
    /// program holds (the TypeScript compiler's sources: 31-42 %, 35-43 %,
    /// 23-44 %; playwright: up to 50 %, 47 %, 37 %). Reserved for the whole
    /// program, each checker's hash tables were two to four times too large,
    /// and a hash table's pages are all resident once a few hundred entries
    /// are in it. A table that needs more than its reservation grows.
    pub(crate) const fn lookup_share(pool_size: usize) -> usize {
        if pool_size > 1 {
            2
        } else {
            1
        }
    }

    /// The initializeTypeChecker slice a constructor runs unless it was
    /// deferred.
    pub(crate) fn initialize_deferred_program_globals(&mut self) {
        self.initialize_program_globals();
        self.run_init_global_type_probes();
    }

    /// Run `operation` over the hook-less result that owns the checker-built
    /// display nodes. The arena persists for the session; each use wraps it
    /// (no roots, no transformers) and takes it back afterwards.
    /// tsrs-native: hook-less TransformationResult owning checker-built display nodes.
    pub(crate) fn with_emit_display<R>(
        &mut self,
        operation: impl FnOnce(&mut tsc_emitter::TransformationResult<'static>) -> R,
    ) -> R {
        let mut display = self.take_emit_display();
        // A panic (one the API answers as its request's error) leaves the
        // display arena to the next use.
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(&mut display)));
        self.restore_emit_display(display);
        result.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    }

    /// The display result wrapped around the retained arena, detached from
    /// the state so a builder can use the arena and the checker together;
    /// [`Self::restore_emit_display`] returns the arena afterwards.
    pub(crate) fn take_emit_display(&mut self) -> tsc_emitter::TransformationResult<'static> {
        debug_assert!(
            !self.emit_display_taken,
            "the display arena is already lent out"
        );
        self.emit_display_taken = true;
        // The display arena keeps the node builder's cache for the checker's
        // lifetime (tsgo's `typeToStringNodebuilder` keeps its
        // `serializedTypes`).
        let arena = self.emit_display.take().unwrap_or_else(|| {
            let mut arena = tsc_emitter::TransformArena::default();
            arena.keep_node_builder_cache();
            arena
        });
        tsc_emitter::transform_nodes(arena, Vec::new(), Vec::new(), true)
            .expect("hook-less checker display transform must initialize")
    }

    pub(crate) fn restore_emit_display(
        &mut self,
        display: tsc_emitter::TransformationResult<'static>,
    ) {
        self.emit_display = Some(display.into_arena());
        self.emit_display_taken = false;
    }

    /// Mount one binder source into the checker display arena on first use.
    /// tsrs-native: per-file arena mount for checker-built display nodes.
    /// Whether `file`'s tree and bind records go back to the system once its
    /// check is captured ([`Self::released_leaves`]).
    pub(crate) fn releases_file(&self, file: usize) -> bool {
        self.released_leaves.get(file).copied().unwrap_or(false)
    }

    pub(crate) fn emit_display_target(
        &mut self,
        file_index: usize,
    ) -> tsc_emitter::TransformSourceId {
        if let Some(&target) = self.emit_display_sources.get(&file_index) {
            return target;
        }
        assert!(file_index < self.binder.file_count());
        let source = self.binder.source(file_index);
        let target = self.with_emit_display(|display| {
            display
                .arena_mut()
                .expect("checker display result remains live")
                .add_source(
                    source,
                    Some(tsc_program::SourceFileId::from_raw(
                        u32::try_from(file_index).expect("checker source index exceeds u32"),
                    )),
                )
        });
        self.emit_display_sources.insert(file_index, target);
        target
    }

    fn from_program_binder(
        mut binder: ProgramBinder<'a>,
        options: &'a CompilerOptions,
        initialize_globals: bool,
        measure_lane_costs: bool,
        pool_size: usize,
    ) -> Self {
        let strict_null_checks = options.strict_option_value(options.strict_null_checks);
        let strict_function_types = options.strict_option_value(options.strict_function_types);
        let stable_type_ordering = options.stable_type_ordering_effective();
        let exact_optional = options.exact_optional_property_types.unwrap_or(false);
        // ProgramBinder has validated one domain for every document. The
        // checker's own type lease belongs to that same session owner.
        let identity_domain = binder
            .source(0)
            .arena
            .identity_leases()
            .expect("validated Program syntax ownership")
            .nodes
            .domain();
        let mut tables = TypeTables::new(strict_null_checks, exact_optional, &identity_domain);

        // The checker init block's symbols (46488-46496, 47006), in
        // tsc allocation order.
        let undefined_symbol = binder.create_symbol(
            SymbolFlags::PROPERTY,
            tsc_types::EscapedName::from_identifier_escaped_text("undefined"),
        );
        let global_this_symbol = binder.create_symbol(
            SymbolFlags::MODULE,
            tsc_types::EscapedName::from_identifier_escaped_text("globalThis"),
        );
        let arguments_symbol = binder.create_symbol(
            SymbolFlags::PROPERTY,
            tsc_types::EscapedName::from_identifier_escaped_text("arguments"),
        );
        let require_symbol = binder.create_symbol(
            SymbolFlags::PROPERTY,
            tsc_types::EscapedName::from_identifier_escaped_text("require"),
        );
        let unknown_symbol = binder.create_symbol(
            SymbolFlags::PROPERTY,
            tsc_types::EscapedName::from_identifier_escaped_text("unknown"),
        );

        // Counts size the type-table hints; exclusive arena ends size the
        // link directories because program-wide identities can have gaps.
        // The Program's sums and bounds over all files, not this checker's
        // pass: `ProgramBinder::checker_starts`.
        let (program_nodes, program_symbols, node_id_end, symbol_id_end) = {
            let starts = binder.checker_starts();
            (
                starts.nodes,
                starts.symbols,
                starts.node_id_end,
                starts.symbol_id_end,
            )
        };
        let lookup_share = Self::lookup_share(pool_size);
        tables.reserve_capacity_hint(program_nodes / lookup_share, program_symbols / lookup_share);
        tsc_types::names::reserve_capacity_hint(program_symbols / 4);

        let mut state = Self {
            binder,
            options,
            emit_display: None,
            emit_display_taken: false,
            import_meta_expression_meta_symbol: None,
            index_symbols: rustc_hash::FxHashMap::default(),
            emit_display_sources: std::collections::BTreeMap::new(),
            child_scratch: Vec::new(),
            instantiation_type_lists: Vec::new(),
            instantiation_text_lists: Vec::new(),
            property_scratch: Vec::new(),
            property_name_sets: Vec::new(),
            unaffected_members: Default::default(),
            tables,
            strict_function_types,
            stable_type_ordering,
            links: LinksTables::with_id_bounds(node_id_end, symbol_id_end),
            signatures: SignatureStore::default(),
            index_info_components: Vec::new(),
            assignment_marking_depth: 0,
            signature_instantiations: rustc_hash::FxHashMap::default(),
            members: Vec::new(),
            members_resolving: 0,
            intersection_recursion_ids: Default::default(),
            augment_filter: None,
            member_shapes: Default::default(),
            member_indexes: Default::default(),
            declared_property_indexes: Default::default(),
            contextual_relevant: Default::default(),
            deferred_reference_members: Default::default(),
            named_member_order_buffers: Default::default(),
            relations: RelationCaches::with_capacity_hint(program_symbols / lookup_share),
            subtype_reduction_cache: rustc_hash::FxHashMap::default(),
            inference_visited: Default::default(),
            infer_memo: crate::infer_memo::InferMemo::new(),
            expression_checks: 0,
            impure_union_reductions: 0,
            provisional_parameter_types: 0,
            inference_walk_buffers: Default::default(),
            array_variances: SharedList::from(crate::variance::ARRAY_VARIANCES),
            speculation_depth: 0,
            order_guard: crate::order_guard::OrderGuard::default(),
            checked_source_files: 0,
            tracer: None,
            #[cfg(test)]
            speculation_commit_count: 0,
            #[cfg(test)]
            speculation_rollback_count: 0,
            empty_object_type: TypeId::new(0),
            empty_type_literal_type: TypeId::new(0),
            unknown_empty_object_type: TypeId::new(0),
            unknown_union_type: TypeId::new(0),
            empty_generic_type: TypeId::new(0),
            any_function_type: TypeId::new(0),
            empty_jsx_object_type: TypeId::new(0),
            empty_fresh_jsx_object_type: TypeId::new(0),
            deferred_global_promise_type: None,
            deferred_global_promise_like_type: None,
            deferred_global_promise_constructor_symbol: None,
            any_signature: SignatureId::new(0),
            unknown_signature: SignatureId::new(0),
            resolving_signature: SignatureId::new(0),
            silent_never_signature: SignatureId::new(0),
            is_inference_partially_blocked: false,
            skip_direct_inference_nodes: std::collections::HashSet::new(),
            call_resolution_stack: Vec::new(),
            apparent_argument_count: None,
            no_constraint_type: TypeId::new(0),
            circular_constraint_type: TypeId::new(0),
            mappers: Default::default(),
            restrictive_mapper: crate::instantiate::MapperId::new(0),
            permissive_mapper: crate::instantiate::MapperId::new(0),
            unique_literal_mapper: crate::instantiate::MapperId::new(0),
            report_unreliable_mapper: crate::instantiate::MapperId::new(0),
            report_unmeasurable_mapper: crate::instantiate::MapperId::new(0),
            marker_super_type: TypeId::new(0),
            marker_sub_type: TypeId::new(0),
            marker_other_type: TypeId::new(0),
            marker_super_type_for_check: TypeId::new(0),
            marker_sub_type_for_check: TypeId::new(0),
            variance_type_parameter: None,
            display_visited_types: rustc_hash::FxHashSet::default(),
            display_symbol_depth: rustc_hash::FxHashMap::default(),
            display_infer_type_parameters: Vec::new(),
            display_approximate_length: 0,
            display_max_truncation_length: 160,
            display_signature_string: false,
            display_truncating: false,
            display_reverse_mapped_stack: Vec::new(),
            display_no_type_reduction: false,
            display_enclosing: None,
            display_collision_retry: false,
            display_mappers: Vec::new(),
            display_reuse_had_error: false,
            display_reuse_visit_depth: 0,
            display_clone_indent: 0,
            display_clone_at_line_start: false,
            marker_types: rustc_hash::FxHashSet::default(),
            variance_stack: Vec::new(),
            reliability_flags: tsc_types::RelationComparisonResult::NONE,
            active_type_mappers: Vec::new(),
            active_type_mappers_caches: Vec::new(),
            instantiation_stack: Vec::new(),
            instantiation_count: 0,
            total_instantiation_count: 0,
            type_parameter_defaults_in_progress: Vec::new(),
            mapped_types_in_progress: Vec::new(),
            flow_analysis_disabled: false,
            canonical_history: false,
            checking_file: None,
            released_leaves: std::sync::Arc::from(Vec::new()),
            deferred_type_argument_checks: rustc_hash::FxHashMap::default(),
            flow_memo: crate::flow_memo::FlowMemo::new(),
            no_flow_context_nodes: Default::default(),
            union_front_cache: Default::default(),
            shared_flow: Default::default(),
            speculative_flow: Default::default(),
            evolving_array_types: rustc_hash::FxHashMap::default(),
            generic_object_flags: rustc_hash::FxHashMap::default(),
            final_array_types: rustc_hash::FxHashMap::default(),
            regular_object_literal_types: rustc_hash::FxHashMap::default(),
            properties_types: rustc_hash::FxHashMap::default(),
            flow_loop_caches: Default::default(),
            last_flow_node: None,
            last_flow_node_reachable: false,
            flow_node_reachable: Default::default(),
            flow_node_post_super: Default::default(),
            within_unreachable_code: false,
            reported_unreachable_nodes: rustc_hash::FxHashSet::default(),
            current_node: None,
            line_profile: crate::line_profile::LineProfiler::with_readers(
                crate::line_profile::enabled(),
                measure_lane_costs,
            ),
            profile_ops: [0; crate::line_profile::OPS],
            deferred_nodes: rustc_hash::FxHashMap::default(),
            deferred_node_sets: Vec::new(),
            deferred_diagnostics: Vec::new(),
            declaration_emit_aliases_marked: rustc_hash::FxHashSet::default(),
            declaration_emit_inputs: None,
            deferred_ambient_module_symbols: Vec::new(),
            potential_unused_renamed_binding_elements_in_types: Vec::new(),
            potentially_unused_identifiers: rustc_hash::FxHashMap::default(),
            deferred_global_disposable_type: None,
            deferred_global_async_disposable_type: None,
            deferred_global_extract_symbol: None,
            init_global_type_probes: rustc_hash::FxHashMap::default(),
            deferred_global_non_nullable_type_alias: None,
            deferred_global_record_symbol: None,
            deferred_global_awaited_symbol: None,
            awaited_type_stack: Vec::new(),
            deferred_global_omit_symbol: None,
            widening_contexts: Vec::new(),
            undefined_properties: rustc_hash::FxHashMap::default(),
            undefined_properties_by_property: rustc_hash::FxHashMap::default(),
            typeof_type: TypeId::new(0),
            contextual_binding_patterns: Vec::new(),
            contextual_types: Default::default(),
            inference_contexts: Vec::new(),
            inference_context_arena: crate::inference_storage::InferencePool::default(),
            inference_info_arena: crate::inference_storage::InferencePool::default(),
            inference_lifetimes: crate::inference_storage::InferenceLifetimes::default(),
            reverse_homomorphic_mapped_cache: rustc_hash::FxHashMap::default(),
            reverse_mapped_cache: rustc_hash::FxHashMap::default(),
            reverse_mapped_source_stack: Vec::new(),
            reverse_mapped_target_stack: Vec::new(),
            reverse_expanding_flags: ExpandingFlags::NONE,
            cached_types: rustc_hash::FxHashMap::default(),
            flow_loop_start: 0,
            flow_loop_stack: Vec::new(),
            flow_type_cache: None,
            flow_invocation_count: 0,
            inline_level: 0,
            switch_types_cache: rustc_hash::FxHashMap::default(),
            exhaustive_switch_cache: rustc_hash::FxHashMap::default(),
            effects_signature_cache: rustc_hash::FxHashMap::default(),
            diagnostics: DiagnosticSink::default(),
            visible_global_diagnostics: Vec::new(),
            partially_checked_ranges: rustc_hash::FxHashMap::default(),
            contained_call_resolutions: rustc_hash::FxHashSet::default(),
            partial_check_records: Vec::new(),
            globals: std::sync::Arc::default(),
            undefined_symbol,
            global_this_symbol,
            arguments_symbol,
            require_symbol,
            unknown_symbol,
            unresolved_symbols: rustc_hash::FxHashMap::default(),
            error_types: rustc_hash::FxHashMap::default(),
            pattern_ambient_modules: Vec::new(),
            pattern_ambient_module_augmentations: rustc_hash::FxHashMap::default(),
            pattern_ambient_module_augmentation_targets: rustc_hash::FxHashMap::default(),
            module_import_attributes_types: rustc_hash::FxHashMap::default(),
            resolving_explicit_type_of_symbol: rustc_hash::FxHashSet::default(),
            conditional_constraint_depth: 0,
            unresolved_module_augmentations: rustc_hash::FxHashMap::default(),
            unresolved_package_root_cache: Default::default(),
            implied_node_format_memo: std::cell::RefCell::new(Vec::new()),
            authoritative_module_provider: None,
            authoritative_sources: &crate::EMPTY_AUTHORITATIVE_SOURCES,
            authoritative_module_failure: std::cell::OnceCell::new(),
            resolved_module_cache: Default::default(),
            outside_import_equals: Default::default(),
            outside_type_declarations: Default::default(),
            module_name_outside_program: std::cell::Cell::new(false),
            host_input_snapshots: Default::default(),
            basic_module_specifier_host: std::cell::OnceCell::new(),
            host_current_directory: "/".into(),
            host_package_json_module_types: Default::default(),
            host_package_json_values: Default::default(),
            host_package_json_names: Default::default(),
            external_helpers_modules: rustc_hash::FxHashMap::default(),
            requested_external_emit_helpers: rustc_hash::FxHashMap::default(),
            distributed_type_parameters: rustc_hash::FxHashMap::default(),
            jsx_implicit_import_containers: rustc_hash::FxHashMap::default(),
            combined_node_flags_cache: Default::default(),
            not_deprecated_front: vec![std::cell::Cell::new(0); 1 << Self::NOT_DEPRECATED_BITS]
                .into_boxed_slice(),
            not_private_front: vec![std::cell::Cell::new(0); 1 << Self::NOT_DEPRECATED_BITS]
                .into_boxed_slice(),
            not_static_private_front: vec![std::cell::Cell::new(0); 1 << Self::NOT_DEPRECATED_BITS]
                .into_boxed_slice(),
            declaration_modifier_front: vec![
                std::cell::Cell::new((0, 0));
                2 << Self::NOT_DEPRECATED_BITS
            ]
            .into_boxed_slice(),
            control_flow_containers: Default::default(),
            jsdoc_tag_cache: Default::default(),
            global_type_memos: Default::default(),
            decorator_context_override_type_cache: Default::default(),
            relation_frame_loan: crate::engine::RelationFrameLoan::None,
            resolution_targets: Vec::new(),
            resolution_results: Vec::new(),
            resolution_property_names: Vec::new(),
            resolution_serials: Vec::new(),
            resolution_start: 0,
            merged_symbols: rustc_hash::FxHashMap::default(),
            merged_symbol_bits: Vec::new(),
            relation_work_pool: Default::default(),
            conditional_branches: vec![
                crate::conditional::ConditionalBranchSlot::default();
                crate::conditional::CONDITIONAL_BRANCH_SLOTS
            ]
            .into_boxed_slice(),
            merged_symbol_sources: rustc_hash::FxHashMap::default(),
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
        // The module resolver's path table (M4 5.8d) is the Program's, not
        // this checker's: `ProgramBinder::path_index`.
        std::sync::Arc::make_mut(&mut state.globals).insert(
            tsc_types::EscapedName::from_identifier_escaped_text("globalThis"),
            global_this_symbol,
        );

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
        // tsc-port: reportUnreliableMapper @6.0.3 (+ reportUnmeasurableMapper)
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
        // tsc-port: emptyJsxObjectType @6.0.3 (+ emptyFreshJsxObjectType)
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
            tsc_types::EscapedName::internal(InternalSymbolName::TYPE),
        );
        state.empty_type_literal_type =
            state.create_resolved_empty_anonymous_type(Some(empty_type_literal_symbol));
        // tsc-port: unknownEmptyObjectType @6.0.3 (+ unknownUnionType)
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
                .get_union_type_ex(&members, tsc_types::UnionReduction::Literal)
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
        // tsc-port: noConstraintType @6.0.3 (+ circularConstraintType)
        // tsc-hash: 06e5cd556cafd99a8a477e291385d1cb488f4d28676756e76a7ef6135c9d198b
        // tsc-span: _tsc.js:47188-47203
        state.no_constraint_type = state.create_resolved_empty_anonymous_type(None);
        state.circular_constraint_type = state.create_resolved_empty_anonymous_type(None);
        // tsc-port: markerSuperType @6.0.3 (+ markerSubType + markerOtherType)
        // tsc-hash: b01ebfcde068d7826bd3c12f41c9869047b7a25af2c00e6dd66cca614c1d8f38
        // tsc-span: _tsc.js:47212-47215
        state.marker_super_type = state.tables.create_synthesized_type_parameter(None);
        state.marker_sub_type = state
            .tables
            .create_synthesized_type_parameter(Some(state.marker_super_type));
        state.marker_other_type = state.tables.create_synthesized_type_parameter(None);
        // tsc-port: markerSuperTypeForCheck @6.0.3 (+ markerSubTypeForCheck)
        // tsc-hash: 119485aecd36a63821c1191c69eb9b05cf44460f4dca261f773403ea54131d2b
        // tsc-span: _tsc.js:47216-47218
        state.marker_super_type_for_check = state.tables.create_synthesized_type_parameter(None);
        state.marker_sub_type_for_check = state
            .tables
            .create_synthesized_type_parameter(Some(state.marker_super_type_for_check));
        // tsc-port: anySignature @6.0.3 (+ unknownSignature + resolvingSignature + silentNeverSignature)
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
            // stableTypeOrdering sorts the typeofNEFacts keys (50137).
            let insertion_order = [
                "string",
                "number",
                "bigint",
                "boolean",
                "symbol",
                "undefined",
                "object",
                "function",
            ];
            let sorted_order = [
                "bigint",
                "boolean",
                "function",
                "number",
                "object",
                "string",
                "symbol",
                "undefined",
            ];
            let members: Vec<TypeId> = if state.stable_type_ordering {
                sorted_order
            } else {
                insertion_order
            }
            .iter()
            .map(|name| state.tables.get_string_literal_type(*name))
            .collect();
            state
                .get_union_type_ex(&members, tsc_types::UnionReduction::Literal)
                .expect("literal unions cannot fail")
        };

        // tsc checker-init 47416-47418: these cache roots are created
        // before any user program type. Union members sort by TypeId
        // when stableTypeOrdering is off, so lazy creation here would
        // incorrectly let an earlier-visited `2`, `""`, or `1n`
        // precede the corresponding zero/empty literal in displays.
        state.tables.get_string_literal_type("");
        state.tables.get_number_literal_type(0.0);
        state.tables.get_bigint_literal_type(PseudoBigInt {
            negative: false,
            base10_value: "0".to_owned(),
        });

        // initializeTypeChecker slice (88732-88906): globals merge +
        // symbol-type seeds + amalgamated-duplicate flush.
        if initialize_globals {
            state.initialize_deferred_program_globals();
            // A state built in one step has no later point that is sure to
            // run; the program driver merges these after the global-scope
            // augmentations (merge_module_augmentations).
            state.merge_deferred_ambient_modules();
        }
        // (has_global_augmentation RETIRED 5.8d: declare-global exports
        // merge in merge_module_augmentations; the resolver failure
        // band and the JSX containment both lifted.)
        state
    }

    /// The 47220-47275 singleton shape: declaration-less, no
    /// parameters, minArg 0, pre-resolved return type.
    fn create_singleton_signature(&mut self, return_type: TypeId) -> SignatureId {
        self.alloc_signature({
            let mut signature_data = crate::state::Signature::new();
            signature_data.set_parameters(Vec::new().into_boxed_slice());
            signature_data.resolved_return_type = Some(return_type);
            signature_data.set_isolated_signature_kind(Some(SignatureKind::Construct));
            signature_data
        })
    }

    /// tsc-port: createAnonymousType @6.0.3
    /// tsc-hash: 801cde8bdea7de88d9052f5f01d296c15ec067902d478f857925edd1106efb93
    /// tsc-span: _tsc.js:50208-50210
    pub(crate) fn create_resolved_empty_anonymous_type(
        &mut self,
        symbol: Option<SymbolId>,
    ) -> TypeId {
        let id = self
            .tables
            .create_type(TypeFlags::OBJECT, TypeData::PLAIN_OBJECT);
        self.tables.type_mut(id).object_flags = ObjectFlags::ANONYMOUS;
        self.tables.type_mut(id).symbol = symbol;
        let members = self.alloc_members(ResolvedMembers::default());
        self.links
            .set_fresh_type_members(id, LinkSlot::Resolved(members));
        id
    }

    /// tsrs-native: arena allocation for ResolvedMembers; tsc creates
    /// and retains ordinary JavaScript objects.
    pub fn alloc_members(&mut self, mut members: ResolvedMembers) -> MembersId {
        // Every published table passes here or freeze_named_members: one
        // index per name sequence.
        self.member_indexes.share(&mut members.members);
        let id = MembersId::new(self.members.len() as u32);
        self.members.push(members);
        id
    }

    /// Finished tables can fill the published Arc directly, without first
    /// copying their IDs into a mutable builder.
    #[inline]
    pub(crate) fn member_table_source<'table>(
        &self,
        table: &'table SymbolTable,
    ) -> crate::member_table::MemberTableSource<'table> {
        crate::member_table::MemberTableSource::new(&self.binder, table)
    }

    /// tsrs-native: numeric MembersId arena accessor; tsc carries the
    /// object reference directly.
    pub fn members_of(&self, id: MembersId) -> &ResolvedMembers {
        &self.members[id.index() as usize]
    }

    /// In-place mutation for tsc's repeated setStructuredTypeMembers
    /// over the SAME ResolvedType object (resolveObjectTypeMembers
    /// 57829/57840: the early write makes partial members observable
    /// to mid-cycle readers, then inheritance mutates the table).
    /// tsrs-native: mutable MembersId arena accessor required by Rust
    /// ownership; tsc mutates the object directly.
    pub fn members_mut(&mut self, id: MembersId) -> &mut ResolvedMembers {
        let members = &mut self.members[id.index() as usize];
        members.forget_weak_properties();
        members
    }

    pub(crate) fn reserve_members(&mut self, additional: usize) {
        self.members.reserve(additional);
    }

    pub(crate) fn member_arena(&self) -> &Vec<ResolvedMembers> {
        &self.members
    }

    /// The resolved property list and the file binder symbols it names are
    /// immutable until the record is opened for writing. A required file
    /// symbol suffices even if earlier optional symbols were transient.
    pub(crate) fn has_only_optional_properties(&mut self, ty: TypeId, id: MembersId) -> bool {
        let members = self.members_of(id);
        let eligible = self.speculation_depth == 0
            && self.members_resolving == 0
            && members.has_no_lists()
            && self
                .links
                .read_ty(ty, |links| links.resolved_members.resolved())
                == Some(id);
        let optional = |p| {
            self.binder
                .symbol(p)
                .flags
                .intersects(SymbolFlags::OPTIONAL)
        };
        if eligible {
            if let Some(answer) = members.weak_properties() {
                debug_assert_eq!(
                    answer,
                    !members.properties.is_empty()
                        && members.properties.iter().copied().all(optional)
                );
                return answer;
            }
        }
        let required = members.properties.iter().copied().find(|&p| !optional(p));
        let answer = required.is_none() && !members.properties.is_empty();
        if eligible {
            let published = |p: SymbolId| p.index() & tsc_types::TRANSIENT_SYMBOL_BIT == 0;
            if required.map_or_else(
                || members.properties.iter().copied().all(published),
                published,
            ) {
                self.members[id.index() as usize].set_weak_properties(answer);
            }
        }
        answer
    }

    /// Write a finished record over the one allocated for it earlier (the
    /// shell that a resolution in progress published): like
    /// [`Self::alloc_members`], its table takes the index of its name sequence.
    pub(crate) fn publish_members(&mut self, id: MembersId, mut members: ResolvedMembers) {
        self.member_indexes.share(&mut members.members);
        *self.members_mut(id) = members;
    }

    /// tsrs-native: arena allocation for Signature objects; tsc uses
    /// ordinary JavaScript object allocation.
    pub fn alloc_signature(&mut self, signature: Signature) -> SignatureId {
        self.retain_inference_mapper(signature.mapper);
        self.signatures.push(signature)
    }

    /// tsrs-native: mutable numeric SignatureId arena accessor; tsc
    /// mutates the object directly.
    pub fn signature_mut(&mut self, id: SignatureId) -> &mut Signature {
        self.signatures.get_mut(id)
    }

    /// tsrs-native: numeric SignatureId arena accessor; tsc carries
    /// the object reference directly.
    pub fn signature_of(&self, id: SignatureId) -> &Signature {
        self.signatures.get(id)
    }

    /// tsrs-native: tsc 59839 `signature.resolvedReturnType ??= type`
    /// as an asserted helper — first write
    /// wins — a re-entrant fill keeps the INNER frame's value and the
    /// caller must consume the returned slot value, not its own
    /// computation.
    pub fn seal_signature_return_type(&mut self, id: SignatureId, resolved: TypeId) -> TypeId {
        if let Some(existing) = self.signature_of(id).resolved_return_type {
            return existing;
        }
        self.signatures.get_mut(id).resolved_return_type = Some(resolved);
        resolved
    }

    /// tsrs-native: fresh signature initialization inside a candidate
    /// transaction.
    ///
    /// Initialize the return type of a freshly cloned signature. This
    /// is part of constructing the signature and may therefore occur
    /// inside an overload trial; unlike `seal_signature_return_type`,
    /// it does not publish a cache on a pre-existing signature.
    pub fn set_fresh_signature_return_type(&mut self, id: SignatureId, resolved: TypeId) {
        let slot = &mut self.signatures.get_mut(id).resolved_return_type;
        assert!(slot.is_none(), "fresh return type initialized twice");
        *slot = Some(resolved);
    }

    /// Empty member table shared by symbols that never had one.
    /// tsrs-native: SymbolId arena projection for tsc's direct
    /// `symbol.members` property access.
    pub fn symbol_members(&self, symbol: SymbolId) -> &SymbolTable {
        self.binder.symbol(symbol).members()
    }

    /// File-scope name resolution for the relpin scratch program — the
    /// M3/5.0 slice of resolveEntityName: the first file's locals (its
    /// root scope), then the merged globals, meaning-filtered, with the
    /// getMergedSymbol chase (49932) so cross-file merged declarations
    /// surface. Full lexical walking is resolveName (M4 5.1).
    /// tsc-port: resolveEntityName @6.0.3
    /// tsc-hash: 0c5ce0e5980d5548db101cd9240b04944dea6e35cde2b0b3416210816fdb85b9
    /// tsc-span: _tsc.js:49292-49393
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

    /// tsrs-native: SymbolId arena projection for tsc's direct
    /// `symbol.flags` property access.
    pub fn symbol_flags(&self, symbol: SymbolId) -> SymbolFlags {
        self.binder.symbol(symbol).flags
    }

    /// tsrs-native: binder-table projection for tsc's direct
    /// `node.symbol` property access.
    pub fn node_symbol(&self, node: NodeId) -> Option<SymbolId> {
        self.binder.node_symbol(node)
    }

    /// The binder's mutable node-flags view (ContainsThis etc.).
    /// tsrs-native: binder-table projection for tsc's direct
    /// `node.flags` property access.
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
            // W2c: the resolution event itself is order-sensitive (which
            // member of the cycle is entered first depends on the checking
            // order), whether or not a diagnostic is later reported.
            self.order_guard
                .note_always(crate::order_guard::OrderReason::CIRCULARITY);
            for result in &mut self.resolution_results[cycle_start..] {
                *result = false;
            }
            return false;
        }
        self.resolution_serials
            .truncate(self.resolution_targets.len());
        self.resolution_serials.push(self.flow_memo.source_now());
        self.resolution_targets.push(target);
        self.resolution_results.push(true);
        self.resolution_property_names.push(property_name);
        true
    }

    /// tsc-port: findResolutionCycleStartIndex @6.0.3
    /// tsc-hash: 251a7ddb169a1bcea40755dfb143b2ccc1043fd9860cc5ad3658792bb09ada88
    /// tsc-span: _tsc.js:55745-55755
    pub(crate) fn find_resolution_cycle_start_index(
        &mut self,
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
                self.flow_memo.taint(self.resolution_serials[i]);
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
                self.links.read_symbol(symbol, |links| links.type_of_symbol.resolved()).is_some()
            }
            TypeSystemPropertyName::DECLARED_TYPE => {
                let ResolutionTarget::Symbol(symbol) = target else {
                    unreachable!("DeclaredType resolution targets are symbols");
                };
                self.links.symbol_declared_type(symbol).is_some()
            }
            TypeSystemPropertyName::RESOLVED_RETURN_TYPE => {
                let ResolutionTarget::Signature(signature) = target else {
                    unreachable!("ResolvedReturnType resolution targets are signatures");
                };
                self.signature_of(signature)
                    .resolved_return_type
                    .is_some()
            }
            TypeSystemPropertyName::IMMEDIATE_BASE_CONSTRAINT => {
                let ResolutionTarget::Type(ty) = target else {
                    unreachable!("ImmediateBaseConstraint resolution targets are types");
                };
                self.links.read_ty(ty, |links| links.resolved_base_constraint.resolved())
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
                self.links.type_cold().base_types_resolved.get(ty)
            }
            TypeSystemPropertyName::RESOLVED_BASE_CONSTRUCTOR_TYPE => {
                let ResolutionTarget::Type(ty) = target else {
                    unreachable!("ResolvedBaseConstructorType resolution targets are types");
                };
                self.links.type_cold().resolved_base_constructor_type.resolved(ty)
                    .is_some()
            }
            TypeSystemPropertyName::WRITE_TYPE => {
                let ResolutionTarget::Symbol(symbol) = target else {
                    unreachable!("WriteType resolution targets are symbols");
                };
                self.links.symbol_cold().write_type.resolved(symbol).is_some()
            }
            TypeSystemPropertyName::PARAMETER_INITIALIZER_CONTAINS_UNDEFINED => {
                let ResolutionTarget::Node(node) = target else {
                    unreachable!(
                        "ParameterInitializerContainsUndefined resolution targets are nodes"
                    );
                };
                // `links.parameterInitializerContainsUndefined !== undefined`
                // (55773).
                self.links
                    .node_cold()
                    .parameter_initializer_contains_undefined
                    .get(node)
                    .is_some()
            }
            TypeSystemPropertyName::ALIAS_TARGET => {
                let ResolutionTarget::Symbol(symbol) = target else {
                    unreachable!("AliasTarget resolution targets are symbols");
                };
                // tsgo `aliasSymbolLinks.Get(target).aliasTarget != nil`.
                self.links
                    .symbol_cold()
                    .alias_target.resolved(symbol)
                    .is_some()
            }
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
        self.resolution_serials
            .truncate(self.resolution_targets.len());
        self.resolution_property_names.pop();
        self.resolution_results
            .pop()
            .expect("pop_type_resolution without matching push")
    }

    // ---- M4 5.0: error helpers (the `error` family, 47565-47587) ----

    /// tsc createDiagnosticForNode(InSourceFile): span from
    /// getErrorSpanForNode, positions in UTF-16 (the binder's
    /// diagnostic_for_node twin, program-wide).
    /// tsc-port: createDiagnosticForNode @6.0.3
    /// tsc-hash: 358ec88d52a45803957de382a162466361b967ba9a0665a2d6d7431d2eff55fe
    /// tsc-span: _tsc.js:13909-13912
    pub fn diagnostic_for_node(
        &self,
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> Diagnostic {
        let args = args.iter().map(|arg| JsStr::from(*arg)).collect::<Vec<_>>();
        self.diagnostic_for_node_js(node, message, &args)
    }

    /// The node span is shared with the scalar entry; name-bearing arguments
    /// remain canonical until the diagnostic output boundary.
    /// tsc-port: createDiagnosticForNode @6.0.3
    /// tsc-hash: 358ec88d52a45803957de382a162466361b967ba9a0665a2d6d7431d2eff55fe
    /// tsc-span: _tsc.js:13909-13912
    /// JS-valued twin of `diagnostic_for_node`; keep their diagnostic behavior aligned.
    pub fn diagnostic_for_node_js(
        &self,
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[JsStr<'_>],
    ) -> Diagnostic {
        let source = self.binder.source_of_node(node);
        let (start, end) = tsc_binder::node_util::get_error_span_for_node(source, node);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .positions()
                .byte_to_utf16((byte) as u32)
                .unwrap_or(byte as u32)
        };
        let start_utf16 = to_utf16(start);
        let end_utf16 = to_utf16(end);
        Diagnostic::new_js(
            Some(source.file_name.clone()),
            Some(start_utf16),
            Some(end_utf16.saturating_sub(start_utf16)),
            MessageChain::new_js_parts(message, args),
        )
    }

    /// tsrs-native: range-only containment bookkeeping for a typed
    /// oracle-crash abort.
    ///
    /// The program layer exempts only a directive targeting this range
    /// from 2578. This deliberately does not publish a PartialCheck:
    /// the abort mirrors tsc's crash rather than representing
    /// unimplemented checker behavior.
    pub(crate) fn mark_oracle_crash_range(&mut self, node: NodeId, abort: CheckAbort) {
        let _ = node;
        match abort {}
    }

    fn mark_partially_checked_range(&mut self, node: NodeId) {
        let file_index = self.binder.file_index_of_node(node);
        let raw = Published.node(node);
        let range = (raw.pos, raw.end);
        let ranges = self.partially_checked_ranges.entry(file_index).or_default();
        if !ranges.contains(&range) {
            ranges.push(range);
        }
    }

    /// tsrs-native: containment bookkeeping for source ranges whose
    /// diagnostics may be incomplete.
    ///
    /// The program layer exempts only directives targeting one of
    /// these ranges instead of suppressing 2578 for the entire file.
    pub(crate) fn mark_partially_checked_node(&mut self, node: NodeId, reason: impl Into<String>) {
        self.mark_partially_checked_range(node);
        let source = self.binder.source_of_node(node);
        let raw = Published.node(node);
        let to_utf16 = |byte: u32| {
            source
                .positions()
                .byte_to_utf16((byte as usize) as u32)
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

    // ---- variance reliability flags ----

    /// tsgo-port: Checker.reportUnreliableWorker / reportUnmeasurableWorker
    /// @7.1 (checker/checker.go:1146-1158): a marker type that reaches a
    /// report mapper sets the mapper's bit.
    pub(crate) fn report_reliability_if_marker(
        &mut self,
        ty: TypeId,
        flag: tsc_types::RelationComparisonResult,
    ) {
        if ty == self.marker_super_type
            || ty == self.marker_sub_type
            || ty == self.marker_other_type
        {
            self.reliability_flags = tsc_types::RelationComparisonResult::from_bits(
                self.reliability_flags.bits() | flag.bits(),
            );
        }
    }

    /// tsc-port: createError @6.0.3
    /// tsc-hash: dedcf6cc6c301274f018ef98543f4abebe1b7826c45f601b914137812caa8cfa
    /// tsc-span: _tsc.js:47580-47582
    ///
    /// No location ⇒ a file-less, program-level diagnostic. tsgo's
    /// `NewDiagnosticForNode(nil, …)` keeps the zero range (0, 0) where a
    /// compiler diagnostic has the undefined range (-1, -1)
    /// (checker/utilities.go:21-29, ast/diagnostic.go:239-241), so a
    /// checker's global diagnostic sorts after the compiler's.
    pub fn create_error(
        &self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> Diagnostic {
        let args = args.iter().map(|arg| JsStr::from(*arg)).collect::<Vec<_>>();
        self.create_error_js(location, message, &args)
    }

    /// tsc-port: createTypeChecker.createError @6.0.3
    /// tsc-hash: dedcf6cc6c301274f018ef98543f4abebe1b7826c45f601b914137812caa8cfa
    /// tsc-span: _tsc.js:47580-47582
    /// JS-valued twin of `create_error`; keep their diagnostic behavior aligned.
    pub fn create_error_js(
        &self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[JsStr<'_>],
    ) -> Diagnostic {
        match location {
            Some(node) => self.diagnostic_for_node_js(node, message, args),
            None => Diagnostic::new(
                None,
                Some(0),
                Some(0),
                MessageChain::new_js_parts(message, args),
            ),
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
        self.push_error_diagnostic(diagnostic)
    }

    /// tsc-port: createTypeChecker.error @6.0.3
    /// tsc-hash: be9cd419909a0ad4fd544342a9a6c97f837da3819b2844e45c7be96b438439c9
    /// tsc-span: _tsc.js:47583-47587
    /// JS-valued twin of `error_at`; keep their diagnostic behavior aligned.
    pub fn error_at_js(
        &mut self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[JsStr<'_>],
    ) -> usize {
        let diagnostic = self.create_error_js(location, message, args);
        self.push_error_diagnostic(diagnostic)
    }

    /// tsc createFileDiagnostic + diagnostics.add over an explicit
    /// byte range — the aggregated 7027 span is the sole consumer
    /// (checkSourceElementUnreachable 86804-86805); same utf16
    /// mapping and duplicate-drop as the node-anchored path.
    /// tsrs-native: explicit-span twin of diagnostic_for_node.
    pub fn error_at_byte_range(
        &mut self,
        node_for_file: NodeId,
        start_byte: usize,
        end_byte: usize,
        message: &'static DiagnosticMessage,
    ) -> usize {
        self.error_at_byte_range_with_args(node_for_file, start_byte, end_byte, message, &[])
    }

    /// tsrs-native: explicit-span diagnostic twin with message
    /// substitution arguments.
    pub fn error_at_byte_range_with_args(
        &mut self,
        node_for_file: NodeId,
        start_byte: usize,
        end_byte: usize,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> usize {
        let source = self.binder.source_of_node(node_for_file);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .positions()
                .byte_to_utf16((byte) as u32)
                .unwrap_or(byte as u32)
        };
        let start_utf16 = to_utf16(start_byte);
        let end_utf16 = to_utf16(end_byte);
        let args = args
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect::<Vec<_>>();
        let diagnostic = Diagnostic::new_js(
            Some(source.file_name.clone()),
            Some(start_utf16),
            Some(end_utf16.saturating_sub(start_utf16)),
            MessageChain::new(message, &args),
        );
        self.push_error_diagnostic(diagnostic)
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
    /// tsrs-native: Rust Diagnostic storage/deduplication adapter;
    /// tsc mutates its diagnostics array directly.
    pub fn push_error_diagnostic(&mut self, diagnostic: Diagnostic) -> usize {
        self.diagnostics.insert_unique(diagnostic)
    }

    /// tsrs-native: Rust diagnostic-construction adapter that attaches
    /// prebuilt RelatedInfo values to the ledgered node diagnostic.
    pub fn error_at_with_related(
        &mut self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[&str],
        related: Vec<tsc_diagnostics::RelatedInfo>,
    ) -> usize {
        let mut diagnostic = self.create_error(location, message, args);
        diagnostic.related = related;
        self.push_error_diagnostic(diagnostic)
    }

    /// tsrs-native: JS twin of error_at_with_related (sibling tsrs-native): attaches
    /// related info to createError
    pub fn error_at_with_related_js(
        &mut self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[JsStr<'_>],
        related: Vec<tsc_diagnostics::RelatedInfo>,
    ) -> usize {
        let mut diagnostic = self.create_error_js(location, message, args);
        diagnostic.related = related;
        self.push_error_diagnostic(diagnostic)
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
        let args = args.iter().map(|arg| JsStr::from(*arg)).collect::<Vec<_>>();
        self.lookup_or_issue_error_js(location, message, &args)
    }

    /// tsc-port: createTypeChecker.lookupOrIssueError @6.0.3
    /// tsc-hash: 9571aad04fba17397e7740b9b0f7b02e8646fb85b89ae01858ad7879ead111d6
    /// tsc-span: _tsc.js:47565-47574
    /// JS-valued twin of `lookup_or_issue_error`; keep their diagnostic behavior aligned.
    pub fn lookup_or_issue_error_js(
        &mut self,
        location: Option<NodeId>,
        message: &'static DiagnosticMessage,
        args: &[JsStr<'_>],
    ) -> usize {
        self.diagnostics.attempts += 1;
        let diagnostic = self.create_error_js(location, message, args);
        // tsgo's lookupOrIssueError adds through DiagnosticsCollection.Add
        // (ast/diagnostic.go:269-297), which returns an existing diagnostic
        // only when it is equal related information included. A diagnostic
        // that already carries related information is not reused; the
        // final dedup merges their related information instead.
        let found = self.diagnostics.iter().position(|existing| {
            existing.file_name == diagnostic.file_name
                && existing.start == diagnostic.start
                && existing.length == diagnostic.length
                && existing.code() == diagnostic.code()
                && existing.message == diagnostic.message
                && existing.related.is_empty()
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
#[path = "../tests/unit/state/test_support.rs"]
pub(crate) mod test_support;

/// The active-mapper instantiation cache key: tsc's
/// `type.id + getAliasId(aliasSymbol, aliasTypeArguments)` (73584) as one
/// value instead of a formatted string. The alias arguments count only with
/// an alias symbol, as in getAliasId, and are interned as one type-list id.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct MapperCacheKey {
    pub(crate) ty: TypeId,
    pub(crate) alias_symbol: Option<SymbolId>,
    pub(crate) alias_arguments: Option<tsc_types::TypeListId>,
}

/// One active mapper scope. Both tables expire together when inference
/// resolves or the scope is popped, including a speculative rollback.
///
/// A scope's first plain answers lie in two inline arrays in the order they
/// came and are found by a scan; the map takes the ninth on. Of the scopes
/// that are popped, 97 to 99 in a hundred never hold more than eight
/// (counted on zod, Effect, Next.js and VS Code, 2026-11-26), and a run
/// stores one to 1.6 million answers: each was a hashed insert.
pub(crate) struct ActiveMapperCache {
    front_keys: [TypeId; ACTIVE_MAPPER_FRONT],
    front_values: [TypeId; ACTIVE_MAPPER_FRONT],
    front_len: u8,
    plain: rustc_hash::FxHashMap<TypeId, TypeId>,
    aliased: rustc_hash::FxHashMap<MapperCacheKey, TypeId>,
}

/// The plain answers an active mapper scope keeps inline.
const ACTIVE_MAPPER_FRONT: usize = 8;

impl Default for ActiveMapperCache {
    fn default() -> Self {
        Self {
            front_keys: [TypeId::new(0); ACTIVE_MAPPER_FRONT],
            front_values: [TypeId::new(0); ACTIVE_MAPPER_FRONT],
            front_len: 0,
            plain: Default::default(),
            aliased: Default::default(),
        }
    }
}

impl ActiveMapperCache {
    #[inline]
    fn front_position(&self, ty: TypeId) -> Option<usize> {
        self.front_keys[..usize::from(self.front_len)]
            .iter()
            .position(|&key| key == ty)
    }

    pub(crate) fn get(&self, key: &MapperCacheKey) -> Option<&TypeId> {
        if key.alias_symbol.is_none() && key.alias_arguments.is_none() {
            if let Some(position) = self.front_position(key.ty) {
                return Some(&self.front_values[position]);
            }
            // The map holds answers only once the inline entries are full.
            if self.plain.is_empty() {
                None
            } else {
                self.plain.get(&key.ty)
            }
        } else {
            self.aliased.get(key)
        }
    }

    pub(crate) fn insert(&mut self, key: MapperCacheKey, value: TypeId) {
        if key.alias_symbol.is_none() && key.alias_arguments.is_none() {
            if let Some(position) = self.front_position(key.ty) {
                // As a map's insert: the later answer replaces the earlier.
                self.front_values[position] = value;
            } else if usize::from(self.front_len) < ACTIVE_MAPPER_FRONT {
                let at = usize::from(self.front_len);
                self.front_keys[at] = key.ty;
                self.front_values[at] = value;
                self.front_len += 1;
            } else {
                self.plain.insert(key.ty, value);
            }
        } else {
            self.aliased.insert(key, value);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.front_len = 0;
        self.plain.clear();
        self.aliased.clear();
    }

    /// A later scope at this depth can reuse an ordinary map. Release a
    /// large allocation when the exiting scope used little of its capacity
    /// (a scope that stayed within its inline entries used none of it, and
    /// its clear costs nothing: the map keeps its storage).
    pub(crate) fn clear_on_pop(&mut self) {
        fn clear_map<K, V>(map: &mut rustc_hash::FxHashMap<K, V>) {
            let capacity = map.capacity();
            if capacity >= 1024 && !map.is_empty() && map.len() <= capacity / 8 {
                *map = Default::default();
            } else {
                map.clear();
            }
        }
        self.front_len = 0;
        clear_map(&mut self.plain);
        clear_map(&mut self.aliased);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        usize::from(self.front_len) + self.plain.len() + self.aliased.len()
    }

    #[cfg(test)]
    pub(crate) fn capacities(&self) -> (usize, usize) {
        (self.plain.capacity(), self.aliased.capacity())
    }
}

impl<T: Clone> SharedList<T> {
    pub(crate) fn heap_bytes_once(&self, usage: &mut tsc_diagnostics::MemoryUsage) -> usize {
        self.0.as_ref().map_or(0, |list| {
            if usage.claim_arc(list) {
                self.heap_bytes()
            } else {
                0
            }
        })
    }
}

impl CheckerState<'_> {
    /// Keeps an index info's computed-name declarations; the handle names
    /// them for every copy of the info.
    pub(crate) fn add_index_info_components(&mut self, nodes: Vec<NodeId>) -> IndexInfoComponents {
        self.index_info_components.push(nodes.into_boxed_slice());
        let count = u32::try_from(self.index_info_components.len())
            .expect("fewer index info component lists than u32::MAX");
        IndexInfoComponents(std::num::NonZeroU32::new(count).expect("a pushed list counts one"))
    }

    /// The computed-name declarations an index info's handle names.
    pub(crate) fn index_info_components(&self, components: IndexInfoComponents) -> &[NodeId] {
        &self.index_info_components[components.0.get() as usize - 1]
    }
}

impl ResolvedMembers {
    fn extras_heap_bytes_once(&self, usage: &mut tsc_diagnostics::MemoryUsage) -> usize {
        self.extras.as_boxed().map_or(0, |extras| {
            std::mem::size_of::<MembersExtras>()
                + extras.call_signatures.heap_bytes_once(usage)
                + extras.construct_signatures.heap_bytes_once(usage)
                + extras.index_infos.heap_bytes_once(usage)
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/state/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/state/resolution_unwind_tests.rs"]
mod resolution_unwind_tests;
