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
    snapshot: std::sync::Arc<tsc_diagnostics::TextSnapshot>,
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
                snapshot: std::sync::Arc::clone(lib.snapshot()),
            })
        })
        .collect::<Result<Vec<_>, H0MemoryError>>()?;
    let decoded_files = files
        .iter()
        .map(|file| {
            Ok(DecodedSource {
                display: file.name.clone(),
                canonical: normalize_source_path(&current_directory, &file.name)?,
                snapshot: std::sync::Arc::clone(file.snapshot()),
            })
        })
        .collect::<Result<Vec<_>, H0MemoryError>>()?;

    let mut host_builder = MemoryCompilerHost::builder(&current_directory).case_sensitive(true);
    let mut trailing_directory_aliases = BTreeSet::new();
    for source in decoded_libs.iter().chain(&decoded_files) {
        host_builder = host_builder.file(
            &source.canonical,
            source.snapshot.text().as_bytes().to_vec(),
        );
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

    let mut source_by_canonical = BTreeMap::<PathBuf, SourceFileId>::new();
    for source in &decoded_libs {
        let path = public_program_path(source)?;
        let source_id = prepared_builder.add_source_file(PreparedSourceFile::from_snapshot(
            path.clone(),
            std::sync::Arc::clone(&source.snapshot),
        ))?;
        prepared_builder.add_library_file(source_id)?;
        source_by_canonical.insert(source.canonical.clone(), source_id);
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
        let mut prepared = PreparedSourceFile::from_snapshot(
            path.clone(),
            std::sync::Arc::clone(&source.snapshot),
        )
        .with_implied_node_formats(implied_node_format, implied_node_format_for_emit);
        if let Some(scope) = package_scope.as_ref() {
            prepared = prepared.with_package_scope(scope.package_json().canonical().clone());
        }
        let source_id = prepared_builder.add_source_file(prepared.clone())?;
        prepared_builder.add_root_file(source_id)?;
        source_by_canonical.insert(source.canonical.clone(), source_id);
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
        if options.no_resolve != Some(true) {
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
    // The runner consumes only the per-file getter projections below, so the
    // session elides the library-prefix completion pass (its cost without
    // `skipDefaultLibCheck` is ~1s of standard-library checking per program;
    // the compared surfaces are assembled before that pass by construction).
    let outcome = ProgramSession::new(prepared).run_for_conformance_harness()?;
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
    source_by_canonical: &BTreeMap<PathBuf, SourceFileId>,
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
    let target = if options.no_resolve == Some(true) && owned_source.is_none() {
        ResolvedModuleTarget::Unloaded {
            resolved_file: host_module.resolved_file().clone(),
            reason: UnloadedModuleReason::NoResolve,
        }
    } else if host_module.extension().is_javascript() && owned_source.is_none() {
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
        ResolvedModuleTarget::Unloaded {
            resolved_file: host_module.resolved_file().clone(),
            reason,
        }
    } else if let Some(target_source) = owned_source {
        ResolvedModuleTarget::Source {
            source: *target_source,
            resolved_file: host_module.resolved_file().clone(),
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
    source_by_canonical: &BTreeMap<PathBuf, SourceFileId>,
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
    let Some(source) = source_by_canonical.get(target_canonical) else {
        return Err(ResolutionError::invalid_data(format!(
            "resolved type-reference source {} is not owned by the prepared program",
            host_directive.resolved_file().display().display()
        )));
    };
    let mut directive =
        ResolvedTypeReferenceDirective::new(host_directive.resolved_file().clone(), *source)
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
#[path = "../tests/unit/h0_memory/tests.rs"]
mod tests;
