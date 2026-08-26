//! Small lexical helpers shared by the pin index and prospective planner.
//!
//! The repository inputs are deliberately not parsed as Rust, JavaScript, or
//! JSON ASTs here. The checked-in pin formats are string-literal formats, and
//! retaining byte offsets is more useful than accepting a lossy parser.

use std::fmt;

/// One of the five pin grammars used by scripts/chain-walk-repin.py.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Grammar {
    A,
    B,
    C,
    D,
    E,
}

impl Grammar {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
        }
    }
}

impl fmt::Display for Grammar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A quoted string with source byte offsets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotedString {
    pub start: usize,
    pub content_start: usize,
    pub content_end: usize,
    pub end: usize,
    pub value: String,
}

/// A hash literal extracted by one of the oracle grammars.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedPin {
    pub start: usize,
    pub end: usize,
    pub path: String,
    pub grammar: Grammar,
    pub literal: String,
}

/// A direct JSON-like path/hash pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathHashPair {
    pub path: String,
    pub hash_key: String,
    pub hash: String,
    pub hash_start: usize,
    pub hash_end: usize,
}

/// An unclassified path-adjacent 64-hex literal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnclassifiedLiteral {
    pub start: usize,
    pub literal: String,
}

/// Returns all non-escaped double-quoted strings in source order.
pub fn quoted_strings(text: &str) -> Vec<QuotedString> {
    let mut strings = Vec::new();
    let mut offset = 0usize;
    while let Some(found) = text[offset..].find('"') {
        let start = offset + found;
        if let Some(quoted) = parse_quote_at(text, start) {
            offset = quoted.end;
            strings.push(quoted);
        } else {
            offset = start + 1;
        }
    }
    strings
}

/// Normalizes an include-style path such as /../../ratchets/a.json.
pub fn normalize_path(value: &str) -> Option<String> {
    let roots = ["ratchets/", "vendor/", "crates/oracle/", ".github/"];
    roots
        .iter()
        .find_map(|root| value.find(root).map(|start| value[start..].to_string()))
}

/// Whether a path belongs to a repository pin surface understood here.
pub fn allowed_path(path: &str) -> bool {
    normalize_path(path).is_some()
}

/// Ports the five repin grammars exactly, retaining the hash-only span.
pub fn extract_oracle_pins(text: &str) -> Result<Vec<ExtractedPin>, String> {
    let strings = quoted_strings(text);
    let bytes = text.as_bytes();
    let mut pins = Vec::new();

    // Pattern A: "path", "hash".
    for quoted in &strings {
        let Some(path) = normalize_path(&quoted.value) else {
            continue;
        };
        let mut offset = skip_whitespace(bytes, quoted.end);
        if bytes.get(offset) != Some(&b',') {
            continue;
        }
        offset = skip_whitespace(bytes, offset + 1);
        if let Some((start, end, literal)) = hash_quoted_at(text, offset) {
            add_pin(&mut pins, &path, Grammar::A, start, end, literal);
        }
    }

    // Pattern B: "path": newline followed by "hash".
    for quoted in &strings {
        let Some(path) = normalize_path(&quoted.value) else {
            continue;
        };
        let mut offset = skip_whitespace(bytes, quoted.end);
        if bytes.get(offset) != Some(&b':') {
            continue;
        }
        offset = skip_whitespace(bytes, offset + 1);
        if !text[quoted.end..offset].contains('\n') {
            continue;
        }
        if let Some((start, end, literal)) = hash_quoted_at(text, offset) {
            add_pin(&mut pins, &path, Grammar::B, start, end, literal);
        }
    }

    // Pattern C: "path": "hash" on one line.
    for quoted in &strings {
        let Some(path) = normalize_path(&quoted.value) else {
            continue;
        };
        let offset = quoted.end;
        if bytes.get(offset) != Some(&b':') || bytes.get(offset + 1) != Some(&b' ') {
            continue;
        }
        if let Some((start, end, literal)) = hash_quoted_at(text, offset + 2) {
            add_pin(&mut pins, &path, Grammar::C, start, end, literal);
        }
    }

    // Pattern D: const X_RELATIVE_PATH = "path" followed by const
    // X_SHA256/EXPECTED_X_SHA256 = "hash".
    for (name, path) in relative_path_constants(text) {
        for hash_name in [format!("{name}_SHA256"), format!("EXPECTED_{name}_SHA256")] {
            if let Some((start, end, literal)) = const_hash(text, &hash_name) {
                add_pin(&mut pins, &path, Grammar::D, start, end, literal);
            }
        }
    }

    // Pattern E: const PATH_CONST = "path" and [PATH_CONST]: "hash".
    for (name, path) in path_constants_for_e(text) {
        let marker = format!("[{name}]");
        let mut offset = 0usize;
        while let Some(found) = text[offset..].find(&marker) {
            let marker_start = offset + found;
            let mut value_start = marker_start + marker.len();
            if bytes.get(value_start) != Some(&b':') {
                offset = value_start;
                continue;
            }
            value_start = skip_whitespace(bytes, value_start + 1);
            if let Some((start, end, literal)) = hash_quoted_at(text, value_start) {
                add_pin(&mut pins, &path, Grammar::E, start, end, literal);
            }
            offset = value_start.saturating_add(1);
        }
    }

    pins.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| left.end.cmp(&right.end))
            .then_with(|| left.grammar.cmp(&right.grammar))
    });
    for pair in pins.windows(2) {
        if pair[0].end > pair[1].start {
            return Err(format!(
                "overlapping extracted pin spans at {} and {}",
                pair[0].start, pair[1].start
            ));
        }
    }
    Ok(pins)
}

/// Finds direct path/hash records inside a byte range.
pub fn path_hash_pairs(text: &str, range_start: usize, range_end: usize) -> Vec<PathHashPair> {
    let end = range_end.min(text.len());
    let start = range_start.min(end);
    let strings = quoted_strings(&text[start..end]);
    let bytes = text.as_bytes();
    let mut pairs = Vec::new();
    for path_key in strings.iter().filter(|quoted| quoted.value == "path") {
        let key_end = start + path_key.end;
        let colon = skip_whitespace(bytes, key_end);
        if bytes.get(colon) != Some(&b':') {
            continue;
        }
        let value_start = skip_whitespace(bytes, colon + 1);
        let Some(path_value) = parse_quote_at(text, value_start) else {
            continue;
        };
        let comma = skip_whitespace(bytes, path_value.end);
        if bytes.get(comma) != Some(&b',') {
            continue;
        }
        let hash_key_start = skip_whitespace(bytes, comma + 1);
        let Some(hash_key) = parse_quote_at(text, hash_key_start) else {
            continue;
        };
        if hash_key.value != "sha256" && hash_key.value != "hash" {
            continue;
        }
        let hash_colon = skip_whitespace(bytes, hash_key.end);
        if bytes.get(hash_colon) != Some(&b':') {
            continue;
        }
        let hash_value_start = skip_whitespace(bytes, hash_colon + 1);
        let Some((hash_start, hash_end, hash)) = hash_quoted_at(text, hash_value_start) else {
            continue;
        };
        pairs.push(PathHashPair {
            path: normalize_path(&path_value.value).unwrap_or(path_value.value),
            hash_key: hash_key.value,
            hash,
            hash_start,
            hash_end,
        });
    }
    pairs
}

/// Returns the JSON container value immediately following a key.
pub fn json_container_after_key(text: &str, key: &str) -> Option<(usize, usize)> {
    let marker = format!("\"{key}\"");
    let bytes = text.as_bytes();
    let mut offset = 0usize;
    while let Some(found) = text[offset..].find(&marker) {
        let key_start = offset + found;
        let key_end = key_start + marker.len();
        let colon = skip_whitespace(bytes, key_end);
        if bytes.get(colon) != Some(&b':') {
            offset = key_end;
            continue;
        }
        let value_start = skip_whitespace(bytes, colon + 1);
        if !matches!(bytes.get(value_start), Some(b'{') | Some(b'[')) {
            offset = key_end;
            continue;
        }
        if let Some(value_end) = matching_container(bytes, value_start) {
            return Some((value_start, value_end));
        }
        offset = key_end;
    }
    None
}

/// Finds every quoted 64-lower-hex literal and its span.
pub fn quoted_hash_literals(text: &str) -> Vec<(usize, usize, String)> {
    quoted_strings(text)
        .into_iter()
        .filter_map(|quoted| {
            if quoted.value.len() == 64 && quoted.value.bytes().all(is_lower_hex) {
                Some((quoted.content_start, quoted.content_end, quoted.value))
            } else {
                None
            }
        })
        .collect()
}

/// Reports path-adjacent 64-hex literals not covered by extracted pins.
pub fn find_unclassified_literals(text: &str, pins: &[ExtractedPin]) -> Vec<UnclassifiedLiteral> {
    let bytes = text.as_bytes();
    let mut literals = Vec::new();
    let mut offset = 0usize;
    while offset + 64 <= bytes.len() {
        if bytes[offset..offset + 64].iter().copied().all(is_hex)
            && (offset == 0 || !is_hex(bytes[offset - 1]))
            && (offset + 64 == bytes.len() || !is_hex(bytes[offset + 64]))
        {
            let covered = pins
                .iter()
                .any(|pin| pin.start == offset && pin.end == offset + 64);
            if !covered && path_is_adjacent(text, offset) {
                literals.push(UnclassifiedLiteral {
                    start: offset,
                    literal: text[offset..offset + 64].to_string(),
                });
            }
            offset += 64;
        } else {
            offset += 1;
        }
    }
    literals
}

pub fn line_number(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

fn add_pin(
    pins: &mut Vec<ExtractedPin>,
    path: &str,
    grammar: Grammar,
    start: usize,
    end: usize,
    literal: String,
) {
    if !pins.iter().any(|pin| pin.start == start && pin.end == end) {
        pins.push(ExtractedPin {
            start,
            end,
            path: path.to_string(),
            grammar,
            literal,
        });
    }
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_hex(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn skip_whitespace(bytes: &[u8], mut offset: usize) -> usize {
    while bytes
        .get(offset)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        offset += 1;
    }
    offset
}

fn parse_quote_at(text: &str, start: usize) -> Option<QuotedString> {
    if text.as_bytes().get(start) != Some(&b'"') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut offset = start + 1;
    while offset < bytes.len() {
        match bytes[offset] {
            b'\\' => offset = offset.saturating_add(2),
            b'\n' | b'\r' => return None,
            b'"' => {
                return Some(QuotedString {
                    start,
                    content_start: start + 1,
                    content_end: offset,
                    end: offset + 1,
                    value: text[start + 1..offset].to_string(),
                });
            }
            _ => offset += 1,
        }
    }
    None
}

fn hash_quoted_at(text: &str, start: usize) -> Option<(usize, usize, String)> {
    let quoted = parse_quote_at(text, start)?;
    if quoted.value.len() == 64 && quoted.value.bytes().all(is_lower_hex) {
        Some((quoted.content_start, quoted.content_end, quoted.value))
    } else {
        None
    }
}

fn relative_path_constants(text: &str) -> Vec<(String, String)> {
    all_const_strings(text)
        .into_iter()
        .filter_map(|(name, path)| {
            name.strip_suffix("_RELATIVE_PATH")
                .map(|base| (base.to_string(), path))
        })
        .collect()
}

fn path_constants_for_e(text: &str) -> Vec<(String, String)> {
    all_const_strings(text)
        .into_iter()
        .filter(|(_, path)| allowed_path(path))
        .collect()
}

fn all_const_strings(text: &str) -> Vec<(String, String)> {
    let bytes = text.as_bytes();
    let mut values = Vec::new();
    let mut offset = 0usize;
    while let Some(found) = text[offset..].find("const ") {
        let start = offset + found;
        let name_start = start + "const ".len();
        let mut name_end = name_start;
        while bytes.get(name_end).is_some_and(|byte| is_word(*byte)) {
            name_end += 1;
        }
        if name_end == name_start {
            offset = name_start;
            continue;
        }
        let after_name = skip_whitespace(bytes, name_end);
        if bytes.get(after_name) != Some(&b'=') {
            offset = name_end;
            continue;
        }
        let value_start = skip_whitespace(bytes, after_name + 1);
        if let Some(quoted) = parse_quote_at(text, value_start) {
            values.push((text[name_start..name_end].to_string(), quoted.value));
        }
        offset = name_end;
    }
    values
}

fn const_hash(text: &str, name: &str) -> Option<(usize, usize, String)> {
    let bytes = text.as_bytes();
    let marker = format!("const {name}");
    let mut offset = 0usize;
    while let Some(found) = text[offset..].find(&marker) {
        let start = offset + found;
        let after_name = start + marker.len();
        if bytes.get(after_name).is_some_and(|byte| is_word(*byte)) {
            offset = after_name;
            continue;
        }
        let after_name = skip_whitespace(bytes, after_name);
        if bytes.get(after_name) != Some(&b'=') {
            offset = after_name;
            continue;
        }
        let value_start = skip_whitespace(bytes, after_name + 1);
        return hash_quoted_at(text, value_start);
    }
    None
}

fn matching_container(bytes: &[u8], start: usize) -> Option<usize> {
    let mut stack = Vec::new();
    let mut offset = start;
    let mut in_string = false;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if in_string {
            match byte {
                b'\\' => offset = offset.saturating_add(2),
                b'"' => {
                    in_string = false;
                    offset += 1;
                }
                _ => offset += 1,
            }
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                offset += 1;
            }
            b'{' | b'[' => {
                stack.push(byte);
                offset += 1;
            }
            b'}' => {
                if stack.pop() != Some(b'{') {
                    return None;
                }
                if stack.is_empty() {
                    return Some(offset + 1);
                }
                offset += 1;
            }
            b']' => {
                if stack.pop() != Some(b'[') {
                    return None;
                }
                if stack.is_empty() {
                    return Some(offset + 1);
                }
                offset += 1;
            }
            _ => offset += 1,
        }
    }
    None
}

fn path_is_adjacent(text: &str, start: usize) -> bool {
    let bytes = text.as_bytes();
    let line_start = bytes[..start]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    let line_end = bytes[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |position| start + position);
    if quoted_strings(&text[line_start..line_end])
        .iter()
        .any(|quoted| allowed_path(&quoted.value))
    {
        return true;
    }
    let context_start = start.saturating_sub(256);
    let context_end = (start + 64 + 256).min(text.len());
    quoted_strings(&text[context_start..context_end])
        .iter()
        .any(|quoted| allowed_path(&quoted.value))
}
