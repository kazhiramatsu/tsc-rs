use core::fmt;
use std::str;

/// Errors raised while producing canonical bytes into a bounded sink.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CanonicalError {
    LimitExceeded,
    DuplicateKey,
    InvalidKeyOrder,
    DepthExceeded,
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::LimitExceeded => "canonical byte limit exceeded",
            Self::DuplicateKey => "canonical object contains a duplicate key",
            Self::InvalidKeyOrder => "canonical object keys are not strictly ordered",
            Self::DepthExceeded => "canonical value nesting exceeds the limit",
        };
        formatter.write_str(name)
    }
}

impl std::error::Error for CanonicalError {}

/// Errors raised by the strict bounded JSON decoder.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DecodeError {
    LimitExceeded,
    UnexpectedEof,
    InvalidToken,
    InvalidEscape,
    InvalidUnicode,
    DuplicateKey,
    UnsortedKey,
    UnknownField,
    NonCanonical,
    TrailingBytes,
    IntegerOverflow,
    DepthExceeded,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::LimitExceeded => "canonical input exceeds the byte limit",
            Self::UnexpectedEof => "unexpected end of canonical input",
            Self::InvalidToken => "invalid canonical JSON token",
            Self::InvalidEscape => "invalid or noncanonical JSON escape",
            Self::InvalidUnicode => "invalid Unicode scalar or surrogate pair",
            Self::DuplicateKey => "canonical object contains a duplicate key",
            Self::UnsortedKey => "canonical object keys are not strictly ordered",
            Self::UnknownField => "canonical object contains an unknown field",
            Self::NonCanonical => "input re-encodes to different canonical bytes",
            Self::TrailingBytes => "canonical input has trailing bytes",
            Self::IntegerOverflow => "canonical integer exceeds the supported width",
            Self::DepthExceeded => "canonical input nesting exceeds the limit",
        };
        formatter.write_str(name)
    }
}

impl std::error::Error for DecodeError {}

/// A sink for canonical bytes. Implementations must reject a write before it
/// crosses their declared byte ceiling.
pub trait CanonicalSink {
    fn write(&mut self, bytes: &[u8]) -> Result<(), CanonicalError>;

    fn remaining(&self) -> u64;
}

/// A bounded in-memory sink used by tests and small control-plane values.
/// Larger effects can provide their own sink without changing the encoder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedBytesSink {
    bytes: Vec<u8>,
    limit: u64,
}

impl BoundedBytesSink {
    pub fn new(limit: u64) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl CanonicalSink for BoundedBytesSink {
    fn write(&mut self, bytes: &[u8]) -> Result<(), CanonicalError> {
        let current = self.bytes.len() as u64;
        let requested = bytes.len() as u64;
        if current
            .checked_add(requested)
            .is_none_or(|total| total > self.limit)
        {
            return Err(CanonicalError::LimitExceeded);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn remaining(&self) -> u64 {
        self.limit - self.bytes.len() as u64
    }
}

/// A value with exactly the JSON subset permitted by the v1 wire protocol.
/// Floating-point values intentionally have no variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanonicalValue {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    String(String),
    Array(Vec<Self>),
    Object(Vec<(String, Self)>),
}

/// A type that can emit itself in the v1 canonical JSON representation.
pub trait CanonicalEncode {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError>;
}

const MAX_ENCODE_DEPTH: usize = 256;

impl CanonicalEncode for CanonicalValue {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        encode_value(self, out, 0)
    }
}

fn encode_value<S: CanonicalSink>(
    value: &CanonicalValue,
    out: &mut S,
    depth: usize,
) -> Result<(), CanonicalError> {
    if depth > MAX_ENCODE_DEPTH {
        return Err(CanonicalError::DepthExceeded);
    }
    match value {
        CanonicalValue::Null => out.write(b"null"),
        CanonicalValue::Bool(true) => out.write(b"true"),
        CanonicalValue::Bool(false) => out.write(b"false"),
        CanonicalValue::Signed(value) => write_integer(out, *value),
        CanonicalValue::Unsigned(value) => write_integer(out, *value),
        CanonicalValue::String(value) => write_string(out, value),
        CanonicalValue::Array(values) => {
            out.write(b"[")?;
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    out.write(b",")?;
                }
                encode_value(value, out, depth + 1)?;
            }
            out.write(b"]")
        }
        CanonicalValue::Object(entries) => {
            let mut sorted = entries.iter().collect::<Vec<_>>();
            sorted.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
            for pair in sorted.windows(2) {
                if pair[0].0.as_bytes() == pair[1].0.as_bytes() {
                    return Err(CanonicalError::DuplicateKey);
                }
            }
            out.write(b"{")?;
            for (index, (key, value)) in sorted.into_iter().enumerate() {
                if index != 0 {
                    out.write(b",")?;
                }
                write_string(out, key)?;
                out.write(b":")?;
                encode_value(value, out, depth + 1)?;
            }
            out.write(b"}")
        }
    }
}

fn write_integer<S: CanonicalSink, T: ToString>(
    out: &mut S,
    value: T,
) -> Result<(), CanonicalError> {
    let text = value.to_string();
    out.write(text.as_bytes())
}

fn write_string<S: CanonicalSink>(out: &mut S, value: &str) -> Result<(), CanonicalError> {
    out.write(b"\"")?;
    for character in value.chars() {
        match character {
            '"' => out.write(br#"\""#)?,
            '\\' => out.write(br#"\\"#)?,
            '\u{08}' => out.write(br#"\b"#)?,
            '\u{09}' => out.write(br#"\t"#)?,
            '\u{0a}' => out.write(br#"\n"#)?,
            '\u{0c}' => out.write(br#"\f"#)?,
            '\u{0d}' => out.write(br#"\r"#)?,
            character if character <= '\u{1f}' => {
                let code = character as u32;
                let hex = [
                    b"0123456789abcdef"[((code >> 4) & 0xf) as usize],
                    b"0123456789abcdef"[(code & 0xf) as usize],
                ];
                out.write(b"\\u00")?;
                out.write(&hex)?;
            }
            character => {
                let mut bytes = [0; 4];
                out.write(character.encode_utf8(&mut bytes).as_bytes())?;
            }
        }
    }
    out.write(b"\"")
}

/// A bounded, push-based strict decoder.
pub trait CanonicalDecoder {
    type Output;

    fn push(&mut self, chunk: &[u8]) -> Result<(), DecodeError>;

    fn finish(self) -> Result<Self::Output, DecodeError>;
}

#[derive(Debug)]
pub struct StrictJsonDecoder {
    bytes: Vec<u8>,
    max_bytes: u64,
    max_depth: usize,
}

impl StrictJsonDecoder {
    pub fn new(max_bytes: u64, max_depth: usize) -> Self {
        Self {
            bytes: Vec::new(),
            max_bytes,
            max_depth,
        }
    }
}

impl CanonicalDecoder for StrictJsonDecoder {
    type Output = CanonicalValue;

    fn push(&mut self, chunk: &[u8]) -> Result<(), DecodeError> {
        let current = self.bytes.len() as u64;
        let requested = chunk.len() as u64;
        if current
            .checked_add(requested)
            .is_none_or(|total| total > self.max_bytes)
        {
            return Err(DecodeError::LimitExceeded);
        }
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    fn finish(self) -> Result<Self::Output, DecodeError> {
        decode_bytes(&self.bytes, self.max_depth)
    }
}

pub fn decode_canonical(
    bytes: &[u8],
    max_bytes: u64,
    max_depth: usize,
) -> Result<CanonicalValue, DecodeError> {
    if bytes.len() as u64 > max_bytes {
        return Err(DecodeError::LimitExceeded);
    }
    decode_bytes(bytes, max_depth)
}

/// Decodes one object and applies a caller-owned typed field set. The generic
/// core does not know adapter nouns; it only performs the closed membership
/// check supplied by the typed schema owner.
pub fn decode_object_with_keys(
    bytes: &[u8],
    max_bytes: u64,
    max_depth: usize,
    allowed_keys: &[&str],
) -> Result<CanonicalValue, DecodeError> {
    let value = decode_canonical(bytes, max_bytes, max_depth)?;
    let CanonicalValue::Object(entries) = &value else {
        return Err(DecodeError::InvalidToken);
    };
    if entries
        .iter()
        .any(|(key, _)| !allowed_keys.iter().any(|allowed| *allowed == key))
    {
        return Err(DecodeError::UnknownField);
    }
    Ok(value)
}

fn decode_bytes(bytes: &[u8], max_depth: usize) -> Result<CanonicalValue, DecodeError> {
    let mut parser = Parser {
        bytes,
        position: 0,
        max_depth,
    };
    let value = parser.parse_value(0)?;
    if parser.position != bytes.len() {
        return Err(DecodeError::TrailingBytes);
    }

    // A strict decoder accepts only the exact bytes emitted by the Rust
    // encoder. This catches alternate escapes, surrogate spelling, and any
    // future parser path that accidentally accepts insignificant whitespace.
    let mut sink = BoundedBytesSink::new(bytes.len() as u64);
    value
        .encode_canonical(&mut sink)
        .map_err(|_| DecodeError::NonCanonical)?;
    if sink.bytes() != bytes {
        return Err(DecodeError::NonCanonical);
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
    max_depth: usize,
}

impl<'a> Parser<'a> {
    fn parse_value(&mut self, depth: usize) -> Result<CanonicalValue, DecodeError> {
        if depth > self.max_depth {
            return Err(DecodeError::DepthExceeded);
        }
        match self.peek() {
            Some(b'n') => {
                self.literal(b"null")?;
                Ok(CanonicalValue::Null)
            }
            Some(b't') => {
                self.literal(b"true")?;
                Ok(CanonicalValue::Bool(true))
            }
            Some(b'f') => {
                self.literal(b"false")?;
                Ok(CanonicalValue::Bool(false))
            }
            Some(b'"') => Ok(CanonicalValue::String(self.parse_string()?)),
            Some(b'[') => self.parse_array(depth),
            Some(b'{') => self.parse_object(depth),
            Some(b'-' | b'0'..=b'9') => self.parse_integer(),
            Some(b' ' | b'\t' | b'\n' | b'\r') => Err(DecodeError::NonCanonical),
            Some(_) => Err(DecodeError::InvalidToken),
            None => Err(DecodeError::UnexpectedEof),
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<CanonicalValue, DecodeError> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        if self.consume_if(b']') {
            return Ok(CanonicalValue::Array(values));
        }
        loop {
            values.push(self.parse_value(depth + 1)?);
            match self.next() {
                Some(b',') => {}
                Some(b']') => break,
                Some(_) => return Err(DecodeError::InvalidToken),
                None => return Err(DecodeError::UnexpectedEof),
            }
        }
        Ok(CanonicalValue::Array(values))
    }

    fn parse_object(&mut self, depth: usize) -> Result<CanonicalValue, DecodeError> {
        self.expect(b'{')?;
        let mut entries = Vec::new();
        let mut previous: Option<Vec<u8>> = None;
        if self.consume_if(b'}') {
            return Ok(CanonicalValue::Object(entries));
        }
        loop {
            if self.peek() != Some(b'"') {
                return Err(DecodeError::InvalidToken);
            }
            let key = self.parse_string()?;
            if let Some(previous) = &previous {
                match previous.as_slice().cmp(key.as_bytes()) {
                    core::cmp::Ordering::Less => {}
                    core::cmp::Ordering::Equal => return Err(DecodeError::DuplicateKey),
                    core::cmp::Ordering::Greater => return Err(DecodeError::UnsortedKey),
                }
            }
            previous = Some(key.as_bytes().to_vec());
            self.expect(b':')?;
            let value = self.parse_value(depth + 1)?;
            entries.push((key, value));
            match self.next() {
                Some(b',') => {}
                Some(b'}') => break,
                Some(_) => return Err(DecodeError::InvalidToken),
                None => return Err(DecodeError::UnexpectedEof),
            }
        }
        Ok(CanonicalValue::Object(entries))
    }

    fn parse_integer(&mut self) -> Result<CanonicalValue, DecodeError> {
        let negative = self.consume_if(b'-');
        let first = self.next().ok_or(DecodeError::UnexpectedEof)?;
        let mut digits = Vec::new();
        match first {
            b'0' => {
                digits.push(first);
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(DecodeError::NonCanonical);
                }
            }
            b'1'..=b'9' => {
                digits.push(first);
                while let Some(byte @ b'0'..=b'9') = self.peek() {
                    self.position += 1;
                    digits.push(byte);
                }
            }
            _ => return Err(DecodeError::InvalidToken),
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(DecodeError::InvalidToken);
        }
        let text = str::from_utf8(&digits).map_err(|_| DecodeError::InvalidToken)?;
        let value = text
            .parse::<u64>()
            .map_err(|_| DecodeError::IntegerOverflow)?;
        if negative {
            if value == 0 {
                return Err(DecodeError::NonCanonical);
            }
            if value == (i64::MAX as u64) + 1 {
                return Ok(CanonicalValue::Signed(i64::MIN));
            }
            let value = i64::try_from(value).map_err(|_| DecodeError::IntegerOverflow)?;
            Ok(CanonicalValue::Signed(-value))
        } else if value <= i64::MAX as u64 {
            Ok(CanonicalValue::Signed(value as i64))
        } else {
            Ok(CanonicalValue::Unsigned(value))
        }
    }

    fn parse_string(&mut self) -> Result<String, DecodeError> {
        self.expect(b'"')?;
        let mut value = String::new();
        loop {
            let byte = self.next().ok_or(DecodeError::UnexpectedEof)?;
            match byte {
                b'"' => return Ok(value),
                b'\\' => self.parse_escape(&mut value)?,
                0..=0x1f => return Err(DecodeError::InvalidToken),
                byte => {
                    let start = self.position - 1;
                    let width = utf8_width(byte).ok_or(DecodeError::InvalidUnicode)?;
                    if width > 1 {
                        if self.position + width - 1 > self.bytes.len() {
                            return Err(DecodeError::UnexpectedEof);
                        }
                        self.position += width - 1;
                    }
                    let character = str::from_utf8(&self.bytes[start..self.position])
                        .map_err(|_| DecodeError::InvalidUnicode)?
                        .chars()
                        .next()
                        .ok_or(DecodeError::InvalidUnicode)?;
                    if character == '\\' || character == '"' || character <= '\u{1f}' {
                        return Err(DecodeError::InvalidToken);
                    }
                    value.push(character);
                }
            }
        }
    }

    fn parse_escape(&mut self, output: &mut String) -> Result<(), DecodeError> {
        match self.next().ok_or(DecodeError::UnexpectedEof)? {
            b'"' => output.push('"'),
            b'\\' => output.push('\\'),
            b'b' => output.push('\u{08}'),
            b'f' => output.push('\u{0c}'),
            b'n' => output.push('\n'),
            b'r' => output.push('\r'),
            b't' => output.push('\t'),
            b'u' => {
                let high = self.parse_hex_unit()?;
                match high {
                    0x0000..=0x001f => {
                        // The canonical encoder emits exactly \u00xx for the
                        // remaining control scalars.
                        if high > 0x001f {
                            return Err(DecodeError::InvalidEscape);
                        }
                        output.push(char::from_u32(high as u32).unwrap());
                    }
                    0xd800..=0xdbff => {
                        if self.next() != Some(b'\\') || self.next() != Some(b'u') {
                            return Err(DecodeError::InvalidUnicode);
                        }
                        let low = self.parse_hex_unit()?;
                        if !(0xdc00..=0xdfff).contains(&low) {
                            return Err(DecodeError::InvalidUnicode);
                        }
                        let scalar =
                            0x10000 + (((high as u32) - 0xd800) << 10) + ((low as u32) - 0xdc00);
                        output.push(char::from_u32(scalar).ok_or(DecodeError::InvalidUnicode)?);
                    }
                    0xdc00..=0xdfff => return Err(DecodeError::InvalidUnicode),
                    value => {
                        output
                            .push(char::from_u32(value as u32).ok_or(DecodeError::InvalidUnicode)?);
                    }
                }
            }
            _ => return Err(DecodeError::InvalidEscape),
        }
        Ok(())
    }

    fn parse_hex_unit(&mut self) -> Result<u16, DecodeError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let byte = self.next().ok_or(DecodeError::UnexpectedEof)?;
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => return Err(DecodeError::InvalidEscape),
            };
            value = (value << 4) | u16::from(digit);
        }
        Ok(value)
    }

    fn literal(&mut self, expected: &[u8]) -> Result<(), DecodeError> {
        for byte in expected {
            if self.next() != Some(*byte) {
                return Err(DecodeError::InvalidToken);
            }
        }
        Ok(())
    }

    fn expect(&mut self, expected: u8) -> Result<(), DecodeError> {
        if self.next() == Some(expected) {
            Ok(())
        } else {
            Err(DecodeError::InvalidToken)
        }
    }

    fn consume_if(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }
}

fn utf8_width(first: u8) -> Option<usize> {
    match first {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}
