use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tsc_program::SourceFileId;

use crate::{EmitHost, EmitOutputPaths, EmitPreflight, EmitRoot};

use super::DeclarationPathResolver;

/// Declaration/reference paths projected once from the production output
/// plan. This is the single behavioral copy used by both execution and replay.
#[derive(Clone, Debug, Default)]
pub struct PlanDeclarationPaths {
    paths: BTreeMap<SourceFileId, EmitOutputPaths>,
    source_paths: BTreeMap<SourceFileId, PathBuf>,
}

impl PlanDeclarationPaths {
    /// tsc-port: getOutputPathsFor @6.0.3
    /// tsc-hash: f3ef9e378ec2b224d2f434b49f6ffd2a9597e7cc102f504653c9027a49c5ebd2
    /// tsc-span: _tsc.js:116373-116387
    pub fn new(host: &dyn EmitHost, preflight: &EmitPreflight) -> Self {
        let paths = preflight
            .plan()
            .units()
            .iter()
            .filter_map(|unit| {
                let EmitRoot::SourceFile(source) = unit.root() else {
                    return None;
                };
                Some((*source, unit.paths().clone()))
            })
            .collect();
        let source_paths = host
            .source_file_ids()
            .iter()
            .filter_map(|&source| {
                host.source_file(source)
                    .map(|emit_source| (source, emit_source.path().to_path_buf()))
            })
            .collect();
        Self {
            paths,
            source_paths,
        }
    }
}

impl DeclarationPathResolver for PlanDeclarationPaths {
    fn declaration_file_path(&self, source: SourceFileId) -> Option<PathBuf> {
        self.paths
            .get(&source)
            .and_then(EmitOutputPaths::declaration_path)
            .map(Path::to_path_buf)
    }

    fn reference_target_path(&self, source: SourceFileId) -> Option<PathBuf> {
        self.paths
            .get(&source)
            .and_then(|paths| paths.declaration_path().or_else(|| paths.javascript_path()))
            .map(Path::to_path_buf)
            .or_else(|| self.source_paths.get(&source).cloned())
    }
}
