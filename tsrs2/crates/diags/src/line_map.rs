#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineMap {
    pub line_starts: Vec<u32>,
    pub byte_to_utf16: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineAndCharacter {
    pub line: u32,
    pub character: u32,
}

pub fn compute_line_map(text: &str) -> LineMap {
    let mut line_starts = Vec::new();
    let mut byte_to_utf16 = vec![0; text.len() + 1];
    let mut utf16_pos = 0u32;
    let mut line_start = 0u32;
    let mut chars = text.char_indices().peekable();

    while let Some((byte_pos, ch)) = chars.next() {
        byte_to_utf16[byte_pos] = utf16_pos;
        let width = ch.len_utf16() as u32;
        utf16_pos += width;
        for slot in byte_to_utf16
            .iter_mut()
            .take(byte_pos + ch.len_utf8() + 1)
            .skip(byte_pos + 1)
        {
            *slot = utf16_pos;
        }

        match ch {
            '\r' => {
                if let Some((next_byte_pos, '\n')) = chars.peek().copied() {
                    chars.next();
                    byte_to_utf16[next_byte_pos] = utf16_pos;
                    utf16_pos += 1;
                    for slot in byte_to_utf16
                        .iter_mut()
                        .take(next_byte_pos + '\n'.len_utf8() + 1)
                        .skip(next_byte_pos + 1)
                    {
                        *slot = utf16_pos;
                    }
                }
                line_starts.push(line_start);
                line_start = utf16_pos;
            }
            '\n' | '\u{2028}' | '\u{2029}' => {
                line_starts.push(line_start);
                line_start = utf16_pos;
            }
            _ => {}
        }
    }

    byte_to_utf16[text.len()] = utf16_pos;
    line_starts.push(line_start);

    LineMap {
        line_starts,
        byte_to_utf16,
    }
}

pub fn compute_line_starts(text: &str) -> Vec<u32> {
    compute_line_map(text).line_starts
}

pub fn get_line_and_character_of_position(line_starts: &[u32], position: u32) -> LineAndCharacter {
    let line = match line_starts.binary_search(&position) {
        Ok(line) => line,
        Err(insert_at) => insert_at.saturating_sub(1),
    };
    LineAndCharacter {
        line: line as u32,
        character: position - line_starts[line],
    }
}

#[cfg(test)]
mod tests {
    use super::{compute_line_map, compute_line_starts, get_line_and_character_of_position};

    #[test]
    fn line_starts_match_tsc_line_breaks() {
        assert_eq!(
            compute_line_starts("a\r\nb\nc\u{2028}d\u{2029}e"),
            vec![0, 3, 5, 7, 9]
        );
    }

    #[test]
    fn columns_are_utf16_code_units() {
        let map = compute_line_map("a😀b\nc");
        assert_eq!(map.byte_to_utf16[0], 0);
        assert_eq!(map.byte_to_utf16["a".len()], 1);
        assert_eq!(map.byte_to_utf16["a😀".len()], 3);
        assert_eq!(
            get_line_and_character_of_position(&map.line_starts, 4),
            super::LineAndCharacter {
                line: 0,
                character: 4,
            }
        );
    }
}
