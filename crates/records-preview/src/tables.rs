//! Safe executable model of the variable SymbolTable record's word layout.
//! Vec is stub storage only: word0 len, word1 flags, then len ordered 8 B entries,
//! then an open-addressed u32 index when len > 16 (0 empty, entry index + 1).
use crate::records::TableEntry;
use crate::support::Id;

pub struct FlatSymbolTable {
    words: Vec<u32>,
}
impl FlatSymbolTable {
    pub const HAS_INDEX: u32 = 1;
    pub fn new(entries: &[TableEntry]) -> Self {
        let len = u32::try_from(entries.len()).expect("table length overflow");
        let indexed = entries.len() > 16;
        let slots = if indexed {
            entries.len().checked_mul(2).unwrap().next_power_of_two()
        } else {
            0
        };
        let mut words = Vec::with_capacity(2 + entries.len() * 2 + slots);
        words.extend([len, if indexed { Self::HAS_INDEX } else { 0 }]);
        for (i, entry) in entries.iter().enumerate() {
            assert!(
                !entries[..i].iter().any(|prior| prior.name == entry.name),
                "duplicate name"
            );
            words.extend([entry.name, entry.symbol.raw()]);
        }
        words.resize(words.len() + slots, 0);
        let mut table = Self { words };
        if indexed {
            let start = 2 + entries.len() * 2;
            for (i, entry) in entries.iter().enumerate() {
                let mut slot = Self::hash(entry.name) & (slots - 1);
                while table.words[start + slot] != 0 {
                    slot = (slot + 1) & (slots - 1);
                }
                table.words[start + slot] = i as u32 + 1;
            }
        }
        table
    }
    fn hash(name: u32) -> usize {
        name.wrapping_mul(0x9e37_79b9) as usize
    }
    pub fn len(&self) -> usize {
        self.words[0] as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn flags(&self) -> u32 {
        self.words[1]
    }
    pub fn words(&self) -> &[u32] {
        &self.words
    }
    pub fn entries(&self) -> impl Iterator<Item = TableEntry> + '_ {
        self.words[2..2 + self.len() * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| TableEntry {
                name: p[0],
                symbol: Id::from_raw(p[1]),
            })
    }
    pub fn get(&self, name: u32) -> Option<Id<crate::records::Symbol>> {
        if self.flags() & Self::HAS_INDEX == 0 {
            return self.entries().find(|e| e.name == name).map(|e| e.symbol);
        }
        let start = 2 + self.len() * 2;
        let slots = self.words.len() - start;
        let mut slot = Self::hash(name) & (slots - 1);
        loop {
            let raw = self.words[start + slot];
            if raw == 0 {
                return None;
            }
            let pos = 2 + (raw as usize - 1) * 2;
            if self.words[pos] == name {
                return Some(Id::from_raw(self.words[pos + 1]));
            }
            slot = (slot + 1) & (slots - 1);
        }
    }
}
