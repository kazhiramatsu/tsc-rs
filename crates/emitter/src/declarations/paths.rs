use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tsc_program::SourceFileId;

use crate::{EmitFailure, EmitHost, EmitOutputPaths, EmitPreflight, EmitRoot, EmitSelection};

use super::DeclarationPathResolver;

/// Declaration/reference paths projected once from the production output
/// plan. This is the single behavioral copy used by both execution and replay.
#[derive(Clone, Debug, Default)]
pub struct PlanDeclarationPaths {
    paths: BTreeMap<SourceFileId, EmitOutputPaths>,
    source_paths: BTreeMap<SourceFileId, PathBuf>,
    root_declaration_paths: BTreeMap<SourceFileId, PathBuf>,
    bundle_declaration_path: Option<PathBuf>,
    bundle_reference_paths: BTreeMap<SourceFileId, PathBuf>,
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
        let bundle_declaration_path = host
            .compiler_options()
            .out_file
            .as_deref()
            .filter(|path| !path.is_empty())
            .and_then(|path| {
                crate::plan::get_output_paths_for_bundle(host.compiler_options(), path, true)
                    .declaration_path()
                    .map(Path::to_path_buf)
            });
        let bundle_reference_paths = if bundle_declaration_path.is_some() {
            host.source_file_ids()
                .iter()
                .filter_map(|&id| {
                    host.source_file(id)
                        .map(|file| (id, crate::plan::declaration_output_path(file.path(), host)))
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        Self {
            paths,
            source_paths,
            root_declaration_paths: BTreeMap::new(),
            bundle_declaration_path,
            bundle_reference_paths,
        }
    }

    /// The declaration transform uses forced paths for its own directory
    /// (_tsc.js:114541-114545) and ordinary paths for references (:114598).
    /// A diagnostic getter computes this read-only projection without an
    /// output plan, blocking diagnostics, or an output sink.
    pub fn for_declaration_diagnostics(host: &dyn EmitHost) -> Result<Self, EmitFailure> {
        let mut result = Self::default();
        for &source in host.source_file_ids() {
            let file = host.source_file(source).ok_or(EmitFailure::Contract(
                crate::EmitContractViolation::PlannedSourceMissing(source),
            ))?;
            result
                .source_paths
                .insert(source, file.path().to_path_buf());
            if crate::plan::source_file_may_emit_forced_declaration(file, host) {
                result.root_declaration_paths.insert(
                    source,
                    crate::plan::declaration_output_path(file.path(), host),
                );
            }
        }
        for source in crate::get_source_files_to_emit(host, EmitSelection::WholeProgram)? {
            let file = host.source_file(source).ok_or(EmitFailure::Contract(
                crate::EmitContractViolation::PlannedSourceMissing(source),
            ))?;
            result
                .paths
                .insert(source, crate::get_output_paths_for(file, host)?);
            result.root_declaration_paths.insert(
                source,
                crate::plan::declaration_output_path(file.path(), host),
            );
        }
        Ok(result)
    }
}

impl DeclarationPathResolver for PlanDeclarationPaths {
    fn bundle_declaration_file_path(&self) -> Option<PathBuf> {
        self.bundle_declaration_path.clone()
    }

    fn declaration_file_path(&self, source: SourceFileId) -> Option<PathBuf> {
        self.root_declaration_paths
            .get(&source)
            .cloned()
            .or_else(|| {
                self.paths
                    .get(&source)
                    .and_then(EmitOutputPaths::declaration_path)
                    .map(Path::to_path_buf)
            })
    }

    fn reference_target_path(&self, source: SourceFileId) -> Option<PathBuf> {
        self.bundle_reference_paths
            .get(&source)
            .cloned()
            .or_else(|| {
                self.paths
                    .get(&source)
                    .and_then(|paths| paths.declaration_path().or_else(|| paths.javascript_path()))
                    .map(Path::to_path_buf)
                    .or_else(|| self.source_paths.get(&source).cloned())
            })
    }
}
