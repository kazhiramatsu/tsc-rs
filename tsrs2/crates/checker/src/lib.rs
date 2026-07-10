#![forbid(unsafe_code)]

use tsrs2_diags::DiagnosticList;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompilerOptions {}

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

fn is_supported_source_file_name(name: &str) -> bool {
    [
        ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json",
    ]
    .iter()
    .any(|extension| name.ends_with(extension))
}

pub fn check_program(files: &[InputFile], _options: &CompilerOptions) -> CheckResult {
    let mut diagnostics = Vec::new();
    let mut syntactic_diagnostics = Vec::new();

    for file in files {
        // tsc createProgram only loads roots with supported extensions;
        // anything else (.txt, extensionless) never yields syntactic
        // diagnostics.
        if !is_supported_source_file_name(&file.name) {
            continue;
        }
        // tsc ensureScriptKind: .json programs parse as JSON values.
        if file.name.ends_with(".json") {
            let source_file = tsrs2_syntax::parse_json_text(file.name.clone(), file.text.clone());
            syntactic_diagnostics.extend(source_file.parse_diagnostics.iter().cloned());
            diagnostics.extend(source_file.parse_diagnostics.iter().cloned());
            continue;
        }
        // tsc getLanguageVariant, restricted to the extensions this engine
        // accepts (JS inputs are not handled yet).
        let language_variant = if file.name.ends_with(".tsx") || file.name.ends_with(".jsx") {
            tsrs2_syntax::LanguageVariant::Jsx
        } else {
            tsrs2_syntax::LanguageVariant::Standard
        };
        let source_file = tsrs2_syntax::parse_source_file(
            file.name.clone(),
            file.text.clone(),
            tsrs2_syntax::ParseOptions { language_variant },
            None,
        );
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
