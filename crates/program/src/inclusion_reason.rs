//! Loader-owned reasons keep source identities and token spans. Public text
//! is materialized only by a consumer that asks to explain an inclusion.
use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use tsc_diagnostics::{JsString, TextSnapshot};

use crate::{PackageId, ProgramPath, SourceInclusionReason};

#[derive(Clone, Debug)]
pub(crate) struct InclusionSource {
    pub path: ProgramPath,
    pub snapshot: Arc<TextSnapshot>,
}

#[derive(Clone, Debug)]
pub(crate) enum InclusionReasonRecord {
    Owned(Arc<SourceInclusionReason>),
    Import {
        parent: usize,
        pos: u32,
        end: u32,
        package_id: Option<Arc<JsString>>,
    },
    SyntheticImport {
        parent: usize,
        specifier: Arc<JsString>,
        import_helpers: bool,
        package_id: Option<Arc<JsString>>,
    },
}

const _: () = assert!(std::mem::size_of::<InclusionReasonRecord>() <= 40);

impl From<SourceInclusionReason> for InclusionReasonRecord {
    fn from(reason: SourceInclusionReason) -> Self {
        Self::Owned(Arc::new(reason))
    }
}

impl InclusionReasonRecord {
    pub(crate) fn with_package_id(mut self, id: Option<&PackageId>) -> Self {
        match &mut self {
            Self::Owned(reason) => {
                Arc::make_mut(reason).set_package_id(id);
            }
            Self::Import { package_id, .. } | Self::SyntheticImport { package_id, .. } => {
                *package_id = id.map(|id| Arc::new(id.display_text()));
            }
        }
        self
    }

    pub(crate) fn parent_mut(&mut self) -> Option<&mut JsString> {
        match self {
            Self::Owned(reason) => Arc::make_mut(reason).parent_mut(),
            Self::Import { .. } | Self::SyntheticImport { .. } => None,
        }
    }

    pub(crate) fn materialize(&self, sources: &[InclusionSource]) -> SourceInclusionReason {
        match self {
            Self::Owned(reason) => (**reason).clone(),
            Self::Import {
                parent,
                pos,
                end,
                package_id,
            } => {
                let parent = &sources[*parent];
                let positions = parent.snapshot.positions();
                let start_byte = positions
                    .utf16_to_byte(*pos)
                    .expect("module request starts at a source token boundary");
                let end_byte = positions
                    .utf16_to_byte(*end)
                    .expect("module request ends at a source token boundary");
                SourceInclusionReason::Import {
                    parent: parent.path.display().to_owned(),
                    reference_text: parent.snapshot.text()[start_byte as usize..end_byte as usize]
                        .to_owned(),
                    pos: *pos,
                    end: *end,
                    package_id: package_id.as_deref().cloned(),
                }
            }
            Self::SyntheticImport {
                parent,
                specifier,
                import_helpers,
                package_id,
            } => SourceInclusionReason::SyntheticImport {
                parent: sources[*parent].path.display().to_owned(),
                specifier: (**specifier).clone(),
                import_helpers: *import_helpers,
                package_id: package_id.as_deref().cloned(),
            },
        }
    }

    fn add_memory_usage(&self, usage: &mut tsc_diagnostics::MemoryUsage) {
        match self {
            Self::Owned(reason) => {
                if usage.arc("include reason", reason) {
                    reason.add_memory_usage(usage);
                }
            }
            Self::Import { package_id, .. } | Self::SyntheticImport { package_id, .. } => {
                if let Some(id) = package_id {
                    if usage.arc("include package id", id) {
                        usage.js_string("strings: stable owners", id);
                    }
                }
                if let Self::SyntheticImport { specifier, .. } = self {
                    if usage.arc("include specifier", specifier) {
                        usage.js_string("strings: stable owners", specifier);
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SourceInclusionReasons {
    records: Vec<InclusionReasonRecord>,
    sources: Option<Arc<Vec<InclusionSource>>>,
    text: OnceLock<Vec<SourceInclusionReason>>,
}

impl SourceInclusionReasons {
    pub(crate) fn push(&mut self, reason: InclusionReasonRecord) {
        debug_assert!(self.text.get().is_none());
        self.records.push(reason);
    }

    pub(crate) fn bind_sources(&mut self, sources: &Arc<Vec<InclusionSource>>) {
        self.sources = Some(Arc::clone(sources));
    }

    pub(crate) fn records_mut(&mut self) -> &mut [InclusionReasonRecord] {
        debug_assert!(self.text.get().is_none());
        &mut self.records
    }

    pub(crate) fn into_text(self) -> Vec<SourceInclusionReason> {
        self.text.into_inner().unwrap_or_else(|| {
            let sources = self.sources.as_deref().map_or(&[][..], Vec::as_slice);
            self.records
                .iter()
                .map(|reason| reason.materialize(sources))
                .collect()
        })
    }

    pub(crate) fn add_memory_usage(&self, usage: &mut tsc_diagnostics::MemoryUsage) {
        usage.vec("loader inclusion records", &self.records);
        for record in &self.records {
            record.add_memory_usage(usage);
        }
        if let Some(sources) = &self.sources {
            if usage.arc("include reason sources", sources) {
                usage.vec("include reason sources", sources);
                for source in sources.iter() {
                    source.path.add_memory_usage(usage);
                    TextSnapshot::add_memory_usage(&source.snapshot, usage);
                }
            }
        }
        if let Some(text) = self.text.get() {
            usage.vec("loader inclusion reasons (retained)", text);
            for reason in text {
                reason.add_memory_usage(usage);
            }
        }
    }
}

impl From<Vec<SourceInclusionReason>> for SourceInclusionReasons {
    fn from(text: Vec<SourceInclusionReason>) -> Self {
        Self {
            text: OnceLock::from(text),
            ..Self::default()
        }
    }
}

impl Deref for SourceInclusionReasons {
    type Target = [SourceInclusionReason];

    fn deref(&self) -> &Self::Target {
        self.text.get_or_init(|| {
            let sources = self.sources.as_deref().map_or(&[][..], Vec::as_slice);
            self.records
                .iter()
                .map(|reason| reason.materialize(sources))
                .collect()
        })
    }
}

impl PartialEq for SourceInclusionReasons {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for SourceInclusionReasons {}

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_diagnostics::DocumentVersion;

    #[test]
    fn import_reasons_use_final_parent_spelling_and_utf16_token_spans() {
        let sources = Arc::new(vec![InclusionSource {
            path: ProgramPath::from_js_parts(
                "/project/Final.ts".into(),
                "/project/final.ts".into(),
            )
            .unwrap(),
            snapshot: TextSnapshot::new("😀\n\"é\"", DocumentVersion::new("1")),
        }]);
        let mut reasons = SourceInclusionReasons::default();
        reasons.push(InclusionReasonRecord::Import {
            parent: 0,
            pos: 3,
            end: 6,
            package_id: None,
        });
        reasons.push(InclusionReasonRecord::SyntheticImport {
            parent: 0,
            specifier: Arc::new("tslib".into()),
            import_helpers: true,
            package_id: None,
        });
        reasons.bind_sources(&sources);
        assert_eq!(
            &*reasons,
            &[
                SourceInclusionReason::Import {
                    parent: "/project/Final.ts".into(),
                    reference_text: "\"é\"".into(),
                    pos: 3,
                    end: 6,
                    package_id: None,
                },
                SourceInclusionReason::SyntheticImport {
                    parent: "/project/Final.ts".into(),
                    specifier: "tslib".into(),
                    import_helpers: true,
                    package_id: None,
                },
            ]
        );
    }
}
