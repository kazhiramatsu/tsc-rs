//! Small, dependency-free substrate for content-addressed CI evidence.
//!
//! The key deliberately has no execution-context fields.  Execution metadata
//! is retained on a receipt for diagnosis, but cannot create another semantic
//! cache version.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

const RECORD_MAGIC: &[u8; 4] = b"NCIR";
const RECORD_VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_STRING_BYTES: u64 = 16 * 1024 * 1024;
static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(1);

/// A SHA-256 digest.
#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Digest([u8; 32]);

impl Digest {
    /// Constructs a digest from its raw 32-byte representation.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns lowercase hexadecimal form.
    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut result = String::with_capacity(64);
        for byte in self.0 {
            result.push(HEX[(byte >> 4) as usize] as char);
            result.push(HEX[(byte & 0x0f) as usize] as char);
        }
        result
    }

    /// Parses exactly 64 lowercase or uppercase hexadecimal characters.
    pub fn from_hex(value: &str) -> Result<Self, DigestError> {
        if value.len() != 64 {
            return Err(DigestError::WrongLength(value.len()));
        }
        let bytes = value.as_bytes();
        let mut output = [0u8; 32];
        for (index, pair) in bytes.chunks_exact(2).enumerate() {
            let high = hex_value(pair[0]).ok_or(DigestError::InvalidHex(index * 2))?;
            let low = hex_value(pair[1]).ok_or(DigestError::InvalidHex(index * 2 + 1))?;
            output[index] = (high << 4) | low;
        }
        Ok(Self(output))
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Digest")
            .field(&self.to_hex())
            .finish()
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl FromStr for Digest {
    type Err = DigestError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_hex(value)
    }
}

/// Errors raised while parsing a digest.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DigestError {
    WrongLength(usize),
    InvalidHex(usize),
}

impl fmt::Display for DigestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength(length) => {
                write!(formatter, "digest has {length} characters; expected 64")
            }
            Self::InvalidHex(index) => {
                write!(formatter, "invalid hexadecimal digit at byte {index}")
            }
        }
    }
}

impl Error for DigestError {}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// SHA-256, implemented here to keep the standalone project usable offline.
pub fn sha256(value: &[u8]) -> Digest {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const ROUND_CONSTANTS: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let block_count = (value.len() + 9).div_ceil(64);
    let mut padded = Vec::with_capacity(block_count * 64);
    padded.extend_from_slice(value);
    padded.push(0x80);
    padded.resize(block_count * 64 - 8, 0);
    padded.extend_from_slice(&(value.len() as u64 * 8).to_be_bytes());

    let mut state = INITIAL;
    for block in padded.chunks_exact(64) {
        let mut schedule = [0u32; 64];
        for (index, word) in schedule.iter_mut().take(16).enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                block[offset],
                block[offset + 1],
                block[offset + 2],
                block[offset + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let s1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
        }

        let mut working = state;
        for (index, &constant) in ROUND_CONSTANTS.iter().enumerate() {
            let s1 = working[4].rotate_right(6)
                ^ working[4].rotate_right(11)
                ^ working[4].rotate_right(25);
            let choice = (working[4] & working[5]) ^ ((!working[4]) & working[6]);
            let temporary_1 = working[7]
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(constant)
                .wrapping_add(schedule[index]);
            let s0 = working[0].rotate_right(2)
                ^ working[0].rotate_right(13)
                ^ working[0].rotate_right(22);
            let majority =
                (working[0] & working[1]) ^ (working[0] & working[2]) ^ (working[1] & working[2]);
            let temporary_2 = s0.wrapping_add(majority);
            working[7] = working[6];
            working[6] = working[5];
            working[5] = working[4];
            working[4] = working[3].wrapping_add(temporary_1);
            working[3] = working[2];
            working[2] = working[1];
            working[1] = working[0];
            working[0] = temporary_1.wrapping_add(temporary_2);
        }
        for (value, addition) in state.iter_mut().zip(working) {
            *value = value.wrapping_add(addition);
        }
    }

    let mut digest = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    Digest(digest)
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_string(output: &mut Vec<u8>, value: &str) {
    put_u64(output, value.len() as u64);
    output.extend_from_slice(value.as_bytes());
}

fn put_digest(output: &mut Vec<u8>, value: Digest) {
    output.extend_from_slice(value.as_bytes());
}

fn put_optional_digest(output: &mut Vec<u8>, value: Option<Digest>) {
    match value {
        Some(value) => {
            output.push(1);
            put_digest(output, value);
        }
        None => output.push(0),
    }
}

/// A complete producer identity.  A bare action name is intentionally not a
/// valid substitute: tool, tool version, definition, and implementation all
/// participate in the producer digest.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Action {
    pub tool: String,
    pub version: String,
    pub definition_digest: Digest,
    pub implementation_digest: Digest,
}

impl Action {
    /// Creates an action from already calculated definition and implementation
    /// digests.
    pub fn new(
        tool: impl Into<String>,
        version: impl Into<String>,
        definition_digest: Digest,
        implementation_digest: Digest,
    ) -> Self {
        Self {
            tool: tool.into(),
            version: version.into(),
            definition_digest,
            implementation_digest,
        }
    }

    /// Creates an action by hashing the action definition bytes.
    pub fn from_definition(
        tool: impl Into<String>,
        version: impl Into<String>,
        definition: &[u8],
        implementation_digest: Digest,
    ) -> Self {
        Self::new(tool, version, sha256(definition), implementation_digest)
    }

    /// Digest of the complete producer identity.
    pub fn producer_digest(&self) -> Digest {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(b"action-producer/v1\0");
        put_string(&mut encoded, &self.tool);
        put_string(&mut encoded, &self.version);
        put_digest(&mut encoded, self.definition_digest);
        put_digest(&mut encoded, self.implementation_digest);
        sha256(&encoded)
    }
}

/// A semantic input manifest entry.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ManifestEntry {
    pub label: String,
    pub digest: Digest,
}

impl ManifestEntry {
    pub fn new(label: impl Into<String>, digest: Digest) -> Self {
        Self {
            label: label.into(),
            digest,
        }
    }
}

/// Semantic inputs, kept separate from execution metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SemanticInputManifest {
    pub entries: Vec<ManifestEntry>,
}

impl SemanticInputManifest {
    /// Constructs a manifest without changing caller order.  `ReceiptKey`
    /// canonicalization sorts it; `try_new` additionally rejects duplicates.
    pub fn new(entries: impl IntoIterator<Item = ManifestEntry>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    pub fn try_new(entries: impl IntoIterator<Item = ManifestEntry>) -> Result<Self, KeyError> {
        let manifest = Self::new(entries);
        validate_manifest(&manifest)?;
        Ok(manifest)
    }
}

/// The two required output projections.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Projection {
    Core,
    Envelope,
}

impl Projection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Envelope => "envelope",
        }
    }
}

impl fmt::Display for Projection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One labelled dependency edge in a receipt key.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DependencyOutput {
    pub label: String,
    pub projection: Projection,
    pub digest: Digest,
}

impl DependencyOutput {
    pub fn new(label: impl Into<String>, projection: Projection, digest: Digest) -> Self {
        Self {
            label: label.into(),
            projection,
            digest,
        }
    }

    pub fn core(label: impl Into<String>, digest: Digest) -> Self {
        Self::new(label, Projection::Core, digest)
    }

    pub fn envelope(label: impl Into<String>, digest: Digest) -> Self {
        Self::new(label, Projection::Envelope, digest)
    }
}

/// Validation failures in the canonical key inputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyError {
    EmptySchemaVersion,
    EmptyActionField(&'static str),
    EmptyInputLabel,
    DuplicateInputLabel(String),
    EmptyDependencyLabel,
}

impl fmt::Display for KeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySchemaVersion => formatter.write_str("schema/version is empty"),
            Self::EmptyActionField(field) => write!(formatter, "action field {field} is empty"),
            Self::EmptyInputLabel => formatter.write_str("semantic input label is empty"),
            Self::DuplicateInputLabel(label) => {
                write!(formatter, "duplicate semantic input label {label:?}")
            }
            Self::EmptyDependencyLabel => formatter.write_str("dependency label is empty"),
        }
    }
}

impl Error for KeyError {}

fn validate_manifest(manifest: &SemanticInputManifest) -> Result<(), KeyError> {
    let mut labels = BTreeSet::new();
    for entry in &manifest.entries {
        if entry.label.is_empty() {
            return Err(KeyError::EmptyInputLabel);
        }
        if !labels.insert(&entry.label) {
            return Err(KeyError::DuplicateInputLabel(entry.label.clone()));
        }
    }
    Ok(())
}

/// The content-addressed receipt key.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ReceiptKey(Digest);

impl ReceiptKey {
    /// Derives the key from the exact five components in the design formula:
    /// schema/version, complete producer digest, semantic manifest, sorted
    /// labelled dependency projections, and an explicit baseline option.
    pub fn new(
        schema_version: &str,
        action: &Action,
        manifest: &SemanticInputManifest,
        dependencies: &[DependencyOutput],
        baseline: Option<Digest>,
    ) -> Self {
        Self::try_new(schema_version, action, manifest, dependencies, baseline)
            .expect("invalid receipt-key component")
    }

    pub fn try_new(
        schema_version: &str,
        action: &Action,
        manifest: &SemanticInputManifest,
        dependencies: &[DependencyOutput],
        baseline: Option<Digest>,
    ) -> Result<Self, KeyError> {
        if schema_version.is_empty() {
            return Err(KeyError::EmptySchemaVersion);
        }
        for (field, value) in [("tool", &action.tool), ("version", &action.version)] {
            if value.is_empty() {
                return Err(KeyError::EmptyActionField(field));
            }
        }
        validate_manifest(manifest)?;
        if dependencies
            .iter()
            .any(|dependency| dependency.label.is_empty())
        {
            return Err(KeyError::EmptyDependencyLabel);
        }

        let mut preimage = Vec::new();
        preimage.extend_from_slice(b"receipt-key/v1\0");
        put_string(&mut preimage, schema_version);
        put_digest(&mut preimage, action.producer_digest());

        let mut inputs = manifest.entries.clone();
        inputs.sort_by(|left, right| left.label.cmp(&right.label));
        put_u64(&mut preimage, inputs.len() as u64);
        for input in inputs {
            put_string(&mut preimage, &input.label);
            put_digest(&mut preimage, input.digest);
        }

        let mut sorted_dependencies = dependencies.to_vec();
        sorted_dependencies.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then_with(|| left.projection.cmp(&right.projection))
                .then_with(|| left.digest.cmp(&right.digest))
        });
        put_u64(&mut preimage, sorted_dependencies.len() as u64);
        for dependency in sorted_dependencies {
            put_string(&mut preimage, &dependency.label);
            preimage.push(match dependency.projection {
                Projection::Core => 0,
                Projection::Envelope => 1,
            });
            put_digest(&mut preimage, dependency.digest);
        }

        match baseline {
            Some(digest) => {
                preimage.push(1);
                put_digest(&mut preimage, digest);
            }
            None => preimage.push(0),
        }
        Ok(Self(sha256(&preimage)))
    }

    pub const fn digest(self) -> Digest {
        self.0
    }

    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }
}

impl fmt::Display for ReceiptKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<Digest> for ReceiptKey {
    fn from(value: Digest) -> Self {
        Self(value)
    }
}

/// The two output digests minted by a successful producer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiptOutputs {
    pub core: Digest,
    pub envelope: Digest,
}

impl ReceiptOutputs {
    pub const fn new(core: Digest, envelope: Digest) -> Self {
        Self { core, envelope }
    }

    pub const fn digest(self, projection: Projection) -> Digest {
        match projection {
            Projection::Core => self.core,
            Projection::Envelope => self.envelope,
        }
    }
}

/// Result status is independent of the key: failed and diagnostic attempts
/// can coexist with a later successful receipt for the same semantic work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReceiptStatus {
    Success,
    Failed {
        error_code: Option<i32>,
        message: String,
    },
    Cancelled {
        reason: String,
    },
    TimedOut {
        deadline_unix_ms: u64,
    },
    Diagnostic {
        code: String,
        message: String,
    },
}

impl ReceiptStatus {
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }
}

/// Execution-only facts retained for observability and never included in a
/// [`ReceiptKey`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecutionMetadata {
    pub shard_count: u32,
    pub priority: i32,
    pub workspace_path: String,
    pub host_fingerprint: Option<Digest>,
    pub worker_id: String,
    pub started_unix_ms: u64,
}

/// A node receipt.  `outputs` is absent for non-success statuses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeReceipt {
    pub key: ReceiptKey,
    pub status: ReceiptStatus,
    pub outputs: Option<ReceiptOutputs>,
    pub execution: ExecutionMetadata,
    pub attempt: u64,
}

impl NodeReceipt {
    pub fn success(key: ReceiptKey, outputs: ReceiptOutputs) -> Self {
        Self {
            key,
            status: ReceiptStatus::Success,
            outputs: Some(outputs),
            execution: ExecutionMetadata::default(),
            attempt: 0,
        }
    }

    pub fn with_execution(mut self, execution: ExecutionMetadata) -> Self {
        self.execution = execution;
        self
    }

    pub fn is_cache_hit(&self) -> bool {
        self.status.is_success() && self.outputs.is_some()
    }

    pub fn failed(key: ReceiptKey, error_code: Option<i32>, message: impl Into<String>) -> Self {
        Self {
            key,
            status: ReceiptStatus::Failed {
                error_code,
                message: message.into(),
            },
            outputs: None,
            execution: ExecutionMetadata::default(),
            attempt: 0,
        }
    }

    pub fn cancelled(key: ReceiptKey, reason: impl Into<String>) -> Self {
        Self {
            key,
            status: ReceiptStatus::Cancelled {
                reason: reason.into(),
            },
            outputs: None,
            execution: ExecutionMetadata::default(),
            attempt: 0,
        }
    }

    pub fn timed_out(key: ReceiptKey, deadline_unix_ms: u64) -> Self {
        Self {
            key,
            status: ReceiptStatus::TimedOut { deadline_unix_ms },
            outputs: None,
            execution: ExecutionMetadata::default(),
            attempt: 0,
        }
    }

    pub fn diagnostic(
        key: ReceiptKey,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            key,
            status: ReceiptStatus::Diagnostic {
                code: code.into(),
                message: message.into(),
            },
            outputs: None,
            execution: ExecutionMetadata::default(),
            attempt: 0,
        }
    }
}

/// A content-addressed, append-only receipt directory.
#[derive(Clone, Debug)]
pub struct ReceiptStore {
    root: PathBuf,
}

impl ReceiptStore {
    /// Opens (and, if necessary, creates) a store directory.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let root = path.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Atomically mints one record.  The temporary file is in the same
    /// directory, and the final name is unique, so rename cannot replace an
    /// existing receipt.  A killed process can leave only an ignored `.tmp`
    /// file and loses at most the in-flight node.
    pub fn mint(&self, receipt: &NodeReceipt) -> io::Result<PathBuf> {
        if !receipt_shape_is_valid(receipt) {
            return Err(invalid_data("receipt status/output shape is invalid"));
        }
        let key = receipt.key.to_hex();
        let unique = unique_file_id();
        let temporary = self.root.join(format!("{key}.{unique}.tmp"));
        let final_path = self.root.join(format!("{key}.{unique}.receipt"));
        let body = encode_receipt(receipt);
        let mut record = Vec::with_capacity(4 + 4 + 8 + body.len() + 32);
        record.extend_from_slice(RECORD_MAGIC);
        put_u32(&mut record, RECORD_VERSION);
        put_u64(&mut record, body.len() as u64);
        record.extend_from_slice(&body);
        put_digest(&mut record, sha256(&body));

        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&record)?;
        file.sync_all()?;
        drop(file);
        if let Err(error) = fs::rename(&temporary, &final_path) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(final_path)
    }

    /// Loads every valid version for a key in deterministic filename order.
    /// Invalid, incomplete, checksum-failing, or otherwise torn records are
    /// skipped and can never become a cache hit.
    pub fn load(&self, key: ReceiptKey) -> io::Result<Vec<NodeReceipt>> {
        let prefix = format!("{}.", key.to_hex());
        let mut paths = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with(&prefix) && file_name.ends_with(".receipt") {
                paths.push(entry.path());
            }
        }
        paths.sort();
        let mut receipts = Vec::new();
        for path in paths {
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            let receipt = match decode_record(&bytes) {
                Ok(receipt) if receipt.key == key => receipt,
                Ok(_) | Err(_) => continue,
            };
            receipts.push(receipt);
        }
        Ok(receipts)
    }

    pub fn latest(&self, key: ReceiptKey) -> io::Result<Option<NodeReceipt>> {
        Ok(self.load(key)?.pop())
    }

    /// Explicit GC-root hook.  The prototype has no mutable `latest` pointer
    /// and therefore returns an empty, caller-owned root set.
    pub fn gc_roots_stub(&self) -> GcRoots {
        GcRoots::default()
    }

    /// GC is intentionally a no-op in this first slice.  It reports zero
    /// deletions so callers cannot accidentally treat missing policy as safe
    /// reclamation.
    pub fn collect_garbage(&self, _roots: &GcRoots) -> io::Result<GcReport> {
        Ok(GcReport { deleted: 0 })
    }
}

fn unique_file_id() -> String {
    let counter = NEXT_FILE_ID.fetch_add(1, AtomicOrdering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{}-{}", std::process::id(), nanos, counter)
}

/// Caller-owned roots for a future mark-and-sweep implementation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GcRoots {
    pub receipt_keys: BTreeSet<ReceiptKey>,
}

/// Result of the deliberately inert GC stub.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GcReport {
    pub deleted: usize,
}

fn encode_receipt(receipt: &NodeReceipt) -> Vec<u8> {
    let mut output = Vec::new();
    put_digest(&mut output, receipt.key.digest());
    match &receipt.status {
        ReceiptStatus::Success => output.push(0),
        ReceiptStatus::Failed {
            error_code,
            message,
        } => {
            output.push(1);
            match error_code {
                Some(code) => {
                    output.push(1);
                    put_i32(&mut output, *code);
                }
                None => output.push(0),
            }
            put_string(&mut output, message);
        }
        ReceiptStatus::Cancelled { reason } => {
            output.push(2);
            put_string(&mut output, reason);
        }
        ReceiptStatus::TimedOut { deadline_unix_ms } => {
            output.push(3);
            put_u64(&mut output, *deadline_unix_ms);
        }
        ReceiptStatus::Diagnostic { code, message } => {
            output.push(4);
            put_string(&mut output, code);
            put_string(&mut output, message);
        }
    }
    match receipt.outputs {
        Some(outputs) => {
            output.push(1);
            put_digest(&mut output, outputs.core);
            put_digest(&mut output, outputs.envelope);
        }
        None => output.push(0),
    }
    put_u32(&mut output, receipt.execution.shard_count);
    put_i32(&mut output, receipt.execution.priority);
    put_string(&mut output, &receipt.execution.workspace_path);
    put_optional_digest(&mut output, receipt.execution.host_fingerprint);
    put_string(&mut output, &receipt.execution.worker_id);
    put_u64(&mut output, receipt.execution.started_unix_ms);
    put_u64(&mut output, receipt.attempt);
    output
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
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
        let result = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(result)
    }

    fn byte(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("length checked"),
        ))
    }

    fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_be_bytes(
            self.take(4)?.try_into().expect("length checked"),
        ))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("length checked"),
        ))
    }

    fn digest(&mut self) -> io::Result<Digest> {
        Ok(Digest::from_bytes(
            self.take(32)?.try_into().expect("length checked"),
        ))
    }

    fn string(&mut self) -> io::Result<String> {
        let length = self.u64()?;
        if length > MAX_STRING_BYTES {
            return Err(invalid_data("record string exceeds limit"));
        }
        let length =
            usize::try_from(length).map_err(|_| invalid_data("record string too large"))?;
        let bytes = self.take(length)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| invalid_data("record string is not UTF-8"))
    }

    fn done(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

fn decode_record(record: &[u8]) -> io::Result<NodeReceipt> {
    let mut cursor = Cursor::new(record);
    if cursor.take(4)? != RECORD_MAGIC {
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

    let mut body_cursor = Cursor::new(body);
    let key = ReceiptKey::from(body_cursor.digest()?);
    let status = match body_cursor.byte()? {
        0 => ReceiptStatus::Success,
        1 => {
            let error_code = match body_cursor.byte()? {
                0 => None,
                1 => Some(body_cursor.i32()?),
                _ => return Err(invalid_data("invalid failed status code marker")),
            };
            ReceiptStatus::Failed {
                error_code,
                message: body_cursor.string()?,
            }
        }
        2 => ReceiptStatus::Cancelled {
            reason: body_cursor.string()?,
        },
        3 => ReceiptStatus::TimedOut {
            deadline_unix_ms: body_cursor.u64()?,
        },
        4 => ReceiptStatus::Diagnostic {
            code: body_cursor.string()?,
            message: body_cursor.string()?,
        },
        _ => return Err(invalid_data("invalid receipt status")),
    };
    let outputs = match body_cursor.byte()? {
        0 => None,
        1 => Some(ReceiptOutputs::new(
            body_cursor.digest()?,
            body_cursor.digest()?,
        )),
        _ => return Err(invalid_data("invalid output marker")),
    };
    let execution = ExecutionMetadata {
        shard_count: body_cursor.u32()?,
        priority: body_cursor.i32()?,
        workspace_path: body_cursor.string()?,
        host_fingerprint: match body_cursor.byte()? {
            0 => None,
            1 => Some(body_cursor.digest()?),
            _ => return Err(invalid_data("invalid host fingerprint marker")),
        },
        worker_id: body_cursor.string()?,
        started_unix_ms: body_cursor.u64()?,
    };
    let receipt = NodeReceipt {
        key,
        status,
        outputs,
        execution,
        attempt: body_cursor.u64()?,
    };
    if !body_cursor.done() || !receipt_shape_is_valid(&receipt) {
        return Err(invalid_data("receipt has trailing body fields"));
    }
    Ok(receipt)
}

fn receipt_shape_is_valid(receipt: &NodeReceipt) -> bool {
    match &receipt.status {
        ReceiptStatus::Success => receipt.outputs.is_some(),
        ReceiptStatus::Failed { .. }
        | ReceiptStatus::Cancelled { .. }
        | ReceiptStatus::TimedOut { .. }
        | ReceiptStatus::Diagnostic { .. } => receipt.outputs.is_none(),
    }
}

/// One ordered item in a sweep batch.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BatchItem {
    pub id: String,
    pub digest: Digest,
}

impl BatchItem {
    pub fn new(id: impl Into<String>, digest: Digest) -> Self {
        Self {
            id: id.into(),
            digest,
        }
    }
}

/// A complete sub-node receipt for one contiguous batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchReceipt {
    pub sweep_id: String,
    pub batch_index: usize,
    pub batch_count: usize,
    pub range_start: usize,
    pub expected_ids: Vec<String>,
    pub items: Vec<BatchItem>,
    pub union_digest: Digest,
    pub complete: bool,
}

impl BatchReceipt {
    /// Mints only a fully proven batch.  A killed worker cannot mint this
    /// record halfway through; its incomplete work is intentionally absent.
    pub fn new(
        sweep_id: impl Into<String>,
        batch_index: usize,
        batch_count: usize,
        range_start: usize,
        expected_ids: Vec<String>,
        items: Vec<BatchItem>,
    ) -> Result<Self, BatchError> {
        let receipt = Self {
            sweep_id: sweep_id.into(),
            batch_index,
            batch_count,
            range_start,
            expected_ids,
            union_digest: batch_union_digest(&items),
            items,
            complete: true,
        };
        receipt.verify()?;
        Ok(receipt)
    }

    /// Makes an untrusted on-disk/recovery candidate for verification tests or
    /// recovery tooling.  It is not accepted by `verify` until complete.
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        sweep_id: impl Into<String>,
        batch_index: usize,
        batch_count: usize,
        range_start: usize,
        expected_ids: Vec<String>,
        items: Vec<BatchItem>,
        union_digest: Digest,
        complete: bool,
    ) -> Self {
        Self {
            sweep_id: sweep_id.into(),
            batch_index,
            batch_count,
            range_start,
            expected_ids,
            items,
            union_digest,
            complete,
        }
    }

    pub fn verify(&self) -> Result<(), BatchError> {
        if self.sweep_id.is_empty() {
            return Err(BatchError::Invalid("empty sweep id".to_string()));
        }
        if self.batch_count == 0 || self.batch_index >= self.batch_count {
            return Err(BatchError::Invalid("invalid batch index/count".to_string()));
        }
        if !self.complete {
            return Err(BatchError::Incomplete);
        }
        let mut expected = BTreeSet::new();
        for id in &self.expected_ids {
            if id.is_empty() {
                return Err(BatchError::Invalid("empty expected item id".to_string()));
            }
            if !expected.insert(id) {
                return Err(BatchError::Duplicate(id.clone()));
            }
        }
        if self.items.len() != self.expected_ids.len() {
            return Err(BatchError::Invalid(
                "batch item count does not prove completeness".to_string(),
            ));
        }
        let mut seen = BTreeSet::new();
        for (index, (item, expected_id)) in self.items.iter().zip(&self.expected_ids).enumerate() {
            if item.id != *expected_id {
                return Err(BatchError::Order {
                    index,
                    expected: expected_id.clone(),
                    actual: item.id.clone(),
                });
            }
            if !seen.insert(&item.id) {
                return Err(BatchError::Duplicate(item.id.clone()));
            }
        }
        if self.union_digest != batch_union_digest(&self.items) {
            return Err(BatchError::Invalid(
                "batch union digest mismatch".to_string(),
            ));
        }
        Ok(())
    }
}

/// Errors proving a batch or a batch union.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BatchError {
    Incomplete,
    Duplicate(String),
    Order {
        index: usize,
        expected: String,
        actual: String,
    },
    Invalid(String),
}

impl fmt::Display for BatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete => formatter.write_str("batch is incomplete"),
            Self::Duplicate(id) => write!(formatter, "duplicate batch item {id:?}"),
            Self::Order {
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "batch item {index} is {actual:?}; expected {expected:?}"
            ),
            Self::Invalid(reason) => formatter.write_str(reason),
        }
    }
}

impl Error for BatchError {}

fn batch_union_digest(items: &[BatchItem]) -> Digest {
    let mut encoded = Vec::new();
    encoded.extend_from_slice(b"batch-union/v1\0");
    put_u64(&mut encoded, items.len() as u64);
    for item in items {
        put_string(&mut encoded, &item.id);
        put_digest(&mut encoded, item.digest);
    }
    sha256(&encoded)
}

/// Verifies a complete ordered, duplicate-free union of contiguous batches
/// against the sweep's global expected ID order.
pub fn verify_batch_union(
    sweep_id: &str,
    expected_ids: &[String],
    batches: &[BatchReceipt],
) -> Result<Digest, BatchError> {
    if sweep_id.is_empty() {
        return Err(BatchError::Invalid("empty sweep id".to_string()));
    }
    let mut expected_set = BTreeSet::new();
    for id in expected_ids {
        if id.is_empty() {
            return Err(BatchError::Invalid("empty expected item id".to_string()));
        }
        if !expected_set.insert(id) {
            return Err(BatchError::Duplicate(id.clone()));
        }
    }
    if expected_ids.is_empty() && batches.is_empty() {
        return Ok(batch_union_digest(&[]));
    }
    if batches.is_empty() {
        return Err(BatchError::Invalid("missing batches".to_string()));
    }

    let batch_count = batches[0].batch_count;
    if batch_count != batches.len() {
        return Err(BatchError::Invalid(
            "batch union is missing a shard".to_string(),
        ));
    }
    let mut ordered = batches.to_vec();
    ordered.sort_by(|left, right| {
        left.range_start
            .cmp(&right.range_start)
            .then_with(|| left.batch_index.cmp(&right.batch_index))
    });
    let mut cursor = 0usize;
    let mut all_items = Vec::new();
    for (expected_index, batch) in ordered.iter().enumerate() {
        if batch.sweep_id != sweep_id {
            return Err(BatchError::Invalid(
                "batch belongs to another sweep".to_string(),
            ));
        }
        if batch.batch_count != batch_count || batch.batch_index != expected_index {
            return Err(BatchError::Invalid(
                "batch indices are not complete and ordered".to_string(),
            ));
        }
        batch.verify()?;
        if batch.range_start != cursor {
            return Err(BatchError::Invalid(
                "batch ranges have a gap or overlap".to_string(),
            ));
        }
        let end = cursor
            .checked_add(batch.expected_ids.len())
            .ok_or_else(|| BatchError::Invalid("batch range overflow".to_string()))?;
        if end > expected_ids.len() || batch.expected_ids != expected_ids[cursor..end] {
            return Err(BatchError::Invalid(
                "batch does not cover its declared global range".to_string(),
            ));
        }
        all_items.extend(batch.items.clone());
        cursor = end;
    }
    if cursor != expected_ids.len() {
        return Err(BatchError::Invalid("batch union is incomplete".to_string()));
    }
    Ok(batch_union_digest(&all_items))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(text: &str) -> Digest {
        sha256(text.as_bytes())
    }

    fn action() -> Action {
        Action::from_definition("rust-oracle", "1.2.3", b"emit-v1", digest("impl"))
    }

    fn manifest() -> SemanticInputManifest {
        SemanticInputManifest::new([
            ManifestEntry::new("source", digest("source")),
            ManifestEntry::new("config", digest("config")),
        ])
    }

    fn key(
        action: &Action,
        manifest: &SemanticInputManifest,
        dependencies: &[DependencyOutput],
        baseline: Option<Digest>,
    ) -> ReceiptKey {
        ReceiptKey::new("receipt-schema/1", action, manifest, dependencies, baseline)
    }

    fn test_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "new-ci-{label}-{}-{}",
            std::process::id(),
            unique_file_id()
        ));
        fs::create_dir_all(&path).expect("test directory");
        path
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256(b"abc").to_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn receipt_key_is_stable_and_each_semantic_field_invalidates() {
        let dependencies = [
            DependencyOutput::core("upstream-core", digest("core")),
            DependencyOutput::envelope("upstream-envelope", digest("envelope")),
        ];
        let baseline = Some(digest("baseline"));
        let original = key(&action(), &manifest(), &dependencies, baseline);
        let reordered_manifest = SemanticInputManifest::new([
            ManifestEntry::new("config", digest("config")),
            ManifestEntry::new("source", digest("source")),
        ]);
        let reordered_dependencies = [dependencies[1].clone(), dependencies[0].clone()];
        assert_eq!(
            original,
            key(
                &action(),
                &reordered_manifest,
                &reordered_dependencies,
                baseline
            )
        );

        let mut changed = action();
        changed.tool = "different-oracle".to_string();
        assert_ne!(
            original,
            key(&changed, &manifest(), &dependencies, baseline)
        );
        let mut changed = action();
        changed.version = "1.2.4".to_string();
        assert_ne!(
            original,
            key(&changed, &manifest(), &dependencies, baseline)
        );
        let mut changed = action();
        changed.definition_digest = digest("different-definition");
        assert_ne!(
            original,
            key(&changed, &manifest(), &dependencies, baseline)
        );
        let mut changed = action();
        changed.implementation_digest = digest("different-implementation");
        assert_ne!(
            original,
            key(&changed, &manifest(), &dependencies, baseline)
        );
        let mut changed_manifest = manifest();
        changed_manifest.entries[0].digest = digest("different-source");
        assert_ne!(
            original,
            key(&action(), &changed_manifest, &dependencies, baseline)
        );
        let mut changed_manifest = manifest();
        changed_manifest.entries[0].label = "different-source-label".to_string();
        assert_ne!(
            original,
            key(&action(), &changed_manifest, &dependencies, baseline)
        );
        let mut changed_dependencies = dependencies.to_vec();
        changed_dependencies[0].label = "renamed-edge".to_string();
        assert_ne!(
            original,
            key(&action(), &manifest(), &changed_dependencies, baseline)
        );
        let mut changed_dependencies = dependencies.to_vec();
        changed_dependencies[0].projection = Projection::Envelope;
        assert_ne!(
            original,
            key(&action(), &manifest(), &changed_dependencies, baseline)
        );
        let mut changed_dependencies = dependencies.to_vec();
        changed_dependencies[0].digest = digest("different-dependency");
        assert_ne!(
            original,
            key(&action(), &manifest(), &changed_dependencies, baseline)
        );
        assert_ne!(
            original,
            key(
                &action(),
                &manifest(),
                &dependencies,
                Some(digest("different-baseline"))
            )
        );
        assert_ne!(original, key(&action(), &manifest(), &dependencies, None));
        assert_ne!(
            original,
            ReceiptKey::new(
                "receipt-schema/2",
                &action(),
                &manifest(),
                &dependencies,
                baseline
            )
        );
    }

    #[test]
    fn core_and_envelope_dependencies_have_independent_invalidation() {
        let producer_a = ReceiptOutputs::new(digest("core-a"), digest("envelope-a"));
        let producer_b = ReceiptOutputs::new(digest("core-a"), digest("envelope-b"));
        let core_a = [DependencyOutput::core("dependency", producer_a.core)];
        let core_b = [DependencyOutput::core("dependency", producer_b.core)];
        let envelope_a = [DependencyOutput::envelope(
            "dependency",
            producer_a.envelope,
        )];
        let envelope_b = [DependencyOutput::envelope(
            "dependency",
            producer_b.envelope,
        )];
        assert_eq!(
            key(&action(), &manifest(), &core_a, None),
            key(&action(), &manifest(), &core_b, None)
        );
        assert_ne!(
            key(&action(), &manifest(), &envelope_a, None),
            key(&action(), &manifest(), &envelope_b, None)
        );
    }

    #[test]
    fn execution_only_fields_are_absent_from_key() {
        let receipt_key = key(&action(), &manifest(), &[], None);
        let first_execution = ExecutionMetadata {
            shard_count: 1,
            priority: 10,
            workspace_path: "/worker/a".to_string(),
            host_fingerprint: Some(digest("host-a")),
            ..ExecutionMetadata::default()
        };
        let mut second_execution = first_execution.clone();
        second_execution.shard_count = 17;
        second_execution.priority = -4;
        second_execution.workspace_path = "/worker/b".to_string();
        second_execution.host_fingerprint = Some(digest("host-b"));
        let first = NodeReceipt::success(
            receipt_key,
            ReceiptOutputs::new(digest("core"), digest("env")),
        )
        .with_execution(first_execution);
        let second = NodeReceipt::success(
            receipt_key,
            ReceiptOutputs::new(digest("core"), digest("env")),
        )
        .with_execution(second_execution);
        assert_eq!(first.key, second.key);
        assert_ne!(first.execution, second.execution);
    }

    #[test]
    fn torn_receipt_is_skipped_without_a_false_hit() {
        let directory = test_directory("torn");
        let store = ReceiptStore::open(&directory).expect("store");
        let receipt_key = key(&action(), &manifest(), &[], None);
        let valid = NodeReceipt::success(
            receipt_key,
            ReceiptOutputs::new(digest("core"), digest("env")),
        );
        let path = directory.join(format!("{}.torn.receipt", receipt_key.to_hex()));
        fs::write(&path, b"NCIR\0\0\0").expect("torn record");
        assert!(store.load(receipt_key).expect("load torn").is_empty());
        store.mint(&valid).expect("mint valid");
        let loaded = store.load(receipt_key).expect("load valid");
        assert_eq!(loaded, vec![valid]);
        let versions = [
            NodeReceipt::failed(receipt_key, Some(2), "first attempt"),
            NodeReceipt::cancelled(receipt_key, "killed"),
            NodeReceipt::timed_out(receipt_key, 42),
            NodeReceipt::diagnostic(receipt_key, "warning", "recorded for diagnosis"),
        ];
        for version in &versions {
            store.mint(version).expect("mint version");
        }
        assert_eq!(store.load(receipt_key).expect("load versions").len(), 5);
    }

    #[test]
    fn batch_union_proves_completeness_order_and_duplicate_freedom() {
        let expected = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ];
        let first = BatchReceipt::new(
            "sweep",
            0,
            2,
            0,
            expected[..2].to_vec(),
            vec![
                BatchItem::new("a", digest("a")),
                BatchItem::new("b", digest("b")),
            ],
        )
        .expect("first batch");
        let second = BatchReceipt::new(
            "sweep",
            1,
            2,
            2,
            expected[2..].to_vec(),
            vec![
                BatchItem::new("c", digest("c")),
                BatchItem::new("d", digest("d")),
            ],
        )
        .expect("second batch");
        let union = verify_batch_union("sweep", &expected, &[second.clone(), first.clone()])
            .expect("union");
        assert_eq!(
            union,
            batch_union_digest(&[first.items.clone(), second.items.clone()].concat())
        );

        let incomplete = BatchReceipt::from_parts(
            "sweep",
            1,
            2,
            2,
            expected[2..].to_vec(),
            vec![BatchItem::new("c", digest("c"))],
            digest("wrong"),
            false,
        );
        assert_eq!(incomplete.verify(), Err(BatchError::Incomplete));
        assert!(verify_batch_union("sweep", &expected, &[first.clone(), incomplete]).is_err());

        let out_of_order = BatchReceipt::new(
            "sweep",
            0,
            2,
            0,
            expected[..2].to_vec(),
            vec![
                BatchItem::new("b", digest("b")),
                BatchItem::new("a", digest("a")),
            ],
        );
        assert!(matches!(out_of_order, Err(BatchError::Order { .. })));

        let duplicate = vec!["a".to_string(), "a".to_string()];
        assert!(verify_batch_union("sweep", &duplicate, &[]).is_err());
    }

    #[test]
    fn malformed_status_record_cannot_be_loaded() {
        let directory = test_directory("malformed");
        let store = ReceiptStore::open(&directory).expect("store");
        let receipt_key = key(&action(), &manifest(), &[], None);
        let path = directory.join(format!("{}.malformed.receipt", receipt_key.to_hex()));
        fs::write(&path, b"NCIR\0\0\0\x01\0\0\0\0\0\0\0\0").expect("malformed record");
        assert!(store.load(receipt_key).expect("load malformed").is_empty());
    }

    #[test]
    fn key_input_validation_is_explicit() {
        let empty_manifest = SemanticInputManifest::new([ManifestEntry::new("", digest("x"))]);
        assert_eq!(
            ReceiptKey::try_new("schema", &action(), &empty_manifest, &[], None),
            Err(KeyError::EmptyInputLabel)
        );
        let duplicate_manifest = SemanticInputManifest::new([
            ManifestEntry::new("x", digest("a")),
            ManifestEntry::new("x", digest("b")),
        ]);
        assert_eq!(
            ReceiptKey::try_new("schema", &action(), &duplicate_manifest, &[], None),
            Err(KeyError::DuplicateInputLabel("x".to_string()))
        );
    }

    #[test]
    fn ordering_is_not_accidentally_based_on_digest_only() {
        let left = DependencyOutput::core("same", digest("a"));
        let right = DependencyOutput::envelope("same", digest("a"));
        assert_eq!(left.label.cmp(&right.label), std::cmp::Ordering::Equal);
        assert_ne!(left.projection, right.projection);
    }
}
