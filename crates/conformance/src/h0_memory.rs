use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use tsc_checker::InputFile;
use tsc_compiler::ProgramSession;
use tsc_diagnostics::Diagnostic;
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    plan_module_requests, HostResolvedModule, ModuleExtension, ModuleResolution, ModuleResolver,
    PackageId, PackageJsonType, PackageMetadata, PreparedProgram, PreparedSourceFile, ProgramPath,
    ResolutionError, ResolutionMode, ResolutionOutcome, ResolvedModuleTarget, SourceFileId,
};

use crate::ConformanceResult;

const SUPPORTED_FIXTURES: [&str; 14] = [
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
    for source in decoded_libs.iter().chain(&decoded_files) {
        host_builder = host_builder.file(&source.canonical, source.text.as_bytes().to_vec());
    }
    let host = host_builder.build()?;

    let mut options = tsc_harness::compiler_options_from_program(program);
    options.no_emit = Some(true);
    let mut resolver = ModuleResolver::new(&host, &options)?;
    let mut prepared_builder =
        PreparedProgram::builder(resolver.path_context().clone(), options.clone());

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
        let implied_node_format = implied_node_format(&source.display, package_scope.as_ref());
        let path = public_program_path(source)?;
        let mut prepared = PreparedSourceFile::new(path.clone(), source.text.clone());
        if let Some(scope) = package_scope.as_ref() {
            prepared = prepared.with_package_scope(scope.package_json().canonical().clone());
        }
        if let Some(mode) = implied_node_format {
            prepared = prepared.with_implied_node_format(mode);
        }
        let source_id = prepared_builder.add_source_file(prepared.clone())?;
        prepared_builder.add_root_file(source_id)?;
        source_by_canonical.insert(source.canonical.clone(), (source_id, path.clone()));
        owned_program_sources.push(OwnedProgramSource { prepared });
    }

    let mut host_resolutions = Vec::new();
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
        for key in plan_module_requests(&source.prepared, &options)? {
            let specifier = key.specifier().to_owned();
            let host_outcome = resolver.resolve(
                source.prepared.path().canonical().as_path(),
                &specifier,
                key.mode(),
            )?;
            host_resolutions.push((key, host_outcome));
        }
    }

    // tsc's host package map is a fold over the complete resolved-module
    // table, so diagnostic facts cannot be finalized while rows are still
    // being discovered.
    let package_map = package_map_from_facts(host_resolutions.iter().filter_map(|(_, outcome)| {
        let ResolutionOutcome::Resolved(module) = outcome else {
            return None;
        };
        Some((module.package_id()?, module.extension()))
    }));
    for (key, host_outcome) in host_resolutions {
        let resolution =
            bind_host_outcome(host_outcome, &source_by_canonical, &options, &package_map)?;
        prepared_builder.add_module_resolution(key, Ok(resolution))?;
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
    let outcome = ProgramSession::new(prepared).run()?;
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

fn bind_host_outcome(
    outcome: ResolutionOutcome<HostResolvedModule>,
    source_by_canonical: &BTreeMap<PathBuf, (SourceFileId, ProgramPath)>,
    options: &tsc_program::CompilerOptions,
    package_map: &BTreeMap<String, bool>,
) -> Result<ModuleResolution, ResolutionError> {
    let ResolutionOutcome::Resolved(host_module) = outcome else {
        return Ok(ModuleResolution::not_found());
    };
    let alternate_result = host_module.alternate_result().cloned();
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
    let target = if host_module.extension().is_javascript() && !options.allow_js {
        ResolvedModuleTarget::Unloaded(host_module.resolved_file().clone())
    } else if let Some((target_source, target_path)) = source_by_canonical.get(target_canonical) {
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
    use super::{package_map_from_facts, supports_fixture, types_package_name, SUPPORTED_FIXTURES};
    use tsc_program::{ModuleExtension, PackageId};

    #[test]
    fn dedicated_route_is_exactly_the_reviewed_package_exports_fixtures() {
        assert!(SUPPORTED_FIXTURES
            .iter()
            .all(|fixture| supports_fixture(fixture)));
        for fixture in [
            "conformance/node/allowJs/nodeModulesAllowJsPackagePatternExportsTrailers.ts",
            "conformance/node/nodeModulesPackagePatternExportsExclude.ts.backup",
            "node/nodeModulesPackagePatternExportsExclude.ts",
        ] {
            assert!(!supports_fixture(fixture), "unexpected H0 route: {fixture}");
        }
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
}
