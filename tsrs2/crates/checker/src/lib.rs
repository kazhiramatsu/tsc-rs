#![forbid(unsafe_code)]

mod js_grammar;

use tsrs2_diags::DiagnosticList;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompilerOptions {
    /// tsc getAllowJSCompilerOption: allowJs ?? !!checkJs.
    pub allow_js: bool,
    pub experimental_decorators: bool,
}

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
        diagnostics.append(&mut tsrs2_binder::bind_source_file(&source_file));
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
