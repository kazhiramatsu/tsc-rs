use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use tsc_checker::InputFile;
use tsc_compiler::ProgramSession;
use tsc_diagnostics::{gen, Diagnostic, MessageChain};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    plan_source_requests, HostModuleResolution, HostResolvedTypeReferenceDirective,
    ModuleExtension, ModuleResolution, ModuleResolver, PackageId, PackageJsonType, PackageMetadata,
    PlannedTypeReferenceDirective, PreparedProgram, PreparedSourceFile, ProgramOptions,
    ProgramPath, ResolutionError, ResolutionMode, ResolutionOutcome, ResolvedModuleTarget,
    ResolvedTypeReferenceDirective, SourceFileId, TypeReferenceResolution,
    TypeReferenceResolutionKey, UnloadedModuleReason,
};

use crate::ConformanceResult;

const SUPPORTED_FIXTURES: [&str; 51] = [
    "conformance/node/nodeModulesPackagePatternExportsExclude.ts",
    "conformance/node/nodeModulesPackagePatternExports.ts",
    "conformance/node/allowJs/nodeModulesAllowJsPackagePatternExportsExclude.ts",
    "conformance/node/allowJs/nodeModulesAllowJsPackagePatternExports.ts",
    "conformance/node/nodeModulesExportsBlocksSpecifierResolution.ts",
    "conformance/node/nodeModulesExportsBlocksTypesVersions.ts",
    "conformance/node/nodeModulesExportsDoubleAsterisk.ts",
    "conformance/node/nodeModulesExportsSourceTs.ts",
    "conformance/node/nodeModulesExportsSpecifierGenerationDirectory.ts",
    "conformance/node/nodeModulesExportsSpecifierGenerationPattern.ts",
    "conformance/node/nodeModulesDeclarationEmitWithPackageExports.ts",
    "conformance/node/nodeModulesPackageExports.ts",
    "conformance/node/nodeModulesPackagePatternExportsTrailers.ts",
    "conformance/node/nodeModulesTypesVersionPackageExports.ts",
    "conformance/externalModules/rewriteRelativeImportExtensions/packageJsonImportsErrors.ts",
    "conformance/moduleResolution/bundler/bundlerCommonJS.ts",
    "conformance/moduleResolution/conditionalExportsResolutionFallbackNull.ts",
    "conformance/node/nodeModulesExportsSpecifierGenerationConditions.ts",
    "conformance/node/nodeModulesImportResolutionIntoExport.ts",
    "conformance/node/nodeModulesImportResolutionNoCycle.ts",
    "conformance/node/nodeModulesPackageImportsRootWildcardNode16.ts",
    "conformance/moduleResolution/bundler/bundlerConditionsExcludesNode.ts",
    "conformance/moduleResolution/conditionalExportsResolutionFallback.ts",
    "conformance/node/nodeModulesConditionalPackageExports.ts",
    "conformance/node/nodePackageSelfName.ts",
    "conformance/node/nodeModulesPackageImports.ts",
    "conformance/node/nodeModulesPackageImportsRootWildcard.ts",
    "conformance/node/allowJs/nodeModulesAllowJsPackageImports.ts",
    "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts",
    "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToUnmapped.ts",
    "conformance/externalModules/rewriteRelativeImportExtensions/nodeModulesTsFiles.ts",
    "conformance/moduleResolution/packageJsonMain_isNonRecursive.ts",
    "conformance/moduleResolution/packageJsonMain.ts",
    "conformance/node/nodeModulesNoDirectoryModule.ts",
    "conformance/jsdoc/importTag17.ts",
    "conformance/typings/typingsLookup1.ts",
    "conformance/typings/typingsLookup3.ts",
    "conformance/externalModules/verbatimModuleSyntaxAmbientConstEnum.ts",
    "conformance/externalModules/verbatimModuleSyntaxConstEnumUsage.ts",
    "conformance/classes/members/privateNames/privateNameEmitHelpers.ts",
    "conformance/classes/members/privateNames/privateNameStaticEmitHelpers.ts",
    "conformance/es2020/modules/exportAsNamespace_missingEmitHelpers.ts",
    "conformance/moduleResolution/resolutionModeImportType1.ts",
    "conformance/moduleResolution/resolutionModeTypeOnlyImport1.ts",
    "conformance/moduleResolution/node10AlternateResult_noResolution.ts",
    "conformance/moduleResolution/node10Alternateresult_noTypes.ts",
    "conformance/salsa/namespaceAssignmentToRequireAlias.ts",
    "conformance/moduleResolution/untypedModuleImport_allowJs.ts",
    "conformance/moduleResolution/untypedModuleImport_withAugmentation.ts",
    "conformance/moduleResolution/untypedModuleImport.ts",
    "conformance/moduleResolution/untypedModuleImport_vsAmbient.ts",
];

pub(crate) fn supports_fixture(fixture: &str) -> bool {
    SUPPORTED_FIXTURES.contains(&fixture)
}

pub(crate) struct H0MemoryCase {
    pub all: Vec<Diagnostic>,
    pub syntactic: Vec<Diagnostic>,
}

#[derive(Debug)]
enum H0MemoryError {
    InvalidPath { path: String, detail: &'static str },
}

impl fmt::Display for H0MemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath { path, detail } => {
                write!(formatter, "invalid H0 memory path {path:?}: {detail}")
            }
        }
    }
}

impl Error for H0MemoryError {}

#[derive(Clone)]
struct DecodedSource {
    display: String,
    canonical: PathBuf,
    text: String,
}

#[derive(Clone)]
struct OwnedProgramSource {
    prepared: PreparedSourceFile,
}

pub(crate) fn run(
    program: &tsc_harness::ProgramJson,
    libs: &[InputFile],
    files: &[InputFile],
) -> ConformanceResult<H0MemoryCase> {
    let current_directory = normalize_current_directory(&program.cwd)?;
    let decoded_libs = libs
        .iter()
        .map(|lib| {
            Ok(DecodedSource {
                display: lib.name.clone(),
                canonical: normalize_source_path(&current_directory, &lib.name)?,
                text: lib.text.clone(),
            })
        })
        .collect::<Result<Vec<_>, H0MemoryError>>()?;
    let decoded_files = files
        .iter()
        .map(|file| {
            Ok(DecodedSource {
                display: file.name.clone(),
                canonical: normalize_source_path(&current_directory, &file.name)?,
                text: file.text.clone(),
            })
        })
        .collect::<Result<Vec<_>, H0MemoryError>>()?;

    let mut host_builder = MemoryCompilerHost::builder(&current_directory).case_sensitive(true);
    let mut trailing_directory_aliases = BTreeSet::new();
    for source in decoded_libs.iter().chain(&decoded_files) {
        host_builder = host_builder.file(&source.canonical, source.text.as_bytes().to_vec());
        // The official harness VFS treats a directory spelling with or
        // without its trailing separator as the same directory. The exact
        // MemoryCompilerHost deliberately does not normalize host queries,
        // so publish both spellings while mounting the harness tree. This is
        // observable for `../` package-root back-references: tsc preserves
        // the trailing separator before asking `directoryExists`.
        for directory in source.canonical.ancestors().skip(1) {
            if directory == Path::new("/") {
                continue;
            }
            let directory = directory
                .to_str()
                .ok_or_else(|| H0MemoryError::InvalidPath {
                    path: source.display.clone(),
                    detail: "normalized source parent is not Unicode",
                })?;
            trailing_directory_aliases.insert(PathBuf::from(format!("{directory}/")));
        }
    }
    for directory in trailing_directory_aliases {
        host_builder = host_builder.directory(directory);
    }
    let host = host_builder.build()?;

    let mut options = tsc_harness::compiler_options_from_program(program);
    options.no_emit = Some(true);
    let program_options = program_options_from_program(program, &current_directory)?;
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)?;
    let mut prepared_builder =
        PreparedProgram::builder(resolver.path_context().clone(), options.clone());
    prepared_builder.set_program_options(program_options.clone());

    let mut source_by_canonical = BTreeMap::<PathBuf, (SourceFileId, ProgramPath)>::new();
    for source in &decoded_libs {
        let path = public_program_path(source)?;
        let source_id = prepared_builder
            .add_source_file(PreparedSourceFile::new(path.clone(), source.text.clone()))?;
        prepared_builder.add_library_file(source_id)?;
        source_by_canonical.insert(source.canonical.clone(), (source_id, path));
    }

    let mut owned_program_sources = Vec::with_capacity(decoded_files.len());
    for source in decoded_files
        .iter()
        .filter(|source| is_program_source(&source.display, &options))
    {
        // package_scope_for_file is the single JSON validation and package
        // scope boundary shared with resolution. In particular, a nearest
        // package with no `type` field stops the search.
        let package_scope = resolver.package_scope_for_file(&source.canonical)?;
        let implied_node_format =
            implied_node_format(&source.display, package_scope.as_ref(), &options);
        let implied_node_format_for_emit =
            implied_node_format_for_emit(&source.display, package_scope.as_ref(), &options);
        let path = public_program_path(source)?;
        let mut prepared = PreparedSourceFile::new(path.clone(), source.text.clone())
            .with_implied_node_formats(implied_node_format, implied_node_format_for_emit);
        if let Some(scope) = package_scope.as_ref() {
            prepared = prepared.with_package_scope(scope.package_json().canonical().clone());
        }
        let source_id = prepared_builder.add_source_file(prepared.clone())?;
        prepared_builder.add_root_file(source_id)?;
        source_by_canonical.insert(source.canonical.clone(), (source_id, path.clone()));
        owned_program_sources.push(OwnedProgramSource { prepared });
    }

    let mut host_resolutions = Vec::new();
    let mut type_reference_indices = BTreeMap::<TypeReferenceResolutionKey, usize>::new();
    let mut host_type_reference_resolutions = Vec::<(
        TypeReferenceResolutionKey,
        ResolutionOutcome<HostResolvedTypeReferenceDirective>,
        Vec<Diagnostic>,
    )>::new();
    for source in &owned_program_sources {
        if source
            .prepared
            .path()
            .display()
            .to_str()
            .is_some_and(is_json_file)
        {
            continue;
        }
        let plan = plan_source_requests(&source.prepared, &options)?;
        for (key, loads_source) in plan.module_requests_with_loadability() {
            let specifier = key.specifier().to_owned();
            let host_outcome = resolver.resolve_with_facts(
                source.prepared.path().canonical().as_path(),
                &specifier,
                key.mode(),
            )?;
            host_resolutions.push((key.clone(), host_outcome, loads_source));
        }
        for directive in plan.type_reference_directives() {
            let key = directive.key().clone();
            let index = if let Some(index) = type_reference_indices.get(&key).copied() {
                index
            } else {
                let host_outcome = resolver.resolve_type_reference(
                    source.prepared.path().canonical().as_path(),
                    key.specifier(),
                    key.mode(),
                    program_options.type_roots(),
                )?;
                let index = host_type_reference_resolutions.len();
                host_type_reference_resolutions.push((key.clone(), host_outcome, Vec::new()));
                type_reference_indices.insert(key.clone(), index);
                index
            };
            if matches!(
                &host_type_reference_resolutions[index].1,
                ResolutionOutcome::NotFound
            ) {
                host_type_reference_resolutions[index].2.push(
                    unresolved_type_reference_diagnostic(&source.prepared, directive)?,
                );
            }
        }
    }

    // tsc's host package map is a fold over the complete resolved-module
    // table, so diagnostic facts cannot be finalized while rows are still
    // being discovered.
    let package_map =
        package_map_from_facts(host_resolutions.iter().filter_map(|(_, outcome, _)| {
            let ResolutionOutcome::Resolved(module) = outcome.outcome() else {
                return None;
            };
            Some((module.package_id()?, module.extension()))
        }));
    for (key, host_outcome, loads_source) in host_resolutions {
        let resolution = bind_host_outcome(
            host_outcome,
            &source_by_canonical,
            &options,
            &package_map,
            loads_source,
        )?;
        prepared_builder.add_module_resolution(key, Ok(resolution))?;
    }
    for (key, host_outcome, diagnostics) in host_type_reference_resolutions {
        let resolution = bind_type_reference_host_outcome(host_outcome, &source_by_canonical)?
            .with_diagnostics(diagnostics);
        prepared_builder.add_type_reference_resolution(key, Ok(resolution))?;
    }

    let packages = resolver
        .observed_package_metadata()
        .cloned()
        .collect::<Vec<_>>();
    drop(resolver);
    for package in packages {
        prepared_builder.add_package_metadata(package)?;
    }

    let prepared = prepared_builder.build()?;
    let outcome = ProgramSession::new(prepared).run_for_conformance_with_harness_lib_cache()?;
    let syntactic = outcome.syntactic_diagnostics().to_vec();
    let all = outcome.conformance_diagnostics().to_vec();
    Ok(H0MemoryCase { all, syntactic })
}

fn public_program_path(source: &DecodedSource) -> Result<ProgramPath, H0MemoryError> {
    ProgramPath::from_trusted_parts(&source.display, &source.canonical).map_err(|_| {
        H0MemoryError::InvalidPath {
            path: source.display.clone(),
            detail: "program path rejected the normalized public/canonical pair",
        }
    })
}

fn program_options_from_program(
    program: &tsc_harness::ProgramJson,
    current_directory: &Path,
) -> Result<ProgramOptions, H0MemoryError> {
    let mut options = ProgramOptions::default();
    if let Some(tsc_harness::OptionValue::Bool(no_lib)) = program_option(&program.options, "noLib")
    {
        options = options.with_no_lib(*no_lib);
    }
    if let Some(type_roots) = string_list_program_option(&program.options, "typeRoots") {
        let type_roots = type_roots
            .iter()
            .map(|root| option_program_path(current_directory, root))
            .collect::<Result<Vec<_>, _>>()?;
        options = options.with_type_roots(type_roots);
    }
    if let Some(types) = string_list_program_option(&program.options, "types") {
        options = options.with_types(types.to_vec());
    }
    Ok(options)
}

fn program_option<'a>(
    options: &'a BTreeMap<String, tsc_harness::OptionValue>,
    name: &str,
) -> Option<&'a tsc_harness::OptionValue> {
    options
        .iter()
        .find_map(|(candidate, value)| candidate.eq_ignore_ascii_case(name).then_some(value))
        .filter(|value| !matches!(value, tsc_harness::OptionValue::Null))
}

fn string_list_program_option<'a>(
    options: &'a BTreeMap<String, tsc_harness::OptionValue>,
    name: &str,
) -> Option<&'a [String]> {
    match program_option(options, name)? {
        tsc_harness::OptionValue::StringList(values) => Some(values),
        _ => None,
    }
}

fn option_program_path(current_directory: &Path, path: &str) -> Result<ProgramPath, H0MemoryError> {
    let canonical = normalize_source_path(current_directory, path)?;
    ProgramPath::from_trusted_parts(canonical.clone(), canonical).map_err(|_| {
        H0MemoryError::InvalidPath {
            path: path.to_owned(),
            detail: "program option path rejected the normalized public/canonical pair",
        }
    })
}

fn bind_host_outcome(
    outcome: HostModuleResolution,
    source_by_canonical: &BTreeMap<PathBuf, (SourceFileId, ProgramPath)>,
    options: &tsc_program::CompilerOptions,
    package_map: &BTreeMap<String, bool>,
    loads_source: bool,
) -> Result<ModuleResolution, ResolutionError> {
    let alternate_result = outcome.alternate_result().cloned();
    let ResolutionOutcome::Resolved(host_module) = outcome.into_outcome() else {
        let mut resolution = ModuleResolution::not_found();
        if let Some(alternate_result) = alternate_result {
            resolution = resolution.with_alternate_result(alternate_result);
        }
        return Ok(resolution);
    };
    let (types_package_exists, package_bundles_types) =
        host_module
            .package_id()
            .map_or((false, false), |package_id| {
                (
                    package_map.contains_key(&types_package_name(package_id.name())),
                    package_map.get(package_id.name()).copied().unwrap_or(false),
                )
            });
    let target_canonical = host_module.resolved_file().canonical().as_path();
    let owned_source = source_by_canonical.get(target_canonical);
    let target = if host_module.extension().is_javascript() && owned_source.is_none() {
        let reason = if matches!(host_module.extension(), ModuleExtension::Jsx)
            && options.jsx.unwrap_or(0) == 0
        {
            UnloadedModuleReason::JsxWithoutJsxOption
        } else if !loads_source {
            UnloadedModuleReason::ResolutionOnly
        } else if host_module.is_external_library_import()
            && (host_module.original_path().is_none()
                || target_canonical.to_str().is_some_and(|path| {
                    path.split('/').any(|component| component == "node_modules")
                }))
        {
            UnloadedModuleReason::NodeModulesDepth
        } else if !options.allow_js {
            UnloadedModuleReason::JavaScriptNotAdmitted
        } else {
            return Err(ResolutionError::invalid_data(format!(
                "resolved JavaScript source {} is not owned by the prepared program",
                host_module.resolved_file().display().display()
            )));
        };
        if host_module.original_path().is_some()
            && !matches!(reason, UnloadedModuleReason::JsxWithoutJsxOption)
        {
            return Err(ResolutionError::unsupported(
                "unloaded-original-path",
                format!(
                    "unloaded JavaScript target {} retains an unsupported lexical-to-physical transition",
                    host_module.resolved_file().display().display()
                ),
            ));
        }
        ResolvedModuleTarget::Unloaded {
            resolved_file: host_module.resolved_file().clone(),
            reason,
        }
    } else if let Some((target_source, target_path)) = owned_source {
        ResolvedModuleTarget::Source {
            source: *target_source,
            // ProgramSession requires the resolved-file spelling to be exactly
            // the owned SourceFile spelling, while HostResolvedModule validates
            // that its canonical identity still matches the probed host path.
            resolved_file: target_path.clone(),
        }
    } else {
        return Err(ResolutionError::invalid_data(format!(
            "resolved source {} is not owned by the prepared program",
            host_module.resolved_file().display().display()
        )));
    };
    let resolved = host_module.into_resolved_module(target)?;
    let mut resolution = ModuleResolution::resolved(resolved)
        .with_types_package_exists(types_package_exists)
        .with_package_bundles_types(package_bundles_types);
    if let Some(alternate_result) = alternate_result {
        resolution = resolution.with_alternate_result(alternate_result);
    }
    Ok(resolution)
}

fn bind_type_reference_host_outcome(
    outcome: ResolutionOutcome<HostResolvedTypeReferenceDirective>,
    source_by_canonical: &BTreeMap<PathBuf, (SourceFileId, ProgramPath)>,
) -> Result<TypeReferenceResolution, ResolutionError> {
    let ResolutionOutcome::Resolved(host_directive) = outcome else {
        return Ok(TypeReferenceResolution::not_found());
    };
    if !is_loadable_type_reference_extension(host_directive.extension()) {
        return Err(ResolutionError::invalid_data(format!(
            "type-reference target {} is not a TypeScript source file",
            host_directive.resolved_file().display().display()
        )));
    }
    let target_canonical = host_directive.resolved_file().canonical().as_path();
    let Some((source, target)) = source_by_canonical.get(target_canonical) else {
        return Err(ResolutionError::invalid_data(format!(
            "resolved type-reference source {} is not owned by the prepared program",
            host_directive.resolved_file().display().display()
        )));
    };
    let mut directive = ResolvedTypeReferenceDirective::new(target.clone(), *source)
        .with_primary(host_directive.primary())
        .with_external_library_import(host_directive.is_external_library_import());
    if let Some(original_path) = host_directive.original_path() {
        directive = directive.with_original_path(original_path.clone());
    }
    if let Some(package_id) = host_directive.package_id() {
        directive = directive.with_package_id(package_id.clone());
    }
    Ok(TypeReferenceResolution::resolved(directive))
}

fn is_loadable_type_reference_extension(extension: &ModuleExtension) -> bool {
    matches!(
        extension,
        ModuleExtension::Ts
            | ModuleExtension::Tsx
            | ModuleExtension::Dts
            | ModuleExtension::Mts
            | ModuleExtension::Dmts
            | ModuleExtension::Cts
            | ModuleExtension::Dcts
    ) || matches!(
        extension,
        ModuleExtension::Arbitrary(arbitrary)
            if arbitrary.starts_with(".d.") && arbitrary.ends_with(".ts")
    )
}

fn unresolved_type_reference_diagnostic(
    source: &PreparedSourceFile,
    directive: &PlannedTypeReferenceDirective,
) -> Result<Diagnostic, ResolutionError> {
    let file_name = source.path().display().to_str().ok_or_else(|| {
        ResolutionError::invalid_data("type-reference diagnostic source is not valid Unicode")
    })?;
    let args = [directive.key().specifier().to_owned()];
    Ok(Diagnostic::new(
        Some(file_name.to_owned()),
        Some(directive.pos()),
        Some(directive.length()),
        MessageChain::new(&gen::Cannot_find_type_definition_file_for_0, &args),
    ))
}

/// tsc-port: getPackagesMap/packageBundlesTypes/typesPackageExists @6.0.3
/// tsc-hash: 74ad8cc4b534899ed13e5017004887e4e20e3faa0a5d0cdfa50d6a1983d292db
/// tsc-span: _tsc.js:123041-123054
fn package_map_from_facts<'a>(
    facts: impl IntoIterator<Item = (&'a PackageId, &'a ModuleExtension)>,
) -> BTreeMap<String, bool> {
    let mut packages = BTreeMap::new();
    for (package_id, extension) in facts {
        let bundles_declaration = matches!(extension, ModuleExtension::Dts);
        packages
            .entry(package_id.name().to_owned())
            .and_modify(|existing| *existing |= bundles_declaration)
            .or_insert(bundles_declaration);
    }
    packages
}

fn types_package_name(package_name: &str) -> String {
    let mangled = match package_name.strip_prefix('@') {
        Some(scoped) => scoped.replace('/', "__"),
        None => package_name.to_owned(),
    };
    format!("@types/{mangled}")
}

fn implied_node_format(
    file_name: &str,
    package_scope: Option<&PackageMetadata>,
    options: &tsc_program::CompilerOptions,
) -> Option<ResolutionMode> {
    if file_name.ends_with(".d.mts") || file_name.ends_with(".mts") || file_name.ends_with(".mjs") {
        return Some(ResolutionMode::EsNext);
    }
    if file_name.ends_with(".d.cts") || file_name.ends_with(".cts") || file_name.ends_with(".cjs") {
        return Some(ResolutionMode::CommonJs);
    }
    if file_name.ends_with(".d.ts")
        || file_name.ends_with(".ts")
        || file_name.ends_with(".tsx")
        || file_name.ends_with(".js")
        || file_name.ends_with(".jsx")
    {
        let package_lookup = matches!(options.emit_module_resolution_kind(), 3..=99)
            || file_name
                .split('/')
                .any(|segment| segment == "node_modules");
        if !package_lookup {
            return None;
        }
        return Some(
            if package_scope.is_some_and(|scope| scope.module_type() == PackageJsonType::Module) {
                ResolutionMode::EsNext
            } else {
                ResolutionMode::CommonJs
            },
        );
    }
    None
}

fn implied_node_format_for_emit(
    file_name: &str,
    package_scope: Option<&PackageMetadata>,
    options: &tsc_program::CompilerOptions,
) -> Option<ResolutionMode> {
    let implied = implied_node_format(file_name, package_scope, options)?;
    if (100..=199).contains(&options.emit_module_kind())
        || [".mts", ".mjs", ".cts", ".cjs"]
            .iter()
            .any(|extension| file_name.ends_with(extension))
    {
        return Some(implied);
    }
    match package_scope.map(PackageMetadata::module_type) {
        Some(PackageJsonType::Module | PackageJsonType::CommonJs) => Some(implied),
        Some(PackageJsonType::Other | PackageJsonType::Unspecified) | None => None,
    }
}

fn is_json_file(file_name: &str) -> bool {
    file_name.ends_with(".json")
}

fn is_program_source(file_name: &str, options: &tsc_program::CompilerOptions) -> bool {
    if is_json_file(file_name) {
        return false;
    }
    if [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|extension| file_name.ends_with(extension))
    {
        return options.allow_js;
    }
    true
}

fn normalize_current_directory(cwd: &str) -> Result<PathBuf, H0MemoryError> {
    if !cwd.starts_with('/') {
        return Err(H0MemoryError::InvalidPath {
            path: cwd.to_owned(),
            detail: "current directory must be absolute POSIX",
        });
    }
    normalize_absolute_posix(cwd)
}

fn normalize_source_path(cwd: &Path, file_name: &str) -> Result<PathBuf, H0MemoryError> {
    if file_name.is_empty() {
        return Err(H0MemoryError::InvalidPath {
            path: file_name.to_owned(),
            detail: "source file name is empty",
        });
    }
    if file_name.starts_with('/') {
        return normalize_absolute_posix(file_name);
    }
    let cwd = cwd.to_str().ok_or_else(|| H0MemoryError::InvalidPath {
        path: cwd.display().to_string(),
        detail: "normalized current directory is not Unicode",
    })?;
    normalize_absolute_posix(&format!("{cwd}/{file_name}"))
}

fn normalize_absolute_posix(path: &str) -> Result<PathBuf, H0MemoryError> {
    if path.contains('\\') || path.contains('\0') {
        return Err(H0MemoryError::InvalidPath {
            path: path.to_owned(),
            detail: "paths must be NUL-free POSIX spellings",
        });
    }
    if !path.starts_with('/') {
        return Err(H0MemoryError::InvalidPath {
            path: path.to_owned(),
            detail: "path must be absolute",
        });
    }
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(H0MemoryError::InvalidPath {
                        path: path.to_owned(),
                        detail: "path traverses above the POSIX root",
                    });
                }
            }
            segment => segments.push(segment),
        }
    }
    Ok(if segments.is_empty() {
        PathBuf::from("/")
    } else {
        PathBuf::from(format!("/{}", segments.join("/")))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        bind_type_reference_host_outcome, implied_node_format, implied_node_format_for_emit,
        package_map_from_facts, program_options_from_program, supports_fixture, types_package_name,
        SUPPORTED_FIXTURES,
    };
    use std::collections::BTreeMap;
    use std::path::Path;
    use tsc_harness::{OptionValue, ProgramJson};
    use tsc_host::MemoryCompilerHost;
    use tsc_program::{
        CompilerOptions, ModuleExtension, ModuleResolver, PackageId, PackageJsonType,
        PackageMetadata, ProgramPath, ResolutionMode, ResolutionOutcome, SourceFileId,
    };

    #[test]
    fn dedicated_route_is_exactly_the_reviewed_h0_fixtures() {
        assert!(SUPPORTED_FIXTURES
            .iter()
            .all(|fixture| supports_fixture(fixture)));
        for fixture in [
            "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts",
            "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToUnmapped.ts",
            "conformance/externalModules/rewriteRelativeImportExtensions/nodeModulesTsFiles.ts",
            "conformance/moduleResolution/packageJsonMain_isNonRecursive.ts",
            "conformance/moduleResolution/packageJsonMain.ts",
            "conformance/node/nodeModulesNoDirectoryModule.ts",
            "conformance/node/nodeModulesPackageExports.ts",
            "conformance/jsdoc/importTag17.ts",
            "conformance/typings/typingsLookup1.ts",
            "conformance/typings/typingsLookup3.ts",
            "conformance/externalModules/verbatimModuleSyntaxAmbientConstEnum.ts",
            "conformance/externalModules/verbatimModuleSyntaxConstEnumUsage.ts",
            "conformance/classes/members/privateNames/privateNameEmitHelpers.ts",
            "conformance/classes/members/privateNames/privateNameStaticEmitHelpers.ts",
            "conformance/es2020/modules/exportAsNamespace_missingEmitHelpers.ts",
            "conformance/moduleResolution/resolutionModeImportType1.ts",
            "conformance/moduleResolution/resolutionModeTypeOnlyImport1.ts",
            "conformance/moduleResolution/node10AlternateResult_noResolution.ts",
            "conformance/moduleResolution/node10Alternateresult_noTypes.ts",
            "conformance/salsa/namespaceAssignmentToRequireAlias.ts",
            "conformance/moduleResolution/untypedModuleImport_allowJs.ts",
            "conformance/moduleResolution/untypedModuleImport_withAugmentation.ts",
            "conformance/moduleResolution/untypedModuleImport.ts",
            "conformance/moduleResolution/untypedModuleImport_vsAmbient.ts",
        ] {
            assert!(supports_fixture(fixture), "missing H0 route: {fixture}");
        }
        for fixture in [
            "conformance/node/allowJs/nodeModulesAllowJsPackagePatternExportsTrailers.ts",
            "conformance/externalModules/rewriteRelativeImportExtensions/nonTSExtensions.ts",
            "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts.backup",
            "conformance/moduleResolution/packageJsonMain_isNonRecursive.ts.backup",
            "conformance/node/nodeModulesPackagePatternExportsExclude.ts.backup",
            "conformance/externalModules/verbatimModuleSyntaxConstEnum.ts",
            "node/nodeModulesPackagePatternExportsExclude.ts",
        ] {
            assert!(!supports_fixture(fixture), "unexpected H0 route: {fixture}");
        }
    }

    #[test]
    fn h0_type_reference_binding_accepts_all_typescript_source_extensions() {
        let host = MemoryCompilerHost::builder("/work")
            .file("/work/root.ts", b"export {};".to_vec())
            .file(
                "/work/node_modules/@types/implementation/package.json",
                br#"{"name":"@types/implementation","version":"1.0.0","types":"index.ts"}"#
                    .to_vec(),
            )
            .file(
                "/work/node_modules/@types/implementation/index.ts",
                b"declare const implementation: true;".to_vec(),
            )
            .file(
                "/work/node_modules/@types/styles/package.json",
                br#"{"name":"@types/styles","version":"1.0.0","types":"index.css"}"#.to_vec(),
            )
            .file(
                "/work/node_modules/@types/styles/index.d.css.ts",
                b"declare const styles: true;".to_vec(),
            )
            .build()
            .expect("build H0 type-reference host");
        let options = CompilerOptions {
            module: Some(199),
            module_resolution: Some(99),
            ..CompilerOptions::default()
        };
        let mut resolver = ModuleResolver::new(&host, &options).expect("create H0 resolver");

        for (index, (name, expected_path, expected_extension)) in [
            (
                "implementation",
                "/work/node_modules/@types/implementation/index.ts",
                ModuleExtension::Ts,
            ),
            (
                "styles",
                "/work/node_modules/@types/styles/index.d.css.ts",
                ModuleExtension::Arbitrary(".d.css.ts".to_owned()),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let outcome = resolver
                .resolve_type_reference(
                    Path::new("/work/root.ts"),
                    name,
                    ResolutionMode::EsNext,
                    None,
                )
                .expect("resolve H0 type-reference target");
            let ResolutionOutcome::Resolved(host_directive) = &outcome else {
                panic!("expected resolved type-reference target: {name}");
            };
            assert_eq!(host_directive.extension(), &expected_extension);
            let target = ProgramPath::from_trusted_parts(expected_path, expected_path)
                .expect("construct target identity");
            let source = SourceFileId::from_raw(
                u32::try_from(index + 1).expect("the focused source id fits u32"),
            );
            let source_by_canonical =
                BTreeMap::from([(target.canonical().as_path().to_path_buf(), (source, target))]);
            assert!(matches!(
                bind_type_reference_host_outcome(outcome, &source_by_canonical)
                    .expect("bind H0 type-reference target")
                    .outcome(),
                ResolutionOutcome::Resolved(_)
            ));
        }
    }

    #[test]
    fn types_versions_package_root_back_reference_uses_the_harness_directory_identity() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");
        let fixture =
            "conformance/declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts";
        let programs = tsc_harness::expand_fixture_file(
            &workspace.join("ts-tests/tests/cases").join(fixture),
            &vendor_lib_dir,
        )
        .expect("expand the typesVersions back-reference fixture");
        assert_eq!(programs.len(), 1, "unexpected matrix expansion");

        let observed = crate::current_case_tsrs(fixture, &programs[0], &vendor_lib_dir)
            .expect("run the typesVersions back-reference fixture");
        assert_eq!(
            observed
                .all
                .iter()
                .map(|diagnostic| (
                    diagnostic.file.as_deref(),
                    diagnostic.code,
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.line,
                    diagnostic.col,
                ))
                .collect::<Vec<_>>(),
            [(Some("main.ts"), 2305, Some(9), Some(2), Some(0), Some(9))]
        );
        assert!(observed.syntactic.is_empty());
    }

    #[test]
    fn const_enum_fixture_and_exact_control_match_the_reviewed_conformance_boundary() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");

        let run_fixture = |fixture: &str| {
            let programs = tsc_harness::expand_fixture_file(
                &workspace.join("ts-tests/tests/cases").join(fixture),
                &vendor_lib_dir,
            )
            .expect("expand focused H0 fixture");
            assert_eq!(programs.len(), 1, "unexpected matrix expansion: {fixture}");
            crate::current_case_tsrs(fixture, &programs[0], &vendor_lib_dir)
                .expect("run focused H0 fixture")
        };

        let emitting =
            run_fixture("conformance/externalModules/verbatimModuleSyntaxAmbientConstEnum.ts");
        let observed = emitting
            .all
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.file.as_deref(),
                    diagnostic.code,
                    diagnostic.start,
                    diagnostic.length,
                    diagnostic.line,
                    diagnostic.col,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            observed,
            [
                (Some("/a.ts"), 2748, Some(9), Some(1), Some(0), Some(9),),
                (Some("/a.ts"), 2748, Some(100), Some(1), Some(3), Some(0),),
                (Some("/b.ts"), 2748, Some(9), Some(1), Some(0), Some(9),),
            ]
        );
        assert!(emitting.syntactic.is_empty());

        let control =
            run_fixture("conformance/externalModules/verbatimModuleSyntaxConstEnumUsage.ts");
        assert!(control.all.is_empty());
        assert!(control.syntactic.is_empty());
    }

    #[test]
    fn external_helper_fixtures_and_missing_tslib_control_match_the_reviewed_boundary() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");

        let run_fixture = |fixture: &str| {
            let programs = tsc_harness::expand_fixture_file(
                &workspace.join("ts-tests/tests/cases").join(fixture),
                &vendor_lib_dir,
            )
            .expect("expand focused H0 fixture");
            assert_eq!(programs.len(), 1, "unexpected matrix expansion: {fixture}");
            crate::current_case_tsrs(fixture, &programs[0], &vendor_lib_dir)
                .expect("run focused H0 fixture")
        };

        for (fixture, expected) in [
            (
                "conformance/classes/members/privateNames/privateNameEmitHelpers.ts",
                vec![
                    ("main.ts", 6133, 34, 2, 3, 4),
                    ("main.ts", 2807, 41, 7, 3, 11),
                    ("main.ts", 2807, 81, 7, 4, 24),
                ],
            ),
            (
                "conformance/classes/members/privateNames/privateNameStaticEmitHelpers.ts",
                vec![
                    ("main.ts", 6133, 29, 2, 2, 11),
                    ("main.ts", 2807, 55, 7, 3, 18),
                    ("main.ts", 6133, 86, 2, 4, 15),
                    ("main.ts", 2807, 100, 4, 4, 29),
                ],
            ),
        ] {
            let observed = run_fixture(fixture);
            assert_eq!(
                observed
                    .all
                    .iter()
                    .map(|diagnostic| {
                        (
                            diagnostic.file.as_deref().unwrap_or_default(),
                            diagnostic.code,
                            diagnostic.start.unwrap_or_default(),
                            diagnostic.length.unwrap_or_default(),
                            diagnostic.line.unwrap_or_default(),
                            diagnostic.col.unwrap_or_default(),
                        )
                    })
                    .collect::<Vec<_>>(),
                expected,
                "unexpected external-helper stream: {fixture}"
            );
            assert!(observed.syntactic.is_empty());
        }

        let control =
            run_fixture("conformance/es2020/modules/exportAsNamespace_missingEmitHelpers.ts");
        assert_eq!(
            control
                .all
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.file.as_deref(),
                        diagnostic.code,
                        diagnostic.line,
                        diagnostic.col,
                    )
                })
                .collect::<Vec<_>>(),
            [(Some("b.ts"), 2354, Some(0), Some(0))]
        );
        assert!(control.syntactic.is_empty());
    }

    #[test]
    fn alternate_resolution_fixtures_and_controls_match_the_reviewed_boundary() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");

        let run_fixture = |fixture: &str| {
            tsc_harness::expand_fixture_file(
                &workspace.join("ts-tests/tests/cases").join(fixture),
                &vendor_lib_dir,
            )
            .expect("expand focused H0 fixture")
            .into_iter()
            .map(|program| {
                let matrix_key = program.matrix_key.clone();
                let observed = crate::current_case_tsrs(fixture, &program, &vendor_lib_dir)
                    .expect("run focused H0 fixture");
                (matrix_key, observed)
            })
            .collect::<Vec<_>>()
        };

        for (fixture, expected) in [
            (
                "conformance/moduleResolution/resolutionModeImportType1.ts",
                [(29, 5, 0, 29), (67, 5, 1, 28), (149, 5, 2, 29)],
            ),
            (
                "conformance/moduleResolution/resolutionModeTypeOnlyImport1.ts",
                [(34, 5, 0, 34), (74, 5, 1, 33), (152, 5, 2, 34)],
            ),
        ] {
            let cases = run_fixture(fixture);
            assert_eq!(cases.len(), 2, "unexpected matrix expansion: {fixture}");
            let bundler = cases
                .iter()
                .find(|(matrix_key, _)| matrix_key == "moduleResolution=bundler")
                .expect("bundler control");
            assert!(bundler.1.all.is_empty(), "{fixture}: bundler control");
            assert!(bundler.1.syntactic.is_empty());

            let classic = cases
                .iter()
                .find(|(matrix_key, _)| matrix_key == "moduleResolution=classic")
                .expect("classic emitting case");
            assert_eq!(
                classic
                    .1
                    .all
                    .iter()
                    .map(|diagnostic| {
                        (
                            diagnostic.file.as_deref(),
                            diagnostic.code,
                            diagnostic.start.unwrap_or_default(),
                            diagnostic.length.unwrap_or_default(),
                            diagnostic.line.unwrap_or_default(),
                            diagnostic.col.unwrap_or_default(),
                            diagnostic.chain.text.as_str(),
                        )
                    })
                    .collect::<Vec<_>>(),
                expected
                    .into_iter()
                    .map(|(start, length, line, col)| {
                        (
                            Some("/app.ts"),
                            2792,
                            start,
                            length,
                            line,
                            col,
                            "Cannot find module 'foo'. Did you mean to set the 'moduleResolution' option to 'nodenext', or to add aliases to the 'paths' option?",
                        )
                    })
                    .collect::<Vec<_>>(),
                "unexpected Classic stream: {fixture}"
            );
            assert!(classic.1.syntactic.is_empty());
        }

        let missing =
            run_fixture("conformance/moduleResolution/node10AlternateResult_noResolution.ts");
        assert_eq!(missing.len(), 1);
        assert_eq!(
            missing[0]
                .1
                .all
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.file.as_deref(),
                        diagnostic.code,
                        diagnostic.start,
                        diagnostic.length,
                        diagnostic.line,
                        diagnostic.col,
                        diagnostic.category.as_str(),
                        diagnostic.pass.as_deref(),
                        diagnostic.chain.text.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    Some("/index.ts"),
                    6133,
                    Some(0),
                    Some(26),
                    Some(0),
                    Some(0),
                    "suggestion",
                    None,
                    "'pkg' is declared but its value is never read.",
                ),
                (
                    Some("/index.ts"),
                    2307,
                    Some(20),
                    Some(5),
                    Some(0),
                    Some(20),
                    "error",
                    None,
                    "Cannot find module 'pkg' or its corresponding type declarations.",
                ),
            ]
        );
        let missing_module = missing[0]
            .1
            .all
            .iter()
            .find(|diagnostic| diagnostic.code == 2307)
            .expect("Node10 missing-module diagnostic");
        assert_eq!(
            missing_module
                .chain
                .next
                .iter()
                .map(|message| {
                    (
                        message.code,
                        message.category.as_str(),
                        message.text.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            [(
                6280,
                "message",
                "There are types at '/node_modules/pkg/definitely-not-index.d.ts', but this result could not be resolved under your current 'moduleResolution' setting. Consider updating to 'node16', 'nodenext', or 'bundler'.",
            )]
        );
        assert!(missing[0].1.syntactic.is_empty());

        let untyped = run_fixture("conformance/moduleResolution/node10Alternateresult_noTypes.ts");
        assert_eq!(untyped.len(), 1);
        assert_eq!(
            untyped[0]
                .1
                .all
                .iter()
                .map(|diagnostic| {
                    (
                        diagnostic.file.as_deref(),
                        diagnostic.code,
                        diagnostic.start,
                        diagnostic.length,
                        diagnostic.line,
                        diagnostic.col,
                        diagnostic.category.as_str(),
                        diagnostic.pass.as_deref(),
                        diagnostic.chain.text.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            [
                (
                    Some("/index.ts"),
                    6133,
                    Some(0),
                    Some(26),
                    Some(0),
                    Some(0),
                    "suggestion",
                    None,
                    "'pkg' is declared but its value is never read.",
                ),
                (
                    Some("/index.ts"),
                    7016,
                    Some(20),
                    Some(5),
                    Some(0),
                    Some(20),
                    "error",
                    None,
                    "Could not find a declaration file for module 'pkg'. '/node_modules/pkg/untyped.js' implicitly has an 'any' type.",
                ),
            ]
        );
        assert_eq!(
            untyped[0].1.all[1]
                .chain
                .next
                .iter()
                .map(|message| {
                    (
                        message.code,
                        message.category.as_str(),
                        message.text.as_str(),
                    )
                })
                .collect::<Vec<_>>(),
            [(
                6280,
                "message",
                "There are types at '/node_modules/pkg/definitely-not-index.d.ts', but this result could not be resolved under your current 'moduleResolution' setting. Consider updating to 'node16', 'nodenext', or 'bundler'.",
            )]
        );
        assert!(untyped[0].1.syntactic.is_empty());
    }

    #[test]
    fn untyped_package_consumers_and_controls_match_the_reviewed_boundary() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        let vendor_lib_dir = workspace.join("vendor/typescript-6.0.3/lib");

        let run_fixture = |fixture: &str| {
            let programs = tsc_harness::expand_fixture_file(
                &workspace.join("ts-tests/tests/cases").join(fixture),
                &vendor_lib_dir,
            )
            .expect("expand focused H0 fixture");
            assert_eq!(programs.len(), 1, "unexpected matrix expansion: {fixture}");
            crate::current_case_tsrs(fixture, &programs[0], &vendor_lib_dir)
                .expect("run focused H0 fixture")
        };

        let expected_diag = |file: &str,
                             code: u32,
                             start: u32,
                             length: u32,
                             line: u32,
                             col: u32,
                             category: &str,
                             text: &str| crate::GoldenDiag {
            file: Some(file.to_owned()),
            start: Some(start),
            length: Some(length),
            line: Some(line),
            col: Some(col),
            code,
            pass: None,
            category: category.to_owned(),
            chain: crate::GoldenMessageChain {
                text: text.to_owned(),
                code,
                category: category.to_owned(),
                next: Vec::new(),
            },
            related: Vec::new(),
            reports_unnecessary: false,
            reports_deprecated: false,
            source: None,
        };

        let cases = [
            (
                "conformance/salsa/namespaceAssignmentToRequireAlias.ts",
                vec![
                    expected_diag(
                        "bug40140.js",
                        7016,
                        18,
                        9,
                        0,
                        18,
                        "suggestion",
                        "Could not find a declaration file for module 'untyped'. '/node_modules/untyped/index.js' implicitly has an 'any' type.",
                    ),
                    expected_diag(
                        "bug40140.js",
                        2339,
                        32,
                        10,
                        1,
                        2,
                        "error",
                        "Property 'assignment' does not exist on type 'typeof import(\"/node_modules/untyped/index\")'.",
                    ),
                    expected_diag(
                        "bug40140.js",
                        2339,
                        59,
                        7,
                        2,
                        2,
                        "error",
                        "Property 'noError' does not exist on type 'typeof import(\"/node_modules/untyped/index\")'.",
                    ),
                ],
            ),
            (
                "conformance/moduleResolution/untypedModuleImport_allowJs.ts",
                vec![
                    expected_diag(
                        "/a.ts",
                        7016,
                        16,
                        5,
                        0,
                        16,
                        "suggestion",
                        "Could not find a declaration file for module 'foo'. '/node_modules/foo/index.js' implicitly has an 'any' type.",
                    ),
                    expected_diag(
                        "/a.ts",
                        2339,
                        28,
                        3,
                        1,
                        4,
                        "error",
                        "Property 'bar' does not exist on type 'typeof import(\"/node_modules/foo/index\")'.",
                    ),
                ],
            ),
            (
                "conformance/moduleResolution/untypedModuleImport_withAugmentation.ts",
                vec![
                    expected_diag(
                        "/a.ts",
                        2665,
                        15,
                        5,
                        0,
                        15,
                        "error",
                        "Invalid module name in augmentation. Module 'foo' resolves to an untyped module at '/node_modules/foo/index.js', which cannot be augmented.",
                    ),
                    expected_diag(
                        "/a.ts",
                        7016,
                        74,
                        5,
                        3,
                        18,
                        "suggestion",
                        "Could not find a declaration file for module 'foo'. '/node_modules/foo/index.js' implicitly has an 'any' type.",
                    ),
                ],
            ),
            (
                "conformance/moduleResolution/untypedModuleImport.ts",
                vec![
                    expected_diag(
                        "/a.ts",
                        7016,
                        21,
                        5,
                        0,
                        21,
                        "suggestion",
                        "Could not find a declaration file for module 'foo'. '/node_modules/foo/index.js' implicitly has an 'any' type.",
                    ),
                    expected_diag(
                        "/b.ts",
                        7016,
                        21,
                        5,
                        0,
                        21,
                        "suggestion",
                        "Could not find a declaration file for module 'foo'. '/node_modules/foo/index.js' implicitly has an 'any' type.",
                    ),
                    expected_diag(
                        "/c.ts",
                        7016,
                        25,
                        5,
                        0,
                        25,
                        "suggestion",
                        "Could not find a declaration file for module 'foo'. '/node_modules/foo/index.js' implicitly has an 'any' type.",
                    ),
                ],
            ),
            (
                "conformance/moduleResolution/untypedModuleImport_vsAmbient.ts",
                Vec::new(),
            ),
        ];

        for (fixture, expected) in cases {
            let observed = run_fixture(fixture);
            assert_eq!(observed.all, expected, "unexpected stream: {fixture}");
            assert!(
                observed.all_empty_related_information.is_empty(),
                "unexpected present-but-empty related information: {fixture}"
            );
            assert!(observed.syntactic.is_empty(), "{fixture}");
        }
    }

    #[test]
    fn program_option_projection_preserves_types_and_normalizes_type_roots() {
        let program = ProgramJson {
            schema: 1,
            cwd: "/work/project".to_owned(),
            options: BTreeMap::from([
                ("noLib".to_owned(), OptionValue::Bool(true)),
                (
                    "typeRoots".to_owned(),
                    OptionValue::StringList(vec!["types".to_owned(), "/shared/types".to_owned()]),
                ),
                (
                    "types".to_owned(),
                    OptionValue::StringList(vec!["*".to_owned(), "explicit".to_owned()]),
                ),
            ]),
            libs: Vec::new(),
            files: Vec::new(),
            matrix_key: String::new(),
        };

        let options = program_options_from_program(&program, Path::new("/work/project"))
            .expect("project program options");
        assert_eq!(options.no_lib(), Some(true));
        let expected_types = vec!["*".to_owned(), "explicit".to_owned()];
        assert_eq!(options.types(), Some(expected_types.as_slice()));
        let roots = options.type_roots().expect("explicit type roots");
        assert_eq!(roots.len(), 2);
        assert_eq!(
            roots[0].canonical().as_path(),
            Path::new("/work/project/types")
        );
        assert_eq!(roots[1].canonical().as_path(), Path::new("/shared/types"));
    }

    #[test]
    fn package_diagnostic_map_is_a_program_wide_exact_dts_fold() {
        let plain = PackageId::new("pkg", "index.js", "1.0.0");
        let bundled = PackageId::new("bundled", "index.d.ts", "1.0.0");
        let types = PackageId::new("@types/pkg", "index.d.mts", "1.0.0");
        let map = package_map_from_facts([
            (&plain, &ModuleExtension::Js),
            (&plain, &ModuleExtension::Dmts),
            (&bundled, &ModuleExtension::Dts),
            (&types, &ModuleExtension::Dmts),
        ]);

        assert_eq!(map.get("pkg"), Some(&false));
        assert_eq!(map.get("bundled"), Some(&true));
        assert_eq!(map.get("@types/pkg"), Some(&false));
        assert!(map.contains_key(&types_package_name("pkg")));
        assert_eq!(types_package_name("@scope/pkg"), "@types/scope__pkg");
    }

    #[test]
    fn implied_format_uses_explicit_extensions_or_node_package_lookup() {
        fn package_scope(module_type: PackageJsonType) -> PackageMetadata {
            let package_json = ProgramPath::from_trusted_parts("/package.json", "/package.json")
                .expect("trusted package path");
            PackageMetadata::from_trusted_parsed(package_json, "{}", None, None, module_type)
        }

        let module_scope = package_scope(PackageJsonType::Module);
        let common_js_scope = package_scope(PackageJsonType::CommonJs);
        let other_scope = package_scope(PackageJsonType::Other);
        let unspecified_scope = package_scope(PackageJsonType::Unspecified);

        let common_js = CompilerOptions {
            module: Some(1),
            ..CompilerOptions::default()
        };
        assert_eq!(
            implied_node_format("/index.ts", Some(&module_scope), &common_js),
            None
        );
        assert_eq!(
            implied_node_format("/index.mts", Some(&module_scope), &common_js),
            Some(ResolutionMode::EsNext)
        );

        let node = CompilerOptions {
            module: Some(102),
            ..CompilerOptions::default()
        };
        assert_eq!(
            implied_node_format("/index.ts", Some(&module_scope), &node),
            Some(ResolutionMode::EsNext)
        );
        assert_eq!(
            implied_node_format("/node_modules/pkg/index.ts", None, &common_js),
            Some(ResolutionMode::CommonJs)
        );
        assert_eq!(
            implied_node_format_for_emit("/node_modules/pkg/index.ts", None, &common_js),
            None
        );

        let es_next = CompilerOptions {
            module: Some(99),
            module_resolution: Some(99),
            ..CompilerOptions::default()
        };
        assert_eq!(
            implied_node_format("/index.ts", Some(&common_js_scope), &es_next),
            Some(ResolutionMode::CommonJs)
        );
        assert_eq!(
            implied_node_format_for_emit("/index.ts", Some(&common_js_scope), &es_next),
            Some(ResolutionMode::CommonJs)
        );
        assert_eq!(
            implied_node_format("/index.ts", Some(&unspecified_scope), &es_next),
            Some(ResolutionMode::CommonJs)
        );
        assert_eq!(
            implied_node_format("/index.ts", Some(&other_scope), &es_next),
            Some(ResolutionMode::CommonJs)
        );
        assert_eq!(
            implied_node_format_for_emit("/index.ts", Some(&unspecified_scope), &es_next),
            None
        );
        assert_eq!(
            implied_node_format_for_emit("/index.ts", Some(&other_scope), &es_next),
            None
        );
        assert_eq!(
            implied_node_format("/index.ts", None, &es_next),
            Some(ResolutionMode::CommonJs)
        );
        assert_eq!(
            implied_node_format_for_emit("/index.ts", None, &es_next),
            None
        );

        let preserve = CompilerOptions {
            module: Some(200),
            module_resolution: Some(99),
            ..CompilerOptions::default()
        };
        assert_eq!(
            implied_node_format_for_emit("/index.ts", Some(&unspecified_scope), &preserve),
            None
        );
        assert_eq!(
            implied_node_format_for_emit("/index.ts", Some(&unspecified_scope), &node),
            Some(ResolutionMode::CommonJs)
        );
    }
}
