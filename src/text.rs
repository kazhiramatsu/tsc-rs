//! Source text with tsc-compatible line/column computation.
//! Internal offsets are UTF-8 bytes; printed columns are UTF-16 code units
//! (1-based), matching tsc exactly.

pub struct SourceText {
    pub text: String,
    /// Byte offsets of line starts.
    line_starts: Vec<u32>,
    /// UTF-16 code-unit offset of each line start (parallel to `line_starts`),
    /// so a byte offset can be turned into the absolute UTF-16 offset tsc and
    /// LSP use, in O(line length).
    line_starts_utf16: Vec<u32>,
}

pub type Span = crate::ast::Span;

impl SourceText {
    pub fn new(mut text: String) -> SourceText {
        if text.starts_with('\u{FEFF}') {
            text.drain(..'\u{FEFF}'.len_utf8());
        }
        let line_starts = compute_line_starts(&text);
        // UTF-16 offset of each line start.
        let mut line_starts_utf16 = Vec::with_capacity(line_starts.len());
        let mut u16_acc: u32 = 0;
        let mut prev_byte: usize = 0;
        for &ls in &line_starts {
            u16_acc += text[prev_byte..ls as usize]
                .chars()
                .map(|c| c.len_utf16() as u32)
                .sum::<u32>();
            line_starts_utf16.push(u16_acc);
            prev_byte = ls as usize;
        }
        SourceText {
            text,
            line_starts,
            line_starts_utf16,
        }
    }

    /// 1-based (line, utf16-column) for a byte offset.
    pub fn line_col(&self, byte_off: u32) -> (u32, u32) {
        // A diagnostic span occasionally carries an offset from a different
        // source file (e.g. a library signature's type parameter surfaced while
        // checking user code); clamp so the formatter degrades to a best-effort
        // position rather than panicking with an out-of-bounds index.
        let byte_off = byte_off.min(self.text.len() as u32);
        let line_idx = match self.line_starts.binary_search(&byte_off) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let line_start = self.line_starts[line_idx] as usize;
        let col_utf16: usize = self.text[line_start..byte_off as usize]
            .chars()
            .map(|c| c.len_utf16())
            .sum();
        (line_idx as u32 + 1, col_utf16 as u32 + 1)
    }

    /// Absolute UTF-16 code-unit offset for a byte offset — the position
    /// convention tsc (and LSP `character`) use, so downstream tooling that
    /// indexes the source in UTF-16 sees the same numbers as `tsc`.
    pub fn utf16_offset(&self, byte_off: u32) -> u32 {
        // clamp out-of-range offsets like `line_col`, so the JSON formatter is
        // equally total (a span carrying another file's offset degrades to a
        // best-effort position instead of panicking).
        let byte_off = byte_off.min(self.text.len() as u32);
        let line_idx = match self.line_starts.binary_search(&byte_off) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let line_start = self.line_starts[line_idx] as usize;
        let col_utf16: u32 = self.text[line_start..byte_off as usize]
            .chars()
            .map(|c| c.len_utf16() as u32)
            .sum();
        self.line_starts_utf16[line_idx] + col_utf16
    }
}

fn compute_line_starts(text: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                i += 1;
                if i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                }
                starts.push(i as u32);
            }
            b'\n' => {
                i += 1;
                starts.push(i as u32);
            }
            0xE2 => {
                // U+2028 LINE SEPARATOR (E2 80 A8), U+2029 PARAGRAPH SEPARATOR (E2 80 A9)
                if i + 2 < bytes.len()
                    && bytes[i + 1] == 0x80
                    && (bytes[i + 2] == 0xA8 || bytes[i + 2] == 0xA9)
                {
                    i += 3;
                    starts.push(i as u32);
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_basic() {
        let t = SourceText::new("ab\ncd\r\nef".to_string());
        assert_eq!(t.line_col(0), (1, 1));
        assert_eq!(t.line_col(1), (1, 2));
        assert_eq!(t.line_col(3), (2, 1));
        assert_eq!(t.line_col(7), (3, 1));
    }

    #[test]
    fn utf16_columns() {
        // "あ" is 3 UTF-8 bytes, 1 UTF-16 unit; "𠮷" is 4 bytes, 2 units.
        let t = SourceText::new("あx\n𠮷y".to_string());
        assert_eq!(t.line_col(3), (1, 2)); // after あ
        assert_eq!(t.line_col(5), (2, 1)); // start of 𠮷 line
        assert_eq!(t.line_col(9), (2, 3)); // after 𠮷 = 2 utf16 units
    }

    #[test]
    fn bom_stripped() {
        let t = SourceText::new("\u{FEFF}abc".to_string());
        assert_eq!(t.text, "abc");
    }
}
