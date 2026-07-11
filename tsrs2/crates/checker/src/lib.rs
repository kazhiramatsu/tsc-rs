#![forbid(unsafe_code)]

pub mod annotate;
pub mod check;
pub mod constraints;
pub mod engine;
pub mod evaluate;
pub mod globals;
pub mod indexed;
pub mod instantiate;
pub mod intersect;
mod js_grammar;
pub mod links;
pub mod merge;
mod plain_js_errors;
pub mod program;
pub mod relate;
pub mod relpin;
pub mod resolve;
pub mod state;
pub mod structural;
pub mod unions;
pub mod variance;

use tsrs2_diags::DiagnosticList;

pub use tsrs2_types::CompilerOptions;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputFile {
    pub name: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CheckResult {
    pub diagnostics: DiagnosticList,
    /// tsc getSyntacticDiagnostics: the per-file parse diagnostics alone.
    pub syntactic_diagnostics: DiagnosticList,
}

/// tsc getSupportedExtensions: JS roots only join the program with allowJs.
fn is_supported_source_file_name(name: &str, allow_js: bool) -> bool {
    let ts_like = [".ts", ".tsx", ".mts", ".cts", ".json"];
    ts_like.iter().any(|extension| name.ends_with(extension)) || (allow_js && is_js_file_name(name))
}

pub(crate) fn is_js_file_name(name: &str) -> bool {
    [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

/// tsc scanner commentDirectiveRegExSingleLine (8202) +
/// getDiagnosticsWithPrecedingDirectives / markPrecedingCommentDirectiveLine
/// (123756): a `// @ts-ignore` / `// @ts-expect-error` comment
/// suppresses bind/check diagnostics on the following line (walking up
/// over blank and comment-only lines). INTERIM SCOPE: only directives
/// on comment-only lines are detected (the scanner-side directive
/// collection lands with real comment ranges); multi-line-comment
/// directives are not handled.
fn filter_by_comment_directives(
    text: &str,
    line_map: &tsrs2_diags::LineMap,
    diagnostics: impl Iterator<Item = tsrs2_diags::Diagnostic>,
) -> Vec<tsrs2_diags::Diagnostic> {
    // LineMap.line_starts are UTF-16 offsets; build BYTE line starts
    // with the same break set (\r\n, \r, \n, U+2028, U+2029) for text
    // slicing.
    let byte_line_starts = compute_byte_line_starts(text);
    let line_text = |line: usize| -> &str {
        let start = byte_line_starts[line];
        let end = byte_line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(text.len());
        &text[start..end]
    };
    let line_starts = &line_map.line_starts;
    let is_directive_line = |line: usize| -> bool {
        let trimmed = line_text(line).trim_start();
        let Some(comment) = trimmed.strip_prefix("//") else {
            return false;
        };
        // regex ^///?\s*@(ts-expect-error|ts-ignore) applied at the
        // comment start.
        let comment = comment.strip_prefix('/').unwrap_or(comment);
        let comment = comment.trim_start();
        comment.starts_with("@ts-expect-error") || comment.starts_with("@ts-ignore")
    };
    let directive_lines: Vec<usize> = (0..byte_line_starts.len())
        .filter(|&line| is_directive_line(line))
        .collect();
    if directive_lines.is_empty() {
        return diagnostics.collect();
    }
    // Diagnostic.start is UTF-16, matching line_starts' units.
    let utf16_line_starts: &[u32] = line_starts;
    diagnostics
        .filter(|diagnostic| {
            let Some(start) = diagnostic.start else {
                return true;
            };
            let diagnostic_line = match utf16_line_starts.binary_search(&start) {
                Ok(line) => line,
                Err(insert) => insert.saturating_sub(1),
            };
            let mut line = diagnostic_line;
            while line > 0 {
                line -= 1;
                if directive_lines.contains(&line) {
                    return false; // suppressed
                }
                let trimmed = line_text(line).trim();
                if !trimmed.is_empty() && !trimmed.starts_with("//") {
                    return true;
                }
            }
            true
        })
        .collect()
}

/// Byte-offset line starts with tsc's line-break set (\r\n, \r, \n,
/// U+2028, U+2029) — index-compatible with LineMap.line_starts.
fn compute_byte_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    let mut chars = text.char_indices().peekable();
    while let Some((byte, ch)) = chars.next() {
        match ch {
            '\r' => {
                let mut next_start = byte + 1;
                if let Some(&(next_byte, '\n')) = chars.peek() {
                    chars.next();
                    next_start = next_byte + 1;
                }
                starts.push(next_start);
            }
            '\n' => starts.push(byte + 1),
            '\u{2028}' | '\u{2029}' => starts.push(byte + ch.len_utf8()),
            _ => {}
        }
    }
    starts
}

pub fn check_program(files: &[InputFile], options: &CompilerOptions) -> CheckResult {
    check_program_with_libs(&[], files, options)
}

/// Program construction under the oracle contract
/// (m4-lib-loading-steps.md §1): `libs` are ORDINARY files prepended
/// to the program in the order given (the harness's priority-sorted
/// expansion; the oracle host runs noLib:true with the same list as
/// prepended roots, so `<reference lib>` is inert and getSourceFiles
/// order == libs ++ files). They ride the same parse/bind/globals-
/// merge pipeline through a per-lib-set CACHED prefix (LibBundle:
/// same-key programs share one parsed+bound copy — exact, because
/// libs are the program prefix and their id bases are therefore
/// identical across programs). Lib files are never CHECKED and no
/// diagnostic band of theirs surfaces — tsc checks files lazily per
/// getDiagnostics(file) call and the oracle driver only ever asks for
/// fixture files, so a lib file's checkSourceFileWorker never runs
/// and diagnostics FILED under a lib file are never collected.
pub fn check_program_with_libs(
    libs: &[InputFile],
    files: &[InputFile],
    options: &CompilerOptions,
) -> CheckResult {
    let mut diagnostics = Vec::new();
    let mut syntactic_diagnostics = Vec::new();

    // tsc host semantics: files are a name-keyed map, so a fixture
    // file sharing a lib's name provides the TEXT everywhere. The
    // cached prefix cannot honor per-program shadowing, so a shadowed
    // lib simply drops from the prefix (the fixture supplies the
    // content at its own position; position drifts from tsc's only in
    // this case, which no corpus fixture produces).
    let fixture_names: std::collections::HashSet<&str> =
        files.iter().map(|file| file.name.as_str()).collect();
    let effective_libs: Vec<&InputFile> = libs
        .iter()
        .filter(|lib| !fixture_names.contains(lib.name.as_str()))
        .collect();
    let bundle = (!effective_libs.is_empty()).then(|| lib_bundle(&effective_libs, options));
    let (lib_sources, lib_binders): (
        &[tsrs2_syntax::SourceFile],
        &[tsrs2_binder::Binder<'_>],
    ) = match bundle {
        Some(bundle) => (bundle.sources, bundle.binders),
        None => (&[], &[]),
    };

    // Fixture-file shadowing (unchanged from the libless world): a
    // later file with the same name shadows an earlier one entirely.
    let mut last_index_by_name = std::collections::BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        last_index_by_name.insert(file.name.as_str(), index);
    }

    // Fixture parse pass (M4 5.0): files parse in program order with
    // contiguous NodeId/NodeArrayId bases CONTINUING FROM THE LIB
    // PREFIX so the checker sees tsc's one-heap identity space; .json
    // inputs parse as JSON values outside the bind program (semantic
    // .json checking is a later stage — ledger note).
    let mut program_sources: Vec<tsrs2_syntax::SourceFile> = Vec::new();
    for (index, file) in files.iter().enumerate() {
        if last_index_by_name.get(file.name.as_str()) != Some(&index) {
            continue;
        }
        // tsc createProgram only loads roots with supported extensions;
        // anything else (.txt, extensionless, .js without allowJs) never
        // yields syntactic diagnostics.
        if !is_supported_source_file_name(&file.name, options.allow_js) {
            continue;
        }
        // tsc ensureScriptKind: .json programs parse as JSON values.
        if file.name.ends_with(".json") {
            let source_file = tsrs2_syntax::parse_json_text(file.name.clone(), file.text.clone());
            syntactic_diagnostics.extend(source_file.parse_diagnostics.iter().cloned());
            diagnostics.extend(source_file.parse_diagnostics.iter().cloned());
            continue;
        }
        // tsc getLanguageVariant: JSX scanning for TSX/JSX/JS script kinds.
        let javascript_file = is_js_file_name(&file.name);
        let language_variant = if file.name.ends_with(".tsx") || javascript_file {
            tsrs2_syntax::LanguageVariant::Jsx
        } else {
            tsrs2_syntax::LanguageVariant::Standard
        };
        let (node_id_base, node_array_id_base) = match program_sources.last() {
            Some(previous) => (previous.arena.node_end(), previous.arena.array_end()),
            None => lib_sources
                .last()
                .map(|previous| (previous.arena.node_end(), previous.arena.array_end()))
                .unwrap_or((0, 0)),
        };
        let source_file = tsrs2_syntax::parse_source_file(
            file.name.clone(),
            file.text.clone(),
            tsrs2_syntax::ParseOptions {
                language_variant,
                javascript_file,
                node_id_base,
                node_array_id_base,
            },
            None,
        );
        // tsc getSyntacticDiagnosticsForFile: JS files prepend the
        // TypeScript-only-syntax walker output to their parse diagnostics.
        if is_js_file_name(&file.name) {
            let js_diagnostics = js_grammar::get_js_syntactic_diagnostics(
                &source_file,
                options.experimental_decorators,
            );
            syntactic_diagnostics.extend(js_diagnostics.iter().cloned());
            diagnostics.extend(js_diagnostics);
        }
        syntactic_diagnostics.extend(source_file.parse_diagnostics.iter().cloned());
        diagnostics.extend(source_file.parse_diagnostics.iter().cloned());
        program_sources.push(source_file);
    }

    // Fixture bind pass: per-file binders with contiguous SymbolId
    // bases continuing from the lib prefix (tsc bindSourceFile per
    // file over one heap).
    let mut binders: Vec<tsrs2_binder::Binder<'_>> = Vec::new();
    for source_file in &program_sources {
        let (symbol_id_seed, symbol_base) = match binders.last() {
            Some(previous) => (previous.next_symbol_id(), previous.symbols.next_id().0),
            None => lib_binders
                .last()
                .map(|previous| (previous.next_symbol_id(), previous.symbols.next_id().0))
                .unwrap_or((1, 0)),
        };
        let mut binder =
            tsrs2_binder::Binder::with_bases(source_file, options, symbol_id_seed, symbol_base);
        binder.bind_source_file();
        // tsc getBindAndCheckDiagnosticsForFileNoCache (123717): plain
        // JS files (checkJs unmodeled beyond the explicit-false skip)
        // filter bind diagnostics to the plainJSErrors allowlist and
        // SKIP the comment-directive merge
        // (includeBindAndCheckDiagnostics = !isPlainJs); TS files get
        // the directive filter (@ts-ignore/@ts-expect-error). Unused
        // @ts-expect-error reporting (2578) stays a deliberate FN: it
        // requires knowing a directive suppressed NOTHING, and the
        // checker's diagnostic surface is still FN-heavy — emitting it
        // now would fabricate 2578s wherever we under-report (FP).
        if is_js_file_name(&source_file.file_name) {
            // canIncludeBindAndCheckDiagnostics: an EXPLICIT checkJs:
            // false fails isPlainJsFile (checkJs must be undefined)
            // AND isCheckJs — the file skips bind/check diagnostics
            // entirely.
            if options.check_js != Some(false) {
                diagnostics.extend(
                    binder
                        .bind_diagnostics
                        .iter()
                        .filter(|diagnostic| plain_js_errors::is_plain_js_error(diagnostic.code()))
                        .cloned(),
                );
            }
        } else {
            diagnostics.extend(filter_by_comment_directives(
                &source_file.text,
                &source_file.line_map,
                binder.bind_diagnostics.iter().cloned(),
            ));
        }
        binders.push(binder);
    }

    // Checker-state construction (M4 5.0) + the check driver (M4 5.4):
    // the initializeTypeChecker slice runs in from_program (globals
    // merge across non-module files — lib prefix first — plus the
    // cross-file duplicate reporting), then FIXTURE files check IN
    // PROGRAM ORDER (tsc getSemanticDiagnostics per file over one
    // checker; lib files are never asked for). Options diagnostics
    // (bad option combos, core-interfaces §8) would gate ahead of this
    // block — none are modeled yet, so the gate is vacuously open.
    let binder_refs: Vec<&tsrs2_binder::Binder<'_>> =
        lib_binders.iter().chain(binders.iter()).collect();
    if !binder_refs.is_empty() {
        let lib_count = lib_binders.len();
        let mut state = state::CheckerState::from_program(binder_refs, options);
        for index in lib_count..state.binder.file_count() {
            state.check_source_file(index);
        }
        let lib_names: std::collections::HashSet<&str> = lib_sources
            .iter()
            .map(|source| source.file_name.as_str())
            .collect();
        let by_name: std::collections::HashMap<&str, &tsrs2_syntax::SourceFile> = program_sources
            .iter()
            .map(|source| (source.file_name.as_str(), source))
            .collect();
        // Per-file assembly (getBindAndCheckDiagnosticsForFileNoCache
        // 123717): plain JS files filter check diagnostics to the
        // plainJSErrors allowlist and skip the directive merge; TS
        // files run the comment-directive filter; file-less
        // program-level diagnostics pass through.
        let mut checker_diagnostics_by_file: std::collections::BTreeMap<
            Option<String>,
            Vec<tsrs2_diags::Diagnostic>,
        > = std::collections::BTreeMap::new();
        for diagnostic in state.diagnostics.iter().cloned() {
            checker_diagnostics_by_file
                .entry(diagnostic.file_name.clone())
                .or_default()
                .push(diagnostic);
        }
        for (file_name, file_diagnostics) in checker_diagnostics_by_file {
            // Diagnostics ANCHORED in a lib file (the lib-side span of
            // a duplicate pair, a lazily-forced lib-internal error)
            // are filed under that lib file and never collected in
            // the oracle world — same exclusion shape as the
            // file-less arm below.
            if file_name
                .as_deref()
                .is_some_and(|name| lib_names.contains(name))
            {
                continue;
            }
            let javascript_file = file_name.as_deref().is_some_and(is_js_file_name);
            if javascript_file {
                if options.check_js != Some(false) {
                    diagnostics.extend(file_diagnostics.into_iter().filter(|diagnostic| {
                        plain_js_errors::is_plain_js_error(diagnostic.code())
                    }));
                }
                continue;
            }
            match file_name.as_deref().and_then(|name| by_name.get(name)) {
                Some(source) => diagnostics.extend(filter_by_comment_directives(
                    &source.text,
                    &source.line_map,
                    file_diagnostics.into_iter(),
                )),
                // File-less (program-level) diagnostics do not join the
                // per-file output. In tsc the only such emitters today
                // (the missing-global 2318/2317 family) fire inside
                // initializeTypeChecker BEFORE getDiagnosticsWorker's
                // previousGlobalDiagnostics snapshot, so per-file
                // getSemanticDiagnostics never surfaces them; our 5.0
                // lazy-global architecture raises them mid-check, which
                // would surface a diagnostic tsc keeps invisible. tsc's
                // genuinely-visible mid-check globals are the deferred
                // wrapper lookups, and our 5.3b port passes
                // reportErrors=false there — nothing observable is
                // dropped. Revisit when getGlobalDiagnostics grows a
                // consumer (program-level API, M8).
                None => {}
            }
        }
        // The aggregate pass is sorted + deduplicated like tsc's
        // getPreEmitDiagnostics / the oracle driver's
        // ts.sortAndDeduplicateDiagnostics; getSyntacticDiagnostics
        // stays per-file unsorted concatenation, matching tsc.
        tsrs2_diags::sort_and_dedupe_diagnostics(&mut diagnostics);
    }

    debug_assert!(tsrs2_binder::is_scaffolded());
    debug_assert!(tsrs2_types::is_scaffolded());

    CheckResult {
        diagnostics,
        syntactic_diagnostics,
    }
}

/// A parsed+bound lib-set prefix, shared across programs.
///
/// EXACTNESS (m4-lib-loading-steps.md D3): libs are the program
/// PREFIX, so for a fixed lib list every lib file's
/// NodeId/NodeArrayId/SymbolId bases are identical across programs —
/// the cached arenas ARE the arenas an uncached run would build. The
/// bundle is deliberately leaked (process-lifetime; bounded by the
/// distinct lib-set count, 39 across the conformance corpus), which
/// resolves the sources↔binders self-reference without unsafe.
/// Read-only-after-bind is structural: ProgramBinder holds shared
/// references and its symbol_mut refuses file-owned ids.
struct LibBundle {
    sources: &'static [tsrs2_syntax::SourceFile],
    binders: &'static [tsrs2_binder::Binder<'static>],
}

/// The per-lib-set bundle cache. Keyed by the ordered (name, text)
/// list plus the FULL CompilerOptions (the binder reads options; the
/// whole struct is small and Eq/Hash — narrow to the proven-read
/// subset only if the option matrix measurably multiplies entries).
/// `TSRS_LIB_BUNDLE_CACHE=0` bypasses the map (fresh build+leak per
/// call) — the L3 A/B lever proving reuse changes nothing.
fn lib_bundle(libs: &[&InputFile], options: &CompilerOptions) -> &'static LibBundle {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    use std::sync::{Mutex, OnceLock};

    type Key = (Vec<(String, u64)>, CompilerOptions);
    static CACHE: OnceLock<Mutex<HashMap<Key, &'static LibBundle>>> = OnceLock::new();

    let cache_enabled = std::env::var_os("TSRS_LIB_BUNDLE_CACHE")
        .map_or(true, |value| value != "0");
    let key: Key = (
        libs.iter()
            .map(|lib| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                lib.text.hash(&mut hasher);
                (lib.name.clone(), hasher.finish())
            })
            .collect(),
        options.clone(),
    );
    if cache_enabled {
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(bundle) = cache.lock().expect("lib bundle cache").get(&key) {
            return bundle;
        }
    }
    let bundle = build_lib_bundle(libs, options);
    if cache_enabled {
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        cache.lock().expect("lib bundle cache").insert(key, bundle);
    }
    bundle
}

fn build_lib_bundle(libs: &[&InputFile], options: &CompilerOptions) -> &'static LibBundle {
    // Binder borrows its CompilerOptions for the bundle's lifetime.
    let options: &'static CompilerOptions = Box::leak(Box::new(options.clone()));
    let mut sources: Vec<tsrs2_syntax::SourceFile> = Vec::new();
    for lib in libs {
        let (node_id_base, node_array_id_base) = match sources.last() {
            Some(previous) => (previous.arena.node_end(), previous.arena.array_end()),
            None => (0, 0),
        };
        sources.push(tsrs2_syntax::parse_source_file(
            lib.name.clone(),
            lib.text.clone(),
            tsrs2_syntax::ParseOptions {
                language_variant: tsrs2_syntax::LanguageVariant::Standard,
                javascript_file: false,
                node_id_base,
                node_array_id_base,
            },
            None,
        ));
    }
    let sources: &'static [tsrs2_syntax::SourceFile] = Box::leak(sources.into_boxed_slice());
    let mut binders: Vec<tsrs2_binder::Binder<'static>> = Vec::new();
    for source in sources {
        let (symbol_id_seed, symbol_base) = match binders.last() {
            Some(previous) => (previous.next_symbol_id(), previous.symbols.next_id().0),
            None => (1, 0),
        };
        let mut binder =
            tsrs2_binder::Binder::with_bases(source, options, symbol_id_seed, symbol_base);
        binder.bind_source_file();
        binders.push(binder);
    }
    let binders: &'static [tsrs2_binder::Binder<'static>] =
        Box::leak(binders.into_boxed_slice());
    Box::leak(Box::new(LibBundle { sources, binders }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_engine_returns_no_diagnostics() {
        let result = check_program(&[], &CompilerOptions::default());
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn js_files_report_typescript_only_syntax() {
        // Pins from tsc program.getSyntacticDiagnostics on an allowJs program.
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "function f(x: number): string { return \"\"; }\ninterface I { a: string }\nenum E { A }\nvar x!;\nimport eq = require(\"m\");\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        );
        let pins: Vec<(u32, u32, u32)> = result
            .syntactic_diagnostics
            .iter()
            .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
            .collect();
        assert_eq!(
            pins,
            [
                (8010, 14, 6),
                (8010, 23, 6),
                (8006, 55, 1),
                (8006, 76, 1),
                (8002, 92, 25),
            ]
        );
    }

    #[test]
    fn js_files_report_type_only_imports_and_export_equals() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "import type { A } from \"m\";\nimport { type B } from \"m\";\nexport type { C };\nexport = 5;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        );
        let pins: Vec<(u32, u32, u32)> = result
            .syntactic_diagnostics
            .iter()
            .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
            .collect();
        assert_eq!(
            pins,
            [(8006, 0, 27), (8006, 37, 6), (8006, 56, 18), (8003, 75, 11)]
        );
    }

    // ---- lib-loading L2: lib-backed programs (oracle-pinned) ----

    fn es5_lib() -> InputFile {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/typescript-6.0.3/lib/lib.es5.d.ts"
        );
        InputFile {
            name: "lib.es5.d.ts".to_owned(),
            text: std::fs::read_to_string(path).expect("vendored lib.es5.d.ts"),
        }
    }

    fn lib_backed_diags(text: &str) -> Vec<(u32, u32, u32, String)> {
        let result = check_program_with_libs(
            &[es5_lib()],
            &[InputFile {
                name: "a.ts".to_owned(),
                text: text.to_owned(),
            }],
            &CompilerOptions::default(),
        );
        result
            .diagnostics
            .iter()
            .map(|d| {
                (
                    d.code(),
                    d.start.unwrap_or(u32::MAX),
                    d.length.unwrap_or(u32::MAX),
                    d.message_text().to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn lib_names_resolve_through_the_loaded_lib() {
        assert_eq!(lib_backed_diags("interface I<T extends Date> { x: T }
"), []);
    }

    #[test]
    fn restricted_lib_set_reports_2583_with_the_lib_argument() {
        // Map is not in es5: the failure is GENUINE under this lib set
        // (the lib_globals gate stands down for lib-loaded programs)
        // and the suggested-lib arm supplies tsc's exact argument.
        let diags = lib_backed_diags("interface I<T extends Map> { x: T }
");
        assert_eq!(
            diags,
            [(
                2583,
                22,
                3,
                "Cannot find name 'Map'. Do you need to change your target library? Try changing the 'lib' compiler option to 'es2015' or later."
                    .to_owned()
            )]
        );
    }

    #[test]
    fn lib_array_members_drive_variance_measurement() {
        // Mutable method parameters are bivariant, so es5 Array
        // measures covariant and `out` holds (oracle-pinned clean)...
        assert_eq!(lib_backed_diags("interface Wrap<out T> { xs: T[] }
"), []);
        // ...including when a fixture declaration MERGES into the lib
        // interface (both member sets resolve; oracle-pinned clean).
        assert_eq!(
            lib_backed_diags(
                "interface Array<T> { fixtureExtra: T }
interface Wrap<out T> { xs: T[] }
"
            ),
            []
        );
        assert_eq!(
            lib_backed_diags(
                "interface Array<T> { sink: (x: T) => void }
interface Wrap<out T> { xs: T[] }
"
            ),
            []
        );
        assert_eq!(
            lib_backed_diags("interface Wrap<out T> { xs: ReadonlyArray<T> }
"),
            []
        );
    }

    #[test]
    fn lib_types_render_in_constraint_failure_args() {
        // Named object types print their symbol name in the 2344 args
        // (type_to_string_slice's named-object arm; oracle-pinned).
        let diags =
            lib_backed_diags("interface Foo<T extends number> { x: T }\ntype X = Foo<Date>;\n");
        assert_eq!(
            diags,
            [(
                2344,
                54,
                4,
                "Type 'Date' does not satisfy the constraint 'number'.".to_owned()
            )]
        );
    }

    #[test]
    fn lib_array_in_parameter_position_reports_2636() {
        let diags = lib_backed_diags("interface Wrap<out T> { f: (xs: T[]) => void }
");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2636, 15, 5));
        assert!(
            diags[0].3.starts_with(
                "Type 'Wrap<sub-T>' is not assignable to type 'Wrap<super-T>'"
            ),
            "{}",
            diags[0].3
        );
    }

    #[test]
    fn check_program_includes_parse_diagnostics() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "\"unterminated".to_owned(),
            }],
            &CompilerOptions::default(),
        );

        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code(), 1002);
    }

    /// Promise<T> is declared in BOTH es2015.promise and
    /// es2015.symbol.wellknown; the merged symbol must expose ONE T
    /// (getSymbolOfDeclaration's getMergedSymbol chase inside
    /// appendTypeParameters) — without the chase the declared type
    /// read `Promise<T, T>` and every `Promise<X>` reference tripped
    /// a spurious 2314 (lib-loading L2 find: the async-fixture FPs).
    #[test]
    fn merged_lib_interface_type_parameters_unify() {
        let vendor = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/typescript-6.0.3/lib/"
        );
        let lib = |name: &str| InputFile {
            name: name.to_owned(),
            text: std::fs::read_to_string(format!("{vendor}{name}")).expect("vendored lib"),
        };
        let result = check_program_with_libs(
            &[
                lib("lib.es5.d.ts"),
                lib("lib.es2015.promise.d.ts"),
                lib("lib.es2015.symbol.wellknown.d.ts"),
            ],
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "type X = Promise<number>;\n".to_owned(),
            }],
            &CompilerOptions::default(),
        );
        assert_eq!(result.diagnostics, []);
    }
}
