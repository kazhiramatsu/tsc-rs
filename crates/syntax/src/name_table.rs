//! The storage of tsgo `SourceFile.GetNameTable` (ast.go): the file's names,
//! computed once on first use (`nameTableOnce`) and kept as long as the
//! file. The language service fills it (`tsc_ls`'s `name_table`).

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Each identifier's (and name-like literal's) text, with its position when
/// it occurs once (tsgo's `-1` is `None`).
pub type NameTable = BTreeMap<String, Option<u32>>;

/// The lazily filled name table of a `SourceFile`. It is derived from the
/// tree, so a copy starts empty and every cell compares equal. The table is
/// boxed so that it adds only a pointer to every `SourceFile` value.
#[derive(Default)]
pub struct NameTableCell(OnceLock<Box<NameTable>>);

impl NameTableCell {
    pub fn get_or_init(&self, init: impl FnOnce() -> NameTable) -> &NameTable {
        self.0.get_or_init(|| Box::new(init()))
    }
}

impl Clone for NameTableCell {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for NameTableCell {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for NameTableCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NameTableCell")
    }
}
