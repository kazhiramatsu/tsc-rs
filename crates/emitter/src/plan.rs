use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tsc_diagnostics::{gen, sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList, MessageChain};
use tsc_program::SourceFileId;

use crate::host::normalize_lexical_path;
use crate::{EmitContractViolation, EmitFailure, EmitHost, EmitSource, UnsupportedEmitFeature};

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

/// Output plan plus Program-owned blocking diagnostics discovered before the
/// first sink callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitPreflight {
    plan: EmitOutputPlan,
    diagnostics: DiagnosticList,
    blocked_outputs: BTreeSet<PathBuf>,
}

impl EmitPreflight {
    pub const fn plan(&self) -> &EmitOutputPlan {
        &self.plan
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn is_emit_blocked(&self, host: &dyn EmitHost, path: &Path) -> bool {
        self.blocked_outputs
            .contains(&host.canonical_output_path(path))
    }
}

/// tsc-port: getSourceFilesToEmit @6.0.3
/// tsc-hash: bbfd59e2d4e5da3c2b08b243e7a31244a2df0a044f358ed5048781751ac40410
/// tsc-span: _tsc.js:16600-16616
pub fn get_source_files_to_emit(
    host: &dyn EmitHost,
    selection: EmitSelection,
) -> Result<Vec<SourceFileId>, EmitFailure> {
    let candidates: Vec<SourceFileId> = match selection {
        EmitSelection::WholeProgram => host.source_file_ids().to_vec(),
        EmitSelection::TargetSourceFile(source) => vec![source],
    };
    candidates
        .into_iter()
        .filter_map(|id| match host.source_file(id) {
            Some(source) if source_file_may_be_emitted_for_host(source, host) => Some(Ok(id)),
            Some(_) => None,
            None => Some(Err(EmitFailure::Contract(
                EmitContractViolation::PlannedSourceMissing(id),
            ))),
        })
        .collect()
}

/// tsc-port: sourceFileMayBeEmitted @6.0.3
/// tsc-hash: 333fcd249758d38eb80146910286d7cabdbbf6f1ea0787f8f1a2c85e9535ecb2
/// tsc-span: _tsc.js:16617-16634
pub fn source_file_may_be_emitted(source: EmitSource<'_>) -> bool {
    source.may_be_emitted() && !is_declaration_file_name(source.path())
}

/// The Program retains source-side eligibility. JSON additionally depends on
/// the emit request having somewhere distinct to copy the source, matching
/// the option-dependent arm of TypeScript's `sourceFileMayBeEmitted`.
pub(crate) fn source_file_may_be_emitted_for_host(
    source: EmitSource<'_>,
    host: &dyn EmitHost,
) -> bool {
    if !source_file_may_be_emitted(source) {
        return false;
    }
    !source
        .path()
        .to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(".json")
        || host.compiler_options().out_dir.is_some()
        || host.compiler_options().out_file.is_some()
}

/// tsc-port: getOutputPathsFor @6.0.3
/// tsc-hash: f3ef9e378ec2b224d2f434b49f6ffd2a9597e7cc102f504653c9027a49c5ebd2
/// tsc-span: _tsc.js:116373-116387
pub fn get_output_paths_for(
    source: EmitSource<'_>,
    host: &dyn EmitHost,
) -> Result<EmitOutputPaths, EmitFailure> {
    let options = host.compiler_options();
    let extension = get_output_extension(source.path(), options.jsx)?;
    let javascript = get_own_emit_output_file_path(source.path(), host, extension);
    let is_json = extension == "json";
    let javascript = (!(options.emit_declaration_only.unwrap_or(false)
        || is_json
            && host.canonical_output_path(source.path())
                == host.canonical_output_path(&javascript)))
    .then_some(javascript);

    let mut paths = match javascript {
        Some(path) => EmitOutputPaths::javascript(path),
        None => EmitOutputPaths::empty(),
    };
    if options.source_map == Some(true) && options.inline_source_map != Some(true) {
        if let Some(path) = paths.javascript_path().map(Path::to_path_buf) {
            paths = paths.with_javascript_map(format!("{}.map", path.to_string_lossy()));
        }
    }
    if (options.declaration == Some(true) || options.composite == Some(true)) && !is_json {
        let declaration = declaration_output_path(source.path(), host);
        if options.declaration_map == Some(true) {
            paths = paths.with_declaration_map(format!("{}.map", declaration.to_string_lossy()));
        }
        paths = paths.with_declaration(declaration);
    }
    Ok(paths)
}

/// tsc-port: forEachEmittedFile @6.0.3
/// tsc-hash: afdd65979d7f7bbcc8a1a406c93f26c2eb788ca31f283e6fae703765ea3fa89a
/// tsc-span: _tsc.js:116312-116341
pub fn for_each_emitted_file(
    host: &dyn EmitHost,
    selection: EmitSelection,
    mut action: impl FnMut(&EmitOutputPaths, &EmitRoot),
) -> Result<(), EmitFailure> {
    let source_files = get_source_files_to_emit(host, selection)?;
    if let Some(out_file) = host.compiler_options().out_file.as_deref() {
        if !source_files.is_empty() {
            let mut paths = EmitOutputPaths::javascript(resolve_option_path(host, out_file));
            if host.compiler_options().source_map == Some(true)
                && host.compiler_options().inline_source_map != Some(true)
            {
                let path = paths
                    .javascript_path()
                    .expect("bundle JavaScript path")
                    .to_path_buf();
                paths = paths.with_javascript_map(format!("{}.map", path.to_string_lossy()));
            }
            if host.compiler_options().declaration == Some(true)
                || host.compiler_options().composite == Some(true)
            {
                let declaration = paths
                    .javascript_path()
                    .expect("bundle JavaScript path")
                    .with_extension("d.ts");
                if host.compiler_options().declaration_map == Some(true) {
                    paths = paths
                        .with_declaration_map(format!("{}.map", declaration.to_string_lossy()));
                }
                paths = paths.with_declaration(declaration);
            }
            action(&paths, &EmitRoot::Bundle(EmitBundle::new(source_files)));
        }
        return Ok(());
    }

    for source_file in source_files {
        let source = host.source_file(source_file).ok_or(EmitFailure::Contract(
            EmitContractViolation::PlannedSourceMissing(source_file),
        ))?;
        let paths = get_output_paths_for(source, host)?;
        if paths.javascript_path().is_some()
            || paths.javascript_map_path().is_some()
            || paths.declaration_path().is_some()
            || paths.declaration_map_path().is_some()
            || paths.build_info_path().is_some()
        {
            action(&paths, &EmitRoot::SourceFile(source_file));
        }
    }
    Ok(())
}

/// Build every output unit and run overwrite/duplicate-output validation
/// before transform, print, or sink dispatch begins.
pub fn preflight_emit(
    host: &dyn EmitHost,
    selection: EmitSelection,
) -> Result<EmitPreflight, EmitFailure> {
    let mut units = Vec::new();
    for_each_emitted_file(host, selection, |paths, root| {
        units.push(EmitOutputUnit::new(
            root.clone(),
            paths.clone(),
            EmitMode::Script,
        ));
    })?;
    let plan = match selection {
        EmitSelection::WholeProgram => EmitOutputPlan::whole_program(units),
        EmitSelection::TargetSourceFile(source) => EmitOutputPlan::targeted(source, units),
    };

    let input_paths = host
        .source_file_ids()
        .iter()
        .filter_map(|id| host.source_file(*id))
        .map(|source| source.canonical_path().to_path_buf())
        .collect::<BTreeSet<_>>();
    let mut emitted_paths = BTreeSet::new();
    let mut blocked_outputs = BTreeSet::new();
    let mut diagnostics = Vec::new();
    let options = host.compiler_options();
    if options.resolve_json_module_effective() {
        if options.emit_module_resolution_kind() == 1 {
            diagnostics.push(option_diagnostic(
                &gen::Option_resolveJsonModule_cannot_be_specified_when_moduleResolution_is_set_to_classic,
            ));
        } else if matches!(options.emit_module_kind(), 0 | 3 | 4) {
            diagnostics.push(option_diagnostic(
                &gen::Option_resolveJsonModule_cannot_be_specified_when_module_is_set_to_none_system_or_umd,
            ));
        }
    }
    for unit in plan.units() {
        for path in [
            unit.paths().javascript_path(),
            unit.paths().declaration_path(),
        ]
        .into_iter()
        .flatten()
        {
            let canonical = host.canonical_output_path(path);
            if input_paths.contains(&canonical) {
                diagnostics.push(overwrite_input_diagnostic(host, path));
                blocked_outputs.insert(canonical.clone());
            }
            if !emitted_paths.insert(canonical.clone()) {
                diagnostics.push(compiler_diagnostic(
                    &gen::Cannot_write_file_0_because_it_would_be_overwritten_by_multiple_input_files,
                    path,
                ));
                blocked_outputs.insert(canonical);
            }
        }
    }
    sort_and_dedupe_diagnostics(&mut diagnostics);
    Ok(EmitPreflight {
        plan,
        diagnostics,
        blocked_outputs,
    })
}

/// tsc-port: getOwnEmitOutputFilePath @6.0.3
/// tsc-hash: 4ddd1ea3136e64d8da7394a321fb709fffd279d36c8b456616956cdd82905b14
/// tsc-span: _tsc.js:16567-16576
fn get_own_emit_output_file_path(
    source_file: &Path,
    host: &dyn EmitHost,
    extension: &'static str,
) -> PathBuf {
    let relocated = host
        .compiler_options()
        .out_dir
        .as_deref()
        .map(|out_dir| source_file_path_in_new_dir(source_file, host, out_dir))
        .unwrap_or_else(|| source_file.to_path_buf());
    relocated.with_extension(extension)
}

/// tsc-port: getOutputExtension @6.0.3
/// tsc-hash: cf61157be90d2652413f6d8ee13d05b2e76048b1f4ee38f8b620691af40632ce
/// tsc-span: _tsc.js:116391-116393
fn get_output_extension(path: &Path, jsx: Option<i32>) -> Result<&'static str, EmitFailure> {
    let file_name = path.to_string_lossy().to_ascii_lowercase();
    if file_name.ends_with(".json") {
        Ok("json")
    } else if (file_name.ends_with(".tsx") || file_name.ends_with(".jsx")) && jsx == Some(1) {
        Ok("jsx")
    } else if file_name.ends_with(".mts") || file_name.ends_with(".mjs") {
        Ok("mjs")
    } else if file_name.ends_with(".cts") || file_name.ends_with(".cjs") {
        Ok("cjs")
    } else if file_name.ends_with(".ts")
        || file_name.ends_with(".tsx")
        || file_name.ends_with(".js")
        || file_name.ends_with(".jsx")
    {
        Ok("js")
    } else {
        Err(EmitFailure::UnsupportedSourceExtension {
            path: path.to_path_buf(),
        })
    }
}

fn declaration_output_path(source_file: &Path, host: &dyn EmitHost) -> PathBuf {
    let options = host.compiler_options();
    let relocated = options
        .declaration_dir
        .as_deref()
        .or(options.out_dir.as_deref())
        .map(|directory| source_file_path_in_new_dir(source_file, host, directory))
        .unwrap_or_else(|| source_file.to_path_buf());
    let lower = relocated.to_string_lossy().to_ascii_lowercase();
    let extension = if lower.ends_with(".mts") || lower.ends_with(".mjs") {
        "d.mts"
    } else if lower.ends_with(".cts") || lower.ends_with(".cjs") {
        "d.cts"
    } else if lower.ends_with(".json") {
        "d.json.ts"
    } else {
        "d.ts"
    };
    relocated.with_extension(extension)
}

fn source_file_path_in_new_dir(
    source_file: &Path,
    host: &dyn EmitHost,
    output_directory: &str,
) -> PathBuf {
    let output_directory = resolve_option_path(host, output_directory);
    let source = absolute_display_path(host, source_file);
    let common = absolute_display_path(host, host.common_source_directory());
    let canonical_source = host.canonical_output_path(&source);
    let canonical_common = host.canonical_output_path(&common);
    let relative = canonical_source
        .starts_with(&canonical_common)
        .then(|| {
            source
                .components()
                .skip(common.components().count())
                .collect::<PathBuf>()
        })
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(source);
    output_directory.join(relative)
}

fn absolute_display_path(host: &dyn EmitHost, path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        host.current_directory().join(path)
    };
    normalize_lexical_path(&absolute)
}

fn resolve_option_path(host: &dyn EmitHost, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        host.current_directory().join(path)
    }
}

fn is_declaration_file_name(path: &Path) -> bool {
    let name = path.to_string_lossy().to_ascii_lowercase();
    name.ends_with(".d.ts")
        || name.ends_with(".d.mts")
        || name.ends_with(".d.cts")
        || name
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|base| base.contains(".d.") && base.ends_with(".ts"))
}

fn compiler_diagnostic(
    message: &'static tsc_diagnostics::DiagnosticMessage,
    path: &Path,
) -> Diagnostic {
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(message, &[path.to_string_lossy().into_owned()]),
    )
}

fn option_diagnostic(message: &'static tsc_diagnostics::DiagnosticMessage) -> Diagnostic {
    Diagnostic::new(None, None, None, MessageChain::new(message, &[]))
}

fn overwrite_input_diagnostic(host: &dyn EmitHost, path: &Path) -> Diagnostic {
    let mut message = MessageChain::new(
        &gen::Cannot_write_file_0_because_it_would_overwrite_input_file,
        &[path.to_string_lossy().into_owned()],
    );
    if host.config_file_path().is_none() {
        message = message.with_next(vec![MessageChain::new(
            &gen::Adding_a_tsconfig_json_file_will_help_organize_projects_that_contain_both_TypeScript_and_JavaScript_files_Learn_more_at_https_aka_ms_tsconfig,
            &[],
        )]);
    }
    Diagnostic::new(None, None, None, message)
}
