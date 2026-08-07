use std::error::Error;
use std::fmt;

use tsc_diagnostics::PositionIndex;

/// A validated offset into source text, measured in UTF-8 bytes.
///
/// Parsed Rust nodes use this domain. It is deliberately not interchangeable
/// with TypeScript's UTF-16 source positions or generated writer positions.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceBytePosition(u32);

impl SourceBytePosition {
    pub fn new(value: u32, positions: &PositionIndex) -> Result<Self, SourcePositionError> {
        if value > positions.byte_len() {
            return Err(SourcePositionError::OutOfBounds {
                domain: PositionDomain::SourceByte,
                position: value,
                length: positions.byte_len(),
            });
        }
        if positions.byte_to_utf16(value).is_none() {
            return Err(SourcePositionError::NotUnicodeScalarBoundary { position: value });
        }
        Ok(Self(value))
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

/// A validated half-open range in source UTF-8 byte space.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceByteRange {
    start: SourceBytePosition,
    end: SourceBytePosition,
}

impl SourceByteRange {
    pub fn new(
        start: u32,
        end: u32,
        positions: &PositionIndex,
    ) -> Result<Self, SourcePositionError> {
        let start = SourceBytePosition::new(start, positions)?;
        let end = SourceBytePosition::new(end, positions)?;
        if start > end {
            return Err(SourcePositionError::InvertedRange {
                start: start.value(),
                end: end.value(),
            });
        }
        Ok(Self { start, end })
    }

    pub const fn start(self) -> SourceBytePosition {
        self.start
    }

    pub const fn end(self) -> SourceBytePosition {
        self.end
    }
}

/// Source range state. The synthetic sentinel is a discriminant, never an
/// integer that can accidentally index source text.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SourceRange {
    Original(SourceByteRange),
    Synthesized,
}

impl SourceRange {
    pub fn from_raw(
        start: u32,
        end: u32,
        positions: &PositionIndex,
    ) -> Result<Self, SourcePositionError> {
        if start == u32::MAX || end == u32::MAX {
            return if start == u32::MAX && end == u32::MAX {
                Ok(Self::Synthesized)
            } else {
                Err(SourcePositionError::MixedSyntheticRange { start, end })
            };
        }
        SourceByteRange::new(start, end, positions).map(Self::Original)
    }
}

/// A source offset in TypeScript's UTF-16 code-unit domain.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceUtf16Position(u32);

impl SourceUtf16Position {
    pub fn from_byte(
        position: SourceBytePosition,
        positions: &PositionIndex,
    ) -> Result<Self, SourcePositionError> {
        positions.byte_to_utf16(position.value()).map(Self).ok_or(
            SourcePositionError::NotUnicodeScalarBoundary {
                position: position.value(),
            },
        )
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

/// A source line and column, both interpreted in TypeScript's UTF-16 domain.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceUtf16Location {
    line: u32,
    column: u32,
}

impl SourceUtf16Location {
    pub fn from_byte(
        position: SourceBytePosition,
        positions: &PositionIndex,
    ) -> Result<Self, SourcePositionError> {
        let utf16 = SourceUtf16Position::from_byte(position, positions)?;
        let location = positions.line_and_character_utf16(utf16.value()).ok_or(
            SourcePositionError::OutOfBounds {
                domain: PositionDomain::SourceUtf16,
                position: utf16.value(),
                length: positions.utf16_len(),
            },
        )?;
        Ok(Self {
            line: location.line,
            column: location.character,
        })
    }

    pub const fn line(self) -> u32 {
        self.line
    }

    pub const fn column(self) -> u32 {
        self.column
    }
}

/// A generated-text position measured in UTF-16 code units.
///
/// This is intentionally distinct from byte offsets and source positions.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GeneratedUtf16Position(u32);

impl GeneratedUtf16Position {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u32 {
        self.0
    }

    pub(crate) fn checked_add(self, amount: u32) -> Option<Self> {
        self.0.checked_add(amount).map(Self)
    }
}

/// Writer line/column observation in generated UTF-16 space.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct GeneratedUtf16Location {
    position: GeneratedUtf16Position,
    line: u32,
    column: u32,
}

impl GeneratedUtf16Location {
    pub(crate) const fn new(position: GeneratedUtf16Position, line: u32, column: u32) -> Self {
        Self {
            position,
            line,
            column,
        }
    }

    pub const fn position(self) -> GeneratedUtf16Position {
        self.position
    }

    pub const fn line(self) -> u32 {
        self.line
    }

    pub const fn column(self) -> u32 {
        self.column
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PositionDomain {
    SourceByte,
    SourceUtf16,
}

/// Rejected cross-domain or invalid source-position conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourcePositionError {
    OutOfBounds {
        domain: PositionDomain,
        position: u32,
        length: u32,
    },
    NotUnicodeScalarBoundary {
        position: u32,
    },
    InvertedRange {
        start: u32,
        end: u32,
    },
    MixedSyntheticRange {
        start: u32,
        end: u32,
    },
}

impl fmt::Display for SourcePositionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds {
                domain,
                position,
                length,
            } => write!(
                formatter,
                "{domain:?} position {position} exceeds length {length}"
            ),
            Self::NotUnicodeScalarBoundary { position } => write!(
                formatter,
                "source byte position {position} is not a Unicode scalar boundary"
            ),
            Self::InvertedRange { start, end } => {
                write!(formatter, "source byte range {start}..{end} is inverted")
            }
            Self::MixedSyntheticRange { start, end } => write!(
                formatter,
                "source range mixes synthesized and original positions: {start}..{end}"
            ),
        }
    }
}

impl Error for SourcePositionError {}
