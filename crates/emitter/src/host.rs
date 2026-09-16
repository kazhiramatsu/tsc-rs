use crate::source_map::paths;
use tsc_diagnostics::{JsStr, JsString};

use tsc_program::{ResolutionMode, SourceFileId};
use tsc_syntax::{FileReference, SourceFile};
use tsc_types::CompilerOptions;

/// Borrowed, read-only facts for one Program source visible to emission.
///
/// The display path is callback-visible while the canonical path is used only
/// for equality and collision checks. `may_be_emitted` is the Program-owned
/// `sourceFileMayBeEmitted` verdict; it is deliberately not reconstructed
/// from resolution provenance in this crate.
#[derive(Clone, Copy, Debug)]
pub struct EmitSource<'host> {
    id: SourceFileId,
    path: JsStr<'host>,
    canonical_path: JsStr<'host>,
    may_be_emitted: bool,
    may_emit_forced_declaration: bool,
    implied_node_format_for_emit: Option<ResolutionMode>,
    is_external_module: Option<bool>,
    syntax: Option<&'host SourceFile>,
}

impl<'host> EmitSource<'host> {
    pub const fn new(
        id: SourceFileId,
        path: JsStr<'host>,
        canonical_path: JsStr<'host>,
        may_be_emitted: bool,
        implied_node_format_for_emit: Option<ResolutionMode>,
        syntax: Option<&'host SourceFile>,
    ) -> Self {
        Self {
            id,
            path,
            canonical_path,
            may_be_emitted,
            may_emit_forced_declaration: may_be_emitted,
            implied_node_format_for_emit,
            is_external_module: match syntax {
                Some(source) => Some(source.external_module_indicator.is_some()),
                None => None,
            },
            syntax,
        }
    }

    pub const fn id(self) -> SourceFileId {
        self.id
    }

    pub const fn path(self) -> JsStr<'host> {
        self.path
    }

    pub const fn canonical_path(self) -> JsStr<'host> {
        self.canonical_path
    }

    pub const fn may_be_emitted(self) -> bool {
        self.may_be_emitted
    }

    pub const fn with_may_emit_forced_declaration(mut self, eligible: bool) -> Self {
        self.may_emit_forced_declaration = eligible;
        self
    }

    pub const fn may_emit_forced_declaration(self) -> bool {
        self.may_emit_forced_declaration
    }

    /// The already-computed `getImpliedNodeFormatForEmitWorker` result owned
    /// by the Program loader. `None` means emission falls back to the
    /// effective compiler `module` kind.
    pub const fn implied_node_format_for_emit(self) -> Option<ResolutionMode> {
        self.implied_node_format_for_emit
    }

    pub const fn syntax(self) -> Option<&'host SourceFile> {
        self.syntax
    }

    /// Use the Program's already parsed module-detection fact before checked
    /// syntax is borrowed. An unknown prepared fact retains any syntax-derived value.
    pub const fn with_is_external_module(mut self, value: Option<bool>) -> Self {
        if value.is_some() {
            self.is_external_module = value;
        }
        self
    }

    pub const fn is_external_module(self) -> Option<bool> {
        self.is_external_module
    }
}

/// Read-only projection of the Program facts reached by the H1 emitter.
///
/// This protocol has no filesystem operations and no write callback. A
/// planning-only adapter may return an [`EmitSource`] without syntax; the
/// checked adapter used by [`crate::emit_files`] supplies the same source's
/// immutable parsed tree while its checker session remains alive.
pub trait EmitHost {
    fn compiler_options(&self) -> &CompilerOptions;
    /// Which public entry produced this emit request. The ordinary Program
    /// route is the default; the H2.8c research adapters override it to admit
    /// the no-check / transpile-forced options.
    /// tsrs-native: typed route plan, see [`crate::EmitRouteKind`].
    fn emit_route(&self) -> crate::EmitRouteKind {
        crate::EmitRouteKind::Program
    }
    fn current_directory(&self) -> JsStr<'_>;
    /// Absolute common source directory; the caller may omit its trailing separator.
    fn common_source_directory(&self) -> JsStr<'_>;
    fn config_file_path(&self) -> Option<JsStr<'_>>;
    fn use_case_sensitive_file_names(&self) -> bool;
    fn source_file_ids(&self) -> &[SourceFileId];
    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>>;

    /// Resolve a preserved triple-slash path reference against its containing
    /// source. Hosts with redirect-aware Program resolution may override this
    /// lexical default; the declaration transformer observes only the resolved
    /// source identity.
    ///
    /// tsc-port: getSourceFileFromReference @6.0.3
    /// tsc-hash: fd088e64de540c2728db419ad570897cc4baebd184a4c4b847a66537bd43f0cf
    /// tsc-span: _tsc.js:124170-124172
    fn source_file_from_reference(
        &self,
        referencing_file: SourceFileId,
        reference: &FileReference,
    ) -> Option<EmitSource<'_>> {
        let referencing = self.source_file(referencing_file)?;
        let directory = paths::directory_path(referencing.path());
        let target = paths::combine_paths(&directory, &reference.file_name);
        let target = self.canonical_output_path(target.as_js());
        self.source_file_ids().iter().find_map(|&id| {
            let candidate = self.source_file(id)?;
            (candidate.canonical_path() == target.as_js()).then_some(candidate)
        })
    }

    /// tsc-port: getEmitModuleFormatOfFileWorker @6.0.3
    /// tsc-hash: ffe7b58092e4af38c9484bef12201ef7524d2e3d26ba829ea59087f1a2c0d2a1
    /// tsc-span: _tsc.js:125493-125495
    fn get_emit_module_format_of_file(&self, id: SourceFileId) -> Option<i32> {
        let source = self.source_file(id)?;
        Some(match source.implied_node_format_for_emit() {
            Some(ResolutionMode::CommonJs) => 1,
            Some(ResolutionMode::EsNext) => 99,
            Some(ResolutionMode::Unspecified) | None => self.compiler_options().emit_module_kind(),
        })
    }

    /// Symlink facts discovered from the program's resolutions (upstream
    /// `getSymlinkCache`, consumed by module specifier generation):
    /// `(real_path, symlink_path)` per aliased file. Hosts without
    /// resolutions expose none.
    fn symlinked_files(&self) -> Vec<(JsString, JsString)> {
        Vec::new()
    }

    /// `(real_directory, symlink_directory)` per guessed directory link (see
    /// [`Self::symlinked_files`]).
    fn symlinked_directories(&self) -> Vec<(JsString, JsString)> {
        Vec::new()
    }

    /// Canonicalize one output spelling with the same case policy used for
    /// source identities. Implementations may override this when their path
    /// model is richer than the frozen POSIX H1 profile.
    fn canonical_output_path(&self, path: JsStr<'_>) -> JsString {
        tsc_program::canonical_emit_path(
            path,
            self.current_directory(),
            self.use_case_sensitive_file_names(),
        )
    }
}
