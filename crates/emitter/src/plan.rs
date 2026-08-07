use std::path::{Path, PathBuf};

use tsc_program::SourceFileId;

use crate::{EmitContractViolation, EmitFailure, UnsupportedEmitFeature};

/// Public request selection retained independently from emitted roots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmitSelection {
    WholeProgram,
    TargetSourceFile(SourceFileId),
}

/// Typed bundle root retained for later `outFile` admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitBundle {
    source_files: Box<[SourceFileId]>,
}

impl EmitBundle {
    pub fn new(source_files: Vec<SourceFileId>) -> Self {
        Self {
            source_files: source_files.into_boxed_slice(),
        }
    }

    pub fn source_files(&self) -> &[SourceFileId] {
        &self.source_files
    }
}

/// Input root paired with one output-path unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmitRoot {
    SourceFile(SourceFileId),
    Bundle(EmitBundle),
}

/// Independent emit mode corresponding to TypeScript's internal emit-only
/// and build-info controls.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EmitMode {
    Script,
    DeclarationOnly,
    BuilderSignature,
    BuildInfoOnly,
}

/// Full `getOutputPathsFor` plus build-info slot shape.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmitOutputPaths {
    javascript: Option<PathBuf>,
    javascript_map: Option<PathBuf>,
    declaration: Option<PathBuf>,
    declaration_map: Option<PathBuf>,
    build_info: Option<PathBuf>,
}

impl EmitOutputPaths {
    pub const fn empty() -> Self {
        Self {
            javascript: None,
            javascript_map: None,
            declaration: None,
            declaration_map: None,
            build_info: None,
        }
    }

    pub fn javascript(path: impl Into<PathBuf>) -> Self {
        Self {
            javascript: Some(path.into()),
            ..Self::empty()
        }
    }

    pub fn with_javascript_map(mut self, path: impl Into<PathBuf>) -> Self {
        self.javascript_map = Some(path.into());
        self
    }

    pub fn with_declaration(mut self, path: impl Into<PathBuf>) -> Self {
        self.declaration = Some(path.into());
        self
    }

    pub fn with_declaration_map(mut self, path: impl Into<PathBuf>) -> Self {
        self.declaration_map = Some(path.into());
        self
    }

    pub fn with_build_info(mut self, path: impl Into<PathBuf>) -> Self {
        self.build_info = Some(path.into());
        self
    }

    pub fn javascript_path(&self) -> Option<&Path> {
        self.javascript.as_deref()
    }

    pub fn javascript_map_path(&self) -> Option<&Path> {
        self.javascript_map.as_deref()
    }

    pub fn declaration_path(&self) -> Option<&Path> {
        self.declaration.as_deref()
    }

    pub fn declaration_map_path(&self) -> Option<&Path> {
        self.declaration_map.as_deref()
    }

    pub fn build_info_path(&self) -> Option<&Path> {
        self.build_info.as_deref()
    }
}

/// One source-file-or-bundle output unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitOutputUnit {
    root: EmitRoot,
    paths: EmitOutputPaths,
    mode: EmitMode,
}

impl EmitOutputUnit {
    pub fn new(root: EmitRoot, paths: EmitOutputPaths, mode: EmitMode) -> Self {
        Self { root, paths, mode }
    }

    pub const fn root(&self) -> &EmitRoot {
        &self.root
    }

    pub const fn paths(&self) -> &EmitOutputPaths {
        &self.paths
    }

    pub const fn mode(&self) -> EmitMode {
        self.mode
    }
}

/// Ordered output plan with selection separate from each emitted root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitOutputPlan {
    selection: EmitSelection,
    units: Box<[EmitOutputUnit]>,
}

impl EmitOutputPlan {
    pub fn whole_program(units: Vec<EmitOutputUnit>) -> Self {
        Self {
            selection: EmitSelection::WholeProgram,
            units: units.into_boxed_slice(),
        }
    }

    pub fn targeted(source_file: SourceFileId, units: Vec<EmitOutputUnit>) -> Self {
        Self {
            selection: EmitSelection::TargetSourceFile(source_file),
            units: units.into_boxed_slice(),
        }
    }

    pub const fn selection(&self) -> EmitSelection {
        self.selection
    }

    pub fn units(&self) -> &[EmitOutputUnit] {
        &self.units
    }

    /// Validate the first H1 profile without invoking an output sink.
    pub fn validate_bootstrap_shape(&self) -> Result<(), EmitFailure> {
        if matches!(self.selection, EmitSelection::TargetSourceFile(_)) {
            return Err(EmitFailure::Unsupported(
                UnsupportedEmitFeature::TargetedSelection,
            ));
        }
        for unit in &self.units {
            if matches!(unit.root, EmitRoot::Bundle(_)) {
                return Err(EmitFailure::Unsupported(UnsupportedEmitFeature::BundleRoot));
            }
            match unit.mode {
                EmitMode::Script => {}
                EmitMode::DeclarationOnly => {
                    return Err(EmitFailure::Unsupported(
                        UnsupportedEmitFeature::DeclarationOnlyMode,
                    ));
                }
                EmitMode::BuilderSignature => {
                    return Err(EmitFailure::Unsupported(
                        UnsupportedEmitFeature::BuilderSignatureMode,
                    ));
                }
                EmitMode::BuildInfoOnly => {
                    return Err(EmitFailure::Unsupported(
                        UnsupportedEmitFeature::BuildInfoOnlyMode,
                    ));
                }
            }
            if unit.paths.javascript_map.is_some() {
                return Err(EmitFailure::Unsupported(
                    UnsupportedEmitFeature::JavaScriptMap,
                ));
            }
            if unit.paths.declaration.is_some() {
                return Err(EmitFailure::Unsupported(
                    UnsupportedEmitFeature::Declaration,
                ));
            }
            if unit.paths.declaration_map.is_some() {
                return Err(EmitFailure::Unsupported(
                    UnsupportedEmitFeature::DeclarationMap,
                ));
            }
            if unit.paths.build_info.is_some() {
                return Err(EmitFailure::Unsupported(UnsupportedEmitFeature::BuildInfo));
            }
            if unit.paths.javascript.is_none() {
                return Err(EmitFailure::Contract(
                    EmitContractViolation::ScriptOutputMissingJavaScriptPath,
                ));
            }
        }
        Ok(())
    }
}
