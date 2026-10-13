//! tsgo `ls/lsconv/linemap.go`: a text's lines as the protocol counts them.

/// tsgo `LSPLineMap`: where each line starts (a UTF-8 offset), with only
/// `\n`, `\r` and `\r\n` breaking lines, and whether the text is ASCII.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LspLineMap {
    pub line_starts: Vec<usize>,
    pub ascii_only: bool,
}

impl LspLineMap {
    /// tsgo `ComputeLSPLineStarts`.
    pub fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut line_starts = Vec::with_capacity(bytecount_newlines(bytes) + 1);
        let mut ascii_only = true;
        let mut position = 0;
        let mut line_start = 0;
        while position < bytes.len() {
            let byte = bytes[position];
            if byte.is_ascii() {
                position += 1;
                if byte == b'\r' && bytes.get(position) == Some(&b'\n') {
                    position += 1;
                }
                if byte == b'\r' || byte == b'\n' {
                    line_starts.push(line_start);
                    line_start = position;
                }
            } else {
                position += utf8_sequence_length(byte);
                ascii_only = false;
            }
        }
        line_starts.push(line_start);
        Self {
            line_starts,
            ascii_only,
        }
    }

    /// tsgo `ComputeIndexOfLineStart`: the line `position` is on.
    pub fn index_of_line_start(&self, position: usize) -> usize {
        match self.line_starts.binary_search(&position) {
            Ok(line) => line,
            Err(0) => 0,
            Err(insertion) => insertion - 1,
        }
    }
}

fn bytecount_newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&byte| byte == b'\n').count()
}

/// The length of the UTF-8 sequence a leading byte starts (a Rust string
/// holds only valid sequences).
fn utf8_sequence_length(byte: u8) -> usize {
    match byte {
        0xF0..=0xFF => 4,
        0xE0..=0xEF => 3,
        _ => 2,
    }
}
