//! The source half of the per-file record contract: stable metadata and
//! the existing take-once syntax/bind owner. Request/range publication is
//! a later consuming substage, not a duplicate owner in this record.
use super::*;

/// Stable source metadata shared by prepared-source views. Ordered program
/// identities remain program-owned; a reused source cannot carry old IDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceOwnedRecords {
    pub(super) path: ProgramPath,
    pub(super) snapshot: Arc<TextSnapshot>,
    pub(super) import_count: Option<usize>,
    pub(super) package_scope: Option<CanonicalPath>,
    pub(super) inclusion_reasons: SourceInclusionReasons,
    pub(super) preparsed_syntax: PreparsedSyntax,
    pub(super) project_reference: Option<ProjectReferenceFile>,
    pub(super) content_mapped: Option<Arc<crate::content_mapper::source::ContentMappedSource>>,
    pub(super) script_kind: Option<ScriptKind>,
    pub(super) paths: SourcePathStorage,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct SourcePathFacts {
    pub(super) alternate_display_paths: Vec<JsString>,
    pub(super) package_redirect_paths: Vec<ProgramPath>,
    pub(super) project_reference_source_paths: Vec<ProgramPath>,
    pub(super) real_path: Option<ProgramPath>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct SourcePathStorage(Option<Box<SourcePathFacts>>);
impl SourcePathStorage {
    pub(super) fn get(&self) -> &SourcePathFacts {
        static EMPTY: SourcePathFacts = SourcePathFacts {
            alternate_display_paths: Vec::new(),
            package_redirect_paths: Vec::new(),
            project_reference_source_paths: Vec::new(),
            real_path: None,
        };
        self.0.as_deref().unwrap_or(&EMPTY)
    }
    pub(super) fn get_mut(&mut self) -> &mut SourcePathFacts {
        self.0.get_or_insert_default()
    }
    pub(super) fn existing_mut(&mut self) -> Option<&mut SourcePathFacts> {
        self.0.as_deref_mut()
    }
    pub(super) fn is_populated(&self) -> bool {
        self.0.is_some()
    }
}
impl PartialEq for SourcePathStorage {
    fn eq(&self, other: &Self) -> bool {
        self.get() == other.get()
    }
}
impl Eq for SourcePathStorage {}

impl SourceOwnedRecords {
    /// The selected source spelling and canonical identity.
    pub fn path(&self) -> &ProgramPath {
        &self.path
    }
    /// The original snapshot used by this source record.
    pub fn snapshot(&self) -> &Arc<TextSnapshot> {
        &self.snapshot
    }
    /// The existing take-once parse and bind owner.
    pub fn preparsed_syntax(&self) -> &PreparsedSyntax {
        &self.preparsed_syntax
    }
}

const _: () = assert!(std::mem::size_of::<PreparedSourceFile>() <= 16);
const _: () = assert!(std::mem::size_of::<SourceOwnedRecords>() <= 272);
const _: () = assert!(std::mem::size_of::<SourcePathFacts>() <= 112);

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> PreparedSourceFile {
        PreparedSourceFile::new(
            ProgramPath::from_js_parts("/p/a.ts".into(), "/p/a.ts".into()).unwrap(),
            "old",
        )
    }
    #[test]
    fn metadata_cow_preserves_the_existing_syntax_slot_and_original() {
        let original = source();
        let clone = original.clone();
        assert!(Arc::ptr_eq(
            original.source_records(),
            clone.source_records()
        ));
        let changed = clone.with_import_count(7);
        assert_eq!(original.import_count(), None);
        assert_eq!(changed.import_count(), Some(7));
        assert!(!Arc::ptr_eq(
            original.source_records(),
            changed.source_records()
        ));
        assert!(Arc::ptr_eq(
            &original.records.preparsed_syntax.slot,
            &changed.records.preparsed_syntax.slot
        ));
        let snapshot = PreparedSourceFile::new(original.path().clone(), "new")
            .snapshot()
            .clone();
        let refreshed = changed.with_snapshot(snapshot);
        assert_eq!(refreshed.text(), "new");
        assert_eq!(refreshed.import_count(), None);
        assert_eq!(original.text(), "old");
        assert!(!Arc::ptr_eq(
            &original.records.preparsed_syntax.slot,
            &refreshed.records.preparsed_syntax.slot
        ));
    }
    #[test]
    fn inline_flags_share_records_but_path_mutation_does_not_change_the_clone() {
        let original = source();
        let flags = original.clone().with_may_be_emitted(false);
        assert!(Arc::ptr_eq(
            original.source_records(),
            flags.source_records()
        ));
        assert!(original.may_be_emitted());
        assert!(!flags.may_be_emitted());
        let physical =
            ProgramPath::from_js_parts("/real/a.ts".into(), "/real/a.ts".into()).unwrap();
        let changed = original.clone().with_real_path(physical.clone());
        assert_eq!(original.real_path(), None);
        assert_eq!(changed.real_path(), Some(&physical));
        let mut respelled = original.clone();
        respelled.respell("/p/A.ts".into()).unwrap();
        assert_eq!(original.path().display(), "/p/a.ts");
        assert!(original.alternate_display_paths().is_empty());
        assert_eq!(
            respelled.alternate_display_paths(),
            &[JsString::from("/p/a.ts")]
        );
    }
}
