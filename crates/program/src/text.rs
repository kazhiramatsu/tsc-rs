use std::error::Error;
use std::fmt;

/// UTF-16 byte order observed at the host text boundary.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HostTextEncoding {
    Utf16Le,
    Utf16Be,
}

impl HostTextEncoding {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Utf16Le => "UTF-16LE",
            Self::Utf16Be => "UTF-16BE",
        }
    }
}

/// A host text payload that cannot be represented by the program's UTF-8
/// source-text contract. `code_unit_index` is zero-based within the UTF-16
/// payload after its two-byte BOM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostTextDecodeError {
    encoding: HostTextEncoding,
    code_unit_index: usize,
    unpaired_surrogate: u16,
}

impl HostTextDecodeError {
    /// The BOM-selected byte order of the rejected payload.
    pub const fn encoding(self) -> HostTextEncoding {
        self.encoding
    }

    /// The zero-based UTF-16 code-unit offset after the BOM.
    pub const fn code_unit_index(self) -> usize {
        self.code_unit_index
    }

    /// The first unpaired surrogate that cannot enter a Rust `String`.
    pub const fn unpaired_surrogate(self) -> u16 {
        self.unpaired_surrogate
    }
}

impl fmt::Display for HostTextDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} contains unpaired surrogate U+{:04X} at code-unit index {}",
            self.encoding.name(),
            self.unpaired_surrogate,
            self.code_unit_index
        )
    }
}

impl Error for HostTextDecodeError {}

/// Decode bytes returned by a [`tsc_host::CompilerHost`] once, before they
/// enter package parsing or source-file construction.
///
/// The BOM selection, odd UTF-16 byte truncation, and invalid UTF-8
/// replacement match the vendored Node host. Node can retain an unpaired
/// surrogate in a JavaScript string, while Rust `String` cannot; that one
/// representation boundary is reported explicitly instead of silently
/// changing source identity to U+FFFD.
///
/// tsc-port: readFile @6.0.3 (decode branches)
/// tsc-hash: 3c9ba7756aa0df5a44c1e977839a56e1e71e24b9b80b866813b677e8a9a01bf4
/// tsc-span: _tsc.js:5139-5163
pub fn decode_host_text(mut bytes: Vec<u8>) -> Result<String, HostTextDecodeError> {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16(&bytes[2..], HostTextEncoding::Utf16Be);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16(&bytes[2..], HostTextEncoding::Utf16Le);
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }

    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(error) => Ok(String::from_utf8_lossy(&error.into_bytes()).into_owned()),
    }
}

fn decode_utf16(bytes: &[u8], encoding: HostTextEncoding) -> Result<String, HostTextDecodeError> {
    let code_units = bytes.chunks_exact(2).map(|pair| {
        let pair = [pair[0], pair[1]];
        match encoding {
            HostTextEncoding::Utf16Le => u16::from_le_bytes(pair),
            HostTextEncoding::Utf16Be => u16::from_be_bytes(pair),
        }
    });
    let mut text = String::with_capacity(bytes.len());
    let mut code_unit_index = 0;
    for decoded in char::decode_utf16(code_units) {
        match decoded {
            Ok(character) => {
                code_unit_index += character.len_utf16();
                text.push(character);
            }
            Err(error) => {
                return Err(HostTextDecodeError {
                    encoding,
                    code_unit_index,
                    unpaired_surrogate: error.unpaired_surrogate(),
                });
            }
        }
    }
    Ok(text)
}
