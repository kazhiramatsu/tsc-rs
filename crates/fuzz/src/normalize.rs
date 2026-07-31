//! Explicit schema-1 class normalization.
//!
//! This is intentionally a bounded raw-to-normalized algorithm: line endings,
//! reviewed virtual-path literals, and ASCII generator identifier tokens. It
//! never strips arbitrary numbers, applies a regex, or performs Unicode
//! folding. Literal `<` is encoded as `<<`; therefore a single `<...>` marker
//! can only have been injected as a typed schema placeholder. Normalized text
//! is not accepted as raw input a second time.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::schema::{
    normalization_placeholder_len_at, validate_public_file_name, CaseSpec, DecisionValue,
};
use crate::{FoundationError, FoundationResult};

pub const NORMALIZATION_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct LiteralMapping {
    ordinal: u32,
    from: String,
    to: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NormalizationSpec {
    schema: u32,
    virtual_paths: Vec<LiteralMapping>,
    generator_identifiers: Vec<LiteralMapping>,
    terminal_numbers: Vec<LiteralMapping>,
}

impl NormalizationSpec {
    /// Derive the only accepted schema-1 mapping from exact CaseSpec facts.
    /// Placeholders are unique by role/ordinal so unrelated paths or
    /// identifiers can never be collapsed by caller policy.
    pub fn for_case(case: &CaseSpec) -> FoundationResult<Self> {
        let validated = case.validated_context()?;
        Self::for_validated_case(validated.case())
    }

    /// Construct from a case whose structural/source validation has already
    /// produced the shared execution context.
    pub(crate) fn for_validated_case(case: &CaseSpec) -> FoundationResult<Self> {
        let mut virtual_paths = Vec::with_capacity(case.libs.len() + case.files.len() + 1);
        if case.cwd != "/" {
            virtual_paths.push(LiteralMapping {
                ordinal: 0,
                from: case.cwd.clone(),
                to: "<@0@>".to_owned(),
            });
        }
        for (index, file) in case.libs.iter().enumerate() {
            push_path_mapping(
                &mut virtual_paths,
                file.name.clone(),
                format!("<@1:{index}@>"),
            )?;
            let resolved = case.resolved_file_name(&file.name)?;
            if resolved != file.name {
                push_path_mapping(&mut virtual_paths, resolved, format!("<@1:{index}@>"))?;
            }
        }
        for (index, file) in case.files.iter().enumerate() {
            push_path_mapping(
                &mut virtual_paths,
                file.name.clone(),
                format!("<@2:{index}@>"),
            )?;
            let resolved = case.resolved_file_name(&file.name)?;
            if resolved != file.name {
                push_path_mapping(&mut virtual_paths, resolved, format!("<@2:{index}@>"))?;
            }
        }

        let mut generator_identifiers = Vec::new();
        let mut sources = BTreeSet::new();
        for decision in &case.decisions {
            if let DecisionValue::Identifier { value } = &decision.value {
                if !sources.insert(value.as_str()) {
                    return Err(FoundationError::new(format!(
                        "generator identifier {:?} is owned by more than one stable decision",
                        value
                    )));
                }
                generator_identifiers.push(LiteralMapping {
                    ordinal: u32::try_from(generator_identifiers.len())
                        .map_err(|_| FoundationError::new("too many identifier mappings"))?,
                    from: value.clone(),
                    to: format!("<#{}#>", decision.ordinal),
                });
            }
        }
        let mut terminal_numbers = Vec::new();
        for (from, to) in [
            (case.provenance.root_seed.to_string(), "<%0%>"),
            (case.provenance.case_seed.to_string(), "<%1%>"),
        ] {
            if from.len() >= 6
                && terminal_numbers
                    .iter()
                    .all(|mapping: &LiteralMapping| mapping.from != from)
            {
                terminal_numbers.push(LiteralMapping {
                    ordinal: u32::try_from(terminal_numbers.len())
                        .map_err(|_| FoundationError::new("too many terminal number mappings"))?,
                    from,
                    to: to.to_owned(),
                });
            }
        }
        let spec = Self {
            schema: case.normalization_schema,
            virtual_paths,
            generator_identifiers,
            terminal_numbers,
        };
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> FoundationResult<()> {
        if self.schema != NORMALIZATION_SCHEMA {
            return Err(FoundationError::new(format!(
                "unsupported normalization schema {}; expected {NORMALIZATION_SCHEMA}",
                self.schema
            )));
        }
        validate_mapping_ordinals(&self.virtual_paths, "virtual_paths")?;
        validate_mapping_ordinals(&self.generator_identifiers, "generator_identifiers")?;
        validate_mapping_ordinals(&self.terminal_numbers, "terminal_numbers")?;

        for (index, mapping) in self.virtual_paths.iter().enumerate() {
            validate_public_file_name(&mapping.from, &format!("virtual_paths[{index}].from"))?;
            validate_replacement(&mapping.to, b'@', &format!("virtual_paths[{index}].to"))?;
        }
        for (index, mapping) in self.generator_identifiers.iter().enumerate() {
            if !is_ascii_identifier(&mapping.from) {
                return Err(FoundationError::new(format!(
                    "generator_identifiers[{index}].from must be a non-empty ASCII identifier"
                )));
            }
            validate_replacement(
                &mapping.to,
                b'#',
                &format!("generator_identifiers[{index}].to"),
            )?;
        }
        for (index, mapping) in self.terminal_numbers.iter().enumerate() {
            if mapping.from.is_empty() || !mapping.from.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(FoundationError::new(format!(
                    "terminal_numbers[{index}].from must be an unsigned decimal string"
                )));
            }
            validate_replacement(&mapping.to, b'%', &format!("terminal_numbers[{index}].to"))?;
        }

        for path in &self.virtual_paths {
            for identifier in &self.generator_identifiers {
                if path.from == identifier.from {
                    return Err(FoundationError::new(format!(
                        "normalization source {:?} is owned by both path and generated-identifier roles",
                        path.from
                    )));
                }
            }
        }
        Ok(())
    }

    /// Normalize one raw compiler string. The returned versioned encoding is
    /// deliberately one-way; feeding it back as raw text would escape its
    /// typed placeholders as literals.
    pub fn normalize(&self, input: &str) -> FoundationResult<String> {
        self.validate()?;
        self.normalize_after_validation(input)
    }

    pub(crate) fn normalize_after_validation(&self, input: &str) -> FoundationResult<String> {
        let lf = normalize_line_endings(input);
        encode_normalized_text(&lf, &self.virtual_paths, &self.generator_identifiers)
    }

    pub fn normalize_exact_path(&self, path: &str) -> FoundationResult<String> {
        self.validate()?;
        validate_public_file_name(path, "diagnostic path")?;
        Ok(self
            .virtual_paths
            .iter()
            .find(|mapping| mapping.from == path)
            .map_or_else(|| encode_literal(path), |mapping| mapping.to.clone()))
    }

    /// Renderer-stage path projection. It deliberately preserves line
    /// endings so `path` and `newline` remain independently classifiable.
    pub fn normalize_renderer_paths(&self, input: &str) -> FoundationResult<String> {
        self.validate()?;
        self.normalize_renderer_paths_after_validation(input)
    }

    pub(crate) fn normalize_renderer_paths_after_validation(
        &self,
        input: &str,
    ) -> FoundationResult<String> {
        encode_normalized_text(input, &self.virtual_paths, &[])
    }

    pub fn normalize_renderer_newlines(&self, input: &str) -> String {
        normalize_line_endings(input)
    }

    pub fn normalize_terminal(&self, input: &str) -> FoundationResult<String> {
        self.validate()?;
        self.normalize_terminal_after_validation(input)
    }

    pub(crate) fn normalize_terminal_after_validation(
        &self,
        input: &str,
    ) -> FoundationResult<String> {
        let normalized = self.normalize_after_validation(input)?;
        let numbers = replace_number_tokens(&normalized, &self.terminal_numbers)?;
        Ok(replace_hex_addresses(&numbers))
    }

    pub fn canonical_bytes(&self) -> FoundationResult<Vec<u8>> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|error| {
            FoundationError::new(format!("cannot serialize normalization spec: {error}"))
        })
    }

    pub fn schema(&self) -> u32 {
        self.schema
    }

    pub fn sha256(&self) -> FoundationResult<String> {
        Ok(crate::schema::sha256_hex(&self.canonical_bytes()?))
    }
}

fn push_path_mapping(
    mappings: &mut Vec<LiteralMapping>,
    from: String,
    to: String,
) -> FoundationResult<()> {
    if let Some(existing) = mappings.iter().find(|mapping| mapping.from == from) {
        if existing.to == to {
            return Ok(());
        }
        return Err(FoundationError::new(format!(
            "normalization source {from:?} is owned by both {:?} and {to:?}",
            existing.to
        )));
    }
    mappings.push(LiteralMapping {
        ordinal: u32::try_from(mappings.len())
            .map_err(|_| FoundationError::new("too many virtual path mappings"))?,
        from,
        to,
    });
    Ok(())
}

fn validate_mapping_ordinals(mappings: &[LiteralMapping], context: &str) -> FoundationResult<()> {
    let mut sources = BTreeSet::new();
    for (index, mapping) in mappings.iter().enumerate() {
        if usize::try_from(mapping.ordinal).ok() != Some(index) {
            return Err(FoundationError::new(format!(
                "{context}[{index}].ordinal must be {index}, found {}",
                mapping.ordinal
            )));
        }
        if mapping.from.is_empty() {
            return Err(FoundationError::new(format!(
                "{context}[{index}].from must not be empty"
            )));
        }
        if !sources.insert(mapping.from.as_str()) {
            return Err(FoundationError::new(format!(
                "{context} contains duplicate source {:?}",
                mapping.from
            )));
        }
    }
    Ok(())
}

fn validate_replacement(replacement: &str, delimiter: u8, context: &str) -> FoundationResult<()> {
    if replacement.as_bytes().get(1) != Some(&delimiter)
        || normalization_placeholder_len_at(replacement, 0) != Some(replacement.len())
    {
        return Err(FoundationError::new(format!(
            "{context} must be one canonical schema-1 placeholder"
        )));
    }
    Ok(())
}

fn normalize_line_endings(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            output.push('\n');
        } else {
            output.push(character);
        }
    }
    output
}

fn encode_normalized_text(
    input: &str,
    path_mappings: &[LiteralMapping],
    identifier_mappings: &[LiteralMapping],
) -> FoundationResult<String> {
    let mut output = String::with_capacity(input.len());
    let mut offset = 0;
    while offset < input.len() {
        let mut matched: Option<(&LiteralMapping, usize)> = None;
        for mapping in path_mappings {
            if let Some(length) = mapping_literal_len_at(input, offset, &mapping.from) {
                if matched.is_none_or(|(_, previous)| length > previous) {
                    matched = Some((mapping, length));
                }
            }
        }
        if let Some((mapping, length)) = matched {
            output.push_str(&mapping.to);
            offset += length;
            continue;
        }

        let mut identifier: Option<&LiteralMapping> = None;
        for mapping in identifier_mappings {
            if input[offset..].starts_with(&mapping.from)
                && is_identifier_boundary(input, offset, mapping.from.len())
            {
                if let Some(previous) = identifier {
                    return Err(FoundationError::new(format!(
                        "ambiguous identifier normalization at byte {offset}: {:?} and {:?}",
                        previous.from, mapping.from
                    )));
                }
                identifier = Some(mapping);
            }
        }
        if let Some(mapping) = identifier {
            output.push_str(&mapping.to);
            offset += mapping.from.len();
            continue;
        }

        let character = input[offset..]
            .chars()
            .next()
            .expect("offset stays on a character boundary");
        if character == '<' {
            output.push_str("<<");
        } else {
            output.push(character);
        }
        offset += character.len_utf8();
    }
    Ok(output)
}

fn mapping_literal_len_at(input: &str, offset: usize, source: &str) -> Option<usize> {
    if path_literal_matches(input, offset, source) {
        return Some(source.len());
    }
    if source.contains('/') && windows_path_literal_matches(input, offset, source) {
        return Some(source.len());
    }
    None
}

fn path_literal_matches(input: &str, offset: usize, needle: &str) -> bool {
    if !input[offset..].starts_with(needle) {
        return false;
    }
    path_literal_boundaries(
        input,
        offset,
        needle.len(),
        needle.starts_with('/') || needle.starts_with('\\'),
    )
}

fn windows_path_literal_matches(input: &str, offset: usize, source: &str) -> bool {
    let Some(end) = offset.checked_add(source.len()) else {
        return false;
    };
    let Some(candidate) = input.get(offset..end) else {
        return false;
    };
    if !candidate
        .bytes()
        .zip(source.bytes())
        .all(|(actual, expected)| actual == if expected == b'/' { b'\\' } else { expected })
    {
        return false;
    }
    path_literal_boundaries(input, offset, source.len(), source.starts_with('/'))
}

fn path_literal_boundaries(input: &str, offset: usize, length: usize, rooted: bool) -> bool {
    let before = input[..offset].chars().next_back();
    let after = input[offset + length..].chars().next();
    before.is_none_or(is_path_boundary)
        && after.is_none_or(is_path_boundary)
        && (rooted || !before.is_some_and(|character| matches!(character, '/' | '\\')))
        && (rooted || !after.is_some_and(|character| matches!(character, '/' | '\\')))
}

fn is_path_boundary(character: char) -> bool {
    character.is_ascii()
        && !character.is_ascii_alphanumeric()
        && !matches!(character, '_' | '$' | '.' | '-' | '~')
}

fn replace_number_tokens(input: &str, mappings: &[LiteralMapping]) -> FoundationResult<String> {
    let mut output = String::with_capacity(input.len());
    let mut offset = 0;
    while offset < input.len() {
        if let Some(length) = normalization_placeholder_len_at(input, offset) {
            output.push_str(&input[offset..offset + length]);
            offset += length;
            continue;
        }
        let matched = mappings.iter().find(|mapping| {
            input[offset..].starts_with(&mapping.from)
                && is_number_boundary(input, offset, mapping.from.len())
        });
        if let Some(mapping) = matched {
            output.push_str(&mapping.to);
            offset += mapping.from.len();
        } else {
            let character = input[offset..]
                .chars()
                .next()
                .expect("offset stays on a character boundary");
            output.push(character);
            offset += character.len_utf8();
        }
    }
    Ok(output)
}

fn is_number_boundary(input: &str, offset: usize, length: usize) -> bool {
    let previous = input[..offset].chars().next_back();
    let next = input[offset + length..].chars().next();
    previous.is_none_or(|character| !character.is_ascii_alphanumeric())
        && next.is_none_or(|character| !character.is_ascii_alphanumeric())
}

fn replace_hex_addresses(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut offset = 0;
    while offset < input.len() {
        if let Some(length) = normalization_placeholder_len_at(input, offset) {
            output.push_str(&input[offset..offset + length]);
            offset += length;
            continue;
        }
        let rest = &input[offset..];
        if rest.starts_with("0x")
            && input[..offset]
                .chars()
                .next_back()
                .is_none_or(|character| !character.is_ascii_alphanumeric())
        {
            let digits = rest[2..].bytes().take_while(u8::is_ascii_hexdigit).count();
            let after = rest[2 + digits..].chars().next();
            if digits >= 6 && after.is_none_or(|character| !character.is_ascii_alphanumeric()) {
                output.push_str("<%2%>");
                offset += 2 + digits;
                continue;
            }
        }
        let character = rest
            .chars()
            .next()
            .expect("offset stays on a character boundary");
        output.push(character);
        offset += character.len_utf8();
    }
    output
}

fn is_identifier_boundary(input: &str, offset: usize, length: usize) -> bool {
    let previous = input[..offset].chars().next_back();
    let next = input[offset + length..].chars().next();
    previous.is_none_or(is_conservative_token_boundary)
        && next.is_none_or(is_conservative_token_boundary)
}

fn encode_literal(input: &str) -> String {
    input.replace('<', "<<")
}

/// Validate the uniquely decodable schema-1 text encoding. `<<` is one raw
/// literal `<`; a single `<` must begin a canonical typed placeholder.
pub(crate) fn validate_class_normalized_text(input: &str, context: &str) -> FoundationResult<()> {
    let mut offset = 0;
    while offset < input.len() {
        let rest = &input[offset..];
        if rest.starts_with("<<") {
            offset += 2;
            continue;
        }
        if rest.starts_with('<') {
            let Some(length) = normalization_placeholder_len_at(input, offset) else {
                return Err(FoundationError::new(format!(
                    "{context} contains an unescaped literal '<' at byte {offset}"
                )));
            };
            if input.as_bytes().get(offset + 1) == Some(&b'%') {
                return Err(FoundationError::new(format!(
                    "{context} contains a terminal-only placeholder at byte {offset}"
                )));
            }
            offset += length;
            continue;
        }
        let character = rest
            .chars()
            .next()
            .expect("offset stays on a character boundary");
        offset += character.len_utf8();
    }
    Ok(())
}

fn is_conservative_token_boundary(character: char) -> bool {
    character.is_ascii() && !is_ascii_identifier_continue(character as u8)
}

fn is_ascii_identifier(text: &str) -> bool {
    let mut bytes = text.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    is_ascii_identifier_start(first) && bytes.all(is_ascii_identifier_continue)
}

const fn is_ascii_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

const fn is_ascii_identifier_continue(byte: u8) -> bool {
    is_ascii_identifier_start(byte) || byte.is_ascii_digit()
}
