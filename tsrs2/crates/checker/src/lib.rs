#![forbid(unsafe_code)]

pub mod annotate;
mod js_grammar;
pub mod links;
pub mod relpin;
pub mod state;

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

fn is_js_file_name(name: &str) -> bool {
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
    let mut diagnostics = Vec::new();
    let mut syntactic_diagnostics = Vec::new();

    // tsc host semantics: files are a name-keyed map, so a later file with
    // the same name shadows an earlier one entirely.
    let mut last_index_by_name = std::collections::BTreeMap::new();
    for (index, file) in files.iter().enumerate() {
        last_index_by_name.insert(file.name.as_str(), index);
    }

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
        let source_file = tsrs2_syntax::parse_source_file(
            file.name.clone(),
            file.text.clone(),
            tsrs2_syntax::ParseOptions {
                language_variant,
                javascript_file,
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
        let binder = tsrs2_binder::bind_source_file(&source_file, options);
        // tsc getBindAndCheckDiagnosticsForFileNoCache: plain JS files
        // (no checkJs) filter bind diagnostics to the plainJSErrors
        // allowlist — none of which the binder emits yet (stage 3.4c);
        // and comment directives (@ts-ignore/@ts-expect-error) suppress
        // preceded diagnostics. Unused @ts-expect-error reporting (2578)
        // waits for the checker (M4) — the partialCheck path.
        if !javascript_file {
            diagnostics.extend(filter_by_comment_directives(
                &source_file.text,
                &source_file.line_map,
                binder.bind_diagnostics.iter().cloned(),
            ));
        }
    }

    debug_assert!(tsrs2_binder::is_scaffolded());
    debug_assert!(tsrs2_types::is_scaffolded());

    CheckResult {
        diagnostics,
        syntactic_diagnostics,
    }
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
}
