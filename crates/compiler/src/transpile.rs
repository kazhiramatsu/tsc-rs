//! H2.8c research prototype: `transpileModule` / `transpileDeclaration`.
//!
//! tsc-port: transpileWorker @6.0.3
//! tsc-hash: 15cd02923bfe29ddb59ffdd59213aaafcd0ae77fabf6d97b87c2b82ee21a17ed
//! tsc-span: typescript.js:146022-146132
//!
//! The pinned TypeScript reaches both public APIs through one worker: it
//! fixes up enum-typed option strings, applies the transpile defaults and
//! forced values (`noCheck`, `isolatedModules`, `noLib`, `noResolve`, …),
//! parses the single input into an in-memory host, creates a whole Program
//! and runs the ordinary emitter. The Rust adapter below does the same over
//! the production loader, checker and emitter; only the emit-route admission
//! (see [`EmitRouteKind`]) differs from an ordinary `ProgramSession`.
//!
//! This module is a research route: its names and shapes are not a stable
//! public API. The Program-level `noCheck` route is reached through
//! [`ProgramSession::with_emit_route`] instead.

use std::path::PathBuf;

use tsc_diagnostics::{gen, Diagnostic, DiagnosticList, JsString, MessageChain};
use tsc_emitter::{EmitArtifactKind, EmitRouteKind, MemoryOutputSink};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    compiler_option_declaration, compiler_option_named_choices, load_emitting_program,
    CompilerOptionValueKind, CompilerOptions, LibraryCatalog, ProgramLoadError, ProgramLoadLimits,
    ProgramOptions,
};

use crate::{DriverError, NoEmitWorkCounters, ProgramSession, SourceApiFacts};

/// `lib.d.ts` served to `transpileDeclaration` (typescript.js:145996-146015).
pub const BAREBONES_LIB_CONTENT: &str = "interface Boolean {}
interface Function {}
interface CallableFunction {}
interface NewableFunction {}
interface IArguments {}
interface Number {}
interface Object {}
interface RegExp {}
interface String {}
interface Array<T> { length: number; [n: number]: T; }
interface SymbolConstructor {
    (desc?: string | number): symbol;
    for(name: string): symbol;
    readonly toStringTag: symbol;
}
declare var Symbol: SymbolConstructor;
interface Symbol {
    readonly [Symbol.toStringTag]: string;
}";
const BAREBONES_LIB_NAME: &str = "lib.d.ts";
const VIRTUAL_LIB_DIRECTORY: &str = "/lib";

/// A caller-supplied `compilerOptions` value before fixup. Mirrors the JSON
/// value domain `transpileModule` accepts; unknown option names are ignored
/// exactly as TypeScript ignores them.
#[derive(Clone, Debug, PartialEq)]
pub enum TranspileOptionValue {
    Bool(bool),
    Number(f64),
    String(JsString),
    StringList(Vec<JsString>),
}

/// `JSDocParsingMode` (typescript.js: ParseAll = 0, ParseNone = 1, ...),
/// the parser's own enum re-exported under the API spelling.
pub use crate::JSDocParsingMode as JsDocParsingMode;

/// `ts.TranspileOptions` minus `transformers` (custom transforms stay an API1
/// control outside this prototype).
#[derive(Clone, Debug, Default)]
pub struct TranspileOptions {
    pub compiler_options: Option<Vec<(JsString, TranspileOptionValue)>>,
    pub file_name: Option<JsString>,
    pub report_diagnostics: Option<bool>,
    pub module_name: Option<JsString>,
    pub renamed_dependencies: Option<Vec<(JsString, JsString)>>,
    pub jsdoc_parsing_mode: Option<JsDocParsingMode>,
}

/// `ts.TranspileOutput`: the three public fields, plus route evidence that
/// is reported separately and never compared as public API.
#[derive(Clone, Debug)]
pub struct TranspileOutput {
    pub output_text: JsString,
    pub diagnostics: DiagnosticList,
    pub source_map_text: Option<JsString>,
    pub evidence: TranspileEvidence,
}

/// Internal route evidence (handoff "別記する内部証拠").
#[derive(Clone, Debug)]
pub struct TranspileEvidence {
    pub route: EmitRouteKind,
    pub input_file_name: JsString,
    pub effective_options: CompilerOptions,
    pub source_files: Vec<JsString>,
    pub writes: Vec<(JsString, EmitArtifactKind)>,
    pub emit_skipped: bool,
    pub fixup_diagnostics: usize,
    pub syntactic_diagnostics: DiagnosticList,
    pub options_diagnostics: DiagnosticList,
    /// `program.getGlobalDiagnostics()` and `getSemanticDiagnostics()` are
    /// never read by transpileWorker; recorded to show what the forced
    /// `noCheck` suppressed.
    pub global_diagnostics_evidence: DiagnosticList,
    pub semantic_diagnostics_evidence: DiagnosticList,
    pub checked_source_files: u32,
    pub work_counters: NoEmitWorkCounters,
}

/// Failures that TypeScript reports as thrown exceptions plus Rust-only typed
/// refusals. Rust-only rows are never counted as TypeScript compatibility.
#[derive(Debug)]
pub enum TranspileError {
    /// `Debug.fail("Output generation failed")` (typescript.js:146131): the
    /// host received no primary output.
    OutputGenerationFailed {
        writes: Vec<(JsString, EmitArtifactKind)>,
    },
    /// Rust-only: more than one primary output reached the host (TypeScript
    /// asserts the same condition).
    MultipleOutputs {
        writes: Vec<(JsString, EmitArtifactKind)>,
    },
    /// Rust-only: an option whose value cannot be represented in the typed
    /// `CompilerOptions` (TypeScript passes such values through untyped).
    UnsupportedOptionValue { name: JsString, detail: String },
    /// Rust-only: a `TranspileOptions` field with no Rust producer. Every
    /// field of [`TranspileOptions`] currently has one; the variant remains
    /// for the API1 `transformers` control, which is outside this adapter.
    UnsupportedTranspileOption { field: &'static str },
    /// Rust-only: the loader refused the single input (extension or text).
    Program(Box<ProgramLoadError>),
    /// Rust-only: the checked session or emitter refused the request.
    Driver(Box<DriverError>),
}

impl std::fmt::Display for TranspileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutputGenerationFailed { .. } => {
                formatter.write_str("Debug Failure. Output generation failed")
            }
            Self::MultipleOutputs { .. } => formatter.write_str("Unexpected multiple outputs"),
            Self::UnsupportedOptionValue { name, detail } => write!(
                formatter,
                "unsupported transpile option value for {}: {detail}",
                name.to_string_lossy()
            ),
            Self::UnsupportedTranspileOption { field } => {
                write!(formatter, "unsupported TranspileOptions field: {field}")
            }
            Self::Program(error) => write!(formatter, "program load failed: {error}"),
            Self::Driver(error) => write!(formatter, "driver failed: {error}"),
        }
    }
}

impl std::error::Error for TranspileError {}

/// tsc-port: transpileModule @6.0.3
/// tsc-hash: 5be8c9ca2ef415ec3a3d1c814585a44abb6c1c3ce96a52f910ddc132d19b86aa
/// tsc-span: typescript.js:145985-145992
pub fn transpile_module(
    input: &str,
    options: &TranspileOptions,
) -> Result<TranspileOutput, TranspileError> {
    transpile_worker(input, options, false)
}

/// tsc-port: transpileDeclaration @6.0.3
/// tsc-hash: 9814448d999703d6fe2557710c5cfe4a13a830b9d68768be3e3ec1bc40ddc1a1
/// tsc-span: typescript.js:145993-146000
pub fn transpile_declaration(
    input: &str,
    options: &TranspileOptions,
) -> Result<TranspileOutput, TranspileError> {
    transpile_worker(input, options, true)
}

/// Options TypeScript forces on both transpile routes
/// (`transpileOptionValueCompilerOptions`, _tsc.js:37935, the declarations
/// carrying `transpileOptionValue`): `noCheck`, `isolatedModules`, `noLib`,
/// `noResolve` become `true`; every other listed option becomes `undefined`.
pub const FORCED_TRUE_OPTIONS: [&str; 4] = ["noCheck", "isolatedModules", "noLib", "noResolve"];
pub const FORCED_UNDEFINED_OPTIONS: [&str; 15] = [
    "incremental",
    "declaration",
    "emitDeclarationOnly",
    "noEmit",
    "lib",
    "outFile",
    "composite",
    "tsBuildInfoFile",
    "paths",
    "rootDirs",
    "types",
    "allowImportingTsExtensions",
    "out",
    "noEmitOnError",
    "declarationDir",
];

/// tsc-port: transpileWorker @6.0.3
/// tsc-hash: 15cd02923bfe29ddb59ffdd59213aaafcd0ae77fabf6d97b87c2b82ee21a17ed
/// tsc-span: typescript.js:146022-146132
fn transpile_worker(
    input: &str,
    transpile_options: &TranspileOptions,
    declaration: bool,
) -> Result<TranspileOutput, TranspileError> {
    let route = if declaration {
        EmitRouteKind::TranspileDeclaration
    } else {
        EmitRouteKind::TranspileJavaScript
    };
    let mut diagnostics = DiagnosticList::new();
    let mut options = match &transpile_options.compiler_options {
        Some(raw) => fixup_compiler_options(raw, &mut diagnostics)?,
        None => CompilerOptions::default(),
    };
    let fixup_diagnostics = diagnostics.len();

    // getDefaultCompilerOptions (typescript.js:153190): target LatestStandard
    // (12), jsx Preserve (1), applied only where the caller left a hole.
    if options.target.is_none() {
        options.target = Some(12);
    }
    if options.jsx.is_none() {
        options.jsx = Some(1);
    }
    // transpileOptionValueCompilerOptions (146034-146039)
    options.no_check = Some(true);
    if options.verbatim_module_syntax != Some(true) {
        options.isolated_modules = Some(true);
    }
    // noLib lives on the Rust ProgramOptions; see `program_options` below.
    options.no_resolve = Some(true);
    options.incremental = None;
    options.declaration = None;
    options.emit_declaration_only = None;
    options.no_emit = None;
    options.lib = None;
    options.out_file = None;
    options.composite = None;
    options.ts_build_info_file = None;
    options.out = None;
    options.no_emit_on_error = None;
    options.declaration_dir = None;
    // suppressOutputPathCheck is selected by the emit route. The internal
    // allowNonTsExtensions option is set before loading the single root.
    if declaration {
        options.declaration = Some(true);
        options.emit_declaration_only = Some(true);
        options.isolated_declarations = Some(true);
    } else {
        options.declaration = Some(false);
        options.declaration_map = Some(false);
    }
    // options.noLib = !declaration (146061) is realized through ProgramOptions.

    // inputFileName (146089): the raw `compilerOptions.jsx` decides the
    // default extension before any fixup.
    let raw_jsx_truthy = transpile_options
        .compiler_options
        .as_ref()
        .and_then(|raw| raw.iter().find(|(name, _)| name.as_str() == Some("jsx")))
        .is_some_and(|(_, value)| match value {
            TranspileOptionValue::Bool(value) => *value,
            TranspileOptionValue::Number(value) => *value != 0.0,
            TranspileOptionValue::String(value) => !value.is_empty(),
            TranspileOptionValue::StringList(_) => true,
        });
    let input_file_name = transpile_options
        .file_name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            JsString::from(if raw_jsx_truthy {
                "module.tsx"
            } else {
                "module.ts"
            })
        });
    let normalized_file_name = tsc_program::normalize_path(input_file_name.as_js());
    let input_file_name_text = normalized_file_name
        .as_str()
        .ok_or_else(|| TranspileError::UnsupportedOptionValue {
            name: JsString::from("fileName"),
            detail: "non-Unicode file names are outside the prototype".to_owned(),
        })?
        .to_owned();

    // allowNonTsExtensions (146041): any extension (or none) is admitted for
    // the single root; the script kind derives from the name with TS as the
    // default. JavaScript spellings additionally set allowJs so the
    // JavaScript-kind emit admission matches an ordinary allowJs Program.
    options.allow_non_ts_extensions = Some(true);
    let lower = input_file_name_text.to_ascii_lowercase();
    if [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        options.allow_js = true;
    }

    // The virtual host (146066-146088): current directory "" becomes "/",
    // the input is the only source; `fileExists` answers the lib only for
    // the declaration route.
    let root_path = if input_file_name_text.starts_with('/') {
        input_file_name_text.clone()
    } else {
        format!("/{input_file_name_text}")
    };
    let mut host = MemoryCompilerHost::builder("/")
        .case_sensitive(false)
        .file(root_path.clone(), input.as_bytes().to_vec());
    let mut program_options = ProgramOptions::default();
    if declaration {
        host = host.file(
            format!("{VIRTUAL_LIB_DIRECTORY}/{BAREBONES_LIB_NAME}"),
            BAREBONES_LIB_CONTENT.as_bytes().to_vec(),
        );
        program_options = program_options.with_default_library_file_name(BAREBONES_LIB_NAME);
    } else {
        program_options = program_options.with_no_lib(true);
    }
    let host = host
        .build()
        .map_err(|error| TranspileError::UnsupportedOptionValue {
            name: JsString::from("fileName"),
            detail: format!("virtual host rejected the input path: {error:?}"),
        })?;
    let prepared = load_emitting_program(
        &host,
        &[PathBuf::from(&root_path)],
        options.clone(),
        program_options,
        &LibraryCatalog::typescript_6_0_3(VIRTUAL_LIB_DIRECTORY),
        ProgramLoadLimits::new(4, 64, 4, 64 * 1024 * 1024, 64 * 1024 * 1024),
    )
    .map_err(|error| TranspileError::Program(Box::new(error)))?;
    let source_files = prepared
        .source_files()
        .iter()
        .map(|source| {
            project_file_name(
                source.path().display().to_owned(),
                &root_path,
                &input_file_name_text,
            )
        })
        .collect::<Vec<_>>();

    // sourceFile.fileName / moduleName / renamedDependencies /
    // jsDocParsingMode (146090-146104) are assigned to the created input
    // SourceFile; the barebones lib keeps its defaults.
    let root_id = prepared.roots().first().and_then(|root| root.source());
    let facts = SourceApiFacts {
        file_name: Some(normalized_file_name),
        // `if (transpileOptions.moduleName)`: the empty string is falsy.
        module_name: transpile_options
            .module_name
            .clone()
            .filter(|name| !name.is_empty()),
        renamed_dependencies: transpile_options
            .renamed_dependencies
            .clone()
            .unwrap_or_default(),
        js_doc_parsing_mode: transpile_options.jsdoc_parsing_mode,
    };
    let mut sink = MemoryOutputSink::new();
    let mut session = ProgramSession::new(prepared).with_emit_route(route);
    if let Some(root_id) = root_id {
        session = session.with_source_api_facts(root_id, facts);
    }
    let outcome = if declaration {
        session.emit_forced_declarations_command_for_transpile(&mut sink)
    } else {
        session.emit_for_cli(&mut sink)
    }
    .map_err(|error| TranspileError::Driver(Box::new(error)))?;

    // reportDiagnostics (146100-146112): syntactic then options; the emit
    // result diagnostics always join (146122-146130).
    let project =
        |diagnostic: &Diagnostic| project_diagnostic(diagnostic, &root_path, &input_file_name_text);
    let syntactic = outcome
        .syntactic_diagnostics
        .iter()
        .map(project)
        .collect::<Vec<_>>();
    let options_diagnostics = outcome
        .options_diagnostics
        .iter()
        .map(project)
        .collect::<Vec<_>>();
    if transpile_options.report_diagnostics == Some(true) {
        diagnostics.extend(syntactic.iter().cloned());
        diagnostics.extend(options_diagnostics.iter().cloned());
    }
    diagnostics.extend(outcome.emit.diagnostics().iter().map(project));

    let writes = sink
        .writes()
        .iter()
        .map(|write| (write.path().to_owned(), write.kind()))
        .collect::<Vec<_>>();
    let mut output_text = None;
    let mut source_map_text = None;
    for write in sink.writes() {
        // host.writeFile (146073-146081): `.map` names become sourceMapText,
        // everything else is the single outputText.
        let text = JsString::from(write.callback_text());
        if write.path().ends_with(".map") {
            if source_map_text.is_some() {
                return Err(TranspileError::MultipleOutputs { writes });
            }
            source_map_text = Some(text);
        } else {
            if output_text.is_some() {
                return Err(TranspileError::MultipleOutputs { writes });
            }
            output_text = Some(text);
        }
    }
    let Some(output_text) = output_text else {
        return Err(TranspileError::OutputGenerationFailed { writes });
    };
    Ok(TranspileOutput {
        output_text,
        diagnostics,
        source_map_text,
        evidence: TranspileEvidence {
            route,
            input_file_name,
            effective_options: options,
            source_files,
            writes,
            emit_skipped: outcome.emit.emit_skipped(),
            fixup_diagnostics,
            syntactic_diagnostics: syntactic,
            options_diagnostics,
            global_diagnostics_evidence: outcome.global_diagnostics.iter().map(project).collect(),
            semantic_diagnostics_evidence: outcome
                .semantic_diagnostics
                .iter()
                .map(project)
                .collect(),
            checked_source_files: outcome.checked_source_files,
            work_counters: outcome.work_counters,
        },
    })
}

/// The virtual host roots the input at "/"; public diagnostics keep the
/// caller's spelling (`sourceFile.fileName`, typescript.js:146090).
fn project_file_name(path: JsString, root_path: &str, input_file_name: &str) -> JsString {
    if path.as_str() == Some(root_path) {
        JsString::from(input_file_name)
    } else {
        path
    }
}

fn project_diagnostic(
    diagnostic: &Diagnostic,
    root_path: &str,
    input_file_name: &str,
) -> Diagnostic {
    let mut projected = diagnostic.clone();
    projected.file_name = projected
        .file_name
        .map(|name| project_file_name(name, root_path, input_file_name));
    projected.file_path = projected
        .file_path
        .map(|name| project_file_name(name, root_path, input_file_name));
    for related in &mut projected.related {
        related.file_name = related
            .file_name
            .take()
            .map(|name| project_file_name(name, root_path, input_file_name));
    }
    projected
}

/// tsc-port: fixupCompilerOptions @6.0.3
/// tsc-hash: 60647892c006801252dcdd247b97c3f01bc09e50af098063ae2b2bc48f244287
/// tsc-span: typescript.js:146139-146156
///
/// Enum-typed options given as strings are parsed with TS6046 on failure;
/// enum-typed numbers outside the map also report TS6046 (and keep their
/// value). Every other option is stored as given. Unknown names are ignored.
pub fn fixup_compiler_options(
    raw: &[(JsString, TranspileOptionValue)],
    diagnostics: &mut DiagnosticList,
) -> Result<CompilerOptions, TranspileError> {
    let mut options = CompilerOptions::default();
    for (name, value) in raw {
        let Some(name_text) = name.as_str() else {
            continue;
        };
        let declaration = compiler_option_declaration(name_text);
        let named_values = match declaration.map(|declaration| declaration.value_kind()) {
            Some(CompilerOptionValueKind::Named(values)) => Some(values),
            _ => None,
        };
        let mut typed = value.clone();
        if let Some(values) = named_values {
            match value {
                TranspileOptionValue::String(text) => {
                    match text
                        .as_str()
                        .and_then(|text| CompilerOptionValueKind::Named(values).named_value(text))
                    {
                        Some(number) => typed = TranspileOptionValue::Number(f64::from(number)),
                        None => {
                            diagnostics.push(invalid_custom_type(name_text));
                            continue;
                        }
                    }
                }
                TranspileOptionValue::Number(number) => {
                    if !values
                        .iter()
                        .any(|candidate| f64::from(candidate.value()) == *number)
                    {
                        diagnostics.push(invalid_custom_type(name_text));
                    }
                }
                TranspileOptionValue::Bool(_) | TranspileOptionValue::StringList(_) => {
                    diagnostics.push(invalid_custom_type(name_text));
                }
            }
        }
        assign_option(&mut options, name_text, &typed)?;
    }
    Ok(options)
}

/// tsc-port: createCompilerDiagnosticForInvalidCustomType @6.0.3
/// tsc-hash: 0330bc27d0b2048dfce2f80180bea27e78fe227e4058973da720817d033fcde3
/// tsc-span: typescript.js:42338-42340
fn invalid_custom_type(name: &str) -> Diagnostic {
    let choices = compiler_option_named_choices(name).unwrap_or_default();
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(
            &gen::Argument_for_0_option_must_be_1,
            &[format!("--{name}"), choices],
        ),
    )
}

fn assign_option(
    options: &mut CompilerOptions,
    name: &str,
    value: &TranspileOptionValue,
) -> Result<(), TranspileError> {
    use TranspileOptionValue as V;
    let unsupported = |detail: &str| TranspileError::UnsupportedOptionValue {
        name: JsString::from(name),
        detail: detail.to_owned(),
    };
    let bool_value = || match value {
        V::Bool(value) => Ok(Some(*value)),
        _ => Err(unsupported("expected a boolean")),
    };
    let i32_value = || match value {
        V::Number(value) => Ok(Some(*value as i32)),
        _ => Err(unsupported("expected a number")),
    };
    let string_value = || match value {
        V::String(value) => Ok(Some(value.clone())),
        _ => Err(unsupported("expected a string")),
    };
    match name {
        "target" => options.target = i32_value()?,
        "module" => options.module = i32_value()?,
        "moduleResolution" => options.module_resolution = i32_value()?,
        "moduleDetection" => options.module_detection = i32_value()?,
        "jsx" => options.jsx = i32_value()?,
        "newLine" => options.new_line = i32_value()?,
        "importsNotUsedAsValues" => options.imports_not_used_as_values = i32_value()?,
        "allowJs" => options.allow_js = bool_value()?.unwrap_or(false),
        "experimentalDecorators" => {
            options.experimental_decorators = bool_value()?.unwrap_or(false);
        }
        "emitDecoratorMetadata" => options.emit_decorator_metadata = bool_value()?,
        "useDefineForClassFields" => options.use_define_for_class_fields = bool_value()?,
        "sourceMap" => options.source_map = bool_value()?,
        "inlineSourceMap" => options.inline_source_map = bool_value()?,
        "inlineSources" => options.inline_sources = bool_value()?,
        "sourceRoot" => options.source_root = string_value()?,
        "mapRoot" => options.map_root = string_value()?,
        "declaration" => options.declaration = bool_value()?,
        "declarationMap" => options.declaration_map = bool_value()?,
        "emitDeclarationOnly" => options.emit_declaration_only = bool_value()?,
        "isolatedDeclarations" => options.isolated_declarations = bool_value()?,
        "stripInternal" => options.strip_internal = bool_value()?,
        "removeComments" => options.remove_comments = bool_value()?,
        "emitBOM" => options.emit_bom = bool_value()?,
        "noEmit" => options.no_emit = bool_value()?,
        "noEmitOnError" => options.no_emit_on_error = bool_value()?,
        "noEmitHelpers" => options.no_emit_helpers = bool_value()?,
        "importHelpers" => options.import_helpers = bool_value()?,
        "downlevelIteration" => options.downlevel_iteration = bool_value()?,
        "noCheck" => options.no_check = bool_value()?,
        // noLib is forced by the route (ProgramOptions), the caller value is
        // overwritten exactly as TypeScript overwrites it.
        "noLib" => {}
        "noResolve" => options.no_resolve = bool_value()?,
        "isolatedModules" => options.isolated_modules = bool_value()?,
        "verbatimModuleSyntax" => options.verbatim_module_syntax = bool_value()?,
        "preserveConstEnums" => options.preserve_const_enums = bool_value()?,
        "esModuleInterop" => options.es_module_interop = bool_value()?,
        "allowSyntheticDefaultImports" => options.allow_synthetic_default_imports = bool_value()?,
        "strict" => options.strict = bool_value()?,
        "checkJs" => options.check_js = bool_value()?,
        "listEmittedFiles" => options.list_emitted_files = bool_value()?,
        "skipLibCheck" => options.skip_lib_check = bool_value()?,
        "skipDefaultLibCheck" => options.skip_default_lib_check = bool_value()?,
        "noErrorTruncation" => options.no_error_truncation = bool_value()?,
        "resolveJsonModule" => options.resolve_json_module = bool_value()?,
        "noImplicitUseStrict" => options.no_implicit_use_strict = bool_value()?,
        "alwaysStrict" => options.always_strict = bool_value()?,
        "incremental" => options.incremental = bool_value()?,
        "composite" => options.composite = bool_value()?,
        "outFile" | "out" => options.out_file = string_value()?,
        "outDir" => options.out_dir = string_value()?,
        "rootDir" => options.root_dir = string_value()?,
        "declarationDir" => options.declaration_dir = string_value()?,
        "tsBuildInfoFile" => options.ts_build_info_file = string_value()?,
        "ignoreDeprecations" => options.ignore_deprecations = string_value()?,
        "jsxFactory" => options.jsx_factory = string_value()?,
        "jsxFragmentFactory" => options.jsx_fragment_factory = string_value()?,
        "jsxImportSource" => options.jsx_import_source = string_value()?,
        "lib" => {
            options.lib = match value {
                V::StringList(values) => Some(
                    values
                        .iter()
                        .map(|value| value.to_string_lossy().into_owned())
                        .collect(),
                ),
                _ => return Err(unsupported("expected a string list")),
            }
        }
        // Unknown or untyped names: TypeScript stores them untouched and no
        // consumer reads them (146029-146031); the typed options drop them.
        _ => {}
    }
    Ok(())
}
