use crate::builtins::has_ascii_file_suffix;
use crate::source_map::paths;
use std::collections::BTreeSet;
use tsc_diagnostics::{JsStr, JsString};

use tsc_diagnostics::{gen, sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList, MessageChain};
use tsc_program::SourceFileId;

use crate::{EmitContractViolation, EmitFailure, EmitHost, EmitSource, UnsupportedEmitFeature};

#[cfg(test)]
#[path = "../tests/unit/bundle_plan/tests.rs"]
mod tests;

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

impl EmitRoot {
    pub fn source_files(&self) -> &[SourceFileId] {
        match self {
            Self::SourceFile(source) => std::slice::from_ref(source),
            Self::Bundle(bundle) => bundle.source_files(),
        }
    }
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

/// Typed provenance for an intentionally absent JavaScript member.
///
/// The output-shape validator cannot reconstruct compiler options, so the
/// planner records the one currently admitted reason at the point where
/// `getOutputPathsFor` omits the JavaScript path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JavascriptOmission {
    EmitDeclarationOnly,
}

/// Full `getOutputPathsFor` plus build-info slot shape.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmitOutputPaths {
    javascript: Option<JsString>,
    javascript_map: Option<JsString>,
    declaration: Option<JsString>,
    declaration_map: Option<JsString>,
    build_info: Option<JsString>,
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

    pub fn javascript(path: impl Into<JsString>) -> Self {
        Self {
            javascript: Some(path.into()),
            ..Self::empty()
        }
    }

    pub fn with_javascript_map(mut self, path: impl Into<JsString>) -> Self {
        self.javascript_map = Some(path.into());
        self
    }

    pub fn with_declaration(mut self, path: impl Into<JsString>) -> Self {
        self.declaration = Some(path.into());
        self
    }

    pub fn with_declaration_map(mut self, path: impl Into<JsString>) -> Self {
        self.declaration_map = Some(path.into());
        self
    }

    pub fn with_build_info(mut self, path: impl Into<JsString>) -> Self {
        self.build_info = Some(path.into());
        self
    }

    pub fn javascript_path(&self) -> Option<JsStr<'_>> {
        self.javascript.as_ref().map(JsString::as_js)
    }

    pub fn javascript_map_path(&self) -> Option<JsStr<'_>> {
        self.javascript_map.as_ref().map(JsString::as_js)
    }

    pub fn declaration_path(&self) -> Option<JsStr<'_>> {
        self.declaration.as_ref().map(JsString::as_js)
    }

    pub fn declaration_map_path(&self) -> Option<JsStr<'_>> {
        self.declaration_map.as_ref().map(JsString::as_js)
    }

    pub fn build_info_path(&self) -> Option<JsStr<'_>> {
        self.build_info.as_ref().map(JsString::as_js)
    }
}

/// One source-file-or-bundle output unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitOutputUnit {
    root: EmitRoot,
    paths: EmitOutputPaths,
    mode: EmitMode,
    javascript_omitted: Option<JavascriptOmission>,
}

impl EmitOutputUnit {
    pub fn new(root: EmitRoot, paths: EmitOutputPaths, mode: EmitMode) -> Self {
        Self {
            root,
            paths,
            mode,
            javascript_omitted: None,
        }
    }

    /// tsc-port: getOutputPathsFor @6.0.3
    /// tsc-hash: f3ef9e378ec2b224d2f434b49f6ffd2a9597e7cc102f504653c9027a49c5ebd2
    /// tsc-span: _tsc.js:116373-116387
    pub fn with_javascript_omitted(mut self, omission: JavascriptOmission) -> Self {
        self.javascript_omitted = Some(omission);
        self
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

    pub const fn javascript_omitted(&self) -> Option<JavascriptOmission> {
        self.javascript_omitted
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
            if matches!(&unit.root, EmitRoot::Bundle(bundle) if bundle.source_files().is_empty()) {
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
            // h2-6a-m-3 G8: a planned `.js.map` is a supported unit member.
            // Declaration maps require their declaration text member.
            if unit.paths.declaration_map.is_some() && unit.paths.declaration.is_none() {
                return Err(EmitFailure::Unsupported(
                    UnsupportedEmitFeature::DeclarationMap,
                ));
            }
            if unit.paths.build_info.is_some() {
                return Err(EmitFailure::Unsupported(UnsupportedEmitFeature::BuildInfo));
            }
            if unit.paths.javascript.is_none()
                && unit.javascript_omitted != Some(JavascriptOmission::EmitDeclarationOnly)
            {
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
    blocked_outputs: BTreeSet<JsString>,
}

impl EmitPreflight {
    pub const fn plan(&self) -> &EmitOutputPlan {
        &self.plan
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn is_emit_blocked(&self, host: &dyn EmitHost, path: JsStr<'_>) -> bool {
        self.blocked_outputs
            .contains(&canonical_case_key(host, path))
    }
}

/// tsc-port: getSourceFilesToEmit @6.0.3
/// tsc-hash: bbfd59e2d4e5da3c2b08b243e7a31244a2df0a044f358ed5048781751ac40410
/// tsc-span: _tsc.js:16600-16616
pub fn get_source_files_to_emit(
    host: &dyn EmitHost,
    selection: EmitSelection,
) -> Result<Vec<SourceFileId>, EmitFailure> {
    select_source_files(host, selection, false)
}

pub(crate) fn get_source_files_for_forced_declaration_emit(
    host: &dyn EmitHost,
    selection: EmitSelection,
) -> Result<Vec<SourceFileId>, EmitFailure> {
    select_source_files(host, selection, true)
}

fn select_source_files(
    host: &dyn EmitHost,
    selection: EmitSelection,
    force_dts_emit: bool,
) -> Result<Vec<SourceFileId>, EmitFailure> {
    let bundle = active_out_file(host).is_some();
    let module_emit_enabled = host.compiler_options().emit_declaration_only == Some(true)
        || matches!(host.compiler_options().emit_module_kind(), 2 | 4);
    let candidates: Vec<SourceFileId> = match (bundle, selection) {
        (true, _) | (_, EmitSelection::WholeProgram) => host.source_file_ids().to_vec(),
        (false, EmitSelection::TargetSourceFile(source)) => vec![source],
    };
    candidates
        .into_iter()
        .filter_map(|id| match host.source_file(id) {
            Some(source)
                if if force_dts_emit {
                    source_file_may_emit_forced_declaration(source, host)
                } else {
                    source_file_may_be_emitted_for_host(source, host)
                } =>
            {
                // outFile always selects from the complete Program, even
                // when Program.emit receives a target source. Only AMD,
                // System, and declaration-only requests include modules.
                if bundle && !module_emit_enabled {
                    let Some(is_external_module) = source.is_external_module() else {
                        return Some(Err(EmitFailure::Contract(
                            EmitContractViolation::CheckedSyntaxUnavailable(id),
                        )));
                    };
                    if is_external_module {
                        return None;
                    }
                }
                Some(Ok(id))
            }
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
/// Shared with the compiler's common-source-directory projection, which
/// deliberately does not apply outFile's external-module selection filter.
#[doc(hidden)]
pub fn source_file_may_be_emitted_for_host(source: EmitSource<'_>, host: &dyn EmitHost) -> bool {
    tsc_program::source_file_may_be_emitted_for_options(
        source.path(),
        source.may_be_emitted(),
        host.compiler_options(),
        host.config_file_path(),
        host.current_directory(),
        host.use_case_sensitive_file_names(),
    )
}

fn no_emit_for_js_source(source: EmitSource<'_>, host: &dyn EmitHost) -> bool {
    if host.compiler_options().no_emit_for_js_files != Some(true) {
        return false;
    }
    let path = source.path();
    [".js", ".jsx", ".mjs", ".cjs", ".json"]
        .iter()
        .any(|extension| has_ascii_file_suffix(path, extension))
}

pub(crate) fn source_file_may_emit_forced_declaration(
    source: EmitSource<'_>,
    host: &dyn EmitHost,
) -> bool {
    !no_emit_for_js_source(source, host)
        && !is_declaration_file_name(source.path())
        && source.may_emit_forced_declaration()
}

/// tsc-port: getOutputPathsFor @6.0.3
/// tsc-hash: f3ef9e378ec2b224d2f434b49f6ffd2a9597e7cc102f504653c9027a49c5ebd2
/// tsc-span: _tsc.js:116373-116387
pub fn get_output_paths_for(
    source: EmitSource<'_>,
    host: &dyn EmitHost,
) -> Result<EmitOutputPaths, EmitFailure> {
    get_output_paths_for_with_force(source, host, false)
}

fn get_output_paths_for_with_force(
    source: EmitSource<'_>,
    host: &dyn EmitHost,
    force_dts_paths: bool,
) -> Result<EmitOutputPaths, EmitFailure> {
    let options = host.compiler_options();
    let extension = get_output_extension(source.path(), options.jsx)?;
    let javascript = get_own_emit_output_file_path(source.path(), host, extension);
    let is_json = extension == "json";
    let javascript = (!(options.emit_declaration_only.unwrap_or(false)
        || is_json
            && host.canonical_output_path(source.path())
                == host.canonical_output_path(javascript.as_js())))
    .then_some(javascript);

    let mut paths = match javascript {
        Some(path) => EmitOutputPaths::javascript(path),
        None => EmitOutputPaths::empty(),
    };
    // getOutputPathsFor forces an undefined map path for JSON source
    // files (116373-116387); getSourceMapFilePath (116388-116390) applies
    // only past that gate (h2-6a-m-3 G6).
    if options.source_map == Some(true) && options.inline_source_map != Some(true) && !is_json {
        if let Some(path) = paths.javascript_path().map(JsStr::to_owned) {
            paths = paths.with_javascript_map(paths::append_suffix(&path, ".map"));
        }
    }
    let declarations_enabled = options.declaration == Some(true) || options.composite == Some(true);
    if force_dts_paths || declarations_enabled && !is_json {
        let declaration = declaration_output_path(source.path(), host);
        if declarations_enabled && options.declaration_map == Some(true) {
            paths = paths.with_declaration_map(paths::append_suffix(&declaration, ".map"));
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
    action: impl FnMut(&EmitOutputPaths, &EmitRoot),
) -> Result<(), EmitFailure> {
    for_each_emitted_file_with_force(host, selection, false, action)
}

fn for_each_emitted_file_with_force(
    host: &dyn EmitHost,
    selection: EmitSelection,
    force_dts_paths: bool,
    mut action: impl FnMut(&EmitOutputPaths, &EmitRoot),
) -> Result<(), EmitFailure> {
    let source_files = if force_dts_paths {
        get_source_files_for_forced_declaration_emit(host, selection)?
    } else {
        get_source_files_to_emit(host, selection)?
    };
    if let Some(out_file) = active_out_file(host) {
        if !source_files.is_empty() {
            let paths =
                get_output_paths_for_bundle(host.compiler_options(), out_file, force_dts_paths);
            action(&paths, &EmitRoot::Bundle(EmitBundle::new(source_files)));
        }
        return Ok(());
    }

    for source_file in source_files {
        let source = host.source_file(source_file).ok_or(EmitFailure::Contract(
            EmitContractViolation::PlannedSourceMissing(source_file),
        ))?;
        let paths = get_output_paths_for_with_force(source, host, force_dts_paths)?;
        // Declaration-only requests still visit a source with no output
        // paths, so emitDeclarationFileOrBundle can mark it skipped.
        if host.compiler_options().emit_declaration_only == Some(true)
            || paths.javascript_path().is_some()
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

fn active_out_file(host: &dyn EmitHost) -> Option<JsStr<'_>> {
    host.compiler_options()
        .out_file
        .as_ref()
        .map(JsString::as_js)
        .filter(|path| !path.is_empty())
}

/// tsc-port: getOutputPathsForBundle @6.0.3
/// tsc-hash: c901ed763ea596470c0d7ac24a1dedf99dfcc4781e59eb4871e9b0743abc4775
/// tsc-span: _tsc.js:116365-116372
pub(crate) fn get_output_paths_for_bundle(
    options: &tsc_types::CompilerOptions,
    out_file: JsStr<'_>,
    force_dts_paths: bool,
) -> EmitOutputPaths {
    // The callback spelling is the raw outFile option. Only collision keys
    // resolve it against the Program directory; outDir/declarationDir do not
    // relocate bundle members.
    let mut paths = if options.emit_declaration_only == Some(true) {
        EmitOutputPaths::empty()
    } else {
        EmitOutputPaths::javascript(out_file)
    };
    if paths.javascript_path().is_some()
        && options.source_map == Some(true)
        && options.inline_source_map != Some(true)
    {
        paths = paths.with_javascript_map(paths::append_suffix(out_file, ".map"));
    }
    let declarations_enabled = options.declaration == Some(true) || options.composite == Some(true);
    if force_dts_paths || declarations_enabled {
        // removeFileExtension strips supported TypeScript extensions only,
        // case-sensitively, with declaration extensions preceding `.ts`.
        let extensionless = [
            ".d.ts", ".d.mts", ".d.cts", ".mjs", ".mts", ".cjs", ".cts", ".ts", ".js", ".tsx",
            ".jsx", ".json",
        ]
        .iter()
        .find_map(|extension| {
            (out_file.as_bytes().len() > extension.len())
                .then(|| out_file.strip_suffix(extension))
                .flatten()
        })
        .unwrap_or(out_file);
        let declaration = paths::append_suffix(extensionless, ".d.ts");
        if declarations_enabled && options.declaration_map == Some(true) {
            paths = paths.with_declaration_map(paths::append_suffix(&declaration, ".map"));
        }
        paths = paths.with_declaration(declaration);
    }
    paths
}

/// Build every output unit and run overwrite/duplicate-output validation
/// before transform, print, or sink dispatch begins.
pub fn preflight_emit(
    host: &dyn EmitHost,
    selection: EmitSelection,
) -> Result<EmitPreflight, EmitFailure> {
    let mut units = Vec::new();
    let emit_declaration_only = host.compiler_options().emit_declaration_only == Some(true);
    for_each_emitted_file(host, selection, |paths, root| {
        let mut unit = EmitOutputUnit::new(root.clone(), paths.clone(), EmitMode::Script);
        if emit_declaration_only && paths.javascript_path().is_none() {
            unit = unit.with_javascript_omitted(JavascriptOmission::EmitDeclarationOnly);
        }
        units.push(unit);
    })?;
    let plan = match selection {
        EmitSelection::WholeProgram => EmitOutputPlan::whole_program(units),
        EmitSelection::TargetSourceFile(source) => EmitOutputPlan::targeted(source, units),
    };

    let input_paths = host
        .source_file_ids()
        .iter()
        .filter_map(|id| host.source_file(*id))
        .map(|source| canonical_case_key(host, source.canonical_path()))
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
    // `suppressOutputPathCheck` is intentionally absent from the typed option
    // surface. Its upstream gate therefore reduces to `!noEmit` here.
    if options.no_emit != Some(true) {
        for unit in plan.units() {
            if options.emit_declaration_only != Some(true) {
                if let Some(path) = unit.paths().javascript_path() {
                    verify_emit_file_path(
                        host,
                        path,
                        &input_paths,
                        &mut emitted_paths,
                        &mut blocked_outputs,
                        &mut diagnostics,
                    );
                }
            }
            if options.declaration == Some(true) || options.composite == Some(true) {
                if let Some(path) = unit.paths().declaration_path() {
                    verify_emit_file_path(
                        host,
                        path,
                        &input_paths,
                        &mut emitted_paths,
                        &mut blocked_outputs,
                        &mut diagnostics,
                    );
                }
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

/// tsrs-native: forced path projection over the ordinary Program preflight.
/// Forced declarations retain the Program's ordinary whole-program blocked
/// paths. Forcing a new declaration path does not add it to that blocked set.
pub(crate) fn preflight_forced_declarations(
    host: &dyn EmitHost,
    selection: EmitSelection,
) -> Result<EmitPreflight, EmitFailure> {
    let mut preflight = preflight_emit(host, EmitSelection::WholeProgram)?;
    let mut units = Vec::new();
    for_each_emitted_file_with_force(host, selection, true, |paths, root| {
        let mut declaration_paths = EmitOutputPaths::empty().with_declaration(
            paths
                .declaration_path()
                .expect("forced declaration output path"),
        );
        if let Some(map) = paths.declaration_map_path() {
            declaration_paths = declaration_paths.with_declaration_map(map);
        }
        units.push(EmitOutputUnit::new(
            root.clone(),
            declaration_paths,
            EmitMode::DeclarationOnly,
        ));
    })?;
    preflight.plan = match selection {
        EmitSelection::WholeProgram => EmitOutputPlan::whole_program(units),
        EmitSelection::TargetSourceFile(source) => EmitOutputPlan::targeted(source, units),
    };
    Ok(preflight)
}

/// tsc-port: getOwnEmitOutputFilePath @6.0.3
/// tsc-hash: 4ddd1ea3136e64d8da7394a321fb709fffd279d36c8b456616956cdd82905b14
/// tsc-span: _tsc.js:16567-16576
fn get_own_emit_output_file_path(
    source_file: JsStr<'_>,
    host: &dyn EmitHost,
    extension: &'static str,
) -> JsString {
    let relocated = host
        .compiler_options()
        .out_dir
        .as_ref()
        .map(JsString::as_js)
        .filter(|directory| !directory.is_empty())
        .map(|out_dir| source_file_path_in_new_dir(source_file, host, out_dir))
        .unwrap_or_else(|| source_file.to_owned());
    paths::append_suffix(
        paths::remove_file_extension(&relocated).as_js(),
        &format!(".{extension}"),
    )
}

/// tsc-port: getOutputExtension @6.0.3
/// tsc-hash: cf61157be90d2652413f6d8ee13d05b2e76048b1f4ee38f8b620691af40632ce
/// tsc-span: _tsc.js:116391-116393
fn get_output_extension(path: JsStr<'_>, jsx: Option<i32>) -> Result<&'static str, EmitFailure> {
    let file_name = path;
    if has_ascii_file_suffix(file_name, ".json") {
        Ok("json")
    } else if (has_ascii_file_suffix(file_name, ".tsx") || has_ascii_file_suffix(file_name, ".jsx"))
        && jsx == Some(1)
    {
        Ok("jsx")
    } else if has_ascii_file_suffix(file_name, ".mts") || has_ascii_file_suffix(file_name, ".mjs") {
        Ok("mjs")
    } else if has_ascii_file_suffix(file_name, ".cts") || has_ascii_file_suffix(file_name, ".cjs") {
        Ok("cjs")
    } else if has_ascii_file_suffix(file_name, ".ts")
        || has_ascii_file_suffix(file_name, ".tsx")
        || has_ascii_file_suffix(file_name, ".js")
        || has_ascii_file_suffix(file_name, ".jsx")
    {
        Ok("js")
    } else {
        Err(EmitFailure::UnsupportedSourceExtension {
            path: path.to_owned(),
        })
    }
}

/// tsc-port: getDeclarationEmitOutputFilePath @6.0.3
/// tsc-hash: 151a4fa19404c1a458b703798d1ed757d717f8c180604136d049b7d4ad38d464
/// tsc-span: _tsc.js:16580-16591
pub(crate) fn declaration_output_path(source_file: JsStr<'_>, host: &dyn EmitHost) -> JsString {
    let options = host.compiler_options();
    let relocated = options
        .declaration_dir
        .as_ref()
        .map(JsString::as_js)
        .filter(|directory| !directory.is_empty())
        .or(options
            .out_dir
            .as_ref()
            .map(JsString::as_js)
            .filter(|directory| !directory.is_empty()))
        .map(|directory| source_file_path_in_new_dir(source_file, host, directory))
        .unwrap_or_else(|| source_file.to_owned());
    let lower = relocated.as_js();
    let extension = if has_ascii_file_suffix(lower, ".mts") || has_ascii_file_suffix(lower, ".mjs")
    {
        "d.mts"
    } else if has_ascii_file_suffix(lower, ".cts") || has_ascii_file_suffix(lower, ".cjs") {
        "d.cts"
    } else if has_ascii_file_suffix(lower, ".json") {
        "d.json.ts"
    } else {
        "d.ts"
    };
    paths::append_suffix(
        paths::remove_file_extension(&relocated).as_js(),
        &format!(".{extension}"),
    )
}

/// tsc-port: verifyEmitFilePath @6.0.3
/// tsc-hash: 89b85e5f8bf04e3625f8bddd1b2a226caa8cd308eb76180bc55db8ac597fdb50
/// tsc-span: _tsc.js:125018-125045
fn verify_emit_file_path(
    host: &dyn EmitHost,
    path: JsStr<'_>,
    input_paths: &BTreeSet<JsString>,
    emitted_paths: &mut BTreeSet<JsString>,
    blocked_outputs: &mut BTreeSet<JsString>,
    diagnostics: &mut DiagnosticList,
) {
    let canonical = canonical_case_key(host, path);
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

fn canonical_case_key(host: &dyn EmitHost, path: JsStr<'_>) -> JsString {
    let canonical = host.canonical_output_path(path);
    tsc_program::canonical_emit_path(
        canonical.as_js(),
        host.current_directory(),
        host.use_case_sensitive_file_names(),
    )
}

fn source_file_path_in_new_dir(
    source_file: JsStr<'_>,
    host: &dyn EmitHost,
    output_directory: JsStr<'_>,
) -> JsString {
    let common = host.common_source_directory();
    let common = if common.is_empty() {
        common.to_owned()
    } else {
        paths::ensure_trailing_directory_separator(common)
    };
    tsc_program::source_file_path_in_new_directory(
        source_file,
        output_directory,
        common.as_js(),
        host.current_directory(),
        host.use_case_sensitive_file_names(),
    )
}

fn is_declaration_file_name(path: JsStr<'_>) -> bool {
    let path = paths::normalize_slashes(path);
    let name = path.as_js();
    let base = name.split_ascii(b'/').next_back().unwrap_or(name);
    [".d.ts", ".d.mts", ".d.cts"]
        .into_iter()
        .any(|ext| has_ascii_file_suffix(name, ext))
        || base
            .as_bytes()
            .windows(3)
            .any(|bytes| bytes.eq_ignore_ascii_case(b".d."))
            && has_ascii_file_suffix(base, ".ts")
}

fn compiler_diagnostic(
    message: &'static tsc_diagnostics::DiagnosticMessage,
    path: JsStr<'_>,
) -> Diagnostic {
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new_js(message, &[path.to_owned()]),
    )
}

fn option_diagnostic(message: &'static tsc_diagnostics::DiagnosticMessage) -> Diagnostic {
    Diagnostic::new(None, None, None, MessageChain::new(message, &[]))
}

fn overwrite_input_diagnostic(host: &dyn EmitHost, path: JsStr<'_>) -> Diagnostic {
    let mut message = MessageChain::new_js(
        &gen::Cannot_write_file_0_because_it_would_overwrite_input_file,
        &[path.to_owned()],
    );
    if host.config_file_path().is_none() {
        message = message.with_next(vec![MessageChain::new(
            &gen::Adding_a_tsconfig_json_file_will_help_organize_projects_that_contain_both_TypeScript_and_JavaScript_files_Learn_more_at_https_aka_ms_tsconfig,
            &[],
        )]);
    }
    Diagnostic::new(None, None, None, message)
}
