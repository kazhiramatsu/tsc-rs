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

use rustc_hash::FxHashMap as HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use tsc_binder::{BindData, Binder, Symbol, SymbolArena, SymbolId, SymbolTable};
use tsc_program::content_mapper::source::ContentMappedSource;
use tsc_syntax::{NodeArray, NodeArrayId, NodeId, ParseOptions, SourceFile};
use tsc_types::{IdentityDomain, JsStr, JsString, SymbolFlags, TRANSIENT_SYMBOL_BIT};

/// Immutable parsed source handle retained by a Program snapshot.
#[derive(Clone, Debug)]
pub struct ParsedDocument {
    pub source: Arc<SourceFile>,
    /// tsgo `ast.GetNodeId` of the source file: the parse's identity, shared
    /// by every snapshot that reuses it.
    id: u64,
}

static NEXT_DOCUMENT_ID: AtomicU64 = AtomicU64::new(1);

/// A new parse identity (tsgo's node id counter): never zero, never reused.
pub fn next_document_id() -> u64 {
    NEXT_DOCUMENT_ID.fetch_add(1, Ordering::Relaxed)
}

impl ParsedDocument {
    /// tsrs-native: constructs an immutable parsed-document handle.
    #[cfg(test)]
    pub fn new(source: Arc<SourceFile>) -> Self {
        Self {
            source,
            id: next_document_id(),
        }
    }

    /// The parse's identity (tsgo's source file node id).
    pub fn id(&self) -> u64 {
        self.id
    }
}

/// A source that has not been published to a registry or snapshot.
///
/// The source is owned by value until `freeze` allocates its shared handle.
/// The builder exposes no Arc or Weak before binding completes.
#[derive(Debug)]
pub struct DocumentBuilder {
    source: SourceFile,
}

impl DocumentBuilder {
    pub fn new(source: SourceFile) -> Self {
        Self { source }
    }

    pub fn source(&self) -> &SourceFile {
        &self.source
    }

    /// The exclusive source borrow must end before publication.
    ///
    /// ```compile_fail,E0505
    /// use tsc_checker::program::DocumentBuilder;
    /// use tsc_binder::{BindData, bind_source_file};
    /// fn publish_while_binding(mut builder: DocumentBuilder, data: BindData) {
    ///     let worker = bind_source_file(builder.source_mut());
    ///     let document = builder.freeze(data);
    ///     drop(worker);
    /// }
    /// ```
    pub fn source_mut(&mut self) -> &mut SourceFile {
        &mut self.source
    }

    /// Consume the builder only after binding has produced its completed data.
    /// Parse identity is assigned here, in the established publication order.
    pub fn freeze(self, data: BindData) -> BoundDocument {
        let parsed = ParsedDocument {
            source: Arc::new(self.source),
            id: next_document_id(),
        };
        BoundDocument::new(Arc::new(parsed), data)
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

/// What a program source's parse and bind depend on besides its file name
/// and text (tsgo `ParseCacheKey` without the hash): the parse options at
/// base-zero identities and the identity domain the document's nodes and
/// symbols belong to. Script kind determines the language/JS context;
/// JSDoc mode and the effective module/JSX indicators include all current
/// file-affecting compiler/format inputs. Program membership and resolution
/// remain outside this key. Transpile overrides are not cached.
#[derive(Clone, Debug)]
pub struct DocumentAddress {
    parse: ParseOptions,
    /// The actual grammar kind is separate because the legacy JSON
    /// `parse_options` shape carries `script_kind = Ts`. Folding the real kind
    /// into those options would broaden the loader's JSON reuse in
    /// `DocumentRegistry::find_parse`, a behavior change deliberately not made
    /// here. This field distinguishes JSON from explicit TypeScript parses
    /// while preserving which parses match the loader today.
    script_kind: tsc_syntax::ScriptKind,
    identity_domain: IdentityDomain,
    /// A content mapper's output (tsgo's content-mapped parse cache).
    content_mapper: Option<ContentMapperAddress>,
}

/// What a content mapper's output is found again by (tsgo's content-mapped
/// parse cache): a transform's output by the mapper's transform identity
/// and the original text it transformed, which give the same outputs with
/// the same mapper diagnostics; a failure stub by itself, since tsgo makes
/// one per program and a program cloned from it keeps it.
#[derive(Clone, Debug)]
enum ContentMapperAddress {
    Transform(String, Arc<str>),
    FailureStub(Arc<ContentMappedSource>),
}

impl PartialEq for ContentMapperAddress {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Transform(identity, text), Self::Transform(other_identity, other_text)) => {
                identity == other_identity && text == other_text
            }
            (Self::FailureStub(stub), Self::FailureStub(other)) => Arc::ptr_eq(stub, other),
            _ => false,
        }
    }
}

impl DocumentAddress {
    /// tsrs-native: the address of a source parsed with `parse` and bound
    /// in `identity_domain`.
    pub fn new(parse: &ParseOptions, identity_domain: &IdentityDomain) -> Self {
        Self {
            parse: ParseOptions {
                node_id_base: 0,
                node_array_id_base: 0,
                ..parse.clone()
            },
            script_kind: parse.script_kind,
            identity_domain: identity_domain.clone(),
            content_mapper: None,
        }
    }

    /// The grammar used by an entry point with legacy parse-option defaults
    /// (JSON). Keep the legacy options as well, preserving existing reuse and
    /// creation order while distinguishing different actual script kinds.
    pub fn with_script_kind(mut self, script_kind: tsc_syntax::ScriptKind) -> Self {
        self.script_kind = script_kind;
        self
    }

    /// The address of a content mapper's output (tsgo
    /// `contentMappedParseCacheKey`, see [`ContentMapperAddress`]).
    pub fn with_content_mapper(mut self, info: &Arc<ContentMappedSource>) -> Self {
        self.content_mapper = Some(if info.is_failure_stub() {
            ContentMapperAddress::FailureStub(Arc::clone(info))
        } else {
            ContentMapperAddress::Transform(
                info.transform_identity.clone(),
                Arc::clone(&info.original_text),
            )
        });
        self
    }
}

impl PartialEq for DocumentAddress {
    fn eq(&self, other: &Self) -> bool {
        self.parse == other.parse
            && self.script_kind == other.script_kind
            && self.identity_domain.same_domain(&other.identity_domain)
            && self.content_mapper == other.content_mapper
    }
}

/// The identity domain of every shared document: the process cache's library
/// bundles and the documents of every [`DocumentRegistry`]. tsgo's parse
/// cache hands a parse to any Program (a pointer belongs to no domain); one
/// reclaiming domain gives tsc-rs the same reach, so Programs with different
/// library sets share their sources, as do files a client created.
pub fn shared_identity_domain() -> &'static IdentityDomain {
    static DOMAIN: std::sync::OnceLock<IdentityDomain> = std::sync::OnceLock::new();
    DOMAIN.get_or_init(IdentityDomain::new)
}

/// The libraries of the process's library bundles (which live as long as
/// the process), where a Program's loader finds them (tsgo's parse cache
/// holds the bundled libraries' parses too).
pub(crate) fn lib_documents() -> &'static DocumentRegistry {
    static DOCUMENTS: std::sync::OnceLock<DocumentRegistry> = std::sync::OnceLock::new();
    DOCUMENTS.get_or_init(DocumentRegistry::new)
}

/// tsgo's parse cache (`project/parsecache.go`): the parsed and bound
/// sources Programs hold, found again by file name, text and
/// [`DocumentAddress`], so a Program built again shares the documents of the
/// files that did not change. An entry lives while a Program holds its
/// document: tsgo counts the Programs that acquired it, and the `Weak` here
/// follows the Programs' `Arc`s.
#[derive(Debug)]
pub struct DocumentRegistry {
    /// [`shared_identity_domain`], the domain of the library bundles too.
    identity_domain: IdentityDomain,
    entries: Mutex<HashMap<(JsString, u64), Vec<RegistryEntry>>>,
}

#[derive(Debug)]
struct RegistryEntry {
    address: DocumentAddress,
    document: Weak<BoundDocument>,
}

impl DocumentRegistry {
    pub fn new() -> Self {
        Self {
            identity_domain: shared_identity_domain().clone(),
            entries: Mutex::default(),
        }
    }

    /// The identity domain of the documents ([`shared_identity_domain`]).
    pub fn identity_domain(&self) -> &IdentityDomain {
        &self.identity_domain
    }

    /// The document a Program holds for `file_name` with `text` at
    /// `address` (tsgo `Acquire` finding the key).
    pub fn get(
        &self,
        file_name: JsStr<'_>,
        text: &str,
        address: &DocumentAddress,
    ) -> Option<Arc<BoundDocument>> {
        let entries = self.lock();
        entries
            .get(&(file_name.to_owned(), text_hash(text)))?
            .iter()
            .filter(|entry| entry.address == *address)
            .find_map(|entry| entry.document.upgrade())
            .filter(|document| document.source().text() == text)
    }

    /// Record a document a Program parsed and bound at `address`, and forget
    /// the entries of the same file and text no Program holds any more.
    pub fn insert(&self, address: DocumentAddress, document: &Arc<BoundDocument>) {
        let source = document.source();
        let key = (source.file_name.clone(), text_hash(source.text()));
        let mut entries = self.lock();
        let slot = entries.entry(key).or_default();
        slot.retain(|entry| entry.document.strong_count() > 0);
        slot.push(RegistryEntry {
            address,
            document: Arc::downgrade(document),
        });
    }

    /// The live documents of `file_name`, at any address (tsgo's parse cache
    /// entries of the file). A content mapper's output is not one: tsgo
    /// keeps it in a cache of its own.
    pub fn documents_of(&self, file_name: JsStr<'_>) -> Vec<Arc<BoundDocument>> {
        self.lock()
            .iter()
            .filter(|((name, _), _)| name.as_js() == file_name)
            .flat_map(|(_, slot)| {
                slot.iter()
                    .filter(|entry| entry.address.content_mapper.is_none())
                    .filter_map(|entry| entry.document.upgrade())
            })
            .collect()
    }

    /// tsgo `ParseCache.Acquire` as a Program's loader asks it (the project
    /// system's `compilerHost.GetSourceFile`): the source a Program holds of
    /// `file_name` with `text`, parsed with `options` in the registry's
    /// domain.
    fn find_parse(
        &self,
        file_name: JsStr<'_>,
        text: &str,
        options: &ParseOptions,
    ) -> Option<tsc_program::CachedParse> {
        let address = DocumentAddress::new(options, &self.identity_domain);
        let document = self
            .get(file_name, text, &address)
            .or_else(|| lib_documents().get(file_name, text, &address))?;
        let source = Arc::clone(&document.parsed.source);
        Some(tsc_program::CachedParse::new(source, document))
    }

    /// Forget the entries no Program holds (tsgo deletes an entry when its
    /// count reaches zero).
    pub fn purge(&self) {
        self.lock().retain(|_, slot| {
            slot.retain(|entry| entry.document.strong_count() > 0);
            !slot.is_empty()
        });
    }

    /// The number of documents a Program still holds.
    pub fn len(&self) -> usize {
        self.lock()
            .values()
            .flatten()
            .filter(|entry| entry.document.strong_count() > 0)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(JsString, u64), Vec<RegistryEntry>>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl tsc_program::ParseCache for DocumentRegistry {
    fn find(
        &self,
        file_name: JsStr<'_>,
        text: &str,
        options: &ParseOptions,
    ) -> Option<tsc_program::CachedParse> {
        self.find_parse(file_name, text, options)
    }
}

impl Default for DocumentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// The hash a document's text is found by (tsgo hashes file contents with
/// xxh3); a hit still compares the text.
fn text_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    text.hash(&mut hasher);
    hasher.finish()
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

    /// Freeze and publish a uniquely owned source after binding completes.
    pub fn publish_builder(
        &mut self,
        builder: DocumentBuilder,
        data: BindData,
    ) -> Result<Arc<BoundDocument>, EphemeralDocumentStoreError> {
        if !builder.source().identity_owned_by(&self.identity_domain) {
            return Err(EphemeralDocumentStoreError::SourceIdentityDomainMismatch);
        }
        if !data.identity_owned_by(&self.identity_domain) {
            return Err(EphemeralDocumentStoreError::BindIdentityDomainMismatch);
        }
        let document = Arc::new(builder.freeze(data));
        self.documents.push(Arc::clone(&document));
        Ok(document)
    }

    /// tsrs-native: adds a completed document a Program already holds (tsgo's
    /// parse cache), after ownership validation.
    pub fn adopt(
        &mut self,
        document: Arc<BoundDocument>,
    ) -> Result<(), EphemeralDocumentStoreError> {
        if !document.source().identity_owned_by(&self.identity_domain) {
            return Err(EphemeralDocumentStoreError::SourceIdentityDomainMismatch);
        }
        if !document.data.identity_owned_by(&self.identity_domain) {
            return Err(EphemeralDocumentStoreError::BindIdentityDomainMismatch);
        }
        self.documents.push(document);
        Ok(())
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
    source_from_project_reference: bool,
    outside_program: bool,
}

impl ProgramFileFacts {
    pub const ORDINARY: Self = Self {
        default_library: false,
        source_from_project_reference: false,
        outside_program: false,
    };
    pub const DEFAULT_LIBRARY: Self = Self {
        default_library: true,
        source_from_project_reference: false,
        outside_program: false,
    };
    /// A source of a referenced project that the Program reads in place of
    /// the project's output (tsgo `IsSourceFromProjectReference`).
    pub const SOURCE_FROM_PROJECT_REFERENCE: Self = Self {
        default_library: false,
        source_from_project_reference: true,
        outside_program: false,
    };
    /// A file that is not one of the Program's source files (tsgo
    /// `checker.Program.SourceFiles()`): the checker reaches it only by
    /// resolving a module to it, so its declarations join no global scope.
    /// The auto-import registry's alias resolver reads a package's files so.
    pub const OUTSIDE_PROGRAM: Self = Self {
        default_library: false,
        source_from_project_reference: false,
        outside_program: true,
    };

    /// tsrs-native: reads Program-owned default-library membership instead of
    /// inferring it from a parsed document or path.
    pub const fn is_default_library(self) -> bool {
        self.default_library
    }

    pub const fn is_source_from_project_reference(self) -> bool {
        self.source_from_project_reference
    }

    pub const fn is_outside_program(self) -> bool {
        self.outside_program
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

/// A scope's symbol table for a lookup that also drives the checker:
/// borrowed for the program lifetime from a program-owned document, shared
/// when the table lives behind an `Arc` (module exports, the globals), and
/// copied only for a legacy borrowed-binder entry.
pub(crate) enum ScopeTable<'a> {
    Borrowed(&'a SymbolTable),
    Shared(Arc<SymbolTable>),
    Owned(SymbolTable),
}

impl std::ops::Deref for ScopeTable<'_> {
    type Target = SymbolTable;

    fn deref(&self) -> &SymbolTable {
        match self {
            Self::Borrowed(table) => table,
            Self::Shared(table) => table,
            Self::Owned(table) => table,
        }
    }
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
    /// Borrowed projections avoid repeatedly following the owned-document
    /// Arc chain. Both entry forms already retain sources for `'a`.
    sources: Vec<&'a SourceFile>,
    /// Program-owned facts copied from the immutable snapshot. They remain
    /// session-local and never mutate shared parsed/bound documents.
    file_facts: Vec<ProgramFileFacts>,
    /// Node-id intervals in parse allocation order (ascending by start).
    node_owners: Vec<ArenaOwner>,
    /// Persistent symbol intervals in identity allocation order.
    symbol_owners: Vec<ArenaOwner>,
    /// A hint into immutable intervals, checked on every lookup. Relaxed
    /// atomics preserve shared-read safety without ordering any other state.
    symbol_owner_hint: AtomicUsize,
    /// Direct id → file routing for nodes and persistent symbols: one load
    /// instead of a hinted interval search (tsc carries object identity).
    node_files: DenseOwners,
    symbol_files: DenseOwners,
    /// Checker-side symbols (tsc createSymbol 47652 adds Transient).
    transient: SymbolArena,
}

/// Page-table routing over sorted, disjoint owner intervals: `id >>
/// OWNER_PAGE_SHIFT` selects a page whose entry is the index of the first
/// interval ending after the page starts; a short in-order scan from there
/// (a page holds a handful of small files at most) finds the owner. The
/// table is a few thousand entries for a large program, so a lookup stays
/// in L1 instead of touching a per-id table (tsc carries object identity).
/// Built only when the covered id range is not much larger than the
/// intervals; a pathological sparse domain keeps the interval search.
#[derive(Debug, Default)]
struct DenseOwners {
    first_page: u32,
    pages: Box<[u32]>,
    owners: Vec<ArenaOwner>,
}

const NO_OWNER: u32 = u32::MAX;
const OWNER_PAGE_SHIFT: u32 = 8;

impl DenseOwners {
    fn build(owners: &[ArenaOwner]) -> Self {
        let (Some(first), Some(last)) = (owners.first(), owners.last()) else {
            return Self::default();
        };
        let first_page = first.start >> OWNER_PAGE_SHIFT;
        let last_page = last.end.saturating_sub(1) >> OWNER_PAGE_SHIFT;
        let page_count = (u64::from(last_page) - u64::from(first_page) + 1) as usize;
        if page_count > owners.len().saturating_mul(64).saturating_add(1 << 16) {
            return Self::default();
        }
        let mut pages = vec![NO_OWNER; page_count].into_boxed_slice();
        let mut owner_index = 0usize;
        for (offset, entry) in pages.iter_mut().enumerate() {
            let page_start = (u64::from(first_page) + offset as u64) << OWNER_PAGE_SHIFT;
            while owner_index < owners.len() && u64::from(owners[owner_index].end) <= page_start {
                owner_index += 1;
            }
            *entry = if owner_index < owners.len() {
                u32::try_from(owner_index).expect("program file count fits u32")
            } else {
                NO_OWNER
            };
        }
        Self {
            first_page,
            pages,
            owners: owners.to_vec(),
        }
    }

    /// The index (into the interval list the table was built from) of the
    /// interval owning `id`.
    #[inline]
    fn lookup_index(&self, id: u32) -> Option<usize> {
        let page = (id >> OWNER_PAGE_SHIFT).checked_sub(self.first_page)? as usize;
        let mut index = *self.pages.get(page)? as usize;
        loop {
            let owner = self.owners.get(index)?;
            if id < owner.start {
                return None;
            }
            if id < owner.end {
                return Some(index);
            }
            index += 1;
        }
    }

    /// The file of the interval owning `id`: the interval just read holds
    /// it, so no second array is read.
    #[inline]
    fn lookup_file(&self, id: u32) -> Option<usize> {
        let page = (id >> OWNER_PAGE_SHIFT).checked_sub(self.first_page)? as usize;
        let mut index = *self.pages.get(page)? as usize;
        loop {
            let owner = self.owners.get(index)?;
            if id < owner.start {
                return None;
            }
            if id < owner.end {
                return Some(owner.file);
            }
            index += 1;
        }
    }

    fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }
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

/// A Program reads its trees and symbols in the stores: every file is
/// published, under leases of one domain.
fn validate_identity_domains(entries: &[ProgramEntry<'_>]) -> Result<(), ProgramIdentityError> {
    let mut program_anchor: Option<&tsc_types::IdentityLease> = None;
    for (file, entry) in entries.iter().enumerate() {
        let data = entry.data();
        let (Some(syntax), Some(symbols), Some(serials)) = (
            entry.source().arena.identity_leases(),
            data.symbol_identity_lease(),
            data.private_name_serial_lease(),
        ) else {
            return Err(ProgramIdentityError::PartialIdentityOwnership { file });
        };
        let anchor = &syntax.nodes;
        if [&syntax.arrays, &syntax.array_items, symbols, serials]
            .iter()
            .any(|lease| !anchor.same_domain(lease))
        {
            return Err(ProgramIdentityError::IdentityDomainMismatch { file });
        }
        match program_anchor {
            Some(program_anchor) if !program_anchor.same_domain(anchor) => {
                return Err(ProgramIdentityError::IdentityDomainMismatch { file });
            }
            Some(_) => {}
            None => program_anchor = Some(anchor),
        }
    }
    Ok(())
}

impl<'a> ProgramBinder<'a> {
    /// The checker-created symbols (memory accounting); see
    /// [`SymbolArena::memory_usage`].
    pub(crate) fn transient_memory_usage(
        &self,
        tables: &mut rustc_hash::FxHashMap<*const SymbolTable, usize>,
    ) -> (usize, usize, usize) {
        self.transient.memory_usage(tables)
    }

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
                let end = data.symbols.next_id().index();
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

        let sources = file_entries.iter().map(ProgramEntry::source).collect();
        let node_files = DenseOwners::build(&node_owners);
        let symbol_files = DenseOwners::build(&symbol_owners);
        Ok(Self {
            file_entries,
            sources,
            file_facts,
            node_owners,
            symbol_owners,
            symbol_owner_hint: AtomicUsize::new(0),
            node_files,
            symbol_files,
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
    /// tsgo `Program.SymbolCount`: every file's binder symbols.
    pub fn binder_symbol_count(&self) -> usize {
        (0..self.file_count())
            .map(|index| self.file(index).symbols.len())
            .sum()
    }

    /// The symbols this checker created.
    pub fn transient_symbol_count(&self) -> usize {
        self.transient.len()
    }

    /// tsgo `Program.IdentifierCount`: every file's identifier nodes.
    pub fn identifier_count(&self) -> usize {
        (0..self.file_count())
            .map(|index| self.source(index).identifier_count())
            .sum()
    }

    pub fn file(&self, index: usize) -> &BindData {
        self.file_entries[index].data()
    }

    /// The immutable flow graph held for the Program lifetime, so a query
    /// can keep it across mutable checker calls after validating `index` once.
    /// Legacy entries clone the completed bind data on construction; their
    /// original binder's graph is identical and remains immutably borrowed.
    pub(crate) fn flow_arena(&self, index: usize) -> &'a tsc_binder::flow::FlowArena {
        match &self.file_entries[index] {
            ProgramEntry::Legacy(entry) => &entry.binder.flow,
            ProgramEntry::Owned(document) => {
                let document: &'a Arc<BoundDocument> = document;
                &document.data.flow
            }
        }
    }

    /// tsrs-native: Rust ProgramBinder SourceFile projection.
    pub fn source(&self, index: usize) -> &'a SourceFile {
        self.sources[index]
    }

    /// Owning file of a node id (nodes allocate contiguously per file).
    /// tsrs-native: validated last-owner hint, then binary-search routing for
    /// Rust's numeric NodeId arena; tsc carries object identity directly.
    #[inline]
    pub fn file_index_of_node(&self, node: NodeId) -> usize {
        self.try_file_index_of_node(node)
            .unwrap_or_else(|| panic!("NodeId {} is outside every program arena", node.index()))
    }

    /// Fallible counterpart used at external identity boundaries such as the
    /// checker-owned emit resolver. Ordinary checker code already owns valid
    /// node identities and continues to use [`Self::file_index_of_node`].
    /// tsrs-native: validation for Rust's source-token/node-id pair.
    ///
    /// The page table answers (or the interval search, for a sparse
    /// domain). A checker's lookups alternate between the file it checks
    /// and the files its declarations come from, so a last-hit hint missed
    /// about as often as it hit.
    #[inline]
    pub(crate) fn try_file_index_of_node(&self, node: NodeId) -> Option<usize> {
        if self.node_files.is_empty() {
            Self::try_owner_index(&self.node_owners, node.index())
                .map(|index| self.node_owners[index].file)
        } else {
            self.node_files.lookup_file(node.index())
        }
    }

    /// The parsed record of `node` (tsc's direct object access).
    #[inline(always)]
    pub fn node_record(&self, node: NodeId) -> &'a tsc_syntax::Node {
        tsc_syntax::published_node(node)
    }

    /// tsrs-native: multi-file arena routing for a numeric NodeId; tsc
    /// carries the SourceFile/object relationship directly.
    #[inline]
    pub fn source_of_node(&self, node: NodeId) -> &'a SourceFile {
        self.sources[self.file_index_of_node(node)]
    }

    fn binder_of_node(&self, node: NodeId) -> &BindData {
        self.file_entries[self.file_index_of_node(node)].data()
    }

    /// The array `id` of a Program's tree.
    #[inline(always)]
    pub fn node_array(&self, id: NodeArrayId) -> NodeArray<'a> {
        tsc_syntax::published_node_array(id)
    }

    fn try_owner_file(owners: &[ArenaOwner], id: u32) -> Option<usize> {
        Self::try_owner_index(owners, id).map(|index| owners[index].file)
    }

    fn try_owner_index(owners: &[ArenaOwner], id: u32) -> Option<usize> {
        let index = owners
            .partition_point(|owner| owner.start <= id)
            .checked_sub(1)?;
        (id < owners[index].end).then_some(index)
    }

    #[inline]
    fn owner_of_symbol(&self, id: SymbolId) -> Result<usize, ()> {
        if id.index() & TRANSIENT_SYMBOL_BIT != 0 {
            return Err(());
        }
        let hint = self.symbol_owner_hint.load(Ordering::Relaxed);
        if let Some(owner) = self.symbol_owners.get(hint) {
            if owner.start <= id.index() && id.index() < owner.end {
                return Ok(owner.file);
            }
        }
        let index = if self.symbol_files.is_empty() {
            Self::try_owner_index(&self.symbol_owners, id.index())
        } else {
            self.symbol_files.lookup_index(id.index())
        }
        .unwrap_or_else(|| {
            panic!(
                "persistent SymbolId {} is outside every program arena",
                id.index()
            )
        });
        self.symbol_owner_hint.store(index, Ordering::Relaxed);
        Ok(self.symbol_owners[index].file)
    }

    /// The file whose bind created the persistent symbol `id`; `None` for a
    /// checker's (transient) symbol.
    pub fn file_index_of_symbol(&self, id: SymbolId) -> Option<usize> {
        if id.index() & TRANSIENT_SYMBOL_BIT != 0 {
            return None;
        }
        Self::try_owner_file(&self.symbol_owners, id.index())
    }

    /// tsrs-native: routes a numeric SymbolId to its binder or
    /// checker-owned transient arena; tsc carries object references.
    #[inline]
    pub fn symbol(&self, id: SymbolId) -> &Symbol {
        if id.index() & TRANSIENT_SYMBOL_BIT != 0 {
            return self.transient.symbol(id);
        }
        tsc_binder::published_symbol(id)
    }

    /// Fallible counterpart to [`Self::symbol`] for externally supplied
    /// checker identities. Unlike the ordinary accessor, this never routes an
    /// unknown id into an arena index and therefore never panics.
    /// tsrs-native: fallible symbol lookup for the emit-resolver
    /// symbol-token validation boundary (h2-7a-m-2 §4).
    pub(crate) fn try_symbol(&self, id: SymbolId) -> Option<&Symbol> {
        if id.index() & TRANSIENT_SYMBOL_BIT != 0 {
            return self.transient.get(id);
        }
        let file = Self::try_owner_file(&self.symbol_owners, id.index())?;
        match &self.file_entries[file] {
            ProgramEntry::Legacy(entry) => entry.binder.symbols.get(id),
            ProgramEntry::Owned(document) => document.data.symbols.get(id),
        }
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

    /// A declaration list of `nodes` for a checker symbol (issued by the
    /// transient arena's pool, which lives as long as the checker's symbols).
    pub fn declarations_from(&mut self, nodes: &[NodeId]) -> tsc_binder::Declarations {
        self.transient.declarations_from(nodes)
    }

    /// Give the checker symbol `symbol` the declaration list `nodes`.
    pub fn set_declarations_from(&mut self, symbol: SymbolId, nodes: &[NodeId]) {
        debug_assert!(
            self.owner_of_symbol(symbol).is_err(),
            "{symbol:?} is a file symbol"
        );
        self.transient.set_declarations_from(symbol, nodes);
    }

    /// Append `declaration` to the checker symbol `symbol`'s list.
    pub fn push_declaration(&mut self, symbol: SymbolId, declaration: NodeId) {
        debug_assert!(
            self.owner_of_symbol(symbol).is_err(),
            "{symbol:?} is a file symbol"
        );
        self.transient.push_declaration(symbol, declaration);
    }

    /// Append `declarations` to the checker symbol `symbol`'s list.
    pub fn extend_declarations(&mut self, symbol: SymbolId, declarations: &[NodeId]) {
        debug_assert!(
            self.owner_of_symbol(symbol).is_err(),
            "{symbol:?} is a file symbol"
        );
        self.transient.extend_declarations(symbol, declarations);
    }

    /// tsrs-native: size the transient arena for `additional` symbols up
    /// front (an allocation hint; no identity depends on it).
    pub fn reserve_transient_symbols(&mut self, additional: usize) {
        self.transient.reserve(additional);
    }

    /// tsc-port: createSymbol @6.0.3
    /// tsc-hash: b9b2c65d71ec1e9d3a55d36fe5224e5f31dd618ee1428293b371d2f2881ad16a
    /// tsc-span: _tsc.js:47652-47658
    ///
    /// Checker-side symbol creation: always Transient. (tsc also seeds
    /// links.checkFlags here; ours live in LinksTables and default 0 —
    /// callers that need CheckFlags set them through the links API.)
    pub fn create_symbol(
        &mut self,
        flags: SymbolFlags,
        escaped_name: tsc_types::EscapedName,
    ) -> SymbolId {
        self.transient
            .alloc(flags | SymbolFlags::TRANSIENT, escaped_name)
    }

    /// tsc container.locals of a scope-owning node.
    /// tsrs-native: binder-table projection for tsc's direct
    /// `container.locals` property access.
    pub fn locals_of(&self, scope: NodeId) -> Option<&SymbolTable> {
        // Most nodes have no locals: one byte answers without a route to
        // the file and a probe of its table.
        if !tsc_binder::published_has_locals(scope) {
            self.debug_assert_no_locals(scope);
            return None;
        }
        self.binder_of_node(scope).locals.get(&scope)
    }

    /// The binder sets `HAS_LOCALS` wherever it creates a locals table; a
    /// table without the bit would be invisible to name resolution.
    #[inline]
    fn debug_assert_no_locals(&self, scope: NodeId) {
        debug_assert!(
            self.try_file_index_of_node(scope)
                .is_none_or(|file| !self.file_entries[file].data().locals.contains_key(&scope)),
            "{scope:?} has a locals table without its HAS_LOCALS bit"
        );
    }

    /// [`Self::locals_of`] held for the program lifetime instead of the
    /// binder borrow, so a scope walk can read the table while the checker
    /// resolves aliases and merges symbols (tsc's forEachSymbolTableInScope
    /// reads the live table object; copying it per lookup was the cost).
    /// A legacy borrowed-binder entry (unit-test callers) owns its bind data,
    /// so its table is copied.
    pub(crate) fn locals_of_scope(&self, scope: NodeId) -> Option<ScopeTable<'a>> {
        if !tsc_binder::published_has_locals(scope) {
            self.debug_assert_no_locals(scope);
            return None;
        }
        match &self.file_entries[self.file_index_of_node(scope)] {
            ProgramEntry::Owned(document) => {
                let document: &'a Arc<BoundDocument> = document;
                document.data.locals.get(&scope).map(ScopeTable::Borrowed)
            }
            ProgramEntry::Legacy(entry) => entry
                .data
                .locals
                .get(&scope)
                .map(|table| ScopeTable::Owned(table.clone())),
        }
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
    #[inline(always)]
    pub fn node_symbol(&self, node: NodeId) -> Option<SymbolId> {
        tsc_binder::published_node_symbol(node)
    }

    /// The binder's mutable node-flags view (ContainsThis etc.).
    /// tsrs-native: binder-table projection for tsc's direct
    /// `node.flags` property access.
    #[inline(always)]
    pub fn flags_of(&self, node: NodeId) -> tsc_types::NodeFlags {
        tsc_binder::published_node_flags(node)
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

    /// tsrs-native: binder-table projection of the source's existing
    /// `commonJsModuleIndicator`, produced by `setCommonJsModuleIndicator`
    /// (_tsc.js:44589-44600). Keep it distinct from forced external status.
    pub fn is_common_js_module_of_node(&self, node: NodeId) -> bool {
        self.file_entries[self.file_index_of_node(node)]
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

impl ProgramBinder<'_> {
    pub(crate) fn transient_record_heap_bytes(&self) -> usize {
        self.transient.record_heap_bytes()
    }
    pub(crate) fn transient_declaration_store_bytes(&self) -> usize {
        self.transient.declaration_store_bytes()
    }
}

impl ProgramSnapshot {
    pub(crate) fn add_memory_usage(&self, usage: &mut tsc_diagnostics::MemoryUsage) {
        usage.vec("snapshot document handles", &self.documents);
        usage.vec("snapshot file facts", &self.file_facts);
    }
}

impl ProgramBinder<'_> {
    pub(crate) fn add_projection_memory_usage(&self, usage: &mut tsc_diagnostics::MemoryUsage) {
        usage.vec("checker program projections", &self.file_entries);
        usage.vec("checker program projections", &self.sources);
        usage.vec("checker program projections", &self.file_facts);
        usage.vec("checker owner indexes", &self.node_owners);
        usage.vec("checker owner indexes", &self.symbol_owners);
        for index in [&self.node_files, &self.symbol_files] {
            usage.vec("checker owner indexes", &index.owners);
            let bytes = std::mem::size_of_val(index.pages.as_ref());
            if bytes != 0 && usage.claim(index.pages.as_ptr() as usize) {
                usage.add("checker owner indexes", index.pages.len(), bytes, bytes);
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/program/tests.rs"]
mod tests;
