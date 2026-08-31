//! ProgramBinder — the checker's program-wide view over per-file
//! binder runs (M4 5.0).
//!
//! tsc's checker sees one heap: nodes and symbols from every file (and
//! the checker's own transient symbols) share an identity space. The
//! greenfield equivalent: each file parses with a NodeId/NodeArrayId
//! base and binds with a SymbolId base, so ids are program-unique by
//! construction and this struct only routes an id to its owning
//! per-file arena. Owner indexes are independently base-sorted and may contain
//! holes; cached documents retain their leases when Program order changes.
//! Checker transient symbols use the tagged high half of `SymbolId`.

use std::collections::HashMap;
use std::sync::Arc;

use tsc_binder::{BindData, Binder, BinderWorker, Symbol, SymbolArena, SymbolId, SymbolTable};
use tsc_diagnostics::{ByteTextChangeRange, DocumentVersion, TextSnapshot};
use tsc_syntax::{
    IncrementalParseError, IncrementalParseOptions, IncrementalParseStats, NodeArray, NodeArrayId,
    NodeId, ParseOptions, SourceFile,
};
use tsc_types::{
    CompilerOptions, IdentityDomain, IdentityError, SymbolFlags, TRANSIENT_SYMBOL_BIT,
};

/// Immutable parsed source handle retained by a Program snapshot.
#[derive(Clone, Debug)]
pub struct ParsedDocument {
    pub source: Arc<SourceFile>,
}

impl ParsedDocument {
    /// tsrs-native: constructs an immutable parsed-document handle.
    pub fn new(source: Arc<SourceFile>) -> Self {
        Self { source }
    }
}

/// Immutable result of one completed bind. The worker that produced `data`
/// is gone before this record is published.
#[derive(Clone, Debug)]
pub struct BoundDocument {
    pub parsed: Arc<ParsedDocument>,
    pub data: BindData,
}

impl BoundDocument {
    /// tsrs-native: publishes a completed parsed/bound document pair.
    pub fn new(parsed: Arc<ParsedDocument>, data: BindData) -> Self {
        Self { parsed, data }
    }

    /// tsrs-native: projects the parsed source retained by this bound record.
    pub fn source(&self) -> &SourceFile {
        &self.parsed.source
    }
}

/// The script-kind part of a document-registry address.
///
/// A path alone is not a sufficient cache key: a host may assign a different
/// script kind to the same extension, and JSON has a different parser entry
/// point from TypeScript. `Other` is retained instead of collapsing unknown
/// extensions into one bucket so a future host override cannot reuse a tree
/// produced for another kind.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum DocumentScriptKind {
    TypeScript,
    Tsx,
    JavaScript,
    Jsx,
    Json,
    Other(String),
}

/// Complete address of one registry document variant.
///
/// The registry namespace is deliberately part of the address even though a
/// `DocumentRegistry` also checks it. This keeps an address self-describing
/// when it is recorded in a Program-building trace. The current implementation
/// stores the full compiler-option bag as the source/bind bucket. That is
/// conservative (checker-only option projections can be split for now), but
/// it cannot reuse stale parse or bind state when a new source-affecting read
/// is added before the generated projection is tightened.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DocumentAddress {
    namespace: String,
    path: String,
    script_kind: DocumentScriptKind,
    compiler_options: CompilerOptions,
    implied_node_format: Option<i32>,
    force_external_module: bool,
    detect_external_module_from_jsx: bool,
}

impl DocumentAddress {
    /// tsrs-native: constructs an address for the pinned registry namespace.
    pub fn new(
        namespace: impl Into<String>,
        path: impl Into<String>,
        script_kind: DocumentScriptKind,
        compiler_options: CompilerOptions,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            path: path.into(),
            script_kind,
            compiler_options,
            implied_node_format: None,
            force_external_module: false,
            detect_external_module_from_jsx: false,
        }
    }

    /// tsrs-native: adds the module-format facts that complete the address key.
    pub fn with_module_facts(
        mut self,
        implied_node_format: Option<i32>,
        force_external_module: bool,
        detect_external_module_from_jsx: bool,
    ) -> Self {
        self.implied_node_format = implied_node_format;
        self.force_external_module = force_external_module;
        self.detect_external_module_from_jsx = detect_external_module_from_jsx;
        self
    }

    /// tsrs-native: returns the registry namespace component.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// tsrs-native: returns the host path component.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// tsrs-native: returns the parser script-kind component.
    pub fn script_kind(&self) -> &DocumentScriptKind {
        &self.script_kind
    }

    /// tsrs-native: returns the conservative source/bind option bucket.
    pub fn compiler_options(&self) -> &CompilerOptions {
        &self.compiler_options
    }

    /// tsrs-native: returns the implied module format component.
    pub const fn implied_node_format(&self) -> Option<i32> {
        self.implied_node_format
    }

    /// tsrs-native: returns the forced external-module fact.
    pub const fn force_external_module(&self) -> bool {
        self.force_external_module
    }

    /// tsrs-native: returns the JSX external-module detection fact.
    pub const fn detect_external_module_from_jsx(&self) -> bool {
        self.detect_external_module_from_jsx
    }
}

/// A reference-counted immutable document handle returned by
/// [`DocumentRegistry::acquire`] or [`DocumentRegistry::update`].
///
/// The handle is intentionally not `Clone`: each active Program snapshot must
/// acquire its own reference and release it exactly once. Cloning the inner
/// `Arc<BoundDocument>` is allowed for the snapshot itself and does not alter
/// registry accounting.
#[derive(Debug)]
pub struct DocumentLease {
    generation: u64,
    address: DocumentAddress,
    document: Arc<BoundDocument>,
}

impl DocumentLease {
    /// tsrs-native: returns the immutable document retained by this lease.
    pub fn document(&self) -> &Arc<BoundDocument> {
        &self.document
    }

    /// tsrs-native: returns the address retained by this lease.
    pub fn address(&self) -> &DocumentAddress {
        &self.address
    }

    /// tsrs-native: returns the host version retained by this lease.
    pub fn version(&self) -> &DocumentVersion {
        self.document.source().snapshot().document_version()
    }

    fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Debug)]
struct RegistryEntry {
    generation: u64,
    snapshot: Arc<TextSnapshot>,
    document: Arc<BoundDocument>,
    references: usize,
}

/// Fail-closed errors for the minimal L0 document registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentRegistryError {
    NamespaceMismatch {
        expected: String,
        actual: String,
    },
    VersionTextMismatch {
        path: String,
        version: DocumentVersion,
    },
    BuiltDocumentDoesNotOwnSnapshot {
        path: String,
    },
    BuiltDocumentPathMismatch {
        expected: String,
        actual: String,
    },
    UnknownLease {
        generation: u64,
    },
    PreviousLeaseAddressMismatch,
    IncrementalParse(IncrementalParseError),
    BindIdentity(IdentityError),
}

impl std::fmt::Display for DocumentRegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NamespaceMismatch { expected, actual } => write!(
                formatter,
                "document registry namespace mismatch: expected {expected:?}, got {actual:?}"
            ),
            Self::VersionTextMismatch { path, version } => write!(
                formatter,
                "document {path:?} changed text without changing host version {:?}",
                version.as_str()
            ),
            Self::BuiltDocumentDoesNotOwnSnapshot { path } => write!(
                formatter,
                "built document {path:?} does not retain the supplied snapshot"
            ),
            Self::BuiltDocumentPathMismatch { expected, actual } => write!(
                formatter,
                "built document path mismatch: expected {expected:?}, got {actual:?}"
            ),
            Self::UnknownLease { generation } => {
                write!(
                    formatter,
                    "unknown or already released document lease {generation}"
                )
            }
            Self::PreviousLeaseAddressMismatch => formatter
                .write_str("incremental document update used a previous lease for another address"),
            Self::IncrementalParse(error) => error.fmt(formatter),
            Self::BindIdentity(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for DocumentRegistryError {}

impl From<IncrementalParseError> for DocumentRegistryError {
    fn from(error: IncrementalParseError) -> Self {
        Self::IncrementalParse(error)
    }
}

#[derive(Debug)]
pub struct IncrementalDocumentUpdate {
    pub lease: DocumentLease,
    pub parse_stats: IncrementalParseStats,
}

#[derive(Clone, Debug, Default)]
pub struct IncrementalDocumentOptions {
    pub parse: ParseOptions,
    pub incremental: IncrementalParseOptions,
}

/// Minimal non-global registry for immutable parsed/bound documents.
///
/// Entries are retained only while at least one explicit lease is active.
/// Different versions of one address may coexist while an older Program is
/// still alive; each version is removed as soon as its last lease is released.
/// This is deliberately a synchronous building block. Synchronization belongs
/// to the future service/project owner, not to the immutable syntax or bind
/// records.
#[derive(Debug)]
pub struct DocumentRegistry {
    namespace: String,
    entries: HashMap<DocumentAddress, Vec<RegistryEntry>>,
    next_generation: u64,
}

impl DocumentRegistry {
    /// tsrs-native: constructs a synchronous, non-global registry namespace.
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            entries: HashMap::new(),
            next_generation: 0,
        }
    }

    /// tsrs-native: returns the namespace owned by this registry.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// tsrs-native: acquires or atomically publishes an exact document variant.
    /// Acquire an existing exact `(address, host version, text)` record, or
    /// build and publish one atomically from the supplied closure.
    pub fn acquire(
        &mut self,
        address: DocumentAddress,
        snapshot: Arc<TextSnapshot>,
        build: impl FnOnce() -> Arc<BoundDocument>,
    ) -> Result<DocumentLease, DocumentRegistryError> {
        self.check_namespace(&address)?;
        if let Some(entries) = self.entries.get_mut(&address) {
            for entry in entries.iter_mut() {
                if entry.snapshot.document_version() == snapshot.document_version() {
                    if entry.snapshot.text() != snapshot.text() {
                        return Err(DocumentRegistryError::VersionTextMismatch {
                            path: address.path.clone(),
                            version: snapshot.document_version().clone(),
                        });
                    }
                    entry.references = entry
                        .references
                        .checked_add(1)
                        .expect("document registry reference count overflow");
                    return Ok(DocumentLease {
                        generation: entry.generation,
                        address,
                        document: Arc::clone(&entry.document),
                    });
                }
            }
        }

        let document = build();
        if document.source().file_name != address.path {
            return Err(DocumentRegistryError::BuiltDocumentPathMismatch {
                expected: address.path,
                actual: document.source().file_name.clone(),
            });
        }
        if !Arc::ptr_eq(document.source().snapshot(), &snapshot) {
            return Err(DocumentRegistryError::BuiltDocumentDoesNotOwnSnapshot {
                path: document.source().file_name.clone(),
            });
        }

        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("document registry generation overflow");
        let generation = self.next_generation;
        self.entries
            .entry(address.clone())
            .or_default()
            .push(RegistryEntry {
                generation,
                snapshot: Arc::clone(&snapshot),
                document: Arc::clone(&document),
                references: 1,
            });
        Ok(DocumentLease {
            generation,
            address,
            document,
        })
    }

    /// tsrs-native: updates an address through the same fail-closed acquire path.
    /// Publish a new host version at an existing address. The exact same
    /// version still follows the acquire path and therefore cannot silently
    /// replace text under an equal host version.
    pub fn update(
        &mut self,
        address: DocumentAddress,
        snapshot: Arc<TextSnapshot>,
        build: impl FnOnce() -> Arc<BoundDocument>,
    ) -> Result<DocumentLease, DocumentRegistryError> {
        self.acquire(address, snapshot, build)
    }

    /// tsrs-native: publish one immutable successor by incrementally reparsing
    /// and fully rebinding the changed document. The previous lease remains
    /// live until its owning Program explicitly releases it; no old tree is
    /// mutated.
    pub fn update_incrementally(
        &mut self,
        previous: &DocumentLease,
        address: DocumentAddress,
        snapshot: Arc<TextSnapshot>,
        change: ByteTextChangeRange,
        options: IncrementalDocumentOptions,
        domain: &IdentityDomain,
    ) -> Result<IncrementalDocumentUpdate, DocumentRegistryError> {
        self.check_namespace(&address)?;
        if previous.address != address {
            return Err(DocumentRegistryError::PreviousLeaseAddressMismatch);
        }
        let previous_is_live = self.entries.get(&address).is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry.generation == previous.generation()
                    && Arc::ptr_eq(&entry.document, previous.document())
            })
        });
        if !previous_is_live {
            return Err(DocumentRegistryError::UnknownLease {
                generation: previous.generation(),
            });
        }

        if let Some(entries) = self.entries.get_mut(&address) {
            for entry in entries.iter_mut() {
                if entry.snapshot.document_version() == snapshot.document_version() {
                    if entry.snapshot.text() != snapshot.text() {
                        return Err(DocumentRegistryError::VersionTextMismatch {
                            path: address.path.clone(),
                            version: snapshot.document_version().clone(),
                        });
                    }
                    entry.references = entry
                        .references
                        .checked_add(1)
                        .expect("document registry reference count overflow");
                    return Ok(IncrementalDocumentUpdate {
                        lease: DocumentLease {
                            generation: entry.generation,
                            address,
                            document: Arc::clone(&entry.document),
                        },
                        parse_stats: IncrementalParseStats::default(),
                    });
                }
            }
        }

        let updated = tsc_syntax::update_language_service_source_file_in_identity_domain(
            Arc::clone(&previous.document.parsed.source),
            Arc::clone(&snapshot),
            change,
            options.parse,
            options.incremental,
            domain,
        )?;
        let worker = BinderWorker::bind_in_identity_domain(
            &updated.source,
            address.compiler_options(),
            domain,
        )
        .map_err(DocumentRegistryError::BindIdentity)?;
        let parsed = Arc::new(ParsedDocument::new(Arc::clone(&updated.source)));
        let document = Arc::new(BoundDocument::new(parsed, worker.into_bind_data()));

        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("document registry generation overflow");
        let generation = self.next_generation;
        self.entries
            .entry(address.clone())
            .or_default()
            .push(RegistryEntry {
                generation,
                snapshot,
                document: Arc::clone(&document),
                references: 1,
            });
        Ok(IncrementalDocumentUpdate {
            lease: DocumentLease {
                generation,
                address,
                document,
            },
            parse_stats: updated.stats,
        })
    }

    /// tsrs-native: releases one lease and reclaims its final live variant.
    /// Release exactly one acquired reference. When the last reference to a
    /// version is released, its registry entry disappears immediately.
    pub fn release(&mut self, lease: DocumentLease) -> Result<(), DocumentRegistryError> {
        self.check_namespace(&lease.address)?;
        let Some(entries) = self.entries.get_mut(&lease.address) else {
            return Err(DocumentRegistryError::UnknownLease {
                generation: lease.generation(),
            });
        };
        let Some(index) = entries
            .iter()
            .position(|entry| entry.generation == lease.generation())
        else {
            return Err(DocumentRegistryError::UnknownLease {
                generation: lease.generation(),
            });
        };
        let entry = &mut entries[index];
        if entry.references == 0 {
            return Err(DocumentRegistryError::UnknownLease {
                generation: lease.generation(),
            });
        }
        entry.references -= 1;
        if entry.references == 0 {
            entries.remove(index);
        }
        if entries.is_empty() {
            self.entries.remove(&lease.address);
        }
        Ok(())
    }

    /// tsrs-native: returns the number of live address/version variants.
    pub fn active_entry_count(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }

    /// tsrs-native: returns the number of explicit live leases.
    pub fn active_reference_count(&self) -> usize {
        self.entries
            .values()
            .flat_map(|entries| entries.iter())
            .map(|entry| entry.references)
            .sum()
    }

    fn check_namespace(&self, address: &DocumentAddress) -> Result<(), DocumentRegistryError> {
        if address.namespace != self.namespace {
            return Err(DocumentRegistryError::NamespaceMismatch {
                expected: self.namespace.clone(),
                actual: address.namespace.clone(),
            });
        }
        Ok(())
    }
}

impl Default for DocumentRegistry {
    fn default() -> Self {
        Self::new("typescript-6.0.3")
    }
}

/// Typed publication failures for the one-shot store. A failed source or bind
/// never enters the store, so callers can contain the failure before creating
/// a ProgramSnapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EphemeralDocumentStoreError {
    SourceIdentityDomainMismatch,
    BindIdentityDomainMismatch,
}

impl std::fmt::Display for EphemeralDocumentStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::SourceIdentityDomainMismatch => {
                "ephemeral document source belongs to a different identity domain"
            }
            Self::BindIdentityDomainMismatch => {
                "ephemeral document bind data belongs to a different identity domain"
            }
        })
    }
}

impl std::error::Error for EphemeralDocumentStoreError {}

/// One-shot document store used by H0.
///
/// It owns direct document slots and an identity domain for exactly one
/// checker run. There is no global lookup, retained version, or background
/// owner. A reusable registry is exercised separately through
/// [`DocumentRegistry`]; both publish the same immutable `BoundDocument`
/// handles consumed by [`ProgramSnapshot`].
#[derive(Debug)]
pub struct EphemeralDocumentStore {
    identity_domain: IdentityDomain,
    documents: Vec<Arc<BoundDocument>>,
}

impl EphemeralDocumentStore {
    /// tsrs-native: constructs a one-shot store for one identity domain.
    pub fn new(identity_domain: IdentityDomain) -> Self {
        Self {
            identity_domain,
            documents: Vec::new(),
        }
    }

    /// tsrs-native: constructs a one-shot store from completed document handles.
    pub fn with_documents(
        identity_domain: IdentityDomain,
        documents: impl IntoIterator<Item = Arc<BoundDocument>>,
    ) -> Self {
        Self {
            identity_domain,
            documents: documents.into_iter().collect(),
        }
    }

    /// tsrs-native: returns the store's identity domain.
    pub fn identity_domain(&self) -> &IdentityDomain {
        &self.identity_domain
    }

    /// tsrs-native: returns the direct immutable document slots.
    pub fn documents(&self) -> &[Arc<BoundDocument>] {
        &self.documents
    }

    /// tsrs-native: publishes a completed bind after ownership validation.
    /// Publish a fully completed bind. The worker must already have been
    /// consumed, so a partial bind can never enter the one-shot store.
    pub fn publish(
        &mut self,
        source: Arc<SourceFile>,
        data: BindData,
    ) -> Result<Arc<BoundDocument>, EphemeralDocumentStoreError> {
        if !source.identity_owned_by(&self.identity_domain) {
            return Err(EphemeralDocumentStoreError::SourceIdentityDomainMismatch);
        }
        if !data.identity_owned_by(&self.identity_domain) {
            return Err(EphemeralDocumentStoreError::BindIdentityDomainMismatch);
        }
        let parsed = Arc::new(ParsedDocument::new(source));
        let document = Arc::new(BoundDocument::new(parsed, data));
        self.documents.push(Arc::clone(&document));
        Ok(document)
    }

    /// tsrs-native: transfers the one-shot slots into an immutable Program snapshot.
    pub fn into_snapshot(self, lib_count: usize) -> Result<ProgramSnapshot, ProgramIdentityError> {
        ProgramSnapshot::new(self.documents, lib_count)
    }

    /// tsrs-native: transfers one-shot document slots and typed membership
    /// facts into Rust's immutable Program snapshot.
    ///
    /// Transfer completed documents together with Program-owned source facts.
    /// Facts deliberately do not live on `BoundDocument`: one cached document
    /// may be a default library in one Program and an ordinary source in
    /// another.
    pub fn into_snapshot_with_file_facts(
        self,
        file_facts: Vec<ProgramFileFacts>,
    ) -> Result<ProgramSnapshot, ProgramIdentityError> {
        ProgramSnapshot::new_with_file_facts(self.documents, file_facts)
    }
}

/// Stable identity of one document in a [`ProgramSnapshot`]'s source order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProgramFileId(u32);

impl ProgramFileId {
    /// tsrs-native: constructs Rust's compact typed Program-file identity.
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// tsrs-native: exposes the stored integer at an explicit identity boundary.
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// tsrs-native: converts the typed Program-file identity for Vec indexing.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Immutable host/program facts for one source-file membership.
///
/// This is intentionally separate from parsed/bound document state. Default
/// library membership is assigned by `createProgram`; neither a `.d.ts`
/// extension nor a `lib.*` basename is an authoritative substitute.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProgramFileFacts {
    default_library: bool,
}

impl ProgramFileFacts {
    pub const ORDINARY: Self = Self {
        default_library: false,
    };
    pub const DEFAULT_LIBRARY: Self = Self {
        default_library: true,
    };

    /// tsrs-native: reads Program-owned default-library membership instead of
    /// inferring it from a parsed document or path.
    pub const fn is_default_library(self) -> bool {
        self.default_library
    }
}

/// Ordered immutable document handles and their per-Program source facts.
/// The checker borrows this snapshot by cloning only its `Arc` handles; it
/// never reparses or rebinds an unchanged document while constructing a fresh
/// checker session.
#[derive(Clone, Debug)]
pub struct ProgramSnapshot {
    documents: Vec<Arc<BoundDocument>>,
    file_facts: Vec<ProgramFileFacts>,
}

impl ProgramSnapshot {
    /// tsrs-native: compatibility constructor for Rust's immutable Program
    /// snapshot, projecting a legacy library-prefix count into typed facts.
    ///
    /// Compatibility constructor for callers whose authoritative library set
    /// is an ordered prefix. New program builders should pass explicit facts
    /// through [`Self::new_with_file_facts`].
    pub fn new(
        documents: Vec<Arc<BoundDocument>>,
        lib_count: usize,
    ) -> Result<Self, ProgramIdentityError> {
        if lib_count > documents.len() {
            return Err(ProgramIdentityError::FileFactsLength {
                documents: documents.len(),
                facts: lib_count,
            });
        }
        let mut file_facts = vec![ProgramFileFacts::DEFAULT_LIBRARY; lib_count];
        file_facts.resize(documents.len(), ProgramFileFacts::ORDINARY);
        Self::new_with_file_facts(documents, file_facts)
    }

    /// tsrs-native: validates and publishes Rust-owned immutable document
    /// handles together with per-Program source facts.
    ///
    /// Validate and publish ordered immutable Program handles with explicit
    /// membership facts supplied by the Program builder.
    pub fn new_with_file_facts(
        documents: Vec<Arc<BoundDocument>>,
        file_facts: Vec<ProgramFileFacts>,
    ) -> Result<Self, ProgramIdentityError> {
        if documents.is_empty() {
            return Err(ProgramIdentityError::EmptyProgram);
        }
        if file_facts.len() != documents.len() {
            return Err(ProgramIdentityError::FileFactsLength {
                documents: documents.len(),
                facts: file_facts.len(),
            });
        }
        Ok(Self {
            documents,
            file_facts,
        })
    }

    /// tsrs-native: returns all ordered immutable document handles.
    pub fn documents(&self) -> &[Arc<BoundDocument>] {
        &self.documents
    }

    /// tsrs-native: returns one immutable document handle by Program order.
    pub fn document(&self, index: usize) -> &Arc<BoundDocument> {
        &self.documents[index]
    }

    /// tsrs-native: indexes immutable per-Program facts by typed file identity.
    pub fn file_facts(&self, file: ProgramFileId) -> ProgramFileFacts {
        self.file_facts[file.index()]
    }

    /// tsrs-native: iterates Rust's dense typed Program-file identity domain.
    pub fn file_ids(&self) -> impl ExactSizeIterator<Item = ProgramFileId> + '_ {
        (0..self.documents.len()).map(|index| {
            ProgramFileId::from_raw(u32::try_from(index).expect("Program file index overflow"))
        })
    }

    /// tsrs-native: returns the number of ordered Program documents.
    pub fn file_count(&self) -> usize {
        self.documents.len()
    }

    /// tsrs-native: derives the legacy library count from explicit immutable
    /// per-file membership facts.
    ///
    /// Return the number of sources whose Program membership is a default
    /// library. No prefix inference is performed.
    pub fn lib_count(&self) -> usize {
        self.file_facts
            .iter()
            .filter(|facts| facts.is_default_library())
            .count()
    }
}

struct LegacyProgramEntry<'a> {
    binder: &'a Binder<'a>,
    data: BindData,
}

enum ProgramEntry<'a> {
    Legacy(Box<LegacyProgramEntry<'a>>),
    Owned(&'a Arc<BoundDocument>),
}

impl<'a> ProgramEntry<'a> {
    fn source(&self) -> &'a SourceFile {
        match self {
            Self::Legacy(entry) => entry.binder.source,
            Self::Owned(document) => document.source(),
        }
    }

    fn data(&self) -> &BindData {
        match self {
            Self::Legacy(entry) => &entry.data,
            Self::Owned(document) => &document.data,
        }
    }
}

/// Source projection used by compatibility callers that previously iterated
/// over `Binder` values. Checker internals access the immutable `BindData`
/// through `ProgramBinder::file`.
pub struct ProgramFile<'a> {
    pub source: &'a SourceFile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ArenaOwner {
    start: u32,
    end: u32,
    file: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramIdentitySpace {
    Node,
    NodeArray,
    PersistentSymbol,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramIdentityError {
    EmptyProgram,
    FileFactsLength {
        documents: usize,
        facts: usize,
    },
    EmptyOwner {
        space: ProgramIdentitySpace,
        file: usize,
    },
    Overlap {
        space: ProgramIdentitySpace,
        first_file: usize,
        second_file: usize,
        first_end: u32,
        second_start: u32,
    },
    PersistentSymbolUsesTransientPartition {
        file: usize,
        start: u32,
        end: u32,
    },
    PartialIdentityOwnership {
        file: usize,
    },
    MixedIdentityOwnership,
    IdentityDomainMismatch {
        file: usize,
    },
}

impl std::fmt::Display for ProgramIdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyProgram => formatter.write_str("a program has no source files"),
            Self::FileFactsLength { documents, facts } => write!(
                formatter,
                "a Program has {documents} documents but {facts} source-fact rows"
            ),
            Self::EmptyOwner { space, file } => {
                write!(formatter, "program file {file} has an empty {space:?} arena")
            }
            Self::Overlap {
                space,
                first_file,
                second_file,
                first_end,
                second_start,
            } => write!(
                formatter,
                "program {space:?} owners overlap: file {first_file} ends at {first_end}, file {second_file} starts at {second_start}"
            ),
            Self::PersistentSymbolUsesTransientPartition { file, start, end } => write!(
                formatter,
                "program file {file} persistent symbols use tagged range {start}..{end}"
            ),
            Self::PartialIdentityOwnership { file } => write!(
                formatter,
                "program file {file} has only a subset of the required identity leases"
            ),
            Self::MixedIdentityOwnership => formatter.write_str(
                "a Program cannot mix identity-owned and unmanaged source/bind records",
            ),
            Self::IdentityDomainMismatch { file } => write!(
                formatter,
                "program file {file} belongs to a different identity domain",
            ),
        }
    }
}

impl std::error::Error for ProgramIdentityError {}

pub struct ProgramBinder<'a> {
    /// Ordered document entries. Legacy unit-test callers retain a borrowed
    /// Binder worker, while production snapshots retain only an Arc-owned
    /// BoundDocument. In both cases checker code sees the same immutable
    /// BindData projection.
    file_entries: Vec<ProgramEntry<'a>>,
    /// Program-owned facts copied from the immutable snapshot. They remain
    /// session-local and never mutate shared parsed/bound documents.
    file_facts: Vec<ProgramFileFacts>,
    /// Node-id intervals in parse allocation order (ascending by start).
    node_owners: Vec<ArenaOwner>,
    /// Node-array-id intervals in parse allocation order.
    array_owners: Vec<ArenaOwner>,
    /// Persistent symbol intervals in identity allocation order.
    symbol_owners: Vec<ArenaOwner>,
    /// Checker-side symbols (tsc createSymbol 47652 adds Transient).
    transient: SymbolArena,
}

fn validate_owner_intervals(
    space: ProgramIdentitySpace,
    owners: &[ArenaOwner],
    require_every_owner_nonempty: bool,
) -> Result<(), ProgramIdentityError> {
    if require_every_owner_nonempty {
        for owner in owners {
            if owner.start >= owner.end {
                return Err(ProgramIdentityError::EmptyOwner {
                    space,
                    file: owner.file,
                });
            }
        }
    }
    for pair in owners.windows(2) {
        if pair[1].start < pair[0].end {
            return Err(ProgramIdentityError::Overlap {
                space,
                first_file: pair[0].file,
                second_file: pair[1].file,
                first_end: pair[0].end,
                second_start: pair[1].start,
            });
        }
    }
    Ok(())
}

fn validate_identity_domains(entries: &[ProgramEntry<'_>]) -> Result<(), ProgramIdentityError> {
    let mut program_anchor: Option<&tsc_types::IdentityLease> = None;
    let mut managed_program = None;
    for (file, entry) in entries.iter().enumerate() {
        let source = entry.source();
        let data = entry.data();
        let leases = [
            source.node_identity_lease(),
            source.array_identity_lease(),
            data.symbol_identity_lease(),
            data.private_name_serial_lease(),
        ];
        let present = leases.iter().filter(|lease| lease.is_some()).count();
        let managed = match present {
            0 => false,
            4 => true,
            _ => return Err(ProgramIdentityError::PartialIdentityOwnership { file }),
        };
        if let Some(expected) = managed_program {
            if expected != managed {
                return Err(ProgramIdentityError::MixedIdentityOwnership);
            }
        } else {
            managed_program = Some(managed);
        }
        if managed {
            let anchor = leases[0].expect("managed source has its node lease");
            if leases
                .iter()
                .flatten()
                .any(|lease| !anchor.same_domain(lease))
            {
                return Err(ProgramIdentityError::IdentityDomainMismatch { file });
            }
            if let Some(program_anchor) = program_anchor {
                if !program_anchor.same_domain(anchor) {
                    return Err(ProgramIdentityError::IdentityDomainMismatch { file });
                }
            } else {
                program_anchor = Some(anchor);
            }
        }
    }
    Ok(())
}

impl<'a> ProgramBinder<'a> {
    /// tsrs-native: constructs the multi-file arena routing tables; tsc
    /// nodes and symbols are direct JavaScript references.
    pub fn new(file_binders: Vec<&'a Binder<'a>>) -> Self {
        Self::try_new(file_binders).expect("invalid Program identity ownership")
    }

    /// tsrs-native: fallible multi-file numeric arena routing constructor;
    /// tsc stores direct object references and has no identity-domain check.
    pub fn try_new(file_binders: Vec<&'a Binder<'a>>) -> Result<Self, ProgramIdentityError> {
        if file_binders.is_empty() {
            return Err(ProgramIdentityError::EmptyProgram);
        }
        let entries = file_binders
            .iter()
            .map(|binder| {
                ProgramEntry::Legacy(Box::new(LegacyProgramEntry {
                    binder,
                    data: BindData::from_binder(binder),
                }))
            })
            .collect::<Vec<_>>();
        Self::try_new_entries(entries)
    }

    /// Construct a checker view over an owned immutable ProgramSnapshot.
    /// tsrs-native: fresh checker view over immutable snapshot handles.
    /// Only the snapshot's Arc handles are cloned; parsed trees and bind
    /// results remain shared and no worker is retained.
    pub fn from_snapshot(snapshot: &'a ProgramSnapshot) -> Self {
        Self::try_from_snapshot(snapshot).expect("invalid Program snapshot identity ownership")
    }

    /// tsrs-native: fallible snapshot adapter used by fresh checker sessions.
    pub fn try_from_snapshot(snapshot: &'a ProgramSnapshot) -> Result<Self, ProgramIdentityError> {
        if snapshot.documents.is_empty() {
            return Err(ProgramIdentityError::EmptyProgram);
        }
        let entries = snapshot
            .documents
            .iter()
            .map(ProgramEntry::Owned)
            .collect::<Vec<_>>();
        Self::try_new_entries_with_file_facts(entries, snapshot.file_facts.clone())
    }

    fn try_new_entries(entries: Vec<ProgramEntry<'a>>) -> Result<Self, ProgramIdentityError> {
        let file_facts = vec![ProgramFileFacts::ORDINARY; entries.len()];
        Self::try_new_entries_with_file_facts(entries, file_facts)
    }

    fn try_new_entries_with_file_facts(
        file_entries: Vec<ProgramEntry<'a>>,
        file_facts: Vec<ProgramFileFacts>,
    ) -> Result<Self, ProgramIdentityError> {
        if file_entries.is_empty() {
            return Err(ProgramIdentityError::EmptyProgram);
        }
        if file_facts.len() != file_entries.len() {
            return Err(ProgramIdentityError::FileFactsLength {
                documents: file_entries.len(),
                facts: file_facts.len(),
            });
        }
        validate_identity_domains(&file_entries)?;

        let mut node_owners: Vec<ArenaOwner> = file_entries
            .iter()
            .enumerate()
            .map(|(file, entry)| ArenaOwner {
                start: entry.source().arena.node_base(),
                end: entry.source().arena.node_end(),
                file,
            })
            .collect();
        node_owners.sort_unstable_by_key(|owner| (owner.start, owner.end, owner.file));

        let mut array_owners: Vec<ArenaOwner> = file_entries
            .iter()
            .enumerate()
            .map(|(file, entry)| ArenaOwner {
                start: entry.source().arena.array_base(),
                end: entry.source().arena.array_end(),
                file,
            })
            .collect();
        array_owners.sort_unstable_by_key(|owner| (owner.start, owner.end, owner.file));

        let mut symbol_owners: Vec<ArenaOwner> = file_entries
            .iter()
            .enumerate()
            .filter_map(|(file, entry)| {
                let data = entry.data();
                let start = data.symbols.base();
                let end = data.symbols.next_id().0;
                (start != end).then_some(ArenaOwner { start, end, file })
            })
            .collect();
        symbol_owners.sort_unstable_by_key(|owner| (owner.start, owner.end, owner.file));

        validate_owner_intervals(ProgramIdentitySpace::Node, &node_owners, true)?;
        validate_owner_intervals(ProgramIdentitySpace::NodeArray, &array_owners, true)?;
        validate_owner_intervals(
            ProgramIdentitySpace::PersistentSymbol,
            &symbol_owners,
            false,
        )?;
        for owner in &symbol_owners {
            if owner.start >= TRANSIENT_SYMBOL_BIT || owner.end > TRANSIENT_SYMBOL_BIT {
                return Err(
                    ProgramIdentityError::PersistentSymbolUsesTransientPartition {
                        file: owner.file,
                        start: owner.start,
                        end: owner.end,
                    },
                );
            }
        }

        Ok(Self {
            file_entries,
            file_facts,
            node_owners,
            array_owners,
            symbol_owners,
            transient: SymbolArena::with_base(TRANSIENT_SYMBOL_BIT),
        })
    }

    /// tsrs-native: Rust ProgramBinder collection accessor.
    pub fn file_count(&self) -> usize {
        self.file_entries.len()
    }

    /// tsrs-native: iterates the binder view's dense typed Program-file identities.
    pub fn file_ids(&self) -> impl ExactSizeIterator<Item = ProgramFileId> + '_ {
        (0..self.file_entries.len()).map(|index| {
            ProgramFileId::from_raw(u32::try_from(index).expect("Program file index overflow"))
        })
    }

    /// tsrs-native: projects immutable Program membership facts through the
    /// binder view's typed file identity.
    pub fn file_facts(&self, file: ProgramFileId) -> ProgramFileFacts {
        self.file_facts[file.index()]
    }

    /// tsrs-native: Rust ProgramBinder iterator over borrowed file
    /// binders.
    pub fn files(&self) -> impl Iterator<Item = ProgramFile<'_>> + '_ {
        self.file_entries.iter().map(|entry| ProgramFile {
            source: entry.source(),
        })
    }

    /// tsrs-native: Rust ProgramBinder indexed file accessor.
    pub fn file(&self, index: usize) -> &BindData {
        self.file_entries[index].data()
    }

    /// tsrs-native: Rust ProgramBinder SourceFile projection.
    pub fn source(&self, index: usize) -> &'a SourceFile {
        self.file_entries[index].source()
    }

    /// Owning file of a node id (nodes allocate contiguously per file).
    /// tsrs-native: binary-search routing for Rust's process-wide
    /// numeric NodeId arena; tsc carries object identity directly.
    pub fn file_index_of_node(&self, node: NodeId) -> usize {
        Self::owner_file(&self.node_owners, node.0, "NodeId")
    }

    /// Fallible counterpart used at external identity boundaries such as the
    /// checker-owned emit resolver. Ordinary checker code already owns valid
    /// node identities and continues to use [`Self::file_index_of_node`].
    /// tsrs-native: validation for Rust's source-token/node-id pair.
    pub(crate) fn try_file_index_of_node(&self, node: NodeId) -> Option<usize> {
        Self::try_owner_file(&self.node_owners, node.0)
    }

    /// tsrs-native: multi-file arena routing for a numeric NodeId; tsc
    /// carries the SourceFile/object relationship directly.
    pub fn source_of_node(&self, node: NodeId) -> &'a SourceFile {
        self.file_entries[self.file_index_of_node(node)].source()
    }

    fn binder_of_node(&self, node: NodeId) -> &BindData {
        self.file_entries[self.file_index_of_node(node)].data()
    }

    /// Owning file's arena lookup for a node-array id (arrays allocate
    /// contiguously per file, like nodes).
    /// tsrs-native: multi-file arena routing for Rust's numeric
    /// NodeArrayId.
    pub fn node_array(&self, id: NodeArrayId) -> &'a NodeArray {
        let index = Self::owner_file(&self.array_owners, id.0, "NodeArrayId");
        self.file_entries[index].source().arena.node_array(id)
    }

    fn owner_file(owners: &[ArenaOwner], id: u32, kind: &str) -> usize {
        Self::try_owner_file(owners, id)
            .unwrap_or_else(|| panic!("{kind} {id} is outside every program arena"))
    }

    fn try_owner_file(owners: &[ArenaOwner], id: u32) -> Option<usize> {
        let index = owners
            .partition_point(|owner| owner.start <= id)
            .checked_sub(1)?;
        let owner = owners[index];
        (id < owner.end).then_some(owner.file)
    }

    fn owner_of_symbol(&self, id: SymbolId) -> Result<usize, ()> {
        if id.0 & TRANSIENT_SYMBOL_BIT != 0 {
            return Err(());
        }
        Ok(Self::owner_file(
            &self.symbol_owners,
            id.0,
            "persistent SymbolId",
        ))
    }

    /// tsrs-native: routes a numeric SymbolId to its binder or
    /// checker-owned transient arena; tsc carries object references.
    pub fn symbol(&self, id: SymbolId) -> &Symbol {
        match self.owner_of_symbol(id) {
            Ok(file) => match &self.file_entries[file] {
                ProgramEntry::Legacy(entry) => entry.binder.symbols.symbol(id),
                ProgramEntry::Owned(document) => document.data.symbols.symbol(id),
            },
            Err(()) => self.transient.symbol(id),
        }
    }

    /// Fallible counterpart to [`Self::symbol`] for externally supplied
    /// checker identities. Unlike the ordinary accessor, this never routes an
    /// unknown id into an arena index and therefore never panics.
    /// tsrs-native: fallible symbol lookup for the emit-resolver
    /// symbol-token validation boundary (h2-7a-m-2 §4).
    pub(crate) fn try_symbol(&self, id: SymbolId) -> Option<&Symbol> {
        if id.0 & TRANSIENT_SYMBOL_BIT != 0 {
            return self
                .transient
                .contains(id)
                .then(|| self.transient.symbol(id));
        }
        let file = Self::try_owner_file(&self.symbol_owners, id.0)?;
        Some(match &self.file_entries[file] {
            ProgramEntry::Legacy(entry) => entry.binder.symbols.symbol(id),
            ProgramEntry::Owned(document) => document.data.symbols.symbol(id),
        })
    }

    /// TRANSIENT symbols only: file binders are read-only after bind
    /// (shared lib binders make this structural — merge writes go to
    /// transient clones, and the merged-symbol mapping lives on
    /// CheckerState).
    /// tsrs-native: mutable arena routing required by Rust ownership;
    /// tsc mutates Symbol objects directly.
    pub fn symbol_mut(&mut self, id: SymbolId) -> &mut Symbol {
        match self.owner_of_symbol(id) {
            Ok(file) => unreachable!(
                "file-binder symbol {id:?} (file {file}) mutated post-bind —                  mergeSymbol clones non-transient targets and the mergedSymbols                  map lives on CheckerState"
            ),
            Err(()) => self.transient.symbol_mut(id),
        }
    }

    /// tsc-port: createSymbol @6.0.3
    /// tsc-hash: b9b2c65d71ec1e9d3a55d36fe5224e5f31dd618ee1428293b371d2f2881ad16a
    /// tsc-span: _tsc.js:47652-47658
    ///
    /// Checker-side symbol creation: always Transient. (tsc also seeds
    /// links.checkFlags here; ours live in LinksTables and default 0 —
    /// callers that need CheckFlags set them through the links API.)
    pub fn create_symbol(&mut self, flags: SymbolFlags, escaped_name: String) -> SymbolId {
        self.transient
            .alloc(flags | SymbolFlags::TRANSIENT, escaped_name)
    }

    /// tsc container.locals of a scope-owning node.
    /// tsrs-native: binder-table projection for tsc's direct
    /// `container.locals` property access.
    pub fn locals_of(&self, scope: NodeId) -> Option<&SymbolTable> {
        self.binder_of_node(scope).locals.get(&scope)
    }

    /// tsrs-native: validates Rust numeric node ownership while projecting
    /// tsc's direct `container.nextContainer` link from binder side tables.
    ///
    /// tsc `container.nextContainer` in the owning file's binder chain.
    /// A cross-file or unknown identity is rejected so semantic scope walks
    /// cannot accept a generated name after leaving their source domain.
    pub(crate) fn next_container_of(&self, container: NodeId) -> Result<Option<NodeId>, ()> {
        let owner = self.try_file_index_of_node(container).ok_or(())?;
        let next = self
            .binder_of_node(container)
            .next_container
            .get(&container)
            .copied();
        match next {
            Some(next) if self.try_file_index_of_node(next) == Some(owner) => Ok(Some(next)),
            Some(_) => Err(()),
            None => Ok(None),
        }
    }

    /// tsc node.symbol (addDeclarationToSymbol).
    /// tsrs-native: binder-table projection for tsc's direct
    /// `node.symbol` property access.
    pub fn node_symbol(&self, node: NodeId) -> Option<SymbolId> {
        self.binder_of_node(node).node_symbol.get(&node).copied()
    }

    /// The binder's mutable node-flags view (ContainsThis etc.).
    /// tsrs-native: binder-table projection for tsc's direct
    /// `node.flags` property access.
    pub fn flags_of(&self, node: NodeId) -> tsc_types::NodeFlags {
        let source = self.source_of_node(node);
        self.binder_of_node(node)
            .flags_of(node, source.arena.node_base())
    }

    /// tsc isExternalOrCommonJsModule for the file owning `node`.
    /// tsc-port: isExternalOrCommonJsModule @6.0.3
    /// tsc-hash: e395fd4c4d5df1373eb3cc17bc653dfcd8f2e41b9e32d949b3063633dc02c07d
    /// tsc-span: _tsc.js:14119-14121
    pub fn is_external_or_common_js_module_of_node(&self, node: NodeId) -> bool {
        let file = self.file_index_of_node(node);
        self.source(file).external_module_indicator.is_some()
            || self.file_entries[file]
                .data()
                .common_js_module_indicator
                .is_some()
    }

    /// tsc-port: isExternalModule @6.0.3
    /// tsc-hash: 5effe04fdce706cc75f238b5c4efbb1f317b3f6bd665389fb71a79a119e7ceaa
    /// tsc-span: _tsc.js:28910-28912
    ///
    /// For the file owning `node` (getSourceFileOfNode folded in):
    /// the externalModuleIndicator ONLY — the CJS indicator the
    /// variant above also admits would over-filter trySymbolTable's
    /// UMD leg (50341).
    pub fn is_external_module_of_node(&self, node: NodeId) -> bool {
        self.source(self.file_index_of_node(node))
            .external_module_indicator
            .is_some()
    }
}

#[cfg(test)]
#[path = "../tests/unit/program/tests.rs"]
mod tests;
