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
}

pub fn check_program(files: &[InputFile], _options: &CompilerOptions) -> CheckResult {
    let mut diagnostics = Vec::new();

    for file in files {
        let mut source_file = tsrs2_syntax::parse_source_file(
            file.name.clone(),
            file.text.clone(),
            tsrs2_syntax::LanguageVariant::Standard,
        );
        diagnostics.append(&mut source_file.parse_diagnostics);
        diagnostics.append(&mut tsrs2_binder::bind_source_file(&source_file));
    }

    debug_assert!(tsrs2_binder::is_scaffolded());
    debug_assert!(tsrs2_types::is_scaffolded());

    CheckResult { diagnostics }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_engine_returns_no_diagnostics() {
        let result = check_program(&[], &CompilerOptions::default());
        assert!(result.diagnostics.is_empty());
    }
}
