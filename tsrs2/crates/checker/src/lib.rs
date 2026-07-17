#![forbid(unsafe_code)]

pub mod access;
pub mod annotate;
pub mod calls;
pub mod check;
pub mod class;
pub mod constraints;
pub mod contextual;
pub mod engine;
pub mod evaluate;
pub mod expr;
pub mod facts;
pub mod functions;
pub mod globals;
pub mod indexed;
pub mod instantiate;
pub mod intersect;
pub mod iterate;
mod js_grammar;
pub mod jsx;
pub mod links;
pub mod literals;
pub mod merge;
pub mod modules;
pub mod operators;
mod plain_js_errors;
pub mod program;
pub mod relate;
pub mod relpin;
pub mod resolve;
pub mod spell;
pub mod state;
pub mod statements;
pub mod structural;
pub mod unions;
pub mod variance;
pub mod widen;

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

/// tsc check directive: extractPragmas walks
/// getLeadingCommentRanges(text, 0) — single-line comments BEFORE the
/// first token — and the LAST ts-check/ts-nocheck pragma wins
/// (processPragmasIntoFields); skipTypeChecking then drops the file's
/// bind+check diagnostics whole (parse diagnostics stay). Pragma names
/// lowercase; the name must end at whitespace/colon/EOL like
/// `@([^\s:]+)`. This producer stays TEXTUAL (exact over leading
/// trivia, which is all extractPragmas reads); the 5.8e directive
/// completion moved @ts-ignore/@ts-expect-error to scanner-collected
/// SourceFile.comment_directives — swap this too if the parser ever
/// grows real pragma processing (M8 surface).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CheckDirective {
    Check,
    NoCheck,
}

fn check_directive(text: &str) -> Option<CheckDirective> {
    let mut rest = text;
    // getLeadingCommentRanges starts after a leading shebang. Keep
    // this test on the RAW offset zero: a BOM before `#!` makes it an
    // ordinary token sequence, not shebang trivia.
    if let Some(after) = rest.strip_prefix("#!") {
        let line_end = after
            .find(['\n', '\r', '\u{2028}', '\u{2029}'])
            .unwrap_or(after.len());
        rest = &after[line_end..];
    }
    let mut directive = None;
    loop {
        // JS WhiteSpace includes BOM; Rust's is_whitespace does not.
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
        if let Some(after) = rest.strip_prefix("//") {
            let line_end = after
                .find(['\n', '\r', '\u{2028}', '\u{2029}'])
                .unwrap_or(after.len());
            let comment = &after[..line_end];
            // singleLinePragmaRegEx: ^///?\s*@([^\s:]+)
            let body = comment.strip_prefix('/').unwrap_or(comment).trim_start();
            if let Some(name_and_tail) = body.strip_prefix('@') {
                let name_end = name_and_tail
                    .find(|c: char| c.is_whitespace() || c == ':')
                    .unwrap_or(name_and_tail.len());
                match name_and_tail[..name_end].to_ascii_lowercase().as_str() {
                    "ts-nocheck" => directive = Some(CheckDirective::NoCheck),
                    "ts-check" => directive = Some(CheckDirective::Check),
                    _ => {}
                }
            }
            rest = &after[line_end..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("/*") {
            match after.find("*/") {
                Some(end) => {
                    rest = &after[end + 2..];
                    continue;
                }
                None => break,
            }
        }
        break;
    }
    directive
}

fn can_include_bind_and_check_diagnostics(
    javascript_file: bool,
    directive: Option<CheckDirective>,
    options: &CompilerOptions,
) -> bool {
    match directive {
        Some(CheckDirective::NoCheck) => false,
        // A per-file @ts-check overrides an explicit checkJs:false.
        Some(CheckDirective::Check) => true,
        None => !javascript_file || options.check_js != Some(false),
    }
}

/// tsc isPlainJsFile (12876): a JS/JSX file is "plain" only when
/// neither a per-file check directive nor the project-level checkJs
/// option was supplied. Checked JS uses the same comment-directive
/// merge as TypeScript files. Upstream also uses its full diagnostic
/// stream; this checker retains the JS-safe allowlist until JSDoc type
/// modeling lands.
fn is_plain_js_file(
    javascript_file: bool,
    directive: Option<CheckDirective>,
    options: &CompilerOptions,
) -> bool {
    javascript_file && directive.is_none() && options.check_js.is_none()
}

/// tsc-port: markPrecedingCommentDirectiveLine @6.0.3
/// tsc-hash: 5fd3ed53a22559eabfbc34ecee39efa38b2df133d5cc00e86dcd42ecae6ea88b
/// tsc-span: _tsc.js:123766-123784
///
/// getDiagnosticsWithPrecedingDirectives (123756) over one file's
/// bind+check list: keep a diagnostic only when no comment directive
/// precedes it. Directives come from the SCANNER
/// (SourceFile.comment_directives) and key on the line of range.end —
/// the line holding a single-line comment, or the line holding a
/// multi-line comment's `*/` (createCommentDirectivesMap 12963; a
/// second directive ending on the same line collapses into it). The
/// walk starts one line above the diagnostic and stops at the first
/// line that is non-empty and not a `//` comment after a JS trim —
/// unlike the retired interim filter, block-comment shell lines STOP
/// the walk, exactly as in tsc.
///
fn filter_by_comment_directives_and_mark_used(
    source: &tsrs2_syntax::SourceFile,
    diagnostics: impl Iterator<Item = tsrs2_diags::Diagnostic>,
    mut used_directive_lines: Option<&mut std::collections::HashSet<usize>>,
) -> Vec<tsrs2_diags::Diagnostic> {
    // getMergedBindAndCheckDiagnostics (123744): no directives, no
    // filtering.
    if source.comment_directives.is_empty() {
        return diagnostics.collect();
    }
    let text = source.text.as_str();
    // LineMap.line_starts are UTF-16 offsets; build BYTE line starts
    // with the same break set (\r\n, \r, \n, U+2028, U+2029) for text
    // slicing and for placing the byte-offset directive ranges.
    let byte_line_starts = compute_byte_line_starts(text);
    let line_of_byte = |offset: usize| -> usize {
        match byte_line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(insert) => insert.saturating_sub(1),
        }
    };
    let directive_lines: std::collections::HashSet<usize> = source
        .comment_directives
        .iter()
        .map(|directive| line_of_byte(directive.end as usize))
        .collect();
    let line_text = |line: usize| -> &str {
        let start = byte_line_starts[line];
        let end = byte_line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(text.len());
        &text[start..end]
    };
    // Diagnostic.start is UTF-16, matching line_starts' units.
    let utf16_line_starts: &[u32] = &source.line_map.line_starts;
    let mut result = Vec::new();
    'diagnostic: for diagnostic in diagnostics {
        let Some(start) = diagnostic.start else {
            result.push(diagnostic);
            continue;
        };
        let diagnostic_line = match utf16_line_starts.binary_search(&start) {
            Ok(line) => line,
            Err(insert) => insert.saturating_sub(1),
        };
        let mut line = diagnostic_line;
        while line > 0 {
            line -= 1;
            if directive_lines.contains(&line) {
                if let Some(used) = used_directive_lines.as_deref_mut() {
                    used.insert(line);
                }
                continue 'diagnostic;
            }
            // `lineText !== "" && !/^\s*\/\/.*$/.test(lineText)`
            // over the trimmed slice: String.trim's set is JS
            // whitespace, and post-trim the regex reduces to a
            // `//` prefix test.
            let trimmed = line_text(line).trim_matches(tsrs2_syntax::is_js_whitespace);
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                break;
            }
        }
        result.push(diagnostic);
    }
    result
}

fn unused_expect_error_diagnostics(
    source: &tsrs2_syntax::SourceFile,
    used_directive_lines: &std::collections::HashSet<usize>,
) -> Vec<tsrs2_diags::Diagnostic> {
    use tsrs2_syntax::CommentDirectiveKind;

    if source.comment_directives.is_empty() {
        return Vec::new();
    }
    let byte_line_starts = compute_byte_line_starts(&source.text);
    let line_of_byte = |offset: usize| -> usize {
        match byte_line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(insert) => insert.saturating_sub(1),
        }
    };
    // createCommentDirectivesMap uses Map construction, so the last
    // directive ending on a line replaces earlier directives there.
    let mut directives_by_line = std::collections::BTreeMap::new();
    for directive in &source.comment_directives {
        directives_by_line.insert(line_of_byte(directive.end as usize), *directive);
    }
    directives_by_line
        .into_iter()
        .filter_map(|(line, directive)| {
            if directive.kind != CommentDirectiveKind::ExpectError
                || used_directive_lines.contains(&line)
            {
                return None;
            }
            let start = source
                .line_map
                .byte_to_utf16
                .get(directive.pos as usize)
                .copied()
                .unwrap_or(directive.pos);
            let end = source
                .line_map
                .byte_to_utf16
                .get(directive.end as usize)
                .copied()
                .unwrap_or(directive.end);
            Some(tsrs2_diags::Diagnostic::new(
                Some(source.file_name.clone()),
                Some(start),
                Some(end.saturating_sub(start)),
                tsrs2_diags::MessageChain::new(
                    &tsrs2_diags::gen::Unused_ts_expect_error_directive,
                    &[],
                ),
            ))
        })
        .collect()
}

/// tsc-port: filterSemanticDiagnostics @6.0.3
/// tsc-hash: 5585b227fa5ab80bc9c14222bfcb199f66a2d8fb5d2fa640667c188b5152fa22
/// tsc-span: _tsc.js:125664-125666
///
/// tsc filters each file's getSemanticDiagnostics output with
/// `!d.skippedOn || !option[d.skippedOn]` (getSemanticDiagnosticsForFile
/// 123698). The only key any emitter passes is "noEmit" (the checker
/// collision band 83235-83353 + the __esModule marker 90103), no
/// parse/bind emitter sets it, and the predicate is per-diagnostic —
/// so one pass over the aggregate list is equivalent to tsc's
/// per-file filter. Runs beside filter_by_comment_directives at the
/// program-layer diagnostics-finalize seam (m4-58 §0 skippedOn).
fn filter_semantic_diagnostics(
    diagnostics: &mut tsrs2_diags::DiagnosticList,
    options: &CompilerOptions,
) {
    if options.no_emit == Some(true) {
        diagnostics.retain(|diagnostic| !diagnostic.skipped_on_no_emit);
    }
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
    let (lib_sources, lib_binders): (&[tsrs2_syntax::SourceFile], &[tsrs2_binder::Binder<'_>]) =
        match bundle {
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
    // Parse the per-file check directive (ts-check/ts-nocheck pragma)
    // ONCE; @ts-ignore/@ts-expect-error ride on each SourceFile's
    // scanner-collected comment_directives.
    let check_directives: std::collections::HashMap<&str, Option<CheckDirective>> = program_sources
        .iter()
        .map(|source| (source.file_name.as_str(), check_directive(&source.text)))
        .collect();
    let mut used_directive_lines: std::collections::HashMap<
        String,
        std::collections::HashSet<usize>,
    > = program_sources
        .iter()
        .map(|source| (source.file_name.clone(), std::collections::HashSet::new()))
        .collect();
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
        // JS files filter bind diagnostics to the plainJSErrors
        // allowlist and SKIP the comment-directive merge
        // (includeBindAndCheckDiagnostics = !isPlainJs); TypeScript
        // and checked JS get the directive filter.
        let javascript_file = is_js_file_name(&source_file.file_name);
        let directive = check_directives
            .get(source_file.file_name.as_str())
            .copied()
            .flatten();
        let include_bind_and_check = !(options.skip_lib_check == Some(true)
            && source_file.is_declaration_file)
            && can_include_bind_and_check_diagnostics(javascript_file, directive, options);
        if include_bind_and_check {
            if javascript_file {
                // The checker does not model JSDoc types yet, so even
                // checked JS must stay on the proven-safe diagnostic
                // allowlist. Unlike plain JS it still participates in
                // @ts-ignore/@ts-expect-error filtering.
                let allowed = binder
                    .bind_diagnostics
                    .iter()
                    .filter(|diagnostic| plain_js_errors::is_plain_js_error(diagnostic.code()))
                    .cloned();
                if is_plain_js_file(true, directive, options) {
                    diagnostics.extend(allowed);
                } else {
                    let used = used_directive_lines
                        .get_mut(source_file.file_name.as_str())
                        .expect("all parsed sources have a directive-use set");
                    diagnostics.extend(filter_by_comment_directives_and_mark_used(
                        source_file,
                        allowed,
                        Some(used),
                    ));
                }
            } else {
                let used = used_directive_lines
                    .get_mut(source_file.file_name.as_str())
                    .expect("all parsed sources have a directive-use set");
                diagnostics.extend(filter_by_comment_directives_and_mark_used(
                    source_file,
                    binder.bind_diagnostics.iter().cloned(),
                    Some(used),
                ));
            }
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
        // The resolver's host view (M4 5.8d): every INPUT path, incl.
        // files the program dropped (.json bodies, .js without
        // allowJs) — the suppression probes need them to keep 2307
        // FP-free.
        state.host_file_paths = files
            .iter()
            .map(|file| state::CheckerState::normalize_program_path(&file.name, ""))
            .collect();
        state.host_package_json_module_types = files
            .iter()
            .filter(|file| {
                file.name
                    .rsplit(['/', '\\'])
                    .next()
                    .is_some_and(|name| name == "package.json")
            })
            .map(|file| {
                let is_module = serde_json::from_str::<serde_json::Value>(&file.text)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("type")
                            .and_then(serde_json::Value::as_str)
                            .map(|value| value == "module")
                    })
                    .unwrap_or(false);
                (
                    state::CheckerState::normalize_program_path(&file.name, ""),
                    is_module,
                )
            })
            .collect();
        // initializeTypeChecker's augmentation passes (88769/88874)
        // run here — AFTER the resolver's host view exists (pass 2
        // resolves module names), BEFORE any file checks.
        state.merge_module_augmentations();
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
        // plainJSErrors allowlist and skip the directive merge;
        // TypeScript and checked JS run the comment-directive filter;
        // file-less program-level diagnostics pass through.
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
            // skipLibCheck suppresses the complete bind/check stream
            // for declaration files, including initialization-time
            // cross-file merge diagnostics that were produced before
            // check_source_file had a chance to skip the file.
            if options.skip_lib_check == Some(true)
                && file_name
                    .as_deref()
                    .and_then(|name| by_name.get(name))
                    .is_some_and(|source| source.is_declaration_file)
            {
                continue;
            }
            let javascript_file = file_name.as_deref().is_some_and(is_js_file_name);
            if javascript_file {
                let Some(source) = file_name.as_deref().and_then(|name| by_name.get(name)) else {
                    continue;
                };
                let directive = check_directives
                    .get(source.file_name.as_str())
                    .copied()
                    .flatten();
                if can_include_bind_and_check_diagnostics(true, directive, options) {
                    let allowed = file_diagnostics
                        .into_iter()
                        .filter(|diagnostic| plain_js_errors::is_plain_js_error(diagnostic.code()));
                    if is_plain_js_file(true, directive, options) {
                        diagnostics.extend(allowed);
                    } else {
                        let used = used_directive_lines
                            .get_mut(source.file_name.as_str())
                            .expect("all parsed sources have a directive-use set");
                        diagnostics.extend(filter_by_comment_directives_and_mark_used(
                            source,
                            allowed,
                            Some(used),
                        ));
                    }
                }
                continue;
            }
            if file_name.is_none() {
                continue;
            }
            if let Some(source) = file_name.as_deref().and_then(|name| by_name.get(name)) {
                let directive = check_directives
                    .get(source.file_name.as_str())
                    .copied()
                    .flatten();
                if !can_include_bind_and_check_diagnostics(false, directive, options) {
                    continue;
                }
                let used = used_directive_lines
                    .get_mut(source.file_name.as_str())
                    .expect("all parsed sources have a directive-use set");
                diagnostics.extend(filter_by_comment_directives_and_mark_used(
                    source,
                    file_diagnostics.into_iter(),
                    Some(used),
                ));
            }
        }
        diagnostics.extend(state.visible_global_diagnostics.iter().cloned());
        // getMergedBindAndCheckDiagnostics' non-partial tail: after the
        // complete bind+check stream has marked directives used, emit
        // 2578 for every remaining @ts-expect-error (never @ts-ignore).
        for (source_index, source) in program_sources.iter().enumerate() {
            let javascript_file = is_js_file_name(&source.file_name);
            let directive = check_directives
                .get(source.file_name.as_str())
                .copied()
                .flatten();
            if options.skip_lib_check == Some(true) && source.is_declaration_file
                || !can_include_bind_and_check_diagnostics(javascript_file, directive, options)
                || is_plain_js_file(javascript_file, directive, options)
                || state
                    .partially_checked_files
                    .contains(&(lib_count + source_index))
            {
                continue;
            }
            let used = used_directive_lines
                .get(source.file_name.as_str())
                .expect("all parsed sources have a directive-use set");
            diagnostics.extend(unused_expect_error_diagnostics(source, used));
        }
        // The aggregate pass is sorted + deduplicated like tsc's
        // getPreEmitDiagnostics / the oracle driver's
        // ts.sortAndDeduplicateDiagnostics; getSyntacticDiagnostics
        // stays per-file unsorted concatenation, matching tsc.
        filter_semantic_diagnostics(&mut diagnostics, options);
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
/// list plus the projection of CompilerOptions onto the binder's three
/// option observables — the only fields a cached bundle can ever
/// expose. The binder crate reads exactly `emit_script_target()`
/// (declare.rs language_version, bind.rs ES2015 gate),
/// `always_strict_effective()` (bind.rs use-strict prologue) and
/// `no_fallthrough_cases_in_switch == Some(true)` (bindCaseBlock), and
/// `Binder.options` is read nowhere outside the binder crate. Keying
/// the full struct rebuilt+leaked one identical bundle per matrix
/// option combination (~11.5 GB peak over the conformance corpus);
/// the projection restores the per-lib-set bound. A new `options.`
/// read in the binder MUST extend this projection.
/// `TSRS_LIB_BUNDLE_CACHE=0` bypasses the map (fresh build+leak per
/// call) — the L3 A/B lever proving reuse changes nothing.
fn lib_bundle(libs: &[&InputFile], options: &CompilerOptions) -> &'static LibBundle {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    type Key = (Vec<(String, u64)>, CompilerOptions);
    static CACHE: OnceLock<Mutex<HashMap<Key, &'static LibBundle>>> = OnceLock::new();

    // Each field holds the observable's canonical preimage, so the
    // projected struct evaluates every binder read identically to the
    // program's own options (ES3/absent targets share the computed
    // ES2025, options.rs:139) while bind-inert fields collapse to one
    // key. The bundle is BUILT from the projection too: whichever
    // program builds first, the leaked options are the same struct.
    let bundle_options = CompilerOptions {
        target: Some(options.emit_script_target().bits()),
        always_strict: Some(options.always_strict_effective()),
        no_fallthrough_cases_in_switch: Some(options.no_fallthrough_cases_in_switch == Some(true)),
        ..CompilerOptions::default()
    };

    let cache_enabled = std::env::var_os("TSRS_LIB_BUNDLE_CACHE").is_none_or(|value| value != "0");
    let key: Key = (
        libs.iter()
            .map(|lib| (lib.name.clone(), lib_text_fingerprint(&lib.text)))
            .collect(),
        bundle_options.clone(),
    );
    if cache_enabled {
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(bundle) = cache.lock().expect("lib bundle cache").get(&key) {
            return bundle;
        }
    }
    let bundle = build_lib_bundle(libs, &bundle_options);
    if cache_enabled {
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        cache.lock().expect("lib bundle cache").insert(key, bundle);
    }
    bundle
}

/// Content fingerprint for the bundle cache key. The key's u64 has
/// always stood in for text identity (64-bit collision accepted); a
/// word-folding FNV variant keeps full-text coverage at a fraction of
/// the SipHash cost, which dominated per-case conformance time.
fn lib_text_fingerprint(text: &str) -> u64 {
    let bytes = text.as_bytes();
    let mut hash = 0xcbf29ce484222325u64;
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        let word = u64::from_le_bytes(chunk.try_into().expect("8-byte chunk"));
        hash = (hash ^ word).wrapping_mul(0x100000001b3).rotate_left(23);
    }
    let mut tail = [0u8; 8];
    tail[..chunks.remainder().len()].copy_from_slice(chunks.remainder());
    hash = (hash ^ u64::from_le_bytes(tail)).wrapping_mul(0x100000001b3);
    hash ^ bytes.len() as u64
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
    let binders: &'static [tsrs2_binder::Binder<'static>] = Box::leak(binders.into_boxed_slice());
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
    fn lib_bundle_key_projects_to_bind_observables() {
        use tsrs2_types::flags::ScriptTarget;
        // A lib name unique to this test: the cache is process-global.
        let lib = InputFile {
            name: "lib.bundle-key-probe.d.ts".to_owned(),
            text: "declare const bundleKeyProbe: number;\n".to_owned(),
        };
        let libs = [&lib];
        let base = CompilerOptions::default();
        let shared = lib_bundle(&libs, &base);

        // Bind-inert options reuse the bundle: the checker consumes
        // them per program, never through the cached prefix.
        let inert = CompilerOptions {
            strict_null_checks: Some(false),
            jsx: Some(2),
            no_emit: Some(true),
            module_resolution: Some(1),
            ..base.clone()
        };
        assert!(std::ptr::eq(shared, lib_bundle(&libs, &inert)));

        // ES3 and an absent target compute the same ES2025
        // languageVersion (options.rs:139) — one bundle.
        let es3 = CompilerOptions {
            target: Some(ScriptTarget::ES3.bits()),
            ..base.clone()
        };
        assert!(std::ptr::eq(shared, lib_bundle(&libs, &es3)));

        // Each bind-time observable splits the key.
        let es5 = CompilerOptions {
            target: Some(ScriptTarget::ES5.bits()),
            ..base.clone()
        };
        assert!(!std::ptr::eq(shared, lib_bundle(&libs, &es5)));
        let loose = CompilerOptions {
            always_strict: Some(false),
            ..base.clone()
        };
        assert!(!std::ptr::eq(shared, lib_bundle(&libs, &loose)));
        let fallthrough = CompilerOptions {
            no_fallthrough_cases_in_switch: Some(true),
            ..base.clone()
        };
        assert!(!std::ptr::eq(shared, lib_bundle(&libs, &fallthrough)));
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

    fn codes_of(source: &str) -> Vec<u32> {
        codes_of_with_options(source, &CompilerOptions::default())
    }

    fn codes_of_with_options(source: &str, options: &CompilerOptions) -> Vec<u32> {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: source.to_owned(),
            }],
            options,
        );
        result.diagnostics.iter().map(|d| d.code()).collect()
    }

    fn strict_options() -> CompilerOptions {
        CompilerOptions {
            strict: Some(true),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        }
    }

    #[test]
    fn typeof_import_follows_value_alias_reexports() {
        let result = check_program(
            &[
                InputFile {
                    name: "a.ts".to_owned(),
                    text: "export const x = 1;\n".to_owned(),
                },
                InputFile {
                    name: "b.ts".to_owned(),
                    text: "export { x } from \"./a\";\n".to_owned(),
                },
                InputFile {
                    name: "main.ts".to_owned(),
                    text: "type T = typeof import(\"./b\").x;\nlet y: T = \"bad\";\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        );
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2322]
        );
    }

    #[test]
    fn bare_import_defer_does_not_run_import_meta_module_checks() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "const x = import.defer;\n".to_owned(),
            }],
            &CompilerOptions {
                module: Some(1),
                ..CompilerOptions::default()
            },
        );
        assert!(!result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1343));
        assert_eq!(
            result
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [1005]
        );
    }

    #[test]
    fn node16_plain_ts_uses_package_scope_for_import_meta() {
        let options = CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        };
        let commonjs = check_program(
            &[InputFile {
                name: "src/main.ts".to_owned(),
                text: "const x = import.meta;\n".to_owned(),
            }],
            &options,
        );
        assert!(commonjs
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1470));

        let esm = check_program(
            &[
                InputFile {
                    name: "package.json".to_owned(),
                    text: "{\"type\":\"module\"}\n".to_owned(),
                },
                InputFile {
                    name: "src/main.ts".to_owned(),
                    text: "const x = import.meta;\n".to_owned(),
                },
            ],
            &options,
        );
        assert!(!esm
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1470));
    }

    #[test]
    fn missing_import_meta_global_is_public_semantic_diagnostic() {
        assert_eq!(
            codes_of_with_options(
                "const x = import.meta;\n",
                &CompilerOptions {
                    module: Some(99),
                    ..CompilerOptions::default()
                },
            ),
            [2318]
        );
    }

    #[test]
    fn node16_esm_import_of_commonjs_has_synthetic_default_even_when_option_is_false() {
        let result = check_program(
            &[
                InputFile {
                    name: "dep.cts".to_owned(),
                    text: "declare const value: { x: number };\nexport = value;\n".to_owned(),
                },
                InputFile {
                    name: "main.mts".to_owned(),
                    text: "import value from \"./dep.cjs\";\nvalue.x;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                allow_synthetic_default_imports: Some(false),
                es_module_interop: Some(false),
                ..CompilerOptions::default()
            },
        );
        let codes: Vec<u32> = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();
        assert!(
            !codes.contains(&1259) && !codes.contains(&1192),
            "native ESM-to-CJS default interop should be accepted: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn node16_json_declaration_rejects_named_esm_imports() {
        let result = check_program(
            &[
                InputFile {
                    name: "data.d.json.ts".to_owned(),
                    text: "export const x: number;\n".to_owned(),
                },
                InputFile {
                    name: "main.mts".to_owned(),
                    text: "import data, { x } from \"./data.d.json.ts\";\ndata.x;\nx;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                allow_importing_ts_extensions: Some(true),
                ..CompilerOptions::default()
            },
        );
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code() == 1544));
    }

    #[test]
    fn node20_commonjs_default_import_uses_module_exports_export() {
        let result = check_program(
            &[
                InputFile {
                    name: "dep.mts".to_owned(),
                    text: "const value = { a: 1 };\nexport { value as \"module.exports\" };\n"
                        .to_owned(),
                },
                InputFile {
                    name: "main.cts".to_owned(),
                    text: "import value from \"./dep.mjs\";\nvalue.a;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                module: Some(102),
                module_resolution: Some(3),
                ..CompilerOptions::default()
            },
        );
        assert!(
            result.diagnostics.is_empty(),
            "Node20 module.exports interop should resolve the default: {:#?}",
            result.diagnostics
        );
    }

    fn js_pair_diagnostics(js: &str, ts: &str) -> Vec<(u32, Option<String>)> {
        check_program(
            &[
                InputFile {
                    name: "a.js".to_owned(),
                    text: js.to_owned(),
                },
                InputFile {
                    name: "b.ts".to_owned(),
                    text: ts.to_owned(),
                },
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..strict_options()
            },
        )
        .diagnostics
        .into_iter()
        .map(|diagnostic| (diagnostic.code(), diagnostic.file_name))
        .collect()
    }

    #[test]
    fn unrelated_destructuring_sibling_guard_keeps_property_miss() {
        assert_eq!(
            codes_of_with_options(
                "function f({a,b}:{a:boolean,b:number}){if(a){b.missing;}}",
                &strict_options(),
            ),
            [2339]
        );
    }

    #[test]
    fn concrete_destructuring_equality_guard_keeps_property_miss() {
        assert_eq!(
            codes_of_with_options(
                "function f({a,b}:{a:boolean,b:number}){if(a===true){b.missing;}}",
                &strict_options(),
            ),
            [2339]
        );
    }

    #[test]
    fn discriminated_destructuring_sibling_still_narrows() {
        assert_eq!(
            codes_of_with_options(
                "type A={kind:'A',payload:{a:number}}|{kind:'B',payload:{b:number}};\
                 function f({kind,payload}:A){if(kind==='A'){payload.a;}}",
                &strict_options(),
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn empty_array_redeclaration_still_reports_incompatible_type() {
        assert_eq!(
            codes_of_with_options("var x = [];\nvar x = 1;\n", &strict_options()),
            [2403]
        );
    }

    #[test]
    fn shadowed_array_function_does_not_trigger_evolving_array_containment() {
        assert_eq!(
            codes_of_with_options(
                "function f(){function Array():number{return 1};var x=[];var x=Array();}",
                &strict_options(),
            ),
            [2403]
        );
    }

    #[test]
    fn array_returning_call_keeps_evolving_array_containment() {
        assert_eq!(
            codes_of_with_options(
                "declare function makeArray():number[];var x=[];var x=makeArray();",
                &strict_options(),
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn js_declared_container_property_miss_in_ts_file_reports() {
        assert_eq!(
            js_pair_diagnostics("class C {}", "const c = new C(); c.missing;"),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_assignment_declared_class_member_stays_available() {
        assert!(js_pair_diagnostics("class C {}\nC.extra = 1;", "C.extra;").is_empty());
    }

    #[test]
    fn shadowed_js_class_assignment_does_not_open_outer_class() {
        assert_eq!(
            js_pair_diagnostics(
                "class C {}\nfunction f(){class C {}\nC.extra = 1;}",
                "C.extra;",
            ),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_assignment_declared_function_member_stays_available() {
        assert!(js_pair_diagnostics("function F() {}\nF.extra = 1;", "F.extra;").is_empty());
    }

    #[test]
    fn js_assignment_declared_prototype_member_stays_available() {
        assert!(
            js_pair_diagnostics("class C {}\nC.prototype.extra = 1;", "new C().extra;").is_empty()
        );
    }

    #[test]
    fn js_static_assignment_does_not_open_instance_side() {
        assert_eq!(
            js_pair_diagnostics("class C {}\nC.extra = 1;", "new C().extra;"),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_prototype_assignment_does_not_open_static_side() {
        assert_eq!(
            js_pair_diagnostics("class C {}\nC.prototype.extra = 1;", "C.extra;"),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_static_this_assignment_does_not_open_instance_side() {
        assert_eq!(
            js_pair_diagnostics("class C { static { this.extra = 1; } }", "new C().extra;",),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_instance_this_assignment_does_not_open_static_side() {
        assert_eq!(
            js_pair_diagnostics("class C { constructor() { this.extra = 1; } }", "C.extra;",),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn js_static_this_assignment_stays_available_on_static_side() {
        assert!(
            js_pair_diagnostics("class C { static { this.extra = 1; } }", "C.extra;",).is_empty()
        );
    }

    #[test]
    fn js_instance_this_assignment_stays_available_on_instance_side() {
        assert!(js_pair_diagnostics(
            "class C { constructor() { this.extra = 1; } }",
            "new C().extra;",
        )
        .is_empty());
    }

    #[test]
    fn nested_non_arrow_function_this_does_not_open_class_instance() {
        let diagnostics = js_pair_diagnostics(
            "class C { method() { function nested() { this.extra = 1; } nested(); } }",
            "new C().extra;",
        );
        assert!(
            diagnostics.contains(&(2339, Some("b.ts".to_owned()))),
            "a nested function owns its `this`: {diagnostics:?}"
        );
    }

    #[test]
    fn nested_js_assignment_does_not_open_direct_static_member() {
        assert_eq!(
            js_pair_diagnostics(
                "class C {}\nC.bucket = {};\nC.bucket.extra = 1;",
                "C.extra;",
            ),
            [(2339, Some("b.ts".to_owned()))]
        );
    }

    #[test]
    fn nested_js_assignment_still_opens_its_actual_receiver() {
        assert!(js_pair_diagnostics(
            "class C {}\nC.bucket = {};\nC.bucket.extra = 1;",
            "C.bucket.extra;",
        )
        .is_empty());
    }

    #[test]
    fn unresolved_module_augmentation_keeps_unrelated_property_miss() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "augmentation.ts".to_owned(),
                    text: "export {};\ndeclare module \"pkg\" { interface X { missing(): void } }\n(\"x\").missing;\n"
                        .to_owned(),
                },
                // The resolver deliberately treats a bare-specifier
                // miss as undecidable when a package.json is present.
                InputFile {
                    name: "package.json".to_owned(),
                    text: "{}".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2339]
        );
    }

    #[test]
    fn unresolved_module_augmentation_does_not_open_same_named_local_type() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/index.d.ts".to_owned(),
                    text: "export interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.ts".to_owned(),
                    text: "export {};\ndeclare module \"pkg\" { interface X { missing(): void } }\ninterface X {}\ndeclare const local: X;\nlocal.missing;\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2339]
        );
    }

    #[test]
    fn unresolved_bare_augmentation_does_not_claim_same_spelled_workspace_file() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/other/index.d.ts".to_owned(),
                    text: "export {};\n".to_owned(),
                },
                InputFile {
                    name: "pkg.ts".to_owned(),
                    text: "interface X {}\ndeclare const local: X;\nlocal.missing;\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.ts".to_owned(),
                    text:
                        "export {};\ndeclare module \"pkg\" { interface X { missing(): void } }\n"
                            .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2339]
        );
    }

    #[test]
    fn unresolved_module_augmentation_contains_index_signature_property() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/index.d.ts".to_owned(),
                    text: "export as namespace Pkg;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.d.ts".to_owned(),
                    text: "import * as Pkg from \"pkg\";\ndeclare module \"pkg\" { interface X { [key: string]: unknown } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "use.ts".to_owned(),
                    text: "declare const value: Pkg.X;\nvalue.anything;\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn unresolved_module_augmentation_contains_computed_property() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/index.d.ts".to_owned(),
                    text: "export as namespace Pkg;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.d.ts".to_owned(),
                    text: "import * as Pkg from \"pkg\";\ndeclare const member: \"extra\";\ndeclare module \"pkg\" { interface X { [member](): void } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "use.ts".to_owned(),
                    text: "declare const value: Pkg.X;\nvalue.extra();\n".to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn unresolved_module_augmentation_matches_export_equals_namespace_target() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/index.d.ts".to_owned(),
                    text: "export as namespace Pkg;\nexport = Package;\ndeclare namespace Package { class X {} }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "augmentation.d.ts".to_owned(),
                    text: "import * as Pkg from \"pkg\";\ndeclare module \"pkg\" { interface X { added(): void } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "use.ts".to_owned(),
                    text: "declare const value: Pkg.X;\nvalue.added();\nfunction use<T extends Pkg.X>(item: T) { item.added(); }\ndeclare const mixed: Pkg.X | { added(): void };\nmixed.added();\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn unresolved_module_augmentation_does_not_open_sibling_package_subpath() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/pkg/a.d.ts".to_owned(),
                    text: "export as namespace PkgA;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "node_modules/pkg/b.d.ts".to_owned(),
                    text: "export as namespace PkgB;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.d.ts".to_owned(),
                    text: "import * as PkgA from \"pkg/a\";\ndeclare module \"pkg/a\" { interface X { added(): void } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "use.ts".to_owned(),
                    text: "declare const aValue: PkgA.X;\naValue.added();\ndeclare const bValue: PkgB.X;\nbValue.added();\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2339]
        );
    }

    #[test]
    fn unresolved_module_augmentation_stays_with_nearest_package_instance() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "app1/node_modules/pkg/index.d.ts".to_owned(),
                    text: "export as namespace PkgOne;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "app2/node_modules/pkg/index.d.ts".to_owned(),
                    text: "export as namespace PkgTwo;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "app1/augmentation.d.ts".to_owned(),
                    text: "import * as PkgOne from \"pkg\";\ndeclare module \"pkg\" { interface X { added(): void } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "app2/use.ts".to_owned(),
                    text: "declare const one: PkgOne.X;\none.added();\ndeclare const two: PkgTwo.X;\ntwo.added();\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2339]
        );
    }

    #[test]
    fn unresolved_node_core_augmentation_matches_only_its_at_types_node_subpath() {
        let diagnostics = check_program(
            &[
                InputFile {
                    name: "node_modules/@types/node/fs.d.ts".to_owned(),
                    text: "export as namespace NodeFs;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "node_modules/@types/node/http.d.ts".to_owned(),
                    text: "export as namespace NodeHttp;\nexport interface X {}\n".to_owned(),
                },
                InputFile {
                    name: "augmentation.d.ts".to_owned(),
                    text: "import * as NodeFs from \"node:fs\";\ndeclare module \"node:fs\" { interface X { added(): void } }\n"
                        .to_owned(),
                },
                InputFile {
                    name: "use.ts".to_owned(),
                    text: "declare const fsValue: NodeFs.X;\nfsValue.added();\ndeclare const httpValue: NodeHttp.X;\nhttpValue.added();\n"
                        .to_owned(),
                },
            ],
            &CompilerOptions::default(),
        )
        .diagnostics;
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            [2339]
        );
    }

    #[test]
    fn value_side_member_publication_survives_reentrant_base_resolution() {
        let diagnostics = codes_of(
            "class B {}\nclass A extends A.make() {\n  static make(): typeof B { return B; }\n}\nA.make();\n",
        );
        assert!(
            !diagnostics.contains(&2339),
            "staged exports must stay visible during base resolution: {diagnostics:?}"
        );
    }

    #[test]
    fn truthy_this_guard_keeps_type_query_assignment_error() {
        assert_eq!(
            codes_of_with_options(
                "class C { m() { if (this) { const x: typeof this = 1; } } }",
                &strict_options(),
            ),
            [2322]
        );
    }

    #[test]
    fn tuple_intersection_array_literal_keeps_element_error() {
        assert_eq!(
            codes_of_with_options(
                "const x: [string] & { p: number } = [1];",
                &strict_options(),
            ),
            [2322]
        );
    }

    #[test]
    fn tuple_intersection_keeps_unrelated_required_member_error() {
        assert_eq!(
            codes_of_with_options(
                "const x: [number] & { p: string } = [1];",
                &strict_options(),
            ),
            [2322]
        );
    }

    #[test]
    fn contextual_tuple_arity_gap_remains_contained() {
        assert_eq!(
            codes_of_with_options(
                "const x: [...number[]] & { length: 2 } = [0, 0];",
                &strict_options(),
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn satisfies_literal_reports_elaborated_member_error() {
        assert_eq!(
            codes_of_with_options(
                "const x = { a: 1 } satisfies { a: string };",
                &strict_options(),
            ),
            [2322]
        );
    }

    #[test]
    fn invalid_interface_computed_name_reports_resolution_error() {
        assert_eq!(codes_of("interface I { [NotThere.x](): void; }"), [2304]);
        assert_eq!(
            codes_of("declare const ns: {}; interface I { [ns.missing](): void; }"),
            [2339]
        );
    }

    #[test]
    fn computed_object_setter_is_checked_without_a_use_site() {
        assert_eq!(
            codes_of_with_options(
                "declare const k: unique symbol; const o = { set [k](v) {} };",
                &strict_options(),
            ),
            [7032, 7006]
        );
    }

    #[test]
    fn unused_expect_error_stays_silent_while_checker_is_incomplete() {
        assert_eq!(
            codes_of("let x: number;\n// @ts-expect-error\nx;\n"),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn unused_expect_error_reports_2578() {
        assert_eq!(codes_of("// @ts-expect-error\nconst x = 1;\n"), [2578]);
    }

    #[test]
    fn contained_expect_error_target_does_not_report_2578() {
        assert_eq!(
            codes_of(
                "// @ts-expect-error\n\
                 const bad = (() => 1) satisfies number;\n"
            ),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn single_line_directive_suppresses_through_comment_lines() {
        // Walk crosses blank and `//` lines, exactly like tsc.
        assert_eq!(
            codes_of("// @ts-ignore\n// note\n\nlet x;\nlet x;\n"),
            [2451]
        );
    }

    #[test]
    fn block_comment_shell_stops_the_directive_walk() {
        // tsc's markPrecedingCommentDirectiveLine stops at any line
        // that is non-empty and not a `//` comment — a block-comment
        // line between directive and diagnostic KEEPS the diagnostic
        // (the retired interim filter walked through these).
        assert_eq!(
            codes_of("// @ts-ignore\n/* shell */\nlet x;\nlet x;\n"),
            [2451, 2451]
        );
    }

    #[test]
    fn trailing_comment_directive_suppresses_the_next_line() {
        // Scanner-collected: the directive comment trails code on its
        // own line, so a line-start scan would miss it.
        assert_eq!(
            codes_of("let a = 1; // @ts-ignore\nlet x;\nlet x;\n"),
            [2451]
        );
    }

    #[test]
    fn multi_line_directive_keys_on_its_closing_line() {
        // Directive on the closing line: suppresses the next line.
        assert_eq!(
            codes_of("/*\n@ts-expect-error */\nlet x;\nlet x;\n"),
            [2451]
        );
        // Directive on an interior line is no directive at all.
        assert_eq!(
            codes_of("/*\n@ts-expect-error\n*/\nlet x;\nlet x;\n"),
            [2451, 2451]
        );
    }

    #[test]
    fn template_literal_fake_directive_does_not_suppress() {
        // The `// @ts-ignore` line sits INSIDE a template literal: the
        // scanner collects nothing, and the walk treats the line as a
        // `//` comment and keeps climbing past it.
        assert_eq!(
            codes_of("const s = `\n// @ts-ignore\n`;\nlet x;\nlet x;\n"),
            [2451, 2451]
        );
    }

    #[test]
    fn directive_on_the_diagnostic_line_itself_does_not_suppress() {
        // The walk starts one line ABOVE the diagnostic.
        assert_eq!(codes_of("let x;\nlet x; // @ts-ignore\n"), [2451, 2451]);
    }

    #[test]
    fn ts_nocheck_suppresses_checked_js_diagnostics() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-nocheck\nlet x;\nlet x;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    }

    #[test]
    fn ts_check_overrides_explicit_check_js_false() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-check\nlet x;\nlet x;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(false),
                ..CompilerOptions::default()
            },
        );
        let pins: Vec<(u32, u32, u32)> = result
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                    diagnostic.length.unwrap_or(u32::MAX),
                )
            })
            .collect();

        assert_eq!(pins, [(2451, 17, 1), (2451, 24, 1)]);
    }

    #[test]
    fn checked_js_uses_comment_directives() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-check\n// @ts-ignore\nlet x;\nlet x;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                ..CompilerOptions::default()
            },
        );
        let codes: Vec<u32> = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();

        assert_eq!(codes, [2451]);
    }

    #[test]
    fn check_js_option_uses_comment_directives() {
        let result = check_program(
            &[InputFile {
                name: "a.js".to_owned(),
                text: "// @ts-ignore\nlet x;\nlet x;\n".to_owned(),
            }],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        );
        let codes: Vec<u32> = result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();

        assert_eq!(codes, [2451]);
    }

    #[test]
    fn check_directive_matches_shebang_bom_and_unicode_line_breaks() {
        assert_eq!(
            check_directive("#!/usr/bin/env node\n// @ts-nocheck\n"),
            Some(CheckDirective::NoCheck)
        );
        assert_eq!(
            check_directive("\u{FEFF}// @ts-nocheck\n"),
            Some(CheckDirective::NoCheck)
        );
        assert_eq!(
            check_directive("\u{FEFF}#!/usr/bin/env node\n// @ts-nocheck\n"),
            None
        );
        assert_eq!(
            check_directive("// @ts-nocheck\u{2028}// @ts-check\u{2029}"),
            Some(CheckDirective::Check)
        );
        assert_eq!(
            check_directive("// @ts-check\u{2028}// @ts-nocheck\u{2029}"),
            Some(CheckDirective::NoCheck)
        );
    }

    #[test]
    fn unicode_line_break_last_ts_check_restores_semantic_diagnostics() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "// @ts-nocheck\u{2028}// @ts-check\u{2028}const value: string = 1;"
                    .to_owned(),
            }],
            &CompilerOptions::default(),
        );

        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2322),
            "{:?}",
            result.diagnostics
        );
    }

    #[test]
    fn bom_before_shebang_does_not_enable_following_ts_nocheck() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "\u{FEFF}#!/usr/bin/env node\n// @ts-nocheck\nconst value: string = 1;\n"
                    .to_owned(),
            }],
            &CompilerOptions::default(),
        );

        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code() == 2322),
            "{:?}",
            result.diagnostics
        );
    }

    #[test]
    fn ts_nocheck_after_shebang_suppresses_semantic_diagnostics() {
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "#!/usr/bin/env node\n// @ts-nocheck\nconst value: string = 1;\n".to_owned(),
            }],
            &CompilerOptions::default(),
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    }

    #[test]
    fn skip_lib_check_preserves_syntax_errors_and_skips_semantic_errors() {
        let result = check_program(
            &[
                InputFile {
                    name: "bad-syntax.d.ts".to_owned(),
                    text: "declare const x: ;\n".to_owned(),
                },
                InputFile {
                    name: "bad-semantic.d.ts".to_owned(),
                    text: "declare const y: Missing;\n".to_owned(),
                },
                InputFile {
                    name: "merge-a.d.ts".to_owned(),
                    text: "declare let merged: number;\n".to_owned(),
                },
                InputFile {
                    name: "merge-b.d.ts".to_owned(),
                    text: "declare let merged: string;\n".to_owned(),
                },
            ],
            &CompilerOptions {
                skip_lib_check: Some(true),
                ..CompilerOptions::default()
            },
        );

        let pins: Vec<(String, u32, u32)> = result
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.file_name.clone().unwrap_or_default(),
                    diagnostic.code(),
                    diagnostic.start.unwrap_or(u32::MAX),
                )
            })
            .collect();
        assert_eq!(pins, [("bad-syntax.d.ts".to_owned(), 1110, 17)]);
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
        assert_eq!(
            lib_backed_diags(
                "interface I<T extends Date> { x: T }
"
            ),
            []
        );
    }

    #[test]
    fn restricted_lib_set_reports_2583_with_the_lib_argument() {
        // Map is not in es5: the failure is GENUINE under this lib set
        // (the lib_globals gate stands down for lib-loaded programs)
        // and the suggested-lib arm supplies tsc's exact argument.
        let diags = lib_backed_diags(
            "interface I<T extends Map> { x: T }
",
        );
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
        assert_eq!(
            lib_backed_diags(
                "interface Wrap<out T> { xs: T[] }
"
            ),
            []
        );
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
            lib_backed_diags(
                "interface Wrap<out T> { xs: ReadonlyArray<T> }
"
            ),
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
        let diags = lib_backed_diags(
            "interface Wrap<out T> { f: (xs: T[]) => void }
",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!((diags[0].0, diags[0].1, diags[0].2), (2636, 15, 5));
        assert!(
            diags[0]
                .3
                .starts_with("Type 'Wrap<sub-T>' is not assignable to type 'Wrap<super-T>'"),
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
