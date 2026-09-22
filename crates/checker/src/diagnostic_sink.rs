//! Ordered diagnostic storage with an auxiliary index for exact duplicates.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};

use tsc_diagnostics::{Diagnostic, DiagnosticList};

/// The checker's mutable diagnostic ledger. Indices and insertion order are
/// stable until the caller changes the underlying list. Arbitrary Vec access
/// through `DerefMut` discards the index; the next lookup rebuilds it lazily.
#[derive(Debug, Default)]
pub struct DiagnosticSink {
    diagnostics: DiagnosticList,
    index: Option<HashMap<u64, Vec<usize>>>,
}

/// Only select a candidate bucket here: equality still compares the complete
/// diagnostic, including JS code units, category, chains and related info.
/// Category/related-info edits keep the bucket, and hash collisions are safe.
fn bucket(diagnostic: &Diagnostic) -> u64 {
    let mut hash = DefaultHasher::new();
    diagnostic.file_name.hash(&mut hash);
    diagnostic.start.hash(&mut hash);
    diagnostic.length.hash(&mut hash);
    diagnostic.message.code.hash(&mut hash);
    hash.finish()
}

impl DiagnosticSink {
    /// Append without deduplication, matching Vec::push even for duplicates.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        let position = self.diagnostics.len();
        if let Some(index) = &mut self.index {
            index.entry(bucket(&diagnostic)).or_default().push(position);
        }
        self.diagnostics.push(diagnostic);
    }

    /// Roll back both the ledger and its index, preserving earlier duplicates.
    pub fn truncate(&mut self, len: usize) {
        if let Some(index) = &mut self.index {
            for diagnostic in self.diagnostics.iter().skip(len).rev() {
                let key = bucket(diagnostic);
                let positions = index.get_mut(&key).expect("indexed diagnostic");
                positions.pop();
                if positions.is_empty() {
                    index.remove(&key);
                }
            }
        }
        self.diagnostics.truncate(len);
    }

    /// Return the first exact match, or append and return the new index.
    pub(crate) fn insert_unique(&mut self, diagnostic: Diagnostic) -> usize {
        // Small ledgers are cheaper to scan and need no additional allocation.
        if self.index.is_none() && self.diagnostics.len() < 32 {
            if let Some(position) = self.diagnostics.iter().position(|row| *row == diagnostic) {
                return position;
            }
        } else {
            let index = self.index.get_or_insert_with(|| {
                let mut index: HashMap<u64, Vec<usize>> = HashMap::new();
                for (position, row) in self.diagnostics.iter().enumerate() {
                    index.entry(bucket(row)).or_default().push(position);
                }
                index
            });
            if let Some(positions) = index.get(&bucket(&diagnostic)) {
                if let Some(position) = positions
                    .iter()
                    .copied()
                    .find(|&position| self.diagnostics[position] == diagnostic)
                {
                    return position;
                }
            }
        }
        let position = self.diagnostics.len();
        self.push(diagnostic);
        position
    }

    /// Keep the index for edits that do not move a diagnostic to another
    /// bucket. Taking it before the callback also invalidates it on unwind.
    pub(crate) fn update<R>(
        &mut self,
        position: usize,
        edit: impl FnOnce(&mut Diagnostic) -> R,
    ) -> R {
        let index = self.index.take();
        let diagnostic = &mut self.diagnostics[position];
        let previous = bucket(diagnostic);
        let result = edit(diagnostic);
        if bucket(diagnostic) == previous {
            self.index = index;
        }
        result
    }

    pub fn into_vec(self) -> DiagnosticList {
        self.diagnostics
    }
}

impl From<DiagnosticList> for DiagnosticSink {
    fn from(diagnostics: DiagnosticList) -> Self {
        Self {
            diagnostics,
            index: None,
        }
    }
}

impl Deref for DiagnosticSink {
    type Target = DiagnosticList;

    fn deref(&self) -> &Self::Target {
        &self.diagnostics
    }
}

impl DerefMut for DiagnosticSink {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.index = None;
        &mut self.diagnostics
    }
}

impl<'a> IntoIterator for &'a DiagnosticSink {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.diagnostics.iter()
    }
}

#[cfg(test)]
#[path = "../tests/unit/state/diagnostic_sink_tests.rs"]
mod tests;
