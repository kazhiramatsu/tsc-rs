use crate::text::{LineAndCharacter, PositionIndex};

pub type LineMap = PositionIndex;

pub fn compute_line_map(text: &str) -> LineMap {
    PositionIndex::new_static(text)
}

pub fn compute_line_starts(text: &str) -> Vec<u32> {
    let index = compute_line_map(text);
    (0..index.line_count())
        .map(|line| {
            index
                .line_start_utf16(line)
                .expect("line below line_count has a start")
        })
        .collect()
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
#[path = "../tests/unit/line_map/tests.rs"]
mod tests;
