//! Static tsgo aa8149273 casing diagnostics: identity, not rendered text, counts.
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use tsc_diagnostics::{
    flattened_message, sort_and_dedupe_diagnostic_indices_with_context, DocumentVersion,
    FormatDiagnosticsHost, TextSnapshot,
};
use tsc_program::WorkerBudget;

#[path = "support/loader_order.rs"]
#[allow(dead_code)]
mod corpus;

#[test]
fn casing_diagnostics_keep_distinct_equal_text_reasons() {
    let fixtures: Vec<corpus::Fixture> =
        serde_json::from_str(include_str!("fixtures/include-reason-multiplicity.json")).unwrap();
    for fixture in fixtures {
        let files: BTreeMap<_, _> = fixture
            .files
            .iter()
            .map(|(name, text)| (format!("/project/{name}"), text.clone()))
            .collect();
        for workers in [
            WorkerBudget::serial(),
            WorkerBudget::new(NonZeroUsize::new(4).unwrap()),
            WorkerBudget::automatic(),
        ] {
            let program = corpus::load(&fixture, workers, false);
            let diagnostics = program.diagnostics().program();
            let host = FormatDiagnosticsHost::new("/project", &files);
            let rows: Vec<_> = sort_and_dedupe_diagnostic_indices_with_context(diagnostics, &host)
                .into_iter().map(|index| {
                    let diagnostic = &diagnostics[index];
                    let file = diagnostic.file_name.as_ref().map(|file|
                        file.to_string_lossy().strip_prefix("/project/")
                            .unwrap_or(&file.to_string_lossy()).to_owned());
                    let position = file.as_ref().and_then(|file| {
                        let (_,text) = fixture.files.iter().find(|(name,_)| name.eq_ignore_ascii_case(file))?;
                        TextSnapshot::new(text.clone(),DocumentVersion::default()).positions()
                            .line_and_character_utf16(diagnostic.start?)
                    });
                    serde_json::json!({
                        "file":file, "line":position.map(|p|p.line+1),
                        "column":position.map(|p|p.character+1), "code":diagnostic.code(),
                        "message":flattened_message(&diagnostic.message,"\n",None).to_string_lossy(),
                    })
                }).collect();
            assert_eq!(rows, fixture.diagnostics, "{}: {workers:?}", fixture.name);
            let paths: Vec<_> = program
                .source_files()
                .iter()
                .map(|source| source.path().display().to_string_lossy().into_owned())
                .collect();
            assert_eq!(paths, fixture.list_files, "{}: listFiles", fixture.name);
            let mut explanation = String::new();
            for (path, reasons) in program.explain_files("/project".into()) {
                explanation.push_str(&path.to_string_lossy());
                explanation.push('\n');
                for reason in reasons {
                    explanation.push_str("   ");
                    explanation.push_str(&reason.to_string_lossy());
                    explanation.push('\n');
                }
            }
            assert_eq!(
                explanation, fixture.explain_files,
                "{}: explainFiles",
                fixture.name
            );
        }
    }
}
