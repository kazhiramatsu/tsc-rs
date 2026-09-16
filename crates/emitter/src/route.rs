//! Typed emit route selection for the H2.8c no-check pipelines.
//!
//! TypeScript reaches the same `emitFiles` worker from three public entries:
//! an ordinary `Program.emit`, `Program.emit` under `noCheck`, and the two
//! `transpileWorker` routes (`transpileModule` / `transpileDeclaration`,
//! typescript.js:146022-146133) which force `noCheck`, `isolatedModules`,
//! `noLib`/`noResolve` and run a single-file Program. The Rust bootstrap
//! option admission (`validate_bootstrap_emit_options`) refuses `noCheck`,
//! `isolatedModules` and `verbatimModuleSyntax` for the ordinary Program
//! route; the research routes below admit them explicitly so the option
//! decision lives in one typed place instead of string branches in passes.
//!
//! tsrs-native: research plan for the H2.8c prototype; no tsc counterpart.

/// Which public TypeScript entry produced the emit request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EmitRouteKind {
    /// Ordinary whole-Program emit. Keeps every existing admission refusal.
    #[default]
    Program,
    /// `Program.emit` with `compilerOptions.noCheck = true`
    /// (_tsc.js:116597 markLinkedReferences, 116650 collectLinkedAliases,
    /// 88132 calculateNodeCheckFlagWorker consumers).
    ProgramNoCheck,
    /// `transpileModule`: forced `noCheck`, `isolatedModules`, `noLib`,
    /// `noResolve`; declaration output disabled.
    TranspileJavaScript,
    /// `transpileDeclaration`: forced `noCheck`, `isolatedModules`,
    /// `noResolve`, `declaration`, `emitDeclarationOnly`,
    /// `isolatedDeclarations`; barebones `lib.d.ts`; forced d.ts emit.
    TranspileDeclaration,
}

impl EmitRouteKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Program => "program",
            Self::ProgramNoCheck => "program-no-check",
            Self::TranspileJavaScript => "transpile-js",
            Self::TranspileDeclaration => "transpile-dts",
        }
    }

    /// Whether `noCheck = true` is admitted for a Files emit on this route.
    pub const fn admits_no_check(self) -> bool {
        !matches!(self, Self::Program)
    }

    /// Whether the transpile-forced `isolatedModules` / caller-supplied
    /// `verbatimModuleSyntax` are admitted. Research routes only: the
    /// ordinary Program route keeps its bootstrap refusal.
    pub const fn admits_isolated_module_options(self) -> bool {
        matches!(self, Self::TranspileJavaScript | Self::TranspileDeclaration)
    }

    pub const fn is_transpile(self) -> bool {
        self.admits_isolated_module_options()
    }

    /// `options.suppressOutputPathCheck = true` (typescript.js:146040): the
    /// transpile host lets the single output overwrite its in-memory input
    /// path, so verifyEmitFilePath is skipped and nothing is emit-blocked.
    pub const fn suppresses_output_path_check(self) -> bool {
        self.is_transpile()
    }
}
