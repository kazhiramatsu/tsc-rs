//! Closed, canonical schema for a replayable generated case.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::{FoundationError, FoundationResult};

pub const CASE_SPEC_SCHEMA: u32 = 1;

/// A cross-language-safe `u64`.
///
/// JSON numbers are deliberately forbidden: the M8 root seed is greater
/// than JavaScript's largest exactly representable integer.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CanonicalU64(u64);

impl CanonicalU64 {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for CanonicalU64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl FromStr for CanonicalU64 {
    type Err = FoundationError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if !is_canonical_unsigned_decimal(text) {
            return Err(FoundationError::new(format!(
                "{text:?} is not a canonical unsigned decimal string"
            )));
        }
        text.parse::<u64>()
            .map(Self)
            .map_err(|_| FoundationError::new(format!("{text:?} does not fit in u64")))
    }
}

impl Serialize for CanonicalU64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CanonicalU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DecimalVisitor;

        impl Visitor<'_> for DecimalVisitor {
            type Value = CanonicalU64;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a canonical u64 decimal string")
            }

            fn visit_str<E>(self, text: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                CanonicalU64::from_str(text).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(DecimalVisitor)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CanonicalI64(i64);

impl CanonicalI64 {
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for CanonicalI64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl FromStr for CanonicalI64 {
    type Err = FoundationError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let digits = text.strip_prefix('-').unwrap_or(text);
        if !is_canonical_unsigned_decimal(digits) || text == "-0" || text.starts_with('+') {
            return Err(FoundationError::new(format!(
                "{text:?} is not a canonical signed decimal string"
            )));
        }
        text.parse::<i64>()
            .map(Self)
            .map_err(|_| FoundationError::new(format!("{text:?} does not fit in i64")))
    }
}

impl Serialize for CanonicalI64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CanonicalI64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DecimalVisitor;

        impl Visitor<'_> for DecimalVisitor {
            type Value = CanonicalI64;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a canonical i64 decimal string")
            }

            fn visit_str<E>(self, text: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                CanonicalI64::from_str(text).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(DecimalVisitor)
    }
}

fn is_canonical_unsigned_decimal(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseProvenance {
    pub root_seed: CanonicalU64,
    pub case_index: CanonicalU64,
    pub case_seed: CanonicalU64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DecisionValue {
    Boolean { value: bool },
    Unsigned { value: CanonicalU64 },
    Signed { value: CanonicalI64 },
    Choice { value: String },
    Identifier { value: String },
    Bytes { value_base64: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StableDecision {
    pub ordinal: u32,
    pub id: String,
    pub value: DecisionValue,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DomainMembership {
    pub ordinal: u32,
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StableValue {
    Boolean { value: bool },
    Unsigned { value: CanonicalU64 },
    Signed { value: CanonicalI64 },
    Text { value: String },
    StringList { values: Vec<String> },
}

/// Closed projection of the values accepted by `ProgramJson.options`.
///
/// Keeping this separate from generator decision values prevents a replay
/// record from representing an option value that neither engine adapter can
/// consume.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CompilerOptionValue {
    Boolean { value: bool },
    Number { value: i32 },
    Text { value: String },
    StringList { values: Vec<String> },
    Null,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrderedSetting {
    pub ordinal: u32,
    pub name: String,
    pub value: CompilerOptionValue,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EncodedFile {
    pub ordinal: u32,
    /// Exact public file name supplied to the compiler. Relative names stay
    /// relative; the cwd is a separate ordered replay fact.
    pub name: String,
    pub text_base64: String,
}

impl EncodedFile {
    pub fn decoded_text(&self) -> FoundationResult<String> {
        let bytes = decode_canonical_base64(&self.text_base64)?;
        String::from_utf8(bytes).map_err(|error| {
            FoundationError::new(format!("{} is not UTF-8 source text: {error}", self.name))
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixEntry {
    pub ordinal: u32,
    pub id: String,
    pub value: StableValue,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrderedArgument {
    pub ordinal: u32,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NodeProcessPolicy {
    pub executable_id: String,
    pub arguments: Vec<OrderedArgument>,
    pub single_threaded: bool,
    pub deadline_ms: CanonicalU64,
    pub rollover_cases: CanonicalU64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RustProcessPolicy {
    pub worker_cap: u32,
    pub deadline_ms: CanonicalU64,
    pub rollover_cases: CanonicalU64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChildProcessPolicy {
    pub policy_id: String,
    pub cases_per_child: CanonicalU64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessPolicy {
    pub schema: u32,
    pub oracle_node: NodeProcessPolicy,
    pub tsrs: RustProcessPolicy,
    pub child: ChildProcessPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseSpec {
    pub schema: u32,
    pub case_id: String,
    pub generator_id: String,
    pub provenance: CaseProvenance,
    pub decisions: Vec<StableDecision>,
    pub domain_membership: Vec<DomainMembership>,
    pub cwd: String,
    pub options: Vec<OrderedSetting>,
    pub libs: Vec<EncodedFile>,
    pub files: Vec<EncodedFile>,
    /// Exact `ProgramJson.matrixKey` selected by expansion.
    pub matrix_key: String,
    /// Ordered, typed generator-side explanation of the matrix key.
    pub matrix: Vec<MatrixEntry>,
    pub normalization_schema: u32,
    pub process_policy: ProcessPolicy,
}

/// A source decoded and indexed while its owning [`CaseSpec`] is validated.
///
/// This is intentionally crate-private. Public artifacts continue to contain
/// only canonical schema data; the index is an execution-local acceleration
/// structure shared by both engine observations.
#[derive(Clone, Debug)]
pub(crate) struct ValidatedSource {
    text: String,
    total_utf16: u32,
    line_starts: Vec<u32>,
    surrogate_interiors: Vec<u32>,
}

impl ValidatedSource {
    fn new(text: String) -> FoundationResult<Self> {
        let mut total_utf16 = 0_u32;
        let mut line_starts = vec![0];
        let mut surrogate_interiors = Vec::new();
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            let width =
                u32::try_from(character.len_utf16()).expect("UTF-16 character width fits u32");
            if width == 2 {
                surrogate_interiors.push(
                    total_utf16.checked_add(1).ok_or_else(|| {
                        FoundationError::new("source UTF-16 length overflows u32")
                    })?,
                );
            }
            total_utf16 = total_utf16
                .checked_add(width)
                .ok_or_else(|| FoundationError::new("source UTF-16 length overflows u32"))?;

            if character == '\r' {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                    total_utf16 = total_utf16.checked_add(1).ok_or_else(|| {
                        FoundationError::new("source UTF-16 length overflows u32")
                    })?;
                }
                line_starts.push(total_utf16);
            } else if matches!(character, '\n' | '\u{2028}' | '\u{2029}') {
                line_starts.push(total_utf16);
            }
        }
        Ok(Self {
            text,
            total_utf16,
            line_starts,
            surrogate_interiors,
        })
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn ensure_utf16_boundary(
        &self,
        requested: u32,
        context: &str,
    ) -> FoundationResult<()> {
        if requested > self.total_utf16
            || self.surrogate_interiors.binary_search(&requested).is_ok()
        {
            return Err(FoundationError::new(format!(
                "{context} {requested} is outside the source or splits a UTF-16 surrogate pair"
            )));
        }
        Ok(())
    }

    pub(crate) fn line_column_at_utf16(
        &self,
        requested: u32,
        context: &str,
    ) -> FoundationResult<(u32, u32)> {
        self.ensure_utf16_boundary(requested, context)?;
        let line_index = self
            .line_starts
            .partition_point(|start| *start <= requested)
            - 1;
        let line = u32::try_from(line_index)
            .map_err(|_| FoundationError::new("source line count overflows u32"))?;
        Ok((line, requested - self.line_starts[line_index]))
    }

    pub(crate) const fn total_utf16(&self) -> u32 {
        self.total_utf16
    }
}

/// A single validation/decode/index pass over a case.
#[derive(Debug)]
pub(crate) struct ValidatedCaseContext<'case> {
    case: &'case CaseSpec,
    sources: BTreeMap<String, ValidatedSource>,
}

impl<'case> ValidatedCaseContext<'case> {
    pub(crate) const fn case(&self) -> &'case CaseSpec {
        self.case
    }

    pub(crate) fn source(&self, path: &str) -> FoundationResult<&ValidatedSource> {
        self.sources.get(path).ok_or_else(|| {
            FoundationError::new(format!(
                "virtual diagnostic path {path:?} is absent from CaseSpec libs/files"
            ))
        })
    }
}

impl CaseSpec {
    pub fn validate(&self) -> FoundationResult<()> {
        self.validate_with_decoded_sources(|_, _| Ok(()))
    }

    pub(crate) fn validated_context(&self) -> FoundationResult<ValidatedCaseContext<'_>> {
        let mut sources = BTreeMap::new();
        self.validate_with_decoded_sources(|file, text| {
            let source = ValidatedSource::new(text)?;
            sources.insert(file.name.clone(), source);
            Ok(())
        })?;
        Ok(ValidatedCaseContext {
            case: self,
            sources,
        })
    }

    fn validate_with_decoded_sources(
        &self,
        mut consume_source: impl FnMut(&EncodedFile, String) -> FoundationResult<()>,
    ) -> FoundationResult<()> {
        if self.schema != CASE_SPEC_SCHEMA {
            return Err(FoundationError::new(format!(
                "unsupported case schema {}; expected {CASE_SPEC_SCHEMA}",
                self.schema
            )));
        }
        validate_id(&self.case_id, "case_id")?;
        validate_id(&self.generator_id, "generator_id")?;
        if self.matrix_key.chars().any(char::is_control) {
            return Err(FoundationError::new(
                "matrix_key must not contain control characters",
            ));
        }
        validate_virtual_path(&self.cwd, "cwd")?;
        validate_nonempty(&self.decisions, "decisions")?;
        validate_nonempty(&self.domain_membership, "domain_membership")?;
        validate_nonempty(&self.files, "files")?;

        validate_ordered_ids(
            self.decisions
                .iter()
                .map(|decision| (decision.ordinal, decision.id.as_str())),
            "decisions",
        )?;
        let mut generator_identifiers = BTreeSet::new();
        for decision in &self.decisions {
            validate_decision_value(&decision.value, &decision.id)?;
            if let DecisionValue::Identifier { value } = &decision.value {
                if !generator_identifiers.insert(value.as_str()) {
                    return Err(FoundationError::new(format!(
                        "generator identifier {value:?} is duplicated across decisions"
                    )));
                }
            }
        }
        validate_ordered_ids(
            self.domain_membership
                .iter()
                .map(|membership| (membership.ordinal, membership.id.as_str())),
            "domain_membership",
        )?;
        validate_utf8_sorted_unique(
            self.domain_membership
                .iter()
                .map(|membership| membership.id.as_str()),
            "domain_membership",
        )?;
        validate_ordered_ids(
            self.options
                .iter()
                .map(|option| (option.ordinal, option.name.as_str())),
            "options",
        )?;
        validate_utf8_sorted_unique(
            self.options.iter().map(|option| option.name.as_str()),
            "options",
        )?;
        for option in &self.options {
            validate_compiler_option(&option.value, &format!("option {}", option.name))?;
        }
        validate_ordered_ids(
            self.matrix
                .iter()
                .map(|entry| (entry.ordinal, entry.id.as_str())),
            "matrix",
        )?;
        for entry in &self.matrix {
            validate_stable_value(&entry.value, &format!("matrix {}", entry.id))?;
        }
        if self.normalization_schema != 1 {
            return Err(FoundationError::new(format!(
                "unsupported CaseSpec normalization_schema {}; expected 1",
                self.normalization_schema
            )));
        }
        self.process_policy.validate()?;

        validate_files_structure(&self.libs, "libs")?;
        validate_files_structure(&self.files, "files")?;
        let mut all_paths = BTreeSet::new();
        let mut all_resolved_paths = BTreeSet::new();
        let mut normalization_path_sources = BTreeSet::new();
        if self.cwd != "/" {
            normalization_path_sources.insert(self.cwd.clone());
        }
        for file in self.libs.iter().chain(&self.files) {
            if !all_paths.insert(file.name.as_str()) {
                return Err(FoundationError::new(format!(
                    "file name {:?} is duplicated across libs/files",
                    file.name
                )));
            }
            let resolved = self.resolved_file_name(&file.name)?;
            if resolved == self.cwd {
                return Err(FoundationError::new(format!(
                    "file name {:?} resolves to cwd {:?}; schema-1 normalization ownership would be ambiguous",
                    file.name, self.cwd
                )));
            }
            if !all_resolved_paths.insert(resolved.clone()) {
                return Err(FoundationError::new(format!(
                    "file name {:?} resolves to duplicate source path {resolved:?} across libs/files",
                    file.name
                )));
            }
            normalization_path_sources.insert(file.name.clone());
            normalization_path_sources.insert(resolved);
            consume_source(file, file.decoded_text()?)?;
        }
        for identifier in generator_identifiers {
            if normalization_path_sources.contains(identifier) {
                return Err(FoundationError::new(format!(
                    "generator identifier {identifier:?} is also an owned path source"
                )));
            }
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> FoundationResult<Vec<u8>> {
        self.validate()?;
        self.canonical_bytes_after_validation()
    }

    pub(crate) fn canonical_bytes_after_validation(&self) -> FoundationResult<Vec<u8>> {
        serde_json::to_vec(self)
            .map_err(|error| FoundationError::new(format!("cannot serialize CaseSpec: {error}")))
    }

    pub fn canonical_sha256(&self) -> FoundationResult<String> {
        Ok(sha256_hex(&self.canonical_bytes()?))
    }

    pub fn from_json_slice(bytes: &[u8]) -> FoundationResult<Self> {
        let case: Self = serde_json::from_slice(bytes)
            .map_err(|error| FoundationError::new(format!("invalid CaseSpec JSON: {error}")))?;
        case.validate()?;
        Ok(case)
    }

    pub fn from_canonical_slice(bytes: &[u8]) -> FoundationResult<Self> {
        let case: Self = serde_json::from_slice(bytes)
            .map_err(|error| FoundationError::new(format!("invalid CaseSpec JSON: {error}")))?;
        case.validate()?;
        if case.canonical_bytes_after_validation()? != bytes {
            return Err(FoundationError::new(
                "CaseSpec input is valid JSON but not canonical compact schema-1 bytes",
            ));
        }
        Ok(case)
    }

    pub fn source_text(&self, path: &str) -> FoundationResult<String> {
        let context = self.validated_context()?;
        Ok(context.source(path)?.text().to_owned())
    }

    pub fn resolved_file_name(&self, name: &str) -> FoundationResult<String> {
        validate_public_file_name(name, "public file name")?;
        if name.starts_with('/') {
            return Ok(name.to_owned());
        }
        if self.cwd == "/" {
            Ok(format!("/{name}"))
        } else {
            Ok(format!("{}/{name}", self.cwd))
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn validate_id(value: &str, context: &str) -> FoundationResult<()> {
    if value.is_empty() || value.trim() != value {
        return Err(FoundationError::new(format!(
            "{context} must be a non-empty, unpadded identifier"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(FoundationError::new(format!(
            "{context} must not contain control characters"
        )));
    }
    Ok(())
}

pub(crate) fn validate_virtual_path(path: &str, context: &str) -> FoundationResult<()> {
    if !path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return Err(FoundationError::new(format!(
            "{context} must be an absolute virtual POSIX path: {path:?}"
        )));
    }
    if path.len() > 1 && path.ends_with('/') {
        return Err(FoundationError::new(format!(
            "{context} must not have a trailing slash: {path:?}"
        )));
    }
    if path.chars().any(char::is_control) {
        return Err(FoundationError::new(format!(
            "{context} contains a control character"
        )));
    }
    if path == "/" {
        return Ok(());
    }
    for component in path.split('/').skip(1) {
        if component.is_empty() || component == "." || component == ".." {
            return Err(FoundationError::new(format!(
                "{context} is not a canonical virtual path: {path:?}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_public_file_name(name: &str, context: &str) -> FoundationResult<()> {
    if name.is_empty()
        || name.contains('\\')
        || name.contains('\0')
        || name.chars().any(char::is_control)
        || name.ends_with('/')
    {
        return Err(FoundationError::new(format!(
            "{context} must be a non-empty canonical POSIX public file name: {name:?}"
        )));
    }
    let components = name.split('/').skip(usize::from(name.starts_with('/')));
    for component in components {
        if component.is_empty() || component == "." || component == ".." {
            return Err(FoundationError::new(format!(
                "{context} is not a canonical public file name: {name:?}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn normalization_placeholder_len_at(input: &str, offset: usize) -> Option<usize> {
    let bytes = input.get(offset..)?.as_bytes();
    if bytes.len() < 5 || bytes[0] != b'<' {
        return None;
    }
    match bytes[1] {
        b'@' if bytes.get(2..5) == Some(b"0@>") => Some(5),
        b'@' if matches!(bytes.get(2), Some(b'1' | b'2')) && bytes.get(3) == Some(&b':') => {
            let end = canonical_u32_end(bytes, 4)?;
            (bytes.get(end) == Some(&b'@') && bytes.get(end + 1) == Some(&b'>')).then_some(end + 2)
        }
        b'#' => {
            let end = canonical_u32_end(bytes, 2)?;
            (bytes.get(end) == Some(&b'#') && bytes.get(end + 1) == Some(&b'>')).then_some(end + 2)
        }
        b'%' if matches!(bytes.get(2), Some(b'0' | b'1' | b'2'))
            && bytes.get(3) == Some(&b'%')
            && bytes.get(4) == Some(&b'>') =>
        {
            Some(5)
        }
        _ => None,
    }
}

fn canonical_u32_end(bytes: &[u8], start: usize) -> Option<usize> {
    let first = *bytes.get(start)?;
    if !first.is_ascii_digit() {
        return None;
    }
    if first == b'0' {
        return Some(start + 1);
    }
    let mut end = start + 1;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    std::str::from_utf8(&bytes[start..end])
        .ok()?
        .parse::<u32>()
        .ok()?;
    Some(end)
}

fn validate_nonempty<T>(values: &[T], context: &str) -> FoundationResult<()> {
    if values.is_empty() {
        return Err(FoundationError::new(format!("{context} must not be empty")));
    }
    Ok(())
}

fn validate_ordered_ids<'a>(
    values: impl Iterator<Item = (u32, &'a str)>,
    context: &str,
) -> FoundationResult<()> {
    let mut ids = BTreeSet::new();
    for (index, (ordinal, id)) in values.enumerate() {
        if usize::try_from(ordinal).ok() != Some(index) {
            return Err(FoundationError::new(format!(
                "{context}[{index}].ordinal must be {index}, found {ordinal}"
            )));
        }
        validate_id(id, &format!("{context}[{index}].id"))?;
        if !ids.insert(id) {
            return Err(FoundationError::new(format!(
                "{context} contains duplicate id {id:?}"
            )));
        }
    }
    Ok(())
}

fn validate_utf8_sorted_unique<'a>(
    values: impl Iterator<Item = &'a str>,
    context: &str,
) -> FoundationResult<()> {
    let mut previous: Option<&str> = None;
    for value in values {
        if previous.is_some_and(|previous| previous.as_bytes() >= value.as_bytes()) {
            return Err(FoundationError::new(format!(
                "{context} must be strictly sorted by UTF-8 bytes; {value:?} is out of order"
            )));
        }
        previous = Some(value);
    }
    Ok(())
}

fn validate_decision_value(value: &DecisionValue, context: &str) -> FoundationResult<()> {
    match value {
        DecisionValue::Choice { value } => validate_id(value, context),
        DecisionValue::Identifier { value } => {
            if !is_ascii_identifier(value) {
                return Err(FoundationError::new(format!(
                    "{context} generator identifier must be an ASCII identifier"
                )));
            }
            Ok(())
        }
        DecisionValue::Bytes { value_base64 } => decode_canonical_base64(value_base64).map(|_| ()),
        DecisionValue::Boolean { .. }
        | DecisionValue::Unsigned { .. }
        | DecisionValue::Signed { .. } => Ok(()),
    }
}

fn validate_stable_value(value: &StableValue, context: &str) -> FoundationResult<()> {
    match value {
        StableValue::Text { value } => {
            if value.chars().any(char::is_control) {
                return Err(FoundationError::new(format!(
                    "{context} text contains a control character"
                )));
            }
            Ok(())
        }
        StableValue::StringList { values } => {
            for (index, value) in values.iter().enumerate() {
                validate_id(value, &format!("{context}[{index}]"))?;
            }
            Ok(())
        }
        StableValue::Boolean { .. } | StableValue::Unsigned { .. } | StableValue::Signed { .. } => {
            Ok(())
        }
    }
}

fn validate_compiler_option(value: &CompilerOptionValue, context: &str) -> FoundationResult<()> {
    match value {
        CompilerOptionValue::Text { value } => {
            if value.chars().any(char::is_control) {
                return Err(FoundationError::new(format!(
                    "{context} text contains a control character"
                )));
            }
            Ok(())
        }
        CompilerOptionValue::StringList { values } => {
            for (index, value) in values.iter().enumerate() {
                if value.chars().any(char::is_control) {
                    return Err(FoundationError::new(format!(
                        "{context}[{index}] contains a control character"
                    )));
                }
            }
            Ok(())
        }
        CompilerOptionValue::Boolean { .. }
        | CompilerOptionValue::Number { .. }
        | CompilerOptionValue::Null => Ok(()),
    }
}

impl ProcessPolicy {
    fn validate(&self) -> FoundationResult<()> {
        if self.schema != 1 {
            return Err(FoundationError::new(format!(
                "unsupported process policy schema {}; expected 1",
                self.schema
            )));
        }
        validate_id(
            &self.oracle_node.executable_id,
            "process_policy.oracle_node.executable_id",
        )?;
        validate_ordered_values(
            self.oracle_node
                .arguments
                .iter()
                .map(|argument| (argument.ordinal, argument.value.as_str())),
            "process_policy.oracle_node.arguments",
        )?;
        let node_arguments = self
            .oracle_node
            .arguments
            .iter()
            .map(|argument| argument.value.as_str())
            .collect::<Vec<_>>();
        if !self.oracle_node.single_threaded
            || !node_arguments.contains(&"--single-threaded")
            || node_arguments
                .iter()
                .any(|argument| argument.starts_with("--no-single-threaded"))
        {
            return Err(FoundationError::new(
                "Node single-thread policy must be derived from an exact --single-threaded launch argument with no negating argument",
            ));
        }
        if self.oracle_node.deadline_ms.get() == 0 || self.oracle_node.rollover_cases.get() == 0 {
            return Err(FoundationError::new(
                "Node deadline and rollover_cases must be positive",
            ));
        }
        if self.tsrs.worker_cap == 0
            || self.tsrs.deadline_ms.get() == 0
            || self.tsrs.rollover_cases.get() == 0
        {
            return Err(FoundationError::new(
                "Rust worker_cap, deadline, and rollover_cases must be positive",
            ));
        }
        validate_id(&self.child.policy_id, "process_policy.child.policy_id")?;
        if self.child.cases_per_child.get() == 0 {
            return Err(FoundationError::new(
                "process_policy.child.cases_per_child must be positive",
            ));
        }
        Ok(())
    }
}

fn validate_ordered_values<'a>(
    values: impl Iterator<Item = (u32, &'a str)>,
    context: &str,
) -> FoundationResult<()> {
    for (index, (ordinal, value)) in values.enumerate() {
        if usize::try_from(ordinal).ok() != Some(index) {
            return Err(FoundationError::new(format!(
                "{context}[{index}].ordinal must be {index}, found {ordinal}"
            )));
        }
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(FoundationError::new(format!(
                "{context}[{index}].value must be non-empty and contain no control characters"
            )));
        }
    }
    Ok(())
}

fn is_ascii_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_' || first == b'$')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$')
}

fn validate_files_structure(files: &[EncodedFile], context: &str) -> FoundationResult<()> {
    let mut paths = BTreeSet::new();
    for (index, file) in files.iter().enumerate() {
        if usize::try_from(file.ordinal).ok() != Some(index) {
            return Err(FoundationError::new(format!(
                "{context}[{index}].ordinal must be {index}, found {}",
                file.ordinal
            )));
        }
        validate_public_file_name(&file.name, &format!("{context}[{index}].name"))?;
        if !paths.insert(file.name.as_str()) {
            return Err(FoundationError::new(format!(
                "{context} contains duplicate name {:?}",
                file.name
            )));
        }
    }
    Ok(())
}

fn decode_canonical_base64(text: &str) -> FoundationResult<Vec<u8>> {
    if !text.len().is_multiple_of(4) {
        return Err(FoundationError::new("base64 length must be divisible by 4"));
    }
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(text.len() / 4 * 3);
    for (chunk_index, chunk) in bytes.chunks_exact(4).enumerate() {
        let is_last = chunk_index + 1 == bytes.len() / 4;
        let padding = match (chunk[2], chunk[3]) {
            (b'=', b'=') => 2,
            (_, b'=') => 1,
            (b'=', _) => {
                return Err(FoundationError::new(
                    "base64 padding may appear only at the end",
                ))
            }
            _ => 0,
        };
        if padding > 0 && !is_last {
            return Err(FoundationError::new(
                "base64 padding may appear only in the final quartet",
            ));
        }
        let a = base64_value(chunk[0])?;
        let b = base64_value(chunk[1])?;
        let c = if padding == 2 {
            0
        } else {
            base64_value(chunk[2])?
        };
        let d = if padding > 0 {
            0
        } else {
            base64_value(chunk[3])?
        };
        if (padding == 2 && b & 0x0f != 0) || (padding == 1 && c & 0x03 != 0) {
            return Err(FoundationError::new(
                "base64 contains non-zero trailing padding bits",
            ));
        }
        output.push((a << 2) | (b >> 4));
        if padding < 2 {
            output.push((b << 4) | (c >> 2));
        }
        if padding == 0 {
            output.push((c << 6) | d);
        }
    }
    Ok(output)
}

fn base64_value(byte: u8) -> FoundationResult<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(FoundationError::new(format!(
            "invalid canonical base64 byte {byte:?}"
        ))),
    }
}
