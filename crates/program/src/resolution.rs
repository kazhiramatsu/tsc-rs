use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use tsrs2_diags::{Diagnostic, DiagnosticList};
use tsrs2_host::HostError;

use crate::path::{CanonicalPath, ProgramPath};
use crate::prepared::SourceFileId;

/// tsc `ResolutionMode`: CommonJS, ESNext, or the valid `undefined` mode.
///
/// `Unspecified` is not an unsupported state. It is the public spelling of
/// the vendored compiler's `undefined` key and remains distinct from both
/// concrete modes.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum ResolutionMode {
    CommonJs,
    EsNext,
    #[default]
    Unspecified,
}

/// The exact authoritative resolution-table key.
///
/// Specifiers retain their original spelling; host case folding applies to
/// the containing source's canonical path only.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ResolutionKey {
    source: CanonicalPath,
    specifier: String,
    mode: ResolutionMode,
}

impl ResolutionKey {
    pub fn new(source: CanonicalPath, specifier: impl Into<String>, mode: ResolutionMode) -> Self {
        Self {
            source,
            specifier: specifier.into(),
            mode,
        }
    }

    pub fn source(&self) -> &CanonicalPath {
        &self.source
    }

    pub fn specifier(&self) -> &str {
        &self.specifier
    }

    pub const fn mode(&self) -> ResolutionMode {
        self.mode
    }
}

/// The containing context for type-reference resolution.
///
/// Automatic `@types` discovery uses TypeScript's synthetic inferred-types
/// containing file, which is a canonical path but not an owned source file.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TypeReferenceResolutionOrigin {
    Source(CanonicalPath),
    Automatic(CanonicalPath),
}

impl TypeReferenceResolutionOrigin {
    pub fn canonical_path(&self) -> &CanonicalPath {
        match self {
            Self::Source(path) | Self::Automatic(path) => path,
        }
    }

    pub const fn is_automatic(&self) -> bool {
        matches!(self, Self::Automatic(_))
    }
}

/// Exact key for source-owned or automatic type-reference resolution.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TypeReferenceResolutionKey {
    origin: TypeReferenceResolutionOrigin,
    specifier: String,
    mode: ResolutionMode,
}

impl TypeReferenceResolutionKey {
    pub fn source(
        source: CanonicalPath,
        specifier: impl Into<String>,
        mode: ResolutionMode,
    ) -> Self {
        Self {
            origin: TypeReferenceResolutionOrigin::Source(source),
            specifier: specifier.into(),
            mode,
        }
    }

    /// Construct an automatic `@types` request. Vendored `createProgram`
    /// always caches these requests under the `undefined` resolution mode.
    pub fn automatic(containing_file: CanonicalPath, specifier: impl Into<String>) -> Self {
        Self {
            origin: TypeReferenceResolutionOrigin::Automatic(containing_file),
            specifier: specifier.into(),
            mode: ResolutionMode::Unspecified,
        }
    }

    pub fn origin(&self) -> &TypeReferenceResolutionOrigin {
        &self.origin
    }

    pub fn specifier(&self) -> &str {
        &self.specifier
    }

    pub const fn mode(&self) -> ResolutionMode {
        self.mode
    }
}

/// A supported resolver branch has exactly two ordinary outcomes.
///
/// Infrastructure failures are [`ResolutionError`] values and therefore
/// cannot be stored as `NotFound`. There is intentionally no `Suppressed`,
/// unknown, or untriaged variant on the H0 contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolutionOutcome<T> {
    Resolved(T),
    NotFound,
}

/// TypeScript's resolved module extension discriminant.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ModuleExtension {
    Ts,
    Tsx,
    Dts,
    Js,
    Jsx,
    Json,
    Mjs,
    Mts,
    Dmts,
    Cjs,
    Cts,
    Dcts,
    /// An exact extension string returned by the default resolver arm, such
    /// as `.d.css.ts` for an arbitrary-extension declaration twin.
    Arbitrary(String),
}

impl ModuleExtension {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Ts => ".ts",
            Self::Tsx => ".tsx",
            Self::Dts => ".d.ts",
            Self::Js => ".js",
            Self::Jsx => ".jsx",
            Self::Json => ".json",
            Self::Mjs => ".mjs",
            Self::Mts => ".mts",
            Self::Dmts => ".d.mts",
            Self::Cjs => ".cjs",
            Self::Cts => ".cts",
            Self::Dcts => ".d.cts",
            Self::Arbitrary(extension) => extension,
        }
    }

    pub const fn is_javascript(&self) -> bool {
        matches!(self, Self::Js | Self::Jsx | Self::Mjs | Self::Cjs)
    }

    pub(crate) fn is_valid(&self) -> bool {
        let Self::Arbitrary(extension) = self else {
            return true;
        };
        extension.starts_with('.')
            && extension.len() > 1
            && !extension.contains(['/', '\\', '\0'])
            && !matches!(
                extension.as_str(),
                ".ts"
                    | ".tsx"
                    | ".d.ts"
                    | ".js"
                    | ".jsx"
                    | ".json"
                    | ".mjs"
                    | ".mts"
                    | ".d.mts"
                    | ".cjs"
                    | ".cts"
                    | ".d.cts"
            )
    }

    pub(crate) fn matches_path(&self, path: &str) -> bool {
        match self {
            Self::Ts => path.ends_with(Self::Ts.as_str()) && !path.ends_with(Self::Dts.as_str()),
            Self::Mts => path.ends_with(Self::Mts.as_str()) && !path.ends_with(Self::Dmts.as_str()),
            Self::Cts => path.ends_with(Self::Cts.as_str()) && !path.ends_with(Self::Dcts.as_str()),
            extension => path.ends_with(extension.as_str()),
        }
    }
}

/// Vendored `PackageId`, retained losslessly for diagnostic consumers.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PackageId {
    name: String,
    submodule_name: String,
    version: String,
    peer_dependencies: Option<String>,
}

impl PackageId {
    pub fn new(
        name: impl Into<String>,
        submodule_name: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            submodule_name: submodule_name.into(),
            version: version.into(),
            peer_dependencies: None,
        }
    }

    pub fn with_peer_dependencies(mut self, peer_dependencies: impl Into<String>) -> Self {
        self.peer_dependencies = Some(peer_dependencies.into());
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn submodule_name(&self) -> &str {
        &self.submodule_name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn peer_dependencies(&self) -> Option<&str> {
        self.peer_dependencies.as_deref()
    }
}

/// A module target either participates in the owned source program or is a
/// successful resolution deliberately not admitted to program membership.
///
/// The second form covers resolution-diagnostic branches such as untyped
/// JavaScript, JSON without `resolveJsonModule`, and arbitrary extensions
/// without `allowArbitraryExtensions`. Vendored `createProgram` retains the
/// resolved row but skips `findSourceFile` for those branches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedModuleTarget {
    Source {
        source: SourceFileId,
        resolved_file: ProgramPath,
    },
    Unloaded(ProgramPath),
}

impl ResolvedModuleTarget {
    pub const fn source(&self) -> Option<SourceFileId> {
        match self {
            Self::Source { source, .. } => Some(*source),
            Self::Unloaded(_) => None,
        }
    }

    pub fn resolved_file(&self) -> &ProgramPath {
        match self {
            Self::Source { resolved_file, .. } | Self::Unloaded(resolved_file) => resolved_file,
        }
    }
}

/// Lossless checker-consumed facts for one resolved module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedModule {
    target: ResolvedModuleTarget,
    extension: ModuleExtension,
    original_path: Option<ProgramPath>,
    is_external_library_import: bool,
    resolved_using_ts_extension: bool,
    package_id: Option<PackageId>,
}

impl ResolvedModule {
    pub fn new(target: ResolvedModuleTarget, extension: ModuleExtension) -> Self {
        Self {
            target,
            extension,
            original_path: None,
            is_external_library_import: false,
            resolved_using_ts_extension: false,
            package_id: None,
        }
    }

    pub fn with_original_path(mut self, original_path: ProgramPath) -> Self {
        self.original_path = Some(original_path);
        self
    }

    pub fn with_external_library_import(mut self, value: bool) -> Self {
        self.is_external_library_import = value;
        self
    }

    pub fn with_resolved_using_ts_extension(mut self, value: bool) -> Self {
        self.resolved_using_ts_extension = value;
        self
    }

    pub fn with_package_id(mut self, package_id: PackageId) -> Self {
        self.package_id = Some(package_id);
        self
    }

    pub fn target(&self) -> &ResolvedModuleTarget {
        &self.target
    }

    pub fn extension(&self) -> &ModuleExtension {
        &self.extension
    }

    pub fn original_path(&self) -> Option<&ProgramPath> {
        self.original_path.as_ref()
    }

    pub const fn is_external_library_import(&self) -> bool {
        self.is_external_library_import
    }

    pub const fn resolved_using_ts_extension(&self) -> bool {
        self.resolved_using_ts_extension
    }

    pub fn package_id(&self) -> Option<&PackageId> {
        self.package_id.as_ref()
    }
}

/// One authoritative module-resolution record, including metadata that is
/// observable even when the ordinary outcome is `NotFound`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleResolution {
    outcome: ResolutionOutcome<ResolvedModule>,
    alternate_result: Option<ProgramPath>,
    diagnostics: DiagnosticList,
}

impl ModuleResolution {
    pub fn resolved(module: ResolvedModule) -> Self {
        Self {
            outcome: ResolutionOutcome::Resolved(module),
            alternate_result: None,
            diagnostics: Vec::new(),
        }
    }

    pub fn not_found() -> Self {
        Self {
            outcome: ResolutionOutcome::NotFound,
            alternate_result: None,
            diagnostics: Vec::new(),
        }
    }

    pub fn with_alternate_result(mut self, path: ProgramPath) -> Self {
        self.alternate_result = Some(path);
        self
    }

    pub fn with_diagnostics(mut self, diagnostics: Vec<Diagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    pub fn outcome(&self) -> &ResolutionOutcome<ResolvedModule> {
        &self.outcome
    }

    pub fn alternate_result(&self) -> Option<&ProgramPath> {
        self.alternate_result.as_ref()
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// Lossless result facts for a resolved `/// <reference types>` request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedTypeReferenceDirective {
    target: ProgramPath,
    source: SourceFileId,
    original_path: Option<ProgramPath>,
    primary: bool,
    is_external_library_import: bool,
    package_id: Option<PackageId>,
}

impl ResolvedTypeReferenceDirective {
    pub fn new(target: ProgramPath, source: SourceFileId) -> Self {
        Self {
            target,
            source,
            original_path: None,
            primary: false,
            is_external_library_import: false,
            package_id: None,
        }
    }

    pub fn with_primary(mut self, value: bool) -> Self {
        self.primary = value;
        self
    }

    pub fn with_original_path(mut self, original_path: ProgramPath) -> Self {
        self.original_path = Some(original_path);
        self
    }

    pub fn with_external_library_import(mut self, value: bool) -> Self {
        self.is_external_library_import = value;
        self
    }

    pub fn with_package_id(mut self, package_id: PackageId) -> Self {
        self.package_id = Some(package_id);
        self
    }

    pub fn target(&self) -> &ProgramPath {
        &self.target
    }

    pub const fn source(&self) -> SourceFileId {
        self.source
    }

    pub fn original_path(&self) -> Option<&ProgramPath> {
        self.original_path.as_ref()
    }

    pub const fn primary(&self) -> bool {
        self.primary
    }

    pub const fn is_external_library_import(&self) -> bool {
        self.is_external_library_import
    }

    pub fn package_id(&self) -> Option<&PackageId> {
        self.package_id.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeReferenceResolution {
    outcome: ResolutionOutcome<ResolvedTypeReferenceDirective>,
    diagnostics: DiagnosticList,
}

impl TypeReferenceResolution {
    pub fn resolved(directive: ResolvedTypeReferenceDirective) -> Self {
        Self {
            outcome: ResolutionOutcome::Resolved(directive),
            diagnostics: Vec::new(),
        }
    }

    pub fn not_found() -> Self {
        Self {
            outcome: ResolutionOutcome::NotFound,
            diagnostics: Vec::new(),
        }
    }

    pub fn with_diagnostics(mut self, diagnostics: Vec<Diagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    pub fn outcome(&self) -> &ResolutionOutcome<ResolvedTypeReferenceDirective> {
        &self.outcome
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ResolutionRequestKind {
    Module,
    TypeReference,
}

/// Typed lookup failure for an incomplete authoritative table.
///
/// A session must propagate this as an H0 infrastructure failure. It must not
/// fall back to the legacy checker resolver or treat absence as `NotFound`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingResolutionError {
    request_kind: ResolutionRequestKind,
    origin: MissingResolutionOrigin,
    specifier: String,
    mode: ResolutionMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum MissingResolutionOrigin {
    Module(CanonicalPath),
    TypeReference(TypeReferenceResolutionOrigin),
}

impl MissingResolutionError {
    pub(crate) fn module(key: &ResolutionKey) -> Self {
        Self {
            request_kind: ResolutionRequestKind::Module,
            origin: MissingResolutionOrigin::Module(key.source().clone()),
            specifier: key.specifier().to_owned(),
            mode: key.mode(),
        }
    }

    pub(crate) fn type_reference(key: &TypeReferenceResolutionKey) -> Self {
        Self {
            request_kind: ResolutionRequestKind::TypeReference,
            origin: MissingResolutionOrigin::TypeReference(key.origin().clone()),
            specifier: key.specifier().to_owned(),
            mode: key.mode(),
        }
    }

    pub const fn request_kind(&self) -> ResolutionRequestKind {
        self.request_kind
    }

    pub fn origin(&self) -> &CanonicalPath {
        match &self.origin {
            MissingResolutionOrigin::Module(origin) => origin,
            MissingResolutionOrigin::TypeReference(origin) => origin.canonical_path(),
        }
    }

    pub fn type_reference_origin(&self) -> Option<&TypeReferenceResolutionOrigin> {
        match &self.origin {
            MissingResolutionOrigin::Module(_) => None,
            MissingResolutionOrigin::TypeReference(origin) => Some(origin),
        }
    }

    pub fn specifier(&self) -> &str {
        &self.specifier
    }

    pub const fn mode(&self) -> ResolutionMode {
        self.mode
    }
}

impl fmt::Display for MissingResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "authoritative {:?} resolution is missing for ({}, {:?}, {:?})",
            self.request_kind,
            self.origin(),
            self.specifier,
            self.mode
        )
    }
}

impl Error for MissingResolutionError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ResolutionErrorKind {
    Host,
    Unsupported,
    Canonicalization,
    InvalidData,
    ResourceLimit,
}

/// A resolver failure that is structurally separate from `NotFound`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolutionError {
    Host(HostError),
    Unsupported {
        feature: String,
        detail: String,
    },
    Canonicalization {
        path: Option<PathBuf>,
        detail: String,
    },
    InvalidData(String),
    ResourceLimit(String),
}

impl ResolutionError {
    pub fn unsupported(feature: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            feature: feature.into(),
            detail: detail.into(),
        }
    }

    pub fn canonicalization(path: Option<PathBuf>, detail: impl Into<String>) -> Self {
        Self::Canonicalization {
            path,
            detail: detail.into(),
        }
    }

    pub fn invalid_data(detail: impl Into<String>) -> Self {
        Self::InvalidData(detail.into())
    }

    pub fn resource_limit(detail: impl Into<String>) -> Self {
        Self::ResourceLimit(detail.into())
    }

    pub const fn kind(&self) -> ResolutionErrorKind {
        match self {
            Self::Host(_) => ResolutionErrorKind::Host,
            Self::Unsupported { .. } => ResolutionErrorKind::Unsupported,
            Self::Canonicalization { .. } => ResolutionErrorKind::Canonicalization,
            Self::InvalidData(_) => ResolutionErrorKind::InvalidData,
            Self::ResourceLimit(_) => ResolutionErrorKind::ResourceLimit,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Host(error) => error.path(),
            Self::Canonicalization { path, .. } => path.as_deref(),
            Self::Unsupported { .. } | Self::InvalidData(_) | Self::ResourceLimit(_) => None,
        }
    }
}

impl From<HostError> for ResolutionError {
    fn from(error: HostError) -> Self {
        Self::Host(error)
    }
}

impl fmt::Display for ResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => write!(formatter, "host failure: {error}"),
            Self::Unsupported { feature, detail } => {
                write!(formatter, "unsupported resolution feature {feature}")?;
                if !detail.is_empty() {
                    write!(formatter, ": {detail}")?;
                }
                Ok(())
            }
            Self::Canonicalization { path, detail } => {
                formatter.write_str("path canonicalization failed")?;
                if let Some(path) = path {
                    write!(formatter, " for {}", path.display())?;
                }
                if !detail.is_empty() {
                    write!(formatter, ": {detail}")?;
                }
                Ok(())
            }
            Self::InvalidData(detail) => write!(formatter, "invalid resolution data: {detail}"),
            Self::ResourceLimit(detail) => {
                write!(formatter, "resolution resource limit exceeded: {detail}")
            }
        }
    }
}

impl Error for ResolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Host(error) => Some(error),
            Self::Unsupported { .. }
            | Self::Canonicalization { .. }
            | Self::InvalidData(_)
            | Self::ResourceLimit(_) => None,
        }
    }
}
