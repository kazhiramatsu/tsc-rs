//! Durable graph transactions and immutable root-generation promotion.
//!
//! Promotion is deliberately split into `stage`, `close`, and `publish` so a
//! coordinator can recover both relevant kill windows.  An open transaction
//! is never promotable.  A closed transaction deterministically names its root
//! generation, making publication an idempotent compare-and-create operation.

use crate::{sha256, Digest, Projection, ReceiptKey, ReceiptOutputs};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const TRANSACTION_MAGIC: &[u8; 4] = b"NCIT";
const GENERATION_MAGIC: &[u8; 4] = b"NCIG";
const RECORD_VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_STRING_BYTES: u64 = 16 * 1024 * 1024;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

/// The content digest of a canonical transaction manifest.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TransactionId(Digest);

impl TransactionId {
    pub const fn from_digest(digest: Digest) -> Self {
        Self(digest)
    }

    pub const fn digest(self) -> Digest {
        self.0
    }

    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }
}

impl fmt::Display for TransactionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The immutable identifier a reader pins for its complete run.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GenerationId(Digest);

impl GenerationId {
    pub const fn from_digest(digest: Digest) -> Self {
        Self(digest)
    }

    pub const fn digest(self) -> Digest {
        self.0
    }

    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }
}

impl fmt::Display for GenerationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// One node selected by a graph transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionNode {
    pub key: ReceiptKey,
    pub receipt_id: Digest,
    pub outputs: ReceiptOutputs,
    pub artifact_blob_ids: Vec<Digest>,
}

impl TransactionNode {
    pub fn new(
        key: ReceiptKey,
        receipt_id: Digest,
        outputs: ReceiptOutputs,
        artifact_blob_ids: impl IntoIterator<Item = Digest>,
    ) -> Self {
        Self {
            key,
            receipt_id,
            outputs,
            artifact_blob_ids: artifact_blob_ids.into_iter().collect(),
        }
    }
}

/// A validated producer-to-consumer edge in a transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionEdge {
    pub producer: ReceiptKey,
    pub consumer: ReceiptKey,
    pub label: String,
    pub projection: Projection,
    pub digest: Digest,
}

impl TransactionEdge {
    pub fn new(
        producer: ReceiptKey,
        consumer: ReceiptKey,
        label: impl Into<String>,
        projection: Projection,
        digest: Digest,
    ) -> Self {
        Self {
            producer,
            consumer,
            label: label.into(),
            projection,
            digest,
        }
    }
}

/// The complete graph selection closed before a root can be published.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionManifest {
    pub pinned_root: Option<GenerationId>,
    pub nodes: Vec<TransactionNode>,
    pub edges: Vec<TransactionEdge>,
}

impl TransactionManifest {
    pub fn new(
        pinned_root: Option<GenerationId>,
        nodes: impl IntoIterator<Item = TransactionNode>,
        edges: impl IntoIterator<Item = TransactionEdge>,
    ) -> Result<Self, ManifestError> {
        let manifest = Self {
            pinned_root,
            nodes: nodes.into_iter().collect(),
            edges: edges.into_iter().collect(),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Revalidates an untrusted or caller-mutated manifest.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.nodes.is_empty() {
            return Err(ManifestError::EmptyGraph);
        }

        let mut nodes = BTreeMap::new();
        let mut receipt_ids = BTreeSet::new();
        for node in &self.nodes {
            if nodes.insert(node.key, node.outputs).is_some() {
                return Err(ManifestError::DuplicateNode(node.key));
            }
            if !receipt_ids.insert(node.receipt_id) {
                return Err(ManifestError::DuplicateReceipt(node.receipt_id));
            }
            let mut blobs = BTreeSet::new();
            for blob in &node.artifact_blob_ids {
                if !blobs.insert(*blob) {
                    return Err(ManifestError::DuplicateArtifact {
                        node: node.key,
                        blob: *blob,
                    });
                }
            }
        }

        let mut edge_labels = BTreeSet::new();
        let mut outgoing: BTreeMap<ReceiptKey, Vec<ReceiptKey>> = self
            .nodes
            .iter()
            .map(|node| (node.key, Vec::new()))
            .collect();
        let mut indegree: BTreeMap<ReceiptKey, usize> =
            self.nodes.iter().map(|node| (node.key, 0)).collect();

        for edge in &self.edges {
            if edge.label.is_empty() {
                return Err(ManifestError::EmptyEdgeLabel);
            }
            let Some(producer_outputs) = nodes.get(&edge.producer) else {
                return Err(ManifestError::MissingProducer(edge.producer));
            };
            if !nodes.contains_key(&edge.consumer) {
                return Err(ManifestError::MissingConsumer(edge.consumer));
            }
            if !edge_labels.insert((edge.consumer, edge.label.clone())) {
                return Err(ManifestError::DuplicateEdgeLabel {
                    consumer: edge.consumer,
                    label: edge.label.clone(),
                });
            }
            let expected = producer_outputs.digest(edge.projection);
            if edge.digest != expected {
                return Err(ManifestError::ProjectionMismatch {
                    producer: edge.producer,
                    projection: edge.projection,
                    expected,
                    actual: edge.digest,
                });
            }
            outgoing
                .get_mut(&edge.producer)
                .expect("producer was checked")
                .push(edge.consumer);
            let degree = indegree
                .get_mut(&edge.consumer)
                .expect("consumer was checked");
            *degree = degree.checked_add(1).ok_or(ManifestError::GraphTooLarge)?;
        }

        let mut ready: BTreeSet<ReceiptKey> = indegree
            .iter()
            .filter_map(|(key, degree)| (*degree == 0).then_some(*key))
            .collect();
        let mut visited = 0usize;
        while let Some(key) = ready.pop_first() {
            visited += 1;
            for consumer in &outgoing[&key] {
                let degree = indegree.get_mut(consumer).expect("consumer was checked");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(*consumer);
                }
            }
        }
        if visited != self.nodes.len() {
            return Err(ManifestError::Cycle);
        }
        Ok(())
    }

    pub fn id(&self) -> TransactionId {
        TransactionId(sha256(&encode_manifest(self)))
    }

    pub fn generation_id(&self) -> GenerationId {
        generation_id(self.id(), self.pinned_root)
    }
}

/// Validation errors that make a transaction non-promotable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManifestError {
    EmptyGraph,
    DuplicateNode(ReceiptKey),
    DuplicateReceipt(Digest),
    DuplicateArtifact {
        node: ReceiptKey,
        blob: Digest,
    },
    EmptyEdgeLabel,
    DuplicateEdgeLabel {
        consumer: ReceiptKey,
        label: String,
    },
    MissingProducer(ReceiptKey),
    MissingConsumer(ReceiptKey),
    ProjectionMismatch {
        producer: ReceiptKey,
        projection: Projection,
        expected: Digest,
        actual: Digest,
    },
    Cycle,
    GraphTooLarge,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyGraph => formatter.write_str("transaction graph is empty"),
            Self::DuplicateNode(key) => write!(formatter, "duplicate transaction node {key}"),
            Self::DuplicateReceipt(receipt) => {
                write!(formatter, "duplicate receipt record {receipt}")
            }
            Self::DuplicateArtifact { node, blob } => {
                write!(formatter, "node {node} repeats artifact blob {blob}")
            }
            Self::EmptyEdgeLabel => formatter.write_str("transaction edge label is empty"),
            Self::DuplicateEdgeLabel { consumer, label } => {
                write!(formatter, "consumer {consumer} repeats edge label {label:?}")
            }
            Self::MissingProducer(key) => write!(formatter, "edge producer {key} is absent"),
            Self::MissingConsumer(key) => write!(formatter, "edge consumer {key} is absent"),
            Self::ProjectionMismatch {
                producer,
                projection,
                expected,
                actual,
            } => write!(
                formatter,
                "edge from {producer} has wrong {projection} digest: expected {expected}, got {actual}"
            ),
            Self::Cycle => formatter.write_str("transaction graph contains a cycle"),
            Self::GraphTooLarge => formatter.write_str("transaction graph counters overflow"),
        }
    }
}

impl Error for ManifestError {}

/// The immutable record published for a closed transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootGeneration {
    id: GenerationId,
    transaction: TransactionId,
    pinned_root: Option<GenerationId>,
}

impl RootGeneration {
    fn for_manifest(manifest: &TransactionManifest) -> Self {
        Self {
            id: manifest.generation_id(),
            transaction: manifest.id(),
            pinned_root: manifest.pinned_root,
        }
    }

    pub const fn id(&self) -> GenerationId {
        self.id
    }

    pub const fn transaction(&self) -> TransactionId {
        self.transaction
    }

    pub const fn pinned_root(&self) -> Option<GenerationId> {
        self.pinned_root
    }
}

/// Result of an immutable compare-and-create operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CasOutcome<T> {
    Created(T),
    AlreadyPresent(T),
}

impl<T> CasOutcome<T> {
    pub const fn was_created(&self) -> bool {
        matches!(self, Self::Created(_))
    }

    pub fn value(&self) -> &T {
        match self {
            Self::Created(value) | Self::AlreadyPresent(value) => value,
        }
    }

    pub fn into_value(self) -> T {
        match self {
            Self::Created(value) | Self::AlreadyPresent(value) => value,
        }
    }
}

/// Deterministic recovery accounting. Temporary files are intentionally
/// ignored and incomplete open transactions remain available for diagnosis.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecoveryReport {
    pub incomplete_transactions: usize,
    pub incomplete_transaction_ids: Vec<TransactionId>,
    pub recovered_generations: usize,
    pub existing_generations: usize,
    pub blocked_transactions: usize,
    pub rejected_records: usize,
}

/// Filesystem-backed transaction and immutable-generation store.
#[derive(Clone, Debug)]
pub struct TransactionStore {
    root: PathBuf,
    transactions: PathBuf,
    generations: PathBuf,
}

impl TransactionStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TransactionError> {
        let root = path.as_ref().to_path_buf();
        let transactions = root.join("transactions");
        let generations = root.join("generations");
        fs::create_dir_all(&transactions)?;
        fs::create_dir_all(&generations)?;
        Ok(Self {
            root,
            transactions,
            generations,
        })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Persists a complete but explicitly open transaction. It cannot be
    /// published until `close` creates the independently checksummed close
    /// record.
    pub fn stage(
        &self,
        manifest: &TransactionManifest,
    ) -> Result<CasOutcome<TransactionId>, TransactionError> {
        manifest.validate()?;
        self.require_pinned_root(manifest.pinned_root)?;
        let id = manifest.id();
        let record = encode_transaction_record(manifest, TransactionPhase::Open);
        let created = publish_immutable(&self.open_path(id), &record)?;
        Ok(if created {
            CasOutcome::Created(id)
        } else {
            CasOutcome::AlreadyPresent(id)
        })
    }

    /// Atomically records the complete marker. Repeating close is safe.
    pub fn close(&self, id: TransactionId) -> Result<CasOutcome<TransactionId>, TransactionError> {
        if let Some(manifest) = self.load_transaction(id, TransactionPhase::Closed)? {
            self.require_pinned_root(manifest.pinned_root)?;
            return Ok(CasOutcome::AlreadyPresent(id));
        }
        let manifest = self
            .load_transaction(id, TransactionPhase::Open)?
            .ok_or(TransactionError::MissingOpenTransaction(id))?;
        self.require_pinned_root(manifest.pinned_root)?;
        let record = encode_transaction_record(&manifest, TransactionPhase::Closed);
        let created = publish_immutable(&self.closed_path(id), &record)?;
        Ok(if created {
            CasOutcome::Created(id)
        } else {
            CasOutcome::AlreadyPresent(id)
        })
    }

    /// Publishes the deterministic generation for a closed transaction.
    /// Concurrent callers race on an atomic hard-link create; one creates the
    /// name and all others verify and adopt the identical immutable bytes.
    pub fn publish(
        &self,
        id: TransactionId,
    ) -> Result<CasOutcome<RootGeneration>, TransactionError> {
        let manifest = self
            .load_transaction(id, TransactionPhase::Closed)?
            .ok_or(TransactionError::TransactionNotClosed(id))?;
        self.require_pinned_root(manifest.pinned_root)?;
        let generation = RootGeneration::for_manifest(&manifest);
        let record = encode_generation_record(&generation);
        let created = publish_immutable(&self.generation_path(generation.id), &record)?;
        Ok(if created {
            CasOutcome::Created(generation)
        } else {
            CasOutcome::AlreadyPresent(generation)
        })
    }

    pub fn promote(
        &self,
        manifest: &TransactionManifest,
    ) -> Result<CasOutcome<RootGeneration>, TransactionError> {
        let id = self.stage(manifest)?.into_value();
        self.close(id)?;
        self.publish(id)
    }

    /// Opens exactly the generation named by the caller. There is no mutable
    /// `latest` pointer to follow.
    pub fn load_generation(
        &self,
        id: GenerationId,
    ) -> Result<Option<RootGeneration>, TransactionError> {
        let path = self.generation_path(id);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let generation = decode_generation_record(&bytes)
            .map_err(|error| TransactionError::CorruptRecord(path.clone(), error.to_string()))?;
        if generation.id != id {
            return Err(TransactionError::RecordIdentityMismatch(path));
        }
        Ok(Some(generation))
    }

    /// Replays publication for every valid closed transaction. Open-only
    /// transactions are counted but never promoted.
    pub fn recover(&self) -> Result<RecoveryReport, TransactionError> {
        let (open, rejected_open) = self.scan_transactions(TransactionPhase::Open)?;
        let (closed, rejected_closed) = self.scan_transactions(TransactionPhase::Closed)?;
        let closed_ids: BTreeSet<_> = closed.keys().copied().collect();
        let incomplete_transaction_ids: Vec<_> = open
            .keys()
            .filter(|id| !closed_ids.contains(id))
            .copied()
            .collect();
        let mut report = RecoveryReport {
            incomplete_transactions: incomplete_transaction_ids.len(),
            incomplete_transaction_ids,
            rejected_records: rejected_open + rejected_closed,
            ..RecoveryReport::default()
        };

        let mut pending: BTreeMap<GenerationId, TransactionId> = closed
            .iter()
            .map(|(id, manifest)| (manifest.generation_id(), *id))
            .collect();
        loop {
            let candidates: Vec<_> = pending.keys().copied().collect();
            let mut progressed = false;
            for generation_id in candidates {
                let transaction_id = pending[&generation_id];
                let manifest = &closed[&transaction_id];
                if let Some(parent) = manifest.pinned_root {
                    if self.load_generation(parent)?.is_none() {
                        continue;
                    }
                }
                match self.publish(transaction_id)? {
                    CasOutcome::Created(_) => report.recovered_generations += 1,
                    CasOutcome::AlreadyPresent(_) => report.existing_generations += 1,
                }
                pending.remove(&generation_id);
                progressed = true;
            }
            if !progressed {
                break;
            }
        }
        report.blocked_transactions = pending.len();
        Ok(report)
    }

    fn require_pinned_root(
        &self,
        pinned_root: Option<GenerationId>,
    ) -> Result<(), TransactionError> {
        if let Some(id) = pinned_root {
            if self.load_generation(id)?.is_none() {
                return Err(TransactionError::MissingPinnedRoot(id));
            }
        }
        Ok(())
    }

    fn load_transaction(
        &self,
        id: TransactionId,
        phase: TransactionPhase,
    ) -> Result<Option<TransactionManifest>, TransactionError> {
        let path = match phase {
            TransactionPhase::Open => self.open_path(id),
            TransactionPhase::Closed => self.closed_path(id),
        };
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let (record_id, manifest) = decode_transaction_record(&bytes, phase)
            .map_err(|error| TransactionError::CorruptRecord(path.clone(), error.to_string()))?;
        if record_id != id {
            return Err(TransactionError::RecordIdentityMismatch(path));
        }
        Ok(Some(manifest))
    }

    fn scan_transactions(
        &self,
        phase: TransactionPhase,
    ) -> Result<(BTreeMap<TransactionId, TransactionManifest>, usize), TransactionError> {
        let suffix = phase.suffix();
        let mut paths = Vec::new();
        for entry in fs::read_dir(&self.transactions)? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_string_lossy().ends_with(suffix) {
                paths.push(entry.path());
            }
        }
        paths.sort();
        let mut valid = BTreeMap::new();
        let mut rejected = 0usize;
        for path in paths {
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    rejected += 1;
                    continue;
                }
            };
            let Ok((id, manifest)) = decode_transaction_record(&bytes, phase) else {
                rejected += 1;
                continue;
            };
            if path != self.transaction_path(id, phase) {
                rejected += 1;
                continue;
            }
            valid.insert(id, manifest);
        }
        Ok((valid, rejected))
    }

    fn transaction_path(&self, id: TransactionId, phase: TransactionPhase) -> PathBuf {
        self.transactions
            .join(format!("{}{suffix}", id.to_hex(), suffix = phase.suffix()))
    }

    fn open_path(&self, id: TransactionId) -> PathBuf {
        self.transaction_path(id, TransactionPhase::Open)
    }

    fn closed_path(&self, id: TransactionId) -> PathBuf {
        self.transaction_path(id, TransactionPhase::Closed)
    }

    fn generation_path(&self, id: GenerationId) -> PathBuf {
        self.generations.join(format!("{}.generation", id.to_hex()))
    }
}

/// Durable transaction/promotion failures.
#[derive(Debug)]
pub enum TransactionError {
    Io(io::Error),
    InvalidManifest(ManifestError),
    MissingOpenTransaction(TransactionId),
    TransactionNotClosed(TransactionId),
    MissingPinnedRoot(GenerationId),
    CorruptRecord(PathBuf, String),
    RecordIdentityMismatch(PathBuf),
    ImmutableConflict(PathBuf),
}

impl fmt::Display for TransactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::InvalidManifest(error) => error.fmt(formatter),
            Self::MissingOpenTransaction(id) => {
                write!(formatter, "open transaction {id} is absent")
            }
            Self::TransactionNotClosed(id) => write!(formatter, "transaction {id} is not closed"),
            Self::MissingPinnedRoot(id) => {
                write!(formatter, "pinned root generation {id} is absent")
            }
            Self::CorruptRecord(path, reason) => {
                write!(formatter, "corrupt record {}: {reason}", path.display())
            }
            Self::RecordIdentityMismatch(path) => {
                write!(
                    formatter,
                    "record identity does not match {}",
                    path.display()
                )
            }
            Self::ImmutableConflict(path) => {
                write!(formatter, "immutable object conflict at {}", path.display())
            }
        }
    }
}

impl Error for TransactionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidManifest(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for TransactionError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ManifestError> for TransactionError {
    fn from(error: ManifestError) -> Self {
        Self::InvalidManifest(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransactionPhase {
    Open,
    Closed,
}

impl TransactionPhase {
    const fn tag(self) -> u8 {
        match self {
            Self::Open => 0,
            Self::Closed => 1,
        }
    }

    const fn suffix(self) -> &'static str {
        match self {
            Self::Open => ".open.transaction",
            Self::Closed => ".closed.transaction",
        }
    }
}

fn generation_id(transaction: TransactionId, pinned_root: Option<GenerationId>) -> GenerationId {
    let mut encoded = Vec::new();
    encoded.extend_from_slice(b"root-generation/v1\0");
    put_digest(&mut encoded, transaction.digest());
    put_optional_generation(&mut encoded, pinned_root);
    GenerationId(sha256(&encoded))
}

fn encode_manifest(manifest: &TransactionManifest) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(b"transaction-manifest/v1\0");
    put_optional_generation(&mut output, manifest.pinned_root);

    let mut nodes = manifest.nodes.clone();
    nodes.sort_by_key(|node| node.key);
    put_u64(&mut output, nodes.len() as u64);
    for mut node in nodes {
        put_digest(&mut output, node.key.digest());
        put_digest(&mut output, node.receipt_id);
        put_digest(&mut output, node.outputs.core);
        put_digest(&mut output, node.outputs.envelope);
        node.artifact_blob_ids.sort();
        put_u64(&mut output, node.artifact_blob_ids.len() as u64);
        for blob in node.artifact_blob_ids {
            put_digest(&mut output, blob);
        }
    }

    let mut edges = manifest.edges.clone();
    edges.sort_by(|left, right| {
        left.consumer
            .cmp(&right.consumer)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.producer.cmp(&right.producer))
            .then_with(|| left.projection.cmp(&right.projection))
            .then_with(|| left.digest.cmp(&right.digest))
    });
    put_u64(&mut output, edges.len() as u64);
    for edge in edges {
        put_digest(&mut output, edge.producer.digest());
        put_digest(&mut output, edge.consumer.digest());
        put_string(&mut output, &edge.label);
        output.push(projection_tag(edge.projection));
        put_digest(&mut output, edge.digest);
    }
    output
}

fn decode_manifest(bytes: &[u8]) -> io::Result<TransactionManifest> {
    let mut cursor = Cursor::new(bytes);
    cursor.expect(b"transaction-manifest/v1\0")?;
    let pinned_root = cursor.optional_generation()?;
    let node_count = cursor.usize()?;
    let mut nodes = Vec::with_capacity(node_count.min(1024));
    for _ in 0..node_count {
        let key = ReceiptKey::from(cursor.digest()?);
        let receipt_id = cursor.digest()?;
        let outputs = ReceiptOutputs::new(cursor.digest()?, cursor.digest()?);
        let blob_count = cursor.usize()?;
        let mut artifact_blob_ids = Vec::with_capacity(blob_count.min(1024));
        for _ in 0..blob_count {
            artifact_blob_ids.push(cursor.digest()?);
        }
        nodes.push(TransactionNode::new(
            key,
            receipt_id,
            outputs,
            artifact_blob_ids,
        ));
    }
    let edge_count = cursor.usize()?;
    let mut edges = Vec::with_capacity(edge_count.min(1024));
    for _ in 0..edge_count {
        let producer = ReceiptKey::from(cursor.digest()?);
        let consumer = ReceiptKey::from(cursor.digest()?);
        let label = cursor.string()?;
        let projection = projection_from_tag(cursor.byte()?)?;
        let digest = cursor.digest()?;
        edges.push(TransactionEdge::new(
            producer, consumer, label, projection, digest,
        ));
    }
    if !cursor.done() {
        return Err(invalid_data("transaction manifest has trailing bytes"));
    }
    TransactionManifest::new(pinned_root, nodes, edges)
        .map_err(|error| invalid_data(&error.to_string()))
}

fn encode_transaction_record(manifest: &TransactionManifest, phase: TransactionPhase) -> Vec<u8> {
    let payload = encode_manifest(manifest);
    let mut body = Vec::new();
    body.push(phase.tag());
    put_digest(&mut body, manifest.id().digest());
    put_u64(&mut body, payload.len() as u64);
    body.extend_from_slice(&payload);
    wrap_record(TRANSACTION_MAGIC, &body)
}

fn decode_transaction_record(
    bytes: &[u8],
    expected_phase: TransactionPhase,
) -> io::Result<(TransactionId, TransactionManifest)> {
    let body = unwrap_record(bytes, TRANSACTION_MAGIC)?;
    let mut cursor = Cursor::new(body);
    if cursor.byte()? != expected_phase.tag() {
        return Err(invalid_data("transaction phase mismatch"));
    }
    let id = TransactionId(cursor.digest()?);
    let payload_length = cursor.usize()?;
    let payload = cursor.take(payload_length)?;
    if !cursor.done() {
        return Err(invalid_data("transaction record has trailing bytes"));
    }
    let manifest = decode_manifest(payload)?;
    if manifest.id() != id {
        return Err(invalid_data("transaction digest mismatch"));
    }
    Ok((id, manifest))
}

fn encode_generation_record(generation: &RootGeneration) -> Vec<u8> {
    let mut body = Vec::new();
    put_digest(&mut body, generation.id.digest());
    put_digest(&mut body, generation.transaction.digest());
    put_optional_generation(&mut body, generation.pinned_root);
    wrap_record(GENERATION_MAGIC, &body)
}

fn decode_generation_record(bytes: &[u8]) -> io::Result<RootGeneration> {
    let body = unwrap_record(bytes, GENERATION_MAGIC)?;
    let mut cursor = Cursor::new(body);
    let id = GenerationId(cursor.digest()?);
    let transaction = TransactionId(cursor.digest()?);
    let pinned_root = cursor.optional_generation()?;
    if !cursor.done() {
        return Err(invalid_data("generation record has trailing bytes"));
    }
    if generation_id(transaction, pinned_root) != id {
        return Err(invalid_data("generation digest mismatch"));
    }
    Ok(RootGeneration {
        id,
        transaction,
        pinned_root,
    })
}

fn wrap_record(magic: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(4 + 4 + 8 + body.len() + 32);
    output.extend_from_slice(magic);
    put_u32(&mut output, RECORD_VERSION);
    put_u64(&mut output, body.len() as u64);
    output.extend_from_slice(body);
    put_digest(&mut output, sha256(body));
    output
}

fn unwrap_record<'a>(record: &'a [u8], magic: &[u8; 4]) -> io::Result<&'a [u8]> {
    let mut cursor = Cursor::new(record);
    if cursor.take(4)? != magic {
        return Err(invalid_data("record magic mismatch"));
    }
    if cursor.u32()? != RECORD_VERSION {
        return Err(invalid_data("unsupported record version"));
    }
    let body_length = cursor.u64()?;
    if body_length > MAX_RECORD_BYTES {
        return Err(invalid_data("record exceeds size limit"));
    }
    let body_length = usize::try_from(body_length).map_err(|_| invalid_data("record too large"))?;
    let body = cursor.take(body_length)?;
    let checksum = cursor.digest()?;
    if !cursor.done() || sha256(body) != checksum {
        return Err(invalid_data("record checksum or trailing bytes mismatch"));
    }
    Ok(body)
}

fn publish_immutable(path: &Path, bytes: &[u8]) -> Result<bool, TransactionError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_data("immutable object has no parent directory"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid_data("immutable object filename is not UTF-8"))?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", unique_suffix()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);

    match fs::hard_link(&temporary, path) {
        Ok(()) => {
            fs::remove_file(&temporary)?;
            sync_directory(parent)?;
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let existing = fs::read(path)?;
            fs::remove_file(&temporary)?;
            if existing == bytes {
                Ok(false)
            } else {
                Err(TransactionError::ImmutableConflict(path.to_path_buf()))
            }
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error.into())
        }
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn unique_suffix() -> String {
    let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{nanos}-{sequence}", std::process::id())
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_string(output: &mut Vec<u8>, value: &str) {
    put_u64(output, value.len() as u64);
    output.extend_from_slice(value.as_bytes());
}

fn put_digest(output: &mut Vec<u8>, value: Digest) {
    output.extend_from_slice(value.as_bytes());
}

fn put_optional_generation(output: &mut Vec<u8>, value: Option<GenerationId>) {
    match value {
        Some(id) => {
            output.push(1);
            put_digest(output, id.digest());
        }
        None => output.push(0),
    }
}

const fn projection_tag(projection: Projection) -> u8 {
    match projection {
        Projection::Core => 0,
        Projection::Envelope => 1,
    }
}

fn projection_from_tag(tag: u8) -> io::Result<Projection> {
    match tag {
        0 => Ok(Projection::Core),
        1 => Ok(Projection::Envelope),
        _ => Err(invalid_data("invalid projection tag")),
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> io::Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| invalid_data("record offset overflow"))?;
        if end > self.bytes.len() {
            return Err(invalid_data("truncated record"));
        }
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    fn expect(&mut self, expected: &[u8]) -> io::Result<()> {
        if self.take(expected.len())? != expected {
            return Err(invalid_data("canonical domain mismatch"));
        }
        Ok(())
    }

    fn byte(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("length checked"),
        ))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("length checked"),
        ))
    }

    fn usize(&mut self) -> io::Result<usize> {
        usize::try_from(self.u64()?).map_err(|_| invalid_data("record count is too large"))
    }

    fn digest(&mut self) -> io::Result<Digest> {
        Ok(Digest::from_bytes(
            self.take(32)?.try_into().expect("length checked"),
        ))
    }

    fn optional_generation(&mut self) -> io::Result<Option<GenerationId>> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(GenerationId(self.digest()?))),
            _ => Err(invalid_data("invalid optional generation tag")),
        }
    }

    fn string(&mut self) -> io::Result<String> {
        let length = self.u64()?;
        if length > MAX_STRING_BYTES {
            return Err(invalid_data("record string exceeds size limit"));
        }
        let length = usize::try_from(length).map_err(|_| invalid_data("string is too large"))?;
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| invalid_data("record string is not UTF-8"))
    }

    fn done(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("new-ci-m3-transaction-{label}-{}", unique_suffix()));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn digest(value: &str) -> Digest {
        sha256(value.as_bytes())
    }

    fn key(value: &str) -> ReceiptKey {
        ReceiptKey::from(digest(value))
    }

    fn manifest(parent: Option<GenerationId>, suffix: &str) -> TransactionManifest {
        let producer_key = key(&format!("producer-{suffix}"));
        let consumer_key = key(&format!("consumer-{suffix}"));
        let producer_outputs = ReceiptOutputs::new(
            digest(&format!("core-{suffix}")),
            digest(&format!("envelope-{suffix}")),
        );
        let consumer_outputs = ReceiptOutputs::new(
            digest(&format!("consumer-core-{suffix}")),
            digest(&format!("consumer-envelope-{suffix}")),
        );
        TransactionManifest::new(
            parent,
            [
                TransactionNode::new(
                    producer_key,
                    digest(&format!("producer-receipt-{suffix}")),
                    producer_outputs,
                    [digest(&format!("producer-blob-{suffix}"))],
                ),
                TransactionNode::new(
                    consumer_key,
                    digest(&format!("consumer-receipt-{suffix}")),
                    consumer_outputs,
                    [digest(&format!("consumer-blob-{suffix}"))],
                ),
            ],
            [TransactionEdge::new(
                producer_key,
                consumer_key,
                "producer-core",
                Projection::Core,
                producer_outputs.core,
            )],
        )
        .expect("valid manifest")
    }

    #[test]
    fn kill_before_close_never_promotes_and_old_root_survives() {
        let directory = TestDirectory::new("before-close");
        let store = TransactionStore::open(&directory.0).expect("open store");
        let first = manifest(None, "first");
        let old_generation = store
            .promote(&first)
            .expect("promote old root")
            .into_value();

        let next = manifest(Some(old_generation.id()), "next");
        let expected_next = next.generation_id();
        store.stage(&next).expect("stage next transaction");
        drop(store); // kill window: complete payload exists, close marker does not.

        let recovered = TransactionStore::open(&directory.0).expect("reopen store");
        let report = recovered.recover().expect("recover before-close window");
        assert_eq!(report.incomplete_transactions, 1);
        assert_eq!(report.incomplete_transaction_ids, vec![next.id()]);
        assert_eq!(report.recovered_generations, 0);
        assert!(recovered
            .load_generation(expected_next)
            .expect("load next")
            .is_none());
        assert_eq!(
            recovered
                .load_generation(old_generation.id())
                .expect("load old root"),
            Some(old_generation)
        );

        // Recovery exposes the durable open transaction for an explicit
        // coordinator decision; it still never promotes it implicitly.
        recovered.close(next.id()).expect("resume and close");
        let promoted = recovered
            .publish(next.id())
            .expect("resume publication")
            .into_value();
        assert_eq!(promoted.id(), expected_next);
    }

    #[test]
    fn kill_after_close_replays_publication_idempotently() {
        let directory = TestDirectory::new("after-close");
        let store = TransactionStore::open(&directory.0).expect("open store");
        let first = manifest(None, "first");
        let old_generation = store
            .promote(&first)
            .expect("promote old root")
            .into_value();
        let next = manifest(Some(old_generation.id()), "next");
        let next_id = next.id();
        let expected_generation = next.generation_id();
        store.stage(&next).expect("stage next transaction");
        store.close(next_id).expect("close next transaction");
        assert!(store
            .load_generation(expected_generation)
            .expect("generation lookup")
            .is_none());
        drop(store); // kill window: close is durable, root publication is absent.

        let recovered = TransactionStore::open(&directory.0).expect("reopen store");
        let first_report = recovered.recover().expect("replay publication");
        assert_eq!(first_report.recovered_generations, 1);
        assert_eq!(first_report.blocked_transactions, 0);
        let new_generation = recovered
            .load_generation(expected_generation)
            .expect("load recovered generation")
            .expect("recovered generation exists");
        assert_eq!(new_generation.transaction(), next_id);
        assert!(recovered
            .load_generation(old_generation.id())
            .expect("load old root")
            .is_some());

        let second_report = recovered.recover().expect("repeat recovery");
        assert_eq!(second_report.recovered_generations, 0);
        assert_eq!(second_report.existing_generations, 2);
    }

    #[test]
    fn concurrent_generation_cas_mint_has_exactly_one_creator() {
        const RACERS: usize = 16;
        let directory = TestDirectory::new("cas-race");
        let store = Arc::new(TransactionStore::open(&directory.0).expect("open store"));
        let transaction = manifest(None, "race");
        let transaction_id = store
            .stage(&transaction)
            .expect("stage transaction")
            .into_value();
        store.close(transaction_id).expect("close transaction");

        let barrier = Arc::new(Barrier::new(RACERS));
        let mut threads = Vec::new();
        for _ in 0..RACERS {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            threads.push(thread::spawn(move || {
                barrier.wait();
                store.publish(transaction_id).expect("CAS publication")
            }));
        }
        let outcomes: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().expect("racer did not panic"))
            .collect();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| outcome.was_created())
                .count(),
            1
        );
        let ids: BTreeSet<_> = outcomes
            .into_iter()
            .map(|outcome| outcome.into_value().id())
            .collect();
        assert_eq!(ids, BTreeSet::from([transaction.generation_id()]));
        assert!(store
            .load_generation(transaction.generation_id())
            .expect("load generation")
            .is_some());
    }

    #[test]
    fn edge_digest_and_acyclicity_are_verified_before_staging() {
        let valid = manifest(None, "validation");
        let mut wrong_digest = valid.clone();
        wrong_digest.edges[0].digest = digest("forged-output");
        assert!(matches!(
            wrong_digest.validate(),
            Err(ManifestError::ProjectionMismatch { .. })
        ));

        let mut cycle = valid;
        let producer = cycle.nodes[0].key;
        let consumer = cycle.nodes[1].key;
        cycle.edges.push(TransactionEdge::new(
            consumer,
            producer,
            "cycle",
            Projection::Core,
            cycle.nodes[1].outputs.core,
        ));
        assert_eq!(cycle.validate(), Err(ManifestError::Cycle));
    }
}
