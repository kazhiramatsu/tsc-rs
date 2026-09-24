use std::collections::VecDeque;
use std::fmt;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

const CHANGE_NUMBER_THRESHOLD: usize = 8;
const CHANGE_LENGTH_THRESHOLD_UTF16: u32 = 256;
const MAX_RETAINED_SNAPSHOTS: usize = 8;

#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct DocumentVersion(Arc<str>);

impl DocumentVersion {
    pub fn new(version: impl Into<Arc<str>>) -> Self {
        Self(version.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
struct SnapshotStoreIdentity;

#[derive(Clone)]
struct SnapshotLineage {
    store: Arc<SnapshotStoreIdentity>,
    revision: u64,
}

impl SnapshotLineage {
    fn is_same_store(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.store, &other.store)
    }
}

impl fmt::Debug for SnapshotLineage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SnapshotLineage")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionIndexKind {
    StaticDense,
    PersistentLines,
}

/// Byte ↔ UTF-16 position index of one immutable text: the line starts in
/// both domains, plus every non-ASCII character as a sparse table. Between
/// two non-ASCII characters each byte is one UTF-16 unit, so a position
/// converts by the difference accumulated at the nearest preceding
/// non-ASCII character (a binary search over that table, which is empty for
/// ASCII text) instead of through a table with one entry per byte.
#[derive(Debug)]
struct DensePositionIndex {
    byte_len: u32,
    utf16_len: u32,
    /// The byte (and UTF-16) length of the text's leading ASCII run: below
    /// it both domains coincide and every position is a scalar boundary,
    /// so the hot conversions answer without consulting `wide`.
    ascii_prefix_end: u32,
    /// Every non-ASCII character, in text order.
    wide: Vec<WideCharacter>,
    line_starts_byte: Vec<u32>,
    line_starts_utf16: Vec<u32>,
    /// The line of the last UTF-16 line/character lookup: consecutive
    /// lookups (a printer mapping every node boundary of a source) mostly
    /// stay on one line, which is then answered without a binary search. A
    /// stale hint only costs the search.
    line_hint: std::sync::atomic::AtomicU32,
}

/// One non-ASCII character: where it ends in both domains, and its widths.
#[derive(Clone, Copy, Debug)]
struct WideCharacter {
    byte_end: u32,
    utf16_end: u32,
    byte_len: u8,
    utf16_len: u8,
}

impl DensePositionIndex {
    fn new(text: &str) -> Self {
        let byte_len =
            u32::try_from(text.len()).expect("source text must fit in the u32 position domain");
        let bytes = text.as_bytes();
        let mut wide = Vec::new();
        let mut position = 0usize;
        // The UTF-16 position of `position`.
        let mut utf16_position = 0u32;
        while let Some(offset) = bytes[position..].iter().position(|&byte| byte >= 0x80) {
            let start = position + offset;
            let character = text[start..]
                .chars()
                .next()
                .expect("a non-ASCII lead byte starts a character");
            let byte_end = start + character.len_utf8();
            utf16_position += (start - position) as u32 + character.len_utf16() as u32;
            wide.push(WideCharacter {
                byte_end: byte_end as u32,
                utf16_end: utf16_position,
                byte_len: character.len_utf8() as u8,
                utf16_len: character.len_utf16() as u8,
            });
            position = byte_end;
        }
        let utf16_len = utf16_position + (bytes.len() - position) as u32;
        let ascii_prefix_end = wide
            .first()
            .map_or(byte_len, |first| first.byte_end - u32::from(first.byte_len));
        let mut index = Self {
            byte_len,
            utf16_len,
            ascii_prefix_end,
            wide,
            line_starts_byte: Vec::new(),
            line_starts_utf16: Vec::new(),
            line_hint: std::sync::atomic::AtomicU32::new(0),
        };
        index.line_starts_byte = compute_line_starts_byte(text);
        index.line_starts_utf16 = index
            .line_starts_byte
            .iter()
            .map(|&start| {
                index
                    .byte_to_utf16(start)
                    .expect("a line starts at a scalar boundary")
            })
            .collect();
        index
    }

    const fn utf16_len(&self) -> u32 {
        self.utf16_len
    }

    /// The line containing UTF-16 `position` (already bounds-checked) and
    /// the offset within it: the hinted line when the position lies on it,
    /// otherwise the greatest line start at or before the position.
    fn line_and_character_utf16(&self, position: u32) -> Option<LineAndCharacter> {
        let starts = &self.line_starts_utf16;
        let hint = self.line_hint.load(std::sync::atomic::Ordering::Relaxed) as usize;
        let line = if hint < starts.len()
            && starts[hint] <= position
            && starts.get(hint + 1).is_none_or(|&next| position < next)
        {
            hint
        } else {
            let line = starts
                .partition_point(|&start| start <= position)
                .checked_sub(1)?;
            self.line_hint
                .store(line as u32, std::sync::atomic::Ordering::Relaxed);
            line
        };
        Some(LineAndCharacter {
            line: line as u32,
            character: position - starts[line],
        })
    }

    fn byte_to_utf16(&self, position: u32) -> Option<u32> {
        if position <= self.ascii_prefix_end {
            return Some(position);
        }
        if position > self.byte_len {
            return None;
        }
        // The wide characters ending at or before `position` carry the
        // accumulated difference; a position inside the next one is not a
        // scalar boundary.
        let index = self
            .wide
            .partition_point(|character| character.byte_end <= position);
        if let Some(next) = self.wide.get(index) {
            if next.byte_end - u32::from(next.byte_len) < position {
                return None;
            }
        }
        let delta = index.checked_sub(1).map_or(0, |previous| {
            let character = self.wide[previous];
            i64::from(character.utf16_end) - i64::from(character.byte_end)
        });
        Some((i64::from(position) + delta) as u32)
    }

    fn utf16_to_byte(&self, position: u32) -> Option<u32> {
        if position <= self.ascii_prefix_end {
            return Some(position);
        }
        if position > self.utf16_len {
            return None;
        }
        let index = self
            .wide
            .partition_point(|character| character.utf16_end <= position);
        if let Some(next) = self.wide.get(index) {
            // Inside the next wide character: the low surrogate of a pair.
            if next.utf16_end - u32::from(next.utf16_len) < position {
                return None;
            }
        }
        let delta = index.checked_sub(1).map_or(0, |previous| {
            let character = self.wide[previous];
            i64::from(character.byte_end) - i64::from(character.utf16_end)
        });
        Some((i64::from(position) + delta) as u32)
    }
}

/// Line starts in bytes: one after every `\n`, `\r\n`, lone `\r`, U+2028
/// and U+2029, found with a three-byte scan (the UTF-8 lead byte of both
/// separators is 0xE2, which never occurs as a continuation byte).
fn compute_line_starts_byte(text: &str) -> Vec<u32> {
    let bytes = text.as_bytes();
    let mut starts = vec![0u32];
    let mut position = 0usize;
    while let Some(offset) = memchr::memchr3(b'\n', b'\r', 0xE2, &bytes[position..]) {
        let at = position + offset;
        let end = match bytes[at] {
            b'\r' if bytes.get(at + 1) == Some(&b'\n') => at + 2,
            b'\r' | b'\n' => at + 1,
            _ if matches!(bytes.get(at + 1..at + 3), Some([0x80, 0xA8 | 0xA9])) => at + 3,
            _ => {
                position = at + 1;
                continue;
            }
        };
        starts.push(end as u32);
        position = end;
    }
    starts
}

#[derive(Clone, Debug)]
struct LineText {
    storage: Arc<str>,
    range: Range<usize>,
    utf16_len: u32,
}

impl LineText {
    fn new(storage: Arc<str>, range: Range<usize>) -> Self {
        let text = &storage[range.clone()];
        let utf16_len = u32::try_from(text.encode_utf16().count())
            .expect("source line must fit in the u32 position domain");
        Self {
            storage,
            range,
            utf16_len,
        }
    }

    fn text(&self) -> &str {
        &self.storage[self.range.clone()]
    }

    fn byte_len(&self) -> u32 {
        u32::try_from(self.range.len()).expect("source line must fit in u32")
    }
}

#[derive(Debug)]
struct LineNode {
    left: Option<Arc<LineNode>>,
    line: LineText,
    right: Option<Arc<LineNode>>,
    priority: u64,
    byte_len: u32,
    utf16_len: u32,
    line_count: u32,
}

impl LineNode {
    fn new(
        left: Option<Arc<Self>>,
        line: LineText,
        right: Option<Arc<Self>>,
        priority: u64,
    ) -> Arc<Self> {
        let byte_len = node_byte_len(&left)
            .checked_add(line.byte_len())
            .and_then(|length| length.checked_add(node_byte_len(&right)))
            .expect("source text must fit in the u32 position domain");
        let utf16_len = node_utf16_len(&left)
            .checked_add(line.utf16_len)
            .and_then(|length| length.checked_add(node_utf16_len(&right)))
            .expect("source text must fit in the u32 position domain");
        let line_count = node_line_count(&left)
            .checked_add(1)
            .and_then(|count| count.checked_add(node_line_count(&right)))
            .expect("source line count must fit in the u32 position domain");
        Arc::new(Self {
            left,
            line,
            right,
            priority,
            byte_len,
            utf16_len,
            line_count,
        })
    }

    fn with_left(self: &Arc<Self>, left: Option<Arc<Self>>) -> Arc<Self> {
        Self::new(left, self.line.clone(), self.right.clone(), self.priority)
    }

    fn with_right(self: &Arc<Self>, right: Option<Arc<Self>>) -> Arc<Self> {
        Self::new(self.left.clone(), self.line.clone(), right, self.priority)
    }
}

fn node_byte_len(node: &Option<Arc<LineNode>>) -> u32 {
    node.as_ref().map_or(0, |node| node.byte_len)
}

fn node_utf16_len(node: &Option<Arc<LineNode>>) -> u32 {
    node.as_ref().map_or(0, |node| node.utf16_len)
}

fn node_line_count(node: &Option<Arc<LineNode>>) -> u32 {
    node.as_ref().map_or(0, |node| node.line_count)
}

fn priority(sequence: u64) -> u64 {
    let mut value = sequence.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn merge_line_nodes(
    left: Option<Arc<LineNode>>,
    right: Option<Arc<LineNode>>,
) -> Option<Arc<LineNode>> {
    match (left, right) {
        (None, right) => right,
        (left, None) => left,
        (Some(left), Some(right)) if left.priority >= right.priority => {
            let merged = merge_line_nodes(left.right.clone(), Some(right));
            Some(left.with_right(merged))
        }
        (Some(left), Some(right)) => {
            let merged = merge_line_nodes(Some(left), right.left.clone());
            Some(right.with_left(merged))
        }
    }
}

fn split_line_nodes(
    root: Option<Arc<LineNode>>,
    lines_on_left: u32,
) -> (Option<Arc<LineNode>>, Option<Arc<LineNode>>) {
    let Some(root) = root else {
        return (None, None);
    };
    let left_count = node_line_count(&root.left);
    if lines_on_left <= left_count {
        let (left, new_left) = split_line_nodes(root.left.clone(), lines_on_left);
        (left, Some(root.with_left(new_left)))
    } else {
        let (new_right, right) = split_line_nodes(
            root.right.clone(),
            lines_on_left.saturating_sub(left_count + 1),
        );
        (Some(root.with_right(new_right)), right)
    }
}

#[derive(Clone, Debug)]
struct PersistentLineTree {
    root: Arc<LineNode>,
    next_line_sequence: u64,
}

impl PersistentLineTree {
    fn from_text(text: Arc<str>) -> Self {
        let ranges = line_ranges(&text);
        let mut root = None;
        let mut next_line_sequence = 0u64;
        for range in ranges {
            let line = LineText::new(Arc::clone(&text), range);
            let node = LineNode::new(None, line, None, priority(next_line_sequence));
            next_line_sequence = next_line_sequence.wrapping_add(1);
            root = merge_line_nodes(root, Some(node));
        }
        Self {
            root: root.expect("line splitting always produces at least one line"),
            next_line_sequence,
        }
    }

    fn byte_len(&self) -> u32 {
        self.root.byte_len
    }

    fn utf16_len(&self) -> u32 {
        self.root.utf16_len
    }

    fn line_count(&self) -> u32 {
        self.root.line_count
    }

    fn line(&self, line: u32) -> Option<&LineText> {
        let mut node = &self.root;
        let mut relative = line;
        loop {
            let left_count = node_line_count(&node.left);
            if relative < left_count {
                node = node.left.as_ref()?;
            } else if relative == left_count {
                return Some(&node.line);
            } else {
                relative -= left_count + 1;
                node = node.right.as_ref()?;
            }
        }
    }

    fn line_start_byte(&self, line: u32) -> Option<u32> {
        self.line_start(line, true)
    }

    fn line_start_utf16(&self, line: u32) -> Option<u32> {
        self.line_start(line, false)
    }

    fn line_start(&self, line: u32, bytes: bool) -> Option<u32> {
        let mut node = &self.root;
        let mut relative = line;
        let mut position = 0u32;
        loop {
            let left_count = node_line_count(&node.left);
            if relative < left_count {
                node = node.left.as_ref()?;
            } else {
                position = position.checked_add(if bytes {
                    node_byte_len(&node.left)
                } else {
                    node_utf16_len(&node.left)
                })?;
                if relative == left_count {
                    return Some(position);
                }
                position = position.checked_add(if bytes {
                    node.line.byte_len()
                } else {
                    node.line.utf16_len
                })?;
                relative -= left_count + 1;
                node = node.right.as_ref()?;
            }
        }
    }

    fn line_for_position(&self, position: u32, bytes: bool) -> Option<u32> {
        let total = if bytes {
            self.byte_len()
        } else {
            self.utf16_len()
        };
        if position > total {
            return None;
        }
        let mut low = 0u32;
        let mut high = self.line_count();
        while low < high {
            let middle = low + (high - low) / 2;
            let start = self.line_start(middle, bytes)?;
            if start <= position {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        Some(low.saturating_sub(1))
    }

    fn byte_to_utf16(&self, position: u32) -> Option<u32> {
        let line = self.line_for_position(position, true)?;
        let byte_start = self.line_start_byte(line)?;
        let utf16_start = self.line_start_utf16(line)?;
        let relative = usize::try_from(position - byte_start).ok()?;
        let text = self.line(line)?.text();
        let relative_utf16 = utf16_offset_at_byte(text, relative)?;
        utf16_start.checked_add(relative_utf16)
    }

    fn utf16_to_byte(&self, position: u32) -> Option<u32> {
        let line = self.line_for_position(position, false)?;
        let byte_start = self.line_start_byte(line)?;
        let utf16_start = self.line_start_utf16(line)?;
        let relative = position - utf16_start;
        let text = self.line(line)?.text();
        let relative_byte = byte_offset_at_utf16(text, relative)?;
        byte_start.checked_add(relative_byte)
    }

    fn replace(&self, byte_range: Range<u32>, inserted_text: &str) -> Self {
        debug_assert!(byte_range.start <= byte_range.end);
        debug_assert!(byte_range.end <= self.byte_len());
        let edited_start_line = self
            .line_for_position(byte_range.start, true)
            .expect("validated byte edit start");
        let edited_end_line = self
            .line_for_position(byte_range.end, true)
            .expect("validated byte edit end");
        let start_line_start = self
            .line_start_byte(edited_start_line)
            .expect("validated start line");
        let end_line_start = self
            .line_start_byte(edited_end_line)
            .expect("validated end line");
        let start_text = self
            .line(edited_start_line)
            .expect("validated start line")
            .text();
        let end_text = self
            .line(edited_end_line)
            .expect("validated end line")
            .text();
        let prefix_end = usize::try_from(byte_range.start - start_line_start)
            .expect("relative byte position fits usize");
        let suffix_start = usize::try_from(byte_range.end - end_line_start)
            .expect("relative byte position fits usize");

        // Include one untouched neighbor on each side. An edit at a line
        // boundary can create or split CRLF (for example inserting LF after
        // a lone CR), so those adjacent leaves are semantically affected even
        // when the edit range itself is empty. Re-splitting this bounded
        // window preserves the invariant that CRLF is never split between
        // persistent leaves while every farther subtree remains shared.
        let replacement_start_line = edited_start_line.saturating_sub(1);
        let replacement_end_line = edited_end_line.saturating_add(1).min(self.line_count() - 1);
        let mut replacement = String::with_capacity(
            prefix_end + inserted_text.len() + end_text.len().saturating_sub(suffix_start),
        );
        for line in replacement_start_line..edited_start_line {
            replacement.push_str(self.line(line).expect("neighbor line exists").text());
        }
        replacement.push_str(&start_text[..prefix_end]);
        replacement.push_str(inserted_text);
        replacement.push_str(&end_text[suffix_start..]);
        for line in edited_end_line + 1..=replacement_end_line {
            replacement.push_str(self.line(line).expect("neighbor line exists").text());
        }
        let replacement: Arc<str> = Arc::from(replacement);

        let (before, replaced_and_after) =
            split_line_nodes(Some(Arc::clone(&self.root)), replacement_start_line);
        let replaced_line_count = replacement_end_line - replacement_start_line + 1;
        let (_, after) = split_line_nodes(replaced_and_after, replaced_line_count);

        let mut inserted_root = None;
        let mut next_line_sequence = self.next_line_sequence;
        let mut replacement_ranges = line_ranges(&replacement);
        if after.is_some()
            && replacement_ranges
                .last()
                .is_some_and(|range| range.is_empty() && range.start == replacement.len())
        {
            replacement_ranges.pop();
        }
        for range in replacement_ranges {
            let line = LineText::new(Arc::clone(&replacement), range);
            let node = LineNode::new(None, line, None, priority(next_line_sequence));
            next_line_sequence = next_line_sequence.wrapping_add(1);
            inserted_root = merge_line_nodes(inserted_root, Some(node));
        }
        let root = merge_line_nodes(merge_line_nodes(before, inserted_root), after)
            .expect("replacement always leaves at least one line");
        Self {
            root,
            next_line_sequence,
        }
    }

    fn materialize(&self) -> Arc<str> {
        let mut text = String::with_capacity(self.byte_len() as usize);
        append_node_text(&self.root, &mut text);
        Arc::from(text)
    }
}

fn append_node_text(node: &Arc<LineNode>, output: &mut String) {
    if let Some(left) = &node.left {
        append_node_text(left, output);
    }
    output.push_str(node.line.text());
    if let Some(right) = &node.right {
        append_node_text(right, output);
    }
}

#[derive(Debug)]
enum PositionIndexData {
    StaticDense(DensePositionIndex),
    /// Built from the shared text on first use: a snapshot's index is
    /// rarely consulted before diagnostics are rendered or output is
    /// printed, and a large source pays for it only then.
    LazyDense {
        text: Arc<str>,
        dense: OnceLock<DensePositionIndex>,
    },
    PersistentLines(PersistentLineTree),
}

#[derive(Debug)]
pub struct PositionIndex {
    data: PositionIndexData,
    byte_len: u32,
}

impl PositionIndex {
    pub fn new_static(text: &str) -> Self {
        Self {
            byte_len: u32::try_from(text.len()).expect("source text must fit in u32"),
            data: PositionIndexData::StaticDense(DensePositionIndex::new(text)),
        }
    }

    /// A static index over `text` whose tables are built on first use.
    pub fn new_lazy(text: Arc<str>) -> Self {
        Self {
            byte_len: u32::try_from(text.len()).expect("source text must fit in u32"),
            data: PositionIndexData::LazyDense {
                text,
                dense: OnceLock::new(),
            },
        }
    }

    fn from_persistent(tree: PersistentLineTree) -> Self {
        Self {
            byte_len: tree.byte_len(),
            data: PositionIndexData::PersistentLines(tree),
        }
    }

    /// The dense tables of a static index (built now if deferred); a
    /// persistent index has none.
    fn dense(&self) -> Option<&DensePositionIndex> {
        match &self.data {
            PositionIndexData::StaticDense(dense) => Some(dense),
            PositionIndexData::LazyDense { text, dense } => {
                Some(dense.get_or_init(|| DensePositionIndex::new(text)))
            }
            PositionIndexData::PersistentLines(_) => None,
        }
    }

    pub const fn kind(&self) -> PositionIndexKind {
        match self.data {
            PositionIndexData::StaticDense(_) | PositionIndexData::LazyDense { .. } => {
                PositionIndexKind::StaticDense
            }
            PositionIndexData::PersistentLines(_) => PositionIndexKind::PersistentLines,
        }
    }

    pub const fn byte_len(&self) -> u32 {
        self.byte_len
    }

    pub fn utf16_len(&self) -> u32 {
        match &self.data {
            PositionIndexData::PersistentLines(tree) => tree.utf16_len(),
            _ => self.dense().expect("static index").utf16_len(),
        }
    }

    pub fn line_count(&self) -> u32 {
        match &self.data {
            PositionIndexData::PersistentLines(tree) => tree.line_count(),
            _ => u32::try_from(self.dense().expect("static index").line_starts_byte.len())
                .expect("source line count must fit in u32"),
        }
    }

    pub fn byte_to_utf16(&self, position: u32) -> Option<u32> {
        match &self.data {
            PositionIndexData::PersistentLines(tree) => tree.byte_to_utf16(position),
            _ => self.dense()?.byte_to_utf16(position),
        }
    }

    pub fn utf16_to_byte(&self, position: u32) -> Option<u32> {
        match &self.data {
            PositionIndexData::PersistentLines(tree) => tree.utf16_to_byte(position),
            _ => self.dense()?.utf16_to_byte(position),
        }
    }

    /// Converts a UTF-16 offset relative to a byte-domain scalar boundary
    /// back into an absolute byte offset. Both the base and the target must
    /// be exact Unicode scalar boundaries.
    pub fn byte_offset_from_utf16_delta(&self, base_byte: u32, delta_utf16: u32) -> Option<u32> {
        let base_utf16 = self.byte_to_utf16(base_byte)?;
        self.utf16_to_byte(base_utf16.checked_add(delta_utf16)?)
    }

    pub fn line_start_byte(&self, line: u32) -> Option<u32> {
        match &self.data {
            PositionIndexData::PersistentLines(tree) => tree.line_start_byte(line),
            _ => self.dense()?.line_starts_byte.get(line as usize).copied(),
        }
    }

    pub fn line_start_utf16(&self, line: u32) -> Option<u32> {
        match &self.data {
            PositionIndexData::PersistentLines(tree) => tree.line_start_utf16(line),
            _ => self.dense()?.line_starts_utf16.get(line as usize).copied(),
        }
    }

    pub fn line_and_character_utf16(&self, position: u32) -> Option<LineAndCharacter> {
        if position > self.utf16_len() {
            return None;
        }
        if let Some(dense) = self.dense() {
            return dense.line_and_character_utf16(position);
        }
        let line = greatest_line_start(self.line_count(), position, |line| {
            self.line_start_utf16(line)
        })?;
        Some(LineAndCharacter {
            line,
            character: position - self.line_start_utf16(line)?,
        })
    }

    pub fn line_and_character_byte(&self, position: u32) -> Option<LineAndCharacter> {
        let utf16 = self.byte_to_utf16(position)?;
        self.line_and_character_utf16(utf16)
    }
}

fn greatest_line_start(
    line_count: u32,
    position: u32,
    mut start: impl FnMut(u32) -> Option<u32>,
) -> Option<u32> {
    let mut low = 0u32;
    let mut high = line_count;
    while low < high {
        let middle = low + (high - low) / 2;
        if start(middle)? <= position {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    Some(low.saturating_sub(1))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineAndCharacter {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug)]
pub struct TextSnapshot {
    document_version: DocumentVersion,
    lineage: SnapshotLineage,
    text: Arc<str>,
    positions: Arc<PositionIndex>,
}

impl TextSnapshot {
    pub fn new(text: impl Into<String>, document_version: DocumentVersion) -> Arc<Self> {
        Self::from_shared_text(Arc::from(text.into()), document_version)
    }

    pub fn from_shared_text(text: Arc<str>, document_version: DocumentVersion) -> Arc<Self> {
        let positions = Arc::new(PositionIndex::new_lazy(Arc::clone(&text)));
        Arc::new(Self {
            document_version,
            lineage: SnapshotLineage {
                store: Arc::new(SnapshotStoreIdentity),
                revision: 0,
            },
            text,
            positions,
        })
    }

    fn from_store(
        text: Arc<str>,
        document_version: DocumentVersion,
        store: Arc<SnapshotStoreIdentity>,
        revision: u64,
        positions: PositionIndex,
    ) -> Arc<Self> {
        debug_assert_eq!(positions.byte_len() as usize, text.len());
        Arc::new(Self {
            document_version,
            lineage: SnapshotLineage { store, revision },
            text,
            positions: Arc::new(positions),
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn shared_text(&self) -> Arc<str> {
        Arc::clone(&self.text)
    }

    pub fn document_version(&self) -> &DocumentVersion {
        &self.document_version
    }

    pub fn positions(&self) -> &PositionIndex {
        &self.positions
    }

    pub fn shared_positions(&self) -> Arc<PositionIndex> {
        Arc::clone(&self.positions)
    }
}

impl PartialEq for TextSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.document_version == other.document_version && self.text == other.text
    }
}

impl Eq for TextSnapshot {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Utf16TextSpan {
    pub start: u32,
    pub length: u32,
}

impl Utf16TextSpan {
    pub const fn new(start: u32, length: u32) -> Self {
        Self { start, length }
    }

    pub fn end(self) -> Option<u32> {
        self.start.checked_add(self.length)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteTextSpan {
    pub start: u32,
    pub length: u32,
}

impl ByteTextSpan {
    pub const fn new(start: u32, length: u32) -> Self {
        Self { start, length }
    }

    pub fn end(self) -> Option<u32> {
        self.start.checked_add(self.length)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Utf16TextChangeRange {
    pub span: Utf16TextSpan,
    pub new_length: u32,
}

impl Utf16TextChangeRange {
    pub const UNCHANGED: Self = Self {
        span: Utf16TextSpan::new(0, 0),
        new_length: 0,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteTextChangeRange {
    pub span: ByteTextSpan,
    pub new_length: u32,
}

impl ByteTextChangeRange {
    pub const UNCHANGED: Self = Self {
        span: ByteTextSpan::new(0, 0),
        new_length: 0,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionUnit {
    Byte,
    Utf16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextEditError {
    RangeOverflow {
        unit: PositionUnit,
    },
    PositionOutOfBounds {
        unit: PositionUnit,
        position: u32,
        length: u32,
    },
    InvalidScalarBoundary {
        unit: PositionUnit,
        position: u32,
    },
    TextTooLong,
}

impl fmt::Display for TextEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RangeOverflow { unit } => write!(formatter, "{unit:?} edit range overflows"),
            Self::PositionOutOfBounds {
                unit,
                position,
                length,
            } => write!(
                formatter,
                "{unit:?} position {position} is outside text length {length}"
            ),
            Self::InvalidScalarBoundary { unit, position } => write!(
                formatter,
                "{unit:?} position {position} is not a Unicode scalar boundary"
            ),
            Self::TextTooLong => formatter.write_str("edited text exceeds the u32 position domain"),
        }
    }
}

impl std::error::Error for TextEditError {}

#[derive(Clone, Debug)]
pub struct TextEditOutcome {
    byte_change: ByteTextChangeRange,
    utf16_change: Utf16TextChangeRange,
    published_snapshot: Option<Arc<TextSnapshot>>,
}

impl TextEditOutcome {
    pub const fn byte_change(&self) -> ByteTextChangeRange {
        self.byte_change
    }

    pub const fn utf16_change(&self) -> Utf16TextChangeRange {
        self.utf16_change
    }

    pub fn published_snapshot(&self) -> Option<&Arc<TextSnapshot>> {
        self.published_snapshot.as_ref()
    }
}

#[derive(Debug)]
struct PublishedSnapshot {
    snapshot: Arc<TextSnapshot>,
    utf16_change_from_previous: Option<Utf16TextChangeRange>,
    byte_change_from_previous: Option<ByteTextChangeRange>,
}

#[derive(Debug)]
pub struct VersionedTextStore {
    store: Arc<SnapshotStoreIdentity>,
    working_tree: PersistentLineTree,
    working_positions: Arc<PositionIndex>,
    current: Arc<TextSnapshot>,
    pending_document_version: DocumentVersion,
    pending_utf16_changes: Vec<Utf16TextChangeRange>,
    pending_byte_changes: Vec<ByteTextChangeRange>,
    history: VecDeque<PublishedSnapshot>,
}

impl VersionedTextStore {
    pub fn new(text: impl Into<String>, document_version: DocumentVersion) -> Self {
        let text: Arc<str> = Arc::from(text.into());
        let store = Arc::new(SnapshotStoreIdentity);
        let positions = PositionIndex::new_static(&text);
        let current = TextSnapshot::from_store(
            Arc::clone(&text),
            document_version.clone(),
            Arc::clone(&store),
            0,
            positions,
        );
        let working_tree = PersistentLineTree::from_text(text);
        let working_positions = Arc::new(PositionIndex::from_persistent(working_tree.clone()));
        let mut history = VecDeque::new();
        history.push_back(PublishedSnapshot {
            snapshot: Arc::clone(&current),
            utf16_change_from_previous: None,
            byte_change_from_previous: None,
        });
        Self {
            store,
            working_tree,
            working_positions,
            current,
            pending_document_version: document_version,
            pending_utf16_changes: Vec::new(),
            pending_byte_changes: Vec::new(),
            history,
        }
    }

    pub fn current_snapshot(&self) -> Arc<TextSnapshot> {
        Arc::clone(&self.current)
    }

    pub fn snapshot(&mut self) -> Arc<TextSnapshot> {
        self.publish_pending();
        Arc::clone(&self.current)
    }

    pub fn pending_edit_count(&self) -> usize {
        self.pending_utf16_changes.len()
    }

    pub fn retained_snapshot_count(&self) -> usize {
        self.history.len()
    }

    pub fn edit_utf16(
        &mut self,
        span: Utf16TextSpan,
        inserted_text: impl AsRef<str>,
        document_version: DocumentVersion,
    ) -> Result<TextEditOutcome, TextEditError> {
        let end = span.end().ok_or(TextEditError::RangeOverflow {
            unit: PositionUnit::Utf16,
        })?;
        if end > self.working_positions.utf16_len() {
            return Err(TextEditError::PositionOutOfBounds {
                unit: PositionUnit::Utf16,
                position: end,
                length: self.working_positions.utf16_len(),
            });
        }
        let byte_start = self.working_positions.utf16_to_byte(span.start).ok_or(
            TextEditError::InvalidScalarBoundary {
                unit: PositionUnit::Utf16,
                position: span.start,
            },
        )?;
        let byte_end = self.working_positions.utf16_to_byte(end).ok_or(
            TextEditError::InvalidScalarBoundary {
                unit: PositionUnit::Utf16,
                position: end,
            },
        )?;
        self.apply_edit(
            ByteTextSpan::new(byte_start, byte_end - byte_start),
            span,
            inserted_text.as_ref(),
            document_version,
        )
    }

    pub fn edit_bytes(
        &mut self,
        span: ByteTextSpan,
        inserted_text: impl AsRef<str>,
        document_version: DocumentVersion,
    ) -> Result<TextEditOutcome, TextEditError> {
        let end = span.end().ok_or(TextEditError::RangeOverflow {
            unit: PositionUnit::Byte,
        })?;
        if end > self.working_positions.byte_len() {
            return Err(TextEditError::PositionOutOfBounds {
                unit: PositionUnit::Byte,
                position: end,
                length: self.working_positions.byte_len(),
            });
        }
        let utf16_start = self.working_positions.byte_to_utf16(span.start).ok_or(
            TextEditError::InvalidScalarBoundary {
                unit: PositionUnit::Byte,
                position: span.start,
            },
        )?;
        let utf16_end = self.working_positions.byte_to_utf16(end).ok_or(
            TextEditError::InvalidScalarBoundary {
                unit: PositionUnit::Byte,
                position: end,
            },
        )?;
        self.apply_edit(
            span,
            Utf16TextSpan::new(utf16_start, utf16_end - utf16_start),
            inserted_text.as_ref(),
            document_version,
        )
    }

    fn apply_edit(
        &mut self,
        byte_span: ByteTextSpan,
        utf16_span: Utf16TextSpan,
        inserted_text: &str,
        document_version: DocumentVersion,
    ) -> Result<TextEditOutcome, TextEditError> {
        let inserted_byte_len =
            u32::try_from(inserted_text.len()).map_err(|_| TextEditError::TextTooLong)?;
        let inserted_utf16_len = u32::try_from(inserted_text.encode_utf16().count())
            .map_err(|_| TextEditError::TextTooLong)?;
        let new_byte_len = self
            .working_positions
            .byte_len()
            .checked_sub(byte_span.length)
            .and_then(|length| length.checked_add(inserted_byte_len))
            .ok_or(TextEditError::TextTooLong)?;
        let new_utf16_len = self
            .working_positions
            .utf16_len()
            .checked_sub(utf16_span.length)
            .and_then(|length| length.checked_add(inserted_utf16_len))
            .ok_or(TextEditError::TextTooLong)?;

        self.working_tree = self.working_tree.replace(
            byte_span.start..byte_span.start + byte_span.length,
            inserted_text,
        );
        debug_assert_eq!(self.working_tree.byte_len(), new_byte_len);
        debug_assert_eq!(self.working_tree.utf16_len(), new_utf16_len);
        self.working_positions =
            Arc::new(PositionIndex::from_persistent(self.working_tree.clone()));
        self.pending_document_version = document_version;
        let byte_change = ByteTextChangeRange {
            span: byte_span,
            new_length: inserted_byte_len,
        };
        let utf16_change = Utf16TextChangeRange {
            span: utf16_span,
            new_length: inserted_utf16_len,
        };
        self.pending_byte_changes.push(byte_change);
        self.pending_utf16_changes.push(utf16_change);

        let should_publish = self.pending_utf16_changes.len() > CHANGE_NUMBER_THRESHOLD
            || utf16_span.length > CHANGE_LENGTH_THRESHOLD_UTF16
            || inserted_utf16_len > CHANGE_LENGTH_THRESHOLD_UTF16;
        let published_snapshot = should_publish.then(|| {
            self.publish_pending();
            Arc::clone(&self.current)
        });
        Ok(TextEditOutcome {
            byte_change,
            utf16_change,
            published_snapshot,
        })
    }

    fn publish_pending(&mut self) {
        if self.pending_utf16_changes.is_empty() {
            return;
        }
        let utf16_change = collapse_utf16_changes(&self.pending_utf16_changes);
        let byte_change = collapse_byte_changes(&self.pending_byte_changes);
        let text = self.working_tree.materialize();
        let revision = self.current.lineage.revision + 1;
        let positions = PositionIndex::from_persistent(self.working_tree.clone());
        let snapshot = TextSnapshot::from_store(
            text,
            self.pending_document_version.clone(),
            Arc::clone(&self.store),
            revision,
            positions,
        );
        self.history.push_back(PublishedSnapshot {
            snapshot: Arc::clone(&snapshot),
            utf16_change_from_previous: Some(utf16_change),
            byte_change_from_previous: Some(byte_change),
        });
        while self.history.len() > MAX_RETAINED_SNAPSHOTS {
            self.history.pop_front();
        }
        self.current = snapshot;
        self.pending_utf16_changes.clear();
        self.pending_byte_changes.clear();
    }

    pub fn utf16_change_range(
        &self,
        old_snapshot: &TextSnapshot,
        new_snapshot: &TextSnapshot,
    ) -> Option<Utf16TextChangeRange> {
        self.change_range_indices(old_snapshot, new_snapshot)
            .map(|(old, new)| {
                collapse_utf16_changes(
                    &(old + 1..=new)
                        .map(|index| {
                            self.history[index]
                                .utf16_change_from_previous
                                .expect("a non-root history entry has a change")
                        })
                        .collect::<Vec<_>>(),
                )
            })
    }

    pub fn byte_change_range(
        &self,
        old_snapshot: &TextSnapshot,
        new_snapshot: &TextSnapshot,
    ) -> Option<ByteTextChangeRange> {
        self.change_range_indices(old_snapshot, new_snapshot)
            .map(|(old, new)| {
                collapse_byte_changes(
                    &(old + 1..=new)
                        .map(|index| {
                            self.history[index]
                                .byte_change_from_previous
                                .expect("a non-root history entry has a change")
                        })
                        .collect::<Vec<_>>(),
                )
            })
    }

    fn change_range_indices(
        &self,
        old_snapshot: &TextSnapshot,
        new_snapshot: &TextSnapshot,
    ) -> Option<(usize, usize)> {
        if !old_snapshot.lineage.is_same_store(&new_snapshot.lineage)
            || !Arc::ptr_eq(&old_snapshot.lineage.store, &self.store)
            || old_snapshot.lineage.revision > new_snapshot.lineage.revision
        {
            return None;
        }
        let old = self.history.iter().position(|entry| {
            entry.snapshot.lineage.revision == old_snapshot.lineage.revision
                && Arc::ptr_eq(&entry.snapshot.lineage.store, &old_snapshot.lineage.store)
        })?;
        let new = self.history.iter().position(|entry| {
            entry.snapshot.lineage.revision == new_snapshot.lineage.revision
                && Arc::ptr_eq(&entry.snapshot.lineage.store, &new_snapshot.lineage.store)
        })?;
        (old <= new).then_some((old, new))
    }
}

pub fn collapse_utf16_changes(changes: &[Utf16TextChangeRange]) -> Utf16TextChangeRange {
    let (start, length, new_length) = collapse_changes(
        changes
            .iter()
            .map(|change| (change.span.start, change.span.length, change.new_length)),
    );
    Utf16TextChangeRange {
        span: Utf16TextSpan::new(start, length),
        new_length,
    }
}

pub fn collapse_byte_changes(changes: &[ByteTextChangeRange]) -> ByteTextChangeRange {
    let (start, length, new_length) = collapse_changes(
        changes
            .iter()
            .map(|change| (change.span.start, change.span.length, change.new_length)),
    );
    ByteTextChangeRange {
        span: ByteTextSpan::new(start, length),
        new_length,
    }
}

fn collapse_changes(changes: impl IntoIterator<Item = (u32, u32, u32)>) -> (u32, u32, u32) {
    let mut changes = changes.into_iter();
    let Some((start, length, new_length)) = changes.next() else {
        return (0, 0, 0);
    };
    let mut old_start = i64::from(start);
    let mut old_end = old_start + i64::from(length);
    let mut new_end = old_start + i64::from(new_length);
    for (start, length, new_length) in changes {
        let next_old_start = i64::from(start);
        let next_old_end = next_old_start + i64::from(length);
        let next_new_end = next_old_start + i64::from(new_length);
        let previous_old_end = old_end;
        let previous_new_end = new_end;
        old_start = old_start.min(next_old_start);
        old_end = previous_old_end.max(previous_old_end + (next_old_end - previous_new_end));
        new_end = next_new_end.max(next_new_end + (previous_new_end - next_old_end));
    }
    (
        u32::try_from(old_start).expect("collapsed change start remains in u32"),
        u32::try_from(old_end - old_start).expect("collapsed old length remains in u32"),
        u32::try_from(new_end - old_start).expect("collapsed new length remains in u32"),
    )
}

fn line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut line_start = 0usize;
    let mut characters = text.char_indices().peekable();
    while let Some((byte_position, character)) = characters.next() {
        let line_end = match character {
            '\r' => {
                if let Some((next_byte, '\n')) = characters.peek().copied() {
                    characters.next();
                    next_byte + 1
                } else {
                    byte_position + 1
                }
            }
            '\n' => byte_position + 1,
            '\u{2028}' | '\u{2029}' => byte_position + character.len_utf8(),
            _ => continue,
        };
        ranges.push(line_start..line_end);
        line_start = line_end;
    }
    ranges.push(line_start..text.len());
    ranges
}

fn utf16_offset_at_byte(text: &str, byte_position: usize) -> Option<u32> {
    if byte_position > text.len() || !text.is_char_boundary(byte_position) {
        return None;
    }
    u32::try_from(text[..byte_position].encode_utf16().count()).ok()
}

fn byte_offset_at_utf16(text: &str, utf16_position: u32) -> Option<u32> {
    let mut current = 0u32;
    for (byte_position, character) in text.char_indices() {
        if current == utf16_position {
            return u32::try_from(byte_position).ok();
        }
        current = current.checked_add(character.len_utf16() as u32)?;
        if current > utf16_position {
            return None;
        }
    }
    (current == utf16_position)
        .then(|| u32::try_from(text.len()).ok())
        .flatten()
}

#[cfg(test)]
#[path = "../tests/unit/text/tests.rs"]
mod tests;

#[cfg(test)]
mod sparse_position_index_tests {
    use super::PositionIndex;

    /// Byte→UTF-16 and UTF-16→byte tables plus line starts, computed
    /// character by character (the former dense construction).
    #[allow(clippy::type_complexity)]
    fn reference(text: &str) -> (Vec<Option<u32>>, Vec<Option<u32>>, Vec<u32>, Vec<u32>) {
        let mut byte_to_utf16 = vec![None; text.len() + 1];
        let mut utf16_to_byte = vec![None; text.encode_utf16().count() + 1];
        byte_to_utf16[0] = Some(0);
        utf16_to_byte[0] = Some(0);
        let mut utf16 = 0u32;
        for (byte, character) in text.char_indices() {
            let byte_end = byte + character.len_utf8();
            utf16 += character.len_utf16() as u32;
            byte_to_utf16[byte_end] = Some(utf16);
            utf16_to_byte[utf16 as usize] = Some(byte_end as u32);
        }
        let mut line_bytes = vec![0u32];
        let mut line_utf16 = vec![0u32];
        let mut characters = text.char_indices().peekable();
        let mut position = 0u32;
        while let Some((byte, character)) = characters.next() {
            position += character.len_utf16() as u32;
            match character {
                '\r' => {
                    let mut end = byte + 1;
                    if let Some((next, '\n')) = characters.peek().copied() {
                        characters.next();
                        position += 1;
                        end = next + 1;
                    }
                    line_bytes.push(end as u32);
                    line_utf16.push(position);
                }
                '\n' | '\u{2028}' | '\u{2029}' => {
                    line_bytes.push((byte + character.len_utf8()) as u32);
                    line_utf16.push(position);
                }
                _ => {}
            }
        }
        (byte_to_utf16, utf16_to_byte, line_bytes, line_utf16)
    }

    #[test]
    fn sparse_index_matches_the_character_walk() {
        for text in [
            "",
            "abc",
            "a\nb\r\nc\rd",
            "é😀日本\u{2028}x\u{2029}y\n",
            "// UTF-16境界😀; generated\ninterface A { x: 1 }\r\n😀\r\né",
            "\u{2028}",
            "😀",
            "\u{e2}\u{80}",
        ] {
            let index = PositionIndex::new_static(text);
            let (byte_to_utf16, utf16_to_byte, line_bytes, line_utf16) = reference(text);
            assert_eq!(index.byte_len() as usize, text.len(), "{text:?}");
            assert_eq!(
                index.utf16_len() as usize,
                text.encode_utf16().count(),
                "{text:?}"
            );
            for (position, expected) in byte_to_utf16.iter().enumerate() {
                assert_eq!(
                    index.byte_to_utf16(position as u32),
                    *expected,
                    "byte {position} of {text:?}"
                );
            }
            assert_eq!(index.byte_to_utf16(text.len() as u32 + 1), None, "{text:?}");
            for (position, expected) in utf16_to_byte.iter().enumerate() {
                assert_eq!(
                    index.utf16_to_byte(position as u32),
                    *expected,
                    "utf16 {position} of {text:?}"
                );
            }
            assert_eq!(index.utf16_to_byte(index.utf16_len() + 1), None, "{text:?}");
            assert_eq!(index.line_count() as usize, line_bytes.len(), "{text:?}");
            for (line, (&byte, &utf16)) in line_bytes.iter().zip(&line_utf16).enumerate() {
                assert_eq!(
                    index.line_start_byte(line as u32),
                    Some(byte),
                    "line {line} of {text:?}"
                );
                assert_eq!(
                    index.line_start_utf16(line as u32),
                    Some(utf16),
                    "line {line} of {text:?}"
                );
            }
            assert_eq!(
                index.line_start_byte(line_bytes.len() as u32),
                None,
                "{text:?}"
            );
        }
    }
}
